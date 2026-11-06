//! Setting a gun down on things.

use super::*;
use glam::Quat;
use prs_collision::Capsule;

/// Flat ground and a soldier looking down -Z at `gap` from whatever is built at z = 0 and beyond.
fn scene(build: impl Fn(&mut Sim)) -> (Sim, SoldierId) {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.vehicles.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = vec![Vec3::new(0.0, 0.0, -50.0)];
    let mut sim = Sim::new(level);
    build(&mut sim);
    let id = sim.spawn_soldier(9);
    sim.set_home(id, Vec3::new(0.0, 0.05, 0.45), 0.0);
    for _ in 0..30 {
        sim.step(&[]);
    }
    (sim, id)
}

fn slab(sim: &mut Sim, center: Vec3, half: Vec3) {
    sim.world.add_box(center, half, Quat::IDENTITY, Surface::Concrete);
}

/// A wall this high and 0.3 thick with its near face at z = 0.
fn wall_top(height: f32) -> impl Fn(&mut Sim) {
    move |sim| slab(sim, Vec3::new(0.0, height * 0.5, -0.15), Vec3::new(5.0, height * 0.5, 0.15))
}

/// A wall with a window in it: sill at `sill`, head at `head`, 1.1 wide, near face at z = 0.
fn window(sill: f32, head: f32) -> impl Fn(&mut Sim) {
    move |sim| {
        slab(sim, Vec3::new(0.0, sill * 0.5, -0.15), Vec3::new(5.0, sill * 0.5, 0.15));
        slab(sim, Vec3::new(0.0, (head + 3.2) * 0.5, -0.15), Vec3::new(5.0, (3.2 - head) * 0.5, 0.15));
        for side in [-1.0, 1.0] {
            slab(sim, Vec3::new(side * 3.05, (sill + head) * 0.5, -0.15), Vec3::new(2.5, (head - sill) * 0.5, 0.15));
        }
    }
}

fn cmd(sim: &Sim, yaw: f32, pitch: f32, buttons: &[Buttons], wish: Vec2) -> PlayerCmd {
    let mut held = Buttons::default();
    for &b in buttons {
        held.set(b, true);
    }
    PlayerCmd { tick: sim.tick + 1, wish_move: wish, yaw, pitch, buttons: held, ..Default::default() }
}

fn hold(sim: &mut Sim, id: SoldierId, seconds: f32, yaw: f32, pitch: f32, buttons: &[Buttons]) {
    for _ in 0..(seconds / TICK_DT) as u32 {
        let c = cmd(sim, yaw, pitch, buttons, Vec2::ZERO);
        sim.step(&[(id, c)]);
    }
}

fn press_rest(sim: &mut Sim, id: SoldierId, yaw: f32, pitch: f32, also: &[Buttons]) {
    let mut buttons = also.to_vec();
    buttons.push(Buttons::REST);
    hold(sim, id, TICK_DT * 1.5, yaw, pitch, &buttons);
    hold(sim, id, TICK_DT * 1.5, yaw, pitch, also);
}

fn crouch(sim: &mut Sim, id: SoldierId) {
    hold(sim, id, TICK_DT * 1.5, 0.0, 0.0, &[Buttons::CROUCH]);
    hold(sim, id, 0.6, 0.0, 0.0, &[]);
}

/// Where the fore-end is on the gun right now, in the world.
fn fore_end(sim: &Sim, id: SoldierId) -> Vec3 {
    let soldier = &sim.soldiers[id];
    let pose = soldier.gun_pose(&sim.weapons);
    let cfg = soldier.held_config(&sim.weapons);
    pose.origin + pose.rotation * Vec3::new(0.0, -cfg.rest.under, -soldier.rest.along - soldier.gun_retract)
}

#[test]
fn it_goes_down_on_a_wall_top_and_the_gun_is_really_there() {
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on);
    let pivot = sim.soldiers[id].rest.pivot;
    assert!((pivot.y - 1.3).abs() < 1e-3, "{pivot}");
    hold(&mut sim, id, 1.0, 0.0, 0.0, &[Buttons::ADS]);
    assert!(sim.soldiers[id].rest.amount >= 1.0);
    let touching = fore_end(&sim, id).distance(pivot + Vec3::Y * sim.soldiers[id].held_config(&sim.weapons).rest.clearance);
    assert!(touching < 0.03, "fore-end {touching} m from the contact");
}

#[test]
fn with_nothing_under_it_nothing_happens_and_it_says_so() {
    let (mut sim, id) = scene(|_| {});
    let eye = sim.soldiers[id].eye();
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(!sim.soldiers[id].rest.on);
    assert!(sim.soldiers[id].rest.refused > 0.0, "the player should be told");
    assert_eq!(sim.soldiers[id].eye(), eye);
    // A tall wall right in front is something to be against but nothing to set anything on.
    let (mut sim, id) = scene(wall_top(3.0));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(!sim.soldiers[id].rest.on);
}

/// The barrel swings about the contact, which stays put to within what is left of the sway.
#[test]
fn the_look_swings_the_barrel_about_the_contact_and_the_eye_goes_the_other_way() {
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 1.5, 0.0, 0.0, &[Buttons::ADS]);
    let pivot = sim.soldiers[id].rest.pivot;
    let (eye, muzzle) = (sim.soldiers[id].eye(), sim.soldiers[id].gun_pose(&sim.weapons).muzzle);
    let mut yaw = 0.0;
    for _ in 0..30 {
        yaw -= 0.4 * TICK_DT;
        let c = cmd(&sim, yaw, 0.0, &[Buttons::ADS], Vec2::ZERO);
        sim.step(&[(id, c)]);
        assert!(sim.soldiers[id].rest.on);
    }
    hold(&mut sim, id, 0.3, yaw, 0.0, &[Buttons::ADS]);
    let (eye_now, muzzle_now) = (sim.soldiers[id].eye(), sim.soldiers[id].gun_pose(&sim.weapons).muzzle);
    let touching = fore_end(&sim, id).distance(pivot + Vec3::Y * sim.soldiers[id].held_config(&sim.weapons).rest.clearance);
    assert!(touching < 0.03, "fore-end {touching} m from the contact");
    // Yaw going down turns to the right, so the muzzle goes right and the eye goes left.
    assert!(muzzle_now.x > muzzle.x + 0.01, "{muzzle} to {muzzle_now}");
    assert!(eye_now.x < eye.x - 0.02, "{eye} to {eye_now}");
}

#[test]
fn swinging_past_the_arc_takes_it_off_smoothly() {
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 1.0, 0.0, 0.0, &[Buttons::ADS]);
    let arc = sim.soldiers[id].held_config(&sim.weapons).rest.arc_yaw;
    let mut yaw = 0.0;
    let mut eye = sim.soldiers[id].eye();
    let mut biggest = 0.0f32;
    while sim.soldiers[id].rest.on {
        yaw -= 0.8 * TICK_DT;
        let c = cmd(&sim, yaw, 0.0, &[Buttons::ADS], Vec2::ZERO);
        sim.step(&[(id, c)]);
        biggest = biggest.max(sim.soldiers[id].eye().distance(eye));
        eye = sim.soldiers[id].eye();
        assert!(yaw.abs() < arc + 0.1, "still on at {yaw}");
    }
    for _ in 0..60 {
        let c = cmd(&sim, yaw, 0.0, &[Buttons::ADS], Vec2::ZERO);
        sim.step(&[(id, c)]);
        biggest = biggest.max(sim.soldiers[id].eye().distance(eye));
        eye = sim.soldiers[id].eye();
    }
    assert_eq!(sim.soldiers[id].rest.eye, Vec3::ZERO);
    assert_eq!(sim.soldiers[id].rest.settle, 0.0);
    assert!(biggest < 0.03, "the eye jumped {biggest} m in a tick");
}

#[test]
fn it_comes_off_for_every_reason_it_should() {
    type Reason = fn(&mut Sim, SoldierId);
    let reasons: &[(&str, Reason)] = &[
        ("pressing again", |sim, id| press_rest(sim, id, 0.0, 0.0, &[])),
        ("walking away", |sim, id| {
            for _ in 0..40 {
                let c = cmd(sim, 0.0, 0.0, &[], Vec2::new(1.0, 0.0));
                sim.step(&[(id, c)]);
            }
        }),
        ("sprinting", |sim, id| {
            for _ in 0..10 {
                let c = cmd(sim, 0.0, 0.0, &[Buttons::SPRINT], Vec2::new(0.0, -1.0));
                sim.step(&[(id, c)]);
                let c = cmd(sim, 0.0, 0.0, &[Buttons::SPRINT], Vec2::Y);
                sim.step(&[(id, c)]);
            }
        }),
        ("changing stance", |sim, id| hold(sim, id, TICK_DT * 1.5, 0.0, 0.0, &[Buttons::CROUCH])),
        ("leaning", |sim, id| hold(sim, id, TICK_DT * 1.5, 0.0, 0.0, &[Buttons::LEAN_LEFT])),
        ("changing guns", |sim, id| hold(sim, id, TICK_DT * 1.5, 0.0, 0.0, &[Buttons::SLOT_2])),
        ("going down", |sim, id| sim.hurt(id, Zone::Chest, 120.0)),
    ];
    for (name, reason) in reasons {
        let (mut sim, id) = scene(wall_top(1.3));
        press_rest(&mut sim, id, 0.0, 0.0, &[]);
        assert!(sim.soldiers[id].rest.on, "{name}");
        hold(&mut sim, id, 0.5, 0.0, 0.0, &[]);
        reason(&mut sim, id);
        hold(&mut sim, id, TICK_DT * 2.0, 0.0, 0.0, &[]);
        assert!(!sim.soldiers[id].rest.on, "still rested after {name}");
    }
}

/// Set down on a truck, which then drives off, and the rest goes with it.
#[test]
fn it_goes_down_on_a_vehicle_and_comes_off_when_that_goes_away() {
    let mut sim = Sim::new(Level::named("range").unwrap());
    let id = sim.spawn_soldier(9);
    // The back of a truck is low, so this is from a crouch.
    let mut on = false;
    'looking: for turn in 0..16 {
        for away in [1.6, 1.8, 2.0, 2.4, 2.8, 3.2, 3.6] {
            let angle = turn as f32 * std::f32::consts::TAU / 16.0;
            let (sin, cos) = angle.sin_cos();
            let center = sim.vehicles[0].pos;
            let at = center + Vec3::new(sin, 0.0, cos) * away;
            let top = Vec3::new(at.x, center.y + 8.0, at.z);
            let Some(hit) = sim.world.raycast(top, Vec3::NEG_Y, 30.0) else { continue };
            let feet = top - Vec3::Y * (hit.distance - 0.03);
            if !sim.world.capsule_is_clear(feet, sim.move_cfg.capsule(Stance::Stand)) {
                continue;
            }
            sim.set_home(id, feet, angle);
            if sim.soldiers[id].body.stance != Stance::Crouch {
                crouch(&mut sim, id);
            }
            hold(&mut sim, id, 0.2, angle, 0.0, &[]);
            press_rest(&mut sim, id, angle, 0.0, &[]);
            if sim.soldiers[id].rest.on {
                on = true;
                break 'looking;
            }
            sim.soldiers[id].rest = GunRest::default();
        }
    }
    assert!(on, "nowhere round the truck to rest on");
    let yaw = sim.soldiers[id].yaw;
    hold(&mut sim, id, 0.5, yaw, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on, "should stay down on a parked truck");
    sim.vehicles[0].pos += Vec3::new(30.0, 0.0, 0.0);
    hold(&mut sim, id, TICK_DT * 3.0, yaw, 0.0, &[]);
    assert!(!sim.soldiers[id].rest.on, "the truck has gone");
}

/// At a window, close up, is where the gun gets in trouble. Rested, with the bottom of the gun on the
/// sill, the barrel is out through the opening and the shot goes out. Bullets hitting the wall is no
/// proof of anything, so this looks for one going by on the far side.
#[test]
fn out_of_a_window_with_the_gun_on_the_sill() {
    let (mut sim, id) = scene(window(1.1, 2.15));
    // As near as they can stand, and crouched, with the eye just over the sill.
    sim.soldiers[id].body.pos.z = 0.32;
    crouch(&mut sim, id);
    press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
    assert!(sim.soldiers[id].rest.on, "eye at {}", sim.soldiers[id].eye());
    hold(&mut sim, id, 1.2, 0.0, 0.02, &[Buttons::ADS]);
    let pose = sim.soldiers[id].gun_pose(&sim.weapons);
    assert!(pose.muzzle.z < -0.3, "the muzzle should be out past the wall: {}", pose.muzzle);
    assert_eq!(sim.soldiers[id].weapon.ready, 1.0);
    assert!(sim.world.capsule_is_clear(sim.soldiers[id].body.pos, sim.move_cfg.capsule(Stance::Crouch)));
    let ammo = sim.soldiers[id].weapon.ammo;
    hold(&mut sim, id, TICK_DT * 2.0, 0.0, 0.02, &[Buttons::ADS, Buttons::FIRE]);
    assert!(sim.soldiers[id].weapon.ammo < ammo, "should have fired");
    assert!(sim.bullets.iter().any(|b| b.bullet.pos.z < -0.3), "no bullet out of the window");
    let mut wall = false;
    for _ in 0..4 {
        sim.step(&[]);
        wall |= sim.events.iter().any(|e| matches!(e, SimEvent::Impact { point, .. } if point.z > -0.4));
    }
    assert!(!wall, "hit the wall");
}

/// However it is pointed, a gun set down never gets a bullet out through a wall.
#[test]
fn set_down_it_still_cannot_shoot_through_anything() {
    let (mut sim, id) = scene(window(1.1, 2.15));
    sim.soldiers[id].body.pos.z = 0.32;
    crouch(&mut sim, id);
    press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
    assert!(sim.soldiers[id].rest.on);
    hold(&mut sim, id, 1.0, 0.0, 0.0, &[Buttons::ADS]);
    // Round to one side past the edge of the opening, and down into the sill.
    for (yaw, pitch) in [(0.55, 0.0), (-0.55, 0.0), (0.0, -0.38), (0.5, -0.35), (0.0, 0.29)] {
        let mut fired = Vec::new();
        for i in 0..90 {
            let t = (i as f32 / 40.0).min(1.0);
            let mut buttons = vec![Buttons::ADS];
            if i % 8 == 0 {
                buttons.push(Buttons::FIRE);
            }
            hold(&mut sim, id, TICK_DT * 1.5, yaw * t, pitch * t, &buttons);
            fired.extend(sim.events.iter().filter_map(|e| match e {
                SimEvent::ShotFired { origin, dir, .. } => Some((*origin, *dir)),
                _ => None,
            }));
        }
        let eye = sim.soldiers[id].eye();
        for (origin, dir) in fired {
            // Wherever the bullet started has to be somewhere clear of the wall on the near side of it.
            let inside = origin.z > -0.3 && origin.z < 0.0 && (origin.y < 1.1 || origin.y > 2.15 || origin.x.abs() > 0.55);
            assert!(!inside, "fired from inside the wall at {origin} going {dir}");
            let from_eye = origin - eye;
            assert!(sim.world.raycast(eye, from_eye.normalize(), from_eye.length()).is_none(), "fired through something from {eye} to {origin}");
        }
        press_rest(&mut sim, id, yaw, pitch, &[Buttons::ADS]);
        hold(&mut sim, id, 0.5, 0.0, 0.0, &[Buttons::ADS]);
        if !sim.soldiers[id].rest.on {
            press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
            hold(&mut sim, id, 1.0, 0.0, 0.0, &[Buttons::ADS]);
        }
    }
}

/// Nothing it does is sudden. Going over to it and coming back, the eye moves like a head moving.
#[test]
fn the_eye_eases_over_to_it_and_back() {
    let (mut sim, id) = scene(wall_top(1.25));
    let mut last = sim.soldiers[id].eye();
    let mut fastest = 0.0f32;
    let mut sample = |sim: &mut Sim, buttons: &[Buttons], ticks: u32| {
        for _ in 0..ticks {
            let c = cmd(sim, 0.0, 0.0, buttons, Vec2::ZERO);
            sim.step(&[(id, c)]);
            fastest = fastest.max(sim.soldiers[id].eye().distance(last) / TICK_DT);
            last = sim.soldiers[id].eye();
        }
    };
    sample(&mut sim, &[Buttons::REST], 1);
    sample(&mut sim, &[], 60);
    assert!(sim.soldiers[id].rest.on);
    sample(&mut sim, &[Buttons::REST], 1);
    sample(&mut sim, &[], 60);
    assert!(fastest < 1.5, "the eye went at {fastest} m/s");
}

/// It takes a moment to do anything for you, and shooting knocks it back.
#[test]
fn it_settles_over_time_and_firing_unsettles_it() {
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
    assert_eq!(sim.soldiers[id].rest.settle, 0.0, "nothing at once");
    let cfg = sim.soldiers[id].held_config(&sim.weapons).rest.clone();
    hold(&mut sim, id, cfg.ease_time + cfg.settle_time * 0.5, 0.0, 0.05, &[Buttons::ADS]);
    let halfway = sim.soldiers[id].rest.settle;
    assert!(halfway > 0.2 && halfway < 0.8, "{halfway}");
    hold(&mut sim, id, cfg.settle_time, 0.0, 0.05, &[Buttons::ADS]);
    assert_eq!(sim.soldiers[id].rest.settle, 1.0);
    sim.soldiers[id].weapon.mode = prs_weapons::FireMode::Auto;
    hold(&mut sim, id, 1.0, 0.0, 0.05, &[Buttons::ADS, Buttons::FIRE]);
    assert!(sim.soldiers[id].rest.settle < 0.1, "a burst should use it up: {}", sim.soldiers[id].rest.settle);
    assert!(sim.soldiers[id].rest.on, "and still be down");
}

/// Measured the way a gun moves, since it is all the bullet ever goes by.
fn peak_wander(buttons_rested: bool) -> (f32, f32, f32) {
    let (mut sim, id) = scene(wall_top(1.3));
    if buttons_rested {
        press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
    }
    hold(&mut sim, id, 3.0, 0.0, 0.05, &[Buttons::ADS]);
    let (mut sway, mut wander) = (0.0f32, 0.0f32);
    for _ in 0..240 {
        hold(&mut sim, id, TICK_DT * 1.5, 0.0, 0.05, &[Buttons::ADS]);
        sway = sway.max(sim.soldiers[id].weapon.sway.length());
        wander = wander.max(sim.soldiers[id].weapon.wander.length());
    }
    sim.soldiers[id].weapon.mode = prs_weapons::FireMode::Auto;
    let mut climb = 0.0f32;
    for _ in 0..30 {
        hold(&mut sim, id, TICK_DT * 1.5, 0.0, 0.05, &[Buttons::ADS, Buttons::FIRE]);
        climb = climb.max(sim.soldiers[id].weapon.recoil.y);
    }
    (sway, wander, climb)
}

#[test]
fn rested_is_steadier_through_the_gun() {
    let (sway_free, wander_free, climb_free) = peak_wander(false);
    let (sway, wander, climb) = peak_wander(true);
    assert!(sway < sway_free * 0.5, "sway {sway} against {sway_free}");
    assert!(wander < wander_free * 0.7, "wander {wander} against {wander_free}");
    assert!(climb < climb_free * 0.85, "recoil {climb} against {climb_free}");
    assert!(climb > climb_free * 0.3, "it still has to kick: {climb} against {climb_free}");
}

/// The rule for the whole game holds on a rest as well: one magazine held down does not take out a group.
#[test]
fn a_rested_gun_held_down_does_not_mow_down_a_group() {
    let (mut sim, id) = scene(wall_top(1.3));
    let mates: Vec<_> = (0..6)
        .map(|i| {
            let mate = sim.spawn_soldier(100 + i);
            sim.set_home(mate, Vec3::new(i as f32 * 1.2 - 3.0, 0.05, -60.0), 0.0);
            mate
        })
        .collect();
    press_rest(&mut sim, id, 0.0, 0.0, &[Buttons::ADS]);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    assert_eq!(sim.soldiers[id].rest.settle, 1.0);
    sim.soldiers[id].weapon.mode = prs_weapons::FireMode::Auto;
    let shots = sim.soldiers[id].weapon.ammo;
    let mut hits = 0;
    while sim.soldiers[id].weapon.ammo > 0 {
        let c = cmd(&sim, 0.0, 0.0, &[Buttons::ADS, Buttons::FIRE], Vec2::ZERO);
        sim.step(&[(id, c)]);
        hits += sim.events.iter().filter(|e| matches!(e, SimEvent::SoldierHit { .. })).count();
    }
    hold(&mut sim, id, 0.5, 0.0, 0.0, &[]);
    let down = mates.iter().filter(|&&m| !sim.soldiers[m].health.is_up()).count();
    assert!(down <= 2, "{down} of {} down with {hits} hits from {shots} rounds", mates.len());
}


struct AtWindow {
    tried: u32,
    fired: u32,
    out_past_the_wall: u32,
    hit_own_wall: u32,
}

/// Every window on the town, from the inside, crouched up close: does a shot get out?
fn out_of_every_window(rested: bool) -> AtWindow {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let id = sim.spawn_soldier(9);
    let mut tally = AtWindow { tried: 0, fired: 0, out_past_the_wall: 0, hit_own_wall: 0 };
    let windows = sim.level.windows.clone();
    crouch(&mut sim, id);
    for window in &windows {
        let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
        let inside = window.at - out * (sim.move_cfg.radius + 0.17);
        let Some(hit) = sim.world.raycast(inside, Vec3::NEG_Y, 4.0) else { continue };
        let feet = inside - Vec3::Y * (hit.distance - 0.03);
        if !sim.world.capsule_is_clear(feet, sim.move_cfg.capsule(Stance::Crouch)) {
            continue;
        }
        let yaw = out.x.atan2(out.z) + std::f32::consts::PI;
        sim.soldiers[id].rest = GunRest::default();
        sim.soldiers[id].weapon.mode = prs_weapons::FireMode::Semi;
        sim.soldiers[id].weapon.ammo = 30;
        sim.set_home(id, feet, yaw);
        hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
        tally.tried += 1;
        if rested {
            press_rest(&mut sim, id, yaw, 0.0, &[]);
        }
        hold(&mut sim, id, 1.2, yaw, 0.0, &[Buttons::ADS]);
        let ammo = sim.soldiers[id].weapon.ammo;
        let c = cmd(&sim, yaw, 0.0, &[Buttons::ADS, Buttons::FIRE], Vec2::ZERO);
        sim.step(&[(id, c)]);
        if sim.soldiers[id].weapon.ammo < ammo {
            tally.fired += 1;
            let origin = sim.events.iter().find_map(|e| match e {
                SimEvent::ShotFired { origin, .. } => Some(*origin),
                _ => None,
            });
            let out_past = origin.is_some_and(|at| (at - window.at).dot(out) > 0.1);
            tally.out_past_the_wall += u32::from(out_past);
            let mut wall = false;
            for _ in 0..3 {
                wall |= sim.events.iter().any(|e| matches!(e, SimEvent::Impact { point, .. } if (*point - window.at).dot(out).abs() < 0.3));
                sim.step(&[]);
            }
            tally.hit_own_wall += u32::from(wall);
        }
        let c = cmd(&sim, yaw, 0.0, &[], Vec2::ZERO);
        sim.step(&[(id, c)]);
    }
    tally
}

/// The whole point: real windows on the real map, and a gun on the sill gets a shot out of them.
#[test]
fn out_of_the_towns_windows() {
    let rested = out_of_every_window(true);
    let free = out_of_every_window(false);
    println!("windows tried {}: rested fired {} with the muzzle out {} and {} into the wall; free fired {} muzzle out {} and {} into the wall", rested.tried, rested.fired, rested.out_past_the_wall, rested.hit_own_wall, free.fired, free.out_past_the_wall, free.hit_own_wall);
    assert!(rested.tried > 50, "{}", rested.tried);
    assert!(rested.fired as f32 >= rested.tried as f32 * 0.9, "{} of {}", rested.fired, rested.tried);
    assert!(rested.out_past_the_wall as f32 >= rested.fired as f32 * 0.9, "{} of {}", rested.out_past_the_wall, rested.fired);
    assert_eq!(rested.hit_own_wall, 0);
    assert!(rested.out_past_the_wall >= free.out_past_the_wall, "{} against {}", rested.out_past_the_wall, free.out_past_the_wall);
}

/// All of it can be changed by a mod, down to taking away the settling and the arc.
#[test]
fn a_mod_can_change_it() {
    let text = "weapons.\"*\".rest.settle_time = 0.01\nweapons.\"*\".rest.arc_yaw = 0.05";
    let active = prs_mods::Mod::parse("test", text).unwrap();
    let table = active.over("weapons", WeaponTable::default()).unwrap();
    let (mut sim, id) = scene(wall_top(1.3));
    sim.set_weapons(table);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on);
    hold(&mut sim, id, 1.5, 0.0, 0.0, &[]);
    assert_eq!(sim.soldiers[id].rest.settle, 1.0);
    hold(&mut sim, id, TICK_DT * 2.0, 0.1, 0.0, &[]);
    assert!(!sim.soldiers[id].rest.on);
}

/// Standing on the back of a truck with the gun on its cab, and the truck moves off and turns. A rest is
/// on a fixed point in the world, so it does not go along, and it lifts without doing anything to the soldier.
#[test]
fn set_down_on_a_truck_you_are_riding_it_lifts_when_the_truck_moves() {
    let mut sim = Sim::new(Level::named("range").unwrap());
    let id = sim.spawn_soldier(9);
    let frame = sim.vehicles[0].frame();
    let deck = frame.point(Vec3::new(0.0, 0.32, 1.65));
    let yaw = frame.yaw();
    sim.set_home(id, deck, yaw);
    hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
    assert!(sim.soldiers[id].body.grounded, "should be on the deck at {}", sim.soldiers[id].body.pos);
    crouch(&mut sim, id);
    press_rest(&mut sim, id, yaw, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on, "the cab should be something to rest on, at {}", sim.soldiers[id].body.pos);
    hold(&mut sim, id, 0.5, yaw, 0.0, &[]);
    let (mut last, mut biggest) = (sim.soldiers[id].eye(), 0.0f32);
    let speed = 2.0;
    for _ in 0..120 {
        let forward = sim.vehicles[0].frame().point(Vec3::NEG_Z) - sim.vehicles[0].pos;
        sim.vehicles[0].pos += forward * speed * TICK_DT;
        sim.vehicles[0].vel = forward * speed;
        sim.vehicles[0].asleep = false;
        hold(&mut sim, id, TICK_DT * 1.5, yaw, 0.0, &[]);
        let eye = sim.soldiers[id].eye();
        biggest = biggest.max(eye.distance(last));
        last = eye;
        assert!(eye.is_finite());
    }
    assert!(!sim.soldiers[id].rest.on, "should have lifted");
    assert_eq!(sim.soldiers[id].rest.eye, Vec3::ZERO);
    assert!(biggest < 0.1, "the eye jumped {biggest} m in a tick");
}

/// What goes wrong at the rest, measured: a shot at each point of a grid across the arc, sights clear of
/// the surface, and how many land on the thing the gun is resting on, or find the gun pulled back or blocked.
#[test]
#[ignore]
fn rest_report() {
    fn survey(name: &str, sim: &mut Sim, id: SoldierId, yaw0: f32) {
        let (mut shots, mut sights_clear, mut on_rest, mut pulled, mut blocked, mut lifted) = (0, 0, 0, 0, 0, 0);
        let pivot0 = sim.soldiers[id].rest.pivot;
        let saved = sim.export_state();
        for yi in -4..=4 {
            for pi in -8..=6 {
                sim.import_state(&saved);
                let (yaw, pitch) = (yaw0 + yi as f32 * 0.12, sim.soldiers[id].rest.pitch + pi as f32 * 0.04);
                for i in 1..=60 {
                    let t = i as f32 / 40.0;
                    let t = t.min(1.0);
                    let base = sim.soldiers[id].rest;
                    hold(sim, id, TICK_DT * 1.5, yaw0 + (yaw - yaw0) * t, base.pitch + (pitch - base.pitch) * t, &[Buttons::ADS]);
                }
                if !sim.soldiers[id].rest.on {
                    lifted += 1;
                    continue;
                }
                let retract = sim.soldiers[id].gun_retract;
                let ammo = sim.soldiers[id].weapon.ammo;
                let dir = view_dir(yaw, pitch);
                let eye = sim.soldiers[id].eye();
                let clear = sim.world.walk_ray(eye, dir, 6.0).is_none();
                hold(sim, id, TICK_DT * 1.5, yaw, pitch, &[Buttons::ADS, Buttons::FIRE]);
                let fired = sim.soldiers[id].weapon.ammo < ammo;
                if retract > 0.01 {
                    pulled += 1;
                }
                if !fired {
                    blocked += 1;
                    continue;
                }
                shots += 1;
                sights_clear += i32::from(clear);
                let mut hit = false;
                for _ in 0..6 {
                    hit |= sim.events.iter().any(|e| match e {
                        SimEvent::Impact { point, .. } | SimEvent::VehicleHit { point, .. } => point.distance(pivot0) < 3.5,
                        _ => false,
                    });
                    sim.step(&[]);
                }
                on_rest += i32::from(hit && clear);
            }
        }
        println!("{name}: {shots} shots, sights clear on {sights_clear}, of those {on_rest} hit the rest; pulled back at {pulled}, would not fire at {blocked}, lifted at {lifted}");
    }
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 1.5, 0.0, 0.0, &[Buttons::ADS]);
    survey("thin wall top", &mut sim, id, 0.0);
    let (mut sim, id) = scene(|sim| slab(sim, Vec3::new(0.0, 0.65, -0.6), Vec3::new(5.0, 0.65, 0.6)));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 1.5, 0.0, 0.0, &[Buttons::ADS]);
    survey("deep block 1.2 m", &mut sim, id, 0.0);
    let (mut sim, id) = scene(window(1.1, 2.15));
    sim.soldiers[id].body.pos.z = 0.32;
    crouch(&mut sim, id);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 1.5, 0.0, 0.0, &[Buttons::ADS]);
    survey("window sill", &mut sim, id, 0.0);
    // The side of a truck, along its length so as to have most of it ahead.
    let mut sim = Sim::new(Level::named("range").unwrap());
    let id = sim.spawn_soldier(9);
    let frame = sim.vehicles[0].frame();
    let along = frame.point(Vec3::new(0.3, 0.0, -2.75));
    let top = Vec3::new(along.x, along.y + 8.0, along.z);
    let feet = top - Vec3::Y * (sim.world.raycast(top, Vec3::NEG_Y, 30.0).unwrap().distance - 0.03);
    let yaw = frame.yaw() + std::f32::consts::PI;
    sim.set_home(id, feet, yaw);
    crouch(&mut sim, id);
    hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
    press_rest(&mut sim, id, yaw, 0.0, &[]);
    println!("hood: on {} pivot {} hull top {}", sim.soldiers[id].rest.on, sim.soldiers[id].rest.pivot, frame.point(Vec3::new(0.0, 0.28, 0.0)));
    if sim.soldiers[id].rest.on {
        hold(&mut sim, id, 1.5, yaw, 0.0, &[Buttons::ADS]);
        survey("truck from the front", &mut sim, id, yaw);
    }
}

/// A mouse going through the very stop a client's does, from wherever the look is now.
fn mouse(sim: &Sim, id: SoldierId, look: &mut Vec2, by: Vec2) {
    let wanted = *look + by;
    if let Some(arc) = sim.rest_arc(id, wanted.x) {
        let cfg = &sim.soldiers[id].held_config(&sim.weapons).rest;
        *look = cfg.stop_look(&arc, *look, wanted, TICK_DT);
    } else {
        *look = wanted;
    }
}

/// Everything that is meant to be true at every tick while a gun is on a rest.
fn all_as_it_should_be(sim: &Sim, id: SoldierId, feet: f32, last_eye: &mut Vec3, fastest: &mut f32) {
    let soldier = &sim.soldiers[id];
    let body = &soldier.body;
    assert!(body.grounded, "off the ground at {}", body.pos);
    assert!((body.pos.y - feet).abs() < 0.02, "feet went from {feet} to {}", body.pos.y);
    let held = prs_movement::held_capsule(&sim.move_cfg, body.eye_height);
    assert!(sim.world.capsule_is_clear(body.pos, held), "body in something at {} with the eye {} up", body.pos, body.eye_height);
    let eye = soldier.eye();
    assert!(sim.world.capsule_is_clear(eye - Vec3::Y * 0.05, Capsule { height: 0.1, radius: 0.05 }), "eye in something at {eye}");
    assert!(body.eye_height <= sim.move_cfg.stand_eye + 1e-3, "floating: {}", body.eye_height);
    let least = soldier.held_config(&sim.weapons).rest.low_eye;
    assert!(body.eye_height >= least - 1e-3, "pancake: {}", body.eye_height);
    *fastest = fastest.max(eye.distance(*last_eye) / TICK_DT);
    *last_eye = eye;
}

/// Sweeps the look to every end of the range there is, hard, and back, checking all of it at every tick.
fn sweep_everything(sim: &mut Sim, id: SoldierId, also: &[Buttons]) -> (f32, f32, f32) {
    let base = Vec2::new(sim.soldiers[id].rest.yaw, sim.soldiers[id].rest.pitch);
    let mut look = base;
    let feet = sim.soldiers[id].body.pos.y;
    let mut last = sim.soldiers[id].eye();
    let (mut fastest, mut lowest, mut highest) = (0.0f32, f32::MAX, f32::MIN);
    for by in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X, Vec2::new(1.0, 1.0), Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, 1.0)] {
        for _ in 0..150 {
            // About a radian a second, as a fair flick is on its way to something.
            mouse(sim, id, &mut look, by * TICK_DT);
            let c = cmd(sim, look.x, look.y, also, Vec2::ZERO);
            sim.step(&[(id, c)]);
            assert!(sim.soldiers[id].rest.on, "came off at {look} going {by}");
            all_as_it_should_be(sim, id, feet, &mut last, &mut fastest);
            lowest = lowest.min(sim.soldiers[id].body.eye_height);
            highest = highest.max(sim.soldiers[id].body.eye_height);
        }
    }
    (fastest, lowest, highest)
}

/// A real window on the town with a good long way clear outside it: where to stand, and which way out.
fn town_window(sim: &Sim) -> (Vec3, f32, Vec3) {
    sim.level
        .windows
        .iter()
        .find_map(|window| {
            let out = Vec3::new(window.outward.x, 0.0, window.outward.z).normalize();
            let inside = window.at - out * (sim.move_cfg.radius + 0.17);
            let hit = sim.world.raycast(inside, Vec3::NEG_Y, 3.0)?;
            let feet = inside - Vec3::Y * (hit.distance - 0.03);
            let land = sim.world.land_height(window.at.x, window.at.z)?;
            let (from, to) = (window.at + out * 0.3, window.at + out * 26.0);
            let top = to + Vec3::Y * 20.0;
            let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 60.0)?.distance;
            // Clear to someone out there, standing or crouched.
            let clear = [0.7, 1.2].iter().all(|up| {
                let way = to.with_y(ground + up) - from;
                sim.world.raycast(from, way.normalize(), way.length()).is_none()
            });
            (clear && window.at.y - land < 2.5 && sim.world.capsule_is_clear(feet, sim.move_cfg.capsule(Stance::Stand))).then_some((feet, out.x.atan2(out.z) + std::f32::consts::PI, out))
        })
        .expect("a window to shoot out of")
}

#[test]
fn from_standing_or_crouched_it_comes_to_the_same_place_at_a_real_window() {
    let settled = |crouched: bool| {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let id = sim.spawn_soldier(9);
        let (feet, yaw, _) = town_window(&sim);
        sim.set_home(id, feet, yaw);
        hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
        if crouched {
            hold(&mut sim, id, TICK_DT * 1.5, yaw, 0.0, &[Buttons::CROUCH]);
            hold(&mut sim, id, 0.6, yaw, 0.0, &[]);
        }
        let from = sim.soldiers[id].body.eye_height;
        press_rest(&mut sim, id, yaw, 0.0, &[]);
        assert!(sim.soldiers[id].rest.on, "should find the sill from {from} up");
        hold(&mut sim, id, 3.0, yaw, 0.0, &[Buttons::ADS]);
        let soldier = &sim.soldiers[id];
        (soldier.eye(), soldier.gun_pose(&sim.weapons).muzzle, soldier.body.eye_height, soldier.body.stance, soldier.rest.pivot)
    };
    let (a, b) = (settled(false), settled(true));
    assert!(a.0.distance(b.0) < 0.01, "eye {} and {}", a.0, b.0);
    assert!(a.1.distance(b.1) < 0.02, "muzzle {} and {}", a.1, b.1);
    assert!((a.2 - b.2).abs() < 0.01, "height {} and {}", a.2, b.2);
    assert_eq!(a.3, b.3);
    assert_eq!(a.4, b.4);
}

#[test]
fn swinging_the_whole_range_at_a_real_window_keeps_the_body_on_its_feet() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let id = sim.spawn_soldier(9);
    let (feet, yaw, _) = town_window(&sim);
    sim.set_home(id, feet, yaw);
    hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
    press_rest(&mut sim, id, yaw, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on, "from standing");
    hold(&mut sim, id, 2.0, yaw, 0.0, &[Buttons::ADS]);
    let (fastest, lowest, highest) = sweep_everything(&mut sim, id, &[Buttons::ADS]);
    assert!(fastest <= sim.soldiers[id].held_config(&sim.weapons).rest.eye_speed + 0.3, "the eye went at {fastest} m/s");
    assert!(highest - lowest > 0.25, "it should go up and down with the aim: {lowest} to {highest}");
}

/// Where the eye comes to, and the bones with it, from aiming up and aiming down over the same rest.
#[test]
fn aiming_up_takes_the_body_down_and_aiming_down_brings_it_up_hit_zones_and_all() {
    let (mut sim, id) = scene(wall_top(1.25));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    let at_pitch = |sim: &mut Sim, by: f32| {
        let mut look = Vec2::new(0.0, sim.soldiers[id].pitch);
        for _ in 0..200 {
            mouse(sim, id, &mut look, Vec2::new(0.0, by * TICK_DT));
            let c = cmd(sim, look.x, look.y, &[Buttons::ADS], Vec2::ZERO);
            sim.step(&[(id, c)]);
        }
        assert!(sim.soldiers[id].rest.on);
        let input = sim.body_input(id);
        (sim.soldiers[id].body.eye_height, sim.body_pose(&input)[prs_body::Bone::Pelvis].start.y, look.y)
    };
    let (up_eye, up_hips, up) = at_pitch(&mut sim, 1.0);
    let (down_eye, down_hips, down) = at_pitch(&mut sim, -1.0);
    assert!(up > down + 0.2, "{up} and {down}");
    assert!(up_eye < down_eye - 0.1, "eye {up_eye} aiming up and {down_eye} aiming down");
    assert!(up_hips < down_hips - 0.03, "hips {up_hips} aiming up and {down_hips} aiming down");
}

#[test]
fn a_ceiling_stops_the_rise_and_something_under_the_knee_stops_the_kneel() {
    // A roof low enough to leave only a crouch and a bit, over a wall to rest on.
    let (mut sim, id) = scene(|sim| {
        slab(sim, Vec3::new(0.0, 0.5, -0.15), Vec3::new(5.0, 0.5, 0.15));
        slab(sim, Vec3::new(0.0, 1.75, 0.5), Vec3::new(5.0, 0.2, 2.0));
    });
    crouch(&mut sim, id);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on, "should rest under the roof");
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    let (_, _, highest) = sweep_everything(&mut sim, id, &[Buttons::ADS]);
    assert!(highest + 0.18 < 1.55 + 0.02, "the body would be in the roof at {highest}");

    // A step beside the feet.
    let (mut sim, id) = scene(|sim| {
        slab(sim, Vec3::new(0.0, 1.05, -0.15), Vec3::new(5.0, 1.05 * 0.5 + 0.525, 0.15));
        slab(sim, Vec3::new(0.0, 0.1, 1.0), Vec3::new(5.0, 0.1, 0.3));
    });
    hold(&mut sim, id, 0.2, 0.0, 0.0, &[]);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    if sim.soldiers[id].rest.on {
        let low = sim.soldiers[id].rest.low;
        let least = sim.soldiers[id].held_config(&sim.weapons).rest.low_eye;
        assert!(low >= least);
    }
}

#[test]
fn coming_off_goes_to_the_nearest_stance() {
    // Low enough that the body is right down, from standing.
    let (mut sim, id) = scene(wall_top(0.95));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    assert!(sim.soldiers[id].body.eye_height < 1.25, "{}", sim.soldiers[id].body.eye_height);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[]);
    assert_eq!(sim.soldiers[id].body.stance, Stance::Crouch);
    assert!((sim.soldiers[id].body.eye_height - sim.move_cfg.crouch_eye).abs() < 1e-3);
    assert_eq!(sim.soldiers[id].body.held_eye, 0.0);

    let (mut sim, id) = scene(wall_top(1.42));
    crouch(&mut sim, id);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[]);
    assert_eq!(sim.soldiers[id].body.stance, Stance::Stand);
}

/// A shot over something the gun is resting on: sights on a soldier a good way past it, every round of a
/// magazine, none on the rest and most on them. `what` says if an event is the rest being hit.
fn a_magazine_at(sim: &mut Sim, id: SoldierId, target: Vec3, crouched: bool, on_rest: &dyn Fn(&SimEvent) -> bool) -> (u32, u32, u32) {
    let them = sim.spawn_soldier(77);
    sim.set_home(them, target, 0.0);
    if crouched {
        sim.soldiers[them].body.stance = Stance::Crouch;
        sim.soldiers[them].body.eye_height = sim.move_cfg.crouch_eye;
    }
    let chest = |sim: &Sim| sim.soldiers[them].body.pos + Vec3::Y * (sim.soldiers[them].body.eye_height * 0.75);
    let aim = |sim: &Sim| {
        let to = chest(sim) - sim.soldiers[id].eye();
        Vec2::new((-to.x).atan2(-to.z), to.y.atan2(Vec2::new(to.x, to.z).length()))
    };
    // Over to it and settled, by way of the same stop as a mouse.
    let mut look = Vec2::new(sim.soldiers[id].yaw, sim.soldiers[id].pitch);
    let turn = |to: Vec2| Vec2::new((to.x + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI, to.y);
    for _ in 0..240 {
        let to = turn(aim(sim) - look);
        mouse(sim, id, &mut look, to * 0.1);
        let c = cmd(sim, look.x, look.y, &[Buttons::ADS], Vec2::ZERO);
        sim.step(&[(id, c)]);
    }
    assert!(sim.soldiers[id].rest.on, "lost the rest getting on to them");
    sim.soldiers[id].weapon.ammo = 30;
    let (mut fired, mut hits, mut struck) = (0, 0, 0);
    let mut tick = 0u32;
    while sim.soldiers[id].weapon.ammo > 0 && tick < 3000 {
        tick += 1;
        let to = turn(aim(sim) - look);
        mouse(sim, id, &mut look, to * 0.1);
        let mut buttons = vec![Buttons::ADS];
        if tick % 30 == 1 {
            buttons.push(Buttons::FIRE);
        }
        let c = cmd(sim, look.x, look.y, &buttons, Vec2::ZERO);
        sim.step(&[(id, c)]);
        assert_eq!(sim.soldiers[id].gun_retract, 0.0, "pulled back off its own rest");
        for event in &sim.events {
            fired += u32::from(matches!(event, SimEvent::ShotFired { .. }));
            hits += u32::from(matches!(event, SimEvent::SoldierHit { soldier, .. } if *soldier == them));
            struck += u32::from(on_rest(event));
        }
        sim.soldiers[them].health = prs_health::Health::new(&sim.health_cfg);
    }
    for _ in 0..30 {
        sim.step(&[]);
        for event in &sim.events {
            hits += u32::from(matches!(event, SimEvent::SoldierHit { soldier, .. } if *soldier == them));
            struck += u32::from(on_rest(event));
        }
    }
    sim.soldiers[them].present = false;
    (fired, hits, struck)
}

fn assert_clean((fired, hits, struck): (u32, u32, u32), name: &str) {
    assert_eq!(fired, 30, "{name}");
    assert_eq!(struck, 0, "{name}: {struck} of {fired} hit the rest");
    assert!(hits >= 15, "{name}: only {hits} of {fired} got there");
}

#[test]
fn over_a_flat_wall_top_every_round_goes_on_and_none_hit_it() {
    for (crouched, name) in [(false, "at someone standing"), (true, "at someone crouched")] {
        let (mut sim, id) = scene(wall_top(1.3));
        press_rest(&mut sim, id, 0.0, 0.0, &[]);
        hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
        let near = |e: &SimEvent| matches!(e, SimEvent::Impact { point, .. } if point.z > -1.0);
        assert_clean(a_magazine_at(&mut sim, id, Vec3::new(1.5, 0.05, -30.0), crouched, &near), name);
    }
}

#[test]
fn out_of_a_real_window_every_round_goes_on_and_none_hit_the_sill() {
    for (crouched, name) in [(false, "at someone standing"), (true, "at someone crouched")] {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let id = sim.spawn_soldier(9);
        let (feet, yaw, out) = town_window(&sim);
        sim.set_home(id, feet, yaw);
        hold(&mut sim, id, 0.3, yaw, 0.0, &[]);
        press_rest(&mut sim, id, yaw, 0.0, &[]);
        assert!(sim.soldiers[id].rest.on);
        hold(&mut sim, id, 2.0, yaw, 0.0, &[Buttons::ADS]);
        let at = feet + out * 25.0;
        let top = Vec3::new(at.x, at.y + 20.0, at.z);
        let target = top - Vec3::Y * (sim.world.raycast(top, Vec3::NEG_Y, 60.0).unwrap().distance - 0.05);
        let near = move |e: &SimEvent| matches!(e, SimEvent::Impact { point, .. } if point.distance(feet) < 2.5);
        assert_clean(a_magazine_at(&mut sim, id, target, crouched, &near), name);
    }
}

/// A flat bonnet is the hard one, with a good long way to go over it. Crouched at the side of a truck
/// with the gun on it, across to someone beyond it on the other side.
fn at_a_truck_hood(sim: &mut Sim, id: SoldierId) -> (f32, Vec3) {
    let frame = sim.vehicles[0].frame();
    let side = frame.point(Vec3::new(1.5, 0.0, -1.55));
    let top = side + Vec3::Y * 8.0;
    let feet = top - Vec3::Y * (sim.world.raycast(top, Vec3::NEG_Y, 30.0).unwrap().distance - 0.03);
    let across = (frame.point(Vec3::new(-1.5, 0.0, -1.55)) - side).with_y(0.0).normalize();
    let yaw = across.x.atan2(across.z) + std::f32::consts::PI;
    sim.set_home(id, feet, yaw);
    hold(sim, id, 0.3, yaw, 0.0, &[]);
    press_rest(sim, id, yaw, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on, "should find the bonnet from {feet}");
    hold(sim, id, 2.0, yaw, 0.0, &[Buttons::ADS]);
    (yaw, across)
}

#[test]
fn over_a_truck_hood_every_round_goes_on_and_none_hit_the_truck() {
    for (crouched, name) in [(false, "at someone standing"), (true, "at someone crouched")] {
        let mut sim = Sim::new(Level::named("range").unwrap());
        let id = sim.spawn_soldier(9);
        let (_, across) = at_a_truck_hood(&mut sim, id);
        let at = sim.soldiers[id].body.pos + across * 25.0;
        let top = Vec3::new(at.x, at.y + 20.0, at.z);
        let target = top - Vec3::Y * (sim.world.raycast(top, Vec3::NEG_Y, 60.0).unwrap().distance - 0.05);
        let truck = |e: &SimEvent| matches!(e, SimEvent::VehicleHit { vehicle: 0, .. });
        assert_clean(a_magazine_at(&mut sim, id, target, crouched, &truck), name);
    }
}

/// Over the truck, right across the range the gun will go, with a magazine going all the while.
#[test]
fn swinging_right_over_a_truck_hood_firing_never_touches_it() {
    let mut sim = Sim::new(Level::named("range").unwrap());
    let id = sim.spawn_soldier(9);
    let (yaw, _) = at_a_truck_hood(&mut sim, id);
    // Round towards the front of the truck, as the cab is the other way, and shooting at that is no mistake.
    let front = sim.vehicles[0].frame().point(Vec3::NEG_Z) - sim.vehicles[0].pos;
    let towards = if yaw_basis(yaw + 0.1).0.dot(front) > yaw_basis(yaw).0.dot(front) { Vec2::X } else { Vec2::NEG_X };
    swing_firing(&mut sim, id, &[Vec2::NEG_Y, towards, Vec2::Y, Vec2::NEG_Y], &|e| matches!(e, SimEvent::VehicleHit { vehicle: 0, .. }));
}

/// Across the whole arc with the trigger going and the sights wherever they will go, and never mind what
/// they are on. The rest itself must not pull the gun back, stop it firing or take a round.
fn swing_firing(sim: &mut Sim, id: SoldierId, legs: &[Vec2], on_rest: &dyn Fn(&SimEvent) -> bool) {
    sim.soldiers[id].weapon.mode = prs_weapons::FireMode::Semi;
    let base = Vec2::new(sim.soldiers[id].rest.yaw, sim.soldiers[id].rest.pitch);
    let mut look = base;
    let (mut fired, mut tick) = (0, 0u32);
    for (i, by) in legs.iter().enumerate() {
        for _ in 0..120 {
            tick += 1;
            mouse(sim, id, &mut look, *by * TICK_DT * 1.2);
            let mut buttons = vec![Buttons::ADS];
            if tick % 12 == 1 {
                buttons.push(Buttons::FIRE);
            }
            sim.soldiers[id].weapon.ammo = sim.soldiers[id].weapon.ammo.max(5);
            let c = cmd(sim, look.x, look.y, &buttons, Vec2::ZERO);
            sim.step(&[(id, c)]);
            assert!(sim.soldiers[id].rest.on, "lifted at {look} on leg {i}");
            assert_eq!(sim.soldiers[id].gun_retract, 0.0, "pulled back at {look} on leg {i}");
            assert_eq!(sim.soldiers[id].weapon.ready, 1.0, "blocked at {look} on leg {i}");
            fired += sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { .. })).count();
            let struck = sim.events.iter().find(|e| on_rest(e));
            assert!(struck.is_none(), "hit the rest with the look at {look}: {struck:?}");
        }
    }
    for _ in 0..30 {
        sim.step(&[]);
        assert!(!sim.events.iter().any(on_rest), "hit the rest late on");
    }
    assert!(fired > 30, "only fired {fired}");
}

#[test]
fn swinging_right_over_a_flat_wall_top_firing_never_touches_it() {
    let (mut sim, id) = scene(wall_top(1.3));
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    let legs = [Vec2::NEG_Y, Vec2::X, Vec2::NEG_X, Vec2::NEG_X, Vec2::X, Vec2::Y, Vec2::Y, Vec2::NEG_Y];
    swing_firing(&mut sim, id, &legs, &|e| matches!(e, SimEvent::Impact { point, .. } if point.z > -1.0 && point.y > 0.5));
}

/// The frame of a window is not the rest. Swung into it the barrel still pulls back and will not fire.
#[test]
fn a_gun_swung_into_the_window_frame_still_pulls_back_and_refuses() {
    // Narrow enough to swing into: a real window is too wide for the short end of the gun to reach it.
    let (mut sim, id) = scene(|sim| {
        slab(sim, Vec3::new(0.0, 0.55, -0.15), Vec3::new(5.0, 0.55, 0.15));
        slab(sim, Vec3::new(0.0, 2.7, -0.15), Vec3::new(5.0, 0.55, 0.15));
        for side in [-1.0, 1.0] {
            slab(sim, Vec3::new(side * 2.56, 1.6, -0.15), Vec3::new(2.5, 0.5, 0.15));
        }
    });
    sim.soldiers[id].body.pos.z = 0.32;
    press_rest(&mut sim, id, 0.0, 0.0, &[]);
    assert!(sim.soldiers[id].rest.on);
    hold(&mut sim, id, 2.0, 0.0, 0.0, &[Buttons::ADS]);
    let mut look = Vec2::ZERO;
    let (mut pulled, mut refused) = (false, false);
    for _ in 0..240 {
        mouse(&sim, id, &mut look, Vec2::new(-TICK_DT, 0.0));
        let ammo = sim.soldiers[id].weapon.ammo;
        let c = cmd(&sim, look.x, look.y, &[Buttons::ADS], Vec2::ZERO);
        sim.step(&[(id, c)]);
        let c = cmd(&sim, look.x, look.y, &[Buttons::ADS, Buttons::FIRE], Vec2::ZERO);
        sim.step(&[(id, c)]);
        pulled |= sim.soldiers[id].gun_retract > 0.02;
        refused |= sim.soldiers[id].weapon.ammo == ammo;
    }
    assert!(pulled || sim.soldiers[id].weapon.ready < 1.0, "should have given way to the frame");
    assert!(refused, "should have refused to fire into it");
}
