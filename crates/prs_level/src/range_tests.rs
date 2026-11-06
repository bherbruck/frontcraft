//! The range's kill house: you can get to it and through it, its targets are in its rooms, and from
//! some doorway you can see each one.

use crate::{Level, TargetKind};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld};
use prs_movement::{MoveConfig, MoveInput, MoveState, step};
use std::sync::OnceLock;

const DT: f32 = 1.0 / 60.0;

fn level() -> &'static Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    LEVEL.get_or_init(Level::training_range)
}

fn world() -> &'static CollisionWorld {
    static WORLD: OnceLock<CollisionWorld> = OnceLock::new();
    WORLD.get_or_init(|| level().collision())
}

fn near_house(p: Vec3) -> bool {
    Vec2::new(p.x, p.z).distance(crate::range::KILL_HOUSE_AT) < 20.0
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
            if flat.length() < 0.35 && to.y.abs() < 0.6 {
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

fn house_targets() -> Vec<(usize, Vec3)> {
    level().targets.iter().copied().enumerate().filter(|(i, _)| level().target_mark(*i).in_room).collect()
}

#[test]
fn the_kill_house_has_targets_of_both_kinds_on_both_floors() {
    let targets = house_targets();
    assert!(targets.len() >= 16, "{}", targets.len());
    let no_shoot = targets.iter().filter(|(i, _)| level().target_mark(*i).kind == TargetKind::NoShoot).count();
    assert!(no_shoot >= 4 && no_shoot * 3 < targets.len(), "{no_shoot} of {}", targets.len());
    let floor = targets.iter().map(|(_, p)| p.y).fold(f32::MAX, f32::min);
    assert!(targets.iter().any(|(_, p)| p.y > floor + 2.5), "nothing upstairs");
    // The lane targets stand as they always did.
    assert!(level().targets.iter().enumerate().filter(|(i, _)| !level().target_mark(*i).in_room).count() >= 19);
}

#[test]
fn every_kill_house_target_stands_inside_a_room_with_room_to_be_hit() {
    let body = Capsule { height: 1.7, radius: 0.25 };
    for (i, p) in house_targets() {
        let chest = p + Vec3::Y;
        assert!(level().rooms.iter().any(|room| room.contains(chest)), "target {i} at {p} is not in a room");
        assert!(world().capsule_is_clear(p + Vec3::Y * 0.05, body), "target {i} at {p} stands in something");
    }
}

/// From the eye of someone standing in some doorway of the house, each target can be seen.
#[test]
fn every_kill_house_target_can_be_seen_from_a_doorway() {
    let doors: Vec<_> = level().doorways.iter().filter(|d| near_house(d.at)).collect();
    assert!(doors.len() >= 15, "{}", doors.len());
    let eye = 1.62;
    for (i, p) in house_targets() {
        let chest = p + Vec3::Y * 1.2;
        let seen = doors.iter().any(|door| {
            [door.at + door.outward * 0.4, door.at - door.outward * 0.4].iter().any(|stand| {
                let from = *stand + Vec3::Y * eye;
                let to = chest - from;
                (to.y.abs() < 2.0) && world().raycast(from, to.normalize(), to.length()).is_none()
            })
        });
        assert!(seen, "target {i} at {p} cannot be seen from any doorway");
    }
}

/// From the firing line to the start line, in at the side door, down the hallway, up the stairs and
/// out of the east end. Every doorway goes both ways, too.
#[test]
fn the_kill_house_can_be_walked_into_up_and_through() {
    let house: Vec<_> = level().doorways.iter().filter(|d| near_house(d.at)).copied().collect();
    let mut failed = Vec::new();
    for (i, door) in house.iter().enumerate() {
        let (out, inside) = (door.at + door.outward * 1.2, door.at - door.outward * 1.2);
        for (from, to) in [(out, inside), (inside, out)] {
            if let Err(why) = walk(from + Vec3::Y * 0.3, &[to]) {
                failed.push(format!("door {i} at {}: {why}", door.at));
                break;
            }
        }
    }
    let stairs: Vec<_> = level().staircases.iter().filter(|s| near_house(s.path[0])).collect();
    assert_eq!(stairs.len(), 1);
    if let Err(why) = walk(stairs[0].path[0], &stairs[0].path[1..]) {
        failed.push(format!("the stairs: {why}"));
    }
    // And from where everyone starts, round to the front of the house.
    let front = house.iter().map(|d| d.at + d.outward * 6.0).min_by(|a, b| a.distance(level().spawn).total_cmp(&b.distance(level().spawn))).unwrap();
    if let Err(why) = walk(level().spawn, &[front]) {
        failed.push(format!("from the spawn: {why}"));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn the_window_wall_and_climbing_lane_have_their_openings() {
    let low = level().windows.iter().filter(|w| !near_house(w.at)).map(|w| w.at.y - w.height * 0.5).collect::<Vec<_>>();
    assert!(low.len() >= 4, "{low:?}");
    for w in level().windows.iter().filter(|w| !near_house(w.at)) {
        let from = w.at + w.outward;
        assert!(world().raycast(from, -w.outward, 2.0).is_none(), "the opening at {} is blocked", w.at);
    }
}
