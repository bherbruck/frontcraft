//! Every vehicle on the maps of real places, as a game starts: settled, upright, touching nothing but the
//! ground, and able to drive out along the open ground in front of it.

use glam::{Vec2, Vec3};
use prs_sim::protocol::{PlayerCmd, TICK_DT};
use prs_sim::seats::SeatedAt;
use prs_sim::vehicles::VehicleEvent;
use prs_sim::{Level, Sim, SimEvent};

const MAPS: &[&str] = &["muttrah", "fallujah"];

fn settled(name: &str) -> Sim {
    let mut sim = Sim::new(Level::named(name).unwrap());
    for _ in 0..(3.0 / TICK_DT) as u32 {
        sim.step(&[]);
    }
    sim
}

#[test]
fn each_side_has_the_same_vehicles() {
    for name in MAPS {
        let sim = settled(name);
        let level = &sim.level;
        let bases: Vec<Vec3> = level.teams.iter().map(|t| t.base_center()).collect();
        let mut kinds = [Vec::new(), Vec::new()];
        for v in &level.vehicles {
            let nearer = if v.pos.distance(bases[0]) < v.pos.distance(bases[1]) { 0 } else { 1 };
            kinds[nearer].push(v.kind);
        }
        kinds.iter_mut().for_each(|k| k.sort());
        assert_eq!(kinds[0], kinds[1], "{name}");
        assert!(kinds[0].len() >= 4, "{name}: {}", kinds[0].len());
    }
}

#[test]
fn every_vehicle_settles_upright_touching_nothing_but_the_ground() {
    for name in MAPS {
        let mut sim = settled(name);
        let hulls: Vec<Vec<(Vec3, Vec3, glam::Quat)>> = sim
            .vehicles
            .iter()
            .map(|v| sim.vehicle_cfgs[v.kind].hull.iter().map(|hull| (v.pos + v.rot * hull.center, hull.half, v.rot)).collect())
            .collect();
        for (v, vehicle) in sim.vehicles.iter().enumerate() {
            let start = sim.level.vehicles[v].pos;
            assert!(vehicle.asleep, "{name}: {v} at {start} has not settled");
            assert!((vehicle.rot * Vec3::Y).y > 0.97, "{name}: {v} at {start} leans");
            assert!(vehicle.wheels.iter().all(|w| w.on_ground), "{name}: {v} at {start}");
            assert!(Vec2::new(vehicle.pos.x - start.x, vehicle.pos.z - start.z).length() < 0.3, "{name}: {v} at {start} moved");
        }
        for v in 0..hulls.len() {
            sim.world.clear_movers();
            for (other, boxes) in hulls.iter().enumerate() {
                if other != v {
                    for &(center, half, rot) in boxes {
                        sim.world.add_mover(center, half, rot);
                    }
                }
            }
            let mut contacts = Vec::new();
            for &(center, half, rot) in &hulls[v] {
                sim.world.box_contacts(center, half, rot, &mut contacts);
            }
            assert!(contacts.is_empty(), "{name}: {v} at {} touches something", sim.level.vehicles[v].pos);
        }
    }
}

/// Each on its own, with all the others still parked: in at the wheel and straight on out without hitting anything.
#[test]
fn every_vehicle_drives_straight_out() {
    for name in MAPS {
        let count = settled(name).vehicles.len();
        for v in 0..count {
            let mut sim = settled(name);
            let id = sim.spawn_soldier(v as u32);
            sim.sit_in(id, SeatedAt { carrier: v as u32, seat: 0 });
            let from = sim.vehicles[v].pos;
            let mut hit = None;
            for _ in 0..(7.0 / TICK_DT) as u32 {
                let yaw = sim.vehicles[v].frame().yaw();
                sim.step(&[(id, PlayerCmd { tick: sim.tick, wish_move: Vec2::Y, yaw, ..Default::default() })]);
                for event in &sim.events {
                    if let SimEvent::Vehicle { vehicle, event: VehicleEvent::Crashed { point, .. } } = event {
                        hit = hit.or(Some((*vehicle, *point)));
                    }
                }
            }
            let went = sim.vehicles[v].pos.distance(from);
            assert!(hit.is_none(), "{name}: {v} from {from} ran into something: {hit:?}");
            assert!(went > 40.0, "{name}: {v} from {from} only went {went}");
        }
    }
}
