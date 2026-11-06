//! A faint line along an edge you could climb right now, so a player can tell before spending the
//! stamina on a jump that gets them nothing. It is drawn from [`Sim::climb_on_offer`], which asks the same
//! planner that a press of jump does, so whatever is lit, jump with W held climbs, and nothing else is lit.
//!
//! The planner costs a handful of rays, so it is asked a few times a second rather than every frame, and
//! the line fades in and out over that, which also keeps it calm.

use bevy::prelude::*;
use prs_runtime::prs_sim::Sim;
use prs_runtime::{LocalSoldier, SimResource};
use serde::{Deserialize, Serialize};

/// All of it is a mod's to change: `view.climb_hint.on = false` takes it away.
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct ClimbHintLook {
    pub on: bool,
    /// How far from the eye an edge can be and still be shown, metres. The planner's own reach is shorter.
    pub range: f32,
    /// How bright it is at most, 0 to 1, and its colour.
    pub strength: f32,
    pub colour: [f32; 3],
    /// Half the length of the line along the edge, and how far over the edge it is drawn.
    pub half_width: f32,
    pub lift: f32,
    /// Times a second to ask, and seconds to fade in or out.
    pub rate: f32,
    pub fade: f32,
}

impl Default for ClimbHintLook {
    fn default() -> Self {
        Self { on: true, range: 2.5, strength: 0.28, colour: [0.92, 0.9, 0.82], half_width: 0.3, lift: 0.012, rate: 8.0, fade: 0.2 }
    }
}

/// What was last found, and how far faded in it is.
#[derive(Resource, Default)]
pub(crate) struct ClimbHint {
    edge: Option<(Vec3, f32)>,
    shown: Option<(Vec3, f32)>,
    alpha: f32,
    since: f32,
}

/// Where the edge to show is: the middle of it where the hands go, and the way the climb faces.
fn offered(sim: &Sim, id: usize, range: f32) -> Option<(Vec3, f32)> {
    let climb = sim.climb_on_offer(id)?;
    let eye = sim.soldiers[id].eye();
    (climb.hands.distance(eye) <= range).then_some((climb.hands, climb.yaw))
}

pub(crate) fn draw_climb_hint(
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    look: Res<ClimbHintLook>,
    time: Res<Time>,
    mut hint: ResMut<ClimbHint>,
    mut gizmos: Gizmos,
) {
    if !look.on {
        return;
    }
    let dt = time.delta_secs();
    hint.since += dt;
    if hint.since >= 1.0 / look.rate.max(0.1) {
        hint.since = 0.0;
        hint.edge = offered(&sim.0, local.0, look.range);
    }
    if let Some(edge) = hint.edge {
        hint.shown = Some(edge);
    }
    let goal = if hint.edge.is_some() { 1.0 } else { 0.0 };
    let step = dt / look.fade.max(1e-3);
    hint.alpha += (goal - hint.alpha).clamp(-step, step);
    let Some((middle, yaw)) = hint.shown.filter(|_| hint.alpha > 0.0) else {
        return;
    };
    let across = Vec3::new(yaw.cos(), 0.0, -yaw.sin()) * look.half_width;
    let at = middle + Vec3::Y * look.lift;
    let [r, g, b] = look.colour;
    gizmos.line(at - across, at + across, Color::srgba(r, g, b, look.strength * hint.alpha));
}
