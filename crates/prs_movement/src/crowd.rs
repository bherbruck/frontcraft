//! Soldiers bumping into each other. All of it is soft: a push is a little displacement that carries no
//! speed, and being in someone's way only slows you down, however hard you try. Nothing can be launched,
//! nobody can be pinned, and nobody can hold a door shut with their body, because squeezing past always
//! gets you through in the end.
//!
//! Soft also suits the network. A client has only its best guess at where anyone else is, and an error
//! in that here is an error in a gentle slope of speed, which never amounts to more than a nudge. The
//! limit is on your speed in every direction, including out the far side, so that being a little
//! earlier or later past someone than the server thought comes to a little and not a sprint's worth.
//! Someone keeping still gives way only slowly. A client seeing someone stand there sees just what the
//! server does, to the bit, so running into them is nearly the same on both.

use glam::{Vec2, Vec3};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CrowdConfig {
    /// How close two soldiers' middles come before they touch.
    pub contact: f32,
    /// Metres a second of push per metre of overlap, and the most it can come to, moving and keeping still.
    pub stiffness: f32,
    pub push_speed: f32,
    pub idle_push_speed: f32,
    /// Below this they count as keeping still.
    pub still_speed: f32,
    /// How deep into someone the limit on speed is all the way down.
    pub brake_depth: f32,
    /// The fastest anyone can go while in the middle of someone else, friend or enemy.
    /// Faster than `push_speed`, so that you always get somewhere.
    pub squeeze_friend: f32,
    pub squeeze_enemy: f32,
    /// How much two bodies have to overlap up and down before they notice.
    pub overlap_height: f32,
    /// How far ahead of where they are shown anyone is looked for, at the most, on a client. Seconds.
    pub look_ahead: f32,
}

impl Default for CrowdConfig {
    fn default() -> Self {
        Self {
            contact: 0.62,
            stiffness: 5.0,
            push_speed: 0.7,
            idle_push_speed: 0.25,
            still_speed: 0.2,
            brake_depth: 0.2,
            squeeze_friend: 1.1,
            squeeze_enemy: 0.85,
            overlap_height: 0.4,
            look_ahead: 0.3,
        }
    }
}

/// Someone else, as far as bumping into them goes.
#[derive(Clone, Copy, Debug)]
pub struct Neighbour {
    pub feet: Vec3,
    /// How fast they are going, as walking behind someone is no reason to go slower than they are.
    pub vel: Vec2,
    pub height: f32,
    pub friend: bool,
    /// Which way to go if standing in the very same spot, as there is no other way to tell.
    pub aside: Vec2,
}

/// What everyone nearby amounts to for one soldier this tick. All zero with nobody close.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Crowding {
    /// Metres a second of displacement, on top of walking, with no effect on speed.
    pub push: Vec2,
    /// The fastest to be going, in any direction. Zero when there is nobody close.
    pub limit: f32,
    /// How far into the deepest contact.
    pub depth: f32,
}

impl Crowding {
    /// `full_speed` is what anyone could be going, and `speed` is what this one is.
    pub fn add(&mut self, cfg: &CrowdConfig, feet: Vec3, height: f32, speed: f32, other: &Neighbour, full_speed: f32) {
        let overlap = (feet.y + height).min(other.feet.y + other.height) - feet.y.max(other.feet.y);
        if overlap < cfg.overlap_height {
            return;
        }
        let apart = Vec2::new(feet.x - other.feet.x, feet.z - other.feet.z);
        let distance = apart.length();
        let depth = cfg.contact - distance;
        if depth <= 0.0 {
            return;
        }
        let away = if distance > 1e-4 { apart / distance } else { other.aside };
        let most = if speed < cfg.still_speed { cfg.idle_push_speed } else { cfg.push_speed };
        self.push = (self.push + away * (depth * cfg.stiffness).min(most)).clamp_length_max(most);
        if depth > self.depth {
            let squeeze = if other.friend { cfg.squeeze_friend } else { cfg.squeeze_enemy };
            let share = (depth / cfg.brake_depth.max(1e-3)).min(1.0);
            self.depth = depth;
            let leaving = other.vel.dot(-away).max(0.0);
            self.limit = (full_speed + (squeeze - full_speed) * share).max(leaving);
        }
    }
}
