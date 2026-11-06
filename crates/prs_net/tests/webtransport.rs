//! The whole loop over real WebTransport on localhost, with the toy game, and the WebSocket it falls
//! back on when WebTransport can't be had.

mod common;

use common::*;
use prs_net::client::{ClientConfig, ClientSession, ClientState};
use prs_net::entity::MAX_FIELDS;
use prs_net::server::{GameServer, ServerConfig};
use prs_net::transport::{FirstThen, Transport, TransportState, pump};
use prs_net::wire::{BaseRef, Baselines, ClientMsg, PROTOCOL_VERSION, RejectReason, ServerMsg};
use prs_net::ws_host::{HostConfig, WsHost};
use prs_net::wt_host::{WtCertificate, WtConfig};
use prs_net::{WsClient, WtClient};
use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PATIENCE_MS: f64 = 3000.0;

fn webtransport() -> WtConfig {
    WtConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        certificate: WtCertificate::SelfSigned { names: vec!["localhost".into(), "127.0.0.1".into()], valid_days: 13 },
        refresh_every: Duration::from_secs(3600),
        idle_timeout: Duration::from_secs(10),
    }
}

/// A server running the toy for `seconds`, and the address of its WebSocket.
fn start(webtransport: Option<WtConfig>, seconds: u64) -> (String, std::thread::JoinHandle<(u32, f64)>) {
    let cfg = HostConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        status_every: None,
        run_for: Some(Duration::from_secs(seconds)),
        webtransport,
        ..Default::default()
    };
    let host = WsHost::bind(cfg).unwrap();
    let url = format!("ws://{}", host.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (game, stats) = host.run(GameServer::new(ToyServer::new(), ServerConfig::default()));
        (game.game.tick, stats.busy_fraction)
    });
    (url, server)
}

fn now_ms() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn either_way(url: &str) -> FirstThen {
    let fallback = url.to_owned();
    FirstThen::new(
        Box::new(WtClient::connect(url)),
        Box::new(move || Box::new(WsClient::connect(&fallback))),
        PATIENCE_MS,
        Box::new(now_ms),
    )
}

/// Plays a few clients for `seconds` of real time and checks that each one played properly.
fn play(url: &str, clients: usize, seconds: u64) -> Vec<(ClientSession<ToyClient>, FirstThen)> {
    let mut bots: Vec<_> =
        (0..clients).map(|_| (ClientSession::new(ToyClient::factory(), ClientConfig::default()), either_way(url))).collect();
    let script = busy();
    let frame = Duration::from_micros(16_667);
    let start = Instant::now();
    let mut last = start;
    while start.elapsed() < Duration::from_secs(seconds) {
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
        }
    }
    for (session, socket) in &mut bots {
        assert_eq!(socket.state(), TransportState::Open);
        assert_eq!(session.state(), ClientState::Playing);
        let stats = session.stats();
        assert_eq!(stats.bad_frames, 0);
        assert!(stats.snapshots > 60, "{stats:?}");
        assert_eq!(session.remotes().len(), clients - 1);
        let prediction = session.predictor().unwrap().stats();
        assert!(prediction.compared > 60, "{prediction:?}");
        assert!(prediction.max_error < 0.5, "{prediction:?}");
    }
    bots
}

#[test]
fn clients_play_over_real_webtransport() {
    let (url, server) = start(Some(webtransport()), 6);
    let bots = play(&url, 4, 5);
    for (_, socket) in &bots {
        assert!(!socket.fell_back());
        assert!(!socket.is_reliable());
    }
    drop(bots);
    let (ticks, busy) = server.join().unwrap();
    assert!(ticks > 300, "{ticks} ticks in 6 s");
    assert!(busy < 0.5, "busy {busy}");
}

#[test]
fn a_server_without_webtransport_is_played_over_the_websocket() {
    let (url, server) = start(None, 6);
    let bots = play(&url, 2, 5);
    for (_, socket) in &bots {
        assert!(socket.fell_back());
        assert!(socket.is_reliable());
    }
    drop(bots);
    server.join().unwrap();
}

struct NoBaselines;

impl Baselines<ToySchema> for NoBaselines {
    fn base_at(&self, _: u32) -> Option<BaseRef<'_, ToySchema>> {
        None
    }

    fn entity_at(&self, _: u32, _: u32) -> Option<[i32; MAX_FIELDS]> {
        None
    }
}

/// What a client of another version hears back, over either transport.
fn refusal(mut socket: Box<dyn Transport>) -> Option<RejectReason> {
    let start = Instant::now();
    let mut said = false;
    while start.elapsed() < Duration::from_secs(4) {
        if !said && socket.state() == TransportState::Open {
            socket.send(&ClientMsg::Hello { protocol: PROTOCOL_VERSION + 1 }.encode());
            said = true;
        }
        while let Some(frame) = socket.recv() {
            if let Ok(ServerMsg::Reject(reason, _)) = ServerMsg::<ToySchema>::decode(&frame, &NoBaselines) {
                return Some(reason);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn a_client_of_another_version_is_turned_away_the_same_way_on_both() {
    let (url, server) = start(Some(webtransport()), 5);
    assert_eq!(refusal(Box::new(WtClient::connect(&url))), Some(RejectReason::Protocol));
    assert_eq!(refusal(Box::new(WsClient::connect(&url))), Some(RejectReason::Protocol));
    server.join().unwrap();
}
