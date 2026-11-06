//! Losing blood, which must never be mistaken for being shot at. Being shot at is a dark round ring and the
//! colour going out of the world. This is red, hugs the edges of the screen in a squared-off frame, and
//! beats, leaving the colour of everything else alone. The readout by the stamina bar says how much blood
//! is left and how long that lasts, which is what decides whether you can finish what you are doing.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use prs_runtime::prs_sim::health::{BleedLook, BleedSeverity, Life, rough_seconds};
use prs_runtime::{BleedShown, LocalSoldier, LocalView, SimResource};

const SIZE: u32 = 96;
/// Below this it is not worth drawing.
const SHOWN_FROM: f32 = 0.004;

#[derive(Component)]
pub(crate) struct BloodFrame;
#[derive(Component)]
pub(crate) struct BloodBar;
#[derive(Component)]
pub(crate) struct BloodBarBack;
#[derive(Component)]
pub(crate) struct BloodWords;

/// How strong the frame is at a point, with (0, 0) the middle of the texture and 1 either edge.
fn frame_alpha(look: &BleedLook, x: f32, y: f32) -> f32 {
    let square = look.edge_squareness.max(1.0);
    let from_middle = (x.abs().powf(square) + y.abs().powf(square)).powf(1.0 / square);
    let t = ((from_middle - look.edge_starts) / (1.0 - look.edge_starts).max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn frame_image(look: &BleedLook) -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let half = SIZE as f32 * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let alpha = frame_alpha(look, (x as f32 + 0.5 - half) / half, (y as f32 + 0.5 - half) / half);
            data.extend_from_slice(&[255, 255, 255, (alpha * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// What the readout says.
fn words(severity: BleedSeverity, seconds_left: Option<f32>, bandaging: f32, stopped: bool, bandages: u32) -> String {
    let name = match severity {
        BleedSeverity::None => return if stopped { "BLEEDING STOPPED".into() } else { String::new() },
        BleedSeverity::Light => "BLEEDING",
        BleedSeverity::Heavy => "BLEEDING HEAVILY",
        BleedSeverity::Critical => "BLEEDING OUT",
    };
    let left = rough_seconds(seconds_left.unwrap_or(0.0));
    if bandaging > 0.0 {
        return format!("BANDAGING {:.0}%   bleeding held", bandaging * 100.0);
    }
    let hint = if bandages > 0 { "B bandage" } else { "no bandages" };
    format!("{name}   down in {left} s   {hint}")
}

pub(crate) fn spawn_blood(mut commands: Commands, shown: Res<BleedShown>, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        BloodFrame,
        ImageNode::new(images.add(frame_image(&shown.look))).with_mode(NodeImageMode::Stretch),
        Visibility::Hidden,
        // Over the ring of being shot at, so that both can be told apart when both are there.
        GlobalZIndex(-9),
        Pickable::IGNORE,
        Node { position_type: PositionType::Absolute, ..default() },
    ));
    commands
        .spawn((
            BloodBarBack,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                bottom: px(38),
                width: px(150),
                height: px(8),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        ))
        .with_child((BloodBar, Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(crate::INK)));
    commands.spawn((
        BloodWords,
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(crate::INK),
        Visibility::Hidden,
        // On its own dark backing, since this is red words in a corner of the screen that may be red too.
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.78)),
        Node { position_type: PositionType::Absolute, left: px(28), bottom: px(50), padding: UiRect::axes(px(6), px(2)), ..default() },
    ));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update_blood(
    shown: Res<BleedShown>,
    view: Res<LocalView>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    mut frame: Query<(&mut Node, &mut ImageNode, &mut Visibility), (With<BloodFrame>, Without<BloodBarBack>)>,
    mut back: Query<&mut Visibility, (With<BloodBarBack>, Without<BloodFrame>)>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), (With<BloodBar>, Without<BloodFrame>)>,
    mut text: Query<(&mut Text, &mut TextColor, &mut Visibility), (With<BloodWords>, Without<BloodFrame>, Without<BloodBarBack>)>,
) {
    if let Ok((mut node, mut image, mut visibility)) = frame.single_mut() {
        let alpha = shown.alpha();
        if alpha < SHOWN_FROM {
            visibility.set_if_neq(Visibility::Hidden);
        } else {
            visibility.set_if_neq(Visibility::Inherited);
            let size = shown.frame.max(1.0) * 100.0;
            let edge = (100.0 - size) * 0.5;
            node.width = percent(size);
            node.height = percent(size);
            node.left = percent(edge);
            node.top = percent(edge);
            let [r, g, b] = shown.look.edge_colour;
            image.color = Color::srgba(r, g, b, alpha);
        }
    }

    let s = &view.blended;
    let max = sim.0.health_cfg.max_health.max(1.0);
    let up = s.life == Life::Up;
    let hurt = up && (s.health < max || s.bleeding);
    let [r, g, b] = shown.colour;
    let colour = if shown.severity == BleedSeverity::None { crate::INK } else { Color::srgb(r, g, b) };
    if let Ok(mut visibility) = back.single_mut() {
        visibility.set_if_neq(if hurt { Visibility::Inherited } else { Visibility::Hidden });
    }
    if let Ok((mut node, mut background)) = bar.single_mut() {
        node.width = percent((s.health / max).clamp(0.0, 1.0) * 100.0);
        background.set_if_neq(BackgroundColor(colour));
    }
    if let Ok((mut line, mut line_colour, mut line_shown)) = text.single_mut() {
        let bandages = sim.0.soldiers.get(local.0).map_or(0, |me| me.inventory.stash.bandages);
        let said = if up { words(shown.severity, shown.seconds_left, shown.bandaging, shown.stopped_for > 0.0, bandages) } else { String::new() };
        if line.0 != said {
            line.0 = said;
        }
        line_colour.set_if_neq(TextColor(colour));
        line_shown.set_if_neq(if line.0.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_the_screen_stays_clear_at_the_very_worst() {
        let look = BleedLook::default();
        let worst = look.critical.frame.min(look.heavy.frame).min(look.light.frame);
        // Half a screen out from the centre on either axis is `1 / frame` of the way along the texture.
        for (x, y) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.5, 0.5)] {
            assert_eq!(frame_alpha(&look, x / worst, y / worst), 0.0, "at {x}, {y}");
        }
        assert!(frame_alpha(&look, 1.0 / worst, 0.0) > 0.9, "and the edge is plainly red");
    }

    #[test]
    fn it_hugs_the_edges_where_the_ring_of_being_shot_at_is_round() {
        let look = BleedLook::default();
        // Along an edge the frame is as strong near the corner as in the middle of that edge.
        assert!((frame_alpha(&look, 0.95, 0.0) - frame_alpha(&look, 0.95, 0.3)).abs() < 0.1);
        // And it does not come in at the corners any more than a little.
        assert_eq!(frame_alpha(&look, 0.45, 0.45), 0.0);
    }

    #[test]
    fn a_light_bleed_barely_reaches_the_screen_and_a_critical_one_comes_right_in() {
        let look = BleedLook::default();
        let at_edge_of_screen = |frame: f32| frame_alpha(&look, 1.0 / frame, 0.0);
        assert!(at_edge_of_screen(look.light.frame) < at_edge_of_screen(look.heavy.frame));
        assert!(at_edge_of_screen(look.heavy.frame) < at_edge_of_screen(look.critical.frame));
        assert!(at_edge_of_screen(look.light.frame) > 0.2, "but it is there to be seen");
    }

    #[test]
    fn the_words_are_different_for_each_and_say_what_to_do() {
        let say = |severity, left| words(severity, left, 0.0, false, 2);
        assert_eq!(say(BleedSeverity::None, None), "");
        assert_eq!(say(BleedSeverity::Light, Some(88.0)), "BLEEDING   down in 90 s   B bandage");
        assert_eq!(say(BleedSeverity::Heavy, Some(23.0)), "BLEEDING HEAVILY   down in 25 s   B bandage");
        assert_eq!(say(BleedSeverity::Critical, Some(7.5)), "BLEEDING OUT   down in 8 s   B bandage");
        assert_eq!(words(BleedSeverity::Critical, Some(7.5), 0.0, false, 0), "BLEEDING OUT   down in 8 s   no bandages");
        assert_eq!(words(BleedSeverity::Heavy, Some(23.0), 0.57, false, 1), "BANDAGING 57%   bleeding held");
        assert_eq!(words(BleedSeverity::None, None, 0.0, true, 1), "BLEEDING STOPPED");
    }
}
