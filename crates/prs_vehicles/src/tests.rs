//! What a vehicle is meant to feel like, as numbers. Change the tuning and these say what you broke.

use super::*;
use glam::Quat;
use prs_collision::{CollisionWorld, Surface};
use std::f32::consts::{FRAC_PI_2, PI};

const DT: f32 = 1.0 / 60.0;

/// Ground from a height function, on a grid `size` metres across.
fn ground(size: f32, cells: u32, surface: Surface, height: impl Fn(f32, f32) -> f32) -> CollisionWorld {
    let n = cells + 1;
    let mut vertices = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (i as f32 / cells as f32 * size - size * 0.5, j as f32 / cells as f32 * size - size * 0.5);
            vertices.push(Vec3::new(x, height(x, z), z));
        }
    }
    let mut indices = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            let a = j * n + i;
            indices.push([a, a + n, a + 1]);
            indices.push([a + 1, a + n, a + n + 1]);
        }
    }
    let mut world = CollisionWorld::default();
    world.add_mesh(vertices, indices, surface);
    world
}

fn flat(surface: Surface) -> CollisionWorld {
    ground(1600.0, 8, surface, |_, _| 0.0)
}

fn truck() -> VehicleConfig {
    VehicleConfig::utility_truck()
}

/// A truck lowered onto the ground at this spot, facing `yaw`, and left to settle.
fn parked(cfg: &VehicleConfig, world: &CollisionWorld, x: f32, z: f32, yaw: f32) -> VehicleState {
    let top = Vec3::new(x, 200.0, z);
    let y = 200.0 - world.raycast(top, Vec3::NEG_Y, 400.0).expect("ground").distance;
    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, cfg, Frame { pos: Vec3::new(x, y + 1.0, z), rot: Quat::from_rotation_y(yaw) });
    run(&mut state, cfg, world, DriveInput::default(), 3.0);
    state
}

fn run(state: &mut VehicleState, cfg: &VehicleConfig, world: &CollisionWorld, input: DriveInput, seconds: f32) {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT) as u32 {
        events.clear();
        step(state, cfg, &input, world, DT, &mut events);
    }
}

fn drive(throttle: f32, steer: f32) -> DriveInput {
    DriveInput { throttle, steer, manned: true, handbrake: false, shift: 0, ..Default::default() }
}

/// How far it is from upright, in radians.
fn tilt(state: &VehicleState) -> f32 {
    (state.rot * Vec3::Y).dot(Vec3::Y).clamp(-1.0, 1.0).acos()
}

fn speed(state: &VehicleState) -> f32 {
    state.vel.length()
}

#[test]
fn it_comes_to_rest_on_its_springs_and_stays_exactly_still() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, 3.0, -4.0, 0.7);
    assert!(state.asleep, "settled and left alone it should sleep: {state:?}");
    let before = state.frame();
    run(&mut state, &cfg, &world, DriveInput::default(), 5.0);
    assert_eq!(state.frame(), before, "a parked vehicle must not move at all");
    // The ride height comes from the wheels at their rest compression, with all four alike.
    let squash: Vec<f32> = state.wheels.iter().map(|w| w.compression).collect();
    let weight = cfg.mass * cfg.gravity / 4.0 / cfg.suspension.stiffness;
    for c in &squash {
        assert!((c - weight).abs() < 0.06, "{squash:?} vs {weight}");
    }
    assert!(tilt(&state) < 0.02, "{}", tilt(&state));
}

#[test]
fn nothing_jitters_while_it_settles() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, &cfg, Frame { pos: Vec3::new(0.0, 1.1, 0.0), rot: Quat::IDENTITY });
    run(&mut state, &cfg, &world, DriveInput::default(), 2.0);
    // Past the first two seconds, how much it moves in a tick has to be fading away and tiny.
    let mut most = 0.0_f32;
    let mut events = Vec::new();
    let mut last = state.pos;
    for _ in 0..60 {
        step(&mut state, &cfg, &DriveInput::default(), &world, DT, &mut events);
        most = most.max(state.pos.distance(last));
        last = state.pos;
    }
    assert!(most < 0.001, "still moving {most} m a tick after two seconds");
}

#[test]
fn top_speed_on_a_road_is_that_of_an_old_truck() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 50.0);
    let top = state.forward_speed();
    assert!((24.0..33.0).contains(&top), "{top} m/s");
}

#[test]
fn it_takes_its_time_to_get_going() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 2.0);
    let two = state.forward_speed();
    assert!((3.5..9.0).contains(&two), "{two} m/s after 2 s");
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 8.0);
    let ten = state.forward_speed();
    assert!((14.0..24.0).contains(&ten), "{ten} m/s after 10 s");
}

/// How far it takes to stop from 20 m/s, straight, on this ground.
fn stopping_distance(surface: Surface) -> f32 {
    let cfg = truck();
    let world = flat(surface);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    let mut events = Vec::new();
    while state.forward_speed() < 20.0 {
        step(&mut state, &cfg, &drive(1.0, 0.0), &world, DT, &mut events);
        assert!(state.pos.x < 700.0, "never got to speed on {surface:?}");
    }
    let start = state.pos;
    for _ in 0..1200 {
        step(&mut state, &cfg, &drive(-1.0, 0.0), &world, DT, &mut events);
        if state.forward_speed() < 0.1 {
            break;
        }
    }
    state.pos.distance(start)
}

#[test]
fn it_stops_in_a_believable_distance_and_later_on_loose_ground() {
    let road = stopping_distance(Surface::Concrete);
    assert!((18.0..34.0).contains(&road), "{road} m on concrete");
    let grass = stopping_distance(Surface::Grass);
    assert!(grass > road * 1.3, "{grass} m on grass vs {road} m on concrete");
    assert!(grass < 70.0, "{grass} m on grass");
}

#[test]
fn it_rides_nose_down_under_braking_and_nose_up_pulling_away() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    let front_rear = |s: &VehicleState| (s.wheels[0].compression, s.wheels[2].compression);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 0.6);
    let (front, rear) = front_rear(&state);
    assert!(rear > front, "pulling away the weight goes back: {front} {rear}");
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 8.0);
    run(&mut state, &cfg, &world, drive(-1.0, 0.0), 0.5);
    let (front, rear) = front_rear(&state);
    assert!(front > rear + 0.02, "braking the weight comes forward: {front} {rear}");
}

#[test]
fn the_handbrake_takes_the_throttle_and_slows_it_in_a_straight_line() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 6.0);
    assert!(state.forward_speed() > 12.0);
    run(&mut state, &cfg, &world, DriveInput { throttle: 1.0, steer: 0.0, manned: true, handbrake: true, shift: 0, ..Default::default() }, 3.0);
    assert!(state.forward_speed() < 8.0, "{}", state.forward_speed());
    assert!((state.rot * Vec3::NEG_Z).dot(Vec3::X) > 0.995, "and it stays straight with nothing asking otherwise");
}

#[test]
fn it_reverses_after_stopping() {
    let cfg = truck();
    let world = flat(Surface::Dirt);
    let mut state = parked(&cfg, &world, 0.0, 0.0, 0.0);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 3.0);
    assert!(state.forward_speed() > 3.0);
    run(&mut state, &cfg, &world, drive(-1.0, 0.0), 5.0);
    assert!(state.forward_speed() < -1.5, "{}", state.forward_speed());
}

#[test]
fn full_lock_at_low_speed_turns_tightly_and_does_not_flip() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, 0.0, 0.0, 0.0);
    let mut heading_change = 0.0_f32;
    let mut most_tilt = 0.0_f32;
    let mut last = state.frame().yaw();
    let mut events = Vec::new();
    for _ in 0..(20.0 / DT) as u32 {
        let throttle = if state.forward_speed() < 6.0 { 0.6 } else { 0.0 };
        step(&mut state, &cfg, &drive(throttle, 1.0), &world, DT, &mut events);
        let yaw = state.frame().yaw();
        let d = (yaw - last + PI).rem_euclid(2.0 * PI) - PI;
        heading_change += d;
        last = yaw;
        most_tilt = most_tilt.max(tilt(&state));
    }
    assert!(heading_change < -2.0 * PI, "steering right should go round: {heading_change}");
    assert!(most_tilt < 0.17, "leaned {most_tilt} rad at low speed");
}

#[test]
fn flat_out_on_flat_ground_it_does_not_roll_however_hard_it_is_steered() {
    let cfg = truck();
    for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass] {
        let world = flat(surface);
        for steer in [1.0, -1.0] {
            let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
            run(&mut state, &cfg, &world, drive(1.0, 0.0), 24.0);
            let fast = speed(&state);
            assert!(fast > 15.0, "{surface:?}: {fast}");
            let mut most = 0.0_f32;
            let mut events = Vec::new();
            for _ in 0..(8.0 / DT) as u32 {
                step(&mut state, &cfg, &drive(1.0, steer), &world, DT, &mut events);
                most = most.max(tilt(&state));
            }
            assert!(most < 0.5, "{surface:?} at {fast} m/s leaned {most} rad");
        }
    }
}

/// A slope running up towards +X, flat at first so it can get going, at `degrees`.
fn hill(degrees: f32, surface: Surface) -> CollisionWorld {
    let slope = degrees.to_radians().tan();
    ground(800.0, 400, surface, move |x, _| (x - 10.0).max(0.0) * slope)
}

/// How far up that slope it gets from a standing start at the foot, flat out for `seconds`.
fn climbed(degrees: f32, surface: Surface, seconds: f32) -> f32 {
    let cfg = truck();
    let world = hill(degrees, surface);
    let mut state = parked(&cfg, &world, 2.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), seconds);
    state.pos.x - 10.0
}

#[test]
fn it_climbs_a_steep_track_and_gives_up_on_steeper() {
    assert!(climbed(18.0, Surface::Dirt, 20.0) > 40.0);
    assert!(climbed(24.0, Surface::Dirt, 20.0) > 25.0);
    // Past what the first gear and tyres can do it gets a run at it, stops and goes no further.
    assert!(climbed(38.0, Surface::Dirt, 25.0) < 16.0);
    assert!(climbed(38.0, Surface::Grass, 25.0) < 16.0);
}

#[test]
fn parked_on_a_slope_its_brakes_hold_it() {
    let cfg = truck();
    let world = hill(15.0, Surface::Dirt);
    let mut state = parked(&cfg, &world, 60.0, 0.0, -FRAC_PI_2);
    let at = state.pos;
    run(&mut state, &cfg, &world, DriveInput::default(), 10.0);
    assert!(state.pos.distance(at) < 0.05, "crept {}", state.pos.distance(at));
    // And across it, the tyres hold it sideways.
    let mut state = parked(&cfg, &world, 60.0, 0.0, 0.0);
    let at = state.pos;
    run(&mut state, &cfg, &world, DriveInput::default(), 10.0);
    assert!(state.pos.distance(at) < 0.05, "slid {}", state.pos.distance(at));
}

#[test]
fn on_a_side_slope_too_steep_it_does_not_stay() {
    let cfg = truck();
    let world = hill(58.0, Surface::Concrete);
    let top = Vec3::new(40.0, 400.0, 0.0);
    let y = 400.0 - world.raycast(top, Vec3::NEG_Y, 800.0).unwrap().distance;
    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, &cfg, Frame { pos: Vec3::new(40.0, y + 2.0, 0.0), rot: Quat::from_rotation_z(58.0_f32.to_radians()) });
    let at = state.pos;
    run(&mut state, &cfg, &world, DriveInput::default(), 3.0);
    // With less grip than the slope is steep it goes down it sideways, and tips if anything catches it.
    assert!(tilt(&state) > 1.4 || at.distance(state.pos) > 5.0, "neither rolled nor slid: {} {}", tilt(&state), at.distance(state.pos));
}

/// A ditch across the way, `depth` deep and `width` wide with steep banks, starting at x = 30.
fn ditch(depth: f32, width: f32) -> CollisionWorld {
    ground(400.0, 800, Surface::Dirt, move |x, _| {
        let dx = (x - (30.0 + width * 0.5)).abs();
        -depth * (1.0 - ((dx - width * 0.18) / (width * 0.32)).clamp(0.0, 1.0))
    })
}

#[test]
fn a_real_ditch_stops_it_dead_and_it_cannot_get_out() {
    let cfg = truck();
    let world = ditch(1.3, 3.0);
    let mut state = parked(&cfg, &world, 20.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 2.5);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 12.0);
    assert!(state.pos.x < 36.0, "drove across a ditch: x {}", state.pos.x);
    let out_forward = state.pos.x;
    run(&mut state, &cfg, &world, drive(-1.0, 0.0), 8.0);
    assert!((state.pos.x - out_forward).abs() < 8.0 && state.pos.x > 24.0, "reversed out too easily: x {}", state.pos.x);
}

#[test]
fn a_shallow_ditch_is_only_a_bump() {
    let cfg = truck();
    let world = ditch(0.25, 2.0);
    let mut state = parked(&cfg, &world, 10.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 8.0);
    assert!(state.pos.x > 40.0, "x {}", state.pos.x);
    assert!(tilt(&state) < 0.3, "{}", tilt(&state));
}

fn walled() -> CollisionWorld {
    let mut world = flat(Surface::Concrete);
    world.add_box(Vec3::new(30.0, 1.5, 0.0), Vec3::new(0.15, 1.5, 80.0), Quat::IDENTITY, Surface::Concrete);
    world
}

#[test]
fn a_wall_stops_it_whatever_the_speed_and_it_never_gets_through() {
    let cfg = truck();
    let world = walled();
    for start in [20.0, -40.0, -160.0] {
        let mut state = parked(&cfg, &world, start, 0.0, -FRAC_PI_2);
        let mut events = Vec::new();
        let mut hit = 0.0_f32;
        for _ in 0..(16.0 / DT) as u32 {
            step(&mut state, &cfg, &drive(1.0, 0.0), &world, DT, &mut events);
            for event in &events {
                if let VehicleEvent::Crashed { speed, .. } = event {
                    hit = hit.max(*speed);
                }
            }
            events.clear();
            assert!(state.pos.x < 29.0, "went through from {start}: {} at {}", state.pos.x, state.vel.x);
        }
        assert!(state.pos.x > 25.0, "never reached the wall from {start}: {}", state.pos.x);
        assert!(state.vel.x.abs() < 2.0, "pushing against a wall from {start}: {}", state.vel.x);
        if start < 0.0 {
            assert!(hit > 8.0, "from {start} hit at {hit}");
        }
    }
}

#[test]
fn steering_takes_a_moment_and_reaches_full_lock_at_any_speed() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 24.0);
    assert!(speed(&state) > 22.0);
    run(&mut state, &cfg, &world, drive(1.0, 1.0), 0.05);
    assert!(state.steer < 0.08, "keyboard steering must ramp: {}", state.steer);
    run(&mut state, &cfg, &world, drive(1.0, 1.0), 1.0);
    assert_eq!(state.steer, cfg.steering.max_angle, "the tyres say how hard it turns, not a limit on the wheel");
}

#[test]
fn a_bad_tyre_makes_it_pull_and_slow_and_no_engine_means_no_power() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let clean = {
        let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
        run(&mut state, &cfg, &world, drive(1.0, 0.0), 10.0);
        state
    };
    let mut flat_tyre = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    flat_tyre.wheels[0].tyre = 0.0;
    run(&mut flat_tyre, &cfg, &world, drive(1.0, 0.0), 10.0);
    assert!(flat_tyre.pos.x < clean.pos.x, "{} {}", flat_tyre.pos.x, clean.pos.x);
    assert!(flat_tyre.pos.z.abs() > 0.5, "should pull to one side: {}", flat_tyre.pos.z);
    let mut dead = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    dead.damage.engine = 0.0;
    run(&mut dead, &cfg, &world, drive(1.0, 0.0), 5.0);
    assert!(speed(&dead) < 0.05);
}

#[test]
fn two_trucks_running_into_each_other_stop_being_in_the_same_place() {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut a = parked(&cfg, &world, -30.0, 0.0, -FRAC_PI_2);
    let mut b = parked(&cfg, &world, 0.0, 0.0, -FRAC_PI_2);
    assert!(b.asleep);
    let mut events = Vec::new();
    let mut pair = Vec::new();
    let mut crashed = false;
    for _ in 0..(8.0 / DT) as u32 {
        step(&mut a, &cfg, &drive(1.0, 0.0), &world, DT, &mut events);
        step(&mut b, &cfg, &DriveInput::default(), &world, DT, &mut events);
        collide(&mut a, &cfg, &mut b, &cfg, &mut pair);
        crashed |= !pair.is_empty();
        pair.clear();
        // Nose to tail they are further apart than this, but they glance off and end up alongside.
        assert!(a.pos.distance(b.pos) > 3.0, "inside each other: {}", a.pos.distance(b.pos));
    }
    assert!(crashed);
    assert!(b.pos.x > 0.5, "the one hit should have been shoved: {}", b.pos.x);
}

#[test]
fn the_same_inputs_give_exactly_the_same_result() {
    let cfg = truck();
    let world = ground(400.0, 80, Surface::Dirt, |x, z| (x * 0.1).sin() * 0.8 + (z * 0.17).sin() * 0.5);
    let script = |tick: u32| drive(1.0 - (tick % 500) as f32 / 500.0, (tick as f32 * 0.02).sin());
    let mut a = parked(&cfg, &world, 0.0, 0.0, 0.3);
    let mut b = a.clone();
    let mut events = Vec::new();
    for tick in 0..2000 {
        step(&mut a, &cfg, &script(tick), &world, DT, &mut events);
        step(&mut b, &cfg, &script(tick), &world, DT, &mut events);
        assert_eq!(a, b, "at {tick}");
    }
}

/// Bullets, against the truck parked at the origin looking down -Z.
fn at_origin() -> (VehicleConfig, VehicleState, Frame) {
    let cfg = truck();
    let state = VehicleState::new(VehicleTable::UTILITY_TRUCK, &cfg, Frame::IDENTITY);
    let frame = state.frame();
    (cfg, state, frame)
}

#[test]
fn a_bullet_finds_the_part_it_really_touches() {
    let (cfg, state, frame) = at_origin();
    // Straight at the bonnet from in front, over the grille.
    let hit = strike(&state, &cfg, &[], &frame, Vec3::new(0.0, 0.2, -9.0), Vec3::Z, 20.0).expect("engine");
    assert_eq!(hit.zone, VehicleZone::Engine);
    assert_eq!(hit.pass, 0.0, "an engine block stops a bullet");
    // At a door from the side, low.
    let hit = strike(&state, &cfg, &[], &frame, Vec3::new(-9.0, 0.0, -0.2), Vec3::X, 20.0).expect("door");
    assert_eq!(hit.zone, VehicleZone::Body);
    assert!(hit.pass > 0.0, "a door lets it through");
    // At a front wheel from the side.
    let hit = strike(&state, &cfg, &[], &frame, Vec3::new(-9.0, -0.42, -1.35), Vec3::X, 20.0).expect("tyre");
    assert_eq!(hit.zone, VehicleZone::Tyre(0));
    // Through the side above the doors, at head height, there is nothing in the way at all.
    assert!(strike(&state, &cfg, &[], &frame, Vec3::new(-9.0, 0.7, -0.25), Vec3::X, 20.0).is_none(), "the sides are open");
}

#[test]
fn an_open_door_is_not_where_a_shut_one_is_and_stops_rounds_where_it_has_swung_to() {
    let (cfg, state, frame) = at_origin();
    let (from, shut) = (Vec3::new(-9.0, 0.0, -0.3), vec![0.0; 4]);
    let closed = strike(&state, &cfg, &shut, &frame, from, Vec3::X, 20.0).expect("the door");
    assert!(closed.pass > 0.0);
    // Open, the way past where it was is clear, right through to the far door.
    let mut open = shut.clone();
    open[0] = 1.0;
    let through = strike(&state, &cfg, &open, &frame, from, Vec3::X, 20.0);
    assert!(through.is_none_or(|hit| hit.distance > closed.distance + 0.5), "{through:?}");
    // And out where it swung to, it is in the way of something coming along the side of the truck.
    let along = Vec3::new(-1.3, 0.0, 5.0);
    assert!(strike(&state, &cfg, &shut, &frame, along, Vec3::NEG_Z, 7.0).is_none());
    assert!(strike(&state, &cfg, &open, &frame, along, Vec3::NEG_Z, 7.0).is_some());
}

#[test]
fn it_goes_from_damaged_to_burning_to_gone() {
    let (cfg, mut state, _) = at_origin();
    let mut events = Vec::new();
    let d = &cfg.damage;
    hurt(&mut state, d, VehicleZone::Engine, d.engine * 0.75, &mut events);
    let power = state.damage.power_share(d);
    assert!(power > 0.2 && power < 0.8, "{power}");
    assert!(state.damage.smoking(d));
    hurt(&mut state, d, VehicleZone::Engine, d.engine, &mut events);
    assert!(events.contains(&VehicleEvent::EngineDead));
    assert_eq!(state.damage.power_share(d), 0.0);
    assert!(state.damage.burning.is_none());
    hurt(&mut state, d, VehicleZone::Engine, 10.0, &mut events);
    assert!(events.contains(&VehicleEvent::OnFire));
    let mut due = false;
    for _ in 0..((d.burn_time + d.respawn_after + 1.0) / DT) as u32 {
        due |= step_damage(&mut state, d, DT, &mut events);
    }
    assert!(events.iter().any(|e| matches!(e, VehicleEvent::Exploded { .. })));
    assert!(state.damage.wrecked);
    assert!(due, "a new one comes in the end");
}

#[test]
fn one_left_on_its_roof_is_replaced_in_the_end() {
    let (cfg, mut state, _) = at_origin();
    state.rot = Quat::from_rotation_z(PI);
    let mut events = Vec::new();
    let mut due = false;
    for _ in 0..((cfg.damage.respawn_after + 1.0) / DT) as u32 {
        due |= step_damage(&mut state, &cfg.damage, DT, &mut events);
    }
    assert!(due);
    // Not while someone is in it, and not if it is on its wheels.
    state.abandoned_for = 0.0;
    state.occupants.sit(0, 7);
    assert!(!step_damage(&mut state, &cfg.damage, cfg.damage.respawn_after + 1.0, &mut events));
    state.occupants.leave(7);
    state.rot = Quat::IDENTITY;
    assert!(!step_damage(&mut state, &cfg.damage, cfg.damage.respawn_after + 1.0, &mut events));
}

#[test]
fn enough_tyre_hits_burst_one() {
    let (cfg, mut state, _) = at_origin();
    let mut events = Vec::new();
    hurt(&mut state, &cfg.damage, VehicleZone::Tyre(2), cfg.damage.tyre * 1.1, &mut events);
    assert!(events.contains(&VehicleEvent::TyreBurst { wheel: 2 }));
    assert_eq!(state.wheels[2].tyre, 0.0);
}

fn car() -> VehicleConfig {
    VehicleConfig::armoured_car()
}

fn parked_car(world: &CollisionWorld, x: f32, yaw: f32) -> VehicleState {
    let cfg = car();
    let top = Vec3::new(x, 200.0, 0.0);
    let y = 200.0 - world.raycast(top, Vec3::NEG_Y, 400.0).expect("ground").distance;
    let mut state = VehicleState::new(VehicleTable::ARMOURED_CAR, &cfg, Frame { pos: Vec3::new(x, y + cfg.ride_height() + 0.1, 0.0), rot: Quat::from_rotation_y(yaw) });
    run(&mut state, &cfg, world, DriveInput::default(), 3.0);
    state
}

#[test]
fn the_armoured_car_is_slow_and_heavy_and_rests_still() {
    let cfg = car();
    let world = flat(Surface::Concrete);
    let mut state = parked_car(&world, -750.0, -FRAC_PI_2);
    assert!(state.asleep, "{state:?}");
    assert!(tilt(&state) < 0.02);
    assert!((state.pos.y - cfg.ride_height()).abs() < 0.06, "{} vs {}", state.pos.y, cfg.ride_height());
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 45.0);
    let top = state.forward_speed();
    assert!((19.0..28.0).contains(&top), "{top} m/s");
    assert!(top < truck_top_speed(), "should be slower than the truck");
}

fn truck_top_speed() -> f32 {
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 45.0);
    state.forward_speed()
}

#[test]
fn the_armoured_car_climbs_what_it_should_and_does_not_flip_on_the_flat() {
    let cfg = car();
    let world = hill(16.0, Surface::Dirt);
    let mut state = parked_car(&world, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 20.0);
    assert!(state.pos.x > 40.0, "x {}", state.pos.x);
    let world = hill(36.0, Surface::Grass);
    let mut state = parked_car(&world, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 20.0);
    assert!(state.pos.x < 24.0, "x {}", state.pos.x);

    let world = flat(Surface::Concrete);
    let mut state = parked_car(&world, -750.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 22.0);
    let mut most = 0.0_f32;
    let mut events = Vec::new();
    for _ in 0..(8.0 / DT) as u32 {
        step(&mut state, &cfg, &drive(1.0, 1.0), &world, DT, &mut events);
        most = most.max(tilt(&state));
    }
    assert!(most < 0.5, "leaned {most}");
}

#[test]
fn rifle_rounds_do_nothing_to_armour_but_scratch_it_and_the_hatch_is_open() {
    let cfg = car();
    let state = VehicleState::new(VehicleTable::ARMOURED_CAR, &cfg, Frame::IDENTITY);
    let frame = state.frame();
    let hit = strike(&state, &cfg, &[], &frame, Vec3::new(-9.0, 0.2, 0.5), Vec3::X, 20.0).expect("side plate");
    assert_eq!(hit.pass, 0.0, "a plate stops it");
    assert_eq!(hit.zone, VehicleZone::Body);
    // Straight down through the hatch there is nothing, until the floor.
    let hit = strike(&state, &cfg, &[], &frame, Vec3::new(0.0, 5.0, 0.55), Vec3::NEG_Y, 4.0);
    assert!(hit.is_none(), "{hit:?}");
    let mut state = state;
    let mut events = Vec::new();
    let weapons_worth = 40.0;
    hurt(&mut state, &cfg.damage, VehicleZone::Body, weapons_worth, &mut events);
    assert!(state.damage.hull > cfg.damage.hull * 0.99, "{}", state.damage.hull);
}

/// However badly it is driven over rough ground and into things, it stays a real thing in the world.
#[test]
fn hard_driving_over_rough_ground_never_goes_wrong() {
    let surface_at = |x: f32, z: f32| (x * 0.07).sin() * 2.2 + (z * 0.11).cos() * 1.6 + (x * 0.31 + z * 0.23).sin() * 0.5 + (z * 0.8).sin() * 0.08;
    // Big enough that holding the steering this loosely does not run it off the edge of the ground.
    let mut world = ground(3200.0, 800, Surface::Dirt, surface_at);
    for i in 0..60 {
        let (x, z) = (((i * 37) % 300) as f32 - 150.0, ((i * 91) % 300) as f32 - 150.0);
        world.add_box(Vec3::new(x, surface_at(x, z) + 1.0, z), Vec3::new(3.0, 1.5, 0.2), Quat::from_rotation_y(i as f32), Surface::Concrete);
    }
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        let y = surface_at(0.0, -170.0) + cfg.ride_height();
        let mut state = VehicleState::new(kind, &cfg, Frame { pos: Vec3::new(0.0, y, -170.0), rot: Quat::IDENTITY });
        let mut events = Vec::new();
        for tick in 0..(180.0 / DT) as u32 {
            let t = tick as f32 * DT;
            let throttle = if (t * 0.11).sin() > -0.8 { 1.0 } else { -1.0 };
            let input = drive(throttle, (t * 0.37).sin().signum() * ((t * 1.1).cos() * 0.5 + 0.5));
            step(&mut state, &cfg, &input, &world, DT, &mut events);
            assert!(state.pos.is_finite() && state.rot.is_finite() && state.vel.is_finite(), "{state:?}");
            assert!(state.vel.length() < 70.0, "going {} at {tick}", state.vel.length());
            assert!(state.ang_vel.length() < 40.0, "spinning {} at {tick}", state.ang_vel.length());
            let under = surface_at(state.pos.x, state.pos.z) - state.pos.y;
            assert!(under < 0.5, "{} under the ground at {tick}", under);
        }
    }
}

/// Prints how it does, for tuning by eye.
#[test]
#[ignore]
fn handling_report() {
    for (name, cfg) in [("car", car()), ("truck", truck())] {
        let world = flat(Surface::Concrete);
        let mut state = if name == "car" { parked_car(&world, -750.0, -FRAC_PI_2) } else { parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2) };
        for second in 1..=30 {
            run(&mut state, &cfg, &world, drive(1.0, 0.0), 1.0);
            if second % 3 == 0 {
                println!("{name} {second} s: {:.1} m/s, gear {}, {:.0} rpm", state.forward_speed(), state.engine.gear, state.engine.rpm);
            }
        }
    }
    let cfg = truck();
    let world = flat(Surface::Concrete);
    let mut state = parked(&cfg, &world, -750.0, 0.0, -FRAC_PI_2);
    println!("rest height {}", state.pos.y);
    for second in 1..=40 {
        run(&mut state, &cfg, &world, drive(1.0, 0.0), 1.0);
        println!("{second} s: {:.1} m/s, gear {}, {:.0} rpm", state.forward_speed(), state.engine.gear, state.engine.rpm);
    }
    for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass, Surface::Gravel] {
        println!("{surface:?}: stops from 20 m/s in {:.1} m", stopping_distance(surface));
    }
    for degrees in [10.0, 15.0, 20.0, 25.0, 30.0, 35.0] {
        println!("{degrees} degrees: {:.1} m up dirt in 20 s, {:.1} m up grass", climbed(degrees, Surface::Dirt, 20.0), climbed(degrees, Surface::Grass, 20.0));
    }
}

/// One run of a step of full steering at a held speed: how hard it corners and how far it slides doing it.
struct Corner {
    /// Most sideways acceleration, in g, averaged over a quarter second so a bump does not count.
    lateral_g: f32,
    /// Most angle between where it points and where it is going, in degrees.
    slip_degrees: f32,
    /// Metres it moved sideways relative to where it was pointing.
    slid: f32,
    /// Fastest sideways speed, m/s, measured at the rear axle.
    rear_slide: f32,
    most_tilt: f32,
}

fn corner_test(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface, speed: f32, lock: Option<f32>) -> Corner {
    let mut cfg = cfg.clone();
    if let Some(lock) = lock {
        cfg.steering.max_angle = lock;
    }
    let world = flat(surface);
    let y = cfg.ride_height() + 0.05;
    let mut state = VehicleState::new(kind, &cfg, Frame { pos: Vec3::new(-400.0, y, 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
    let mut events = Vec::new();
    let hold = |state: &VehicleState| if state.forward_speed() < speed { 1.0 } else { 0.0 };
    for _ in 0..(40.0 / DT) as u32 {
        let t = hold(&state);
        step(&mut state, &cfg, &drive(t, 0.0), &world, DT, &mut events);
        if state.forward_speed() >= speed {
            break;
        }
    }
    let (mut lateral_g, mut slip_degrees, mut slid, mut rear_slide, mut most_tilt) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let rear = cfg.wheels.iter().map(|w| w.mount.z).fold(f32::MIN, f32::max);
    let mut window = std::collections::VecDeque::new();
    for _ in 0..(4.0 / DT) as u32 {
        let before = state.vel;
        let t = hold(&state);
        step(&mut state, &cfg, &drive(t, 1.0), &world, DT, &mut events);
        let accel = (state.vel - before) / DT;
        let side = state.rot * Vec3::X;
        window.push_back(accel.dot(side));
        if window.len() > 15 {
            window.pop_front();
            lateral_g = lateral_g.max((window.iter().sum::<f32>() / 15.0).abs() / 9.81);
        }
        let v_side = state.vel.dot(side);
        slid += v_side.abs() * DT;
        if state.vel.length() > 1.0 {
            slip_degrees = slip_degrees.max(v_side.atan2(state.forward_speed().abs()).abs().to_degrees());
        }
        let at_rear = state.vel + state.ang_vel.cross(state.rot * Vec3::new(0.0, 0.0, rear));
        rear_slide = rear_slide.max(at_rear.dot(side).abs());
        most_tilt = most_tilt.max(tilt(&state));
    }
    Corner { lateral_g, slip_degrees, slid, rear_slide, most_tilt }
}

/// The most these tyres corner at this speed, however far the wheels are turned to get it.
fn peak_lateral(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface, speed: f32) -> f32 {
    [0.1, 0.13, 0.16, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6].iter().map(|lock| corner_test(cfg, kind, surface, speed, Some(*lock)).lateral_g).fold(0.0, f32::max)
}

/// From a standstill flat out: how far the wheels run ahead of the ground, and how soon it gets going.
fn launch_test(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface) -> (f32, f32, f32) {
    let world = flat(surface);
    let y = cfg.ride_height() + 0.05;
    let mut state = VehicleState::new(kind, cfg, Frame { pos: Vec3::new(-400.0, y, 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
    run(&mut state, cfg, &world, DriveInput::default(), 2.0);
    let (mut most_slip, mut spin_over, mut time_to_10) = (0.0_f32, 0.0_f32, f32::NAN);
    let mut events = Vec::new();
    for tick in 0..(10.0 / DT) as u32 {
        step(&mut state, cfg, &drive(1.0, 0.0), &world, DT, &mut events);
        for (wheel, config) in state.wheels.iter().zip(&cfg.wheels) {
            most_slip = most_slip.max(wheel.slip);
            spin_over = spin_over.max(wheel.spin * config.radius - state.forward_speed());
        }
        if time_to_10.is_nan() && state.forward_speed() >= 10.0 {
            time_to_10 = tick as f32 * DT;
        }
    }
    (most_slip, spin_over, time_to_10)
}

/// `cargo test -p prs_vehicles grip_report -- --ignored --nocapture`. What the tyres do, by vehicle and ground.
#[test]
#[ignore]
fn grip_report() {
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Gravel, Surface::Grass] {
            // With nothing holding the wheel back, so this is the tyres' limit and not the steering's.
            let limit = peak_lateral(&cfg, kind, surface, 11.1);
            let at_40 = corner_test(&cfg, kind, surface, 11.1, None);
            let at_60 = corner_test(&cfg, kind, surface, 16.7, None);
            let (slip, over, ten) = launch_test(&cfg, kind, surface);
            println!(
                "{name} {surface:?}: tyre limit {:.2} g | 40 km/h full lock {:.2} g, slip {:.1} deg, slid {:.1} m, rear {:.1} m/s, tilt {:.2} | 60 km/h {:.2} g, slip {:.1} deg, slid {:.1} m, rear {:.1} m/s | launch slip {:.2}, wheels over by {:.1} m/s, 0-36 km/h {:.1} s",
                limit, at_40.lateral_g, at_40.slip_degrees, at_40.slid, at_40.rear_slide, at_40.most_tilt, at_60.lateral_g, at_60.slip_degrees, at_60.slid, at_60.rear_slide, slip, over, ten
            );
        }
    }
}

/// A scripted minute of what a player does with the keys: `(throttle, steer)`, starting at `speed`. Gives
/// the worst slip at the rear axle in degrees, sideways metres slid there, and how much the heading turned.
fn keys_test(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface, speed: f32, throttle: f32, steer: f32, brake: bool) -> (f32, f32, f32, f32) {
    let world = flat(surface);
    let y = cfg.ride_height() + 0.05;
    let mut state = VehicleState::new(kind, cfg, Frame { pos: Vec3::new(-400.0, y, 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
    let mut events = Vec::new();
    for _ in 0..(40.0 / DT) as u32 {
        if state.forward_speed() >= speed {
            break;
        }
        step(&mut state, cfg, &drive(1.0, 0.0), &world, DT, &mut events);
    }
    let (mut worst, mut slid, mut turned) = (0.0_f32, 0.0_f32, 0.0_f32);
    let rear = cfg.wheels.iter().map(|w| w.mount.z).fold(f32::MIN, f32::max);
    let mut last = state.frame().yaw();
    let start = state.vel.length();
    for _ in 0..(3.0 / DT) as u32 {
        step(&mut state, cfg, &DriveInput { throttle, steer, manned: true, handbrake: brake, shift: 0, ..Default::default() }, &world, DT, &mut events);
        let side = state.rot * Vec3::X;
        // At the rear axle, which a tyre that holds drags straight along whatever the turn: anything
        // there is the back end sliding. The middle of a tight turn goes sideways with no slide at all.
        let at_rear = state.vel + state.ang_vel.cross(state.rot * Vec3::new(0.0, 0.0, rear));
        let v_side = at_rear.dot(side);
        slid += v_side.abs() * DT;
        if at_rear.length() > 1.0 {
            worst = worst.max(v_side.atan2(at_rear.dot(state.rot * Vec3::NEG_Z).abs()).abs().to_degrees());
        }
        let yaw = state.frame().yaw();
        turned += wrap(yaw - last).abs();
        last = yaw;
    }
    (worst, slid, turned.to_degrees(), start - state.vel.length())
}

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

#[test]
#[ignore]
fn keys_report() {
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass] {
            for (what, speed, throttle, steer, brake) in [("W+D 5", 5.0, 1.0, 1.0, false), ("W+D 11", 11.0, 1.0, 1.0, false), ("W+D 20", 20.0, 1.0, 1.0, false), ("lift+D 20", 20.0, 0.0, 1.0, false), ("S+D 11", 11.0, -1.0, 1.0, false), ("S+D 20", 20.0, -1.0, 1.0, false)] {
                let (slip, slid, turned, lost) = keys_test(&cfg, kind, surface, speed, throttle, steer, brake);
                println!("{name} {surface:?} {what}: slip {slip:.1} deg, slid {slid:.1} m, turned {turned:.0} deg, speed lost {lost:.1}");
            }
        }
    }
}

/// What a truck of this class can do, from about 0.6 to 0.8 g on something firm and less on something loose.
#[test]
fn it_corners_as_hard_as_a_truck_of_its_class_and_less_on_loose_ground() {
    let cfg = truck();
    let peak = |surface| peak_lateral(&cfg, VehicleTable::UTILITY_TRUCK, surface, 11.1);
    let (concrete, dirt, gravel, grass) = (peak(Surface::Concrete), peak(Surface::Dirt), peak(Surface::Gravel), peak(Surface::Grass));
    println!("concrete {concrete:.2} g, dirt {dirt:.2} g, gravel {gravel:.2} g, grass {grass:.2} g");
    assert!((0.7..0.8).contains(&concrete), "{concrete}");
    assert!((0.62..0.72).contains(&dirt), "{dirt}");
    assert!((0.55..0.65).contains(&gravel), "{gravel}");
    assert!((0.5..0.6).contains(&grass), "{grass}");
    assert!(concrete > dirt && dirt > gravel && gravel > grass);
}

/// A full press of a key at 40 km/h is a hard turn. The tail follows the nose.
#[test]
fn full_lock_at_40_kmh_turns_and_does_not_slide() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Gravel, Surface::Grass] {
            let corner = corner_test(&cfg, kind, surface, 11.1, None);
            assert!(corner.slid < 3.0, "{surface:?} {}", corner.slid);
            assert!(corner.slip_degrees < 4.0, "{surface:?} {}", corner.slip_degrees);
            assert!(corner.rear_slide < 1.0, "{surface:?} {}", corner.rear_slide);
        }
    }
}

/// Tyres that keep their grip under the brake: no swapping the ends over on anything firm at 70 km/h.
#[test]
fn braking_hard_on_full_lock_does_not_spin_it() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt] {
            let (slip, slid, ..) = keys_test(&cfg, kind, surface, 20.0, -1.0, 1.0, false);
            assert!(slip < 15.0, "{surface:?} {slip}");
            assert!(slid < 3.0, "{surface:?} {slid}");
        }
    }
}

/// It is still something heavy on tyres: off the throttle into a hard turn fast enough the back steps out.
#[test]
fn driven_hard_enough_it_still_lets_go_and_loose_ground_lets_go_first() {
    let cfg = truck();
    let slip = |surface| keys_test(&cfg, VehicleTable::UTILITY_TRUCK, surface, 20.0, 0.0, 1.0, false).0;
    let (concrete, grass) = (slip(Surface::Concrete), slip(Surface::Grass));
    // How much sooner grass lets go is in the corner it can hold: see the test of its peak.
    assert!(grass > 4.0 && concrete > 4.0, "{grass} on grass and {concrete} on concrete");
    let (road, _, _) = launch_test(&cfg, VehicleTable::UTILITY_TRUCK, Surface::Concrete);
    let (field, _, _) = launch_test(&cfg, VehicleTable::UTILITY_TRUCK, Surface::Grass);
    assert!(field > road, "pulling away on grass the wheels slip more: {field} and {road}");
}

fn press(state: &mut VehicleState, cfg: &VehicleConfig, world: &CollisionWorld, throttle: f32, shift: i8) {
    let mut events = Vec::new();
    step(state, cfg, &DriveInput { throttle, steer: 0.0, manned: true, handbrake: false, shift, ..Default::default() }, world, DT, &mut events);
}

/// On a long straight, moving at about `speed` in automatic.
fn rolling(cfg: &VehicleConfig, world: &CollisionWorld, speed: f32) -> VehicleState {
    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, cfg, Frame { pos: Vec3::new(-400.0, cfg.ride_height() + 0.05, 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
    for _ in 0..(40.0 / DT) as u32 {
        if state.forward_speed() >= speed {
            break;
        }
        press(&mut state, cfg, world, 1.0, 0);
    }
    state
}

#[test]
fn a_press_is_one_gear_and_after_that_the_box_is_the_drivers() {
    let (cfg, world) = (truck(), flat(Surface::Concrete));
    let mut state = rolling(&cfg, &world, 8.0);
    assert!(!state.engine.manual, "automatic until someone changes gear");
    let gear = state.engine.gear;
    press(&mut state, &cfg, &world, 1.0, 1);
    assert_eq!(state.engine.gear, gear + 1);
    assert!(state.engine.manual);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 12.0);
    assert_eq!(state.engine.gear, gear + 1, "and nothing changes it for them");
    run(&mut state, &cfg, &world, DriveInput { throttle: -1.0, manned: true, ..DriveInput::default() }, 1.5);
    press(&mut state, &cfg, &world, 1.0, -1);
    assert_eq!(state.engine.gear, gear);
}

#[test]
fn each_gear_has_its_own_top_speed_and_its_own_pull() {
    let (cfg, world) = (truck(), flat(Surface::Concrete));
    let held_to = |gear: i8, seconds: f32| {
        let mut state = parked(&cfg, &world, -400.0, 0.0, -FRAC_PI_2);
        press(&mut state, &cfg, &world, 0.0, 1);
        for _ in 2..gear {
            press(&mut state, &cfg, &world, 0.0, 1);
        }
        if gear == 1 {
            press(&mut state, &cfg, &world, 0.0, -1);
        }
        assert_eq!(state.engine.gear, gear);
        run(&mut state, &cfg, &world, drive(1.0, 0.0), seconds);
        state
    };
    let radius = cfg.wheels[0].radius;
    for gear in 1..=3 {
        let state = held_to(gear, 30.0);
        let limit = cfg.engine.redline_rpm / 60.0 * std::f32::consts::TAU / cfg.engine.ratio(gear) * radius;
        assert!(state.forward_speed() <= limit * 1.05, "gear {gear}: {} against {limit}", state.forward_speed());
        assert!(state.engine.rpm <= cfg.engine.redline_rpm + 1.0);
    }
    assert!(held_to(1, 30.0).forward_speed() < held_to(2, 30.0).forward_speed());
    assert!(held_to(1, 2.0).forward_speed() > held_to(4, 2.0).forward_speed() * 1.5, "first pulls away far harder than top");
}

#[test]
fn a_change_down_that_would_overrev_it_does_not_go_in() {
    let (cfg, world) = (truck(), flat(Surface::Concrete));
    let mut state = rolling(&cfg, &world, 22.0);
    let gear = state.engine.gear;
    assert!(gear >= 2, "{gear}");
    let mut events = Vec::new();
    for _ in 0..4 {
        step(&mut state, &cfg, &DriveInput { throttle: 1.0, steer: 0.0, manned: true, handbrake: false, shift: -1, ..Default::default() }, &world, DT, &mut events);
        let wheel_rate = state.wheels.iter().zip(&cfg.wheels).filter(|(_, w)| w.driven).map(|(s, _)| s.spin.abs()).fold(0.0, f32::max);
        let revs = wheel_rate * cfg.engine.ratio(state.engine.gear) * 60.0 / std::f32::consts::TAU;
        assert!(revs <= cfg.engine.redline_rpm * 1.05, "gear {}: {revs}", state.engine.gear);
    }
    assert!(state.engine.gear >= 2, "{}", state.engine.gear);
}

#[test]
fn in_reverse_the_keys_do_nothing_and_the_throttle_still_finds_first() {
    let (cfg, world) = (truck(), flat(Surface::Concrete));
    let mut state = parked(&cfg, &world, -400.0, 0.0, -FRAC_PI_2);
    run(&mut state, &cfg, &world, drive(-1.0, 0.0), 3.0);
    assert_eq!(state.engine.gear, -1);
    press(&mut state, &cfg, &world, -1.0, 1);
    press(&mut state, &cfg, &world, -1.0, -1);
    assert_eq!(state.engine.gear, -1);
    assert!(!state.engine.manual, "a change that did not go in changes nothing");
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 6.0);
    assert_eq!(state.engine.gear, 2, "and out of reverse, the automatic takes it from there");
}

#[test]
fn it_is_automatic_again_for_the_next_driver_or_on_stopping_if_so_set() {
    let (mut cfg, world) = (truck(), flat(Surface::Concrete));
    let mut state = rolling(&cfg, &world, 8.0);
    press(&mut state, &cfg, &world, 1.0, 1);
    run(&mut state, &cfg, &world, drive(0.0, 0.0), 0.5);
    assert!(state.engine.manual);
    run(&mut state, &cfg, &world, DriveInput::default(), 0.1);
    assert!(!state.engine.manual, "nobody at the wheel");

    cfg.engine.manual_until_stop = true;
    let mut state = rolling(&cfg, &world, 8.0);
    press(&mut state, &cfg, &world, 1.0, 1);
    assert!(state.engine.manual);
    run(&mut state, &cfg, &world, DriveInput { throttle: -1.0, manned: true, ..DriveInput::default() }, 4.0);
    assert!(!state.engine.manual);
}

/// What the engine note follows: the revs fall and the foot comes off for a change up.
#[test]
fn a_change_up_can_be_heard_in_the_revs_and_the_throttle() {
    let (cfg, world) = (truck(), flat(Surface::Concrete));
    let mut state = rolling(&cfg, &world, 8.0);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 0.6);
    let rpm = state.engine.rpm;
    press(&mut state, &cfg, &world, 1.0, 1);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 0.2);
    assert!(state.engine.load < 0.6, "{}", state.engine.load);
    run(&mut state, &cfg, &world, drive(1.0, 0.0), 0.3);
    assert!(state.engine.rpm < rpm * 0.85, "{} from {rpm}", state.engine.rpm);
    assert!(state.engine.load > 0.8, "{}", state.engine.load);
}

/// Heading east at `speed`, settled on its springs, with the nose `beta` degrees to the left of the way it is
/// going and the wheels already turned to `steer`.
fn sliding(cfg: &VehicleConfig, world: &CollisionWorld, kind: VehicleKind, speed: f32, beta: f32, steer: f32) -> VehicleState {
    let mut state = VehicleState::new(kind, cfg, Frame { pos: Vec3::new(-400.0, cfg.ride_height(), 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
    run(&mut state, cfg, world, DriveInput::default(), 2.0);
    state.rot = Quat::from_rotation_y(beta.to_radians()) * state.rot;
    state.vel = Vec3::X * speed;
    state.ang_vel = Vec3::ZERO;
    state.steer = steer * cfg.steering.max_angle;
    state.asleep = false;
    state
}

/// Which way it turns to the right, in radians per second, seen from above.
fn yaw_right(state: &VehicleState) -> f32 {
    -state.ang_vel.y
}

/// How far the way it is going is to the right of the way it was going, in radians.
fn path_right(state: &VehicleState, was: Vec3) -> f32 {
    let (a, b) = (glam::Vec2::new(was.x, was.z), glam::Vec2::new(state.vel.x, state.vel.z));
    a.perp_dot(b).atan2(a.dot(b))
}

/// `cargo test -p prs_vehicles drift_report -- --ignored --nocapture`. In a slide, steered each way.
#[test]
#[ignore]
fn drift_report() {
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Grass] {
            let world = flat(surface);
            for beta in [-60.0, -30.0, -12.0, 12.0, 30.0, 60.0] {
                for throttle in [0.0, 1.0] {
                    let line: Vec<String> = [-1.0, 0.0, 1.0].iter().map(|&steer| {
                        let mut state = sliding(&cfg, &world, kind, 18.0, beta, steer);
                        let was = state.vel;
                        let yaw_start = state.frame().yaw();
                        let mut events = Vec::new();
                        let mut first = 0.0;
                        for tick in 0..(1.0 / DT) as u32 {
                            step(&mut state, &cfg, &drive(throttle, steer), &world, DT, &mut events);
                            if tick == (0.25 / DT) as u32 {
                                first = yaw_right(&state);
                            }
                        }
                        let turned = -wrap(state.frame().yaw() - yaw_start);
                        format!("steer {steer:+.0}: yaw@.25s {first:+.2} nose {:+4.0} path {:+4.0} speed {:4.1}", turned.to_degrees(), path_right(&state, was).to_degrees(), state.vel.length())
                    }).collect();
                    println!("{name} {surface:?} beta {beta:+.0} throttle {throttle}: {}", line.join(" | "));
                }
            }
        }
    }
    // As it was with the wheel limited by speed: at 65 km/h they turned no further than this.
    let mut limited = truck();
    limited.steering.max_angle = 0.048;
    let world = flat(Surface::Grass);
    for beta in [-30.0, -12.0, 12.0, 30.0] {
        let line: Vec<String> = [-1.0, 0.0, 1.0].iter().map(|&steer| {
            let mut state = sliding(&limited, &world, VehicleTable::UTILITY_TRUCK, 18.0, beta, steer);
            let (was, heading) = (state.vel, state.frame().yaw());
            run(&mut state, &limited, &world, drive(0.0, steer), 1.0);
            format!("steer {steer:+.0}: nose {:+4.0} path {:+4.0}", (-wrap(state.frame().yaw() - heading)).to_degrees(), path_right(&state, was).to_degrees())
        }).collect();
        println!("truck limited lock beta {beta:+.0}: {}", line.join(" | "));
    }
    // Into it from a straight line, at each speed, a second at a time.
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass] {
            for speed in [5.6, 11.1, 16.7, 25.0] {
                let world = flat(surface);
                let mut state = sliding(&cfg, &world, kind, speed, 0.0, 0.0);
                let mut events = Vec::new();
                let mut trace = String::new();
                let (mut most_tilt, was) = (0.0_f32, state.vel);
                for tick in 0..(4.0 / DT) as u32 {
                    let throttle = if state.forward_speed() < speed { 1.0 } else { 0.0 };
                    step(&mut state, &cfg, &drive(throttle, 1.0), &world, DT, &mut events);
                    most_tilt = most_tilt.max(tilt(&state));
                    if tick % 30 == 29 {
                        let beta = (state.vel.dot(state.rot * Vec3::X)).atan2(state.forward_speed()).to_degrees();
                        trace += &format!(" [{:.1}s yaw {:+.2} beta {:+.0} v {:.1}]", (tick + 1) as f32 * DT, yaw_right(&state), beta, state.vel.length());
                    }
                }
                println!("{name} {surface:?} {:.0} km/h full right: path {:+.0} deg, tilt {most_tilt:.2}{trace}", speed * 3.6, path_right(&state, was).to_degrees());
            }
        }
    }
}

/// `cargo test -p prs_vehicles rolled_report -- --ignored --nocapture`. Dropped on its roof, or tipped over at
/// speed, with the throttle and steering held.
#[test]
#[ignore]
fn rolled_report() {
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Grass] {
            for (what, roll, vel, spin) in [
                ("roof, still", PI, Vec3::ZERO, Vec3::ZERO),
                ("roof at 60 km/h", PI, Vec3::X * 16.7, Vec3::ZERO),
                ("side at 60 km/h", FRAC_PI_2, Vec3::X * 16.7, Vec3::ZERO),
                ("barrel at 60 km/h", 0.6, Vec3::X * 16.7, Vec3::X * 4.0),
            ] {
                let world = flat(surface);
                let mut state = VehicleState::new(kind, &cfg, Frame { pos: Vec3::new(-400.0, 2.5, 0.0), rot: Quat::from_rotation_x(roll) * Quat::from_rotation_y(-FRAC_PI_2) });
                state.vel = vel;
                state.ang_vel = spin;
                state.asleep = false;
                let mut events = Vec::new();
                let (mut top, mut highest, mut fastest, mut crashes) = (0.0_f32, 0.0_f32, 0.0_f32, 0);
                let mut slept_at = None;
                for tick in 0..(12.0 / DT) as u32 {
                    step(&mut state, &cfg, &drive(1.0, 1.0), &world, DT, &mut events);
                    crashes += events.iter().filter(|e| matches!(e, VehicleEvent::Crashed { .. })).count();
                    events.clear();
                    assert!(state.pos.is_finite() && state.rot.is_finite());
                    if tick > 5 {
                        top = top.max(state.vel.length());
                        highest = highest.max(state.pos.y);
                        fastest = fastest.max(state.ang_vel.length());
                    }
                    if state.asleep && slept_at.is_none() {
                        slept_at = Some(tick as f32 * DT);
                    }
                }
                println!("{name} {surface:?} {what}: fastest {top:.1} m/s, spin {fastest:.1} rad/s, highest {highest:.2} m, {crashes} crashes, ends y {:.2} up {:.2} at {:.1} m/s spin {:.2}, slept at {slept_at:?}, hull {:.0}", state.pos.y, (state.rot * Vec3::Y).y, state.vel.length(), state.ang_vel.length(), state.damage.hull);
            }
        }
    }
}

/// Whichever way it goes over, nothing throws it about, and it comes to rest on what it landed on.
#[test]
fn on_its_roof_or_its_side_it_stops_and_stays_stopped() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for (roll, vel) in [(PI, Vec3::ZERO), (PI, Vec3::X * 16.7), (FRAC_PI_2, Vec3::X * 16.7)] {
            let world = flat(Surface::Grass);
            let start = Vec3::new(-400.0, 2.5, 0.0);
            let mut state = VehicleState::new(kind, &cfg, Frame { pos: start, rot: Quat::from_rotation_x(roll) * Quat::from_rotation_y(-FRAC_PI_2) });
            state.vel = vel;
            state.asleep = false;
            let mut events = Vec::new();
            for tick in 0..(8.0 / DT) as u32 {
                step(&mut state, &cfg, &drive(1.0, 1.0), &world, DT, &mut events);
                assert!(state.pos.y <= start.y + 0.05, "thrown up to {}", state.pos.y);
                assert!(state.vel.length() < vel.length() + 7.5, "thrown at {}", state.vel.length());
                assert!(state.ang_vel.length() < 6.0, "spun at {}", state.ang_vel.length());
                assert!(tick < 60 || state.pos.y > 0.5, "into the ground: {}", state.pos.y);
            }
            let (rest, turned) = (state.pos, state.rot);
            run(&mut state, &cfg, &world, drive(1.0, 1.0), 4.0);
            assert!(state.pos.distance(rest) < 0.05, "drove on its roof: {}", state.pos.distance(rest));
            assert!(state.rot.angle_between(turned) < 0.02);
            run(&mut state, &cfg, &world, DriveInput::default(), 4.0);
            assert!(state.asleep, "with nobody at the wheel it settles, at {} m/s", state.vel.length());
        }
    }
}

/// From a straight line, full lock either way at any speed turns that way and keeps on turning that way, and at
/// speed it is the tyres that say how hard.
#[test]
fn full_lock_at_any_speed_turns_the_way_it_is_steered() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Gravel, Surface::Grass] {
            for speed in [5.6, 11.1, 16.7, 25.0] {
                for steer in [-1.0_f32, 1.0] {
                    let world = flat(surface);
                    let mut state = sliding(&cfg, &world, kind, speed, 0.0, 0.0);
                    let was = state.vel;
                    let mut events = Vec::new();
                    for tick in 0..(4.0 / DT) as u32 {
                        let throttle = if state.forward_speed() < speed { 1.0 } else { 0.0 };
                        step(&mut state, &cfg, &drive(throttle, steer), &world, DT, &mut events);
                        if tick > 15 {
                            assert!(yaw_right(&state) * steer > 0.0, "{surface:?} at {speed} steering {steer}: {} at {tick}", yaw_right(&state));
                        }
                        if tick == 60 {
                            assert!(path_right(&state, was) * steer > 0.1, "{surface:?} at {speed} steering {steer}: {}", path_right(&state, was));
                        }
                    }
                }
            }
        }
    }
}

/// Already sliding, the way it goes follows the wheel, however far it is out of line up to 30 degrees and either
/// way the back has stepped out. The nose may not: with the wheels hard over into a slide the front tyres
/// are past their peak and their force is turned aside from the body, so the rear, which is not, brings the tail
/// back in a little. That is the truck straightening up and it is small. At 60 degrees and more every tyre is
/// far past its peak and it goes where it points, whichever way the wheel is turned, which is not tested here.
#[test]
fn in_a_slide_the_way_it_goes_follows_the_wheel() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Grass] {
            let world = flat(surface);
            for beta in [-30.0, -12.0, 12.0, 30.0] {
                for steer in [-1.0_f32, 1.0] {
                    let mut state = sliding(&cfg, &world, kind, 18.0, beta, steer);
                    let (was, heading) = (state.vel, state.frame().yaw());
                    run(&mut state, &cfg, &world, drive(0.0, steer), 1.0);
                    let path = path_right(&state, was);
                    assert!(path * steer > 0.05, "{surface:?} beta {beta} steer {steer}: path {path}");
                    let nose = -wrap(state.frame().yaw() - heading) * steer;
                    assert!(nose > -25f32.to_radians(), "{surface:?} beta {beta} steer {steer}: nose went {nose} the other way");
                }
            }
        }
    }
}

/// Left and right are the same thing mirrored, to within what floats allow.
#[test]
fn a_slide_to_the_left_is_a_slide_to_the_right_in_a_mirror() {
    let cfg = truck();
    let world = flat(Surface::Grass);
    for (beta, steer) in [(25.0, 1.0_f32), (25.0, -1.0), (8.0, 1.0)] {
        let mut right = sliding(&cfg, &world, VehicleTable::UTILITY_TRUCK, 18.0, beta, steer);
        let mut left = sliding(&cfg, &world, VehicleTable::UTILITY_TRUCK, 18.0, -beta, -steer);
        run(&mut right, &cfg, &world, drive(1.0, steer), 1.5);
        run(&mut left, &cfg, &world, drive(1.0, -steer), 1.5);
        assert!((yaw_right(&right) + yaw_right(&left)).abs() < 0.05, "{} {}", yaw_right(&right), yaw_right(&left));
        assert!((right.pos.x - left.pos.x).abs() < 0.1 && (right.pos.z + left.pos.z).abs() < 0.1, "{:?} {:?}", right.pos, left.pos);
    }
}

/// A second of full lock and the handbrake from a straight line: how far the nose came round, how far the back
/// stepped out and the way it ended up going, all to the right if that is the way it was steered.
fn handbrake_turn(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface, speed: f32, steer: f32) -> (f32, f32, f32, f32) {
    let world = flat(surface);
    let mut state = sliding(cfg, &world, kind, speed, 0.0, 0.0);
    let (was, heading) = (state.vel, state.frame().yaw());
    let rear = cfg.wheels.iter().map(|w| w.mount.z).fold(f32::MIN, f32::max);
    let mut events = Vec::new();
    let mut slip = 0.0_f32;
    for _ in 0..(1.0 / DT) as u32 {
        step(&mut state, cfg, &DriveInput { steer, manned: true, handbrake: true, ..DriveInput::default() }, &world, DT, &mut events);
        let at_rear = state.vel + state.ang_vel.cross(state.rot * Vec3::new(0.0, 0.0, rear));
        if at_rear.length() > 1.0 {
            let side = at_rear.dot(state.rot * Vec3::X);
            slip = slip.max(side.atan2(at_rear.dot(state.rot * Vec3::NEG_Z).abs()).abs().to_degrees());
        }
    }
    (-wrap(state.frame().yaw() - heading).to_degrees(), slip, path_right(&state, was).to_degrees(), state.vel.length())
}

/// `cargo test -p prs_vehicles handbrake_report -- --ignored --nocapture`.
#[test]
#[ignore]
fn handbrake_report() {
    for (name, cfg, kind) in [("truck", truck(), VehicleTable::UTILITY_TRUCK), ("car", car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass] {
            for speed in [11.1, 18.0] {
                let (nose, slip, path, left) = handbrake_turn(&cfg, kind, surface, speed, 1.0);
                println!("HB {name} {surface:?} {:.0} km/h: nose {nose:+.0} rear slip {slip:.0} path {path:+.0} speed left {left:.1}, foot brake path {:+.0}", speed * 3.6, foot_brake_turn(&cfg, kind, surface, speed));
            }
        }
    }
}

/// S on the same full lock, for comparison: the foot brake is on all four and is not meant to do this.
fn foot_brake_turn(cfg: &VehicleConfig, kind: VehicleKind, surface: Surface, speed: f32) -> f32 {
    let world = flat(surface);
    let mut state = sliding(cfg, &world, kind, speed, 0.0, 0.0);
    let was = state.vel;
    run(&mut state, cfg, &world, drive(-1.0, 1.0), 1.0);
    path_right(&state, was).to_degrees()
}

/// Steer, hold Space, and the back lets go: the nose comes round into the turn on any ground, and further than
/// the way it is going, which is what makes it a slide and not a corner.
#[test]
fn the_handbrake_kicks_the_tail_out_into_the_turn_on_any_ground() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Gravel, Surface::Grass] {
            for speed in [11.1, 18.0] {
                let (nose, slip, path, _) = handbrake_turn(&cfg, kind, surface, speed, 1.0);
                assert!(nose > 25.0, "{surface:?} at {speed}: nose {nose}");
                assert!(slip > 15.0, "{surface:?} at {speed}: slip {slip}");
                assert!(path > 5.0, "{surface:?} at {speed}: path {path}");
                assert!(nose > path, "{surface:?} at {speed}: nose {nose}, path {path}");
            }
        }
    }
}

/// Steering still counts with it held: which way and how much decide what the truck does.
#[test]
fn with_the_handbrake_on_the_steering_still_steers() {
    let cfg = truck();
    let (right, ..) = handbrake_turn(&cfg, VehicleTable::UTILITY_TRUCK, Surface::Dirt, 18.0, 1.0);
    let (none, ..) = handbrake_turn(&cfg, VehicleTable::UTILITY_TRUCK, Surface::Dirt, 18.0, 0.0);
    let (left, ..) = handbrake_turn(&cfg, VehicleTable::UTILITY_TRUCK, Surface::Dirt, 18.0, -1.0);
    assert!(right > 25.0 && left < -25.0 && none.abs() < 3.0, "{left} {none} {right}");
}

#[test]
fn a_handbrake_turn_to_the_left_is_one_to_the_right_in_a_mirror() {
    let cfg = truck();
    for surface in [Surface::Concrete, Surface::Grass] {
        let right = handbrake_turn(&cfg, VehicleTable::UTILITY_TRUCK, surface, 18.0, 1.0);
        let left = handbrake_turn(&cfg, VehicleTable::UTILITY_TRUCK, surface, 18.0, -1.0);
        assert!((right.0 + left.0).abs() < 1.0 && (right.2 + left.2).abs() < 1.0, "{right:?} {left:?}");
    }
}

/// Held too long it goes all the way round. Let go in time, opposite lock catches it.
#[test]
fn held_too_long_it_spins_and_let_go_it_can_be_caught() {
    let cfg = truck();
    for surface in [Surface::Concrete, Surface::Dirt, Surface::Grass] {
        let world = flat(surface);
        let mut spun = sliding(&cfg, &world, VehicleTable::UTILITY_TRUCK, 18.0, 0.0, 0.0);
        let mut turned = 0.0;
        let mut last = spun.frame().yaw();
        for _ in 0..(3.0 / DT) as u32 {
            run(&mut spun, &cfg, &world, DriveInput { steer: 1.0, manned: true, handbrake: true, ..DriveInput::default() }, DT);
            turned -= wrap(spun.frame().yaw() - last);
            last = spun.frame().yaw();
        }
        assert!(turned.to_degrees() > 100.0, "{surface:?} only came round {}", turned.to_degrees());

        let mut caught = sliding(&cfg, &world, VehicleTable::UTILITY_TRUCK, 18.0, 0.0, 0.0);
        run(&mut caught, &cfg, &world, DriveInput { steer: 1.0, manned: true, handbrake: true, ..DriveInput::default() }, 0.6);
        run(&mut caught, &cfg, &world, drive(1.0, -1.0), 0.8);
        run(&mut caught, &cfg, &world, drive(1.0, 0.0), 1.7);
        let along = caught.vel.dot(caught.rot * Vec3::NEG_Z);
        let side = caught.vel.dot(caught.rot * Vec3::X);
        println!("{surface:?}: caught at {:.0} deg of slip going {:.1} m/s", side.atan2(along).abs().to_degrees(), caught.vel.length());
        assert!(along > 0.0 && side.atan2(along).abs().to_degrees() < 10.0, "{surface:?}: {side} across at {along}");
    }
}

/// On a slope it holds still on the handbrake, as parked does.
#[test]
fn the_handbrake_holds_it_on_a_slope() {
    let cfg = truck();
    let world = hill(14.0, Surface::Dirt);
    let top = Vec3::new(40.0, 400.0, 0.0);
    let y = 400.0 - world.raycast(top, Vec3::NEG_Y, 800.0).unwrap().distance;
    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, &cfg, Frame { pos: Vec3::new(40.0, y + cfg.ride_height() + 0.2, 0.0), rot: Quat::from_rotation_z(14.0_f32.to_radians()) });
    run(&mut state, &cfg, &world, DriveInput { manned: true, handbrake: true, ..DriveInput::default() }, 3.0);
    let at = state.pos;
    run(&mut state, &cfg, &world, DriveInput { manned: true, handbrake: true, ..DriveInput::default() }, 5.0);
    assert!(state.pos.distance(at) < 0.1, "rolled {}", state.pos.distance(at));
}

/// S on full lock goes straight on or wide, either way, at any point until it has all but stopped. A wheel that
/// has locked pushes back along the way it is going, and never off to one side because it is turned.
#[test]
fn braking_hard_on_full_lock_never_goes_the_other_way() {
    for (cfg, kind) in [(truck(), VehicleTable::UTILITY_TRUCK), (car(), VehicleTable::ARMOURED_CAR)] {
        for surface in [Surface::Concrete, Surface::Dirt, Surface::Gravel, Surface::Grass] {
            let world = flat(surface);
            for speed in [11.1, 18.0] {
                let mut mirrored = [0.0_f32; 2];
                for (i, steer) in [1.0_f32, -1.0].into_iter().enumerate() {
                    let mut state = sliding(&cfg, &world, kind, speed, 0.0, 0.0);
                    let was = state.vel;
                    let mut events = Vec::new();
                    for _ in 0..(3.0 / DT) as u32 {
                        step(&mut state, &cfg, &drive(-1.0, steer), &world, DT, &mut events);
                        if state.forward_speed() > 2.0 {
                            let path = path_right(&state, was).to_degrees() * steer;
                            assert!(path > -1.0, "{surface:?} at {speed} steering {steer}: {path} the other way");
                            mirrored[i] = path;
                        }
                    }
                }
                assert!((mirrored[0] - mirrored[1]).abs() < 0.5, "{surface:?} at {speed}: {mirrored:?}");
            }
        }
    }
}

fn jeep() -> VehicleConfig {
    VehicleTable::default()[VehicleTable::LIGHT_JEEP].clone()
}

/// The jeep is quicker than the truck off the line and flat out, has four seats on a lighter, smaller body, and
/// breaks sooner.
#[test]
fn the_jeep_is_quick_small_and_fragile() {
    let (j, t) = (jeep(), truck());
    let world = flat(Surface::Concrete);
    let flat_out = |cfg: &VehicleConfig, kind: VehicleKind, seconds: f32| {
        let mut state = VehicleState::new(kind, cfg, Frame { pos: Vec3::new(-750.0, cfg.ride_height() + 0.05, 0.0), rot: Quat::from_rotation_y(-FRAC_PI_2) });
        run(&mut state, cfg, &world, DriveInput::default(), 1.0);
        run(&mut state, cfg, &world, drive(1.0, 0.0), seconds);
        state.forward_speed()
    };
    let (jeep_top, truck_top) = (flat_out(&j, VehicleTable::LIGHT_JEEP, 40.0), flat_out(&t, VehicleTable::UTILITY_TRUCK, 40.0));
    let (jeep_off, truck_off) = (flat_out(&j, VehicleTable::LIGHT_JEEP, 4.0), flat_out(&t, VehicleTable::UTILITY_TRUCK, 4.0));
    println!("jeep {jeep_top:.1} m/s top, {jeep_off:.1} after 4 s; truck {truck_top:.1}, {truck_off:.1}");
    assert!(jeep_top > truck_top + 2.0 && jeep_off > truck_off, "{jeep_top} {truck_top} {jeep_off} {truck_off}");
    assert!(jeep_top < 40.0, "{jeep_top}");
    assert_eq!(j.seats.len(), 4);
    assert!(j.entries.iter().all(|entry| entry.door.is_none()), "no doors");
    assert!(j.damage.hull < t.damage.hull && j.mass < t.mass);
    let mut parked = VehicleState::new(VehicleTable::LIGHT_JEEP, &j, Frame { pos: Vec3::new(0.0, j.ride_height() + 0.05, 0.0), rot: Quat::IDENTITY });
    run(&mut parked, &j, &world, DriveInput::default(), 3.0);
    assert!(parked.asleep && tilt(&parked) < 0.02, "it settles on its springs");
}

/// Full lock and the handbrake are the same rules on any vehicle: into the turn, never the other way.
#[test]
fn the_jeep_turns_and_slides_the_way_it_is_steered() {
    let j = jeep();
    for surface in [Surface::Concrete, Surface::Grass] {
        let (nose, _, path, _) = handbrake_turn(&j, VehicleTable::LIGHT_JEEP, surface, 18.0, 1.0);
        assert!(nose > 20.0 && path > 0.0, "{surface:?}: {nose} {path}");
        let corner = corner_test(&j, VehicleTable::LIGHT_JEEP, surface, 11.1, None);
        assert!(corner.slid < 3.0, "{surface:?}: {}", corner.slid);
    }
}
