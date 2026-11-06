//! A pilot with no eyes, flying the same controls as anyone else: collective and pedals as keys that are
//! either down or up, and the stick where the mouse would put it. Proves the machine can be flown, and
//! flies it where a scene wants it.

use glam::{Vec2, Vec3};

use crate::{HeliInput, HeliState};

/// How often the keys are looked at: about as often as someone can tap them.
pub const KEY_TICKS: u32 = 6;

#[derive(Clone, Copy, Debug)]
pub struct AutopilotGains {
    pub lean_per_speed: f32,
    /// Lean to hold a speed against drag.
    pub lean_per_speed_squared: f32,
    pub max_lean: f32,
    pub stick_per_lean: f32,
    pub stick_per_rate: f32,
    pub drift_integral: f32,
    pub drift_below: f32,
    /// Share of the time the collective key is down, per m/s of climb wanted and per m/s^2 of it arriving.
    pub collective_per_climb: f32,
    pub collective_per_climb_rate: f32,
    /// Share of the time a pedal key is down, per radian off heading and per rad/s of turn.
    pub pedal_per_heading: f32,
    pub pedal_per_yaw_rate: f32,
}

impl Default for AutopilotGains {
    fn default() -> Self {
        Self {
            lean_per_speed: 0.045,
            lean_per_speed_squared: 5.5e-5,
            max_lean: 0.35,
            stick_per_lean: 3.2,
            stick_per_rate: 1.8,
            drift_integral: 0.005,
            drift_below: 1.5,
            collective_per_climb: 0.18,
            collective_per_climb_rate: 0.14,
            pedal_per_heading: 1.6,
            pedal_per_yaw_rate: 1.2,
        }
    }
}

/// What to fly right now.
#[derive(Clone, Copy, Debug)]
pub struct Order {
    /// Metres per second over the ground, world x and z.
    pub velocity: Vec2,
    /// Metres per second, up.
    pub climb: f32,
    pub heading: f32,
    /// How much lean it may use for this order.
    pub lean_limit: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Autopilot {
    pub gains_override: Option<AutopilotGains>,
    ticks: u32,
    collective_key: f32,
    collective_owed: f32,
    pedal_key: f32,
    pedal_owed: f32,
    drift: Vec2,
    last_climb: f32,
}

impl Autopilot {
    /// One tick of hands and feet to carry out an order.
    pub fn fly(&mut self, state: &HeliState, order: &Order, dt: f32) -> HeliInput {
        let g = self.gains_override.unwrap_or_default();
        self.ticks += 1;

        // How much of the time each key should be down. They are looked at a few times a second, and
        // what could not be done one time is owed the next.
        let climb_rate = (state.vel.y - self.last_climb) / dt;
        self.last_climb = state.vel.y;
        let climb_error = order.climb - state.vel.y;
        let up = g.collective_per_climb * climb_error - g.collective_per_climb_rate * climb_rate;
        // Nothing to feel for until it starts to get light on the skids.
        let up = if state.on_ground && order.climb > 0.0 { 1.0 } else { up };
        self.collective_owed += up.clamp(-1.0, 1.0) * dt;

        // Positive pedal is nose right, and heading counts up to the left.
        let turn = wrap(order.heading - state.heading());
        let pedal = g.pedal_per_yaw_rate * state.spin.y - g.pedal_per_heading * turn;
        self.pedal_owed += pedal.clamp(-1.0, 1.0) * dt;

        if self.ticks % KEY_TICKS == 0 {
            let window = KEY_TICKS as f32 * dt;
            self.collective_key = tap(&mut self.collective_owed, window);
            self.pedal_key = tap(&mut self.pedal_owed, window);
        }

        let stick = self.stick(state, order, &g, dt);
        HeliInput { collective: self.collective_key, pedals: self.pedal_key, stick, engine_switch: false }
    }

    /// Lean to change speed, and stick to get the lean. A centred stick holds whatever attitude it has,
    /// so this has to take the lean off as well as put it on.
    fn stick(&mut self, state: &HeliState, order: &Order, g: &AutopilotGains, dt: f32) -> Vec2 {
        let forward_flat = flat(state.forward());
        let right_flat = Vec3::new(-forward_flat.z, 0.0, forward_flat.x);
        let error = Vec3::new(order.velocity.x - state.vel.x, 0.0, order.velocity.y - state.vel.z);
        let error_body = Vec2::new(error.dot(right_flat), error.dot(forward_flat));
        // The steady lean that holds it still against the tail rotor's push, and nothing to do with cruising.
        if order.velocity.length() < g.drift_below && error_body.length() < g.drift_below && !state.on_ground {
            self.drift = (self.drift + error_body * g.drift_integral * dt).clamp(Vec2::splat(-0.1), Vec2::splat(0.1));
        }
        let limit = g.max_lean.min(order.lean_limit);
        let wanted_body = Vec2::new(order.velocity.x * right_flat.x + order.velocity.y * right_flat.z, order.velocity.x * forward_flat.x + order.velocity.y * forward_flat.z);
        let against_drag = wanted_body * wanted_body.length() * g.lean_per_speed_squared;
        let lean_wanted = (error_body * g.lean_per_speed + self.drift + against_drag).clamp(Vec2::splat(-limit), Vec2::splat(limit));

        // Leans now: forward is nose down, right is right side down.
        let nose_down = -state.forward().y.asin();
        let right_down = -(state.rot * Vec3::X).y.asin();
        let lean_error = lean_wanted - Vec2::new(right_down, nose_down);
        // Rates that would increase those leans: nose down is -x, right side down is -z.
        let rate = Vec2::new(-state.spin.z, -state.spin.x);
        (lean_error * g.stick_per_lean - rate * g.stick_per_rate).clamp(Vec2::NEG_ONE, Vec2::ONE)
    }
}

/// Down for this window if more than half of it is owed.
fn tap(owed: &mut f32, window: f32) -> f32 {
    if owed.abs() < window * 0.5 {
        return 0.0;
    }
    let direction = owed.signum();
    *owed -= direction * window;
    direction
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z).normalize_or(Vec3::NEG_Z)
}

pub fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
