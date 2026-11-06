//! How big the HUD is drawn. The window's height chooses a size, so that type is about as big on a tall
//! screen as on a small one, and the player can make it bigger or smaller than that from the menu. A mod
//! can change how the size follows the window and how far a player may take it.
//!
//! Anything placed at a spot on the screen, such as a mark over someone's head, comes in window pixels and
//! goes to the HUD through [`to_hud`], because the HUD's pixels are the window's divided by this scale.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// How the size follows the window, and how far a player can change it.
#[derive(Resource, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HudScaleRules {
    /// At this window height, in logical pixels, the HUD is drawn as designed.
    pub reference_height: f32,
    /// How closely the size follows the height: 1 grows it in proportion, 0 never at all.
    pub curve: f32,
    /// The smallest and largest the window can make it.
    pub least: f32,
    pub most: f32,
    /// How far the player's own setting goes either way, and one step of it.
    pub player_least: f32,
    pub player_most: f32,
    pub player_step: f32,
}

impl Default for HudScaleRules {
    fn default() -> Self {
        Self { reference_height: 720.0, curve: 0.8, least: 0.75, most: 2.5, player_least: 0.6, player_most: 1.6, player_step: 0.1 }
    }
}

impl HudScaleRules {
    /// What a window this tall gets before the player has a say.
    pub fn for_height(&self, height: f32) -> f32 {
        (height / self.reference_height.max(1.0)).max(0.0).powf(self.curve).clamp(self.least, self.most)
    }

    /// A player's setting, kept to what the rules allow.
    pub fn held(&self, size: f32) -> f32 {
        size.clamp(self.player_least, self.player_most)
    }
}

/// The player's own setting, times what the window chooses. 1 is just what the window chooses.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct HudSize(pub f32);

pub(crate) struct ScalePlugin;

impl Plugin for ScalePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(prs_mods::tuned("hud.scale", HudScaleRules::default())).insert_resource(HudSize(1.0)).add_systems(PreUpdate, apply_scale);
    }
}

fn apply_scale(rules: Res<HudScaleRules>, size: Res<HudSize>, window: Single<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let wanted = rules.for_height(window.height()) * rules.held(size.0);
    if (scale.0 - wanted).abs() > 1e-4 {
        scale.0 = wanted;
    }
}

/// A spot in window pixels, as a spot in the HUD's pixels.
pub(crate) fn to_hud(at: Vec2, scale: &UiScale) -> Vec2 {
    at / scale.0.max(1e-3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taller_window_gets_bigger_type_within_limits() {
        let rules = HudScaleRules::default();
        assert_eq!(rules.for_height(720.0), 1.0);
        let full_hd = rules.for_height(1080.0);
        assert!(full_hd > 1.25 && full_hd < 1.5, "{full_hd}");
        assert!(rules.for_height(1440.0) > full_hd);
        assert_eq!(rules.for_height(100.0), rules.least);
        assert_eq!(rules.for_height(100_000.0), rules.most);
        // An ultrawide is as tall as a 1080p screen, so it gets the same size.
        assert_eq!(rules.for_height(1080.0), full_hd);
    }

    #[test]
    fn a_player_cannot_take_it_past_the_rules() {
        let rules = HudScaleRules::default();
        assert_eq!(rules.held(9.0), rules.player_most);
        assert_eq!(rules.held(0.0), rules.player_least);
        assert_eq!(rules.held(1.2), 1.2);
    }

    #[test]
    fn spots_on_the_window_come_to_the_hud_divided_by_the_scale() {
        assert_eq!(to_hud(Vec2::new(300.0, 150.0), &UiScale(1.5)), Vec2::new(200.0, 100.0));
    }
}
