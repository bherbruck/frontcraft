//! Helicopters through the whole game: people getting in, commands flying it, bullets breaking it.

use super::*;
use crate::copters::HELI_CARRIERS;
use prs_helicopter::autopilot::{Autopilot, Order};
use prs_helicopter::{Engine, HeliEvent, HeliZone, VirtualStick};
use prs_level::SpawnPoint;
use prs_seats::SeatedAt;

fn sim_with_helicopter() -> Sim {
    let mut level = Level::flat(1000.0);
    level.helipads.push(SpawnPoint { pos: Vec3::ZERO, yaw: 0.0 });
    let mut sim = Sim::new(level);
    sim.step(&[]);
    sim
}

/// A soldier put straight into a seat. Going through the doors is tested in `rides_tests`.
fn seated(sim: &mut Sim, seed: u32, seat: usize) -> SoldierId {
    let id = sim.spawn_soldier(seed);
    sim.sit_in(id, SeatedAt { carrier: HELI_CARRIERS, seat: seat as u8 });
    assert_eq!(sim.soldiers[id].seat, Some(SeatedAt { carrier: HELI_CARRIERS, seat: seat as u8 }), "should have got in");
    id
}

fn up_to_speed(sim: &mut Sim, pilot: SoldierId) {
    let mut buttons = Buttons::default();
    buttons.set(Buttons::JUMP, true);
    sim.step(&[(pilot, PlayerCmd { tick: sim.tick, buttons, ..Default::default() })]);
    assert_eq!(sim.helicopters[0].state.engine, Engine::Starting);
    while sim.helicopters[0].state.rotor_fraction(&sim.heli_cfg) < 0.97 {
        sim.step(&[(pilot, PlayerCmd { tick: sim.tick, ..Default::default() })]);
        assert!(sim.tick < 60 * 40);
    }
}

/// Flies what the order says for a while, through nothing but commands.
fn fly(sim: &mut Sim, pilot: SoldierId, autopilot: &mut Autopilot, seconds: f32, order: impl Fn(&Sim) -> Order) {
    for _ in 0..(seconds / TICK_DT) as u32 {
        let wanted = order(sim);
        let input = autopilot.fly(&sim.helicopters[0].state, &wanted, TICK_DT);
        let cmd = PlayerCmd {
            tick: sim.tick,
            wish_move: Vec2::new(input.pedals, input.collective),
            stick: VirtualStick { pos: input.stick }.quantised(),
            ..Default::default()
        };
        sim.step(&[(pilot, cmd)]);
    }
}

fn stay_at(sim: &Sim, height: f32) -> Order {
    let state = &sim.helicopters[0].state;
    Order { velocity: Vec2::ZERO, climb: (height - state.pos.y) * 0.25, heading: 0.0, lean_limit: 0.12 }
}

fn in_the_air(sim: &mut Sim, pilot: SoldierId, height: f32) {
    let mut autopilot = Autopilot::default();
    up_to_speed(sim, pilot);
    fly(sim, pilot, &mut autopilot, 20.0, |sim| stay_at(sim, height));
}

#[test]
fn a_pilot_flies_it_with_nothing_but_commands() {
    let mut sim = sim_with_helicopter();
    let pilot = seated(&mut sim, 1, 0);
    in_the_air(&mut sim, pilot, 15.0);
    let state = &sim.helicopters[0].state;
    assert!((state.pos.y - 15.0).abs() < 2.0, "at {}", state.pos);
    // And sat where the seat says, up there in the air.
    let eye = sim.helicopters[0].frame().point(sim.heli_cfg.seats[0].eye);
    assert!(sim.soldiers[pilot].eye().distance(eye) < 0.2, "eye {} against {eye}", sim.soldiers[pilot].eye());
}

#[test]
fn someone_in_the_back_cannot_fly_it() {
    let mut sim = sim_with_helicopter();
    let rider = seated(&mut sim, 2, 2);
    let mut buttons = Buttons::default();
    buttons.set(Buttons::JUMP, true);
    sim.step(&[(rider, PlayerCmd { tick: sim.tick, buttons, ..Default::default() })]);
    assert_eq!(sim.helicopters[0].state.engine, Engine::Off);
}

#[test]
fn a_flying_helicopter_restored_from_an_export_stays_identical() {
    let mut original = sim_with_helicopter();
    let pilot = seated(&mut original, 1, 0);
    in_the_air(&mut original, pilot, 12.0);

    let state = original.export_state();
    let mut level = Level::flat(1000.0);
    level.helipads.push(SpawnPoint { pos: Vec3::ZERO, yaw: 0.0 });
    let mut copy = Sim::new(level);
    copy.import_state(&state);
    assert_eq!(copy.export_state(), state);

    let mut stick = VirtualStick::default();
    for i in 0..300 {
        stick.pos = Vec2::new((i as f32 * 0.03).sin(), (i as f32 * 0.021).cos()) * 0.2;
        let cmd = PlayerCmd { tick: original.tick, wish_move: Vec2::new(0.1, 0.0), stick: stick.quantised(), ..Default::default() };
        original.step(&[(pilot, cmd)]);
        copy.step(&[(pilot, cmd)]);
        assert_eq!(original.helicopters, copy.helicopters, "tick {i}");
    }
}

#[test]
fn a_client_predicting_its_pilot_keeps_up_with_the_server() {
    let mut server = sim_with_helicopter();
    let pilot = seated(&mut server, 1, 0);
    up_to_speed(&mut server, pilot);
    let mut level = Level::flat(1000.0);
    level.helipads.push(SpawnPoint { pos: Vec3::ZERO, yaw: 0.0 });
    let mut client = Sim::new(level);
    client.import_state(&server.export_state());
    for _ in 0..600 {
        let tick = server.tick + 1;
        let cmd = PlayerCmd { tick, wish_move: Vec2::new(0.0, 0.5), stick: [5, -3], ..Default::default() };
        server.step(&[(pilot, cmd)]);
        client.step_predicted(pilot, &cmd);
        assert_eq!(server.helicopters[0].state, client.helicopters[0].state);
    }
    assert!(server.helicopters[0].state.pos.y > 2.0, "should have lifted off");
}

/// A bullet from `from` along `dir`, shot by someone standing far away.
fn shoot(sim: &mut Sim, shooter: SoldierId, from: Vec3, dir: Vec3, seconds: f32) {
    let weapon = sim.soldiers[shooter].weapon.weapon;
    let bullet = Bullet::new(from, dir, &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
    for _ in 0..(seconds / TICK_DT) as u32 {
        sim.step(&[]);
    }
}

#[test]
fn bullets_break_what_they_hit() {
    let mut sim = sim_with_helicopter();
    let shooter = sim.spawn_soldier(9);
    let pos = sim.helicopters[0].state.pos;
    let zone_shot = |sim: &mut Sim, at: Vec3| {
        let before = sim.helicopters[0].state.health;
        shoot(sim, shooter, pos + at + Vec3::X * 15.0, Vec3::NEG_X, 0.1);
        (before, sim.helicopters[0].state.health)
    };
    let (before, after) = zone_shot(&mut sim, Vec3::new(0.0, 1.3, 1.3));
    assert!(after.engine < before.engine, "the engine");
    let (before, after) = zone_shot(&mut sim, Vec3::new(0.0, 0.8, 9.0));
    assert!(after.tail < before.tail, "the tail rotor");
}

#[test]
fn enough_hits_on_the_tail_rotor_lose_it_and_it_spins_in_the_air() {
    let mut sim = sim_with_helicopter();
    let pilot = seated(&mut sim, 1, 0);
    in_the_air(&mut sim, pilot, 40.0);
    let hp = sim.heli_cfg.damage.tail_hp;
    sim.bullet_into_helicopter(0, HeliZone::TailRotor, hp + 1.0);
    assert!(sim.events.iter().any(|e| matches!(e, SimEvent::Heli { event: HeliEvent::TailRotorLost, .. })));
    let mut autopilot = Autopilot::default();
    fly(&mut sim, pilot, &mut autopilot, 4.0, |sim| stay_at(sim, 40.0));
    assert!(sim.helicopters[0].state.spin.y.abs() > 1.0, "spinning at {}", sim.helicopters[0].state.spin.y);
}

#[test]
fn a_door_gunner_fires_from_the_real_gun_out_of_the_door_and_nowhere_else() {
    let mut sim = sim_with_helicopter();
    let gunner = seated(&mut sim, 3, 3);
    let frame = sim.carrier_frame(HELI_CARRIERS);
    let eye = frame.point(sim.heli_cfg.seats[3].eye);
    // Right door: out to the right of the airframe, which is yaw -90 degrees from its nose.
    let mut buttons = Buttons::default();
    buttons.set(Buttons::FIRE, true);
    let out = -std::f32::consts::FRAC_PI_2;
    for _ in 0..90 {
        sim.step(&[(gunner, PlayerCmd { tick: sim.tick, yaw: out, pitch: -0.05, ..Default::default() })]);
    }
    let mut fired = None;
    for _ in 0..30 {
        sim.step(&[(gunner, PlayerCmd { tick: sim.tick, yaw: out, pitch: -0.05, buttons, ..Default::default() })]);
        for event in &sim.events {
            if let SimEvent::ShotFired { soldier, origin, dir, .. } = event
                && *soldier == gunner
            {
                fired = Some((*origin, *dir));
            }
        }
    }
    let (origin, dir) = fired.expect("the gun should bear out of the door");
    assert!(origin.distance(eye) < 1.5, "fired from {origin} with the eye at {eye}");
    assert!(origin.x > eye.x, "the muzzle is out of the door: {origin}");
    assert!(dir.x > 0.95, "going {dir}");

    // Straight forward along the fuselage it will not bear.
    for _ in 0..30 {
        sim.step(&[(gunner, PlayerCmd { tick: sim.tick, yaw: 0.0, pitch: 0.0, ..Default::default() })]);
    }
    let rounds = sim.soldiers[gunner].weapon.ammo;
    for _ in 0..30 {
        sim.step(&[(gunner, PlayerCmd { tick: sim.tick, yaw: 0.0, pitch: 0.0, buttons, ..Default::default() })]);
    }
    assert_eq!(sim.soldiers[gunner].weapon.ammo, rounds, "nothing fired into the cockpit");
}

#[test]
fn a_bullet_through_the_windscreen_finds_the_pilot() {
    let mut sim = sim_with_helicopter();
    let pilot = seated(&mut sim, 1, 0);
    let shooter = sim.spawn_soldier(9);
    sim.set_home(shooter, Vec3::new(0.0, 0.0, -80.0), 0.0);
    sim.step(&[]);
    let health = sim.soldiers[pilot].health.health;
    let chest = sim.helicopters[0].frame().point(Vec3::new(0.65, 0.1, -2.3));
    let from = chest + Vec3::new(0.0, 0.0, -30.0);
    shoot(&mut sim, shooter, from, (chest - from).normalize(), 0.1);
    assert!(sim.soldiers[pilot].health.health < health, "pilot untouched at {}", sim.soldiers[pilot].health.health);
    assert!(sim.helicopters[0].state.health.cockpit < sim.heli_cfg.damage.cockpit_hp);
}

#[test]
fn coming_down_hard_hurts_who_is_in_it_and_a_wreck_lets_them_go_and_comes_back() {
    let mut sim = sim_with_helicopter();
    let pilot = seated(&mut sim, 1, 0);
    sim.helicopters[0].state.pos.y = 30.0;
    sim.helicopters[0].state.parked = false;
    sim.helicopters[0].state.on_ground = false;
    sim.helicopters[0].state.skids = 0;
    for _ in 0..240 {
        sim.step(&[]);
    }
    assert!(sim.helicopters[0].state.wrecked);
    assert_eq!(sim.soldiers[pilot].seat, None);
    assert!(sim.soldiers[pilot].health.health < sim.health_cfg.max_health, "{}", sim.soldiers[pilot].health.health);
    let again = (sim.heli_cfg.damage.respawn_after / TICK_DT) as u32 + 10;
    for _ in 0..again {
        sim.step(&[]);
    }
    let state = &sim.helicopters[0].state;
    assert!(!state.wrecked && state.parked && state.health.structure == sim.heli_cfg.damage.structure_hp);
}

#[test]
fn there_is_a_helicopter_on_each_side_of_the_town_and_on_the_range() {
    let town = Sim::new(Level::named("town").unwrap());
    assert_eq!(town.helicopters.len(), 2);
    let range = Sim::new(Level::named("range").unwrap());
    assert_eq!(range.helicopters.len(), 1);
}

#[test]
fn a_shot_is_judged_against_the_helicopter_where_the_shooter_saw_it() {
    let mut sim = sim_with_helicopter();
    sim.remember_past_ticks(40);
    let shooter = sim.spawn_soldier(9);
    sim.set_home(shooter, Vec3::new(200.0, 0.1, 0.0), 0.0);
    let state = &mut sim.helicopters[0].state;
    state.pos.y = 300.0;
    state.engine = Engine::Running;
    state.spool = 1.0;
    state.rotor_speed = 33.9;
    state.collective = 0.45;
    state.parked = false;
    state.on_ground = false;
    state.skids = 0;
    state.vel = Vec3::new(0.0, 0.0, -40.0);
    const LAG: u32 = 24;
    sim.set_view_lag(shooter, LAG);
    for _ in 0..LAG + 2 {
        sim.step(&[]);
    }
    let then = sim.past_carrier_frame(HELI_CARRIERS, LAG).expect("on record");
    let now = sim.carrier_frame(HELI_CARRIERS);
    assert!(then.pos.distance(now.pos) > 14.0, "it should have moved on by {}", then.pos.distance(now.pos));

    // From just off the side of it, at where the cabin was on the shooter's screen.
    let from = then.point(Vec3::new(5.0, 0.0, 0.0));
    let weapon = sim.soldiers[shooter].weapon.weapon;
    let at_seen = (then.point(Vec3::ZERO) - from).normalize();
    let bullet = Bullet::new(from, at_seen, &sim.weapons[weapon].ballistics);
    let structure = sim.helicopters[0].state.health.structure;
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: LAG });
    sim.step(&[]);
    assert!(sim.helicopters[0].state.health.structure < structure, "a shot at what was on screen hits");

    // The same line with no lag finds nothing there.
    let structure = sim.helicopters[0].state.health.structure;
    let then = sim.past_carrier_frame(HELI_CARRIERS, LAG).expect("on record");
    let from = then.point(Vec3::new(5.0, 0.0, 0.0));
    let bullet = Bullet::new(from, (then.point(Vec3::ZERO) - from).normalize(), &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
    sim.step(&[]);
    assert_eq!(sim.helicopters[0].state.health.structure, structure, "that was where it no longer is");
}

#[test]
fn walking_into_a_turning_tail_rotor_hurts_and_a_stopped_one_does_not() {
    let mut sim = sim_with_helicopter();
    let walker = sim.spawn_soldier(4);
    let hub = sim.helicopters[0].frame().point(sim.heli_cfg.tail_rotor_hub());
    sim.set_home(walker, Vec3::new(hub.x, 0.05, hub.z), 0.0);
    for _ in 0..30 {
        sim.step(&[]);
    }
    assert_eq!(sim.soldiers[walker].health.health, sim.health_cfg.max_health, "a stopped rotor is just metal");
    sim.helicopters[0].state.rotor_speed = sim.heli_cfg.rotor.nominal_speed;
    sim.helicopters[0].state.engine = Engine::Running;
    sim.helicopters[0].state.spool = 1.0;
    for _ in 0..30 {
        sim.step(&[]);
    }
    assert!(sim.soldiers[walker].health.health < sim.health_cfg.max_health * 0.5, "{}", sim.soldiers[walker].health.health);
}

#[test]
fn nobody_gets_in_from_under_a_high_hover_and_getting_out_low_hurts_by_the_drop() {
    let mut sim = sim_with_helicopter();
    let walker = sim.spawn_soldier(5);
    let frame = sim.carrier_frame(HELI_CARRIERS);
    let door = frame.point(sim.heli_cfg.entries[3].stand);
    sim.set_home(walker, Vec3::new(door.x, 0.05, door.z), std::f32::consts::FRAC_PI_2);
    let state = &mut sim.helicopters[0].state;
    state.pos.y = 25.0;
    assert!(!sim.heli_at_door_height(0), "too high to reach");
    sim.helicopters[0].state.pos.y = 1.65;
    assert!(sim.heli_at_door_height(0));

    let rider = seated(&mut sim, 6, 3);
    let state = &mut sim.helicopters[0].state;
    state.pos.y = 1.65 + 2.0;
    state.on_ground = false;
    state.parked = false;
    let feet = Vec3::new(state.pos.x, 0.0, state.pos.z);
    let before = sim.soldiers[rider].health.health;
    sim.jump_out_hurts(rider, 0, feet);
    assert!(sim.soldiers[rider].health.health < before, "a two metre drop off a hover");
}

/// F at a door, by the same button anyone uses, facing it.
fn press_use(sim: &mut Sim, id: SoldierId, yaw: f32, pitch: f32) {
    let mut buttons = Buttons::default();
    buttons.set(Buttons::USE, true);
    sim.step(&[(id, PlayerCmd { tick: sim.tick, yaw, pitch, buttons, ..Default::default() })]);
    sim.step(&[(id, PlayerCmd { tick: sim.tick, yaw, pitch, ..Default::default() })]);
}

/// Stands someone at this door, facing its handle.
fn at_door(sim: &mut Sim, seed: u32, entry: usize) -> (SoldierId, f32, f32) {
    let id = sim.spawn_soldier(seed);
    let frame = sim.carrier_frame(HELI_CARRIERS);
    let door = sim.heli_cfg.entries[entry].clone();
    let stand = frame.point(door.stand);
    let spot = Vec3::new(stand.x, 0.05, stand.z);
    sim.set_home(id, spot, 0.0);
    let to = frame.point(door.hand) - sim.soldiers[id].eye();
    (id, (-to.x).atan2(-to.z), (to.y / to.length()).asin())
}

#[test]
fn getting_in_takes_a_walk_to_the_right_door_and_time_and_you_can_be_shot_doing_it() {
    let mut sim = sim_with_helicopter();
    for (e, door) in sim.heli_cfg.entries.clone().iter().enumerate() {
        let (id, yaw, pitch) = at_door(&mut sim, 20 + e as u32, e);
        press_use(&mut sim, id, yaw, pitch);
        let seat = sim.soldiers[id].seat.unwrap_or_else(|| panic!("{} should have let them start", door.name));
        assert_eq!(seat.carrier, HELI_CARRIERS);
        assert!(door.seats.contains(&seat.seat), "{} leads to its own seats", door.name);
        assert!(sim.soldiers[id].transit.is_some(), "and it takes a moment");
        for _ in 0..((door.time + 0.2) / TICK_DT) as u32 {
            sim.step(&[(id, PlayerCmd { tick: sim.tick, yaw, pitch, ..Default::default() })]);
        }
        assert!(sim.soldiers[id].transit.is_none(), "{} is through", door.name);
    }

    // Halfway through the pilot's door their hands are not on anything yet.
    let mut sim = sim_with_helicopter();
    let (id, yaw, pitch) = at_door(&mut sim, 40, 0);
    press_use(&mut sim, id, yaw, pitch);
    assert_eq!(sim.soldiers[id].seat.map(|s| s.seat), Some(0));
    let mut buttons = Buttons::default();
    buttons.set(Buttons::JUMP, true);
    sim.step(&[(id, PlayerCmd { tick: sim.tick, yaw, pitch, buttons, ..Default::default() })]);
    assert_eq!(sim.helicopters[0].state.engine, Engine::Off, "not until they are sat down");
}

#[test]
fn from_far_away_or_under_a_high_hover_nothing_opens() {
    let mut sim = sim_with_helicopter();
    let (id, yaw, pitch) = at_door(&mut sim, 41, 0);
    sim.set_home(id, Vec3::new(30.0, 0.05, 30.0), 0.0);
    press_use(&mut sim, id, yaw, pitch);
    assert_eq!(sim.soldiers[id].seat, None);

    let mut sim = sim_with_helicopter();
    let (id, yaw, pitch) = at_door(&mut sim, 42, 0);
    let state = &mut sim.helicopters[0].state;
    state.pos.y = 25.0;
    state.parked = false;
    state.on_ground = false;
    state.skids = 0;
    press_use(&mut sim, id, yaw, pitch);
    assert_eq!(sim.soldiers[id].seat, None, "too high to reach");
}

#[test]
fn getting_out_is_through_a_door_and_takes_time_and_seats_inside_connect() {
    let mut sim = sim_with_helicopter();
    let id = seated(&mut sim, 43, 0);
    let (yaw, pitch) = (sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    press_use(&mut sim, id, yaw, pitch);
    assert!(sim.soldiers[id].transit.is_some(), "on their way out");
    for _ in 0..(4.0 / TICK_DT) as u32 {
        sim.step(&[(id, PlayerCmd { tick: sim.tick, yaw, pitch, ..Default::default() })]);
    }
    assert_eq!(sim.soldiers[id].seat, None);
    let feet = sim.soldiers[id].body.pos;
    assert!(feet.y < 0.5, "on the ground at {feet}");
    assert!(sim.world.capsule_is_clear(feet, sim.move_cfg.capsule(Stance::Stand)), "standing somewhere clear");

    let mut sim = sim_with_helicopter();
    let id = seated(&mut sim, 44, 0);
    for (key, seat) in [(Buttons::SLOT_4, 3u8), (Buttons::SLOT_3, 2), (Buttons::SLOT_2, 1)] {
        let mut buttons = Buttons::default();
        buttons.set(key, true);
        sim.step(&[(id, PlayerCmd { tick: sim.tick, buttons, ..Default::default() })]);
        assert_eq!(sim.soldiers[id].seat.map(|s| s.seat), Some(seat), "over to seat {seat}");
        assert!(sim.soldiers[id].transit.is_some());
        for _ in 0..(3.0 / TICK_DT) as u32 {
            sim.step(&[(id, PlayerCmd { tick: sim.tick, ..Default::default() })]);
        }
    }
}

/// Fragments find a helicopter on the ground as they do a truck: one bang on each part they reach.
#[test]
fn a_frag_beside_a_helicopter_hits_it_and_counts_each_part_once() {
    let mut sim = sim_with_helicopter();
    let thrower = sim.spawn_soldier(1);
    sim.set_home(thrower, Vec3::new(300.0, 0.1, 300.0), 0.0);
    let before = sim.helicopters[0].state;
    let at = sim.helicopters[0].state.pos + Vec3::new(3.0, 0.3, 0.0);
    sim.thrown.push(prs_throwables::Thrown { id: 77, kind: ThrownKind::Frag, thrower, pos: at, vel: Vec3::ZERO, fuse: 0.01, at_rest: true });
    let mut zones = Vec::new();
    for _ in 0..4 {
        sim.step(&[]);
        for event in &sim.events {
            if let SimEvent::HeliHit { heli: 0, shooter, zone, .. } = *event {
                assert_eq!(shooter, thrower);
                zones.push(zone);
            }
        }
    }
    assert!(!zones.is_empty(), "nothing hit it from 3 m");
    for (i, zone) in zones.iter().enumerate() {
        assert!(!zones[i + 1..].contains(zone), "{zone:?} counted twice in {zones:?}");
    }
    assert_ne!(sim.helicopters[0].state, before, "it should show");
}
