//! The edges of the view closing in while you are being shot at. A dark ring baked once into a small
//! texture and stretched over the screen, which costs one quad and asks nothing of WebGL2. It draws
//! under the rest of the HUD. How far it closes in comes from the view's `SuppressionLook`.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use prs_view::SuppressionLook;

const SIZE: u32 = 96;
/// From this share of the way to the edge of the texture the ring starts, and at the edge it is full.
const RING_STARTS: f32 = 0.42;
/// Below this it is not worth drawing.
const SHOWN_FROM: f32 = 0.005;

#[derive(Component)]
pub(crate) struct Vignette;

/// How dark the ring is at a point, with 0 the middle of the texture and 1 the edge.
fn ring_alpha(from_middle: f32) -> f32 {
    let t = ((from_middle - RING_STARTS) / (1.0 - RING_STARTS)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ring_image() -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let half = SIZE as f32 * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = Vec2::new(x as f32 + 0.5 - half, y as f32 + 0.5 - half) / half;
            let alpha = ring_alpha(at.length());
            data.extend_from_slice(&[6, 7, 9, (alpha * 255.0) as u8]);
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

pub(crate) fn spawn_vignette(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Vignette,
        ImageNode::new(images.add(ring_image())).with_mode(NodeImageMode::Stretch),
        Visibility::Hidden,
        GlobalZIndex(-10),
        Pickable::IGNORE,
        Node { position_type: PositionType::Absolute, ..default() },
    ));
}

pub(crate) fn update_vignette(look: Res<SuppressionLook>, mut vignette: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<Vignette>>) {
    let Ok((mut node, mut image, mut visibility)) = vignette.single_mut() else {
        return;
    };
    let shown = look.shown;
    if shown < SHOWN_FROM {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    }
    visibility.set_if_neq(Visibility::Inherited);
    // Bigger than the screen, so the ring's inside edge is what you see closing in.
    let ring = look.ring_open + (look.ring_closed - look.ring_open) * shown;
    let size = ring * 100.0;
    let edge = (100.0 - size) * 0.5;
    node.width = percent(size);
    node.height = percent(size);
    node.left = percent(edge);
    node.top = percent(edge);
    image.color = Color::srgba(1.0, 1.0, 1.0, look.darkness * shown.powf(0.6));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_the_ring_is_clear_and_it_darkens_smoothly_to_the_edge() {
        assert_eq!(ring_alpha(0.0), 0.0);
        assert_eq!(ring_alpha(RING_STARTS), 0.0);
        assert_eq!(ring_alpha(1.0), 1.0);
        let mut last = 0.0;
        for i in 0..=20 {
            let alpha = ring_alpha(i as f32 / 20.0);
            assert!(alpha >= last);
            last = alpha;
        }
    }

    #[test]
    fn at_full_suppression_the_centre_of_the_screen_stays_clear() {
        let look = SuppressionLook::default();
        // The ring is `ring_closed` screens across, so half a screen out is this far along it.
        assert_eq!(ring_alpha(0.0 / look.ring_closed), 0.0);
        assert!(ring_alpha(0.3 / look.ring_closed) == 0.0, "a third of the way out should be untouched");
        assert!(ring_alpha(1.0 / look.ring_closed) > 0.5, "the edge of the screen should be dark");
        assert!(ring_alpha(1.0 / look.ring_open) < 0.5, "and barely so when it first starts");
    }
}
