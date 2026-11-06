//! The simulation's events on the wire. See [`crate::goes_on_wire`] for which ones go.

use prs_net::wire::{Reader, WireError, Writer};
use prs_sim::health::{HealthEvent, Zone};
use prs_sim::weapons::WeaponId;
use prs_sim::vehicles::{VehicleEvent, VehicleZone};
use prs_sim::helicopter::{HeliEvent, HeliZone};
use prs_sim::throwables::ThrownKind;
use prs_sim::{SeatEvent, SimEvent, Surface};

const SHOT: u8 = 0;
const IMPACT: u8 = 1;
const TARGET: u8 = 2;
const HIT: u8 = 3;
const HEALTH: u8 = 4;
const RESPAWNED: u8 = 5;
const KIT_DROPPED: u8 = 6;
const KIT_GONE: u8 = 7;
const SEAT: u8 = 8;
const VEHICLE: u8 = 9;
const VEHICLE_HIT: u8 = 10;
const HELI: u8 = 11;
const BULLET_PASSED: u8 = 13;
const HELI_HIT: u8 = 12;
const THROWN: u8 = 14;
const BURST: u8 = 15;

fn kind_to_u8(kind: ThrownKind) -> u8 {
    match kind {
        ThrownKind::Frag => 0,
        ThrownKind::Smoke => 1,
    }
}

fn kind_from_u8(v: u8) -> Result<ThrownKind, WireError> {
    match v {
        0 => Ok(ThrownKind::Frag),
        1 => Ok(ThrownKind::Smoke),
        _ => Err(WireError::BadTag),
    }
}

pub(crate) fn surface_to_u8(s: Surface) -> u8 {
    match s {
        Surface::Dirt => 0,
        Surface::Wood => 1,
        Surface::Concrete => 2,
        Surface::Metal => 3,
        Surface::Flesh => 4,
        Surface::Grass => 5,
        Surface::Gravel => 6,
    }
}

pub(crate) fn surface_from_u8(v: u8) -> Result<Surface, WireError> {
    Ok(match v {
        0 => Surface::Dirt,
        1 => Surface::Wood,
        2 => Surface::Concrete,
        3 => Surface::Metal,
        4 => Surface::Flesh,
        5 => Surface::Grass,
        6 => Surface::Gravel,
        _ => return Err(WireError::BadTag),
    })
}

fn zone_to_u8(zone: Zone) -> u8 {
    match zone {
        Zone::Head => 0,
        Zone::Chest => 1,
        Zone::Abdomen => 2,
        Zone::Arm => 3,
        Zone::Leg => 4,
    }
}

fn zone_from_u8(v: u8) -> Result<Zone, WireError> {
    Ok(match v {
        0 => Zone::Head,
        1 => Zone::Chest,
        2 => Zone::Abdomen,
        3 => Zone::Arm,
        4 => Zone::Leg,
        _ => return Err(WireError::BadTag),
    })
}

fn health_to_u8(event: HealthEvent) -> u8 {
    match event {
        HealthEvent::BleedingStarted => 0,
        HealthEvent::BleedingStopped => 1,
        HealthEvent::ArmWounded => 2,
        HealthEvent::LegWounded => 3,
        HealthEvent::Downed => 4,
        HealthEvent::Died => 5,
        HealthEvent::Revived => 6,
        HealthEvent::Healed => 7,
    }
}

fn health_from_u8(v: u8) -> Result<HealthEvent, WireError> {
    Ok(match v {
        0 => HealthEvent::BleedingStarted,
        1 => HealthEvent::BleedingStopped,
        2 => HealthEvent::ArmWounded,
        3 => HealthEvent::LegWounded,
        4 => HealthEvent::Downed,
        5 => HealthEvent::Died,
        6 => HealthEvent::Revived,
        7 => HealthEvent::Healed,
        _ => return Err(WireError::BadTag),
    })
}

fn write_heli_event(event: HeliEvent, w: &mut Writer) {
    match event {
        HeliEvent::EngineStarted => w.u8(0),
        HeliEvent::EngineStopped => w.u8(1),
        HeliEvent::EngineDied => w.u8(2),
        HeliEvent::TailRotorLost => w.u8(3),
        HeliEvent::RotorLost => w.u8(4),
        HeliEvent::Touchdown { speed } => {
            w.u8(5);
            w.f32(speed);
        }
        HeliEvent::Crunch { point, speed } => {
            w.u8(6);
            w.vec3(point);
            w.f32(speed);
        }
        HeliEvent::RotorStrike { point, canopy } => {
            w.u8(7);
            w.vec3(point);
            w.u8(canopy as u8);
        }
        HeliEvent::Wrecked => w.u8(8),
        HeliEvent::Respawned => w.u8(9),
    }
}

fn read_heli_event(r: &mut Reader) -> Result<HeliEvent, WireError> {
    Ok(match r.u8()? {
        0 => HeliEvent::EngineStarted,
        1 => HeliEvent::EngineStopped,
        2 => HeliEvent::EngineDied,
        3 => HeliEvent::TailRotorLost,
        4 => HeliEvent::RotorLost,
        5 => HeliEvent::Touchdown { speed: r.f32()? },
        6 => HeliEvent::Crunch { point: r.vec3()?, speed: r.f32()? },
        7 => HeliEvent::RotorStrike { point: r.vec3()?, canopy: r.u8()? != 0 },
        8 => HeliEvent::Wrecked,
        9 => HeliEvent::Respawned,
        _ => return Err(WireError::BadTag),
    })
}

pub(crate) fn write(event: &SimEvent, w: &mut Writer) {
    match *event {
        SimEvent::ShotFired { soldier, origin, dir, weapon } => {
            w.u8(SHOT);
            w.varu(soldier as u64);
            w.vec3(origin);
            w.vec3(dir);
            w.u16(weapon.0);
        }
        SimEvent::Impact { point, normal, surface } => {
            w.u8(IMPACT);
            w.vec3(point);
            w.vec3(normal);
            w.u8(surface_to_u8(surface));
        }
        SimEvent::TargetHit { target, point, headshot } => {
            w.u8(TARGET);
            w.varu(target as u64);
            w.vec3(point);
            w.u8(headshot as u8);
        }
        SimEvent::SoldierHit { soldier, shooter, zone, point, damage } => {
            w.u8(HIT);
            w.varu(soldier as u64);
            w.varu(shooter as u64);
            w.u8(zone_to_u8(zone));
            w.vec3(point);
            w.f32(damage);
        }
        SimEvent::Health { soldier, event } => {
            w.u8(HEALTH);
            w.varu(soldier as u64);
            w.u8(health_to_u8(event));
        }
        SimEvent::Respawned { soldier } => {
            w.u8(RESPAWNED);
            w.varu(soldier as u64);
        }
        SimEvent::KitDropped { kit, pos } => {
            w.u8(KIT_DROPPED);
            w.varu(kit as u64);
            w.vec3(pos);
        }
        SimEvent::KitGone { kit } => {
            w.u8(KIT_GONE);
            w.varu(kit as u64);
        }
        SimEvent::Seat { soldier, event } => {
            w.u8(SEAT);
            w.varu(soldier as u64);
            match event {
                SeatEvent::Entered { carrier, seat } => {
                    w.u8(0);
                    w.varu(carrier as u64);
                    w.u8(seat);
                }
                SeatEvent::Swapped { carrier, seat } => {
                    w.u8(1);
                    w.varu(carrier as u64);
                    w.u8(seat);
                }
                SeatEvent::Left { carrier } => {
                    w.u8(2);
                    w.varu(carrier as u64);
                }
                SeatEvent::ExitBlocked { carrier } => {
                    w.u8(3);
                    w.varu(carrier as u64);
                }
                SeatEvent::Entering { carrier, seat } => {
                    w.u8(4);
                    w.varu(carrier as u64);
                    w.u8(seat);
                }
                SeatEvent::Leaving { carrier } => {
                    w.u8(5);
                    w.varu(carrier as u64);
                }
            }
        }
        SimEvent::Vehicle { vehicle, event } => {
            w.u8(VEHICLE);
            w.varu(vehicle as u64);
            match event {
                VehicleEvent::Crashed { point, speed } => {
                    w.u8(0);
                    w.vec3(point);
                    w.f32(speed);
                }
                VehicleEvent::EngineDead => w.u8(1),
                VehicleEvent::LiftBlocked => w.u8(6),
                VehicleEvent::Heave { point } => {
                    w.u8(7);
                    w.vec3(point);
                }
                VehicleEvent::Landed { point, speed } => {
                    w.u8(8);
                    w.vec3(point);
                    w.f32(speed);
                }
                VehicleEvent::TyreBurst { wheel } => {
                    w.u8(2);
                    w.u8(wheel);
                }
                VehicleEvent::OnFire => w.u8(3),
                VehicleEvent::Exploded { point } => {
                    w.u8(4);
                    w.vec3(point);
                }
                VehicleEvent::Respawned => w.u8(5),
            }
        }
        SimEvent::VehicleHit { vehicle, shooter, zone, point, normal } => {
            w.u8(VEHICLE_HIT);
            w.varu(vehicle as u64);
            w.varu(shooter as u64);
            match zone {
                VehicleZone::Body => w.u8(0),
                VehicleZone::Engine => w.u8(1),
                VehicleZone::Tyre(wheel) => {
                    w.u8(2);
                    w.u8(wheel);
                }
            }
            w.vec3(point);
            w.vec3(normal);
        }
        SimEvent::Heli { heli, event } => {
            w.u8(HELI);
            w.varu(heli as u64);
            write_heli_event(event, w);
        }
        SimEvent::HeliHit { heli, shooter, zone, point } => {
            w.u8(HELI_HIT);
            w.varu(heli as u64);
            w.varu(shooter as u64);
            w.u8(match zone {
                HeliZone::Engine => 0,
                HeliZone::TailRotor => 1,
                HeliZone::MainRotor => 2,
                HeliZone::Cockpit => 3,
                HeliZone::Structure => 4,
            });
            w.vec3(point);
        }
        SimEvent::BulletPassed { soldier, shooter, point, weapon, distance, cracked, amount } => {
            w.u8(BULLET_PASSED);
            w.varu(soldier as u64);
            w.varu(shooter as u64);
            w.vec3(point);
            w.u16(weapon.0);
            w.f32(distance);
            w.u8(cracked as u8);
            w.f32(amount);
        }
        SimEvent::ThrownReleased { soldier, id, kind, origin, vel, fuse } => {
            w.u8(THROWN);
            w.varu(soldier as u64);
            w.varu(id as u64);
            w.u8(kind_to_u8(kind));
            w.vec3(origin);
            w.vec3(vel);
            w.f32(fuse);
        }
        SimEvent::ThrownBurst { id, kind, point } => {
            w.u8(BURST);
            w.varu(id as u64);
            w.u8(kind_to_u8(kind));
            w.vec3(point);
        }
        SimEvent::Move { .. } | SimEvent::Weapon { .. } | SimEvent::Carry { .. } | SimEvent::ThrownBounced { .. } => {
            unreachable!("the server filters these out with goes_on_wire")
        }
    }
}

pub(crate) fn read(r: &mut Reader) -> Result<SimEvent, WireError> {
    Ok(match r.u8()? {
        SHOT => SimEvent::ShotFired {
            soldier: r.varu32()? as usize,
            origin: r.vec3()?,
            dir: r.vec3()?,
            weapon: WeaponId(r.u16()?),
        },
        IMPACT => SimEvent::Impact { point: r.vec3()?, normal: r.vec3()?, surface: surface_from_u8(r.u8()?)? },
        TARGET => SimEvent::TargetHit { target: r.varu32()? as usize, point: r.vec3()?, headshot: r.u8()? != 0 },
        HIT => SimEvent::SoldierHit {
            soldier: r.varu32()? as usize,
            shooter: r.varu32()? as usize,
            zone: zone_from_u8(r.u8()?)?,
            point: r.vec3()?,
            damage: r.f32()?,
        },
        HEALTH => SimEvent::Health { soldier: r.varu32()? as usize, event: health_from_u8(r.u8()?)? },
        RESPAWNED => SimEvent::Respawned { soldier: r.varu32()? as usize },
        KIT_DROPPED => SimEvent::KitDropped { kit: r.varu32()?, pos: r.vec3()? },
        KIT_GONE => SimEvent::KitGone { kit: r.varu32()? },
        SEAT => {
            let soldier = r.varu32()? as usize;
            let event = match r.u8()? {
                0 => SeatEvent::Entered { carrier: r.varu32()?, seat: r.u8()? },
                1 => SeatEvent::Swapped { carrier: r.varu32()?, seat: r.u8()? },
                2 => SeatEvent::Left { carrier: r.varu32()? },
                3 => SeatEvent::ExitBlocked { carrier: r.varu32()? },
                4 => SeatEvent::Entering { carrier: r.varu32()?, seat: r.u8()? },
                5 => SeatEvent::Leaving { carrier: r.varu32()? },
                _ => return Err(WireError::BadTag),
            };
            SimEvent::Seat { soldier, event }
        }
        VEHICLE => {
            let vehicle = r.varu32()? as usize;
            let event = match r.u8()? {
                0 => VehicleEvent::Crashed { point: r.vec3()?, speed: r.f32()? },
                1 => VehicleEvent::EngineDead,
                2 => VehicleEvent::TyreBurst { wheel: r.u8()? },
                3 => VehicleEvent::OnFire,
                4 => VehicleEvent::Exploded { point: r.vec3()? },
                5 => VehicleEvent::Respawned,
                6 => VehicleEvent::LiftBlocked,
                7 => VehicleEvent::Heave { point: r.vec3()? },
                8 => VehicleEvent::Landed { point: r.vec3()?, speed: r.f32()? },
                _ => return Err(WireError::BadTag),
            };
            SimEvent::Vehicle { vehicle, event }
        }
        VEHICLE_HIT => {
            let (vehicle, shooter) = (r.varu32()? as usize, r.varu32()? as usize);
            let zone = match r.u8()? {
                0 => VehicleZone::Body,
                1 => VehicleZone::Engine,
                2 => VehicleZone::Tyre(r.u8()?),
                _ => return Err(WireError::BadTag),
            };
            SimEvent::VehicleHit { vehicle, shooter, zone, point: r.vec3()?, normal: r.vec3()? }
        }
        THROWN => SimEvent::ThrownReleased {
            soldier: r.varu32()? as usize,
            id: r.varu32()?,
            kind: kind_from_u8(r.u8()?)?,
            origin: r.vec3()?,
            vel: r.vec3()?,
            fuse: r.f32()?,
        },
        BURST => SimEvent::ThrownBurst { id: r.varu32()?, kind: kind_from_u8(r.u8()?)?, point: r.vec3()? },
        BULLET_PASSED => SimEvent::BulletPassed {
            soldier: r.varu32()? as usize,
            shooter: r.varu32()? as usize,
            point: r.vec3()?,
            weapon: WeaponId(r.u16()?),
            distance: r.f32()?,
            cracked: r.u8()? != 0,
            amount: r.f32()?,
        },
        HELI => SimEvent::Heli { heli: r.varu32()? as usize, event: read_heli_event(r)? },
        HELI_HIT => SimEvent::HeliHit {
            heli: r.varu32()? as usize,
            shooter: r.varu32()? as usize,
            zone: match r.u8()? {
                0 => HeliZone::Engine,
                1 => HeliZone::TailRotor,
                2 => HeliZone::MainRotor,
                3 => HeliZone::Cockpit,
                4 => HeliZone::Structure,
                _ => return Err(WireError::BadTag),
            },
            point: r.vec3()?,
        },
        _ => return Err(WireError::BadTag),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn events_round_trip() {
        let p = Vec3::new(1.5, -2.25, 3.0);
        for event in [
            SimEvent::ShotFired { soldier: 3, origin: p, dir: Vec3::X, weapon: WeaponId(2) },
            SimEvent::Impact { point: p, normal: Vec3::Y, surface: Surface::Metal },
            SimEvent::TargetHit { target: 9, point: p, headshot: true },
            SimEvent::SoldierHit { soldier: 1, shooter: 2, zone: Zone::Leg, point: p, damage: 33.5 },
            SimEvent::Health { soldier: 4, event: HealthEvent::Downed },
            SimEvent::Respawned { soldier: 5 },
            SimEvent::KitDropped { kit: 7, pos: p },
            SimEvent::KitGone { kit: 7 },
            SimEvent::Seat { soldier: 2, event: SeatEvent::Entered { carrier: 1, seat: 3 } },
            SimEvent::Seat { soldier: 2, event: SeatEvent::Swapped { carrier: 1, seat: 0 } },
            SimEvent::Seat { soldier: 2, event: SeatEvent::Left { carrier: 1 } },
            SimEvent::Seat { soldier: 2, event: SeatEvent::ExitBlocked { carrier: 1 } },
            SimEvent::Seat { soldier: 2, event: SeatEvent::Entering { carrier: 1, seat: 2 } },
            SimEvent::Seat { soldier: 2, event: SeatEvent::Leaving { carrier: 1 } },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::Crashed { point: p, speed: 12.5 } },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::EngineDead },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::TyreBurst { wheel: 3 } },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::OnFire },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::Exploded { point: p } },
            SimEvent::Vehicle { vehicle: 1, event: VehicleEvent::Respawned },
            SimEvent::VehicleHit { vehicle: 2, shooter: 1, zone: VehicleZone::Body, point: p, normal: Vec3::X },
            SimEvent::VehicleHit { vehicle: 2, shooter: 1, zone: VehicleZone::Tyre(1), point: p, normal: Vec3::X },
        ] {
            let mut w = Writer::default();
            write(&event, &mut w);
            assert_eq!(read(&mut Reader::new(&w.buf)), Ok(event));
        }
    }
}
