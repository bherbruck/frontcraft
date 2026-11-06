//! Where bots fight each other on the town, and how fast it kills: each shot and hit by range, and the time
//! from the first shot at someone to them going down. The new sight against something like the old one, with
//! everything else the same. `cargo test -p prs_ai range_report -- --ignored --nocapture`.

use super::*;
use prs_mode::{Match, Mode};
use prs_sim::health::HealthEvent;
use prs_sim::protocol::TICK_DT;
use prs_sim::{Level, SimEvent};

const BUCKET: f32 = 25.0;
const BUCKETS: usize = 12;
/// Further apart than this, a first shot and a going down are not the same fight.
const ENGAGEMENT: f32 = 30.0;

#[derive(Default)]
struct Ranges {
    shots: [u32; BUCKETS],
    hits: [u32; BUCKETS],
    /// Seconds from the first shot at someone to them going down, for each that did.
    to_down: Vec<f32>,
    /// Engagements that ended without them going down.
    survived: u32,
    deaths: u32,
}

fn bucket(range: f32) -> usize {
    ((range / BUCKET) as usize).min(BUCKETS - 1)
}

/// Close to the sight there was before: anything showing is seen, by the same chance at range as before,
/// through leaves and shade, and to any distance in view.
fn old_sight() -> TroopsConfig {
    let mut cfg = TroopsConfig::default();
    let skill = &mut cfg.fight.skill;
    skill.notice_rate = 1.0 / (cfg.fight.think_ticks as f32 * TICK_DT);
    skill.visible_power = 0.0;
    skill.track_share = 0.0;
    skill.quiet_range = f32::MAX;
    skill.still_hidden_beyond = f32::MAX;
    skill.view_range = 420.0;
    cfg.fight.sight.leaf_density = 0.0;
    cfg.fight.sight.room_shade = 1.0;
    cfg
}

fn play(cfg: TroopsConfig, seconds: f32) -> Ranges {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut mode = Mode::advance_and_secure();
    mode.cfg.bots_per_team = Some(16);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, cfg);
    let mut out = Ranges::default();
    // Who is being shot at, since the first shot of this fight, by tick.
    let mut first_shot: Vec<Option<u32>> = Vec::new();
    let mut cmds = Vec::new();
    for tick in 0..(seconds / TICK_DT) as u32 {
        cmds.clear();
        cmds.extend_from_slice(troops.think(&sim, &game));
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        first_shot.resize(sim.soldiers.len(), None);
        for slot in first_shot.iter_mut() {
            if slot.is_some_and(|at| (tick - at) as f32 * TICK_DT > ENGAGEMENT) {
                *slot = None;
                out.survived += 1;
            }
        }
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { soldier, origin, .. } => {
                    let target = troops.platoon.fighters.iter().find(|f| f.soldier == soldier).and_then(|f| f.contact);
                    if let Some(contact) = target {
                        out.shots[bucket(origin.distance(contact.at))] += 1;
                        first_shot[contact.soldier].get_or_insert(tick);
                    }
                }
                SimEvent::SoldierHit { soldier, shooter, .. } => {
                    out.hits[bucket(sim.soldiers[shooter].eye().distance(sim.soldiers[soldier].eye()))] += 1;
                }
                SimEvent::Health { soldier, event: HealthEvent::Downed | HealthEvent::Died } => {
                    if let Some(at) = first_shot.get_mut(soldier).and_then(Option::take) {
                        out.to_down.push((tick - at) as f32 * TICK_DT);
                    }
                    if matches!(event, SimEvent::Health { event: HealthEvent::Died, .. }) {
                        out.deaths += 1;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn print(name: &str, r: &Ranges, seconds: f32) {
    let shots: u32 = r.shots.iter().sum();
    let hits: u32 = r.hits.iter().sum();
    println!("{name}: {shots} shots, {hits} hits, {} deaths in {seconds:.0} s", r.deaths);
    for b in 0..BUCKETS {
        if r.shots[b] + r.hits[b] == 0 {
            continue;
        }
        let from = b as f32 * BUCKET;
        let to = if b == BUCKETS - 1 { "+".to_string() } else { format!("{:.0}", from + BUCKET) };
        let rate = r.hits[b] as f32 / r.shots[b].max(1) as f32 * 100.0;
        println!("  {from:>4.0}-{to:<4} m  shots {:>5} ({:>4.1}%)  hits {:>4} ({:>4.1}%)  hit rate {rate:>5.1}%", r.shots[b], r.shots[b] as f32 * 100.0 / shots.max(1) as f32, r.hits[b], r.hits[b] as f32 * 100.0 / hits.max(1) as f32);
    }
    let mut times = r.to_down.clone();
    times.sort_by(f32::total_cmp);
    let at = |q: f32| times.get(((times.len() as f32 - 1.0) * q).round() as usize).copied().unwrap_or(f32::NAN);
    println!(
        "  first shot to down: {} fights went down, {} did not; median {:.1} s, quarter {:.1} s, three quarters {:.1} s",
        times.len(),
        r.survived,
        at(0.5),
        at(0.25),
        at(0.75)
    );
}

#[test]
#[ignore]
fn range_report() {
    let seconds = 900.0;
    print("old-like sight", &play(old_sight(), seconds), seconds);
    print("new sight", &play(TroopsConfig::default(), seconds), seconds);
}
