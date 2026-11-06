//! The WebTransport side of a server. QUIC needs async, so it runs on a small runtime of its own and
//! hands each finished connection to the game thread, which polls it like a WebSocket: no locks, and
//! nothing a client does can hold up a tick.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use wtransport::endpoint::IncomingSession;
use wtransport::{Connection, Endpoint, Identity, RecvStream, SendStream, ServerConfig};

use wtransport::error::SendDatagramError;

use crate::quic::{StreamFrames, WtInfo, goes_on_stream, stream_frame};

/// How long a connection the game has let go of gets to deliver what it was last given.
const LAST_WORDS: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct WtConfig {
    /// A UDP port, apart from the WebSocket's.
    pub bind: SocketAddr,
    pub certificate: WtCertificate,
    /// How often to read the certificate again, or make a new one, while running.
    pub refresh_every: Duration,
    /// A connection that says nothing for this long is gone.
    pub idle_timeout: Duration,
}

impl Default for WtConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 4433)),
            certificate: WtCertificate::SelfSigned {
                names: ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec(),
                // Browsers take a certificate by its hash only if it lasts under two weeks.
                valid_days: 13,
            },
            // Well inside the life of either kind: Let's Encrypt renews a month before the end.
            refresh_every: Duration::from_secs(12 * 3600),
            idle_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug)]
pub enum WtCertificate {
    /// PEM files, such as the ones a web server keeps from Let's Encrypt for the same name.
    Files { chain: PathBuf, key: PathBuf },
    /// Made at start for these names. A browser only trusts it because the page names its hash, which
    /// it allows for certificates that last under two weeks, so a new one is made before then.
    SelfSigned { names: Vec<String>, valid_days: u32 },
}

impl WtCertificate {
    async fn identity(&self) -> Result<Identity, String> {
        match self {
            Self::Files { chain, key } => Identity::load_pemfiles(chain, key).await.map_err(|e| e.to_string()),
            Self::SelfSigned { names, valid_days } => Identity::self_signed_builder()
                .subject_alt_names(names)
                .from_now_utc()
                .validity_days(*valid_days)
                .build()
                .map_err(|e| e.to_string()),
        }
    }
}

/// One client's connection, as the game thread sees it.
pub(crate) struct WtConnection {
    inbox: Receiver<Vec<u8>>,
    outbox: UnboundedSender<Vec<u8>>,
    /// Bytes handed over but not yet taken by QUIC.
    queued: Arc<AtomicUsize>,
}

pub(crate) struct Gone;

impl WtConnection {
    pub(crate) fn recv(&self) -> Result<Option<Vec<u8>>, Gone> {
        match self.inbox.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Gone),
        }
    }

    pub(crate) fn send(&self, frame: Vec<u8>) -> Result<(), Gone> {
        self.queued.fetch_add(frame.len(), Ordering::Relaxed);
        self.outbox.send(frame).map_err(|_| Gone)
    }

    pub(crate) fn queued(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }
}

/// A bound UDP socket taking WebTransport sessions, on a thread of its own.
pub(crate) struct WtListener {
    pub(crate) incoming: Receiver<WtConnection>,
    pub(crate) info: Arc<std::sync::RwLock<WtInfo>>,
    pub(crate) local_addr: SocketAddr,
}

/// What the listener needs from the rest of the host.
pub(crate) struct WtLimits {
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) handshake_timeout: Duration,
    pub(crate) max_handshakes: usize,
}

pub(crate) fn listen(cfg: WtConfig, limits: WtLimits) -> std::io::Result<WtListener> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("webtransport")
        .enable_all()
        .build()?;
    let identity = runtime.block_on(cfg.certificate.identity()).map_err(std::io::Error::other)?;
    let info = Arc::new(std::sync::RwLock::new(info_for(cfg.bind.port(), &cfg.certificate, &identity)));
    let endpoint = {
        let _inside = runtime.enter();
        Endpoint::server(server_config(&cfg, identity)?)?
    };
    let local_addr = endpoint.local_addr()?;
    if let Ok(mut info) = info.write() {
        info.port = local_addr.port();
    }
    let (done, incoming) = mpsc::channel();
    let shared_info = info.clone();
    std::thread::Builder::new().name("webtransport".into()).spawn(move || {
        runtime.block_on(serve(endpoint, cfg, limits, done, shared_info));
    })?;
    Ok(WtListener { incoming, info, local_addr })
}

fn server_config(cfg: &WtConfig, identity: Identity) -> std::io::Result<ServerConfig> {
    Ok(ServerConfig::builder()
        .with_bind_address(cfg.bind)
        .with_identity(identity)
        .max_idle_timeout(Some(cfg.idle_timeout))
        .map_err(std::io::Error::other)?
        .build())
}

fn info_for(port: u16, certificate: &WtCertificate, identity: &Identity) -> WtInfo {
    let certificate_hash = match certificate {
        WtCertificate::SelfSigned { .. } => identity.certificate_chain().as_slice().first().map(|c| *c.hash().as_ref()),
        WtCertificate::Files { .. } => None,
    };
    WtInfo { port, certificate_hash }
}

async fn serve(
    endpoint: Endpoint<wtransport::endpoint::endpoint_side::Server>,
    cfg: WtConfig,
    limits: WtLimits,
    done: Sender<WtConnection>,
    info: Arc<std::sync::RwLock<WtInfo>>,
) {
    let limits = Arc::new(limits);
    let handshaking = Arc::new(AtomicUsize::new(0));
    let mut refresh = tokio::time::interval_at(tokio::time::Instant::now() + cfg.refresh_every, cfg.refresh_every);
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                // Past this many, a flood that never finishes would only pile up.
                if handshaking.load(Ordering::Relaxed) >= limits.max_handshakes {
                    continue;
                }
                handshaking.fetch_add(1, Ordering::Relaxed);
                let (limits, done, handshaking) = (limits.clone(), done.clone(), handshaking.clone());
                tokio::spawn(async move {
                    let opened = tokio::time::timeout(limits.handshake_timeout, open(incoming, &limits)).await;
                    handshaking.fetch_sub(1, Ordering::Relaxed);
                    if let Ok(Some((connection, stream))) = opened {
                        run_connection(connection, stream, &done).await;
                    }
                });
            }
            _ = refresh.tick() => {
                // Keeps serving with what it has if the new one can't be had: the old may still be good.
                if let Ok(identity) = cfg.certificate.identity().await {
                    let fresh = info_for(cfg.bind.port(), &cfg.certificate, &identity);
                    if let Ok(config) = server_config(&cfg, identity)
                        && endpoint.reload_config(config, false).is_ok()
                        && let Ok(mut info) = info.write()
                    {
                        info.certificate_hash = fresh.certificate_hash;
                    }
                }
            }
        }
    }
}

/// A session that is let in, with the stream its client opened first.
async fn open(incoming: IncomingSession, limits: &WtLimits) -> Option<(Connection, (SendStream, RecvStream))> {
    let request = incoming.await.ok()?;
    if !crate::ws_host::origin_allowed(&limits.allowed_origins, request.origin()) {
        request.forbidden().await;
        return None;
    }
    let connection = request.accept().await.ok()?;
    let stream = connection.accept_bi().await.ok()?;
    Some((connection, stream))
}

async fn run_connection(connection: Connection, (mut send, mut recv): (SendStream, RecvStream), done: &Sender<WtConnection>) {
    let (inbox_tx, inbox) = mpsc::channel();
    let (outbox, mut outgoing): (_, UnboundedReceiver<Vec<u8>>) = unbounded_channel();
    let queued = Arc::new(AtomicUsize::new(0));
    if done.send(WtConnection { inbox, outbox, queued: queued.clone() }).is_err() {
        return;
    }
    let reading_stream = async {
        let mut frames = StreamFrames::default();
        let mut buffer = vec![0; 16 * 1024];
        while let Ok(Some(read)) = recv.read(&mut buffer).await {
            frames.push(&buffer[..read]);
            loop {
                match frames.next_frame() {
                    Ok(Some(frame)) => {
                        if inbox_tx.send(frame).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => return,
                }
            }
        }
    };
    let reading_datagrams = async {
        while let Ok(datagram) = connection.receive_datagram().await {
            if inbox_tx.send(datagram.payload().to_vec()).is_err() {
                return;
            }
        }
    };
    let writing = async {
        // Ends when the game lets go of the connection.
        while let Some(frame) = outgoing.recv().await {
            queued.fetch_sub(frame.len(), Ordering::Relaxed);
            let on_stream = goes_on_stream(&frame, connection.max_datagram_size())
                || match connection.send_datagram(&frame) {
                    Ok(()) => false,
                    Err(SendDatagramError::NotConnected) => return,
                    // The most a datagram can be has shrunk since it was asked.
                    Err(_) => true,
                };
            if on_stream && send.write_all(&stream_frame(&frame)).await.is_err() {
                return;
            }
        }
        // Whatever was last said, such as why it was turned away, should reach it before the end.
        let _ = tokio::time::timeout(LAST_WORDS, send.finish()).await;
    };
    tokio::select! {
        _ = reading_stream => {}
        _ = reading_datagrams => {}
        _ = writing => {}
    }
    connection.close(0u32.into(), b"");
}
