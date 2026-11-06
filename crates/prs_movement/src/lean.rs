//! Leaning out sideways from the waist, toggled. It lives apart from walking because all it asks of
//! the feet is a slower pace: the capsule stays where it is, and only the eye and whatever is carried
//! along with it go over. So what fits is measured, and the lean is only ever as far as that.
//!
//! The lean follows a critically damped spring with a limit on how hard it can pull, so it starts and
//! stops gently and changing your mind halfway has no kink in it. That is what keeps the eye from ever
//! snapping, and a sudden start is the part of a sideways move that upsets a stomach.

use crate::{MoveConfig, MoveState, Stance, yaw_basis};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld};

/// Below these the spring is called at rest, so a soldier back in the centre costs no queries at all.
const REST_AMOUNT: f32 = 1e-3;
const REST_RATE: f32 = 1e-2;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LeanConfig {
    /// How far the eye goes sideways at full lean, standing and crouched. Zero turns leaning off.
    pub eye_side: f32,
    pub eye_side_crouch: f32,
    /// How far down it comes on the way, since it swings about the waist.
    pub eye_drop: f32,
    /// How quickly the spring closes in on the lean wanted, per second. As much as one a tick is used.
    pub rate: f32,
    /// Hardest it can start or stop, in leans a second a second.
    pub max_accel: f32,
    /// Fastest walk while fully leaned, metres per second.
    pub max_speed: f32,
    /// How far the eye keeps from anything, which is what a lean shortens to.
    pub clearance: f32,
    /// The least it may have if something turns up faster than the spring can pull back. Only then does
    /// the lean stop dead rather than ease, and this is still more than the camera's near plane.
    pub least_clearance: f32,
}

impl Default for LeanConfig {
    fn default() -> Self {
        Self {
            eye_side: 0.38,
            eye_side_crouch: 0.34,
            eye_drop: 0.07,
            rate: 11.0,
            max_accel: 30.0,
            max_speed: 1.5,
            clearance: 0.13,
            least_clearance: 0.07,
        }
    }
}

/// Every field is public: a client has to restore all of it to replay from what the server says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lean {
    /// Which way the soldier wants to lean: -1 = left, 0 = upright, 1 = right.
    pub side: i8,
    /// How far over they are: -1 = fully left, 1 = fully right.
    pub amount: f32,
    /// How fast that is changing, per second.
    pub rate: f32,
    /// Where that puts the eye from upright: x = to the right, y = up. See [`lean_eye`].
    pub eye: Vec2,
}

impl Lean {
    /// From the upright eye to the leaned one, for someone looking this way.
    pub fn offset(&self, yaw: f32) -> Vec3 {
        yaw_basis(yaw).1 * self.eye.x + Vec3::Y * self.eye.y
    }
}

/// Where a lean puts the eye from upright: x = to the right, y = up. It swings, so the drop grows with
/// the square of the lean, and the height of the eye says how crouched this soldier is by now.
pub fn lean_eye(cfg: &MoveConfig, amount: f32, eye_height: f32) -> Vec2 {
    let crouch = ((cfg.stand_eye - eye_height) / (cfg.stand_eye - cfg.crouch_eye).max(1e-3)).clamp(0.0, 1.0);
    let side = cfg.lean.eye_side + (cfg.lean.eye_side_crouch - cfg.lean.eye_side) * crouch;
    Vec2::new(amount * side, -amount * amount * cfg.lean.eye_drop)
}

pub struct LeanInput<'a> {
    pub left_pressed: bool,
    pub right_pressed: bool,
    pub yaw: f32,
    /// Whether this soldier can hold themselves up at all.
    pub up: bool,
    /// What the carried gun leaves of a full lean, 0 to 1, given the eye's offset at full lean that way
    /// and which way it is (-1 or 1). The gun is not this crate's to know about.
    pub gun_fit: &'a dyn Fn(Vec3, f32) -> f32,
}

/// How far a lean this way fits, 0 to 1: the share to aim for, and the share not to go past.
fn eye_fit(state: &MoveState, cfg: &MoveConfig, sign: f32, yaw: f32, world: &CollisionWorld) -> (f32, f32) {
    let c = &cfg.lean;
    let full = Lean { eye: lean_eye(cfg, sign, state.eye_height), ..Lean::default() }.offset(yaw);
    let reach = full.length();
    if reach < 1e-4 {
        return (1.0, 1.0);
    }
    // A sphere the size of the clearance, swept out from the upright eye: that is inside the capsule, so
    // anything the leaned eye could end up inside lies in the way of this.
    let ball = Capsule { height: c.clearance * 2.0, radius: c.clearance };
    let start = state.eye() - Vec3::Y * c.clearance;
    match world.cast_capsule(start, ball, full / reach, reach) {
        Some(hit) => {
            let spare = (c.clearance - c.least_clearance).max(0.0);
            ((hit.distance / reach).min(1.0), ((hit.distance + spare) / reach).min(1.0))
        }
        None => (1.0, 1.0),
    }
}

/// One tick of leaning. Run it after moving, so it fits where the soldier is now. Not while prone or
/// sprinting, and sprinting cancels it. In the air the body comes upright but the choice stays, so
/// stepping off a kerb does not lose it.
pub fn step_lean(state: &mut MoveState, cfg: &MoveConfig, input: &LeanInput, world: &CollisionWorld, dt: f32) {
    let allowed = input.up && state.stance != Stance::Prone && !state.sprinting;
    let mut lean = state.lean;
    if !allowed {
        lean.side = 0;
    } else if input.left_pressed {
        lean.side = if lean.side == -1 { 0 } else { -1 };
    } else if input.right_pressed {
        lean.side = if lean.side == 1 { 0 } else { 1 };
    }
    let airborne = !state.grounded && state.coyote == 0.0;
    let wanted = if airborne { 0 } else { lean.side };
    if wanted == 0 && lean.amount == 0.0 && lean.rate == 0.0 {
        state.lean = Lean { side: lean.side, ..Lean::default() };
        return;
    }

    let target_sign = f32::from(wanted);
    let fit = (wanted != 0).then(|| eye_fit(state, cfg, target_sign, input.yaw, world));
    let target = fit.map_or(0.0, |(eye, _)| {
        let full = Lean { eye: lean_eye(cfg, target_sign, state.eye_height), ..Lean::default() }.offset(input.yaw);
        target_sign * eye.min((input.gun_fit)(full, target_sign))
    });

    // Critically damped. Held to one rate a tick, because past that a stiff spring would blow up.
    let w = cfg.lean.rate.min(1.0 / dt);
    let pull = w * w * (target - lean.amount) - 2.0 * w * lean.rate;
    lean.rate += pull.clamp(-cfg.lean.max_accel, cfg.lean.max_accel) * dt;
    lean.amount += lean.rate * dt;

    // Whatever the spring is doing, the eye does not go into a wall.
    if lean.amount != 0.0 {
        let sign = lean.amount.signum();
        let same_way = sign == target_sign;
        let (_, limit) = fit.filter(|_| same_way).unwrap_or_else(|| eye_fit(state, cfg, sign, input.yaw, world));
        if lean.amount.abs() > limit {
            lean.amount = sign * limit;
            lean.rate = 0.0;
        }
    }
    if wanted == 0 && lean.amount.abs() < REST_AMOUNT && lean.rate.abs() < REST_RATE {
        lean.amount = 0.0;
        lean.rate = 0.0;
    }
    lean.eye = lean_eye(cfg, lean.amount, state.eye_height);
    state.lean = lean;
}
