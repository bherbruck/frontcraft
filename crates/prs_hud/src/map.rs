//! The map: the ground from the level, baked once into a picture, with the capture points, your squad
//! and you on top of it. Nothing else, so it never tells you where an enemy is. The same panel serves
//! the spawn screen and M.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::LineBreak;
use prs_level::{Ground, Level, Paint};
use prs_models::TeamLooks;
use prs_runtime::prs_mode::{MatchState, PointState, SpawnAt};
use prs_runtime::{GameMode, LevelReady, LocalSoldier, SimResource};

use crate::deploy::{DeployAction, DeployButton, MapScreen, MapSlot, PointerTracked, Pointed, Screens};
use crate::theme::*;

/// Texels across. Enough to make out a building at the size the map is shown.
const TEXELS: usize = 512;
/// Metres of height between contour lines.
const CONTOUR: f32 = 5.0;
/// Solids shorter than this do not show from above, and trees are this many metres across.
const MIN_SOLID_HEIGHT: f32 = 0.8;
const TREE_SHADE: [f32; 3] = [0.2, 0.3, 0.14];
const SOLID_SHADE: [f32; 3] = [0.16, 0.15, 0.14];
/// Share of the smaller window side the panel takes on M, and where it sits. On the spawn screen it
/// fills the space the screen leaves it.
const PANEL_VMIN: f32 = 86.0;
const PANEL_TOP_VMIN: f32 = 7.0;
const PANEL_LEFT_VMIN: f32 = 3.0;
const MEMBER_SIZE: f32 = 8.0;
const YOU_SIZE: f32 = 9.0;
/// How far from its mark a label starts, in pixels.
const LABEL_OFFSET: f32 = 6.0;
/// Most squad members drawn, and most capture points. A mod with more would want a different screen.
const MAX_MEMBER_MARKS: usize = 16;
const MAX_POINT_MARKS: usize = 16;

pub(crate) struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel)
            .add_systems(Update, (bake_map.run_if(resource_added::<LevelReady>), place_panel, update_marks, update_spawn_marks).chain().after(PointerTracked));
    }
}

#[derive(Component)]
struct MapPanel;
#[derive(Component)]
struct PointMark(usize);
/// A point's name, and under it how it stands.
#[derive(Component)]
struct PointLabel(usize);
#[derive(Component)]
struct PointName;
#[derive(Component)]
struct PointStatus;
#[derive(Component)]
struct MemberMark;
#[derive(Component)]
struct YouMark;
/// Somewhere to come in, which is clicked to choose it.
#[derive(Component)]
struct SpawnMark(SpawnAt);
#[derive(Component)]
struct SpawnLabel;

fn label_text(size: f32, color: Color) -> impl Bundle {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), TEXT_SHADOW, TextLayout::linebreak(LineBreak::NoWrap))
}

/// Starts out with nothing, since the picture has to wait for the level.
fn spawn_panel(mut commands: Commands) {
    commands
        .spawn((
            MapPanel,
            GlobalZIndex(5),
            Visibility::Hidden,
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::VMin(PANEL_LEFT_VMIN),
                top: Val::VMin(PANEL_TOP_VMIN),
                width: Val::VMin(PANEL_VMIN),
                height: Val::VMin(PANEL_VMIN),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.2, 0.22, 0.16)),
        ))
        .with_children(|panel| {
            for index in 0..MAX_POINT_MARKS {
                panel.spawn((
                    PointMark(index),
                    Node { position_type: PositionType::Absolute, border: UiRect::all(px(1)), border_radius: BorderRadius::MAX, ..default() },
                    BackgroundColor(NEUTRAL),
                    BorderColor::all(NEUTRAL),
                    Visibility::Hidden,
                ));
                panel
                    .spawn((PointLabel(index), Node { position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, ..default() }, Visibility::Hidden))
                    .with_children(|label| {
                        label.spawn((PointName, label_text(TYPE_SMALL, INK)));
                        label.spawn((PointStatus, label_text(TYPE_LABEL, WARN)));
                    });
            }
            for _ in 0..MAX_MEMBER_MARKS {
                panel.spawn((
                    MemberMark,
                    Node { position_type: PositionType::Absolute, width: px(MEMBER_SIZE), height: px(MEMBER_SIZE), margin: UiRect::all(px(-MEMBER_SIZE * 0.5)), border: UiRect::all(px(1)), ..default() },
                    BackgroundColor(NEUTRAL),
                    BorderColor::all(MARK_EDGE),
                    Visibility::Hidden,
                ));
            }
            for (at, action) in [(SpawnAt::Base, DeployAction::Base), (SpawnAt::Rally, DeployAction::Rally)] {
                panel
                    .spawn((
                        SpawnMark(at),
                        DeployButton(action),
                        Pointed::default(),
                        Node { position_type: PositionType::Absolute, border: UiRect::all(px(2)), ..default() },
                        BackgroundColor(NEUTRAL),
                        BorderColor::all(MARK_EDGE),
                        Visibility::Hidden,
                    ))
                    .with_child((
                        SpawnLabel,
                        label_text(TYPE_SMALL, INK),
                        Node { position_type: PositionType::Absolute, top: px(-2), ..default() },
                    ));
            }
            // You, with a bar on the front that shows which way you are looking.
            panel
                .spawn((
                    YouMark,
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(YOU_SIZE),
                        height: px(YOU_SIZE),
                        margin: UiRect::all(px(-YOU_SIZE * 0.5)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    Visibility::Hidden,
                ))
                .with_child((
                    Node { position_type: PositionType::Absolute, left: px(YOU_SIZE * 0.5 - 1.0), top: px(-YOU_SIZE), width: px(2), height: px(YOU_SIZE), ..default() },
                    BackgroundColor(Color::WHITE),
                ));
        });
}

/// Where in the panel a point in the world is, as shares of its width and height. North is up.
pub(crate) fn map_share(level: &Level, x: f32, z: f32) -> Vec2 {
    let size = level.terrain.size.max(1.0);
    Vec2::new(x / size + 0.5, z / size + 0.5)
}

fn bake_map(sim: Res<SimResource>, mut images: ResMut<Assets<Image>>, mut panel: Query<&mut ImageNode, With<MapPanel>>) {
    let Ok(mut node) = panel.single_mut() else {
        return;
    };
    let level = &sim.0.level;
    let pixels = map_pixels(level, TEXELS);
    let image = Image::new(
        Extent3d { width: TEXELS as u32, height: TEXELS as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    node.image = images.add(image);
}

/// A muted colour for each sort of ground, since this is a map and not a photograph.
fn ground_shade(ground: Ground) -> [f32; 3] {
    match ground {
        Ground::Natural | Ground::Meadow | Ground::Pasture | Ground::Orchard => [0.45, 0.5, 0.33],
        Ground::Wheat | Ground::Stubble => [0.62, 0.57, 0.36],
        Ground::Plough => [0.46, 0.38, 0.28],
        Ground::Road | Ground::Paving => [0.62, 0.6, 0.55],
        Ground::Track | Ground::Dirt | Ground::Yard => [0.6, 0.52, 0.38],
        Ground::DitchBed | Ground::Shingle | Ground::Scrub => [0.5, 0.47, 0.38],
    }
}

/// The ground from above: triangles laid down texel by texel, then shaded by slope with contour lines
/// on, then everything standing on it.
fn map_pixels(level: &Level, n: usize) -> Vec<u8> {
    let terrain = &level.terrain;
    let scale = n as f32 / terrain.size.max(1.0);
    let half = terrain.size * 0.5;
    let mut heights = vec![f32::NAN; n * n];
    let mut shades = vec![[0.45, 0.5, 0.33]; n * n];
    for [a, b, c] in &terrain.indices {
        let corners = [*a as usize, *b as usize, *c as usize];
        let [p, q, r] = corners.map(|i| {
            let v = terrain.vertices[i];
            Vec3::new((v.x + half) * scale, v.y, (v.z + half) * scale)
        });
        let lo_x = p.x.min(q.x).min(r.x).floor().max(0.0) as usize;
        let hi_x = (p.x.max(q.x).max(r.x).ceil() as usize).min(n - 1);
        let lo_z = p.z.min(q.z).min(r.z).floor().max(0.0) as usize;
        let hi_z = (p.z.max(q.z).max(r.z).ceil() as usize).min(n - 1);
        let d = (q.z - r.z) * (p.x - r.x) + (r.x - q.x) * (p.z - r.z);
        if d.abs() < 1e-6 {
            continue;
        }
        let ground = terrain.ground.get(corners[0]).copied().unwrap_or_default();
        for j in lo_z..=hi_z {
            for i in lo_x..=hi_x {
                let (x, z) = (i as f32 + 0.5, j as f32 + 0.5);
                let w0 = ((q.z - r.z) * (x - r.x) + (r.x - q.x) * (z - r.z)) / d;
                let w1 = ((r.z - p.z) * (x - r.x) + (p.x - r.x) * (z - r.z)) / d;
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.01 && w1 >= -0.01 && w2 >= -0.01 {
                    heights[j * n + i] = w0 * p.y + w1 * q.y + w2 * r.y;
                    shades[j * n + i] = ground_shade(ground);
                }
            }
        }
    }
    let mut pixels = vec![[0.0f32; 3]; n * n];
    let height = |i: usize, j: usize| {
        let h = heights[j.min(n - 1) * n + i.min(n - 1)];
        if h.is_nan() { 0.0 } else { h }
    };
    let metres = terrain.size / n as f32;
    for j in 0..n {
        for i in 0..n {
            let here = height(i, j);
            let (east, south) = (height(i + 1, j), height(i, j + 1));
            // Lit from the north-west, as maps are.
            let light = (1.0 + ((here - east) + (here - south)) * 0.5 / metres.max(1e-3) * 0.9).clamp(0.6, 1.3);
            let crossed = (here / CONTOUR).floor() != (east / CONTOUR).floor() || (here / CONTOUR).floor() != (south / CONTOUR).floor();
            let contour = if crossed { 0.82 } else { 1.0 };
            let [r, g, b] = shades[j * n + i];
            pixels[j * n + i] = [r * light * contour, g * light * contour, b * light * contour];
        }
    }
    for solid in &level.solids {
        if solid.half.y * 2.0 < MIN_SOLID_HEIGHT || matches!(solid.paint, Paint::FloorTile | Paint::Packed) {
            continue;
        }
        let right = solid.rotation * Vec3::X;
        let ahead = solid.rotation * Vec3::Z;
        let reach = (Vec2::new(solid.half.x, solid.half.z).length() * scale).ceil() as i32;
        let cx = ((solid.center.x + half) * scale) as i32;
        let cz = ((solid.center.z + half) * scale) as i32;
        for j in (cz - reach).max(0)..=(cz + reach).min(n as i32 - 1) {
            for i in (cx - reach).max(0)..=(cx + reach).min(n as i32 - 1) {
                let offset = Vec3::new((i as f32 + 0.5) / scale - half - solid.center.x, 0.0, (j as f32 + 0.5) / scale - half - solid.center.z);
                if offset.dot(right).abs() <= solid.half.x && offset.dot(ahead).abs() <= solid.half.z {
                    pixels[j as usize * n + i as usize] = SOLID_SHADE;
                }
            }
        }
    }
    for tree in &level.trees {
        let radius = (tree.radius * 0.7 * scale).max(1.0) as i32;
        let cx = ((tree.base.x + half) * scale) as i32;
        let cz = ((tree.base.z + half) * scale) as i32;
        for j in (cz - radius).max(0)..=(cz + radius).min(n as i32 - 1) {
            for i in (cx - radius).max(0)..=(cx + radius).min(n as i32 - 1) {
                if (i - cx).pow(2) + (j - cz).pow(2) <= radius * radius {
                    pixels[j as usize * n + i as usize] = TREE_SHADE;
                }
            }
        }
    }
    pixels.iter().flat_map(|&[r, g, b]| [to_byte(r), to_byte(g), to_byte(b), 255]).collect()
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

/// The biggest square that fits where the screen that is up leaves room for the map, in the middle of it.
fn place_panel(
    screens: Res<Screens>,
    slots: Query<(&MapSlot, &ComputedNode, &UiGlobalTransform)>,
    mut panel: Query<(&mut Node, &mut Visibility), With<MapPanel>>,
) {
    let Ok((mut node, mut visibility)) = panel.single_mut() else {
        return;
    };
    let shown = screens.deploy || screens.map;
    visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    let screen = if screens.deploy { MapScreen::Deploy } else { MapScreen::Overview };
    let room = slots.iter().find(|(slot, node, _)| slot.0 == screen && node.size().min_element() > 0.0);
    // Before the first layout there is nowhere yet, and it goes where it went before there were screens.
    let (left, top, side) = match room {
        Some((_, slot, transform)) => {
            let scale = slot.inverse_scale_factor;
            let size = slot.size() * scale;
            let center = transform.translation * scale;
            let side = size.min_element();
            (px(center.x - side * 0.5), px(center.y - side * 0.5), px(side))
        }
        None => (Val::VMin(PANEL_LEFT_VMIN), Val::VMin(PANEL_TOP_VMIN), Val::VMin(PANEL_VMIN)),
    };
    if node.left != left || node.top != top || node.width != side {
        node.left = left;
        node.top = top;
        node.width = side;
        node.height = side;
    }
}

/// A word on how a point stands for your side, if there is anything to say.
pub(crate) fn point_status(state: &MatchState, index: usize, point: &PointState, team: u8) -> Option<&'static str> {
    let enemy_inside = point.owner.is_some_and(|owner| point.inside[1 - owner as usize] > 0);
    let falling = point.owner.is_some() && point.rate < 0.0;
    if enemy_inside || falling {
        return Some(if point.owner == Some(team) { "UNDER ATTACK" } else { "ATTACKED" });
    }
    if point.owner.is_none() && point.rate > 0.0 {
        return Some("BEING TAKEN");
    }
    (state.attack_target(team) == Some(index)).then_some("NEXT")
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_marks(
    screens: Res<Screens>,
    sim: Res<SimResource>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    looks: Res<TeamLooks>,
    mut points: Query<(&PointMark, &mut Node, &mut BackgroundColor, &mut BorderColor, &mut Visibility), (Without<PointLabel>, Without<MemberMark>, Without<YouMark>)>,
    mut labels: Query<(&PointLabel, &mut Node, &mut Visibility, &Children), (Without<PointMark>, Without<MemberMark>, Without<YouMark>)>,
    mut words: Query<(&mut Text, &mut TextColor, Has<PointName>)>,
    mut members: Query<(&mut Node, &mut BackgroundColor, &mut Visibility), (With<MemberMark>, Without<PointMark>, Without<PointLabel>, Without<YouMark>)>,
    mut you: Query<(&mut Node, &mut UiTransform, &mut Visibility), (With<YouMark>, Without<PointMark>, Without<PointLabel>, Without<MemberMark>)>,
) {
    if !(screens.deploy || screens.map) {
        return;
    }
    let (sim, state) = (&sim.0, &game.0.state);
    let level = &sim.level;
    let me = &sim.soldiers[local.0];
    let size = level.terrain.size.max(1.0);
    let team = state.player(local.0).map_or(me.team, |p| p.team);
    let next = state.attack_target(team);
    for (mark, mut node, mut background, mut border, mut visibility) in &mut points {
        let (Some(point), Some(zone)) = (state.points.get(mark.0), level.capture_points.get(mark.0)) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let at = map_share(level, zone.center.x, zone.center.z);
        let across = zone.radius * 2.0 / size;
        node.left = percent((at.x - across * 0.5) * 100.0);
        node.top = percent((at.y - across * 0.5) * 100.0);
        node.width = percent(across * 100.0);
        node.height = percent(across * 100.0);
        let color = point.claimant.map_or(NEUTRAL, |t| looks.of(t).banner);
        background.0 = color.with_alpha(0.25 + 0.5 * point.progress_at(sim.tick));
        let width = if next == Some(mark.0) || point_status(state, mark.0, point, team).is_some() { TARGET_RING } else { 1.0 };
        let edge = match point_status(state, mark.0, point, team) {
            Some("NEXT") => Color::WHITE,
            Some(_) => WARN,
            None => color.with_alpha(0.6),
        };
        node.border = UiRect::all(px(width));
        *border = BorderColor::all(edge);
    }
    for (label, mut node, mut visibility, children) in &mut labels {
        let (Some(zone), Some(point)) = (level.capture_points.get(label.0), state.points.get(label.0)) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let at = map_share(level, zone.center.x, zone.center.z);
        let across = zone.radius / size;
        node.left = percent((at.x + across) * 100.0);
        node.top = percent(at.y * 100.0);
        node.margin = UiRect::new(px(LABEL_OFFSET), px(0), px(-TYPE_SMALL * 0.75), px(0));
        let letter = state.chain.iter().position(|&i| i == label.0).map_or(String::new(), |order| format!("{}  ", (b'A' + order as u8) as char));
        let status = point_status(state, label.0, point, team);
        for &child in children {
            let Ok((mut text, mut color, is_name)) = words.get_mut(child) else { continue };
            let line = if is_name { format!("{letter}{}", zone.name) } else { status.unwrap_or("").to_string() };
            if text.0 != line {
                text.0 = line;
            }
            if !is_name {
                color.0 = if status == Some("NEXT") { INK } else { WARN };
            }
        }
    }
    let squad = state.squad_of(local.0);
    let mates = squad.map_or(&[][..], |s| &s.members[..]);
    let mut mates = mates.iter().copied().filter(|&m| m != local.0 && sim.soldiers[m].present && !sim.soldiers[m].health.is_dead());
    for (mut node, mut background, mut visibility) in &mut members {
        let Some(mate) = mates.next() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let pos = sim.soldiers[mate].body.pos;
        let at = map_share(level, pos.x, pos.z);
        node.left = percent(at.x * 100.0);
        node.top = percent(at.y * 100.0);
        background.0 = looks.of(team).banner;
    }
    for (mut node, mut transform, mut visibility) in &mut you {
        let on_the_field = me.present && !me.health.is_dead();
        visibility.set_if_neq(if on_the_field { Visibility::Inherited } else { Visibility::Hidden });
        let at = map_share(level, me.body.pos.x, me.body.pos.z);
        node.left = percent(at.x * 100.0);
        node.top = percent(at.y * 100.0);
        // Yaw 0 looks north, which is up, and turning left is positive.
        transform.rotation = Rot2::radians(-me.yaw);
    }
}

/// Your main base and your squad's rally point, to click on the spawn screen. The one chosen is bigger and
/// ringed, and the label says which. On M they show where your rally point is and nothing else.
#[allow(clippy::type_complexity)]
fn update_spawn_marks(
    screens: Res<Screens>,
    sim: Res<SimResource>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    looks: Res<TeamLooks>,
    mut marks: Query<(&SpawnMark, &Pointed, &mut Node, &mut BackgroundColor, &mut BorderColor, &mut Visibility, &Children)>,
    mut labels: Query<(&mut Text, &mut TextColor, &mut Node), (With<SpawnLabel>, Without<SpawnMark>)>,
) {
    if !(screens.deploy || screens.map) {
        return;
    }
    let (sim, state) = (&sim.0, &game.0.state);
    let level = &sim.level;
    let Some(player) = state.player(local.0) else {
        for (.., mut visibility, _) in &mut marks {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    let rally = state.squad_of(local.0).and_then(|s| s.rally);
    let banner = looks.of(player.team).banner;
    for (mark, pointed, mut node, mut background, mut border, mut visibility, children) in &mut marks {
        let (pos, name) = match mark.0 {
            SpawnAt::Base => (level.teams.get(player.team as usize).map(|t| t.base_center()), "MAIN BASE".to_string()),
            SpawnAt::Rally => (rally.map(|r| r.pos), rally.map_or(String::new(), |r| format!("RALLY  {} left", r.spawns_left))),
        };
        let shown = pos.is_some() && (screens.deploy || mark.0 == SpawnAt::Rally);
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        let Some(pos) = pos else { continue };
        let chosen = screens.deploy && player.spawn == mark.0;
        let side = if chosen { SPAWN_MARK_CHOSEN } else { SPAWN_MARK };
        let at = map_share(level, pos.x, pos.z);
        node.left = percent(at.x * 100.0);
        node.top = percent(at.y * 100.0);
        node.width = px(side);
        node.height = px(side);
        node.margin = UiRect::all(px(-side * 0.5));
        background.0 = if pointed.hovered { banner.mix(&Color::WHITE, 0.35) } else { banner };
        *border = BorderColor::all(if chosen || pointed.hovered { Color::WHITE } else { MARK_EDGE });
        for &child in children {
            let Ok((mut text, mut color, mut label)) = labels.get_mut(child) else { continue };
            let line = if chosen { format!("{name}   SPAWN HERE") } else { name.clone() };
            if text.0 != line {
                text.0 = line;
            }
            color.0 = if chosen { Color::WHITE } else { INK };
            label.left = px(side + LABEL_OFFSET);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_is_up_and_east_is_right_and_the_middle_is_the_middle() {
        let level = Level::flat(1000.0);
        assert_eq!(map_share(&level, 0.0, 0.0), Vec2::splat(0.5));
        assert_eq!(map_share(&level, 500.0, -500.0), Vec2::new(1.0, 0.0));
    }

    #[test]
    fn the_picture_shows_the_land_and_what_stands_on_it() {
        let mut level = Level::flat(100.0);
        level.solids.push(prs_level::Solid {
            center: Vec3::new(25.0, 3.0, -25.0),
            half: Vec3::new(5.0, 3.0, 5.0),
            rotation: Quat::IDENTITY,
            paint: Paint::Brick,
        });
        let n = 64;
        let pixels = map_pixels(&level, n);
        assert_eq!(pixels.len(), n * n * 4);
        let at = |x: f32, z: f32| {
            let share = map_share(&level, x, z);
            let (i, j) = ((share.x * n as f32) as usize, (share.y * n as f32) as usize);
            pixels[(j * n + i) * 4]
        };
        assert!(at(25.0, -25.0) < at(-25.0, 25.0), "a building is darker than the ground");
        assert!(at(-25.0, 25.0) > 0, "the ground was drawn at all");
    }
}
