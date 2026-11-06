//! The server's side of the game, with no sockets: bytes in, bytes out, one call per tick.
//!
//! The only copy of the game that counts lives here. Clients send commands and nothing else, so
//! position, aim and hits are all decided here, and a command only ever becomes input to the game.

use prs_protocol::{PlayerCmd, TICK_HZ};
use std::collections::{BTreeMap, VecDeque};

use crate::entity::{MAX_FIELDS, NetEntity};
use crate::game::{Relevance, Schema, ServerGame, Viewer};
use crate::names::{NameRules, clean_name};
use crate::validate::{CmdLimits, Rejected, check, clamp_turn};
use crate::wire::{
    BaseRef, ClientMsg, CmdPacket, EntityOut, PROTOCOL_VERSION, RejectReason, Snapshot, VoiceChannel, VoiceFrame, VoiceUp, Welcome, encode_entity,
    encode_info, encode_reject, encode_voice, encode_welcome, finish_snapshot, write_snapshot_start,
};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub max_players: usize,
    /// Ticks between snapshots. 2 at 60 Hz is 30 snapshots a second.
    pub snapshot_every: u32,
    /// How far back a client's acknowledged snapshot can still be used as a delta baseline, in
    /// ticks. Past this it gets a full snapshot.
    pub baseline_window: u32,
    /// How long to keep acting on a client's last command with nothing new, in ticks. After this
    /// the player stands still rather than running on or firing on its own.
    pub max_repeat_ticks: u32,
    /// First player's seed. Later ones count up from it.
    pub first_seed: u32,
    /// Furthest back a shot can be judged, in ticks, however far back its shooter says they are
    /// seeing. Bigger is fairer to high pings and worse for whoever is being shot around a corner.
    /// 0 turns lag compensation off.
    pub max_rewind_ticks: u32,
    pub interest: InterestConfig,
    /// The most a datagram may carry, with room for what QUIC adds. A snapshot for a client on a transport
    /// that can lose things is kept under this, so that it goes as one.
    pub datagram_bytes: usize,
    pub limits: CmdLimits,
    pub voice: VoiceConfig,
    pub names: NameRules,
    /// Told to a client refused for playing by other rules, such as which mod this runs.
    pub rules_note: String,
}

/// How a client's share of the network is spent when not everything fits.
#[derive(Clone, Debug)]
pub struct InterestConfig {
    /// Most bytes of entities to put in one snapshot, as long as anything close is sent regardless.
    pub snapshot_budget: usize,
    /// Anything nearer than this goes in every snapshot it has changed for, budget or not, in metres.
    pub always_within: f32,
    /// Out to here something goes in every snapshot it fits in. Further off it is sent in proportion
    /// to how far away it is, in metres.
    pub full_rate_within: f32,
    /// However far off, it is sent at least this often, in snapshots.
    pub slowest_every: f32,
    /// Something within this cone of where the player is looking counts as full rate wherever it is.
    /// The cosine of the angle.
    pub aim_cone: f32,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self { snapshot_budget: 1200, always_within: 40.0, full_rate_within: 120.0, slowest_every: 10.0, aim_cone: 0.97 }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_players: 64,
            snapshot_every: 2,
            baseline_window: 120,
            max_repeat_ticks: 30,
            first_seed: 0x5EED,
            max_rewind_ticks: 36,
            interest: InterestConfig::default(),
            datagram_bytes: 1100,
            limits: CmdLimits::default(),
            voice: VoiceConfig::default(),
            names: NameRules::default(),
            rules_note: String::new(),
        }
    }
}

/// What a server lets through of people talking. Nothing here is ever kept or written down.
#[derive(Clone, Debug)]
pub struct VoiceConfig {
    /// Off, and voice is thrown away on arrival and clients are told not to bother.
    pub enabled: bool,
    /// The biggest frame taken from a talker.
    pub max_frame_bytes: usize,
    /// What one talker may send in a second. Anything over is thrown away, until the next second.
    pub talker_frames_per_second: u32,
    pub talker_bytes_per_second: u32,
    /// What one listener may be sent in a second, however many are talking.
    pub listener_bytes_per_second: u32,
    /// Bytes waiting to go to one listener. It is the oldest that goes when there are more.
    pub queue_bytes: usize,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_frame_bytes: 512,
            // Room for the shortest frames a codec is set to here, and for a burst after a stall.
            talker_frames_per_second: 60,
            talker_bytes_per_second: 8000,
            // A dozen people talking over each other at the usual rate is already no use to anyone.
            listener_bytes_per_second: 30000,
            queue_bytes: 4096,
        }
    }
}

impl ClientVoice {
    fn keep_only(&mut self, keep: impl Fn(u16) -> bool) {
        self.queue.retain(|(speaker, _)| keep(*speaker));
        self.queued_bytes = self.queue.iter().map(|(_, frame)| frame.len()).sum();
    }
}

/// Starts a new second of voice for this client once the last one is over.
fn roll_voice_window(voice: &mut ClientVoice, tick: u32) {
    if tick.wrapping_sub(voice.window_start) >= TICK_HZ as u32 {
        voice.window_start = tick;
        voice.frames_up = 0;
        voice.bytes_up = 0;
        voice.bytes_down = 0;
    }
}

/// Which connection is which, as issued by [`GameServer::connect`].
pub type ClientId = u32;

#[derive(Clone, Copy, Debug, Default)]
pub struct ClientCounters {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub snapshots_sent: u64,
    pub full_snapshots_sent: u64,
    /// Player ticks run on a real command.
    pub cmds_used: u64,
    /// Player ticks where the command had not come, so the last one ran again.
    pub cmds_repeated: u64,
    /// Player ticks after repeating ran out.
    pub cmds_idled: u64,
    pub cmds_late: u64,
    pub cmds_future: u64,
    pub cmds_not_finite: u64,
    pub turns_clamped: u64,
    pub packets_over_rate: u64,
    pub bad_frames: u64,
    /// Voice taken from this client, and thrown away for being too big or too much.
    pub voice_taken: u64,
    pub voice_refused: u64,
    /// Voice sent to this client, and thrown away because it could not keep up.
    pub voice_sent: u64,
    pub voice_dropped: u64,
}

/// What a client has been sent of one entity.
struct Known {
    /// Oldest first: tick and fields of everything sent and not yet too old to delta against.
    sent: VecDeque<(u32, [i32; MAX_FIELDS])>,
    /// Built up each snapshot until it is high enough to go.
    priority: f32,
    /// When it was last told this is gone, while waiting to hear that it knows.
    gone_at: Option<u32>,
    /// The newest of what was sent that it is known to have, kept for as long as it is in view. Without
    /// it, something that never changes was sent whole again every time the window moved past it.
    held: Option<(u32, [i32; MAX_FIELDS])>,
}

struct Client<S: Schema> {
    id: ClientId,
    player: Option<u16>,
    /// Its player stays unstepped until the first command, so a client begins exactly where the server does.
    started: bool,
    /// Whether the transport guarantees delivery, so deltas need not wait for acknowledgement.
    reliable: bool,
    queue: BTreeMap<u32, PlayerCmd>,
    last_cmd: PlayerCmd,
    repeated: u32,
    newest_seen: u32,
    ack_snapshot: u32,
    ack_bits: u32,
    known: BTreeMap<u32, Known>,
    last_sent: u32,
    /// The earliest any command arrived since the last snapshot, in ticks to spare.
    slack: Option<i32>,
    echo: Option<(u16, u32)>,
    /// Packets accepted in the second that starts at `window_start`, and refusals in that second.
    window_start: u32,
    window_packets: u32,
    window_rejects: u32,
    /// The network can't keep up, so skip snapshots until it does. Deltas make that safe.
    backlogged: bool,
    kicked: bool,
    welcome: Option<Vec<u8>>,
    /// The newest request taken in. They are taken strictly in order.
    request_seen: u32,
    /// The tick being drawn, according to the client.
    view_tick: u32,
    /// What this client was sent of its own player, to delta against.
    own_history: VecDeque<(u32, S::Own)>,
    voice: ClientVoice,
    /// What the server has agreed to call this player, once it has one.
    name: String,
    counters: ClientCounters,
}

/// A client's part in voice. Off until it asks, so it costs nobody who isn't using it anything.
#[derive(Default)]
struct ClientVoice {
    listening: bool,
    muted: Vec<u16>,
    /// Who said it and the frame, ready to go after everything the game had to say.
    queue: VecDeque<(u16, Vec<u8>)>,
    queued_bytes: usize,
    /// When this second began, and what it has seen so far of this client talking and being talked to.
    window_start: u32,
    frames_up: u32,
    bytes_up: u32,
    bytes_down: u32,
}

/// One tick's worth of the world, shared across clients. Each sees all of it but themselves.
struct WorldRecord {
    tick: u32,
    shared: Vec<u8>,
}

pub struct GameServer<G: ServerGame> {
    pub game: G,
    cfg: ServerConfig,
    rules: u64,
    clients: Vec<Client<G::S>>,
    next_client: ClientId,
    next_seed: u32,
    /// Players in the game, sorted.
    players: Vec<u16>,
    history: VecDeque<WorldRecord>,
    events: VecDeque<(u32, <G::S as Schema>::Event)>,
    outgoing: Vec<(ClientId, Vec<u8>)>,
    cmds: Vec<(u16, PlayerCmd)>,
    tick_events: Vec<<G::S as Schema>::Event>,
    to_kick: Vec<ClientId>,
}

impl<G: ServerGame> GameServer<G> {
    pub fn new(game: G, cfg: ServerConfig) -> Self {
        Self {
            rules: game.rules_hash(),
            next_seed: cfg.first_seed,
            game,
            cfg,
            clients: Vec::new(),
            next_client: 1,
            players: Vec::new(),
            history: VecDeque::new(),
            events: VecDeque::new(),
            outgoing: Vec::new(),
            cmds: Vec::new(),
            tick_events: Vec::new(),
            to_kick: Vec::new(),
        }
    }

    pub fn config(&self) -> &ServerConfig {
        &self.cfg
    }

    /// A transport has a new connection. It gets a player once it has said hello.
    pub fn connect(&mut self, reliable: bool) -> ClientId {
        let id = self.next_client;
        self.next_client += 1;
        self.clients.push(Client {
            id,
            player: None,
            started: false,
            reliable,
            queue: BTreeMap::new(),
            last_cmd: PlayerCmd::default(),
            repeated: 0,
            newest_seen: 0,
            ack_snapshot: 0,
            ack_bits: 0,
            known: BTreeMap::new(),
            last_sent: 0,
            slack: None,
            echo: None,
            window_start: self.game.tick(),
            window_packets: 0,
            window_rejects: 0,
            backlogged: false,
            kicked: false,
            welcome: None,
            request_seen: 0,
            view_tick: 0,
            own_history: VecDeque::new(),
            voice: ClientVoice::default(),
            name: String::new(),
            counters: ClientCounters::default(),
        });
        id
    }

    pub fn disconnect(&mut self, id: ClientId) {
        let Some(at) = self.clients.iter().position(|c| c.id == id) else { return };
        let client = self.clients.swap_remove(at);
        if let Some(player) = client.player {
            self.players.retain(|&p| p != player);
            self.game.remove(player);
            // That number can go to somebody new, who has done nothing to be muted for.
            for other in &mut self.clients {
                other.voice.muted.retain(|&p| p != player);
                other.voice.keep_only(|speaker| speaker != player);
            }
        }
    }

    /// The host says its socket to this client is keeping up, or is not.
    pub fn set_backlogged(&mut self, id: ClientId, backlogged: bool) {
        if let Some(client) = self.clients.iter_mut().find(|c| c.id == id) {
            client.backlogged = backlogged;
            if backlogged {
                client.counters.voice_dropped += client.voice.queue.len() as u64;
                client.voice.keep_only(|_| false);
            }
        }
    }

    pub fn counters(&self, id: ClientId) -> Option<ClientCounters> {
        self.clients.iter().find(|c| c.id == id).map(|c| c.counters)
    }

    pub fn player_of(&self, id: ClientId) -> Option<u16> {
        self.clients.iter().find(|c| c.id == id).and_then(|c| c.player)
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Clients that should be dropped: they have earned it, or have been turned away.
    pub fn drain_kicked(&mut self) -> Vec<ClientId> {
        std::mem::take(&mut self.to_kick)
    }

    pub fn drain_outgoing(&mut self) -> Vec<(ClientId, Vec<u8>)> {
        let mut outgoing = std::mem::take(&mut self.outgoing);
        // Last, so that the game's state is in the socket's way ahead of anybody's voice.
        for client in &mut self.clients {
            client.voice.queued_bytes = 0;
            for (_, frame) in client.voice.queue.drain(..) {
                client.counters.voice_sent += 1;
                client.counters.bytes_out += frame.len() as u64;
                outgoing.push((client.id, frame));
            }
        }
        outgoing
    }

    /// Takes one frame from a client. A frame that makes no sense is counted and ignored.
    pub fn receive(&mut self, id: ClientId, frame: &[u8]) {
        let Some(at) = self.clients.iter().position(|c| c.id == id) else { return };
        self.clients[at].counters.bytes_in += frame.len() as u64;
        match ClientMsg::decode(frame) {
            Ok(ClientMsg::Hello { protocol }) => self.on_hello(at, protocol),
            Ok(ClientMsg::Join { rules, name }) => self.on_join(at, rules, &name),
            Ok(ClientMsg::Cmds(packet)) => self.on_cmds(at, &packet),
            Ok(ClientMsg::VoiceUp(up)) => self.on_voice(at, up),
            Ok(ClientMsg::VoiceSet { listening, muted }) => self.on_voice_set(at, listening, muted),
            Err(_) => self.clients[at].counters.bad_frames += 1,
        }
    }

    fn on_hello(&mut self, at: usize, protocol: u16) {
        let id = self.clients[at].id;
        if protocol != PROTOCOL_VERSION {
            self.refuse(id, RejectReason::Protocol);
        } else {
            let (map_hash, map_bytes) = self.game.map_file();
            self.outgoing.push((id, encode_info(&self.game.map_name(), &self.game.mode_name(), self.cfg.voice.enabled, map_hash, map_bytes)));
        }
    }

    /// Takes effect at once: anything of theirs still waiting to go is gone now as well.
    fn on_voice_set(&mut self, at: usize, listening: bool, muted: Vec<u16>) {
        let voice = &mut self.clients[at].voice;
        voice.listening = listening && self.cfg.voice.enabled;
        voice.muted = muted;
        let (listening, muted) = (voice.listening, voice.muted.clone());
        voice.keep_only(|speaker| listening && !muted.contains(&speaker));
    }

    /// Sends a moment of someone talking to whoever the game says can hear it. The client says only
    /// which way it is talking, and nothing it says can add a listener.
    fn on_voice(&mut self, at: usize, up: VoiceUp) {
        let cfg = &self.cfg.voice;
        if !cfg.enabled {
            return;
        }
        let tick = self.game.tick();
        let talker = &mut self.clients[at];
        let Some(speaker) = talker.player.filter(|_| talker.started) else { return };
        talker.counters.voice_taken += 1;
        roll_voice_window(&mut talker.voice, tick);
        let len = up.bytes.len() as u32;
        talker.voice.frames_up += 1;
        talker.voice.bytes_up += len;
        let too_much = talker.voice.frames_up > cfg.talker_frames_per_second || talker.voice.bytes_up > cfg.talker_bytes_per_second;
        if up.bytes.is_empty() || up.bytes.len() > cfg.max_frame_bytes || too_much {
            talker.counters.voice_refused += 1;
            return;
        }
        let Some(from) = self.game.voice_from(speaker, up.channel) else { return };
        let at_place = (up.channel == VoiceChannel::Local).then(|| VoiceFrame::place(from));
        let frame = encode_voice(&VoiceFrame { speaker, channel: up.channel, seq: up.seq, at: at_place, bytes: up.bytes });
        for listener in &mut self.clients {
            let Some(player) = listener.player else { continue };
            if player == speaker || !listener.voice.listening || listener.voice.muted.contains(&speaker) {
                continue;
            }
            if !self.game.voice_reaches(speaker, player, up.channel) {
                continue;
            }
            roll_voice_window(&mut listener.voice, tick);
            listener.voice.bytes_down += frame.len() as u32;
            if listener.backlogged || listener.voice.bytes_down > cfg.listener_bytes_per_second {
                listener.counters.voice_dropped += 1;
                continue;
            }
            listener.voice.queued_bytes += frame.len();
            listener.voice.queue.push_back((speaker, frame.clone()));
            while listener.voice.queued_bytes > cfg.queue_bytes {
                let Some((_, oldest)) = listener.voice.queue.pop_front() else { break };
                listener.voice.queued_bytes -= oldest.len();
                listener.counters.voice_dropped += 1;
            }
        }
    }

    fn refuse(&mut self, id: ClientId, reason: RejectReason) {
        let note = if reason == RejectReason::Rules { self.cfg.rules_note.as_str() } else { "" };
        self.outgoing.push((id, encode_reject(reason, note)));
        self.to_kick.push(id);
    }

    fn on_join(&mut self, at: usize, rules: u64, asked_name: &str) {
        let id = self.clients[at].id;
        // Asking again means the answer got lost.
        if let Some(welcome) = &self.clients[at].welcome {
            self.outgoing.push((id, welcome.clone()));
            return;
        }
        let taken = self.clients.iter().filter(|c| c.player.is_some()).count();
        if rules != self.rules {
            return self.refuse(id, RejectReason::Rules);
        }
        if taken >= self.cfg.max_players {
            return self.refuse(id, RejectReason::Full);
        }

        let seed = self.next_seed;
        self.next_seed = self.next_seed.wrapping_add(1);
        let player = self.game.spawn(seed);
        let taken: Vec<&str> = self.clients.iter().filter(|c| c.player.is_some()).map(|c| c.name.as_str()).collect();
        let name = clean_name(asked_name, player, &taken, &self.cfg.names);
        self.game.set_name(player, &name);
        self.clients[at].name = name;
        let welcome = Welcome::<G::S> {
            soldier: player,
            tick: self.game.tick(),
            own: self.game.own(player),
            snapshot_every: self.cfg.snapshot_every,
            baseline_window: self.cfg.baseline_window,
        };
        let welcome = encode_welcome(&welcome);
        self.clients[at].player = Some(player);
        self.clients[at].welcome = Some(welcome.clone());
        self.outgoing.push((id, welcome));
    }

    fn on_cmds(&mut self, at: usize, packet: &CmdPacket) {
        let next_tick = self.game.tick() + 1;
        let limits = self.cfg.limits.clone();
        let client = &mut self.clients[at];
        if client.player.is_none() {
            return;
        }
        if next_tick.wrapping_sub(client.window_start) >= TICK_HZ as u32 {
            client.window_start = next_tick;
            client.window_packets = 0;
            client.window_rejects = 0;
        }
        client.window_packets += 1;
        if client.window_packets > limits.max_packets_per_second {
            client.counters.packets_over_rate += 1;
            return;
        }
        let now = self.game.tick();
        let player = client.player;
        for (seq, bytes) in &packet.requests {
            if *seq == client.request_seen + 1 {
                client.request_seen = *seq;
                if let Some(player) = player {
                    self.game.request(player, bytes);
                }
            }
        }
        if packet.ack_snapshot <= now {
            if packet.ack_snapshot > client.ack_snapshot {
                client.ack_snapshot = packet.ack_snapshot;
                client.ack_bits = packet.ack_bits;
            }
        }
        client.echo = Some((packet.sent_ms, now));
        client.view_tick = packet.view_tick;

        let mut newest = None;
        for cmd in &packet.cmds {
            match check(cmd, next_tick, &limits) {
                Ok(cmd) => {
                    client.queue.entry(cmd.tick).or_insert(cmd);
                    newest = newest.max(Some(cmd.tick));
                }
                Err(Rejected::Late) => {
                    // A resend of something already used is only redundancy doing its job.
                    if cmd.tick > client.newest_seen {
                        client.counters.cmds_late += 1;
                    }
                    newest = newest.max(Some(cmd.tick));
                }
                Err(reason) => {
                    client.window_rejects += 1;
                    match reason {
                        Rejected::FromTheFuture => {
                            client.counters.cmds_future += 1;
                            // An honest client's clock can be this wrong, and this is how it finds out.
                            newest = newest.max(Some(cmd.tick));
                        }
                        _ => client.counters.cmds_not_finite += 1,
                    }
                }
            }
        }
        if let Some(tick) = newest {
            client.newest_seen = client.newest_seen.max(tick);
            let spare = tick as i64 - next_tick as i64;
            let spare = spare.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            client.slack = Some(client.slack.map_or(spare, |s| s.min(spare)));
        }
        let kick_at = limits.kick_after_rejects_per_second;
        if kick_at > 0 && client.window_rejects >= kick_at && !client.kicked {
            client.kicked = true;
            let id = client.id;
            self.outgoing.push((id, encode_reject(RejectReason::Kicked, "")));
            self.to_kick.push(id);
        }
    }

    /// Runs one tick and queues whatever clients need to hear.
    pub fn tick(&mut self) {
        let next_tick = self.game.tick() + 1;
        self.cmds.clear();
        for client in &mut self.clients {
            let Some(player) = client.player else { continue };
            let started = client.started;
            if let Some(cmd) = next_cmd(client, next_tick, &self.cfg) {
                if !started {
                    self.game.start(player);
                    self.players.push(player);
                    self.players.sort_unstable();
                }
                // Claimed by the client, which is why it has a limit and only ever helps its aim.
                let lag = cmd.tick.saturating_sub(client.view_tick).min(self.cfg.max_rewind_ticks);
                let lag = if client.view_tick == 0 { 0 } else { lag };
                self.game.set_view_lag(player, lag);
                let (yaw, pitch) = self.game.look(player);
                let clamped = clamp_turn(&cmd, yaw, pitch, &self.cfg.limits);
                if clamped != cmd {
                    client.counters.turns_clamped += 1;
                }
                self.cmds.push((player, clamped));
            }
        }
        self.tick_events.clear();
        self.game.step(&self.cmds, &mut self.tick_events);

        let tick = self.game.tick();
        for event in self.tick_events.drain(..) {
            self.events.push_back((tick, event));
        }
        let oldest = tick.saturating_sub(self.cfg.baseline_window);
        while self.events.front().is_some_and(|(t, _)| *t < oldest) {
            self.events.pop_front();
        }
        if tick % self.cfg.snapshot_every.max(1) == 0 {
            self.send_snapshots();
        }
    }

    fn send_snapshots(&mut self) {
        let tick = self.game.tick();
        let world = self.game.capture_world();
        self.history.push_back(WorldRecord { tick, shared: self.game.shared() });
        while self.history.front().is_some_and(|r| r.tick + self.cfg.baseline_window < tick) {
            self.history.pop_front();
        }
        let record = self.history.back().expect("just pushed");
        let every = self.cfg.snapshot_every.max(1);

        for client in &mut self.clients {
            let Some(player) = client.player else { continue };
            if client.backlogged {
                continue;
            }
            // On a reliable transport what was last sent will arrive, so it can be used right away.
            let known = if client.reliable { client.last_sent } else { client.ack_snapshot };
            let viewer = self.game.viewer(player);
            let own = client.started.then(|| self.game.own(player));

            let mut snapshot = Snapshot::<G::S> {
                tick,
                cmd_slack: client.slack.take(),
                echo: client.echo.take().map(|(ms, at)| (ms, (tick - at).min(255) as u8)),
                request_ack: client.request_seen,
                own,
                entities: Vec::new(),
                gone: Vec::new(),
                shared: record.shared.clone(),
                events: self
                    .events
                    .iter()
                    .filter(|(t, e)| *t > known && !G::predicted_by(e, player))
                    .filter(|(_, e)| {
                        self.game.event_reach(e).is_none_or(|(at, reach)| at.distance_squared(viewer.position) <= reach * reach)
                    })
                    .cloned()
                    .collect(),
            };
            if base_ref(&self.history, client, known).is_none() {
                client.counters.full_snapshots_sent += 1;
            }
            // Over a transport that can lose things a snapshot goes as one datagram, so it has to fit in one,
            // with room for what is forgotten. One that can't fit still goes, the slow way.
            let budget = if client.reliable {
                Budget { bytes: self.cfg.interest.snapshot_budget, hard: false }
            } else {
                let rest = write_snapshot_start(&snapshot, known, base_ref(&self.history, client, known).as_ref()).len() + GONE_ROOM;
                let room = self.cfg.datagram_bytes.saturating_sub(rest);
                Budget { bytes: room.min(self.cfg.interest.snapshot_budget), hard: true }
            };
            let (out, gone) = choose_entities::<G::S>(client, &world, player, &viewer, tick, every, &self.cfg, budget);
            snapshot.gone = gone;
            let base = base_ref(&self.history, client, known);
            let start = write_snapshot_start(&snapshot, known, base.as_ref());
            let bytes = finish_snapshot(start, &out);
            if let Some(own) = snapshot.own {
                client.own_history.push_back((tick, own));
            }
            while client.own_history.front().is_some_and(|(t, _)| t + self.cfg.baseline_window < tick) {
                client.own_history.pop_front();
            }
            client.counters.bytes_out += bytes.len() as u64;
            client.counters.snapshots_sent += 1;
            client.last_sent = tick;
            self.outgoing.push((client.id, bytes));
        }
    }
}

/// Whether the client is known to have the snapshot of this tick.
fn has_snapshot(reliable: bool, ack: u32, ack_bits: u32, tick: u32, every: u32) -> bool {
    if reliable || tick == ack {
        return true;
    }
    let back = ack.saturating_sub(tick) / every;
    tick < ack && (1..=32).contains(&back) && ack_bits & (1 << (back - 1)) != 0
}

/// Spends this client's budget on what matters most to it, and works out what it should forget.
/// What a client is known to have from the snapshot of `known`, if it is still kept.
fn base_ref<'a, S: Schema>(history: &'a VecDeque<WorldRecord>, client: &'a Client<S>, known: u32) -> Option<BaseRef<'a, S>> {
    history.iter().find(|r| r.tick == known).map(|record| BaseRef {
        own: client.own_history.iter().find(|(t, _)| *t == known).map(|(_, own)| own),
        shared: &record.shared,
    })
}

/// Bytes of entities a snapshot may hold. A hard one is never gone over, even for what is close by.
#[derive(Clone, Copy)]
struct Budget {
    bytes: usize,
    hard: bool,
}

/// Kept free in a datagram for the list of what to forget, which is chosen with the entities.
const GONE_ROOM: usize = 32;

#[allow(clippy::too_many_arguments)]
fn choose_entities<S: Schema>(
    client: &mut Client<S>,
    world: &[(NetEntity, Relevance)],
    player: u16,
    viewer: &Viewer,
    tick: u32,
    every: u32,
    cfg: &ServerConfig,
    budget: Budget,
) -> (Vec<Vec<u8>>, Vec<u32>) {
    let interest = &cfg.interest;
    let oldest = tick.saturating_sub(cfg.baseline_window);
    let mut candidates: Vec<(f32, f32, usize, Option<u32>)> = Vec::new();
    for (at, (entity, relevance)) in world.iter().enumerate() {
        if entity.key == player as u32 {
            continue;
        }
        let to = relevance.position - viewer.position;
        let distance = to.length();
        if distance > relevance.max_range {
            continue;
        }
        let known = client.known.entry(entity.key).or_insert_with(|| Known { sent: VecDeque::new(), priority: 0.0, gone_at: None, held: None });
        known.gone_at = None;
        let (reliable, ack, ack_bits) = (client.reliable, client.ack_snapshot, client.ack_bits);
        // A client that has said nothing about the window may have lost track, as when a hidden tab reads
        // a long backlog at once. Then nothing is taken as held, and it gets everything whole again.
        if !reliable && ack < oldest {
            known.held = None;
        }
        if let Some(&arrived) = known.sent.iter().rev().find(|(t, _)| has_snapshot(reliable, ack, ack_bits, *t, every))
            && known.held.is_none_or(|(held, _)| arrived.0 > held)
        {
            known.held = Some(arrived);
        }
        while known.sent.front().is_some_and(|(t, _)| *t < oldest) {
            known.sent.pop_front();
        }
        // Against the newest it is known to have. Nothing to say if that is what it is now.
        let base = known.held;
        let base_tick = base.map(|(t, _)| t);
        let newest_sent_same = known.sent.back().is_some_and(|(_, fields)| *fields == entity.fields);
        if base.is_some_and(|(_, fields)| fields == entity.fields) || (client.reliable && newest_sent_same) {
            known.priority = 0.0;
            continue;
        }
        // In the sights or close by it goes every time. Further off it waits its turn.
        let aimed = distance > 0.0 && to.dot(viewer.forward) / distance >= interest.aim_cone;
        let every_n = if aimed {
            1.0
        } else {
            (distance / interest.full_rate_within.max(1.0)).clamp(1.0, interest.slowest_every.max(1.0))
        };
        known.priority += relevance.importance / every_n;
        if known.priority >= 1.0 {
            candidates.push((known.priority, distance, at, base_tick));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));

    let mut out = Vec::with_capacity(candidates.len());
    let mut spent = 0;
    for (_, distance, at, base_tick) in candidates {
        if !budget.hard && spent >= budget.bytes && distance > interest.always_within {
            continue;
        }
        let entity = &world[at].0;
        let known = client.known.get_mut(&entity.key).expect("just made");
        let base = base_tick.and_then(|t| known.held.as_ref().filter(|(held, _)| *held == t).map(|(_, fields)| (tick - t, fields)));
        let bytes = encode_entity::<S>(&EntityOut { entity, base });
        // Its priority is kept, so it goes first next time.
        if budget.hard && spent + bytes.len() > budget.bytes {
            continue;
        }
        spent += bytes.len();
        out.push(bytes);
        known.sent.push_back((tick, entity.fields));
        known.priority = 0.0;
    }

    // Anything no longer in the world or out of range is forgotten, and said so until it has heard.
    let mut gone = Vec::new();
    let in_range = |key: u32| {
        world.iter().any(|(e, r)| e.key == key && r.position.distance(viewer.position) <= r.max_range)
    };
    let mut forgotten = Vec::new();
    for (&key, known) in &mut client.known {
        if in_range(key) {
            continue;
        }
        let told = *known.gone_at.get_or_insert(tick);
        gone.push(key);
        let heard = client.reliable || (client.ack_snapshot >= told);
        if heard {
            forgotten.push(key);
        }
    }
    for key in forgotten {
        client.known.remove(&key);
    }
    (out, gone)
}

/// What a client's player does on `tick`, or `None` if it has not begun.
fn next_cmd<S: Schema>(client: &mut Client<S>, tick: u32, cfg: &ServerConfig) -> Option<PlayerCmd> {
    while client.queue.first_key_value().is_some_and(|(t, _)| *t < tick) {
        client.queue.pop_first();
    }
    if let Some(cmd) = client.queue.remove(&tick) {
        client.started = true;
        client.repeated = 0;
        client.last_cmd = cmd;
        client.counters.cmds_used += 1;
        return Some(cmd);
    }
    if !client.started {
        return None;
    }
    client.repeated += 1;
    if client.repeated <= cfg.max_repeat_ticks {
        client.counters.cmds_repeated += 1;
        Some(PlayerCmd { tick, ..client.last_cmd })
    } else {
        // Keep looking the same way, with hands off everything.
        client.counters.cmds_idled += 1;
        Some(PlayerCmd { tick, yaw: client.last_cmd.yaw, pitch: client.last_cmd.pitch, ..Default::default() })
    }
}
