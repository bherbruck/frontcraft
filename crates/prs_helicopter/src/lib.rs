//! A transport helicopter's flight model, as plain data and pure functions.
//!
//! The rotor is blade elements on momentum theory rather than a lift number, so translational lift,
//! climb and descent, autorotation and rotor droop come out of one set of sums instead of being
//! written in one by one. What momentum theory cannot do (vortex ring, the ground) is added on top.
//! Knows nothing about Bevy, soldiers, or who is flying.

pub mod aero;
pub mod autopilot;
pub mod config;
pub mod damage;
mod ground;
pub mod instruments;
pub mod net;
mod stick;

pub use config::HeliConfig;
pub use damage::{HeliZone, ZoneHealth, hit_zone};
pub use instruments::{DialConfig, Reading, dial_angles};
pub use net::{NetTolerance, NetView};
pub use stick::VirtualStick;

use glam::{Quat, Vec2, Vec3};
use prs_collision::CollisionWorld;

use aero::{AirSample, RotorInput};
use ground::GroundSample;

/// One tick of what the pilot's hands and feet are doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeliInput {
    /// -1 to 1: moving the collective down or up, as W and S.
    pub collective: f32,
    /// -1 to 1: left or right pedal, nose left or right, as A and D.
    pub pedals: f32,
    /// Cyclic position: x to the right, y forward, each -1 to 1.
    pub stick: Vec2,
    /// Starts a stopped engine and stops a running one.
    pub engine_switch: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Engine {
    #[default]
    Off,
    Starting,
    Running,
    /// Shot out or crashed. Nothing brings it back.
    Dead,
}

/// Everything that changes from tick to tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeliState {
    /// The centre of mass.
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    /// Rad/s in the airframe's frame: x is nose up, y nose left, z is roll to the left.
    pub spin: Vec3,
    /// Rad/s.
    pub rotor_speed: f32,
    /// Where the blades are, radians. Bullets and things that stick up can find them.
    pub rotor_angle: f32,
    pub engine: Engine,
    /// 0 to 1: how far through its start the gas generator is.
    pub spool: f32,
    /// 0 to 1 of `max_power`: what the engine is giving right now.
    pub power: f32,
    /// 0 to 1.
    pub collective: f32,
    pub pedal: f32,
    pub pedal_trim: f32,
    /// Where the cyclic actually is.
    pub cyclic: Vec2,
    /// Tilt of the tip path plane from the mast, radians: x to the right, y forward.
    pub disc: Vec2,
    /// Air the rotor is pushing down, m/s.
    pub induced: f32,
    /// 0 to 1: how deep in its own wake the rotor is.
    pub vortex: f32,
    /// Newtons on the last physics step.
    pub thrust: f32,
    /// Buffet is a slow wander through this, and needs nothing random.
    pub buffet: f32,
    /// Which skids were touching, a bit for each. Touching for the first time is an impact.
    pub skids: u8,
    pub on_ground: bool,
    /// Standing still on its skids. Physics stops here, which is why it cannot jitter.
    pub parked: bool,
    pub health: ZoneHealth,
    pub wrecked: bool,
}

impl HeliState {
    /// On the ground at `feet` under the skids, engine off.
    pub fn parked_at(cfg: &HeliConfig, feet: Vec3, yaw: f32) -> Self {
        let settle = cfg.mass * cfg.gravity / (cfg.gear.points.len() as f32 * cfg.gear.stiffness);
        let height = -cfg.gear.points[0].y - settle;
        Self {
            pos: feet + Vec3::Y * height,
            vel: Vec3::ZERO,
            rot: Quat::from_rotation_y(yaw),
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
            skids: 0b1111,
            on_ground: true,
            parked: true,
            health: ZoneHealth::new(&cfg.damage),
            wrecked: false,
        }
    }

    /// The rotor's speed as a share of nominal.
    pub fn rotor_fraction(&self, cfg: &HeliConfig) -> f32 {
        self.rotor_speed / cfg.rotor.nominal_speed
    }

    pub fn up(&self) -> Vec3 {
        self.rot * Vec3::Y
    }

    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }

    /// Heading, radians, matching a soldier's yaw: 0 is towards -z, more is to the left.
    pub fn heading(&self) -> f32 {
        let f = self.forward();
        (-f.x).atan2(-f.z)
    }

    /// The disc's axis in the airframe's frame.
    pub fn disc_normal(&self) -> Vec3 {
        Vec3::new(self.disc.x, 1.0, -self.disc.y).normalize()
    }

    /// Speed along the nose, what an airspeed needle reads.
    pub fn airspeed(&self) -> f32 {
        self.vel.dot(self.forward()).max(0.0)
    }

    /// Whether nothing here will change until someone does something.
    pub fn asleep(&self) -> bool {
        self.parked && self.rotor_speed == 0.0 && self.spool == 0.0 && matches!(self.engine, Engine::Off | Engine::Dead)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeliEvent {
    EngineStarted,
    EngineStopped,
    EngineDied,
    TailRotorLost,
    RotorLost,
    /// The skids met the ground this fast.
    Touchdown { speed: f32 },
    /// Skids or airframe met something hard enough to hurt.
    Crunch { point: Vec3, speed: f32 },
    /// The blades hit something, and whether it was only a tree.
    RotorStrike { point: Vec3, canopy: bool },
    Wrecked,
    /// A new one on the pad in place of a wreck. Only the game can say this.
    Respawned,
}

/// What the machine can fly into. `canopy` is what only the rotor hits, like the crowns of trees.
pub struct HeliWorld<'a> {
    pub solid: &'a CollisionWorld,
    pub canopy: &'a CollisionWorld,
    /// Half the width of the map.
    pub half_size: f32,
}

/// Advances one tick. Everything the machine did is pushed on to `events`.
pub fn step(
    state: &mut HeliState,
    cfg: &HeliConfig,
    input: &HeliInput,
    world: &HeliWorld,
    dt: f32,
    events: &mut Vec<HeliEvent>,
) {
    if input.engine_switch {
        switch_engine(state, events);
    }
    if state.asleep() && *input == HeliInput::default() {
        return;
    }
    let sample = GroundSample::take(state, cfg, world);
    let substeps = cfg.substeps.max(1);
    let h = dt / substeps as f32;
    let mut impact = 0.0f32;
    for _ in 0..substeps {
        step_controls(state, cfg, input, h);
        step_engine_spool(state, cfg, h);
        let forces = step_rotor(state, cfg, &sample, h);
        if state.parked && !state.wrecked {
            state.parked = state.thrust <= cfg.gear.park_thrust * cfg.mass * cfg.gravity;
        }
        if state.parked {
            continue;
        }
        impact = impact.max(integrate(state, cfg, &sample, &forces, world.half_size, h));
    }
    ground::touchdown(state, cfg, impact, events);
    ground::strike_obstacles(state, cfg, world, dt, events);
    ground::try_to_park(state, cfg);
    damage::consequences(state, cfg, events);
}

fn switch_engine(state: &mut HeliState, events: &mut Vec<HeliEvent>) {
    match state.engine {
        Engine::Off => {
            state.engine = Engine::Starting;
            events.push(HeliEvent::EngineStarted);
        }
        Engine::Starting | Engine::Running => {
            state.engine = Engine::Off;
            events.push(HeliEvent::EngineStopped);
        }
        Engine::Dead => {}
    }
}

fn step_controls(state: &mut HeliState, cfg: &HeliConfig, input: &HeliInput, h: f32) {
    let c = &cfg.controls;
    state.collective = (state.collective + input.collective.clamp(-1.0, 1.0) * c.collective_rate * h).clamp(0.0, 1.0);
    let pedals = input.pedals.clamp(-1.0, 1.0);
    state.pedal_trim = (state.pedal_trim + pedals * c.pedal_trim_rate * h).clamp(-1.0, 1.0);
    let wanted = (state.pedal_trim + pedals * c.pedal_kick).clamp(-1.0, 1.0);
    state.pedal += (wanted - state.pedal).clamp(-c.pedal_rate * h, c.pedal_rate * h);
    let stick = input.stick.clamp(Vec2::NEG_ONE, Vec2::ONE);
    let reach = c.cyclic_rate * h;
    state.cyclic += (stick - state.cyclic).clamp(Vec2::splat(-reach), Vec2::splat(reach));
}

fn step_engine_spool(state: &mut HeliState, cfg: &HeliConfig, h: f32) {
    let e = &cfg.engine;
    match state.engine {
        Engine::Starting => {
            state.spool += h / e.start_time;
            if state.spool >= 1.0 {
                state.spool = 1.0;
                state.engine = Engine::Running;
            }
        }
        Engine::Running => state.spool = 1.0,
        Engine::Off | Engine::Dead => state.spool = (state.spool - h / e.stop_time).max(0.0),
    }
}

/// What the rotors and engine are doing to the airframe on one physics step.
struct RotorForces {
    /// Along the disc's axis, in the airframe's frame.
    thrust: Vec3,
    /// Newtons of tail rotor thrust along the airframe's x.
    tail: f32,
    /// N m the engine puts on the airframe by driving the rotor.
    reaction: f32,
}

fn step_rotor(state: &mut HeliState, cfg: &HeliConfig, sample: &GroundSample, h: f32) -> RotorForces {
    let r = &cfg.rotor;
    let air = AirSample::at(&cfg.air, state.pos.y);
    let normal = state.disc_normal();
    let hub_velocity = state.rot.inverse() * state.vel + state.spin.cross(r.hub);
    let axial = hub_velocity.dot(normal);
    let edgewise = (hub_velocity - normal * axial).length();
    let health = damage::rotor_share(state, cfg);

    let hover_induced = aero::hover_induced(r, cfg.mass * cfg.gravity, air.density);
    let cushion = sample.cushion * aero::ground_effect_fade(r, edgewise, hover_induced);
    let flow = RotorInput {
        speed: state.rotor_speed,
        collective: state.collective,
        induced: state.induced,
        axial,
        edgewise,
        density: air.density,
        cushion,
        vortex: state.vortex,
        health,
    };
    let out = aero::rotor(r, &flow);
    state.thrust = out.thrust;
    state.induced += (out.induced_target - state.induced) * (h / r.inflow_lag).min(1.0);

    let vortex_target = aero::vortex_target(r, axial, edgewise, hover_induced);
    let vortex_rate = if vortex_target > state.vortex { r.vrs_onset } else { r.vrs_recovery };
    state.vortex += (vortex_target - state.vortex) * (h / vortex_rate).min(1.0);
    state.buffet += r.vrs_buffet_rate * h;

    // The disc follows the stick, is blown back by the wind, and lags behind the airframe turning under it.
    let tip = (state.rotor_speed * r.radius).max(1.0);
    let authority = damage::cyclic_share(state, cfg);
    let wanted = Vec2::new(
        state.cyclic.x * r.max_tilt.x * authority - r.flapback * hub_velocity.x / tip + r.rate_lag * state.spin.z,
        state.cyclic.y * r.max_tilt.y * authority + r.flapback * hub_velocity.z / tip + r.rate_lag * state.spin.x,
    );
    let wanted = if state.health.rotor <= 0.0 || state.wrecked { Vec2::ZERO } else { wanted.clamp(-2.0 * r.max_tilt, 2.0 * r.max_tilt) };
    state.disc += (wanted - state.disc) * (h / r.disc_lag).min(1.0);

    let (tail, tail_power) = tail_rotor(state, cfg, air.density, hub_velocity);
    let rotor_spin = state.rotor_speed.max(0.1 * r.nominal_speed);
    let friction = cfg.engine.drive_loss * (state.rotor_speed / r.nominal_speed).clamp(0.0, 1.0);
    let wanted_power = out.power + tail_power + friction;
    let engine_torque = step_engine_power(state, cfg, wanted_power, air.density, h);
    let aero_torque = wanted_power / rotor_spin;
    state.rotor_speed = (state.rotor_speed + (engine_torque - aero_torque) / r.inertia * h)
        .clamp(0.0, r.max_speed * r.nominal_speed);
    state.rotor_angle = (state.rotor_angle + state.rotor_speed * h).rem_euclid(std::f32::consts::TAU);
    if state.health.rotor <= 0.0 || state.wrecked {
        let stop = cfg.damage.wreck_rotor_stop * r.nominal_speed * h;
        state.rotor_speed = (state.rotor_speed - stop).max(0.0);
    }

    RotorForces { thrust: normal * out.thrust, tail, reaction: engine_torque }
}

/// Moves engine power towards what the governor wants, returning the torque on the rotor shaft.
fn step_engine_power(state: &mut HeliState, cfg: &HeliConfig, wanted_power: f32, density: f32, h: f32) -> f32 {
    let (e, r) = (&cfg.engine, &cfg.rotor);
    let limit = state.spool.powf(e.spool_exponent) * damage::engine_share(state, cfg) * density / cfg.air.density;
    let error = 1.0 - state.rotor_speed / r.nominal_speed;
    let demand = (wanted_power / e.max_power + e.governor_gain * error).clamp(0.0, limit);
    let rate = if demand > state.power { e.power_up_rate } else { e.power_down_rate };
    state.power += (demand - state.power).clamp(-rate * h, rate * h);
    state.power = state.power.clamp(0.0, limit);
    if state.health.rotor <= 0.0 {
        state.power = 0.0;
    }
    (state.power * e.max_power / state.rotor_speed.max(1.0)).min(e.max_torque)
}

/// Thrust along the airframe's x and the power it takes. Nothing if it has been shot off.
fn tail_rotor(state: &HeliState, cfg: &HeliConfig, density: f32, velocity_at_hub: Vec3) -> (f32, f32) {
    if state.health.tail <= 0.0 {
        return (0.0, 0.0);
    }
    let t = &cfg.tail;
    let share = state.rotor_speed / cfg.rotor.nominal_speed;
    // The linkage knows what the engine is doing, and is geared to the rotor like the rest of it.
    let reaction = state.power * cfg.engine.max_power / state.rotor_speed.max(1.0);
    let mixed = t.torque_mix * reaction / t.at.z.abs().max(0.1);
    let pedal = -state.pedal * t.max_thrust;
    let limit = t.max_thrust * share * share;
    let at_tail = velocity_at_hub.x + state.spin.y * t.at.z;
    let thrust = (mixed + pedal).clamp(-limit, limit) - t.inflow_loss * share * at_tail;
    let area = std::f32::consts::PI * t.radius * t.radius;
    let induced = (thrust.abs() / (2.0 * density * area)).sqrt();
    (thrust, thrust.abs() * induced * cfg.rotor.induced_factor)
}

/// Sums forces and moves everything one physics step. Returns the hardest a skid just touched down.
fn integrate(
    state: &mut HeliState,
    cfg: &HeliConfig,
    sample: &GroundSample,
    rotor: &RotorForces,
    half_size: f32,
    h: f32,
) -> f32 {
    let air = AirSample::at(&cfg.air, state.pos.y);
    let body_velocity = state.rot.inverse() * state.vel;
    let hover_thrust = cfg.mass * cfg.gravity;

    let mut force = rotor.thrust;
    let mut moment = cfg.rotor.hub.cross(rotor.thrust);
    let tilt_share = (state.rotor_speed / cfg.rotor.nominal_speed).powi(2);
    moment += Vec3::new(-state.disc.y, 0.0, -state.disc.x) * cfg.rotor.hub_moment * tilt_share;

    let tail = Vec3::X * rotor.tail;
    force += tail;
    moment += cfg.tail.at.cross(tail);
    moment.y -= rotor.reaction;

    let (aero_force, aero_moment) = aero::airframe(&cfg.airframe, air.density, body_velocity, state.spin);
    force += aero_force;
    moment += aero_moment;

    // Buffet from the rotor sitting in its own wake.
    let buffet = state.vortex * cfg.rotor.vrs_buffet * (state.thrust / hover_thrust).clamp(0.0, 1.0);
    moment.x += buffet * state.buffet.sin();
    moment.z += buffet * (state.buffet * 0.73 + 1.0).sin();
    moment.x += sample.edge.y * state.thrust * cfg.rotor.ground_effect_arm * cfg.rotor.radius;
    moment.z += sample.edge.x * state.thrust * cfg.rotor.ground_effect_arm * cfg.rotor.radius;

    let mut world_force = state.rot * force + Vec3::NEG_Y * hover_thrust;
    world_force += edge_wind(state.pos, &cfg.air, half_size) * cfg.mass;
    let contact = ground::skid_forces(state, cfg, sample);
    world_force += contact.force;
    moment += contact.moment;

    state.vel += world_force / cfg.mass * h;
    state.pos += state.vel * h;
    let inertia = cfg.inertia;
    let gyro = state.spin.cross(state.spin * inertia);
    state.spin += (moment - gyro) / inertia * h;
    let turn = state.spin * h;
    state.rot = (state.rot * Quat::from_scaled_axis(turn)).normalize();
    contact.impact
}

/// Past the margin the wind pushes you back towards the map, harder the further you go.
fn edge_wind(pos: Vec3, air: &config::AirConfig, half_size: f32) -> Vec3 {
    let inner = half_size - air.edge_margin;
    let past = |x: f32| (x.abs() - inner).max(0.0) * x.signum();
    Vec3::new(-past(pos.x), 0.0, -past(pos.z)) * air.edge_push
}

#[cfg(test)]
mod tests;
