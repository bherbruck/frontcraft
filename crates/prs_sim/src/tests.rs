//! These are the design goals written as tests. A tuning change that lets a player mow down a
//! squad, or makes patient shooting unrewarding, fails here.

use super::*;
use prs_health::Life;
use prs_movement::Stance;
use prs_weapons::FireMode;

const TICKS: f32 = 60.0;

/// A flat box floor with one standing target straight down range.
fn range(distance: f32) -> Sim {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = vec![Vec3::new(0.0, 0.0, -distance)];
    level.vehicles.clear();
    Sim::new(level)
}

struct Shooter<'a> {
    sim: &'a mut Sim,
    id: SoldierId,
    yaw: f32,
    pitch: f32,
    wish: Vec2,
    buttons: Buttons,
    /// What the player has seen of the gun lately, for reacting late.
    seen: std::collections::VecDeque<Vec2>,
}

impl<'a> Shooter<'a> {
    fn new(sim: &'a mut Sim, seed: u32) -> Self {
        let id = sim.spawn_soldier(seed);
        Self { sim, id, yaw: 0.0, pitch: 0.0, wish: Vec2::ZERO, buttons: Buttons::default(), seen: Default::default() }
    }

    fn tick(&mut self) {
        let cmd = PlayerCmd {
            tick: self.sim.tick,
            wish_move: self.wish,
            yaw: self.yaw,
            pitch: self.pitch,
            buttons: self.buttons,
            ..Default::default()
        };
        self.sim.step(&[(self.id, cmd)]);
    }

    fn wait(&mut self, seconds: f32) {
        for _ in 0..(seconds * TICKS) as usize {
            self.tick();
        }
    }

    fn aim_at_target(&mut self) {
        let eye = self.sim.soldiers[self.id].body.eye();
        let chest = self.sim.targets[0].pos + Vec3::Y * 1.2;
        let d = chest - eye;
        self.yaw = (-d.x).atan2(-d.z);
        self.pitch = (d.y / d.length()).asin();
    }

    /// Holds the sights on the chest like a player would, correcting for the sway and recoil they
    /// can see, but reacting 100 ms late.
    fn track(&mut self, seconds: f32) {
        for _ in 0..(seconds * TICKS) as usize {
            self.track_tick();
        }
    }

    fn track_tick(&mut self) {
        self.react_late();
        self.tick();
    }

    fn react_late(&mut self) {
        let now = self.weapon().aim_offset();
        self.seen.push_back(now);
        let late = if self.seen.len() > 6 { self.seen.pop_front().unwrap() } else { self.seen[0] };
        self.aim_at_target();
        self.yaw -= late.x;
        self.pitch -= late.y;
    }

    fn stance(&mut self, stance: Stance) {
        self.wait(0.2);
        let button = match stance {
            Stance::Crouch => Buttons::CROUCH,
            Stance::Prone => Buttons::PRONE,
            Stance::Stand => return,
        };
        self.buttons.set(button, true);
        self.tick();
        self.buttons.set(button, false);
        self.wait(1.0);
    }

    fn weapon(&self) -> &WeaponState {
        &self.sim.soldiers[self.id].weapon
    }

    /// Target hits until all current bullets land.
    fn hits(&mut self) -> u32 {
        for _ in 0..(TICKS as usize) {
            self.tick();
        }
        self.sim.targets[0].hits
    }

    /// Resets the target so a downed one does not swallow later bullets.
    fn keep_target_up(&mut self) {
        self.sim.targets[0].down_for = 0.0;
    }
}

/// Fires `rounds` with `aim` run every tick so a bot can try to fight the recoil. Counts how many
/// bullets would have hit a target that stays standing throughout.
fn mag_dump(sim: &mut Sim, seed: u32, rounds: u32, mut aim: impl FnMut(&mut Shooter)) -> u32 {
    let mut s = Shooter::new(sim, seed);
    s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
    s.buttons.set(Buttons::ADS, true);
    s.aim_at_target();
    s.wait(3.0);
    s.buttons.set(Buttons::FIRE, true);
    let start_ammo = s.weapon().ammo;
    while start_ammo - s.weapon().ammo < rounds {
        aim(&mut s);
        s.keep_target_up();
        s.tick();
    }
    s.buttons.set(Buttons::FIRE, false);
    for _ in 0..(TICKS as usize) {
        s.keep_target_up();
        s.tick();
    }
    s.sim.targets[0].hits
}

/// Fires aimed single shots, waiting `gap` seconds between them.
fn aimed_shots(sim: &mut Sim, seed: u32, rounds: u32, stance: Stance, gap: f32) -> u32 {
    let mut s = Shooter::new(sim, seed);
    s.buttons.set(Buttons::ADS, true);
    s.stance(stance);
    s.track(3.0);
    for _ in 0..rounds {
        s.buttons.set(Buttons::FIRE, true);
        s.track_tick();
        s.buttons.set(Buttons::FIRE, false);
        for _ in 0..(gap * TICKS) as usize {
            s.keep_target_up();
            s.track_tick();
        }
    }
    s.hits()
}

fn over_seeds(trials: u32, mut f: impl FnMut(u32) -> u32) -> f32 {
    (0..trials).map(|i| f(i * 7919 + 11) as f32).sum::<f32>() / trials as f32
}

const TRIALS: u32 = 40;
const MAG: u32 = 30;

#[test]
fn settled_first_shot_is_reliable_at_100m() {
    let rate = over_seeds(TRIALS, |seed| aimed_shots(&mut range(100.0), seed, 1, Stance::Crouch, 0.0));
    assert!(rate > 0.8, "first aimed shot should usually land, got {rate}");
}

#[test]
fn patience_is_rewarded_at_100m() {
    let taps = over_seeds(TRIALS, |seed| aimed_shots(&mut range(100.0), seed, 10, Stance::Prone, 1.5)) / 10.0;
    assert!(taps > 0.75, "patient prone shots hit only {taps}");
}

#[test]
fn mag_dump_at_100m_is_a_waste_of_ammo() {
    let rate = over_seeds(TRIALS, |seed| mag_dump(&mut range(100.0), seed, MAG, |_| {})) / MAG as f32;
    assert!(rate < 0.06, "uncompensated spray hit {rate}");
}

/// The strongest fair player: pulls down by exactly the average climb every tick, as if they had
/// memorised the weapon perfectly with no reaction delay.
#[test]
fn perfect_recoil_memorisation_does_not_rescue_a_mag_dump() {
    let rate = over_seeds(TRIALS, |seed| {
        let mut sim = range(100.0);
        // Average climb seen over many seeds with no compensation.
        let profile = climb_profile();
        let mut tick = 0;
        let hits = mag_dump(&mut sim, seed, MAG, |s| {
            s.aim_at_target();
            s.pitch -= profile[tick.min(profile.len() - 1)].to_radians();
            tick += 1;
        });
        hits
    }) / MAG as f32;
    assert!(rate < 0.14, "memorised pull-down hit {rate}");
}

/// A superb player: watches the sights and corrects for all of it, reacting 100 ms late. Shots
/// arrive every 80 ms, so each correction answers a kick that has already been overtaken.
#[test]
fn even_excellent_reactions_cannot_hold_a_magazine_on_target() {
    let rate = over_seeds(TRIALS, |seed| {
            mag_dump(&mut range(100.0), seed, MAG, |s| s.react_late())
    }) / MAG as f32;
    assert!(rate < 0.3, "reacting spray hit {rate}");
}

fn climb_profile() -> Vec<f32> {
    let mut average = Vec::new();
    let trials = 30;
    for seed in 0..trials {
        let mut sim = range(100.0);
        let mut s = Shooter::new(&mut sim, seed * 13 + 1);
        s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
        s.buttons.set(Buttons::ADS, true);
        s.wait(3.0);
        s.buttons.set(Buttons::FIRE, true);
        let mut i = 0;
        while s.weapon().ammo > 0 {
            s.tick();
            if average.len() <= i {
                average.push(0.0);
            }
            average[i] += s.weapon().recoil.y / trials as f32;
            i += 1;
        }
    }
    average
}

#[test]
fn run_and_gun_does_not_work_even_at_25m() {
    let rate = over_seeds(TRIALS, |seed| {
        let mut sim = range(25.0);
        let mut s = Shooter::new(&mut sim, seed);
        s.wish = Vec2::new(1.0, 0.0);
        s.wait(1.0);
        let start = s.weapon().ammo;
        s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
        s.buttons.set(Buttons::FIRE, true);
        while start - s.weapon().ammo < MAG {
            s.aim_at_target();
            s.keep_target_up();
            s.tick();
        }
        s.buttons.set(Buttons::FIRE, false);
        for _ in 0..30 {
            s.keep_target_up();
            s.tick();
        }
        s.sim.targets[0].hits
    }) / MAG as f32;
    assert!(rate < 0.2, "strafing hip fire at 25m hit {rate}");
}

#[test]
fn a_shot_right_after_running_usually_misses_but_waiting_fixes_it() {
    let shot_after = |settle: f32, seed: u32| -> u32 {
        let mut sim = range(100.0);
        let mut s = Shooter::new(&mut sim, seed);
        s.wish = Vec2::new(1.0, 0.0);
        s.wait(2.0);
        s.wish = Vec2::ZERO;
        s.buttons.set(Buttons::ADS, true);
        s.track(settle);
        s.buttons.set(Buttons::FIRE, true);
        s.track(1.0 / TICKS);
        s.buttons.set(Buttons::FIRE, false);
        s.hits()
    };
    let snap = over_seeds(TRIALS, |seed| shot_after(0.3, seed));
    let settled = over_seeds(TRIALS, |seed| shot_after(2.5, seed));
    assert!(snap < 0.3, "snap shot hit {snap}");
    assert!(settled >= 0.4, "settled shot hit {settled}");
    assert!(settled > snap * 2.0);
}

#[test]
fn a_squad_of_five_cannot_be_cleared_with_one_magazine_of_spray() {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = (0..5).map(|i| Vec3::new(-6.0 + i as f32 * 3.0, 0.0, -60.0)).collect();

    let mut cleared = 0;
    for seed in 0..TRIALS {
        let mut sim = Sim::new(level.clone());
        let mut s = Shooter::new(&mut sim, seed * 31 + 5);
        s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
        s.buttons.set(Buttons::ADS, true);
        s.wait(3.0);
        s.buttons.set(Buttons::FIRE, true);
        let start = s.weapon().ammo;
        // Sweeps across the line, as a player trying to mow them down would.
        while start - s.weapon().ammo < MAG {
            let i = (start - s.weapon().ammo) as f32 / MAG as f32;
            let x = -6.0 + 12.0 * i;
            s.yaw = (-x).atan2(60.0);
            s.pitch = 0.0;
            s.tick();
        }
        s.buttons.set(Buttons::FIRE, false);
        s.wait(0.5);
        if s.sim.targets.iter().filter(|t| t.hits > 0).count() == 5 {
            cleared += 1;
        }
    }
    assert!(cleared <= 1, "cleared a squad on the spray {cleared}/{TRIALS} times");
}

/// Fires a burst from settled sights, tracking the target as a player would.
fn burst(sim: &mut Sim, seed: u32, rounds: u32, ads: bool) -> u32 {
    let mut s = Shooter::new(sim, seed);
    s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
    s.buttons.set(Buttons::ADS, ads);
    s.track(3.0);
    let start = s.weapon().ammo;
    s.buttons.set(Buttons::FIRE, true);
    while start - s.weapon().ammo < rounds {
        s.keep_target_up();
        s.track_tick();
    }
    s.buttons.set(Buttons::FIRE, false);
    for _ in 0..30 {
        s.keep_target_up();
        s.tick();
    }
    s.sim.targets[0].hits
}

/// The other side of the anti-spray design: the rifle must still work up close, or close quarters
/// would feel broken rather than dangerous.
#[test]
fn short_bursts_still_work_in_close_quarters() {
    let aimed = over_seeds(TRIALS, |seed| burst(&mut range(15.0), seed, 3, true));
    assert!(aimed >= 2.0, "aimed 3 round burst at 15m landed only {aimed}");
    let hip = over_seeds(TRIALS, |seed| burst(&mut range(6.0), seed, 3, false));
    assert!(hip >= 1.0, "hip burst at 6m landed only {hip}");
}

fn shot_origins(s: &Shooter) -> Vec<(Vec3, Vec3)> {
    s.sim
        .events
        .iter()
        .filter_map(|e| match *e {
            SimEvent::ShotFired { origin, dir, .. } => Some((origin, dir)),
            _ => None,
        })
        .collect()
}

#[test]
fn bullets_leave_from_the_muzzle_not_the_eye() {
    let mut sim = range(100.0);
    let mut s = Shooter::new(&mut sim, 4);
    s.wait(0.5);
    s.yaw = 0.8;
    s.pitch = 0.1;
    s.wait(0.2);
    let before = s.sim.soldiers[s.id].gun_pose(&s.sim.weapons);
    s.buttons.set(Buttons::FIRE, true);
    s.tick();
    let shots = shot_origins(&s);
    assert_eq!(shots.len(), 1);
    let (origin, dir) = shots[0];
    let eye = s.sim.soldiers[s.id].body.eye();
    assert!(origin.distance(eye) > 0.5, "fired from {origin}, eye at {eye}");
    assert!(origin.distance(before.muzzle) < 0.02, "muzzle {} but shot from {origin}", before.muzzle);
    assert!(dir.dot(before.forward) > 0.999);
}

#[test]
fn hip_fire_comes_from_the_side_of_the_body() {
    let mut sim = range(100.0);
    let mut s = Shooter::new(&mut sim, 4);
    s.wait(0.5);
    s.buttons.set(Buttons::FIRE, true);
    s.tick();
    let (origin, _) = shot_origins(&s)[0];
    let eye = s.sim.soldiers[s.id].body.eye();
    assert!(origin.x > eye.x + 0.05, "muzzle should be right of the eye: {origin} vs {eye}");
    assert!(origin.y < eye.y - 0.05, "muzzle should be below the eye: {origin} vs {eye}");
}

fn soldier_at_wall(gap: f32) -> Sim {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = vec![Vec3::new(0.0, 0.0, -50.0)];
    level.solids.push(prs_level::Solid {
        center: Vec3::new(0.0, 2.0, -gap - 0.2),
        half: Vec3::new(5.0, 2.0, 0.2),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    Sim::new(level)
}

#[test]
fn a_gun_never_pokes_through_a_wall_and_cannot_shoot_through_one() {
    // Close enough that the capsule fits but a full-length rifle does not.
    let mut sim = soldier_at_wall(0.35);
    let mut s = Shooter::new(&mut sim, 6);
    s.wait(1.0);
    let pose = s.sim.soldiers[s.id].gun_pose(&s.sim.weapons);
    assert!(pose.muzzle.z > -0.36, "barrel through the wall: {}", pose.muzzle);
    s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
    s.buttons.set(Buttons::FIRE, true);
    let ammo = s.weapon().ammo;
    s.wait(1.0);
    assert_eq!(s.weapon().ammo, ammo, "fired with the barrel blocked");
    assert_eq!(s.hits(), 0);
}

#[test]
fn near_a_wall_the_gun_pulls_back_but_can_still_fire() {
    let mut sim = soldier_at_wall(0.62);
    let mut s = Shooter::new(&mut sim, 6);
    s.wait(1.0);
    let pose = s.sim.soldiers[s.id].gun_pose(&s.sim.weapons);
    assert!(s.sim.soldiers[s.id].gun_retract > 0.05, "gun should have pulled back");
    assert!(pose.muzzle.z > -0.625, "barrel through the wall: {}", pose.muzzle);
    let ammo = s.weapon().ammo;
    s.buttons.set(Buttons::FIRE, true);
    s.wait(0.1);
    assert!(s.weapon().ammo < ammo, "short of the limit, the gun should still fire");
}

#[test]
fn sprinting_soldier_cannot_shoot() {
    let mut sim = range(50.0);
    let mut s = Shooter::new(&mut sim, 3);
    s.wish = Vec2::Y;
    s.buttons.set(Buttons::SPRINT, true);
    s.wait(1.0);
    s.buttons.set(Buttons::FIRE, true);
    let ammo = s.weapon().ammo;
    s.wait(1.0);
    assert_eq!(s.weapon().ammo, ammo);
}

#[test]
fn walls_stop_bullets() {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = vec![Vec3::new(0.0, 0.0, -50.0)];
    level.solids.push(prs_level::Solid {
        center: Vec3::new(0.0, 2.0, -20.0),
        half: Vec3::new(5.0, 2.0, 0.3),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let mut s = Shooter::new(&mut sim, 9);
    s.buttons.set(Buttons::ADS, true);
    s.wait(2.0);
    s.buttons.set(Buttons::FIRE, true);
    s.tick();
    s.buttons.set(Buttons::FIRE, false);
    assert_eq!(s.hits(), 0);
}

#[test]
fn simulation_is_deterministic_for_the_same_commands() {
    let play = || {
        let mut sim = range(100.0);
        let mut s = Shooter::new(&mut sim, 77);
        s.wish = Vec2::new(0.4, 0.8);
        s.wait(1.0);
        s.buttons.set(Buttons::FIRE, true);
        s.wait(0.6);
        let p = s.sim.soldiers[s.id].body.pos;
        let w = s.sim.soldiers[s.id].weapon.clone();
        (p, w.recoil, w.instability, w.ammo)
    };
    assert_eq!(play(), play());
}

/// Tuning aid, prints hit rates: `cargo test -p prs_sim balance_report -- --ignored --nocapture`
#[test]
#[ignore]
fn balance_report() {
    let tap = |range_m: f32, stance| {
        over_seeds(TRIALS, |seed| aimed_shots(&mut range(range_m), seed, 1, stance, 0.0))
    };
    let taps = |range_m: f32, stance, gap| {
        over_seeds(TRIALS, |seed| aimed_shots(&mut range(range_m), seed, 10, stance, gap)) / 10.0
    };
    let spray = |range_m: f32| {
        over_seeds(TRIALS, |seed| mag_dump(&mut range(range_m), seed, MAG, |_| {})) / MAG as f32
    };
    let reacting = |range_m: f32| {
        over_seeds(TRIALS, |seed| {
                    mag_dump(&mut range(range_m), seed, MAG, |s| s.react_late())
        }) / MAG as f32
    };
    let burst3 = |range_m: f32, ads: bool| {
        over_seeds(TRIALS, |seed| burst(&mut range(range_m), seed, 3, ads)) / 3.0
    };
    println!("{:<34}{:>7}{:>7}{:>7}{:>7}{:>7}", "hit rate", "10m", "25m", "50m", "100m", "200m");
    let row = |name: &str, f: &dyn Fn(f32) -> f32| {
        let cells: Vec<String> = [10.0, 25.0, 50.0, 100.0, 200.0].iter().map(|r| format!("{:>6.0}%", f(*r) * 100.0)).collect();
        println!("{name:<34}{}", cells.join(""));
    };
    row("first shot, settled, standing", &|r| tap(r, Stance::Stand));
    row("first shot, settled, crouched", &|r| tap(r, Stance::Crouch));
    row("first shot, settled, prone", &|r| tap(r, Stance::Prone));
    row("10 taps at 1.5s, prone", &|r| taps(r, Stance::Prone, 1.5));
    row("10 taps at 1.5s, standing", &|r| taps(r, Stance::Stand, 1.5));
    row("10 fast taps at 0.25s, standing", &|r| taps(r, Stance::Stand, 0.25));
    row("3 round burst, aimed", &|r| burst3(r, true));
    row("3 round burst, hip", &|r| burst3(r, false));
    row("30 round spray, no compensation", &spray);
    row("30 round spray, 100ms reactions", &reacting);
    for weapon in [RIFLE, LMG, DMR, PISTOL] {
        println!("\n{}", WeaponTable::default()[weapon].name);
        row("first shot, settled, standing", &|r| gun_rate(r, weapon, Stance::Stand, 1, 0.0));
        row("first shot, settled, prone", &|r| gun_rate(r, weapon, Stance::Prone, 1, 0.0));
        row("5 taps at 1.5s, prone", &|r| gun_rate(r, weapon, Stance::Prone, 5, 1.5));
        row("5 fast taps at 0.3s, standing", &|r| gun_rate(r, weapon, Stance::Stand, 5, 0.3));
        if weapon == LMG {
            row("15 round burst, standing ads", &|r| lmg_burst(r, Stance::Stand, true, 15));
            row("15 round burst, prone", &|r| lmg_burst(r, Stance::Prone, true, 15));
            row("15 round burst, hip", &|r| lmg_burst(r, Stance::Stand, false, 15));
        }
    }
}

/// Steps a soldier through a tick with these buttons and move.
fn drive(sim: &mut Sim, id: SoldierId, wish: Vec2, yaw: f32, pitch: f32, buttons: Buttons) {
    let cmd = PlayerCmd { tick: sim.tick, wish_move: wish, yaw, pitch, buttons, ..Default::default() };
    sim.step(&[(id, cmd)]);
}

#[test]
fn hands_stay_on_the_gun_as_the_sim_moves_it() {
    use prs_body::Bone;
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(9);
    let mut ads = Buttons::default();
    ads.set(Buttons::ADS, true);
    let mut buttons = ads;
    for tick in 0..400 {
        // Stand a moment, aim, fire, and drift the look about.
        buttons.set(Buttons::FIRE, tick > 120 && tick % 40 == 0);
        let (yaw, pitch) = ((tick as f32 * 0.01).sin() * 0.6, (tick as f32 * 0.013).sin() * 0.4);
        drive(&mut sim, id, Vec2::ZERO, yaw, pitch, buttons);
        let input = sim.body_input(id);
        let pose = sim.body_pose(&input);
        let g = &input.gun;
        let wrist = g.origin + g.rotation * g.grips.trigger_wrist;
        let gap = pose[Bone::HandR].start.distance(wrist);
        assert!(gap < 0.01, "trigger hand is {gap} off the gun at tick {tick}");
        let support = g.origin + g.rotation * g.grips.support_wrist;
        let gap = pose[Bone::HandL].start.distance(support);
        assert!(gap < sim.body_cfg.look.support_slide + 0.01, "off hand is {gap} off the gun at tick {tick}");
    }
}

#[test]
fn everyone_running_the_same_sim_computes_the_same_body() {
    let script = |sim: &mut Sim, id| {
        for tick in 0..300 {
            let wish = if tick < 150 { Vec2::Y } else { Vec2::X };
            let mut buttons = Buttons::default();
            buttons.set(Buttons::SPRINT, tick < 90);
            buttons.set(Buttons::JUMP, tick == 100);
            buttons.set(Buttons::CROUCH, tick == 200);
            drive(sim, id, wish, tick as f32 * 0.005, 0.1, buttons);
        }
    };
    let (mut a, mut b) = (range(100.0), range(100.0));
    let (ida, idb) = (a.spawn_soldier(4), b.spawn_soldier(4));
    script(&mut a, ida);
    script(&mut b, idb);
    assert_eq!(a.body_pose(&a.body_input(ida)), b.body_pose(&b.body_input(idb)));
}

#[test]
fn working_out_a_body_changes_nothing_in_the_sim() {
    let (mut with_body, mut without) = (range(100.0), range(100.0));
    let (a, b) = (with_body.spawn_soldier(4), without.spawn_soldier(4));
    let mut buttons = Buttons::default();
    buttons.set(Buttons::ADS, true);
    for tick in 0..200 {
        buttons.set(Buttons::FIRE, tick % 25 == 0);
        drive(&mut with_body, a, Vec2::new(0.3, 0.6), 0.2, 0.05, buttons);
        drive(&mut without, b, Vec2::new(0.3, 0.6), 0.2, 0.05, buttons);
        let _ = with_body.body_pose(&with_body.body_input(a));
    }
    let (x, y) = (&with_body.soldiers[a], &without.soldiers[b]);
    assert_eq!(x.body.pos, y.body.pos);
    assert_eq!(x.gun_pose(&with_body.weapons).muzzle, y.gun_pose(&without.weapons).muzzle);
}

#[test]
fn the_body_turns_when_the_look_does_and_walks_in_time_with_the_stride() {
    use prs_body::Bone;
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(4);
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    }
    let hips_before = sim.soldiers[id].anim.hip_yaw;
    for _ in 0..120 {
        drive(&mut sim, id, Vec2::ZERO, 2.0, 0.0, Buttons::default());
    }
    assert!(sim.soldiers[id].anim.hip_yaw - hips_before > 1.5, "hips never caught up with the look");

    let mut seen = Vec::new();
    for _ in 0..120 {
        drive(&mut sim, id, Vec2::Y, 2.0, 0.0, Buttons::default());
        let pose = sim.body_pose(&sim.body_input(id));
        seen.push(pose[Bone::FootL].start.y);
    }
    let lifted = seen.iter().filter(|y| **y > sim.body_cfg.bones.ankle_height + 0.03).count();
    let planted = seen.iter().filter(|y| **y < sim.body_cfg.bones.ankle_height + 0.005).count();
    assert!(lifted > 5 && planted > 5, "a walking foot should lift and plant: {lifted} up, {planted} down");
}

// ---- Weapons as data ----

/// A flat range where everyone is issued this gun.
fn range_with(distance: f32, weapon: WeaponId) -> Sim {
    let mut sim = range(distance);
    sim.inventory_cfg.loadout.primary = weapon;
    sim
}

const RIFLE: WeaponId = WeaponTable::ASSAULT_RIFLE;
const LMG: WeaponId = WeaponTable::MACHINE_GUN;
const DMR: WeaponId = WeaponTable::MARKSMAN_RIFLE;
const PISTOL: WeaponId = WeaponTable::PISTOL;

#[test]
fn two_soldiers_can_hold_different_guns_and_their_shots_say_which() {
    let mut sim = range(100.0);
    let a = sim.spawn_soldier(1);
    let mut loadout = sim.inventory_cfg.loadout;
    loadout.primary = LMG;
    let b = sim.spawn_soldier_with(2, 0, loadout);
    assert_eq!(sim.soldiers[a].weapon.weapon, RIFLE);
    assert_eq!(sim.soldiers[b].weapon.weapon, LMG);
    assert_eq!(sim.soldiers[b].weapon.ammo, sim.weapons[LMG].magazine);
    for _ in 0..40 {
        drive(&mut sim, a, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    }
    let mut fire = Buttons::default();
    fire.set(Buttons::FIRE, true);
    let cmd = |tick| PlayerCmd { tick, buttons: fire, ..Default::default() };
    sim.step(&[(a, cmd(sim.tick)), (b, cmd(sim.tick))]);
    let fired: Vec<_> = sim.events.iter().filter_map(|e| match *e {
        SimEvent::ShotFired { soldier, weapon, origin, .. } => Some((soldier, weapon, origin)),
        _ => None,
    }).collect();
    assert_eq!(fired.len(), 2);
    assert!(fired.contains(&(a, RIFLE, fired[0].2)));
    assert!(fired.contains(&(b, LMG, fired[1].2)));
    assert!(sim.bullets[0].bullet.vel.length() != sim.bullets[1].bullet.vel.length(), "each gun has its own ballistics");
}

#[test]
fn every_gun_fires_from_its_own_muzzle_with_hands_on_its_own_grips() {
    use prs_body::Bone;
    for weapon in [RIFLE, LMG, DMR, PISTOL] {
        let mut sim = range_with(100.0, weapon);
        let id = sim.spawn_soldier(9);
        for ads in [false, true] {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::ADS, ads);
            for _ in 0..90 {
                drive(&mut sim, id, Vec2::ZERO, 0.3, 0.05, buttons);
            }
            let input = sim.body_input(id);
            let pose = sim.body_pose(&input);
            let g = &input.gun;
            let wrist = g.origin + g.rotation * g.grips.trigger_wrist;
            let name = sim.weapons[weapon].name.clone();
            assert!(pose[Bone::HandR].start.distance(wrist) < 0.01, "{name}: trigger hand off the gun, ads {ads}");
            let support = g.origin + g.rotation * g.grips.support_wrist;
            let slack = sim.body_cfg.look.support_slide + 0.01;
            assert!(pose[Bone::HandL].start.distance(support) < slack, "{name}: off hand off the gun, ads {ads}");

            let before = sim.soldiers[id].gun_pose(&sim.weapons);
            buttons.set(Buttons::FIRE, true);
            drive(&mut sim, id, Vec2::ZERO, 0.3, 0.05, buttons);
            let origin = sim.events.iter().find_map(|e| match *e {
                SimEvent::ShotFired { origin, .. } => Some(origin),
                _ => None,
            });
            let origin = origin.unwrap_or_else(|| panic!("{name} did not fire"));
            assert!(origin.distance(before.muzzle) < 0.03, "{name}: muzzle {} but shot from {origin}", before.muzzle);
            for _ in 0..30 {
                drive(&mut sim, id, Vec2::ZERO, 0.3, 0.05, Buttons::default());
            }
        }
    }
}

fn gun_rate(range_m: f32, weapon: WeaponId, stance: Stance, rounds: u32, gap: f32) -> f32 {
    over_seeds(TRIALS, |seed| aimed_shots(&mut range_with(range_m, weapon), seed, rounds, stance, gap)) / rounds as f32
}

#[test]
fn the_marksman_rifle_reaches_further_than_anything_else_from_a_rest() {
    let dmr = gun_rate(450.0, DMR, Stance::Prone, 5, 1.5);
    let rifle = gun_rate(450.0, RIFLE, Stance::Prone, 5, 1.5);
    let lmg = gun_rate(450.0, LMG, Stance::Prone, 5, 1.5);
    assert!(dmr > rifle, "marksman {dmr} vs rifle {rifle}");
    assert!(dmr > lmg, "marksman {dmr} vs machine gun {lmg}");
    assert!(dmr > 0.5, "{dmr}");
}

#[test]
fn a_pistol_is_for_close_range() {
    let near = gun_rate(10.0, PISTOL, Stance::Stand, 5, 0.5);
    let far = gun_rate(200.0, PISTOL, Stance::Stand, 5, 0.5);
    assert!(near > 0.7, "{near}");
    assert!(far < 0.25, "{far}");
    let rifle = gun_rate(200.0, RIFLE, Stance::Stand, 5, 0.5);
    assert!(rifle > far * 2.0, "rifle {rifle} vs pistol {far}");
}

/// A long burst tracked with good reactions, from a given stance.
fn lmg_burst(range_m: f32, stance: Stance, ads: bool, rounds: u32) -> f32 {
    over_seeds(TRIALS, |seed| {
        let mut sim = range_with(range_m, LMG);
        let mut s = Shooter::new(&mut sim, seed);
        s.buttons.set(Buttons::ADS, ads);
        s.stance(stance);
        s.track(3.0);
        let start = s.weapon().ammo;
        s.buttons.set(Buttons::FIRE, true);
        while start - s.weapon().ammo < rounds {
            s.keep_target_up();
            s.track_tick();
        }
        s.buttons.set(Buttons::FIRE, false);
        for _ in 0..30 {
            s.keep_target_up();
            s.tick();
        }
        s.sim.targets[0].hits
    }) / rounds as f32
}

#[test]
fn a_machine_gun_is_brutal_from_the_hip_and_a_different_animal_lying_down() {
    let prone = lmg_burst(100.0, Stance::Prone, true, 15);
    let shouldered = lmg_burst(100.0, Stance::Stand, true, 15);
    let hip = lmg_burst(100.0, Stance::Stand, false, 15);
    assert!(prone > shouldered * 2.0, "prone {prone} vs standing {shouldered}");
    assert!(prone > 0.3, "lying down it should hold a burst near a man at 100m: {prone}");
    // One round in fifteen goes where it was pointed before the gun started jumping.
    assert!(hip < 0.09, "from the hip at 100m: {hip}");
    let hip_close = lmg_burst(15.0, Stance::Stand, false, 15);
    assert!(hip_close < 0.45, "even at 15m a hip burst should spray: {hip_close}");
}

// ---- Inventory ----

fn press(button: Buttons) -> Buttons {
    let mut buttons = Buttons::default();
    buttons.set(button, true);
    buttons
}

fn idle_for(sim: &mut Sim, id: SoldierId, seconds: f32) {
    for _ in 0..(seconds * TICKS) as usize {
        drive(sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    }
}

#[test]
fn changing_guns_takes_real_time_with_the_gun_coming_down_and_up() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.5);
    let (stow, draw) = (sim.weapons[RIFLE].stow_time, sim.weapons[PISTOL].draw_time);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_2));
    // The rifle goes on being the rifle until it is all the way down.
    let mut ticks = 1;
    while sim.soldiers[id].weapon.weapon == RIFLE {
        assert!(sim.soldiers[id].weapon.ready > 0.0 || ticks as f32 / TICKS >= stow - 0.05, "swapped too early");
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        assert!(sim.events.iter().all(|e| !matches!(e, SimEvent::ShotFired { .. })), "fired while stowing");
        ticks += 1;
        assert!(ticks < 200);
    }
    assert!((ticks as f32 / TICKS - stow).abs() < 0.1, "took {} s to stow", ticks as f32 / TICKS);
    assert!(sim.soldiers[id].weapon.ready < 0.1);
    assert_eq!(sim.soldiers[id].inventory.held, Slot::Sidearm);

    let mut up = 0;
    while sim.soldiers[id].weapon.ready < 1.0 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
        up += 1;
        assert!(up < 200);
    }
    assert!((up as f32 / TICKS - draw).abs() < 0.1, "took {} s to draw", up as f32 / TICKS);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
    assert!(sim.events.iter().any(|e| matches!(e, SimEvent::ShotFired { weapon, .. } if *weapon == PISTOL)));
}

#[test]
fn each_gun_keeps_its_rounds_and_magazines_while_put_away() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.5);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
    let rifle_ammo = sim.soldiers[id].weapon.ammo;
    assert_eq!(rifle_ammo, sim.weapons[RIFLE].magazine - 1);
    let rifle_mode = sim.soldiers[id].weapon.mode;
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_2));
    idle_for(&mut sim, id, 2.0);
    assert_eq!(sim.soldiers[id].weapon.ammo, sim.weapons[PISTOL].magazine);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_1));
    idle_for(&mut sim, id, 2.0);
    assert_eq!(sim.soldiers[id].weapon.weapon, RIFLE);
    assert_eq!(sim.soldiers[id].weapon.ammo, rifle_ammo);
    assert_eq!(sim.soldiers[id].weapon.mode, rifle_mode);
}

/// Reloading early keeps the part-used magazine, and reloading again later finds it: the choice between
/// a quick top-up and fewer full magazines is yours to live with.
#[test]
fn a_reload_keeps_the_part_used_magazine_for_later() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.5);
    let capacity = sim.weapons[RIFLE].magazine;
    let total = sim.soldiers[id].weapon.gun().rounds();
    sim.soldiers[id].weapon.ammo = 12;
    let total_after_firing = total - (capacity - 12);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::RELOAD));
    let reload = sim.weapons[RIFLE].reload_time;
    idle_for(&mut sim, id, reload + 0.2);
    let s = &sim.soldiers[id].weapon;
    assert_eq!(s.ammo, capacity + 1);
    assert_eq!(s.gun().rounds(), total_after_firing);
    assert!(s.magazines.contains(&11), "the old magazine is still there: {:?}", s.magazines);
    assert_eq!(s.magazines.len(), sim.weapons[RIFLE].spare_magazines as usize);
}

#[test]
fn weight_slows_a_machine_gunner_lightly_and_a_rifleman_not_at_all() {
    let top_speed = |sim: &mut Sim, id: SoldierId| {
        for _ in 0..120 {
            drive(sim, id, Vec2::Y, 0.0, 0.0, Buttons::default());
        }
        sim.soldiers[id].body.horizontal_speed()
    };
    let mut sim = range(500.0);
    let rifleman = sim.spawn_soldier(1);
    let rifle_speed = top_speed(&mut sim, rifleman);
    assert!((rifle_speed - sim.move_cfg.run_speed).abs() < 0.05, "{rifle_speed}");
    let mut sim = range_with(500.0, LMG);
    let gunner = sim.spawn_soldier(1);
    let lmg_speed = top_speed(&mut sim, gunner);
    assert!(lmg_speed < rifle_speed * 0.95, "{lmg_speed} vs {rifle_speed}");
    assert!(lmg_speed > rifle_speed * 0.8, "only lightly: {lmg_speed} vs {rifle_speed}");
}

#[test]
fn a_dead_soldier_drops_a_kit_and_f_swaps_with_it() {
    let mut sim = range(50.0);
    let me = sim.spawn_soldier(1);
    let mut loadout = sim.inventory_cfg.loadout;
    loadout.primary = LMG;
    let other = sim.spawn_soldier_with(2, 0, loadout);
    sim.set_home(other, Vec3::new(30.0, 0.05, 0.0), 0.0);
    idle_for(&mut sim, me, 0.5);
    sim.hurt(other, Zone::Head, 200.0);
    assert_eq!(sim.kits.len(), 1);
    assert_eq!(sim.kits[0].stash.guns[0].as_ref().unwrap().weapon, LMG);
    assert_eq!(sim.soldiers[other].weapon.ammo, 0, "they left it all behind");

    // Too far to reach: F does nothing.
    drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::USE));
    assert_eq!(sim.soldiers[me].weapon.weapon, RIFLE);
    idle_for(&mut sim, me, 0.1);

    sim.soldiers[me].body.pos = sim.kits[0].pos + Vec3::new(1.0, 0.05, 0.0);
    drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::USE));
    assert_eq!(sim.soldiers[me].weapon.weapon, LMG);
    assert_eq!(sim.soldiers[me].weapon.ready, 0.0, "the picked up gun still has to come up");
    assert_eq!(sim.kits[0].stash.guns[0].as_ref().unwrap().weapon, RIFLE, "and your rifle is there for someone else");
}

// ---- Damage ----

/// Someone standing on a flat range, and a shooter off to one side who will not get in the way.
fn victim_on_range() -> (Sim, SoldierId, SoldierId) {
    let mut sim = range(500.0);
    sim.targets.clear();
    let shooter = sim.spawn_soldier(1);
    let victim = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(200.0, 0.05, 200.0), 0.0);
    sim.set_home(victim, Vec3::new(0.0, 0.05, 0.0), 0.0);
    idle_for(&mut sim, shooter, 1.0);
    (sim, shooter, victim)
}

/// A bullet of this gun fired straight at a point from `from`.
fn fire_at(sim: &mut Sim, shooter: SoldierId, weapon: WeaponId, from: Vec3, at: Vec3) {
    let bullet = Bullet::new(from, (at - from).normalize(), &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
}

/// Steps until the bullets are gone and returns all the soldier hits that happened on the way.
fn hits_landing(sim: &mut Sim) -> Vec<(SoldierId, SoldierId, Zone, f32)> {
    let mut hits = Vec::new();
    for _ in 0..120 {
        sim.step(&[]);
        for event in &sim.events {
            if let SimEvent::SoldierHit { soldier, shooter, zone, damage, .. } = *event {
                hits.push((soldier, shooter, zone, damage));
            }
        }
        if sim.bullets.is_empty() {
            break;
        }
    }
    hits
}

fn bone_middle(sim: &Sim, id: SoldierId, bone: prs_body::Bone) -> Vec3 {
    let pose = sim.body_pose(&sim.body_input(id));
    (pose[bone].start + pose[bone].end()) * 0.5
}

fn from_the_front(at: Vec3, range_m: f32) -> Vec3 {
    at + Vec3::new(0.0, 0.0, -range_m)
}

/// Everyone here faces -Z with their arms round the gun, so from behind nothing covers the trunk.
fn from_behind(at: Vec3, range_m: f32) -> Vec3 {
    at + Vec3::new(0.0, 0.0, range_m)
}

fn from_the_side(at: Vec3, range_m: f32) -> Vec3 {
    at + Vec3::new(range_m, 0.0, 0.0)
}

#[test]
fn a_bullet_finds_the_body_part_it_was_aimed_at() {
    use prs_body::Bone;
    for (bone, zone) in [
        (Bone::Head, Zone::Head),
        (Bone::Chest, Zone::Chest),
        (Bone::ThighL, Zone::Leg),
        (Bone::ShinR, Zone::Leg),
        (Bone::Pelvis, Zone::Abdomen),
    ] {
        let (mut sim, shooter, victim) = victim_on_range();
        let at = bone_middle(&sim, victim, bone);
        fire_at(&mut sim, shooter, RIFLE, from_behind(at, 30.0), at);
        let hits = hits_landing(&mut sim);
        assert_eq!(hits.len(), 1, "{bone:?}: {hits:?}");
        assert_eq!((hits[0].0, hits[0].1), (victim, shooter));
        assert_eq!(hits[0].2, zone, "{bone:?}");
    }
}

#[test]
fn a_bullet_passing_either_side_touches_nothing() {
    let (mut sim, shooter, victim) = victim_on_range();
    let at = bone_middle(&sim, victim, prs_body::Bone::Chest) + Vec3::X * 0.8;
    fire_at(&mut sim, shooter, RIFLE, from_the_front(at, 30.0), at);
    assert!(hits_landing(&mut sim).is_empty());
}

#[test]
fn someone_lying_down_is_hit_where_they_look() {
    use prs_body::Bone;
    let (mut sim, shooter, victim) = victim_on_range();
    let down = |sim: &mut Sim| {
        drive(sim, victim, Vec2::ZERO, 0.0, 0.0, press(Buttons::PRONE));
        idle_for(sim, victim, 2.0);
    };
    down(&mut sim);
    let standing_chest = sim.soldiers[victim].body.pos + Vec3::Y * 1.3;
    fire_at(&mut sim, shooter, RIFLE, from_the_front(standing_chest, 30.0), standing_chest);
    assert!(hits_landing(&mut sim).is_empty(), "that bullet went over the top of them");

    let thigh = bone_middle(&sim, victim, Bone::ThighL);
    assert!(thigh.y < 0.45, "lying down the thigh should be low: {thigh}");
    let behind = sim.soldiers[victim].body.pos.z;
    assert!(thigh.z > behind + 0.3, "and well behind the eye: {thigh}");
    fire_at(&mut sim, shooter, RIFLE, from_the_side(thigh, 30.0), thigh);
    let hits = hits_landing(&mut sim);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].2, Zone::Leg, "{hits:?}");
}

#[test]
fn a_wall_covers_what_is_behind_it_and_only_that() {
    use prs_body::Bone;
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.targets.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    // Waist high and right in front of the victim, between them and the shooter.
    level.solids.push(prs_level::Solid {
        center: Vec3::new(0.0, 0.55, -2.0),
        half: Vec3::new(3.0, 0.55, 0.2),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let shooter = sim.spawn_soldier(1);
    let victim = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(100.0, 0.05, 100.0), 0.0);
    idle_for(&mut sim, shooter, 1.0);

    let shin = bone_middle(&sim, victim, Bone::ShinL);
    let from = Vec3::new(shin.x, shin.y, -30.0);
    fire_at(&mut sim, shooter, RIFLE, from, shin);
    assert!(hits_landing(&mut sim).is_empty(), "the wall should have stopped it");

    let head = bone_middle(&sim, victim, Bone::Head);
    let from = Vec3::new(head.x, head.y, -30.0);
    fire_at(&mut sim, shooter, RIFLE, from, head);
    let hits = hits_landing(&mut sim);
    assert_eq!(hits.len(), 1, "the head is above the wall");
    assert_eq!(hits[0].2, Zone::Head);
}

#[test]
fn someone_else_firing_through_a_wall_hits_the_wall() {
    let mut sim = soldier_at_wall(20.0);
    sim.targets.clear();
    let mut buttons = press(Buttons::ADS);
    let shooter = sim.spawn_soldier(1);
    let victim = sim.spawn_soldier(2);
    sim.set_home(victim, Vec3::new(0.0, 0.05, -30.0), 0.0);
    for _ in 0..120 {
        drive(&mut sim, shooter, Vec2::ZERO, 0.0, 0.0, buttons);
    }
    buttons.set(Buttons::FIRE, true);
    drive(&mut sim, shooter, Vec2::ZERO, 0.0, 0.0, buttons);
    assert!(sim.events.iter().any(|e| matches!(e, SimEvent::ShotFired { .. })));
    let mut stopped = false;
    for _ in 0..60 {
        sim.step(&[]);
        stopped |= sim.events.iter().any(|e| matches!(e, SimEvent::Impact { .. }));
        assert!(sim.events.iter().all(|e| !matches!(e, SimEvent::SoldierHit { .. })));
    }
    assert!(stopped, "the shot should have ended on the wall");
    assert_eq!(sim.soldiers[victim].health.health, sim.health_cfg.max_health);
}

#[test]
fn nobody_ever_hits_themselves_whichever_way_they_fire() {
    let mut sim = range(500.0);
    sim.targets.clear();
    let id = sim.spawn_soldier(3);
    idle_for(&mut sim, id, 0.5);
    for tick in 0..2000 {
        let yaw = tick as f32 * 0.21;
        let pitch = (tick as f32 * 0.17).sin() * 1.5;
        let mut buttons = press(Buttons::FIRE);
        buttons.set(Buttons::ADS, tick % 300 < 150);
        buttons.set(Buttons::PRONE, tick % 500 == 400);
        sim.soldiers[id].weapon.ammo = 30;
        drive(&mut sim, id, Vec2::ZERO, yaw, pitch, buttons);
        assert!(sim.events.iter().all(|e| !matches!(e, SimEvent::SoldierHit { .. })), "shot themselves at tick {tick}");
    }
    assert_eq!(sim.soldiers[id].health.health, sim.health_cfg.max_health);
}

#[test]
fn a_bullet_hits_less_hard_the_further_it_has_flown() {
    use prs_body::Bone;
    let damage_at = |range_m: f32, weapon: WeaponId| {
        let (mut sim, shooter, victim) = victim_on_range();
        let at = bone_middle(&sim, victim, Bone::Head);
        // Aimed high enough to arrive at the head despite the drop.
        let zero = zero_elevation(&sim.weapons[weapon].ballistics, range_m, 0.0);
        let from = at + Vec3::new(range_m, 0.0, 0.0);
        let dir = Vec3::new(-zero.cos(), zero.sin(), 0.0);
        let bullet = Bullet::new(from, dir, &sim.weapons[weapon].ballistics);
        sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
        hits_landing(&mut sim).first().map(|h| h.3)
    };
    let (near, far) = (damage_at(20.0, RIFLE).unwrap(), damage_at(250.0, RIFLE).unwrap());
    assert!(far < near * 0.9, "{far} vs {near}");
    let (rifle, pistol) = (damage_at(20.0, RIFLE).unwrap(), damage_at(20.0, PISTOL).unwrap());
    assert!(pistol < rifle * 0.7, "pistol {pistol} vs rifle {rifle}");
}

#[test]
fn a_head_shot_ends_it_and_a_body_shot_does_not() {
    use prs_body::Bone;
    let shot = |bone: Bone| {
        let (mut sim, shooter, victim) = victim_on_range();
        let at = bone_middle(&sim, victim, bone);
        fire_at(&mut sim, shooter, RIFLE, from_behind(at, 40.0), at);
        hits_landing(&mut sim);
        sim.soldiers[victim].health.life
    };
    assert_eq!(shot(Bone::Head), Life::Dead);
    assert_eq!(shot(Bone::Chest), Life::Up);
}

#[test]
fn it_takes_two_rifle_rounds_to_the_chest_to_put_someone_down() {
    use prs_body::Bone;
    let (mut sim, shooter, victim) = victim_on_range();
    for round in 1..=2 {
        let at = bone_middle(&sim, victim, Bone::Chest);
        fire_at(&mut sim, shooter, RIFLE, from_behind(at, 40.0), at);
        hits_landing(&mut sim);
        let want = if round == 2 { Life::Down } else { Life::Up };
        assert_eq!(sim.soldiers[victim].health.life, want, "after {round}");
    }
}

/// One person with one magazine, sweeping across five people standing in the open.
#[test]
fn one_person_spraying_cannot_take_out_a_squad_at_range() {
    for distance in [60.0_f32, 120.0] {
        let mut wiped = 0;
        let mut total_put_out = 0;
        for seed in 0..TRIALS {
            let mut sim = range(500.0);
            sim.targets.clear();
            let mut s = Shooter::new(&mut sim, seed * 31 + 5);
            let squad: Vec<SoldierId> = (0..5)
                .map(|i| {
                    let id = s.sim.spawn_soldier(seed * 7 + i);
                    let x = -6.0 + i as f32 * 3.0;
                    s.sim.set_home(id, Vec3::new(x, 0.05, -distance), 0.0);
                    id
                })
                .collect();
            s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
            s.buttons.set(Buttons::ADS, true);
            s.wait(3.0);
            s.buttons.set(Buttons::FIRE, true);
            let start = s.weapon().ammo;
            while start - s.weapon().ammo < MAG {
                let i = (start - s.weapon().ammo) as f32 / MAG as f32;
                let x = -6.0 + 12.0 * i;
                s.yaw = (-x).atan2(distance);
                s.pitch = 0.015;
                s.tick();
            }
            s.buttons.set(Buttons::FIRE, false);
            s.wait(0.5);
            let out = squad.iter().filter(|&&id| !s.sim.soldiers[id].health.is_up()).count();
            total_put_out += out;
            if out == 5 {
                wiped += 1;
            }
        }
        let average = total_put_out as f32 / TRIALS as f32;
        assert_eq!(wiped, 0, "cleared a squad at {distance} m with one magazine");
        assert!(average < 1.0, "averaged {average} people out at {distance} m");
    }
}

/// A careful shooter with good reactions does what a spray cannot: one aimed round at a time.
#[test]
fn patient_aimed_fire_is_what_actually_wins_fights() {
    use prs_body::Bone;
    let mut downed = 0;
    for seed in 0..TRIALS {
        let mut sim = range(500.0);
        sim.targets.clear();
        sim.level.targets.clear();
        let mut s = Shooter::new(&mut sim, seed + 3);
        let victim = s.sim.spawn_soldier(seed + 900);
        s.sim.set_home(victim, Vec3::new(0.0, 0.05, -100.0), 0.0);
        s.buttons.set(Buttons::ADS, true);
        s.stance(Stance::Crouch);
        for _ in 0..4 {
            for _ in 0..(1.5 * TICKS) as usize {
                let chest = bone_middle(s.sim, victim, Bone::Chest);
                let d = chest - s.sim.soldiers[s.id].body.eye();
                let now = s.weapon().aim_offset();
                s.seen.push_back(now);
                let late = if s.seen.len() > 6 { s.seen.pop_front().unwrap() } else { s.seen[0] };
                s.yaw = (-d.x).atan2(-d.z) - late.x;
                s.pitch = (d.y / d.length()).asin() - late.y;
                s.tick();
            }
            s.buttons.set(Buttons::FIRE, true);
            s.tick();
            s.buttons.set(Buttons::FIRE, false);
        }
        s.wait(1.0);
        if !s.sim.soldiers[victim].health.is_up() {
            downed += 1;
        }
    }
    let rate = downed as f32 / TRIALS as f32;
    assert!(rate > 0.5, "four aimed rounds at 100 m put someone down only {rate} of the time");
}

#[test]
fn the_range_targets_still_work_alongside_soldiers() {
    let mut sim = range(40.0);
    let mut s = Shooter::new(&mut sim, 4);
    let bystander = s.sim.spawn_soldier(5);
    s.sim.set_home(bystander, Vec3::new(10.0, 0.05, 0.0), 0.0);
    s.buttons.set(Buttons::ADS, true);
    s.track(3.0);
    s.buttons.set(Buttons::FIRE, true);
    s.track_tick();
    s.buttons.set(Buttons::FIRE, false);
    assert_eq!(s.hits(), 1);
    assert_eq!(s.sim.soldiers[bystander].health.health, s.sim.health_cfg.max_health);
}

#[test]
fn every_bone_stays_within_the_reach_a_bullet_checks() {
    use prs_body::Bone;
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(1);
    let check = |sim: &Sim| {
        let pose = sim.body_pose(&sim.body_input(id));
        let middle = sim.soldiers[id].body.pos + Vec3::Y * hits::BODY_MIDDLE;
        for bone in Bone::ALL {
            for point in [pose[bone].start, pose[bone].end()] {
                let d = point.distance(middle);
                assert!(d + 0.25 < hits::BODY_REACH, "{bone:?} is {d} from the middle");
            }
        }
    };
    idle_for(&mut sim, id, 0.5);
    check(&sim);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::PRONE));
    idle_for(&mut sim, id, 2.0);
    check(&sim);
    sim.hurt(id, Zone::Chest, 120.0);
    idle_for(&mut sim, id, 2.0);
    assert_eq!(sim.soldiers[id].anim.collapse, 1.0);
    check(&sim);
}

// ---- Wounds and healing ----

fn wound(sim: &mut Sim, id: SoldierId, zone: Zone, raw: f32) {
    sim.hurt(id, zone, raw);
}

#[test]
fn a_leg_wound_takes_the_sprint_and_slows_you_down() {
    let mut sim = range(800.0);
    let id = sim.spawn_soldier(1);
    let mut buttons = press(Buttons::SPRINT);
    let speed = |sim: &mut Sim, buttons: Buttons| {
        for _ in 0..120 {
            drive(sim, id, Vec2::Y, 0.0, 0.0, buttons);
        }
        sim.soldiers[id].body.horizontal_speed()
    };
    let fit = speed(&mut sim, buttons);
    assert!(sim.soldiers[id].body.sprinting);
    wound(&mut sim, id, Zone::Leg, 40.0);
    let hurt = speed(&mut sim, buttons);
    assert!(!sim.soldiers[id].body.sprinting, "no sprinting on that leg");
    assert!(hurt < fit * 0.65, "{hurt} vs {fit}");
    buttons.set(Buttons::SPRINT, false);
    assert!(speed(&mut sim, buttons) < sim.move_cfg.run_speed * 0.8);
}

/// Wounds shake the real gun, so the sights show it and a steady aim fights it. They do not widen the
/// cone: `prs_weapons` proves the same directly.
#[test]
fn a_wounded_arm_and_blood_loss_shake_the_gun_you_can_see() {
    let steadiness = |zone: Option<Zone>, health: Option<f32>| {
        let mut sim = range(100.0);
        let id = sim.spawn_soldier(11);
        if let Some(zone) = zone {
            wound(&mut sim, id, zone, 40.0);
            sim.soldiers[id].health.bleed = 0.0;
        }
        if let Some(health) = health {
            sim.soldiers[id].health.health = health;
        }
        let mut spread = 0.0_f32;
        for tick in 0..420 {
            drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::ADS));
            if tick > 240 {
                spread = spread.max(sim.soldiers[id].weapon.aim_offset().length());
            }
        }
        spread
    };
    let fit = steadiness(None, None);
    let arm = steadiness(Some(Zone::Arm), None);
    let weak = steadiness(None, Some(15.0));
    assert!(arm > fit * 2.0, "arm {arm} vs fit {fit}");
    assert!(weak > fit * 2.0, "blood loss {weak} vs fit {fit}");
}

#[test]
fn a_wounded_arm_really_does_cost_shots_at_range() {
    let rate = |hurt: bool| {
        over_seeds(TRIALS, |seed| {
            let mut sim = range(150.0);
            let mut s = Shooter::new(&mut sim, seed + 1);
            if hurt {
                wound(s.sim, s.id, Zone::Arm, 40.0);
                s.sim.soldiers[s.id].health.bleed = 0.0;
            }
            s.buttons.set(Buttons::ADS, true);
            s.stance(Stance::Crouch);
            s.track(3.0);
            s.buttons.set(Buttons::FIRE, true);
            s.track_tick();
            s.buttons.set(Buttons::FIRE, false);
            s.hits()
        })
    };
    let (fit, hurt) = (rate(false), rate(true));
    assert!(hurt < fit * 0.85, "wounded {hurt} vs fit {fit}");
}

#[test]
fn down_means_down_you_can_look_but_not_act() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(1);
    idle_for(&mut sim, id, 0.5);
    wound(&mut sim, id, Zone::Chest, 120.0);
    assert_eq!(sim.soldiers[id].health.life, Life::Down);
    let (ammo, pos) = (sim.soldiers[id].weapon.ammo, sim.soldiers[id].body.pos);
    let mut buttons = press(Buttons::FIRE);
    buttons.set(Buttons::SPRINT, true);
    buttons.set(Buttons::RELOAD, true);
    buttons.set(Buttons::JUMP, true);
    let (mut yaw, gun_yaw) = (0.0, sim.soldiers[id].yaw);
    for tick in 0..180 {
        yaw += 0.03;
        buttons.set(Buttons::SLOT_2, tick % 20 == 0);
        drive(&mut sim, id, Vec2::Y, yaw, 0.2, buttons);
    }
    let s = &sim.soldiers[id];
    assert_eq!(s.weapon.ammo, ammo, "fired while down");
    assert!(!s.weapon.reloading());
    assert!(s.body.pos.distance(pos) < 0.2, "moved while down: {} to {}", pos, s.body.pos);
    assert_eq!(s.body.stance, Stance::Prone);
    assert_eq!(s.weapon.ready, 0.0, "the gun is down");
    assert_eq!(s.yaw, gun_yaw, "the gun and body do not turn with the look");
    assert_eq!(s.inventory.selected, Slot::Primary);
    assert_eq!(s.anim.collapse, 1.0);
}

#[test]
fn someone_left_alone_bleeds_out_for_good_and_comes_back_with_a_new_kit_at_their_spawn() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(1);
    let home = Vec3::new(3.0, 0.05, 3.0);
    sim.set_home(id, home, 0.5);
    for _ in 0..30 {
        drive(&mut sim, id, Vec2::Y, 0.5, 0.0, Buttons::default());
    }
    sim.soldiers[id].weapon.ammo = 4;
    wound(&mut sim, id, Zone::Chest, 60.0);
    let cfg = sim.health_cfg.clone();
    let until_down = (cfg.max_health - 60.0) / cfg.chest.bleed;
    let mut saw = (false, false, false);
    let total = until_down + cfg.down_time + cfg.respawn_time + 2.0;
    for tick in 0..(total * TICKS) as usize {
        sim.step(&[]);
        for event in &sim.events {
            match event {
                SimEvent::Health { event: HealthEvent::Downed, .. } => saw.0 = true,
                SimEvent::Health { event: HealthEvent::Died, .. } => saw.1 = true,
                SimEvent::Respawned { .. } => saw.2 = true,
                _ => {}
            }
        }
        if tick == ((until_down - 1.0) * TICKS) as usize {
            assert!(!saw.0, "went down too soon");
        }
    }
    assert_eq!(saw, (true, true, true), "down, dead, back");
    let s = &sim.soldiers[id];
    assert!(s.health.is_up());
    assert_eq!(s.health.health, cfg.max_health);
    assert!(s.body.pos.distance(home) < 0.2, "at {}", s.body.pos);
    assert_eq!(s.weapon.ammo, sim.weapons[RIFLE].magazine);
    assert_eq!(s.anim.collapse, 0.0);
    assert_eq!(sim.kits.len(), 1, "the old kit is still lying where they fell");
    assert_eq!(sim.kits[0].stash.guns[0].as_ref().unwrap().ammo, 4);
}

/// Holds fire with a bandage selected until it is applied or `seconds` run out, calling `each_tick`.
fn hold_treatment(sim: &mut Sim, id: SoldierId, slot: Buttons, seconds: f32, mut each_tick: impl FnMut(&mut Sim, usize)) {
    drive(sim, id, Vec2::ZERO, 0.0, 0.0, slot);
    for tick in 0..(seconds * TICKS) as usize {
        each_tick(sim, tick);
        drive(sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        if !sim.soldiers[id].inventory.selected.is_gun() {
            continue;
        }
        break;
    }
}

#[test]
fn a_bandage_takes_seconds_with_the_gun_down_and_cannot_be_shot_through() {
    let mut sim = range(50.0);
    let id = sim.spawn_soldier(1);
    idle_for(&mut sim, id, 0.5);
    wound(&mut sim, id, Zone::Abdomen, 30.0);
    assert!(sim.soldiers[id].health.bleeding());
    let ammo = sim.soldiers[id].weapon.ammo;
    let bandages = sim.soldiers[id].inventory.stash.bandages;
    let (stow, time) = (sim.weapons[RIFLE].stow_time, sim.health_cfg.bandage_time);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_3));
    let mut ticks = 0;
    while sim.soldiers[id].health.bleeding() {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        assert_eq!(sim.soldiers[id].weapon.ammo, ammo, "the trigger is bandaging, not firing");
        ticks += 1;
        assert!(ticks < 600);
    }
    let took = ticks as f32 / TICKS;
    assert!((took - (stow + time)).abs() < 0.15, "took {took} s");
    assert_eq!(sim.soldiers[id].inventory.stash.bandages, bandages - 1);
    assert_eq!(sim.soldiers[id].inventory.selected, Slot::Primary, "back to the gun afterwards");

    // The finger is still on the trigger, and the gun must not fire by itself when it comes back up.
    for _ in 0..120 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        assert_eq!(sim.soldiers[id].weapon.ammo, ammo);
    }
}

/// The design goal: a bandage saves you if you have cover, and does nothing if you are still being shot.
#[test]
fn a_bandage_saves_you_only_if_you_have_cover() {
    let mut sim = range(500.0);
    sim.targets.clear();
    let shooter = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(200.0, 0.05, 200.0), 0.0);
    let id = sim.spawn_soldier(1);
    idle_for(&mut sim, id, 0.5);
    wound(&mut sim, id, Zone::Chest, 60.0);

    // Nobody shooting: patched up in time, and alive long after.
    let mut covered = sim;
    hold_treatment(&mut covered, id, press(Buttons::SLOT_3), 8.0, |_, _| {});
    assert!(!covered.soldiers[id].health.bleeding());
    idle_for(&mut covered, id, 30.0);
    assert!(covered.soldiers[id].health.is_up(), "should have survived in cover");

    // Still being hit every two seconds: it never gets finished.
    let mut sim = range(500.0);
    sim.targets.clear();
    let shooter = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(200.0, 0.05, 200.0), 0.0);
    let id = sim.spawn_soldier(1);
    idle_for(&mut sim, id, 0.5);
    wound(&mut sim, id, Zone::Chest, 60.0);
    let mut finished = false;
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_3));
    for tick in 0..(30.0 * TICKS) as usize {
        if tick % (2.0 * TICKS) as usize == 0 && sim.soldiers[id].health.is_up() {
            let at = bone_middle(&sim, id, prs_body::Bone::UpperArmL);
            fire_at(&mut sim, shooter, RIFLE, from_the_side(at, -20.0), at);
        }
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        finished |= sim.events.iter().any(|e| matches!(e, SimEvent::Carry { event: CarryEvent::TreatmentFinished { .. }, .. }));
    }
    assert!(!finished, "bandaged under fire");
    assert!(!sim.soldiers[id].health.is_up(), "should be out of it by now");
}

#[test]
fn you_can_bandage_an_ally_in_front_of_you_but_not_one_behind_or_far_away() {
    let mut sim = range(100.0);
    let (me, ally) = (sim.spawn_soldier(1), sim.spawn_soldier(2));
    sim.set_home(ally, Vec3::new(0.0, 0.05, 1.5), 0.0);
    idle_for(&mut sim, me, 0.5);
    wound(&mut sim, ally, Zone::Leg, 20.0);
    // They are behind you, so there is nobody to treat, and you are fine.
    hold_treatment(&mut sim, me, press(Buttons::SLOT_3), 1.5, |_, _| {});
    assert!(sim.soldiers[me].treatment.is_none());

    sim.set_home(ally, Vec3::new(0.0, 0.05, -1.5), 0.0);
    let time = sim.health_cfg.bandage_time;
    hold_treatment(&mut sim, me, press(Buttons::FIRE), time + 1.0, |_, _| {});
    assert!(!sim.soldiers[ally].health.bleeding(), "they should be patched up");
    assert_eq!(sim.soldiers[me].inventory.stash.bandages, 1);
}

#[test]
fn a_medkit_revives_a_downed_ally_slowly_and_moving_starts_it_over() {
    let mut sim = range(100.0);
    let (me, ally) = (sim.spawn_soldier(1), sim.spawn_soldier(2));
    sim.set_home(ally, Vec3::new(0.0, 0.05, -1.5), 0.0);
    idle_for(&mut sim, me, 0.5);
    wound(&mut sim, ally, Zone::Chest, 110.0);
    assert_eq!(sim.soldiers[ally].health.life, Life::Down);

    let revive = sim.health_cfg.revive_time;
    drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::SLOT_4));
    // Most of the way there, then a step away interrupts it.
    for _ in 0..((revive + 0.2) * TICKS) as usize {
        drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
    }
    assert!(sim.soldiers[me].treatment.is_some());
    for _ in 0..30 {
        drive(&mut sim, me, Vec2::X, 0.0, 0.0, press(Buttons::FIRE));
    }
    assert!(sim.soldiers[me].treatment.is_none(), "walking off should stop it");
    assert_eq!(sim.soldiers[ally].health.life, Life::Down);
    assert_eq!(sim.soldiers[me].inventory.stash.medkits, 1, "an interrupted medkit is not used up");
    idle_for(&mut sim, me, 0.5);
    sim.soldiers[me].body.pos = Vec3::new(0.0, 0.05, 0.0);

    let start = sim.tick;
    while !sim.soldiers[ally].health.is_up() {
        drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        assert!(sim.tick - start < (revive * 2.0 * TICKS) as u32, "never revived");
    }
    let took = (sim.tick - start) as f32 / TICKS;
    assert!(took > revive - 0.2, "took only {took}");
    assert_eq!(sim.soldiers[ally].health.health, sim.health_cfg.revive_health);
    assert_eq!(sim.soldiers[me].inventory.stash.medkits, 0);
    // Back on their feet at the player's pace, and the gun comes back up.
    idle_for(&mut sim, me, 3.0);
    assert_eq!(sim.soldiers[ally].anim.collapse, 0.0);
    assert_eq!(sim.soldiers[ally].weapon.ready, 1.0);
}

#[test]
fn you_cannot_treat_the_other_team() {
    let mut sim = range(100.0);
    let me = sim.spawn_soldier(1);
    let loadout = sim.inventory_cfg.loadout;
    let enemy = sim.spawn_soldier_with(2, 1, loadout);
    sim.set_home(enemy, Vec3::new(0.0, 0.05, -1.5), 0.0);
    idle_for(&mut sim, me, 0.5);
    wound(&mut sim, enemy, Zone::Chest, 110.0);
    hold_treatment(&mut sim, me, press(Buttons::SLOT_4), 2.0, |_, _| {});
    assert!(sim.soldiers[me].treatment.is_none());
}

#[test]
fn being_shot_while_down_can_finish_you() {
    use prs_body::Bone;
    let (mut sim, shooter, victim) = victim_on_range();
    wound(&mut sim, victim, Zone::Chest, 120.0);
    idle_for(&mut sim, shooter, 2.0);
    let at = bone_middle(&sim, victim, Bone::Chest);
    fire_at(&mut sim, shooter, RIFLE, from_the_side(at, 30.0), at);
    let hits = hits_landing(&mut sim);
    assert_eq!(hits.len(), 1, "a body on the ground can still be hit");
    assert!(sim.soldiers[victim].health.is_dead());
    // A dead body stops nothing.
    fire_at(&mut sim, shooter, RIFLE, from_the_side(at, 30.0), at);
    assert!(hits_landing(&mut sim).is_empty());
}

#[test]
fn the_same_commands_give_the_same_fight() {
    let play = || {
        let (mut sim, shooter, victim) = victim_on_range();
        sim.soldiers[shooter].body.pos = Vec3::new(0.0, 0.05, 25.0);
        let mut trace = Vec::new();
        for tick in 0..900 {
            let mut buttons = press(Buttons::ADS);
            buttons.set(Buttons::FIRE, tick % 50 == 0 && tick > 200);
            buttons.set(Buttons::RELOAD, tick == 600);
            drive(&mut sim, shooter, Vec2::ZERO, 0.0, 0.01, buttons);
            trace.push(sim.soldiers[victim].health.health);
        }
        (trace, sim.soldiers[victim].health, sim.soldiers[shooter].weapon.ammo, sim.kits.len())
    };
    assert_eq!(play(), play());
}

// ---- Exporting state, for a client to replay from what the server says ----

/// A busy tick of commands for two soldiers that between them use nearly everything there is.
fn busy_cmds(tick: u32) -> [(SoldierId, PlayerCmd); 2] {
    let beat = |every: u32, at: u32| tick % every == at;
    let mut a = Buttons::default();
    a.set(Buttons::ADS, tick % 400 < 200);
    a.set(Buttons::FIRE, tick % 400 < 300 && beat(35, 0));
    a.set(Buttons::RELOAD, beat(500, 450));
    a.set(Buttons::PRONE, beat(700, 650));
    a.set(Buttons::SPRINT, tick % 900 > 800);
    a.set(Buttons::JUMP, beat(900, 870));
    a.set(Buttons::SLOT_2, beat(1100, 1000));
    a.set(Buttons::SLOT_1, beat(1100, 1050));
    a.set(Buttons::SLOT_3, beat(1300, 1200));
    a.set(Buttons::LEAN_LEFT, beat(320, 40));
    a.set(Buttons::GRENADE, beat(1500, 300));
    a.set(Buttons::LEAN_RIGHT, beat(320, 180));
    let mut b = Buttons::default();
    b.set(Buttons::CROUCH, beat(300, 100));
    b.set(Buttons::FIRE, beat(90, 10));
    b.set(Buttons::USE, beat(250, 200));
    b.set(Buttons::FIRE_MODE, beat(600, 500));
    b.set(Buttons::LEAN_RIGHT, beat(210, 20));
    b.set(Buttons::GRENADE, beat(1700, 400));
    let walk = Vec2::new((tick as f32 * 0.01).sin(), (tick as f32 * 0.007).cos());
    let look = (tick as f32 * 0.003).sin() * 0.1;
    let cmd = |wish, yaw, pitch, buttons| PlayerCmd { tick, wish_move: wish, yaw, pitch, buttons, ..Default::default() };
    [(0, cmd(walk * 0.4, look, 0.01, a)), (1, cmd(walk, look + 3.14, 0.02, b))]
}

fn busy_sim() -> Sim {
    let mut sim = range(120.0);
    sim.spawn_soldier(0x11);
    let loadout = sim.inventory_cfg.loadout;
    sim.spawn_soldier_with(0x22, 1, Loadout { primary: LMG, ..loadout });
    sim.set_home(1, Vec3::new(1.0, 0.05, -35.0), 3.14);
    sim
}

/// The rule a client relies on: restore everything at any moment, in whatever state, and the two sims
/// agree on every later tick, down to the last bit of every timer and random number.
#[test]
fn a_sim_restored_from_an_export_stays_identical_to_the_original() {
    let mut original = busy_sim();
    for tick in 0..6000 {
        original.step(&busy_cmds(tick));
        // Fires a round into both soldiers now and then, so wounds, dying and kits come into it too.
        if tick % 170 == 0 {
            for id in [0, 1] {
                let at = bone_middle(&original, id, prs_body::Bone::Chest);
                fire_at(&mut original, 1 - id, RIFLE, at + Vec3::new(9.0, 0.0, 0.0), at);
            }
        }
        if tick % 700 != 650 {
            continue;
        }
        let state = original.export_state();
        // A fresh sim that knows nothing but the level and what it has just been told.
        let mut restored = busy_sim();
        restored.import_state(&state);
        assert_eq!(restored.export_state(), state);
        let mut twin = busy_sim();
        twin.import_state(&state);
        for later in tick + 1..tick + 200 {
            twin.step(&busy_cmds(later));
            restored.step(&busy_cmds(later));
            assert_eq!(restored.export_state(), twin.export_state(), "restored at {tick}, differs at {later}");
        }
        // And against the original itself, which has gone on from the same point.
        let mut ahead = original.export_state();
        let mut copy = busy_sim();
        copy.import_state(&ahead);
        for later in tick + 1..tick + 60 {
            original.step(&busy_cmds(later));
            copy.step(&busy_cmds(later));
            ahead = copy.export_state();
            assert_eq!(original.export_state(), ahead, "restored at {tick}, differs at {later}");
        }
    }
    // The script really did get round to the things it is meant to exercise.
    let state = original.export_state();
    assert!(state.soldiers.iter().any(|s| s.lives > 0), "nobody ever died in this script");
    assert!(!state.kits.is_empty() || state.next_kit > 0);
}

/// One soldier alone, restored by hand: the same state, moved onto someone in a fresh sim.
#[test]
fn a_single_soldier_restored_on_a_fresh_sim_stays_identical() {
    let mut original = range(100.0);
    let id = original.spawn_soldier(9);
    let script = |tick: u32| {
        let mut b = Buttons::default();
        b.set(Buttons::FIRE, tick % 11 == 0);
        b.set(Buttons::ADS, tick % 240 > 100);
        b.set(Buttons::JUMP, tick % 333 == 0);
        (Vec2::new(0.3, 0.7), (tick as f32 * 0.02).sin(), b)
    };
    for tick in 0..400 {
        let (wish, yaw, b) = script(tick);
        drive(&mut original, id, wish, yaw, 0.0, b);
    }
    original.hurt(id, Zone::Arm, 25.0);
    let mut fresh = range(100.0);
    let copy = fresh.spawn_soldier(12345);
    fresh.soldiers[copy] = original.soldiers[id].clone();
    for tick in 400..1000 {
        let (wish, yaw, b) = script(tick);
        drive(&mut original, id, wish, yaw, 0.0, b);
        drive(&mut fresh, copy, wish, yaw, 0.0, b);
        assert_eq!(original.soldiers[id], fresh.soldiers[copy], "differs at tick {tick}");
    }
}

// ---- Hitting what the shooter saw, and clients that only predict themselves.

/// Two soldiers on open ground, the target 20 m down -Z from the shooter and side on.
fn firing_range() -> (Sim, SoldierId, SoldierId) {
    let mut sim = Sim::new(Level::flat(400.0));
    let shooter = sim.spawn_soldier(1);
    let target = sim.spawn_soldier(2);
    sim.set_home(shooter, Vec3::new(0.0, 0.1, 0.0), 0.0);
    sim.set_home(target, Vec3::new(0.0, 0.1, -20.0), 0.0);
    for _ in 0..30 {
        sim.step(&[]);
    }
    (sim, shooter, target)
}

/// A bullet from the shooter straight at where `aim_at` is, chest height.
fn shoot_at(sim: &mut Sim, shooter: SoldierId, aim_at: Vec3, rewind: u32) {
    let from = sim.soldiers[shooter].body.eye();
    let dir = (aim_at + Vec3::Y * 1.2 - from).normalize();
    let weapon = sim.soldiers[shooter].weapon.weapon;
    let bullet = prs_ballistics::Bullet::new(from, dir, &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind });
}

fn was_hit(sim: &Sim, target: SoldierId) -> bool {
    sim.events.iter().any(|e| matches!(e, SimEvent::SoldierHit { soldier, .. } if *soldier == target))
}

#[test]
fn a_bullet_hits_someone_where_the_shooter_saw_them() {
    const LAG: u32 = 12;
    let (mut sim, shooter, target) = firing_range();
    sim.remember_past_ticks(60);
    let run = PlayerCmd { wish_move: Vec2::new(1.0, 0.0), ..Default::default() };
    for _ in 0..60 {
        sim.step(&[(target, run)]);
    }
    let seen_at = sim.soldiers[target].body.pos;
    for _ in 0..LAG {
        sim.step(&[(target, run)]);
    }
    assert!(sim.soldiers[target].body.pos.distance(seen_at) > 0.5, "far enough to be a clean miss");
    shoot_at(&mut sim, shooter, seen_at, LAG);
    let mut hit = false;
    for _ in 0..10 {
        sim.step(&[(target, run)]);
        hit |= was_hit(&sim, target);
    }
    assert!(hit, "aimed where they were seen, and the shot was fired with that much lag");
}

#[test]
fn without_rewinding_the_same_bullet_misses() {
    const LAG: u32 = 12;
    let (mut sim, shooter, target) = firing_range();
    sim.remember_past_ticks(60);
    let run = PlayerCmd { wish_move: Vec2::new(1.0, 0.0), ..Default::default() };
    for _ in 0..60 {
        sim.step(&[(target, run)]);
    }
    let seen_at = sim.soldiers[target].body.pos;
    for _ in 0..LAG {
        sim.step(&[(target, run)]);
    }
    shoot_at(&mut sim, shooter, seen_at, 0);
    for _ in 0..10 {
        sim.step(&[(target, run)]);
        assert!(!was_hit(&sim, target));
    }
}

#[test]
fn someone_already_dead_cannot_be_hit_in_the_past() {
    const LAG: u32 = 12;
    let (mut sim, shooter, target) = firing_range();
    sim.remember_past_ticks(60);
    let seen_at = sim.soldiers[target].body.pos;
    for _ in 0..LAG {
        sim.step(&[]);
    }
    sim.hurt(target, Zone::Head, 10_000.0);
    assert!(sim.soldiers[target].health.is_dead());
    shoot_at(&mut sim, shooter, seen_at, LAG);
    for _ in 0..10 {
        sim.step(&[]);
        assert!(!was_hit(&sim, target));
    }
}

#[test]
fn asking_for_more_rewind_than_is_kept_uses_the_present() {
    let (mut sim, shooter, target) = firing_range();
    sim.remember_past_ticks(5);
    let at = sim.soldiers[target].body.pos;
    shoot_at(&mut sim, shooter, at, 1000);
    let mut hit = false;
    for _ in 0..10 {
        sim.step(&[]);
        hit |= was_hit(&sim, target);
    }
    assert!(hit, "nothing that far back is on record, so they are where they are");
}

#[test]
fn a_soldier_who_is_not_present_takes_no_time_and_no_bullets() {
    let (mut sim, shooter, target) = firing_range();
    sim.soldiers[target].present = false;
    let before = sim.soldiers[target].clone();
    let at = before.body.pos;
    shoot_at(&mut sim, shooter, at, 0);
    for _ in 0..30 {
        sim.step(&[]);
        assert!(!was_hit(&sim, target));
    }
    assert_eq!(sim.soldiers[target], before);
}

#[test]
fn predicting_one_soldier_gives_exactly_what_the_whole_sim_gives() {
    let (mut server, shooter, target) = firing_range();
    let mut client = Sim::new(Level::flat(400.0));
    client.import_state(&server.export_state());
    let mut buttons = Buttons::default();
    buttons.set(Buttons::FIRE, true);
    for i in 0..240u32 {
        let tick = server.tick + 1;
        let cmd = PlayerCmd {
            tick,
            wish_move: Vec2::new(0.0, (i % 90 < 60) as u32 as f32),
            yaw: i as f32 * 0.01,
            pitch: 0.05,
            buttons: if i % 50 < 10 { buttons } else { Buttons::default() },
            ..Default::default()
        };
        server.step(&[(shooter, cmd), (target, PlayerCmd { tick, ..Default::default() })]);
        client.step_predicted(shooter, &cmd);
        assert_eq!(client.soldiers[shooter], server.soldiers[shooter], "tick {i}");
    }
}

#[test]
fn a_tracer_decides_nothing() {
    let (mut sim, shooter, target) = firing_range();
    let at = sim.soldiers[target].body.pos;
    shoot_at(&mut sim, shooter, at, 0);
    let health = sim.soldiers[target].health;
    for _ in 0..60 {
        sim.fly_tracers();
    }
    assert_eq!(sim.soldiers[target].health, health);
    assert!(sim.events.is_empty());
}

// ---- Leaning.

/// A soldier on flat ground facing -Z, 1 m behind the end of a long wall that runs off to their left and
/// has its end this far to their right. Set `wall_right` to 100 for no wall at all.
fn corner(wall_right: f32) -> (Sim, SoldierId) {
    let mut level = Level::flat(200.0);
    level.solids.push(prs_level::Solid {
        center: Vec3::new(wall_right - 10.0, 1.5, -1.0),
        half: Vec3::new(10.0, 1.5, 0.15),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let id = sim.spawn_soldier(7);
    settle(&mut sim, id, 0);
    (sim, id)
}

fn settle(sim: &mut Sim, id: SoldierId, ticks: usize) {
    for _ in 0..30 + ticks {
        hold(sim, id, Buttons::default());
    }
}

fn hold(sim: &mut Sim, id: SoldierId, buttons: Buttons) {
    drive(sim, id, Vec2::ZERO, 0.0, 0.0, buttons);
}

/// One press and release of a key, then as long as it takes for what that started to finish.
fn tap(sim: &mut Sim, id: SoldierId, button: Buttons) {
    hold(sim, id, button);
    settle(sim, id, 90);
}

#[test]
fn a_lean_toggles_and_the_other_key_swaps_side() {
    let (mut sim, id) = corner(100.0);
    let side = |sim: &Sim| sim.soldiers[id].body.lean.side;
    tap(&mut sim, id, Buttons::LEAN_LEFT);
    assert_eq!(side(&sim), -1);
    assert!(sim.soldiers[id].body.lean.amount < -0.95);
    tap(&mut sim, id, Buttons::LEAN_LEFT);
    assert_eq!(side(&sim), 0);
    assert_eq!(sim.soldiers[id].body.lean.amount, 0.0, "comes right back to rest");
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    assert_eq!(side(&sim), 1);
    tap(&mut sim, id, Buttons::LEAN_LEFT);
    assert_eq!(side(&sim), -1);
    assert!(sim.soldiers[id].body.lean.amount < -0.95);
}

#[test]
fn nobody_leans_lying_down_or_sprinting_and_sprinting_ends_a_lean() {
    let (mut sim, id) = corner(100.0);
    tap(&mut sim, id, Buttons::PRONE);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    assert_eq!(sim.soldiers[id].body.lean.side, 0);
    tap(&mut sim, id, Buttons::PRONE);
    tap(&mut sim, id, Buttons::CROUCH);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    assert_eq!(sim.soldiers[id].body.lean.side, 1, "crouched is fine");
    tap(&mut sim, id, Buttons::CROUCH);
    assert_eq!(sim.soldiers[id].body.lean.side, 1, "and standing up keeps it");
    // Off along the wall, away from it.
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::Y, std::f32::consts::PI, 0.0, Buttons::SPRINT);
    }
    assert_eq!(sim.soldiers[id].body.lean.side, 0);
    drive(&mut sim, id, Vec2::Y, std::f32::consts::PI, 0.0, Buttons::SPRINT.union(Buttons::LEAN_LEFT));
    assert_eq!(sim.soldiers[id].body.lean.side, 0);
}

#[test]
fn going_down_ends_a_lean() {
    let (mut sim, id) = corner(100.0);
    tap(&mut sim, id, Buttons::LEAN_LEFT);
    sim.hurt(id, Zone::Chest, 120.0);
    settle(&mut sim, id, 120);
    assert_eq!(sim.soldiers[id].body.lean, prs_movement::Lean::default());
}

/// The eye never jumps: it speeds up and slows down gently, whatever is pressed and whenever.
#[test]
fn the_eye_eases_with_no_snap_even_changing_its_mind() {
    let (mut sim, id) = corner(100.0);
    let script = |tick: u32| match tick {
        5 | 13 | 60 => Buttons::LEAN_LEFT,
        20 | 40 | 95 => Buttons::LEAN_RIGHT,
        _ => Buttons::default(),
    };
    let lean = sim.move_cfg.lean.clone();
    let mut eye = sim.soldiers[id].eye();
    let mut step = 0.0_f32;
    let (mut fastest, mut sharpest) = (0.0_f32, 0.0_f32);
    for tick in 0..200 {
        hold(&mut sim, id, script(tick));
        let now = sim.soldiers[id].eye();
        let side = now.x - eye.x;
        fastest = fastest.max(side.abs());
        sharpest = sharpest.max((side - step).abs());
        step = side;
        eye = now;
    }
    assert!(fastest < 2.5 * TICK_DT, "{fastest} m a tick is too fast for an eye");
    let most = lean.eye_side * lean.max_accel * TICK_DT * TICK_DT;
    assert!(sharpest <= most * 1.01, "the eye's speed changed {sharpest} m/tick in one tick, against {most}");
}

#[test]
fn a_lean_moves_the_eye_and_gun_over_and_the_bullet_leaves_from_there() {
    let (mut sim, id) = corner(100.0);
    let upright = (sim.soldiers[id].eye(), sim.soldiers[id].gun_pose(&sim.weapons));
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    let soldier = &sim.soldiers[id];
    let (eye, gun) = (soldier.eye(), soldier.gun_pose(&sim.weapons));
    let c = &sim.move_cfg.lean;
    assert!((eye.x - upright.0.x - c.eye_side).abs() < 0.01, "{eye}");
    assert!((upright.0.y - eye.y - c.eye_drop).abs() < 0.01, "{eye}");
    assert!((gun.muzzle.x - upright.1.muzzle.x - c.eye_side).abs() < 0.12, "{}", gun.muzzle);
    assert!(gun.rotation.angle_between(upright.1.rotation) > 0.1, "the gun should roll with the body");

    // A tick with the trigger pulled, against the gun as it was when it fired.
    hold(&mut sim, id, Buttons::FIRE);
    let origin = sim.events.iter().find_map(|e| match e {
        SimEvent::ShotFired { origin, .. } => Some(*origin),
        _ => None,
    });
    let origin = origin.expect("the shot went off");
    assert!(origin.distance(sim.soldiers[id].gun_pose(&sim.weapons).muzzle) < 0.02, "left from {origin}");
}

/// Rolled over, the sights are still zeroed: at the zero range the bullet's line crosses the sight
/// line and does not pass off to one side, which it would if the elevation stayed straight up.
#[test]
fn a_rolled_gun_still_shoots_where_its_sights_are_at_the_zero_range() {
    let (mut sim, id) = corner(100.0);
    let mut weapons = WeaponTable::default();
    weapons.weapons[0].dispersion_moa = 0.0;
    sim.set_weapons(weapons);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    settle(&mut sim, id, 300);
    let eye = sim.soldiers[id].eye();
    hold(&mut sim, id, Buttons::ADS);
    for _ in 0..120 {
        hold(&mut sim, id, Buttons::ADS);
    }
    hold(&mut sim, id, Buttons::ADS.union(Buttons::FIRE));
    let (origin, dir) = sim
        .events
        .iter()
        .find_map(|e| match e {
            SimEvent::ShotFired { origin, dir, .. } => Some((*origin, *dir)),
            _ => None,
        })
        .expect("the shot went off");
    let soldier = &sim.soldiers[id];
    let range = soldier.held_config(&sim.weapons).zero_range;
    let sights = soldier.eye() + soldier.gun_pose(&sim.weapons).forward * range;
    assert!(eye.distance(soldier.eye()) < 0.01);
    let off = origin + dir * range - sights;
    // Fired from below the sights and rising to them. Gravity brings it the rest of the way, so it
    // may be high here, but sideways it has to have arrived.
    assert!(off.x.abs() < 0.012, "{off} from the sight line at {range} m");
}

#[test]
fn a_wall_alongside_shortens_a_lean_and_the_eye_stays_out_of_it() {
    let mut level = Level::flat(200.0);
    // A wall running along the right side of someone looking down -Z, right up against them.
    level.solids.push(prs_level::Solid {
        center: Vec3::new(0.32 + 0.15, 1.5, -2.0),
        half: Vec3::new(0.15, 1.5, 8.0),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let id = sim.spawn_soldier(7);
    settle(&mut sim, id, 0);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    let lean = sim.soldiers[id].body.lean;
    assert_eq!(lean.side, 1);
    assert!(lean.amount > 0.1, "it leans what it can: {}", lean.amount);
    assert!(lean.amount < 0.7, "and no more: {}", lean.amount);
    let eye = sim.soldiers[id].eye();
    assert!(eye.x < 0.32 - sim.move_cfg.lean.least_clearance + 0.001, "eye at {eye}");

    tap(&mut sim, id, Buttons::LEAN_LEFT);
    assert!(sim.soldiers[id].body.lean.amount < -0.9, "the other way there is room");
}

#[test]
fn the_eye_is_never_inside_anything_whatever_a_leaning_soldier_does_in_town() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let id = sim.spawn_soldier(7);
    let stand = sim.move_cfg.lean.least_clearance * 0.8;
    let ball = Capsule { height: stand * 2.0, radius: stand };
    // In among the houses around the market, going back and forth along whatever is there.
    let market = sim.level.capture_points.iter().find(|p| p.name == "Market").unwrap().center;
    sim.set_home(id, market + Vec3::Y, 0.0);
    let mut leaned = 0;
    for tick in 0..7200u32 {
        let t = tick as f32 * TICK_DT;
        let mut b = Buttons::default();
        b.set(Buttons::LEAN_LEFT, tick % 400 == 20);
        b.set(Buttons::LEAN_RIGHT, tick % 400 == 220);
        b.set(Buttons::CROUCH, tick % 1100 == 500);
        drive(&mut sim, id, Vec2::new((t * 0.31).sin(), (t * 0.17).cos()), t * 0.9, (t * 0.4).sin() * 0.3, b);
        let soldier = &sim.soldiers[id];
        leaned += usize::from(soldier.body.lean.amount.abs() > 0.5);
        let eye = soldier.eye();
        assert!(sim.world.capsule_is_clear(eye - Vec3::Y * stand, ball), "tick {tick}: eye in something at {eye}");
    }
    assert!(leaned > 2000, "only leaned out for {leaned} ticks");
}

#[test]
fn leaning_limits_how_fast_you_walk() {
    let (mut sim, id) = corner(100.0);
    tap(&mut sim, id, Buttons::LEAN_LEFT);
    let yaw = std::f32::consts::PI;
    let mut top = 0.0_f32;
    for _ in 0..120 {
        drive(&mut sim, id, Vec2::Y, yaw, 0.0, Buttons::default());
        top = top.max(sim.soldiers[id].body.horizontal_speed());
    }
    let limit = sim.move_cfg.lean.max_speed;
    assert!(top <= limit + 0.01 && top > limit * 0.9, "walked at {top} with a limit of {limit}");
}

/// Going over shakes the gun, as a change of stance does, and it settles to what it was. A lean that
/// has settled is no worse to shoot from, and the fixed cone stays what it was all along.
#[test]
fn changing_lean_unsettles_the_gun_and_a_settled_lean_costs_nothing() {
    let (mut sim, id) = corner(100.0);
    for _ in 0..300 {
        hold(&mut sim, id, Buttons::ADS);
    }
    let calm = sim.soldiers[id].weapon.instability;
    let cone = sim.soldiers[id].held_config(&sim.weapons).dispersion_moa;
    hold(&mut sim, id, Buttons::ADS.union(Buttons::LEAN_RIGHT));
    let mut worst = calm;
    for _ in 0..40 {
        hold(&mut sim, id, Buttons::ADS);
        worst = worst.max(sim.soldiers[id].weapon.instability);
    }
    assert!(worst > calm + 0.1, "{worst} against {calm}");
    for _ in 0..600 {
        hold(&mut sim, id, Buttons::ADS);
    }
    let settled = sim.soldiers[id].weapon.instability;
    assert!((settled - calm).abs() < 0.01, "{settled} against {calm}");
    assert_eq!(sim.soldiers[id].held_config(&sim.weapons).dispersion_moa, cone);
}

/// The whole point: someone holding a corner sees past it with a little of themselves showing, where
/// stepping out to get that view shows everything.
#[test]
fn holding_a_corner_shows_less_of_you_than_stepping_out() {
    let (mut sim, id) = corner(0.2);
    let target = Vec3::new(3.0, 1.5, -25.0);
    let sees = |sim: &Sim, eye: Vec3| {
        let path = target - eye;
        sim.world.raycast(eye, path.normalize(), path.length()).is_none()
    };
    assert!(!sees(&sim, sim.soldiers[id].eye()), "the wall should hide the target to begin with");
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    assert!(sees(&sim, sim.soldiers[id].eye()), "the lean should clear the corner");

    let shown = |sim: &Sim| {
        let pose = sim.body_pose(&sim.body_input(id));
        prs_body::Bone::ALL
            .iter()
            .filter(|bone| {
                let at = pose[**bone].start.lerp(pose[**bone].end(), 0.5);
                let path = target - at;
                sim.world.raycast(at, path.normalize(), path.length()).is_none()
            })
            .count()
    };
    let leaning = shown(&sim);
    assert!(leaning > 0, "the head at least shows to something it can see");

    // Now the same view with both feet moved out from behind the wall, and standing up straight.
    let eye = sim.soldiers[id].eye();
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    sim.soldiers[id].body.pos.x += eye.x - sim.soldiers[id].eye().x;
    settle(&mut sim, id, 30);
    let stepping = shown(&sim);
    assert!(stepping >= leaning + 3, "{leaning} bones show leaning, and {stepping} stepping out");
}

/// Hit zones are built from bones, so they lean too.
#[test]
fn a_leaning_head_is_hit_where_it_is_and_not_where_it_was() {
    let (mut sim, id) = corner(100.0);
    let shooter = sim.spawn_soldier(8);
    sim.set_home(shooter, Vec3::new(20.0, 0.1, 20.0), 0.0);
    settle(&mut sim, id, 0);
    let where_head = |sim: &Sim| bone_middle(sim, id, prs_body::Bone::Head);
    let was = where_head(&sim);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    let head = where_head(&sim);
    assert!(head.distance(was) > 0.2, "the head stayed at {head}");

    // Square across where the head used to be, going by to its left, and it hits nothing.
    let before = sim.soldiers[id].health.health;
    fire_at(&mut sim, shooter, RIFLE, was + Vec3::new(0.0, 0.0, 3.0), was);
    settle(&mut sim, id, 10);
    assert_eq!(sim.soldiers[id].health.health, before, "shot through where the head no longer is");

    fire_at(&mut sim, shooter, RIFLE, head + Vec3::new(0.0, 0.0, 3.0), head);
    let mut zone = None;
    for _ in 0..10 {
        hold(&mut sim, id, Buttons::default());
        zone = zone.or(sim.events.iter().find_map(|e| match e {
            SimEvent::SoldierHit { zone, .. } => Some(*zone),
            _ => None,
        }));
    }
    assert_eq!(zone, Some(Zone::Head));
}

// ---- Where a gun is, for every gun and every way of holding it ----

/// The muzzle is out in front of the eye along the barrel, and the hands can reach what they hold. A
/// bullet starts from that muzzle. If any of this failed, a shot would come from somewhere behind you.
#[test]
fn every_gun_fires_from_a_muzzle_ahead_of_the_eye_and_within_reach() {
    for weapon in [RIFLE, LMG, DMR, PISTOL] {
        for ads in [false, true] {
            for stance in [Stance::Stand, Stance::Crouch, Stance::Prone] {
                let mut sim = range_with(300.0, weapon);
                let mut s = Shooter::new(&mut sim, 7);
                s.yaw = 0.7;
                s.pitch = -0.05;
                s.buttons.set(Buttons::ADS, ads);
                s.stance(stance);
                s.wait(3.0);
                let cfg = s.sim.weapons[weapon].clone();
                let name = format!("{} ads {ads} {stance:?}", cfg.name);
                let bones = &s.sim.body_cfg.bones;
                let reach = bones.shoulder + bones.upper_arm + bones.forearm + bones.hand;

                let soldier = &s.sim.soldiers[s.id];
                let eye = soldier.body.eye();
                let pose = soldier.gun_pose(&s.sim.weapons);
                let along = (pose.muzzle - eye).dot(pose.forward);
                assert!(along > 0.4, "{name}: muzzle only {along} m ahead of the eye");
                let trigger = pose.origin + pose.rotation * cfg.hold.grips.trigger;
                assert!(trigger.distance(eye) < reach, "{name}: grip out of reach at {}", trigger.distance(eye));
                assert!(pose.muzzle.distance(eye) < reach + cfg.hold.muzzle_forward, "{name}");

                s.buttons.set(Buttons::FIRE, true);
                s.tick();
                let shots = shot_origins(&s);
                assert_eq!(shots.len(), 1, "{name}");
                let (origin, dir) = shots[0];
                assert!(origin.distance(pose.muzzle) < 0.02, "{name}: muzzle {} but shot from {origin}", pose.muzzle);
                assert!(dir.dot(pose.forward) > 0.998, "{name}");
                assert!((origin - eye).dot(dir) > 0.4, "{name}: shot starts behind the eye");
            }
        }
    }
}

/// A gun held further out than the eye goes finds the wall first, and that decides how far the lean goes.
#[test]
fn a_gun_held_out_wide_shortens_a_lean_as_well() {
    let mut level = Level::flat(200.0);
    level.solids.push(prs_level::Solid {
        center: Vec3::new(0.75 + 0.15, 1.5, -2.0),
        half: Vec3::new(0.15, 1.5, 8.0),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let mut weapons = WeaponTable::default();
    weapons.weapons[0].hold.hip.x = 0.6;
    sim.set_weapons(weapons);
    let id = sim.spawn_soldier(7);
    settle(&mut sim, id, 0);
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    let lean = sim.soldiers[id].body.lean.amount;
    assert!(lean < 0.85, "it should stop short for the gun: {lean}");
    let eye = sim.soldiers[id].eye();
    assert!(eye.x + sim.move_cfg.lean.clearance < 0.7, "and well before the eye gets anywhere near: {eye}");
}

/// Leaning round the end of a wall into another part of it, the barrel has to come back like any other time.
#[test]
fn a_barrel_leaned_at_a_wall_pulls_back_rather_than_going_through() {
    let mut level = Level::flat(200.0);
    level.solids.push(prs_level::Solid {
        center: Vec3::new(5.3, 1.5, -0.75),
        half: Vec3::new(5.0, 1.5, 0.15),
        rotation: glam::Quat::IDENTITY,
        paint: prs_level::Paint::Concrete,
    });
    let mut sim = Sim::new(level);
    let id = sim.spawn_soldier(7);
    settle(&mut sim, id, 60);
    assert_eq!(sim.soldiers[id].gun_retract, 0.0, "it starts out clear of the wall");
    tap(&mut sim, id, Buttons::LEAN_RIGHT);
    let soldier = &sim.soldiers[id];
    assert!(soldier.body.lean.amount > 0.9);
    assert!(soldier.gun_retract > 0.0, "the gun should have pulled back");
    assert!(soldier.gun_pose(&sim.weapons).muzzle.z > -0.6, "muzzle at {}", soldier.gun_pose(&sim.weapons).muzzle);
}

/// A lean is predicted by whoever is doing it, so a client stepping only themselves has to come to
/// exactly what the server does, down to the spring and the walls it fits against.
#[test]
fn a_predicted_lean_agrees_with_the_server_to_the_bit() {
    let mut server = Sim::new(Level::named("range").unwrap());
    let id = server.spawn_soldier(3);
    let mut client = Sim::new(Level::named("range").unwrap());
    client.spawn_soldier(3);
    // The compound's wall on the right, so that it shortens the lean part of the way along.
    for sim in [&mut server, &mut client] {
        let top = Vec3::new(-27.3, 60.0, -24.0);
        let ground = sim.world.raycast(top, Vec3::NEG_Y, 120.0).unwrap().distance;
        sim.set_home(id, top - Vec3::Y * (ground - 0.05), 0.0);
    }
    let mut shortest = 1.0_f32;
    for tick in 0..900u32 {
        let mut buttons = Buttons::default();
        buttons.set(Buttons::LEAN_LEFT, tick % 300 == 20);
        buttons.set(Buttons::LEAN_RIGHT, tick % 300 == 130);
        buttons.set(Buttons::CROUCH, tick == 500);
        let wish = Vec2::new(((tick as f32) * 0.02).sin(), 0.3);
        let cmd = PlayerCmd { tick: tick + 1, wish_move: wish, yaw: (tick as f32 * 0.004).sin() * 0.2, pitch: 0.0, buttons, ..Default::default() };
        server.step(&[(id, cmd)]);
        client.step_predicted(id, &cmd);
        assert_eq!(server.soldiers[id].body, client.soldiers[id].body, "differs at tick {tick}");
        assert_eq!(server.soldiers[id].gun_pose(&server.weapons).muzzle, client.soldiers[id].gun_pose(&client.weapons).muzzle);
        if server.soldiers[id].body.lean.side == -1 {
            shortest = shortest.min(server.soldiers[id].body.lean.amount.abs());
        }
    }
    assert!(shortest < 0.9, "the wall never came into it: {shortest}");
}

// ---- Suppression ----

/// A soldier standing still with a shooter off to one side, and how rattled the soldier is.
fn level_after_pass(sideways: f32) -> (f32, Vec<SimEvent>) {
    let (mut sim, shooter, victim) = victim_on_range();
    let eye = sim.soldiers[victim].eye();
    let at = eye + Vec3::X * sideways;
    fire_at(&mut sim, shooter, RIFLE, at + Vec3::new(0.0, 0.0, 60.0), at);
    let mut events = Vec::new();
    for _ in 0..30 {
        sim.step(&[]);
        events.extend(sim.events.iter().copied().filter(|e| matches!(e, SimEvent::BulletPassed { .. })));
    }
    (sim.soldiers[victim].suppression.level, events)
}

#[test]
fn a_near_miss_at_a_metre_rattles_you_and_one_thirty_metres_off_does_nothing() {
    let (near, events) = level_after_pass(1.0);
    assert!(near > 0.1, "{near}");
    assert_eq!(events.len(), 1, "a bullet passes once: {events:?}");
    let SimEvent::BulletPassed { distance, cracked, .. } = events[0] else { unreachable!() };
    assert!((distance - 1.0).abs() < 0.1, "{distance}");
    assert!(cracked);
    let (far, events) = level_after_pass(30.0);
    assert_eq!(far, 0.0);
    assert!(events.is_empty());
    assert!(level_after_pass(0.6).0 > near, "closer is worse");
}

#[test]
fn your_own_bullets_do_not_rattle_you_and_a_mates_past_your_shoulder_does_not_either() {
    let mut sim = range(500.0);
    sim.targets.clear();
    let (me, mate) = (sim.spawn_soldier(1), sim.spawn_soldier(2));
    sim.set_home(mate, Vec3::new(1.0, 0.05, 0.0), 0.0);
    idle_for(&mut sim, me, 1.0);
    for _ in 0..120 {
        drive(&mut sim, me, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE));
        sim.soldiers[me].weapon.ammo = 30;
    }
    assert_eq!(sim.soldiers[me].suppression.level, 0.0);
    assert_eq!(sim.soldiers[mate].suppression.level, 0.0, "firing next to someone is not firing at them");
}

#[test]
fn friendly_fire_coming_past_you_counts_the_same_as_enemy_fire() {
    let (mut sim, shooter, victim) = victim_on_range();
    assert_eq!(sim.soldiers[shooter].team, sim.soldiers[victim].team);
    let at = sim.soldiers[victim].eye() + Vec3::X;
    fire_at(&mut sim, shooter, RIFLE, at + Vec3::new(0.0, 0.0, 60.0), at);
    hits_landing(&mut sim);
    assert!(sim.soldiers[victim].suppression.level > 0.1);
    assert_eq!(sim.soldiers[victim].suppression.last_shooter, Some(shooter));
}

#[test]
fn a_bullet_striking_close_by_counts_and_a_wall_between_you_takes_most_of_it() {
    let wall = |gap: f32| {
        let mut sim = soldier_at_wall(gap);
        sim.targets.clear();
        let shooter = sim.spawn_soldier(1);
        let victim = sim.spawn_soldier(2);
        sim.set_home(shooter, Vec3::new(100.0, 0.05, 100.0), 0.0);
        idle_for(&mut sim, shooter, 1.0);
        (sim, shooter, victim)
    };
    // From your side of the wall, into the wall a metre away.
    let (mut sim, shooter, victim) = wall(1.0);
    let eye = sim.soldiers[victim].eye();
    fire_at(&mut sim, shooter, RIFLE, eye + Vec3::new(40.0, 0.0, 30.0), eye + Vec3::new(0.5, 0.0, -1.2));
    hits_landing(&mut sim);
    let open = sim.soldiers[victim].suppression.level;
    assert!(open > 0.05, "{open}");

    // From the far side of the same wall, stopping on it at the same place.
    let (mut sim, shooter, victim) = wall(1.0);
    let eye = sim.soldiers[victim].eye();
    fire_at(&mut sim, shooter, RIFLE, eye + Vec3::new(0.5, 0.0, -40.0), eye + Vec3::new(0.5, 0.0, -1.2));
    hits_landing(&mut sim);
    let covered = sim.soldiers[victim].suppression.level;
    assert!(covered > 0.0 && covered < open * 0.6, "behind cover {covered} vs in the open {open}");
}

#[test]
fn bigger_rounds_rattle_more_and_a_slow_pistol_round_least() {
    let of = |weapon: WeaponId, flown: f32| {
        let (mut sim, shooter, victim) = victim_on_range();
        let at = sim.soldiers[victim].eye() + Vec3::X;
        fire_at(&mut sim, shooter, weapon, at + Vec3::new(0.0, 0.0, flown), at);
        hits_landing(&mut sim);
        sim.soldiers[victim].suppression.level
    };
    assert!(of(DMR, 40.0) > of(LMG, 40.0) && of(LMG, 40.0) > of(RIFLE, 40.0) && of(RIFLE, 40.0) > of(PISTOL, 40.0));
    // Slower than sound by now, so it no longer cracks either.
    assert!(of(PISTOL, 200.0) < of(PISTOL, 8.0) * 0.8, "{} vs {}", of(PISTOL, 200.0), of(PISTOL, 8.0));
}

/// A round passing by every so often, at the eye of someone holding the sights on a target.
fn shots_under_fire(seed: u32, rattled: bool) -> u32 {
    let mut sim = range(150.0);
    let mut s = Shooter::new(&mut sim, seed + 1);
    let enemy = s.sim.spawn_soldier(seed + 500);
    s.sim.set_home(enemy, Vec3::new(120.0, 0.05, 120.0), 0.0);
    s.buttons.set(Buttons::ADS, true);
    s.stance(Stance::Crouch);
    for tick in 0..(5.0 * TICKS) as u32 {
        if rattled && tick % 40 == 0 {
            let at = s.sim.soldiers[s.id].eye() + Vec3::new(1.0, 0.3, 0.0);
            fire_at(s.sim, enemy, RIFLE, at + Vec3::new(30.0, 0.0, 30.0), at);
        }
        s.track_tick();
    }
    if rattled {
        assert!(s.sim.soldiers[s.id].suppression.level > 0.5);
    }
    s.buttons.set(Buttons::FIRE, true);
    s.track_tick();
    s.buttons.set(Buttons::FIRE, false);
    s.hits()
}

/// It costs shots through the gun itself, and the cone stays the weapon's own.
#[test]
fn being_suppressed_really_does_cost_shots_through_the_gun_moving() {
    let calm = over_seeds(TRIALS, |seed| shots_under_fire(seed, false));
    let under_fire = over_seeds(TRIALS, |seed| shots_under_fire(seed, true));
    assert!(under_fire < calm * 0.8, "under fire {under_fire} vs calm {calm}");

    let mut sim = range(150.0);
    let id = sim.spawn_soldier(4);
    sim.soldiers[id].suppression.level = 1.0;
    sim.soldiers[id].suppression.hold = 1000.0;
    let limit = sim.weapons[RIFLE].dispersion_radius() * 1.001;
    let mut fired = 0;
    for tick in 0..2000 {
        let mut buttons = press(Buttons::ADS);
        buttons.set(Buttons::FIRE, tick % 12 == 0);
        sim.soldiers[id].weapon.ammo = 30;
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons);
        for event in &sim.events {
            if let SimEvent::Weapon { event: WeaponEvent::Fired { gun_offset, direction_offset }, .. } = event {
                fired += 1;
                assert!((*direction_offset - *gun_offset).length() <= limit, "the cone got wider");
            }
        }
    }
    assert!(fired > 100);
}

#[test]
fn it_wears_off_slowly_and_coming_back_from_the_dead_starts_calm() {
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.2);
    let cfg = sim.suppression_cfg.clone();
    sim.soldiers[id].suppression.add(&cfg, 1.0, 9);
    idle_for(&mut sim, id, cfg.hold + 2.0);
    let some = sim.soldiers[id].suppression.level;
    assert!(some > 0.6 && some < 0.95, "{some}");
    idle_for(&mut sim, id, 10.0);
    assert_eq!(sim.soldiers[id].suppression.level, 0.0);

    sim.soldiers[id].suppression.add(&cfg, 1.0, 9);
    sim.hurt(id, Zone::Head, 300.0);
    let respawn = sim.health_cfg.respawn_time;
    idle_for(&mut sim, id, respawn + 0.5);
    assert!(sim.soldiers[id].health.is_up());
    assert_eq!(sim.soldiers[id].suppression, prs_suppression::Suppression::default());
}

#[test]
fn a_mod_can_turn_it_off_altogether() {
    let (mut sim, shooter, victim) = victim_on_range();
    sim.suppression_cfg.enabled = false;
    let at = sim.soldiers[victim].eye() + Vec3::X;
    fire_at(&mut sim, shooter, RIFLE, at + Vec3::new(0.0, 0.0, 60.0), at);
    for _ in 0..30 {
        sim.step(&[]);
        assert!(sim.events.iter().all(|e| !matches!(e, SimEvent::BulletPassed { .. })));
    }
    assert_eq!(sim.soldiers[victim].suppression.level, 0.0);
}

#[test]
fn suppression_comes_through_an_export_and_stays_in_step() {
    let (mut sim, shooter, victim) = victim_on_range();
    let at = sim.soldiers[victim].eye() + Vec3::X;
    fire_at(&mut sim, shooter, RIFLE, at + Vec3::new(0.0, 0.0, 60.0), at);
    hits_landing(&mut sim);
    let state = sim.export_state();
    assert!(state.soldiers[victim].suppression.level > 0.0);
    let (mut other, ..) = victim_on_range();
    other.import_state(&state);
    for _ in 0..600 {
        sim.step(&[]);
        other.step(&[]);
        assert_eq!(sim.soldiers[victim].suppression, other.soldiers[victim].suppression);
        assert_eq!(sim.soldiers[victim].weapon.instability, other.soldiers[victim].weapon.instability);
    }
}

/// A bot with human reactions: looks at someone's chest, and corrects for the gun 100 ms late.
struct FightBot {
    id: SoldierId,
    seen: std::collections::VecDeque<Vec2>,
}

impl FightBot {
    fn new(id: SoldierId) -> Self {
        Self { id, seen: Default::default() }
    }

    fn cmd(&mut self, sim: &Sim, at: SoldierId, wish: Vec2, buttons: Buttons) -> (SoldierId, PlayerCmd) {
        let me = &sim.soldiers[self.id];
        self.seen.push_back(me.weapon.aim_offset());
        let late = if self.seen.len() > 6 { self.seen.pop_front().unwrap() } else { self.seen[0] };
        let d = sim.soldiers[at].body.pos + Vec3::Y * (sim.soldiers[at].body.eye_height * 0.75) - me.eye();
        let yaw = (-d.x).atan2(-d.z) - late.x;
        let pitch = (d.y / d.length()).asin() - late.y;
        (self.id, PlayerCmd { tick: sim.tick, wish_move: wish, yaw, pitch, buttons, ..Default::default() })
    }
}

/// One person holding the trigger down against two who take turns: one puts aimed bursts on them from
/// the ground while the other moves up, stops and takes careful shots. Returns whether the sprayer went
/// down first, and how many rounds of theirs landed.
fn fire_and_move_fight(seed: u32, suppression: bool) -> (bool, u32) {
    let mut sim = range(500.0);
    sim.targets.clear();
    sim.suppression_cfg.enabled = suppression;
    let sprayer = sim.spawn_soldier(seed * 3 + 1);
    let (base, mover) = (sim.spawn_soldier(seed * 3 + 2), sim.spawn_soldier(seed * 3 + 3));
    sim.set_home(sprayer, Vec3::new(0.0, 0.05, 0.0), 0.0);
    sim.set_home(base, Vec3::new(-8.0, 0.05, -90.0), 3.14);
    sim.set_home(mover, Vec3::new(10.0, 0.05, -100.0), 3.14);
    sim.soldiers[sprayer].weapon.mode = FireMode::Auto;
    sim.soldiers[base].weapon.mode = FireMode::Auto;

    let mut bots = (FightBot::new(sprayer), FightBot::new(base), FightBot::new(mover));
    let mut landed = 0;
    for tick in 0..(60.0 * TICKS) as u32 {
        let alive = |sim: &Sim, id: SoldierId| sim.soldiers[id].health.is_up();
        if !alive(&sim, sprayer) {
            return (true, landed);
        }
        if !alive(&sim, base) && !alive(&sim, mover) {
            return (false, landed);
        }

        // The sprayer goes for whoever is still up and nearest, and only stops to reload.
        let (near, far) = {
            let d = |id: SoldierId| sim.soldiers[id].body.pos.distance(sim.soldiers[sprayer].body.pos);
            if d(mover) < d(base) { (mover, base) } else { (base, mover) }
        };
        let aim = if alive(&sim, near) { near } else { far };
        let mut b = press(Buttons::ADS);
        if sim.soldiers[sprayer].weapon.ammo == 0 {
            b.set(Buttons::RELOAD, tick % 30 == 0);
        } else if tick > 120 {
            b.set(Buttons::FIRE, true);
        }
        let spray = bots.0.cmd(&sim, aim, Vec2::ZERO, b);

        // Base of fire: prone, three rounds every second and a bit.
        let mut b = press(Buttons::ADS);
        b.set(Buttons::PRONE, tick == 5);
        if sim.soldiers[base].weapon.ammo == 0 {
            b.set(Buttons::RELOAD, tick % 30 == 0);
        } else {
            b.set(Buttons::FIRE, tick > 180 && tick % 75 < 15);
        }
        let covering = bots.1.cmd(&sim, sprayer, Vec2::ZERO, b);

        // Mover: three seconds forward, then three stopped with one careful shot at the end.
        let phase = tick % 360;
        let moving = phase < 180 && sim.soldiers[mover].body.pos.distance(sim.soldiers[sprayer].body.pos) > 30.0;
        let mut b = Buttons::default();
        b.set(Buttons::ADS, !moving);
        if sim.soldiers[mover].weapon.ammo == 0 {
            b.set(Buttons::RELOAD, tick % 30 == 0);
        } else {
            b.set(Buttons::FIRE, !moving && (phase == 300 || phase == 340));
        }
        let wish = if moving { Vec2::Y } else { Vec2::ZERO };
        let moved = bots.2.cmd(&sim, sprayer, wish, b);

        sim.step(&[spray, covering, moved]);
        landed += sim.events.iter().filter(|e| matches!(e, SimEvent::SoldierHit { shooter, .. } if *shooter == sprayer)).count() as u32;
    }
    (false, landed)
}

/// Two against one is a head start. The point here is that spraying does not make up for it. Being shot
/// at rattles both sides alike, so this is not what suppression is for on its own: the numbers are
/// printed to compare with it turned off.
#[test]
fn fire_and_move_beats_a_lone_sprayer() {
    const FIGHTS: u32 = 24;
    let run = |suppression: bool| {
        let results: Vec<(bool, u32)> = (0..FIGHTS).map(|seed| fire_and_move_fight(seed, suppression)).collect();
        (results.iter().filter(|r| r.0).count(), results.iter().map(|r| r.1).sum::<u32>())
    };
    let (wins, landed) = run(true);
    let (wins_off, landed_off) = run(false);
    println!("fire and move won {wins}/{FIGHTS}, sprayer landed {landed}; suppression off: won {wins_off}/{FIGHTS}, sprayer landed {landed_off}");
    assert!(wins as u32 >= FIGHTS * 3 / 4, "won only {wins}/{FIGHTS}");
}

// ---- Mods ----

const ARCADE: &str = include_str!("../../../mods/arcade.toml");
const HARDCORE: &str = include_str!("../../../mods/hardcore.toml");
const HEAVY_RECOIL: &str = include_str!("../../../mods/heavy-recoil.toml");

/// `range`, under a mod.
fn modded_range(distance: f32, text: &str) -> Sim {
    let mut level = Level::training_range();
    level.solids.clear();
    level.trees.clear();
    level.terrain = prs_level::Terrain::generate(1200.0, 8, |_| 0.0);
    level.spawn = Vec3::new(0.0, 0.05, 0.0);
    level.targets = vec![Vec3::new(0.0, 0.0, -distance)];
    let active = prs_mods::Mod::parse("test", text).unwrap_or_else(|e| panic!("{e}"));
    Sim::with_mod(level, &active).unwrap_or_else(|e| panic!("{e}"))
}

fn spray_rate(mut sim: Sim, seed: u32) -> u32 {
    mag_dump(&mut sim, seed, MAG, |s| s.react_late())
}

#[test]
fn one_line_is_a_mod_and_it_changes_only_that() {
    let plain = modded_range(50.0, "");
    let tuned = modded_range(50.0, "weapons.m4.rounds_per_minute = 900");
    assert_eq!(tuned.weapons[RIFLE].rounds_per_minute, 900.0);
    assert_eq!(format!("{:?}", tuned.weapons[LMG]), format!("{:?}", plain.weapons[LMG]));
    assert_eq!(format!("{:?}", tuned.move_cfg), format!("{:?}", plain.move_cfg));
    assert_eq!(tuned.weapons[RIFLE].key, "m4");
    assert_eq!(tuned.weapons[LMG].key, "m249", "ids stay in the same order");
    assert_eq!(tuned.weapons[RIFLE].name, plain.weapons[RIFLE].name);
}

#[test]
fn a_mistake_in_a_mod_names_its_path() {
    let level = || Level::flat(100.0);
    let bad = |text: &str| {
        let active = prs_mods::Mod::parse("oops", text).unwrap();
        Sim::with_mod(level(), &active).err().map(|e| e.to_string()).expect("should be refused")
    };
    assert_eq!(bad("weapons.m4.rounds_per_minuet = 9"), "mod 'oops': there is no setting weapons.m4.rounds_per_minuet. Did you mean rounds_per_minute?");
    assert_eq!(bad("movement.run_speed = \"fast\""), "mod 'oops': movement.run_speed should be a number, not text");
    assert!(bad("health.max_health = { a = 1 }").contains("health.max_health should be a number"));
    assert!(bad("vehicles.tank.mass = 1").contains("vehicles.tank"));
}

/// Going through a mod's tree and back must not move a number, or merely mentioning a section would
/// change the rules and be refused by a server playing the very same ones.
#[test]
fn a_mod_that_sets_things_to_what_they_are_changes_nothing() {
    let plain = modded_range(50.0, "");
    let same = modded_range(
        50.0,
        &format!(
            "weapons.m4.magazine = {}\nmovement.prone_speed = {}\nhealth.bandage_time = {}\nsuppression.reach = {}\n\
             inventory.kit_lifetime = {}\nbody.collapse.fall_time = {}\nvehicles.truck.mass = {}\nhelicopter.seats = []\n\
             vehicle_ride.steps = 0\nworld.pitch_limit = {}",
            plain.weapons[RIFLE].magazine,
            plain.move_cfg.prone_speed,
            plain.health_cfg.bandage_time,
            plain.suppression_cfg.reach,
            plain.inventory_cfg.kit_lifetime,
            plain.body_cfg.collapse.fall_time,
            plain.vehicle_cfgs.vehicles[0].mass,
            plain.world_cfg.pitch_limit,
        )
        .replace("helicopter.seats = []\n", "")
        .replace("vehicle_ride.steps = 0\n", ""),
    );
    assert_eq!(format!("{:?}", same.weapons), format!("{:?}", plain.weapons));
    assert_eq!(format!("{:?}", same.move_cfg), format!("{:?}", plain.move_cfg));
    assert_eq!(format!("{:?}", same.health_cfg), format!("{:?}", plain.health_cfg));
    assert_eq!(format!("{:?}", same.suppression_cfg), format!("{:?}", plain.suppression_cfg));
    assert_eq!(format!("{:?}", same.inventory_cfg), format!("{:?}", plain.inventory_cfg));
    assert_eq!(format!("{:?}", same.body_cfg), format!("{:?}", plain.body_cfg));
    assert_eq!(format!("{:?}", same.vehicle_cfgs), format!("{:?}", plain.vehicle_cfgs));
}

#[test]
fn arcade_is_instant_aim_and_next_to_no_recoil() {
    let sim = modded_range(100.0, ARCADE);
    let cfg = &sim.weapons[RIFLE];
    assert!(cfg.ads_time <= 0.02, "{}", cfg.ads_time);
    assert!(cfg.recoil.kick_up < 5.0);

    // Sights up within a tick or two of asking.
    let mut sim = modded_range(100.0, ARCADE);
    let mut s = Shooter::new(&mut sim, 5);
    s.buttons.set(Buttons::ADS, true);
    s.wait(2.0 / TICKS);
    assert_eq!(s.weapon().ads, 1.0);

    // A whole magazine stays on someone at 100 m, where the real game gets a few rounds out of thirty.
    let arcade = over_seeds(TRIALS, |seed| spray_rate(modded_range(100.0, ARCADE), seed)) / MAG as f32;
    let real = over_seeds(TRIALS, |seed| spray_rate(modded_range(100.0, ""), seed)) / MAG as f32;
    assert!(arcade > 0.7 && arcade > real * 3.0, "arcade {arcade} vs real {real}");

    // And firing on the run actually works.
    let run_and_gun = over_seeds(TRIALS, |seed| {
        let mut sim = modded_range(25.0, ARCADE);
        let mut s = Shooter::new(&mut sim, seed);
        s.wish = Vec2::new(1.0, 0.0);
        s.wait(1.0);
        let start = s.weapon().ammo;
        s.sim.soldiers[s.id].weapon.mode = FireMode::Auto;
        s.buttons.set(Buttons::FIRE, true);
        while start - s.weapon().ammo < MAG {
            s.aim_at_target();
            s.keep_target_up();
            s.tick();
        }
        s.sim.targets[0].hits
    }) / MAG as f32;
    assert!(run_and_gun > 0.5, "strafing at 25 m hit {run_and_gun}");
}

#[test]
fn hardcore_hurts_more_and_a_bandage_takes_longer() {
    let plain = modded_range(50.0, "");
    let hard = modded_range(50.0, HARDCORE);
    assert!(hard.health_cfg.chest.bleed > plain.health_cfg.chest.bleed);
    assert!(hard.health_cfg.bandage_time > plain.health_cfg.bandage_time);
    assert!(hard.health_cfg.respawn_time > plain.health_cfg.respawn_time);

    // One rifle round to the chest puts you down, where the real game takes two.
    let mut sim = modded_range(50.0, HARDCORE);
    let id = sim.spawn_soldier(1);
    let raw = sim.weapons[RIFLE].damage;
    sim.hurt(id, Zone::Chest, raw);
    assert!(!sim.soldiers[id].health.is_up());
}

#[test]
fn heavy_recoil_makes_bursts_worse_and_leaves_a_first_shot_alone() {
    let first = |text: &str| {
        over_seeds(TRIALS, |seed| {
            let mut sim = modded_range(100.0, text);
            aimed_shots(&mut sim, seed, 1, Stance::Crouch, 0.0)
        })
    };
    assert_eq!(first(HEAVY_RECOIL), first(""), "the first shot has not kicked yet");
    let burst_hits = |text: &str, range_m: f32| {
        over_seeds(TRIALS, |seed| burst(&mut modded_range(range_m, text), seed, 3, true))
    };
    let (heavy, plain) = (burst_hits(HEAVY_RECOIL, 25.0), burst_hits("", 25.0));
    assert!(heavy < plain, "3 round burst at 25 m: heavy {heavy} vs plain {plain}");
}

/// The user's rule from the start: zero and extreme values have to keep working, so that instant aim
/// is a config change and not a crash.
#[test]
fn zero_and_extreme_values_do_not_break_anything() {
    let text = "[weapons.\"*\"]\nads_time = 0.0\nraise_time = 0.0\nlower_time = 0.0\nstow_time = 0.0\ndraw_time = 0.0\n\
                dispersion_moa = 0.0\nreload_time = 0.0\nreload_empty_time = 0.0\n\
                [weapons.\"*\".recoil]\nkick_up = 0.0\nkick_side = 0.0\nrecovery_rate = 0.0\n\
                [weapons.\"*\".stability]\nmax = 0.0\nrise_time = 0.0\nsettle_time = 0.0\n\
                [health]\nbandage_time = 0.0\nrespawn_time = 0.0\ndown_time = 0.0\n\
                [movement]\nsprint_speed = 40.0\nstamina_sprint_drain = 0.0\nstand_crouch_time = 0.0\ncrouch_prone_time = 0.0\n\
                [suppression]\nreach = 0.0\nhold = 0.0\ndecay = 0.0";
    let mut sim = modded_range(50.0, text);
    let id = sim.spawn_soldier(3);
    for tick in 0..600 {
        let mut b = Buttons::default();
        b.set(Buttons::FIRE, tick % 7 == 0);
        b.set(Buttons::ADS, tick % 90 < 45);
        b.set(Buttons::RELOAD, tick % 100 == 50);
        b.set(Buttons::SLOT_2, tick % 200 == 100);
        b.set(Buttons::SLOT_1, tick % 200 == 0);
        b.set(Buttons::PRONE, tick % 150 == 30);
        b.set(Buttons::SPRINT, tick % 300 > 200);
        drive(&mut sim, id, Vec2::new(0.2, 0.8), tick as f32 * 0.01, 0.0, b);
        let s = &sim.soldiers[id];
        assert!(s.body.pos.is_finite() && s.weapon.aim_offset().is_finite(), "at {tick}: {} {}", s.body.pos, s.weapon.aim_offset());
        assert!(s.weapon.ads.is_finite() && s.weapon.ready.is_finite() && s.weapon.instability.is_finite(), "at {tick}");
    }
}

/// Held through being down it is the same press, so getting back up does not fire it off again.
#[test]
fn a_button_held_while_down_does_not_act_again_on_being_revived() {
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.5);
    let mode = sim.soldiers[id].weapon.mode;
    sim.hurt(id, Zone::Chest, 120.0);
    assert!(!sim.soldiers[id].health.is_up());
    for _ in 0..30 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE_MODE));
    }
    let cfg = sim.health_cfg.clone();
    let mut none = Vec::new();
    sim.soldiers[id].health.medkit(&cfg, &mut none);
    for _ in 0..30 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE_MODE));
    }
    assert_eq!(sim.soldiers[id].weapon.mode, mode);
    idle_for(&mut sim, id, 0.1);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::FIRE_MODE));
    assert_ne!(sim.soldiers[id].weapon.mode, mode, "a real press still works");
}

// ---- Grenades and smoke.

/// Open ground, with a wall from x = -9 to 9 at z = -6 and a soldier just this side of it.
fn grenade_range(walls: bool) -> Sim {
    let mut level = Level::flat(400.0);
    if walls {
        level.solids.push(prs_level::Solid {
            center: Vec3::new(0.0, 1.5, -6.0),
            half: Vec3::new(9.0, 1.5, 0.2),
            rotation: glam::Quat::IDENTITY,
            paint: prs_level::Paint::Concrete,
        });
    }
    Sim::new(level)
}

/// Sets a frag going right there with so long left on it, from nobody who is standing anywhere near.
fn frag_at(sim: &mut Sim, pos: Vec3, id: u32, thrower: SoldierId) {
    sim.thrown.push(prs_throwables::Thrown { id, kind: ThrownKind::Frag, thrower, pos, vel: Vec3::ZERO, fuse: 0.05, at_rest: true });
}

fn place_soldier(sim: &mut Sim, at: Vec3, seed: u32) -> SoldierId {
    let id = sim.spawn_soldier(seed);
    sim.set_home(id, at, 0.0);
    id
}

/// A second soldier well out of the way, to be the one who threw it.
fn thrower_far_off(sim: &mut Sim) -> SoldierId {
    place_soldier(sim, Vec3::new(150.0, 0.1, 150.0), 901)
}

#[test]
fn a_grenade_the_other_side_of_a_wall_does_not_hurt_you() {
    let mut sim = grenade_range(true);
    let me = place_soldier(&mut sim, Vec3::new(0.0, 0.1, -3.5), 1);
    let far = thrower_far_off(&mut sim);
    for _ in 0..30 {
        sim.step(&[]);
    }
    let before = sim.soldiers[me].health.health;
    for id in 0..20 {
        frag_at(&mut sim, Vec3::new(id as f32 * 0.2 - 2.0, 0.15, -8.0), id, far);
        for _ in 0..10 {
            sim.step(&[]);
        }
    }
    assert_eq!(sim.soldiers[me].health.health, before, "came through the wall");
    assert_eq!(sim.soldiers[me].health.bleeding(), false);
}

#[test]
fn the_same_grenade_in_the_open_hurts_and_rattles_you() {
    let mut hit = 0;
    for run in 0..20 {
        let mut sim = grenade_range(false);
        let me = place_soldier(&mut sim, Vec3::new(0.0, 0.1, -3.5), 1);
        let far = thrower_far_off(&mut sim);
        for _ in 0..30 {
            sim.step(&[]);
        }
        frag_at(&mut sim, Vec3::new(0.0, 0.15, -8.0), run + 40, far);
        for _ in 0..10 {
            sim.step(&[]);
        }
        let soldier = &sim.soldiers[me];
        assert!(soldier.suppression.level > 0.1, "should have rattled them: {}", soldier.suppression.level);
        assert_eq!(soldier.suppression.last_shooter, Some(far));
        hit += usize::from(soldier.attacker == Some(far));
    }
    assert_eq!(hit, 20, "hit {hit} times of 20 at 4.5 m in the open");
}

/// The rule that matters: spread out, one grenade is not the end of a squad. Thrown right into the middle.
#[test]
fn one_grenade_does_not_reliably_wipe_a_spread_squad() {
    let (mut wiped, mut out_total, mut hurt_total) = (0, 0, 0);
    let runs = 40;
    for run in 0..runs {
        let mut sim = grenade_range(false);
        let spots = [Vec3::new(-5.0, 0.1, 2.0), Vec3::new(5.0, 0.1, 3.0), Vec3::new(3.0, 0.1, -6.0), Vec3::new(-4.0, 0.1, -5.0)];
        let squad: Vec<SoldierId> = spots.iter().enumerate().map(|(i, at)| place_soldier(&mut sim, *at, i as u32 + 10)).collect();
        let far = thrower_far_off(&mut sim);
        for _ in 0..30 {
            sim.step(&[]);
        }
        frag_at(&mut sim, Vec3::new(run as f32 * 0.05 - 1.0, 0.15, 0.0), run as u32 + 100, far);
        for _ in 0..10 {
            sim.step(&[]);
        }
        let out = squad.iter().filter(|&&id| !sim.soldiers[id].health.is_up()).count();
        hurt_total += squad.iter().filter(|&&id| sim.soldiers[id].attacker == Some(far)).count();
        out_total += out;
        wiped += usize::from(out == squad.len());
    }
    let mean = out_total as f32 / runs as f32;
    println!("grenade into a squad spread 5-7 m out: {mean:.2} of 4 out, {:.2} hit, wiped {wiped}/{runs}", hurt_total as f32 / runs as f32);
    assert_eq!(wiped, 0, "wiped {wiped} of {runs}");
    assert!(mean < 1.5, "{mean} of 4 put out by one grenade");
    assert!(hurt_total > 0, "and it should do something");
}

#[test]
fn up_close_a_grenade_is_as_bad_as_it_sounds() {
    let mut out = 0;
    for run in 0..20 {
        let mut sim = grenade_range(false);
        let me = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
        let far = thrower_far_off(&mut sim);
        for _ in 0..30 {
            sim.step(&[]);
        }
        frag_at(&mut sim, Vec3::new(1.0, 0.15, 0.0), run + 300, far);
        for _ in 0..10 {
            sim.step(&[]);
        }
        out += usize::from(!sim.soldiers[me].health.is_up());
    }
    assert!(out >= 15, "only {out} of 20 at 1 m");
}

fn hold_throw(sim: &mut Sim, id: SoldierId, yaw: f32, pitch: f32, underarm: bool, ticks: u32) {
    drive(sim, id, Vec2::ZERO, yaw, pitch, Buttons::GRENADE);
    for _ in 0..60 {
        drive(sim, id, Vec2::ZERO, yaw, pitch, Buttons::default());
    }
    let mut b = Buttons::FIRE;
    if underarm {
        b = b.union(Buttons::ADS);
    }
    for _ in 0..ticks {
        drive(sim, id, Vec2::ZERO, yaw, pitch, b);
    }
    drive(sim, id, Vec2::ZERO, yaw, pitch, if underarm { Buttons::ADS } else { Buttons::default() });
}

/// Where a throw comes to rest, flying the soldier's own one until it stops.
fn landing(sim: &mut Sim, id: SoldierId, yaw: f32) -> Vec3 {
    let mut last = Vec3::ZERO;
    for _ in 0..240 {
        if let Some(t) = sim.thrown.last() {
            last = t.pos;
        }
        drive(sim, id, Vec2::ZERO, yaw, 0.0, Buttons::default());
    }
    last
}

#[test]
fn a_throw_leaves_the_hand_goes_a_long_way_and_underarm_goes_a_little() {
    let mut sim = grenade_range(false);
    let id = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
    for _ in 0..30 {
        sim.step(&[]);
    }
    assert_eq!(sim.soldiers[id].inventory.stash.frags, 2);
    let start = sim.soldiers[id].body.pos;
    hold_throw(&mut sim, id, 0.0, 0.3, false, 60);
    assert_eq!(sim.soldiers[id].inventory.stash.frags, 1);
    assert_eq!(sim.soldiers[id].inventory.selected, Slot::Primary, "back to the gun");
    let far = landing(&mut sim, id, 0.0).distance(start);
    assert!((15.0..50.0).contains(&far), "overarm went {far} m");
    hold_throw(&mut sim, id, 0.0, 0.0, true, 60);
    let near = landing(&mut sim, id, 0.0).distance(start);
    assert!(near < 12.0 && near > 1.0, "underarm went {near} m");
}

#[test]
fn holding_on_too_long_is_your_own_fault_and_a_quick_flick_does_not_go_far() {
    let mut sim = grenade_range(false);
    let id = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
    for _ in 0..30 {
        sim.step(&[]);
    }
    let fuse = sim.throw_cfg.frag.fuse;
    hold_throw(&mut sim, id, 0.0, 0.2, false, (fuse / TICK_DT) as u32 + 2);
    for _ in 0..4 {
        sim.step(&[]);
    }
    assert!(!sim.soldiers[id].health.is_up() || sim.soldiers[id].health.health < sim.health_cfg.max_health, "it went off in the hand");

    let mut sim = grenade_range(false);
    let id = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
    for _ in 0..30 {
        sim.step(&[]);
    }
    let start = sim.soldiers[id].body.pos;
    hold_throw(&mut sim, id, 0.0, 0.3, false, 2);
    let flick = landing(&mut sim, id, 0.0).distance(start);
    let mut sim = grenade_range(false);
    let id = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
    for _ in 0..30 {
        sim.step(&[]);
    }
    hold_throw(&mut sim, id, 0.0, 0.3, false, 60);
    let full = landing(&mut sim, id, 0.0).distance(start);
    assert!(flick < full * 0.7, "{flick} against {full}");
}

#[test]
fn smoke_goes_off_grows_and_blocks_a_line_of_sight_for_a_while() {
    let mut sim = grenade_range(false);
    let id = place_soldier(&mut sim, Vec3::new(0.0, 0.1, 0.0), 1);
    for _ in 0..30 {
        sim.step(&[]);
    }
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::GRENADE);
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::default());
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::GRENADE);
    assert_eq!(sim.soldiers[id].inventory.selected, Slot::Smoke, "the second press goes round to smoke");
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::default());
    }
    for _ in 0..40 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::FIRE);
    }
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.2, Buttons::default());
    assert_eq!(sim.soldiers[id].inventory.stash.smokes, 1);
    for _ in 0..(10.0 / TICK_DT) as u32 {
        sim.step(&[]);
    }
    assert_eq!(sim.clouds.len(), 1);
    let cloud = sim.clouds[0];
    let (a, b) = (cloud.pos + Vec3::new(-40.0, 0.0, 0.0), cloud.pos + Vec3::new(40.0, 0.0, 0.0));
    assert!(sim.smoke_blocks(a, b));
    assert!(!sim.smoke_blocks(a + Vec3::Y * 30.0, b + Vec3::Y * 30.0));
    for _ in 0..(40.0 / TICK_DT) as u32 {
        sim.step(&[]);
    }
    assert!(sim.clouds.is_empty(), "should be gone by now");
    assert!(!sim.smoke_blocks(a, b));
}

/// The fuse doesn't care where the grenade is. It goes off in the air and keeps falling with its smoke.
#[test]
fn smoke_that_goes_off_in_the_air_falls_to_the_ground_with_its_cloud() {
    let mut sim = grenade_range(false);
    let start = Vec3::new(0.0, 25.0, -20.0);
    let ground = sim.world.walk_ray(start, Vec3::NEG_Y, 100.0).map(|hit| start.y - hit.distance).unwrap();
    sim.thrown.push(Thrown { id: 77, kind: ThrownKind::Smoke, thrower: 0, pos: start, vel: Vec3::ZERO, fuse: 0.3, at_rest: false });
    let (mut bursts, mut first_height, mut last_height) = (0, None, f32::MAX);
    for _ in 0..(8.0 / TICK_DT) as u32 {
        sim.step(&[]);
        bursts += sim.events.iter().filter(|e| matches!(e, SimEvent::ThrownBurst { id: 77, .. })).count();
        if let Some(cloud) = sim.clouds.iter().find(|c| c.id == 77) {
            first_height.get_or_insert(cloud.pos.y);
            assert!(cloud.pos.y <= last_height + 1e-3, "going back up");
            last_height = cloud.pos.y;
        }
    }
    assert_eq!(bursts, 1, "goes off once");
    assert!(first_height.unwrap() > ground + 15.0, "goes off up there: {first_height:?}");
    let cfg = &sim.throw_cfg.smoke;
    assert!((last_height - (ground + cfg.radius * cfg.lift)).abs() < 0.5, "settles on the ground: {last_height} over {ground}");
    assert!(sim.thrown.is_empty(), "and the grenade is done");
}

/// A client hears it has gone off while its own copy is still falling, and both end up in the same place.
#[test]
fn a_client_sees_smoke_fall_the_same_way() {
    let mut server = grenade_range(false);
    let mut client = grenade_range(false);
    let start = Vec3::new(0.0, 25.0, -20.0);
    let thrown = Thrown { id: 78, kind: ThrownKind::Smoke, thrower: 0, pos: start, vel: Vec3::ZERO, fuse: 0.3, at_rest: false };
    server.thrown.push(thrown);
    client.add_thrown(thrown);
    for _ in 0..(8.0 / TICK_DT) as u32 {
        server.step(&[]);
        let heard: Vec<SimEvent> = server.events.clone();
        for event in heard {
            if let SimEvent::ThrownBurst { id, kind, point } = event {
                client.hear_burst(id, kind, point);
            }
        }
        client.fly_thrown();
    }
    let (s, c) = (server.clouds[0], client.clouds[0]);
    assert!(s.pos.distance(c.pos) < 0.05, "{} and {}", s.pos, c.pos);
    assert!(client.thrown.is_empty());
}

/// Both sides work out an id for a throw for themselves, so a thrower's own copy needs no word back.
#[test]
fn a_predicted_throw_agrees_with_the_server_to_the_bit() {
    let mut server = grenade_range(false);
    let id = place_soldier(&mut server, Vec3::new(0.0, 0.1, 0.0), 3);
    let mut client = grenade_range(false);
    place_soldier(&mut client, Vec3::new(0.0, 0.1, 0.0), 3);
    for tick in 0..400u32 {
        let mut buttons = Buttons::default();
        buttons.set(Buttons::GRENADE, tick == 20);
        buttons.set(Buttons::FIRE, (120..170).contains(&tick));
        let cmd = PlayerCmd { tick: tick + 1, yaw: 0.2, pitch: 0.2, buttons, ..Default::default() };
        server.step(&[(id, cmd)]);
        client.step_predicted(id, &cmd);
        client.fly_thrown();
        assert_eq!(server.soldiers[id].throw, client.soldiers[id].throw, "differs at tick {tick}");
        assert_eq!(server.soldiers[id].inventory.stash.frags, client.soldiers[id].inventory.stash.frags);
        if let (Some(a), Some(b)) = (server.thrown.first(), client.thrown.first()) {
            assert_eq!(a, b, "the arc differs at tick {tick}");
        }
    }
    assert_eq!(server.soldiers[id].inventory.stash.frags, 1);
}

// ---- B: a bandage on yourself, anywhere ----

fn bleeding_soldier(sim: &mut Sim) -> SoldierId {
    let id = sim.spawn_soldier(4);
    idle_for(sim, id, 0.5);
    sim.hurt(id, Zone::Abdomen, 20.0);
    assert!(sim.soldiers[id].health.bleeding());
    id
}

#[test]
fn b_bandages_you_on_foot_in_its_normal_time_with_nothing_taken_out() {
    let mut sim = range(100.0);
    let id = bleeding_soldier(&mut sim);
    let (bandages, selected, ammo) = (sim.soldiers[id].inventory.stash.bandages, sim.soldiers[id].inventory.selected, sim.soldiers[id].weapon.ammo);
    let (stow, time) = (sim.weapons[RIFLE].stow_time, sim.health_cfg.bandage_time);
    let mut buttons = press(Buttons::BANDAGE);
    buttons.set(Buttons::FIRE, true);
    let mut ticks = 0;
    while sim.soldiers[id].health.bleeding() {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, buttons);
        assert_eq!(sim.soldiers[id].weapon.ammo, ammo, "no firing with a hand on the bandage");
        ticks += 1;
        assert!(ticks < 900);
    }
    let took = ticks as f32 / TICKS;
    assert!((took - (stow + time)).abs() < 0.2, "took {took}");
    assert_eq!(sim.soldiers[id].inventory.stash.bandages, bandages - 1);
    assert_eq!(sim.soldiers[id].inventory.selected, selected, "the gun was never put away for good");
    idle_for(&mut sim, id, 1.5);
    assert_eq!(sim.soldiers[id].weapon.ready, 1.0, "and it comes back up");
}

#[test]
fn a_bandage_on_yourself_carries_on_at_the_pace_of_aiming_in_any_stance() {
    // How fast this soldier goes in this stance, bandaging or aiming, or the first to finish.
    let walk = |button: Option<Buttons>, held: Buttons, wounded: bool| {
        let mut sim = range(100.0);
        let id = bleeding_soldier(&mut sim);
        if let Some(button) = button {
            drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(button));
            idle_for(&mut sim, id, 2.0);
        }
        let bandages = sim.soldiers[id].inventory.stash.bandages;
        let (mut ticks, mut fastest) = (0, 0.0_f32);
        while ticks < 180 || (wounded && sim.soldiers[id].health.bleeding()) {
            // Shift doesn't hurry a bandage, but it would take the sights down.
            let mut buttons = press(held);
            buttons.set(Buttons::SPRINT, wounded);
            drive(&mut sim, id, Vec2::Y, 0.0, 0.0, buttons);
            fastest = fastest.max(sim.soldiers[id].body.horizontal_speed());
            ticks += 1;
            assert!(ticks < 900, "never finished from {button:?}");
        }
        if wounded {
            assert_eq!(sim.soldiers[id].inventory.stash.bandages, bandages - 1, "from {button:?}");
        }
        fastest
    };
    for button in [None, Some(Buttons::CROUCH), Some(Buttons::PRONE)] {
        let (bandaging, aiming) = (walk(button, Buttons::BANDAGE, true), walk(button, Buttons::ADS, false));
        assert!((bandaging - aiming).abs() < 0.05, "from {button:?}: {bandaging} bandaging, {aiming} aiming");
        assert!(bandaging > 0.1, "from {button:?}: {bandaging}");
    }
}

#[test]
fn nobody_goes_down_while_a_bandage_goes_on_and_it_bleeds_again_if_they_stop() {
    let mut sim = range(100.0);
    let id = bleeding_soldier(&mut sim);
    sim.soldiers[id].health.health = 1.0;
    sim.soldiers[id].health.bleed = 8.0;
    let before = sim.soldiers[id].health.health;
    let mut ticks = 0;
    while sim.soldiers[id].health.bleeding() {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
        assert!(sim.soldiers[id].health.is_up(), "down after {ticks} ticks of a bandage");
        ticks += 1;
        assert!(ticks < 900);
    }
    assert!(sim.soldiers[id].health.health >= before - 0.2, "lost {}", before - sim.soldiers[id].health.health);

    let mut sim = range(100.0);
    let id = bleeding_soldier(&mut sim);
    for _ in 0..(1.5 * TICKS) as usize {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    let held = sim.soldiers[id].health.health;
    idle_for(&mut sim, id, 1.0);
    assert!(sim.soldiers[id].health.bleeding());
    assert!(sim.soldiers[id].health.health < held, "bleeds again once you let go");
}

#[test]
fn a_mod_can_have_you_bleed_through_a_bandage() {
    let mut sim = range(100.0);
    sim.health_cfg.bandage_holds_bleeding = false;
    let id = bleeding_soldier(&mut sim);
    let before = sim.soldiers[id].health.health;
    for _ in 0..TICKS as usize * 2 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    assert!(sim.soldiers[id].health.health < before);
}

#[test]
fn a_mod_can_make_walking_stop_a_bandage() {
    let mut sim = range(100.0);
    sim.health_cfg.bandage_on_the_move = false;
    let id = bleeding_soldier(&mut sim);
    for _ in 0..(2.0 * TICKS) as usize {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    assert!(sim.soldiers[id].treatment.is_some());
    for _ in 0..TICKS as usize {
        drive(&mut sim, id, Vec2::Y, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    assert!(sim.soldiers[id].treatment.is_none(), "walking stops it");
}

#[test]
fn b_stops_for_letting_go_and_being_hit() {
    let mut sim = range(100.0);
    let id = bleeding_soldier(&mut sim);
    let hold = |sim: &mut Sim, wish: Vec2, seconds: f32| {
        for _ in 0..(seconds * TICKS) as usize {
            drive(sim, id, wish, 0.0, 0.0, press(Buttons::BANDAGE));
        }
    };
    hold(&mut sim, Vec2::ZERO, 2.0);
    assert!(sim.soldiers[id].treatment.is_some(), "under way");
    idle_for(&mut sim, id, 0.1);
    assert!(sim.soldiers[id].treatment.is_none(), "letting go stops it");

    hold(&mut sim, Vec2::ZERO, 2.0);
    assert!(sim.soldiers[id].treatment.is_some());
    sim.hurt(id, Zone::Arm, 5.0);
    assert!(sim.soldiers[id].treatment.is_none(), "being hit stops it");
    assert!(sim.soldiers[id].health.bleeding());
    assert_eq!(sim.soldiers[id].inventory.stash.bandages, 2, "and none of that used one up");
}

#[test]
fn b_with_nothing_to_bandage_or_nothing_to_do_it_with_does_nothing() {
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(4);
    idle_for(&mut sim, id, 0.5);
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    assert_eq!(sim.soldiers[id].weapon.ready, 1.0, "not bleeding: the gun stays up");
    assert!(sim.soldiers[id].treatment.is_none());

    sim.hurt(id, Zone::Leg, 20.0);
    sim.soldiers[id].inventory.stash.bandages = 0;
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, press(Buttons::BANDAGE));
    }
    assert_eq!(sim.soldiers[id].weapon.ready, 1.0, "none left: the gun stays up");
    assert!(sim.soldiers[id].treatment.is_none());
}

/// Someone standing still and hurt, with a bandage to use. The tests of what treatment does to a body.
fn hurt_on_the_spot(zone: prs_health::Zone) -> (Sim, SoldierId) {
    let mut sim = range(100.0);
    let id = sim.spawn_soldier(4);
    for _ in 0..60 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    }
    sim.hurt(id, zone, 15.0);
    (sim, id)
}

fn bandage_held() -> Buttons {
    let mut buttons = Buttons::default();
    buttons.set(Buttons::BANDAGE, true);
    buttons
}

#[test]
fn the_hands_follow_a_treatment_from_start_to_finish() {
    let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Arm);
    let mut ticks = 0;
    while sim.soldiers[id].treatment.is_none() || sim.soldiers[id].weapon.ready > 0.0 {
        assert!(sim.soldiers[id].anim.care == 0.0, "no hands off the gun until the gun is out of the way");
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, bandage_held());
        ticks += 1;
        assert!(ticks < 120, "treatment never started");
    }
    assert_eq!(sim.soldiers[id].weapon.ready, 0.0, "the gun came down first");
    let total = sim.soldiers[id].treatment.unwrap().total;
    assert_eq!(sim.soldiers[id].anim.care_site, prs_body::CareSite::Arm);

    let mut ticks_treating = 0;
    let mut last = 0.0;
    while sim.soldiers[id].treatment.is_some() {
        let t = sim.soldiers[id].treatment.unwrap();
        let anim = sim.soldiers[id].anim;
        assert!((anim.care_progress - (1.0 - t.left / t.total)).abs() < 1e-5, "the motion is the treatment's own progress");
        assert!(anim.care_progress >= last);
        last = anim.care_progress;
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, bandage_held());
        ticks_treating += 1;
    }
    assert!((ticks_treating as f32 * TICK_DT - total).abs() < 0.05, "took {ticks_treating} ticks for {total} s");
    assert!(sim.soldiers[id].anim.care > 0.9, "the hands were on it right up to the end");
    assert!(sim.soldiers[id].anim.slung > 0.9, "and the gun out of the way");
    assert!(sim.soldiers[id].dressed.arm && !sim.soldiers[id].dressed.leg);

    let reach = (sim.body_cfg.care.return_time / TICK_DT) as usize + 2;
    for _ in 0..reach.max((sim.body_cfg.look.sling_time / TICK_DT) as usize + 2) {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    }
    let anim = sim.soldiers[id].anim;
    assert_eq!((anim.care, anim.slung), (0.0, 0.0), "back on the gun afterwards");
}

#[test]
fn letting_go_or_being_hit_takes_the_hands_back_to_the_gun() {
    for hit in [false, true] {
        let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Leg);
        for _ in 0..120 {
            drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, bandage_held());
        }
        assert!(sim.soldiers[id].treatment.is_some() && sim.soldiers[id].anim.care == 1.0);
        let was = sim.soldiers[id].anim.care_progress;
        if hit {
            sim.hurt(id, prs_health::Zone::Arm, 5.0);
        }
        let held = Buttons::default();
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, held);
        assert!(sim.soldiers[id].treatment.is_none());
        let anim = sim.soldiers[id].anim;
        assert!(anim.care > 0.5 && anim.care < 1.0, "the hands ease off rather than snap: {}", anim.care);
        assert_eq!(anim.care_progress, was, "and hold what they were doing on the way");
        for _ in 0..60 {
            drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, held);
        }
        let anim = sim.soldiers[id].anim;
        assert_eq!((anim.care, anim.slung), (0.0, 0.0));
        assert!(!sim.soldiers[id].dressed.leg, "nothing stays on from a treatment that did not finish");
    }
}

#[test]
fn a_dressing_stays_on_the_limb_until_respawn() {
    let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Leg);
    for _ in 0..600 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, bandage_held());
    }
    assert!(sim.soldiers[id].dressed.leg, "still on");
    assert_eq!(sim.soldiers[id].health.bleed, 0.0);
    sim.soldiers[id].health.leg_wound = 0.0;
    drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, Buttons::default());
    assert!(!sim.soldiers[id].dressed.leg, "a wound that has gone needs no dressing");
}

#[test]
fn treatment_animation_is_the_same_for_everyone_who_runs_the_same_sim() {
    let run = || {
        let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Arm);
        for tick in 0..200 {
            let held = if tick < 150 { bandage_held() } else { Buttons::default() };
            drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, held);
        }
        (sim.soldiers[id].anim, sim.body_pose(&sim.body_input(id)))
    };
    assert_eq!(run(), run());
}

#[test]
fn looks_are_all_that_change_when_a_body_goes_on_a_treatment() {
    // The eye and everything aimed from it stay where the sim had them.
    let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Arm);
    let eye = sim.soldiers[id].eye();
    for _ in 0..120 {
        drive(&mut sim, id, Vec2::ZERO, 0.0, 0.0, bandage_held());
    }
    assert!(sim.soldiers[id].anim.care > 0.9);
    assert!(sim.soldiers[id].eye().distance(eye) < 1e-3, "the eye never moves for it");
}

#[test]
fn bandaging_on_the_move_the_hands_keep_time_with_the_treatment_and_the_legs_keep_walking() {
    let (mut sim, id) = hurt_on_the_spot(prs_health::Zone::Arm);
    let mut seen = 0;
    while sim.soldiers[id].health.bleeding() {
        drive(&mut sim, id, Vec2::Y, 0.0, 0.0, bandage_held());
        if let Some(t) = sim.soldiers[id].treatment {
            let anim = sim.soldiers[id].anim;
            assert!((anim.care_progress - (1.0 - t.left / t.total)).abs() < 1e-5);
            if t.left < t.total * 0.5 {
                assert_eq!(anim.care, 1.0, "the hands are on it");
                assert!(anim.moving > 0.9, "and the legs are walking");
                assert!(sim.soldiers[id].body.horizontal_speed() > 0.5);
            }
            seen += 1;
        }
        assert!(seen < 900, "never finished");
    }
    assert!(seen > 100, "a treatment should have run: {seen}");
    assert!(sim.soldiers[id].dressed.arm);
}
