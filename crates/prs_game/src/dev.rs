//! Scripted runs for checking the game without a person at the controls.
//!
//! `PRS_SHOT=path.png` plays a short script, saves a screenshot at the end, and exits. Choose the
//! moment with `PRS_SCENE=hip|ads|zoom|freelook|recoil|run|prone|reload|range|third|buddy`.
//!
//! For what soldiers carry and what happens to them, `PRS_SCENE=` takes
//! * `gun:<rifle|lmg|dmr|pistol>:<hip|ads|third|side>`, for each gun in the hands and from outside.
//!   Add `:fire` to catch it a tick after a shot, with time slowed right down so the flash is still up.
//!   `PRS_AFTER=<seconds>` catches it that much later, and `PRS_WALL=<metres>` stands you that far from
//!   the nearest building, or that far inside it if negative, to see the flash on what is around you
//! * `switch` halfway through changing to the sidearm, `bandage` in the middle of bandaging yourself
//! * `panel` with the carry panel open, `kit` looking at a dropped kit with the prompt up
//! * `suppressed:low` and `suppressed:high` at the sights, with bullets going by at that level
//! * `bleed:<light|heavy|critical>[:sup][:trough]` at the sights at that bleed, held at the top of a beat or
//!   between beats, with `:sup` being shot at as well. `bleed:heavy:bandage` puts a bandage on with B
//! * `wounded` with an arm wound and blood loss at the sights
//! * `down` your own view when down, `downside` your own body from outside when down
//! * `buddydown` the other soldier collapsed, `buddyfall` the same in the middle of falling
//! * `buddydead` the other soldier dead, gun gone, with the kit beside them
//!
//! For leaning, `PRS_SCENE=lean:<left|right>:<hip|ads>:<first|third|front|back>:<open|corner|wall>[:crouch]`,
//! on the range. `corner` is at the end of the compound's front wall, leaning out past it, and `wall` is
//! hard up against a wall on the side leaned to. `walkers` looks along a street with `PRS_WALKERS` on it.
//!
//! `pour:<fall|ground>:<close|near|far>[:late]` a smoke grenade pouring, falling with its trail or lying on the
//! ground, with `late` for when its smoke has nearly run out
//! `throw:<frag|smoke>:<air|hand|cloud>` on the range: winding up, with one in the air, and with the smoke full
//! grown between you and the targets. `frag` catches a frag going off in front of you.
//!
//! For bodies, `PRS_SCENE=body:<stance>:<action>:<camera>` takes
//! * stance: `stand`, `crouch`, `prone`
//! * action: `idle`, `ads`, `walk`, `run`, `sprint`, `strafe`, `back`, `jump`, `fall`, `reload`, `turn`,
//!   `bandage`, `bandagearm`, `bandageleg`, `bandagestop` (letting go partway through), `bandagewalk`, `bandagewalkleg`, `medkit`
//! * camera: first person `ahead`, `down`, `feet`, `up`, `downleft`, `downright`, or from outside `out`,
//!   `outdown`, `side`, `front`, `back`, and up close `sidenear`, `frontnear`, `leftnear`, `bentnear`, `uppednear`,
//!   `feetnear`
//!
//! `PRS_SCENE=mode:<what>` looks at what a game mode shows: see `dev_mode.rs`.
//!
//! For vehicles, `PRS_SCENE=truck:<how>`, or `truck:<how>:car` or `truck:<how>:jeep` for the others, takes
//! * `outside`, `crew` with people in it, `climb` with someone halfway in, `smoke`, `fire` and `wreck`, stood
//!   looking at the truck from outside, and `door` with a hand to the driver's door and the prompt up
//! * `driver` (`driver:lock` with the wheel over), `passenger`, `rear`, `side` and `sights` from those seats, the last two out of the side
//! * `drive`, `steer`, `wall` and `chase` underway, from the seat and from behind
//! * `lift:<start|mid|high>:<first|third>` someone righting it off its side, so far along
//! * `push[:car|:jeep][:two]:<first|side|behind>` leaning into the back of it, with the other soldier too for `two`
//!
//! For helicopters, on either map, `PRS_SCENE=heli:<how>` takes
//! * `outside` on its pad, `spin` with the rotor up to speed, `hover` above you, `dust` close over the ground, `wreck`
//! * `cockpit` from the pilot's seat on the ground, `hovering` in the air, `cruise` at speed, `left` looking out the door
//! * `gunner` from the door seat, firing out, `chase` going by a little way off, `follow` from behind at speed
//!
//! For climbing, `PRS_SCENE=climb:<what>:<camera>`: see `dev_climb.rs`.
//! For a gun set down on a window sill, `PRS_SCENE=rest:<camera>[:<swing>]`: see `dev_rest.rs`.
//!
//! `armbands:<metres>:<front|side|back>` stands one soldier of each side in front of you.
//!
//! `sky:<x>:<y>:<z>:<yaw>:<pitch>` looks from that place in the world, in degrees, with the fog pushed out to
//! `PRS_FOG=<metres>` (6000 unless told), for looking over a whole map from above.
//!
//! `PRS_END=<seconds>` moves the moment of the shot, to catch another point of a walk cycle, and
//! `PRS_AT=x,y,z[,yaw]` starts your soldier somewhere else.

use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use prs_input::Captured;
#[cfg(not(target_arch = "wasm32"))]
use prs_runtime::{LocalSoldier, PendingInput, SimResource, ThirdPerson};
#[cfg(not(target_arch = "wasm32"))]
use prs_view::ViewSettings;
#[cfg(not(target_arch = "wasm32"))]
use prs_runtime::prs_sim::protocol::Buttons;
#[cfg(not(target_arch = "wasm32"))]
use prs_runtime::prs_sim::health::Zone;
#[cfg(not(target_arch = "wasm32"))]
use prs_runtime::prs_sim::weapons::{Gun, WeaponId, WeaponTable};
#[cfg(not(target_arch = "wasm32"))]
use prs_runtime::prs_sim::{Sim, SoldierId};

pub struct DevPlugin;

#[cfg(not(target_arch = "wasm32"))]
impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var("PRS_SHOT") else {
            return;
        };
        let scene = std::env::var("PRS_SCENE").unwrap_or_else(|_| "hip".into());
        if let Some(what) = scene.strip_prefix("mode:") {
            app.insert_resource(crate::dev_mode::ModeScene(what.to_owned())).add_systems(
                RunFixedMainLoop,
                crate::dev_mode::drive_mode.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop).after(drive),
            );
        }
        if scene.starts_with("sky:") {
            app.add_systems(PreStartup, clear_the_air).add_systems(Update, look_from_the_sky.after(prs_runtime::GunDrawn));
        }
        app.insert_resource(Script { path, scene, saved: false }).add_systems(
            RunFixedMainLoop,
            drive.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop).after(prs_input::ReadInput),
        );
    }
}

#[cfg(target_arch = "wasm32")]
impl Plugin for DevPlugin {
    fn build(&self, _app: &mut App) {}
}

/// Lets a `sky:` scene see the whole map: the air clears and the view reaches out to `PRS_FOG`.
#[cfg(not(target_arch = "wasm32"))]
fn clear_the_air(mut atmosphere: ResMut<prs_atmosphere::Atmosphere>, mut reach: ResMut<prs_atmosphere::ViewReach>) {
    let fog: f32 = std::env::var("PRS_FOG").ok().and_then(|v| v.parse().ok()).unwrap_or(6000.0);
    atmosphere.fog_visibility = fog;
    reach.0 = fog / atmosphere.fog_within_view;
}

/// Puts the camera wherever a `sky:` scene says, after the view has put it at the soldier's eye.
#[cfg(not(target_arch = "wasm32"))]
fn look_from_the_sky(script: Res<Script>, mut cameras: Query<&mut Transform, With<prs_view::WorldCamera>>) {
    let numbers: Vec<f32> = script.scene.trim_start_matches("sky:").split(':').filter_map(|v| v.parse().ok()).collect();
    let [x, y, z, yaw, pitch, ..] = numbers[..] else { return };
    for mut transform in &mut cameras {
        *transform = Transform::from_xyz(x, y, z).with_rotation(Quat::from_euler(EulerRot::YXZ, yaw.to_radians(), pitch.to_radians(), 0.0));
    }
}

/// Scripts one of the `body:` scenes, for looking at a stance from a given angle.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn drive_body(
    t: f32,
    stance: &str,
    action: &str,
    camera: &str,
    input: &mut PendingInput,
    held: &mut Buttons,
    third: &mut ThirdPerson,
    end: &mut f32,
) {
    // Rendering takes a moment to come up, and stance changes are worth catching on the way.
    held.set(Buttons::CROUCH, stance == "crouch" && t > 1.0 && t < 1.35);
    held.set(Buttons::PRONE, stance == "prone" && t > 1.0 && t < 1.35);
    // Settle into the stance first, then start whatever comes next.
    let start = 2.0;
    let going = t > start;
    let wish = match action {
        "walk" | "run" => Vec2::new(0.0, 1.0),
        "sprint" => Vec2::new(0.0, 1.0),
        "strafe" => Vec2::new(1.0, 0.0),
        "back" => Vec2::new(0.0, -1.0),
        "bandagewalk" | "bandagewalkleg" => Vec2::new(0.0, 1.0),
        _ => Vec2::ZERO,
    };
    input.wish_move = if going { wish } else { Vec2::ZERO };
    // Off to the right of the firing line, where there is nothing in the way.
    input.yaw = -1.2;
    held.set(Buttons::ADS, action == "ads" || action == "walk");
    held.set(Buttons::SPRINT, action == "sprint" && going);
    held.set(Buttons::RELOAD, action == "reload" && going);
    held.set(Buttons::JUMP, matches!(action, "jump" | "fall") && going);
    held.set(Buttons::BANDAGE, action.starts_with("bandage") && going && (action != "bandagestop" || t < start + 2.0));
    held.set(Buttons::SLOT_4, action == "medkit" && t > 1.0 && t < 1.35);
    held.set(Buttons::FIRE, action == "medkit" && going);
    if action == "turn" && going {
        input.yaw += ((t - start) * 1.2).min(1.3);
    }
    if action == "fall" {
        *end = start + 0.55;
    } else if action == "jump" {
        *end = start + 0.2;
    } else if action == "reload" {
        *end = start + 1.0;
    }

    third.0 = !matches!(camera, "ahead" | "down" | "feet" | "up" | "downleft" | "downright");
    match camera {
        "ahead" => {}
        "down" => input.pitch = -0.8,
        "feet" => input.pitch = -1.45,
        "up" => input.pitch = 1.1,
        "downleft" => {
            input.pitch = -0.7;
            input.free_look = Vec2::new(1.4, -0.75);
        }
        "downright" => {
            input.pitch = -0.7;
            input.free_look = Vec2::new(-1.4, -0.75);
        }
        "outdown" => input.pitch = -1.0,
        "bentnear" => {
            input.pitch = -1.45;
            input.free_look = Vec2::new(-1.45, 1.35);
        }
        "uppednear" => {
            input.pitch = 1.1;
            input.free_look = Vec2::new(-1.45, -1.2);
        }
        "feetnear" => input.free_look = Vec2::new(-1.2, -0.45),
        "sidenear" => input.free_look = Vec2::new(-1.45, -0.1),
        "frontnear" => input.free_look = Vec2::new(std::f32::consts::PI - 0.5, -0.1),
        "leftnear" => input.free_look = Vec2::new(1.3, -0.15),
        "side" => input.free_look = Vec2::new(-1.45, -0.28),
        "front" => input.free_look = Vec2::new(std::f32::consts::PI - 0.4, -0.28),
        "back" => input.free_look = Vec2::new(0.25, -0.28),
        _ => {}
    }
}

/// Scripts one of the `lean:` scenes. Returns where to put your soldier and the way to face, the first time.
#[cfg(not(target_arch = "wasm32"))]
fn drive_lean(t: f32, parts: &[&str], held: &mut Buttons, third: &mut ThirdPerson, input: &mut PendingInput, settings: &mut ViewSettings) -> Vec2 {
    let right = parts.first() != Some(&"left");
    let side = if right { 1.0 } else { -1.0 };
    held.set(Buttons::ADS, parts.get(1) == Some(&"ads") && t > 0.5);
    held.set(Buttons::CROUCH, parts.get(4) == Some(&"crouch") && t > 0.3 && t < 0.9);
    held.set(if right { Buttons::LEAN_RIGHT } else { Buttons::LEAN_LEFT }, t > 1.0 && t < 1.4);
    input.pitch = 0.012;
    match parts.get(2).copied().unwrap_or("first") {
        "third" => third.0 = true,
        "front" => {
            third.0 = true;
            input.free_look = Vec2::new(std::f32::consts::PI, -0.1);
            settings.third_person_offset = Vec3::new(0.0, -0.3, 2.6);
        }
        "back" => {
            third.0 = true;
            settings.third_person_offset = Vec3::new(0.0, -0.1, 2.6);
        }
        _ => {}
    }
    // The compound on the range: the front wall ends at x = -27.8 and -40.2, level with z = -18.
    let (at, yaw) = match parts.get(3).copied().unwrap_or("open") {
        "corner" => (Vec2::new(-34.0 + side * 5.7, -16.9), -side * 0.16),
        "wall" => (Vec2::new(-34.0 - side * 6.5, -24.0), 0.0),
        _ => (Vec2::new(8.0, 14.0), -0.1),
    };
    input.yaw = yaw;
    at
}

/// Scripts one of the `truck:` scenes, on the map's first truck or, with `:car` on the end, its first
/// armoured car.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn drive_truck(
    t: f32,
    spec: &str,
    sim: &mut Sim,
    id: SoldierId,
    input: &mut PendingInput,
    held: &mut Buttons,
    third: &mut ThirdPerson,
    settings: &mut ViewSettings,
    end: &mut f32,
    once: &mut bool,
    buddy: &mut prs_runtime::BuddyInput,
) {
    use prs_runtime::prs_sim::seats::{SeatedAt, Transit, TransitKind};
    use prs_runtime::prs_sim::vehicles::{VehicleEvent, VehicleTable, VehicleZone};

    let parts: Vec<&str> = spec.split(':').collect();
    let how = parts.first().copied().unwrap_or("outside");
    let wanted = match parts.get(1).copied() {
        Some("car") => VehicleTable::ARMOURED_CAR,
        Some("jeep") => VehicleTable::LIGHT_JEEP,
        _ => VehicleTable::UTILITY_TRUCK,
    };
    let carrier = sim.vehicles.iter().position(|vehicle| vehicle.kind == wanted).unwrap_or(0) as u32;
    let frame = sim.carrier_frame(carrier);
    let seat_of = |seat: u8| SeatedAt { carrier, seat };
    if !*once {
        *once = true;
        let damage = sim.vehicle_cfgs[sim.vehicles[carrier as usize].kind].damage;
        let v = carrier as usize;
        match how {
            "driver" | "drive" | "steer" | "chase" | "wall" => sim.sit_in(id, seat_of(0)),
            "passenger" | "side" | "sights" => sim.sit_in(id, seat_of(1)),
            "rear" => sim.sit_in(id, seat_of(3)),
            "crew" => {
                sim.sit_in(BUDDY, seat_of(0));
                let mate = sim.spawn_soldier(0xCEE);
                sim.sit_in(mate, seat_of(1));
                let other = sim.spawn_soldier(0xCEF);
                sim.sit_in(other, seat_of(3));
            }
            "smoke" => sim.bullet_into_vehicle(v, VehicleZone::Engine, damage.engine * 0.7),
            "fire" => {
                sim.bullet_into_vehicle(v, VehicleZone::Engine, damage.engine + 1.0);
                sim.bullet_into_vehicle(v, VehicleZone::Engine, 10.0);
                sim.bullet_into_vehicle(v, VehicleZone::Tyre(1), damage.tyre + 1.0);
            }
            "climb" => {
                sim.sit_in(BUDDY, seat_of(0));
                sim.soldiers[BUDDY].transit = Some(Transit::new(TransitKind::In { entry: 0 }));
                // Held halfway through the door, however long the scene takes to get here.
                sim.soldiers[BUDDY].transit.as_mut().expect("just set").t = 0.45;
            }
            "lift" => {
                // On its side and left to settle, which is the start of what there is to see.
                let vehicle = &mut sim.vehicles[v];
                vehicle.rot *= Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                vehicle.pos.y += 1.0;
                prs_runtime::prs_sim::vehicles::wake(vehicle);
            }
            "wreck" => {
                sim.vehicles[v].damage.wrecked = true;
                sim.events.push(prs_runtime::prs_sim::SimEvent::Vehicle { vehicle: v, event: VehicleEvent::OnFire });
            }
            _ => {}
        }
    }
    match how {
        "push" => {
            // Leaning into the back of it from 1 s and walking it along to the end; with `two`, the other soldier at
            // a corner as well. The camera is `first`, `side` or `behind`.
            let v = carrier as usize;
            let vehicle = &sim.vehicles[v];
            let spots = sim.vehicle_cfgs[vehicle.kind].push.spots.clone();
            let two = parts.contains(&"two");
            let pushers: Vec<(SoldierId, usize)> = if two { vec![(id, spots.len() - 3), (BUDDY, spots.len() - 1)] } else { vec![(id, spots.len() - 2)] };
            let mut told = None;
            for &(who, spot) in &pushers {
                let vehicle = &sim.vehicles[v];
                let at = vehicle.pos + vehicle.rot * spots[spot];
                if t < 0.9 {
                    let out = vehicle.rot * Vec3::Z;
                    let to = at + Vec3::new(out.x, 0.0, out.z).normalize_or(Vec3::Z) * 0.9;
                    let top = to + Vec3::Y * 5.0;
                    let ground = sim.world.raycast(top, Vec3::NEG_Y, 20.0).map_or(to.y, |hit| top.y - hit.distance);
                    sim.soldiers[who].body.pos = Vec3::new(to.x, ground + 0.02, to.z);
                }
                let to = (at - sim.soldiers[who].eye()).normalize_or(Vec3::NEG_Z);
                let (yaw, pitch) = ((-to.x).atan2(-to.z), to.y.asin() * 0.5);
                if who == id {
                    input.yaw = yaw;
                    input.pitch = pitch;
                } else {
                    let mut buttons = Buttons::default();
                    buttons.set(Buttons::USE, t > 1.0 && (t * 60.0) as u32 % 2 == 0 || sim.soldiers[who].pushing.is_some());
                    told = Some(prs_runtime::prs_sim::protocol::PlayerCmd { yaw, pitch, buttons, ..Default::default() });
                }
            }
            buddy.0 = told;
            held.set(Buttons::USE, t > 1.0);
            *end = 3.5;
            let camera = parts.last().copied().unwrap_or("behind");
            third.0 = camera != "first";
            settings.third_person_offset = Vec3::new(0.3, 0.2, 3.6);
            input.free_look = if camera == "side" { Vec2::new(-1.45, -0.15) } else { Vec2::new(0.25, -0.15) };
        }
        "lift" => {
            // Hands on from 1.5 s, and however far it has got by the end of the scene.
            let v = carrier as usize;
            let vehicle = &sim.vehicles[v];
            let roof = vehicle.rot * Vec3::Y;
            if t < 1.5 {
                let at = vehicle.pos + Vec3::new(roof.x, 0.0, roof.z).normalize_or(Vec3::X) * 1.75 + Vec3::Z * 0.4;
                let top = at + Vec3::Y * 5.0;
                let ground = sim.world.raycast(top, Vec3::NEG_Y, 20.0).map_or(at.y, |hit| top.y - hit.distance);
                sim.soldiers[id].body.pos = Vec3::new(at.x, ground + 0.02, at.z);
            }
            let eye = sim.soldiers[id].eye();
            let to = (sim.nearest_of_hull(v, eye) - eye).normalize_or(Vec3::NEG_Z);
            input.yaw = (-to.x).atan2(-to.z);
            input.pitch = to.y.asin();
            held.set(Buttons::USE, t > 1.5);
            *end = match parts.get(1).copied() {
                Some("start") => 1.4,
                Some("mid") => 4.5,
                _ => 6.3,
            };
            third.0 = parts.get(2) == Some(&"third");
            settings.third_person_offset = Vec3::new(0.6, 0.3, 4.5);
        }
        "door" => {
            let entry = sim.carrier_entries(carrier)[0].clone();
            let stand = frame.point(entry.stand);
            let top = stand + Vec3::Y * 4.0;
            let ground = sim.world.raycast(top, Vec3::NEG_Y, 9.0).map_or(stand.y, |hit| top.y - hit.distance);
            let feet = Vec3::new(stand.x, ground + 0.05, stand.z);
            sim.soldiers[id].body.pos = feet;
            let to = frame.point(entry.hand) - (feet + Vec3::Y * sim.move_cfg.stand_eye);
            input.yaw = (-to.x).atan2(-to.z);
            input.pitch = (to.y / to.length()).asin();
        }
        "driver" => {
            input.pitch = -0.08;
            input.wish_move = Vec2::new(if parts.get(1) == Some(&"lock") { 1.0 } else { 0.0 }, 0.0);
        }
        "passenger" | "rear" => input.pitch = -0.05,
        "side" | "sights" => {
            input.yaw = frame.yaw() - std::f32::consts::FRAC_PI_2;
            input.pitch = 0.02;
            held.set(Buttons::ADS, how == "sights");
            *end = 3.5;
        }
        "drive" | "steer" | "wall" => {
            input.wish_move = Vec2::new(if how == "steer" && t > 4.0 { 1.0 } else { 0.0 }, 1.0);
            input.pitch = -0.08;
            *end = if how == "wall" { 9.0 } else { 6.0 };
        }
        "chase" => {
            input.wish_move = Vec2::new(if t > 3.0 { 1.0 } else { 0.0 }, 1.0);
            third.0 = true;
            settings.third_person_offset = Vec3::new(0.0, 0.8, 8.0);
            input.pitch = -0.12;
            *end = 5.0;
        }
        _ => {
            // Stood off to one side of it, looking at it from outside.
            let at = frame.point(Vec3::new(-5.5, 0.0, -3.5));
            let top = Vec3::new(at.x, at.y + 5.0, at.z);
            let ground = sim.world.raycast(top, Vec3::NEG_Y, 20.0).map_or(at.y, |hit| top.y - hit.distance);
            sim.soldiers[id].body.pos = Vec3::new(at.x, ground + 0.05, at.z);
            let to = frame.pos - Vec3::new(at.x, ground, at.z);
            input.yaw = (-to.x).atan2(-to.z);
            input.pitch = -0.04;
            *end = if how == "fire" { 6.0 } else { 3.0 };
            if how == "climb" {
                // It would finish by itself, and this is the moment to see.
                if let Some(transit) = sim.soldiers[BUDDY].transit.as_mut() {
                    transit.t = 0.45;
                }
            }
        }
    }
}

/// Puts this gun in a soldier's hands, up and ready.
#[cfg(not(target_arch = "wasm32"))]
fn give(sim: &mut Sim, id: SoldierId, weapon: WeaponId) {
    let gun = Gun::issued(weapon, &sim.weapons[weapon]);
    sim.soldiers[id].weapon.put_in_hands(gun);
    sim.soldiers[id].weapon.ready = 1.0;
}

/// The other soldier on the firing line, for pointing the camera at.
#[cfg(not(target_arch = "wasm32"))]
const BUDDY: SoldierId = 1;

/// `armbands:<metres>:<front|side|back>`: one soldier of each side standing side by side that far in
/// front of you, facing the way asked, for judging whether a side can be told at a glance.
#[cfg(not(target_arch = "wasm32"))]
fn line_up_both_sides(spec: &str, sim: &mut Sim, local: SoldierId, input: &mut PendingInput) {
    let parts: Vec<&str> = spec.split(':').collect();
    let distance: f32 = parts.first().and_then(|d| d.parse().ok()).unwrap_or(5.0);
    let turn = match parts.get(1).copied() {
        Some("side") => std::f32::consts::FRAC_PI_2,
        Some("back") => 0.0,
        _ => std::f32::consts::PI,
    };
    // Spawned once, after everyone the level brought, and left unstepped so they hold the pose.
    if !sim.soldiers.iter().any(|s| s.seed == 0xA4B0) {
        for team in [0, 1] {
            let id = sim.spawn_soldier(0xA4B0 + team as u32);
            sim.soldiers[id].team = team;
            sim.soldiers[id].seed = 0xA4B0;
        }
    }
    let lineup: Vec<SoldierId> = (0..sim.soldiers.len()).filter(|&i| sim.soldiers[i].seed == 0xA4B0).collect();
    // The range buddy is somewhere else for this, so as not to stand in the way.
    sim.soldiers[BUDDY].body.pos = sim.soldiers[local].body.pos + Vec3::new(0.0, 0.0, 60.0);
    input.yaw = -1.2;
    input.pitch = 0.0;
    let (forward, right) = (Vec3::new(-input.yaw.sin(), 0.0, -input.yaw.cos()), Vec3::new(input.yaw.cos(), 0.0, -input.yaw.sin()));
    let centre = sim.soldiers[local].body.pos + forward * distance;
    for (n, id) in lineup.into_iter().enumerate() {
        let spot = centre + right * if n == 0 { -0.6 } else { 0.6 };
        let ground = sim.world.raycast(spot + Vec3::Y * 20.0, Vec3::NEG_Y, 40.0).map_or(spot.y, |hit| spot.y + 20.0 - hit.distance);
        let soldier = &mut sim.soldiers[id];
        soldier.body.pos = Vec3::new(spot.x, ground, spot.z);
        soldier.yaw = input.yaw + turn;
        soldier.pitch = 0.0;
        soldier.anim.hip_yaw = soldier.yaw;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn look_at_buddy(input: &mut PendingInput, sim: &Sim, local: SoldierId) {
    let from = sim.soldiers[local].body.eye();
    let d = sim.soldiers[BUDDY].body.pos + Vec3::Y * 0.2 - from;
    input.yaw = (-d.x).atan2(-d.z);
    input.pitch = (d.y / d.length()).asin();
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct Script {
    path: String,
    scene: String,
    saved: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut script: ResMut<Script>,
    mut input: ResMut<PendingInput>,
    mut captured: ResMut<Captured>,
    mut third: ResMut<ThirdPerson>,
    mut settings: ResMut<ViewSettings>,
    mut sim: ResMut<SimResource>,
    local: Res<LocalSoldier>,
    (mut panel, mut bleed_shown): (ResMut<prs_hud::CarryPanelOpen>, ResMut<prs_runtime::BleedShown>),
    (mut hurt_once, mut heli_pilot, mut climb_stool): (Local<bool>, Local<prs_runtime::prs_sim::helicopter::autopilot::Autopilot>, Local<Option<SoldierId>>),
    mut clock: ResMut<Time<Virtual>>,
    mut shot_at: Local<Option<f32>>,
    mut at_wall: Local<bool>,
    mut placed: Local<bool>,
    (mut exit, mut buddy): (MessageWriter<AppExit>, ResMut<prs_runtime::BuddyInput>),
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};

    captured.0 = script.scene != "pause";
    if !*placed {
        *placed = true;
        // `PRS_AT=x,y,z[,yaw]` starts your soldier somewhere else, to see a body on a slope or up close.
        let at: Vec<f32> = std::env::var("PRS_AT").unwrap_or_default().split(',').filter_map(|v| v.parse().ok()).collect();
        if let [x, y, z, ..] = at[..] {
            sim.0.soldiers[local.0].body.pos = Vec3::new(x, y, z);
        }
        if let [_, _, _, yaw] = at[..] {
            input.yaw = yaw;
        }
    }
    let t = time.elapsed_secs();
    let mut held = Buttons::default();
    let mut end = 3.0;
    let mut slowed = false;
    let scene = script.scene.clone();
    if let Some(spec) = scene.strip_prefix("armbands:") {
        line_up_both_sides(spec, &mut sim.0, local.0, &mut input);
    }
    if let Some(spec) = scene.strip_prefix("body:") {
        let parts: Vec<&str> = spec.split(':').collect();
        drive_body(t, parts.first().copied().unwrap_or("stand"), parts.get(1).copied().unwrap_or("idle"), parts.get(2).copied().unwrap_or("out"), &mut input, &mut held, &mut third, &mut end);
        // A wound where it says, so that there is something to bandage. `PRS_END` picks the moment.
        let zone = match parts.get(1).copied() {
            Some("bandagearm" | "bandagestop" | "bandagewalk" | "medkit") => Some(Zone::Arm),
            Some("bandageleg" | "bandagewalkleg") => Some(Zone::Leg),
            Some("bandage") => Some(Zone::Chest),
            _ => None,
        };
        if let Some(zone) = zone
            && !*hurt_once
        {
            *hurt_once = true;
            sim.0.hurt(local.0, zone, 20.0);
        }
        // Square on to the whole body rather than over the shoulder.
        let camera = parts.get(2).copied().unwrap_or_default();
        settings.third_person_offset = match camera {
            "feetnear" => Vec3::new(0.0, -1.1, 1.5),
            c if c.ends_with("near") => Vec3::new(0.0, 0.0, 1.5),
            _ => Vec3::new(0.0, -0.1, 3.4),
        };
    }
    if let Some(spec) = scene.strip_prefix("lean:") {
        let parts: Vec<&str> = spec.split(':').collect();
        let at = drive_lean(t, &parts, &mut held, &mut third, &mut input, &mut settings);
        if t < 0.2 {
            let top = Vec3::new(at.x, 60.0, at.y);
            let ground = sim.0.world.raycast(top, Vec3::NEG_Y, 120.0).map_or(0.0, |hit| 60.0 - hit.distance);
            sim.0.soldiers[local.0].body.pos = Vec3::new(at.x, ground + 0.05, at.y);
        }
    }
    if let Some(spec) = scene.strip_prefix("pour:") {
        let parts: Vec<&str> = spec.split(':').collect();
        let falling = parts.first() == Some(&"fall");
        let away = match parts.get(1).copied() {
            Some("close") => 1.6,
            Some("near") => 6.0,
            _ => 16.0,
        };
        input.yaw = 0.0;
        input.pitch = if falling { 0.3 } else { -(1.6_f32 / away).atan() };
        end = if falling { 3.4 } else { 4.7 };
        // Once rendering has come up, so that its very start can be seen.
        if t > 2.0 && !*hurt_once {
            *hurt_once = true;
            let me = sim.0.soldiers[local.0].body.pos;
            let at = me + Vec3::NEG_Z * away;
            let top = at + Vec3::Y * 60.0;
            let ground = sim.0.world.raycast(top, Vec3::NEG_Y, 120.0).map_or(at.y, |hit| top.y - hit.distance);
            let pos = at.with_y(ground + if falling { 13.0 } else { 0.05 });
            sim.0.thrown.push(prs_runtime::prs_sim::throwables::Thrown { id: 900, kind: prs_runtime::prs_sim::throwables::ThrownKind::Smoke, thrower: local.0, pos, vel: Vec3::ZERO, fuse: 0.02, at_rest: false });
        }
        if parts.get(2) == Some(&"late") {
            let cfg = sim.0.throw_cfg.smoke.clone();
            if let Some(cloud) = sim.0.clouds.first_mut() {
                cloud.age = cloud.age.max(cfg.grow_time + cfg.hold_time + cfg.fade_time * 0.6);
            }
        }
    }
    if let Some(spec) = scene.strip_prefix("throw:") {
        let parts: Vec<&str> = spec.split(':').collect();
        let smoke = parts.first() == Some(&"smoke");
        let how = parts.get(1).copied().unwrap_or("air");
        input.yaw = 0.0;
        input.pitch = 0.2;
        third.0 = how == "third";
        held.set(Buttons::GRENADE, t > 0.5 && t < 0.7 || (smoke && t > 0.9 && t < 1.1));
        held.set(Buttons::FIRE, t > 2.4 && t < 3.2);
        end = match how {
            "hand" | "third" => 3.1,
            "air" => 3.7,
            _ => 16.0,
        };
        if how == "cloud" {
            input.pitch = 0.0;
        }
    }
    if let Some(spec) = scene.strip_prefix("gun:") {
        let parts: Vec<&str> = spec.split(':').collect();
        let weapon = match parts.first().copied().unwrap_or("rifle") {
            "lmg" => WeaponTable::MACHINE_GUN,
            "dmr" => WeaponTable::MARKSMAN_RIFLE,
            "pistol" => WeaponTable::PISTOL,
            _ => WeaponTable::ASSAULT_RIFLE,
        };
        if sim.0.soldiers[local.0].weapon.weapon != weapon {
            give(&mut sim.0, local.0, weapon);
        }
        let how = parts.get(1).copied().unwrap_or("hip");
        input.pitch = 0.012;
        input.yaw = -1.2;
        if let Some(gap) = std::env::var("PRS_WALL").ok().and_then(|v| v.parse::<f32>().ok()) {
            const TOWARDS_BUILDING: f32 = -0.7;
            input.yaw = TOWARDS_BUILDING;
            if t > 1.5 && !*at_wall {
                *at_wall = true;
                let soldier = &sim.0.soldiers[local.0];
                let (from, dir) = (soldier.body.eye(), prs_runtime::prs_sim::view_dir(TOWARDS_BUILDING, 0.0));
                if let Some(hit) = sim.0.world.raycast(from, dir, 200.0) {
                    let feet = soldier.body.pos + dir * (hit.distance - gap);
                    sim.0.soldiers[local.0].body.pos = feet;
                }
            }
        }
        held.set(Buttons::ADS, how == "ads" || how == "side");
        third.0 = matches!(how, "third" | "side");
        if how == "side" {
            input.free_look = Vec2::new(-1.45, -0.1);
            settings.third_person_offset = Vec3::new(0.0, 0.0, 1.5);
        }
        if parts.get(2) == Some(&"fire") {
            // Slowed well before the shot, because a frame can take longer than the moment to catch.
            const SHOT: f32 = 2.6;
            if t > SHOT - 0.2 {
                clock.set_relative_speed(if t > SHOT { 0.004 } else { 0.05 });
            }
            held.set(Buttons::FIRE, t > SHOT);
            // By the rounds gone, since a bullet into a wall right ahead is over within its first tick.
            let soldier = &sim.0.soldiers[local.0];
            if shot_at.is_none() && soldier.weapon.ammo < soldier.held_config(&sim.0.weapons).magazine {
                *shot_at = Some(t);
            }
            let after = std::env::var("PRS_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            end = shot_at.map_or(f32::MAX, |shot| shot + after);
            slowed = true;
        }
    }
    if let Some(spec) = scene.strip_prefix("heli:") {
        crate::dev_heli::drive_heli(spec, t, &mut sim.0, local.0, &mut input, &mut held, &mut third, &mut settings, &mut end, &mut hurt_once, &mut heli_pilot);
    }
    if let Some(spec) = scene.strip_prefix("rest:") {
        if crate::dev_rest::drive_rest(spec, t, &mut sim.0, local.0, &mut input, &mut held, &mut third, &mut settings, &mut hurt_once) && shot_at.is_none() {
            *shot_at = Some(t);
        }
        end = shot_at.unwrap_or(f32::MAX);
    }
    if let Some(spec) = scene.strip_prefix("climb:") {
        if crate::dev_climb::drive_climb(spec, t, &mut sim.0, local.0, &mut input, &mut held, &mut third, &mut settings, &mut clock, &mut hurt_once, &mut climb_stool) && shot_at.is_none() {
            *shot_at = Some(t);
        }
        end = shot_at.unwrap_or(f32::MAX);
    }
    if let Some(spec) = scene.strip_prefix("truck:") {
        drive_truck(t, spec, &mut sim.0, local.0, &mut input, &mut held, &mut third, &mut settings, &mut end, &mut hurt_once, &mut buddy);
        // Hurt in the seat, and holding B: `truck:driver:bandage`.
        if spec.ends_with(":bandage") {
            let health = &mut sim.0.soldiers[local.0].health;
            if t > 1.0 && t < 1.2 {
                health.bleed = 1.0;
                health.arm_wound = 0.5;
            }
            held.set(Buttons::BANDAGE, t > 1.2);
        }
    }
    let once = |done: &mut bool, sim: &mut Sim, f: &dyn Fn(&mut Sim)| {
        if !*done {
            *done = true;
            f(sim);
        }
    };
    if let Some(spec) = scene.strip_prefix("bleed:") {
        let parts: Vec<&str> = spec.split(':').collect();
        let (health, bleed) = match parts.first().copied().unwrap_or("light") {
            "heavy" => (60.0, 2.0),
            "critical" => (30.0, 4.0),
            _ => (90.0, 1.0),
        };
        input.pitch = 0.012;
        let bandaging = parts.contains(&"bandage");
        let soldier = &mut sim.0.soldiers[local.0];
        if !bandaging || !*hurt_once {
            *hurt_once = true;
            soldier.health.health = health;
            soldier.health.bleed = bleed;
        }
        if parts.contains(&"sup") {
            soldier.suppression.level = 1.0;
            soldier.suppression.hold = 1.0;
        }
        end = 3.5;
        if bandaging {
            held.set(Buttons::BANDAGE, t > 1.0);
        } else {
            held.set(Buttons::ADS, true);
            // No beat at all, so that shots of one point in it can be compared.
            let look = &mut bleed_shown.look;
            for band in [&mut look.light, &mut look.heavy, &mut look.critical] {
                band.beats_per_minute = 0.0;
            }
            bleed_shown.beats_per_minute = 0.0;
            bleed_shown.phase = if parts.contains(&"trough") { 0.5 } else { 0.0 };
        }
    }
    if let Some(how) = scene.strip_prefix("suppressed:") {
        held.set(Buttons::ADS, true);
        input.pitch = 0.012;
        let level = if how == "high" { 1.0 } else { 0.25 };
        let suppression = &mut sim.0.soldiers[local.0].suppression;
        suppression.level = level;
        suppression.hold = 1.0;
        end = 3.5;
    }
    match script.scene.as_str() {
        "switch" => {
            // Held for a while, because rendering in software a frame can take longer than a tap.
            held.set(Buttons::SLOT_2, t > 2.0);
            end = 2.0 + sim.0.weapons[WeaponTable::ASSAULT_RIFLE].stow_time * 0.9;
        }
        "bandage" => {
            once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(0, Zone::Leg, 25.0));
            held.set(Buttons::SLOT_3, t > 0.8 && t < 0.9);
            held.set(Buttons::FIRE, t > 1.4);
            end = 3.5;
        }
        "panel" => {
            panel.0 = true;
            end = 1.5;
        }
        "kit" | "buddydead" => {
            once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(BUDDY, Zone::Head, 200.0));
            look_at_buddy(&mut input, &sim.0, local.0);
            if script.scene == "kit" {
                let buddy = sim.0.soldiers[BUDDY].body.pos;
                sim.0.soldiers[local.0].body.pos = buddy + Vec3::new(-1.0, 0.05, 1.0);
            } else {
                sim.0.soldiers[local.0].body.pos = sim.0.soldiers[BUDDY].body.pos + Vec3::new(-2.2, 0.05, 2.8);
            }
            end = 3.0;
        }
        "wounded" => {
            once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(0, Zone::Arm, 40.0));
            sim.0.soldiers[local.0].health.health = 30.0;
            held.set(Buttons::ADS, true);
            input.pitch = 0.012;
            end = 3.5;
        }
        "down" => {
            once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(0, Zone::Chest, 120.0));
            end = 3.5;
        }
        "downside" => {
            once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(0, Zone::Chest, 120.0));
            third.0 = true;
            input.free_look = Vec2::new(-1.45, -0.4);
            settings.third_person_offset = Vec3::new(0.0, 0.0, 3.0);
            end = 3.5;
        }
        "treatally" | "treatallyside" => {
            // On your feet beside someone bleeding, with a bandage in your hand and your finger down.
            if t > 1.0 {
                once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(BUDDY, Zone::Arm, 15.0));
            }
            sim.0.soldiers[local.0].body.pos = sim.0.soldiers[BUDDY].body.pos + Vec3::new(-0.9, 0.05, 1.1);
            look_at_buddy(&mut input, &sim.0, local.0);
            held.set(Buttons::SLOT_3, t > 1.2 && t < 1.5);
            held.set(Buttons::FIRE, t > 1.8);
            if script.scene == "treatallyside" {
                third.0 = true;
                input.free_look = Vec2::new(-1.45, -0.2);
                settings.third_person_offset = Vec3::new(0.0, -0.2, 2.6);
            }
            end = 4.0;
        }
        "buddydown" | "buddyfall" => {
            if t > 1.5 {
                once(&mut hurt_once, &mut sim.0, &|sim| sim.hurt(BUDDY, Zone::Chest, 120.0));
            }
            sim.0.soldiers[local.0].body.pos = sim.0.soldiers[BUDDY].body.pos + Vec3::new(-2.2, 0.05, 2.8);
            look_at_buddy(&mut input, &sim.0, local.0);
            end = if script.scene == "buddyfall" { 1.9 } else { 4.5 };
        }
        "ads" => {
            held.set(Buttons::ADS, true);
            input.pitch = 0.012;
        }
        "zoom" => {
            held.set(Buttons::ADS, true);
            input.zoom = true;
            input.pitch = 0.012;
        }
        "freelook" => {
            held.set(Buttons::ADS, true);
            input.free_look = Vec2::new(0.9, -0.2);
        }
        "recoil" => {
            held.set(Buttons::ADS, true);
            held.set(Buttons::FIRE, t > 2.5);
            input.pitch = 0.012;
            end = 2.62;
        }
        "run" => {
            input.wish_move = Vec2::new(0.0, 1.0);
            held.set(Buttons::SPRINT, true);
            input.yaw = 0.9;
        }
        "prone" => {
            held.set(Buttons::PRONE, t > 0.5 && t < 0.6);
            held.set(Buttons::ADS, t > 2.0);
            end = 4.0;
        }
        "reload" => {
            held.set(Buttons::RELOAD, t > 1.0);
            end = 2.2;
        }
        "third" => {
            third.0 = true;
            held.set(Buttons::ADS, t > 1.0);
            input.yaw = -0.5;
        }
        "buddy" => {
            // Stand next to the other soldier and look along their rifle.
            input.yaw = (-5.0_f32).atan2(-0.5);
            input.pitch = -0.08;
            end = 3.0;
        }
        "range" => {
            input.pitch = 0.07;
            input.yaw = 0.35;
        }
        "walkers" => {
            input.pitch = 0.0;
            end = 4.0;
        }
        _ => {}
    }
    input.held = held;
    if let Some(at) = std::env::var("PRS_END").ok().and_then(|v| v.parse().ok()) {
        end = at;
    }

    if t > end && !script.saved {
        script.saved = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(script.path.clone()));
    }
    if t > end + if slowed { 0.03 } else { 1.0 } {
        exit.write(AppExit::Success);
    }
}
