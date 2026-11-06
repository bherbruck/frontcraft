//! Riding in things, as whole-game scenes: who can do what from where, and that a seat never gets in the
//! way of a gun being a real thing in the real world.

use super::*;
use prs_seats::SeatRole;
use prs_vehicles::{VehicleConfig, VehicleKind, VehicleZone};
use std::f32::consts::{FRAC_PI_2, PI};

const TRUCK: VehicleKind = VehicleTable::UTILITY_TRUCK;
/// Long enough to get through any door or across to any seat.
const TRANSIT_TICKS: u32 = 150;

/// Flat open ground with one truck parked at the origin looking down -Z, settled on its springs.
fn yard() -> Sim {
    yard_with(|_| {})
}

fn yard_with(extra: impl Fn(&mut Level)) -> Sim {
    let mut level = Level::flat(1000.0);
    level.vehicles.push(prs_level::VehicleSpawn { kind: 0, pos: Vec3::ZERO, yaw: 0.0 });
    extra(&mut level);
    let mut sim = Sim::new(level);
    for _ in 0..90 {
        sim.step(&[]);
    }
    assert!(sim.vehicles[0].asleep, "the truck should have settled");
    sim
}

/// A soldier on foot at the door of a seat, facing its handle.
fn beside_seat(sim: &mut Sim, seed: u32, seat: usize) -> SoldierId {
    beside_seat_of(sim, seed, 0, seat)
}

fn beside_seat_of(sim: &mut Sim, seed: u32, carrier: u32, seat: usize) -> SoldierId {
    let id = sim.spawn_soldier(seed);
    let frame = sim.carrier_frame(carrier);
    let entry = sim.carrier_entries(carrier).iter().find(|entry| entry.seats.contains(&(seat as u8))).unwrap().clone();
    let stand = frame.point(entry.stand);
    let top = Vec3::new(stand.x, stand.y + 3.0, stand.z);
    let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 8.0).map_or(3.0, |hit| hit.distance);
    let spot = Vec3::new(stand.x, ground + 0.05, stand.z);
    let to_hand = frame.point(entry.hand) - (spot + Vec3::Y * sim.move_cfg.stand_eye);
    sim.set_home(id, spot, (-to_hand.x).atan2(-to_hand.z));
    sim.soldiers[id].pitch = (to_hand.y / to_hand.length()).asin();
    id
}

fn cmd(sim: &Sim, wish: Vec2, yaw: f32, pitch: f32, buttons: Buttons) -> PlayerCmd {
    PlayerCmd { tick: sim.tick, wish_move: wish, yaw, pitch, buttons, ..Default::default() }
}

/// Presses a button for a tick and lets it go again.
fn press(sim: &mut Sim, id: SoldierId, button: Buttons) -> Vec<SimEvent> {
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    let mut b = Buttons::default();
    b.set(button, true);
    let down = cmd(sim, Vec2::ZERO, yaw, pitch, b);
    sim.step(&[(id, down)]);
    let events = sim.events.clone();
    let up = cmd(sim, Vec2::ZERO, yaw, pitch, Buttons::default());
    sim.step(&[(id, up)]);
    events
}

fn seated(sim: &mut Sim, seed: u32, seat: usize) -> SoldierId {
    let id = beside_seat(sim, seed, seat);
    press(sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].seat, Some(SeatedAt { carrier: 0, seat: seat as u8 }), "should have got in");
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    hold(sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].transit, None, "should be in the seat by now");
    id
}

/// Holds the gun on a yaw and pitch for this many ticks, collecting what happened.
fn hold(sim: &mut Sim, id: SoldierId, wish: Vec2, yaw: f32, pitch: f32, buttons: Buttons, ticks: u32) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let c = cmd(sim, wish, yaw, pitch, buttons);
        sim.step(&[(id, c)]);
        events.extend_from_slice(&sim.events);
    }
    events
}

fn buttons(list: &[Buttons]) -> Buttons {
    let mut b = Buttons::default();
    for &button in list {
        b.set(button, true);
    }
    b
}

fn shots(events: &[SimEvent]) -> Vec<(Vec3, Vec3)> {
    events.iter().filter_map(|e| if let SimEvent::ShotFired { origin, dir, .. } = *e { Some((origin, dir)) } else { None }).collect()
}

#[test]
fn f_at_a_door_takes_time_and_gets_you_to_the_seat_it_serves() {
    let mut sim = yard();
    let id = beside_seat(&mut sim, 1, 1);
    let start = sim.soldiers[id].eye();
    let events = press(&mut sim, id, Buttons::USE);
    assert!(events.iter().any(|e| matches!(e, SimEvent::Seat { event: SeatEvent::Entering { seat: 1, .. }, .. })), "{events:?}");
    assert_eq!(sim.soldiers[id].seat, Some(SeatedAt { carrier: 0, seat: 1 }));
    assert_eq!(sim.vehicles[0].occupants.0[1], Some(id as u32), "the seat is theirs from the start");
    assert!(sim.soldiers[id].transit.is_some());
    assert!(sim.soldiers[id].eye().distance(start) < 0.1, "they start from where they stood");
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    // Partway through they are somewhere in the door, with the gun going down.
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), 25);
    let eye = sim.soldiers[id].eye();
    let seat_eye = sim.carrier_frame(0).point(sim.carrier_seats(0)[1].eye);
    assert!(eye.distance(seat_eye) > 0.15 && eye.distance(start) > 0.15, "{eye} between {start} and {seat_eye}");
    let events = hold(&mut sim, id, Vec2::ZERO, yaw, pitch, buttons(&[Buttons::FIRE]), TRANSIT_TICKS);
    assert!(shots(&events).is_empty(), "nobody shoots climbing in");
    assert!(events.iter().any(|e| matches!(e, SimEvent::Seat { event: SeatEvent::Entered { seat: 1, .. }, .. })), "{events:?}");
    assert_eq!(sim.soldiers[id].transit, None);
    assert!(sim.soldiers[id].eye().distance(seat_eye) < 1e-3);
}

#[test]
fn a_door_only_opens_for_someone_at_it_and_facing_it() {
    let mut sim = yard();
    let far = sim.spawn_soldier(1);
    sim.set_home(far, Vec3::new(9.0, 0.05, 0.0), FRAC_PI_2);
    press(&mut sim, far, Buttons::USE);
    assert_eq!(sim.soldiers[far].seat, None, "from across the yard");
    let away = beside_seat(&mut sim, 2, 1);
    sim.soldiers[away].yaw += PI;
    press(&mut sim, away, Buttons::USE);
    assert_eq!(sim.soldiers[away].seat, None, "with their back to it");
    // On the left, with the driver's seat taken and the door they are at serving nobody else.
    let taken = seated(&mut sim, 3, 0);
    let late = beside_seat(&mut sim, 4, 0);
    press(&mut sim, late, Buttons::USE);
    assert_eq!(sim.soldiers[late].seat, None, "that door's seat has someone in it");
    assert_eq!(sim.vehicles[0].occupants.0[0], Some(taken as u32));
}

#[test]
fn a_door_against_a_wall_does_not_open_and_you_go_round_to_the_other_or_shuffle() {
    let wall = |level: &mut Level| {
        level.solids.push(prs_level::Solid { center: Vec3::new(1.25, 1.5, 0.0), half: Vec3::new(0.15, 1.5, 8.0), rotation: Quat::IDENTITY, paint: prs_level::Paint::Concrete });
    };
    let mut sim = yard_with(wall);
    // The other side of the wall from the door, as near as anyone can get to it.
    let id = sim.spawn_soldier(1);
    sim.set_home(id, Vec3::new(1.9, 0.05, -0.25), FRAC_PI_2);
    press(&mut sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].seat, None);
    // So in by the driver's door, and across.
    let id = seated(&mut sim, 2, 0);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    press(&mut sim, id, Buttons::SLOT_2);
    assert_eq!(sim.soldiers[id].seat.unwrap().seat, 1);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].transit, None);
    // And no way out of it on that side: it says so and nobody moves.
    let events = press(&mut sim, id, Buttons::USE);
    assert!(sim.soldiers[id].seat.is_some());
    assert_eq!(sim.soldiers[id].transit, None);
    assert!(events.iter().any(|e| matches!(e, SimEvent::Seat { event: SeatEvent::ExitBlocked { .. }, .. })), "{events:?}");
}

#[test]
fn you_can_crawl_out_of_a_truck_on_its_side() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    sim.vehicles[0].rot = Quat::from_rotation_z(FRAC_PI_2);
    sim.vehicles[0].pos.y = 1.0;
    for _ in 0..60 {
        let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
    }
    press(&mut sim, id, Buttons::USE);
    assert!(sim.soldiers[id].transit.is_some());
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].seat, None);
    assert!(sim.world.capsule_is_clear(sim.soldiers[id].body.pos, sim.move_cfg.capsule(Stance::Stand)));
}

/// A truck nose down in the fields' real ditch still lets people out, on the slope of the bank.
#[test]
fn you_can_get_out_of_a_truck_in_a_ditch() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let id = sim.spawn_soldier(1);
    let cfg = sim.vehicle_cfgs[TRUCK].clone();
    let top = Vec3::new(110.0, 200.0, 170.0);
    let ground = 200.0 - sim.world.raycast(top, Vec3::NEG_Y, 400.0).unwrap().distance;
    sim.vehicles[0] = VehicleState::new(TRUCK, &cfg, prs_seats::Frame { pos: Vec3::new(110.0, ground + cfg.ride_height(), 170.0), rot: Quat::from_rotation_y(PI) });
    sim.sit_in(id, SeatedAt { carrier: 0, seat: 0 });
    for _ in 0..1200 {
        let c = cmd(&sim, Vec2::new(0.0, if sim.vehicles[0].vel.length() < 3.0 { 1.0 } else { 0.0 }), sim.soldiers[id].yaw, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
    }
    assert!(sim.vehicles[0].pos.z > 180.0 && sim.vehicles[0].pos.z < 196.0, "should be in the ditch: {}", sim.vehicles[0].pos);
    let events = press(&mut sim, id, Buttons::USE);
    assert!(sim.soldiers[id].transit.is_some(), "the door should open: {events:?}");
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].seat, None);
}

#[test]
fn someone_stood_on_a_hull_goes_where_it_goes_and_steps_off_it() {
    let mut sim = armour();
    let driver = sim.spawn_soldier(1);
    sim.sit_in(driver, SeatedAt { carrier: 0, seat: 0 });
    let rider = sim.spawn_soldier(2);
    sim.soldiers[rider].body.pos = sim.carrier_frame(0).point(Vec3::new(0.0, 1.03, 1.6));
    let idle = |sim: &Sim| cmd(sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    for _ in 0..30 {
        let (d, r) = (idle(&sim), idle(&sim));
        sim.step(&[(driver, d), (rider, r)]);
    }
    assert!(sim.soldiers[rider].deck.is_some(), "{}", sim.soldiers[rider].body.pos);
    for _ in 0..300 {
        let d = cmd(&sim, Vec2::new(0.0, 0.5), 0.0, 0.0, Buttons::default());
        let r = idle(&sim);
        sim.step(&[(driver, d), (rider, r)]);
    }
    assert!(sim.vehicles[0].vel.length() > 2.0);
    let local = sim.carrier_frame(0).to_local(sim.soldiers[rider].body.pos);
    assert!(local.y > 0.9 && local.y < 1.2 && local.z > 1.0 && local.z < 2.2, "{local}");
}

#[test]
fn a_door_opens_while_someone_goes_through_and_shuts_after() {
    let mut sim = yard();
    let id = beside_seat(&mut sim, 1, 1);
    assert_eq!(sim.vehicle_doors[0][1], 0.0);
    press(&mut sim, id, Buttons::USE);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), 30);
    assert_eq!(sim.vehicle_doors[0][1], 1.0, "open by halfway");
    assert_eq!(sim.vehicle_doors[0][0], 0.0, "and only that one");
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.vehicle_doors[0][1], 0.0, "shut behind them");
}

/// By the time the body is at the door there is no panel where it is.
#[test]
fn going_through_a_door_the_body_never_shares_a_place_with_it() {
    let mut sim = yard();
    let id = beside_seat(&mut sim, 1, 1);
    press(&mut sim, id, Buttons::USE);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    let cfg = sim.vehicle_cfgs[TRUCK].clone();
    let door = cfg.entries[1].door.unwrap();
    for tick in 0..70 {
        hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), 1);
        if sim.soldiers[id].transit.is_none() {
            break;
        }
        let pose = sim.body_pose(&sim.body_input(id));
        let frame = sim.carrier_frame(0);
        let (center, rot) = cfg.panel_at(door.panel as usize, &sim.vehicle_doors[0]);
        let half = cfg.panels[door.panel as usize].half;
        for bone in [prs_body::Bone::ThighR, prs_body::Bone::ShinR, prs_body::Bone::FootR, prs_body::Bone::ThighL, prs_body::Bone::ShinL, prs_body::Bone::FootL, prs_body::Bone::Pelvis, prs_body::Bone::Chest] {
            let end = frame.to_local(pose[bone].end());
            let inside = (rot.conjugate() * (end - center)).abs().cmplt(half).all();
            // While it swings it passes them, which takes as long as it takes to open.
            assert!(!inside || tick < 18, "{bone:?} inside the door at tick {tick}: {end}");
        }
    }
}

#[test]
fn a_door_with_no_room_to_swing_does_not_open() {
    // A wall too far off to stop anyone standing at the door, and in the way of it swinging.
    let mut sim = yard_with(|level| {
        level.solids.push(prs_level::Solid { center: Vec3::new(1.87, 1.5, -0.45), half: Vec3::new(0.05, 1.5, 0.2), rotation: Quat::IDENTITY, paint: prs_level::Paint::Concrete });
    });
    let id = beside_seat(&mut sim, 1, 1);
    press(&mut sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].seat, None);
}

#[test]
fn someone_in_a_doorway_can_be_shot_and_goes_back_the_way_they_came() {
    let mut sim = yard();
    let shooter = sim.spawn_soldier(9);
    sim.set_home(shooter, Vec3::new(30.0, 0.05, 30.0), 0.0);
    let id = beside_seat(&mut sim, 1, 1);
    press(&mut sim, id, Buttons::USE);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), 15);
    assert!(sim.soldiers[id].transit.is_some());
    let at = chest(&sim, id);
    round_from(&mut sim, shooter, at, at + Vec3::new(20.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::SoldierHit { soldier, .. } if *soldier == id)), "{events:?}");
    if sim.soldiers[id].health.is_up() {
        hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    }
    assert_eq!(sim.soldiers[id].seat, None, "they should be back out");
    assert_eq!(sim.vehicles[0].occupants.0[1], None);
    assert!(sim.soldiers[id].body.pos.x > 1.2, "on their feet outside: {}", sim.soldiers[id].body.pos);
}

#[test]
fn nobody_climbs_into_something_moving() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    hold(&mut sim, driver, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default(), 120);
    assert!(sim.vehicles[0].vel.length() > 3.0);
    let late = beside_seat(&mut sim, 2, 1);
    let (yaw, pitch) = (sim.soldiers[late].yaw, sim.soldiers[late].pitch);
    let c1 = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
    let c2 = cmd(&sim, Vec2::ZERO, yaw, pitch, buttons(&[Buttons::USE]));
    sim.step(&[(driver, c1), (late, c2)]);
    assert_eq!(sim.soldiers[late].seat, None);
}

#[test]
fn f_does_whichever_of_a_door_and_a_kit_you_are_facing() {
    let mut sim = yard();
    let id = beside_seat(&mut sim, 1, 1);
    let spot = sim.soldiers[id].body.pos;
    // A kit right beside them, on the side away from the truck.
    let dead = sim.spawn_soldier(2);
    sim.set_home(dead, spot + Vec3::new(0.6, 0.0, 0.0), 0.0);
    sim.hurt(dead, Zone::Head, 500.0);
    assert!(sim.kits.len() == 1 && sim.kit_in_reach(id).is_some());
    assert!(matches!(sim.use_target(id), Some(UseTarget::Door { seat: 1, .. })), "{:?}", sim.use_target(id));
    let to_kit = sim.kits[0].pos - sim.soldiers[id].eye();
    sim.soldiers[id].yaw = (-to_kit.x).atan2(-to_kit.z);
    sim.soldiers[id].pitch = (to_kit.y / to_kit.length()).asin();
    assert!(matches!(sim.use_target(id), Some(UseTarget::Kit(0))), "{:?}", sim.use_target(id));
}

#[test]
fn q_and_e_do_nothing_in_a_seat() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 3);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, buttons(&[Buttons::LEAN_RIGHT]), 40);
    assert_eq!(sim.soldiers[id].body.lean.amount, 0.0);
    assert!(sim.soldiers[id].seat.is_some());
}

#[test]
fn a_seat_takes_one_at_a_time_and_a_full_truck_takes_no_more() {
    let mut sim = yard();
    for seat in 0..4 {
        seated(&mut sim, seat as u32, seat);
    }
    let late = beside_seat(&mut sim, 9, 1);
    press(&mut sim, late, Buttons::USE);
    assert_eq!(sim.soldiers[late].seat, None);
}

#[test]
fn the_number_keys_only_go_to_free_seats_connected_to_yours() {
    let mut sim = yard();
    let a = seated(&mut sim, 1, 0);
    let b = seated(&mut sim, 2, 2);
    let (yaw, pitch) = (sim.soldiers[a].yaw, sim.soldiers[a].pitch);
    press(&mut sim, a, Buttons::SLOT_4);
    assert_eq!(sim.soldiers[a].seat.unwrap().seat, 0, "the cab does not connect to the back");
    press(&mut sim, a, Buttons::SLOT_2);
    assert_eq!(sim.soldiers[a].seat.unwrap().seat, 1, "the cab does");
    assert!(sim.soldiers[a].transit.is_some(), "and it takes a moment");
    assert_eq!(sim.vehicles[0].occupants.0, vec![None, Some(a as u32), Some(b as u32), None]);
    hold(&mut sim, a, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[a].transit, None);
    let seat_eye = sim.carrier_frame(0).point(sim.carrier_seats(0)[1].eye);
    assert!(sim.soldiers[a].eye().distance(seat_eye) < 1e-3);
    // And it did not change what they hold.
    assert_eq!(sim.soldiers[a].inventory.selected, Slot::Primary);
}

#[test]
fn the_driver_drives_and_cannot_shoot_and_is_carried_with_their_eye_in_the_seat() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    assert_eq!(sim.carrier_seats(0)[0].role, SeatRole::Driver);
    let events = hold(&mut sim, id, Vec2::new(0.0, 1.0), 0.0, 0.0, buttons(&[Buttons::FIRE]), 180);
    assert!(shots(&events).is_empty(), "a driver has no hands for a gun");
    assert!(sim.vehicles[0].pos.z < -5.0, "should have driven off forwards: {}", sim.vehicles[0].pos);
    assert!(sim.soldiers[id].weapon.ready < 0.05, "the gun is away");
    let eye = sim.carrier_frame(0).point(sim.carrier_seats(0)[0].eye);
    assert!(sim.soldiers[id].eye().distance(eye) < 1e-4, "{} {}", sim.soldiers[id].eye(), eye);
    assert!(sim.gun_put_away(id));
}

#[test]
fn a_passenger_shoots_out_of_their_side_from_the_real_muzzle_and_misses_their_own_truck() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 3);
    // Right side, square off the truck.
    let (yaw, pitch) = (-FRAC_PI_2, 0.02);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, buttons(&[Buttons::ADS]), 80);
    let mut muzzles = Vec::new();
    let mut fired = Vec::new();
    let mut hit_self = false;
    for tick in 0..300 {
        let mut b = buttons(&[Buttons::ADS]);
        b.set(Buttons::FIRE, tick % 40 == 0);
        let before = sim.soldiers[id].gun_pose(&sim.weapons).muzzle;
        let c = cmd(&sim, Vec2::ZERO, yaw, pitch, b);
        sim.step(&[(id, c)]);
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { origin, .. } => {
                    fired.push(origin);
                    muzzles.push(before);
                }
                SimEvent::VehicleHit { .. } => hit_self = true,
                _ => {}
            }
        }
    }
    assert!(fired.len() >= 3, "{fired:?}");
    for (origin, muzzle) in fired.iter().zip(&muzzles) {
        assert!(origin.distance(*muzzle) < 0.04, "a shot left {origin} and the muzzle was at {muzzle}");
    }
    assert!(!hit_self, "out of the open side a round touches nothing of the truck");
}

#[test]
fn a_gun_does_not_bear_outside_its_arc() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 1);
    // Straight ahead over the bonnet is nobody's arc but the windscreen's, and the post is in the way.
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 120);
    let events = hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::FIRE]), 60);
    assert!(shots(&events).is_empty());
    assert!(sim.soldiers[id].weapon.ready < 0.5, "it should have come down: {}", sim.soldiers[id].weapon.ready);
    // Swing it out of the side and it comes up and fires.
    hold(&mut sim, id, Vec2::ZERO, -FRAC_PI_2, 0.0, Buttons::default(), 120);
    let events = hold(&mut sim, id, Vec2::ZERO, -FRAC_PI_2, 0.0, buttons(&[Buttons::FIRE]), 30);
    assert_eq!(shots(&events).len(), 1);
}

#[test]
fn the_look_stops_at_what_a_neck_can_do_and_turns_with_the_truck() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 1);
    hold(&mut sim, id, Vec2::ZERO, PI, 0.0, Buttons::default(), 2);
    let arc = sim.carrier_seats(0)[1].look;
    let seat = sim.carrier_seats(0)[1];
    let off = prs_seats::relative_look(&sim.carrier_frame(0), &seat, sim.soldiers[id].yaw, 0.0).0;
    assert!((off.abs() - arc.yaw.y).abs() < 1e-3, "{off}");
    // Driven round by someone else, the same world look ends up out of reach and is brought along.
    let driver = seated(&mut sim, 2, 0);
    let (mut a, b) = (sim.soldiers[id].yaw, 0.0);
    for _ in 0..600 {
        let wheel = cmd(&sim, Vec2::new(1.0, 0.5), 0.0, 0.0, Buttons::default());
        let ride = cmd(&sim, Vec2::ZERO, a, b, Buttons::default());
        sim.step(&[(driver, wheel), (id, ride)]);
        a = sim.soldiers[id].yaw;
    }
    let off = prs_seats::relative_look(&sim.carrier_frame(0), &sim.carrier_seats(0)[1], a, b).0;
    assert!(off.abs() <= arc.yaw.y + 1e-3, "{off}");
}

#[test]
fn riding_rough_unsettles_the_gun_and_never_the_bullet() {
    // Two identical shooters out of the side, one parked and one with a driver at speed.
    let wander = |moving: bool| {
        let mut sim = yard();
        let id = seated(&mut sim, 7, 3);
        let driver = seated(&mut sim, 8, 0);
        let mut most = 0.0_f32;
        let mut shots_off = 0.0_f32;
        for tick in 0..500 {
            let mut b = buttons(&[Buttons::ADS]);
            b.set(Buttons::FIRE, tick > 300 && tick % 20 == 0);
            let wish = if moving { Vec2::new(0.0, 1.0) } else { Vec2::ZERO };
            let frame = sim.carrier_frame(0);
            let yaw = frame.yaw() - FRAC_PI_2;
            let (wheel, ride) = (cmd(&sim, wish, 0.0, 0.0, Buttons::default()), cmd(&sim, Vec2::ZERO, yaw, 0.0, b));
            sim.step(&[(driver, wheel), (id, ride)]);
            if tick > 200 {
                most = most.max(sim.soldiers[id].weapon.aim_offset().length());
            }
            for event in &sim.events {
                if let SimEvent::ShotFired { dir, .. } = *event {
                    let gun = sim.soldiers[id].gun_pose(&sim.weapons).forward;
                    shots_off = shots_off.max(gun.angle_between(dir));
                }
            }
        }
        (most, shots_off)
    };
    let (parked, _) = wander(false);
    let (moving, off) = wander(true);
    assert!(moving > parked * 1.3, "{moving} riding vs {parked} parked");
    // Between one tick's recoil and zero, a bullet leaves along the gun however rough the ride is.
    assert!(off < 0.03, "{off}");
}

#[test]
fn f_gets_you_out_at_your_door_in_your_own_time_standing_on_the_ground() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 1);
    let events = press(&mut sim, id, Buttons::USE);
    assert!(events.iter().any(|e| matches!(e, SimEvent::Seat { event: SeatEvent::Leaving { .. }, .. })), "{events:?}");
    assert!(sim.soldiers[id].seat.is_some(), "not straight away");
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    let events = hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert!(events.iter().any(|e| matches!(e, SimEvent::Seat { event: SeatEvent::Left { .. }, .. })), "{events:?}");
    let soldier = &sim.soldiers[id];
    assert_eq!(soldier.seat, None);
    let door = sim.carrier_frame(0).point(sim.carrier_entries(0)[1].stand);
    assert!(Vec2::new(soldier.body.pos.x - door.x, soldier.body.pos.z - door.z).length() < 0.3, "{}", soldier.body.pos);
    assert!(soldier.body.pos.y < 0.3, "{}", soldier.body.pos);
    assert_eq!(sim.vehicles[0].occupants.0[1], None);
    assert_eq!(soldier.body.stance, Stance::Stand);
    assert!(sim.world.capsule_is_clear(soldier.body.pos, sim.move_cfg.capsule(Stance::Stand)));
}

#[test]
fn jumping_out_at_speed_hurts_and_stepping_out_does_not() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    hold(&mut sim, driver, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default(), 360);
    assert!(sim.vehicles[0].vel.length() > 8.0, "{}", sim.vehicles[0].vel.length());
    let full = sim.soldiers[driver].health.health;
    // With nobody at the wheel it brakes while they climb out.
    hold(&mut sim, driver, Vec2::new(0.0, 1.0), 0.0, 0.0, buttons(&[Buttons::USE]), 1);
    assert!(sim.soldiers[driver].transit.is_some(), "the way out is open at any speed");
    let speed_at_door = sim.vehicles[0].vel.length();
    let mut left_at = 0.0;
    for _ in 0..TRANSIT_TICKS {
        let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
        left_at = if sim.soldiers[driver].seat.is_some() { sim.vehicles[0].vel.length() } else { left_at };
        sim.step(&[(driver, c)]);
    }
    assert_eq!(sim.soldiers[driver].seat, None);
    assert!(speed_at_door > left_at, "nobody at the wheel, it brakes");
    if left_at > sim.vehicle_cfgs[TRUCK].ride.exit_speed {
        assert!(sim.soldiers[driver].health.health < full);
    }
    let mut sim = yard();
    let calm = seated(&mut sim, 2, 1);
    let full = sim.soldiers[calm].health.health;
    press(&mut sim, calm, Buttons::USE);
    let (yaw, pitch) = (sim.soldiers[calm].yaw, sim.soldiers[calm].pitch);
    hold(&mut sim, calm, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[calm].health.health, full);
}

#[test]
fn a_passenger_jumping_from_a_truck_at_speed_is_hurt() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    let passenger = seated(&mut sim, 2, 1);
    let idle = |sim: &Sim| cmd(sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    for _ in 0..360 {
        let wheel = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
        sim.step(&[(driver, wheel), (passenger, idle(&sim))]);
    }
    assert!(sim.vehicles[0].vel.length() > 8.0);
    let full = sim.soldiers[passenger].health.health;
    let wheel = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
    let out = cmd(&sim, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::USE]));
    sim.step(&[(driver, wheel), (passenger, out)]);
    for _ in 0..TRANSIT_TICKS {
        let wheel = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
        sim.step(&[(driver, wheel), (passenger, idle(&sim))]);
    }
    assert_eq!(sim.soldiers[passenger].seat, None);
    assert!(sim.soldiers[passenger].health.health < full, "that should have hurt");
}

/// A rifle round at this point from this far off, straight along +X.
fn round_from(sim: &mut Sim, shooter: SoldierId, at: Vec3, from: Vec3) {
    let weapon = sim.soldiers[shooter].weapon.weapon;
    let bullet = Bullet::new(from, (at - from).normalize(), &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
}

fn events_until_bullets_land(sim: &mut Sim) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for _ in 0..60 {
        sim.step(&[]);
        events.extend_from_slice(&sim.events);
        if sim.bullets.is_empty() {
            break;
        }
    }
    events
}

#[test]
fn bullets_hurt_the_part_they_hit_and_sound_like_metal() {
    let mut sim = yard();
    let shooter = sim.spawn_soldier(1);
    sim.set_home(shooter, Vec3::new(30.0, 0.05, -30.0), 0.0);
    let frame = sim.carrier_frame(0);
    let engine = frame.point(Vec3::new(0.0, 0.2, -1.5));
    round_from(&mut sim, shooter, engine, engine + Vec3::new(0.0, 4.0, -12.0));
    let before = sim.vehicles[0].damage;
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::VehicleHit { zone: VehicleZone::Engine, .. })), "{events:?}");
    assert!(sim.vehicles[0].damage.engine < before.engine);
    assert_eq!(sim.vehicles[0].damage.hull, before.hull);
    // A tyre, from the side.
    let cfg: &VehicleConfig = &sim.vehicle_cfgs[TRUCK];
    let wheel = frame.point(sim.vehicles[0].axle(cfg, 1));
    let tyre = sim.vehicles[0].wheels[1].tyre;
    round_from(&mut sim, shooter, wheel, wheel + Vec3::new(12.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::VehicleHit { zone: VehicleZone::Tyre(1), .. })), "{events:?}");
    assert!(sim.vehicles[0].wheels[1].tyre < tyre);
}

fn chest(sim: &Sim, id: SoldierId) -> Vec3 {
    let pose = sim.body_pose(&sim.body_input(id));
    (pose[prs_body::Bone::Chest].start + pose[prs_body::Bone::Chest].end()) * 0.5
}

#[test]
fn a_passenger_is_exposed_through_the_open_side_and_a_door_only_slows_a_round() {
    let mut sim = yard();
    let shooter = sim.spawn_soldier(1);
    sim.set_home(shooter, Vec3::new(30.0, 0.05, -30.0), 0.0);
    let inside = seated(&mut sim, 2, 1);
    hold(&mut sim, inside, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 2);
    let at = chest(&sim, inside);
    round_from(&mut sim, shooter, at, at + Vec3::new(14.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    let clear = events.iter().find_map(|e| if let SimEvent::SoldierHit { soldier, damage, .. } = *e { (soldier == inside).then_some(damage) } else { None });
    let clear = clear.expect("nothing covers the upper body");
    assert!(!events.iter().any(|e| matches!(e, SimEvent::VehicleHit { .. })), "{events:?}");
    // Now through the door to the thigh below the sill. Slowed, it does less than it would have.
    let pose = sim.body_pose(&sim.body_input(inside));
    let thigh = (pose[prs_body::Bone::ThighR].start + pose[prs_body::Bone::ThighR].end()) * 0.5;
    round_from(&mut sim, shooter, thigh, thigh + Vec3::new(14.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::VehicleHit { zone: VehicleZone::Body, .. })), "{events:?}");
    let through = events.iter().find_map(|e| if let SimEvent::SoldierHit { soldier, damage, .. } = *e { (soldier == inside).then_some(damage) } else { None });
    let through = through.expect("a door does not stop a rifle round");
    assert!(through < clear, "{through} through a door vs {clear} in the open, allowing for the leg being a leg");
}

#[test]
fn someone_downed_in_a_seat_comes_out_of_it() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 3);
    sim.hurt(id, Zone::Chest, 120.0);
    assert!(!sim.soldiers[id].health.is_up());
    assert_eq!(sim.soldiers[id].seat, None);
    assert_eq!(sim.vehicles[0].occupants.0[3], None);
    assert!(sim.soldiers[id].body.pos.y < 0.3, "should be on the ground: {}", sim.soldiers[id].body.pos);
}

#[test]
fn a_shot_out_truck_stops_then_burns_then_goes_up_and_takes_people_with_it() {
    let mut sim = yard();
    let inside = seated(&mut sim, 1, 0);
    let nearby = sim.spawn_soldier(2);
    sim.set_home(nearby, Vec3::new(5.0, 0.05, 0.0), 0.0);
    let d = sim.vehicle_cfgs[TRUCK].damage;
    sim.bullet_into_vehicle(0, VehicleZone::Engine, d.engine + 1.0);
    assert_eq!(sim.vehicles[0].damage.engine, 0.0);
    let start = sim.vehicles[0].pos;
    hold(&mut sim, inside, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default(), 120);
    assert!(sim.vehicles[0].pos.distance(start) < 0.05, "a dead engine does not drive");
    sim.bullet_into_vehicle(0, VehicleZone::Engine, 20.0);
    assert!(sim.vehicles[0].damage.burning.is_some());
    // Sitting in a fire is unhealthy.
    let before = sim.soldiers[inside].health.health;
    hold(&mut sim, inside, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 200);
    assert!(sim.soldiers[inside].health.health < before);
    press(&mut sim, inside, Buttons::USE);
    hold(&mut sim, inside, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 100);
    assert_eq!(sim.soldiers[inside].seat, None, "anyone can get out of a fire, being burned or not");
    sim.soldiers[inside].body.pos.x -= 60.0;
    let health_near = sim.soldiers[nearby].health.health;
    let mut blew = false;
    for _ in 0..((d.burn_time + 1.0) * 60.0) as u32 {
        sim.step(&[]);
        blew |= sim.events.iter().any(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::Exploded { .. }, .. }));
    }
    assert!(blew);
    assert!(sim.vehicles[0].damage.wrecked);
    assert!(sim.soldiers[nearby].health.health < health_near, "the blast should have reached 5 m");
    // Nothing gets in a wreck.
    let late = beside_seat(&mut sim, 3, 1);
    press(&mut sim, late, Buttons::USE);
    assert_eq!(sim.soldiers[late].seat, None);
    // In time a new one comes.
    for _ in 0..((d.respawn_after + 1.0) * 60.0) as u32 {
        sim.step(&[]);
    }
    assert!(!sim.vehicles[0].damage.wrecked);
    assert_eq!(sim.vehicles[0].damage.engine, d.engine);
}

#[test]
fn a_soldier_on_foot_cannot_walk_through_a_truck_and_being_run_down_hurts() {
    let mut sim = yard();
    let walker = sim.spawn_soldier(1);
    sim.set_home(walker, Vec3::new(-6.0, 0.05, 0.0), -FRAC_PI_2);
    hold(&mut sim, walker, Vec2::new(0.0, 1.0), -FRAC_PI_2, 0.0, Buttons::default(), 240);
    assert!(sim.soldiers[walker].body.pos.x < 0.0, "walked through it: {}", sim.soldiers[walker].body.pos);
    assert!(sim.soldiers[walker].health.health >= sim.health_cfg.max_health, "walking into a parked truck is harmless");

    let driver = seated(&mut sim, 2, 0);
    let victim = sim.spawn_soldier(3);
    sim.set_home(victim, Vec3::new(0.0, 0.05, -45.0), 0.0);
    let full = sim.soldiers[victim].health.health;
    for _ in 0..480 {
        let wheel = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
        sim.step(&[(driver, wheel)]);
    }
    assert!(sim.vehicles[0].pos.z < -45.0, "should have gone past: {}", sim.vehicles[0].pos);
    assert!(sim.soldiers[victim].health.health < full * 0.8, "{}", sim.soldiers[victim].health.health);
}

#[test]
fn a_crash_hurts_the_truck_and_who_is_in_it() {
    let mut sim = yard_with(|level| {
        level.solids.push(prs_level::Solid {
            center: Vec3::new(0.0, 1.5, -80.0),
            half: Vec3::new(9.0, 1.5, 0.5),
            rotation: Quat::IDENTITY,
            paint: prs_level::Paint::Concrete,
        });
    });
    let id = seated(&mut sim, 1, 0);
    let (hull, health) = (sim.vehicles[0].damage.hull, sim.soldiers[id].health.health);
    let mut crashed = 0.0_f32;
    for _ in 0..480 {
        let wheel = cmd(&sim, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default());
        sim.step(&[(id, wheel)]);
        for event in &sim.events {
            if let SimEvent::Vehicle { event: VehicleEvent::Crashed { speed, .. }, .. } = event {
                crashed = crashed.max(*speed);
            }
        }
    }
    assert!(crashed > 9.0, "{crashed}");
    assert!(sim.vehicles[0].damage.hull < hull);
    assert!(sim.soldiers[id].health.health < health);
    assert!(sim.vehicles[0].pos.z > -80.0);
}

#[test]
fn a_body_in_a_seat_is_sitting_and_stays_inside_the_truck() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 1);
    hold(&mut sim, id, Vec2::ZERO, -FRAC_PI_2, 0.0, buttons(&[Buttons::ADS]), 60);
    let pose = sim.body_pose(&sim.body_input(id));
    let frame = sim.carrier_frame(0);
    let seat = &sim.carrier_seats(0)[1];
    assert!(pose[prs_body::Bone::Pelvis].start.distance(frame.point(seat.hips)) < 0.25, "{}", pose[prs_body::Bone::Pelvis].start);
    // Knees are bent: thighs well off vertical, and the feet stay where the seat says.
    for foot in [prs_body::Bone::FootL, prs_body::Bone::FootR] {
        assert!(pose[foot].start.distance(frame.point(seat.feet)) < 0.3, "{:?}", pose[foot]);
    }
    let thigh = pose[prs_body::Bone::ThighR].end() - pose[prs_body::Bone::ThighR].start;
    assert!(thigh.normalize().y.abs() < 0.7, "{thigh}");
    // Every bone is within reach of the bullet check, however it is sitting.
    let middle = sim.soldiers[id].body.pos + Vec3::Y * hits::BODY_MIDDLE;
    for bone in prs_body::Bone::ALL {
        assert!(pose[bone].start.distance(middle) < hits::BODY_REACH, "{bone:?}");
    }
}

#[test]
fn a_driver_keeps_their_hands_on_the_wheel_whichever_way_they_look() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    for yaw in [0.0, 1.0, -1.5, 2.1] {
        hold(&mut sim, id, Vec2::ZERO, yaw, 0.0, Buttons::default(), 2);
        let pose = sim.body_pose(&sim.body_input(id));
        let frame = sim.carrier_frame(0);
        let [l, r] = sim.carrier_seats(0)[0].hands.unwrap();
        assert!(pose[prs_body::Bone::HandL].start.distance(frame.point(l)) < 0.2, "{yaw}: {:?}", pose[prs_body::Bone::HandL]);
        assert!(pose[prs_body::Bone::HandR].start.distance(frame.point(r)) < 0.2, "{yaw}: {:?}", pose[prs_body::Bone::HandR]);
    }
}

#[test]
fn hands_go_round_with_the_steering_wheel_and_stay_on_it() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    let (middle, radius) = sim.vehicle_cfgs[TRUCK].steering_wheel().unwrap();
    let hand_at = |sim: &Sim| {
        let pose = sim.body_pose(&sim.body_input(id));
        sim.carrier_frame(0).to_local(pose[prs_body::Bone::HandR].start)
    };
    let straight = hand_at(&sim);
    hold(&mut sim, id, Vec2::new(1.0, 0.3), 0.0, 0.0, Buttons::default(), 30);
    assert!(sim.vehicles[0].steer > 0.1);
    let turned = hand_at(&sim);
    assert!(turned.distance(straight) > 0.05, "{straight} to {turned}");
    // Steering right, the top of the wheel goes right, so the right hand comes down.
    assert!(turned.y < straight.y, "{straight} to {turned}");
    assert!((turned.distance(middle) - radius).abs() < 0.1, "still on the rim: {} from a rim of {radius}", turned.distance(middle));
}

fn busy_ride_cmds(sim: &Sim, tick: u32) -> [(SoldierId, PlayerCmd); 2] {
    let beat = |every: u32, at: u32| tick % every == at;
    let mut d = Buttons::default();
    d.set(Buttons::SLOT_1, beat(900, 100));
    let mut p = Buttons::default();
    p.set(Buttons::ADS, tick % 400 < 300);
    p.set(Buttons::FIRE, tick % 400 < 300 && beat(30, 0));
    p.set(Buttons::RELOAD, beat(1000, 350));
    p.set(Buttons::SLOT_4, beat(900, 450));
    p.set(Buttons::SLOT_3, beat(900, 700));
    let wish = Vec2::new((tick as f32 * 0.013).sin(), (tick as f32 * 0.004).cos());
    let frame = sim.carrier_frame(0);
    let yaw = frame.yaw() - FRAC_PI_2 + (tick as f32 * 0.02).sin() * 0.4;
    let drive = PlayerCmd { tick, wish_move: wish, yaw: 0.0, pitch: 0.0, buttons: d, ..Default::default() };
    let ride = PlayerCmd { tick, wish_move: Vec2::ZERO, yaw, pitch: 0.02, buttons: p, ..Default::default() };
    [(0, drive), (1, ride)]
}

fn busy_ride() -> Sim {
    let mut sim = yard_with(|level| {
        level.vehicles.push(prs_level::VehicleSpawn { kind: 0, pos: Vec3::new(12.0, 0.0, -30.0), yaw: 0.7 });
    });
    seated(&mut sim, 11, 0);
    seated(&mut sim, 12, 3);
    sim
}

/// Everything about a vehicle and who is in it comes across in an export, down to the last bit of every
/// spring: restore it at any moment and the two go on identically.
#[test]
fn a_sim_with_vehicles_restored_from_an_export_stays_identical() {
    let mut original = busy_ride();
    let mut crashes = 0;
    let mut burned = false;
    for tick in 0..2400 {
        let cmds = busy_ride_cmds(&original, tick);
        original.step(&cmds);
        crashes += original.events.iter().filter(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::Crashed { .. }, .. })).count();
        if tick == 900 {
            original.bullet_into_vehicle(0, VehicleZone::Tyre(1), 500.0);
        }
        if tick == 1300 {
            original.bullet_into_vehicle(1, VehicleZone::Engine, 5000.0);
            original.bullet_into_vehicle(1, VehicleZone::Engine, 50.0);
            burned = true;
        }
        if tick % 400 != 350 {
            continue;
        }
        let state = original.export_state();
        let mut restored = busy_ride();
        restored.import_state(&state);
        assert_eq!(restored.export_state(), state);
        let mut twin = busy_ride();
        twin.import_state(&state);
        for later in tick + 1..tick + 120 {
            let cmds = busy_ride_cmds(&twin, later);
            twin.step(&cmds);
            let cmds = busy_ride_cmds(&restored, later);
            restored.step(&cmds);
            assert_eq!(restored.export_state(), twin.export_state(), "restored at {tick}, differs at {later}");
        }
        // And against the original itself, which goes on from the same point.
        let mut copy = busy_ride();
        copy.import_state(&state);
        for later in tick + 1..tick + 60 {
            let cmds = busy_ride_cmds(&original, later);
            original.step(&cmds);
            copy.step(&cmds);
            assert_eq!(original.export_state(), copy.export_state(), "restored at {tick}, differs at {later}");
        }
    }
    assert!(burned);
    assert!(original.vehicles[0].vel.length() > 0.0 || crashes > 0 || original.vehicles[0].pos.length() > 10.0);
    assert!(original.vehicles[1].damage.burning.is_some() || original.vehicles[1].damage.wrecked);
}

#[test]
fn a_client_predicting_its_driver_steps_the_same_as_the_server() {
    let mut server = busy_ride();
    let mut client = busy_ride();
    client.import_state(&server.export_state());
    for tick in 0..200 {
        let wish = Vec2::new((tick as f32 * 0.03).sin(), 1.0);
        let cmd = PlayerCmd { tick: server.tick + 1, wish_move: wish, yaw: 0.0, pitch: 0.0, buttons: Buttons::default(), ..Default::default() };
        server.step(&[(0, cmd)]);
        client.step_predicted(0, &cmd);
        assert_eq!(server.soldiers[0].body.pos, client.soldiers[0].body.pos, "at {tick}");
        assert_eq!(server.vehicles[0].pos, client.vehicles[0].pos, "at {tick}");
    }
}

/// An armoured car at the origin and someone in the hatch, someone in the back.
fn armour() -> Sim {
    let mut level = Level::flat(1000.0);
    level.vehicles.push(prs_level::VehicleSpawn { kind: VehicleTable::ARMOURED_CAR.0, pos: Vec3::ZERO, yaw: 0.0 });
    let mut sim = Sim::new(level);
    for _ in 0..120 {
        sim.step(&[]);
    }
    sim
}

/// Up onto the armoured car from behind with hands and feet, across the deck, and in at the hatch.
#[test]
fn you_climb_onto_the_armoured_car_and_get_in_at_the_hatch() {
    let mut sim = armour();
    let id = sim.spawn_soldier(1);
    let frame = sim.carrier_frame(0);
    let behind = frame.point(Vec3::new(0.0, 0.0, 4.3));
    let top = Vec3::new(behind.x, 5.0, behind.z);
    let ground = 5.0 - sim.world.raycast(top, Vec3::NEG_Y, 9.0).unwrap().distance;
    sim.set_home(id, Vec3::new(behind.x, ground + 0.05, behind.z), 0.0);
    // At the hatch from the ground it does not open: it is out of reach up there.
    assert!(!matches!(sim.use_target(id), Some(UseTarget::Door { seat: 1, .. })));
    let hatch_stand = frame.point(sim.carrier_entries(0).iter().find(|entry| entry.name == "hatch").unwrap().stand);
    let (mut walked, mut b) = (0, Buttons::default());
    for tick in 0..900 {
        let on_deck = sim.soldiers[id].body.pos.y > hatch_stand.y - 0.3;
        b.set(Buttons::JUMP, tick % 100 == 50 && !on_deck);
        let near = Vec2::new(sim.soldiers[id].body.pos.x - hatch_stand.x, sim.soldiers[id].body.pos.z - hatch_stand.z).length() < 0.6;
        let wish = if on_deck && near { Vec2::ZERO } else { Vec2::new(0.0, 1.0) };
        let c = cmd(&sim, wish, 0.0, 0.0, b);
        sim.step(&[(id, c)]);
        walked += 1;
        if on_deck && near {
            break;
        }
    }
    let soldier = &sim.soldiers[id];
    assert!(soldier.body.pos.y > hatch_stand.y - 0.3, "never got onto the deck: {} after {walked}", soldier.body.pos);
    // Stood on it, not in it.
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 30);
    assert!((sim.soldiers[id].body.pos.y - hatch_stand.y).abs() < 0.15, "{} on a deck at {}", sim.soldiers[id].body.pos.y, hatch_stand.y);
    sim.soldiers[id].pitch = -0.7;
    let target = sim.use_target(id);
    assert!(matches!(target, Some(UseTarget::Door { seat: 1, .. })), "{target:?}");
    hold(&mut sim, id, Vec2::ZERO, 0.0, -0.7, buttons(&[Buttons::USE]), 1);
    assert_eq!(sim.soldiers[id].seat.map(|at| at.seat), Some(1));
    hold(&mut sim, id, Vec2::ZERO, 0.0, -0.1, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].transit, None);
}

#[test]
fn from_the_hatch_there_is_a_way_through_the_inside_to_the_driver() {
    let mut sim = armour();
    let id = sim.spawn_soldier(1);
    sim.sit_in(id, SeatedAt { carrier: 0, seat: 1 });
    for (key, seat) in [(Buttons::SLOT_1, None), (Buttons::SLOT_4, Some(3)), (Buttons::SLOT_3, Some(2)), (Buttons::SLOT_1, Some(0))] {
        press(&mut sim, id, key);
        if let Some(seat) = seat {
            assert_eq!(sim.soldiers[id].seat.unwrap().seat, seat);
        } else {
            assert_eq!(sim.soldiers[id].seat.unwrap().seat, 1, "not straight from the hatch");
        }
        hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), TRANSIT_TICKS);
    }
    assert!(sim.is_at_controls(id));
}

#[test]
fn the_gunner_in_the_hatch_shoots_all_round_and_is_up_there_to_be_shot() {
    let mut sim = armour();
    let gunner = sim.spawn_soldier(1);
    sim.sit_in(gunner, SeatedAt { carrier: 0, seat: 1 });
    let shooter = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(40.0, 0.05, 40.0), 0.0);
    for yaw in [0.0, FRAC_PI_2, PI, -FRAC_PI_2, 2.5] {
        hold(&mut sim, gunner, Vec2::ZERO, yaw, 0.03, buttons(&[Buttons::ADS]), 90);
        let events = hold(&mut sim, gunner, Vec2::ZERO, yaw, 0.03, buttons(&[Buttons::ADS, Buttons::FIRE]), 3);
        let fired = shots(&events);
        assert_eq!(fired.len(), 1, "yaw {yaw}");
        let until = events_until_bullets_land(&mut sim);
        assert!(!until.iter().any(|e| matches!(e, SimEvent::VehicleHit { .. })), "yaw {yaw}: shot its own car: {until:?}");
        hold(&mut sim, gunner, Vec2::ZERO, yaw, 0.03, Buttons::default(), 30);
    }
    // The whole body goes round with the gun, and stays in the hatch.
    let pose = sim.body_pose(&sim.body_input(gunner));
    let at = sim.carrier_frame(0).point(sim.carrier_seats(0)[1].hips);
    assert!(pose[prs_body::Bone::Pelvis].start.distance(at) < 0.3);
    let up = chest(&sim, gunner);
    assert!(up.y > sim.vehicles[0].pos.y + 1.15, "the chest should be out of the roof: {up}");
    round_from(&mut sim, shooter, up, up + Vec3::new(20.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::SoldierHit { soldier, .. } if *soldier == gunner)), "{events:?}");
}

#[test]
fn those_inside_armour_are_out_of_the_fight_and_out_of_harm() {
    let mut sim = armour();
    let inside = seated(&mut sim, 1, 3);
    let shooter = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(40.0, 0.05, 40.0), 0.0);
    hold(&mut sim, inside, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 60);
    assert!(sim.gun_put_away(inside), "no firing ports");
    let events = hold(&mut sim, inside, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::FIRE]), 30);
    assert!(shots(&events).is_empty());
    let body = chest(&sim, inside);
    round_from(&mut sim, shooter, body, body + Vec3::new(20.0, 0.0, 0.0));
    let events = events_until_bullets_land(&mut sim);
    assert!(events.iter().any(|e| matches!(e, SimEvent::VehicleHit { .. })), "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, SimEvent::SoldierHit { .. })), "{events:?}");
}

/// Q and E, one gear to a press however long the key is held, and nobody but the driver.
#[test]
fn q_and_e_change_gear_at_the_wheel_one_press_at_a_time() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    let passenger = seated(&mut sim, 2, 1);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 10);
    hold(&mut sim, passenger, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::LEAN_RIGHT]), 10);
    assert_eq!(sim.vehicles[0].engine.gear, 1, "a passenger leaning changes nothing");
    assert!(!sim.vehicles[0].engine.manual);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::LEAN_RIGHT]), 30);
    assert_eq!(sim.vehicles[0].engine.gear, 2, "held for half a second, still one press");
    assert!(sim.vehicles[0].engine.manual);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 2);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::LEAN_RIGHT]), 2);
    assert_eq!(sim.vehicles[0].engine.gear, 3);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default(), 2);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::LEAN_LEFT]), 2);
    assert_eq!(sim.vehicles[0].engine.gear, 2);
}

#[test]
fn w_and_a_together_are_full_throttle_and_full_lock_and_space_is_the_handbrake() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    let diagonal = Vec2::new(-1.0, 1.0).clamp_length_max(1.0);
    hold(&mut sim, id, diagonal, 0.0, 0.0, Buttons::default(), 120);
    assert!(sim.vehicles[0].steer < -0.3, "should be near full lock left: {}", sim.vehicles[0].steer);
    assert!(sim.vehicles[0].engine.load > 0.95, "{}", sim.vehicles[0].engine.load);
    hold(&mut sim, id, Vec2::new(0.0, 1.0), 0.0, 0.0, Buttons::default(), 120);
    assert!(sim.vehicles[0].vel.length() > 6.0);
    let going = sim.vehicles[0].vel.length();
    hold(&mut sim, id, Vec2::new(0.0, 1.0), 0.0, 0.0, buttons(&[Buttons::JUMP]), 60);
    assert!(sim.vehicles[0].vel.length() < going - 2.0, "{}", sim.vehicles[0].vel.length());
    assert_eq!(sim.vehicles[0].engine.load, 0.0, "off the throttle, whatever W says");
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::JUMP]), 300);
    assert!(sim.vehicles[0].vel.length() < 0.05, "and it holds still: {}", sim.vehicles[0].vel.length());
}

#[test]
fn every_map_has_vehicles_that_start_clear_upright_and_able_to_drive_off() {
    for name in ["town", "range"] {
        let mut sim = Sim::new(Level::named(name).unwrap());
        assert!(sim.vehicles.len() >= 2, "{name}");
        for _ in 0..120 {
            sim.step(&[]);
        }
        for (v, vehicle) in sim.vehicles.iter().enumerate() {
            let start = sim.level.vehicles[v].pos;
            assert!((vehicle.rot * Vec3::Y).y > 0.94, "{name} {v} at {start} tips {:?}", vehicle.rot * Vec3::Y);
            assert!(vehicle.wheels.iter().all(|w| w.on_ground), "{name} {v} at {start}");
            assert!(vehicle.asleep, "{name} {v} at {start} has not settled");
            let drifted = Vec2::new(vehicle.pos.x - start.x, vehicle.pos.z - start.z).length();
            assert!(drifted < 0.4, "{name} {v} at {start} moved {drifted} getting started");
        }
        // Each drives off where it points without a building in the way, with room to get in at a door.
        for v in 0..sim.vehicles.len() {
            let id = sim.spawn_soldier(v as u32);
            let entry = sim.carrier_entries(v as u32).iter().find(|entry| entry.seats.contains(&0)).unwrap().clone();
            let frame = sim.carrier_frame(v as u32);
            let stand = frame.point(entry.stand);
            let top = stand + Vec3::Y * 3.0;
            let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 8.0).unwrap().distance;
            let spot = Vec3::new(stand.x, ground + 0.05, stand.z);
            let to_hand = frame.point(entry.hand) - (spot + Vec3::Y * sim.move_cfg.stand_eye);
            sim.set_home(id, spot, (-to_hand.x).atan2(-to_hand.z));
            sim.soldiers[id].pitch = (to_hand.y / to_hand.length()).asin();
            press(&mut sim, id, Buttons::USE);
            assert_eq!(sim.soldiers[id].seat.map(|at| at.carrier), Some(v as u32), "{name} {v}: could not board");
            let yaw = sim.soldiers[id].yaw;
            hold(&mut sim, id, Vec2::ZERO, yaw, 0.0, Buttons::default(), TRANSIT_TICKS);
            let from = sim.vehicles[v].pos;
            for _ in 0..240 {
                let c = cmd(&sim, Vec2::new(0.0, 1.0), sim.soldiers[id].yaw, 0.0, Buttons::default());
                sim.step(&[(id, c)]);
            }
            let moved = sim.vehicles[v].pos.distance(from);
            assert!(moved > 15.0, "{name} {v} at {from} only went {moved}");
            press(&mut sim, id, Buttons::USE);
        }
    }
}

/// Drives a vehicle along these points on the map, steering by itself, and says how it went.
fn drive_route(sim: &mut Sim, id: SoldierId, route: &[Vec2], seconds: f32) -> (usize, u32, f32) {
    let v = sim.soldiers[id].seat.unwrap().carrier as usize;
    let mut next = 0;
    let mut crashes = 0;
    let mut most_tilt = 0.0_f32;
    let (mut stuck_for, mut backing) = (0, 0);
    for _ in 0..(seconds * 60.0) as u32 {
        if next >= route.len() {
            break;
        }
        let vehicle = &sim.vehicles[v];
        let flat = Vec2::new(vehicle.pos.x, vehicle.pos.z);
        if flat.distance(route[next]) < 7.0 {
            next += 1;
            continue;
        }
        let to = route[next] - flat;
        let want = (-to.x).atan2(-to.y);
        let off = (want - vehicle.frame().yaw() + PI).rem_euclid(2.0 * PI) - PI;
        // Yaw grows to the left, and wish x steers right.
        let steer = (-off * 2.0).clamp(-1.0, 1.0);
        let speed = vehicle.vel.length();
        // Slows for each point, as someone picking their way through a chicane would.
        let limit = if flat.distance(route[next]) < 30.0 { 7.0 } else { 14.0 };
        let throttle = if off.abs() > 0.5 && speed > 6.0 { -0.3 } else if speed > limit { 0.0 } else { 1.0 };
        // Backs off and tries again if something has stopped it, as anyone would.
        stuck_for = if speed < 0.5 { stuck_for + 1 } else { 0 };
        if stuck_for > 120 {
            backing = 120;
            stuck_for = 0;
        }
        let (steer, throttle) = if backing > 0 {
            backing -= 1;
            (-steer, -1.0)
        } else {
            (steer, throttle)
        };
        let c = cmd(sim, Vec2::new(steer, throttle), sim.soldiers[id].yaw, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
        crashes += sim.events.iter().filter(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::Crashed { .. }, .. })).count() as u32;
        let vehicle = &sim.vehicles[v];
        most_tilt = most_tilt.max((vehicle.rot * Vec3::Y).y.clamp(-1.0, 1.0).acos());
        assert!(vehicle.pos.is_finite() && vehicle.rot.is_finite(), "blew up: {:?}", vehicle.pos);
    }
    (next, crashes, most_tilt)
}

/// A real drive on the real map, out of the base and over the road through the ridge to the village.
#[test]
fn a_truck_can_drive_from_the_base_to_the_village_by_road() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    for _ in 0..90 {
        sim.step(&[]);
    }
    let id = sim.spawn_soldier(1);
    sim.sit_in(id, SeatedAt { carrier: 0, seat: 0 });
    let route: Vec<Vec2> = [(60.0, 424.0), (46.0, 412.0), (46.0, 400.0), (14.0, 300.0), (-8.0, 245.0), (2.0, 170.0), (9.0, 150.0), (9.0, 135.0), (9.0, 110.0), (12.0, 100.0), (2.0, 40.0)].iter().map(|&(x, z)| Vec2::new(x, z)).collect();
    let (reached, crashes, tilt) = drive_route(&mut sim, id, &route, 120.0);
    assert_eq!(reached, route.len(), "got to {} at {} with {crashes} crashes", reached, sim.vehicles[0].pos);
    assert!(tilt < 0.6, "leaned {tilt}");
    assert!(!sim.vehicles[0].damage.wrecked);
}

/// How much a tick costs with vehicles moving, for keeping an eye on it.
#[test]
#[ignore]
fn drive_cost_report() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    for _ in 0..90 {
        sim.step(&[]);
    }
    let started = std::time::Instant::now();
    for _ in 0..600 {
        sim.step(&[]);
    }
    println!("{} parked vehicles: {:?} a tick", sim.vehicles.len(), started.elapsed() / 600);
    let ids: Vec<SoldierId> = (0..sim.vehicles.len())
        .map(|v| {
            let id = sim.spawn_soldier(v as u32);
            sim.sit_in(id, SeatedAt { carrier: v as u32, seat: 0 });
            id
        })
        .collect();
    let started = std::time::Instant::now();
    for tick in 0..600 {
        let cmds: Vec<(SoldierId, PlayerCmd)> = ids
            .iter()
            .map(|&id| (id, PlayerCmd { tick, wish_move: Vec2::new((tick as f32 * 0.01).sin(), 1.0), yaw: 0.0, pitch: 0.0, buttons: Buttons::default(), ..Default::default() }))
            .collect();
        sim.step(&cmds);
    }
    println!("{} driven vehicles: {:?} a tick", sim.vehicles.len(), started.elapsed() / 600);
}

/// How a truck meets one of the map's real field ditches, driven straight at it at each speed.
fn at_a_real_ditch(speed: f32, x: f32, z: f32) -> (f32, f32, bool) {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let id = sim.spawn_soldier(1);
    // Well off the road, north of the ditch that runs along z = 180, pointing south across it.
    let start = Vec3::new(x, 0.0, z);
    let top = start + Vec3::Y * 200.0;
    let ground = 200.0 - sim.world.raycast(top, Vec3::NEG_Y, 400.0).unwrap().distance;
    let v = 0;
    let cfg = sim.vehicle_cfgs[sim.vehicles[v].kind].clone();
    sim.vehicles[v] = VehicleState::new(sim.vehicles[v].kind, &cfg, prs_seats::Frame { pos: Vec3::new(x, ground + cfg.ride_height(), z), rot: Quat::from_rotation_y(PI) });
    sim.sit_in(id, SeatedAt { carrier: v as u32, seat: 0 });
    let mut most_tilt = 0.0_f32;
    for _ in 0..(40.0 * 60.0) as u32 {
        let vehicle = &sim.vehicles[v];
        let throttle = if vehicle.vel.length() < speed { 1.0 } else { 0.0 };
        let c = cmd(&sim, Vec2::new(0.0, throttle), sim.soldiers[id].yaw, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
        most_tilt = most_tilt.max((sim.vehicles[v].rot * Vec3::Y).y.clamp(-1.0, 1.0).acos());
    }
    let hull = sim.vehicles[v].damage.hull;
    (sim.vehicles[v].pos.z, most_tilt, hull < cfg.damage.hull)
}

/// Taken gently it goes in and stays there. A fast enough run bounds across, and pays for it.
#[test]
fn the_fields_ditches_are_a_real_risk() {
    let (end, _, _) = at_a_real_ditch(3.0, -180.0, 95.0);
    assert!((100.0..118.0).contains(&end), "stopped at z {end}");
    let (end, _, _) = at_a_real_ditch(3.0, 110.0, 170.0);
    assert!(end < 196.0, "stopped at z {end}");
    let (end, leaned, hurt) = at_a_real_ditch(15.0, -180.0, 95.0);
    assert!(end > 150.0, "stopped at z {end}");
    assert!(leaned < 1.0, "should stay on its wheels: {leaned}");
    assert!(hurt, "that should have cost something");
}

/// The reason B exists: nobody bleeds out in a vehicle with a bandage in their pocket, moving or not.
#[test]
fn b_works_in_a_seat_even_at_the_wheel_and_on_the_move() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    sim.hurt(id, Zone::Abdomen, 20.0);
    let mut ticks = 0;
    while sim.soldiers[id].health.bleeding() {
        hold(&mut sim, id, Vec2::Y, 0.0, 0.0, buttons(&[Buttons::BANDAGE]), 1);
        ticks += 1;
        assert!(ticks < 900, "never finished, treatment {:?}", sim.soldiers[id].treatment);
    }
    assert!(sim.vehicles[0].vel.length() > 1.0, "the truck really was moving");
    assert!(sim.soldiers[id].seat.is_some(), "and they never left it");
    assert_eq!(sim.soldiers[id].inventory.stash.bandages, 1);
}

#[test]
fn b_stops_for_moving_seats_and_starts_again_once_you_are_there() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 3);
    sim.hurt(id, Zone::Leg, 20.0);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::BANDAGE]), 90);
    assert!(sim.soldiers[id].treatment.is_some());
    let mut b = buttons(&[Buttons::BANDAGE]);
    b.set(Buttons::SLOT_3, true);
    hold(&mut sim, id, Vec2::ZERO, 0.0, 0.0, b, 2);
    if sim.soldiers[id].transit.is_some() {
        assert!(sim.soldiers[id].treatment.is_none(), "not while climbing across");
    }
}

#[test]
fn v_does_nothing_at_the_wheel_and_works_from_a_seat_you_can_fire_from() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    let mode = sim.soldiers[driver].weapon.mode;
    hold(&mut sim, driver, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::FIRE_MODE]), 2);
    assert_eq!(sim.soldiers[driver].weapon.mode, mode, "no gun to switch at the wheel");

    let mut sim = yard();
    let passenger = seated(&mut sim, 2, 3);
    let mode = sim.soldiers[passenger].weapon.mode;
    hold(&mut sim, passenger, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::FIRE_MODE]), 2);
    assert_ne!(sim.soldiers[passenger].weapon.mode, mode, "this seat has a gun");
}

// ---- Grenades among vehicles.

fn frag_beside(sim: &mut Sim, thrower: SoldierId, at: Vec3, id: u32) {
    sim.thrown.push(prs_throwables::Thrown { id, kind: prs_throwables::ThrownKind::Frag, thrower, pos: at, vel: Vec3::ZERO, fuse: 0.02, at_rest: true });
    for _ in 0..4 {
        sim.step(&[]);
    }
}

#[test]
fn a_frag_finds_someone_in_an_open_truck_and_marks_the_truck() {
    let mut sim = yard();
    let rider = seated(&mut sim, 1, 1);
    let far = sim.spawn_soldier(2);
    sim.set_home(far, Vec3::new(300.0, 0.1, 300.0), 0.0);
    let hull = sim.vehicles[0].damage.hull;
    let mut hit = 0;
    let mut tyres_or_more = false;
    for run in 0..12 {
        let before = sim.soldiers[rider].health.health;
        sim.soldiers[rider].attacker = None;
        frag_beside(&mut sim, far, Vec3::new(4.0, 0.15, 0.0), run + 900);
        tyres_or_more |= sim.vehicles[0].damage.hull < hull || sim.vehicles[0].wheels.iter().any(|w| w.tyre < 1.0);
        hit += usize::from(sim.soldiers[rider].attacker == Some(far) || sim.soldiers[rider].health.health < before);
        let fresh = Health::new(&sim.health_cfg);
        sim.soldiers[rider].health = fresh;
    }
    assert!(hit >= 6, "in an open-sided truck, 4 m from a frag, hit {hit} times of 12");
    assert!(tyres_or_more, "and the truck should have taken something");
}

#[test]
fn armour_keeps_a_frag_off_whoever_is_inside_and_off_whoever_is_behind_it() {
    let mut sim = armour();
    let id = sim.spawn_soldier(1);
    // In the back, where armour is all round.
    sim.sit_in(id, SeatedAt { carrier: 0, seat: 3 });
    assert!(sim.soldiers[id].seat.is_some(), "should have got in");
    // Crouched right behind it, on the far side from the frag.
    let behind = sim.spawn_soldier(4);
    sim.set_home(behind, Vec3::new(-2.2, 0.1, 0.0), 0.0);
    sim.soldiers[behind].body.stance = Stance::Crouch;
    sim.soldiers[behind].body.eye_height = sim.move_cfg.crouch_eye;
    let far = sim.spawn_soldier(2);
    sim.set_home(far, Vec3::new(300.0, 0.1, 300.0), 0.0);
    for _ in 0..10 {
        sim.step(&[]);
    }
    let (inside, outside) = (sim.soldiers[id].health.health, sim.soldiers[behind].health.health);
    for run in 0..10 {
        frag_beside(&mut sim, far, Vec3::new(2.6, 0.15, 0.0), run + 950);
    }
    assert_eq!(sim.soldiers[id].health.health, inside, "came through the armour");
    assert!(sim.soldiers[behind].health.health >= outside - 40.0, "the car should have covered someone crouched behind it");
}

/// `cargo test -p prs_sim town_slip_report -- --ignored --nocapture`. Keys held as a player holds them, over
/// the real ground: how often the truck is going some way other than where it points.
#[test]
#[ignore]
fn town_slip_report() {
    for (name, start, yaw) in [("fields west", Vec2::new(-300.0, 0.0), -FRAC_PI_2), ("fields east", Vec2::new(200.0, 130.0), -FRAC_PI_2), ("south road", Vec2::new(40.0, 395.0), 0.1), ("north fields", Vec2::new(-200.0, -160.0), FRAC_PI_2)] {
        for kind in [VehicleTable::UTILITY_TRUCK, VehicleTable::ARMOURED_CAR] {
            let mut sim = Sim::new(Level::named("town").unwrap());
            let id = sim.spawn_soldier(1);
            let cfg = sim.vehicle_cfgs[kind].clone();
            let top = Vec3::new(start.x, 200.0, start.y);
            let ground = 200.0 - sim.world.raycast(top, Vec3::NEG_Y, 400.0).unwrap().distance;
            sim.vehicles[0] = VehicleState::new(kind, &cfg, prs_seats::Frame { pos: Vec3::new(start.x, ground + cfg.ride_height(), start.y), rot: Quat::from_rotation_y(yaw) });
            sim.sit_in(id, SeatedAt { carrier: 0, seat: 0 });
            let (mut ticks, mut over5, mut over10, mut over20, mut air, mut speed_sum, mut slid, mut worst) = (0u32, 0u32, 0u32, 0u32, 0u32, 0.0_f32, 0.0_f32, 0.0_f32);
            let mut crashes = 0;
            for tick in 0..(40.0 * 60.0) as u32 {
                // Round in lazy esses, brake and steer now and then, as anyone does.
                let phase = (tick / 150) % 4;
                let steer = [1.0, 0.0, -1.0, 0.0][phase as usize];
                let throttle = if tick % 600 > 540 { -1.0 } else { 1.0 };
                let c = cmd(&sim, Vec2::new(steer, throttle), sim.soldiers[id].yaw, 0.0, Buttons::default());
                sim.step(&[(id, c)]);
                crashes += sim.events.iter().filter(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::Crashed { .. }, .. })).count();
                let v = &sim.vehicles[0];
                if v.vel.length() > 3.0 {
                    // At the rear axle: the middle of a tight turn goes sideways without sliding at all.
                    let rear = cfg.wheels.iter().map(|w| w.mount.z).fold(f32::MIN, f32::max);
                    let at_rear = v.vel + v.ang_vel.cross(v.rot * Vec3::new(0.0, 0.0, rear));
                    let (side, along) = (at_rear.dot(v.rot * Vec3::X), at_rear.dot(v.rot * Vec3::NEG_Z));
                    let slip = side.atan2(along.abs()).abs().to_degrees();
                    ticks += 1;
                    over5 += (slip > 5.0) as u32;
                    over10 += (slip > 10.0) as u32;
                    over20 += (slip > 20.0) as u32;
                    worst = worst.max(slip);
                    slid += side.abs() / 60.0;
                    air += v.wheels.iter().filter(|w| !w.on_ground).count() as u32;
                    speed_sum += v.vel.length();
                }
            }
            let share = |n: u32| n as f32 * 100.0 / ticks.max(1) as f32;
            println!("{name} {}: {crashes} crashes, mean {:.1} m/s, slip >5 deg {:.0}% >10 {:.0}% >20 {:.0}%, worst {:.0}, slid {:.0} m, wheels up {:.0}% of the time, {:?}", cfg.name, speed_sum / ticks.max(1) as f32, share(over5), share(over10), share(over20), worst, slid, air as f32 * 25.0 / ticks.max(1) as f32, sim.vehicles[0].wheels[0].surface);
        }
    }
}

#[test]
#[ignore]
fn rolled_crew_report() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    let passenger = seated(&mut sim, 2, 1);
    let health = sim.soldiers[driver].health.health;
    sim.vehicles[0].pos.y += 2.5;
    sim.vehicles[0].rot = Quat::from_rotation_z(PI) * sim.vehicles[0].rot;
    sim.vehicles[0].asleep = false;
    hold(&mut sim, driver, Vec2::new(1.0, 1.0), 0.0, 0.0, Buttons::default(), 360);
    let v = &sim.vehicles[0];
    println!("settled at {:?} up {:?} speed {:.2} asleep {} running {} driver's blood {health} -> {} seat {:?}", v.pos, v.rot * Vec3::Y, v.vel.length(), v.asleep, v.engine.running, sim.soldiers[driver].health.health, sim.soldiers[driver].seat);
    println!("driver's eye {:?}", sim.seated_eye_at(driver, &sim.carrier_frame(0)));
    for (who, name) in [(driver, "driver"), (passenger, "passenger")] {
        let before = sim.soldiers[who].seat;
        press(&mut sim, who, Buttons::USE);
        let (yaw, pitch) = (sim.soldiers[who].yaw, sim.soldiers[who].pitch);
        hold(&mut sim, who, Vec2::ZERO, yaw, pitch, Buttons::default(), 120);
        let s = &sim.soldiers[who];
        println!("{name}: seat {before:?} -> {:?}, transit {:?}, at {:?}, velocity {:?}, blood {}", s.seat, s.transit, s.body.pos, s.body.vel, s.health.health);
    }
}

// ---- Righting a vehicle by hand.

/// A vehicle put on its side or roof, settled, with someone stood at it looking at it, at the height of its
/// `side` (1 is the way a body-right points), and their id.
fn overturned(sim: &mut Sim, v: usize, roll: f32) {
    let vehicle = &mut sim.vehicles[v];
    vehicle.rot = vehicle.rot * Quat::from_rotation_z(roll);
    vehicle.pos.y += 1.5;
    prs_vehicles::wake(vehicle);
    for _ in 0..300 {
        sim.step(&[]);
    }
}

fn at_hull(sim: &mut Sim, seed: u32, v: usize, off: Vec3) -> SoldierId {
    let id = sim.spawn_soldier(seed);
    let at = sim.vehicles[v].pos + off;
    let top = at + Vec3::Y * 5.0;
    let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 20.0).map_or(5.0, |hit| hit.distance);
    sim.soldiers[id].body.pos = Vec3::new(at.x, ground + 0.02, at.z);
    id
}

/// Facing what they mean to lift.
fn facing_hull(sim: &Sim, id: SoldierId, v: usize) -> (f32, f32) {
    let eye = sim.soldiers[id].eye();
    let to = (sim.nearest_of_hull(v, eye) - eye).normalize_or(Vec3::NEG_Z);
    ((-to.x).atan2(-to.z), to.y.asin())
}

/// All of these holding F on it for up to this long, however much they mean to.
fn heaving(sim: &mut Sim, who: &[SoldierId], v: usize, seconds: f32) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for tick in 0..(seconds * 60.0) as u32 {
        let cmds: Vec<(SoldierId, PlayerCmd)> = who
            .iter()
            .map(|&id| {
                let (yaw, pitch) = facing_hull(sim, id, v);
                // The press has to be a press, for those who come back to it.
                let held = sim.soldiers[id].lifting.is_some() || tick % 2 == 0;
                (id, cmd(sim, Vec2::ZERO, yaw, pitch, if held { buttons(&[Buttons::USE]) } else { Buttons::default() }))
            })
            .collect();
        sim.step(&cmds);
        events.extend_from_slice(&sim.events);
    }
    events
}

fn up_of(sim: &Sim, v: usize) -> f32 {
    (sim.vehicles[v].rot * Vec3::Y).y
}

fn time_to_right(roll: f32, lifters: usize) -> Option<f32> {
    let mut sim = yard();
    overturned(&mut sim, 0, roll);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let who: Vec<SoldierId> = (0..lifters).map(|i| at_hull(&mut sim, 100 + i as u32, 0, side * 2.2 + Vec3::X * 0.5 * i as f32)).collect();
    for step in 0..(40 * 4) {
        heaving(&mut sim, &who, 0, 0.25);
        if up_of(&sim, 0) > 0.95 && sim.vehicles[0].wheels.iter().filter(|w| w.on_ground).count() >= 3 {
            return Some(step as f32 * 0.25);
        }
    }
    None
}

#[test]
fn one_soldier_holding_f_rights_a_truck_on_its_side_slowly_and_it_goes_through_every_pose_between() {
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    let (yaw, pitch) = facing_hull(&sim, id, 0);
    assert!(matches!(sim.use_target_looking(id, view_dir(yaw, pitch)), Some(UseTarget::Lift { vehicle: 0 })));
    let (mut last, mut highest, mut fastest, mut groans, mut landed) = (up_of(&sim, 0), 0.0_f32, 0.0_f32, 0, 0);
    let (start, mut least) = (sim.soldiers[id].body.stamina, 1.0_f32);
    let mut came_up_at = None;
    for tick in 0..(12 * 60) {
        for event in heaving(&mut sim, &[id], 0, 1.0 / 60.0) {
            match event {
                SimEvent::Vehicle { event: VehicleEvent::Heave { .. }, .. } => groans += 1,
                SimEvent::Vehicle { event: VehicleEvent::Landed { .. }, .. } => landed += 1,
                _ => {}
            }
        }
        let up = up_of(&sim, 0);
        assert!((up - last).abs() < 0.05, "no teleporting: {last} to {up}");
        last = up;
        highest = highest.max(sim.vehicles[0].pos.y);
        fastest = fastest.max(sim.vehicles[0].vel.length());
        least = least.min(sim.soldiers[id].body.stamina);
        if tick > 60 && sim.soldiers[id].lifting.is_some() {
            assert!(sim.soldiers[id].anim.slung > 0.0 || sim.soldiers[id].weapon.ready > 0.0, "the gun goes away");
        }
        if came_up_at.is_none() && up > 0.95 {
            came_up_at = Some(tick as f32 / 60.0);
        }
    }
    let came_up_at = came_up_at.expect("it should be up");
    println!("up after {came_up_at} s, {groans} groans, highest {highest}, fastest {fastest}");
    assert!((4.0..10.0).contains(&came_up_at), "{came_up_at}");
    assert!(groans >= 2 && landed == 1, "{groans} {landed}");
    assert!(highest < 1.6 && fastest < 3.0, "no launching: {highest} {fastest}");
    assert!(sim.vehicles[0].wheels.iter().all(|w| w.on_ground));
    assert!(least < start - 0.1, "{least} from {start}");
    assert_eq!(sim.soldiers[id].lifting, None);
}

#[test]
fn off_its_roof_it_comes_onto_its_side_and_then_its_wheels() {
    let took = time_to_right(PI, 1).expect("it should come up");
    assert!((8.0..22.0).contains(&took), "{took}");
    assert!(time_to_right(PI, 1) > time_to_right(FRAC_PI_2, 1));
}

#[test]
fn two_are_faster_than_one_and_either_side_will_do() {
    let (one, two) = (time_to_right(FRAC_PI_2, 1).unwrap(), time_to_right(FRAC_PI_2, 2).unwrap());
    assert!(two < one * 0.7, "{two} against {one}");
    assert!(time_to_right(-FRAC_PI_2, 1).is_some());
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let (friend, foe) = (at_hull(&mut sim, 1, 0, side * 2.2), at_hull(&mut sim, 2, 0, side * 2.2 + Vec3::Z));
    sim.soldiers[foe].team = sim.soldiers[friend].team + 1;
    heaving(&mut sim, &[friend, foe], 0, 1.0);
    assert!(sim.soldiers[friend].lifting.is_some() && sim.soldiers[foe].lifting.is_some());
}

#[test]
fn an_armoured_car_takes_two_and_one_gets_nowhere() {
    let mut sim = yard_with(|level| level.vehicles[0].kind = VehicleTable::ARMOURED_CAR.0);
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let one = at_hull(&mut sim, 1, 0, side * 2.1);
    let was = up_of(&sim, 0);
    heaving(&mut sim, &[one], 0, 4.0);
    assert!((up_of(&sim, 0) - was).abs() < 0.02, "{}", up_of(&sim, 0));
    let two = at_hull(&mut sim, 2, 0, side * 2.1 + Vec3::Z * 0.8);
    heaving(&mut sim, &[one, two], 0, 16.0);
    assert!(up_of(&sim, 0) > 0.95, "{}", up_of(&sim, 0));
}

/// Holding F, they can shuffle about: a step along the hull and the flip goes on and finishes. Walking off, they stop
/// at arm's length and it still goes on, and only letting go ends it.
#[test]
fn moving_about_while_righting_it_holds_them_at_arms_length_and_never_lets_go() {
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    heaving(&mut sim, &[id], 0, 1.5);
    assert!(sim.soldiers[id].lifting.is_some());
    // Sideways, along the truck, for 0.3 m or so.
    let from = sim.soldiers[id].body.pos;
    for _ in 0..90 {
        let (yaw, pitch) = facing_hull(&sim, id, 0);
        let c = cmd(&sim, Vec2::new(1.0, 0.0), yaw, pitch, buttons(&[Buttons::USE]));
        sim.step(&[(id, c)]);
        if sim.soldiers[id].body.pos.distance(from) > 0.3 {
            break;
        }
    }
    assert!(sim.soldiers[id].body.pos.distance(from) > 0.25, "they moved: {}", sim.soldiers[id].body.pos.distance(from));
    assert!(sim.soldiers[id].lifting.is_some(), "and are still at it");
    // Straight back from it, for a good while.
    for _ in 0..120 {
        let (yaw, pitch) = facing_hull(&sim, id, 0);
        let c = cmd(&sim, Vec2::new(0.0, -1.0), yaw, pitch, buttons(&[Buttons::USE]));
        sim.step(&[(id, c)]);
    }
    let reach = sim.vehicle_cfgs[sim.vehicles[0].kind].lift.hold_within;
    let feet = sim.soldiers[id].body.pos;
    let nearest = sim.nearest_of_hull(0, feet + Vec3::Y * 0.5);
    assert!(Vec2::new(feet.x - nearest.x, feet.z - nearest.z).length() <= reach + 0.05, "held at arm's length");
    assert!(sim.soldiers[id].lifting.is_some(), "walking off is not letting go");
    heaving(&mut sim, &[id], 0, 10.0);
    assert!(up_of(&sim, 0) > 0.95, "and it still comes up: {}", up_of(&sim, 0));
    // Letting go is.
    let (mut sim, id, _) = part_way(1.0);
    let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    sim.step(&[(id, c)]);
    assert_eq!(sim.soldiers[id].lifting, None);
}

fn part_way(lift_for: f32) -> (Sim, SoldierId, f32) {
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    heaving(&mut sim, &[id], 0, lift_for);
    let up = up_of(&sim, 0);
    (sim, id, up)
}

fn falls_back(mut sim: Sim, up: f32, id: SoldierId, let_go: impl Fn(&mut Sim, SoldierId)) {
    assert!(up > 0.2, "{up}");
    let_go(&mut sim, id);
    for _ in 0..240 {
        let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
        assert!(sim.vehicles[0].vel.length() < 8.0 && sim.vehicles[0].pos.y < 2.0, "{:?}", sim.vehicles[0].vel);
    }
    assert_eq!(sim.soldiers[id].lifting, None);
    assert!(up_of(&sim, 0) < 0.1, "should have gone back down from {up}: {}", up_of(&sim, 0));
}

#[test]
fn let_go_part_way_and_it_falls_back_heavily() {
    let (sim, id, up) = part_way(2.5);
    falls_back(sim, up, id, |_, _| {});
}

#[test]
fn hit_part_way_and_they_let_go_and_it_falls_back() {
    let (sim, id, up) = part_way(2.5);
    falls_back(sim, up, id, |sim, id| {
        sim.knock(id, 5.0);
        assert_eq!(sim.soldiers[id].lifting, None, "as a bandage is");
    });
}

#[test]
fn jumping_or_running_out_of_breath_ends_it() {
    let (mut sim, id, _) = part_way(1.0);
    let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::USE, Buttons::JUMP]));
    sim.step(&[(id, c)]);
    assert_eq!(sim.soldiers[id].lifting, None, "a jump");
    let (mut sim, id, _) = part_way(1.0);
    sim.soldiers[id].body.stamina = 0.0;
    heaving(&mut sim, &[id], 0, 0.1);
    assert_eq!(sim.soldiers[id].lifting, None, "no breath");
}

#[test]
fn past_its_balance_it_goes_on_over_by_itself() {
    let (mut sim, id, _) = part_way(0.1);
    while up_of(&sim, 0) < sim.vehicle_cfgs[sim.vehicles[0].kind].lift.done_above - 0.02 {
        heaving(&mut sim, &[id], 0, 1.0 / 60.0);
    }
    let up = up_of(&sim, 0);
    for _ in 0..240 {
        let c = cmd(&sim, Vec2::ZERO, 0.0, 0.0, Buttons::default());
        sim.step(&[(id, c)]);
    }
    assert!(up_of(&sim, 0) > 0.95, "from {up}: {}", up_of(&sim, 0));
}

#[test]
fn a_wall_in_the_way_stops_it_and_the_prompt_says_so() {
    let mut sim = yard_with(|level| {
        // Close by its underside, which is the way it has to swing.
        level.solids.push(prs_level::Solid { center: Vec3::new(1.5, 1.5, 0.0), half: Vec3::new(0.15, 1.5, 8.0), rotation: Quat::IDENTITY, paint: prs_level::Paint::Concrete });
    });
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    let blocked = (0..8 * 60).any(|_| {
        heaving(&mut sim, &[id], 0, 1.0 / 60.0).iter().any(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::LiftBlocked, .. }))
    });
    println!("up {}, at {:?}", up_of(&sim, 0), sim.vehicles[0].pos);
    assert!(blocked);
    assert!(sim.soldiers[id].lifting.is_none() && sim.soldiers[id].lift_refused > 0.0);
    heaving(&mut sim, &[id], 0, 0.5);
    assert!(sim.soldiers[id].lifting.is_none(), "and nobody tries again while it says so");
    assert!(sim.vehicles[0].pos.x < 1.35, "and through nothing: {}", sim.vehicles[0].pos.x);
    assert!(up_of(&sim, 0) < 0.7, "{}", up_of(&sim, 0));
}

#[test]
fn nobody_in_a_seat_can_lift_and_f_there_still_gets_out() {
    let mut sim = yard();
    let id = seated(&mut sim, 1, 0);
    overturned(&mut sim, 0, FRAC_PI_2);
    assert_eq!(sim.soldiers[id].seat.map(|at| at.carrier), Some(0));
    assert!(!sim.vehicles[0].engine.running, "over like that the engine stops");
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    assert!(!matches!(sim.use_target_looking(id, view_dir(yaw, pitch)), Some(UseTarget::Lift { .. })));
    press(&mut sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].lifting, None);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), TRANSIT_TICKS);
    assert_eq!(sim.soldiers[id].seat, None, "out");
}

#[test]
fn f_at_the_door_of_one_on_its_wheels_is_still_the_door() {
    let mut sim = yard();
    let id = beside_seat(&mut sim, 1, 0);
    press(&mut sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].lifting, None);
    assert!(sim.soldiers[id].seat.is_some());
}

#[test]
fn being_worked_on_it_is_not_written_off() {
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    sim.vehicles[0].abandoned_for = 1000.0;
    heaving(&mut sim, &[id], 0, 0.1);
    assert_eq!(sim.vehicles[0].abandoned_for, 0.0);
}

#[test]
#[ignore]
fn lift_report() {
    for (what, roll) in [("side", FRAC_PI_2), ("other side", -FRAC_PI_2), ("roof", PI)] {
        for lifters in [1, 2, 3] {
            println!("LIFT truck from its {what}, {lifters}: {:?} s", time_to_right(roll, lifters));
        }
    }
    let mut sim = yard();
    overturned(&mut sim, 0, FRAC_PI_2);
    println!("LIFT lies at {:?} up {:?}", sim.vehicles[0].pos, sim.vehicles[0].rot * Vec3::Y);
    let side = sim.vehicles[0].rot * Vec3::Y;
    let id = at_hull(&mut sim, 7, 0, side * 2.2);
    let (start, stamina) = (sim.vehicles[0].pos, sim.soldiers[id].body.stamina);
    println!("LIFT target {:?}", sim.use_target_looking(id, { let (y, p) = facing_hull(&sim, id, 0); crate::view_dir(y, p) }));
    for second in 0..12 {
        let events = heaving(&mut sim, &[id], 0, 1.0);
        let v = &sim.vehicles[0];
        println!("LIFT t {} up {:.2} pos {:?} ang {:.2} vel {:.2} lifting {:?} moved {:.2} stamina {:.2} soldier {:?} events {}", second + 1, up_of(&sim, 0), v.pos, v.ang_vel.length(), v.vel.length(), sim.soldiers[id].lifting, v.pos.distance(start), stamina - sim.soldiers[id].body.stamina, sim.soldiers[id].body.pos, events.iter().filter(|e| matches!(e, SimEvent::Vehicle { .. })).count());
    }
}

/// Nobody told to look anywhere goes on looking the same way out of their seat.
#[test]
fn someone_given_no_look_turns_with_the_truck() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    let idle = seated(&mut sim, 3, 3);
    let relative = |sim: &Sim| wrapped(sim.soldiers[idle].yaw - sim.vehicles[0].frame().yaw());
    let was = relative(&sim);
    let start = sim.vehicles[0].frame().yaw();
    for _ in 0..(5 * 60) {
        let c = cmd(&sim, Vec2::new(1.0, 1.0), sim.vehicles[0].frame().yaw(), 0.0, Buttons::default());
        sim.step(&[(driver, c)]);
    }
    assert!(wrapped(sim.vehicles[0].frame().yaw() - start).abs() > 0.5);
    assert!((relative(&sim) - was).abs() < 0.03, "{} from {was}", relative(&sim));
}

fn wrapped(a: f32) -> f32 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// Firing out of the side while the truck goes hard round a corner and the look goes with it: every round leaves the real
/// muzzle, and none of them goes into the truck.
#[test]
fn a_passenger_shooting_in_a_hard_turn_fires_from_the_real_muzzle_and_misses_their_own_truck() {
    let mut sim = yard();
    let driver = seated(&mut sim, 1, 0);
    let id = seated(&mut sim, 2, 3);
    let (relative, pitch) = (-FRAC_PI_2, 0.02);
    let mut fired = 0;
    for tick in 0..(5 * 60) {
        let yaw = sim.vehicles[0].frame().yaw();
        let press = if tick > 60 && tick % 12 == 0 { buttons(&[Buttons::FIRE, Buttons::ADS]) } else { buttons(&[Buttons::ADS]) };
        let (a, b) = (cmd(&sim, Vec2::new(1.0, 1.0), yaw, 0.0, Buttons::default()), cmd(&sim, Vec2::ZERO, yaw + relative, pitch, press));
        let muzzle = sim.soldiers[id].gun_pose(&sim.weapons).muzzle;
        sim.step(&[(driver, a), (id, b)]);
        for event in &sim.events {
            assert!(!matches!(event, SimEvent::VehicleHit { .. }), "hit their own truck at {tick}: {event:?}");
            if let SimEvent::ShotFired { origin, .. } = event {
                fired += 1;
                assert!(origin.distance(muzzle) < 0.6, "{origin} a tick after the gun was at {muzzle}");
            }
        }
    }
    assert!(fired >= 5, "{fired}");
}

#[test]
fn a_new_round_puts_every_vehicle_back_where_it_was_parked() {
    let mut sim = yard();
    let home = sim.vehicles[0].pos;
    sim.vehicles[0].pos += Vec3::new(40.0, 0.0, 25.0);
    sim.vehicles[0].damage.hull = 1.0;
    sim.reset_rides();
    assert_eq!(sim.vehicles[0].pos, sim.vehicles[0].home.pos);
    assert!(sim.vehicles[0].pos.distance(home) < 0.5, "back at its spot, not where it was driven to");
    assert!(sim.vehicles[0].damage.hull > 1.0, "and new");
}

// ---- Pushing a vehicle by hand.

/// A yard with one vehicle of this kind, and something under it, if anything.
fn yard_of(kind: u8, under: Option<prs_level::Solid>) -> Sim {
    let mut level = Level::flat(1000.0);
    level.vehicles.push(prs_level::VehicleSpawn { kind, pos: Vec3::ZERO, yaw: 0.0 });
    level.solids.extend(under);
    let mut sim = Sim::new(level);
    for _ in 0..240 {
        sim.step(&[]);
    }
    sim
}

/// Someone stood off one of its spots, back from it this far, facing it.
fn at_spot(sim: &mut Sim, seed: u32, v: usize, spot: usize, back: f32) -> SoldierId {
    let vehicle = &sim.vehicles[v];
    let cfg = &sim.vehicle_cfgs[vehicle.kind];
    let at = vehicle.pos + vehicle.rot * cfg.push.spots[spot];
    let out = (vehicle.rot * (cfg.push.spots[spot] * Vec3::new(0.0, 0.0, 1.0))).normalize_or(Vec3::Z);
    let feet = Vec3::new(at.x + out.x * back, 0.0, at.z + out.z * back);
    let id = sim.spawn_soldier(seed);
    sim.soldiers[id].body.pos = Vec3::new(feet.x, 0.02, feet.z);
    id
}

fn facing_spot(sim: &Sim, id: SoldierId, v: usize, spot: usize) -> (f32, f32) {
    let vehicle = &sim.vehicles[v];
    let at = vehicle.pos + vehicle.rot * sim.vehicle_cfgs[vehicle.kind].push.spots[spot];
    let to = (at - sim.soldiers[id].eye()).normalize_or(Vec3::NEG_Z);
    ((-to.x).atan2(-to.z), to.y.asin())
}

/// These holding F at these spots for this long, walking with it as they go.
fn pushing(sim: &mut Sim, who: &[(SoldierId, usize)], v: usize, seconds: f32, extra: &[(SoldierId, PlayerCmd)]) -> Vec<SimEvent> {
    let mut events = Vec::new();
    for tick in 0..(seconds * 60.0) as u32 {
        let mut cmds: Vec<(SoldierId, PlayerCmd)> = who
            .iter()
            .map(|&(id, spot)| {
                let (yaw, pitch) = facing_spot(sim, id, v, spot);
                let held = sim.soldiers[id].pushing.is_some() || tick % 2 == 0;
                let walk = if sim.soldiers[id].pushing.is_some() { Vec2::Y } else { Vec2::ZERO };
                (id, cmd(sim, walk, yaw, pitch, if held { buttons(&[Buttons::USE]) } else { Buttons::default() }))
            })
            .collect();
        cmds.extend(extra.iter().map(|&(id, mut c)| {
            c.tick = sim.tick;
            (id, c)
        }));
        sim.step(&cmds);
        events.extend_from_slice(&sim.events);
    }
    events
}

/// A kerb across the way under the middle of whatever is parked over it.
fn kerb(height: f32) -> prs_level::Solid {
    prs_level::Solid { center: Vec3::new(0.0, height * 0.5, 0.0), half: Vec3::new(4.0, height * 0.5, 0.3), rotation: Quat::IDENTITY, paint: prs_level::Paint::Concrete }
}

/// How high off the ground its underside is, parked.
fn belly(kind: u8) -> f32 {
    let sim = yard_of(kind, None);
    let vehicle = &sim.vehicles[0];
    let hull = sim.vehicle_cfgs[vehicle.kind].hull[0];
    vehicle.pos.y + hull.center.y - hull.half.y
}

/// Pushing from behind, the back spots: how far it went in this long.
fn pushed_by(kind: u8, under: Option<prs_level::Solid>, pushers: usize, seconds: f32) -> (f32, bool) {
    let mut sim = yard_of(kind, under);
    let from = sim.vehicles[0].pos;
    if pushers == 1 {
        println!("PUSH kind {kind} sits with {} wheels down, load {:.0}", sim.vehicles[0].wheels.iter().filter(|w| w.on_ground).count(), sim.vehicles[0].wheels.iter().map(|w| w.load).sum::<f32>());
    }
    let spots = [4, 3, 5, 4, 3, 5, 3, 5, 4];
    let who: Vec<(SoldierId, usize)> = (0..pushers).map(|i| (at_spot(&mut sim, 10 + i as u32, 0, spots[i], 0.75 + 0.55 * (i / 3) as f32), spots[i])).collect();
    pushing(&mut sim, &who, 0, seconds, &[]);
    let all_down = sim.vehicles[0].wheels.iter().all(|w| w.on_ground);
    (Vec2::new(sim.vehicles[0].pos.x - from.x, sim.vehicles[0].pos.z - from.z).length(), all_down)
}

#[test]
fn one_soldier_frees_a_jeep_hung_up_on_a_hump_and_rolls_it_slowly_on_the_flat() {
    let (went, down) = pushed_by(2, Some(kerb(belly(2))), 1, 8.0);
    assert!(went > 2.0 && down, "{went} {down}");
    let (rolled, _) = pushed_by(2, None, 1, 5.0);
    assert!((2.0..6.0).contains(&rolled), "at a fair walk: {rolled}");
}

#[test]
fn anyone_moves_anything_and_more_hands_free_the_heavy_ones() {
    let freed = |kind, n| {
        let (went, down) = pushed_by(kind, Some(kerb(belly(kind))), n, 8.0);
        went > 2.0 && down
    };
    assert!(freed(0, 2), "two free a truck");
    assert!(freed(1, 3), "and three an armoured car");
    // On the flat one alone moves even seven and a half tonnes on grass, at a crawl, and two go faster.
    let (truck, car, cars) = (pushed_by(0, None, 1, 5.0).0, pushed_by(1, None, 1, 5.0).0, pushed_by(1, None, 2, 5.0).0);
    assert!((1.0..4.0).contains(&truck), "a truck, slowly: {truck}");
    assert!((0.3..1.5).contains(&car), "a crawl: {car}");
    assert!(cars > car * 1.5, "{cars} against {car}");
}

fn pushed_jeep() -> (Sim, SoldierId) {
    let mut sim = yard_of(2, None);
    let id = at_spot(&mut sim, 10, 0, 4, 0.75);
    pushing(&mut sim, &[(id, 4)], 0, 1.0, &[]);
    assert!(sim.soldiers[id].pushing.is_some());
    (sim, id)
}

/// Leaning in, they can step about at the back of it; walking off they stop with their arms out and go on pushing.
#[test]
fn moving_about_while_pushing_holds_them_at_arms_length() {
    let (mut sim, id) = pushed_jeep();
    for wish in [Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0)] {
        for _ in 0..90 {
            let (yaw, pitch) = facing_spot(&sim, id, 0, 4);
            let c = cmd(&sim, wish, yaw, pitch, buttons(&[Buttons::USE]));
            sim.step(&[(id, c)]);
        }
        assert!(sim.soldiers[id].pushing.is_some(), "still at it after {wish}");
        let spot = sim.vehicles[0].pos + sim.vehicles[0].rot * sim.vehicle_cfgs[sim.vehicles[0].kind].push.spots[4];
        let feet = sim.soldiers[id].body.pos;
        assert!(Vec2::new(feet.x - spot.x, feet.z - spot.z).length() < 0.9, "within reach: {feet} and {spot}");
    }
}

#[test]
fn letting_go_being_hit_or_going_prone_ends_a_push_and_it_comes_to_rest_and_sleeps() {
    let (mut sim, id) = pushed_jeep();
    sim.knock(id, 5.0);
    assert_eq!(sim.soldiers[id].pushing, None, "as a bandage is");
    let (mut sim2, id2) = pushed_jeep();
    hold(&mut sim2, id2, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::USE, Buttons::PRONE]), 1);
    hold(&mut sim2, id2, Vec2::ZERO, 0.0, 0.0, buttons(&[Buttons::USE]), 30);
    assert_eq!(sim2.soldiers[id2].pushing, None, "prone");
    let (yaw, pitch) = facing_spot(&sim, id, 0, 4);
    hold(&mut sim, id, Vec2::ZERO, yaw, pitch, Buttons::default(), 240);
    assert!(sim.vehicles[0].asleep, "with nobody on it, it settles and sleeps again");
}

/// Somebody at the wheel puts it in reverse into the one pushing the back: they get out of its way, and whatever
/// it does to them is the same as any vehicle backing into anyone.
#[test]
fn a_vehicle_coming_back_at_whoever_pushes_it_makes_them_let_go() {
    let (mut sim, id) = pushed_jeep();
    let driver = sim.spawn_soldier(3);
    sim.sit_in(driver, SeatedAt { carrier: 0, seat: 0 });
    let mut reverse = cmd(&sim, Vec2::new(0.0, -1.0), sim.vehicles[0].frame().yaw(), 0.0, Buttons::default());
    reverse.tick = sim.tick;
    let mut let_go = None;
    for tick in 0..120 {
        pushing(&mut sim, &[(id, 4)], 0, 1.0 / 60.0, &[(driver, reverse)]);
        if let_go.is_none() && sim.soldiers[id].pushing.is_none() {
            let_go = Some(tick);
        }
    }
    assert!(let_go.is_some_and(|t| t < 60), "{let_go:?}");
}

/// Driving off while they push is fine: it goes away from them, and they keep up as far as they can.
#[test]
fn the_driver_can_rev_while_they_push() {
    let mut sim = yard_of(0, Some(kerb(belly(0))));
    let driver = sim.spawn_soldier(3);
    sim.sit_in(driver, SeatedAt { carrier: 0, seat: 0 });
    let a = at_spot(&mut sim, 10, 0, 4, 0.75);
    let from = sim.vehicles[0].pos;
    let rev = cmd(&sim, Vec2::new(0.0, 1.0), sim.vehicles[0].frame().yaw(), 0.0, Buttons::default());
    pushing(&mut sim, &[(a, 4)], 0, 6.0, &[(driver, rev)]);
    assert!(sim.vehicles[0].pos.distance(from) > 3.0, "{}", sim.vehicles[0].pos.distance(from));
}

#[test]
fn nobody_in_a_seat_pushes_and_f_at_a_door_is_still_the_door() {
    let mut sim = yard_of(0, None);
    let id = beside_seat(&mut sim, 1, 0);
    press(&mut sim, id, Buttons::USE);
    assert_eq!(sim.soldiers[id].pushing, None);
    assert!(sim.soldiers[id].seat.is_some());
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    assert!(!matches!(sim.use_target_looking(id, view_dir(yaw, pitch)), Some(UseTarget::Push { .. })));
}

/// Seen from outside, someone pushing leans well in from the ankles with both hands on the vehicle at the spot.
#[test]
fn a_pusher_leans_in_with_both_hands_on_it() {
    for kind in [0u8, 1, 2] {
        let mut sim = yard_of(kind, None);
        let spot = 4;
        let id = at_spot(&mut sim, 10, 0, spot, 1.2);
        pushing(&mut sim, &[(id, spot)], 0, 2.0, &[]);
        assert!(sim.soldiers[id].pushing.is_some());
        let input = sim.body_input(id);
        let hands = input.push.expect("hands on it");
        let world = &sim.world;
        let pose = prs_body::solve(&sim.body_cfg, &input, &|at: Vec3| {
            world.raycast(at + Vec3::Y * 2.0, Vec3::NEG_Y, 4.0).map(|hit| prs_body::GroundHit { height: at.y + 2.0 - hit.distance, normal: hit.normal })
        });
        let (left, right) = (pose[prs_body::Bone::HandL].end(), pose[prs_body::Bone::HandR].end());
        let ankle = (pose[prs_body::Bone::FootL].start + pose[prs_body::Bone::FootR].start) * 0.5;
        let head = pose[prs_body::Bone::Head].start;
        let ahead = Vec2::new(head.x - ankle.x, head.z - ankle.z).length();
        let lean = ahead.atan2(head.y - ankle.y).to_degrees();
        println!("kind {kind}: hands off by {:.2} and {:.2} m, leaning {lean:.0} degrees from the ankles", left.distance(hands[0]), right.distance(hands[1]));
        assert!(left.distance(hands[0]) < 0.05 && right.distance(hands[1]) < 0.05, "kind {kind}: {left} {right} for {hands:?}");
        assert!((25.0..50.0).contains(&lean), "kind {kind}: {lean}");
    }
}

#[test]
#[ignore]
fn push_report() {
    for (name, kind) in [("truck", 0), ("car", 1), ("jeep", 2)] {
        let flat: Vec<String> = (1..=4).map(|n| format!("{:.1}", pushed_by(kind, None, n, 5.0).0)).collect();
        let needs = |over: f32| (1..=6).find(|&n| { let (went, down) = pushed_by(kind, Some(kerb(belly(kind) + over)), n, 8.0); went > 2.0 && down });
        let rocked = pushed_by(kind, Some(kerb(belly(kind))), 1, 8.0).0;
        println!("PUSH {name}: metres in 5 s on flat grass by 1..4: {} | off a hump just touching by {:?}, 3 cm up by {:?}; one alone on it moves it {rocked:.2} m in 8 s", flat.join(" "), needs(0.0), needs(0.03));
    }
}
