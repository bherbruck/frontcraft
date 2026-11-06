//! WebTransport on a desktop. QUIC is async, so the connection lives on a thread of its own with a
//! small runtime, and the game polls it through channels without ever waiting.
//!
//! It asks the server beside the WebSocket where WebTransport is, over the same plain or TLS connection
//! the WebSocket would use: see `ws_native`.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use wtransport::error::SendDatagramError;
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Endpoint};

use crate::ws_native::dial;
use crate::quic::{StreamFrames, WtInfo, goes_on_stream, info_url, session_url, stream_frame};
use crate::transport::{Transport, TransportState};

const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const KEEP_ALIVE: Duration = Duration::from_secs(2);
const MAX_INFO_BYTES: usize = 4096;

const CONNECTING: u8 = 0;
const OPEN: u8 = 1;
const CLOSED: u8 = 2;

pub struct WtClient {
    state: Arc<AtomicU8>,
    inbox: Receiver<Vec<u8>>,
    outbox: UnboundedSender<Vec<u8>>,
}

impl WtClient {
    /// Starts connecting, given the address of the server's WebSocket, and returns straight away.
    pub fn connect(websocket: &str) -> Self {
        let state = Arc::new(AtomicU8::new(CONNECTING));
        let (inbox_tx, inbox) = mpsc::channel();
        let (outbox, outgoing) = unbounded_channel();
        let (websocket, shared) = (websocket.to_owned(), state.clone());
        let started = std::thread::Builder::new().name("webtransport".into()).spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            runtime.block_on(run(&websocket, &shared, inbox_tx, outgoing));
            shared.store(CLOSED, Ordering::Relaxed);
        });
        if started.is_err() {
            state.store(CLOSED, Ordering::Relaxed);
        }
        Self { state, inbox, outbox }
    }
}

/// Asks the server where WebTransport is.
fn ask(websocket: &str) -> Option<WtInfo> {
    let (status, body) = get(&info_url(websocket)?).ok()?;
    (status == 200).then(|| WtInfo::from_json(&body))?
}

/// A bare HTTP/1.0 GET over the same plain or TLS connection the WebSocket would use. The status and body.
fn get(url: &str) -> Result<(u16, String), String> {
    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => return Err("only http:// and https://".into()),
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let mut stream = dial(secure, rest)?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    // Read to the end, or as far as there is: a server going away without saying goodbye over TLS has
    // still said all of an answer this small.
    let mut response = Vec::new();
    let mut chunk = [0; 1024];
    while response.len() < MAX_INFO_BYTES {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
        }
    }
    let response = String::from_utf8_lossy(&response);
    let (head, body) = response.split_once("\r\n\r\n").ok_or("no answer")?;
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("no status")?;
    Ok((status, body.to_owned()))
}

async fn run(
    websocket: &str,
    state: &AtomicU8,
    inbox: mpsc::Sender<Vec<u8>>,
    mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let websocket_owned = websocket.to_owned();
    let Ok(Some(info)) = tokio::task::spawn_blocking(move || ask(&websocket_owned)).await else { return };
    let Some(url) = session_url(websocket, &info) else { return };
    let builder = ClientConfig::builder().with_bind_default();
    let builder = match info.certificate_hash {
        Some(hash) => builder.with_server_certificate_hashes([Sha256Digest::new(hash)]),
        None => builder.with_native_certs(),
    };
    let Ok(builder) = builder.max_idle_timeout(Some(IDLE_TIMEOUT)) else { return };
    let config = builder.keep_alive_interval(Some(KEEP_ALIVE)).build();
    let Ok(endpoint) = Endpoint::client(config) else { return };
    let Ok(connection) = endpoint.connect(&url).await else { return };
    let Ok(opening) = connection.open_bi().await else { return };
    let Ok((mut send, mut recv)) = opening.await else { return };
    state.store(OPEN, Ordering::Relaxed);

    let reading_stream = async {
        let mut frames = StreamFrames::default();
        let mut buffer = vec![0; 16 * 1024];
        while let Ok(Some(read)) = recv.read(&mut buffer).await {
            frames.push(&buffer[..read]);
            loop {
                match frames.next_frame() {
                    Ok(Some(frame)) => {
                        if inbox.send(frame).is_err() {
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
            if inbox.send(datagram.payload().to_vec()).is_err() {
                return;
            }
        }
    };
    let writing = async {
        while let Some(frame) = outgoing.recv().await {
            let on_stream = goes_on_stream(&frame, connection.max_datagram_size())
                || match connection.send_datagram(&frame) {
                    Ok(()) => false,
                    Err(SendDatagramError::NotConnected) => return,
                    Err(_) => true,
                };
            if on_stream && send.write_all(&stream_frame(&frame)).await.is_err() {
                return;
            }
        }
    };
    tokio::select! {
        _ = reading_stream => {}
        _ = reading_datagrams => {}
        _ = writing => {}
    }
    connection.close(0u32.into(), b"");
}

impl Transport for WtClient {
    fn state(&mut self) -> TransportState {
        match self.state.load(Ordering::Relaxed) {
            CONNECTING => TransportState::Connecting,
            OPEN => TransportState::Open,
            _ => TransportState::Closed,
        }
    }

    fn send(&mut self, frame: &[u8]) {
        if self.state.load(Ordering::Relaxed) == OPEN {
            let _ = self.outbox.send(frame.to_vec());
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        match self.inbox.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    fn is_reliable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Goes out to the internet, so only on request: `PRS_ASK=wss://host cargo test -p prs_net -- --ignored asks`.
    #[test]
    #[ignore = "needs the internet"]
    fn asks_a_real_server_over_tls() {
        let server = std::env::var("PRS_ASK").unwrap_or_else(|_| "wss://play.frontcraft.online".into());
        let url = info_url(&server).unwrap();
        let answer = get(&url);
        println!("{url}: {answer:?}");
        assert!(answer.is_ok(), "{answer:?}");
    }
}
