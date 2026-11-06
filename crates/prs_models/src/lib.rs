//! Low-poly models used by more than one view of the world. The same gun is drawn in your hands, in
//! everyone else's and on the ground, so what you see of a gun is what others see.

use bevy::prelude::*;

mod gun;
mod helicopter;
pub mod mesh_check;
mod soldier;
mod vehicle;

pub use gun::*;
pub use helicopter::*;
pub use soldier::*;
pub use vehicle::*;

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TeamLooks::tuned()).insert_resource(VehiclePaint::default()).add_systems(PreStartup, build_gun_models).add_systems(Update, (show_guns, update_flashes).chain());
    }
}
