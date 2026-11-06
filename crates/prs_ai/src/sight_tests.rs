//! What a bot can see, as tests: not through leaves, hardly a sliver of someone at range or through a window
//! from the back of a room, quickly someone firing in the open, and never someone lying still far off.
//! `cargo test -p prs_ai sight_report -- --ignored --nocapture` prints how long each takes.

use super::*;
use prs_sim::protocol::{Buttons, TICK_DT};
use prs_level::{Room, Tree, TreeKind};
use prs_sim::{Level, Paint, Solid};

/// What the one being looked for does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Doing {
    Still,
    Prone,
    Firing,
}

/// Seconds until a bot at `from`, facing `to`, has someone at `to` in sight, if it does within `limit`.
/// Also says whether what it aimed at could be seen from where it stood.
fn time_to_spot(level: Level, from: Vec3, to: Vec3, doing: Doing, limit: f32) -> (Option<f32>, bool) {
    let mut sim = Sim::new(level);
    let loadout = sim.inventory_cfg.loadout;
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let facing = |a: Vec3, b: Vec3| {
        let d = flat(b) - flat(a);
        (-d.x).atan2(-d.y)
    };
    let them = sim.spawn_soldier_with(5, 1, loadout);
    sim.set_home(them, to, facing(to, from));
    let bot = sim.spawn_soldier_with(6, 0, loadout);
    sim.set_home(bot, from, facing(from, to));
    let mut platoon = Platoon::new(&sim, FightConfig::default(), WalkerConfig::default());
    platoon.enlist(&sim, bot, 0, 0, true, 0);
    let areas = vec![WalkArea { center: flat(from), radius: 1.0, doors: Vec::new() }];
    let away = facing(to, from) + std::f32::consts::PI;
    for tick in 0..(limit / TICK_DT) as u32 {
        let mut buttons = Buttons::default();
        // Once on the ground, which takes a moment after being put down.
        buttons.set(Buttons::PRONE, doing == Doing::Prone && tick == 30);
        // Into the air and away, so nothing is hit but everyone can see the flash.
        buttons.set(Buttons::FIRE, doing == Doing::Firing && tick % 30 == 0);
        let yaw = if doing == Doing::Firing { away } else { facing(to, from) };
        let cmd = PlayerCmd { tick: sim.tick, yaw, pitch: if doing == Doing::Firing { 0.6 } else { 0.0 }, buttons, ..Default::default() };
        let mut cmds = platoon.think(&sim, &areas, None).to_vec();
        cmds.push((them, cmd));
        sim.step(&cmds);
        sim.soldiers[them].weapon.magazines = vec![30, 30];
        let fighter = &platoon.fighters[0];
        if let Some(contact) = fighter.contact.filter(|c| c.visible && c.soldier == them) {
            let eye = sim.soldiers[bot].eye();
            let path = contact.at - eye;
            let clear = sim.world.raycast(eye, path.normalize(), path.length() - 0.05).is_none();
            return (Some(tick as f32 * TICK_DT), clear);
        }
    }
    (None, false)
}

fn ground() -> Level {
    Level::flat(1600.0)
}

fn spot(z: f32) -> Vec3 {
    Vec3::new(0.0, 0.1, z)
}

/// Trees here keep their leaves above a standing soldier's head, so what they hide is seen from above: a
/// lookout on a rise, and between it and the one it looks for a broadleaf tree.
const LOOKOUT: f32 = 10.4;

fn under_a_tree() -> Level {
    let mut level = ground();
    level.trees.push(Tree { base: Vec3::new(0.0, 0.0, -50.0), height: 9.0, radius: 4.0, kind: TreeKind::Broadleaf });
    // Wide enough that a bot moving about on it stays up there.
    level.solids.push(Solid { center: Vec3::new(0.0, LOOKOUT * 0.5, 15.0), half: Vec3::new(15.0, LOOKOUT * 0.5, 15.0), rotation: glam::Quat::IDENTITY, paint: Paint::Rock });
    level
}

/// On top of the rise.
fn lookout() -> Vec3 {
    Vec3::new(0.0, LOOKOUT + 0.1, 0.0)
}

/// The same, with the tree gone, to show that it was the leaves.
fn bare_rise() -> Level {
    let mut level = under_a_tree();
    level.trees.clear();
    level
}

/// A wall just in front of them, high enough that only the head shows over it.
fn behind_a_wall(range: f32) -> Level {
    let mut level = ground();
    let top = 1.45;
    level.solids.push(Solid { center: Vec3::new(0.0, top * 0.5, -range + 1.5), half: Vec3::new(3.0, top * 0.5, 0.2), rotation: glam::Quat::IDENTITY, paint: Paint::Concrete });
    level
}

/// A window on the town with open ground in front of it at `range`, and the room behind it: where to stand
/// outside, the window, which way is out, and the room.
fn a_window(range: f32) -> (Level, Vec3, Vec3, Vec3, Room) {
    let level = Level::named("town").unwrap();
    let world = level.collision();
    let mut found = None;
    for window in &level.windows {
        let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize_or_zero();
        let Some(room) = level.rooms.iter().copied().find(|room| room.contains(window.at - out * 2.5)) else { continue };
        let outside = window.at + out * range;
        let Some(hit) = world.raycast(outside + Vec3::Y * 60.0, Vec3::NEG_Y, 120.0) else { continue };
        let feet = outside + Vec3::Y * (60.0 - hit.distance + 0.05);
        let eye = feet + Vec3::Y * 1.62;
        let path = window.at - eye;
        let open = world.raycast(eye, path.normalize(), path.length() - 0.3).is_none();
        // A window on the ground floor, looked at from level with it.
        if open && window.at.y - feet.y < 2.5 {
            found = Some((feet, window.at, out, room));
            break;
        }
    }
    if let Some((feet, at, out, room)) = found {
        return (level, feet, at, out, room);
    }
    panic!("no window on the town looks out over open ground at {range} m");
}

/// Standing in the room behind a window, this far back from it.
fn in_the_room(level: &Level, window: Vec3, out: Vec3, back: f32, aside: f32) -> Vec3 {
    let right = Vec3::new(-out.z, 0.0, out.x);
    let at = window - out * back + right * aside;
    let world = level.collision();
    let hit = world.raycast(at + Vec3::Y * 0.5, Vec3::NEG_Y, 4.0).expect("a floor under the window");
    at + Vec3::Y * (0.5 - hit.distance + 0.05)
}

#[test]
fn nobody_is_seen_through_a_canopy_at_80_m() {
    let (seen, _) = time_to_spot(under_a_tree(), lookout(), spot(-80.0), Doing::Still, 30.0);
    assert_eq!(seen, None, "seen through the leaves");
    let (bare, _) = time_to_spot(bare_rise(), lookout(), spot(-80.0), Doing::Still, 30.0);
    assert!(bare.is_some(), "and without the tree, seen at once");
}

#[test]
fn a_head_over_a_wall_at_150_m_takes_a_long_time_to_make_out() {
    let (seen, _) = time_to_spot(behind_a_wall(150.0), spot(0.0), spot(-150.0), Doing::Still, 15.0);
    assert_eq!(seen, None, "a sliver spotted in under 15 s");
}

#[test]
fn someone_firing_in_the_open_at_150_m_is_seen_quickly() {
    let (seen, _) = time_to_spot(ground(), spot(0.0), spot(-150.0), Doing::Firing, 30.0);
    assert!(seen.is_some_and(|t| t < 4.0), "{seen:?}");
}

#[test]
fn someone_lying_still_at_200_m_is_not_seen() {
    let (seen, _) = time_to_spot(ground(), spot(0.0), spot(-200.0), Doing::Prone, 60.0);
    assert_eq!(seen, None);
}

#[test]
fn back_from_a_window_is_hard_to_see_and_at_it_or_firing_is_easier_and_aimed_at_through_the_opening() {
    let (level, outside, window, out, _) = a_window(100.0);
    let back = in_the_room(&level, window, out, 2.0, 0.0);
    let (hidden, _) = time_to_spot(level.clone(), outside, back, Doing::Still, 12.0);
    assert_eq!(hidden, None, "two metres back in a room, seen from 100 m in under 12 s");
    let at = in_the_room(&level, window, out, 0.4, 0.0);
    let (seen, clear) = time_to_spot(level.clone(), outside, at, Doing::Still, 40.0);
    assert!(seen.is_some(), "standing in the window, never seen");
    assert!(clear, "aimed at a part of them behind the wall");
    let (firing, _) = time_to_spot(level, outside, back, Doing::Firing, 40.0);
    assert!(firing.is_some_and(|t| t < 12.0), "firing from the back of the room: {firing:?}");
}

#[test]
fn deep_in_a_room_away_from_the_window_nobody_outside_sees_you() {
    let (level, outside, _, out, room) = a_window(100.0);
    let deep = in_the_room(&level, room.center - out * (room.half.min_element() * 0.6), out, 0.0, 0.0);
    let (seen, _) = time_to_spot(level, outside, deep, Doing::Still, 60.0);
    assert_eq!(seen, None);
}

#[test]
#[ignore]
fn sight_report() {
    let show = |what: &str, (seen, clear): (Option<f32>, bool)| match seen {
        Some(t) => println!("  {what:<44} seen after {t:>5.1} s{}", if clear { "" } else { "  (aimed through cover)" }),
        None => println!("  {what:<44} not seen"),
    };
    show("standing in the open, 80 m", time_to_spot(ground(), spot(0.0), spot(-80.0), Doing::Still, 60.0));
    show("from a rise, through a canopy, 80 m", time_to_spot(under_a_tree(), lookout(), spot(-80.0), Doing::Still, 60.0));
    show("from a rise, no tree, 80 m", time_to_spot(bare_rise(), lookout(), spot(-80.0), Doing::Still, 60.0));
    show("standing in the open, 150 m", time_to_spot(ground(), spot(0.0), spot(-150.0), Doing::Still, 60.0));
    show("head over a wall, 150 m", time_to_spot(behind_a_wall(150.0), spot(0.0), spot(-150.0), Doing::Still, 120.0));
    show("head over a wall, 60 m", time_to_spot(behind_a_wall(60.0), spot(0.0), spot(-60.0), Doing::Still, 120.0));
    show("firing in the open, 150 m", time_to_spot(ground(), spot(0.0), spot(-150.0), Doing::Firing, 60.0));
    show("lying still in the open, 200 m", time_to_spot(ground(), spot(0.0), spot(-200.0), Doing::Prone, 120.0));
    show("lying still in the open, 80 m", time_to_spot(ground(), spot(0.0), spot(-80.0), Doing::Prone, 120.0));
    for range in [50.0, 100.0, 150.0] {
        let (level, outside, window, out, room) = a_window(range);
        for (what, back, doing) in [("2 m back from a window", 2.0, Doing::Still), ("in a window", 0.4, Doing::Still), ("firing 2 m back from a window", 2.0, Doing::Firing)] {
            let at = in_the_room(&level, window, out, back, 0.0);
            show(&format!("{what}, {range:.0} m"), time_to_spot(level.clone(), outside, at, doing, 60.0));
        }
        let deep = room.center - out * (room.half.min_element() * 0.6);
        let deep = in_the_room(&level, deep + out * 0.0, out, 0.0, 0.0);
        show(&format!("deep in the room, {range:.0} m"), time_to_spot(level, outside, deep, Doing::Still, 60.0));
    }
}


