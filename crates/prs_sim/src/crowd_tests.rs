//! Soldiers bumping into each other, standing on each other, and climbing with a gun.

use super::*;
use glam::Quat;
use prs_movement::ClimbKind;

fn cmd(tick: u32, yaw: f32, wish: Vec2, buttons: &[Buttons]) -> PlayerCmd {
    let mut held = Buttons::default();
    for &b in buttons {
        held.set(b, true);
    }
    PlayerCmd { tick, wish_move: wish, yaw, buttons: held, ..Default::default() }
}

/// Two soldiers on flat ground, the second just ahead of the first looking the same way.
fn two() -> (Sim, SoldierId, SoldierId) {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    let b = sim.spawn_soldier(2);
    sim.set_home(a, Vec3::new(0.0, 0.05, 0.0), 0.0);
    sim.set_home(b, Vec3::new(0.0, 0.05, -0.8), 0.0);
    settle(&mut sim);
    (sim, a, b)
}

fn settle(sim: &mut Sim) {
    for _ in 0..30 {
        sim.step(&[]);
    }
}

fn run(sim: &mut Sim, seconds: f32, cmds: impl Fn(u32) -> Vec<(SoldierId, PlayerCmd)>) {
    for _ in 0..(seconds * TICK_HZ as f32) as u32 {
        let tick = sim.tick + 1;
        sim.step(&cmds(tick));
    }
}

/// One crouches and stays that way, leaving the key up again.
fn crouch(sim: &mut Sim, who: SoldierId) {
    let yaw = sim.soldiers[who].yaw;
    let tick = sim.tick + 1;
    sim.step(&[(who, cmd(tick, yaw, Vec2::ZERO, &[Buttons::CROUCH]))]);
    run(sim, 0.6, |_| Vec::new());
}

/// Walking forward with jump down.
fn climb_on(sim: &mut Sim, who: SoldierId) {
    let yaw = sim.soldiers[who].yaw;
    let tick = sim.tick + 1;
    sim.step(&[(who, cmd(tick, yaw, Vec2::Y, &[Buttons::JUMP]))]);
    while sim.soldiers[who].body.climb.active() {
        let tick = sim.tick + 1;
        sim.step(&[(who, cmd(tick, yaw, Vec2::Y, &[]))]);
    }
}

fn mounted() -> (Sim, SoldierId, SoldierId) {
    let (mut sim, a, b) = two();
    crouch(&mut sim, b);
    climb_on(&mut sim, a);
    assert_eq!(sim.soldiers[a].body.perch.on, Some(b as u32));
    (sim, a, b)
}

/// What has to be true however someone comes off a back: down on the ground, with nothing in it that
/// could be called a launch.
fn comes_down_gently(sim: &mut Sim, who: SoldierId, stool: SoldierId, stool_cmds: impl Fn(u32) -> Option<PlayerCmd>) {
    let shrug = sim.move_cfg.stool.shrug_speed + 0.01;
    let top = sim.soldiers[who].body.pos.y;
    let mut gone = false;
    for _ in 0..120 {
        let tick = sim.tick + 1;
        let cmds: Vec<_> = stool_cmds(tick).map(|c| (stool, c)).into_iter().collect();
        sim.step(&cmds);
        let body = &sim.soldiers[who].body;
        assert!(body.pos.y <= top + 1e-3, "went up to {}", body.pos);
        assert!(body.vel.y <= 1e-3, "launched: {}", body.vel);
        assert!(body.horizontal_speed() <= shrug, "thrown: {}", body.vel);
        gone |= body.perch.on.is_none();
    }
    assert!(gone, "still up there");
    let body = &sim.soldiers[who].body;
    assert!(body.grounded && body.pos.y < 0.1, "at {}", body.pos);
    assert_eq!(sim.soldiers[who].health.health, sim.health_cfg.max_health);
}

#[test]
fn standing_on_a_crouched_back() {
    let (sim, a, b) = mounted();
    let (rider, stool) = (&sim.soldiers[a], &sim.soldiers[b]);
    assert!((rider.body.pos.y - stool.body.pos.y - sim.move_cfg.stool.back_height).abs() < 0.05);
    assert!(rider.body.grounded);
    assert!(sim.is_carrying(b));
}

#[test]
fn a_stool_standing_up_shrugs_the_rider_off() {
    let (mut sim, a, b) = mounted();
    let yaw = sim.soldiers[b].yaw;
    let stand = cmd(sim.tick + 1, yaw, Vec2::ZERO, &[Buttons::CROUCH]);
    comes_down_gently(&mut sim, a, b, |_| Some(stand));
    assert_eq!(sim.soldiers[b].body.stance, Stance::Stand);
}

#[test]
fn a_stool_going_prone_shrugs_the_rider_off() {
    let (mut sim, a, b) = mounted();
    let yaw = sim.soldiers[b].yaw;
    let lie = cmd(sim.tick + 1, yaw, Vec2::ZERO, &[Buttons::PRONE]);
    comes_down_gently(&mut sim, a, b, |_| Some(lie));
}

#[test]
fn a_stool_going_down_shrugs_the_rider_off() {
    let (mut sim, a, b) = mounted();
    sim.hurt(b, Zone::Chest, 500.0);
    comes_down_gently(&mut sim, a, b, |_| None);
}

#[test]
fn a_rider_going_down_comes_off_with_them() {
    let (mut sim, a, _) = mounted();
    sim.hurt(a, Zone::Chest, 120.0);
    run(&mut sim, 2.0, |_| Vec::new());
    let body = &sim.soldiers[a].body;
    assert_eq!(body.perch.on, None);
    assert!(body.grounded && body.pos.y < 0.1, "{}", body.pos);
}

/// Shuffling is all they can do, and past a step or two the rider is left behind. Nobody gets carried.
#[test]
fn a_stool_cannot_carry_anyone_and_cannot_hurry() {
    let (mut sim, a, b) = mounted();
    let yaw = sim.soldiers[b].yaw;
    let spot = sim.soldiers[a].body.pos;
    let mut carried = 0.0f32;
    let mut fastest = 0.0f32;
    for _ in 0..240 {
        let tick = sim.tick + 1;
        sim.step(&[(b, cmd(tick, yaw, Vec2::Y, &[]))]);
        if sim.soldiers[a].body.perch.on.is_some() {
            carried = carried.max((sim.soldiers[a].body.pos - spot).length());
            fastest = fastest.max(sim.soldiers[b].body.horizontal_speed());
        }
    }
    assert!(sim.soldiers[a].body.perch.on.is_none(), "still riding");
    assert!(carried < 1e-3, "rode {carried} m");
    assert!(fastest <= sim.move_cfg.stool.carry_speed + 0.01, "{fastest}");
}

/// Reading the same table for what cannot be stood on: it is all the same refusal.
#[test]
fn only_a_crouched_back_will_do() {
    // Standing.
    let (mut sim, a, _) = two();
    climb_on(&mut sim, a);
    assert!(sim.soldiers[a].body.perch.on.is_none(), "stood on someone standing");

    // Lying.
    let (mut sim, a, b) = two();
    let tick = sim.tick + 1;
    sim.step(&[(b, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::PRONE]))]);
    run(&mut sim, 1.5, |_| Vec::new());
    climb_on(&mut sim, a);
    assert!(sim.soldiers[a].body.perch.on.is_none(), "stood on someone lying down");

    // Down, and dead, crouched as they were.
    for raw in [120.0, 900.0] {
        let (mut sim, a, b) = two();
        crouch(&mut sim, b);
        sim.hurt(b, Zone::Chest, raw);
        run(&mut sim, 2.0, |_| Vec::new());
        assert!(!sim.soldiers[b].health.is_up());
        climb_on(&mut sim, a);
        assert!(sim.soldiers[a].body.perch.on.is_none(), "stood on a body");
    }

    // Halfway down into a crouch is not there yet.
    let (mut sim, a, b) = two();
    let tick = sim.tick + 1;
    sim.step(&[(b, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::CROUCH]))]);
    climb_on(&mut sim, a);
    assert!(sim.soldiers[a].body.perch.on.is_none(), "stood on someone still going down");
}

/// Nobody stands on someone who is standing on someone, even if they crouch up there to help.
#[test]
fn no_towers() {
    let (mut sim, a, b) = mounted();
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::CROUCH]))]);
    run(&mut sim, 0.6, |_| Vec::new());
    assert_eq!(sim.soldiers[a].body.stance, Stance::Crouch);
    let c = sim.spawn_soldier(3);
    sim.set_home(c, sim.soldiers[a].body.pos * Vec3::new(1.0, 0.0, 1.0) + Vec3::new(0.0, 0.05, 0.75), 0.0);
    settle(&mut sim);
    assert_eq!(sim.soldiers[a].body.perch.on, Some(b as u32));
    climb_on(&mut sim, c);
    assert!(sim.soldiers[c].body.perch.on.is_none(), "a tower");
    assert!(sim.soldiers[c].body.pos.y < 0.5 || sim.soldiers[c].body.perch.on != Some(a as u32));
}

/// And a back that already has someone on it stays with them.
#[test]
fn one_rider_at_a_time() {
    let (mut sim, a, b) = mounted();
    let c = sim.spawn_soldier(3);
    sim.set_home(c, Vec3::new(0.0, 0.05, -1.6), std::f32::consts::PI);
    settle(&mut sim);
    climb_on(&mut sim, c);
    assert_eq!(sim.soldiers[a].body.perch.on, Some(b as u32));
    assert!(sim.soldiers[c].body.perch.on.is_none());
}

/// The point of it all. Over the limit alone, and in reach with a back to stand on, enemy or friend.
#[test]
fn two_soldiers_reach_what_one_cannot() {
    let (mut sim, a, b) = two();
    sim.soldiers[b].team = 1;
    crouch(&mut sim, b);
    let height = sim.move_cfg.climb.max_height + 0.6;
    let (x, half) = (Vec3::new(0.0, height * 0.5, -3.1), Vec3::new(8.0, height * 0.5, 2.0));
    sim.world.add_box(x, half, Quat::IDENTITY, Surface::Concrete);
    assert_eq!(sim.climb_ahead(a, 0.0), Some(ClimbKind::Stool));
    climb_on(&mut sim, a);
    assert_eq!(sim.soldiers[a].body.perch.on, Some(b as u32));

    assert_eq!(sim.climb_ahead(a, 0.0), Some(ClimbKind::PullUp), "from the back");

    // Straight off one climb and into the next is not on, so a moment's breath first.
    let at = sim.soldiers[a].body.pos;
    climb_on(&mut sim, a);
    assert_eq!(sim.soldiers[a].body.pos.y, at.y, "no chaining");
    run(&mut sim, 0.8, |_| Vec::new());
    climb_on(&mut sim, a);
    let rider = &sim.soldiers[a].body;
    assert!((rider.pos.y - height).abs() < 0.05, "{}", rider.pos);
    assert!(rider.perch.on.is_none());
    assert!(!sim.is_carrying(b) || { sim.step(&[]); !sim.is_carrying(b) });
}

#[test]
fn a_climb_with_a_gun_in_hand_brings_it_down_and_it_will_not_fire() {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
    sim.world.add_box(Vec3::new(0.0, 0.7, -3.0), Vec3::new(8.0, 0.7, 2.0), Quat::IDENTITY, Surface::Concrete);
    settle(&mut sim);
    let ammo = sim.soldiers[a].weapon.ammo;
    let ready_pose = sim.soldiers[a].gun_pose(&sim.weapons);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
    assert!(sim.soldiers[a].body.climb.active());
    let mut lowest = 1.0f32;
    while sim.soldiers[a].body.climb.active() {
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::FIRE, Buttons::ADS]))]);
        let soldier = &sim.soldiers[a];
        lowest = lowest.min(soldier.weapon.ready);
        assert_eq!(soldier.weapon.ads, 0.0);
    }
    assert_eq!(sim.soldiers[a].weapon.ammo, ammo, "fired on the way up");
    assert!(lowest < 0.3, "the gun only came down to {lowest}");
    let up_there = sim.soldiers[a].gun_pose(&sim.weapons);
    assert_ne!(up_there.muzzle, ready_pose.muzzle);
}

#[test]
fn reloading_keeps_hands_off_a_ledge() {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
    sim.world.add_box(Vec3::new(0.0, 0.7, -3.0), Vec3::new(8.0, 0.7, 2.0), Quat::IDENTITY, Surface::Concrete);
    settle(&mut sim);
    sim.soldiers[a].weapon.ammo -= 5;
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::RELOAD]))]);
    assert!(sim.soldiers[a].weapon.reload_timer > 0.0);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
    assert!(!sim.soldiers[a].body.climb.active());
}

#[test]
fn going_down_on_the_way_up_lets_go_and_nobody_stays_hanging() {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
    sim.world.add_box(Vec3::new(0.0, 1.0, -3.0), Vec3::new(8.0, 1.0, 2.0), Quat::IDENTITY, Surface::Concrete);
    settle(&mut sim);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
    run(&mut sim, 0.4, |tick| vec![(0, cmd(tick, 0.0, Vec2::Y, &[]))]);
    assert!(sim.soldiers[a].body.climb.active());
    sim.hurt(a, Zone::Chest, 120.0);
    run(&mut sim, 2.0, |_| Vec::new());
    let body = &sim.soldiers[a].body;
    assert!(!body.climb.active());
    assert!(body.grounded && body.pos.y < 0.1, "{}", body.pos);
}

#[test]
fn a_leg_wound_and_a_machine_gun_are_felt_on_a_ledge() {
    let setup = |height: f32, loadout: Option<prs_inventory::Loadout>| {
        let mut sim = Sim::new(Level::flat(200.0));
        let kit = loadout.unwrap_or(sim.inventory_cfg.loadout);
        let a = sim.spawn_soldier_with(1, 0, kit);
        sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
        sim.world.add_box(Vec3::new(0.0, height * 0.5, -3.0), Vec3::new(8.0, height * 0.5, 2.0), Quat::IDENTITY, Surface::Concrete);
        settle(&mut sim);
        (sim, a)
    };
    let high = 2.1;
    let (sim, a) = setup(high, None);
    assert_eq!(sim.climb_ahead(a, 0.0), Some(ClimbKind::PullUp));
    let (mut sim, a) = setup(high, None);
    sim.hurt(a, Zone::Leg, 60.0);
    assert_eq!(sim.climb_ahead(a, 0.0), None, "leg wound {}", sim.soldiers[a].health.leg_wound);

    let gunner = prs_inventory::Loadout { primary: WeaponTable::MACHINE_GUN, ..InventoryConfig::default().loadout };
    let (sim, a) = setup(high, Some(gunner));
    assert_eq!(sim.climb_ahead(a, 0.0), None, "burden {}", sim.burden_of(&sim.soldiers[a]));
    let (sim, a) = setup(1.2, Some(gunner));
    assert_eq!(sim.climb_ahead(a, 0.0), Some(ClimbKind::Mantle));
}

/// A bandage in the hands is hands full, and they stay off a ledge until it is done with.
#[test]
fn treating_keeps_hands_off_a_ledge() {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
    sim.world.add_box(Vec3::new(0.0, 0.7, -3.0), Vec3::new(8.0, 0.7, 2.0), Quat::IDENTITY, Surface::Concrete);
    settle(&mut sim);
    sim.hurt(a, Zone::Leg, 10.0);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::SLOT_3]))]);
    // The gun has to be down first.
    run(&mut sim, 1.0, |_| Vec::new());
    run(&mut sim, 0.2, |tick| vec![(0, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::FIRE]))]);
    assert!(sim.soldiers[a].treatment.is_some(), "should be bandaging");
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::FIRE, Buttons::JUMP]))]);
    assert!(!sim.soldiers[a].body.climb.active(), "climbed with a bandage on the go");
}

/// Nobody climbs out of a seat: the jump key there is not theirs to use for it.
#[test]
fn no_climbing_from_a_seat() {
    let mut sim = Sim::new(Level::named("range").unwrap());
    let a = sim.spawn_soldier(1);
    sim.sit_in(a, prs_seats::SeatedAt { carrier: 0, seat: 1 });
    run(&mut sim, 0.2, |_| Vec::new());
    // Right up against something that could be climbed, as far as a body on foot goes.
    let at = sim.soldiers[a].body.pos;
    let yaw = sim.soldiers[a].yaw;
    let forward = yaw_basis(yaw).0;
    sim.world.add_box(at + forward * 2.0, Vec3::new(1.0, 0.7, 1.0), Quat::IDENTITY, Surface::Concrete);
    assert_eq!(sim.climb_ahead(a, yaw), None);
    for _ in 0..30 {
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, yaw, Vec2::Y, &[Buttons::JUMP]))]);
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, yaw, Vec2::Y, &[]))]);
        assert!(!sim.soldiers[a].body.climb.active(), "climbed out of a seat");
    }
    assert!(sim.soldiers[a].seat.is_some(), "and still sitting");
}

/// Coming from the same spot they spread apart, slowly, and nothing about it gets either moving.
#[test]
fn two_in_one_spot_ease_apart_with_no_launch() {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    let b = sim.spawn_soldier(2);
    for who in [a, b] {
        sim.set_home(who, Vec3::new(0.0, 0.05, 0.0), 0.0);
    }
    for _ in 0..240 {
        sim.step(&[]);
        for who in [a, b] {
            let body = &sim.soldiers[who].body;
            assert!(body.horizontal_speed() < 1e-3, "launched: {}", body.vel);
            assert!(body.pos.y < 0.2, "{}", body.pos);
        }
    }
    let apart = sim.soldiers[a].body.pos.distance(sim.soldiers[b].body.pos);
    assert!(apart > sim.move_cfg.crowd.contact - 0.05, "{apart}");
}

/// Someone walking into someone stops short of walking through them, and someone can always squeeze by.
#[test]
fn walking_into_someone_slows_you_right_down() {
    let (mut sim, a, b) = two();
    sim.set_home(a, Vec3::new(0.0, 0.05, 4.0), 0.0);
    settle(&mut sim);
    let mut fastest_in_contact = 0.0f32;
    for _ in 0..240 {
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::SPRINT]))]);
        let (me, them) = (&sim.soldiers[a].body, &sim.soldiers[b].body);
        let depth = sim.move_cfg.crowd.contact - Vec2::new(me.pos.x - them.pos.x, me.pos.z - them.pos.z).length();
        if depth > sim.move_cfg.crowd.brake_depth {
            fastest_in_contact = fastest_in_contact.max(me.horizontal_speed());
        }
    }
    assert!(fastest_in_contact <= sim.move_cfg.crowd.squeeze_friend + 0.15, "{fastest_in_contact}");
}

/// A level with one door in a wall, at z = 0 and 1.2 wide, with the wall running off to either side.
fn door() -> (Sim, SoldierId, SoldierId) {
    let mut sim = Sim::new(Level::flat(200.0));
    let a = sim.spawn_soldier(1);
    let b = sim.spawn_soldier(2);
    for side in [-1.0, 1.0] {
        sim.world.add_box(Vec3::new(side * 8.6, 1.5, 0.0), Vec3::new(8.0, 1.5, 0.15), Quat::IDENTITY, Surface::Concrete);
    }
    sim.world.add_box(Vec3::new(0.0, 2.6, 0.0), Vec3::new(0.7, 0.4, 0.15), Quat::IDENTITY, Surface::Concrete);
    (sim, a, b)
}

/// Walking through someone standing in a door, or leaning right back against you the whole way. Slowly,
/// and for friends and enemies alike, because a door that one soldier can shut is griefing.
#[test]
fn nobody_can_plug_a_door() {
    for (team, resists) in [(0, false), (0, true), (1, false), (1, true)] {
        let (mut sim, a, b) = door();
        sim.soldiers[b].team = team;
        sim.set_home(a, Vec3::new(0.0, 0.05, 2.0), 0.0);
        sim.set_home(b, Vec3::new(0.0, 0.05, 0.0), std::f32::consts::PI);
        settle(&mut sim);
        let mut seconds = 0.0;
        while sim.soldiers[a].body.pos.z > -0.8 {
            let tick = sim.tick + 1;
            let mut cmds = vec![(a, cmd(tick, 0.0, Vec2::Y, &[]))];
            if resists {
                cmds.push((b, cmd(tick, std::f32::consts::PI, Vec2::Y, &[])));
            }
            sim.step(&cmds);
            seconds += TICK_DT;
            assert!(seconds < 20.0, "team {team} resisting {resists} held the door against them at {}", sim.soldiers[a].body.pos);
        }
        // Past them or not, it took some effort.
        assert!(seconds > 1.0 || !resists, "team {team}: {seconds}");
    }
}

/// Two walking into each other down a corridor, as a bot might, do not hang there.
#[test]
fn two_meeting_in_a_door_both_get_through() {
    let (mut sim, a, b) = door();
    sim.set_home(a, Vec3::new(0.0, 0.05, 3.0), 0.0);
    sim.set_home(b, Vec3::new(0.0, 0.05, -3.0), std::f32::consts::PI);
    settle(&mut sim);
    for _ in 0..(20.0 * TICK_HZ as f32) as u32 {
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[])), (b, cmd(tick, std::f32::consts::PI, Vec2::Y, &[]))]);
    }
    assert!(sim.soldiers[a].body.pos.z < -5.0 && sim.soldiers[b].body.pos.z > 5.0, "{} and {}", sim.soldiers[a].body.pos, sim.soldiers[b].body.pos);
}

#[test]
fn someone_on_another_floor_is_not_in_the_way() {
    let (mut sim, a, b) = two();
    sim.soldiers[b].body.pos = sim.soldiers[a].body.pos + Vec3::new(0.05, 3.0, 0.0);
    sim.world.add_box(sim.soldiers[a].body.pos + Vec3::new(0.0, 2.9, 0.0), Vec3::new(4.0, 0.05, 4.0), Quat::IDENTITY, Surface::Wood);
    let (ax, bx) = (sim.soldiers[a].body.pos, sim.soldiers[b].body.pos.x);
    run(&mut sim, 1.0, |_| Vec::new());
    assert_eq!(sim.soldiers[a].body.pos.x, ax.x);
    assert_eq!(sim.soldiers[b].body.pos.x, bx);
}

#[test]
fn someone_lying_down_can_be_stepped_over() {
    let (mut sim, a, b) = two();
    let tick = sim.tick + 1;
    sim.step(&[(b, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::PRONE]))]);
    run(&mut sim, 1.5, |_| Vec::new());
    let start = sim.soldiers[a].body.pos;
    run(&mut sim, 1.0, |tick| vec![(0, cmd(tick, 0.0, Vec2::Y, &[]))]);
    assert!((sim.soldiers[a].body.pos - start).length() > 2.0, "{}", sim.soldiers[a].body.pos);
}

/// A climb and a perch go along with everything else in the state, and step on identically from there.
#[test]
fn restoring_in_the_middle_of_a_climb_and_on_a_back_carries_on_the_same() {
    let (mut sim, a, b) = two();
    crouch(&mut sim, b);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
    run(&mut sim, 0.2, |tick| vec![(0, cmd(tick, 0.0, Vec2::Y, &[]))]);
    assert!(sim.soldiers[a].body.climb.active());
    let saved = sim.export_state();
    let mut other = Sim::new(Level::flat(200.0));
    other.import_state(&saved);
    for _ in 0..120 {
        let tick = sim.tick + 1;
        let wish = if sim.soldiers[a].body.climb.active() { Vec2::Y } else { Vec2::ZERO };
        let cmds = [(a, cmd(tick, 0.0, wish, &[]))];
        sim.step(&cmds);
        other.step(&cmds);
        assert_eq!(sim.soldiers[a].body, other.soldiers[a].body);
        assert_eq!(sim.soldiers[b].body, other.soldiers[b].body);
    }
    assert_eq!(sim.soldiers[a].body.perch.on, Some(b as u32));
}

/// A truck turning up lets a climb go, with the soldier falling from where they were, unhurt by climbing.
#[test]
fn something_driven_into_a_climb_ends_it() {
    let mut sim = Sim::new(Level::named("range").unwrap());
    let a = sim.spawn_soldier(1);
    let from = sim.vehicles[0].pos + Vec3::new(0.0, 0.0, 12.0);
    let top = Vec3::new(from.x, 40.0, from.z);
    let ground = sim.world.raycast(top, Vec3::NEG_Y, 60.0).map_or(0.0, |hit| 40.0 - hit.distance);
    sim.set_home(a, Vec3::new(from.x, ground + 0.05, from.z), 0.0);
    settle(&mut sim);
    let foot = sim.soldiers[a].body.pos;
    sim.world.add_box(Vec3::new(foot.x, foot.y + 0.6, foot.z - 2.6), Vec3::new(3.0, 0.6, 2.0), Quat::IDENTITY, Surface::Concrete);
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
    assert!(sim.soldiers[a].body.climb.active());
    run(&mut sim, 0.2, |tick| vec![(0, cmd(tick, 0.0, Vec2::Y, &[]))]);
    sim.vehicles[0].pos = sim.soldiers[a].body.pos + Vec3::Y * 0.4;
    sim.vehicles[0].asleep = false;
    let tick = sim.tick + 1;
    sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[]))]);
    assert!(!sim.soldiers[a].body.climb.active());
}

/// What it costs when everyone is in a heap, and when they are spread across a town.
#[test]
#[ignore]
fn crowd_cost_report() {
    use std::time::Instant;
    for (name, spread) in [("heaped", 4.0f32), ("spread", 200.0)] {
        for count in [32usize, 64] {
            let mut sim = Sim::new(Level::flat(500.0));
            for i in 0..count {
                let id = sim.spawn_soldier(i as u32);
                let angle = i as f32 * 2.4;
                let radius = spread * (i as f32 / count as f32).sqrt();
                sim.set_home(id, Vec3::new(angle.cos() * radius, 0.05, angle.sin() * radius), 0.0);
            }
            for _ in 0..30 {
                sim.step(&[]);
            }
            let walk: Vec<_> = (0..count).map(|id| (id, cmd(0, id as f32, Vec2::Y, &[]))).collect();
            let start = Instant::now();
            let ticks = 600;
            for _ in 0..ticks {
                sim.step(&walk);
            }
            let whole = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
            let start = Instant::now();
            for _ in 0..ticks {
                sim.take_down_crowd(0);
                for id in 0..count {
                    std::hint::black_box(sim.crowding_of(id));
                }
            }
            let crowd = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
            println!("{count} soldiers {name}: {whole:.3} ms a tick all in, of that the crowd {crowd:.4} ms");
        }
    }
}

/// What is offered to be shown is exactly what a press of jump with W held does, whatever the reason it
/// would or would not: in reach or not, too high, cooling down, lying down, reloading.
#[test]
fn what_is_offered_is_what_a_jump_does() {
    type Setup = fn(&mut Sim, SoldierId);
    let cases: &[(&str, f32, Setup)] = &[
        ("waist high", 0.9, |_, _| {}),
        ("chest high", 1.4, |_, _| {}),
        ("at the limit", 2.1, |_, _| {}),
        ("over the limit", 2.6, |_, _| {}),
        ("nothing there", 0.0, |_, _| {}),
        ("cooling down", 1.0, |sim, id| sim.soldiers[id].body.climb_cooldown = 0.3),
        ("lying down", 1.0, |sim, id| {
            let tick = sim.tick + 1;
            sim.step(&[(id, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::PRONE]))]);
            run(sim, 1.5, |_| Vec::new());
        }),
        ("reloading", 1.0, |sim, id| {
            sim.soldiers[id].weapon.ammo -= 5;
            let tick = sim.tick + 1;
            sim.step(&[(id, cmd(tick, 0.0, Vec2::ZERO, &[Buttons::RELOAD]))]);
        }),
    ];
    for (name, height, setup) in cases {
        let mut sim = Sim::new(Level::flat(200.0));
        let a = sim.spawn_soldier(1);
        sim.set_home(a, Vec3::new(0.0, 0.05, -0.6), 0.0);
        if *height > 0.0 {
            sim.world.add_box(Vec3::new(0.0, height * 0.5, -3.0), Vec3::new(8.0, height * 0.5, 2.0), Quat::IDENTITY, Surface::Concrete);
        }
        settle(&mut sim);
        setup(&mut sim, a);
        let offered = sim.climb_on_offer(a);
        let tick = sim.tick + 1;
        sim.step(&[(a, cmd(tick, 0.0, Vec2::Y, &[Buttons::JUMP]))]);
        let climbed = sim.soldiers[a].body.climb.active();
        assert_eq!(offered.is_some(), climbed, "{name}: offered {offered:?}, climbed {climbed}");
        if let Some(offered) = offered {
            assert_eq!(offered.to, sim.soldiers[a].body.climb.to, "{name}");
        }
    }
}
