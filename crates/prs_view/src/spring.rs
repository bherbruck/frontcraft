//! Damped springs for camera and weapon motion. Purely visual, so they run at frame rate.

use bevy::prelude::*;

/// Sub-stepped so a slow frame cannot blow up a stiff spring.
const MAX_SUBSTEP: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: Vec3,
    pub velocity: Vec3,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub const fn new(stiffness: f32, damping: f32) -> Self {
        Self { value: Vec3::ZERO, velocity: Vec3::ZERO, stiffness, damping }
    }

    pub fn update(&mut self, target: Vec3, dt: f32) {
        let steps = (dt / MAX_SUBSTEP).ceil().clamp(1.0, 32.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            let accel = (target - self.value) * self.stiffness - self.velocity * self.damping;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
    }
}
