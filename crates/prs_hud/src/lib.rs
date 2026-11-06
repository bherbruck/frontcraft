//! Deliberately minimal. No crosshair: you aim with the sights or you guess. `F3` turns on the
//! tuning overlay, whose dot marks exactly where a bullet would go.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use prs_input::{Captured, InputSettings, MouseOverUi, MouseStats};
use prs_runtime::{GameMode, LocalSoldier, LocalView, NetStatus, PendingInput, PlayerNames, SimResource};
use prs_runtime::prs_sim::Care;
use prs_runtime::prs_sim::protocol::TICK_DT;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::prs_sim::inventory::Slot;
use prs_runtime::prs_sim::TransitKind;
use prs_runtime::prs_sim::movement::Stance;
use prs_runtime::prs_sim::weapons::{FireMode, WeaponConfig};
use prs_audio::AudioLevels;
use prs_voice::{VoiceSettings, VoiceTuning, Voices};
use prs_view::ViewSettings;

mod board;
mod cockpit;
mod deploy;
mod field;
mod blood;
mod map;
mod saved;
mod theme;
mod voice;
mod nametags;
mod overview;
mod scale;
mod vignette;

pub use deploy::{DeployAction, DeployButton, Screens};
pub use scale::{HudScaleRules, HudSize};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        // What it shows of voice, and the settings for it, come from there, whoever else adds it.
        if !app.is_plugin_added::<prs_voice::VoicePlugin>() {
            app.add_plugins(prs_voice::VoicePlugin);
        }
        app.insert_resource(DebugOverlay(false))
            .insert_resource(CarryPanelOpen(false))
            .add_systems(Startup, spawn_hud)
            .add_systems(Startup, saved::load_settings)
            .add_systems(Update, saved::save_settings)
            .add_observer(apply_step)
            .add_systems(Startup, (vignette::spawn_vignette, blood::spawn_blood))
            .add_systems(Update, blood::update_blood)
            .add_systems(Update, vignette::update_vignette)
            .add_systems(
                Update,
                (toggle_debug, hover_menu, update_menu_values, update_hud, update_prompt, update_carry_panel, update_aim_dot).chain(),
            )
            .add_plugins((cockpit::CockpitPlugin, deploy::DeployPlugin, map::MapPlugin, overview::OverviewPlugin, scale::ScalePlugin, board::BoardPlugin, voice::VoiceHudPlugin, nametags::NameTagPlugin, field::FieldPlugin));
    }
}

/// Faster than this nobody needs telling how to get out of their seat.
const PROMPT_SEAT_SPEED: f32 = 1.0;

#[derive(Resource)]
struct DebugOverlay(bool);

/// Whether the panel showing what you carry is open.
#[derive(Resource)]
pub struct CarryPanelOpen(pub bool);

#[derive(Component)]
struct CarryPanel;
#[derive(Component)]
struct Prompt;

#[derive(Component)]
struct AmmoText;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct DebugText;
#[derive(Component)]
struct StaminaBar;
#[derive(Component)]
struct AimDot;
#[derive(Component)]
struct Intro;

#[derive(Component, Clone, Copy, PartialEq)]
enum Setting {
    Fov,
    ZoomFov,
    Sensitivity,
    MasterVolume,
    EffectsVolume,
    SoundSet,
    RestStopsAtArc,
    Microphone,
    VoiceVolume,
    HudSize,
    Mute(u16),
}

/// A menu button: changes `Setting` by one step in `direction`.
#[derive(Component, Clone, Copy)]
struct Step {
    setting: Setting,
    direction: f32,
}

/// One press of a volume button.
const VOLUME_STEP: f32 = 0.05;

const INK: Color = Color::srgba(0.95, 0.95, 0.9, 0.9);

const INTRO: &str = "CLICK TO PLAY\n\n\
    WASD move   SHIFT sprint / hold breath while aiming\n\
    SPACE jump   W + SPACE climb   X crouch   Z prone   Q E lean left, right: again to come back\n\
    LEFT fire   RIGHT tap sights, hold zoom   R reload   V fire mode\n\
    G frag, smoke: hold LEFT and let go to throw, RIGHT for underarm\n\
    1 2 guns   3 4 bandage, medkit: hold LEFT to treat an ally in front of you or yourself\n\
    F use what you face: a kit, a door or hatch, and get out of a seat   1 2 3 4 in a seat change seat\n\
    B hold to bandage yourself, anywhere   I what you carry   M map   hold TAB scoreboard   ENTER squad and kit   T place rally point (squad leader)\n\
    Hold CAPS LOCK, H or MOUSE 4 talk out loud   Y or MOUSE 5 squad radio: turn the microphone on below\n\
    C rest the gun   ALT free look   [ ] sensitivity   F3 tuning overlay   F4 view from outside   ESC pause\n\n\
    Stop. Get low. Let it settle. One shot.";

fn spawn_hud(mut commands: Commands) {
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    commands.spawn((
        AmmoText,
        Text::new(""),
        font(26.0),
        TextColor(INK),
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            bottom: px(46),
            ..default()
        },
    ));
    commands.spawn((
        StatusText,
        Text::new(""),
        font(15.0),
        TextColor(INK),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            bottom: px(24),
            ..default()
        },
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                bottom: px(28),
                width: px(150),
                height: px(5),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_child((
            StaminaBar,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(INK),
        ));
    commands.spawn((
        Prompt,
        Text::new(""),
        font(17.0),
        TextColor(INK),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            bottom: percent(22),
            ..default()
        },
    ));
    commands.spawn((
        CarryPanel,
        Text::new(""),
        font(16.0),
        TextColor(INK),
        Visibility::Hidden,
        BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            top: px(24),
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));
    commands.spawn((
        DebugText,
        Text::new(""),
        font(14.0),
        TextColor(Color::srgb(0.75, 1.0, 0.75)),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: px(28),
            top: px(24),
            ..default()
        },
    ));
    commands.spawn((
        AimDot,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(4),
            height: px(4),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::srgb(0.5, 1.0, 0.5)),
    ));
    commands
        .spawn((
            Intro,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.55)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(5),
                    ..default()
                },
            ))
            .with_children(|column| {
                column.spawn((
                    Text::new(INTRO),
                    font(20.0),
                    TextColor(INK),
                    TextLayout::justify(Justify::Center),
                    Node { margin: UiRect::bottom(px(14)), ..default() },
                ));
                for (label, setting) in [
                    ("FOV", Setting::Fov),
                    ("Zoom FOV", Setting::ZoomFov),
                    ("Mouse sensitivity", Setting::Sensitivity),
                    ("Volume", Setting::MasterVolume),
                    ("Effects volume", Setting::EffectsVolume),
                    ("Sound set", Setting::SoundSet),
                    ("Rest stops at arc", Setting::RestStopsAtArc),
                    ("Microphone", Setting::Microphone),
                    ("Voice volume", Setting::VoiceVolume),
                    ("HUD size", Setting::HudSize),
                ] {
                    menu_row(column, label, setting);
                }
                column.spawn((
                    voice::MuteList,
                    Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(5), ..default() },
                ));
            });
        });
}

fn menu_row(parent: &mut ChildSpawnerCommands, label: &str, setting: Setting) {
    let font = TextFont { font_size: FontSize::Px(16.0), ..default() };
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(10),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                font.clone(),
                TextColor(INK),
                TextLayout::justify(Justify::Right),
                Node { width: px(190), ..default() },
            ));
            step_button(row, "-", setting, -1.0);
            row.spawn((
                setting,
                Text::new(""),
                font.clone(),
                TextColor(INK),
                TextLayout::justify(Justify::Center),
                Node { width: px(70), ..default() },
            ));
            step_button(row, "+", setting, 1.0);
        });
}

fn step_button(parent: &mut ChildSpawnerCommands, label: &str, setting: Setting, direction: f32) {
    parent
        .spawn((
            Button,
            Step { setting, direction },
            Hovered::default(),
            Node {
                width: px(28),
                height: px(24),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON),
        ))
        .with_child((
            Text::new(label),
            TextFont { font_size: FontSize::Px(16.0), ..default() },
            TextColor(INK),
        ));
}

const BUTTON: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.3);

fn apply_step(
    activate: On<Activate>,
    steps: Query<&Step>,
    mut view: ResMut<ViewSettings>,
    mut input: ResMut<InputSettings>,
    mut audio: ResMut<AudioLevels>,
    mut voice: ResMut<VoiceSettings>,
    mut voices: ResMut<Voices>,
    tuning: Res<VoiceTuning>,
    mut size: ResMut<HudSize>,
    rules: Res<HudScaleRules>,
) {
    let Ok(step) = steps.get(activate.entity) else {
        return;
    };
    match step.setting {
        Setting::Fov => {
            let fov = view.fov();
            view.set_fov(fov + 2.0 * step.direction);
        }
        Setting::ZoomFov => {
            let fov = view.zoom_fov();
            view.set_zoom_fov(fov + 2.0 * step.direction);
        }
        Setting::Sensitivity => input.adjust_sensitivity(1.1_f32.powf(step.direction)),
        Setting::MasterVolume => {
            let volume = audio.master();
            audio.set_master(volume + VOLUME_STEP * step.direction);
        }
        Setting::EffectsVolume => {
            let volume = audio.effects();
            audio.set_effects(volume + VOLUME_STEP * step.direction);
        }
        // Only two to choose from, so either button goes to the other.
        Setting::SoundSet => {
            let other = match audio.sound_set() {
                prs_audio::SoundSet::New => prs_audio::SoundSet::Current,
                prs_audio::SoundSet::Current => prs_audio::SoundSet::New,
            };
            audio.choose_sound_set(other);
        }
        Setting::RestStopsAtArc => input.rest_stops_at_arc = step.direction > 0.0,
        Setting::Microphone => voice.set_mic_on(step.direction > 0.0),
        Setting::VoiceVolume => {
            let volume = voice.volume();
            voice.set_volume(volume + tuning.volume_step * step.direction);
        }
        Setting::HudSize => size.0 = rules.held(size.0 + rules.player_step * step.direction),
        Setting::Mute(player) => voices.set_muted(player, step.direction > 0.0),
    }
}

fn hover_menu(
    mut buttons: Query<(&Hovered, &mut BackgroundColor), With<Step>>,
    captured: Res<Captured>,
    mut over_ui: ResMut<MouseOverUi>,
) {
    let mut any = false;
    for (hovered, mut background) in &mut buttons {
        any |= hovered.get();
        let color = if hovered.get() { BUTTON_HOVER } else { BUTTON };
        background.set_if_neq(BackgroundColor(color));
    }
    over_ui.0 = any && !captured.0;
}

fn update_menu_values(
    view: Res<ViewSettings>,
    input: Res<InputSettings>,
    audio: Res<AudioLevels>,
    voice: Res<VoiceSettings>,
    voices: Res<Voices>,
    size: Res<HudSize>,
    new: Query<(), Added<Setting>>,
    mut values: Query<(&Setting, &mut Text)>,
) {
    let changed = view.is_changed() || input.is_changed() || audio.is_changed() || voice.is_changed() || size.is_changed();
    if !changed && !voices.is_changed() && new.is_empty() {
        return;
    }
    for (setting, mut text) in &mut values {
        text.0 = match setting {
            Setting::Fov => format!("{:.0}", view.fov()),
            Setting::ZoomFov => format!("{:.0}", view.zoom_fov()),
            Setting::Sensitivity => format!("{:.2}", input.sensitivity),
            Setting::MasterVolume => format!("{:.0}%", audio.master() * 100.0),
            Setting::EffectsVolume => format!("{:.0}%", audio.effects() * 100.0),
            Setting::SoundSet => format!("{:?}", audio.sound_set()),
            Setting::RestStopsAtArc => if input.rest_stops_at_arc { "On" } else { "Off" }.to_string(),
            Setting::Microphone => if voice.mic_on() { "On" } else { "Off" }.to_string(),
            Setting::VoiceVolume => format!("{:.0}%", voice.volume() * 100.0),
            Setting::HudSize => format!("{:.0}%", size.0 * 100.0),
            Setting::Mute(player) => if voices.is_muted(*player) { "Muted" } else { "Heard" }.to_string(),
        };
    }
}

fn toggle_debug(keys: Res<ButtonInput<KeyCode>>, mut debug: ResMut<DebugOverlay>) {
    if keys.just_pressed(KeyCode::F3) {
        debug.0 = !debug.0;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    view: Res<LocalView>,
    captured: Res<Captured>,
    debug: Res<DebugOverlay>,
    settings: Res<InputSettings>,
    mouse_stats: Res<MouseStats>,
    sim: Res<SimResource>,
    net: Res<NetStatus>,
    time: Res<Time>,
    mut ammo: Query<&mut Text, (With<AmmoText>, Without<StatusText>, Without<DebugText>)>,
    mut status: Query<&mut Text, (With<StatusText>, Without<AmmoText>, Without<DebugText>)>,
    mut debug_text: Query<(&mut Text, &mut Visibility), (With<DebugText>, Without<AmmoText>, Without<StatusText>, Without<Intro>)>,
    mut stamina: Query<&mut Node, With<StaminaBar>>,
    mut intro: Query<&mut Visibility, (With<Intro>, Without<DebugText>)>,
) {
    let s = &view.blended;
    for mut text in &mut ammo {
        let feel = magazine_feel(s.ammo, &sim.0.weapons[s.weapon]);
        text.0 = if s.life == Life::Up { format!("{feel}  +{}", s.spare_magazines) } else { String::new() };
    }
    for mut text in &mut status {
        let mode = match s.mode {
            FireMode::Semi => "SEMI",
            FireMode::Auto => "AUTO",
        };
        let stance = match s.stance {
            Stance::Stand => "STANDING",
            Stance::Crouch => "CROUCHED",
            Stance::Prone => "PRONE",
        };
        let reload = if s.reload_fraction > 0.0 { "RELOADING  " } else { "" };
        let arm = if s.arm_wound > 0.0 { "ARM  " } else { "" };
        let leg = if s.leg_wound > 0.0 { "LEG  " } else { "" };
        let rested = if s.rested { "RESTED  " } else { "" };
        text.0 = if let (Life::Up, Some((gear, manual))) = (s.life, s.driving) {
            format!("{arm}{leg}{}", gear_text(gear, manual))
        } else if s.life == Life::Up {
            format!("{arm}{leg}{reload}{rested}{stance}  {mode}")
        } else {
            String::new()
        };
    }
    for mut node in &mut stamina {
        node.width = percent(s.stamina * 100.0);
    }
    for mut visibility in &mut intro {
        visibility.set_if_neq(if captured.0 { Visibility::Hidden } else { Visibility::Inherited });
    }
    for (mut text, mut visibility) in &mut debug_text {
        visibility.set_if_neq(if debug.0 { Visibility::Inherited } else { Visibility::Hidden });
        if debug.0 {
            let speed = Vec2::new(s.velocity.x, s.velocity.z).length();
            let network = match &*net {
                NetStatus::Playing { rtt_ms, lead_ticks, reconciles, players, over } => {
                    format!("\nnet rtt {rtt_ms:.0} ms over {over}, view {lead_ticks} ticks behind\nreconciles {reconciles}, players {players}")
                }
                _ => String::new(),
            };
            text.0 = format!(
                "fps {:>4.0}\nspeed {speed:>5.2} m/s\nrounds {}\nhealth {:>4.0}\ngun wander {:>5.2} deg\nheat {:>5.2}\nbreath {:>4.0}%\nstamina {:>4.0}%\nsensitivity {:.2}\nmouse spikes dropped {} (largest {:.0} px){network}",
                1.0 / time.delta_secs().max(0.0001),
                s.ammo,
                s.health,
                s.instability,
                s.heat,
                s.breath * 100.0,
                s.stamina * 100.0,
                settings.sensitivity,
                mouse_stats.spikes_dropped,
                mouse_stats.largest_spike,
            );
        }
    }
}

/// A side's name from the map, or a number if the map does not say.
/// What to call a player anywhere on screen: what the server says they are called, or failing that
/// their number. Always shown as plain text.
pub(crate) fn player_name(names: &PlayerNames, soldier: usize, bot: bool) -> String {
    match names.of(soldier) {
        Some(name) if !bot => name.to_owned(),
        _ if bot => format!("Bot {soldier}"),
        _ => format!("Player {soldier}"),
    }
}

pub(crate) fn team_name(level: &prs_runtime::prs_sim::Level, team: u8) -> String {
    level.teams.get(team as usize).map_or_else(|| format!("Team {}", team + 1), |t| t.name.clone())
}

/// How full a magazine feels in the hand, which is all a soldier knows without counting. Never says
/// more than is true, and is exact only about the things that are: full, and nothing left.
pub fn magazine_feel(rounds: u32, cfg: &WeaponConfig) -> &'static str {
    let share = rounds as f32 / cfg.magazine.max(1) as f32;
    match share {
        s if s <= 0.0 => "EMPTY",
        s if s >= 1.0 => "FULL",
        s if s >= 0.66 => "HEAVY",
        s if s >= 0.33 => "HALF",
        s if s >= 0.1 => "LIGHT",
        _ => "NEARLY EMPTY",
    }
}

/// One line of quiet text for what is happening to you or what you can do next.
fn update_prompt(
    view: Res<LocalView>,
    sim: Res<SimResource>,
    net: Res<NetStatus>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    mut prompt: Query<&mut Text, With<Prompt>>,
) {
    let Ok(mut text) = prompt.single_mut() else {
        return;
    };
    let s = &view.blended;
    let player = game.0.state.player(local.0);
    let care_name = |care: Care| match care {
        Care::Bandage => "Bandaging",
        Care::Medkit => "Treating",
    };
    let line = net.problem().unwrap_or_else(|| match s.life {
        Life::Dead => match player {
            Some(player) => format!("Dead. Back in {:.0}", player.wait_seconds(sim.0.tick)),
            None => "Dead.".to_string(),
        },
        Life::Down => {
            let waited = player.and_then(|p| p.down_since).map_or(0.0, |since| sim.0.tick.saturating_sub(since) as f32 * TICK_DT);
            let give_up = waited >= game.0.mode.cfg.give_up_after;
            let hint = if give_up { "   ENTER give up" } else { "" };
            format!("Down. Waiting for a medic. {:.0}{hint}", s.life_clock.max(0.0))
        }
        Life::Up => {
            if let Some((care, progress)) = s.treating {
                let who = if s.treating_yourself { "yourself" } else { "an ally" };
                format!("{} {who}  {:.0}%", care_name(care), progress * 100.0)
            } else if sim.0.soldiers.get(local.0).is_some_and(|me| me.transit.is_some_and(|t| matches!(t.kind, TransitKind::Across { .. }))) {
                "Moving seats".to_string()
            } else if matches!(s.selected, Slot::Bandage | Slot::Medkit) {
                let what = if s.selected == Slot::Bandage { "bandage" } else { "medkit" };
                format!("Hold fire to use {what}")
            } else if matches!(s.selected, Slot::Frag | Slot::Smoke) {
                let what = if s.selected == Slot::Frag { "the frag" } else { "the smoke" };
                format!("Hold fire and let go to throw {what}. Right mouse for underarm. G for the other")
            } else if s.rest_refused {
                "Nothing here to rest the gun on".to_string()
            } else if s.lifting || s.pushing {
                "Keep holding F".to_string()
            } else if s.push_next {
                "Hold F to push".to_string()
            } else if s.lift_refused {
                "Something is in the way".to_string()
            } else if let Some(crew) = s.lift_next {
                if crew > 1 { format!("Hold F to right it: it takes {crew}") } else { "Hold F to right it".to_string() }
            } else if let Some(door) = s.door_in_reach {
                format!("F  get in: {door}")
            } else if let (true, Some(weapon)) = (s.kit_is_next, s.kit_gun) {
                format!("F  take {} from the kit", sim.0.weapons[weapon].name)
            } else if s.kit_is_next {
                "F  go through the kit".to_string()
            } else if s.driving.is_some() {
                "F  get out   1-4  move seat   Q/E  gears   Space  handbrake".to_string()
            } else if s.seat.is_some() {
                "F  get out   1-4  move seat".to_string()
            } else {
                String::new()
            }
        }
    });
    if text.0 != line {
        text.0 = line;
    }
}

/// The gear for the driver, and who is changing it.
fn gear_text(gear: i8, manual: bool) -> String {
    match (gear, manual) {
        (g, _) if g < 0 => "GEAR R".to_string(),
        (g, true) => format!("GEAR {g}  MANUAL"),
        (g, false) => format!("GEAR {g}  AUTO"),
    }
}

/// What you carry, on I. Magazines are by feel too, since that is what you would have to go on.
fn update_carry_panel(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    captured: Res<Captured>,
    mut open: ResMut<CarryPanelOpen>,
    mut panel: Query<(&mut Text, &mut Visibility), With<CarryPanel>>,
) {
    if captured.0 && keys.just_pressed(KeyCode::KeyI) {
        open.0 = !open.0;
    }
    let Ok((mut text, mut visibility)) = panel.single_mut() else {
        return;
    };
    visibility.set_if_neq(if open.0 { Visibility::Inherited } else { Visibility::Hidden });
    if !open.0 {
        return;
    }
    let sim = &sim.0;
    let soldier = &sim.soldiers[local.0];
    let inventory = &soldier.inventory;
    let describe = |rounds: u32, magazines: &[u32], weapon: &WeaponConfig| {
        let spare: Vec<&str> = magazines.iter().map(|&m| magazine_feel(m, weapon)).collect();
        format!("{}  {}\n    spare: {}", weapon.name, magazine_feel(rounds, weapon), if spare.is_empty() { "none".into() } else { spare.join(", ") })
    };
    let gun_line = |slot: Slot, stored: usize| {
        if inventory.held == slot {
            let w = &soldier.weapon;
            describe(w.ammo, &w.magazines, soldier.held_config(&sim.weapons))
        } else if let Some(gun) = &inventory.stash.guns[stored] {
            describe(gun.ammo, &gun.magazines, &sim.weapons[gun.weapon])
        } else {
            "nothing".into()
        }
    };
    let mark = |slot: Slot| if inventory.selected == slot { ">" } else { " " };
    let line = format!(
        "{}1 {}\n{}2 {}\n{}3 Bandages  {}\n{}4 Medkits  {}\n{}G Frags  {}\n{}G Smoke  {}\n\nCarrying {:.1} kg",
        mark(Slot::Primary),
        gun_line(Slot::Primary, 0),
        mark(Slot::Sidearm),
        gun_line(Slot::Sidearm, 1),
        mark(Slot::Bandage),
        inventory.stash.bandages,
        mark(Slot::Medkit),
        inventory.stash.medkits,
        mark(Slot::Frag),
        inventory.stash.frags,
        mark(Slot::Smoke),
        inventory.stash.smokes,
        soldier.load(&sim.inventory_cfg, &sim.weapons),
    );
    if text.0 != line {
        text.0 = line;
    }
}

/// Puts the dot on the gun's true aim point, which is not the middle of the screen.
fn update_aim_dot(
    view: Res<LocalView>,
    input: Res<PendingInput>,
    debug: Res<DebugOverlay>,
    windows: Query<&Window>,
    cameras: Query<&Projection, With<prs_view::WorldCamera>>,
    mut dot: Query<(&mut Node, &mut Visibility), With<AimDot>>,
    settings: Res<ViewSettings>,
    hud_scale: Res<UiScale>,
) {
    let (Ok(window), Ok(Projection::Perspective(perspective)), Ok((mut node, mut visibility))) =
        (windows.single(), cameras.single(), dot.single_mut())
    else {
        return;
    };
    // While free looking the gun can be anywhere, even behind you.
    let shown = debug.0 && input.free_look.length() < 0.01;
    visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    let scale = window.height() * 0.5 / (perspective.fov * 0.5).tan();
    let sights = prs_view::sight_offset(view.blended.aim_offset, view.blended.recoil, settings.camera_recoil_share);
    let along = scale::to_hud(Vec2::new(-sights.x.tan(), -sights.y.tan()) * scale, &hud_scale);
    let (dx, dy) = (along.x, along.y);
    node.margin = UiRect::new(px(dx - 2.0), px(0), px(dy - 2.0), px(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sound_set_button_goes_to_the_other_set_and_says_which() {
        let mut app = App::new();
        app.insert_resource(ViewSettings::default())
            .insert_resource(InputSettings { sensitivity: 1.0, zoom_hold_seconds: 0.18, free_look_yaw_limit: 2.15, free_look_return_rate: 12.0, rest_stops_at_arc: true })
            .insert_resource(AudioLevels::default())
            .init_resource::<VoiceSettings>()
            .init_resource::<Voices>()
            .insert_resource(VoiceTuning::default())
            .insert_resource(HudSize(1.0))
            .insert_resource(HudScaleRules::default())
            .add_observer(apply_step)
            .add_systems(Update, update_menu_values);
        let button = app.world_mut().spawn(Step { setting: Setting::SoundSet, direction: 1.0 }).id();
        let shown = app.world_mut().spawn((Setting::SoundSet, Text::new(""))).id();
        app.update();
        assert_eq!(app.world().get::<Text>(shown).unwrap().0, "New");
        for expected in ["Current", "New"] {
            app.world_mut().trigger(Activate { entity: button });
            app.update();
            assert_eq!(app.world().get::<Text>(shown).unwrap().0, expected);
        }
        assert_eq!(app.world().resource::<AudioLevels>().chosen_sound_set(), Some(prs_audio::SoundSet::New));
    }

    #[test]
    fn magazine_feel_only_gets_exact_at_the_ends() {
        let cfg = WeaponConfig::assault_rifle();
        assert_eq!(magazine_feel(30, &cfg), "FULL");
        assert_eq!(magazine_feel(31, &cfg), "FULL", "with one up the spout");
        assert_eq!(magazine_feel(0, &cfg), "EMPTY");
        assert_eq!(magazine_feel(1, &cfg), "NEARLY EMPTY");
        // A few rounds either way feels the same, which is what keeps it honest.
        assert_eq!(magazine_feel(25, &cfg), magazine_feel(21, &cfg));
        assert_eq!(magazine_feel(15, &cfg), magazine_feel(11, &cfg));
        assert_ne!(magazine_feel(25, &cfg), magazine_feel(15, &cfg));
    }

    #[test]
    fn a_belt_feels_the_same_at_the_same_share() {
        let lmg = WeaponConfig::machine_gun();
        assert_eq!(magazine_feel(50, &lmg), "HALF");
        assert_eq!(magazine_feel(15, &WeaponConfig::assault_rifle()), "HALF");
    }
}
