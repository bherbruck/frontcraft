//! How far the world is drawn. One number sets the far plane, through `ViewReach` the fog and the sky, and
//! how far the level's detail reaches, so a weak machine can pull it in from the pause menu and a mod can
//! push it out for helicopters.

use bevy::prelude::*;

use crate::WorldCamera;
use prs_atmosphere::ViewReach;

#[derive(Resource, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ViewDistance {
    /// Metres to the farthest chunk of the level that is still drawn.
    pub metres: f32,
    /// The range the pause menu offers, and its step.
    pub min: f32,
    pub max: f32,
    pub step: f32,
    /// How far past `metres` the far plane sits, so a chunk counted as in range is not sliced by it.
    pub far_plane_margin: f32,
}

impl Default for ViewDistance {
    fn default() -> Self {
        Self {
            // Helicopters see across a 4 km map. The web has to draw it all with fewer draw calls.
            metres: if cfg!(target_arch = "wasm32") { 2500.0 } else { 4000.0 },
            min: 400.0,
            max: 6000.0,
            step: 250.0,
            far_plane_margin: 400.0,
        }
    }
}

impl ViewDistance {
    /// Sets the distance, kept to the range offered.
    pub fn set(&mut self, metres: f32) {
        self.metres = metres.clamp(self.min.min(self.max), self.max.max(self.min));
    }

    pub fn far_plane(&self) -> f32 {
        self.metres + self.far_plane_margin
    }
}

pub(crate) fn apply_far_plane(
    distance: Res<ViewDistance>,
    mut reach: ResMut<ViewReach>,
    mut cameras: Query<&mut Projection, With<WorldCamera>>,
) {
    if !distance.is_changed() {
        return;
    }
    reach.set_if_neq(ViewReach(distance.far_plane()));
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.far = distance.far_plane();
        }
    }
}

/// Before the camera is made, so it starts with the right far plane, fog and sky.
pub(crate) fn reach_from_distance(distance: Res<ViewDistance>, mut reach: ResMut<ViewReach>) {
    *reach = ViewReach(distance.far_plane());
}
