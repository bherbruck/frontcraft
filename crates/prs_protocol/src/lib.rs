//! The one thing every side of the game agrees on: what a player asked for on a given tick.
//!
//! A `PlayerCmd` is what a client will send to a server, so nothing here may know about
//! input devices, rendering, or how the simulation interprets a command.

use glam::Vec2;

pub const TICK_HZ: f64 = 60.0;
pub const TICK_DT: f32 = 1.0 / TICK_HZ as f32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons(pub u32);

impl Buttons {
    pub const JUMP: Self = Self(1 << 0);
    /// Sprints when moving, holds breath when aiming down sights.
    pub const SPRINT: Self = Self(1 << 1);
    pub const FIRE: Self = Self(1 << 2);
    pub const ADS: Self = Self(1 << 3);
    pub const RELOAD: Self = Self(1 << 4);
    pub const FIRE_MODE: Self = Self(1 << 5);
    pub const CROUCH: Self = Self(1 << 6);
    pub const PRONE: Self = Self(1 << 7);
    /// Number keys: primary, sidearm, bandage, medkit.
    pub const SLOT_1: Self = Self(1 << 8);
    pub const SLOT_2: Self = Self(1 << 9);
    pub const SLOT_3: Self = Self(1 << 10);
    pub const SLOT_4: Self = Self(1 << 11);
    /// Whatever you are at and facing: a door, a hatch, a kit on the ground. Gets out of a seat.
    pub const USE: Self = Self(1 << 12);
    /// Pressing one leans that way, pressing it again comes back, and the other swaps side.
    pub const LEAN_LEFT: Self = Self(1 << 13);
    pub const LEAN_RIGHT: Self = Self(1 << 14);
    /// Takes out something to throw, and goes round what there is to throw.
    pub const GRENADE: Self = Self(1 << 15);
    /// Held, puts a bandage on yourself, wherever you are, with no need to take one out first.
    pub const BANDAGE: Self = Self(1 << 16);
    /// Sets the gun down on whatever is under it, or lifts it off again.
    pub const REST: Self = Self(1 << 17);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One tick of player intent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerCmd {
    pub tick: u32,
    /// x = strafe right, y = forward. Length is at most 1.
    pub wish_move: Vec2,
    /// Radians, unwrapped. View direction before recoil and sway are applied.
    pub yaw: f32,
    pub pitch: f32,
    pub buttons: Buttons,
    /// A pilot's cyclic: x to the right and y forward, from -127 to 127. Zero for everyone else.
    pub stick: [i8; 2],
}
