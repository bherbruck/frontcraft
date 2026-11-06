//! Scripted climbing scenes, on the real walls of whatever map is up. See `dev.rs` for how they are chosen.
//!
//! `PRS_SCENE=climb:<what>:<camera>`, best with `PRS_MAP=town PRS_MODE=practice`, takes
//! * what: `vault` over a thin wall, `mantle` and `pullup` onto a top, `stool` standing on a crouched soldier,
//!   `mount` on the way up onto one, `roof` halfway up a roof from a crouched soldier's back
//! * camera: `first`, `down` in first person looking at your feet, or from outside `third`, `side`, `front`
//!
//! `PRS_THROUGH=<0 to 1>` says how far through a climb to catch it.

use bevy::prelude::*;
use prs_runtime::prs_sim::movement::{ClimbKind, MoveInput, MoveState, Perch, Stance, plan_climb, yaw_basis};
use prs_runtime::prs_sim::protocol::Buttons;
use prs_runtime::prs_sim::{Sim, SoldierId};
use prs_runtime::{PendingInput, ThirdPerson};
use prs_view::ViewSettings;

/// A place to climb from: where the feet are, on the ground, and which way to look.
struct Found {
    feet: Vec3,
    yaw: f32,
}

/// The first place on the map where this is the climb you would get, looked for by asking at every side
/// of everything built.
fn find(sim: &Sim, what: &str) -> Option<Found> {
    let c = &sim.move_cfg;
    let body = c.capsule(Stance::Stand);
    for solid in &sim.level.solids {
        let (x, z) = (solid.rotation * Vec3::X, solid.rotation * Vec3::Z);
        let top = solid.center.y + solid.half.y + 0.5;
        for (normal, half, _along, along_half) in [(x, solid.half.x, z, solid.half.z), (-x, solid.half.x, z, solid.half.z), (z, solid.half.z, x, solid.half.x), (-z, solid.half.z, x, solid.half.x)] {
            let flat = Vec3::new(normal.x, 0.0, normal.z).normalize_or_zero();
            if flat == Vec3::ZERO || along_half < 1.5 {
                continue;
            }
            let side = solid.center + normal * half;
            let spot = side + flat * (c.radius + 0.25);
            let high = Vec3::new(spot.x, top, spot.z);
            let Some(hit) = sim.world.raycast(high, Vec3::NEG_Y, top + 200.0) else { continue };
            let feet = high - Vec3::Y * (hit.distance - 0.02);
            if hit.distance <= 0.0 || !sim.world.capsule_is_clear(feet, body) {
                continue;
            }
            // Under the sky, so the camera has somewhere to be.
            if sim.world.raycast(feet + Vec3::Y * 2.0, Vec3::Y, 20.0).is_some() {
                continue;
            }
            let yaw = flat.x.atan2(flat.z);
            let input = MoveInput { yaw, ..MoveInput::default() };
            let mut state = MoveState::new(feet, c);
            state.grounded = true;
            let plan = |state: &MoveState| plan_climb(state, c, &input, &sim.world);
            let fits = match what {
                "vault" => plan(&state).is_some_and(|p| p.kind == ClimbKind::Vault && p.to.y < p.top - 0.5 && p.top > feet.y + 0.7),
                "mantle" => plan(&state).is_some_and(|p| p.kind == ClimbKind::Mantle && p.to.y > p.top - 0.1),
                "pullup" => plan(&state).is_some_and(|p| p.kind == ClimbKind::PullUp && p.to.y > p.top - 0.1),
                // A wall too high to climb, right in front.
                "refused" => plan(&state).is_none() && (2.4..3.2).contains(&(solid.center.y + solid.half.y - feet.y)) && solid.half.y > 0.8,
                "roof" => {
                    state.pos += Vec3::Y * c.stool.back_height;
                    state.perch = Perch { on: Some(1), spot: state.pos };
                    plan(&state).is_some_and(|p| p.kind == ClimbKind::PullUp && p.to.y > feet.y + 2.7)
                        && plan(&MoveState { pos: feet, perch: Perch::default(), ..state.clone() }).is_none()
                }
                _ => false,
            };
            if fits {
                return Some(Found { feet, yaw });
            }
        }
    }
    None
}

/// A ground floor window from inside, facing out through it, a hand's width off the wall.
fn find_window(sim: &Sim) -> Option<Found> {
    let c = &sim.move_cfg;
    sim.level.windows.iter().find_map(|window| {
        let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
        let spot = window.at - out * (c.radius + 0.32);
        let high = spot + Vec3::Y * 0.5;
        let hit = sim.world.raycast(high, Vec3::NEG_Y, 4.0)?;
        let feet = high - Vec3::Y * (hit.distance - 0.02);
        let low = sim.world.land_height(window.at.x, window.at.z).is_some_and(|land| window.at.y - land < 2.5);
        let open = sim.world.raycast(window.at + out * 0.3, out, 10.0).is_none();
        let yaw = (-out.x).atan2(-out.z);
        let input = MoveInput { yaw, ..MoveInput::default() };
        let mut state = MoveState::new(feet, c);
        state.grounded = true;
        (low && open && sim.world.capsule_is_clear(feet, c.capsule(Stance::Stand)) && plan_climb(&state, c, &input, &sim.world).is_some()).then_some(Found { feet, yaw })
    })
}

/// Scripts a `climb:` scene. Returns true once it has caught what it is after.
#[allow(clippy::too_many_arguments)]
pub fn drive_climb(
    spec: &str,
    t: f32,
    sim: &mut Sim,
    id: SoldierId,
    input: &mut PendingInput,
    held: &mut Buttons,
    third: &mut ThirdPerson,
    settings: &mut ViewSettings,
    clock: &mut Time<Virtual>,
    once: &mut bool,
    stool: &mut Local<Option<SoldierId>>,
) -> bool {
    let parts: Vec<&str> = spec.split(':').collect();
    let what = parts.first().copied().unwrap_or("vault");
    let on_someone = matches!(what, "stool" | "mount" | "roof");
    if !*once {
        *once = true;
        let found = match what {
            "stool" | "mount" => {
                let at = sim.soldiers[id].body.pos;
                Some(Found { feet: at + Vec3::Y * 0.05, yaw: sim.soldiers[id].yaw })
            }
            "window" => find_window(sim),
            "hint" => match parts.get(1).copied().unwrap_or("sill") {
                "sill" => find_window(sim),
                "wall" => find(sim, "mantle"),
                _ => find(sim, "refused"),
            },
            _ => find(sim, what),
        };
        if let Some(found) = found {
            input.yaw = found.yaw;
            let forward = yaw_basis(found.yaw).0;
            let mut feet = found.feet;
            if on_someone {
                let helper = sim.spawn_soldier(0x57001);
                // With their back where the climbing soldier's feet would be, so from there it is the same climb.
                let at = feet + forward * sim.move_cfg.stool.back_offset;
                sim.set_home(helper, at, found.yaw);
                let body = &mut sim.soldiers[helper].body;
                body.stance = Stance::Crouch;
                body.eye_height = sim.move_cfg.crouch_eye;
                sim.soldiers[helper].anim.hip_yaw = found.yaw;
                **stool = Some(helper);
                feet -= forward * 0.7;
            }
            sim.soldiers[id].body.pos = feet;
            sim.soldiers[id].yaw = found.yaw;
        } else {
            warn!("nowhere to do a {what} on this map");
        }
    }

    // Just standing there looking at it, to see what is lit and what is not.
    if what == "hint" {
        input.pitch = -0.45;
        if parts.get(2) == Some(&"third") {
            third.0 = true;
            input.free_look = Vec2::new(-0.9, -0.35);
            settings.third_person_offset = Vec3::new(0.0, 0.3, 2.6);
        }
        return t > 2.5;
    }
    input.wish_move = Vec2::ZERO;
    let through: f32 = std::env::var("PRS_THROUGH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.45);
    let body = sim.soldiers[id].body.clone();
    // The first press is onto them, and the second off them and up.
    let wants_off = what == "roof" && body.perch.on.is_some();
    let after_settling = if what == "stool" { t > 1.0 } else { t > 1.2 };
    let on_way = body.climb.active();
    let press_on = !on_way && after_settling && (body.perch.on.is_none() && !(what == "stool" && t > 1.5) || wants_off);
    if press_on && body.climb_cooldown == 0.0 {
        input.wish_move = Vec2::Y;
        // Off and on again, as a key has to be pressed for each climb.
        held.set(Buttons::JUMP, (t * 10.0) as i32 % 2 == 0);
    }
    if on_way {
        input.wish_move = Vec2::Y;
    }
    // In slow motion on the way up, since a frame can take longer than the moment to catch.
    clock.set_relative_speed(if on_way { 0.25 } else { 1.0 });

    match parts.get(1).copied().unwrap_or("first") {
        "first" => input.pitch = -0.05,
        "down" => input.pitch = -1.2,
        "third" => {
            third.0 = true;
            settings.third_person_offset = Vec3::new(0.0, 0.2, 3.2);
            input.pitch = -0.1;
        }
        "side" => {
            third.0 = true;
            input.free_look = Vec2::new(-1.45, -0.15);
            settings.third_person_offset = Vec3::new(0.0, 0.1, 3.0);
        }
        "front" => {
            third.0 = true;
            input.free_look = Vec2::new(std::f32::consts::PI - 0.5, -0.25);
            settings.third_person_offset = Vec3::new(0.0, 0.1, 3.2);
        }
        _ => {}
    }

    // Far enough back to get the one underneath in as well.
    if on_someone && third.0 {
        settings.third_person_offset = Vec3::new(0.0, -0.7, 4.0);
    }

    match what {
        "stool" => body.perch.on.is_some() && t > 3.0,
        "mount" => body.climb.kind == ClimbKind::Stool && body.climb.t >= through,
        "roof" => body.climb.kind == ClimbKind::PullUp && body.climb.t >= through,
        _ => on_way && body.climb.t >= through,
    }
}
