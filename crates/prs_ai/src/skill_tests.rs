//! How dangerous a bot is, measured: one soldier shooting at another who stands, crouches or walks across
//! in the open at 25, 75 and 150 m, for a while, with the target never going down. Every miss is the gun
//! pointing somewhere else, because that is all a bot can do: it holds the same gun as anyone.
//! `cargo test -p prs_ai skill_report -- --ignored --nocapture` prints the table.

use super::*;
use prs_sim::health::Health;
use prs_sim::protocol::{Buttons, TICK_DT};
use prs_sim::{Level, SimEvent};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Standing,
    Crouched,
    Walking,
}

/// What came of one stretch of shooting.
#[derive(Clone, Copy, Debug, Default)]
struct Outcome {
    shots: u32,
    hits: u32,
    /// Seconds to the first hit, if there was one.
    first_hit: Option<f32>,
}

impl Outcome {
    fn hit_rate(&self) -> f32 {
        self.hits as f32 / self.shots.max(1) as f32
    }

    /// Hits a minute: how fast it hurts, whatever it takes in rounds.
    fn hits_per_minute(&self, seconds: f32) -> f32 {
        self.hits as f32 * 60.0 / seconds
    }
}

/// Seconds of each trial, and trials of each case, each from a slightly different place.
const SECONDS: f32 = 60.0;
const TRIALS: u32 = 8;
/// Seconds a walking target goes one way before turning back.
const TURN_EVERY: f32 = 4.0;

fn trial(skill: &SkillConfig, range: f32, target: Target, trial: u32) -> Outcome {
    let mut sim = Sim::new(Level::flat(1600.0));
    let loadout = sim.inventory_cfg.loadout;
    let shift = trial as f32 * 7.0;
    let aim = sim.spawn_soldier_with(70 + trial, 1, loadout);
    sim.set_home(aim, Vec3::new(shift, 0.1, -range), 0.0);
    let shooter = sim.spawn_soldier_with(90 + trial, 0, loadout);
    sim.set_home(shooter, Vec3::new(shift, 0.1, 0.0), 0.0);
    let cfg = FightConfig { skill: skill.clone(), ..Default::default() };
    let mut platoon = Platoon::new(&sim, cfg, WalkerConfig { seed: 0x5C11 + trial, ..Default::default() });
    platoon.enlist(&sim, shooter, 0, 0, true, 0);
    let areas = vec![WalkArea { center: Vec2::new(shift, 0.0), radius: 5.0, doors: Vec::new() }];
    let mut out = Outcome::default();
    for tick in 0..(SECONDS / TICK_DT) as u32 {
        let t = tick as f32 * TICK_DT;
        let mut buttons = Buttons::default();
        buttons.set(Buttons::CROUCH, target == Target::Crouched && tick == 1);
        // Across the line of fire, at a walk, back and forth.
        let across = if (t / TURN_EVERY) as u32 % 2 == 0 { 1.0 } else { -1.0 };
        let wish = if target == Target::Walking { Vec2::new(across * 0.5, 0.0) } else { Vec2::ZERO };
        let target_cmd = PlayerCmd { tick: sim.tick, wish_move: wish, yaw: 0.0, buttons, ..Default::default() };
        let mut cmds = platoon.think(&sim, &areas, None).to_vec();
        cmds.push((aim, target_cmd));
        sim.step(&cmds);
        sim.soldiers[shooter].weapon.magazines = vec![30, 30];
        sim.soldiers[aim].health = Health::new(&sim.health_cfg);
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { soldier, .. } if soldier == shooter => out.shots += 1,
                SimEvent::SoldierHit { soldier, .. } if soldier == aim => {
                    out.hits += 1;
                    out.first_hit.get_or_insert(t);
                }
                _ => {}
            }
        }
    }
    out
}

/// All the trials of one case, added up. The time to first hit is the mean of those that hit, and a trial
/// with no hit counts as the whole length.
fn case(skill: &SkillConfig, range: f32, target: Target) -> (Outcome, f32) {
    let outcomes: Vec<Outcome> = (0..TRIALS).map(|n| trial(skill, range, target, n)).collect();
    let total = outcomes.iter().fold(Outcome::default(), |a, b| Outcome { shots: a.shots + b.shots, hits: a.hits + b.hits, first_hit: None });
    let first = outcomes.iter().map(|o| o.first_hit.unwrap_or(SECONDS)).sum::<f32>() / TRIALS as f32;
    (total, first)
}

const RANGES: [f32; 3] = [25.0, 75.0, 150.0];
const TARGETS: [Target; 3] = [Target::Standing, Target::Crouched, Target::Walking];

/// Hits a minute, against a standing target in the open, at a range.
fn lethality(skill: &SkillConfig, range: f32) -> f32 {
    case(skill, range, Target::Standing).0.hits_per_minute(SECONDS * TRIALS as f32)
}

#[test]
fn the_usual_bot_hurts_about_half_as_fast_as_a_regular_at_mid_range_and_is_still_a_threat_close_in() {
    let usual = SkillConfig::default();
    let regular = SkillConfig::regular();
    let (mid_usual, mid_regular) = (lethality(&usual, 75.0), lethality(&regular, 75.0));
    assert!(mid_usual <= mid_regular * 0.65, "{mid_usual:.1} hits a minute at 75 m against a regular's {mid_regular:.1}");
    assert!(mid_usual > 0.0, "it should still land some at 75 m");
    let (close, first) = case(&usual, 25.0, Target::Standing);
    assert!(close.hits > 0, "nothing landed at 25 m");
    assert!(first < 15.0, "{first:.1} s to the first hit at 25 m is no threat");
}

#[test]
#[ignore]
fn skill_report() {
    for (name, skill) in [("usual", SkillConfig::default()), ("recruit", SkillConfig::recruit()), ("regular", SkillConfig::regular()), ("veteran", SkillConfig::veteran())] {
        println!("{name}");
        for range in RANGES {
            for target in TARGETS {
                let (out, first) = case(&skill, range, target);
                println!(
                    "  {range:>4.0} m {target:<9?} shots {:>4}  hits {:>3}  rate {:>5.1}%  hits/min {:>5.1}  first hit {first:>5.1} s",
                    out.shots,
                    out.hits,
                    out.hit_rate() * 100.0,
                    out.hits_per_minute(SECONDS * TRIALS as f32),
                );
            }
        }
    }
}

/// The harder bots are a mod each, for a server to choose. Read as a server would, they come out as the
/// skills they say they are.
#[test]
fn the_harder_bots_are_mods_that_come_out_as_the_named_skills() {
    for (name, text, skill) in [
        ("bots-normal", include_str!("../../../mods/bots-normal.toml"), SkillConfig::regular()),
        ("bots-hard", include_str!("../../../mods/bots-hard.toml"), SkillConfig::veteran()),
    ] {
        let read = prs_mods::Mod::parse(name, text).unwrap_or_else(|e| panic!("{e}"));
        let troops = read.over("ai.troops", TroopsConfig::default()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(troops.fight.skill, skill, "{name}");
    }
}
