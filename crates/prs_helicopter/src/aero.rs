//! The sums for the rotor and the airframe in the air. Pure functions of the state of the air.

use glam::Vec3;

use crate::config::{AirConfig, AirframeConfig, RotorConfig};

/// A smallest speed to divide by, so that a stopped rotor or still air stays finite.
const LEAST_SPEED: f32 = 0.5;

pub struct AirSample {
    pub density: f32,
}

impl AirSample {
    pub fn at(air: &AirConfig, height: f32) -> Self {
        let thin = ((height - air.ceiling) / air.ceiling_fade.max(1.0)).clamp(0.0, 1.0);
        let density = air.density * (-height.max(0.0) / air.scale_height).exp();
        Self { density: density * (1.0 - thin) + 0.01 * thin }
    }
}

pub struct RotorInput {
    /// Rad/s.
    pub speed: f32,
    /// 0 to 1.
    pub collective: f32,
    /// Induced flow down through the disc, m/s, as it stands.
    pub induced: f32,
    /// Speed along the disc's axis. Going up is positive.
    pub axial: f32,
    /// Speed across the disc.
    pub edgewise: f32,
    pub density: f32,
    /// 0 is free air. What the ground takes off the induced flow.
    pub cushion: f32,
    /// How much of its thrust vortex ring is taking away, 0 to 1 of the worst.
    pub vortex: f32,
    /// Share of thrust left in shot up blades.
    pub health: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct RotorOut {
    pub thrust: f32,
    /// Watts the rotor takes from the shaft. Negative when the air is driving it.
    pub power: f32,
    pub induced_target: f32,
}

/// Blade element thrust for the flow as it stands, then what momentum says the flow should become.
pub fn rotor(r: &RotorConfig, f: &RotorInput) -> RotorOut {
    let tip = (f.speed * r.radius).max(LEAST_SPEED);
    let area = r.area();
    let solidity = r.solidity();
    let mu = f.edgewise / tip;
    let pitch = r.min_pitch + (r.max_pitch - r.min_pitch) * f.collective.clamp(0.0, 1.0);
    let inflow = (f.induced + f.axial) / tip;
    let unstalled = 0.5 * r.lift_slope * (pitch / 3.0 * (1.0 + 1.5 * mu * mu) - inflow * 0.5);

    // Past the stall the blades lose lift and only make drag.
    let past = (unstalled - r.stall_loading).max(0.0);
    let loading = unstalled.min(r.stall_loading) - past * r.stall_falloff;
    let dynamic = f.density * area * tip * tip;
    let ideal = loading * solidity * dynamic * f.health;
    let thrust = ideal * (1.0 - r.vrs_loss * f.vortex);

    let profile = dynamic * tip * solidity * r.profile_drag / 8.0 * (1.0 + 4.65 * mu * mu);
    let stalled = 1.0 + r.stall_power * past / r.stall_loading;
    let power = ideal * (r.induced_factor * f.induced + f.axial) + profile * stalled;

    let hover_squared = ideal.abs() / (2.0 * f.density * area);
    let through = f.axial + f.induced;
    let wake = (f.edgewise * f.edgewise + through * through + r.wake_floor * r.wake_floor * hover_squared)
        .sqrt()
        .max(LEAST_SPEED);
    let induced_target = ideal.signum() * hover_squared / wake * (1.0 - f.cushion);
    RotorOut { thrust, power, induced_target }
}

/// Induced flow in a free hover with this thrust.
pub fn hover_induced(r: &RotorConfig, thrust: f32, density: f32) -> f32 {
    (thrust / (2.0 * density * r.area())).sqrt()
}

/// How much of the cushion is left at this speed across the ground.
pub fn ground_effect_fade(r: &RotorConfig, edgewise: f32, hover_induced: f32) -> f32 {
    let ratio = edgewise / (r.ground_effect_fade * hover_induced);
    1.0 / (1.0 + ratio * ratio)
}

/// What the ground takes off the induced flow with the rotor this high over it (Cheeseman and Bennett).
pub fn cushion_at(r: &RotorConfig, height: f32) -> f32 {
    let ratio = r.radius / (4.0 * height.max(r.radius * 0.4));
    ratio * ratio
}

/// How deep in its own wake a rotor at these speeds would settle.
pub fn vortex_target(r: &RotorConfig, axial: f32, edgewise: f32, hover_induced: f32) -> f32 {
    let descent = -axial / hover_induced;
    let rising = smooth(r.vrs_start, r.vrs_full, descent);
    let falling = 1.0 - smooth(r.vrs_full, r.vrs_end, descent);
    let clear = smooth(r.vrs_clear_from, r.vrs_clear_at, edgewise / hover_induced);
    rising * falling.powf(0.5) * (1.0 - clear)
}

fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from).max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Force and moment on the airframe from the air it is moving through, in its own frame.
pub fn airframe(a: &AirframeConfig, density: f32, v: Vec3, spin: Vec3) -> (Vec3, Vec3) {
    let half = 0.5 * density;
    let mut force = -half * a.drag_area * v.abs() * v;
    let mut moment = -a.damping * spin;

    // Flying forwards is towards -z, and the surfaces work only with the air coming from the front.
    let forward = (-v.z).max(0.0);
    let head = forward * forward;

    let fin_arm = Vec3::Z * a.fin_arm;
    let at_fin = v + spin.cross(fin_arm);
    let slip = at_fin.x.atan2(forward.max(LEAST_SPEED)).clamp(-a.fin_stall, a.fin_stall);
    let fin = Vec3::X * (-half * head * a.fin_area * a.fin_slope * slip);
    force += fin;
    moment += fin_arm.cross(fin);

    let stab_arm = Vec3::Z * a.stabiliser_arm;
    let at_stab = v + spin.cross(stab_arm);
    let alpha = (-at_stab.y).atan2(forward.max(LEAST_SPEED)) + a.stabiliser_incidence;
    let alpha = alpha.clamp(-a.stabiliser_stall, a.stabiliser_stall);
    let stab = Vec3::Y * (half * head * a.stabiliser_area * a.stabiliser_slope * alpha);
    force += stab;
    moment += stab_arm.cross(stab);
    (force, moment)
}
