use super::*;
use glam::Quat;

const DT: f32 = 1.0 / 60.0;

fn flat() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    w.add_box(Vec3::new(0.0, -1.0, 0.0), Vec3::new(500.0, 1.0, 500.0), Quat::IDENTITY, Surface::Dirt);
    w
}

fn soldier(cfg: &MoveConfig) -> MoveState {
    MoveState::new(Vec3::new(0.0, 0.02, 0.0), cfg)
}

/// Runs `input` for `seconds`, returning all events.
fn run(state: &mut MoveState, cfg: &MoveConfig, world: &CollisionWorld, input: MoveInput, seconds: f32) -> Vec<MoveEvent> {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT).round() as usize {
        step(state, cfg, &input, world, DT, &mut events);
    }
    events
}

fn forward() -> MoveInput {
    MoveInput { wish: Vec2::Y, ..default_input() }
}

fn default_input() -> MoveInput {
    MoveInput::default()
}

/// A hillside through the origin, rising towards -Z at this angle, and a soldier stood on it.
fn on_a_hill(cfg: &MoveConfig, radians: f32) -> (CollisionWorld, MoveState) {
    let tilt = Quat::from_rotation_x(radians);
    let mut world = CollisionWorld::default();
    world.add_box(tilt * Vec3::new(0.0, -1.0, 0.0), Vec3::new(500.0, 1.0, 500.0), tilt, Surface::Dirt);
    let mut s = soldier(cfg);
    run(&mut s, cfg, &world, default_input(), 0.3);
    (world, s)
}

#[test]
fn walking_across_a_hill_goes_where_you_are_headed() {
    let cfg = MoveConfig::default();
    for slope in [10.0_f32, 25.0, 40.0] {
        for off_uphill in [30.0_f32, 45.0, 70.0, -45.0] {
            let (world, mut s) = on_a_hill(&cfg, slope.to_radians());
            let yaw = off_uphill.to_radians();
            run(&mut s, &cfg, &world, MoveInput { yaw, ..forward() }, 0.5);
            let from = s.pos;
            run(&mut s, &cfg, &world, MoveInput { yaw, ..forward() }, 2.0);
            let went = s.pos - from;
            let wanted = yaw_basis(yaw).0;
            let off = horizontal(went).normalize().dot(wanted).clamp(-1.0, 1.0).acos().to_degrees();
            assert!(off < 1.5, "{slope} degree hill, {off_uphill} off straight up: pushed {off} degrees sideways");
            assert!(went.y > 0.0, "still going up");
        }
    }
}

#[test]
fn a_hill_still_slows_you_going_up_it_and_more_the_steeper_and_straighter() {
    let cfg = MoveConfig::default();
    let pace = |slope: f32, off_uphill: f32| {
        let (world, mut s) = on_a_hill(&cfg, slope.to_radians());
        let input = MoveInput { yaw: off_uphill.to_radians(), ..forward() };
        run(&mut s, &cfg, &world, input, 1.0);
        let from = s.pos;
        run(&mut s, &cfg, &world, input, 2.0);
        horizontal(s.pos - from).length() / 2.0
    };
    assert!(pace(25.0, 0.0) < cfg.run_speed * 0.9, "{}", pace(25.0, 0.0));
    assert!(pace(40.0, 0.0) < pace(25.0, 0.0));
    assert!(pace(25.0, 0.0) < pace(25.0, 45.0));
    assert!(pace(25.0, 45.0) < pace(25.0, 80.0));
}

#[test]
fn a_crawl_gets_to_the_speed_it_is_set_to_and_stops_when_you_do() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, default_input(), 0.3);
    run(&mut s, &cfg, &world, MoveInput { prone_pressed: true, ..default_input() }, DT);
    run(&mut s, &cfg, &world, default_input(), 2.0);
    assert_eq!(s.stance, Stance::Prone);
    run(&mut s, &cfg, &world, forward(), 1.0);
    assert!((s.horizontal_speed() - cfg.prone_speed).abs() < 0.03, "{}", s.horizontal_speed());
    let at = s.pos;
    run(&mut s, &cfg, &world, default_input(), 0.3);
    assert!(s.pos.distance(at) < 0.1, "slid {}", s.pos.distance(at));
    assert!(s.horizontal_speed() < 0.01);
}

#[test]
fn reaches_run_speed_quickly_then_holds_it() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, forward(), 0.3);
    assert!(s.horizontal_speed() > cfg.run_speed * 0.8, "slow to respond: {}", s.horizontal_speed());
    run(&mut s, &cfg, &world, forward(), 2.0);
    assert!((s.horizontal_speed() - cfg.run_speed).abs() < 0.15, "{}", s.horizontal_speed());
    assert!(s.grounded);
}

#[test]
fn stops_decisively_when_keys_released() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, forward(), 1.0);
    run(&mut s, &cfg, &world, default_input(), 0.35);
    assert!(s.horizontal_speed() < 0.05, "still sliding at {}", s.horizontal_speed());
}

#[test]
fn walls_stop_motion_into_them_but_allow_sliding_along() {
    let cfg = MoveConfig::default();
    let mut world = flat();
    world.add_box(Vec3::new(0.0, 2.0, -3.0), Vec3::new(50.0, 2.0, 0.5), Quat::IDENTITY, Surface::Concrete);
    let mut s = soldier(&cfg);
    // Yaw so that "forward" runs diagonally into the wall.
    let input = MoveInput { yaw: 0.6, ..forward() };
    run(&mut s, &cfg, &world, input, 3.0);
    assert!(s.pos.z > -2.5, "went through the wall: {}", s.pos);
    assert!(s.pos.x < -2.0, "stuck instead of sliding: {}", s.pos);
    assert!(s.pos.y < 0.1, "climbed the wall: {}", s.pos);
}

#[test]
fn walks_up_stairs_without_jumping() {
    let cfg = MoveConfig::default();
    let mut world = flat();
    for i in 0..6 {
        let h = 0.3 * (i + 1) as f32;
        world.add_box(
            Vec3::new(0.0, h * 0.5, -3.0 - i as f32 * 0.6),
            Vec3::new(3.0, h * 0.5, 0.3),
            Quat::IDENTITY,
            Surface::Concrete,
        );
    }
    let mut s = soldier(&cfg);
    let mut events = Vec::new();
    let mut highest: f32 = 0.0;
    let mut airborne = 0;
    for _ in 0..100 {
        step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        highest = highest.max(s.pos.y);
        airborne += i32::from(!s.grounded);
    }
    assert!(highest > 1.7, "did not climb: {highest} at {}", s.pos);
    assert!(airborne < 5, "stairs should not feel like hopping: {airborne} ticks in the air");
    assert!(events.iter().any(|e| matches!(e, MoveEvent::Stepped { .. })));
}

#[test]
fn cannot_step_onto_chest_high_cover() {
    let cfg = MoveConfig::default();
    let mut world = flat();
    world.add_box(Vec3::new(0.0, 0.6, -3.0), Vec3::new(5.0, 0.6, 0.5), Quat::IDENTITY, Surface::Wood);
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, forward(), 2.0);
    assert!(s.pos.y < 0.1 && s.pos.z > -2.5, "{}", s.pos);
}

#[test]
fn stays_glued_to_a_downhill_ramp() {
    let cfg = MoveConfig::default();
    let mut world = flat();
    // Rises towards +Z at about 26 degrees, so yaw 0 (looking down -Z) runs downhill.
    let tilt = Quat::from_rotation_x(-0.45);
    world.add_box(Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 0.5, 30.0), tilt, Surface::Dirt);
    let mut s = MoveState::new(Vec3::new(0.0, 12.0, 12.0), &cfg);
    run(&mut s, &cfg, &world, default_input(), 2.0);
    assert!(s.grounded, "should land on ramp: {}", s.pos);
    let mut airborne = 0;
    let mut events = Vec::new();
    for _ in 0..90 {
        step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        if !s.grounded {
            airborne += 1;
        }
    }
    assert_eq!(airborne, 0, "lost the ground running downhill");
}

#[test]
fn cannot_bunny_hop() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, forward(), 1.0);
    let start = s.pos;
    let mut jumps = 0;
    let mut events = Vec::new();
    for _ in 0..(6.0 / DT) as usize {
        let input = MoveInput { jump_pressed: true, ..forward() };
        events.clear();
        step(&mut s, &cfg, &input, &world, DT, &mut events);
        jumps += events.iter().filter(|e| matches!(e, MoveEvent::Jumped)).count();
    }
    assert!(jumps > 1, "jump never fired");
    assert!(jumps <= 7, "stamina should end jump spam, got {jumps}");
    let average = (s.pos - start).length() / 6.0;
    assert!(average < cfg.run_speed * 0.95, "jump spam must not outrun running: {average}");
}

#[test]
fn jump_works_from_coyote_time_and_input_buffer() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, default_input(), 0.2);
    let mut events = Vec::new();
    step(&mut s, &cfg, &MoveInput { jump_pressed: true, ..default_input() }, &world, DT, &mut events);
    assert!(events.contains(&MoveEvent::Jumped));
    assert!(s.vel.y > 3.0);
    let events = run(&mut s, &cfg, &world, default_input(), 1.2);
    assert!(s.pos.y < 0.05);
    assert!(events.iter().any(|e| matches!(e, MoveEvent::Landed { .. })));
}

#[test]
fn sprint_runs_out_and_locks_until_recovered() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    let sprint = MoveInput { sprint_held: true, ..forward() };
    run(&mut s, &cfg, &world, sprint, 2.0);
    assert!(s.sprinting);
    assert!(s.horizontal_speed() > cfg.run_speed + 1.0);
    run(&mut s, &cfg, &world, sprint, 13.0);
    assert!(!s.sprinting, "stamina {}", s.stamina);
    run(&mut s, &cfg, &world, sprint, 1.5);
    assert!(!s.sprinting, "must not flicker back on at low stamina");
}

#[test]
fn standing_up_is_blocked_under_a_low_ceiling() {
    let cfg = MoveConfig::default();
    let mut world = flat();
    world.add_box(Vec3::new(0.0, 2.3, 0.0), Vec3::new(5.0, 1.0, 5.0), Quat::IDENTITY, Surface::Concrete);
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, default_input(), 0.1);
    run(&mut s, &cfg, &world, MoveInput { crouch_pressed: true, ..default_input() }, DT);
    run(&mut s, &cfg, &world, default_input(), 1.0);
    assert_eq!(s.stance, Stance::Crouch);
    run(&mut s, &cfg, &world, MoveInput { crouch_pressed: true, ..default_input() }, DT);
    run(&mut s, &cfg, &world, default_input(), 1.0);
    assert_eq!(s.stance, Stance::Crouch, "stood up into the ceiling");
}

#[test]
fn prone_is_slow_and_takes_time_to_get_into() {
    let cfg = MoveConfig::default();
    let world = flat();
    let mut s = soldier(&cfg);
    run(&mut s, &cfg, &world, default_input(), 0.1);
    run(&mut s, &cfg, &world, MoveInput { prone_pressed: true, ..default_input() }, DT);
    assert!(s.stance_timer > 0.5);
    run(&mut s, &cfg, &world, forward(), 2.0);
    assert!(s.horizontal_speed() <= cfg.prone_speed + 0.01);
    assert!((s.eye_height - cfg.prone_eye).abs() < 0.01);
}

#[test]
fn each_foot_makes_a_sound() {
    let cfg = MoveConfig::default();
    let (world, mut state) = (flat(), soldier(&cfg));
    run(&mut state, &cfg, &world, forward(), 1.0);
    let before = state.stride;
    let events = run(&mut state, &cfg, &world, forward(), 4.0);
    let steps = events.iter().filter(|e| matches!(e, MoveEvent::Footstep { .. })).count() as f32;
    let strides = state.stride - before;
    assert!((steps - strides * cfg.footsteps_per_stride).abs() <= 1.0, "{steps} steps in {strides} strides");
}

mod climbing {
    use super::*;

    /// A block in the way across the whole world's width, its near face at z = -1.
    fn block(world: &mut CollisionWorld, height: f32, depth: f32) {
        world.add_box(Vec3::new(0.0, height * 0.5, -1.0 - depth * 0.5), Vec3::new(8.0, height * 0.5, depth * 0.5), Quat::IDENTITY, Surface::Concrete);
    }

    fn climber(cfg: &MoveConfig, world: &CollisionWorld) -> MoveState {
        let mut s = soldier(cfg);
        run(&mut s, cfg, world, default_input(), 0.3);
        s
    }

    /// Runs at a wall and presses once, this far from its face, with W held. Whether they climbed, and how high they got.
    fn run_up_and_press(cfg: &MoveConfig, height: f32, depth: f32, from_face: f32) -> (bool, f32) {
        let mut world = flat();
        block(&mut world, height, depth);
        let mut s = soldier(cfg);
        s.pos.z = 5.0;
        run(&mut s, cfg, &world, default_input(), 0.2);
        let mut events = Vec::new();
        let (mut pressed, mut climbed, mut highest) = (false, false, 0.0_f32);
        for _ in 0..360 {
            let now = !pressed && s.pos.z - cfg.radius + 1.0 <= from_face;
            pressed |= now;
            step(&mut s, cfg, &MoveInput { jump_pressed: now, ..forward() }, &world, DT, &mut events);
            climbed |= s.climb.active();
            highest = highest.max(s.pos.y);
        }
        (climbed, highest)
    }

    #[test]
    fn pressing_a_little_too_early_catches_the_ledge() {
        let cfg = MoveConfig::default();
        for height in [0.7, 0.8, 0.9, 1.1, 1.3, 1.5, 1.8, 2.1] {
            for from_face in [0.2, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 2.0] {
                // The lowest can be cleared by the jump alone, which is as good.
                let (_, highest) = run_up_and_press(&cfg, height, 2.0, from_face);
                assert!(highest > height - 0.1, "{height} m wall, pressed {from_face} m out: got up {highest}");
            }
        }
    }

    /// However a run into a wall happens to leave you up against it, to the millimetre, it can be climbed.
    #[test]
    fn up_against_a_wall_always_climbs() {
        let cfg = MoveConfig::default();
        let tallest_to_walk_onto = 0.7;
        for i in 0..32 {
            let height = tallest_to_walk_onto + i as f32 * 0.05;
            for gap in [0.0, 0.001, 0.002, 0.003, 0.005, 0.01, 0.02] {
                let mut world = flat();
                block(&mut world, height, 2.0);
                let mut s = climber(&cfg, &world);
                s.pos.z = -1.0 + cfg.radius + SKIN + gap;
                let mut events = Vec::new();
                step(&mut s, &cfg, &press(), &world, DT, &mut events);
                assert_eq!(s.climb.active(), height <= cfg.climb.max_height, "{height} m wall, {gap} m off it");
            }
        }
    }

    #[test]
    fn a_jump_adds_nothing_to_how_high_you_can_get() {
        let cfg = MoveConfig::default();
        for from_face in [0.2, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 2.0] {
            let (climbed, highest) = run_up_and_press(&cfg, cfg.climb.max_height + 0.05, 2.0, from_face);
            assert!(!climbed && highest < cfg.jump_height + 0.1, "pressed {from_face} m out: got up {highest}");
            // A thin wall can only be gone over up to the chin, jumping or not.
            let (climbed, _) = run_up_and_press(&cfg, cfg.climb.mantle_height + 0.05, 0.25, from_face);
            assert!(!climbed, "over a thin wall, pressed {from_face} m out");
        }
    }

    #[test]
    fn jumping_with_no_wall_is_still_just_a_jump() {
        let cfg = MoveConfig::default();
        let world = flat();
        let mut s = climber(&cfg, &world);
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        for _ in 0..60 {
            step(&mut s, &cfg, &forward(), &world, DT, &mut events);
            assert!(!s.climb.active());
        }
        assert!(events.contains(&MoveEvent::Jumped));
    }

    fn press() -> MoveInput {
        MoveInput { jump_pressed: true, ..forward() }
    }

    /// Presses jump walking forward, and carries on until the climb is over, keeping an eye on the eye.
    fn climb(s: &mut MoveState, cfg: &MoveConfig, world: &CollisionWorld) -> Vec<f32> {
        let mut events = Vec::new();
        let mut eyes = vec![s.eye().y];
        step(s, cfg, &press(), world, DT, &mut events);
        for _ in 0..600 {
            if !s.climb.active() {
                break;
            }
            step(s, cfg, &forward(), world, DT, &mut events);
            eyes.push(s.eye().y);
        }
        eyes
    }

    #[test]
    fn each_height_gets_its_own_way_up_and_ends_on_top() {
        let cfg = MoveConfig::default();
        for (height, kind, end) in [
            (0.9, ClimbKind::Vault, Stance::Stand),
            (1.4, ClimbKind::Mantle, Stance::Stand),
            (2.1, ClimbKind::PullUp, Stance::Crouch),
        ] {
            let mut world = flat();
            block(&mut world, height, 4.0);
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.6;
            let mut events = Vec::new();
            step(&mut s, &cfg, &press(), &world, DT, &mut events);
            assert_eq!(s.climb.kind, kind, "at {height}");
            climb(&mut s, &cfg, &world);
            assert!((s.pos.y - height).abs() < 0.05 && s.pos.z < -1.0, "at {height} ended at {}", s.pos);
            assert_eq!(s.stance, end, "at {height}");
            assert!(s.grounded, "at {height}");
            assert!(s.horizontal_speed() < 1e-3 && s.vel.y.abs() < 1e-3, "no speed out of it: {}", s.vel);
        }
    }

    #[test]
    fn higher_takes_longer() {
        let cfg = MoveConfig::default();
        let mut last = 0.0;
        for height in [0.9, 1.4, 2.1] {
            let mut world = flat();
            block(&mut world, height, 4.0);
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.6;
            let mut events = Vec::new();
            step(&mut s, &cfg, &press(), &world, DT, &mut events);
            assert!(s.climb.duration > last, "{height}: {}", s.climb.duration);
            last = s.climb.duration;
        }
    }

    #[test]
    fn over_the_limit_is_only_a_jump() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, cfg.climb.max_height + 0.1, 4.0);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active());
        assert!(events.contains(&MoveEvent::Jumped));
    }

    #[test]
    fn a_thin_wall_top_is_not_stood_on_but_can_be_gone_over() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 1.0, 0.3);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        climb(&mut s, &cfg, &world);
        assert!(s.pos.z < -1.3 - cfg.radius, "over to the far side: {}", s.pos);
        assert!(s.pos.y < 0.1, "and down: {}", s.pos);

        // Chest high is still over and down. Over the chin there is nothing to do.
        let mut world = flat();
        block(&mut world, 1.5, 0.3);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert_eq!(s.climb.kind, ClimbKind::Mantle);

        let mut world = flat();
        block(&mut world, 2.0, 0.3);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active(), "{:?}", s.climb.kind);
    }

    #[test]
    fn no_climbing_into_a_space_the_body_does_not_fit() {
        let cfg = MoveConfig::default();
        // A slot too low for a crouched body on top of the block.
        let mut world = flat();
        block(&mut world, 1.0, 4.0);
        world.add_box(Vec3::new(0.0, 2.6, -3.0), Vec3::new(8.0, 0.5, 2.0), Quat::IDENTITY, Surface::Concrete);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active(), "{:?}", s.climb.kind);

        // Enough for a crouch and no more, so that is how it ends.
        let mut world = flat();
        block(&mut world, 1.0, 4.0);
        world.add_box(Vec3::new(0.0, 3.0, -3.0), Vec3::new(8.0, 0.7, 2.0), Quat::IDENTITY, Surface::Concrete);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        climb(&mut s, &cfg, &world);
        assert_eq!(s.stance, Stance::Crouch);
        assert!(s.pos.y > 0.9, "{}", s.pos);
        assert!(world.capsule_is_clear(s.pos, cfg.capsule(Stance::Crouch)));
    }

    #[test]
    fn no_climbing_up_through_something_overhead() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 1.2, 4.0);
        // A roof over where you stand, and nothing over where you are going.
        world.add_box(Vec3::new(0.0, 2.3, 3.0), Vec3::new(8.0, 0.1, 3.95), Quat::IDENTITY, Surface::Concrete);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        run(&mut s, &cfg, &world, default_input(), 0.1);
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active(), "came up through the slab at {}", s.pos);
    }

    #[test]
    fn no_climbing_through_a_wall_to_what_is_behind_it() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        // A tall wall with a low step straight behind it.
        world.add_box(Vec3::new(0.0, 3.0, -0.9), Vec3::new(8.0, 3.0, 0.05), Quat::IDENTITY, Surface::Concrete);
        world.add_box(Vec3::new(0.0, 0.5, -3.0), Vec3::new(8.0, 0.5, 2.0), Quat::IDENTITY, Surface::Concrete);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.5;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active(), "through the wall to {}", s.climb.to);
    }

    #[test]
    fn a_slope_is_not_a_ledge() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        // Too steep to walk up, and only a jump however hard you ask.
        world.add_box(Vec3::new(0.0, 1.0, -8.0), Vec3::new(8.0, 0.2, 10.0), Quat::from_rotation_x(1.0), Surface::Dirt);
        let mut s = climber(&cfg, &world);
        run(&mut s, &cfg, &world, forward(), 2.0);
        let mut events = Vec::new();
        for _ in 0..600 {
            step(&mut s, &cfg, &press(), &world, DT, &mut events);
            assert!(!s.climb.active(), "up the slope at {}", s.pos);
        }
    }

    #[test]
    fn it_is_not_for_lying_down_being_hurt_or_having_hands_full() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 1.0, 4.0);
        let spot = |s: &mut MoveState| s.pos.z = -0.6;
        let tried = |s: &mut MoveState, input: MoveInput| {
            let mut events = Vec::new();
            step(s, &cfg, &input, &world, DT, &mut events);
            s.climb.active()
        };

        let mut s = climber(&cfg, &world);
        spot(&mut s);
        run(&mut s, &cfg, &world, MoveInput { prone_pressed: true, ..default_input() }, DT);
        run(&mut s, &cfg, &world, default_input(), 1.0);
        assert!(!tried(&mut s, press()), "from prone");

        let mut s = climber(&cfg, &world);
        spot(&mut s);
        assert!(!tried(&mut s, MoveInput { busy_hands: true, ..press() }), "reloading");
        let mut s = climber(&cfg, &world);
        spot(&mut s);
        assert!(!tried(&mut s, MoveInput { collapsing: true, ..press() }), "going down");
        let mut s = climber(&cfg, &world);
        spot(&mut s);
        assert!(!tried(&mut s, MoveInput { hurt: 1.0, ..press() }), "legs gone");
        let mut s = climber(&cfg, &world);
        spot(&mut s);
        assert!(!tried(&mut s, MoveInput { jump_pressed: true, ..default_input() }), "just jumping");
    }

    #[test]
    fn a_wound_or_a_heavy_kit_lowers_the_limit_and_slows_it() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 2.1, 4.0);
        let start = |input: MoveInput| {
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.6;
            let mut events = Vec::new();
            step(&mut s, &cfg, &input, &world, DT, &mut events);
            s.climb
        };
        assert!(start(press()).active());
        assert!(!start(MoveInput { burden: 0.1, ..press() }).active(), "machine gunner, at the limit");
        assert!(!start(MoveInput { hurt: 0.3, ..press() }).active());

        let mut world = flat();
        block(&mut world, 1.0, 4.0);
        let start = |input: MoveInput| {
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.6;
            let mut events = Vec::new();
            step(&mut s, &cfg, &input, &world, DT, &mut events);
            s.climb
        };
        let (fresh, hurt, heavy) = (start(press()), start(MoveInput { hurt: 0.4, ..press() }), start(MoveInput { burden: 0.1, ..press() }));
        assert!(hurt.active() && hurt.duration > fresh.duration * 1.4, "{} against {}", hurt.duration, fresh.duration);
        assert!(heavy.active() && heavy.duration > fresh.duration * 1.2, "{} against {}", heavy.duration, fresh.duration);
    }

    #[test]
    fn it_costs_stamina_and_exhausted_soldiers_still_can() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 1.0, 4.0);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        climb(&mut s, &cfg, &world);
        assert!(s.stamina < 0.9, "{}", s.stamina);

        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        s.stamina = 0.0;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(s.climb.active());
        assert_eq!(s.stamina, 0.0);
        let winded = s.climb.duration;

        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(winded > s.climb.duration * 1.5, "{winded} against {}", s.climb.duration);
    }

    /// Going up a run of ledges as fast as a player possibly can is well behind the stairs.
    #[test]
    fn a_staircase_of_ledges_is_slower_than_stairs_even_with_no_breath_to_stop_you() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        // More ledges than anyone can get up in the time, so the top is never reached and fallen off.
        for i in 0..60 {
            let h = 0.8 * (i + 1) as f32;
            world.add_box(Vec3::new(0.0, h * 0.5, -2.0 - i as f32), Vec3::new(8.0, h * 0.5, 0.5), Quat::IDENTITY, Surface::Concrete);
        }
        let mut s = climber(&cfg, &world);
        let mut events = Vec::new();
        let seconds = 20.0;
        let mut climbed = 0;
        for tick in 0..(seconds / DT) as u32 {
            let input = MoveInput { jump_pressed: tick % 2 == 0, sprint_held: true, ..forward() };
            let was = s.climb.active();
            step(&mut s, &cfg, &input, &world, DT, &mut events);
            climbed += i32::from(!was && s.climb.active());
        }
        let rate = s.pos.y / seconds;
        // Real stairs are rise 0.2 per run 0.3, which at a run is 2.2 m/s straight up.
        assert!(rate < 0.6, "{rate} m/s up, {climbed} climbs, stamina {}", s.stamina);
    }

    /// Sprinting into a low wall and hopping over it, over and over, gets nowhere faster than a walk.
    #[test]
    fn no_speed_comes_out_of_vaulting() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        for i in 0..30 {
            world.add_box(Vec3::new(0.0, 0.45, -3.0 - i as f32 * 2.5), Vec3::new(8.0, 0.45, 0.15), Quat::IDENTITY, Surface::Concrete);
        }
        let mut s = climber(&cfg, &world);
        let start = s.pos;
        let mut events = Vec::new();
        let seconds = 20.0;
        let mut fastest = 0.0f32;
        for tick in 0..(seconds / DT) as u32 {
            let input = MoveInput { jump_pressed: tick % 2 == 0, sprint_held: true, ..forward() };
            let climbing = s.climb.active();
            step(&mut s, &cfg, &input, &world, DT, &mut events);
            if !climbing && !s.climb.active() {
                fastest = fastest.max(s.horizontal_speed());
            }
        }
        assert!(fastest < cfg.sprint_speed + 0.01, "{fastest}");
        let rate = (s.pos - start).length() / seconds;
        assert!(rate < cfg.run_speed * 0.8, "{rate} m/s over the walls");
    }

    #[test]
    fn something_turning_up_in_the_way_lets_go_with_no_speed() {
        let cfg = MoveConfig::default();
        let mut world = flat();
        block(&mut world, 1.4, 4.0);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        for _ in 0..30 {
            step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        }
        assert!(s.climb.active());
        let height = s.pos.y;
        assert!(height > 0.2, "{height}");

        // A slab comes in overhead while they are halfway up, closing the gap.
        world.add_box(s.pos + Vec3::Y * (s.eye_height + 0.3), Vec3::new(8.0, 0.1, 4.0), Quat::IDENTITY, Surface::Metal);
        step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        assert!(!s.climb.active());
        assert!(s.vel.length() < cfg.gravity * DT * 3.0, "launched: {}", s.vel);
        assert!(s.pos.y <= height + 1e-3);
        for _ in 0..240 {
            step(&mut s, &cfg, &default_input(), &world, DT, &mut events);
        }
        assert!(s.grounded, "still falling at {}", s.pos);
        assert!(s.pos.y < 0.1, "stuck at {}", s.pos);
    }

    /// The eye is what a player feels. No step, nothing sudden, and never up into anything.
    #[test]
    fn the_eye_eases_all_the_way() {
        let cfg = MoveConfig::default();
        for height in [0.9, 1.4, 2.1] {
            let mut world = flat();
            block(&mut world, height, 4.0);
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.6;
            let eyes = climb(&mut s, &cfg, &world);
            let speeds: Vec<f32> = eyes.windows(2).map(|w| (w[1] - w[0]) / DT).collect();
            let top = speeds.iter().fold(0.0f32, |all, v| all.max(v.abs()));
            let hardest = speeds.windows(2).fold(0.0f32, |all, v| all.max((v[1] - v[0]).abs() / DT));
            assert!(top < 4.5, "{height}: {top} m/s");
            assert!(hardest < cfg.gravity * 1.2, "{height}: {hardest} m/s/s against gravity {}", cfg.gravity);
            assert!(speeds[0].abs() < 0.2, "{height}: starts at {}", speeds[0]);
            assert!(speeds[speeds.len() - 1].abs() < 0.2, "{height}: ends at {}", speeds[speeds.len() - 1]);
        }
    }

    #[test]
    fn a_stool_offer_ends_on_the_back_and_the_back_is_what_holds_you_up() {
        let cfg = MoveConfig::default();
        let world = flat();
        let mut s = climber(&cfg, &world);
        let spot = s.pos + Vec3::new(0.0, cfg.stool.back_height, -0.7);
        let offer = StoolOffer { on: 7, spot };
        let mut events = Vec::new();
        step(&mut s, &cfg, &MoveInput { stool: Some(offer), ..press() }, &world, DT, &mut events);
        assert_eq!(s.climb.kind, ClimbKind::Stool);
        while s.climb.active() {
            step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        }
        assert_eq!(s.perch.on, Some(7));
        assert_eq!(s.pos, spot);
        for _ in 0..120 {
            step(&mut s, &cfg, &default_input(), &world, DT, &mut events);
        }
        assert_eq!(s.pos, spot, "stands still up there");
        assert!(s.grounded);

        // No lying down up there, and no jumping off it for height.
        step(&mut s, &cfg, &MoveInput { prone_pressed: true, ..default_input() }, &world, DT, &mut events);
        assert_ne!(s.stance, Stance::Prone);
        events.clear();
        step(&mut s, &cfg, &MoveInput { jump_pressed: true, ..default_input() }, &world, DT, &mut events);
        assert!(!events.contains(&MoveEvent::Jumped));

        // Walking off the edge is just that.
        for _ in 0..30 {
            step(&mut s, &cfg, &forward(), &world, DT, &mut events);
        }
        assert_eq!(s.perch.on, None);
        for _ in 0..90 {
            step(&mut s, &cfg, &default_input(), &world, DT, &mut events);
        }
        assert!(s.grounded && s.pos.y < 0.1, "{}", s.pos);
    }

    /// A rider reaches further, by the height of the back and the shove, than anyone does alone.
    #[test]
    fn from_a_back_a_ledge_over_the_limit_is_in_reach() {
        let cfg = MoveConfig::default();
        let height = cfg.climb.max_height + 0.7;
        let mut world = flat();
        block(&mut world, height, 4.0);
        let mut s = climber(&cfg, &world);
        s.pos.z = -0.6;
        let mut events = Vec::new();
        step(&mut s, &cfg, &press(), &world, DT, &mut events);
        assert!(!s.climb.active(), "alone");

        let mut s = climber(&cfg, &world);
        s.jump_cooldown = 0.0;
        s.pos = Vec3::new(0.0, cfg.stool.back_height, -0.6);
        s.perch = Perch { on: Some(1), spot: s.pos };
        s.grounded = true;
        climb(&mut s, &cfg, &world);
        assert!((s.pos.y - height).abs() < 0.05, "from a back: {}", s.pos);
        assert_eq!(s.perch.on, None);
    }

    #[test]
    fn a_crowd_pushes_gently_and_never_sets_anyone_moving() {
        let cfg = MoveConfig::default();
        let world = flat();
        let mut s = climber(&cfg, &world);
        let mut crowd = Crowding::default();
        let other = Neighbour { feet: s.pos + Vec3::new(0.1, 0.0, 0.0), vel: Vec2::ZERO, height: cfg.stand_height, friend: true, aside: Vec2::X };
        crowd.add(&cfg.crowd, s.pos, cfg.stand_height, 0.0, &other, cfg.sprint_speed);
        assert!(crowd.push.x < 0.0 && crowd.push.length() <= cfg.crowd.idle_push_speed + 1e-4, "{}", crowd.push);
        let start = s.pos;
        let mut events = Vec::new();
        step(&mut s, &cfg, &MoveInput { crowd, ..default_input() }, &world, DT, &mut events);
        assert!(s.pos.x < start.x, "moved {}", s.pos);
        assert!(s.vel.length() < 1e-3, "no speed from it: {}", s.vel);

        // Stood on top of a roof or down a floor below, nobody notices.
        let mut crowd = Crowding::default();
        let above = Neighbour { feet: s.pos + Vec3::new(0.1, cfg.stand_height, 0.0), ..other };
        crowd.add(&cfg.crowd, s.pos, cfg.stand_height, 0.0, &above, cfg.sprint_speed);
        assert_eq!(crowd, Crowding::default());
    }

    /// A gap in a wall over a sill: `high` from sill to lintel and `wide` across, the wall's near face at
    /// z = -1 and 0.3 thick, the sill at 1.0.
    fn gap(world: &mut CollisionWorld, high: f32, wide: f32) {
        let (sill, head) = (1.0, 1.0 + high);
        world.add_box(Vec3::new(0.0, sill * 0.5, -1.15), Vec3::new(8.0, sill * 0.5, 0.15), Quat::IDENTITY, Surface::Concrete);
        world.add_box(Vec3::new(0.0, head + 1.5, -1.15), Vec3::new(8.0, 1.5, 0.15), Quat::IDENTITY, Surface::Concrete);
        for side in [-1.0, 1.0] {
            world.add_box(Vec3::new(side * (wide * 0.5 + 4.0), (sill + head) * 0.5, -1.15), Vec3::new(4.0, high * 0.5, 0.15), Quat::IDENTITY, Surface::Concrete);
        }
    }

    /// A window anyone could get through is gone through flat, and one nobody could is not tried at all.
    #[test]
    fn through_a_gap_a_body_fits_and_not_through_one_it_does_not() {
        let cfg = MoveConfig::default();
        for (high, wide, fits) in [(1.1, 1.1, true), (0.85, 1.0, true), (0.6, 1.1, false), (1.1, 0.5, false)] {
            let mut world = flat();
            gap(&mut world, high, wide);
            let mut s = climber(&cfg, &world);
            s.pos.z = -0.55;
            let mut events = Vec::new();
            step(&mut s, &cfg, &press(), &world, DT, &mut events);
            assert_eq!(s.climb.active(), fits, "{high} high and {wide} wide");
            if fits {
                assert!(s.climb.squeeze > 0.0, "{high} high has no room to go through upright");
                climb(&mut s, &cfg, &world);
                assert!(s.pos.z < -1.3 - cfg.radius && s.grounded, "{high} high: ended at {}", s.pos);
            } else {
                assert!(!events.iter().any(|e| matches!(e, MoveEvent::Climbed { .. })));
            }
        }
    }
}

