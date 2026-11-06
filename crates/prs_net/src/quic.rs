//! What both ends of a WebTransport connection agree on, with no sockets in it.
//!
//! A frame goes as one datagram if it may be lost and fits. Anything else goes on the one stream both
//! ends keep open, with its length in front, so the stream can be cut back into frames.

use crate::wire::{MAX_FRAME_BYTES, must_arrive};

/// Where a client learns how to reach the server by WebTransport, beside the WebSocket.
pub const INFO_PATH: &str = "/webtransport";
/// The path a client asks for when it opens a session.
pub const SESSION_PATH: &str = "/play";

const LENGTH_BYTES: usize = 4;

/// Whether a frame has to go on the stream rather than as a datagram.
pub fn goes_on_stream(frame: &[u8], max_datagram: Option<usize>) -> bool {
    must_arrive(frame) || max_datagram.is_none_or(|most| frame.len() > most)
}

/// A frame with its length in front, for the stream.
pub fn stream_frame(frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(LENGTH_BYTES + frame.len());
    out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    out.extend_from_slice(frame);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTooBig;

/// Cuts what arrives on a stream back into frames, however it was split on the way.
#[derive(Default)]
pub struct StreamFrames {
    pending: Vec<u8>,
}

impl StreamFrames {
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// The next whole frame, if there is one. A length over the most a frame may be ends the stream.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameTooBig> {
        let Some(length) = self.pending.first_chunk::<LENGTH_BYTES>() else { return Ok(None) };
        let length = u32::from_le_bytes(*length) as usize;
        if length > MAX_FRAME_BYTES {
            return Err(FrameTooBig);
        }
        if self.pending.len() < LENGTH_BYTES + length {
            return Ok(None);
        }
        let frame = self.pending[LENGTH_BYTES..LENGTH_BYTES + length].to_vec();
        self.pending.drain(..LENGTH_BYTES + length);
        Ok(Some(frame))
    }
}

/// How to reach a server by WebTransport, as it tells its clients.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WtInfo {
    pub port: u16,
    /// The SHA-256 of a certificate the server made itself, which is only trusted by being named.
    /// `None` for one any browser already trusts.
    pub certificate_hash: Option<[u8; 32]>,
}

impl WtInfo {
    pub fn to_json(&self) -> String {
        match &self.certificate_hash {
            Some(hash) => format!(r#"{{"port":{},"hash":"{}"}}"#, self.port, hex(hash)),
            None => format!(r#"{{"port":{}}}"#, self.port),
        }
    }

    /// Reads back what [`to_json`](Self::to_json) wrote. Nothing else needs reading.
    pub fn from_json(text: &str) -> Option<Self> {
        let field = |name: &str| {
            let key = format!(r#""{name}":"#);
            let rest = &text[text.find(&key)? + key.len()..];
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            Some(rest[..end].trim().trim_matches('"').to_owned())
        };
        let port = field("port")?.parse().ok()?;
        let certificate_hash = match field("hash") {
            Some(hash) => Some(unhex(&hash)?),
            None => None,
        };
        Some(Self { port, certificate_hash })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut out = [0; 32];
    if text.len() != out.len() * 2 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Where to ask for [`WtInfo`], given the address of the WebSocket beside it. Under the same path: a proxy
/// that sends `/range` to one server and everything else to another sends this to the right one.
pub fn info_url(websocket: &str) -> Option<String> {
    let (scheme, rest) = websocket.split_once("://")?;
    let scheme = match scheme {
        "ws" => "http",
        "wss" => "https",
        _ => return None,
    };
    let address = rest.split(['?', '#']).next()?.trim_end_matches('/');
    Some(format!("{scheme}://{address}{INFO_PATH}"))
}

/// The session to open, given the address of the WebSocket beside it and what the server said.
pub fn session_url(websocket: &str, info: &WtInfo) -> Option<String> {
    let rest = websocket.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    // An IPv6 address keeps its brackets, and loses only the port after them.
    let host = match authority.rfind(']') {
        Some(end) => &authority[..=end],
        None => authority.split(':').next()?,
    };
    Some(format!("https://{host}:{}{SESSION_PATH}", info.port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_come_back_whole_however_the_stream_splits_them() {
        let frames = [vec![1u8, 2, 3], vec![], vec![9; 3000]];
        let bytes: Vec<u8> = frames.iter().flat_map(|f| stream_frame(f)).collect();
        for chunk in [1, 3, 7, 4096] {
            let mut reader = StreamFrames::default();
            let mut out = Vec::new();
            for piece in bytes.chunks(chunk) {
                reader.push(piece);
                while let Some(frame) = reader.next_frame().unwrap() {
                    out.push(frame);
                }
            }
            assert_eq!(out, frames);
        }
    }

    #[test]
    fn a_length_too_long_to_be_a_frame_ends_the_stream() {
        let mut reader = StreamFrames::default();
        reader.push(&u32::MAX.to_le_bytes());
        assert_eq!(reader.next_frame(), Err(FrameTooBig));
    }

    #[test]
    fn what_the_server_says_reads_back() {
        for info in [WtInfo { port: 4433, certificate_hash: Some([0xa7; 32]) }, WtInfo { port: 1, certificate_hash: None }] {
            assert_eq!(WtInfo::from_json(&info.to_json()), Some(info));
        }
        assert_eq!(WtInfo::from_json("rubbish"), None);
    }

    #[test]
    fn a_websocket_address_says_where_to_look() {
        assert_eq!(info_url("wss://play.example:443/x?y").as_deref(), Some("https://play.example:443/x/webtransport"));
        assert_eq!(info_url("wss://play.example/range/").as_deref(), Some("https://play.example/range/webtransport"));
        assert_eq!(info_url("ws://127.0.0.1:7777").as_deref(), Some("http://127.0.0.1:7777/webtransport"));
        let info = WtInfo { port: 4433, certificate_hash: None };
        assert_eq!(session_url("wss://play.example", &info).as_deref(), Some("https://play.example:4433/play"));
        assert_eq!(session_url("ws://[::1]:7777/", &info).as_deref(), Some("https://[::1]:4433/play"));
        assert_eq!(session_url("wss://play.example/range", &info).as_deref(), Some("https://play.example:4433/play"));
    }

    #[test]
    fn only_what_may_be_lost_and_fits_goes_as_a_datagram() {
        let snapshot = [18u8, 0, 0];
        assert!(!goes_on_stream(&snapshot, Some(1200)));
        assert!(goes_on_stream(&snapshot, Some(2)));
        assert!(goes_on_stream(&snapshot, None));
        assert!(goes_on_stream(&[1u8, 0], Some(1200)));
    }
}
