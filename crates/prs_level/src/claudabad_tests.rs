//! Holds Claudabad to what a map promises: you can get from either base to every flag on foot, over the
//! bridges and up out of the canal, and nothing is built in anything else.

use crate::{Level, MapSpec};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld};
use prs_movement::{MoveConfig, MoveInput, MoveState, step};
use std::sync::OnceLock;

const DT: f32 = 1.0 / 60.0;

fn spec() -> &'static MapSpec {
    static SPEC: OnceLock<MapSpec> = OnceLock::new();
    SPEC.get_or_init(MapSpec::claudabad)
}

fn level() -> &'static Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    LEVEL.get_or_init(|| spec().build())
}

fn world() -> &'static CollisionWorld {
    static WORLD: OnceLock<CollisionWorld> = OnceLock::new();
    WORLD.get_or_init(|| level().collision())
}

fn body() -> Capsule {
    let c = MoveConfig::default();
    Capsule { height: c.stand_height, radius: c.radius }
}

/// The ground under a point, from the land and anything standing on it.
fn ground(p: Vec2) -> Vec3 {
    let top = Vec3::new(p.x, 200.0, p.y);
    top - Vec3::Y * world().raycast(top, Vec3::NEG_Y, 400.0).expect("ground everywhere").distance
}

/// Walks a soldier towards each point in turn, or says where they got stuck.
fn walk(start: Vec3, path: &[Vec3]) -> Result<MoveState, String> {
    let c = MoveConfig::default();
    let mut state = MoveState::new(start + Vec3::Y * 0.05, &c);
    let mut events = Vec::new();
    for target in path {
        let mut ticks = 0;
        loop {
            let to = *target - state.pos;
            let flat = Vec2::new(to.x, to.z);
            if flat.length() < 0.4 && to.y.abs() < 0.8 {
                break;
            }
            ticks += 1;
            if ticks > 60 * 40 {
                return Err(format!("stuck at {} on the way to {target}", state.pos));
            }
            let input = MoveInput { wish: Vec2::Y, yaw: (-flat.x).atan2(-flat.y), ..MoveInput::default() };
            events.clear();
            step(&mut state, &c, &input, world(), DT, &mut events);
        }
    }
    Ok(state)
}

#[test]
fn every_spawn_is_clear_and_on_the_ground() {
    assert_eq!(level().teams.len(), 2);
    for team in &level().teams {
        for spawn in &team.spawns {
            assert!(world().capsule_is_clear(spawn.pos, body()), "{} spawn at {} is blocked", team.name, spawn.pos);
            let hit = world().cast_capsule(spawn.pos, body(), Vec3::NEG_Y, 2.0).expect("no ground under a spawn");
            assert!(hit.distance < 0.3, "{} spawn at {} floats {}", team.name, spawn.pos, hit.distance);
        }
    }
}

/// A flag stands in the open, where it can be seen and fought round.
#[test]
fn every_capture_point_stands_on_open_ground() {
    assert_eq!(level().capture_points.len(), 7);
    for point in &level().capture_points {
        let feet = point.center + Vec3::Y * 0.1;
        assert!(world().capsule_is_clear(feet, body()), "{} is inside something", point.name);
        assert!(world().raycast(feet + Vec3::Y * 0.2, Vec3::Y, 60.0).is_none(), "{} has something over it", point.name);
    }
}

/// The two sides' halves are the same distances turned round: each flag's twin is where it should be.
#[test]
fn each_sides_flags_mirror_the_others() {
    let points = &spec().capture_points;
    for (a, b) in [(0, 6), (1, 5), (2, 4)] {
        let (p, q) = (points[a].at, points[b].at);
        assert!((p + q).length() < 1.0, "{} at {p} and {} at {q} are not turned round the middle", points[a].name, points[b].name);
    }
    let bases: Vec<Vec2> = level().teams.iter().map(|t| t.base_center()).map(|c| Vec2::new(c.x, c.z)).collect();
    assert!((bases[0] + bases[1]).length() < 1.0, "{bases:?}");
}

/// Every bridge can be walked over end to end, and the parapets keep you on it.
#[test]
fn every_bridge_can_be_walked_across() {
    assert_eq!(spec().bridges.len(), 10);
    for bridge in &spec().bridges {
        let dir = (bridge.to - bridge.from).normalize();
        let (from, to) = (bridge.from - dir * 4.0, bridge.to + dir * 4.0);
        for (a, b) in [(from, to), (to, from)] {
            let state = walk(ground(a), &[ground(b)]).unwrap_or_else(|why| panic!("bridge at {}: {why}", bridge.from));
            assert!(state.pos.y > ground(b).y - 0.5, "fell off the bridge at {} to {}", bridge.from, state.pos);
        }
    }
}

/// Fallen in, there is always a way out: down the bed to the steps by a bridge and up them. Without them
/// the canal is too deep to climb out of.
#[test]
fn the_canal_is_left_by_the_steps_and_not_up_its_sides() {
    // Nowhere along the canal is far from a way out.
    for canal in spec().ditches.iter().filter(|d| d.cuts_roads) {
        for pair in canal.points.windows(2) {
            for k in 0..=10 {
                let p = pair[0].lerp(pair[1], k as f32 / 10.0);
                let nearest = spec().steps.iter().map(|s| s.to.distance(p)).fold(f32::MAX, f32::min);
                assert!(nearest < 95.0 || p.abs().max_element() > spec().terrain.size * 0.5 - 100.0, "{nearest:.0} m from {p} to a way out of the canal");
            }
        }
    }
    for steps in &spec().steps {
        let dir = (steps.from - steps.to).normalize();
        let below = ground(steps.to + dir * -1.5);
        walk(below, &[ground(steps.to), ground(steps.from), ground(steps.from + dir * 2.0)]).unwrap_or_else(|why| panic!("steps at {}: {why}", steps.from));
    }
    // Straight at the bank anywhere else, a soldier stays in.
    let c = MoveConfig::default();
    let bed = ground(Vec2::new(-200.0, -4.0));
    let mut state = MoveState::new(bed + Vec3::Y * 0.05, &c);
    let mut events = Vec::new();
    for _ in 0..60 * 6 {
        step(&mut state, &c, &MoveInput { wish: Vec2::Y, yaw: std::f32::consts::PI, ..MoveInput::default() }, world(), DT, &mut events);
        events.clear();
    }
    assert!(state.pos.y < bed.y + 1.0, "walked out of the canal up its side, to {}", state.pos);
}

/// Every doorway and staircase in a sample of the compounds works, as the town's all do.
#[test]
fn a_sample_of_doors_and_stairs_works() {
    let mut failed = Vec::new();
    for (i, door) in level().doorways.iter().enumerate().step_by(7) {
        let (out, inside) = (door.at + door.outward * 1.2, door.at - door.outward * 1.2);
        if let Err(why) = walk(out + Vec3::Y * 0.3, &[inside]).and_then(|_| walk(inside + Vec3::Y * 0.3, &[out])) {
            failed.push(format!("door {i} at {}: {why}", door.at));
        }
    }
    for (i, stairs) in level().staircases.iter().enumerate().step_by(3) {
        if let Err(why) = walk(stairs.path[0], &stairs.path[1..]) {
            failed.push(format!("stairs {i} at {}: {why}", stairs.path[0]));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// No compound is built in another, on a road or in the canal.
#[test]
fn nothing_is_built_inside_anything_else() {
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
    let near = |p: Vec2, points: &[Vec2]| {
        points.windows(2).map(|w| {
            let t = ((p - w[0]).dot(w[1] - w[0]) / (w[1] - w[0]).length_squared()).clamp(0.0, 1.0);
            (w[0] + (w[1] - w[0]) * t).distance(p)
        }).fold(f32::MAX, f32::min)
    };
    let spec = spec();
    let boxes: Vec<[Vec2; 4]> = spec.compounds.iter().map(|c| corners(c.at, c.yaw, c.half)).collect();
    let mut clashes = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        for (j, b) in boxes.iter().enumerate().skip(i + 1) {
            if !separate(a, b) {
                clashes.push(format!("compounds at {} and {}", spec.compounds[i].at, spec.compounds[j].at));
            }
        }
        for canal in spec.ditches.iter().filter(|d| d.cuts_roads) {
            if a.iter().any(|&c| near(c, &canal.points) < canal.width * 0.5 + 1.0) {
                clashes.push(format!("the compound at {} stands in the canal", spec.compounds[i].at));
            }
        }
        for road in &spec.roads {
            let inside = |p: Vec2| { let l = Vec2::from_angle(-spec.compounds[i].yaw).rotate(p - spec.compounds[i].at); l.abs().cmple(spec.compounds[i].half).all() };
            if road.points.windows(2).any(|w| (0..=10).any(|k| inside(w[0].lerp(w[1], k as f32 / 10.0)))) {
                clashes.push(format!("a road runs through the compound at {}", spec.compounds[i].at));
            }
        }
    }
    assert!(clashes.is_empty(), "{}", clashes.join("\n"));
}

/// From each team's spawn to every flag and to the other base's gate, on foot, by a grid search over
/// what is clear to stand in and not too steep. The canal is only crossed by its bridges.
#[test]
fn the_whole_map_joins_up_on_foot() {
    let c = MoveConfig::default();
    let w = world();
    let cell = 5.0_f32;
    let half = spec().terrain.size * 0.5 - cell;
    let n = (half * 2.0 / cell) as i32;
    let index = |p: Vec2| (((p.x + half) / cell).round() as i32, ((p.y + half) / cell).round() as i32);
    let point = |i: i32, j: i32| Vec2::new(i as f32 * cell - half, j as f32 * cell - half);
    let stand = |p: Vec2| -> Option<f32> {
        let g = ground(p);
        w.capsule_is_clear(g + Vec3::Y * 0.45, body()).then_some(g.y)
    };
    let steep = (1.0 - c.max_slope_cos * c.max_slope_cos).sqrt() / c.max_slope_cos;
    for team in &level().teams {
        let mut seen = vec![false; (n * n) as usize];
        let start = index(Vec2::new(team.spawns[0].pos.x, team.spawns[0].pos.z));
        let mut stack = vec![start];
        seen[(start.1 * n + start.0) as usize] = true;
        while let Some((i, j)) = stack.pop() {
            let Some(y) = stand(point(i, j)) else { continue };
            for (a, b) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if a < 0 || b < 0 || a >= n || b >= n || seen[(b * n + a) as usize] {
                    continue;
                }
                let Some(ny) = stand(point(a, b)) else { continue };
                if (ny - y).abs() / cell > steep.max(c.step_height / cell) {
                    continue;
                }
                seen[(b * n + a) as usize] = true;
                stack.push((a, b));
            }
        }
        let reach = |p: Vec2| {
            let (i, j) = index(p);
            (-3..=3).any(|a| (-3..=3).any(|b| { let (x, z) = (i + a, j + b); x >= 0 && z >= 0 && x < n && z < n && seen[(z * n + x) as usize] }))
        };
        for point in &level().capture_points {
            assert!(reach(Vec2::new(point.center.x, point.center.z)), "{} cannot get to {}", team.name, point.name);
        }
        for other in &level().teams {
            let s = other.spawns[0].pos;
            assert!(reach(Vec2::new(s.x, s.z)), "{} cannot get to {}'s base", team.name, other.name);
        }
    }
}

/// It fits what a server may send a client with plenty to spare: a quarter of the build budget, and the
/// file's own limits.
#[test]
fn it_fits_the_map_limits_with_room_to_spare() {
    let limits = crate::maps::MapLimits::default();
    assert!(spec().build_within(limits.max_build_steps / 4).is_ok());
    let data = crate::MapData::built_in("claudabad").expect("built in");
    assert!(data.level_within(&limits).is_ok());
}

/// Not a pass or fail: how big it is next to the town.
#[test]
#[ignore]
fn size_report() {
    let town = MapSpec::town().build();
    for (name, l) in [("town", &town), ("claudabad", level())] {
        println!(
            "{name}: solids {}, trees {}, rooms {}, doors {}, windows {}, stairs {}, terrain tris {}",
            l.solids.len(), l.trees.len(), l.rooms.len(), l.doorways.len(), l.windows.len(), l.staircases.len(), l.terrain.indices.len()
        );
    }
}
