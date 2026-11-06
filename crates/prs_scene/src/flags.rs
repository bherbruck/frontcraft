//! What a mode has standing in the world: a flag on a pole at each capture point, and each squad's
//! rally point lying on the ground. Both are there for anyone to see, on any side.

use bevy::prelude::*;
use prs_models::TeamLooks;
use prs_runtime::prs_mode::PointState;
use prs_runtime::{GameMode, SimResource};

/// Metres.
const POLE_HEIGHT: f32 = 9.0;
const POLE_WIDTH: f32 = 0.12;
/// How far above a point to start looking for what the pole stands on.
const POLE_LOOK_FROM: f32 = 60.0;
const CLOTH_SIZE: Vec3 = Vec3::new(2.4, 1.4, 0.04);
/// How far up the pole a flag is that nobody has any hold on.
const CLOTH_LOW: f32 = 1.4;
const NEUTRAL_CLOTH: Color = Color::srgb(0.86, 0.86, 0.82);
const POLE_COLOR: Color = Color::srgb(0.6, 0.6, 0.58);
const RALLY_SIZE: Vec3 = Vec3::new(0.7, 0.45, 0.5);

#[derive(Component)]
pub(crate) struct Flag(usize);

#[derive(Component)]
pub(crate) struct Cloth(usize);

/// A flag lives as long as the level does, and is hidden in any mode that has no use for it.
pub(crate) fn spawn_flags(
    mut commands: Commands,
    sim: Res<SimResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let pole = meshes.add(Cuboid::new(POLE_WIDTH, POLE_HEIGHT, POLE_WIDTH));
    let pole_material = materials.add(StandardMaterial { base_color: POLE_COLOR, perceptual_roughness: 0.9, ..default() });
    let cloth = meshes.add(Cuboid::from_size(CLOTH_SIZE));
    for (index, point) in sim.0.level.capture_points.iter().enumerate() {
        let material = materials.add(StandardMaterial { base_color: NEUTRAL_CLOTH, perceptual_roughness: 1.0, double_sided: true, ..default() });
        // A point can be right on a building, and then the flag goes on the roof so it can be seen.
        let sky = point.center + Vec3::Y * POLE_LOOK_FROM;
        let foot = sim.0.world.raycast(sky, Vec3::NEG_Y, POLE_LOOK_FROM * 2.0).map_or(point.center, |hit| sky + Vec3::NEG_Y * hit.distance);
        commands
            .spawn((Flag(index), Transform::from_translation(foot), Visibility::Hidden))
            .with_children(|flag| {
                flag.spawn((Mesh3d(pole.clone()), MeshMaterial3d(pole_material.clone()), Transform::from_xyz(0.0, POLE_HEIGHT * 0.5, 0.0)));
                flag.spawn((
                    Cloth(index),
                    Mesh3d(cloth.clone()),
                    MeshMaterial3d(material),
                    Transform::from_xyz(CLOTH_SIZE.x * 0.5, CLOTH_LOW, 0.0),
                ));
            });
    }
}

/// How high up the pole the cloth goes for how far a side has got. Up as a point is taken and down as
/// it is lost, so you can read how a point stands from a long way off.
fn cloth_height(point: &PointState, tick: u32) -> f32 {
    CLOTH_LOW + (POLE_HEIGHT - CLOTH_SIZE.y * 0.5 - CLOTH_LOW) * point.progress_at(tick)
}

pub(crate) fn update_flags(
    game: Res<GameMode>,
    sim: Res<SimResource>,
    looks: Res<TeamLooks>,
    mut flags: Query<(&Flag, &mut Visibility)>,
    mut cloths: Query<(&Cloth, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let points = &game.0.state.points;
    let tick = sim.0.tick;
    for (flag, mut visibility) in &mut flags {
        visibility.set_if_neq(if flag.0 < points.len() { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (cloth, mut transform, material) in &mut cloths {
        let Some(point) = points.get(cloth.0) else { continue };
        transform.translation.y = cloth_height(point, tick);
        let color = point.claimant.map_or(NEUTRAL_CLOTH, |team| looks.of(team).banner);
        if materials.get(&material.0).is_some_and(|m| m.base_color != color)
            && let Some(mut wanted) = materials.get_mut(&material.0)
        {
            wanted.base_color = color;
        }
    }
}

/// A rally point on the ground, keyed by the squad and where it lies.
#[derive(Component)]
pub(crate) struct RallyBag {
    squad: u32,
    pos: Vec3,
}

/// Keeps what is drawn on the ground the same as what the mode says is there.
pub(crate) fn update_rallies(
    mut commands: Commands,
    game: Res<GameMode>,
    looks: Res<TeamLooks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mesh: Local<Option<Handle<Mesh>>>,
    drawn: Query<(Entity, &RallyBag)>,
) {
    let squads = &game.0.state.squads;
    for (entity, bag) in &drawn {
        let there = squads.iter().any(|s| s.id == bag.squad && s.rally.is_some_and(|r| r.pos == bag.pos));
        if !there {
            commands.entity(entity).despawn();
        }
    }
    for squad in squads {
        let Some(rally) = squad.rally else { continue };
        if drawn.iter().any(|(_, bag)| bag.squad == squad.id && bag.pos == rally.pos) {
            continue;
        }
        let mesh = mesh.get_or_insert_with(|| meshes.add(Cuboid::from_size(RALLY_SIZE))).clone();
        let material = materials.add(StandardMaterial { base_color: looks.of(squad.team).vest, perceptual_roughness: 1.0, ..default() });
        commands.spawn((
            RallyBag { squad: squad.id, pos: rally.pos },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(rally.pos + Vec3::Y * RALLY_SIZE.y * 0.5).with_rotation(Quat::from_rotation_y(rally.yaw)),
        ));
    }
}
