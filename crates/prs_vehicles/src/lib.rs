//! Ground vehicles as plain data and one fixed step, with no idea what a soldier or a renderer is.
//!
//! A chassis is one rigid body held up by a ray per wheel. Weight transfer, body roll and rolling over
//! are not written anywhere: they come out of tyre forces applied where the tyre touches, below the
//! centre of mass. Everything tunable is in [`VehicleConfig`], so a mod can replace all of it.

mod config;
mod damage;
mod dynamics;

pub use config::*;
pub use damage::*;
pub use dynamics::*;

use glam::{Quat, Vec3};
pub use prs_seats::Frame;
use prs_seats::Occupants;

/// A row in the [`VehicleTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VehicleKind(pub u8);

/// One wheel. Nothing here depends on how it is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelState {
    /// Metres the suspension is pushed up from fully hanging, 0 to its travel.
    pub compression: f32,
    /// Radians per second, forwards positive.
    pub spin: f32,
    /// Radians, wrapped to a turn. Only for drawing and for sound.
    pub angle: f32,
    /// Whether it is on something, and what.
    pub on_ground: bool,
    pub surface: prs_collision::Surface,
    /// Newtons pressing it down, as of the last step.
    pub load: f32,
    /// How far it is sliding over its limit, 0 to 1, for sound.
    pub slip: f32,
    /// What is left of the tyre. It runs flat at zero.
    pub tyre: f32,
}

/// The engine and gearbox.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineState {
    pub rpm: f32,
    /// -1 reverse, then 1 up. Index into the gears is one less.
    pub gear: i8,
    /// Seconds left of a gear change, when no power gets through.
    pub shift_left: f32,
    /// How hard the driver is asking for power, 0 to 1. What the engine note follows.
    pub load: f32,
    pub running: bool,
    /// Someone has changed gear by hand, so the gearbox leaves it to them. See [`Engine::manual_until_stop`].
    pub manual: bool,
}

/// Everything about one vehicle that changes from tick to tick. Plain data: copying this is all there
/// is to saving it, sending it or predicting from it.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleState {
    pub kind: VehicleKind,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub wheels: Vec<WheelState>,
    pub engine: EngineState,
    /// Radians the steered wheels are turned, positive to the right.
    pub steer: f32,
    pub occupants: Occupants,
    pub damage: Damage,
    /// With nothing asked of it and nothing moving, it is left out of the simulation entirely.
    pub asleep: bool,
    pub still_for: f32,
    /// Seconds it has been left on its side or its roof with nobody in it.
    pub abandoned_for: f32,
    /// How long it has hardly moved for those lifting it.
    pub lift_stall: f32,
    /// It has been over since it last had a wheel on the ground, so the next one down is it landing.
    pub was_over: bool,
    /// Where it comes back to once it has been written off.
    pub home: Frame,
}

impl VehicleState {
    /// Parked at this frame, as if lowered gently onto whatever is there.
    pub fn new(kind: VehicleKind, cfg: &VehicleConfig, home: Frame) -> Self {
        let wheel = WheelState { tyre: cfg.damage.tyre, ..WheelState::default() };
        Self {
            kind,
            pos: home.pos,
            rot: home.rot,
            vel: Vec3::ZERO,
            ang_vel: Vec3::ZERO,
            wheels: vec![wheel; cfg.wheels.len()],
            engine: EngineState { rpm: 0.0, gear: 1, shift_left: 0.0, load: 0.0, running: false, manual: false },
            steer: 0.0,
            occupants: Occupants::empty(cfg.seats.len()),
            damage: Damage::new(&cfg.damage),
            asleep: false,
            still_for: 0.0,
            abandoned_for: 0.0,
            lift_stall: 0.0,
            was_over: false,
            home,
        }
    }

    pub fn frame(&self) -> Frame {
        Frame { pos: self.pos, rot: self.rot }
    }

    /// Metres a second along where it points, negative in reverse.
    pub fn forward_speed(&self) -> f32 {
        self.vel.dot(self.rot * Vec3::NEG_Z)
    }

    /// The middle of a wheel, in the chassis's frame.
    pub fn axle(&self, cfg: &VehicleConfig, wheel: usize) -> Vec3 {
        let (w, state) = (&cfg.wheels[wheel], &self.wheels[wheel]);
        w.mount + Vec3::NEG_Y * (w.travel - state.compression)
    }

    /// The driver's seat, if this vehicle has one and someone is in it.
    pub fn driver(&self, cfg: &VehicleConfig) -> Option<u32> {
        self.occupants.of_role(&cfg.seats, prs_seats::SeatRole::Driver)
    }
}

#[cfg(test)]
mod tests;
