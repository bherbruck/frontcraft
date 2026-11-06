//! Plugs the real simulation into `prs_net`. This is the one crate that knows both what a soldier
//! is and what goes on the wire, so when a soldier grows it is the only one that has to change.
//!
//! - [`own`]: a player's own soldier, every field at full precision, for prediction to compare with.
//! - [`remote`]: other soldiers, quantised, with what it takes to draw them and their guns.
//! - [`events`] and [`shared`]: what happens, and what changes rarely.
//! - [`client`] and [`server`]: the simulation as each side runs it.

pub mod client;
pub mod events;
pub mod fingerprint;
pub mod mode;
pub mod own;
pub mod remote;
pub mod rides;
pub mod server;
pub mod shared;
pub mod testkit;

pub use client::SimClient;
pub use mode::WireEvent;
pub use server::{ReachConfig, SimServer, SimServerConfig};

use glam::Vec3;
use prs_net::entity::NetEntity;
use prs_net::game::Schema;
use prs_net::wire::{Reader, WireError, Writer};
use prs_sim::health::Life;
use prs_mode::Mode;
use prs_sim::{Sim, SimEvent};
use remote::RemotePose;
use rides::{HeliPose, OwnState, VehiclePose};

/// Limits on what a client takes from a server. A server is trusted with the game, but not with how much
/// memory a player's machine gives it: one that is broken or hostile could name any soldier at all.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NetLimits {
    /// What a map from a server may ask to be built.
    pub map: prs_sim::MapLimits,
    /// Seconds a map's download may take in all before it is given up on.
    pub map_fetch_seconds: f32,
    /// Soldier numbers at or above this are refused, and the client never makes room for more. Well above
    /// the most players and bots a server runs. A mod changes it under `net_limits`.
    pub max_soldiers: usize,
    /// The largest map file a client fetches from a server, in bytes. One said to be bigger is refused
    /// before anything is fetched, and a download is stopped past it.
    pub max_map_bytes: usize,
}

impl Default for NetLimits {
    fn default() -> Self {
        Self { map: prs_sim::MapLimits::default(), map_fetch_seconds: 60.0, max_soldiers: 512, max_map_bytes: 16 << 20 }
    }
}

/// The limits for this run, from the active mod. Read once, the first time they are needed.
pub fn limits() -> NetLimits {
    static LIMITS: std::sync::OnceLock<NetLimits> = std::sync::OnceLock::new();
    *LIMITS.get_or_init(|| prs_sim::prs_mods::tuned("net_limits", NetLimits::default()))
}

/// A soldier's number off the wire, refused if it is past what any server should have.
pub(crate) fn read_soldier(r: &mut Reader) -> Result<prs_sim::SoldierId, WireError> {
    let id = r.varu()?;
    if id >= limits().max_soldiers as u64 {
        return Err(WireError::TooMany);
    }
    Ok(id as prs_sim::SoldierId)
}

pub const KIND_SOLDIER: u8 = 0;
pub const KIND_VEHICLE: u8 = 1;
pub const KIND_HELI: u8 = 2;

/// Anyone or anything else, as drawn at one moment.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyPose {
    Soldier(RemotePose),
    Vehicle(VehiclePose),
    Heli(HeliPose),
}

/// Hashes everything both sides must share and neither sends: rules, guns and the map. Prediction by
/// other rules than the server's would be wrong on every tick, so a client that differs is refused.
///
/// The map goes in by its shape and the hash of its file, and not its numbers. Terrain and buildings come
/// out of `sin` and `cos`, and those differ in the last bits between native and wasm (see [`fingerprint`]),
/// so hashing the numbers would refuse every browser. The same file on both sides is the same map. The
/// configs are plain constants, and those do agree.
pub fn rules_hash(sim: &Sim, mode: &Mode) -> u64 {
    let level = &sim.level;
    prs_net::hash_text(&format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?} {} {} {} {} {} {} {} {} {:016x}",
        mode,
        sim.vehicle_cfgs,
        sim.heli_cfg,
        sim.heli_ride,
        sim.move_cfg,
        sim.weapons,
        sim.health_cfg,
        sim.suppression_cfg,
        sim.throw_cfg,
        sim.world_cfg,
        sim.inventory_cfg,
        sim.body_cfg,
        level.terrain.size,
        level.terrain.cells,
        level.solids.len(),
        level.trees.len(),
        level.targets.len(),
        level.rooms.len(),
        sim.vehicles.len(),
        sim.helicopters.len(),
        level.source,
    ))
}

pub struct SimSchema;

impl Schema for SimSchema {
    type Own = OwnState;
    type Event = WireEvent;
    type Pose = AnyPose;

    fn groups(kind: u8) -> Option<&'static [(usize, usize)]> {
        match kind {
            KIND_SOLDIER => Some(remote::GROUPS),
            KIND_VEHICLE => Some(rides::VEHICLE_GROUPS),
            KIND_HELI => Some(rides::HELI_GROUPS),
            _ => None,
        }
    }

    fn write_own(own: &OwnState, base: Option<&OwnState>, w: &mut Writer) {
        rides::write_own(own, base, w);
    }

    fn read_own(base: Option<&OwnState>, r: &mut Reader) -> Result<OwnState, WireError> {
        rides::read_own(base, r)
    }

    fn own_matches(a: &OwnState, b: &OwnState, epsilon: f32) -> bool {
        rides::own_matches(a, b, epsilon)
    }

    fn own_position(own: &OwnState) -> Vec3 {
        own.soldier.body.pos
    }

    fn write_event(event: &WireEvent, w: &mut Writer) {
        mode::write_event(event, w);
    }

    fn read_event(r: &mut Reader) -> Result<WireEvent, WireError> {
        mode::read_event(r)
    }

    /// What happens to you can't wait for the rest of the world to catch up on screen.
    fn urgent(event: &WireEvent, me: u16) -> bool {
        let me = me as usize;
        let event = match event {
            WireEvent::Sim(event) => event,
            WireEvent::Mode(_) => return true,
        };
        match *event {
            SimEvent::SoldierHit { soldier, .. }
            | SimEvent::BulletPassed { soldier, .. }
            | SimEvent::Health { soldier, .. }
            | SimEvent::Respawned { soldier } => soldier == me,
            _ => false,
        }
    }

    fn pose(entity: &NetEntity) -> AnyPose {
        match entity.kind() {
            KIND_VEHICLE => AnyPose::Vehicle(VehiclePose::from_entity(entity)),
            KIND_HELI => AnyPose::Heli(HeliPose::from_entity(entity)),
            _ => AnyPose::Soldier(RemotePose::from_entity(entity)),
        }
    }

    fn lerp_pose(a: &AnyPose, b: &AnyPose, t: f32) -> AnyPose {
        match (a, b) {
            (AnyPose::Soldier(a), AnyPose::Soldier(b)) => AnyPose::Soldier(a.lerp(b, t)),
            (AnyPose::Vehicle(a), AnyPose::Vehicle(b)) => AnyPose::Vehicle(a.lerp(b, t)),
            (AnyPose::Heli(a), AnyPose::Heli(b)) => AnyPose::Heli(a.lerp(b, t)),
            _ => b.clone(),
        }
    }

    fn pose_position(pose: &AnyPose) -> Vec3 {
        match pose {
            AnyPose::Soldier(s) => s.feet,
            AnyPose::Vehicle(v) => v.pos,
            AnyPose::Heli(h) => h.pos,
        }
    }

    fn extrapolate_pose(pose: &AnyPose, seconds: f32) -> AnyPose {
        match pose {
            // The dead and the downed stay where they fell.
            AnyPose::Soldier(s) if s.life == Life::Up => {
                AnyPose::Soldier(RemotePose { feet: s.feet + s.velocity * seconds, ..*s })
            }
            AnyPose::Vehicle(v) => AnyPose::Vehicle(VehiclePose { pos: v.pos + v.vel * seconds, ..v.clone() }),
            AnyPose::Heli(h) => AnyPose::Heli(HeliPose { pos: h.pos + h.vel * seconds, ..*h }),
            other => other.clone(),
        }
    }
}

/// Whether anyone else needs to hear of this. A client predicts its own steps, reloads and what it
/// carries, and only your own of those make a sound so far, so those stay home. Being hit, health and
/// what is on the ground come from the server even for you.
pub(crate) fn goes_on_wire(event: &SimEvent) -> bool {
    !matches!(event, SimEvent::Move { .. } | SimEvent::Weapon { .. } | SimEvent::Carry { .. } | SimEvent::ThrownBounced { .. })
}

/// Whether a prediction should show this right away rather than wait to hear from the server.
pub(crate) fn shown_when_predicted(event: &SimEvent) -> bool {
    matches!(
        event,
        SimEvent::ShotFired { .. }
            | SimEvent::Move { .. }
            | SimEvent::Weapon { .. }
            | SimEvent::Carry { .. }
            | SimEvent::Seat { .. }
            | SimEvent::ThrownReleased { .. }
            | SimEvent::ThrownBounced { .. }
    )
}

#[cfg(test)]
mod climb_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use prs_sim::Level;

    /// What a client takes from a server is up to the player, so a mod can change it.
    #[test]
    fn a_mod_can_set_the_net_limits() {
        let found = prs_sim::prs_mods::Mod::parse(
            "limits",
            "[net_limits]\nmax_soldiers = 64\nmax_map_bytes = 1000000\nmap_fetch_seconds = 5.0\n[net_limits.map]\nmax_trees = 10\n",
        )
        .unwrap();
        let limits = found.over("net_limits", NetLimits::default()).unwrap();
        assert_eq!((limits.max_soldiers, limits.max_map_bytes, limits.map_fetch_seconds), (64, 1_000_000, 5.0));
        assert_eq!(limits.map.max_trees, 10);
        assert_eq!(limits.map.max_cells, prs_sim::MapLimits::default().max_cells, "and the rest as they were");
    }

    #[test]
    fn rounding_in_the_map_is_not_a_different_map_but_tuning_or_another_map_is() {
        let base = Sim::new(Level::named("range").unwrap());
        let mut level = Level::named("range").unwrap();
        level.terrain.vertices[100].y += 1e-5;
        level.solids[0].center.x += 1e-5;
        let practice = Mode::free_practice();
        assert_eq!(rules_hash(&Sim::new(level), &practice), rules_hash(&base, &practice));

        let mut tuned = Sim::new(Level::named("range").unwrap());
        tuned.move_cfg.run_speed *= 1.01;
        assert_ne!(rules_hash(&tuned, &practice), rules_hash(&base, &practice));
        assert_ne!(rules_hash(&Sim::new(Level::named("town").unwrap()), &practice), rules_hash(&base, &practice));
        assert_ne!(rules_hash(&base, &Mode::named("aas").unwrap()), rules_hash(&base, &practice));
    }
}
