//! A whole game on a simulated network: a real server session and real client sessions, with a
//! virtual clock, so tests are fast and exactly repeatable. Works with any game, so the toy in this
//! crate's tests and the real simulation run through the same harness.

use glam::Vec3;
use prs_protocol::PlayerCmd;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::client::{ClientConfig, ClientSession, GameFactory};
use crate::game::{PredictedGame, Schema, ServerGame};
use crate::server::{ClientId, GameServer, ServerConfig};
use crate::simnet::{LinkConfig, LinkMode, SimEndpoint, sim_connection};
use crate::transport::{Transport, pump};

/// How often clients and server are looked at: finer than a frame, so arrivals are not quantised.
const STEP_MS: f64 = 1000.0 / 480.0;
pub const TICK_MS: f64 = 1000.0 / 60.0;

/// The network and client settings a player joins with.
#[derive(Clone)]
pub struct Net {
    pub up: LinkConfig,
    pub down: LinkConfig,
    pub reliable: bool,
    pub client: ClientConfig,
}

impl Net {
    /// Like UDP or WebTransport datagrams. A command gets more chances, as packets can be lost.
    pub fn datagram(latency_ms: f64, jitter_ms: f64, loss: f64) -> Self {
        let link = LinkConfig { latency_ms, jitter_ms, loss, mode: LinkMode::Datagram };
        let client = ClientConfig { cmd_redundancy: 4, ..Default::default() };
        Self { up: link, down: link, reliable: false, client }
    }

    /// Like WebTransport as the hosts use it: datagrams, with what must arrive on a stream beside them.
    pub fn quic(latency_ms: f64, jitter_ms: f64, loss: f64, retransmit_ms: f64) -> Self {
        let link = LinkConfig { latency_ms, jitter_ms, loss, mode: LinkMode::Quic { retransmit_ms } };
        Self { up: link, down: link, ..Self::datagram(latency_ms, jitter_ms, loss) }
    }

    /// Like a WebSocket: nothing is lost, and a lost packet holds up everything behind it.
    pub fn tcp(latency_ms: f64, jitter_ms: f64, loss: f64, retransmit_ms: f64) -> Self {
        let mode = LinkMode::Reliable { retransmit_ms };
        let link = LinkConfig { latency_ms, jitter_ms, loss, mode };
        Self { up: link, down: link, reliable: true, client: ClientConfig::default() }
    }
}

/// What a client drew on one frame.
pub struct Frame {
    pub view: Vec3,
    pub play_tick: f64,
    pub remotes: Vec<(u32, Vec3)>,
}

pub struct Player<C: PredictedGame> {
    pub session: ClientSession<C>,
    pub socket: SimEndpoint,
    pub server_side: SimEndpoint,
    pub id: ClientId,
    pub who: u32,
    input: Rc<dyn Fn(u32, u32) -> PlayerCmd>,
    next_frame_ms: f64,
    /// A real client's clock is not quite anyone else's.
    pub clock_skew: f64,
    pub frame_ms: f64,
    pub frames: Vec<Frame>,
    pub server_events: Vec<<C::S as Schema>::Event>,
    pub predicted_events: Vec<C::Event>,
    /// Like a browser tab out of sight: no frames until then, or only throttled ones. Frames still arrive
    /// and wait to be read, as they do in a browser.
    pub hidden: Option<Hidden>,
    /// Most real time one frame is allowed to take in. Bevy's `Time<Virtual>` caps it like this.
    pub max_frame_ms: f64,
    last_frame_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Hidden {
    pub until_ms: f64,
    /// A frame this often while hidden, as a browser's throttled timers give. `None` for none at all.
    pub frame_every_ms: Option<f64>,
}

impl<C: PredictedGame> Player<C> {
    pub fn reconciles(&self) -> u32 {
        self.session.predictor().map_or(0, |p| p.stats().reconciles)
    }
}

/// Places to look in on a run, for anything a game wants to record for itself.
pub trait Observer<C: PredictedGame, G: ServerGame> {
    fn after_tick(&mut self, _server: &GameServer<G>) {}
    fn after_frame(&mut self, _who: u32, _session: &ClientSession<C>) {}
}

impl<C: PredictedGame, G: ServerGame> Observer<C, G> for () {}

pub struct World<C: PredictedGame, G: ServerGame> {
    pub clock: Rc<Cell<f64>>,
    pub server: GameServer<G>,
    next_tick_ms: f64,
    pub players: Vec<Player<C>>,
    /// Per player, per tick, where the server has them.
    pub truth: HashMap<u16, HashMap<u32, Vec3>>,
    position_of: fn(&G, u16) -> Vec3,
    /// Keeping every frame and tick takes memory that long runs with many players may not want.
    pub record: bool,
}

impl<C: PredictedGame, G: ServerGame<S = C::S>> World<C, G> {
    pub fn new(game: G, cfg: ServerConfig, position_of: fn(&G, u16) -> Vec3) -> Self {
        Self {
            clock: Rc::new(Cell::new(0.0)),
            server: GameServer::new(game, cfg),
            next_tick_ms: TICK_MS,
            players: Vec::new(),
            truth: HashMap::new(),
            position_of,
            record: true,
        }
    }

    /// `input` gives the command for (player number, tick).
    pub fn join(&mut self, game: GameFactory<C>, input: Rc<dyn Fn(u32, u32) -> PlayerCmd>, net: &Net) -> usize {
        let who = self.players.len() as u32;
        let (socket, server_side) =
            sim_connection(self.clock.clone(), net.up, net.down, 0x1000 + who as u64 * 77);
        let id = self.server.connect(net.reliable);
        self.players.push(Player {
            session: ClientSession::new(game, net.client.clone()),
            socket,
            server_side,
            id,
            who,
            input,
            next_frame_ms: self.clock.get(),
            clock_skew: 1.0,
            frame_ms: TICK_MS,
            frames: Vec::new(),
            server_events: Vec::new(),
            predicted_events: Vec::new(),
            hidden: None,
            max_frame_ms: 2000.0,
            last_frame_ms: self.clock.get(),
        });
        self.players.len() - 1
    }

    pub fn run(&mut self, seconds: f64) {
        self.run_observed(seconds, &mut ());
    }

    pub fn run_observed(&mut self, seconds: f64, observer: &mut dyn Observer<C, G>) {
        let end = self.clock.get() + seconds * 1000.0;
        while self.clock.get() < end {
            let now = self.clock.get() + STEP_MS;
            self.clock.set(now);
            for player in &mut self.players {
                while let Some(frame) = player.server_side.recv() {
                    self.server.receive(player.id, &frame);
                }
            }
            if now >= self.next_tick_ms {
                self.next_tick_ms += TICK_MS;
                self.server.tick();
                self.record_truth();
                observer.after_tick(&self.server);
            }
            for (id, frame) in self.server.drain_outgoing() {
                if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
                    player.server_side.send(&frame);
                }
            }
            for player in &mut self.players {
                if let Some(hidden) = player.hidden {
                    if now >= hidden.until_ms {
                        player.hidden = None;
                        player.next_frame_ms = now;
                    } else {
                        match hidden.frame_every_ms {
                            Some(every) if now - player.last_frame_ms >= every => player.next_frame_ms = now,
                            _ => continue,
                        }
                    }
                }
                pump(&mut player.session, &mut player.socket);
                if now >= player.next_frame_ms {
                    player.next_frame_ms = (player.next_frame_ms + player.frame_ms).max(now);
                    // A late frame catches up the way Bevy's fixed steps do: one update per step it is owed.
                    let owed_real = now - player.last_frame_ms;
                    let owed = owed_real.min(player.max_frame_ms).max(player.frame_ms);
                    player.last_frame_ms = now;
                    let steps = (owed / player.frame_ms).round().max(1.0) as u32;
                    let dt = (player.frame_ms * player.clock_skew / 1000.0) as f32;
                    let (input, who) = (player.input.clone(), player.who);
                    player.session.frame_started((owed_real / 1000.0) as f32);
                    for step in 0..steps {
                        if step > 0 {
                            pump(&mut player.session, &mut player.socket);
                        }
                        player.session.update(dt, |tick| input(who, tick));
                    }
                    player.session.draw(dt * steps as f32);
                    pump(&mut player.session, &mut player.socket);
                    player.server_events.append(&mut player.session.drain_server_events());
                    player.predicted_events.append(&mut player.session.drain_predicted_events());
                    if self.record {
                        let frame = Frame {
                            view: player.session.view_position().unwrap_or_default(),
                            play_tick: player.session.interp().map_or(0.0, |i| i.play_tick()),
                            remotes: player
                                .session
                                .remotes()
                                .iter()
                                .map(|(id, pose)| (*id, <C::S as Schema>::pose_position(pose)))
                                .collect(),
                        };
                        player.frames.push(frame);
                    }
                    observer.after_frame(who, &player.session);
                }
            }
        }
    }

    fn record_truth(&mut self) {
        if !self.record {
            return;
        }
        let tick = self.server.game.tick();
        for player in &self.players {
            let Some(id) = self.server.player_of(player.id) else { continue };
            let position = (self.position_of)(&self.server.game, id);
            self.truth.entry(id).or_default().insert(tick, position);
        }
    }

    /// Where the server had a player at a moment between ticks.
    pub fn truth_at(&self, player: u16, tick: f64) -> Option<Vec3> {
        let history = self.truth.get(&player)?;
        let (a, b) = (history.get(&(tick.floor() as u32))?, history.get(&(tick.ceil() as u32))?);
        Some(a.lerp(*b, tick.fract() as f32))
    }

    pub fn player_id(&self, player: usize) -> u16 {
        self.server.player_of(self.players[player].id).expect("joined")
    }
}
