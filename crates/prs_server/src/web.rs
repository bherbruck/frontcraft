//! The page, served from a folder, by the server that plays it, so there is one address and one port.
//!
//! Hand written because all it needs is a few headers and copying a file, and every HTTP crate worth
//! having would bring an async runtime into a server with none. Each request has a short thread of its own,
//! far from the game, so a slow download or a bad request costs a tick nothing.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct WebConfig {
    /// Downloads to run at once. More than this are asked to come back.
    pub max_downloads: usize,
    /// How long a client may take to accept the next piece of a file before it is given up on.
    pub stall_timeout: Duration,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { max_downloads: 64, stall_timeout: Duration::from_secs(30) }
    }
}

const COPY_BYTES: usize = 64 * 1024;
const ENCODINGS: [(&str, &str); 2] = [("br", "br"), ("gzip", "gz")];
/// `index.html` asks for the wasm and script with their build's id in this query, so those never go stale.
const VERSIONED_QUERY: &str = "v=";

pub struct WebRoot {
    root: PathBuf,
    cfg: WebConfig,
    downloads: AtomicUsize,
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("css") => "text/css; charset=utf-8",
        Some("toml") | Some("txt") => "text/plain; charset=utf-8",
        Some("wav") => "audio/wav",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Undoes `%xx`. `None` for anything that isn't valid, which nobody honest sends.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The path under the root that an address means, if it means one. Nothing that climbs up out of the folder gets
/// past this, however it is spelled.
fn relative_path(address: &str) -> Option<PathBuf> {
    let decoded = percent_decode(address)?;
    if decoded.contains('\\') || decoded.contains('\0') {
        return None;
    }
    let mut path = PathBuf::new();
    for part in Path::new(decoded.trim_start_matches('/')).components() {
        match part {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(path)
}

struct Request<'a> {
    head_only: bool,
    address: &'a str,
    versioned: bool,
    accepts: String,
}

fn parse(request: &str) -> Option<Request<'_>> {
    let mut lines = request.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let head_only = match first.next()? {
        "GET" => false,
        "HEAD" => true,
        _ => return None,
    };
    let target = first.next()?;
    let (address, query) = target.split_once('?').unwrap_or((target, ""));
    let accepts = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("accept-encoding"))
        .map_or(String::new(), |(_, value)| value.to_ascii_lowercase());
    Some(Request { head_only, address, versioned: query.split('&').any(|q| q.starts_with(VERSIONED_QUERY)), accepts })
}

pub fn answer_status(stream: &mut TcpStream, status: &str) -> io::Result<()> {
    let body = format!("{status}\n");
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

impl WebRoot {
    pub fn new(folder: &Path, cfg: WebConfig) -> io::Result<Self> {
        Ok(Self { root: folder.canonicalize()?, cfg, downloads: AtomicUsize::new(0) })
    }

    /// The file an address means, with no way out of the folder even by a symbolic link.
    fn find(&self, address: &str) -> Option<PathBuf> {
        let mut path = self.root.join(relative_path(address)?);
        if path.is_dir() {
            path.push("index.html");
        }
        let real = path.canonicalize().ok()?;
        (real.starts_with(&self.root) && real.is_file()).then_some(real)
    }

    /// Answers one request and closes the connection.
    pub fn respond(&self, mut stream: TcpStream, request: &str) -> io::Result<()> {
        stream.set_write_timeout(Some(self.cfg.stall_timeout))?;
        let Some(asked) = parse(request) else {
            return answer_status(&mut stream, "405 Method Not Allowed");
        };
        let Some(path) = self.find(asked.address) else {
            return answer_status(&mut stream, "404 Not Found");
        };
        if self.downloads.fetch_add(1, Ordering::Relaxed) >= self.cfg.max_downloads {
            self.downloads.fetch_sub(1, Ordering::Relaxed);
            return answer_status(&mut stream, "503 Service Unavailable");
        }
        let result = self.send(&mut stream, &path, &asked);
        self.downloads.fetch_sub(1, Ordering::Relaxed);
        result
    }

    fn send(&self, stream: &mut TcpStream, path: &Path, asked: &Request) -> io::Result<()> {
        // The precompressed copy if there is one and this client can read it.
        let packed = ENCODINGS.iter().find_map(|(encoding, suffix)| {
            if !asked.accepts.contains(encoding) {
                return None;
            }
            let mut name = path.as_os_str().to_owned();
            name.push(format!(".{suffix}"));
            let candidate = PathBuf::from(name);
            candidate.is_file().then_some((*encoding, candidate))
        });
        let mut file = File::open(packed.as_ref().map_or(path, |(_, packed)| packed))?;
        let length = file.metadata()?.len();
        let encoded = packed.map_or(String::new(), |(encoding, _)| format!("Content-Encoding: {encoding}\r\n"));
        // Whatever has its build in its address can be kept for good. Anything else must be checked every time,
        // or a browser would hold on to an old page after a deploy.
        let cache = if asked.versioned { "public, max-age=31536000, immutable" } else { "no-cache" };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {length}\r\n{encoded}Vary: Accept-Encoding\r\nCache-Control: {cache}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
            content_type(path),
        )?;
        if asked.head_only {
            return Ok(());
        }
        let mut piece = vec![0; COPY_BYTES];
        loop {
            let read = file.read(&mut piece)?;
            if read == 0 {
                return Ok(());
            }
            stream.write_all(&piece[..read])?;
        }
    }
}

/// The map this server plays, at `/map/<name>.toml`, with or without a page, for clients that do not have it.
/// The same bytes it hashed, so whatever fetches it gets what it was told the hash of.
pub struct MapFile {
    address: String,
    text: Vec<u8>,
}

impl MapFile {
    pub fn new(name: &str, text: &str) -> Self {
        Self { address: format!("/map/{name}.toml"), text: text.as_bytes().to_vec() }
    }

    /// Answers if this asks for the map, and leaves anything else for whatever serves the page.
    pub fn respond(&self, stream: &mut TcpStream, request: &str) -> Option<io::Result<()>> {
        let asked = parse(request)?;
        if asked.address != self.address {
            return None;
        }
        // Asked for by its hash, it can be kept for good. A page from anywhere may fetch it: it is no secret.
        let cache = if asked.versioned { "public, max-age=31536000, immutable" } else { "no-cache" };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nAccess-Control-Allow-Origin: *\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
            self.text.len()
        );
        let sent = stream.write_all(head.as_bytes()).and_then(|()| if asked.head_only { Ok(()) } else { stream.write_all(&self.text) });
        Some(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_climbs_out_of_the_folder_however_it_is_spelled() {
        for bad in [
            "/../secret",
            "/a/../../secret",
            "/%2e%2e/secret",
            "/%2E%2E%2Fsecret",
            "/..%2fsecret",
            "/a/%2e%2e/%2e%2e/secret",
            "/..\\secret",
            "/%5c..%5csecret",
            "/a%00b",
            "/%zz",
            "/%",
        ] {
            assert_eq!(relative_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn ordinary_addresses_mean_what_they_say() {
        assert_eq!(relative_path("/"), Some(PathBuf::new()));
        assert_eq!(relative_path("/prs_game.js"), Some(PathBuf::from("prs_game.js")));
        assert_eq!(relative_path("/tune/a%20b.wav"), Some(PathBuf::from("tune/a b.wav")));
        // Absolute after the slash is stripped is only an odd name under the folder.
        assert_eq!(relative_path("//etc/passwd"), Some(PathBuf::from("etc/passwd")));
    }

    #[test]
    fn a_request_is_read_for_what_matters() {
        let got = parse("GET /a.wasm?v=abc HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip, BR\r\n\r\n").unwrap();
        assert!(!got.head_only && got.versioned);
        assert_eq!(got.address, "/a.wasm");
        assert!(got.accepts.contains("br"));
        assert!(parse("HEAD / HTTP/1.1\r\n\r\n").unwrap().head_only);
        assert!(parse("POST / HTTP/1.1\r\n\r\n").is_none());
    }
}
