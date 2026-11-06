//! Scripted scenes of a gun set down on something, on the real windows of the town. See `dev.rs` for
//! how they are chosen.
//!
//! `PRS_SCENE=rest:<first|third|side|front|outside>[:<yaw>[:<pitch>[:hood]]]`, best with `PRS_MAP=town PRS_MODE=practice`,
//! goes to a ground floor window from the inside, standing, sets the gun on the sill and raises the sights.
//! `<yaw>` and `<pitch>` swing the barrel on the rest by that many radians, as far as they will go, and `hood`
//! is the bonnet of a truck in place of the window. `outside` looks back in from across the street, and
//! `nowhere` tries it with nothing to rest on, to see what it says.

use bevy::prelude::*;
use prs_runtime::prs_sim::movement::Stance;
use prs_runtime::prs_sim::protocol::Buttons;
use prs_runtime::prs_sim::{Sim, SoldierId};
use prs_runtime::{PendingInput, ThirdPerson};
use prs_view::ViewSettings;

/// A window with a good bit of clear ground outside it, and the place inside it, and the way out through it.
fn find_window(sim: &Sim) -> Option<(Vec3, f32)> {
    let c = &sim.move_cfg;
    sim.level.windows.iter().find_map(|window| {
        let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
        let inside = window.at - out * (c.radius + 0.17);
        let hit = sim.world.raycast(inside, Vec3::NEG_Y, 3.0)?;
        let feet = inside - Vec3::Y * (hit.distance - 0.03);
        let open = sim.world.raycast(window.at + out * 0.3, out, 15.0).is_none();
        let low = sim.world.land_height(window.at.x, window.at.z).is_some_and(|land| window.at.y - land < 2.5);
        (open && low && sim.world.capsule_is_clear(feet, c.capsule(Stance::Crouch))).then_some((feet, out.x.atan2(out.z) + std::f32::consts::PI))
    })
}

/// Beside the bonnet of the first truck, looking across it.
fn beside_truck(sim: &Sim) -> Option<(Vec3, f32)> {
    let frame = sim.vehicles.first()?.frame();
    let side = frame.point(Vec3::new(1.5, 0.0, -1.55));
    let across = (frame.point(Vec3::new(-1.5, 0.0, -1.55)) - side).with_y(0.0).normalize();
    let top = side + Vec3::Y * 8.0;
    let hit = sim.world.raycast(top, Vec3::NEG_Y, 30.0)?;
    Some((top - Vec3::Y * (hit.distance - 0.03), across.x.atan2(across.z) + std::f32::consts::PI))
}

/// Scripts a `rest:` scene. Returns true once it is what it should be.
#[allow(clippy::too_many_arguments)]
pub fn drive_rest(
    spec: &str,
    t: f32,
    sim: &mut Sim,
    id: SoldierId,
    input: &mut PendingInput,
    held: &mut Buttons,
    third: &mut ThirdPerson,
    settings: &mut ViewSettings,
    once: &mut bool,
) -> bool {
    let parts: Vec<&str> = spec.split(':').collect();
    let camera = parts.first().copied().unwrap_or("first");
    let swing: f32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let tip: f32 = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let hood = parts.get(3) == Some(&"hood");
    if !*once {
        *once = true;
        if camera != "nowhere" {
            let place = if hood { beside_truck(sim) } else { find_window(sim) };
            match place {
                Some((feet, yaw)) => {
                    sim.soldiers[id].body.pos = feet;
                    sim.soldiers[id].yaw = yaw;
                    input.yaw = yaw;
                    sim.set_home(id, feet, yaw);
                }
                None => warn!("nowhere to rest on on this map"),
            }
        }
    }
    let rest = sim.soldiers[id].rest;
    // By what has happened and not the clock, as a frame can take longer than a tap.
    held.set(Buttons::REST, t > 1.2 && !rest.on && rest.refused == 0.0 && (t * 10.0) as i32 % 2 == 0);
    held.set(Buttons::ADS, rest.on);
    if !rest.on {
        input.pitch = 0.03;
    }
    if t > 3.0 && rest.on {
        // Over to it through the same stop a mouse goes through.
        let wanted = Vec2::new(rest.yaw + swing, rest.pitch + tip);
        let now = Vec2::new(input.yaw, input.pitch);
        let next = now + (wanted - now).clamp_length_max(0.8 / 60.0);
        if let Some(arc) = sim.rest_arc(id, next.x) {
            let got = sim.soldiers[id].held_config(&sim.weapons).rest.stop_look(&arc, now, next, 1.0 / 60.0);
            input.yaw = got.x;
            input.pitch = got.y;
        }
    }
    third.0 = matches!(camera, "third" | "side" | "outside" | "front");
    match camera {
        "third" => settings.third_person_offset = Vec3::new(0.0, 0.25, 1.8),
        "side" => {
            input.free_look = Vec2::new(-1.45, -0.12 - input.pitch);
            settings.third_person_offset = Vec3::new(0.0, -0.1, 2.0);
        }
        "front" => {
            input.free_look = Vec2::new(2.3, -0.1 - input.pitch);
            settings.third_person_offset = Vec3::new(0.0, -0.1, 2.0);
        }
        "outside" => {
            input.free_look = Vec2::new(std::f32::consts::PI - 0.25, -0.1);
            settings.third_person_offset = Vec3::new(0.0, 0.0, 2.4);
        }
        _ => {}
    }
    if camera == "nowhere" {
        return rest.refused > 0.0;
    }
    t > 6.0 && rest.settle >= 1.0
}
