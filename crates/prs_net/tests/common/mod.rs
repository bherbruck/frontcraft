//! A game small enough to reason about, with the awkward parts of the real one: private state that
//! only stays in step if the commands do, a weapon with a cooldown, and events.

#![allow(dead_code)]

use glam::{Vec2, Vec3};
use prs_net::client::GameFactory;
use prs_net::entity::NetEntity;
use prs_net::game::{PredictedGame, Relevance, Schema, ServerGame, Viewer};
use prs_net::server::ServerConfig;
use prs_net::testkit::World;
use prs_net::wire::{Reader, Welcome, WireError, Writer};
use prs_protocol::{Buttons, PlayerCmd, TICK_DT};
use std::rc::Rc;

pub const RUN_SPEED: f32 = 5.0;
const ACCELERATION: f32 = 30.0;
const SHOT_EVERY: f32 = 0.1;
const MAGAZINE: u32 = 30;
pub const TOY_SHOT_REACH: f32 = 500.0;
const POS_SCALE: f32 = 512.0;
const VEL_SCALE: f32 = 128.0;
const ANGLE_SCALE: f32 = 16384.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Toy {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub ammo: u32,
    pub wobble: f32,
    /// Never seen by anyone, but decides `wobble`, so a client has to be told it to stay in step.
    pub phase: f32,
    pub cooldown: f32,
}

impl Toy {
    fn new(pos: Vec3, seed: u32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            ammo: MAGAZINE,
            wobble: 0.0,
            phase: seed as f32 * 0.01,
            cooldown: 0.0,
        }
    }

    /// Returns whether a shot went off.
    fn step(&mut self, cmd: &PlayerCmd) -> bool {
        let (s, c) = cmd.yaw.sin_cos();
        let forward = Vec3::new(-s, 0.0, -c);
        let right = Vec3::new(c, 0.0, -s);
        let goal = (forward * cmd.wish_move.y + right * cmd.wish_move.x) * RUN_SPEED;
        let change = goal - self.vel;
        self.vel += change.clamp_length_max(ACCELERATION * TICK_DT);
        self.pos += self.vel * TICK_DT;
        self.yaw = cmd.yaw;
        self.pitch = cmd.pitch.clamp(-1.5, 1.5);
        self.phase += TICK_DT * (1.0 + self.vel.length());
        self.wobble = self.phase.sin() * 0.01;
        self.cooldown = (self.cooldown - TICK_DT).max(0.0);
        if cmd.buttons.contains(Buttons::FIRE) && self.cooldown == 0.0 && self.ammo > 0 {
            self.cooldown = SHOT_EVERY;
            self.ammo -= 1;
            return true;
        }
        if cmd.buttons.contains(Buttons::RELOAD) {
            self.ammo = MAGAZINE;
        }
        false
    }

    fn own(&self) -> ToyOwn {
        ToyOwn {
            pos: self.pos,
            vel: self.vel,
            yaw: self.yaw,
            pitch: self.pitch,
            wobble: self.wobble,
            ammo: self.ammo,
            phase: self.phase,
            cooldown: self.cooldown,
        }
    }

    fn apply(&mut self, own: &ToyOwn) {
        self.pos = own.pos;
        self.vel = own.vel;
        self.yaw = own.yaw;
        self.pitch = own.pitch;
        self.wobble = own.wobble;
        self.ammo = own.ammo;
        self.phase = own.phase;
        self.cooldown = own.cooldown;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToyOwn {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub wobble: f32,
    pub ammo: u32,
    pub phase: f32,
    pub cooldown: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToyShot {
    pub shooter: u16,
    pub from: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToyPose {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub wobble: f32,
}

pub struct ToySchema;

impl Schema for ToySchema {
    type Own = ToyOwn;
    type Event = ToyShot;
    type Pose = ToyPose;
    fn groups(kind: u8) -> Option<&'static [(usize, usize)]> {
        (kind == 0).then_some(&[(0, 3), (3, 3), (6, 1), (7, 1)][..])
    }

    fn write_own(own: &ToyOwn, _: Option<&ToyOwn>, w: &mut Writer) {
        w.vec3(own.pos);
        w.vec3(own.vel);
        w.f32(own.yaw);
        w.f32(own.pitch);
        w.f32(own.wobble);
        w.varu(own.ammo as u64);
        w.f32(own.phase);
        w.f32(own.cooldown);
    }

    fn read_own(_: Option<&ToyOwn>, r: &mut Reader) -> Result<ToyOwn, WireError> {
        Ok(ToyOwn {
            pos: r.vec3()?,
            vel: r.vec3()?,
            yaw: r.f32()?,
            pitch: r.f32()?,
            wobble: r.f32()?,
            ammo: r.varu32()?,
            phase: r.f32()?,
            cooldown: r.f32()?,
        })
    }

    fn own_matches(a: &ToyOwn, b: &ToyOwn, epsilon: f32) -> bool {
        a.ammo == b.ammo
            && a.pos.distance(b.pos) <= epsilon
            && a.vel.distance(b.vel) <= epsilon
            && (a.yaw - b.yaw).abs() <= epsilon
            && (a.pitch - b.pitch).abs() <= epsilon
            && (a.wobble - b.wobble).abs() <= epsilon
            && (a.phase - b.phase).abs() <= epsilon
            && (a.cooldown - b.cooldown).abs() <= epsilon
    }

    fn own_position(own: &ToyOwn) -> Vec3 {
        own.pos
    }

    fn write_event(event: &ToyShot, w: &mut Writer) {
        w.varu(event.shooter as u64);
        w.vec3(event.from);
    }

    fn read_event(r: &mut Reader) -> Result<ToyShot, WireError> {
        Ok(ToyShot { shooter: r.varu32()? as u16, from: r.vec3()? })
    }

    fn urgent(_: &ToyShot, _: u16) -> bool {
        false
    }

    fn pose(e: &NetEntity) -> ToyPose {
        let f = &e.fields;
        ToyPose {
            pos: Vec3::new(f[0] as f32, f[1] as f32, f[2] as f32) / POS_SCALE,
            vel: Vec3::new(f[3] as f32, f[4] as f32, f[5] as f32) / VEL_SCALE,
            yaw: f[6] as f32 / ANGLE_SCALE,
            wobble: f[7] as f32 / ANGLE_SCALE,
        }
    }

    fn lerp_pose(a: &ToyPose, b: &ToyPose, t: f32) -> ToyPose {
        ToyPose {
            pos: a.pos.lerp(b.pos, t),
            vel: a.vel.lerp(b.vel, t),
            yaw: a.yaw + (b.yaw - a.yaw) * t,
            wobble: a.wobble + (b.wobble - a.wobble) * t,
        }
    }

    fn pose_position(pose: &ToyPose) -> Vec3 {
        pose.pos
    }

    fn extrapolate_pose(pose: &ToyPose, seconds: f32) -> ToyPose {
        ToyPose { pos: pose.pos + pose.vel * seconds, ..*pose }
    }
}

fn rules_of(speed_rules: u64) -> u64 {
    prs_net::hash_text(&format!("toy rules {speed_rules}"))
}

pub struct ToyClient {
    pub me: Toy,
    pub rules: u64,
    pub id: u16,
    pub shared: Vec<u8>,
}

impl ToyClient {
    pub fn new() -> Self {
        Self { me: Toy::new(Vec3::ZERO, 0), rules: rules_of(1), id: 0, shared: Vec::new() }
    }

    pub fn factory() -> GameFactory<ToyClient> {
        Box::new(|_, _| Some(Self::new()))
    }

    pub fn with_other_rules() -> GameFactory<ToyClient> {
        Box::new(|_, _| Some(Self { rules: rules_of(2), ..Self::new() }))
    }
}

impl PredictedGame for ToyClient {
    type S = ToySchema;
    type Saved = Toy;
    type Event = ToyShot;

    fn rules_hash(&self) -> u64 {
        self.rules
    }

    fn apply_shared(&mut self, shared: &[u8]) {
        self.shared = shared.to_vec();
    }

    fn on_server_event(&mut self, _: &ToyShot) -> bool {
        false
    }

    fn join(&mut self, welcome: &Welcome<ToySchema>) {
        self.id = welcome.soldier;
        self.me.apply(&welcome.own);
    }

    fn step(&mut self, cmd: &PlayerCmd, events: Option<&mut Vec<ToyShot>>) {
        let fired = self.me.step(cmd);
        if let (true, Some(events)) = (fired, events) {
            events.push(ToyShot { shooter: self.id, from: self.me.pos });
        }
    }

    fn save(&self) -> Toy {
        self.me.clone()
    }

    fn restore(&mut self, saved: &Toy) {
        self.me = saved.clone();
    }

    fn own_of(saved: &Toy) -> ToyOwn {
        saved.own()
    }

    fn apply_own(saved: &mut Toy, own: &ToyOwn) {
        saved.apply(own);
    }

    fn position(&self) -> Vec3 {
        self.me.pos
    }

    fn show_remotes(&mut self, _: &[(u32, ToyPose)], _: f32) {}
}

pub struct ToyServer {
    pub tick: u32,
    pub players: Vec<Option<Toy>>,
    pub started: Vec<bool>,
    pub shots: Vec<u32>,
    pub view_lag: Vec<u32>,
    pub requests: Vec<(u16, Vec<u8>)>,
    pub max_range: f32,
    /// Stands in for the rarely changing world state.
    pub shared: Vec<u8>,
}

impl ToyServer {
    pub fn new() -> Self {
        Self {
            tick: 0,
            players: Vec::new(),
            started: Vec::new(),
            shots: Vec::new(),
            view_lag: Vec::new(),
            requests: Vec::new(),
            max_range: 10_000.0,
            shared: vec![1, 2, 3],
        }
    }

    pub fn player(&self, id: u16) -> &Toy {
        self.players[id as usize].as_ref().expect("in the game")
    }

    pub fn player_mut(&mut self, id: u16) -> &mut Toy {
        self.players[id as usize].as_mut().expect("in the game")
    }

    pub fn position_of(&self, id: u16) -> Vec3 {
        self.player(id).pos
    }
}

impl ServerGame for ToyServer {
    type S = ToySchema;

    fn rules_hash(&self) -> u64 {
        rules_of(1)
    }

    fn map_name(&self) -> String {
        "toyland".into()
    }

    fn tick(&self) -> u32 {
        self.tick
    }

    fn start(&mut self, id: u16) {
        self.started[id as usize] = true;
    }

    fn set_view_lag(&mut self, id: u16, ticks: u32) {
        self.view_lag[id as usize] = ticks;
    }

    fn shared(&self) -> Vec<u8> {
        self.shared.clone()
    }

    fn mode_name(&self) -> String {
        "toymode".into()
    }

    fn request(&mut self, id: u16, bytes: &[u8]) {
        self.requests.push((id, bytes.to_vec()));
    }

    fn spawn(&mut self, seed: u32) -> u16 {
        let slot = self.players.iter().position(Option::is_none).unwrap_or(self.players.len());
        if slot == self.players.len() {
            self.players.push(None);
            self.started.push(false);
            self.shots.push(0);
            self.view_lag.push(0);
        }
        let at = Vec3::new(slot as f32 * 2.0, 0.0, 0.0);
        self.players[slot] = Some(Toy::new(at, seed));
        self.started[slot] = false;
        slot as u16
    }

    fn remove(&mut self, id: u16) {
        self.players[id as usize] = None;
        self.started[id as usize] = false;
    }

    fn look(&self, id: u16) -> (f32, f32) {
        let toy = self.player(id);
        (toy.yaw, toy.pitch)
    }

    fn step(&mut self, cmds: &[(u16, PlayerCmd)], events: &mut Vec<ToyShot>) {
        self.tick += 1;
        for (id, cmd) in cmds {
            let toy = self.players[*id as usize].as_mut().expect("in the game");
            if toy.step(cmd) {
                self.shots[*id as usize] += 1;
                events.push(ToyShot { shooter: *id, from: toy.pos });
            }
        }
    }

    fn capture_world(&self) -> Vec<(NetEntity, Relevance)> {
        (0..self.players.len() as u16)
            .filter(|&id| self.started[id as usize] && self.players[id as usize].is_some())
            .map(|id| {
                let toy = self.player(id);
                (capture_toy(toy, id), Relevance { position: toy.pos, importance: 1.0, max_range: self.max_range })
            })
            .collect()
    }

    fn viewer(&self, id: u16) -> Viewer {
        let toy = self.player(id);
        let (s, c) = toy.yaw.sin_cos();
        Viewer { position: toy.pos, forward: Vec3::new(-s, 0.0, -c) }
    }

    fn event_reach(&self, event: &ToyShot) -> Option<(Vec3, f32)> {
        Some((event.from, TOY_SHOT_REACH))
    }

    fn own(&self, id: u16) -> ToyOwn {
        self.player(id).own()
    }

    fn predicted_by(event: &ToyShot, id: u16) -> bool {
        event.shooter == id
    }
}

fn capture_toy(toy: &Toy, id: u16) -> NetEntity {
    let mut e = NetEntity::new(0, id as u32);
    for axis in 0..3 {
        e.fields[axis] = (toy.pos[axis] * POS_SCALE).round() as i32;
        e.fields[3 + axis] = (toy.vel[axis] * VEL_SCALE).round() as i32;
    }
    e.fields[6] = (toy.yaw * ANGLE_SCALE).round() as i32;
    e.fields[7] = (toy.wobble * ANGLE_SCALE).round() as i32;
    e
}

pub type ToyWorld = World<ToyClient, ToyServer>;

pub fn toy_world(cfg: ServerConfig) -> ToyWorld {
    World::new(ToyServer::new(), cfg, ToyServer::position_of)
}

/// Stands still.
pub fn idle() -> Rc<dyn Fn(u32, u32) -> PlayerCmd> {
    Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() })
}

/// Walks steadily round a circle.
pub fn patrol() -> Rc<dyn Fn(u32, u32) -> PlayerCmd> {
    Rc::new(|who, tick| {
        let t = (tick + who * 37) as f32 / 60.0;
        PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), yaw: t * 0.6, ..Default::default() }
    })
}

/// Runs, turns, stops, fires bursts and reloads.
pub fn busy() -> Rc<dyn Fn(u32, u32) -> PlayerCmd> {
    Rc::new(|who, tick| {
        let phase = (tick + who * 37) % 600;
        let t = phase as f32 / 60.0;
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, (360..420).contains(&phase));
        buttons.set(Buttons::RELOAD, phase == 560);
        PlayerCmd {
            tick,
            wish_move: if phase < 300 { Vec2::new((t * 2.0).sin(), 1.0).normalize() } else { Vec2::ZERO },
            yaw: t * 0.6 + (t * 3.0).sin() * 0.05,
            pitch: (t * 0.3).sin() * 0.15,
            buttons,
            ..Default::default()
        }
    })
}
