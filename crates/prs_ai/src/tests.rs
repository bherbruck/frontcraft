//! Long runs on the real maps. What is held to here is that nobody ends up standing with their nose in
//! a wall: they keep getting somewhere, stay where they were put, and different seeds go different ways.

use super::*;
use prs_sim::Level;
use prs_sim::protocol::TICK_DT;

fn crowd(map: &str, cfg: WalkerConfig) -> (Sim, Crowd) {
    let mut sim = Sim::new(Level::named(map).expect("a map by that name"));
    let crowd = Crowd::populate(&mut sim, cfg);
    (sim, crowd)
}

fn step(sim: &mut Sim, crowd: &mut Crowd) {
    let cmds = crowd.think(sim).to_vec();
    sim.step(&cmds);
}

/// Seconds a walker has spent meaning to get somewhere without getting a metre from where it was.
#[derive(Clone, Copy)]
struct Progress {
    anchor: Vec2,
    stalled: f32,
    worst: f32,
}

const METRE: f32 = 1.0;

fn walks_and_stays(map: &str, seed: u32) {
    let cfg = WalkerConfig { seed, ..Default::default() };
    // Once it notices, it strikes out, and a tight spot can take a few goes.
    let limit = cfg.stuck_time * 3.0 + cfg.unstick_time * 3.0;
    let (mut sim, mut crowd) = crowd(map, cfg);
    assert!(crowd.walkers.iter().all(|w| w.rays_spent == 0));
    let mut progress: Vec<Progress> = crowd
        .walkers
        .iter()
        .map(|w| Progress { anchor: Vec2::new(sim.soldiers[w.soldier].body.pos.x, sim.soldiers[w.soldier].body.pos.z), stalled: 0.0, worst: 0.0 })
        .collect();
    let half = sim.level.terrain.size * 0.5;
    let mut travelled = vec![0.0; crowd.walkers.len()];
    let seconds = 240.0;
    for _ in 0..(seconds / TICK_DT) as u32 {
        step(&mut sim, &mut crowd);
        for (i, walker) in crowd.walkers.iter().enumerate() {
            let body = &sim.soldiers[walker.soldier].body;
            let at = Vec2::new(body.pos.x, body.pos.z);
            assert!(at.abs().max_element() < half, "{map}: walker {i} left the map at {at}");
            let area = &crowd.areas[walker.area];
            assert!(at.distance(area.center) < area.radius + crowd.cfg.goal_range.y, "{map}: walker {i} wandered off to {at}");
            let p = &mut progress[i];
            if at.distance(p.anchor) > METRE {
                travelled[i] += at.distance(p.anchor);
                p.anchor = at;
                p.stalled = 0.0;
            } else if walker.mood != Mood::Stop {
                p.stalled += TICK_DT;
                p.worst = p.worst.max(p.stalled);
            }
        }
    }
    let rays: u32 = crowd.walkers.iter().map(|w| w.rays_spent).sum();
    let per_tick = rays as f32 * TICK_DT / seconds / crowd.walkers.len() as f32;
    assert!(per_tick < 0.5, "{map}: {per_tick} rays a tick per walker");
    for (i, p) in progress.iter().enumerate() {
        assert!(p.worst < limit, "{map}: walker {i} stuck for {:.1} s near {} (limit {limit:.1})", p.worst, p.anchor);
        assert!(travelled[i] > 50.0, "{map}: walker {i} only got {:.0} m in {seconds} s", travelled[i]);
    }
}

#[test]
fn on_the_town_they_keep_moving_and_stay_put() {
    for seed in [WalkerConfig::default().seed, 1, 2] {
        walks_and_stays("town", seed);
    }
}

#[test]
fn on_the_range_they_keep_moving_and_stay_put() {
    for seed in [WalkerConfig::default().seed, 1, 2] {
        walks_and_stays("range", seed);
    }
}

/// Doors are a thing they use, and they come back out again.
#[test]
fn some_of_them_go_indoors_and_come_out() {
    let (mut sim, mut crowd) = crowd("town", WalkerConfig::default());
    let mut was_in = vec![false; crowd.walkers.len()];
    let (mut entered, mut left) = (0, 0);
    for tick in 0..18000 {
        step(&mut sim, &mut crowd);
        if tick % 15 != 0 {
            continue;
        }
        for (i, walker) in crowd.walkers.iter().enumerate() {
            let chest = sim.soldiers[walker.soldier].body.pos + Vec3::Y;
            let inside = sim.level.rooms.iter().any(|room| room.contains(chest));
            entered += usize::from(inside && !was_in[i]);
            left += usize::from(was_in[i] && !inside);
            was_in[i] = inside;
        }
    }
    assert!(entered >= 3, "went indoors {entered} times in 5 minutes");
    assert!(left >= 2, "came back out {left} times");
}

#[test]
fn the_same_seed_walks_the_same_way() {
    let cfg = WalkerConfig { count: 4, ..Default::default() };
    let (mut a, mut one) = crowd("range", cfg.clone());
    let (mut b, mut two) = crowd("range", cfg);
    for _ in 0..1200 {
        step(&mut a, &mut one);
        step(&mut b, &mut two);
    }
    assert_eq!(a.export_state(), b.export_state());
    assert_eq!(one, two);
}

#[test]
fn different_seeds_spread_out() {
    let cfg = WalkerConfig::default();
    let (mut sim, mut crowd) = crowd("town", cfg);
    for _ in 0..3600 {
        step(&mut sim, &mut crowd);
    }
    let feet: Vec<Vec3> = crowd.walkers.iter().map(|w| sim.soldiers[w.soldier].body.pos).collect();
    for (i, a) in feet.iter().enumerate() {
        for b in &feet[i + 1..] {
            assert!(a.distance(*b) > 0.3, "two walkers on one spot at {a}");
        }
    }
    let middle = crowd.areas[0].center;
    let town = crowd.walkers.iter().filter(|w| w.area == 0).map(|w| sim.soldiers[w.soldier].body.pos);
    let spread: f32 = town.clone().map(|p| Vec2::new(p.x, p.z).distance(middle)).sum::<f32>() / town.count() as f32;
    assert!(spread > 20.0, "all bunched up, {spread} m out on average");
}

#[test]
fn both_teams_get_some_and_some_are_near_the_spawn() {
    let (sim, crowd) = crowd("town", WalkerConfig::default());
    let on = |team| crowd.walkers.iter().filter(|w| sim.soldiers[w.soldier].team == team).count();
    assert_eq!((on(0), on(1)), (6, 6));
    assert_eq!(crowd.walkers.iter().filter(|w| w.area == 1).count(), 3);
    for w in &crowd.walkers {
        let capsule = sim.move_cfg.capsule(prs_sim::movement::Stance::Stand);
        assert!(sim.world.capsule_is_clear(sim.soldiers[w.soldier].body.pos, capsule), "started inside something");
    }
}

#[test]
fn someone_killed_gets_going_again_once_back() {
    let cfg = WalkerConfig { count: 2, ..Default::default() };
    let (mut sim, mut crowd) = crowd("range", cfg);
    let id = crowd.walkers[0].soldier;
    for _ in 0..200 {
        step(&mut sim, &mut crowd);
    }
    sim.hurt(id, prs_sim::health::Zone::Head, 500.0);
    let respawn = sim.health_cfg.respawn_time;
    for _ in 0..((respawn + 2.0) / TICK_DT) as u32 {
        step(&mut sim, &mut crowd);
    }
    let lives = sim.soldiers[id].lives;
    assert_eq!(lives, 1);
    let from = sim.soldiers[id].body.pos;
    for _ in 0..1800 {
        step(&mut sim, &mut crowd);
    }
    assert!(sim.soldiers[id].body.pos.distance(from) > 2.0, "never set off again");
}

/// `cargo test -p prs_ai crowd_report -- --ignored --nocapture`
#[test]
#[ignore]
fn crowd_report() {
    for map in ["town", "range"] {
        for count in [12, 32] {
            let (mut sim, mut crowd) = crowd(map, WalkerConfig { count, ..Default::default() });
            for _ in 0..600 {
                step(&mut sim, &mut crowd);
            }
            let rays_before: u32 = crowd.walkers.iter().map(|w| w.rays_spent).sum();
            let ticks = 3600;
            let (mut thinking, mut stepping) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
            for _ in 0..ticks {
                let t = std::time::Instant::now();
                let cmds = crowd.think(&sim).to_vec();
                thinking += t.elapsed();
                let t = std::time::Instant::now();
                sim.step(&cmds);
                stepping += t.elapsed();
            }
            let rays: u32 = crowd.walkers.iter().map(|w| w.rays_spent).sum::<u32>() - rays_before;
            let t = std::time::Instant::now();
            for id in 0..sim.soldiers.len() {
                std::hint::black_box(sim.body_pose(&sim.body_input(id)));
            }
            let bones = t.elapsed();
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0 / ticks as f64;
            println!(
                "{map} {count} walkers: think {:.3} ms/tick, sim step {:.3} ms/tick, {:.1} rays/tick, all bones {:.3} ms/frame",
                ms(thinking),
                ms(stepping),
                rays as f32 / ticks as f32,
                bones.as_secs_f64() * 1000.0,
            );
        }
    }
}

mod in_a_match {
    use crate::{Troops, TroopsConfig};
    use prs_mode::{Match, Mode, ModeEvent, Phase};
    use prs_sim::protocol::TICK_DT;
    use prs_sim::{Level, Sim, SoldierId};

    fn run(map: &str, seconds: f32) -> (Sim, Match, Vec<ModeEvent>) {
        let mut sim = Sim::new(Level::named(map).unwrap());
        let mut game = Match::new(Mode::advance_and_secure(), &mut sim);
        game.fill_bots(&mut sim);
        let mut troops = Troops::new(&sim, TroopsConfig::default());
        let mut seen = Vec::new();
        let mut cmds: Vec<(SoldierId, _)> = Vec::new();
        for _ in 0..(seconds / TICK_DT) as u32 {
            cmds.clear();
            cmds.extend_from_slice(troops.think(&sim, &game));
            sim.step(&cmds);
            game.step(&mut sim, &[]);
            seen.extend(game.events.iter().copied());
        }
        (sim, game, seen)
    }

    #[test]
    fn bots_on_both_sides_head_for_their_objective_and_take_it() {
        let (_, game, events) = run("town", 480.0);
        let taken = |team: u8| events.iter().any(|e| matches!(e, ModeEvent::PointCaptured { team: t, .. } if *t == team));
        assert!(taken(0), "south never took anything");
        assert!(taken(1), "north never took anything");
        assert_eq!(game.state.phase, Phase::Playing);
    }

    #[test]
    fn the_same_seeds_give_the_same_match() {
        let (sim_a, a, _) = run("range", 60.0);
        let (sim_b, b, _) = run("range", 60.0);
        assert_eq!(a.state, b.state);
        assert_eq!(sim_a.export_state(), sim_b.export_state());
    }
}

mod around_bases {
    use crate::{Troops, TroopsConfig};
    use glam::Vec3;
    use prs_mode::{Match, Mode};
    use prs_sim::protocol::TICK_DT;
    use prs_sim::{Level, Sim, SoldierId};

    #[test]
    fn a_bot_put_inside_the_enemy_base_gets_out_before_the_clock_runs_out() {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let mut game = Match::new(Mode::advance_and_secure(), &mut sim);
        game.fill_bots(&mut sim);
        let mut troops = Troops::new(&sim, TroopsConfig::default());
        let mut cmds: Vec<(SoldierId, _)> = Vec::new();
        // This is about finding the way out, so whoever holds the base holds their fire: whether one of them happens to
        // look its way is nothing to do with it.
        let defenders: Vec<SoldierId> = game.state.players.iter().filter(|p| p.team == 1).map(|p| p.soldier).collect();
        let mut step = |sim: &mut Sim, game: &mut Match| {
            cmds.clear();
            cmds.extend_from_slice(troops.think(sim, game));
            for (who, cmd) in &mut cmds {
                if defenders.contains(who) {
                    cmd.buttons.set(prs_sim::protocol::Buttons::FIRE, false);
                    cmd.buttons.set(prs_sim::protocol::Buttons::GRENADE, false);
                }
            }
            sim.step(&cmds);
            game.step(sim, &[]);
        };
        for _ in 0..30 {
            step(&mut sim, &mut game);
        }
        let bot = game.state.players.iter().find(|p| p.team == 0).unwrap().soldier;
        let base = &sim.level.teams[1];
        // Most of the way in, on open ground that way out.
        let (center, radius) = (base.base_center(), base.base_radius);
        let top = center + Vec3::new(radius * 0.5, 30.0, 0.0);
        let hit = sim.world.raycast(top, Vec3::NEG_Y, 80.0).unwrap();
        sim.soldiers[bot].body.pos = top + Vec3::NEG_Y * (hit.distance - 0.1);
        let warning = game.mode.cfg.base_protection.as_ref().unwrap().warning_seconds;
        let mut warned = false;
        let mut out_in_time = false;
        // Once out, anyone can shoot them like anyone else, so all this looks at is the clock up to then.
        for _ in 0..(warning / TICK_DT) as u32 {
            step(&mut sim, &mut game);
            let trespassing = game.state.player(bot).unwrap().trespass_since.is_some();
            warned |= trespassing;
            out_in_time |= warned && !trespassing && sim.soldiers[bot].health.is_up();
        }
        assert!(warned, "should have been in there");
        assert!(out_in_time, "should have got out before the clock ran out");
    }
}
