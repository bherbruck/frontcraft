//! The simulation as a client runs it: its own soldier for real, everyone else only as they are drawn.
//! Nothing here decides what a bullet hits, because `Sim::step_predicted` never asks.

use glam::{Quat, Vec3};
use prs_net::predict::CorrectionStyle;
use prs_net::game::{PredictedGame, Schema};
use prs_net::wire::Welcome;
use prs_sim::body::AnimInput;
use prs_sim::health::Life;
use prs_sim::protocol::PlayerCmd;
use prs_mode::{Deployment, MatchState, Mode, ModeEvent};
use prs_sim::throwables::Thrown;
use prs_sim::{Level, Sim, SimEvent, SoldierId, stance_blend};

use crate::remote::RemotePose;
use crate::rides::{self, OwnState};
use crate::{AnyPose, SimSchema, WireEvent, rules_hash, shared, shown_when_predicted};

pub struct SimClient {
    pub sim: Sim,
    pub local: SoldierId,
    rules: u64,
    /// The match as the server last said, for showing. A client never decides anything about it.
    pub match_state: Option<MatchState>,
    match_changed: bool,
    /// Whether joining is all it takes to be in the game, with no spawn screen first.
    deploys_itself: bool,
    /// What everyone is called, by number, as the server last said.
    pub names: shared::Names,
}

impl SimClient {
    /// Set up for this map and mode, or `None` if this client does not have them.
    pub fn setup(map: &str, mode: &str) -> Option<Self> {
        Some(Self::with_sim(Sim::new(Level::named(map)?), Mode::named(mode)?))
    }

    /// For a sim set up differently, as a mod would. It must match the server's.
    pub fn with_sim(mut sim: Sim, mode: Mode) -> Self {
        // Only the server's match brings anyone back.
        sim.hold_respawns = true;
        let deploys_itself = mode.cfg.deployment == Deployment::Automatic;
        Self { rules: rules_hash(&sim, &mode), sim, local: 0, match_state: None, match_changed: false, names: Vec::new(), deploys_itself }
    }

    /// The match state, if it has changed since this was last asked.
    pub fn take_match_update(&mut self) -> Option<&MatchState> {
        std::mem::take(&mut self.match_changed).then_some(self.match_state.as_ref()).flatten()
    }

    fn grow_to(&mut self, id: SoldierId) {
        while self.sim.soldiers.len() <= id {
            let new = self.sim.spawn_soldier(0);
            self.sim.soldiers[new].present = false;
        }
    }
}

impl SimClient {
    /// The helicopter whose controls the local player has.
    fn flying(&self) -> Option<usize> {
        let at = self.sim.soldiers.get(self.local)?.seat?;
        prs_sim::heli_of(at.carrier).filter(|_| self.sim.is_at_controls(self.local))
    }

    /// The vehicle this player is driving, if they are.
    fn driving(&self) -> Option<usize> {
        let at = self.sim.soldiers.get(self.local)?.seat?;
        (prs_sim::heli_of(at.carrier).is_none() && self.sim.is_at_controls(self.local)).then_some(at.carrier as usize)
    }
}

impl PredictedGame for SimClient {
    type S = SimSchema;
    type Saved = OwnState;
    type Event = SimEvent;

    fn rules_hash(&self) -> u64 {
        self.rules
    }

    fn apply_shared(&mut self, bytes: &[u8]) {
        // Comes from the server we chose to play on, and a bad one only costs what is on the ground.
        if let Ok((state, names)) = shared::apply(&mut self.sim, bytes) {
            self.names = names;
            // The roster names everyone in the match, including people too far off to have been sent yet,
            // and anything reading it looks them up by id.
            let named = state.players.iter().map(|p| p.soldier).chain(state.squads.iter().flat_map(|s| s.members.iter().copied()));
            if let Some(most) = named.max() {
                self.grow_to(most);
            }
            self.match_state = Some(state);
            self.match_changed = true;
        }
    }

    fn on_server_event(&mut self, event: &WireEvent) -> bool {
        match event {
            WireEvent::Sim(SimEvent::ShotFired { soldier, origin, dir, weapon }) if *soldier != self.local => {
                self.sim.add_tracer(*soldier, *origin, *dir, *weapon);
                false
            }
            WireEvent::Sim(SimEvent::ThrownReleased { soldier, id, kind, origin, vel, fuse }) if *soldier != self.local => {
                self.sim.add_thrown(Thrown { id: *id, kind: *kind, thrower: *soldier, pos: *origin, vel: *vel, fuse: *fuse, at_rest: false });
                false
            }
            WireEvent::Sim(SimEvent::ThrownBurst { id, kind, point }) => {
                self.sim.hear_burst(*id, *kind, *point);
                false
            }
            // Coming back is being put somewhere new, and is not something to fade across.
            WireEvent::Sim(SimEvent::Respawned { soldier }) => *soldier == self.local,
            WireEvent::Mode(ModeEvent::Deployed { soldier }) => *soldier == self.local,
            _ => false,
        }
    }

    fn join(&mut self, welcome: &Welcome<SimSchema>) {
        self.local = welcome.soldier as SoldierId;
        self.grow_to(self.local);
        rides::apply_own(&mut self.sim, self.local, &welcome.own);
        // The server holds them still until the first command comes, which is the one about to go.
        self.sim.soldiers[self.local].present = self.deploys_itself;
    }

    fn step(&mut self, cmd: &PlayerCmd, events: Option<&mut Vec<SimEvent>>) {
        let bullets = self.sim.bullets.len();
        let thrown = self.sim.thrown.len();
        self.sim.step_predicted(self.local, cmd);
        match events {
            Some(out) => {
                self.sim.fly_tracers();
                self.sim.fly_thrown();
                out.extend(self.sim.events.iter().copied().filter(shown_when_predicted));
            }
            // This has already happened as far as anyone watching is concerned.
            None => {
                self.sim.bullets.truncate(bullets);
                self.sim.thrown.truncate(thrown);
            }
        }
        self.sim.events.clear();
    }

    fn save(&self) -> OwnState {
        rides::own_state(&self.sim, self.local)
    }

    fn restore(&mut self, saved: &OwnState) {
        if let Some(treatment) = &saved.soldier.treatment {
            self.grow_to(treatment.patient);
        }
        rides::apply_own(&mut self.sim, self.local, saved);
    }

    fn own_of(saved: &OwnState) -> OwnState {
        saved.clone()
    }

    fn apply_own(saved: &mut OwnState, own: &OwnState) {
        saved.clone_from(own);
    }

    fn position(&self) -> Vec3 {
        self.sim.soldiers[self.local].body.pos
    }

    fn attitude(&self) -> Quat {
        if let Some(v) = self.driving() {
            return self.sim.vehicles[v].rot;
        }
        self.flying().map_or(Quat::IDENTITY, |h| self.sim.helicopters[h].state.rot)
    }

    fn correction_style(&self, usual: &CorrectionStyle) -> CorrectionStyle {
        if let Some(v) = self.driving() {
            let view = &self.sim.vehicle_cfgs[self.sim.vehicles[v].kind].net_view;
            return CorrectionStyle { smoothing_rate: view.fade_rate, snap_distance: view.snap_distance, max_speed: view.fade_speed, max_turn_rate: view.fade_turn };
        }
        if self.flying().is_none() {
            return *usual;
        }
        let view = &self.sim.heli_cfg.net_view;
        CorrectionStyle { smoothing_rate: view.fade_rate, snap_distance: view.snap_distance, max_speed: view.fade_speed, max_turn_rate: view.fade_turn }
    }

    fn remotes_from(&mut self, tick: f64) {
        self.sim.others_seen_at = Some(tick as f32);
    }

    fn show_remotes(&mut self, remotes: &[(u32, <SimSchema as Schema>::Pose)], dt: f32) {
        // Whatever we are driving is ours to predict and is not drawn from what the server sent.
        let at_controls = self.sim.soldiers[self.local].seat.filter(|_| self.sim.is_at_controls(self.local));
        // Things first, as who sits in them follows them.
        for (_, pose) in remotes {
            match pose {
                AnyPose::Vehicle(vehicle) => rides::show_vehicle(&mut self.sim, vehicle, at_controls),
                AnyPose::Heli(heli) => rides::show_heli(&mut self.sim, heli, at_controls),
                AnyPose::Soldier(_) => {}
            }
        }
        let mut listed = remotes
            .iter()
            .filter_map(|(key, pose)| match pose {
                AnyPose::Soldier(soldier) => Some((*key as SoldierId, soldier)),
                _ => None,
            })
            .peekable();
        let most = remotes
            .iter()
            .filter(|(_, pose)| matches!(pose, AnyPose::Soldier(_)))
            .map(|(key, _)| *key as SoldierId)
            .max();
        if let Some(most) = most {
            self.grow_to(most);
        }
        for id in 0..self.sim.soldiers.len() {
            if id == self.local {
                continue;
            }
            let pose: Option<&RemotePose> = listed.next_if(|(other, _)| *other == id).map(|(_, pose)| pose);
            let sim = &mut self.sim;
            let lifted = pose.filter(|pose| pose.lifting).and_then(|pose| sim.lifted_near(pose.feet));
            let pushed = pose.filter(|pose| pose.pushing).and_then(|pose| sim.pushed_near(pose.feet));
            let soldier = &mut sim.soldiers[id];
            soldier.present = pose.is_some();
            let Some(pose) = pose else { continue };
            pose.apply(soldier, &sim.weapons, &sim.move_cfg);
            let (crouch, prone) = stance_blend(&sim.move_cfg, pose.eye_height);
            let input = AnimInput {
                yaw: pose.yaw,
                velocity: pose.velocity,
                grounded: pose.grounded,
                crouch,
                prone,
                down: pose.life != Life::Up,
            };
            soldier.anim.step(&sim.body_cfg, &input, dt);
            soldier.anim.collapse = pose.collapse;
            let heaving = pose.lifting || pose.pushing;
            soldier.anim.step_brace(&sim.body_cfg, pose.carrying, dt);
            soldier.anim.step_heave(&sim.body_cfg, heaving && pose.slung > 0.0, dt);
            soldier.anim.slung = pose.slung;
            soldier.lifting = lifted;
            soldier.pushing = pushed;
            soldier.anim.step_care(&sim.body_cfg, pose.care, dt);
            if pose.seat.is_some() {
                sim.place_seated(id);
            }
        }
    }
}
