//! The simulation as a server runs it: the only copy that counts.

use glam::Vec3;
use prs_net::entity::NetEntity;
use prs_net::game::{Relevance, ServerGame, Viewer};
use prs_net::wire::VoiceChannel;
use prs_ai::Troops;
use prs_mode::{Deployment, Match, Mode, ModeCmd};
use prs_sim::protocol::PlayerCmd;
use prs_sim::{Level, Sim, SimEvent, SoldierId};

use crate::rides::OwnState;
use crate::{SimSchema, WireEvent, goes_on_wire, mode, remote, rides, rules_hash, shared};

#[derive(Clone, Debug)]
pub struct SimServerConfig {
    /// Metres between players at the spawn, as they spiral out from it.
    pub spawn_spacing: f32,
    /// How far above the spawn to start looking for the ground under a player.
    pub spawn_probe_height: f32,
    /// A player starts this far above the ground under them.
    pub spawn_lift: f32,
    /// How many ticks back to keep where everyone was, for judging shots by what their shooter saw.
    pub rewind_ticks: u32,
    /// How many bots a team gets instead of what the mode says. Clients don't need to agree on this.
    pub bots_per_team: Option<usize>,
    /// How often to tell clients the state of the match when nothing has happened, in ticks.
    pub match_state_every: u32,
    pub reach: ReachConfig,
}

/// How far off things matter to a client, in metres, and how much they matter compared to a soldier.
#[derive(Clone, Debug)]
pub struct ReachConfig {
    pub soldier_range: f32,
    pub vehicle_range: f32,
    pub vehicle_importance: f32,
    pub heli_range: f32,
    /// High enough to go in every snapshot at any distance. A helicopter crosses metres between
    /// snapshots, so anything less shows as a judder and puts it metres from where a shot is judged.
    pub heli_importance: f32,
    /// A shot is heard far beyond where the bullet lands is seen.
    pub shot_heard: f32,
    pub impact_seen: f32,
    /// How far anyone can be heard talking out loud. Nothing is sent to anyone further off than this.
    pub voice_heard: f32,
}

impl Default for ReachConfig {
    fn default() -> Self {
        Self {
            soldier_range: 1500.0,
            vehicle_range: 2500.0,
            vehicle_importance: 1.5,
            heli_range: 4000.0,
            heli_importance: 10.0,
            shot_heard: 3000.0,
            impact_seen: 400.0,
            voice_heard: 80.0,
        }
    }
}

impl Default for SimServerConfig {
    fn default() -> Self {
        Self { spawn_spacing: 1.5, spawn_probe_height: 30.0, spawn_lift: 0.1, rewind_ticks: 40, bots_per_team: None, match_state_every: 30, reach: ReachConfig::default() }
    }
}

/// Golden angle in radians, which spreads a spiral evenly however many are on it.
const SPIRAL_TURN: f32 = 2.399_963;

pub struct SimServer {
    pub sim: Sim,
    pub game: Match,
    bots: Option<Troops>,
    cfg: SimServerConfig,
    map: String,
    mode: String,
    rules: u64,
    free: Vec<SoldierId>,
    /// Waiting on their first command.
    parked: Vec<SoldierId>,
    mode_cmds: Vec<(SoldierId, ModeCmd)>,
    /// What clients are being sent, remade only as often as they should hear of the match.
    shared_bytes: Vec<u8>,
    all_cmds: Vec<(SoldierId, PlayerCmd)>,
    names: shared::Names,
}

impl SimServer {
    /// `None` if there is no map or mode by that name.
    pub fn new(map: &str, mode: &str, cfg: SimServerConfig) -> Option<Self> {
        Some(Self::with_sim(Sim::new(Level::named(map)?), map, mode, Mode::named(mode)?, cfg))
    }

    /// For a sim set up differently, as a mod would. Clients must set theirs up the same.
    pub fn with_sim(mut sim: Sim, map: &str, mode_name: &str, mut mode: Mode, cfg: SimServerConfig) -> Self {
        sim.remember_past_ticks(cfg.rewind_ticks);
        let rules = rules_hash(&sim, &mode);
        // After the hash: a client only ever sees bots as soldiers, so it needs to know nothing of this.
        if cfg.bots_per_team.is_some() {
            mode.cfg.bots_per_team = cfg.bots_per_team;
        }
        let wants_bots = mode.cfg.bots_per_team.is_some_and(|bots| bots > 0);
        let mut game = Match::new(mode, &mut sim);
        game.fill_bots(&mut sim);
        let bots = wants_bots.then(|| Troops::new(&sim, prs_ai::tuned_troops()));
        let shared_bytes = shared::write(&sim, &game.state, &Vec::new());
        Self {
            rules,
            sim,
            game,
            bots,
            cfg,
            map: map.to_owned(),
            mode: mode_name.to_owned(),
            free: Vec::new(),
            parked: Vec::new(),
            mode_cmds: Vec::new(),
            shared_bytes,
            all_cmds: Vec::new(),
            names: Vec::new(),
        }
    }

    /// Whether a player is out in the world, as opposed to dead or still choosing where to deploy.
    fn has_a_body(&self, id: u16) -> bool {
        let id = id as SoldierId;
        let Some(soldier) = self.sim.soldiers.get(id) else { return false };
        soldier.present && !soldier.health.is_dead() && self.game.state.player(id).is_none_or(|p| p.deployed)
    }

    fn spawn_point(&self, id: SoldierId) -> Vec3 {
        let angle = id as f32 * SPIRAL_TURN;
        let radius = self.cfg.spawn_spacing * (id as f32).sqrt();
        let spawn = self.sim.level.spawn;
        let flat = spawn + Vec3::new(angle.cos(), 0.0, angle.sin()) * radius;
        let top = flat + Vec3::Y * self.cfg.spawn_probe_height;
        let ground = self.sim.world.raycast(top, Vec3::NEG_Y, self.cfg.spawn_probe_height * 2.0);
        ground.map_or(flat, |hit| top + Vec3::NEG_Y * (hit.distance - self.cfg.spawn_lift))
    }
}

impl ServerGame for SimServer {
    type S = SimSchema;

    fn rules_hash(&self) -> u64 {
        self.rules
    }

    fn map_name(&self) -> String {
        self.map.clone()
    }

    fn mode_name(&self) -> String {
        self.mode.clone()
    }

    fn tick(&self) -> u32 {
        self.sim.tick
    }

    fn spawn(&mut self, seed: u32) -> u16 {
        let new = self.sim.spawn_soldier(seed);
        let id = match self.free.pop() {
            Some(slot) => {
                let fresh = self.sim.soldiers.pop().expect("just spawned");
                self.sim.soldiers[slot] = fresh;
                slot
            }
            None => new,
        };
        let at = self.spawn_point(id);
        let yaw = self.sim.level.spawn_yaw;
        self.sim.set_home(id, at, yaw);
        self.game.join(&mut self.sim, id, None, false);
        self.game.fill_bots(&mut self.sim);
        // Held still until their first command, and then for as long as the mode says.
        self.parked.push(id);
        self.sim.soldiers[id].present = false;
        id as u16
    }

    fn start(&mut self, id: u16) {
        let id = id as SoldierId;
        self.parked.retain(|&other| other != id);
        if self.game.mode.cfg.deployment == Deployment::Automatic {
            self.sim.soldiers[id].present = true;
        }
    }

    fn remove(&mut self, id: u16) {
        let id = id as SoldierId;
        self.parked.retain(|&other| other != id);
        self.game.leave(&mut self.sim, id);
        self.game.fill_bots(&mut self.sim);
        self.sim.soldiers[id].present = false;
        self.free.push(id);
        self.names.retain(|(named, _)| *named as SoldierId != id);
        self.shared_bytes = shared::write(&self.sim, &self.game.state, &self.names);
    }

    fn request(&mut self, id: u16, bytes: &[u8]) {
        if let Ok(cmd) = mode::decode_cmd(bytes) {
            self.mode_cmds.push((id as SoldierId, cmd));
        }
    }

    fn set_view_lag(&mut self, id: u16, ticks: u32) {
        self.sim.set_view_lag(id as SoldierId, ticks.min(self.cfg.rewind_ticks));
    }

    fn look(&self, id: u16) -> (f32, f32) {
        let soldier = &self.sim.soldiers[id as usize];
        (soldier.yaw, soldier.pitch)
    }

    fn step(&mut self, cmds: &[(u16, PlayerCmd)], events: &mut Vec<WireEvent>) {
        self.all_cmds.clear();
        self.all_cmds.extend(cmds.iter().map(|(id, cmd)| (*id as SoldierId, *cmd)));
        if let Some(bots) = &mut self.bots {
            self.all_cmds.extend_from_slice(bots.think(&self.sim, &self.game));
        }
        self.sim.step(&self.all_cmds);
        self.game.step(&mut self.sim, &self.mode_cmds);
        self.mode_cmds.clear();
        // It reads this step's events and may add to them, so they are collected after.
        events.extend(self.sim.events.iter().copied().filter(goes_on_wire).map(WireEvent::Sim));
        events.extend(self.game.events.iter().copied().map(WireEvent::Mode));
        if !self.game.events.is_empty() || self.sim.tick % self.cfg.match_state_every.max(1) == 0 {
            self.shared_bytes = shared::write(&self.sim, &self.game.state, &self.names);
        }
    }

    fn capture_world(&self) -> Vec<(NetEntity, Relevance)> {
        let reach = &self.cfg.reach;
        let sim = &self.sim;
        let soldiers = (0..sim.soldiers.len()).filter(|&id| sim.soldiers[id].present).map(|id| {
            let relevance = Relevance { position: sim.soldiers[id].body.pos, importance: 1.0, max_range: reach.soldier_range };
            (remote::capture(sim, id), relevance)
        });
        let vehicles = (0..sim.vehicles.len()).map(|v| {
            let relevance =
                Relevance { position: sim.vehicles[v].pos, importance: reach.vehicle_importance, max_range: reach.vehicle_range };
            (rides::capture_vehicle(sim, v), relevance)
        });
        let helis = (0..sim.helicopters.len()).map(|h| {
            let relevance =
                Relevance { position: sim.helicopters[h].state.pos, importance: reach.heli_importance, max_range: reach.heli_range };
            (rides::capture_heli(sim, h), relevance)
        });
        soldiers.chain(vehicles).chain(helis).collect()
    }

    fn viewer(&self, id: u16) -> Viewer {
        let soldier = &self.sim.soldiers[id as usize];
        Viewer { position: soldier.body.pos, forward: prs_sim::view_dir(soldier.yaw, soldier.pitch) }
    }

    fn set_name(&mut self, id: u16, name: &str) {
        self.names.retain(|(named, _)| *named != id);
        self.names.push((id, name.to_owned()));
        self.shared_bytes = shared::write(&self.sim, &self.game.state, &self.names);
    }

    fn voice_from(&self, speaker: u16, channel: VoiceChannel) -> Option<Vec3> {
        let soldier = self.sim.soldiers.get(speaker as SoldierId)?;
        match channel {
            // A dead man tells no tales, nor does someone waiting on the deploy screen.
            VoiceChannel::Local => self.has_a_body(speaker).then(|| soldier.body.pos),
            VoiceChannel::Squad => Some(Vec3::ZERO),
        }
    }

    fn voice_reaches(&self, speaker: u16, listener: u16, channel: VoiceChannel) -> bool {
        match channel {
            VoiceChannel::Local => {
                let (Some(from), Some(ear)) = (self.sim.soldiers.get(speaker as SoldierId), self.sim.soldiers.get(listener as SoldierId)) else {
                    return false;
                };
                self.has_a_body(speaker)
                    && self.game.state.player(listener as SoldierId).is_none_or(|p| p.deployed)
                    && from.body.pos.distance(ear.body.pos) <= self.cfg.reach.voice_heard
            }
            VoiceChannel::Squad => {
                let state = &self.game.state;
                let (Some(from), Some(to)) = (state.player(speaker as SoldierId), state.player(listener as SoldierId)) else {
                    return false;
                };
                from.team == to.team && from.squad.is_some() && from.squad == to.squad
            }
        }
    }

    fn event_reach(&self, event: &WireEvent) -> Option<(Vec3, f32)> {
        let reach = &self.cfg.reach;
        let WireEvent::Sim(event) = event else { return None };
        match *event {
            SimEvent::ShotFired { origin, .. } => Some((origin, reach.shot_heard)),
            SimEvent::Impact { point, .. } | SimEvent::TargetHit { point, .. } => Some((point, reach.impact_seen)),
            _ => None,
        }
    }

    fn own(&self, id: u16) -> OwnState {
        rides::own_state(&self.sim, id as SoldierId)
    }

    fn shared(&self) -> Vec<u8> {
        self.shared_bytes.clone()
    }

    fn predicted_by(event: &WireEvent, id: u16) -> bool {
        matches!(event, WireEvent::Sim(SimEvent::ShotFired { soldier, .. } | SimEvent::Seat { soldier, .. }) if *soldier == id as SoldierId)
    }
}
