//! Holds the town to its promises: every stair climbs, every door fits, cover hides who it should.

use crate::spec::*;
use crate::{Level, MapSpec, Paint};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld, Surface};
use prs_movement::{MoveConfig, MoveEvent, MoveInput, MoveState, Stance, step};
use std::sync::OnceLock;
use std::time::Instant;

const DT: f32 = 1.0 / 60.0;

fn level() -> &'static Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    LEVEL.get_or_init(|| MapSpec::town().build())
}

fn world() -> &'static CollisionWorld {
    static WORLD: OnceLock<CollisionWorld> = OnceLock::new();
    WORLD.get_or_init(|| level().collision())
}

fn cfg() -> MoveConfig {
    MoveConfig::default()
}

fn body(c: &MoveConfig) -> Capsule {
    Capsule { height: c.stand_height, radius: c.radius }
}

/// Walks a soldier towards each point in turn. Returns where they got stuck, if they did.
fn walk(start: Vec3, path: &[Vec3], c: &MoveConfig, w: &CollisionWorld) -> Result<MoveState, String> {
    let mut state = MoveState::new(start + Vec3::Y * 0.05, c);
    let mut events: Vec<MoveEvent> = Vec::new();
    for _ in 0..60 {
        step(&mut state, c, &MoveInput::default(), w, DT, &mut events);
    }
    for target in path {
        let mut ticks = 0;
        loop {
            let to = *target - state.pos;
            let flat = Vec2::new(to.x, to.z);
            if flat.length() < 0.35 && to.y.abs() < 0.6 {
                break;
            }
            ticks += 1;
            if ticks > 60 * 25 {
                return Err(format!("stuck at {} on the way to {target}", state.pos));
            }
            let yaw = (-flat.x).atan2(-flat.y);
            let input = MoveInput { wish: Vec2::Y, yaw, ..MoveInput::default() };
            events.clear();
            step(&mut state, c, &input, w, DT, &mut events);
        }
    }
    Ok(state)
}

/// The surfaces underfoot in the order they came, walking a soldier along a path.
fn footsteps_along(path: &[Vec3], c: &MoveConfig, w: &CollisionWorld) -> Vec<Surface> {
    let mut state = MoveState::new(path[0] + Vec3::Y * 0.05, c);
    let mut heard = Vec::new();
    let mut events = Vec::new();
    for target in &path[1..] {
        for _ in 0..60 * 20 {
            let to = *target - state.pos;
            let flat = Vec2::new(to.x, to.z);
            if flat.length() < 0.35 {
                break;
            }
            let input = MoveInput { wish: Vec2::Y, yaw: (-flat.x).atan2(-flat.y), ..MoveInput::default() };
            events.clear();
            step(&mut state, c, &input, w, DT, &mut events);
            for event in &events {
                if let MoveEvent::Footstep { surface, .. } = event
                    && heard.last() != Some(surface)
                {
                    heard.push(*surface);
                }
            }
        }
    }
    heard
}

#[test]
fn footsteps_change_from_road_to_field() {
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let c = cfg();
    // On the west road, then straight off it north into the wheat.
    let lift = |x: f32, z: f32| {
        let p = Vec2::new(x, z);
        Vec3::new(x, relief.height(p) + 0.1, z)
    };
    let road = spec.roads[1].points[2].lerp(spec.roads[1].points[3], 0.3);
    let heard = footsteps_along(&[lift(road.x, road.y), lift(road.x + 12.0, road.y), lift(road.x + 12.0, road.y - 45.0)], &c, world());
    assert_eq!(heard.first(), Some(&Surface::Gravel), "{heard:?}");
    assert_eq!(heard.last(), Some(&Surface::Grass), "{heard:?}");
}

#[test]
fn footsteps_change_going_indoors_onto_wood() {
    let c = cfg();
    let w = world();
    let down = |p: Vec3| w.raycast(p + Vec3::Y * 0.3, Vec3::NEG_Y, 0.6).map(|hit| hit.surface);
    let mut walked = 0;
    for door in &level().doorways {
        let (outside, inside) = (door.at + door.outward * 6.0, door.at - door.outward * 4.5);
        if down(inside) != Some(Surface::Wood) || down(door.at + door.outward * 1.5) == Some(Surface::Wood) {
            continue;
        }
        let heard = footsteps_along(&[outside + Vec3::Y * 0.2, inside], &c, w);
        if heard.last() == Some(&Surface::Wood) && heard.len() >= 2 {
            walked += 1;
        }
    }
    assert!(walked > 10, "only {walked} doors led from outside onto a wooden floor");
}

#[test]
fn surfaces_underfoot_are_what_is_drawn() {
    let w = world();
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let on = |x: f32, z: f32| {
        let p = Vec2::new(x, z);
        w.raycast(Vec3::new(x, relief.height(p) + 0.3, z), Vec3::NEG_Y, 1.0).unwrap().surface
    };
    // The market square, a road, a yard and a field.
    assert_eq!(on(-30.0, 16.0), Surface::Concrete);
    let road = spec.roads[0].points[2].lerp(spec.roads[0].points[3], 0.5);
    assert_eq!(on(road.x, road.y), Surface::Gravel);
    assert_eq!(on(-178.0, -48.0), Surface::Grass);
    // Every kind of solid reports the surface of its paint, seen from straight above.
    let mut tin_roofs = 0;
    for solid in &level().solids {
        let top = solid.center + solid.rotation * (Vec3::Y * solid.half.y) + Vec3::Y * 0.05;
        if solid.paint == Paint::Tin && solid.half.y < 0.2 {
            let hit = w.raycast(top, Vec3::NEG_Y, 0.3).unwrap();
            assert_eq!(hit.surface, Surface::Metal);
            tin_roofs += 1;
        }
    }
    assert!(tin_roofs > 4, "{tin_roofs}");
}

const SENTRY_CLEAR_RANGE: f32 = 75.0;

/// On both maps the stand-in soldier has somewhere to stand, and open air in front of the gun.
#[test]
fn the_sentry_has_a_clear_line_of_fire() {
    let c = cfg();
    let range = Level::training_range();
    let range_world = range.collision();
    for (l, w) in [(level(), world()), (&range, &range_world)] {
        let sentry = l.sentry;
        assert!(w.capsule_is_clear(sentry.pos, body(&c)), "sentry stands in something at {}", sentry.pos);
        let down = w.cast_capsule(sentry.pos, body(&c), Vec3::NEG_Y, 2.0).expect("no ground under the sentry");
        assert!(down.distance < 0.3, "floats {}", down.distance);
        let forward = Vec3::new(-sentry.yaw.sin(), 0.0, -sentry.yaw.cos());
        let eye = sentry.pos + Vec3::Y * c.stand_eye;
        // The land can stop the bullets, as long as it is well away.
        let hit = w.raycast(eye, forward, SENTRY_CLEAR_RANGE);
        assert!(hit.is_none(), "blocked at {hit:?}");
    }
}

/// A flag is seen from afar and fought around, so it stands in the open and not on a roof.
#[test]
fn every_capture_point_stands_on_open_ground() {
    let c = cfg();
    for point in &level().capture_points {
        let feet = point.center + Vec3::Y * 0.1;
        assert!(world().capsule_is_clear(feet, body(&c)), "{} is inside something", point.name);
        assert!(world().raycast(feet + Vec3::Y * 0.2, Vec3::Y, 60.0).is_none(), "{} has something over it", point.name);
    }
}

#[test]
fn every_staircase_can_be_climbed() {
    let c = cfg();
    assert!(level().staircases.len() > 15, "{}", level().staircases.len());
    let mut failed = Vec::new();
    for (i, stairs) in level().staircases.iter().enumerate() {
        let end = *stairs.path.last().unwrap();
        match walk(stairs.path[0], &stairs.path[1..], &c, world()) {
            Ok(state) => {
                if (state.pos.y - end.y).abs() > 0.3 {
                    failed.push(format!("stairs {i}: ended at {} not {end}", state.pos));
                }
            }
            Err(why) => failed.push(format!("stairs {i} at {}: {why}", stairs.path[0])),
        }
    }
    assert!(failed.is_empty(), "{} of {}:\n{}", failed.len(), level().staircases.len(), failed.join("\n"));
}

#[test]
fn every_doorway_can_be_walked_through_both_ways() {
    let c = cfg();
    let mut failed = Vec::new();
    for (i, door) in level().doorways.iter().enumerate() {
        assert!(door.width > c.radius * 2.0 + 0.2, "door {i} too narrow");
        let out = door.at + door.outward * 1.2;
        let inside = door.at - door.outward * 1.2;
        for (from, to) in [(out, inside), (inside, out)] {
            if let Err(why) = walk(from + Vec3::Y * 0.3, &[to], &c, world()) {
                failed.push(format!("door {i} at {}: {why}", door.at));
                break;
            }
        }
    }
    assert!(failed.is_empty(), "{} of {}:\n{}", failed.len(), level().doorways.len(), failed.join("\n"));
}

#[test]
fn doorways_fit_a_standing_capsule() {
    let c = cfg();
    for door in &level().doorways {
        let feet = door.at + Vec3::Y * 0.05;
        assert!(world().capsule_is_clear(feet, body(&c)), "blocked door at {}", door.at);
    }
}

#[test]
fn windows_can_be_shot_through_and_the_wall_beside_them_cannot() {
    let mut blocked = Vec::new();
    for window in &level().windows {
        let from = window.at + window.outward;
        if world().raycast(from, -window.outward, 2.0).is_some() {
            blocked.push(format!("window at {}", window.at));
        }
        // The wall just above the window.
        let above = from + Vec3::Y * (window.height * 0.5 + 0.12);
        assert!(world().raycast(above, -window.outward, 2.0).is_some(), "no wall above window at {}", window.at);
    }
    assert!(blocked.is_empty(), "{} of {}:\n{}", blocked.len(), level().windows.len(), blocked.join("\n"));
}

#[test]
fn walls_stop_bullets_and_are_solid_from_both_sides() {
    let mut count = 0;
    for solid in &level().solids {
        if !matches!(solid.paint, Paint::Limewash | Paint::Mudbrick | Paint::Ochre | Paint::Cement | Paint::Brick) {
            continue;
        }
        count += 1;
        let axis = solid.rotation * Vec3::Z;
        let side = solid.half.z + 0.5;
        for dir in [axis, -axis] {
            let from = solid.center - dir * side;
            let hit = world().raycast(from, dir, side * 2.0);
            assert!(hit.is_some(), "a bullet passed through the solid at {}", solid.center);
        }
    }
    assert!(count > 100, "{count}");
}

#[test]
fn every_spawn_is_clear_and_on_the_ground() {
    let c = cfg();
    assert!(level().teams.len() == 2);
    for team in &level().teams {
        assert!(team.spawns.len() >= 8, "{}", team.name);
        for spawn in &team.spawns {
            assert!(world().capsule_is_clear(spawn.pos, body(&c)), "{} spawn at {} is blocked", team.name, spawn.pos);
            let hit = world().cast_capsule(spawn.pos, body(&c), Vec3::NEG_Y, 2.0).expect("no ground under spawn");
            assert!(hit.distance < 0.3, "{} spawn at {} floats {}", team.name, spawn.pos, hit.distance);
            assert!(hit.normal.y > c.max_slope_cos, "steep spawn at {}", spawn.pos);
        }
    }
}

#[test]
fn cover_heights_suit_each_stance() {
    let c = cfg();
    let s = BuildStyle::default();
    assert!(s.prone_cover > c.height(Stance::Prone) && s.prone_cover < c.eye(Stance::Crouch));
    assert!(s.crouch_cover > c.height(Stance::Crouch) && s.crouch_cover < c.eye(Stance::Stand));
    assert!(s.high_wall > c.height(Stance::Stand));
    assert!(s.parapet < c.eye(Stance::Crouch), "cannot fire over a parapet crouched");
    assert!(s.window_sill < c.eye(Stance::Crouch) && s.window_head > c.eye(Stance::Stand));
    assert!(s.stair_rise < c.step_height && s.plinth < c.step_height);
    assert!(s.door_head > c.stand_height && s.headroom > c.stand_height);
}

#[test]
fn ditches_hide_a_crouching_soldier_from_across_the_flat() {
    let c = cfg();
    let spec = MapSpec::town();
    let mut hidden = 0;
    let mut tested = 0;
    let relief = crate::relief::Relief::new(&spec);
    for ditch in spec.ditches.iter().filter(|d| d.depth < 2.0) {
        let (a, b) = (ditch.points[0], ditch.points[1]);
        let mid = a.lerp(b, 0.5);
        let across = (b - a).normalize().perp();
        for side in [1.0, -1.0] {
            tested += 1;
            let eye = Vec3::new(mid.x, relief.height(mid) + c.eye(Stance::Crouch), mid.y);
            let far = mid + across * side * 30.0;
            let to = Vec3::new(far.x, relief.height(far) + c.eye(Stance::Stand), far.y);
            let dir = to - eye;
            hidden += world().raycast(eye, dir.normalize(), dir.length()).is_some() as u32;
        }
    }
    assert!(hidden * 10 >= tested * 7, "only {hidden} of {tested} ditch sight lines are blocked");
}

#[test]
fn ground_stays_walkable_across_a_ditch() {
    let c = cfg();
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let ditch = &spec.ditches[0];
    let (a, b) = (ditch.points[0], ditch.points[1]);
    let mid = a.lerp(b, 0.5);
    let across = (b - a).normalize().perp();
    let (from, to) = (mid + across * 6.0, mid - across * 6.0);
    let lift = |p: Vec2| Vec3::new(p.x, relief.height(p) + 0.1, p.y);
    walk(lift(from), &[lift(to)], &c, world()).unwrap();
    walk(lift(to), &[lift(from)], &c, world()).unwrap();
}

/// The two bases never see each other, and neither sees the town centre from the spawn.
#[test]
fn ridges_break_the_long_sight_lines() {
    let c = cfg();
    let south = level().teams[0].spawns[0].pos + Vec3::Y * c.stand_eye;
    let north = level().teams[1].spawns[0].pos + Vec3::Y * c.stand_eye;
    let market = level().capture_points.iter().find(|p| p.name == "Market").unwrap().center + Vec3::Y * c.stand_eye;
    for (a, b) in [(south, north), (south, market), (north, market)] {
        let to = b - a;
        assert!(world().raycast(a, to.normalize(), to.length()).is_some(), "{a} can see {b}");
    }
}

/// Fields have to allow the long shot that makes crossing them a decision. The knoll over the
/// farmland should see far across it, and a soldier lying down in the wheat should be much harder
/// to see than one standing up.
#[test]
fn farmland_has_long_lanes_that_going_prone_takes_away() {
    let c = cfg();
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let from = Vec2::new(-185.0, -214.0);
    let eye = Vec3::new(from.x, relief.height(from) + c.stand_eye + 0.5, from.y);
    let (mut standing, mut prone, mut total) = (0, 0, 0);
    for x in (-400..-140).step_by(20) {
        for z in (-110..-10).step_by(20) {
            let p = Vec2::new(x as f32, z as f32);
            let ground = relief.height(p);
            total += 1;
            let clear = |height: f32| {
                let to = Vec3::new(p.x, ground + height, p.y) - eye;
                world().raycast(eye, to.normalize(), to.length()).is_none()
            };
            standing += clear(c.stand_eye) as u32;
            prone += clear(c.prone_eye) as u32;
        }
    }
    assert!(standing * 2 > total, "only {standing} of {total} of the fields are in sight");
    assert!(prone < standing, "{prone} prone, {standing} standing");
}

#[test]
fn rooms_count_as_indoors_to_the_ear() {
    // What the sound code asks: a roof within 12 m, and walls within 15 m on 3 of 8 bearings.
    let (roof_reach, wall_reach, walls_for_indoors) = (12.0, 15.0, 3);
    let mut outdoors = Vec::new();
    for room in &level().rooms {
        for local in [Vec3::new(0.5, 0.0, 0.5), Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 0.0, -0.5)] {
            let p = room.center + room.rotation * (local * room.half) ;
            let up = world().raycast(p, Vec3::Y, roof_reach).is_some();
            let walls = (0..8)
                .filter(|i| {
                    let a = *i as f32 * std::f32::consts::FRAC_PI_4;
                    world().raycast(p, Vec3::new(a.cos(), 0.0, a.sin()), wall_reach).is_some()
                })
                .count();
            if !up || walls < walls_for_indoors {
                outdoors.push(format!("{p}: roof {up}, walls {walls}"));
            }
        }
    }
    assert!(outdoors.is_empty(), "{} points outdoors:\n{}", outdoors.len(), outdoors.join("\n"));
}

#[test]
fn nobody_walks_off_the_map() {
    let c = cfg();
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let edge = spec.terrain.size * 0.5;
    for start in [Vec2::new(edge - 50.0, 80.0), Vec2::new(-edge + 50.0, -80.0), Vec2::new(60.0, edge - 50.0), Vec2::new(-60.0, -edge + 50.0)] {
        let mut state = MoveState::new(Vec3::new(start.x, relief.height(start) + 0.1, start.y), &c);
        let outward = if start.x.abs() > start.y.abs() { Vec2::new(start.x.signum(), 0.0) } else { Vec2::new(0.0, start.y.signum()) };
        let yaw = (-outward.x).atan2(-outward.y);
        let input = MoveInput { wish: Vec2::Y, yaw, ..MoveInput::default() };
        let mut events = Vec::new();
        for _ in 0..60 * 30 {
            step(&mut state, &c, &input, world(), DT, &mut events);
            events.clear();
        }
        assert!(state.pos.x.abs() < edge && state.pos.z.abs() < edge, "walked out to {}", state.pos);
        assert!(state.pos.y > -5.0, "fell through at {}", state.pos);
    }
}

/// Walkable grid search: every door, stair foot, spawn and capture point joins up on foot.
#[test]
fn the_whole_map_joins_up_on_foot() {
    let c = cfg();
    let w = world();
    let spec = MapSpec::town();
    let relief = crate::relief::Relief::new(&spec);
    let cell = 3.0_f32;
    let half = spec.terrain.size * 0.5 - cell;
    let n = (half * 2.0 / cell) as i32;
    let index = |p: Vec2| (((p.x + half) / cell).round() as i32, ((p.y + half) / cell).round() as i32);
    let point = |i: i32, j: i32| Vec2::new(i as f32 * cell - half, j as f32 * cell - half);
    let mut seen = vec![false; (n * n) as usize + 1];
    let start = index({ let s = level().teams[0].spawns[0].pos; Vec2::new(s.x, s.z) });
    let body = Capsule { height: 1.8, radius: c.radius };
    let stand = |p: Vec2| -> Option<f32> {
        let y = relief.height(p);
        let feet = Vec3::new(p.x, y + 0.45, p.y);
        if !w.capsule_is_clear(feet, body) {
            return None;
        }
        Some(y)
    };
    let mut stack = vec![start];
    seen[(start.1 * n + start.0) as usize] = true;
    while let Some((i, j)) = stack.pop() {
        let p = point(i, j);
        let Some(y) = stand(p) else { continue };
        for (a, b) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
            if a < 0 || b < 0 || a >= n || b >= n {
                continue;
            }
            let at = (b * n + a) as usize;
            if seen[at] {
                continue;
            }
            let q = point(a, b);
            let Some(ny) = stand(q) else { continue };
            // Too steep to walk: the slope limit from the movement rules.
            if (ny - y).abs() / cell > (1.0 - c.max_slope_cos * c.max_slope_cos).sqrt() / c.max_slope_cos {
                continue;
            }
            seen[at] = true;
            stack.push((a, b));
        }
    }
    let reach = |p: Vec2, radius: i32| {
        let (i, j) = index(p);
        (-radius..=radius).any(|a| (-radius..=radius).any(|b| {
            let (x, z) = (i + a, j + b);
            x >= 0 && z >= 0 && x < n && z < n && seen[(z * n + x) as usize]
        }))
    };
    let mut missing = Vec::new();
    for team in &level().teams {
        for s in &team.spawns {
            if !reach(Vec2::new(s.pos.x, s.pos.z), 2) {
                missing.push(format!("spawn {}", s.pos));
            }
        }
    }
    for point in &level().capture_points {
        if !reach(Vec2::new(point.center.x, point.center.z), 3) {
            missing.push(format!("capture point {}", point.name));
        }
    }
    let mut doors = 0;
    for door in level().doorways.iter().filter(|d| d.at.y < relief.height(Vec2::new(d.at.x, d.at.z)) + 0.5) {
        let out = door.at + door.outward * 3.0;
        doors += 1;
        if !reach(Vec2::new(out.x, out.z), 2) && !reach(Vec2::new(door.at.x - door.outward.x * 3.0, door.at.z - door.outward.z * 3.0), 2) {
            missing.push(format!("door {}", door.at));
        }
    }
    assert!(missing.is_empty(), "{} of {doors} doors etc:\n{}", missing.len(), missing.join("\n"));
}

/// Not a pass or fail: the numbers to weigh a change against.
#[test]
#[ignore]
fn query_report() {
    let l = level();
    let w = world();
    let c = cfg();
    let time = |name: &str, n: u32, f: &mut dyn FnMut(u32)| {
        let start = Instant::now();
        for i in 0..n {
            f(i);
        }
        println!("{name}: {:.2} us", start.elapsed().as_secs_f64() * 1e6 / n as f64);
    };
    println!(
        "solids {}, trees {}, rooms {}, doors {}, windows {}, stairs {}, terrain tris {}, verts {}",
        l.solids.len(), l.trees.len(), l.rooms.len(), l.doorways.len(), l.windows.len(), l.staircases.len(), l.terrain.indices.len(), l.terrain.vertices.len()
    );
    let start = Instant::now();
    let _ = MapSpec::town().build();
    println!("build map: {:?}", start.elapsed());
    let start = Instant::now();
    let _ = l.collision();
    println!("build collision: {:?}", start.elapsed());
    let town = |i: u32| {
        let f = i as f32;
        Vec3::new((f * 0.37).sin() * 60.0, 3.0, (f * 0.11).cos() * 60.0)
    };
    let mut hits = 0;
    time("bullet step raycast, 15 m", 5000, &mut |i| {
        let dir = Vec3::new((i as f32).sin(), -0.02, (i as f32).cos()).normalize();
        hits += w.raycast(town(i), dir, 15.0).is_some() as u32;
    });
    time("long raycast, 800 m", 2000, &mut |i| {
        let dir = Vec3::new((i as f32).sin(), 0.0, (i as f32).cos()).normalize();
        hits += w.raycast(town(i), dir, 800.0).is_some() as u32;
    });
    let body = Capsule { height: c.stand_height, radius: c.radius };
    time("capsule cast, 10 cm", 5000, &mut |i| {
        let dir = Vec3::new((i as f32).sin(), 0.0, (i as f32).cos()).normalize();
        hits += w.cast_capsule(town(i), body, dir, 0.1).is_some() as u32;
    });
    time("capsule cast down", 5000, &mut |i| {
        hits += w.cast_capsule(town(i), body, Vec3::NEG_Y, 0.1).is_some() as u32;
    });
    let mut state = MoveState::new(level().spawn, &c);
    let mut events = Vec::new();
    let input = MoveInput { wish: Vec2::Y, ..MoveInput::default() };
    time("whole movement step", 5000, &mut |_| {
        step(&mut state, &c, &input, w, DT, &mut events);
        events.clear();
    });
    println!("(hits {hits})");
}

/// Nothing is built through anything else: no two compounds, and no two buildings in one, overlap.
#[test]
fn nothing_is_built_inside_anything_else() {
    let spec = MapSpec::town();
    let corners = |at: Vec2, yaw: f32, half: Vec2| -> [Vec2; 4] {
        let axis = Vec2::from_angle(-yaw);
        [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)].map(|(a, b)| at + axis.rotate(half * Vec2::new(a, b)))
    };
    let separate = |a: &[Vec2; 4], b: &[Vec2; 4]| {
        let edges = [a[1] - a[0], a[2] - a[1], b[1] - b[0], b[2] - b[1]];
        edges.iter().any(|e| {
            let n = e.perp().normalize();
            let range = |p: &[Vec2; 4]| p.iter().fold((f32::MAX, f32::MIN), |r, c| (r.0.min(c.dot(n)), r.1.max(c.dot(n))));
            let (ra, rb) = (range(a), range(b));
            ra.1 < rb.0 || rb.1 < ra.0
        })
    };
    let mut clashes = Vec::new();
    let boxes: Vec<[Vec2; 4]> = spec.compounds.iter().map(|c| corners(c.at, c.yaw, c.half)).collect();
    for (i, a) in boxes.iter().enumerate() {
        for (j, b) in boxes.iter().enumerate().skip(i + 1) {
            if !separate(a, b) {
                clashes.push(format!("compounds at {} and {}", spec.compounds[i].at, spec.compounds[j].at));
            }
        }
    }
    for compound in &spec.compounds {
        let footprint = |b: &Building| {
            let stairs = if b.flights.iter().any(|f| f.from.is_none()) { spec.style.stair_width } else { 0.0 };
            corners(b.at + Vec2::new(stairs * 0.5, 0.0), b.yaw, b.half + Vec2::new(stairs * 0.5, 0.0))
        };
        for (i, a) in compound.buildings.iter().enumerate() {
            for b in compound.buildings.iter().skip(i + 1) {
                if !separate(&footprint(a), &footprint(b)) {
                    clashes.push(format!("buildings at {} and {} in the compound at {}", a.at, b.at, compound.at));
                }
            }
            let f = footprint(a);
            let inside = f.iter().all(|c| c.x.abs() < compound.half.x && c.y.abs() < compound.half.y);
            if !inside {
                clashes.push(format!("building at {} outside the compound at {}", a.at, compound.at));
            }
        }
    }
    assert!(clashes.is_empty(), "{}", clashes.join("\n"));
}

#[test]
fn every_helipad_is_flat_with_room_for_a_rotor() {
    for level in [MapSpec::town().build(), Level::training_range()] {
        let world = level.collision();
        assert!(!level.helipads.is_empty());
        for pad in &level.helipads {
            let height = |x: f32, z: f32| {
                let top = Vec3::new(pad.pos.x + x, pad.pos.y + 20.0, pad.pos.z + z);
                top.y - world.raycast(top, Vec3::NEG_Y, 40.0).expect("ground under a helipad").distance
            };
            for (x, z) in [(-1.3, -2.0), (1.3, -2.0), (-1.3, 1.7), (1.3, 1.7), (0.0, 0.0)] {
                assert!((height(x, z) - pad.pos.y).abs() < 0.35, "not flat at {} near {x} {z}: {}", pad.pos, height(x, z));
            }
            // Nothing stands up into the disc, or close enough around it to catch a blade or the tail.
            for i in 0..48 {
                let angle = i as f32 / 48.0 * std::f32::consts::TAU;
                let out = Vec3::new(angle.cos(), 0.0, angle.sin());
                for above in [2.5, 4.0, 5.5] {
                    let hub = pad.pos + Vec3::Y * above;
                    assert!(world.raycast(hub, out, 9.5).is_none(), "something {above} m up near the pad at {} in direction {out}", pad.pos);
                }
            }
        }
    }
}
