//! The part of a server that owns real sockets. One thread runs the game and polls every connection
//! without blocking, so there are no locks and a slow client can never hold up a tick.

use prs_protocol::TICK_HZ;
use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::{Error, Message, WebSocket, accept_hdr_with_config};


use crate::game::ServerGame;
use crate::quic::{INFO_PATH, WtInfo};
use crate::wt_host::{WtConfig, WtConnection, WtLimits, WtListener};
use crate::server::{ClientId, GameServer};
use crate::wire::MAX_FRAME_BYTES;

/// More than any browser or proxy sends to ask for a WebSocket.
const MAX_REQUEST_PEEK: usize = 8192;
const PEEK_PATIENCE: Duration = Duration::from_secs(5);
const PEEK_WAIT: Duration = Duration::from_millis(5);

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub bind: SocketAddr,
    /// How long a client gets to finish the WebSocket handshake.
    pub handshake_timeout: Duration,
    /// How long a client's socket may stay full before it is dropped.
    pub backlog_timeout: Duration,
    /// Bytes queued for a slow client before it counts as gone.
    pub max_write_buffer: usize,
    /// Print a status line this often. `None` for quiet.
    pub status_every: Option<Duration>,
    /// Stop after this long. `None` to run until killed.
    pub run_for: Option<Duration>,
    /// Which pages may connect from a browser, by their `Origin`. Empty means any. Anything that is not
    /// a browser sends none and is let in, because the header only ever protected browsers' users.
    pub allowed_origins: Vec<String>,
    /// Connections allowed to be part way through saying hello. More than this are turned away, so that
    /// a flood of them that never finish cannot run the machine out of threads.
    pub max_handshakes: usize,
    /// Who answers plain HTTP that isn't `/health`. Nobody, if `None`.
    pub plain_http: Option<PlainHttp>,
    /// WebTransport on a UDP port of its own as well, for clients that can. Those that can't, or find it
    /// blocked, still have the WebSocket.
    pub webtransport: Option<WtConfig>,
}

/// Takes a connection and the request read from it, on a thread of its own, away from the game.
#[derive(Clone)]
pub struct PlainHttp(pub Arc<dyn Fn(TcpStream, &str) + Send + Sync>);

impl std::fmt::Debug for PlainHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("PlainHttp")
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 7777)),
            handshake_timeout: Duration::from_secs(5),
            backlog_timeout: Duration::from_secs(5),
            max_write_buffer: 1 << 20,
            status_every: Some(Duration::from_secs(10)),
            run_for: None,
            allowed_origins: Vec::new(),
            max_handshakes: 64,
            plain_http: None,
            webtransport: None,
        }
    }
}

struct Connection {
    link: Link,
    backlogged_since: Option<Instant>,
}

enum Link {
    Ws(WebSocket<TcpStream>),
    Wt(WtConnection),
}

/// How to reach WebTransport, for anyone who asks over HTTP. Changes when a new certificate is made.
type SharedWtInfo = Option<Arc<RwLock<WtInfo>>>;

/// What a run of the server did, for its caller to report.
#[derive(Clone, Debug, Default)]
pub struct HostStats {
    pub ticks: u64,
    pub ticks_behind_max: u64,
    /// Time spent running ticks and I/O, as a fraction of the time available.
    pub busy_fraction: f64,
    pub times: TickTimes,
}

const TIME_BUCKET_MS: f64 = 0.1;
const TIME_BUCKETS: usize = 1000;

/// How long ticks took and how late they started, in milliseconds. A tick has 16.7 ms to itself.
#[derive(Clone, Debug, Default)]
pub struct TickTimes {
    count: u64,
    total_ms: f64,
    pub worst_ms: f64,
    /// How far behind its time any tick started: the longest anyone was left waiting.
    pub worst_late_ms: f64,
    buckets: Vec<u32>,
}

impl TickTimes {
    pub fn record(&mut self, took_ms: f64, late_ms: f64) {
        self.buckets.resize(TIME_BUCKETS, 0);
        self.count += 1;
        self.total_ms += took_ms;
        self.worst_ms = self.worst_ms.max(took_ms);
        self.worst_late_ms = self.worst_late_ms.max(late_ms);
        let bucket = (took_ms / TIME_BUCKET_MS) as usize;
        self.buckets[bucket.min(TIME_BUCKETS - 1)] += 1;
    }

    pub fn average_ms(&self) -> f64 {
        self.total_ms / self.count.max(1) as f64
    }

    /// The time that this share of ticks came in under, from 0 to 1.
    pub fn percentile_ms(&self, share: f64) -> f64 {
        let wanted = (self.count as f64 * share).ceil() as u64;
        let mut seen = 0;
        for (i, count) in self.buckets.iter().enumerate() {
            seen += *count as u64;
            if seen >= wanted {
                return (i + 1) as f64 * TIME_BUCKET_MS;
            }
        }
        self.worst_ms
    }
}

impl std::fmt::Display for TickTimes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "tick avg {:.2} ms, p99 {:.1} ms, worst {:.1} ms, started up to {:.1} ms late",
            self.average_ms(),
            self.percentile_ms(0.99),
            self.worst_ms,
            self.worst_late_ms,
        )
    }
}

fn would_block(e: &Error) -> bool {
    matches!(e, Error::Io(io) if io.kind() == ErrorKind::WouldBlock)
}

/// Looks at what a connection has sent without taking it, until the end of the request's headers.
fn peek_request(stream: &TcpStream) -> io::Result<String> {
    let mut buffer = [0; MAX_REQUEST_PEEK];
    let deadline = Instant::now();
    loop {
        let seen = stream.peek(&mut buffer)?;
        if seen == 0 {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        if buffer[..seen].windows(4).any(|end| end == b"\r\n\r\n") || seen == buffer.len() {
            return Ok(String::from_utf8_lossy(&buffer[..seen]).into_owned());
        }
        if deadline.elapsed() > PEEK_PATIENCE {
            return Err(ErrorKind::TimedOut.into());
        }
        std::thread::sleep(PEEK_WAIT);
    }
}

/// Whether a page with this origin may connect, given what is allowed. No origin means not a browser.
pub(crate) fn origin_allowed(allowed: &[String], origin: Option<&str>) -> bool {
    allowed.is_empty() || origin.is_none_or(|origin| allowed.iter().any(|ok| ok.eq_ignore_ascii_case(origin)))
}

/// Anything that is not asking for a WebSocket. A host checks `/health` to see that the server is up,
/// and whatever else there is to ask for is up to `cfg.plain_http`.
fn answer_plain_http(
    mut stream: TcpStream,
    request: &str,
    cfg: &HostConfig,
    wt: &SharedWtInfo,
    handshaking: &AtomicUsize,
) -> io::Result<()> {
    // Taken off the connection now that it has been looked at: closing with it unread can lose the answer.
    let mut unread = vec![0; request.len()];
    stream.read_exact(&mut unread)?;
    let lower = request.to_ascii_lowercase();
    let healthy = lower.starts_with("get /health ") || lower.starts_with("head /health ");
    if lower.starts_with(&format!("get {INFO_PATH} ")) {
        let info = wt.as_ref().and_then(|info| info.read().ok().map(|info| info.to_json()));
        // Any page may ask: it says nothing that trying to connect wouldn't.
        let response = match info {
            Some(body) => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n\
                 Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            ),
            None => "HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_owned(),
        };
        return stream.write_all(response.as_bytes());
    }
    if !healthy && let Some(PlainHttp(answer)) = &cfg.plain_http {
        // However long this takes, it isn't holding up anyone who wants to join.
        handshaking.fetch_sub(1, Ordering::Relaxed);
        answer(stream, request);
        handshaking.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    let (status, body) = if healthy { ("200 OK", "ok\n") } else { ("404 Not Found", "This is a game server.\n") };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes())
}

fn handshake(
    stream: TcpStream,
    cfg: &HostConfig,
    wt: &SharedWtInfo,
    done: &Sender<WebSocket<TcpStream>>,
    handshaking: &AtomicUsize,
) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(cfg.handshake_timeout))?;
    stream.set_write_timeout(Some(cfg.handshake_timeout))?;
    let request = peek_request(&stream)?;
    if !request.to_ascii_lowercase().contains("upgrade: websocket") {
        return answer_plain_http(stream, &request, cfg, wt, handshaking);
    }
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
        .max_write_buffer_size(cfg.max_write_buffer);
    let allowed = cfg.allowed_origins.clone();
    let check_origin = move |request: &Request, response: Response| {
        let origin = request.headers().get("origin").and_then(|value| value.to_str().ok());
        if origin_allowed(&allowed, origin) {
            return Ok(response);
        }
        let mut refusal = ErrorResponse::new(Some("This page may not connect to this server.".to_owned()));
        *refusal.status_mut() = StatusCode::FORBIDDEN;
        Err(refusal)
    };
    let ws = accept_hdr_with_config(stream, check_origin, Some(ws_config)).map_err(io::Error::other)?;
    ws.get_ref().set_nonblocking(true)?;
    let _ = done.send(ws);
    Ok(())
}

fn accept_new(
    listener: &TcpListener,
    cfg: &HostConfig,
    wt: &SharedWtInfo,
    done: &Sender<WebSocket<TcpStream>>,
    handshaking: &Arc<AtomicUsize>,
) {
    while let Ok((stream, _)) = listener.accept() {
        if handshaking.load(Ordering::Relaxed) >= cfg.max_handshakes {
            continue;
        }
        handshaking.fetch_add(1, Ordering::Relaxed);
        // Handshakes can dawdle, and this thread must not. Each gets a short thread of its own.
        let (cfg, wt, done, handshaking) = (cfg.clone(), wt.clone(), done.clone(), handshaking.clone());
        std::thread::spawn(move || {
            let _ = handshake(stream, &cfg, &wt, &done, &handshaking);
            handshaking.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

/// A bound socket, ready to run a game on.
pub struct WsHost {
    listener: TcpListener,
    cfg: HostConfig,
    wt: Option<WtListener>,
}

impl WsHost {
    pub fn bind(cfg: HostConfig) -> io::Result<Self> {
        let listener = TcpListener::bind(cfg.bind)?;
        listener.set_nonblocking(true)?;
        let wt = match &cfg.webtransport {
            Some(wt) => {
                let limits = WtLimits {
                    allowed_origins: cfg.allowed_origins.clone(),
                    handshake_timeout: cfg.handshake_timeout,
                    max_handshakes: cfg.max_handshakes,
                };
                Some(crate::wt_host::listen(wt.clone(), limits)?)
            }
            None => None,
        };
        Ok(Self { listener, cfg, wt })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The UDP address WebTransport is on, if it is.
    pub fn webtransport_addr(&self) -> Option<SocketAddr> {
        self.wt.as_ref().map(|wt| wt.local_addr)
    }

    /// Runs the game until `run_for` is up, or forever. Gives the game back at the end.
    pub fn run<G: ServerGame>(self, game: GameServer<G>) -> (GameServer<G>, HostStats) {
        run(self.listener, self.cfg, self.wt, game)
    }
}

fn run<G: ServerGame>(
    listener: TcpListener,
    cfg: HostConfig,
    wt: Option<WtListener>,
    mut game: GameServer<G>,
) -> (GameServer<G>, HostStats) {
    let wt_info: SharedWtInfo = wt.as_ref().map(|wt| wt.info.clone());
    let (ready_tx, ready_rx): (_, Receiver<WebSocket<TcpStream>>) = channel();
    let mut connections: HashMap<ClientId, Connection> = HashMap::new();

    let tick_time = Duration::from_secs_f64(1.0 / TICK_HZ);
    let start = Instant::now();
    let mut next_tick = start + tick_time;
    let mut next_status = cfg.status_every.map(|every| start + every);
    let mut stats = HostStats::default();
    let mut busy = Duration::ZERO;
    let handshaking = Arc::new(AtomicUsize::new(0));
    let mut recent = TickTimes::default();

    loop {
        let now = Instant::now();
        if cfg.run_for.is_some_and(|limit| now - start >= limit) {
            break;
        }
        if now < next_tick {
            std::thread::sleep((next_tick - now).min(Duration::from_millis(1)));
            continue;
        }
        let began = now;

        accept_new(&listener, &cfg, &wt_info, &ready_tx, &handshaking);
        while let Ok(ws) = ready_rx.try_recv() {
            let id = game.connect(true);
            connections.insert(id, Connection { link: Link::Ws(ws), backlogged_since: None });
        }
        while let Some(Ok(connection)) = wt.as_ref().map(|wt| wt.incoming.try_recv()) {
            // Snapshots can be lost on the way, so they are built on what the client says it has.
            let id = game.connect(false);
            connections.insert(id, Connection { link: Link::Wt(connection), backlogged_since: None });
        }

        read_all(&mut game, &mut connections);

        // Fell behind by more than a moment? Don't try to catch up by running flat out.
        let behind = (now - next_tick).as_secs_f64() / tick_time.as_secs_f64();
        stats.ticks_behind_max = stats.ticks_behind_max.max(behind as u64);
        game.tick();
        stats.ticks += 1;
        next_tick += tick_time;
        if next_tick < now {
            next_tick = now + tick_time;
        }

        write_all(&mut game, &mut connections, &cfg);
        let took = began.elapsed();
        busy += took;
        let (took_ms, late_ms) = (took.as_secs_f64() * 1000.0, behind * tick_time.as_secs_f64() * 1000.0);
        stats.times.record(took_ms, late_ms);
        recent.record(took_ms, late_ms);

        if next_status.is_some_and(|at| now >= at) {
            let elapsed = (now - start).as_secs_f64();
            println!(
                "t={:.0}s tick={} players={} busy={:.1}% {}",
                elapsed,
                game.game.tick(),
                game.player_count(),
                100.0 * busy.as_secs_f64() / elapsed,
                recent,
            );
            recent = TickTimes::default();
            next_status = cfg.status_every.map(|every| now + every);
        }
    }
    stats.busy_fraction = busy.as_secs_f64() / start.elapsed().as_secs_f64();
    (game, stats)
}

fn read_all<G: ServerGame>(game: &mut GameServer<G>, connections: &mut HashMap<ClientId, Connection>) {
    let mut gone = Vec::new();
    for (&id, connection) in connections.iter_mut() {
        loop {
            match &mut connection.link {
                Link::Ws(ws) => match ws.read() {
                    Ok(Message::Binary(bytes)) => game.receive(id, &bytes),
                    // Pings are answered by the library. Text means nothing here.
                    Ok(_) => {}
                    Err(e) if would_block(&e) => break,
                    Err(_) => {
                        gone.push(id);
                        break;
                    }
                },
                Link::Wt(wt) => match wt.recv() {
                    Ok(Some(frame)) => game.receive(id, &frame),
                    Ok(None) => break,
                    Err(_) => {
                        gone.push(id);
                        break;
                    }
                },
            }
        }
    }
    for id in gone {
        connections.remove(&id);
        game.disconnect(id);
    }
}

fn write_all<G: ServerGame>(game: &mut GameServer<G>, connections: &mut HashMap<ClientId, Connection>, cfg: &HostConfig) {
    let mut gone = Vec::new();
    for (id, frame) in game.drain_outgoing() {
        let Some(connection) = connections.get_mut(&id) else { continue };
        let ws = match &mut connection.link {
            Link::Ws(ws) => ws,
            Link::Wt(wt) => {
                if wt.send(frame).is_err() {
                    gone.push(id);
                }
                continue;
            }
        };
        match ws.write(Message::binary(frame)) {
            Ok(()) => {}
            Err(e) if would_block(&e) => {}
            // A full buffer is a client that has stopped reading. The flush below decides its fate.
            Err(Error::WriteBufferFull(_)) => {}
            Err(_) => gone.push(id),
        }
    }
    let now = Instant::now();
    for (&id, connection) in connections.iter_mut() {
        let flushed = match &mut connection.link {
            Link::Ws(ws) => ws.flush(),
            // Datagrams are never held up. Only what waits on the stream can pile up.
            Link::Wt(wt) if wt.queued() > cfg.max_write_buffer => Err(Error::Io(ErrorKind::WouldBlock.into())),
            Link::Wt(_) => Ok(()),
        };
        match flushed {
            Ok(()) => {
                if connection.backlogged_since.take().is_some() {
                    game.set_backlogged(id, false);
                }
            }
            Err(e) if would_block(&e) => {
                let since = *connection.backlogged_since.get_or_insert_with(|| {
                    game.set_backlogged(id, true);
                    now
                });
                if now - since > cfg.backlog_timeout {
                    gone.push(id);
                }
            }
            Err(_) => gone.push(id),
        }
    }
    for id in game.drain_kicked() {
        if let Some(Connection { link: Link::Ws(ws), .. }) = connections.get_mut(&id) {
            // Best effort, so that it can hear why. WebTransport sees to its own on the way out.
            let _ = ws.flush();
        }
        gone.push(id);
    }
    for id in gone {
        connections.remove(&id);
        game.disconnect(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_pages_may_connect_and_anything_not_a_browser_may() {
        let allowed = vec!["https://frontcraft.online".to_owned()];
        assert!(origin_allowed(&allowed, Some("https://frontcraft.online")));
        assert!(origin_allowed(&allowed, Some("HTTPS://FRONTCRAFT.ONLINE")));
        assert!(!origin_allowed(&allowed, Some("https://evil.example")));
        assert!(origin_allowed(&allowed, None));
        assert!(origin_allowed(&[], Some("https://anything.example")));
    }

    #[test]
    fn tick_times_add_up() {
        let mut times = TickTimes::default();
        for _ in 0..99 {
            times.record(1.0, 0.0);
        }
        times.record(30.0, 12.0);
        assert!((times.average_ms() - 1.29).abs() < 1e-9);
        assert!(times.percentile_ms(0.99) < 1.2);
        assert_eq!(times.worst_ms, 30.0);
        assert_eq!(times.worst_late_ms, 12.0);
    }
}
