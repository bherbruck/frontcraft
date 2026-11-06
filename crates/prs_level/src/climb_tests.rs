//! Where climbing gets you on the town. Everything anyone could climb from the ground is found by asking,
//! from a place beside every side of everything built, alone and from someone's back.

use crate::relief::Relief;
use crate::spec::MapSpec;
use crate::{Level, Solid};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld};
use prs_movement::{ClimbKind, MoveConfig, MoveInput, MoveState, Perch, Stance, plan_climb, step};
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn level() -> &'static Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    LEVEL.get_or_init(|| Level::named("town").unwrap())
}

fn world() -> &'static CollisionWorld {
    static WORLD: OnceLock<CollisionWorld> = OnceLock::new();
    WORLD.get_or_init(|| level().collision())
}

struct Found {
    alone: bool,
    from: Vec3,
    to: Vec3,
    kind: ClimbKind,
    above_ground: f32,
}

/// Each climb to be had beside this solid: one standing point every metre along each of its four sides.
fn climbs_beside(solid: &Solid, c: &MoveConfig, relief: &Relief, out: &mut Vec<Found>) {
    let body = c.capsule(Stance::Stand);
    let top = solid.center.y + solid.half.y + 0.5;
    let (x, z) = (solid.rotation * Vec3::X, solid.rotation * Vec3::Z);
    for (normal, half, along, along_half) in [(x, solid.half.x, z, solid.half.z), (-x, solid.half.x, z, solid.half.z), (z, solid.half.z, x, solid.half.x), (-z, solid.half.z, x, solid.half.x)] {
        let flat = Vec3::new(normal.x, 0.0, normal.z);
        if flat.length() < 0.5 {
            continue;
        }
        let flat = flat.normalize();
        let yaw = (flat.x).atan2(flat.z);
        let steps = (along_half * 2.0).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let side = solid.center + normal * half + along * (i as f32 / steps as f32 * 2.0 - 1.0) * along_half;
            let spot = side + flat * (c.radius + 0.25);
            let high = Vec3::new(spot.x, top, spot.z);
            let Some(hit) = world().raycast(high, Vec3::NEG_Y, top + 200.0) else { continue };
            let feet = high - Vec3::Y * (hit.distance - 0.02);
            let ground = relief.height(Vec2::new(feet.x, feet.z));
            if hit.distance <= 0.0 || feet.y - ground > 0.4 || !world().capsule_is_clear(feet, body) {
                continue;
            }
            let input = MoveInput { yaw, ..MoveInput::default() };
            let mut state = MoveState::new(feet, c);
            state.grounded = true;
            if let Some(climb) = plan_climb(&state, c, &input, world()) {
                out.push(Found { alone: true, from: feet, to: climb.to, kind: climb.kind, above_ground: climb.to.y - ground });
            }
            let back = feet + Vec3::Y * c.stool.back_height;
            if world().capsule_is_clear(back, body) {
                state.pos = back;
                state.perch = Perch { on: Some(1), spot: back };
                if let Some(climb) = plan_climb(&state, c, &input, world()) {
                    out.push(Found { alone: false, from: back, to: climb.to, kind: climb.kind, above_ground: climb.to.y - ground });
                }
            }
        }
    }
}

fn everything() -> &'static Vec<Found> {
    static ALL: OnceLock<Vec<Found>> = OnceLock::new();
    ALL.get_or_init(|| {
        let c = MoveConfig::default();
        let spec = MapSpec::town();
        let relief = Relief::new(&spec);
        let mut found = Vec::new();
        for solid in &level().solids {
            climbs_beside(solid, &c, &relief, &mut found);
        }
        found
    })
}

/// What climbing does to the map, to look at after a change to either.
#[test]
#[ignore]
fn climb_report() {
    let all = everything();
    println!("{} climbs found, {} of them alone", all.len(), all.iter().filter(|f| f.alone).count());
    for (name, alone) in [("alone", true), ("from a back", false)] {
        let mut heights: BTreeMap<(i32, String), usize> = BTreeMap::new();
        let mut places = BTreeMap::<(i32, i32), f32>::new();
        for found in all.iter().filter(|f| f.alone == alone) {
            *heights.entry(((found.above_ground * 2.0).floor() as i32, format!("{:?}", found.kind))).or_default() += 1;
            if found.above_ground > 2.4 {
                let place = places.entry(((found.to.x / 6.0).floor() as i32, (found.to.z / 6.0).floor() as i32)).or_default();
                *place = place.max(found.above_ground);
            }
        }
        println!("{name}: landing height above the ground, half metre buckets");
        for ((bucket, kind), count) in &heights {
            println!("  {:4.1} m {kind:7} {count}", *bucket as f32 * 0.5);
        }
        println!("{name}: {} places 2.4 m or more up", places.len());
        let mut tops: BTreeMap<String, (usize, f32)> = BTreeMap::new();
        for found in all.iter().filter(|f| f.alone == alone && f.above_ground > 2.4) {
            let on = level().solids.iter().find(|s| {
                let local = s.rotation.inverse() * (found.to - s.center);
                local.x.abs() <= s.half.x && local.z.abs() <= s.half.z && (local.y - s.half.y).abs() < 0.3
            });
            let entry = tops.entry(on.map_or("nothing".to_owned(), |s| format!("{:?}", s.paint))).or_default();
            entry.0 += 1;
            entry.1 = entry.1.max(found.above_ground);
        }
        println!("{name}: what those are, with how many ways up and the highest: {tops:?}");
        let mut in_zones: BTreeMap<&str, usize> = BTreeMap::new();
        for found in all.iter().filter(|f| f.alone == alone && f.above_ground > 2.4) {
            for zone in &level().capture_points {
                if Vec2::new(found.to.x - zone.center.x, found.to.z - zone.center.z).length() <= zone.radius {
                    *in_zones.entry(&zone.name).or_default() += 1;
                }
            }
        }
        println!("{name}: up high inside a capture zone: {in_zones:?}");
    }
    let highest = all.iter().map(|f| f.above_ground).fold(0.0f32, f32::max);
    println!("highest anywhere: {highest:.2} m");
    let chained = from_up_there();
    let top = chained.iter().map(|at| at.y).fold(f32::MIN, f32::max);
    println!("{} places on from there, alone, with climbs and walking off left out; highest at y = {top:.2}", chained.len());
    // And without any help at all, which anyone on the other side can do just the same.
    let spec = MapSpec::town();
    let relief = Relief::new(&spec);
    let solo = carry_on(all.iter().filter(|f| f.alone).map(|f| f.to).collect());
    let over = |at: &Vec3| at.y - relief.height(Vec2::new(at.x, at.z));
    let in_zone = |at: &Vec3| level().capture_points.iter().any(|z| Vec2::new(at.x - z.center.x, at.z - z.center.z).length() <= z.radius);
    let high = solo.iter().map(over).fold(0.0f32, f32::max);
    let high_in_zone = solo.iter().filter(|at| in_zone(at)).map(over).fold(0.0f32, f32::max);
    println!("alone and carrying on: {} climbs, highest {high:.2} m over the land, {high_in_zone:.2} m inside a zone", solo.len());
}

/// Where someone can carry on to from every place that took a back to reach, going on climbing alone.
fn from_up_there() -> Vec<Vec3> {
    carry_on(everything().iter().filter(|f| !f.alone && f.above_ground > 2.4).map(|f| f.to).collect())
}

/// Everywhere going on climbing alone from these gets to.
fn carry_on(mut open: Vec<Vec3>) -> Vec<Vec3> {
    let c = MoveConfig::default();
    let mut seen = std::collections::BTreeSet::new();
    let mut reached = Vec::new();
    while let Some(at) = open.pop() {
        if !seen.insert(((at.x * 2.0) as i32, (at.y * 2.0) as i32, (at.z * 2.0) as i32)) {
            continue;
        }
        for turn in 0..12 {
            let mut state = MoveState::new(at, &c);
            state.grounded = true;
            let input = MoveInput { yaw: turn as f32 * std::f32::consts::TAU / 12.0, ..MoveInput::default() };
            if let Some(climb) = plan_climb(&state, &c, &input, world())
                && climb.to.y > at.y
            {
                reached.push(climb.to);
                open.push(climb.to);
            }
        }
    }
    reached
}

/// Going on from a roof doesn't get anyone off the map either, however far they keep at it.
#[test]
fn nothing_climbs_out_of_the_map_from_a_roof() {
    let spec = MapSpec::town();
    let safe = spec.terrain.size * 0.5 - spec.terrain.rim.face;
    for at in from_up_there() {
        assert!(at.x.abs() < safe && at.z.abs() < safe, "to {at}");
    }
}

/// Every climb there is on the map, gone through as a player would. They all finish without letting go,
/// standing somewhere a body fits, with the eye never in anything on the way.
#[test]
fn every_climb_goes_as_planned_and_the_eye_stays_out_of_everything() {
    const DT: f32 = 1.0 / 60.0;
    let c = MoveConfig::default();
    let eye_ball = Capsule { height: 0.1, radius: 0.05 };
    let (mut gone, mut events) = (0, Vec::new());
    for found in everything().iter().step_by(3) {
        let mut state = MoveState::new(found.from, &c);
        state.grounded = true;
        if !found.alone {
            state.perch = Perch { on: Some(1), spot: found.from };
        }
        let flat = Vec3::new(found.to.x - found.from.x, 0.0, found.to.z - found.from.z);
        let yaw = (-flat.x).atan2(-flat.z);
        // The way found, tried again along the line it went: anything nearby can come up first.
        let press = MoveInput { yaw, wish: Vec2::Y, jump_pressed: true, ..MoveInput::default() };
        step(&mut state, &c, &press, world(), DT, &mut events);
        if !state.climb.active() {
            continue;
        }
        gone += 1;
        let walk = MoveInput { jump_pressed: false, ..press };
        let target = state.climb.to;
        for _ in 0..600 {
            step(&mut state, &c, &walk, world(), DT, &mut events);
            let centre = state.eye() - Vec3::Y * 0.05;
            assert!(world().capsule_is_clear(centre, eye_ball), "eye in something at {} going to {target}", state.eye());
            if !state.climb.active() {
                break;
            }
        }
        assert!(!state.climb.active(), "still climbing at {} going to {target}", state.pos);
        assert_eq!(state.pos, target, "let go on the way to {target}");
        assert!(world().capsule_is_clear(state.pos, c.capsule(state.stance)), "ended up in something at {}", state.pos);
        events.clear();
    }
    assert!(gone > 1000, "only {gone} went");
}

/// Nothing that can be climbed lands anyone within the rim, or anywhere off the map.
#[test]
fn nothing_climbs_out_of_the_map() {
    let spec = MapSpec::town();
    let edge = spec.terrain.size * 0.5;
    let safe = edge - spec.terrain.rim.face;
    for found in everything() {
        assert!(found.to.x.abs() < safe && found.to.z.abs() < safe, "{:?} from {} to {}", found.kind, found.from, found.to);
    }
}

/// The rim is land, too steep to walk and with no edge to get a hand to. Asked all the way round, from
/// the foot of it up to the top of what can be walked, looking straight at it and from either side.
#[test]
fn the_rim_cannot_be_climbed_alone_or_from_a_back() {
    let c = MoveConfig::default();
    let spec = MapSpec::town();
    let relief = Relief::new(&spec);
    let edge = spec.terrain.size * 0.5;
    let body = Capsule { height: c.stand_height, radius: c.radius };
    let mut asked = 0;
    for side in 0..4 {
        let (out, across) = match side {
            0 => (Vec2::X, Vec2::Y),
            1 => (Vec2::NEG_X, Vec2::Y),
            2 => (Vec2::Y, Vec2::X),
            _ => (Vec2::NEG_Y, Vec2::X),
        };
        let mut along = -edge;
        while along < edge {
            let mut inward = 2.0;
            while inward < spec.terrain.rim.inset {
                let at = out * (edge - inward) + across * along;
                let feet = Vec3::new(at.x, relief.height(at) + 0.05, at.y);
                for turn in [-0.8f32, 0.0, 0.8] {
                    let dir = Vec2::new(out.x * turn.cos() - out.y * turn.sin(), out.x * turn.sin() + out.y * turn.cos());
                    let yaw = (-dir.x).atan2(-dir.y);
                    let input = MoveInput { yaw, ..MoveInput::default() };
                    let mut state = MoveState::new(feet, &c);
                    state.grounded = true;
                    if !world().capsule_is_clear(feet, body) {
                        continue;
                    }
                    asked += 1;
                    let alone = plan_climb(&state, &c, &input, world());
                    assert!(alone.is_none(), "alone at {feet} to {}", alone.unwrap().to);
                    state.pos += Vec3::Y * c.stool.back_height;
                    state.perch = Perch { on: Some(1), spot: state.pos };
                    let boosted = plan_climb(&state, &c, &input, world());
                    assert!(boosted.is_none(), "from a back at {feet} to {}", boosted.unwrap().to);
                }
                inward += 6.0;
            }
            along += 6.0;
        }
    }
    assert!(asked > 10_000, "only asked {asked}");
}

/// One way through one window: where to stand, facing in through it.
struct ThroughWindow {
    window: usize,
    storey_up: bool,
    inward: bool,
    crouched: bool,
    feet: Vec3,
    yaw: f32,
}

/// Every window on the town, from in front of it on both sides, standing and crouched.
fn window_tries(c: &MoveConfig) -> Vec<ThroughWindow> {
    let mut tries = Vec::new();
    for (i, window) in level().windows.iter().enumerate() {
        let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
        let land = Relief::new(&MapSpec::town()).height(Vec2::new(window.at.x, window.at.z));
        let storey_up = window.at.y - land > 3.0;
        for inward in [true, false] {
            // Facing the window from in front of it, a hand's width off the wall.
            let side = if inward { out } else { -out };
            let spot = window.at + side * (c.radius + 0.15 + 0.17);
            let high = spot + Vec3::Y * 0.5;
            let Some(hit) = world().raycast(high, Vec3::NEG_Y, 6.0) else { continue };
            let feet = high - Vec3::Y * (hit.distance - 0.02);
            if hit.distance <= 0.0 || !world().capsule_is_clear(feet, c.capsule(Stance::Stand)) {
                continue;
            }
            let facing = -side;
            let yaw = (-facing.x).atan2(-facing.z);
            for crouched in [false, true] {
                tries.push(ThroughWindow { window: i, storey_up, inward, crouched, feet, yaw });
            }
        }
    }
    tries
}

fn try_window(c: &MoveConfig, t: &ThroughWindow) -> Option<prs_movement::Climb> {
    let mut state = MoveState::new(t.feet, c);
    state.grounded = true;
    if t.crouched {
        state.stance = Stance::Crouch;
        state.eye_height = c.crouch_eye;
    }
    plan_climb(&state, c, &MoveInput { yaw: t.yaw, ..MoveInput::default() }, world())
}

#[test]
#[ignore]
fn window_report() {
    let c = MoveConfig::default();
    let tries = window_tries(&c);
    for storey_up in [false, true] {
        for inward in [true, false] {
            for crouched in [false, true] {
                let these: Vec<_> = tries.iter().filter(|t| t.storey_up == storey_up && t.inward == inward && t.crouched == crouched).collect();
                let through = these.iter().filter(|t| try_window(&c, t).is_some_and(|climb| (climb.to - level().windows[t.window].at).dot(Vec3::new(level().windows[t.window].outward.x, 0.0, level().windows[t.window].outward.z)) * if t.inward { -1.0 } else { 1.0 } > 0.2)).count();
                println!(
                    "{} {} {}: through {} of {}",
                    if storey_up { "upstairs" } else { "ground floor" },
                    if inward { "from outside in" } else { "from inside out" },
                    if crouched { "crouched" } else { "standing" },
                    through,
                    these.len()
                );
            }
        }
    }
}

/// Whether a climb took them through to the far side of its window.
fn came_through(t: &ThroughWindow, at: Vec3) -> bool {
    let window = &level().windows[t.window];
    let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
    (at - window.at).dot(out) * if t.inward { -1.0 } else { 1.0 } > 0.2
}

/// Goes through with a climb as a player would, and checks the whole way: never let go, never the eye in
/// anything, and nothing sudden for the eye. Returns where they ended up.
fn climb_through(c: &MoveConfig, t: &ThroughWindow) -> Result<Vec3, String> {
    const DT: f32 = 1.0 / 60.0;
    let mut state = MoveState::new(t.feet, c);
    state.grounded = true;
    if t.crouched {
        state.stance = Stance::Crouch;
        state.eye_height = c.crouch_eye;
    }
    let mut events = Vec::new();
    let press = MoveInput { yaw: t.yaw, wish: Vec2::Y, jump_pressed: true, ..MoveInput::default() };
    step(&mut state, c, &press, world(), DT, &mut events);
    if !state.climb.active() {
        return Err("did not start".into());
    }
    let target = state.climb.to;
    let ball = Capsule { height: 0.1, radius: 0.05 };
    let (mut last, mut speed, mut fastest, mut hardest) = (state.eye().y, 0.0f32, 0.0f32, 0.0f32);
    let walk = MoveInput { jump_pressed: false, ..press };
    for _ in 0..600 {
        step(&mut state, c, &walk, world(), DT, &mut events);
        if !world().capsule_is_clear(state.eye() - Vec3::Y * 0.05, ball) {
            return Err(format!("eye in something at {}", state.eye()));
        }
        let now = (state.eye().y - last) / DT;
        hardest = hardest.max((now - speed).abs() / DT);
        fastest = fastest.max(now.abs());
        speed = now;
        last = state.eye().y;
        if !state.climb.active() {
            break;
        }
    }
    if state.pos != target {
        return Err(format!("let go at {} on the way to {target}", state.pos));
    }
    if !world().capsule_is_clear(state.pos, c.capsule(state.stance)) {
        return Err(format!("ended up in something at {}", state.pos));
    }
    if fastest > 3.0 {
        return Err(format!("the eye went at {fastest} m/s"));
    }
    if hardest > 40.0 {
        return Err(format!("the eye was thrown about at {hardest} m/s/s"));
    }
    Ok(state.pos)
}

/// The town's windows are all of a kind: a sill at chest height in a wall a hand thick, with a lintel
/// close enough over it that nobody goes through upright. Each way, from standing and from a crouch, at
/// every ground floor window, a soldier goes through flat over the sill and ends up on the other side.
fn through_every_ground_floor_window(inward: bool, crouched: bool) {
    let c = MoveConfig::default();
    let tries: Vec<_> = window_tries(&c).into_iter().filter(|t| !t.storey_up && t.inward == inward && t.crouched == crouched).collect();
    assert!(tries.len() > 300, "only {} windows to try", tries.len());
    let mut failed = Vec::new();
    for t in &tries {
        match climb_through(&c, t) {
            Ok(at) if came_through(t, at) => {}
            Ok(at) => failed.push(format!("window {} ended at {at}, not through", t.window)),
            Err(why) => failed.push(format!("window {}: {why}", t.window)),
        }
    }
    assert!(failed.is_empty(), "{} of {} failed, the first: {:?}", failed.len(), tries.len(), &failed[..failed.len().min(3)]);
}

#[test]
fn in_through_a_ground_floor_window_standing() {
    through_every_ground_floor_window(true, false);
}

#[test]
fn in_through_a_ground_floor_window_crouched() {
    through_every_ground_floor_window(true, true);
}

#[test]
fn out_through_a_ground_floor_window_standing() {
    through_every_ground_floor_window(false, false);
}

#[test]
fn out_through_a_ground_floor_window_crouched() {
    through_every_ground_floor_window(false, true);
}

/// Out of an upstairs window is a drop nobody climbs down, so it is refused, and not by half: there is no
/// climb at all, rather than one that starts and lets go.
#[test]
fn out_of_an_upstairs_window_is_refused() {
    let c = MoveConfig::default();
    let tries: Vec<_> = window_tries(&c).into_iter().filter(|t| t.storey_up && !t.inward).collect();
    assert!(tries.len() > 100, "only {}", tries.len());
    for t in &tries {
        if let Some(climb) = try_window(&c, t) {
            let land = Relief::new(&MapSpec::town()).height(Vec2::new(climb.to.x, climb.to.z));
            assert!(climb.to.y - land > 2.0, "window {} climbs out and down to the ground at {}", t.window, climb.to);
        }
    }
}

/// Into an upstairs window from whatever is outside it, a roof next door, say, goes the same as downstairs.
#[test]
fn in_through_an_upstairs_window_from_a_roof() {
    let c = MoveConfig::default();
    let tries: Vec<_> = window_tries(&c).into_iter().filter(|t| t.storey_up && t.inward && try_window(&c, t).is_some()).collect();
    assert!(!tries.is_empty(), "nowhere to try it");
    for t in &tries {
        let at = climb_through(&c, t).unwrap_or_else(|why| panic!("window {}: {why}", t.window));
        assert!(came_through(t, at), "window {} ended at {at}", t.window);
    }
}

