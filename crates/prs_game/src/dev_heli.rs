//! Scripted helicopter scenes. See `dev.rs` for how they are chosen.

use bevy::prelude::*;
use prs_runtime::prs_sim::helicopter::autopilot::{Autopilot, Order};
use prs_runtime::prs_sim::helicopter::{Engine, HeliZone};
use prs_runtime::prs_sim::protocol::Buttons;
use prs_runtime::prs_sim::seats::SeatedAt;
use prs_runtime::prs_sim::{Sim, SoldierId, heli_carrier};
use prs_runtime::{PendingInput, ThirdPerson};
use prs_view::ViewSettings;

/// Scripts one of the `heli:` scenes, on the map's first helicopter.
#[allow(clippy::too_many_arguments)]
pub fn drive_heli(
    spec: &str,
    t: f32,
    sim: &mut Sim,
    id: SoldierId,
    input: &mut PendingInput,
    held: &mut Buttons,
    third: &mut ThirdPerson,
    settings: &mut ViewSettings,
    end: &mut f32,
    once: &mut bool,
    pilot: &mut Autopilot,
) {
    let carrier = heli_carrier(0);
    let air = matches!(spec, "hovering" | "cruise" | "left" | "follow" | "hover" | "dust" | "gunner" | "chase");
    let keep_flying = matches!(spec, "hovering" | "cruise" | "left" | "follow");
    let fast = matches!(spec, "cruise" | "follow");
    if !*once {
        *once = true;
        match spec {
            "cockpit" | "hovering" | "cruise" | "left" | "follow" => sim.sit_in(id, SeatedAt { carrier, seat: 0 }),
            "gunner" => sim.sit_in(id, SeatedAt { carrier, seat: 3 }),
            _ => {}
        }
        let state = &mut sim.helicopters[0].state;
        if spec != "outside" && spec != "wreck" {
            state.engine = Engine::Running;
            state.spool = 1.0;
            state.power = 0.3;
            state.rotor_speed = sim.heli_cfg.rotor.nominal_speed;
        }
        if air {
            state.pos.y += match spec {
                "hover" => 9.0,
                "dust" => 3.0,
                _ => 40.0,
            };
            state.parked = false;
            state.on_ground = false;
            state.skids = 0;
            state.collective = 0.45;
            state.induced = 8.0;
            state.power = 0.6;
        }
        if fast {
            state.vel = state.forward() * 38.0;
        }
        if spec == "wreck" {
            sim.bullet_into_helicopter(0, HeliZone::Structure, 1.0e5);
        }
    }
    let state = sim.helicopters[0].state;
    let frame = sim.carrier_frame(carrier);
    let home_yaw = sim.helicopters[0].home_yaw;
    if keep_flying {
        let forward = Vec2::new(state.forward().x, state.forward().z).normalize();
        let speed = if fast { 38.0 } else { 0.0 };
        let order = Order { velocity: forward * speed, climb: 0.0, heading: state.heading(), lean_limit: 0.2 };
        let wanted = pilot.fly(&state, &order, 1.0 / 60.0);
        input.wish_move = Vec2::new(wanted.pedals, wanted.collective);
        input.stick.pos = wanted.stick;
    } else if spec == "chase" {
        let held_there = &mut sim.helicopters[0].state;
        held_there.vel = held_there.forward() * 40.0;
        held_there.spin = Vec3::ZERO;
        held_there.rot = Quat::from_rotation_y(home_yaw) * Quat::from_rotation_x(-0.08);
    } else if air {
        // Pinned in the air with the rotor turning, to look at from outside or from the door.
        let held_there = &mut sim.helicopters[0].state;
        held_there.vel = Vec3::ZERO;
        held_there.spin = Vec3::ZERO;
        held_there.rot = Quat::from_rotation_y(home_yaw);
    }
    match spec {
        "cockpit" | "hovering" | "cruise" => *end = 3.5,
        "left" => {
            input.free_look = Vec2::new(1.1, -0.15);
            *end = 3.0;
        }
        "follow" => {
            third.0 = true;
            settings.third_person_offset = Vec3::new(2.0, 2.5, 16.0);
            *end = 3.5;
        }
        "gunner" => {
            input.yaw = frame.yaw() - std::f32::consts::FRAC_PI_2;
            input.pitch = -0.25;
            held.set(Buttons::FIRE, t > 2.5);
            *end = 3.0;
        }
        _ => {
            // Stood off to one side, looking at it from outside.
            // `PRS_AROUND=<degrees>` walks round it.
            let around = std::env::var("PRS_AROUND").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
            let at = sim.helicopters[0].home + Quat::from_rotation_y(home_yaw + around) * Vec3::new(-11.0, 0.0, -9.0);
            let top = at + Vec3::Y * 30.0;
            let ground = sim.world.raycast(top, Vec3::NEG_Y, 60.0).map_or(at.y, |hit| top.y - hit.distance);
            let feet = Vec3::new(at.x, ground + 0.05, at.z);
            sim.soldiers[id].body.pos = feet;
            let d = state.pos - (feet + Vec3::Y * 1.6);
            input.yaw = (-d.x).atan2(-d.z);
            input.pitch = (d.y / d.length()).asin().clamp(-1.0, 1.0);
            *end = 3.0;
        }
    }
}
