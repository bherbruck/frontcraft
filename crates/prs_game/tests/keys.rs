//! Every binding through the real path: keyboard and mouse messages into the whole game with no window
//! or graphics card, one frame at a time. The sim's tests say what a button does. These say which key
//! gets to that button, and what the same key does on the spawn screen, while down, in a door and in a
//! helicopter. They set the map through the environment, so run them with `--test-threads=1`.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::time::TimeUpdateStrategy;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitPlugin;
use prs_hud::{CarryPanelOpen, Screens};
use prs_input::{Captured, InputSettings, UiCursor};
use prs_runtime::prs_sim::health::Life;
use prs_runtime::prs_sim::inventory::Slot;
use prs_runtime::prs_sim::movement::Stance;
use prs_runtime::prs_sim::throwables::Cloud;
use prs_runtime::prs_sim::{Sim, Soldier};
use prs_runtime::{GameMode, LocalSoldier, PendingInput, SimResource, ThirdPerson};
use prs_view::ViewSettings;
use std::sync::Mutex;
use std::time::Duration;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
/// Where settings go during a test. Empty means nowhere, so these leave a player's own alone.
static SETTINGS_FILE: Mutex<String> = Mutex::new(String::new());

fn settings_file() -> String {
    SETTINGS_FILE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

struct Game {
    app: App,
    window: Entity,
}

impl Game {
    fn on(map: &str, mode: &str) -> Self {
        // Safe as only these tests set it, one at a time, and nothing else is running yet.
        unsafe {
            std::env::set_var("PRS_MAP", map);
            std::env::set_var("PRS_MODE", mode);
            std::env::set_var("PRS_WALKERS", "0");
            std::env::set_var("PRS_SETTINGS_FILE", settings_file());
        }
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .disable::<bevy::log::LogPlugin>()
                .set(RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(WgpuSettings { backends: None, ..default() })), ..default() }),
        )
        .add_plugins((
            prs_runtime::RuntimePlugin,
            prs_models::ModelsPlugin,
            prs_atmosphere::AtmospherePlugin,
            prs_input::InputPlugin,
            prs_view::ViewPlugin,
            prs_scene::ScenePlugin,
            prs_audio::AudioPlugin,
            prs_hud::HudPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_nanos(16_666_667)));
        app.finish();
        app.cleanup();
        app.update();
        let window = app.world_mut().query_filtered::<Entity, With<PrimaryWindow>>().iter(app.world()).next().expect("a window");
        app.world_mut().resource_mut::<Captured>().0 = true;
        let mut game = Self { app, window };
        game.frames(3);
        game
    }

    fn range() -> Self {
        Self::on("range", "practice")
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn seconds(&mut self, s: f32) {
        self.frames((s * 60.0) as usize);
    }

    fn key(&mut self, code: KeyCode, state: ButtonState) {
        let window = self.window;
        self.app.world_mut().write_message(KeyboardInput { key_code: code, logical_key: Key::Dead(None), state, text: None, repeat: false, window });
    }

    fn down(&mut self, code: KeyCode) {
        self.key(code, ButtonState::Pressed);
        self.frames(1);
    }

    fn up(&mut self, code: KeyCode) {
        self.key(code, ButtonState::Released);
        self.frames(1);
    }

    fn tap(&mut self, code: KeyCode) {
        self.down(code);
        self.up(code);
        self.frames(1);
    }

    fn mouse(&mut self, button: MouseButton, state: ButtonState) {
        let window = self.window;
        self.app.world_mut().write_message(MouseButtonInput { button, state, window });
        self.frames(1);
    }

    fn click(&mut self, button: MouseButton) {
        self.mouse(button, ButtonState::Pressed);
        self.mouse(button, ButtonState::Released);
        self.frames(1);
    }

    fn turn(&mut self, delta: Vec2) {
        self.app.world_mut().write_message(MouseMotion { delta });
        self.frames(1);
    }

    fn sim(&mut self) -> &mut Sim {
        &mut self.app.world_mut().resource_mut::<SimResource>().into_inner().0
    }

    fn id(&self) -> usize {
        self.app.world().resource::<LocalSoldier>().0
    }

    fn me(&mut self) -> &mut Soldier {
        let id = self.id();
        &mut self.sim().soldiers[id]
    }

    fn smoke_voices(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&prs_audio::SmokeVoice>().iter(world).count()
    }

    fn input(&self) -> PendingInput {
        *self.app.world().resource::<PendingInput>()
    }

    fn screens(&self) -> (bool, bool, bool) {
        let s = self.app.world().resource::<Screens>();
        (s.deploy, s.map, s.board)
    }

    /// Stands where nothing is in the way, looking down the range.
    fn in_the_open(&mut self) {
        let id = self.id();
        let at = Vec3::new(-20.0, 0.0, 30.0);
        let sim = self.sim();
        let ground = sim.world.raycast(at + Vec3::Y * 60.0, Vec3::NEG_Y, 120.0).map_or(0.0, |hit| 60.0 - hit.distance);
        sim.set_home(id, at + Vec3::Y * (ground + 0.05), 0.0);
        self.frames(30);
    }

    /// In front of a door or hatch, facing it, and in through it with F.
    fn in_through_a_door(&mut self, carrier: u32, entry: usize) {
        let id = self.id();
        let sim = self.sim();
        let frame = sim.carrier_frame(carrier);
        let door = sim.carrier_entries(carrier)[entry].clone();
        let stand = frame.point(door.stand);
        let hand = frame.point(door.hand);
        let ground = sim.world.raycast(stand + Vec3::Y * 5.0, Vec3::NEG_Y, 10.0).map_or(stand.y, |hit| stand.y + 5.0 - hit.distance);
        let spot = Vec3::new(stand.x, ground + 0.05, stand.z);
        sim.soldiers[id].body.pos = spot;
        sim.soldiers[id].body.vel = Vec3::ZERO;
        self.frames(10);
        let eye = self.me().eye();
        let to = hand - eye;
        {
            let mut input = self.app.world_mut().resource_mut::<PendingInput>();
            input.yaw = (-to.x).atan2(-to.z);
            input.pitch = (to.y / to.length()).asin();
        }
        self.frames(3);
        self.tap(KeyCode::KeyF);
    }

    fn seated(&mut self) -> Option<u8> {
        self.me().seat.map(|at| at.seat)
    }

    fn until_settled_in_a_seat(&mut self) {
        for _ in 0..600 {
            self.frames(1);
            if self.me().seat.is_some() && self.me().transit.is_none() {
                return;
            }
        }
        panic!("never settled in a seat");
    }
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn alt_turns_the_head_and_leaves_the_gun_and_it_eases_back_and_stops_at_its_limit() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let (yaw, pitch) = (g.input().yaw, g.input().pitch);
    let gun_yaw = g.me().yaw;

    g.key(KeyCode::AltLeft, ButtonState::Pressed);
    for _ in 0..10 {
        g.turn(Vec2::new(-40.0, 0.0));
    }
    assert!(g.input().free_look.x > 0.2, "{}", g.input().free_look);
    assert_eq!((g.input().yaw, g.input().pitch), (yaw, pitch), "the look the gun follows has not moved");
    assert_eq!(g.me().yaw, gun_yaw, "nor has the gun");

    // As far as it goes, one way and the other. The limit is the same as the setting.
    let limit = g.app.world().resource::<InputSettings>().free_look_yaw_limit;
    assert_eq!(limit, 2.15);
    for _ in 0..200 {
        g.turn(Vec2::new(-60.0, 0.0));
    }
    assert_eq!(g.input().free_look.x, limit);
    for _ in 0..400 {
        g.turn(Vec2::new(60.0, 0.0));
    }
    assert_eq!(g.input().free_look.x, -limit);
    assert_eq!(g.input().yaw, yaw);

    // Let go and it comes back by itself, with the mouse turning the gun again.
    g.up(KeyCode::AltLeft);
    g.frames(6);
    let partway = g.input().free_look.x.abs();
    assert!(partway > 0.01 && partway < limit, "eases rather than snaps: {partway}");
    g.seconds(1.0);
    assert!(g.input().free_look.length() < 1e-3);
    g.turn(Vec2::new(-40.0, 0.0));
    assert!(g.input().yaw > yaw);
}

#[test]
fn right_mouse_tap_flips_the_sights_and_hold_zooms_with_or_without_them() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    assert!(!g.input().held.contains(prs_runtime::prs_sim::protocol::Buttons::ADS));
    g.click(MouseButton::Right);
    g.frames(30);
    assert!(g.me().weapon.ads > 0.9, "a tap raises the sights: {}", g.me().weapon.ads);
    assert!(!g.input().zoom);
    g.click(MouseButton::Right);
    g.frames(30);
    assert!(g.me().weapon.ads < 0.1, "and another lowers them");

    // Held, it zooms while it is held, from the hip and with the sights staying down.
    g.mouse(MouseButton::Right, ButtonState::Pressed);
    g.seconds(0.4);
    assert!(g.input().zoom);
    assert!(g.me().weapon.ads < 0.1, "holding zooms and does not raise the sights: {}", g.me().weapon.ads);
    g.mouse(MouseButton::Right, ButtonState::Released);
    g.frames(2);
    assert!(!g.input().zoom);
    assert!(g.me().weapon.ads < 0.1);
}

#[test]
fn q_and_e_lean_again_comes_back_and_the_other_swaps() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    g.tap(KeyCode::KeyQ);
    assert_eq!(g.me().body.lean.side, -1);
    g.tap(KeyCode::KeyE);
    assert_eq!(g.me().body.lean.side, 1);
    g.tap(KeyCode::KeyE);
    assert_eq!(g.me().body.lean.side, 0);
}

#[test]
fn x_and_z_change_stance_and_again_stands_up() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    g.tap(KeyCode::KeyX);
    g.seconds(0.5);
    assert_eq!(g.me().body.stance, Stance::Crouch);
    g.tap(KeyCode::KeyX);
    g.seconds(0.5);
    assert_eq!(g.me().body.stance, Stance::Stand);
    g.tap(KeyCode::KeyZ);
    g.seconds(1.0);
    assert_eq!(g.me().body.stance, Stance::Prone);
    g.tap(KeyCode::KeyZ);
    g.seconds(1.0);
    assert_eq!(g.me().body.stance, Stance::Stand);
}

#[test]
fn one_to_four_choose_what_is_in_your_hands_on_foot() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    for (key, slot) in [(KeyCode::Digit2, Slot::Sidearm), (KeyCode::Digit3, Slot::Bandage), (KeyCode::Digit4, Slot::Medkit), (KeyCode::Digit1, Slot::Primary)] {
        g.tap(key);
        assert_eq!(g.me().inventory.selected, slot, "{key:?}");
    }
}

#[test]
fn v_changes_fire_mode_once_per_press_and_nothing_else_touches_it() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let start = g.me().weapon.mode;
    for key in [KeyCode::Digit2, KeyCode::Digit1, KeyCode::KeyR, KeyCode::KeyF, KeyCode::KeyX, KeyCode::KeyX, KeyCode::KeyQ, KeyCode::KeyQ, KeyCode::Space] {
        g.tap(key);
        g.seconds(0.1);
    }
    g.seconds(2.0);
    assert_eq!(g.me().weapon.mode, start);
    g.down(KeyCode::KeyV);
    g.seconds(1.0);
    let flipped = g.me().weapon.mode;
    assert_ne!(flipped, start, "pressing V changes it");
    g.up(KeyCode::KeyV);
    g.seconds(0.5);
    assert_eq!(g.me().weapon.mode, flipped, "once, however long it was held");
}

#[test]
fn space_jumps_and_with_w_against_a_low_wall_it_climbs() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let feet = g.me().body.pos.y;
    g.down(KeyCode::Space);
    g.up(KeyCode::Space);
    let mut highest = feet;
    for _ in 0..30 {
        g.frames(1);
        highest = highest.max(g.me().body.pos.y);
    }
    assert!(highest > feet + 0.3, "should have jumped: {feet} to {highest}");
    g.seconds(1.0);

    // Square on to something waist high, on the side nearest the start.
    let id = g.id();
    let sim = g.sim();
    let wall = sim.level.solids.iter().find(|s| s.half.y > 0.4 && s.half.y < 0.8 && s.half.x > 1.0).expect("the firing line has one").clone();
    let spot = wall.center + Vec3::new(0.0, -wall.half.y + 0.1, wall.half.z + 0.55);
    sim.soldiers[id].body.pos = spot;
    {
        let mut input = g.app.world_mut().resource_mut::<PendingInput>();
        input.yaw = 0.0;
        input.pitch = 0.0;
    }
    g.frames(20);
    g.key(KeyCode::KeyW, ButtonState::Pressed);
    g.frames(5);
    g.down(KeyCode::Space);
    g.frames(3);
    assert!(g.me().body.climb.active(), "W and Space at a wall should start a climb");
    g.up(KeyCode::Space);
    g.up(KeyCode::KeyW);
}

#[test]
fn m_i_tab_f3_and_f4_only_show_things() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    g.tap(KeyCode::KeyM);
    assert!(g.screens().1, "M opens the map");
    g.tap(KeyCode::KeyM);
    assert!(!g.screens().1);
    g.tap(KeyCode::KeyI);
    assert!(g.app.world().resource::<CarryPanelOpen>().0);
    g.tap(KeyCode::KeyI);
    g.down(KeyCode::Tab);
    assert!(g.screens().2, "held Tab shows the board");
    g.up(KeyCode::Tab);
    assert!(!g.screens().2);
    g.tap(KeyCode::F4);
    assert!(g.app.world().resource::<ThirdPerson>().0);
    g.tap(KeyCode::F4);
    assert!(!g.app.world().resource::<ThirdPerson>().0);
    assert!(g.app.world().resource::<Captured>().0, "none of them gives the mouse back");
}

#[test]
fn nothing_in_the_pause_menu_acts_on_the_game_behind_it() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    g.tap(KeyCode::Escape);
    assert!(!g.app.world().resource::<Captured>().0);
    let (pos, ammo) = (g.me().body.pos, g.me().weapon.ammo);
    let selected = g.me().inventory.selected;
    for key in [KeyCode::KeyW, KeyCode::Space, KeyCode::KeyR, KeyCode::Digit2, KeyCode::KeyF, KeyCode::KeyM, KeyCode::KeyI, KeyCode::KeyT, KeyCode::Enter] {
        g.tap(key);
    }
    g.seconds(0.5);
    assert!(g.me().body.pos.distance(pos) < 0.05);
    assert_eq!(g.me().weapon.ammo, ammo);
    assert_eq!(g.me().inventory.selected, selected);
    assert!(!g.screens().1, "M does not open the map behind the menu");
    assert!(!g.app.world().resource::<CarryPanelOpen>().0, "nor I the panel");
}

#[test]
fn a_click_that_takes_the_mouse_back_does_not_fire() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let ammo = g.me().weapon.ammo;
    g.tap(KeyCode::Escape);
    g.click(MouseButton::Left);
    assert!(g.app.world().resource::<Captured>().0);
    g.seconds(0.2);
    assert_eq!(g.me().weapon.ammo, ammo);
    g.click(MouseButton::Left);
    g.seconds(0.1);
    assert_eq!(g.me().weapon.ammo, ammo - 1, "the next one does");
}

#[test]
fn in_a_vehicle_the_number_keys_are_seats_and_f_gets_out_and_the_rest_does_no_harm() {
    let _one = serial();
    let mut g = Game::on("range", "practice");
    assert!(!g.sim().vehicles.is_empty(), "the range has a truck");
    g.in_through_a_door(0, 0);
    assert!(g.seated().is_some(), "F at a door starts getting in");
    g.until_settled_in_a_seat();

    let selected = g.me().inventory.selected;
    let (ammo, stance) = (g.me().weapon.ammo, g.me().body.stance);
    let seat = g.seated();
    for key in [KeyCode::KeyX, KeyCode::KeyZ, KeyCode::KeyQ, KeyCode::KeyE, KeyCode::KeyR, KeyCode::KeyV] {
        g.tap(key);
    }
    assert_eq!(g.me().body.stance, stance, "no stance in a seat");
    assert_eq!(g.me().body.lean.side, 0, "no leaning in a seat");
    assert_eq!(g.me().inventory.selected, selected);

    // Across to the next seat, which takes a moment. F does nothing until you have got there.
    g.tap(KeyCode::Digit2);
    g.seconds(0.1);
    assert_eq!(g.me().inventory.selected, selected, "number keys do not change what you carry here");
    assert_ne!(g.seated(), seat, "they are the seats");
    g.tap(KeyCode::KeyF);
    assert!(g.seated().is_some());
    g.until_settled_in_a_seat();
    assert_eq!(g.me().weapon.ammo, ammo);

    g.tap(KeyCode::KeyF);
    g.seconds(3.0);
    assert_eq!(g.seated(), None, "F gets out");
    g.tap(KeyCode::Digit2);
    assert_eq!(g.me().inventory.selected, Slot::Sidearm, "and on foot they are your kit again");
}

#[test]
fn in_a_door_nothing_but_time_gets_you_through_and_the_gun_stays_quiet() {
    let _one = serial();
    let mut g = Game::on("range", "practice");
    g.in_through_a_door(0, 0);
    assert!(g.me().transit.is_some(), "partway through the door");
    let ammo = g.me().weapon.ammo;
    for key in [KeyCode::KeyF, KeyCode::Digit2, KeyCode::KeyX, KeyCode::KeyZ, KeyCode::KeyQ, KeyCode::Space, KeyCode::KeyR] {
        g.tap(key);
    }
    g.mouse(MouseButton::Left, ButtonState::Pressed);
    g.frames(10);
    g.mouse(MouseButton::Left, ButtonState::Released);
    assert_eq!(g.me().weapon.ammo, ammo, "no firing out of a door you are halfway through");
    assert_eq!(g.me().body.lean.side, 0);
}

#[test]
fn at_a_helicopters_controls_the_mouse_is_the_stick_and_alt_is_your_head() {
    let _one = serial();
    let mut g = Game::on("range", "practice");
    let carrier = prs_runtime::prs_sim::heli_carrier(0);
    assert!(!g.sim().helicopters.is_empty(), "the range has a helicopter");
    let pilots_door = g.sim().carrier_entries(carrier).iter().position(|e| e.seats.contains(&(prs_runtime::prs_sim::PILOT_SEAT as u8))).expect("a pilot's door");
    g.in_through_a_door(carrier, pilots_door);
    g.until_settled_in_a_seat();
    g.frames(5);
    assert!(g.input().at_controls);

    let stick = g.input().stick;
    for _ in 0..10 {
        g.turn(Vec2::new(30.0, -10.0));
    }
    assert_ne!(g.input().stick, stick, "the mouse moves the stick");
    assert!(g.input().free_look.length() < 1e-3);

    g.key(KeyCode::AltLeft, ButtonState::Pressed);
    for _ in 0..10 {
        g.turn(Vec2::new(-30.0, 0.0));
    }
    assert!(g.input().free_look.x > 0.1, "with Alt it turns your head");
    g.up(KeyCode::AltLeft);

    // Space starts the engine rather than jumping out of it, and F is still the way out.
    let on = g.sim().helicopters[0].state.engine;
    g.tap(KeyCode::Space);
    g.seconds(0.1);
    assert_ne!(g.sim().helicopters[0].state.engine, on, "Space is the engine switch");
    g.tap(KeyCode::KeyF);
    g.seconds(3.0);
    assert_eq!(g.seated(), None);
}

/// A real match: you start off the field, with a pointer of your own, and only the screen hears you.
#[test]
fn on_the_spawn_screen_the_keys_belong_to_the_screen() {
    let _one = serial();
    let mut g = Game::on("town", "aas");
    g.frames(10);
    assert!(g.screens().0, "a match starts on the spawn screen");
    assert!(g.app.world().resource::<UiCursor>().active, "with its own pointer");
    let (pos, ammo, selected) = (g.me().body.pos, g.me().weapon.ammo, g.me().inventory.selected);
    let yaw = g.input().yaw;

    for key in [KeyCode::KeyW, KeyCode::Space, KeyCode::KeyR, KeyCode::KeyF, KeyCode::KeyX, KeyCode::KeyQ, KeyCode::KeyT, KeyCode::KeyM] {
        g.tap(key);
    }
    g.turn(Vec2::new(100.0, 0.0));
    g.click(MouseButton::Left);
    g.seconds(0.3);
    assert!(g.me().body.pos.distance(pos) < 0.2, "not walking about behind the screen");
    assert_eq!(g.me().weapon.ammo, ammo);
    assert_eq!(g.input().yaw, yaw, "the mouse is the pointer here");
    assert!(!g.screens().1, "no map over the spawn screen");

    // Number keys choose a kit, and do not also change what is in your hands.
    g.tap(KeyCode::Digit2);
    g.seconds(0.1);
    assert_eq!(g.me().inventory.selected, selected, "a number here is a kit and not a slot");
    let id = g.id();
    let kit = g.app.world().resource::<GameMode>().0.state.player(id).map(|p| p.kit);
    assert_eq!(kit, Some(1), "the second kit");

    // Enter goes. On the field the same keys are yours again.
    g.tap(KeyCode::Enter);
    for _ in 0..60 * 40 {
        g.frames(1);
        if !g.screens().0 {
            break;
        }
    }
    assert!(!g.screens().0, "deployed");
    assert!(!g.app.world().resource::<UiCursor>().active);
}

#[test]
fn while_down_you_can_only_look_and_enter_is_giving_up() {
    let _one = serial();
    let mut g = Game::on("town", "aas");
    g.frames(10);
    g.tap(KeyCode::Enter);
    for _ in 0..60 * 40 {
        g.frames(1);
        if !g.screens().0 {
            break;
        }
    }
    assert!(!g.screens().0, "deployed");
    let id = g.id();
    g.sim().hurt(id, prs_runtime::prs_sim::health::Zone::Chest, 120.0);
    g.seconds(1.5);
    assert_eq!(g.me().health.life, Life::Down);
    let (ammo, selected, stance) = (g.me().weapon.ammo, g.me().inventory.selected, g.me().body.stance);
    let pos = g.me().body.pos;
    for key in [KeyCode::KeyW, KeyCode::Space, KeyCode::KeyR, KeyCode::KeyF, KeyCode::KeyX, KeyCode::KeyZ, KeyCode::KeyQ, KeyCode::Digit2, KeyCode::KeyV] {
        g.tap(key);
    }
    g.click(MouseButton::Left);
    g.seconds(0.2);
    assert_eq!(g.me().weapon.ammo, ammo);
    assert_eq!(g.me().inventory.selected, selected);
    assert_eq!(g.me().body.stance, stance);
    assert!(g.me().body.pos.distance(pos) < 0.2);

    // The map and the board still work, and the look is yours.
    let yaw = g.input().yaw;
    g.turn(Vec2::new(-30.0, 0.0));
    assert!(g.input().yaw > yaw);
    g.tap(KeyCode::KeyM);
    assert!(g.screens().1);
    g.tap(KeyCode::KeyM);
}

#[test]
fn view_settings_do_not_care_which_keys_are_down() {
    let _one = serial();
    let mut g = Game::range();
    let before = g.app.world().resource::<ViewSettings>().fov();
    g.tap(KeyCode::BracketRight);
    assert!(g.app.world().resource::<InputSettings>().sensitivity > 1.0);
    g.tap(KeyCode::BracketLeft);
    assert!((g.app.world().resource::<InputSettings>().sensitivity - 1.0).abs() < 1e-3);
    assert_eq!(g.app.world().resource::<ViewSettings>().fov(), before);
}

/// Found by the soak: a key held through a pause, the spawn screen or being down acted again on the way
/// back as if freshly pressed. Most visibly V, flipping the fire mode with nobody touching it.
#[test]
fn a_key_held_through_the_pause_menu_does_not_act_again_on_coming_back() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let mode = g.me().weapon.mode;
    g.tap(KeyCode::Escape);
    g.down(KeyCode::KeyV);
    g.frames(5);
    g.click(MouseButton::Left);
    assert!(g.app.world().resource::<Captured>().0);
    g.seconds(0.2);
    assert_eq!(g.me().weapon.mode, mode, "V went down while paused, so it does not count now");
    g.up(KeyCode::KeyV);
    g.seconds(0.1);
    g.tap(KeyCode::KeyV);
    g.seconds(0.1);
    assert_ne!(g.me().weapon.mode, mode, "and once it has come up, it works");
}

#[test]
fn settings_come_back_next_time_and_a_damaged_file_gives_the_defaults() {
    let _one = serial();
    let dir = std::env::temp_dir().join(format!("prs-settings-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.txt");
    let _ = std::fs::remove_file(&file);
    *SETTINGS_FILE.lock().unwrap() = file.display().to_string();

    let mut g = Game::range();
    let fov_before = g.app.world().resource::<ViewSettings>().fov();
    g.tap(KeyCode::BracketRight);
    g.app.world_mut().resource_mut::<ViewSettings>().set_fov(75.0);
    g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().set_master(0.35);
    g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().set_effects(0.6);
    assert_eq!(g.app.world().resource::<prs_audio::AudioLevels>().sound_set(), prs_audio::SoundSet::New);
    g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().choose_sound_set(prs_audio::SoundSet::Current);
    // It writes a moment after things stop changing.
    g.seconds(1.0);
    let sensitivity = g.app.world().resource::<InputSettings>().sensitivity;
    assert_ne!(sensitivity, 1.0);
    assert_ne!(fov_before, 75.0);
    drop(g);

    let mut again = Game::range();
    assert_eq!(again.app.world().resource::<ViewSettings>().fov(), 75.0);
    assert!((again.app.world().resource::<InputSettings>().sensitivity - sensitivity).abs() < 1e-4);
    let levels = *again.app.world().resource::<prs_audio::AudioLevels>();
    assert_eq!((levels.master(), levels.effects()), (0.35, 0.6));
    assert_eq!(levels.sound_set(), prs_audio::SoundSet::Current);
    again.frames(2);
    drop(again);

    std::fs::write(&file, b"\xff\xfe not settings at all \x00\x01").unwrap();
    let fresh = Game::range();
    assert_eq!(fresh.app.world().resource::<ViewSettings>().fov(), ViewSettings::default().fov());
    assert_eq!(fresh.app.world().resource::<InputSettings>().sensitivity, 1.0);
    drop(fresh);

    std::fs::write(&file, "fov = potato\nsensitivity = NaN\nmaster_volume = 0.5\n").unwrap();
    let partial = Game::range();
    assert_eq!(partial.app.world().resource::<ViewSettings>().fov(), ViewSettings::default().fov());
    assert_eq!(partial.app.world().resource::<prs_audio::AudioLevels>().master(), 0.5, "what could be read still counts");

    *SETTINGS_FILE.lock().unwrap() = String::new();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Right mouse means underarm with a grenade out. It should not also zoom or leave the sights up.
#[test]
fn right_mouse_with_a_grenade_out_does_not_zoom_or_raise_the_sights_afterwards() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    g.tap(KeyCode::KeyG);
    g.seconds(1.0);
    g.mouse(MouseButton::Right, ButtonState::Pressed);
    g.seconds(0.4);
    assert!(!g.input().zoom, "holding for underarm should not zoom");
    g.mouse(MouseButton::Left, ButtonState::Pressed);
    g.seconds(0.5);
    assert!(g.me().throw.underarm, "right mouse held is what makes it underarm");
    g.mouse(MouseButton::Left, ButtonState::Released);
    g.mouse(MouseButton::Right, ButtonState::Released);
    g.seconds(0.1);
    g.tap(KeyCode::Digit1);
    g.seconds(2.0);
    assert!(g.me().weapon.ads < 0.1, "the gun comes back at the hip: {}", g.me().weapon.ads);
}

/// B reaches the bandage through the real key, on foot, with nothing taken out first.
#[test]
fn holding_b_bandages_you_and_nothing_else_changes() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let id = g.id();
    g.sim().hurt(id, prs_runtime::prs_sim::health::Zone::Leg, 20.0);
    let selected = g.me().inventory.selected;
    g.key(KeyCode::KeyB, ButtonState::Pressed);
    g.seconds(1.0);
    assert!(g.me().treatment.is_some(), "under way");
    g.seconds(5.0);
    assert!(!g.me().health.bleeding());
    assert_eq!(g.me().inventory.selected, selected);
    g.up(KeyCode::KeyB);
}

/// A smoke grenade can be heard from when it goes off until the smoke is gone, through the real audio systems.
#[test]
fn a_smoke_grenade_hisses_for_as_long_as_it_pours_and_then_stops() {
    let mut game = Game::range();
    game.seconds(0.2);
    assert_eq!(game.smoke_voices(), 0);
    let near = game.me().body.pos + Vec3::new(2.0, 0.0, 0.0);
    game.sim().clouds.push(Cloud { id: 900, pos: near, age: 0.0 });
    game.seconds(0.2);
    assert!(game.smoke_voices() > 0, "it should start right away");
    // Well into its life it is still going, one stretch after another.
    let cfg = game.sim().throw_cfg.smoke.clone();
    game.sim().clouds[0].age = cfg.grow_time + 1.0;
    game.seconds(5.0);
    assert!(game.smoke_voices() > 0, "it should keep going");
    // Right at the end of the smoke.
    let end = cfg.grow_time + cfg.hold_time + cfg.fade_time;
    game.sim().clouds[0].age = end - 0.2;
    game.seconds(1.0);
    assert!(game.sim().clouds.is_empty(), "the cloud is done");
    game.seconds(1.0);
    assert_eq!(game.smoke_voices(), 0, "and so is the hiss");
}

/// New renders a fresh sound for each event. Current plays what it made at the start, as it always has.
/// The switch takes hold on the very next event, without a restart.
#[test]
fn the_sound_set_switches_at_once_and_every_kind_of_event_plays_on_both() {
    use prs_runtime::SimMessage;
    use prs_runtime::prs_sim::weapons::WeaponEvent;
    use prs_runtime::prs_sim::{SimEvent, Surface};
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    let soldier = g.app.world().resource::<prs_runtime::LocalSoldier>().0;
    // Sounds that have finished go away again, so it is the ones made that are counted.
    #[derive(Resource, Default)]
    struct Made(usize);
    g.app.init_resource::<Made>().add_systems(Update, |mut events: MessageReader<AssetEvent<AudioSource>>, mut made: ResMut<Made>| {
        made.0 += events.read().filter(|event| matches!(event, AssetEvent::Added { .. })).count();
    });
    let rendered = |g: &mut Game| g.app.world().resource::<Made>().0;
    let tell = |g: &mut Game, event: SimEvent| {
        g.app.world_mut().write_message(SimMessage(event));
        g.frames(2);
    };
    for (set, fresh) in [(prs_audio::SoundSet::New, true), (prs_audio::SoundSet::Current, false), (prs_audio::SoundSet::New, true)] {
        g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().choose_sound_set(set);
        let before = rendered(&mut g);
        tell(&mut g, SimEvent::Weapon { soldier, event: WeaponEvent::DryFire });
        assert_eq!(rendered(&mut g) > before, fresh, "{set:?}");
        // Whatever else is in the set: nothing here may panic, and each of these renders per event in both.
        // A reload's own sounds follow its progress, not these, and are tested on their own.
        for event in [WeaponEvent::AdsRaised, WeaponEvent::AdsLowered] {
            let before = rendered(&mut g);
            tell(&mut g, SimEvent::Weapon { soldier, event });
            assert!(rendered(&mut g) > before, "{set:?} {event:?}");
        }
        let near = g.me().body.pos;
        for surface in [Surface::Dirt, Surface::Grass, Surface::Gravel, Surface::Wood, Surface::Concrete, Surface::Metal] {
            let before = rendered(&mut g);
            tell(&mut g, SimEvent::Impact { point: near, normal: Vec3::Y, surface });
            g.seconds(0.1);
            assert!(rendered(&mut g) > before, "{set:?} {surface:?}");
        }
    }
}

#[test]
fn a_helicopter_changes_to_the_other_sets_loops_at_once() {
    let _one = serial();
    let mut g = Game::on("town", "aas");
    let handles = |g: &mut Game| -> Vec<AssetId<AudioSource>> {
        let world = g.app.world_mut();
        world.query_filtered::<&AudioPlayer<AudioSource>, With<prs_audio::RotorLoop>>().iter(world).map(|player| player.0.id()).collect()
    };
    g.frames(3);
    let new = handles(&mut g);
    assert!(!new.is_empty(), "this map has helicopters");
    g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().choose_sound_set(prs_audio::SoundSet::Current);
    g.frames(3);
    let current = handles(&mut g);
    assert_eq!(current.len(), new.len());
    assert!(current.iter().all(|id| !new.contains(id)));
}

/// Counts your own sounds started, which is how a test can tell one was played without hearing it. Sounds
/// from out in the world, such as other people shooting on the range, come from a direction and are left out.
#[derive(Resource, Default)]
struct SoundsMade(usize);

fn count_sounds(g: &mut Game) {
    g.app.init_resource::<SoundsMade>().add_systems(Update, |started: Query<&PlaybackSettings, Added<AudioPlayer<AudioSource>>>, mut made: ResMut<SoundsMade>| {
        made.0 += started.iter().filter(|playing| !playing.spatial).count();
    });
}

/// Steps a frame at a time until `seconds` have gone, noting how far through the reload each new sound came.
fn reload_sounds_over(g: &mut Game, seconds: f32) -> Vec<f32> {
    let mut heard = Vec::new();
    for _ in 0..(seconds * 60.0) as usize {
        let before = g.app.world().resource::<SoundsMade>().0;
        let progress = g.app.world().resource::<prs_runtime::LocalView>().blended.reload_progress;
        g.frames(1);
        if g.app.world().resource::<SoundsMade>().0 > before {
            heard.push(progress.max(g.app.world().resource::<prs_runtime::LocalView>().blended.reload_progress));
        }
    }
    heard
}

/// The magazine is heard coming off when the hand gets to it and going in when the hand brings the new one
/// back, at the points of the reload the hands move at, in either sound set.
#[test]
fn reload_sounds_come_when_the_hands_get_there_and_stop_if_it_is_abandoned() {
    let _one = serial();
    let mut g = Game::range();
    g.in_the_open();
    count_sounds(&mut g);
    let hand = g.sim().body_cfg.reload.clone();
    for set in [prs_audio::SoundSet::New, prs_audio::SoundSet::Current] {
        g.app.world_mut().resource_mut::<prs_audio::AudioLevels>().choose_sound_set(set);
        // A round out, so there is something to reload, and long enough for it to have landed and been heard.
        g.click(MouseButton::Left);
        g.seconds(3.0);
        g.tap(KeyCode::KeyR);
        let heard = reload_sounds_over(&mut g, 3.5);
        assert_eq!(heard.len(), 3, "{set:?}: {heard:?}");
        assert!((heard[0] - hand.at_magazine).abs() < 0.03, "{set:?}: out at {}", heard[0]);
        // Started early by as long as it takes to seat, so it lands when the hand gets there.
        assert!(heard[1] < hand.back_at_magazine && hand.back_at_magazine - heard[1] < 0.12, "{set:?}: in at {}", heard[1]);
        assert!((heard[2] - hand.leave_magazine).abs() < 0.03, "{set:?}: back at {}", heard[2]);
    }
    g.click(MouseButton::Left);
    g.seconds(3.0);
    g.tap(KeyCode::KeyR);
    let early = reload_sounds_over(&mut g, 0.6);
    assert_eq!(early.len(), 1, "{early:?}");
    g.tap(KeyCode::Digit2);
    let abandoned = reload_sounds_over(&mut g, 3.0);
    assert_eq!(g.app.world().resource::<prs_runtime::LocalView>().blended.reload_progress, 0.0, "putting the gun away abandons it");
    // Changing guns makes its own sounds. Nothing more of the reload is heard on top of them.
    g.tap(KeyCode::Digit1);
    g.seconds(3.0);
    g.tap(KeyCode::Digit2);
    let changed = reload_sounds_over(&mut g, 3.0);
    assert_eq!(abandoned.len(), changed.len(), "after abandoning {abandoned:?}, just changing {changed:?}");
}
