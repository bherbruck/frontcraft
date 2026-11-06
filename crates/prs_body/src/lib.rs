//! A soldier's skeleton and how it moves, worked out from what the simulation knows.
//!
//! Nothing here draws anything or knows about Bevy, so a dedicated server or anyone watching can
//! compute the same pose: [`solve`] takes a soldier's state and gives world-space bones. The eye and
//! the gun are inputs and never outputs. The sim says where they are, and the body arranges itself
//! to fit, so animation can never move the camera or the aim.
//!
//! Bones are rigid segments. That keeps the faceted low-poly look, costs one transform per bone, and
//! needs nothing from the renderer that WebGL2 might not have.

mod anim;
mod config;
mod ik;
mod solve;

pub use anim::*;
pub use config::*;
pub use ik::{frame, two_bone};
pub use solve::*;

#[cfg(test)]
mod tests;
