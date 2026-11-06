//! The machine meeting the world: skids, the cushion of air under the rotor, and the things it can
//! fly into. The ground under each skid is looked up once a tick, and the stiff part runs on that.

use glam::{Quat, Vec2, Vec3};

use crate::aero;
use crate::config::HeliConfig;
use crate::damage;
use crate::{HeliEvent, HeliState, HeliWorld};

/// A plane under each skid, and how much of a cushion the rotor is over.
pub struct GroundSample {
    planes: [Option<(Vec3, Vec3)>; 4],
    /// What the ground takes off the induced flow.
    pub cushion: f32,
    /// How much more of it there is to one side: x to the right, y forward.
    pub edge: Vec2,
}

impl GroundSample {
    pub fn take(state: &HeliState, cfg: &HeliConfig, world: &HeliWorld) -> Self {
        let g = &cfg.gear;
        let mut planes = [None; 4];
        for (i, point) in g.points.iter().enumerate() {
            let at = state.pos + state.rot * *point;
            let top = at + Vec3::Y * g.probe_up;
            if let Some(hit) = world.solid.raycast(top, Vec3::NEG_Y, g.probe_up + g.probe_down) {
                planes[i] = Some((top + Vec3::NEG_Y * hit.distance, hit.normal));
            }
        }
        let r = &cfg.rotor;
        let hub = state.pos + state.rot * r.hub;
        let reach = r.radius * 2.0;
        let side = state.rot * Vec3::X * r.radius * 0.7;
        let front = state.rot * Vec3::NEG_Z * r.radius * 0.7;
        let cushion_at = |offset: Vec3| {
            world.solid.raycast(hub + offset, Vec3::NEG_Y, reach).map_or(0.0, |hit| aero::cushion_at(r, hit.distance))
        };
        let (middle, right, left, ahead, behind) =
            (cushion_at(Vec3::ZERO), cushion_at(side), cushion_at(-side), cushion_at(front), cushion_at(-front));
        Self {
            planes,
            cushion: (middle + right + left + ahead + behind) * 0.2,
            edge: Vec2::new(right - left, ahead - behind),
        }
    }
}

pub struct SkidForces {
    pub force: Vec3,
    /// In the airframe's frame.
    pub moment: Vec3,
    /// How fast a skid that had not been touching just met the ground.
    pub impact: f32,
}

/// Each skid end is a stiff damped spring, with friction that can bring it to a stop but no further.
pub fn skid_forces(state: &mut HeliState, cfg: &HeliConfig, sample: &GroundSample) -> SkidForces {
    let g = &cfg.gear;
    let mut out = SkidForces { force: Vec3::ZERO, moment: Vec3::ZERO, impact: 0.0 };
    let mut skids = 0u8;
    for (i, point) in g.points.iter().enumerate() {
        let Some((plane, normal)) = sample.planes[i] else {
            continue;
        };
        let at = state.pos + state.rot * *point;
        let depth = (plane - at).dot(normal);
        if depth <= 0.0 {
            continue;
        }
        skids |= 1 << i;
        let velocity = state.vel + state.rot * state.spin.cross(*point);
        let into = velocity.dot(normal);
        if state.skids & (1 << i) == 0 {
            out.impact = out.impact.max(-into);
        }
        let press = (g.stiffness * depth - g.damping * into).max(0.0);
        let sliding = velocity - normal * into;
        let friction = (-sliding * g.friction_rate).clamp_length_max(g.friction * press);
        let force = normal * press + friction;
        out.force += force;
        out.moment += point.cross(state.rot.inverse() * force);
    }
    state.skids = skids;
    state.on_ground = skids != 0;
    out
}

/// What landing that hard does.
pub fn touchdown(state: &mut HeliState, cfg: &HeliConfig, impact: f32, events: &mut Vec<HeliEvent>) {
    if impact <= 0.0 {
        return;
    }
    let d = &cfg.damage;
    events.push(HeliEvent::Touchdown { speed: impact });
    if impact > d.landing_safe {
        let share = (impact - d.landing_safe) / (d.landing_fatal - d.landing_safe).max(0.1);
        damage::apply(state, cfg, crate::HeliZone::Structure, share * d.structure_hp);
        events.push(HeliEvent::Crunch { point: state.pos, speed: impact });
    }
}

/// Airframe and blades against everything else. These are lines from the centre of the machine, so
/// nothing is missed however fast it is going.
pub fn strike_obstacles(state: &mut HeliState, cfg: &HeliConfig, world: &HeliWorld, dt: f32, events: &mut Vec<HeliEvent>) {
    if state.parked {
        return;
    }
    let mut touching = state.on_ground;
    for probe in &cfg.airframe.probes {
        let reach = state.rot * probe.at;
        let length = reach.length();
        let dir = reach / length;
        let Some(hit) = world.solid.raycast(state.pos, dir, length) else {
            continue;
        };
        touching = true;
        let depth = length - hit.distance;
        let normal = if hit.normal.length_squared() > 0.5 { hit.normal } else { -dir };
        let arm = dir * hit.distance;
        let speed = bounce(state, cfg, arm, normal);
        state.pos += normal * depth.min(0.2) * 0.5;
        let d = &cfg.damage;
        if speed > d.collision_safe {
            let share = (speed - d.collision_safe) / (d.collision_fatal - d.collision_safe).max(0.1);
            damage::apply(state, cfg, probe.zone, share * d.structure_hp);
            events.push(HeliEvent::Crunch { point: state.pos + arm, speed });
        }
    }
    if state.wrecked && touching {
        let keep = (1.0 - cfg.damage.wreck_settle * dt).max(0.0);
        state.vel *= keep;
        state.spin *= keep;
        if state.vel.length() < cfg.damage.wreck_park_speed && state.spin.length() < cfg.damage.wreck_park_speed {
            state.parked = true;
        }
    }
    strike_blades(state, cfg, world, dt, events);
}

/// An impulse at a point on the airframe that stops it going further in. Returns how fast it was going in.
fn bounce(state: &mut HeliState, cfg: &HeliConfig, arm: Vec3, normal: Vec3) -> f32 {
    let inverse = state.rot.inverse();
    let local_arm = inverse * arm;
    let local_normal = inverse * normal;
    let velocity = state.vel + state.rot * state.spin.cross(local_arm);
    let into = -velocity.dot(normal);
    if into <= 0.0 {
        return 0.0;
    }
    let lever = local_arm.cross(local_normal);
    let turned = (lever / cfg.inertia).cross(local_arm);
    let effective = 1.0 / cfg.mass + local_normal.dot(turned);
    let impulse = (1.0 + cfg.airframe.restitution) * into / effective;
    state.vel += normal * impulse / cfg.mass;
    state.spin += lever * impulse / cfg.inertia;
    into
}

/// Rays round the disc that turn with the rotor, so that they sweep all of it as the blades do.
fn strike_blades(state: &mut HeliState, cfg: &HeliConfig, world: &HeliWorld, dt: f32, events: &mut Vec<HeliEvent>) {
    let (r, d) = (&cfg.rotor, &cfg.damage);
    if state.health.rotor <= 0.0 || state.rotor_speed < 0.05 * r.nominal_speed {
        return;
    }
    let hub = state.pos + state.rot * r.hub;
    let normal = state.rot * state.disc_normal();
    let tilt = Quat::from_rotation_arc(Vec3::Y, normal);
    let rays = d.strike_rays.max(1);
    let share = state.rotor_speed / r.nominal_speed;
    let mut struck: Option<(Vec3, bool)> = None;
    for i in 0..rays {
        let angle = state.rotor_angle + std::f32::consts::TAU * i as f32 / rays as f32;
        let dir = tilt * Vec3::new(angle.cos(), 0.0, -angle.sin());
        if let Some(hit) = world.solid.raycast(hub, dir, r.radius) {
            struck = Some((hub + dir * hit.distance, false));
            break;
        }
        if struck.is_none()
            && let Some(hit) = world.canopy.raycast(hub, dir, r.radius)
        {
            struck = Some((hub + dir * hit.distance, true));
        }
    }
    let Some((point, canopy)) = struck else {
        return;
    };
    let loss = if canopy { d.strike_canopy } else { d.strike_solid } * share;
    damage::apply(state, cfg, crate::HeliZone::MainRotor, loss);
    state.rotor_speed *= 1.0 - d.strike_brake;
    state.spin.y -= d.strike_kick / cfg.inertia.y * dt;
    events.push(HeliEvent::RotorStrike { point, canopy });
}

/// Stops everything if it has come to rest on its skids, or a wreck has come to rest anyhow.
pub fn try_to_park(state: &mut HeliState, cfg: &HeliConfig) {
    if state.parked {
        state.vel = Vec3::ZERO;
        state.spin = Vec3::ZERO;
        return;
    }
    let g = &cfg.gear;
    let still = state.vel.length() < g.park_speed && state.spin.length() < g.park_spin;
    let light = state.thrust < g.park_thrust * cfg.mass * cfg.gravity;
    if still && light && state.skids == 0b1111 {
        state.parked = true;
        state.vel = Vec3::ZERO;
        state.spin = Vec3::ZERO;
    }
}
