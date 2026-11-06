//! How the full-screen menus look: colours, sizes of type and the spacing between things, in one place.
//! Sizes are logical pixels. Team colours are not here, because they come from `TeamLooks`.

use bevy::prelude::*;

/// Text, and text that matters less.
pub(crate) const INK: Color = Color::srgb(0.93, 0.93, 0.89);
pub(crate) const INK_DIM: Color = Color::srgba(0.93, 0.93, 0.89, 0.62);
pub(crate) const INK_FAINT: Color = Color::srgba(0.93, 0.93, 0.89, 0.38);
/// Something to look at now.
pub(crate) const WARN: Color = Color::srgb(0.98, 0.72, 0.3);

/// Over the world behind a screen, and behind panels on it.
pub(crate) const SHADE: Color = Color::srgba(0.02, 0.03, 0.03, 0.7);
pub(crate) const PANEL: Color = Color::srgb(0.07, 0.08, 0.08);
pub(crate) const RULE: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);

/// Anything to click, as it is, under the pointer, held down, and the choice already made.
pub(crate) const ROW: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);
pub(crate) const ROW_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.13);
pub(crate) const ROW_PRESSED: Color = Color::srgba(1.0, 1.0, 1.0, 0.2);
pub(crate) const ROW_CHOSEN: Color = Color::srgba(1.0, 1.0, 1.0, 0.16);
/// A row that cannot be taken right now.
pub(crate) const ROW_OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.02);

/// The way into the game: ready to go, going once the wait is over, and still waiting.
pub(crate) const GO: Color = Color::srgb(0.24, 0.47, 0.25);
pub(crate) const GO_HOVER: Color = Color::srgb(0.29, 0.56, 0.3);
pub(crate) const READY: Color = Color::srgb(0.2, 0.36, 0.22);
pub(crate) const WAITING: Color = Color::srgb(0.2, 0.22, 0.21);
pub(crate) const WAITING_HOVER: Color = Color::srgb(0.26, 0.29, 0.27);

/// A point nobody holds.
pub(crate) const NEUTRAL: Color = Color::srgb(0.88, 0.88, 0.84);
pub(crate) const MARK_EDGE: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
pub(crate) const TEXT_SHADOW: TextShadow = TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) };

/// The scale of type, largest first.
pub(crate) const TYPE_ACTION: f32 = 20.0;
pub(crate) const TYPE_TITLE: f32 = 17.0;
pub(crate) const TYPE_BODY: f32 = 14.0;
pub(crate) const TYPE_SMALL: f32 = 12.0;
pub(crate) const TYPE_LABEL: f32 = 11.0;

/// Spacing: between things in a group, between groups, and inside a panel.
pub(crate) const GAP: f32 = 4.0;
pub(crate) const SECTION: f32 = 14.0;
pub(crate) const PAD: f32 = 14.0;
/// Heights of the bar across the top, a row and the button that gets you in.
pub(crate) const BAR_HEIGHT: f32 = 44.0;
pub(crate) const ROW_HEIGHT: f32 = 30.0;
pub(crate) const ACTION_HEIGHT: f32 = 52.0;
/// The width of the panel beside the map, the least it may shrink to, and the most of the screen it takes.
pub(crate) const PANEL_WIDTH: f32 = 380.0;
pub(crate) const PANEL_MIN_WIDTH: f32 = 250.0;
pub(crate) const PANEL_MAX_SHARE: f32 = 42.0;
/// The coloured edge on a chosen row.
pub(crate) const ACCENT_WIDTH: f32 = 3.0;
/// How far one notch of the wheel moves a list.
pub(crate) const SCROLL_STEP: f32 = 40.0;

/// Marks on the map: a spawn, the one chosen, and the width of the ring round the point to attack next.
pub(crate) const SPAWN_MARK: f32 = 16.0;
pub(crate) const SPAWN_MARK_CHOSEN: f32 = 22.0;
pub(crate) const TARGET_RING: f32 = 2.0;
