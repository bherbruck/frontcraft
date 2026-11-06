//! Every vehicle on the town, as a game starts: settled, upright, clear of the bases and of each other, and able
//! to drive out of its gate.

use glam::{Vec2, Vec3};
use prs_sim::protocol::{PlayerCmd, TICK_DT};
use prs_sim::seats::SeatedAt;
use prs_sim::vehicles::VehicleEvent;
use prs_sim::{Level, Sim, SimEvent};

fn town() -> Sim {
    let mut sim = Sim::new(Level::named("town").unwrap());
    for _ in 0..(3.0 / TICK_DT) as u32 {
        sim.step(&[]);
    }
    sim
}

#[test]
fn each_base_has_the_same_vehicles_and_they_are_mirrored() {
    let sim = town();
    let spawns = &sim.level.vehicles;
    let (south, north): (Vec<&prs_sim::VehicleSpawn>, Vec<_>) = spawns.iter().partition(|spawn| spawn.pos.z > 0.0);
    assert_eq!(south.len(), north.len());
    assert!(south.len() >= 5, "{}", south.len());
    for s in &south {
        let mirrored = Vec2::new(90.0 - s.pos.x, -s.pos.z);
        assert!(
            north.iter().any(|n| n.kind == s.kind && Vec2::new(n.pos.x, n.pos.z).distance(mirrored) < 0.1),
            "nothing in the north base to match kind {} at {}",
            s.kind,
            s.pos
        );
    }
}

#[test]
fn every_vehicle_settles_upright_touching_nothing_but_the_ground() {
    let mut sim = town();
    let hulls: Vec<Vec<(Vec3, Vec3, glam::Quat)>> = sim
        .vehicles
        .iter()
        .map(|v| sim.vehicle_cfgs[v.kind].hull.iter().map(|hull| (v.pos + v.rot * hull.center, hull.half, v.rot)).collect())
        .collect();
    for (v, vehicle) in sim.vehicles.iter().enumerate() {
        let start = sim.level.vehicles[v].pos;
        assert!(vehicle.asleep, "{v} at {start} has not settled");
        assert!((vehicle.rot * Vec3::Y).y > 0.97, "{v} at {start} leans");
        assert!(vehicle.wheels.iter().all(|w| w.on_ground), "{v} at {start}");
        assert!(Vec2::new(vehicle.pos.x - start.x, vehicle.pos.z - start.z).length() < 0.3, "{v} at {start} moved");
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
        assert!(contacts.is_empty(), "{v} at {} touches something: {:?}", sim.level.vehicles[v].pos, contacts.first().map(|c| c.point));
    }
}

/// Each on its own, with all the others still parked: in at the wheel and straight on out without hitting anything.
#[test]
fn every_vehicle_drives_straight_out_of_its_gate() {
    let count = town().vehicles.len();
    for v in 0..count {
        let mut sim = town();
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
        assert!(hit.is_none(), "{v} from {from} ran into something: {hit:?}");
        assert!(went > 40.0, "{v} from {from} only went {went}");
    }
}
