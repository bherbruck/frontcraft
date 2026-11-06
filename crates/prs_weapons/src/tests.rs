use super::*;

const DT: f32 = 1.0 / 60.0;

fn rifle() -> WeaponConfig {
    WeaponConfig::assault_rifle()
}

fn run(s: &mut WeaponState, cfg: &WeaponConfig, input: WeaponInput, seconds: f32) -> Vec<WeaponEvent> {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT).round() as usize {
        step(s, cfg, &input, DT, &mut events);
    }
    events
}

fn aiming() -> WeaponInput {
    WeaponInput { ads_held: true, ..Default::default() }
}

fn shots(events: &[WeaponEvent]) -> usize {
    events.iter().filter(|e| matches!(e, WeaponEvent::Fired { .. })).count()
}

#[test]
fn raised_sights_start_shaky_and_settle_with_time() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    run(&mut s, &cfg, WeaponInput::default(), 3.0);
    let hip = s.instability;
    run(&mut s, &cfg, aiming(), 0.5);
    assert!(s.instability > 0.15, "raising sights must not be instantly steady: {}", s.instability);
    run(&mut s, &cfg, aiming(), 3.0);
    assert!(s.instability < 0.1, "{}", s.instability);
    assert!(hip > s.instability * 5.0);
}

#[test]
fn moving_unsettles_the_gun_and_stopping_recovers_slowly() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    run(&mut s, &cfg, aiming(), 3.0);
    let moving = WeaponInput {
        body: BodyState { speed_fraction: 1.0, ..Default::default() },
        ..aiming()
    };
    run(&mut s, &cfg, moving, 0.5);
    assert!(s.instability > 1.0, "{}", s.instability);
    run(&mut s, &cfg, aiming(), 0.5);
    assert!(s.instability > 0.3, "should still be unsettled: {}", s.instability);
}

/// The core rule: firing and moving never widen the spread. A shot leaves where the gun visibly
/// points, within the weapon's fixed mechanical dispersion and no further.
#[test]
fn a_bullet_leaves_along_the_gun_within_its_fixed_dispersion() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 11);
    s.mode = FireMode::Auto;
    let fire = WeaponInput { fire_held: true, ..Default::default() };
    let mut seen = 0;
    let mut varied = false;
    for _ in 0..120 {
        // The trigger runs last in a step, using the gun's position from that same step.
        let mut events = Vec::new();
        step(&mut s, &cfg, &fire, DT, &mut events);
        for e in events {
            if let WeaponEvent::Fired { gun_offset, direction_offset } = e {
                let off_axis = (direction_offset - gun_offset).length();
                assert!(off_axis <= cfg.dispersion_radius() * 1.001, "{off_axis}");
                varied |= off_axis > 0.0;
                seen += 1;
            }
        }
    }
    assert!(seen > 10);
    assert!(varied, "dispersion should actually scatter rounds");
}

#[test]
fn a_blocked_barrel_lowers_the_gun_and_stops_it_firing() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 3);
    s.mode = FireMode::Auto;
    let blocked = WeaponInput { fire_held: true, muzzle_blocked: true, ..Default::default() };
    assert_eq!(shots(&run(&mut s, &cfg, blocked, 1.0)), 0);
    assert_eq!(s.ready, 0.0);
    let clear = WeaponInput { fire_held: true, ..Default::default() };
    assert_eq!(shots(&run(&mut s, &cfg, clear, cfg.raise_time - 0.05)), 0);
    assert!(shots(&run(&mut s, &cfg, clear, 0.3)) > 0);
}

#[test]
fn single_shot_kicks_sharply_then_recovers() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 7);
    run(&mut s, &cfg, aiming(), 3.0);
    let fire = WeaponInput { fire_held: true, ..aiming() };
    assert_eq!(shots(&run(&mut s, &cfg, fire, DT)), 1);
    run(&mut s, &cfg, aiming(), 0.1);
    assert!(s.recoil.y > 0.5, "kick too soft: {}", s.recoil.y);
    run(&mut s, &cfg, aiming(), 1.0);
    assert!(s.recoil.length() < 0.05, "{:?}", s.recoil);
}

#[test]
fn semi_needs_a_trigger_reset_and_auto_does_not() {
    let cfg = rifle();
    let mut semi = WeaponState::new(&cfg, 3);
    semi.mode = FireMode::Semi;
    let fire = WeaponInput { fire_held: true, ..Default::default() };
    assert_eq!(shots(&run(&mut semi, &cfg, fire, 1.0)), 1);

    let mut auto = WeaponState::new(&cfg, 3);
    auto.mode = FireMode::Auto;
    let n = shots(&run(&mut auto, &cfg, fire, 1.0));
    assert!((11..=14).contains(&n), "750rpm for a second fired {n}");
}

#[test]
fn cannot_fire_while_sprinting_and_must_raise_afterwards() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 3);
    s.mode = FireMode::Auto;
    let sprint = WeaponInput {
        fire_held: true,
        body: BodyState { sprinting: true, speed_fraction: 1.5, ..Default::default() },
        ..Default::default()
    };
    assert_eq!(shots(&run(&mut s, &cfg, sprint, 1.0)), 0);
    let stop = WeaponInput { fire_held: true, ..Default::default() };
    assert_eq!(shots(&run(&mut s, &cfg, stop, cfg.raise_time - 0.05)), 0, "gun came up too fast");
    assert!(shots(&run(&mut s, &cfg, stop, 0.2)) > 0);
}

#[test]
fn magazine_empties_then_dry_fires_then_reloads() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 3);
    s.mode = FireMode::Auto;
    let fire = WeaponInput { fire_held: true, ..Default::default() };
    assert_eq!(shots(&run(&mut s, &cfg, fire, 5.0)), cfg.magazine as usize);
    run(&mut s, &cfg, WeaponInput::default(), 0.1);
    assert!(run(&mut s, &cfg, fire, DT).contains(&WeaponEvent::DryFire));

    let reload = WeaponInput { reload_pressed: true, ..Default::default() };
    assert!(run(&mut s, &cfg, reload, DT).contains(&WeaponEvent::ReloadStarted));
    assert_eq!(shots(&run(&mut s, &cfg, fire, 1.0)), 0, "fired mid reload");
    assert!(run(&mut s, &cfg, WeaponInput::default(), cfg.reload_empty_time).contains(&WeaponEvent::ReloadFinished));
    assert_eq!(s.ammo, cfg.magazine);
}

#[test]
fn holding_breath_steadies_then_punishes_overuse() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 5);
    run(&mut s, &cfg, aiming(), 3.0);
    let normal = s.sway_amp;
    let hold = WeaponInput { breath_held: true, ..aiming() };
    run(&mut s, &cfg, hold, 2.0);
    assert!(s.sway_amp < normal * 0.5, "{} vs {}", s.sway_amp, normal);
    run(&mut s, &cfg, hold, 6.0);
    assert!(s.sway_amp > normal * 1.2, "gasping should be worse than breathing: {}", s.sway_amp);
}

#[test]
fn same_seed_same_shots() {
    let cfg = rifle();
    let (mut a, mut b) = (WeaponState::new(&cfg, 42), WeaponState::new(&cfg, 42));
    a.mode = FireMode::Auto;
    b.mode = FireMode::Auto;
    let fire = WeaponInput { fire_held: true, ..Default::default() };
    assert_eq!(run(&mut a, &cfg, fire, 1.0), run(&mut b, &cfg, fire, 1.0));
}

/// Measures the gun over ten seconds: (largest distance from its average, fastest movement deg/s).
fn gun_motion(cfg: &WeaponConfig, input: WeaponInput) -> (f32, f32) {
    let mut s = WeaponState::new(cfg, 21);
    run(&mut s, cfg, input, 4.0);
    let mut path = Vec::new();
    let mut events = Vec::new();
    for _ in 0..600 {
        step(&mut s, cfg, &input, DT, &mut events);
        path.push(s.recoil + s.sway + s.wander);
    }
    let mean = path.iter().sum::<Vec2>() / path.len() as f32;
    let reach = path.iter().map(|p| p.distance(mean)).fold(0.0, f32::max);
    let speed = path.windows(2).map(|w| w[0].distance(w[1]) / DT).fold(0.0, f32::max);
    (reach, speed)
}

/// Standing still must feel still. Motion has to be slow enough to read as breathing.
#[test]
fn a_soldier_at_rest_holds_the_gun_nearly_still() {
    let cfg = rifle();
    let (hip_reach, hip_speed) = gun_motion(&cfg, WeaponInput::default());
    let (ads_reach, ads_speed) = gun_motion(&cfg, aiming());
    let report = format!("hip {hip_reach:.2} deg, {hip_speed:.2} deg/s; ads {ads_reach:.2} deg, {ads_speed:.2} deg/s");
    assert!(hip_reach < 0.6 && hip_speed < 0.7, "{report}");
    assert!(ads_reach < 0.5 && ads_speed < 0.6, "{report}");
}

#[test]
fn a_reload_takes_the_fullest_magazine_and_keeps_the_partly_used_one() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    s.ammo = 11;
    s.magazines = vec![6, 30, 17];
    let reload = WeaponInput { reload_pressed: true, ..Default::default() };
    run(&mut s, &cfg, reload, DT);
    assert!(s.reloading());
    run(&mut s, &cfg, WeaponInput::default(), cfg.reload_time + 0.1);
    // One stays in the chamber and the other ten went back in their magazine.
    assert_eq!(s.ammo, 31);
    let mut left = s.magazines.clone();
    left.sort();
    assert_eq!(left, vec![6, 10, 17]);
    assert_eq!(s.gun().rounds(), 11 + 6 + 30 + 17, "rounds must not vanish in a reload");
}

#[test]
fn an_empty_magazine_is_thrown_away() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    s.ammo = 0;
    s.magazines = vec![12];
    run(&mut s, &cfg, WeaponInput { reload_pressed: true, ..Default::default() }, DT);
    run(&mut s, &cfg, WeaponInput::default(), cfg.reload_empty_time + 0.1);
    assert_eq!(s.ammo, 12);
    assert!(s.magazines.is_empty(), "an empty magazine is thrown away: {:?}", s.magazines);
}

#[test]
fn there_is_no_reloading_into_a_worse_magazine() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    s.ammo = 21;
    s.magazines = vec![9, 20];
    let events = run(&mut s, &cfg, WeaponInput { reload_pressed: true, ..Default::default() }, DT);
    assert!(!s.reloading());
    assert!(!events.contains(&WeaponEvent::ReloadStarted));
}

#[test]
fn stowing_lowers_the_gun_and_it_cannot_fire_on_the_way_down_or_up() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    let stow = WeaponInput { stowing: true, fire_held: true, ..Default::default() };
    assert_eq!(shots(&run(&mut s, &cfg, stow, cfg.stow_time + 0.02)), 0);
    assert_eq!(s.ready, 0.0);
    let other = WeaponConfig::pistol();
    s.put_in_hands(Gun::issued(WeaponTable::PISTOL, &other));
    assert_eq!(s.weapon, WeaponTable::PISTOL);
    assert_eq!(s.ammo, other.magazine);
    let fire = WeaponInput { fire_held: true, ..Default::default() };
    let early = run(&mut s, &other, fire, other.draw_time * 0.5);
    assert_eq!(shots(&early), 0);
    assert!(s.ready > 0.3 && s.ready < 0.7, "{}", s.ready);
}

/// Wounds shake the real gun. The cone a bullet leaves in stays the weapon's own.
#[test]
fn wounds_move_the_gun_and_never_widen_the_cone() {
    for cfg in WeaponTable::default().weapons {
        let mut s = WeaponState::new(&cfg, 21);
        s.mode = FireMode::Semi;
        let hurt = BodyState { arm_wound: 1.0, blood_loss: 0.9, ..Default::default() };
        let mut held = WeaponInput { ads_held: true, body: hurt, ..Default::default() };
        run(&mut s, &cfg, held, 3.0);
        let mut healthy = WeaponState::new(&cfg, 21);
        run(&mut healthy, &cfg, aiming(), 3.0);
        assert!(s.instability > healthy.instability + 0.5, "{}: {} vs {}", cfg.name, s.instability, healthy.instability);
        for _ in 0..200 {
            held.fire_held = !held.fire_held;
            s.ammo = 5;
            let mut events = Vec::new();
            step(&mut s, &cfg, &held, 0.6, &mut events);
            for event in events {
                if let WeaponEvent::Fired { gun_offset, direction_offset } = event {
                    let off = (direction_offset - gun_offset).length();
                    assert!(off <= cfg.dispersion_radius() * 1.001, "{}: {off}", cfg.name);
                }
            }
        }
    }
}

#[test]
fn putting_the_gun_away_abandons_a_reload_without_touching_a_round() {
    let cfg = rifle();
    let mut s = WeaponState::new(&cfg, 1);
    s.ammo = 9;
    let total = s.gun().rounds();
    run(&mut s, &cfg, WeaponInput { reload_pressed: true, ..Default::default() }, DT);
    assert!(s.reloading());
    let events = run(&mut s, &cfg, WeaponInput { stowing: true, ..Default::default() }, cfg.reload_time + 0.1);
    assert!(!s.reloading());
    assert!(!events.contains(&WeaponEvent::ReloadFinished));
    assert_eq!((s.ammo, s.gun().rounds()), (9, total));
}

/// Being shot at shakes the real gun, for every gun, and the cone a bullet leaves in stays its own.
#[test]
fn suppression_moves_the_gun_and_never_widens_the_cone() {
    for cfg in WeaponTable::default().weapons {
        let mut s = WeaponState::new(&cfg, 21);
        let rattled = BodyState { suppression: 1.0, ..Default::default() };
        let mut held = WeaponInput { ads_held: true, body: rattled, ..Default::default() };
        run(&mut s, &cfg, held, 3.0);
        let mut calm = WeaponState::new(&cfg, 21);
        run(&mut calm, &cfg, aiming(), 3.0);
        assert!(s.instability > calm.instability + 1.0, "{}: {} vs {}", cfg.name, s.instability, calm.instability);
        for _ in 0..200 {
            held.fire_held = !held.fire_held;
            s.ammo = 5;
            let mut events = Vec::new();
            step(&mut s, &cfg, &held, 0.6, &mut events);
            for event in events {
                if let WeaponEvent::Fired { gun_offset, direction_offset } = event {
                    let off = (direction_offset - gun_offset).length();
                    assert!(off <= cfg.dispersion_radius() * 1.001, "{}: {off}", cfg.name);
                }
            }
        }
    }
}
