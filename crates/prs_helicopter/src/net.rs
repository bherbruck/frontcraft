//! How far apart two copies of a helicopter can be and still be the same one, for a pilot predicting
//! theirs against the server's. The wire itself is in `prs_net_sim`.

use crate::HeliState;

/// How far apart two states can be before anyone should do anything about it. Native and wasm floats
/// differ in the last bits, and these are set well above what that grows to over a replay (see
/// `small_errors_stay_small` in the tests) and well below anything anyone could see or feel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetTolerance {
    /// Metres, and m/s.
    pub pos: f32,
    pub vel: f32,
    /// Radians, and rad/s.
    pub rot: f32,
    pub spin: f32,
    /// Rad/s of rotor, and radians of where the blades are.
    pub rotor_speed: f32,
    pub rotor_angle: f32,
    /// Shares of full travel, of power and of hit points.
    pub control: f32,
    /// m/s of induced flow, share of vortex, and newtons of thrust.
    pub induced: f32,
    pub vortex: f32,
    pub thrust: f32,
}

/// How a correction to the pilot's own machine shows. Sliding the whole world past a cockpit is what makes
/// someone feel ill, so a big one is spread out over time rather than got over with.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetView {
    /// Per second, for small ones.
    pub fade_rate: f32,
    /// The fastest it fades whatever its size, m/s and rad/s.
    pub fade_speed: f32,
    pub fade_turn: f32,
    /// Bigger than this and it is a teleport after all, which nobody should see in the air.
    pub snap_distance: f32,
}

impl Default for NetView {
    fn default() -> Self {
        Self { fade_rate: 10.0, fade_speed: 12.0, fade_turn: 0.5, snap_distance: 20.0 }
    }
}

impl Default for NetTolerance {
    fn default() -> Self {
        Self {
            pos: 0.005,
            vel: 0.02,
            rot: 0.002,
            spin: 0.01,
            rotor_speed: 0.02,
            rotor_angle: 0.1,
            control: 0.002,
            induced: 0.05,
            vortex: 0.01,
            thrust: 300.0,
        }
    }
}

impl HeliState {
    /// Whether these are the same state as far as prediction need care. Discrete things have to match.
    pub fn close_to(&self, other: &Self, t: &NetTolerance) -> bool {
        // The destructure lists every field, so that a new one has to decide what it is here.
        let Self {
            pos,
            vel,
            rot,
            spin,
            rotor_speed,
            rotor_angle,
            engine,
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
            skids,
            on_ground: _,
            parked,
            health,
            wrecked,
        } = self;
        let near = |a: f32, b: f32, within: f32| (a - b).abs() <= within;
        let angle_gap = (rotor_angle - other.rotor_angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        *engine == other.engine
            && *skids == other.skids
            && *parked == other.parked
            && *wrecked == other.wrecked
            && *health == other.health
            && pos.distance(other.pos) <= t.pos
            && vel.distance(other.vel) <= t.vel
            && rot.angle_between(other.rot) <= t.rot
            && spin.distance(other.spin) <= t.spin
            && near(*rotor_speed, other.rotor_speed, t.rotor_speed)
            && angle_gap.abs() <= t.rotor_angle
            && near(*spool, other.spool, t.control)
            && near(*power, other.power, t.control)
            && near(*collective, other.collective, t.control)
            && near(*pedal, other.pedal, t.control)
            && near(*pedal_trim, other.pedal_trim, t.control)
            && cyclic.distance(other.cyclic) <= t.control
            && disc.distance(other.disc) <= t.rot
            && near(*induced, other.induced, t.induced)
            && near(*vortex, other.vortex, t.vortex)
            && near(*thrust, other.thrust, t.thrust)
            && near(*buffet, other.buffet, t.rotor_angle)
    }
}
