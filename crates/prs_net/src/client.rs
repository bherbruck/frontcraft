//! One player's connection to a server, without the connection: bytes in, bytes out, on whatever
//! clock the host gives it. The host calls [`ClientSession::update`] once a frame.

use glam::Vec3;
use prs_protocol::{PlayerCmd, TICK_DT};
use std::collections::VecDeque;

use crate::clock::{ClockConfig, ClockSync};
use crate::entity::MAX_FIELDS;
use crate::game::{PredictedGame, Schema};
use crate::interp::{InterpConfig, Interpolator};
use crate::predict::{PredictConfig, Predictor};
use crate::validate::CmdLimits;
use crate::wire::{
    BaseRef, Baselines, ClientMsg, CmdPacket, PROTOCOL_VERSION, RejectReason, ServerMsg, Snapshot,
    VoiceFrame, VoiceUp, Welcome, WireError,
};

/// Frames held for a host that has stopped collecting them, as nothing else would stop it growing.
const VOICE_IN_FRAMES: usize = 256;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// What the player would like to be called. The server has the last word.
    pub name: String,
    pub clock: ClockConfig,
    pub interp: InterpConfig,
    pub predict: PredictConfig,
    /// Commands in each packet on a link that can lose them, so older ones get more chances. Over TCP
    /// only the new ones are sent.
    pub cmd_redundancy: usize,
    /// Most ticks to run in one frame after a hitch. Beyond this the time is simply skipped.
    pub max_catchup_ticks: u32,
    /// Running more ticks than this at once is shown through the visual offset, not as a jump.
    pub smooth_catchup_above: u32,
    /// Keep this the same as the server's `CmdLimits::max_wish`. A command is shortened to it here too,
    /// so that the one predicted with is the one the server runs.
    pub max_wish: f32,
    /// Most requests to carry in one packet, however many are waiting.
    pub requests_per_packet: usize,
    /// How long to wait for an answer before asking again, as anything can be lost on the way in.
    pub handshake_resend_ms: f64,
    /// A frame that took longer than this, as when a browser tab was out of sight, means the player was
    /// away. The ticks the host would run to catch up are not run: the clock is put right by the server.
    /// Keep it at least the longest frame the host catches up on in full, so that a slow machine is never
    /// taken for an absent player.
    pub away_after_s: f32,
    /// ...and this many times as long as frames usually take. A machine that is always that slow is slow,
    /// not away, and gets every tick it can.
    pub away_after_usual: f32,
    /// Weight of a new frame in what frames usually take.
    pub usual_frame_smoothing: f32,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            clock: ClockConfig::default(),
            interp: InterpConfig::default(),
            predict: PredictConfig::default(),
            cmd_redundancy: 4,
            max_catchup_ticks: 60,
            smooth_catchup_above: 1,
            max_wish: CmdLimits::default().max_wish,
            requests_per_packet: 8,
            handshake_resend_ms: 250.0,
            away_after_s: 2.0,
            away_after_usual: 4.0,
            usual_frame_smoothing: 0.1,
            name: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ClientStats {
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub frames_up: u64,
    pub frames_down: u64,
    pub snapshots: u32,
    /// Older than one already seen. Only happens on transports that can reorder.
    pub stale_snapshots: u32,
    pub missing_baselines: u32,
    pub bad_frames: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientState {
    Joining,
    Playing,
    Rejected(RejectReason),
}

/// One snapshot as kept for the server to delta against.
struct Kept<S: Schema> {
    tick: u32,
    own: Option<S::Own>,
    shared: Vec<u8>,
}

struct BaselineStore<S: Schema> {
    window: u32,
    snapshot_every: u32,
    kept: VecDeque<Kept<S>>,
}

/// What a decoder needs: per snapshot from the store, and per entity from what is being drawn.
struct Lookup<'a, G: PredictedGame> {
    store: &'a BaselineStore<G::S>,
    interp: Option<&'a Interpolator<G::S>>,
}

impl<G: PredictedGame> Baselines<G::S> for Lookup<'_, G> {
    fn base_at(&self, tick: u32) -> Option<BaseRef<'_, G::S>> {
        let kept = self.store.kept.iter().find(|k| k.tick == tick)?;
        Some(BaseRef { own: kept.own.as_ref(), shared: &kept.shared })
    }

    fn entity_at(&self, key: u32, tick: u32) -> Option<[i32; MAX_FIELDS]> {
        self.interp?.entity_at(key, tick)
    }
}

impl<S: Schema> BaselineStore<S> {
    fn newest(&self) -> u32 {
        self.kept.back().map_or(0, |k| k.tick)
    }

    /// Which of the snapshots before the newest are here too.
    fn ack_bits(&self) -> u32 {
        let newest = self.newest();
        let mut bits = 0;
        for kept in &self.kept {
            let back = newest.saturating_sub(kept.tick) / self.snapshot_every.max(1);
            if (1..=32).contains(&back) {
                bits |= 1 << (back - 1);
            }
        }
        bits
    }

    fn push(&mut self, kept: Kept<S>) {
        let tick = kept.tick;
        self.kept.push_back(kept);
        while self.kept.front().is_some_and(|k| k.tick + self.window < tick) {
            self.kept.pop_front();
        }
    }
}

struct Joined<G: PredictedGame> {
    clock: ClockSync,
    predictor: Predictor<G>,
    interp: Interpolator<G::S>,
    /// Commands sent, kept to send again.
    recent: VecDeque<PlayerCmd>,
    fresh: usize,
    last_event_tick: u32,
    last_snapshot: u32,
    me: u16,
    /// Events waiting for their moment to come round on screen, oldest first.
    pending_events: VecDeque<(u32, <G::S as Schema>::Event)>,
}

/// Builds the game once the server has said which map and mode, or says it does not have them.
pub type GameFactory<G> = Box<dyn FnOnce(&str, &str) -> Option<G>>;

pub struct ClientSession<G: PredictedGame> {
    cfg: ClientConfig,
    factory: Option<GameFactory<G>>,
    /// Built, and held until `Welcome` says who we are.
    waiting: Option<G>,
    state: ClientState,
    reject_note: String,
    joined: Option<Joined<G>>,
    baselines: BaselineStore<G::S>,
    outgoing: Vec<Vec<u8>>,
    /// Whether the server carries voice, as far as it has said.
    voice_allowed: bool,
    /// Waiting for a transport with room, and never for long.
    voice_up: VecDeque<Vec<u8>>,
    /// Frames of our own talking held for a transport, oldest lost first. Late is worse than missing.
    voice_up_most: usize,
    voice_in: Vec<VoiceFrame>,
    /// What our own player did, predicted, ready to present.
    predicted_events: Vec<G::Event>,
    /// What the server says happened to everybody else.
    server_events: Vec<<G::S as Schema>::Event>,
    remotes: Vec<(u32, <G::S as Schema>::Pose)>,
    now_ms: f64,
    /// The handshake message waiting on an answer, and when it last went out.
    asking: Option<Vec<u8>>,
    asked_at_ms: Option<f64>,
    mode: Option<String>,
    /// Requests not known to have arrived yet.
    requests: VecDeque<(u32, Vec<u8>)>,
    next_request: u32,
    stats: ClientStats,
    /// Whether the transport can lose what is sent.
    lossy: bool,
    /// Set by a frame that came after an absence, until it has been drawn.
    away: bool,
    /// How long frames usually take, in seconds.
    usual_frame_s: Option<f32>,
    /// Whether the transport has a frame still waiting to go out. Commands then wait to go in the next
    /// packet, all together, rather than queue up behind it one packet each.
    held_back: bool,
}

impl<G: PredictedGame> ClientSession<G> {
    /// `factory` must set up the game by the same rules as the server's, or the server will refuse it.
    pub fn new(factory: GameFactory<G>, cfg: ClientConfig) -> Self {
        let hello = ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode();
        Self {
            cfg,
            factory: Some(factory),
            waiting: None,
            state: ClientState::Joining,
            reject_note: String::new(),
            joined: None,
            baselines: BaselineStore { window: 0, snapshot_every: 1, kept: VecDeque::new() },
            outgoing: vec![hello.clone()],
            voice_allowed: false,
            voice_up: VecDeque::new(),
            voice_up_most: 1,
            voice_in: Vec::new(),
            predicted_events: Vec::new(),
            server_events: Vec::new(),
            remotes: Vec::new(),
            now_ms: 0.0,
            asking: Some(hello),
            asked_at_ms: None,
            mode: None,
            requests: VecDeque::new(),
            next_request: 0,
            lossy: false,
            away: false,
            usual_frame_s: None,
            held_back: false,
            stats: ClientStats::default(),
        }
    }

    /// What the server said it is playing, once it has.
    pub fn mode_name(&self) -> Option<&str> {
        self.mode.as_deref()
    }

    /// Asks the game for something beyond moving a soldier. It will arrive, and in order.
    pub fn send_request(&mut self, bytes: Vec<u8>) {
        self.next_request += 1;
        self.requests.push_back((self.next_request, bytes));
    }

    pub fn state(&self) -> ClientState {
        self.state
    }

    pub fn stats(&self) -> ClientStats {
        self.stats
    }

    /// The game, once the server has said which map.
    /// What the server added when it refused, if it did.
    pub fn reject_note(&self) -> &str {
        &self.reject_note
    }

    pub fn game(&self) -> Option<&G> {
        self.waiting.as_ref().or(self.joined.as_ref().map(|j| &j.predictor.game))
    }

    pub fn game_mut(&mut self) -> Option<&mut G> {
        self.waiting.as_mut().or(self.joined.as_mut().map(|j| &mut j.predictor.game))
    }

    pub fn predictor(&self) -> Option<&Predictor<G>> {
        self.joined.as_ref().map(|j| &j.predictor)
    }

    pub fn clock(&self) -> Option<&ClockSync> {
        self.joined.as_ref().map(|j| &j.clock)
    }

    pub fn interp(&self) -> Option<&Interpolator<G::S>> {
        self.joined.as_ref().map(|j| &j.interp)
    }

    /// Everybody else, as they should be drawn this frame.
    pub fn remotes(&self) -> &[(u32, <G::S as Schema>::Pose)] {
        &self.remotes
    }

    pub fn view_position(&self) -> Option<Vec3> {
        self.joined.as_ref().map(|j| j.predictor.view_position())
    }

    pub fn drain_predicted_events(&mut self) -> Vec<G::Event> {
        std::mem::take(&mut self.predicted_events)
    }

    pub fn drain_server_events(&mut self) -> Vec<<G::S as Schema>::Event> {
        std::mem::take(&mut self.server_events)
    }

    pub fn drain_outgoing(&mut self) -> Vec<Vec<u8>> {
        let frames = std::mem::take(&mut self.outgoing);
        for frame in &frames {
            self.stats.bytes_up += frame.len() as u64;
            self.stats.frames_up += 1;
        }
        if self.asking.is_some() && !frames.is_empty() {
            self.asked_at_ms.get_or_insert(self.now_ms);
        }
        frames
    }

    /// Whether the server has said it carries voice.
    pub fn voice_allowed(&self) -> bool {
        self.voice_allowed
    }

    /// Queues a moment of talking. It will be gone rather than late.
    pub fn send_voice(&mut self, up: VoiceUp) {
        if !self.voice_allowed || self.state != ClientState::Playing {
            return;
        }
        if self.voice_up.len() >= self.voice_up_most.max(1) {
            self.voice_up.pop_front();
        }
        self.voice_up.push_back(ClientMsg::VoiceUp(up).encode());
    }

    /// How many frames of our own talking can wait for the transport. Enough to cover the time between
    /// two of [`crate::transport::pump`] on a slow machine, and not so many that it is heard late.
    pub fn set_voice_buffer(&mut self, frames: usize) {
        self.voice_up_most = frames.max(1);
    }

    /// Tells the server whether to send this client voice, and whose never to.
    pub fn set_voice(&mut self, listening: bool, muted: Vec<u16>) {
        self.outgoing.push(ClientMsg::VoiceSet { listening, muted }.encode());
    }

    /// Voice for a transport to send, after everything from [`Self::drain_outgoing`], if it has room.
    /// Whether the transport is still busy with what it was last given.
    pub fn set_held_back(&mut self, held_back: bool) {
        self.held_back = held_back;
    }

    /// Whether what is sent can be lost, which is only known once a transport is open.
    pub fn set_lossy(&mut self, lossy: bool) {
        self.lossy = lossy;
    }

    pub fn drain_voice_up(&mut self) -> Vec<Vec<u8>> {
        self.voice_up.drain(..).collect()
    }

    /// Voice that has come in since last asked.
    pub fn drain_voice_in(&mut self) -> Vec<VoiceFrame> {
        std::mem::take(&mut self.voice_in)
    }

    pub fn receive(&mut self, frame: &[u8]) {
        self.stats.bytes_down += frame.len() as u64;
        self.stats.frames_down += 1;
        let lookup = Lookup::<G> { store: &self.baselines, interp: self.joined.as_ref().map(|j| &j.interp) };
        match ServerMsg::<G::S>::decode(frame, &lookup) {
            Ok(ServerMsg::Info { map, mode, voice }) => {
                self.voice_allowed = voice;
                self.on_info(&map, mode);
            }
            Ok(ServerMsg::Voice(frame)) => {
                if self.voice_in.len() < VOICE_IN_FRAMES {
                    self.voice_in.push(frame);
                }
            }
            Ok(ServerMsg::Welcome(welcome)) => self.on_welcome(&welcome),
            Ok(ServerMsg::Reject(reason, note)) => {
                self.state = ClientState::Rejected(reason);
                self.reject_note = note;
            }
            Ok(ServerMsg::Snapshot(snapshot)) => self.on_snapshot(snapshot),
            // Only the newest snapshot matters, and the next one will be against something we have.
            Err(WireError::MissingBaseline) => self.stats.missing_baselines += 1,
            Err(_) => self.stats.bad_frames += 1,
        }
    }

    fn on_info(&mut self, map: &str, mode: String) {
        let Some(factory) = self.factory.take() else { return };
        let game = factory(map, &mode);
        self.mode = Some(mode);
        let Some(game) = game else {
            self.state = ClientState::Rejected(RejectReason::UnknownMap);
            return;
        };
        let join = ClientMsg::Join { rules: game.rules_hash(), name: self.cfg.name.clone() }.encode();
        self.outgoing.push(join.clone());
        self.asking = Some(join);
        self.asked_at_ms = None;
        self.waiting = Some(game);
    }

    fn on_welcome(&mut self, welcome: &Welcome<G::S>) {
        let Some(mut game) = self.waiting.take() else { return };
        game.join(welcome);
        let rtt = (self.now_ms - self.asked_at_ms.unwrap_or(self.now_ms)) as f32;
        self.asking = None;
        let clock = ClockSync::new(self.cfg.clock.clone(), welcome.tick, rtt);
        let predictor = Predictor::new(game, clock.tick(), self.cfg.predict.clone());
        self.baselines.window = welcome.baseline_window;
        self.baselines.snapshot_every = welcome.snapshot_every;
        self.joined = Some(Joined {
            clock,
            predictor,
            interp: Interpolator::new(self.cfg.interp.clone(), welcome.snapshot_every),
            recent: VecDeque::new(),
            fresh: 0,
            last_event_tick: welcome.tick,
            last_snapshot: 0,
            me: welcome.soldier,
            pending_events: VecDeque::new(),
        });
        self.state = ClientState::Playing;
    }

    fn on_snapshot(&mut self, snapshot: Snapshot<G::S>) {
        let Some(joined) = &mut self.joined else { return };
        if snapshot.tick <= joined.last_snapshot {
            self.stats.stale_snapshots += 1;
            return;
        }
        joined.last_snapshot = snapshot.tick;
        self.stats.snapshots += 1;
        while self.requests.front().is_some_and(|(seq, _)| *seq <= snapshot.request_ack) {
            self.requests.pop_front();
        }

        let now_ms = self.now_ms as u32 as u16;
        joined.clock.on_report(snapshot.cmd_slack, snapshot.echo.map(|(sent_ms, _)| sent_ms), now_ms);
        if let Some((sent_ms, held)) = snapshot.echo {
            let elapsed = (self.now_ms as u32 as u16).wrapping_sub(sent_ms) as f32;
            joined.clock.on_rtt((elapsed - held as f32 * TICK_DT * 1000.0).max(0.0));
        }
        if let Some(own) = &snapshot.own {
            joined.predictor.reconcile(snapshot.tick, own);
        }
        for (tick, event) in snapshot.events {
            if tick <= joined.last_event_tick {
                continue;
            }
            if <G::S as Schema>::urgent(&event, joined.me) {
                if joined.predictor.game.on_server_event(&event) {
                    joined.predictor.clear_visual();
                }
                self.server_events.push(event);
            } else {
                joined.pending_events.push_back((tick, event));
            }
        }
        joined.last_event_tick = joined.last_event_tick.max(snapshot.tick);
        let shared_changed = self.baselines.kept.back().is_none_or(|k| k.shared != snapshot.shared);
        if shared_changed {
            joined.predictor.game.apply_shared(&snapshot.shared);
        }
        joined.interp.push(snapshot.tick, &snapshot.entities, &snapshot.gone);
        self.baselines.push(Kept {
            tick: snapshot.tick,
            own: snapshot.own,
            shared: snapshot.shared,
        });
    }

    /// Advances `dt` seconds. `input` gives the command for a tick; its `tick` field is filled in here.
    /// Says how much real time the frame about to run covers, before its updates.
    ///
    /// After a long absence a host runs a burst of updates to catch up, all in one instant: Bevy runs two
    /// seconds of them. Those would put the clock that far ahead of real time, and every command they
    /// stamp from the future. So for that frame the updates do nothing, and the server says how late the
    /// clock is.
    pub fn frame_started(&mut self, real_dt: f32) {
        let usual = self.usual_frame_s.unwrap_or(real_dt.min(self.cfg.away_after_s));
        self.away = real_dt > self.cfg.away_after_s && real_dt > usual * self.cfg.away_after_usual;
        // Absences count too, so that a machine whose every frame is that long stops looking away after a few.
        self.usual_frame_s = Some(usual + (real_dt - usual) * self.cfg.usual_frame_smoothing);
    }

    pub fn update(&mut self, dt: f32, mut input: impl FnMut(u32) -> PlayerCmd) {
        if self.away {
            return;
        }
        self.now_ms += dt as f64 * 1000.0;
        if let (Some(asking), Some(at), ClientState::Joining) = (&self.asking, self.asked_at_ms, self.state)
            && self.now_ms - at >= self.cfg.handshake_resend_ms
        {
            self.outgoing.push(asking.clone());
            self.asked_at_ms = None;
        }
        let Some(joined) = &mut self.joined else { return };

        joined.clock.advance(dt);
        let goal = joined.clock.tick();
        if goal < joined.predictor.tick() {
            joined.predictor.restart_at(goal);
            joined.recent.clear();
            joined.fresh = 0;
        }
        let behind = goal - joined.predictor.tick();
        if behind > 0 {
            let run = behind.min(self.cfg.max_catchup_ticks);
            let (before, attitude_before) = (joined.predictor.game.position(), joined.predictor.game.attitude());
            for tick in goal + 1 - run..=goal {
                let cmd = input(tick);
                let cmd = PlayerCmd { tick, wish_move: cmd.wish_move.clamp_length_max(self.cfg.max_wish), ..cmd };
                joined.predictor.step(cmd, &mut self.predicted_events);
                joined.recent.push_back(cmd);
                joined.fresh += 1;
            }
            if run > self.cfg.smooth_catchup_above {
                joined.predictor.absorb_jump(before, attitude_before);
            }
            let copies = if self.lossy { self.cfg.cmd_redundancy.max(1) } else { 1 };
            let keep = joined.fresh.max(copies);
            while joined.recent.len() > keep {
                joined.recent.pop_front();
            }
        }
        if joined.fresh > 0 && !self.held_back {
            let packet = CmdPacket {
                ack_snapshot: self.baselines.newest(),
                ack_bits: self.baselines.ack_bits(),
                sent_ms: self.now_ms as u32 as u16,
                view_tick: joined.interp.play_tick().max(0.0) as u32,
                requests: self.requests.iter().take(self.cfg.requests_per_packet).cloned().collect(),
                cmds: joined.recent.iter().copied().collect(),
            };
            joined.fresh = 0;
            self.outgoing.push(ClientMsg::Cmds(packet).encode());
        }

        joined.predictor.decay_visual(dt);
    }

    /// Moves everyone else on to this frame. Apart from [`Self::update`] so a host can run this at
    /// its frame rate and that at its tick rate.
    pub fn draw(&mut self, dt: f32) {
        self.away = false;
        let Some(joined) = &mut self.joined else { return };
        joined.interp.advance(dt);
        self.remotes = joined.interp.sample();
        joined.predictor.game.remotes_from(joined.interp.play_tick());
        joined.predictor.game.show_remotes(&self.remotes, dt);
        let play = joined.interp.play_tick();
        while joined.pending_events.front().is_some_and(|(tick, _)| *tick as f64 <= play) {
            let (_, event) = joined.pending_events.pop_front().expect("just looked");
            if joined.predictor.game.on_server_event(&event) {
                joined.predictor.clear_visual();
            }
            self.server_events.push(event);
        }
    }
}
