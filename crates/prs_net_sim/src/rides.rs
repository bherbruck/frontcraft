//! Vehicles and helicopters on the wire. Whoever is at the controls predicts it and gets all of it to
//! the bit, along with their own soldier. Everyone else gets a compact copy to draw.
//!
//! Transforms go as raw bits rather than quantised. Soldiers bump into them on both sides, so a client
//! needs the very same numbers for a parked truck to push them the same way, and something that is not
//! moving then costs nothing at all. The destructures list every field with no `..`, so a new field
//! stops this compiling until someone decides how it goes on the wire.

use glam::{Quat, Vec2, Vec3};
use prs_net::entity::NetEntity;
use prs_net::wire::{Reader, WireError, Writer};
use prs_sim::helicopter::{Engine, HeliState, NetTolerance, ZoneHealth};
use prs_sim::seats::{Occupants, SeatedAt};
use prs_sim::vehicles::{Damage, EngineState, VehicleKind, VehicleState, WheelState};
use prs_sim::{Sim, Soldier, SoldierId};

use crate::events::{surface_from_u8, surface_to_u8};
use crate::{KIND_HELI, KIND_VEHICLE, own};

const MAX_SEATS: usize = 64;
const MAX_WHEELS: usize = 8;
const COMPRESSION_SCALE: f32 = 2048.0;
const ANGLE_SCALE: f32 = 4096.0;
const SPIN_SCALE: f32 = 32.0;
const VEL_SCALE: f32 = 128.0;
const ANG_VEL_SCALE: f32 = 1024.0;
const FRACTION_SCALE: f32 = 255.0;
const HEALTH_SCALE: f32 = 8.0;
const RPM_SCALE: f32 = 1.0;
const TILT_SCALE: f32 = 4096.0;
const RADIANS_PER_TURN: f32 = std::f32::consts::TAU;

const POSE: usize = 0;
const VEL: usize = 7;
/// How fast it is turning. Riding in something shakes a gun by this, so a passenger needs it.
const ANG_VEL: usize = 10;
const STEER: usize = 13;
const RPM: usize = 14;
const LOAD: usize = 15;
const FLAGS: usize = 16;
const HULL: usize = 17;
const ENGINE: usize = 18;
const WHEELS: usize = 19;
const WHEEL_FIELDS: usize = 4;
/// How far open each door is, four bits each, all in one field past the last wheel.
const DOORS: usize = WHEELS + WHEEL_FIELDS * MAX_WHEELS;
const DOOR_STEPS: f32 = 15.0;
const MAX_DOORS: usize = 8;

const RUNNING: i32 = 1 << 0;
const BURNING: i32 = 1 << 1;
const WRECKED: i32 = 1 << 2;
/// A bit per wheel from here up for being on the ground. Riding in something airborne shakes a gun.
const WHEEL_ON_GROUND: u32 = 3;

/// Fields that change together share a bit in the delta's mask: (first field, how many).
pub(crate) const VEHICLE_GROUPS: &[(usize, usize)] = &[
    (POSE, 7),
    (VEL, 6),
    (STEER, 1),
    (RPM, 2),
    (FLAGS, 1),
    (HULL, 2),
    (DOORS, 1),
    (WHEELS, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 2, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 3, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 4, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 5, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 6, WHEEL_FIELDS),
    (WHEELS + WHEEL_FIELDS * 7, WHEEL_FIELDS),
];

const H_ROTOR: usize = 13;
const H_ANGLE: usize = 14;
const H_COLLECTIVE: usize = 15;
const H_DISC: usize = 16;
const H_FLAGS: usize = 18;
const H_HEALTH: usize = 19;
/// What it sounds like from outside: the engine, and how hard the rotor is working.
const H_SPOOL: usize = 21;
const H_POWER: usize = 22;
const H_THRUST: usize = 23;
/// Tens of newtons is far finer than an ear.
const THRUST_SCALE: f32 = 0.1;

pub(crate) const HELI_GROUPS: &[(usize, usize)] =
    &[(POSE, 7), (VEL, 6), (H_ROTOR, 2), (H_COLLECTIVE, 3), (H_FLAGS, 1), (H_HEALTH, 5), (H_SPOOL, 3)];

const ON_GROUND: i32 = 1 << 2;
const PARKED: i32 = 1 << 3;
const H_WRECKED: i32 = 1 << 4;

fn quant(value: f32, scale: f32) -> i32 {
    (value * scale).round() as i32
}

fn bits_of(value: f32) -> i32 {
    value.to_bits() as i32
}

fn from_bits(value: i32) -> f32 {
    f32::from_bits(value as u32)
}

fn put_transform(f: &mut [i32], pos: Vec3, rot: Quat) {
    for axis in 0..3 {
        f[POSE + axis] = bits_of(pos[axis]);
    }
    for (i, v) in rot.to_array().into_iter().enumerate() {
        f[POSE + 3 + i] = bits_of(v);
    }
}

fn get_transform(f: &[i32]) -> (Vec3, Quat) {
    let pos = Vec3::new(from_bits(f[POSE]), from_bits(f[POSE + 1]), from_bits(f[POSE + 2]));
    let rot = Quat::from_array([from_bits(f[POSE + 3]), from_bits(f[POSE + 4]), from_bits(f[POSE + 5]), from_bits(f[POSE + 6])]);
    (pos, rot)
}

/// The short way round from one wrapped angle to another.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let half = RADIANS_PER_TURN / 2.0;
    a + ((b - a + half).rem_euclid(RADIANS_PER_TURN) - half) * t
}

pub fn capture_vehicle(sim: &Sim, v: usize) -> NetEntity {
    let vehicle = &sim.vehicles[v];
    let mut entity = NetEntity::new(KIND_VEHICLE, v as u32);
    let f = &mut entity.fields;
    put_transform(f, vehicle.pos, vehicle.rot);
    for axis in 0..3 {
        f[VEL + axis] = quant(vehicle.vel[axis], VEL_SCALE);
        f[ANG_VEL + axis] = quant(vehicle.ang_vel[axis], ANG_VEL_SCALE);
    }
    f[STEER] = quant(vehicle.steer, ANGLE_SCALE);
    f[DOORS] = sim.vehicle_doors[v].iter().take(MAX_DOORS).enumerate().fold(0, |all, (i, share)| all | quant(*share, DOOR_STEPS) << (i * 4));
    f[RPM] = quant(vehicle.engine.rpm, RPM_SCALE);
    f[LOAD] = quant(vehicle.engine.load, FRACTION_SCALE);
    f[FLAGS] = if vehicle.engine.running { RUNNING } else { 0 }
        | if vehicle.damage.burning.is_some() { BURNING } else { 0 }
        | if vehicle.damage.wrecked { WRECKED } else { 0 }
        | vehicle.wheels.iter().take(MAX_WHEELS).enumerate().fold(0, |all, (i, wheel)| {
            all | (wheel.on_ground as i32) << (WHEEL_ON_GROUND + i as u32)
        });
    f[HULL] = quant(vehicle.damage.hull, HEALTH_SCALE);
    f[ENGINE] = quant(vehicle.damage.engine, HEALTH_SCALE);
    for (i, wheel) in vehicle.wheels.iter().take(MAX_WHEELS).enumerate() {
        let at = WHEELS + i * WHEEL_FIELDS;
        f[at] = quant(wheel.compression, COMPRESSION_SCALE);
        f[at + 1] = quant(wheel.angle, ANGLE_SCALE);
        f[at + 2] = quant(wheel.spin, SPIN_SCALE);
        f[at + 3] = quant(wheel.tyre, HEALTH_SCALE);
    }
    entity
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelPose {
    pub on_ground: bool,
    pub compression: f32,
    pub angle: f32,
    pub spin: f32,
    pub tyre: f32,
}

/// A vehicle in world units, as drawn at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct VehiclePose {
    pub id: u32,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub steer: f32,
    pub rpm: f32,
    pub load: f32,
    pub running: bool,
    pub burning: bool,
    pub wrecked: bool,
    pub hull: f32,
    pub engine: f32,
    pub wheels: [WheelPose; MAX_WHEELS],
    pub doors: [f32; MAX_DOORS],
}

impl VehiclePose {
    pub fn from_entity(entity: &NetEntity) -> Self {
        let f = &entity.fields;
        let (pos, rot) = get_transform(f);
        let mut wheels = [WheelPose::default(); MAX_WHEELS];
        for (i, wheel) in wheels.iter_mut().enumerate() {
            let at = WHEELS + i * WHEEL_FIELDS;
            *wheel = WheelPose {
                on_ground: f[FLAGS] & (1 << (WHEEL_ON_GROUND + i as u32)) != 0,
                compression: f[at] as f32 / COMPRESSION_SCALE,
                angle: f[at + 1] as f32 / ANGLE_SCALE,
                spin: f[at + 2] as f32 / SPIN_SCALE,
                tyre: f[at + 3] as f32 / HEALTH_SCALE,
            };
        }
        Self {
            id: entity.id(),
            pos,
            rot,
            vel: Vec3::new(f[VEL] as f32, f[VEL + 1] as f32, f[VEL + 2] as f32) / VEL_SCALE,
            ang_vel: Vec3::new(f[ANG_VEL] as f32, f[ANG_VEL + 1] as f32, f[ANG_VEL + 2] as f32) / ANG_VEL_SCALE,
            steer: f[STEER] as f32 / ANGLE_SCALE,
            rpm: f[RPM] as f32 / RPM_SCALE,
            load: f[LOAD] as f32 / FRACTION_SCALE,
            running: f[FLAGS] & RUNNING != 0,
            burning: f[FLAGS] & BURNING != 0,
            wrecked: f[FLAGS] & WRECKED != 0,
            hull: f[HULL] as f32 / HEALTH_SCALE,
            engine: f[ENGINE] as f32 / HEALTH_SCALE,
            wheels,
            doors: std::array::from_fn(|i| ((f[DOORS] >> (i * 4)) & 15) as f32 / DOOR_STEPS),
        }
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let mut wheels = other.wheels;
        for (i, wheel) in wheels.iter_mut().enumerate() {
            let (a, b) = (&self.wheels[i], &other.wheels[i]);
            wheel.compression = mix(a.compression, b.compression);
            wheel.angle = lerp_angle(a.angle, b.angle, t);
            wheel.spin = mix(a.spin, b.spin);
        }
        Self {
            // Exactly either end when they are the same, so something parked stays to the bit.
            pos: if self.pos == other.pos { other.pos } else { self.pos.lerp(other.pos, t) },
            rot: if self.rot == other.rot { other.rot } else { self.rot.slerp(other.rot, t) },
            vel: self.vel.lerp(other.vel, t),
            ang_vel: self.ang_vel.lerp(other.ang_vel, t),
            steer: mix(self.steer, other.steer),
            rpm: mix(self.rpm, other.rpm),
            load: mix(self.load, other.load),
            wheels,
            ..other.clone()
        }
    }
}

pub(crate) fn show_vehicle(sim: &mut Sim, pose: &VehiclePose, at_controls: Option<SeatedAt>) {
    // Nobody predicts doors, so even a driver's are as the server says.
    if let Some(doors) = sim.vehicle_doors.get_mut(pose.id as usize) {
        for (door, seen) in doors.iter_mut().zip(pose.doors) {
            *door = seen;
        }
    }
    if at_controls.is_some_and(|at| at.carrier == pose.id) {
        return;
    }
    let Some(vehicle) = sim.vehicles.get_mut(pose.id as usize) else { return };
    vehicle.pos = pose.pos;
    vehicle.rot = pose.rot;
    vehicle.vel = pose.vel;
    vehicle.ang_vel = pose.ang_vel;
    vehicle.steer = pose.steer;
    vehicle.engine.rpm = pose.rpm;
    vehicle.engine.load = pose.load;
    vehicle.engine.running = pose.running;
    vehicle.damage.burning = pose.burning.then_some(vehicle.damage.burning.unwrap_or(0.0));
    vehicle.damage.wrecked = pose.wrecked;
    vehicle.damage.hull = pose.hull;
    vehicle.damage.engine = pose.engine;
    for (wheel, seen) in vehicle.wheels.iter_mut().zip(&pose.wheels) {
        wheel.on_ground = seen.on_ground;
        wheel.compression = seen.compression;
        wheel.angle = seen.angle;
        wheel.spin = seen.spin;
        wheel.tyre = seen.tyre;
    }
}

fn engine_to_u8(engine: Engine) -> u8 {
    match engine {
        Engine::Off => 0,
        Engine::Starting => 1,
        Engine::Running => 2,
        Engine::Dead => 3,
    }
}

fn engine_from_u8(v: u8) -> Engine {
    match v {
        1 => Engine::Starting,
        2 => Engine::Running,
        3 => Engine::Dead,
        _ => Engine::Off,
    }
}

pub fn capture_heli(sim: &Sim, h: usize) -> NetEntity {
    let state = &sim.helicopters[h].state;
    let mut entity = NetEntity::new(KIND_HELI, h as u32);
    let f = &mut entity.fields;
    put_transform(f, state.pos, state.rot);
    for axis in 0..3 {
        f[VEL + axis] = quant(state.vel[axis], VEL_SCALE);
        f[ANG_VEL + axis] = quant(state.spin[axis], ANG_VEL_SCALE);
    }
    f[H_ROTOR] = quant(state.rotor_speed, SPIN_SCALE);
    f[H_ANGLE] = quant(state.rotor_angle.rem_euclid(RADIANS_PER_TURN), ANGLE_SCALE);
    f[H_COLLECTIVE] = quant(state.collective, FRACTION_SCALE);
    f[H_DISC] = quant(state.disc.x, TILT_SCALE);
    f[H_DISC + 1] = quant(state.disc.y, TILT_SCALE);
    f[H_FLAGS] = engine_to_u8(state.engine) as i32
        | if state.on_ground { ON_GROUND } else { 0 }
        | if state.parked { PARKED } else { 0 }
        | if state.wrecked { H_WRECKED } else { 0 };
    let ZoneHealth { engine, tail, rotor, cockpit, structure } = state.health;
    for (i, v) in [engine, tail, rotor, cockpit, structure].into_iter().enumerate() {
        f[H_HEALTH + i] = quant(v, HEALTH_SCALE);
    }
    f[H_SPOOL] = quant(state.spool, FRACTION_SCALE);
    f[H_POWER] = quant(state.power, FRACTION_SCALE);
    f[H_THRUST] = quant(state.thrust, THRUST_SCALE);
    entity
}

/// A helicopter in world units, as drawn at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeliPose {
    pub id: u32,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub spin: Vec3,
    pub rotor_speed: f32,
    pub rotor_angle: f32,
    pub collective: f32,
    pub disc: Vec2,
    pub engine: Engine,
    pub on_ground: bool,
    pub parked: bool,
    pub wrecked: bool,
    pub health: ZoneHealth,
    pub spool: f32,
    pub power: f32,
    pub thrust: f32,
}

impl HeliPose {
    pub fn from_entity(entity: &NetEntity) -> Self {
        let f = &entity.fields;
        let (pos, rot) = get_transform(f);
        let health = |i: usize| f[H_HEALTH + i] as f32 / HEALTH_SCALE;
        Self {
            id: entity.id(),
            pos,
            rot,
            vel: Vec3::new(f[VEL] as f32, f[VEL + 1] as f32, f[VEL + 2] as f32) / VEL_SCALE,
            spin: Vec3::new(f[ANG_VEL] as f32, f[ANG_VEL + 1] as f32, f[ANG_VEL + 2] as f32) / ANG_VEL_SCALE,
            rotor_speed: f[H_ROTOR] as f32 / SPIN_SCALE,
            rotor_angle: f[H_ANGLE] as f32 / ANGLE_SCALE,
            collective: f[H_COLLECTIVE] as f32 / FRACTION_SCALE,
            disc: Vec2::new(f[H_DISC] as f32, f[H_DISC + 1] as f32) / TILT_SCALE,
            engine: engine_from_u8((f[H_FLAGS] & 3) as u8),
            on_ground: f[H_FLAGS] & ON_GROUND != 0,
            parked: f[H_FLAGS] & PARKED != 0,
            wrecked: f[H_FLAGS] & H_WRECKED != 0,
            health: ZoneHealth { engine: health(0), tail: health(1), rotor: health(2), cockpit: health(3), structure: health(4) },
            spool: f[H_SPOOL] as f32 / FRACTION_SCALE,
            power: f[H_POWER] as f32 / FRACTION_SCALE,
            thrust: f[H_THRUST] as f32 / THRUST_SCALE,
        }
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            pos: if self.pos == other.pos { other.pos } else { self.pos.lerp(other.pos, t) },
            rot: if self.rot == other.rot { other.rot } else { self.rot.slerp(other.rot, t) },
            vel: self.vel.lerp(other.vel, t),
            spin: self.spin.lerp(other.spin, t),
            rotor_speed: mix(self.rotor_speed, other.rotor_speed),
            rotor_angle: lerp_angle(self.rotor_angle, other.rotor_angle, t),
            collective: mix(self.collective, other.collective),
            disc: self.disc.lerp(other.disc, t),
            spool: mix(self.spool, other.spool),
            power: mix(self.power, other.power),
            thrust: mix(self.thrust, other.thrust),
            ..*other
        }
    }
}

pub(crate) fn show_heli(sim: &mut Sim, pose: &HeliPose, at_controls: Option<SeatedAt>) {
    if at_controls.is_some_and(|at| prs_sim::copters::heli_of(at.carrier) == Some(pose.id as usize)) {
        return;
    }
    let Some(heli) = sim.helicopters.get_mut(pose.id as usize) else { return };
    let state = &mut heli.state;
    state.pos = pose.pos;
    state.rot = pose.rot;
    state.vel = pose.vel;
    state.spin = pose.spin;
    state.rotor_speed = pose.rotor_speed;
    state.rotor_angle = pose.rotor_angle;
    state.collective = pose.collective;
    state.disc = pose.disc;
    state.engine = pose.engine;
    state.on_ground = pose.on_ground;
    state.parked = pose.parked;
    state.wrecked = pose.wrecked;
    state.health = pose.health;
    state.spool = pose.spool;
    state.power = pose.power;
    state.thrust = pose.thrust;
}

pub(crate) fn write_occupants(occupants: &Occupants, w: &mut Writer) {
    w.varu(occupants.0.len() as u64);
    for who in &occupants.0 {
        w.varu(who.map_or(0, |id| id as u64 + 1));
    }
}

pub(crate) fn read_occupants(r: &mut Reader) -> Result<Occupants, WireError> {
    let n = r.count(MAX_SEATS)?;
    let mut seats = Vec::with_capacity(n);
    for _ in 0..n {
        seats.push(r.varu32()?.checked_sub(1));
    }
    Ok(Occupants(seats))
}

/// What a player predicts: their soldier, and what they are at the controls of.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnState {
    pub soldier: Soldier,
    pub carried: Carried,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Carried {
    Nothing,
    Vehicle(u32, VehicleState),
    Heli(u32, HeliState),
}

pub(crate) fn own_state(sim: &Sim, id: SoldierId) -> OwnState {
    let soldier = sim.soldiers[id].clone();
    let carried = match soldier.seat.filter(|_| sim.is_at_controls(id)) {
        None => Carried::Nothing,
        Some(at) => match prs_sim::copters::heli_of(at.carrier) {
            Some(h) => Carried::Heli(h as u32, sim.helicopters[h].state),
            None => Carried::Vehicle(at.carrier, sim.vehicles[at.carrier as usize].clone()),
        },
    };
    OwnState { soldier, carried }
}

pub(crate) fn apply_own(sim: &mut Sim, id: SoldierId, own: &OwnState) {
    sim.soldiers[id].clone_from(&own.soldier);
    match &own.carried {
        Carried::Nothing => {}
        Carried::Vehicle(v, state) => {
            if let Some(vehicle) = sim.vehicles.get_mut(*v as usize) {
                vehicle.clone_from(state);
            }
        }
        Carried::Heli(h, state) => {
            if let Some(heli) = sim.helicopters.get_mut(*h as usize) {
                heli.state = *state;
            }
        }
    }
}

fn each_vehicle_float(v: &mut VehicleState, f: &mut dyn FnMut(&mut f32)) {
    let VehicleState { kind: _, pos, rot, vel, ang_vel, wheels, engine, steer, occupants: _, damage, asleep: _, still_for, home, abandoned_for, lift_stall, was_over: _ } = v;
    let EngineState { rpm, gear: _, shift_left, load, running: _, manual: _ } = engine;
    let Damage { hull, engine: engine_health, burning: _, wrecked: _, wreck_age } = damage;
    for axis in 0..3 {
        f(&mut pos[axis]);
        f(&mut vel[axis]);
        f(&mut ang_vel[axis]);
        f(&mut home.pos[axis]);
    }
    let mut quat = |q: &mut Quat| {
        let mut a = q.to_array();
        a.iter_mut().for_each(&mut *f);
        *q = Quat::from_array(a);
    };
    quat(rot);
    quat(&mut home.rot);
    for value in [steer, rpm, shift_left, load, hull, engine_health, wreck_age, still_for, abandoned_for, lift_stall] {
        f(value);
    }
    for wheel in wheels {
        let WheelState { compression, spin, angle, on_ground: _, surface: _, load, slip, tyre } = wheel;
        for value in [compression, spin, angle, load, slip, tyre] {
            f(value);
        }
    }
}

fn vehicle_floats(v: &VehicleState) -> Vec<f32> {
    let mut copy = v.clone();
    let mut out = Vec::new();
    each_vehicle_float(&mut copy, &mut |value| out.push(*value));
    out
}

fn write_vehicle_discrete(v: &VehicleState, w: &mut Writer) {
    w.u8(v.kind.0);
    w.u8(v.wheels.len() as u8);
    for wheel in &v.wheels {
        w.u8(wheel.on_ground as u8);
        w.u8(surface_to_u8(wheel.surface));
    }
    w.u8(v.engine.gear as u8);
    w.u8(v.engine.running as u8 | (v.asleep as u8) << 1 | (v.damage.wrecked as u8) << 2 | (v.engine.manual as u8) << 3 | (v.was_over as u8) << 4);
    w.u8(v.damage.burning.is_some() as u8);
    if let Some(burning) = v.damage.burning {
        w.f32(burning);
    }
    write_occupants(&v.occupants, w);
}

fn read_vehicle_discrete(v: &mut VehicleState, r: &mut Reader) -> Result<(), WireError> {
    v.kind = VehicleKind(r.u8()?);
    let wheels = r.u8()? as usize;
    v.wheels.resize(wheels, WheelState::default());
    for wheel in &mut v.wheels {
        wheel.on_ground = r.u8()? != 0;
        wheel.surface = surface_from_u8(r.u8()?)?;
    }
    v.engine.gear = r.u8()? as i8;
    let flags = r.u8()?;
    v.engine.running = flags & 1 != 0;
    v.asleep = flags & 2 != 0;
    v.damage.wrecked = flags & 4 != 0;
    v.engine.manual = flags & 8 != 0;
    v.was_over = flags & 16 != 0;
    v.damage.burning = if r.u8()? != 0 { Some(r.f32()?) } else { None };
    v.occupants = read_occupants(r)?;
    Ok(())
}

fn discrete_vehicle_bytes(v: &VehicleState) -> Vec<u8> {
    let mut w = Writer::default();
    write_vehicle_discrete(v, &mut w);
    w.buf
}

fn blank_vehicle() -> VehicleState {
    let frame = prs_sim::seats::Frame { pos: Vec3::ZERO, rot: Quat::from_array([0.0; 4]) };
    VehicleState {
        kind: VehicleKind(0),
        pos: Vec3::ZERO,
        rot: frame.rot,
        vel: Vec3::ZERO,
        ang_vel: Vec3::ZERO,
        wheels: Vec::new(),
        engine: EngineState::default(),
        steer: 0.0,
        occupants: Occupants(Vec::new()),
        damage: Damage { hull: 0.0, engine: 0.0, burning: None, wrecked: false, wreck_age: 0.0 },
        asleep: false,
        still_for: 0.0,
        home: frame,
        abandoned_for: 0.0,
        lift_stall: 0.0,
        was_over: false,
    }
}

fn each_heli_float(h: &mut HeliState, f: &mut dyn FnMut(&mut f32)) {
    let HeliState {
        pos,
        vel,
        rot,
        spin,
        rotor_speed,
        rotor_angle,
        engine: _,
        spool,
        power,
        collective,
        pedal,
        pedal_trim,
        cyclic,
        disc,
        induced,
        vortex,
        thrust,
        buffet,
        skids: _,
        on_ground: _,
        parked: _,
        health,
        wrecked: _,
    } = h;
    for axis in 0..3 {
        f(&mut pos[axis]);
        f(&mut vel[axis]);
        f(&mut spin[axis]);
    }
    let mut a = rot.to_array();
    a.iter_mut().for_each(&mut *f);
    *rot = Quat::from_array(a);
    f(&mut cyclic.x);
    f(&mut cyclic.y);
    f(&mut disc.x);
    f(&mut disc.y);
    let ZoneHealth { engine, tail, rotor, cockpit, structure } = health;
    for value in [
        rotor_speed, rotor_angle, spool, power, collective, pedal, pedal_trim, induced, vortex, thrust, buffet, engine,
        tail, rotor, cockpit, structure,
    ] {
        f(value);
    }
}

fn heli_floats(h: &HeliState) -> Vec<f32> {
    let mut copy = *h;
    let mut out = Vec::new();
    each_heli_float(&mut copy, &mut |value| out.push(*value));
    out
}

fn blank_heli() -> HeliState {
    HeliState {
        pos: Vec3::ZERO,
        vel: Vec3::ZERO,
        rot: Quat::from_array([0.0; 4]),
        spin: Vec3::ZERO,
        rotor_speed: 0.0,
        rotor_angle: 0.0,
        engine: Engine::Off,
        spool: 0.0,
        power: 0.0,
        collective: 0.0,
        pedal: 0.0,
        pedal_trim: 0.0,
        cyclic: Vec2::ZERO,
        disc: Vec2::ZERO,
        induced: 0.0,
        vortex: 0.0,
        thrust: 0.0,
        buffet: 0.0,
        skids: 0,
        on_ground: false,
        parked: false,
        health: ZoneHealth { engine: 0.0, tail: 0.0, rotor: 0.0, cockpit: 0.0, structure: 0.0 },
        wrecked: false,
    }
}

const NOTHING: u8 = 0;
const VEHICLE: u8 = 1;
const HELI: u8 = 2;

pub(crate) fn write_own(own: &OwnState, base: Option<&OwnState>, w: &mut Writer) {
    own::write(&own.soldier, base.map(|b| &b.soldier), w);
    match &own.carried {
        Carried::Nothing => w.u8(NOTHING),
        Carried::Vehicle(id, vehicle) => {
            w.u8(VEHICLE);
            w.varu(*id as u64);
            let base = base.and_then(|b| match &b.carried {
                Carried::Vehicle(other, state) if other == id => Some(state),
                _ => None,
            });
            // The base goes first as the count of floats depends on how many wheels.
            w.buf.extend_from_slice(&[base.is_some() as u8]);
            write_vehicle_discrete(vehicle, w);
            let blank = blank_like(vehicle);
            own::write_changed(&vehicle_floats(vehicle), &vehicle_floats(base.unwrap_or(&blank)), w);
        }
        Carried::Heli(id, heli) => {
            w.u8(HELI);
            w.varu(*id as u64);
            let base = base.and_then(|b| match &b.carried {
                Carried::Heli(other, state) if other == id => Some(state),
                _ => None,
            });
            w.buf.extend_from_slice(&[base.is_some() as u8]);
            write_heli_discrete(heli, w);
            let blank = blank_heli();
            own::write_changed(&heli_floats(heli), &heli_floats(base.unwrap_or(&blank)), w);
        }
    }
}

fn blank_like(vehicle: &VehicleState) -> VehicleState {
    let mut blank = blank_vehicle();
    blank.wheels.resize(vehicle.wheels.len(), WheelState::default());
    blank
}

fn write_heli_discrete(h: &HeliState, w: &mut Writer) {
    w.u8(engine_to_u8(h.engine));
    w.u8(h.skids);
    w.u8(h.on_ground as u8 | (h.parked as u8) << 1 | (h.wrecked as u8) << 2);
}

fn read_heli_discrete(h: &mut HeliState, r: &mut Reader) -> Result<(), WireError> {
    h.engine = engine_from_u8(r.u8()?);
    h.skids = r.u8()?;
    let flags = r.u8()?;
    h.on_ground = flags & 1 != 0;
    h.parked = flags & 2 != 0;
    h.wrecked = flags & 4 != 0;
    Ok(())
}

pub(crate) fn read_own(base: Option<&OwnState>, r: &mut Reader) -> Result<OwnState, WireError> {
    let soldier = own::read(base.map(|b| &b.soldier), r)?;
    let carried = match r.u8()? {
        NOTHING => Carried::Nothing,
        VEHICLE => {
            let id = r.varu32()?;
            let has_base = r.u8()? != 0;
            let earlier = base.and_then(|b| match &b.carried {
                Carried::Vehicle(other, state) if *other == id => Some(state),
                _ => None,
            });
            if has_base && earlier.is_none() {
                return Err(WireError::MissingBaseline);
            }
            let mut vehicle = earlier.cloned().unwrap_or_else(blank_vehicle);
            read_vehicle_discrete(&mut vehicle, r)?;
            let count = vehicle_floats(&vehicle).len();
            own::read_changed(count, r, |each| each_vehicle_float(&mut vehicle, each))?;
            Carried::Vehicle(id, vehicle)
        }
        HELI => {
            let id = r.varu32()?;
            let has_base = r.u8()? != 0;
            let earlier = base.and_then(|b| match &b.carried {
                Carried::Heli(other, state) if *other == id => Some(*state),
                _ => None,
            });
            if has_base && earlier.is_none() {
                return Err(WireError::MissingBaseline);
            }
            let mut heli = earlier.unwrap_or_else(blank_heli);
            read_heli_discrete(&mut heli, r)?;
            let count = heli_floats(&heli).len();
            own::read_changed(count, r, |each| each_heli_float(&mut heli, each))?;
            Carried::Heli(id, heli)
        }
        _ => return Err(WireError::BadTag),
    };
    Ok(OwnState { soldier, carried })
}

/// Close enough, as a share of how big the number is: engine speed and tyre load are in the thousands,
/// and the same rounding in those is a bigger number.
fn floats_agree(a: &[f32], b: &[f32], epsilon: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= epsilon * x.abs().max(1.0))
}

/// How much further apart a passenger's gun may wander than `epsilon` before it is put right. Their gun
/// is shaken by the ride, and the ride is 100 ms or so behind the server's, so these never quite agree.
const PASSENGER_SLACK: f32 = 200.0;

/// Whether two states agree closely enough to leave alone. A passenger rides what they sit in as they
/// see it, in the past, and the server has it in the present. So only what is theirs is compared.
pub(crate) fn own_matches(a: &OwnState, b: &OwnState, epsilon: f32) -> bool {
    let passenger = a.soldier.seat.is_some() && b.soldier.seat == a.soldier.seat && a.carried == Carried::Nothing;
    // On a hull the client only sees in the past, where they are on it is what has to agree. That is
    // among the floats. The world position it comes to is not.
    let on_deck = a.soldier.deck.is_some() && a.soldier.deck.map(|d| d.vehicle) == b.soldier.deck.map(|d| d.vehicle);
    // Pushing, they go where the vehicle goes, which the client sees in the past, so the same goes for them.
    let pushing = a.soldier.pushing.is_some() && a.soldier.pushing == b.soldier.pushing;
    let soldiers = if on_deck || pushing {
        let mut carried_along = b.soldier.clone();
        carried_along.body.pos = a.soldier.body.pos;
        carried_along.body.ground_normal = a.soldier.body.ground_normal;
        if pushing {
            // How their legs go follows from how they are carried, so that goes too.
            carried_along.body.vel = a.soldier.body.vel;
            carried_along.anim = a.soldier.anim;
        }
        own::matches(&a.soldier, &carried_along, epsilon * PASSENGER_SLACK)
    } else if passenger {
        let mut carried_along = b.soldier.clone();
        carried_along.body = a.soldier.body.clone();
        carried_along.anim = a.soldier.anim;
        carried_along.gun_retract = a.soldier.gun_retract;
        carried_along.yaw = a.soldier.yaw;
        carried_along.pitch = a.soldier.pitch;
        own::matches(&a.soldier, &carried_along, epsilon * PASSENGER_SLACK)
    } else {
        own::matches(&a.soldier, &b.soldier, epsilon)
    };
    let carried = match (&a.carried, &b.carried) {
        (Carried::Nothing, Carried::Nothing) => true,
        (Carried::Vehicle(x, p), Carried::Vehicle(y, q)) => {
            x == y && floats_agree(&vehicle_floats(p), &vehicle_floats(q), epsilon) && discrete_vehicle_bytes(p) == discrete_vehicle_bytes(q)
        }
        (Carried::Heli(x, p), Carried::Heli(y, q)) => {
            x == y && p.close_to(q, &NetTolerance::default())
        }
        _ => false,
    };
    soldiers && carried
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_sim::Level;

    fn soldier_on(sim: &mut Sim) -> Soldier {
        let id = sim.spawn_soldier(9);
        sim.soldiers[id].clone()
    }

    fn round_trip(own: &OwnState, base: Option<&OwnState>) -> OwnState {
        let mut w = Writer::default();
        write_own(own, base, &mut w);
        read_own(base, &mut Reader::new(&w.buf)).unwrap()
    }

    #[test]
    fn a_vehicle_comes_through_to_the_bit_for_its_driver() {
        let mut sim = Sim::new(Level::named("range").unwrap());
        let soldier = soldier_on(&mut sim);
        for _ in 0..30 {
            sim.step(&[]);
        }
        assert!(!sim.vehicles.is_empty(), "this map has vehicles to test with");
        let vehicle = sim.vehicles[0].clone();
        let own = OwnState { soldier: soldier.clone(), carried: Carried::Vehicle(0, vehicle.clone()) };
        assert_eq!(round_trip(&own, None), own);

        let mut moved = vehicle.clone();
        moved.pos.x += 1.0;
        moved.wheels[0].spin = 3.0;
        let later = OwnState { soldier, carried: Carried::Vehicle(0, moved) };
        assert_eq!(round_trip(&later, Some(&own)), later);
    }

    #[test]
    fn a_helicopter_comes_through_to_the_bit_for_its_pilot() {
        let mut sim = Sim::new(Level::named("range").unwrap());
        let soldier = soldier_on(&mut sim);
        assert!(!sim.helicopters.is_empty(), "this map has a helicopter to test with");
        let mut heli = sim.helicopters[0].state;
        heli.thrust = 123.5;
        heli.skids = 5;
        heli.engine = Engine::Starting;
        let own = OwnState { soldier: soldier.clone(), carried: Carried::Heli(0, heli) };
        assert_eq!(round_trip(&own, None), own);
        let mut later = heli;
        later.buffet = 9.0;
        let later = OwnState { soldier, carried: Carried::Heli(0, later) };
        assert_eq!(round_trip(&later, Some(&own)), later);
    }

    #[test]
    fn something_parked_is_drawn_to_the_bit_and_costs_almost_nothing() {
        let mut sim = Sim::new(Level::named("range").unwrap());
        for _ in 0..600 {
            sim.step(&[]);
        }
        let entity = capture_vehicle(&sim, 0);
        let pose = VehiclePose::from_entity(&entity);
        assert_eq!(pose.lerp(&pose, 0.37).pos.x.to_bits(), sim.vehicles[0].pos.x.to_bits());
        assert_eq!(pose.lerp(&pose, 0.37).rot, sim.vehicles[0].rot);
        sim.step(&[]);
        assert_eq!(capture_vehicle(&sim, 0), entity, "it has gone to sleep");
    }

    #[test]
    fn a_wheel_angle_goes_the_short_way_round() {
        assert!((lerp_angle(6.2, 0.1, 0.5) - 6.2916).abs() < 0.01);
    }

    #[test]
    fn a_passengers_position_is_not_a_disagreement_but_their_gun_is() {
        let mut sim = Sim::new(Level::named("range").unwrap());
        let mut soldier = soldier_on(&mut sim);
        soldier.seat = Some(SeatedAt { carrier: 0, seat: 1 });
        let a = OwnState { soldier: soldier.clone(), carried: Carried::Nothing };
        let mut b = a.clone();
        b.soldier.body.pos.x += 3.0;
        assert!(own_matches(&a, &b, 1e-3));
        b.soldier.weapon.ammo += 1;
        assert!(!own_matches(&a, &b, 1e-3));
    }
}
