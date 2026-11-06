//! A real client in a real browser, with no game around it: real `prs_sim` compiled to wasm, a real
//! browser WebSocket and a real server. It walks around and reports how prediction is going.
//!
//! ```sh
//! cargo build -p prs_net_sim --example browser_probe --target wasm32-unknown-unknown --release
//! wasm-bindgen bin/wasm32-unknown-unknown/release/examples/browser_probe.wasm --target web --out-dir <dir>
//! ```
//! then a page that calls `new Probe(url)` and `frame(dt)` on a timer, and reads `stats()`.
//! `fingerprint` gives the same text as the `prs_fingerprint` binary, to compare native with wasm.
#![cfg(target_arch = "wasm32")]

use glam::Vec2;
use prs_net::WsClient;
use prs_net::client::{ClientConfig, ClientSession};
use prs_net::transport::{Transport, TransportState, pump};
use prs_net_sim::SimClient;
use prs_sim::protocol::{Buttons, PlayerCmd};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn fingerprint(map: &str, ticks: u32, every: u32) -> Option<String> {
    prs_net_sim::fingerprint::run(map, ticks, every)
}

#[wasm_bindgen]
pub fn fingerprint_ride(map: &str, carrier: u32, flying: bool, ticks: u32, every: u32) -> Option<String> {
    prs_net_sim::fingerprint::run_ride(map, carrier, flying, ticks, every)
}

#[wasm_bindgen]
pub struct Probe {
    session: ClientSession<SimClient>,
    socket: WsClient,
}

#[wasm_bindgen]
impl Probe {
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Probe {
        let make = |map: &str, mode: &str| SimClient::setup(map, mode);
        Probe { session: ClientSession::new(Box::new(make), ClientConfig::default()), socket: WsClient::connect(url) }
    }

    pub fn frame(&mut self, dt: f32) {
        pump(&mut self.session, &mut self.socket);
        self.session.update(dt, |tick| {
            let t = tick as f32 / 60.0;
            let mut buttons = Buttons::default();
            buttons.set(Buttons::FIRE, tick % 240 < 20);
            buttons.set(Buttons::JUMP, tick % 300 == 0);
            PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), yaw: t * 0.5, pitch: (t * 0.3).sin() * 0.1, buttons, ..Default::default() }
        });
        self.session.draw(dt);
        pump(&mut self.session, &mut self.socket);
        self.session.drain_predicted_events();
        self.session.drain_server_events();
    }

    pub fn stats(&mut self) -> String {
        let s = self.session.stats();
        let p = self.session.predictor().map(|p| p.stats()).unwrap_or_default();
        let rtt = self.session.clock().map_or(-1.0, |c| c.rtt_ms());
        let open = self.socket.state() == TransportState::Open;
        format!(
            "{{\"open\":{open},\"state\":\"{:?}\",\"snapshots\":{},\"bytes_down\":{},\"bytes_up\":{},\"bad_frames\":{},\"missing_baselines\":{},\"rtt_ms\":{rtt},\"compared\":{},\"reconciles\":{},\"max_error_m\":{},\"max_correction_m\":{},\"remotes\":{}}}",
            self.session.state(),
            s.snapshots,
            s.bytes_down,
            s.bytes_up,
            s.bad_frames,
            s.missing_baselines,
            p.compared,
            p.reconciles,
            p.max_error,
            p.max_correction,
            self.session.remotes().len(),
        )
    }
}
