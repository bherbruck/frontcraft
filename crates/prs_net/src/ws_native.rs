//! A WebSocket on a desktop. Connects on its own thread, so asking never holds up a frame, and
//! afterwards never blocks. `wss://` goes over TLS, checked against the system's own certificates, for a
//! public server behind a proxy. `ws://` is plain, for a server on the same network.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::client::client_with_config;
use tungstenite::{Error, Message, WebSocket};

use crate::transport::{Transport, TransportState};
use crate::wire::MAX_FRAME_BYTES;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The connection under the WebSocket, with or without TLS.
pub(crate) enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(tcp) => tcp,
            Stream::Tls(tls) => tls.get_ref(),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.read(buf),
            Stream::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.write(buf),
            Stream::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(tcp) => tcp.flush(),
            Stream::Tls(tls) => tls.flush(),
        }
    }
}

enum Link {
    Connecting(Receiver<Result<WebSocket<Stream>, String>>),
    Open(Box<WebSocket<Stream>>),
    Closed,
}

pub struct WsClient {
    link: Link,
}

/// What to trust: the certificates the system trusts.
fn tls_config() -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    let found = rustls_native_certs::load_native_certs();
    let (added, _) = roots.add_parsable_certificates(found.certs);
    if added == 0 {
        return Err("no trusted certificates on this system".into());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn connect(url: &str) -> Result<WebSocket<Stream>, String> {
    let (secure, rest) = match (url.strip_prefix("wss://"), url.strip_prefix("ws://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => return Err("only ws:// and wss:// urls".into()),
    };
    let stream = dial(secure, rest)?;
    let config = WebSocketConfig::default().max_message_size(Some(MAX_FRAME_BYTES));
    let (ws, _) = client_with_config(url, stream, Some(config)).map_err(|e| e.to_string())?;
    let tcp = ws.get_ref().tcp();
    tcp.set_read_timeout(None).map_err(|e| e.to_string())?;
    tcp.set_nonblocking(true).map_err(|e| e.to_string())?;
    Ok(ws)
}

/// A blocking connection to what follows the scheme in a URL, over TLS if `secure`. Reads time out, so
/// that whatever is said over it next cannot hang for ever.
pub(crate) fn dial(secure: bool, rest: &str) -> Result<Stream, String> {
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority.rsplit_once(':').map_or(authority, |(host, _)| host).trim_matches(['[', ']']);
    let authority = if authority.contains(':') { authority.to_owned() } else { format!("{authority}:{}", if secure { 443 } else { 80 }) };
    let addr = authority
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address for host")?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    // Every frame here is small and urgent. Nagle would hold them back for tens of milliseconds.
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    // The handshakes block, on this thread, but not for ever.
    stream.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(|e| e.to_string())?;
    Ok(if secure {
        let name = ServerName::try_from(host.to_owned()).map_err(|e| e.to_string())?;
        let tls = ClientConnection::new(tls_config()?, name).map_err(|e| e.to_string())?;
        Stream::Tls(Box::new(StreamOwned::new(tls, stream)))
    } else {
        Stream::Plain(stream)
    })
}

impl WsClient {
    /// Starts connecting and returns straight away.
    pub fn connect(url: &str) -> Self {
        let (tx, rx) = channel();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(connect(&url));
        });
        Self { link: Link::Connecting(rx) }
    }

    fn poll_connect(&mut self) {
        if let Link::Connecting(rx) = &self.link {
            match rx.try_recv() {
                Ok(Ok(ws)) => self.link = Link::Open(Box::new(ws)),
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => self.link = Link::Closed,
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

fn would_block(e: &Error) -> bool {
    matches!(e, Error::Io(io) if io.kind() == ErrorKind::WouldBlock)
}

impl Transport for WsClient {
    fn state(&mut self) -> TransportState {
        self.poll_connect();
        match self.link {
            Link::Connecting(_) => TransportState::Connecting,
            Link::Open(_) => TransportState::Open,
            Link::Closed => TransportState::Closed,
        }
    }

    fn send(&mut self, frame: &[u8]) {
        let Link::Open(ws) = &mut self.link else { return };
        if let Err(e) = ws.send(Message::binary(frame.to_vec())) {
            // Queued, and goes out on a later flush.
            if !would_block(&e) {
                self.link = Link::Closed;
            }
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        let Link::Open(ws) = &mut self.link else { return None };
        loop {
            match ws.read() {
                Ok(Message::Binary(bytes)) => return Some(bytes.to_vec()),
                Ok(_) => {}
                Err(e) if would_block(&e) => {
                    if ws.flush().is_err_and(|e| !would_block(&e)) {
                        self.link = Link::Closed;
                    }
                    return None;
                }
                Err(_) => {
                    self.link = Link::Closed;
                    return None;
                }
            }
        }
    }

    fn is_reliable(&self) -> bool {
        true
    }
}
