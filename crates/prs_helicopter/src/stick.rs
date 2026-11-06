//! The soft stick a mouse pushes around. A mouse stays where you leave it and has no centre, so this is
//! what turns it into something with a resting place. It lives on the player's side of the wire: only
//! its position goes in a command.

use glam::Vec2;

use crate::config::StickConfig;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VirtualStick {
    pub pos: Vec2,
}

impl VirtualStick {
    /// `look` is mouse movement in radians of look: x to the right, y up. Forward on the mouse is
    /// forward on the stick, and so nose down.
    pub fn nudge(&mut self, cfg: &StickConfig, look: Vec2) {
        self.pos = (self.pos + look * cfg.per_radian).clamp(Vec2::NEG_ONE, Vec2::ONE);
    }

    pub fn relax(&mut self, cfg: &StickConfig, dt: f32) {
        self.pos *= 0.5f32.powf(dt / cfg.centre_half_life.max(1e-3));
    }

    /// As a command carries it.
    pub fn quantised(&self) -> [i8; 2] {
        [(self.pos.x * 127.0).round() as i8, (self.pos.y * 127.0).round() as i8]
    }
}
