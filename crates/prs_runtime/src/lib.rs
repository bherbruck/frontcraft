//! Hosts the simulation inside Bevy for a local player.
//!
//! Everything client-facing talks to this crate rather than to each other: input fills
//! [`PendingInput`], presentation reads [`LocalView`] and [`SimMessage`]. With no server named, all of
//! this runs here in `tick_sim`. With one, [`net`] runs it instead and leaves those three alone.

mod bleed;
mod net;

use bevy::prelude::*;
use prs_ai::{Crowd, Troops, WalkerConfig};
use prs_mode::{Deployment, Match, Mode, ModeCmd, ModeEvent};
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_HZ};
use prs_sim::health::Life;
use prs_sim::inventory::Slot;
use prs_sim::weapons::{WeaponId, WeaponTable};
use prs_sim::seats::{Frame, SeatedAt};
use prs_sim::{Care, Level, Sim, SimEvent, SoldierId};

pub use net::{NetStatus, PlayerNames, VoiceLink};
pub use prs_net::wire::{VoiceChannel, VoiceFrame, VoiceUp};
pub use prs_mode;
pub use bleed::BleedShown;
pub use prs_sim;

const DEFAULT_MAP: &str = "town";
const DEFAULT_MODE: &str = "aas";

pub struct RuntimePlugin;

impl Plugin for RuntimePlugin {
    fn build(&self, app: &mut App) {
        common_setup(app);
        match requested_server() {
            Some(server) => {
                let mut sim = net::placeholder_sim();
                let game = Match::new(Mode::free_practice(), &mut sim);
                insert_sim(app, sim, 0);
                app.insert_resource(GameMode(game));
                net::build(app, server, requested_name());
            }
            None => {
                let mut sim = Sim::new(choose_level());
                let local = sim.spawn_soldier(0x5EED);
                let mode = choose_mode();
                let practising = mode.cfg.deployment == Deployment::Automatic;
                let mut game = Match::new(mode, &mut sim);
                let bots = if practising {
                    Bots::practice(&mut sim, &mut game)
                } else {
                    if let Some(count) = requested_walkers() {
                        game.mode.cfg.bots_per_team = Some(count / 2);
                    }
                    Bots::Troops(Troops::new(&sim, prs_ai::tuned_troops()))
                };
                game.join(&mut sim, local, None, false);
                game.fill_bots(&mut sim);
                insert_sim(app, sim, local);
                app.insert_resource(LevelReady)
                    .insert_resource(GameMode(game))
                    .insert_resource(bots)
                    .add_systems(FixedUpdate, tick_sim);
            }
        }
    }
}

/// What is the same whether a server is playing or this machine is.
fn common_setup(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
        .insert_resource(ThirdPerson(false))
        .insert_resource(BleedShown::new(prs_sim::prs_mods::tuned("view.bleed", prs_sim::health::BleedLook::default())))
        .add_systems(Update, bleed::update_bleed_shown)
        .insert_resource(PendingInput::default())
        // Empty when playing alone, and filled in by the server online.
        .init_resource::<PlayerNames>()
        .insert_resource(LookScale(1.0))
        .insert_resource(DrawnGun::default())
        .insert_resource(VehicleFrames::default())
        .insert_resource(HeliFrames::default())
        .insert_resource(BuddyInput::default())
        .insert_resource(NetStatus::Offline)
        .insert_resource(PendingModeCmds::default())
        .add_message::<SimMessage>()
        .add_message::<ModeMessage>()
            .add_systems(
                RunFixedMainLoop,
                (blend_view, blend_vehicles).chain().in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            );
}

fn insert_sim(app: &mut App, sim: Sim, local: SoldierId) {
    let view = LocalState::capture(&sim, local);
    app.insert_resource(LocalSoldier(local))
        .insert_resource(SimResource(sim))
        .insert_resource(LocalView { previous: view, current: view, blended: view });
}

/// Present once [`SimResource`] holds the level being played, which online takes until the server has
/// said which. Whatever builds from the level waits for this.
#[derive(Resource, Clone, Copy)]
pub struct LevelReady;

/// A setting given on the command line as `--name value` or `--name=value`, which is what a shortcut can
/// pass, or else from the environment.
#[cfg(not(target_arch = "wasm32"))]
fn desktop_setting(flag: &str, var: &str) -> Option<String> {
    from_args(std::env::args().skip(1), flag).or_else(|| std::env::var(var).ok())
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn from_args(args: impl Iterator<Item = String>, flag: &str) -> Option<String> {
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            return Some(value.to_owned());
        }
        if arg == flag {
            return args.next();
        }
    }
    None
}

/// The server to play on, if one was asked for: `--server wss://host` or `PRS_SERVER`. Otherwise this is
/// single player.
#[cfg(not(target_arch = "wasm32"))]
fn requested_server() -> Option<String> {
    desktop_setting("--server", "PRS_SERVER")
}

/// `?server=ws://host:port`.
#[cfg(target_arch = "wasm32")]
fn requested_server() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    server_from_query(&search)
}

/// What the player would like to be called on a server, if they have said. The server has the last word.
#[cfg(not(target_arch = "wasm32"))]
fn requested_name() -> String {
    desktop_setting("--name", "PRS_NAME").unwrap_or_default()
}

/// `?name=...`, as the page puts it there.
#[cfg(target_arch = "wasm32")]
fn requested_name() -> String {
    let search = web_sys::window().and_then(|window| window.location().search().ok()).unwrap_or_default();
    name_from_query(&search)
}

/// Undoes what a browser does to text in a query: `+` for spaces and `%XX` for everything else.
#[cfg(any(target_arch = "wasm32", test))]
fn name_from_query(search: &str) -> String {
    let Some(value) = search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("name=")) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let mut rest = value.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let hex = |at: usize| after.get(at).and_then(|digit| (*digit as char).to_digit(16));
        match (byte, hex(0), hex(1)) {
            (b'%', Some(high), Some(low)) => {
                bytes.push((high * 16 + low) as u8);
                rest = &after[2..];
            }
            (b'+', _, _) => {
                bytes.push(b' ');
                rest = after;
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Browsers and people both sometimes escape what is in a query, so either way works here.
#[cfg(any(target_arch = "wasm32", test))]
fn server_from_query(search: &str) -> Option<String> {
    let value = search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("server="))?;
    Some(value.replace("%3A", ":").replace("%3a", ":").replace("%2F", "/").replace("%2f", "/"))
}

/// The map to play: the one asked for if it exists, otherwise [`DEFAULT_MAP`].
fn choose_level() -> Level {
    let name = requested_map().unwrap_or_else(|| DEFAULT_MAP.into());
    Level::named(&name).unwrap_or_else(|| {
        warn!("no map called {name}, using {DEFAULT_MAP}");
        Level::named(DEFAULT_MAP).expect("the default map exists")
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn requested_map() -> Option<String> {
    desktop_setting("--map", "PRS_MAP")
}

/// A browser has no environment, so the page address says: `?map=town`.
#[cfg(target_arch = "wasm32")]
fn requested_map() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    map_from_query(&search).map(str::to_owned)
}

#[cfg(any(target_arch = "wasm32", test))]
fn map_from_query(search: &str) -> Option<&str> {
    search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("map="))
}

/// How many soldiers should walk about, if asked. `PRS_WALKERS=0`, and scripted screenshots have none
/// unless they ask, so that nobody strolls through a picture of something else.
#[cfg(not(target_arch = "wasm32"))]
fn requested_walkers() -> Option<usize> {
    match std::env::var("PRS_WALKERS") {
        Ok(count) => count.parse().ok(),
        Err(_) => std::env::var("PRS_SHOT").is_ok().then_some(0),
    }
}

/// `?walkers=20`.
#[cfg(target_arch = "wasm32")]
fn requested_walkers() -> Option<usize> {
    let search = web_sys::window()?.location().search().ok()?;
    search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("walkers="))?.parse().ok()
}

/// The mode to play: the one asked for, or else the training range is for practice and any other map
/// is for playing properly. A scripted run for a screenshot is practice unless it says otherwise,
/// because that is the mode with no screens in the way.
fn choose_mode() -> Mode {
    let map = requested_map().unwrap_or_else(|| DEFAULT_MAP.into());
    let asked = requested_mode();
    mode_for(&map, asked.as_deref(), scripted_run())
}

fn mode_for(map: &str, asked: Option<&str>, scripted: bool) -> Mode {
    let fallback = if map == "range" || scripted { "practice" } else { DEFAULT_MODE };
    let name = asked.unwrap_or(fallback);
    Mode::named(name).unwrap_or_else(|| {
        warn!("no mode called {name}, playing {fallback}");
        Mode::named(fallback).expect("the default modes exist")
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn requested_mode() -> Option<String> {
    desktop_setting("--mode", "PRS_MODE")
}

#[cfg(target_arch = "wasm32")]
fn requested_mode() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    mode_from_query(&search).map(str::to_owned)
}

#[cfg(any(target_arch = "wasm32", test))]
fn mode_from_query(search: &str) -> Option<&str> {
    search.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("mode="))
}

#[cfg(not(target_arch = "wasm32"))]
fn scripted_run() -> bool {
    std::env::var("PRS_SHOT").is_ok()
}

#[cfg(target_arch = "wasm32")]
fn scripted_run() -> bool {
    false
}

const BUDDY_SHOT_EVERY_TICKS: u32 = 170;

fn spawn_range_buddy(sim: &mut Sim) -> SoldierId {
    let mut loadout = sim.inventory_cfg.loadout;
    loadout.primary = WeaponTable::MACHINE_GUN;
    let id = sim.spawn_soldier_with(0xB0DD, 0, loadout);
    let sentry = sim.level.sentry;
    sim.set_home(id, sentry.pos, sentry.yaw);
    id
}

/// What the other soldier on the range does instead of standing on the firing line, for a scene that needs them.
#[derive(Resource, Default)]
pub struct BuddyInput(pub Option<PlayerCmd>);

fn buddy_cmd(tick: u32, yaw: f32) -> PlayerCmd {
    let mut buttons = Buttons::default();
    buttons.set(Buttons::ADS, true);
    buttons.set(Buttons::FIRE, tick % BUDDY_SHOT_EVERY_TICKS == 0);
    PlayerCmd { tick, wish_move: Vec2::ZERO, yaw, pitch: 0.012, buttons, ..default() }
}

/// Whoever isn't you, as played on this machine.
#[derive(Resource)]
enum Bots {
    /// Everyone out for a walk, and someone on the firing line.
    Practice { buddy: SoldierId, crowd: Crowd },
    /// Bots playing the match.
    Troops(Troops),
}

impl Bots {
    fn practice(sim: &mut Sim, game: &mut Match) -> Self {
        let buddy = spawn_range_buddy(sim);
        let cfg = prs_ai::tuned_walkers();
        let count = requested_walkers().unwrap_or(cfg.count);
        let crowd = Crowd::populate(sim, WalkerConfig { count, ..cfg });
        game.join(sim, buddy, None, true);
        for walker in &crowd.walkers {
            game.join(sim, walker.soldier, None, true);
        }
        Self::Practice { buddy, crowd }
    }

    fn think(&mut self, sim: &Sim, game: &Match, cmds: &mut Vec<(SoldierId, PlayerCmd)>, told: Option<PlayerCmd>) {
        match self {
            Self::Practice { buddy, crowd } => {
                let cmd = told.map_or_else(|| buddy_cmd(sim.tick, sim.level.sentry.yaw), |cmd| PlayerCmd { tick: sim.tick, ..cmd });
                cmds.push((*buddy, cmd));
                cmds.extend_from_slice(crowd.think(sim));
            }
            Self::Troops(troops) => cmds.extend_from_slice(troops.think(sim, game)),
        }
    }
}

/// The rules of the match. Online this is the server's word on them, for showing.
#[derive(Resource)]
pub struct GameMode(pub Match);

/// What the local player has asked of the mode since the last tick.
#[derive(Resource, Default)]
pub struct PendingModeCmds(pub Vec<ModeCmd>);

#[derive(Message, Clone, Copy, Debug)]
pub struct ModeMessage(pub ModeEvent);

/// Whether you are watching your own soldier from outside.
#[derive(Resource, Debug)]
pub struct ThirdPerson(pub bool);

#[derive(Resource)]
pub struct SimResource(pub Sim);

#[derive(Resource, Clone, Copy)]
pub struct LocalSoldier(pub SoldierId);

#[derive(Message, Clone, Copy, Debug)]
pub struct SimMessage(pub SimEvent);

/// Multiplier on mouse look, set by whoever owns the camera so that zooming in slows aim to match.
#[derive(Resource, Debug, Clone, Copy)]
pub struct LookScale(pub f32);

/// The local player's latest intent. The input crate overwrites it every frame; ticks read it.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PendingInput {
    /// Looking closer. Only changes the view, so the sim never sees it.
    pub zoom: bool,
    /// Yaw and pitch of the head away from the gun. Moves the view and nothing else.
    pub free_look: Vec2,
    pub wish_move: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub held: Buttons,
    /// Pressed since the last tick. Latched so a tap shorter than a tick is never lost.
    pub tapped: Buttons,
    /// With the controls of a helicopter the mouse moves this, and looking is left to free look.
    pub at_controls: bool,
    pub stick: prs_sim::helicopter::VirtualStick,
}

/// Your eye and gun as they were actually drawn this frame, including the view's cosmetic springs.
/// Your body is fitted to these, so hands stay on what you see. The sim knows nothing of it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct DrawnGun {
    pub eye: Vec3,
    pub origin: Vec3,
    pub rotation: Quat,
}

/// Runs once [`DrawnGun`] is up to date for the frame. Anything drawn to fit the gun runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GunDrawn;

/// Snapshot of the local soldier that presentation reads, so it never reaches into the sim.
#[derive(Clone, Copy, Debug)]
pub struct LocalState {
    /// How much they are being shot at, 0 to 1. From the server when there is one.
    pub suppression: f32,
    /// The seat they are in, if riding.
    pub seat: Option<SeatedAt>,
    /// At the wheel of a vehicle: the gear it is in, -1 for reverse, and whether it is being changed by hand.
    pub driving: Option<(i8, bool)>,
    /// Hands on the controls and no gun to draw.
    pub gun_put_away: bool,
    /// The door or hatch F would go through, if that is what F would do, and the same for a kit.
    pub door_in_reach: Option<&'static str>,
    pub kit_is_next: bool,
    /// A vehicle on its side or roof that F would put their hands to, and how many it takes.
    pub lift_next: Option<u32>,
    /// Their hands are on one, and something has just stopped them.
    pub lifting: bool,
    /// One end of a vehicle F would lean into, and whether they are.
    pub push_next: bool,
    pub pushing: bool,
    pub lift_refused: bool,
    pub feet: Vec3,
    pub eye_height: f32,
    pub velocity: Vec3,
    pub grounded: bool,
    pub sprinting: bool,
    pub stance: prs_sim::movement::Stance,
    pub stride: f32,
    pub stamina: f32,
    /// Gun offset from recoil, instability and sway, radians: x = yaw, y = pitch.
    pub aim_offset: Vec2,
    /// The recoil part of `aim_offset`, radians.
    pub recoil: Vec2,
    pub ads: f32,
    pub ready: f32,
    pub gun_retract: f32,
    /// What a gun set down on something has done to the eye, and how things stand with it.
    pub rest_eye: Vec3,
    pub rested: bool,
    pub rest_refused: bool,
    /// -1 = fully left, 1 = fully right.
    pub lean: f32,
    /// Where that puts the eye from upright: x = to the right, y = up.
    pub lean_eye: Vec2,
    pub reload_fraction: f32,
    /// How far through a reload, the other way round from `reload_fraction`. 0 when not reloading.
    pub reload_progress: f32,
    pub anim: prs_sim::body::BodyAnim,
    pub instability: f32,
    pub heat: f32,
    pub breath: f32,
    pub ammo: u32,
    /// Spare magazines, whatever is left in them.
    pub spare_magazines: u32,
    pub mode: prs_sim::weapons::FireMode,
    pub weapon: WeaponId,
    /// The gun's own aim as the sim has it, which stops following the view once down.
    pub gun_yaw: f32,
    pub gun_pitch: f32,
    /// On the way to another gun or to using an item, so the gun is out of the way.
    pub changing: bool,
    pub selected: Slot,
    pub life: Life,
    /// Seconds left of being down, or until coming back when dead.
    pub life_clock: f32,
    pub health: f32,
    pub bleeding: bool,
    /// Health per second running out.
    pub bleed: f32,
    pub arm_wound: f32,
    pub leg_wound: f32,
    /// What you are doing for someone, and how far through, from 0 to 1.
    pub treating: Option<(Care, f32)>,
    pub treating_yourself: bool,
    /// A kit in reach, and the gun on top of it.
    pub kit_gun: Option<WeaponId>,
    pub kit_in_reach: bool,
    pub collapse: f32,
    pub slung: f32,
    /// How far round onto the back it is, for leaning into a vehicle.
    pub on_back: f32,
}

impl LocalState {
    fn capture(sim: &Sim, id: SoldierId) -> Self {
        let soldier = &sim.soldiers[id];
        let (body, weapon) = (&soldier.body, &soldier.weapon);
        let cfg = soldier.held_config(&sim.weapons);
        let reload_total = if weapon.ammo == 0 { cfg.reload_empty_time } else { cfg.reload_time };
        let inventory = &soldier.inventory;
        let kit = sim.kit_in_reach(id).map(|i| &sim.kits[i]);
        Self {
            suppression: soldier.suppression.level,
            feet: body.pos,
            eye_height: body.eye_height,
            velocity: body.vel,
            grounded: body.grounded,
            sprinting: body.sprinting,
            stance: body.stance,
            stride: body.stride,
            stamina: body.stamina,
            aim_offset: weapon.aim_offset(),
            recoil: weapon.recoil * std::f32::consts::PI / 180.0,
            ads: weapon.ads,
            ready: weapon.ready,
            gun_retract: soldier.gun_retract,
            rest_eye: soldier.rest.eye,
            rested: soldier.rest.on,
            rest_refused: soldier.rest.refused > 0.0,
            lean: body.lean.amount,
            lean_eye: body.lean.eye,
            reload_fraction: (weapon.reload_timer / reload_total).clamp(0.0, 1.0),
            reload_progress: weapon.reload_progress(cfg),
            anim: soldier.anim,
            instability: weapon.instability,
            heat: weapon.heat,
            breath: weapon.breath,
            ammo: weapon.ammo,
            spare_magazines: weapon.magazines.len() as u32,
            mode: weapon.mode,
            weapon: weapon.weapon,
            gun_yaw: soldier.yaw,
            gun_pitch: soldier.pitch,
            changing: inventory.gun_wanted().is_some() || !inventory.selected.is_gun() || weapon.drawing(),
            selected: inventory.selected,
            life: soldier.health.life,
            life_clock: soldier.health.clock,
            health: soldier.health.health,
            bleeding: soldier.health.bleeding(),
            bleed: soldier.health.bleed,
            arm_wound: soldier.health.arm_wound,
            leg_wound: soldier.health.leg_wound,
            treating: soldier.treatment.map(|t| (t.care, 1.0 - t.left / t.total.max(1e-3))),
            treating_yourself: soldier.treatment.is_some_and(|t| t.patient == id),
            kit_gun: kit.and_then(|k| k.stash.guns.iter().flatten().next().map(|g| g.weapon)),
            kit_in_reach: kit.is_some(),
            seat: soldier.seat,
            driving: soldier.seat.filter(|_| sim.is_at_controls(id)).and_then(|at| sim.vehicles.get(at.carrier as usize)).map(|v| (v.engine.gear, v.engine.manual)),
            gun_put_away: sim.gun_put_away(id),
            door_in_reach: match sim.use_target(id) {
                Some(prs_sim::UseTarget::Door { carrier, entry, .. }) => Some(sim.carrier_entries(carrier)[entry as usize].name),
                _ => None,
            },
            kit_is_next: matches!(sim.use_target(id), Some(prs_sim::UseTarget::Kit(_))),
            lift_next: match sim.use_target(id) {
                Some(prs_sim::UseTarget::Lift { vehicle }) => Some(sim.vehicle_cfgs[sim.vehicles[vehicle].kind].lift.crew),
                _ => None,
            },
            lifting: soldier.lifting.is_some(),
            push_next: matches!(sim.use_target(id), Some(prs_sim::UseTarget::Push { .. })),
            pushing: soldier.pushing.is_some(),
            lift_refused: soldier.lift_refused > 0.0,
            collapse: soldier.anim.collapse,
            slung: soldier.anim.slung,
            on_back: soldier.anim.heave,
        }
    }

    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            feet: a.feet.lerp(b.feet, t),
            eye_height: a.eye_height.lerp(b.eye_height, t),
            velocity: a.velocity.lerp(b.velocity, t),
            stride: a.stride.lerp(b.stride, t),
            aim_offset: a.aim_offset.lerp(b.aim_offset, t),
            recoil: a.recoil.lerp(b.recoil, t),
            ads: a.ads.lerp(b.ads, t),
            ready: a.ready.lerp(b.ready, t),
            gun_retract: a.gun_retract.lerp(b.gun_retract, t),
            rest_eye: a.rest_eye.lerp(b.rest_eye, t),
            lean: a.lean.lerp(b.lean, t),
            lean_eye: a.lean_eye.lerp(b.lean_eye, t),
            anim: a.anim.lerp(&b.anim, t),
            instability: a.instability.lerp(b.instability, t),
            collapse: a.collapse.lerp(b.collapse, t),
            slung: a.slung.lerp(b.slung, t),
            on_back: a.on_back.lerp(b.on_back, t),
            suppression: a.suppression.lerp(b.suppression, t),
            ..*b
        }
    }
}

/// Simulation runs at a fixed rate and rendering does not. `blended` is the local soldier at this
/// frame's exact moment between the last two ticks, which is what stops motion from juddering.
#[derive(Resource, Debug)]
pub struct LocalView {
    previous: LocalState,
    current: LocalState,
    pub blended: LocalState,
}

fn tick_sim(
    mut sim: ResMut<SimResource>,
    mut game: ResMut<GameMode>,
    local: Res<LocalSoldier>,
    mut bots: ResMut<Bots>,
    mut cmds: Local<Vec<(SoldierId, PlayerCmd)>>,
    mut mode_cmds: ResMut<PendingModeCmds>,
    mut asked: Local<Vec<(SoldierId, ModeCmd)>>,
    mut input: ResMut<PendingInput>,
    mut view: ResMut<LocalView>,
    mut messages: MessageWriter<SimMessage>,
    mut mode_messages: MessageWriter<ModeMessage>,
    buddy: Res<BuddyInput>,
) {
    let sim = &mut sim.0;
    let game = &mut game.0;
    let cmd = PlayerCmd {
        tick: sim.tick,
        wish_move: input.wish_move,
        yaw: input.yaw,
        pitch: input.pitch,
        buttons: input.held.union(input.tapped),
        stick: input.stick.quantised(),
        ..default()
    };
    input.tapped = Buttons::default();
    cmds.clear();
    cmds.push((local.0, cmd));
    bots.think(sim, game, &mut cmds, buddy.0);
    sim.step(&cmds);
    asked.clear();
    asked.extend(mode_cmds.0.drain(..).map(|cmd| (local.0, cmd)));
    game.step(sim, &asked);
    for &event in &sim.events {
        messages.write(SimMessage(event));
    }
    for &event in &game.events {
        mode_messages.write(ModeMessage(event));
    }
    view.previous = view.current;
    view.current = LocalState::capture(sim, local.0);
}

fn blend_view(mut view: ResMut<LocalView>, time: Res<Time<Fixed>>) {
    let t = time.overstep_fraction();
    view.blended = LocalState::lerp(&view.previous, &view.current, t);
}

/// Every vehicle at this frame's exact moment between ticks, as [`LocalView`] is for your own soldier.
#[derive(Resource, Debug, Default)]
pub struct VehicleFrames {
    tick: u32,
    previous: Vec<Frame>,
    current: Vec<Frame>,
    pub blended: Vec<Frame>,
    /// What is carrying your look: which vehicle, the turn it had got to, and the tilt your look has followed so far.
    carrying: Option<(u32, Quat, Quat)>,
    /// Come into a seat looking somewhere it does not allow, and on the way back to what it does.
    settling: bool,
}

/// Every helicopter at this frame's exact moment between ticks.
#[derive(Resource, Debug)]
pub struct HeliFrames {
    tick: u32,
    previous: Vec<Frame>,
    current: Vec<Frame>,
    pub blended: Vec<Frame>,
    /// What is left of the last correction to your own prediction, metres. A pilot's whole machine is
    /// drawn off by this along with the eye inside it, so a correction fades with the cockpit round you
    /// and the camera never jumps. Always zero offline.
    pub own_offset: Vec3,
    /// The same for which way it faces.
    pub own_turn: Quat,
}

impl Default for HeliFrames {
    fn default() -> Self {
        Self { tick: 0, previous: Vec::new(), current: Vec::new(), blended: Vec::new(), own_offset: Vec3::ZERO, own_turn: Quat::IDENTITY }
    }
}

/// Blends vehicles, and if you are in one seats your eye in it exactly and takes your look round with it.
/// A carried look is still a world look, so that is all the rest of the game ever sees of it.
fn blend_vehicles(
    sim: Res<SimResource>,
    time: Res<Time<Fixed>>,
    real: Res<Time>,
    mut frames: ResMut<VehicleFrames>,
    mut view: ResMut<LocalView>,
    mut helis: ResMut<HeliFrames>,
    mut input: ResMut<PendingInput>,
    local: Res<LocalSoldier>,
) {
    let sim = &sim.0;
    let frames = &mut *frames;
    let helis = &mut *helis;
    if helis.tick != sim.tick || helis.current.len() != sim.helicopters.len() {
        helis.tick = sim.tick;
        std::mem::swap(&mut helis.previous, &mut helis.current);
        helis.current.clear();
        helis.current.extend(sim.helicopters.iter().map(|heli| heli.frame()));
        if helis.previous.len() != helis.current.len() {
            helis.previous.clone_from(&helis.current);
        }
    }
    helis.blended.clear();
    helis.blended.extend(helis.previous.iter().zip(&helis.current).map(|(a, b)| a.lerp(b, time.overstep_fraction())));
    input.at_controls = false;
    if let Some(h) = view.blended.seat.filter(|at| at.seat as usize == prs_sim::PILOT_SEAT).and_then(|at| prs_sim::heli_of(at.carrier))
        && let Some(frame) = helis.blended.get_mut(h)
    {
        frame.pos += helis.own_offset;
        frame.rot = (helis.own_turn * frame.rot).normalize();
    }
    if frames.tick != sim.tick || frames.current.len() != sim.vehicles.len() {
        frames.tick = sim.tick;
        std::mem::swap(&mut frames.previous, &mut frames.current);
        frames.current.clear();
        frames.current.extend(sim.vehicles.iter().map(|vehicle| vehicle.frame()));
        if frames.previous.len() != frames.current.len() {
            frames.previous.clone_from(&frames.current);
        }
    }
    let t = time.overstep_fraction();
    frames.blended.clear();
    frames.blended.extend(frames.previous.iter().zip(&frames.current).map(|(a, b)| a.lerp(b, t)));
    // A driver's own machine is put right by the same fading offset, so a correction never moves the
    // cockpit relative to the world in one go and the camera inside it never jumps.
    if let Some(at) = view.blended.seat.filter(|_| sim.is_at_controls(local.0))
        && prs_sim::heli_of(at.carrier).is_none()
        && let Some(frame) = frames.blended.get_mut(at.carrier as usize)
    {
        frame.pos += helis.own_offset;
        frame.rot = (helis.own_turn * frame.rot).normalize();
    }

    let Some(at) = view.blended.seat else {
        frames.carrying = None;
        return;
    };
    let carried = match prs_sim::heli_of(at.carrier) {
        Some(h) => helis.blended.get(h).copied(),
        None => frames.blended.get(at.carrier as usize).copied(),
    };
    let Some(frame) = carried else {
        frames.carrying = None;
        return;
    };
    let seat = &sim.carrier_seats(at.carrier)[at.seat as usize];
    let eye = sim.seated_eye_at(local.0, &frame).unwrap_or_else(|| frame.point(seat.eye));
    view.blended.feet = eye - Vec3::Y * view.blended.eye_height;
    let yaw = frame.yaw();
    // The turn exactly, and the tilt by way of something that follows it, so a slope comes through and a bump not.
    let ride = sim.carrier_ride_cfg(at.carrier);
    let (tilt, turn) = prs_sim::seats::tilt_and_turn(frame.rot);
    let followed = match frames.carrying {
        Some((carrier, last_turn, last_tilt)) if carrier == at.carrier => {
            let tilt = last_tilt.slerp(tilt, 1.0 - (-ride.tilt_rate * real.delta_secs()).exp());
            (input.yaw, input.pitch) = prs_sim::seats::carry_look(input.yaw, input.pitch, tilt * (turn * last_turn.inverse()) * last_tilt.inverse());
            tilt
        }
        _ => tilt,
    };
    frames.carrying = Some((at.carrier, turn, followed));
    // With the controls in your hands you look where the machine points and turn your head to see more.
    if prs_sim::heli_of(at.carrier).is_some() && at.seat as usize == prs_sim::PILOT_SEAT {
        input.at_controls = true;
        input.yaw = yaw;
        input.pitch = 0.0;
        return;
    }
    // On the way through a door or across to a seat the look is your own, and once there it comes round, not in one go.
    if sim.soldiers[local.0].transit.is_some() {
        frames.settling = true;
        return;
    }
    let (limited_yaw, limited_pitch) = prs_sim::seats::limit_look(&frame, seat, input.yaw, input.pitch);
    let off = Vec2::new(limited_yaw - input.yaw, limited_pitch - input.pitch);
    let most = ride.look_settle * real.delta_secs();
    if frames.settling && off.length() > most {
        let step = off.normalize_or_zero() * most;
        input.yaw += step.x;
        input.pitch += step.y;
    } else {
        frames.settling = false;
        input.yaw = limited_yaw;
        input.pitch = limited_pitch;
    }
}

#[cfg(test)]
mod map_choice_tests {
    use super::{from_args, map_from_query, mode_for, mode_from_query, name_from_query, server_from_query};
    use prs_mode::Deployment;

    #[test]
    fn a_name_comes_out_of_the_query_as_it_was_typed() {
        assert_eq!(name_from_query("?server=ws://x&name=Big+Bear%21&map=town"), "Big Bear!");
        assert_eq!(name_from_query("?name=J%C3%BCrgen"), "Jürgen");
        assert_eq!(name_from_query("?name=50%"), "50%", "a stray percent stays");
        assert_eq!(name_from_query("?map=town"), "");
    }

    #[test]
    fn the_plain_address_on_the_town_is_advance_and_secure_with_bots() {
        let mode = mode_for("town", mode_from_query(""), false);
        assert_eq!(mode.cfg.name, "Advance and Secure");
        assert!(mode.cfg.bots_per_team.is_some_and(|bots| bots > 0));
    }

    #[test]
    fn the_page_address_names_the_mode() {
        let practice = mode_for("town", mode_from_query("?mode=practice"), false);
        assert_eq!(practice.cfg.deployment, Deployment::Automatic);
        assert_eq!(mode_for("town", mode_from_query("?x=1&mode=tdm"), false).cfg.name, "Team Deathmatch");
        assert_eq!(mode_for("range", None, false).cfg.name, "Free Practice");
        assert_eq!(mode_for("range", Some("aas"), false).cfg.name, "Advance and Secure");
        assert_eq!(mode_for("town", Some("nonsense"), false).cfg.name, "Advance and Secure");
    }

    #[test]
    fn a_shortcut_can_say_where_to_play_either_way_it_writes_it() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter();
        assert_eq!(from_args(args(&["--server", "wss://a.example"]), "--server"), Some("wss://a.example".into()));
        assert_eq!(from_args(args(&["--map=range", "--server=ws://b:1"]), "--server"), Some("ws://b:1".into()));
        assert_eq!(from_args(args(&["--map=range"]), "--server"), None);
        assert_eq!(from_args(args(&["--server"]), "--server"), None);
        assert_eq!(from_args(args(&["--servers=x"]), "--server"), None);
    }

    #[test]
    fn the_page_address_names_the_server() {
        assert_eq!(server_from_query("?server=ws://a.example:7777"), Some("ws://a.example:7777".into()));
        assert_eq!(server_from_query("?x=1&server=ws%3A%2F%2Fa%3A1"), Some("ws://a:1".into()));
        assert_eq!(server_from_query("?map=town"), None);
    }

    #[test]
    fn the_page_address_names_the_map() {
        assert_eq!(map_from_query("?map=town"), Some("town"));
        assert_eq!(map_from_query("?x=1&map=town&y=2"), Some("town"));
        assert_eq!(map_from_query("?minimap=town"), None);
        assert_eq!(map_from_query(""), None);
    }
}

#[cfg(test)]
mod carried_look_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use prs_sim::seats::SeatedAt;
    use prs_sim::{Level, view_dir};

    fn world_with(sim: Sim, local: usize, yaw: f32, pitch: f32) -> World {
        let view = LocalState::capture(&sim, local);
        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(VehicleFrames::default());
        world.insert_resource(HeliFrames::default());
        world.insert_resource(LocalSoldier(local));
        world.insert_resource(PendingInput { yaw, pitch, ..default() });
        world.insert_resource(LocalView { previous: view, current: view, blended: view });
        world.insert_resource(SimResource(sim));
        world
    }

    fn frame_passes(world: &mut World) -> (f32, f32) {
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        world.run_system_once(blend_vehicles).unwrap();
        let input = world.resource::<PendingInput>();
        (input.yaw, input.pitch)
    }

    /// A real passenger's view, with the mouse left alone, while someone else throws the truck about on real ground:
    /// how far the look strays from where it started in the seat, in degrees, and how far the truck turned.
    fn in_a_hard_drive(seat: u8) -> (f32, f32) {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let driver = sim.spawn_soldier(1);
        let local = sim.spawn_soldier(2);
        sim.sit_in(driver, SeatedAt { carrier: 0, seat: 0 });
        sim.sit_in(local, SeatedAt { carrier: 0, seat });
        let yaw = sim.vehicles[0].frame().yaw() + sim.carrier_seats(0)[seat as usize].facing;
        let mut world = world_with(sim, local, yaw, 0.0);
        let (mut start, mut strayed, mut turned, mut last) = (None, 0.0_f32, 0.0_f32, None);
        for tick in 0..(12 * 60) {
            let (yaw, pitch) = frame_passes(&mut world);
            let wish = Vec2::new(if (tick / 200) % 2 == 0 { 1.0 } else { -1.0 }, 1.0);
            let sim = &mut world.resource_mut::<SimResource>().into_inner().0;
            let frame = sim.vehicles[0].frame();
            let inside = frame.rot.inverse() * view_dir(yaw, pitch);
            strayed = strayed.max(inside.angle_between(*start.get_or_insert(inside)).to_degrees());
            if let Some(last) = last {
                let delta: f32 = frame.yaw() - last;
                turned += ((delta + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs();
            }
            last = Some(frame.yaw());
            let cmds = [
                (driver, PlayerCmd { tick: sim.tick, wish_move: wish, yaw: frame.yaw(), ..default() }),
                (local, PlayerCmd { tick: sim.tick, yaw, pitch, ..default() }),
            ];
            sim.step(&cmds);
        }
        (strayed, turned.to_degrees())
    }

    #[test]
    fn with_the_mouse_alone_you_go_on_looking_at_the_same_part_of_the_inside() {
        for seat in 1..4 {
            let (strayed, turned) = in_a_hard_drive(seat);
            println!("seat {seat}: strayed {strayed:.1} degrees while it turned {turned:.0}");
            assert!(turned > 90.0, "{turned}");
            assert!(strayed < 8.0, "seat {seat}: {strayed}");
        }
    }

    #[test]
    fn coming_into_a_seat_looking_out_of_its_arc_the_view_comes_round_and_does_not_jump() {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let local = sim.spawn_soldier(2);
        sim.sit_in(local, SeatedAt { carrier: 0, seat: 1 });
        let cfg = &sim.carrier_seats(0)[1];
        let settle = sim.carrier_ride_cfg(0).look_settle;
        // As far from what the seat allows as can be.
        let wrong = sim.vehicles[0].frame().yaw() + cfg.facing + std::f32::consts::PI;
        sim.soldiers[local].transit = Some(prs_sim::seats::Transit::new(prs_sim::TransitKind::In { entry: 0 }));
        let mut world = world_with(sim, local, wrong, 0.0);
        let held = frame_passes(&mut world);
        assert_eq!(held, (wrong, 0.0), "on the way in the look is your own");
        world.resource_mut::<SimResource>().into_inner().0.soldiers[local].transit = None;
        let mut last = held;
        for _ in 0..240 {
            let now = frame_passes(&mut world);
            let moved = Vec2::new(now.0 - last.0, now.1 - last.1).length();
            assert!(moved <= settle / 60.0 + 1e-3, "jumped {moved}");
            last = now;
        }
        let sim = &world.resource::<SimResource>().0;
        let frame = sim.vehicles[0].frame();
        let limited = prs_sim::seats::limit_look(&frame, &sim.carrier_seats(0)[1], last.0, last.1);
        assert_eq!(limited, last, "and it gets there");
    }
}
