//! Turns devices into player intent. Look is applied every frame, before the simulation and the
//! camera read it, so aiming has no added latency however the tick rate lands.

mod aim;
mod spike;

use aim::AimButton;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use prs_runtime::prs_sim::inventory::Slot;
use prs_runtime::prs_sim::protocol::Buttons;
use prs_runtime::prs_sim::weapons::{RestArc, RestConfig};
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::{LocalSoldier, LookScale, PendingInput, SimMessage, SimResource};
use spike::SpikeFilter;

const PITCH_LIMIT: f32 = 1.54;
/// Radians per pixel at sensitivity 1.
const BASE_SENSITIVITY: f32 = 0.0011;

/// Keys that are a button of the sim, and only ever one, so they all get the same treatment.
const BINDINGS: [(KeyCode, Buttons); 15] = [
    (KeyCode::KeyB, Buttons::BANDAGE),
    (KeyCode::Space, Buttons::JUMP),
    (KeyCode::KeyX, Buttons::CROUCH),
    (KeyCode::KeyZ, Buttons::PRONE),
    (KeyCode::KeyR, Buttons::RELOAD),
    (KeyCode::KeyV, Buttons::FIRE_MODE),
    (KeyCode::Digit1, Buttons::SLOT_1),
    (KeyCode::Digit2, Buttons::SLOT_2),
    (KeyCode::Digit3, Buttons::SLOT_3),
    (KeyCode::Digit4, Buttons::SLOT_4),
    (KeyCode::KeyF, Buttons::USE),
    (KeyCode::KeyQ, Buttons::LEAN_LEFT),
    (KeyCode::KeyE, Buttons::LEAN_RIGHT),
    (KeyCode::KeyG, Buttons::GRENADE),
    (KeyCode::KeyC, Buttons::REST),
];

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InputSettings {
            sensitivity: 1.0,
            zoom_hold_seconds: 0.18,
            free_look_yaw_limit: 2.15,
            free_look_return_rate: 12.0,
            rest_stops_at_arc: prs_runtime::prs_sim::prs_mods::tuned("view.rest_stops_at_arc", true),
        })
            .insert_resource(Captured(false))
            .insert_resource(MouseOverUi(false))
            .insert_resource(UiCursor::default())
            .insert_resource(MouseStats::default())
            .add_systems(
                RunFixedMainLoop,
                (update_capture, face_where_you_came_in, read_input)
                    .chain()
                    .in_set(ReadInput)
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            );
    }
}

/// Runs before each batch of ticks. Anything overriding input for a tick orders itself after this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadInput;

#[derive(Resource, Debug)]
pub struct InputSettings {
    pub sensitivity: f32,
    /// Right mouse held this long zooms. Released sooner, it was a tap that flips the sights.
    pub zoom_hold_seconds: f32,
    /// How far the head turns left or right of the gun while free looking, in radians.
    pub free_look_yaw_limit: f32,
    /// How quickly the head comes back to the gun on letting go, per second.
    pub free_look_return_rate: f32,
    /// Whether the mouse stops at the end of a rested gun's arc. Otherwise swinging on past it lifts the
    /// gun off, which is the rule of the sim. So this is the player's own and needs no one's agreement.
    pub rest_stops_at_arc: bool,
}

impl InputSettings {
    pub fn adjust_sensitivity(&mut self, factor: f32) {
        self.sensitivity = (self.sensitivity * factor).clamp(0.1, 8.0);
    }
}

/// Set by menus, so that clicking a control does not also capture the mouse.
#[derive(Resource, Debug)]
pub struct MouseOverUi(pub bool);

/// Mouse glitches thrown away so far, for the tuning overlay.
#[derive(Resource, Debug, Default)]
pub struct MouseStats {
    pub spikes_dropped: u32,
    pub largest_spike: f32,
}

/// A pointer the game draws itself, for a screen that comes up in the middle of playing. The mouse stays
/// captured all the while and only stops driving the soldier, because a browser wants a click to capture
/// it again, and a player should not have to give one for dying.
#[derive(Resource, Debug, Default)]
pub struct UiCursor {
    /// Set by whoever has a screen up.
    pub active: bool,
    /// In window coordinates.
    pub position: Vec2,
    /// The button went down this frame.
    pub clicked: bool,
    /// The button is down.
    pub held: bool,
}

/// Whether the game currently owns the mouse.
#[derive(Resource, Debug)]
pub struct Captured(pub bool);

fn update_capture(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut captured: ResMut<Captured>,
    over_ui: Res<MouseOverUi>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let (window, mut cursor) = window.into_inner();
    let mut release = keys.just_pressed(KeyCode::Escape) || !window.focused;
    release |= browser_dropped_pointer_lock(captured.0);

    if release {
        captured.0 = false;
    } else if mouse.just_pressed(MouseButton::Left) && !over_ui.0 {
        captured.0 = true;
    }

    let (grab, visible) = if captured.0 {
        (CursorGrabMode::Locked, false)
    } else {
        (CursorGrabMode::None, true)
    };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
    }
    if cursor.visible != visible {
        cursor.visible = visible;
    }
}

/// Browsers release pointer lock on Esc without telling the page through a key press.
#[cfg(target_arch = "wasm32")]
fn browser_dropped_pointer_lock(captured: bool) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WAS_LOCKED: AtomicBool = AtomicBool::new(false);

    let locked = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some();
    let was_locked = WAS_LOCKED.swap(locked, Ordering::Relaxed);
    captured && was_locked && !locked
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_dropped_pointer_lock(_captured: bool) -> bool {
    false
}

/// Your look is yours and the sim only follows it, so coming back in has to turn it to match.
fn face_where_you_came_in(
    mut messages: MessageReader<SimMessage>,
    local: Res<LocalSoldier>,
    sim: Res<SimResource>,
    mut input: ResMut<PendingInput>,
) {
    for SimMessage(event) in messages.read() {
        if matches!(event, SimEvent::Respawned { soldier } if *soldier == local.0) {
            let soldier = &sim.0.soldiers[local.0];
            input.yaw = soldier.yaw;
            input.pitch = soldier.pitch;
            input.free_look = Vec2::ZERO;
        }
    }
}

/// The same mouse movement turns the head or the gun, so free look has the same sensitivity.
/// `rest` is what a gun set down on something has to say about it: its rules, and where the look can go
/// at some yaw, which is the very sum the sim goes by. `dt` is how long this look took.
fn apply_look(input: &mut PendingInput, look: Vec2, free_looking: bool, settings: &InputSettings, rest: Option<(&RestConfig, &dyn Fn(f32) -> Option<RestArc>)>, dt: f32) {
    if free_looking {
        let limit = settings.free_look_yaw_limit;
        input.free_look.x = (input.free_look.x - look.x).clamp(-limit, limit);
        input.free_look.y =
            (input.free_look.y - look.y).clamp(-PITCH_LIMIT - input.pitch, PITCH_LIMIT - input.pitch);
    } else {
        let wanted = Vec2::new(input.yaw - look.x, (input.pitch - look.y).clamp(-PITCH_LIMIT, PITCH_LIMIT));
        let now = Vec2::new(input.yaw, input.pitch);
        let arc = rest.filter(|_| settings.rest_stops_at_arc).and_then(|(cfg, arc)| arc(wanted.x).map(|arc| (cfg, arc)));
        let got = arc.map_or(wanted, |(cfg, arc)| cfg.stop_look(&arc, now, wanted, dt));
        input.yaw = got.x;
        input.pitch = got.y;
    }
}

fn read_input(
    (sim, local): (Res<SimResource>, Res<LocalSoldier>),
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    captured: Res<Captured>,
    mut settings: ResMut<InputSettings>,
    look_scale: Res<LookScale>,
    time: Res<Time>,
    mut stats: ResMut<MouseStats>,
    mut spikes: Local<SpikeFilter>,
    mut input: ResMut<PendingInput>,
    mut trigger_blocked: Local<bool>,
    mut aim: Local<AimButton>,
    mut stale: Local<Buttons>,
    mut cursor: ResMut<UiCursor>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::BracketRight) {
        settings.adjust_sensitivity(1.1);
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        settings.adjust_sensitivity(1.0 / 1.1);
    }

    cursor.clicked = false;
    cursor.held = false;
    if captured.0 && cursor.active {
        let size = Vec2::new(window.width(), window.height());
        cursor.position = (cursor.position + motion.delta * settings.sensitivity).clamp(Vec2::ZERO, size);
        cursor.clicked = mouse.just_pressed(MouseButton::Left);
        cursor.held = mouse.pressed(MouseButton::Left);
    }
    if !captured.0 || cursor.active {
        input.stick = default();
        input.wish_move = Vec2::ZERO;
        input.held = Buttons::default();
        *trigger_blocked = true;
        input.zoom = false;
        input.free_look = Vec2::ZERO;
        *aim = AimButton::default();
        // Whatever is down now was pressed for the menu or the screen. It has to come up before it counts,
        // or a key held through either would act as a fresh press on the way back.
        *stale = BINDINGS.iter().filter(|(key, _)| keys.pressed(*key)).fold(Buttons::default(), |all, (_, button)| all.union(*button));
        return;
    }
    // Motion on the capture frame is the cursor travelling to the click, not aiming.
    let just_captured = *trigger_blocked && mouse.just_pressed(MouseButton::Left);

    let delta = if just_captured { Vec2::ZERO } else { motion.delta };
    let delta = spikes.filter(delta, time.delta_secs());
    stats.spikes_dropped = spikes.dropped;
    stats.largest_spike = spikes.largest_dropped;
    let look = delta * BASE_SENSITIVITY * settings.sensitivity * look_scale.0;
    let free_looking = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if input.at_controls && !free_looking {
        // Mouse forward is stick forward. Down the screen is the way pixels count.
        let stick = sim.0.heli_cfg.stick.clone();
        input.stick.nudge(&stick, Vec2::new(look.x, -look.y));
    } else {
        let soldier = sim.0.soldiers.get(local.0);
        let arc = |yaw: f32| sim.0.rest_arc(local.0, yaw);
        let rest = soldier.filter(|s| s.rest.on).map(|s| (&s.held_config(&sim.0.weapons).rest, &arc as &dyn Fn(f32) -> Option<RestArc>));
        apply_look(&mut input, look, free_looking, &settings, rest, time.delta_secs());
    }
    let stick = sim.0.heli_cfg.stick.clone();
    input.stick.relax(&stick, time.delta_secs());
    if !free_looking {
        input.free_look *= (-settings.free_look_return_rate * time.delta_secs()).exp();
    }

    let axis = |plus: KeyCode, minus: KeyCode| {
        f32::from(keys.pressed(plus) as u8) - f32::from(keys.pressed(minus) as u8)
    };
    input.wish_move = Vec2::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    )
    .clamp_length_max(1.0);

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mut held = Buttons::default();
    held.set(Buttons::SPRINT, shift);
    let mut still_stale = Buttons::default();
    for (key, button) in BINDINGS {
        let down = keys.pressed(key);
        if stale.contains(button) && down {
            still_stale = still_stale.union(button);
        } else {
            held.set(button, down);
        }
    }
    *stale = still_stale;
    held.set(Buttons::FIRE, mouse.pressed(MouseButton::Left) && !*trigger_blocked);
    // With a grenade out, right mouse held simply means underarm. It neither zooms nor flips the sights.
    let throwing = sim.0.soldiers.get(local.0).is_some_and(|s| matches!(s.inventory.selected, Slot::Frag | Slot::Smoke));
    if throwing {
        aim.set_aside();
    } else {
        aim.update(
            mouse.just_pressed(MouseButton::Right),
            mouse.pressed(MouseButton::Right),
            time.delta_secs(),
            settings.zoom_hold_seconds,
        );
    }
    held.set(Buttons::ADS, if throwing { mouse.pressed(MouseButton::Right) } else { aim.sights });
    input.zoom = aim.zooming;
    input.held = held;

    let mut tapped = input.tapped;
    for (key, button) in BINDINGS {
        if keys.just_pressed(key) {
            tapped = tapped.union(button);
        }
    }
    if mouse.just_pressed(MouseButton::Left) && !*trigger_blocked {
        tapped = tapped.union(Buttons::FIRE);
    }
    input.tapped = tapped;

    // The click that captures the mouse must not also fire a round. Cleared last, because on a
    // slow frame that click can be pressed and released within this one update.
    if !mouse.pressed(MouseButton::Left) {
        *trigger_blocked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> InputSettings {
        InputSettings {
            sensitivity: 1.0,
            zoom_hold_seconds: 0.18,
            free_look_yaw_limit: 1.9,
            free_look_return_rate: 12.0,
            rest_stops_at_arc: true,
        }
    }

    #[test]
    fn free_look_turns_the_head_as_far_as_the_gun_would_turn_and_leaves_the_gun_alone() {
        let look = Vec2::new(0.2, -0.1);
        let mut gun = PendingInput::default();
        apply_look(&mut gun, look, false, &settings(), None, DT);
        let mut head = PendingInput::default();
        apply_look(&mut head, look, true, &settings(), None, DT);
        assert_eq!(head.free_look, Vec2::new(gun.yaw, gun.pitch));
        assert_eq!((head.yaw, head.pitch), (0.0, 0.0));
    }

    const DT: f32 = 1.0 / 60.0;

    fn arc_round(base: Vec2, cfg: &RestConfig) -> RestArc {
        RestArc { yaw_low: base.x - cfg.arc_yaw, yaw_high: base.x + cfg.arc_yaw, pitch_low: base.y - cfg.arc_down, pitch_high: base.y + cfg.arc_up }
    }

    #[test]
    fn a_rested_gun_stops_at_the_end_of_its_arc_and_comes_straight_back() {
        let cfg = RestConfig::default();
        let base = Vec2::new(1.0, 0.1);
        let arc = |_: f32| Some(arc_round(base, &cfg));
        let mut input = PendingInput { yaw: base.x, pitch: base.y, ..default() };
        // Mouse going one way for a long while, far harder than anyone could.
        for _ in 0..600 {
            apply_look(&mut input, Vec2::new(-0.05, 0.0), false, &settings(), Some((&cfg, &arc)), DT);
            assert!(input.yaw - base.x < cfg.arc_yaw, "{}", input.yaw - base.x);
        }
        assert!((input.yaw - base.x - cfg.arc_yaw).abs() < 0.01, "should get to the end: {}", input.yaw - base.x);
        let at_end = input.yaw;
        apply_look(&mut input, Vec2::new(0.02, 0.0), false, &settings(), Some((&cfg, &arc)), DT);
        assert!((at_end - input.yaw - 0.02).abs() < 1e-5, "the whole of it on the first frame: {}", at_end - input.yaw);
    }

    /// The floor comes up under the gun as it swings over something deeper. The look goes along, gently.
    #[test]
    fn a_limit_coming_up_past_the_look_draws_it_back_without_a_jump() {
        let cfg = RestConfig::default();
        let arc = |_: f32| Some(RestArc { yaw_low: -1.0, yaw_high: 1.0, pitch_low: 0.2, pitch_high: 0.5 });
        let mut input = PendingInput { yaw: 0.0, pitch: -0.2, ..default() };
        let mut last = input.pitch;
        for _ in 0..120 {
            apply_look(&mut input, Vec2::ZERO, false, &settings(), Some((&cfg, &arc)), DT);
            assert!(input.pitch - last <= cfg.pull_rate * DT + 1e-5, "{} from {last}", input.pitch);
            last = input.pitch;
        }
        assert!((input.pitch - 0.2).abs() < 0.01, "{}", input.pitch);
    }

    #[test]
    fn with_the_setting_off_a_rested_gun_swings_on_past_its_arc() {
        let cfg = RestConfig::default();
        let arc = |_: f32| Some(arc_round(Vec2::ZERO, &cfg));
        let free = InputSettings { rest_stops_at_arc: false, ..settings() };
        let mut input = PendingInput::default();
        apply_look(&mut input, Vec2::new(-2.0, 0.0), false, &free, Some((&cfg, &arc)), DT);
        assert_eq!(input.yaw, 2.0);
    }

    /// The head goes where it likes on a rest, and coming back to the gun moves nothing of the gun's.
    #[test]
    fn free_look_on_a_rest_is_not_stopped_and_cannot_push_the_gun_past_its_arc() {
        let cfg = RestConfig::default();
        let arc = |_: f32| Some(arc_round(Vec2::ZERO, &cfg));
        let at_end = cfg.arc_yaw - cfg.arc_margin;
        let mut input = PendingInput { yaw: at_end, ..default() };
        apply_look(&mut input, Vec2::new(-1.5, 0.0), true, &settings(), Some((&cfg, &arc)), DT);
        assert_eq!(input.free_look.x, 1.5);
        assert_eq!(input.yaw, at_end);
        // What the return does, in `read_input`, and the gun is where it was.
        input.free_look *= 0.0;
        assert_eq!(input.yaw, at_end);
    }

    #[test]
    fn a_mod_can_change_the_default_for_stopping_at_the_arc() {
        let mods = prs_runtime::prs_sim::prs_mods::Mod::parse("test", "view.rest_stops_at_arc = false").unwrap();
        assert!(!mods.over("view.rest_stops_at_arc", true).unwrap());
    }

    #[test]
    fn free_look_stays_within_the_neck_and_never_looks_past_straight_up() {
        let mut input = PendingInput { pitch: 1.0, ..default() };
        apply_look(&mut input, Vec2::new(-50.0, -50.0), true, &settings(), None, DT);
        assert_eq!(input.free_look.x, 1.9);
        assert!((input.pitch + input.free_look.y - PITCH_LIMIT).abs() < 1e-5);
    }
}
