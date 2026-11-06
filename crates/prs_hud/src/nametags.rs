//! A teammate's name over their head when you look right at them from close by, as in Project Reality.
//! Never anyone else's: a name tag on an enemy would say where they are.

use bevy::prelude::*;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::{GameMode, LocalSoldier, LocalView, PlayerNames, SimResource};
use prs_view::WorldCamera;

use crate::deploy::Screens;
use crate::{INK, player_name};

/// How name tags work. A mod can turn them off or change how close and how straight you have to look.
#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
pub struct NameTags {
    pub shown: bool,
    /// Metres. Further off nobody's name shows.
    pub range: f32,
    /// How far off the middle of the screen they can be, in degrees.
    pub cone_degrees: f32,
    /// Metres above their eyes.
    pub above_eye: f32,
}

impl Default for NameTags {
    fn default() -> Self {
        Self { shown: true, range: 25.0, cone_degrees: 4.0, above_eye: 0.75 }
    }
}

pub(crate) struct NameTagPlugin;

impl Plugin for NameTagPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(prs_mods::tuned("name_tags", NameTags::default()))
            .add_systems(Startup, spawn_tag)
            .add_systems(Update, update_tag);
    }
}

#[derive(Component)]
struct NameTag;

fn spawn_tag(mut commands: Commands) {
    commands.spawn((
        NameTag,
        Visibility::Hidden,
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(INK),
        TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
        Node { position_type: PositionType::Absolute, ..default() },
    ));
}

#[allow(clippy::too_many_arguments)]
fn update_tag(
    tags: Res<NameTags>,
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    names: Res<PlayerNames>,
    screens: Res<Screens>,
    scale: Res<UiScale>,
    cameras: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    mut tag: Query<(&mut Text, &mut Node, &mut Visibility, &ComputedNode), With<NameTag>>,
) {
    let Ok((mut text, mut node, mut visibility, size)) = tag.single_mut() else { return };
    let (game, sim) = (&game.0, &sim.0);
    let looking = tags.shown && view.blended.life == Life::Up && !screens.deploy && !screens.map && !screens.board;
    let (Ok((camera, transform)), Some(me), true) = (cameras.single(), game.state.player(local.0), looking) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let eye = transform.translation();
    let forward = transform.forward().as_vec3();
    let mut best: Option<(f32, Vec3, String)> = None;
    for player in game.state.players.iter().filter(|p| p.team == me.team && p.soldier != local.0) {
        let Some(soldier) = sim.soldiers.get(player.soldier) else { continue };
        if !soldier.present || soldier.health.is_dead() {
            continue;
        }
        let top = soldier.eye() + Vec3::Y * tags.above_eye;
        let offset = soldier.eye() - eye;
        let distance = offset.length();
        if distance > tags.range || distance < 0.5 {
            continue;
        }
        let angle = forward.angle_between(offset / distance).to_degrees();
        if angle > tags.cone_degrees || best.as_ref().is_some_and(|(nearest, ..)| *nearest <= angle) {
            continue;
        }
        if sim.world.raycast(eye, offset / distance, distance - 0.3).is_some() {
            continue;
        }
        best = Some((angle, top, player_name(&names, player.soldier, player.bot)));
    }
    let Some((at, name)) = best.and_then(|(_, top, name)| Some((crate::scale::to_hud(camera.world_to_viewport(transform, top).ok()?, &scale), name))) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    if text.0 != name {
        text.0 = name;
    }
    // Centred over them, by the size it came to last time.
    let half = size.size() * size.inverse_scale_factor() / 2.0;
    node.left = px(at.x - half.x);
    node.top = px(at.y - half.y * 2.0);
    visibility.set_if_neq(Visibility::Inherited);
}
