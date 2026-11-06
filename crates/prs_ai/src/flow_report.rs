//! What the ways across a map cost a server: memory and the time to work them out. Run with
//! `PRS_FLOW_MAPS=town,claudabad,town@4096 cargo test -p prs_ai --release flow_report -- --ignored --nocapture`.

use crate::{Troops, tuned_troops};
use prs_sim::{Level, Sim};
use std::time::Instant;

#[test]
#[ignore = "a measurement, not a check"]
fn flow_report() {
    let maps = std::env::var("PRS_FLOW_MAPS").unwrap_or_else(|_| "town,claudabad,town@4096".into());
    for map in maps.split(',') {
        // `town@4096` is the town with ground out to that size all round, for a map bigger than any yet.
        let (name, stretch) = map.split_once('@').map_or((map, None), |(name, size)| (name, size.parse::<f32>().ok()));
        let Some(mut level) = Level::named(name) else {
            println!("{map}: no such map");
            continue;
        };
        if let Some(size) = stretch {
            level.terrain.size = size;
        }
        let size = level.terrain.size;
        let sim = Sim::new(level);
        let began = Instant::now();
        let troops = Troops::new(&sim, tuned_troops());
        let took = began.elapsed().as_secs_f64();
        let (walk, road) = (troops.flow.bytes(), troops.road.bytes());
        println!(
            "{map}: {size:.0} m, {} + {} fields, {:.1} MB walking + {:.1} MB driving, built in {took:.2} s",
            troops.flow.fields.len(),
            troops.road.fields.len(),
            walk as f64 / 1e6,
            road as f64 / 1e6,
        );
    }
}
