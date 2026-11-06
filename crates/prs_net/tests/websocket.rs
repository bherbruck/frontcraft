//! The whole loop over real WebSockets on localhost, with the toy game: sockets, framing, handshake.

mod common;

use common::*;
use prs_net::WsClient;
use prs_net::client::{ClientConfig, ClientSession, ClientState};
use prs_net::server::{GameServer, ServerConfig};
use prs_net::transport::{Transport, TransportState, pump};
use prs_net::ws_host::{HostConfig, WsHost};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[test]
fn clients_play_over_a_real_websocket() {
    let cfg = HostConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        status_every: None,
        run_for: Some(Duration::from_secs(5)),
        ..Default::default()
    };
    let host = WsHost::bind(cfg).unwrap();
    let url = format!("ws://{}", host.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (game, stats) = host.run(GameServer::new(ToyServer::new(), ServerConfig::default()));
        (game.game.tick, game.game.shots.clone(), stats)
    });

    let mut bots: Vec<_> = (0..4)
        .map(|_| {
            (ClientSession::new(ToyClient::factory(), ClientConfig::default()), WsClient::connect(&url))
        })
        .collect();
    let script = busy();
    let frame = Duration::from_micros(16_667);
    let start = Instant::now();
    let mut last = start;
    let mut sent_open = false;
    while start.elapsed() < Duration::from_secs(4) {
        std::thread::sleep(frame.saturating_sub(last.elapsed()));
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        for (who, (session, socket)) in bots.iter_mut().enumerate() {
            pump(session, socket);
            let script = script.clone();
            session.update(dt, |tick| script(who as u32, tick));
            session.draw(dt);
            pump(session, socket);
            session.drain_predicted_events();
            session.drain_server_events();
            sent_open |= socket.state() == TransportState::Open;
        }
    }
    assert!(sent_open);
    for (session, socket) in &mut bots {
        assert_eq!(socket.state(), TransportState::Open);
        assert_eq!(session.state(), ClientState::Playing);
        let stats = session.stats();
        assert_eq!(stats.bad_frames, 0);
        assert_eq!(stats.missing_baselines, 0);
        assert!(stats.snapshots > 60, "{stats:?}");
        assert_eq!(session.remotes().len(), 3);
        let prediction = session.predictor().unwrap().stats();
        assert!(prediction.compared > 60, "{prediction:?}");
        assert!(prediction.max_error < 0.5, "{prediction:?}");
    }
    drop(bots);
    let (ticks, _shots, stats) = server.join().unwrap();
    assert!(ticks > 250, "{ticks} ticks in 5 s");
    assert!(stats.busy_fraction < 0.5, "{stats:?}");
}

fn start_host(allowed_origins: Vec<String>) -> SocketAddr {
    let cfg = HostConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        status_every: None,
        run_for: Some(Duration::from_secs(4)),
        allowed_origins,
        ..Default::default()
    };
    let host = WsHost::bind(cfg).unwrap();
    let addr = host.local_addr().unwrap();
    std::thread::spawn(move || host.run(GameServer::new(ToyServer::new(), ServerConfig::default())));
    addr
}

fn plain_request(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
}

#[test]
fn a_host_can_check_health_over_plain_http_on_the_same_port() {
    let addr = start_host(Vec::new());
    let answer = plain_request(addr, "GET /health HTTP/1.1\r\nHost: healthcheck.railway.app\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let answer = plain_request(addr, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 404"), "{answer}");
    // Rubbish, then nothing at all, then going away: none of these should stop it answering.
    let _ = plain_request(addr, "\u{1}\u{2} nonsense\r\n\r\n");
    drop(TcpStream::connect(addr).unwrap());
    let answer = plain_request(addr, "GET /health HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

#[test]
fn a_page_listing_servers_can_ask_how_many_are_playing() {
    let addr = start_host(Vec::new());
    let answer = plain_request(addr, "GET /status HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("Access-Control-Allow-Origin: *"), "{answer}");
    assert!(answer.ends_with(&format!(r#"{{"players":0,"max":{}}}"#, ServerConfig::default().max_players)), "{answer}");
}

fn websocket_request(addr: SocketAddr, origin: &str) -> String {
    plain_request(
        addr,
        &format!(
            "GET / HTTP/1.1\r\nHost: {addr}\r\nOrigin: {origin}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        ),
    )
}

#[test]
fn a_page_from_somewhere_else_is_turned_away_and_a_listed_one_is_not() {
    let addr = start_host(vec!["https://frontcraft.online".to_owned()]);
    let answer = websocket_request(addr, "https://evil.example");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    let stream = std::thread::spawn(move || websocket_request(addr, "https://frontcraft.online"));
    assert!(stream.join().unwrap().starts_with("HTTP/1.1 101"));
}
