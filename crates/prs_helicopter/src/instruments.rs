//! What the gauges in the cockpit read, in numbers and as needle positions. A gauge is a reading, so
//! everyone looking at one sees the same thing, and a shot out cockpit stops them.

use crate::{HeliConfig, HeliState};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// m/s along the nose.
    pub airspeed: f32,
    /// Metres above where the skids would rest if it came straight down.
    pub height: f32,
    /// m/s, up.
    pub climb: f32,
    /// Share of nominal.
    pub rotor: f32,
    /// Share of the engine's maximum.
    pub power: f32,
}

impl Reading {
    /// `height` is how far it is above the ground right now. The sim knows that and this does not.
    pub fn of(state: &HeliState, cfg: &HeliConfig, height: f32) -> Self {
        Self {
            airspeed: state.airspeed(),
            height,
            climb: state.vel.y,
            rotor: state.rotor_fraction(cfg),
            power: state.power,
        }
    }
}

/// Which way the needles go and how far, per unit of what they read.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DialConfig {
    /// Radians clockwise from straight up, per m/s on the airspeed dial.
    pub airspeed_per: f32,
    /// One whole turn of the altimeter is this many metres.
    pub altimeter_turn: f32,
    /// Radians clockwise, per m/s up. The needle rests pointing left, at level flight.
    pub climb_per: f32,
    pub climb_limit: f32,
    /// Radians clockwise per unit of rotor speed. Full speed is straight up.
    pub rotor_per: f32,
}

impl Default for DialConfig {
    fn default() -> Self {
        Self { airspeed_per: 0.075, altimeter_turn: 100.0, climb_per: 0.2, climb_limit: 15.0, rotor_per: 3.0 }
    }
}

/// Needle angles for the airspeed, height, climb and rotor dials, radians clockwise from straight up.
pub fn dial_angles(cfg: &DialConfig, reading: &Reading) -> [f32; 4] {
    [
        reading.airspeed * cfg.airspeed_per,
        reading.height / cfg.altimeter_turn * std::f32::consts::TAU,
        -std::f32::consts::FRAC_PI_2 + reading.climb.clamp(-cfg.climb_limit, cfg.climb_limit) * cfg.climb_per,
        (reading.rotor - 1.0) * cfg.rotor_per,
    ]
}
