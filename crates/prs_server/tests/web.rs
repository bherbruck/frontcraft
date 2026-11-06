//! The page and the game on one real port.

use prs_net::server::{GameServer, ServerConfig};
use prs_net::ws_host::{HostConfig, HostStats, PlainHttp, WsHost};
use prs_net_sim::{SimServer, SimServerConfig};
use prs_server::web::{WebConfig, WebRoot};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// A folder like a web build, with a secret beside it that must stay out of reach.
fn site(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("prs_web_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("site");
    std::fs::create_dir_all(root.join("tune")).unwrap();
    std::fs::write(base.join("secret"), "the secret").unwrap();
    std::fs::write(root.join("index.html"), "<html>page</html>").unwrap();
    std::fs::write(root.join("tune/index.html"), "tuner").unwrap();
    std::fs::write(root.join("prs_game_bg.wasm"), b"\0asm plain").unwrap();
    std::fs::write(root.join("prs_game_bg.wasm.br"), "brotli bytes").unwrap();
    std::fs::write(root.join("prs_game_bg.wasm.gz"), "gzip bytes").unwrap();
    std::fs::write(root.join("big.bin"), vec![7u8; 64 * 1024 * 1024]).unwrap();
    std::os::unix::fs::symlink(base.join("secret"), root.join("link")).unwrap();
    root
}

fn host(root: &PathBuf, seconds: f64) -> (SocketAddr, JoinHandle<HostStats>) {
    let web = Arc::new(WebRoot::new(root, WebConfig::default()).unwrap());
    let cfg = HostConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        status_every: None,
        run_for: Some(Duration::from_secs_f64(seconds)),
        plain_http: Some(PlainHttp(Arc::new(move |stream, request| {
            let _ = web.respond(stream, request);
        }))),
        ..Default::default()
    };
    let listening = WsHost::bind(cfg).unwrap();
    let addr = listening.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let game = SimServer::new("range", "practice", SimServerConfig::default()).unwrap();
        listening.run(GameServer::new(game, ServerConfig::default())).1
    });
    (addr, handle)
}

fn get(addr: SocketAddr, target: &str, headers: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    write!(stream, "GET {target} HTTP/1.1\r\nHost: x\r\n{headers}\r\n").unwrap();
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn the_page_comes_with_the_right_headers_and_the_folder_stays_shut() {
    let root = site("headers");
    let (addr, _) = host(&root, 6.0);

    let page = get(addr, "/", "");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("text/html") && page.contains("Cache-Control: no-cache") && page.ends_with("<html>page</html>"), "{page}");
    assert!(get(addr, "/tune/", "").ends_with("tuner"));

    let plain = get(addr, "/prs_game_bg.wasm?v=abc", "");
    assert!(plain.contains("Content-Type: application/wasm") && plain.contains("immutable"), "{plain}");
    assert!(plain.contains("Vary: Accept-Encoding") && !plain.contains("Content-Encoding"), "{plain}");
    assert!(plain.ends_with("asm plain"));

    let br = get(addr, "/prs_game_bg.wasm", "Accept-Encoding: gzip, br\r\n");
    assert!(br.contains("Content-Encoding: br") && br.contains("application/wasm") && br.ends_with("brotli bytes"), "{br}");
    assert!(br.contains("Cache-Control: no-cache"), "no build in the address, so it must be checked: {br}");
    let gz = get(addr, "/prs_game_bg.wasm", "Accept-Encoding: gzip\r\n");
    assert!(gz.contains("Content-Encoding: gzip") && gz.ends_with("gzip bytes"), "{gz}");

    for bad in ["/../secret", "/%2e%2e/secret", "/..%2fsecret", "/link", "/nothing", "/%", "/a/../../secret"] {
        let answer = get(addr, bad, "");
        assert!(answer.starts_with("HTTP/1.1 404"), "{bad}: {answer}");
        assert!(!answer.contains("the secret"), "{bad}");
    }
    assert!(get(addr, "/health", "").contains("ok"));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn a_download_nobody_reads_costs_the_game_nothing() {
    let root = site("slow");
    let (addr, server) = host(&root, 4.0);
    // Ask for a lot and read none of it, from several connections, and send rubbish from others.
    let mut stuck = Vec::new();
    for _ in 0..20 {
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(stream, "GET /big.bin HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        stuck.push(stream);
    }
    for _ in 0..20 {
        // Says nothing at all, holding on to a place in the queue.
        stuck.push(TcpStream::connect(addr).unwrap());
    }
    let page = get(addr, "/", "");
    assert!(page.ends_with("<html>page</html>"), "others still get served: {page}");
    let stats = server.join().unwrap();
    println!("with 40 stuck connections: {}", stats.times);
    assert!(stats.ticks > 200, "{} ticks in 4 s", stats.ticks);
    // A busy machine can hold up a thread for reasons of its own. A blocked game loop would be stuck for the whole run.
    assert!(stats.times.worst_late_ms < 1000.0, "{}", stats.times);
    drop(stuck);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
