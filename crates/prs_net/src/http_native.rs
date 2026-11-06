//! Fetching a file from a server on a desktop, over the same plain or TLS connection a WebSocket to it would
//! use. Blocking, so it belongs on a thread of its own. Whatever is at the other end is not trusted: the
//! whole answer is held to a size, its head to a much smaller one, and the lot to a time.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::ws_native::dial;

/// The status line and headers of an answer never need more than this.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// A bare HTTP/1.0 GET of `url`, `http://` or `https://`, giving the status and the body. Gives up on a body
/// past `max_bytes` or an answer that takes longer than `timeout` in all, and counts what of the body has
/// come so far in `received`, for a loading screen.
pub fn fetch(url: &str, max_bytes: usize, timeout: Duration, received: &AtomicUsize) -> Result<(u16, Vec<u8>), String> {
    let deadline = Instant::now() + timeout;
    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => return Err("only http:// and https://".into()),
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let mut stream = dial(secure, rest)?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {authority}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    let mut chunk = vec![0; 16 * 1024];
    let mut head_len = None;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("no whole answer in {:.1} s", timeout.as_secs_f32()));
        }
        // No one read may wait past the deadline either.
        stream.tcp().set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        let read = match stream.read(&mut chunk) {
            // A server going away without saying goodbye over TLS has still said everything up to there.
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let searched = response.len().saturating_sub(3);
        response.extend_from_slice(&chunk[..read]);
        match head_len {
            None => {
                // Only what is new, and where it joins what came before.
                head_len = response[searched..].windows(4).position(|w| w == b"\r\n\r\n").map(|at| searched + at + 4);
                if head_len.is_none() && response.len() > MAX_HEAD_BYTES {
                    return Err("an answer with no end to its head".into());
                }
            }
            Some(_) => {}
        }
        let body = response.len() - head_len.unwrap_or(response.len());
        received.store(body, Ordering::Relaxed);
        if body > max_bytes {
            return Err(format!("more than {max_bytes} bytes"));
        }
    }
    let head_len = head_len.ok_or("no answer")?;
    let head = String::from_utf8_lossy(&response[..head_len]);
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("no status")?;
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok());
    let body = response.split_off(head_len);
    if length.is_some_and(|length| length != body.len()) {
        return Err(format!("cut short at {} of {} bytes", body.len(), length.unwrap_or_default()));
    }
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    const PATIENCE: Duration = Duration::from_secs(10);

    /// Answers one request with `answer`, then keeps the connection open for `linger`.
    fn serve_once(answer: Vec<u8>, linger: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let _ = stream.write_all(&answer);
            std::thread::sleep(linger);
        });
        format!("http://{address}/map/town.toml?v=1")
    }

    #[test]
    fn a_file_comes_back_whole_and_counted() {
        let body = vec![b'x'; 100_000];
        let mut answer = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        answer.extend_from_slice(&body);
        let received = AtomicUsize::new(0);
        let (status, got) = fetch(&serve_once(answer, Duration::ZERO), 1 << 20, PATIENCE, &received).unwrap();
        assert_eq!((status, got.len()), (200, body.len()));
        assert_eq!(received.load(Ordering::Relaxed), body.len());
    }

    #[test]
    fn too_much_or_too_little_is_an_error() {
        let received = AtomicUsize::new(0);
        let big = format!("HTTP/1.1 200 OK\r\n\r\n{}", "y".repeat(5000)).into_bytes();
        assert!(fetch(&serve_once(big, Duration::ZERO), 1000, PATIENCE, &received).is_err());
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nabc".to_vec();
        assert!(fetch(&serve_once(short, Duration::ZERO), 1000, PATIENCE, &received).is_err());
        let missing = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec();
        assert_eq!(fetch(&serve_once(missing, Duration::ZERO), 1000, PATIENCE, &received).unwrap().0, 404);
    }

    /// A server that never ends its head cannot make the client hold on to it for ever.
    #[test]
    fn a_head_that_never_ends_is_given_up_on() {
        let received = AtomicUsize::new(0);
        let endless = format!("HTTP/1.1 200 OK\r\n{}", "X-Junk: a\r\n".repeat(100_000)).into_bytes();
        let error = fetch(&serve_once(endless, Duration::ZERO), 1 << 30, PATIENCE, &received).unwrap_err();
        assert!(error.contains("head"), "{error}");
    }

    /// One that says nothing, and does not hang up, is given up on in time.
    #[test]
    fn a_server_that_stalls_is_given_up_on() {
        let received = AtomicUsize::new(0);
        let start = Instant::now();
        let stalled = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc".to_vec();
        assert!(fetch(&serve_once(stalled, Duration::from_secs(8)), 1000, Duration::from_millis(300), &received).is_err());
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
    }
}
