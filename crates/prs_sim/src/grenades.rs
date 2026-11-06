//! Throwing things, and what happens when they go off. A soldier winds up by holding the trigger with one
//! selected. Letting go throws it from the hand, along the look. On a server it then goes off where it ends
//! up, and each fragment is a real ray: walls stop them and bones catch them, so a wall covers you from a
//! grenade the same way it covers you from a bullet, and being spread out is what keeps a squad alive.

use crate::hits::{BODY_MIDDLE, BODY_REACH, bone_radius, zone_of};
use crate::*;
use prs_body::Bone;
use prs_collision::ray_vs_bone;
use glam::EulerRot;
use prs_helicopter::HeliZone;
use prs_throwables::{FragConfig, fragment_dir};
use prs_vehicles::VehicleZone;

/// Most panels one line goes through. Past a couple there is nothing left of it anyway.
const MAX_PANELS: usize = 4;

/// What a soldier is doing with something to throw. Plain data, like the rest of a soldier.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThrowHold {
    /// Seconds the trigger has been held with one in hand. For a frag this is also how long the fuse has run.
    pub held: f32,
    /// Going to throw it underarm, for somewhere close.
    pub underarm: bool,
    /// How many this soldier has thrown, which makes each one's id without asking anyone.
    pub thrown: u32,
}

/// The id of a soldier's nth throw. Worked out on both sides, so that a client's own copy needs no word back.
fn thrown_id(soldier: SoldierId, nth: u32) -> u32 {
    ((soldier as u32) << 20) | (nth & 0xF_FFFF)
}

impl Sim {
    pub(crate) fn step_throw(&mut self, id: SoldierId, buttons: Buttons, up: bool) {
        let soldier = &self.soldiers[id];
        let kind = match soldier.inventory.selected {
            Slot::Frag => ThrownKind::Frag,
            Slot::Smoke => ThrownKind::Smoke,
            _ => {
                self.soldiers[id].throw.held = 0.0;
                return;
            }
        };
        let holding = soldier.throw.held > 0.0;
        if !up {
            // Whatever they had the pin out of goes where they fell.
            if holding {
                self.release(id, kind, true);
            }
            return;
        }
        // Hands are not free until the gun is out of the way.
        let wants = buttons.contains(Buttons::FIRE) && !soldier.hold_fire && soldier.weapon.ready <= 0.0;
        if wants {
            let throw = &mut self.soldiers[id].throw;
            throw.held += TICK_DT;
            throw.underarm = buttons.contains(Buttons::ADS);
            // Held too long, a frag goes off right there.
            if kind == ThrownKind::Frag && throw.held >= self.throw_cfg.frag.fuse {
                self.release(id, kind, true);
            }
        } else if holding {
            self.release(id, kind, false);
        }
    }

    /// Lets go of one. `dropped` is out of a hand that is no longer throwing anything.
    fn release(&mut self, id: SoldierId, kind: ThrownKind, dropped: bool) {
        let cfg = &self.throw_cfg;
        let soldier = &self.soldiers[id];
        let hold = soldier.throw;
        let strength = (hold.held / cfg.windup.max(1e-3)).clamp(cfg.least_strength, 1.0);
        let (hand, speed, lift) = if hold.underarm {
            (cfg.underarm_hand, cfg.underarm_speed, cfg.underarm_lift)
        } else {
            (cfg.overarm_hand, cfg.overarm_speed, cfg.overarm_lift)
        };
        let look = Quat::from_euler(EulerRot::YXZ, soldier.yaw, soldier.pitch, 0.0);
        let eye = soldier.eye();
        let wanted = eye + look * hand;
        // A hand against a wall lets go on this side of it.
        let path = wanted - eye;
        let reach = path.length().max(1e-3);
        let origin = match self.world.raycast(eye, path / reach, reach) {
            Some(hit) => eye + path / reach * (hit.distance - cfg.radius * 2.0).max(0.0),
            None => wanted,
        };
        let vel = if dropped { soldier.body.vel * cfg.inherit } else { view_dir(soldier.yaw, soldier.pitch + lift) * speed * strength + soldier.body.vel * cfg.inherit };
        let fuse = match kind {
            ThrownKind::Frag => (cfg.frag.fuse - hold.held).max(0.0),
            ThrownKind::Smoke => cfg.smoke.fuse,
        };
        let thrown = Thrown { id: thrown_id(id, hold.thrown), kind, thrower: id, pos: origin, vel, fuse, at_rest: false };
        let soldier = &mut self.soldiers[id];
        match kind {
            ThrownKind::Frag => soldier.inventory.stash.frags = soldier.inventory.stash.frags.saturating_sub(1),
            ThrownKind::Smoke => soldier.inventory.stash.smokes = soldier.inventory.stash.smokes.saturating_sub(1),
        }
        soldier.throw = ThrowHold { thrown: hold.thrown + 1, ..ThrowHold::default() };
        // Back to the gun, which will not fire until the finger comes off.
        soldier.inventory.selected = soldier.inventory.held;
        soldier.hold_fire = true;
        self.thrown.push(thrown);
        self.events.push(SimEvent::ThrownReleased { soldier: id, id: thrown.id, kind, origin, vel, fuse });
    }

    /// One tick of everything thrown, and whatever goes off. Only for the side that decides.
    pub(crate) fn step_thrown(&mut self) {
        let mut i = 0;
        while i < self.thrown.len() {
            if let Some(bounce) = self.thrown[i].fly(&self.throw_cfg, &self.world, TICK_DT) {
                self.events.push(SimEvent::ThrownBounced { point: bounce.point, speed: bounce.speed });
            }
            let thrown = self.thrown[i];
            if thrown.fuse > 0.0 {
                i += 1;
                continue;
            }
            if thrown.kind == ThrownKind::Frag {
                self.thrown.swap_remove(i);
                self.burst(&thrown);
                continue;
            }
            if !self.clouds.iter().any(|c| c.id == thrown.id) {
                self.burst(&thrown);
            }
            if self.follow_smoke(&thrown) {
                self.thrown.swap_remove(i);
            } else {
                i += 1;
            }
        }
        self.age_clouds();
    }

    /// Keeps a smoke grenade's cloud over it as it falls. True once it has got wherever it is going.
    fn follow_smoke(&mut self, thrown: &Thrown) -> bool {
        let cfg = &self.throw_cfg.smoke;
        if let Some(cloud) = self.clouds.iter_mut().find(|c| c.id == thrown.id) {
            cloud.over(thrown.pos, cfg);
        }
        thrown.at_rest || thrown.fuse < -cfg.fall_time
    }

    fn age_clouds(&mut self) {
        let cfg = &self.throw_cfg.smoke;
        for cloud in &mut self.clouds {
            cloud.age += TICK_DT;
        }
        self.clouds.retain(|cloud| !cloud.finished(cfg));
    }

    /// A client's copy of what is flying: it hits walls and does nothing, and waits to hear from the server.
    pub fn fly_thrown(&mut self) {
        let (cfg, world) = (&self.throw_cfg, &self.world);
        let events = &mut self.events;
        let clouds = &mut self.clouds;
        self.thrown.retain_mut(|thrown| {
            if let Some(bounce) = thrown.fly(cfg, world, TICK_DT) {
                events.push(SimEvent::ThrownBounced { point: bounce.point, speed: bounce.speed });
            }
            // Heard to have gone off: on its way down with its cloud, until the server says where that ended.
            if let Some(cloud) = clouds.iter_mut().find(|c| c.id == thrown.id) {
                cloud.over(thrown.pos, &cfg.smoke);
                return !thrown.at_rest && thrown.fuse > -cfg.smoke.fall_time;
            }
            thrown.fuse > -cfg.client_patience
        });
        self.age_clouds();
    }

    /// Something thrown that a client has been told about.
    pub fn add_thrown(&mut self, thrown: Thrown) {
        if !self.thrown.iter().any(|t| t.id == thrown.id) {
            self.thrown.push(thrown);
        }
    }

    /// The server's word that one has gone off, on a client.
    pub fn hear_burst(&mut self, id: u32, kind: ThrownKind, point: Vec3) {
        if kind != ThrownKind::Smoke {
            self.thrown.retain(|t| t.id != id);
        } else if !self.clouds.iter().any(|c| c.id == id) {
            self.clouds.push(Cloud::new(id, point, &self.throw_cfg.smoke));
        }
    }

    /// Where the grenade under a cloud is now, falling or at rest. For showing it: no rule cares.
    pub fn smoke_source(&self, cloud: &Cloud) -> Vec3 {
        self.thrown.iter().find(|t| t.id == cloud.id).map_or(cloud.grenade_at(&self.throw_cfg.smoke), |t| t.pos)
    }

    /// Whether smoke stops anyone seeing along this line.
    pub fn smoke_blocks(&self, a: Vec3, b: Vec3) -> bool {
        prs_throwables::smoke_blocks(&self.clouds, &self.throw_cfg.smoke, a, b)
    }

    fn burst(&mut self, thrown: &Thrown) {
        self.events.push(SimEvent::ThrownBurst { id: thrown.id, kind: thrown.kind, point: thrown.pos });
        match thrown.kind {
            ThrownKind::Smoke => self.clouds.push(Cloud::new(thrown.id, thrown.pos, &self.throw_cfg.smoke)),
            ThrownKind::Frag => self.frag_burst(thrown),
        }
    }

    fn frag_burst(&mut self, thrown: &Thrown) {
        let cfg = self.throw_cfg.frag.clone();
        // A little up off whatever it lay on, so the ground takes only what went down into it.
        let center = thrown.pos + Vec3::Y * (self.throw_cfg.radius * 2.0);
        let reach = cfg.fragment_reach.max(cfg.blast_radius);

        // Only those it could reach, and their bones once, since this is worked out for all of them at once.
        let near: Vec<(SoldierId, Pose)> = (0..self.soldiers.len())
            .filter(|&id| {
                let s = &self.soldiers[id];
                s.present && !s.health.is_dead() && (s.body.pos + Vec3::Y * BODY_MIDDLE).distance(center) < reach + BODY_REACH
            })
            .map(|id| (id, self.body_pose(&self.body_input(id))))
            .collect();

        let mut damage = vec![Vec::<(Zone, Vec3, f32)>::new(); near.len()];
        let mut struck = Vec::new();
        for i in 0..cfg.fragments {
            let dir = fragment_dir(thrown.id, i, cfg.fragments);
            self.fly_fragment(center, dir, &cfg, &near, &mut damage, &mut struck);
        }

        // The blast, on anything with a line to it, through whatever is not armour.
        for v in 0..self.vehicles.len() {
            let to = self.vehicles[v].pos - center;
            let distance = to.length();
            if distance > 1e-3 && distance < cfg.blast_radius + self.vehicle_cfgs[self.vehicles[v].kind].reach() {
                let dir = to / distance;
                let wall = self.world.raycast(center, dir, distance).map_or(distance, |hit| hit.distance);
                if let Some(found) = self.vehicle_strike_on(v, center, dir, wall).filter(|f| f.distance < cfg.blast_radius) {
                    let share = 1.0 - found.distance / cfg.blast_radius.max(1e-3);
                    struck.push(Struck::Vehicle(v, found.zone, center + dir * found.distance, found.normal, cfg.blast_damage * share * share * cfg.vehicle_share));
                }
            }
        }
        for (n, (id, _)) in near.iter().enumerate() {
            let id = *id;
            let chest = self.soldiers[id].body.pos + Vec3::Y * BODY_MIDDLE;
            let eye = self.soldiers[id].eye();
            let distance = chest.distance(center);
            if distance < cfg.blast_radius {
                let through = self.clear_share(center, chest);
                if through > 0.0 {
                    let share = 1.0 - distance / cfg.blast_radius.max(1e-3);
                    damage[n].push((Zone::Chest, chest, cfg.blast_damage * share * share * through));
                }
            }
            if distance < cfg.suppress_reach && self.clear_share(center, eye) > 0.0 {
                let amount = cfg.suppress * (1.0 - distance / cfg.suppress_reach.max(1e-3));
                self.soldiers[id].suppression.add(&self.suppression_cfg, amount, thrown.thrower);
            }
        }

        for (n, (id, _)) in near.iter().enumerate() {
            for &(zone, point, raw) in &damage[n] {
                let id = *id;
                if self.soldiers[id].health.is_dead() {
                    break;
                }
                self.health_events.clear();
                let result = self.soldiers[id].health.hit(&self.health_cfg, zone, raw, &mut self.health_events);
                self.soldiers[id].attacker = Some(thrown.thrower);
                self.events.push(SimEvent::SoldierHit { soldier: id, shooter: thrown.thrower, zone, point, damage: result.damage });
                self.interrupt_hands(id);
                self.interrupt_transit(id);
                self.settle_health(id);
            }
        }
        self.land_on_vehicles(thrown.thrower, &struck);
    }

    fn vehicle_strike_on(&self, v: usize, from: Vec3, dir: Vec3, length: f32) -> Option<prs_vehicles::Strike> {
        let vehicle = &self.vehicles[v];
        prs_vehicles::strike(vehicle, &self.vehicle_cfgs[vehicle.kind], &self.vehicle_doors[v], &vehicle.frame(), from, dir, length)
    }

    /// How much of a blast gets along a line: nothing through a wall, and what each panel lets through of the rest.
    fn clear_share(&self, from: Vec3, to: Vec3) -> f32 {
        let path = to - from;
        let length = path.length();
        if length < 1e-3 {
            return 1.0;
        }
        let dir = path / length;
        if self.world.raycast(from, dir, length).is_some() {
            return 0.0;
        }
        let (mut at, mut left, mut share) = (from, length, 1.0);
        for _ in 0..MAX_PANELS {
            let nearest = (0..self.vehicles.len())
                .filter_map(|v| self.vehicle_strike_on(v, at, dir, left))
                .min_by(|a, b| a.distance.total_cmp(&b.distance));
            let Some(found) = nearest else { break };
            share *= found.pass;
            let on = found.distance + self.world_cfg.panel_through;
            if share <= 0.0 || on >= left {
                break;
            }
            at += dir * on;
            left -= on;
        }
        share
    }

    /// One fragment, from the middle out: through what lets it through a little weaker, and stopped by the rest.
    fn fly_fragment(&mut self, center: Vec3, dir: Vec3, cfg: &FragConfig, near: &[(SoldierId, Pose)], damage: &mut [Vec<(Zone, Vec3, f32)>], struck: &mut Vec<Struck>) {
        let (mut from, mut travelled, mut power) = (center, 0.0, 1.0);
        for _ in 0..MAX_PANELS {
            let left = cfg.fragment_reach - travelled;
            if left <= 0.0 || power <= 0.0 {
                return;
            }
            let wall = self.world.raycast(from, dir, left).map_or(left, |hit| hit.distance);
            let mut bone_hit: Option<(usize, Bone, f32)> = None;
            for (n, (_, pose)) in near.iter().enumerate() {
                for bone in Bone::ALL {
                    let radius = bone_radius(&self.body_cfg, bone);
                    if let Some(d) = ray_vs_bone(from, dir, wall, pose[bone].start, pose[bone].end(), radius)
                        && bone_hit.is_none_or(|b| d < b.2)
                    {
                        bone_hit = Some((n, bone, d));
                    }
                }
            }
            let limit = bone_hit.map_or(wall, |b| b.2);
            let vehicle = (0..self.vehicles.len())
                .filter_map(|v| self.vehicle_strike_on(v, from, dir, limit).map(|found| (v, found)))
                .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance));
            let heli = self
                .helicopters
                .iter()
                .enumerate()
                .filter_map(|(h, heli)| prs_helicopter::hit_zone(&heli.state, &self.heli_cfg, from, dir, limit).map(|(zone, d)| (h, zone, d)))
                .min_by(|a, b| a.2.total_cmp(&b.2));
            let strength = |d: f32| {
                let share = (1.0 - (travelled + d) / cfg.fragment_reach.max(1e-3)).max(0.0);
                cfg.fragment_damage * share.powf(cfg.fragment_falloff) * power
            };
            let vehicle_at = vehicle.as_ref().map_or(f32::MAX, |v| v.1.distance);
            let heli_at = heli.map_or(f32::MAX, |h| h.2);
            let (at, pass) = if vehicle_at < heli_at && vehicle_at < limit {
                let (v, found) = vehicle.expect("nearest");
                struck.push(Struck::Vehicle(v, found.zone, from + dir * found.distance, found.normal, strength(found.distance) * cfg.vehicle_share));
                (found.distance, found.pass)
            } else if heli_at < limit {
                let (h, zone, d) = heli.expect("nearest");
                struck.push(Struck::Heli(h, zone, from + dir * d, strength(d) * cfg.vehicle_share));
                (d, prs_helicopter::damage::passes(&self.heli_cfg.damage, zone).unwrap_or(0.0))
            } else {
                if let Some((n, bone, d)) = bone_hit {
                    damage[n].push((zone_of(bone), from + dir * d, strength(d)));
                }
                return;
            };
            let on = at + self.world_cfg.panel_through;
            from += dir * on;
            travelled += on;
            power *= pass;
        }
    }

    /// Adds up what landed on each part of each vehicle, so that one grenade is one bang on it and not dozens.
    fn land_on_vehicles(&mut self, thrower: SoldierId, struck: &[Struck]) {
        let mut done: Vec<Struck> = Vec::new();
        for hit in struck {
            match (*hit, done.iter_mut().find(|d| d.same_part(hit))) {
                (Struck::Vehicle(.., raw), Some(Struck::Vehicle(.., total))) => *total += raw,
                (Struck::Heli(.., raw), Some(Struck::Heli(.., total))) => *total += raw,
                (first, _) => done.push(first),
            }
        }
        for hit in done {
            match hit {
                Struck::Vehicle(v, zone, point, normal, raw) => {
                    self.bullet_into_vehicle(v, zone, raw);
                    self.events.push(SimEvent::VehicleHit { vehicle: v, shooter: thrower, zone, point, normal });
                }
                Struck::Heli(h, zone, point, raw) => {
                    self.bullet_into_helicopter(h, zone, raw);
                    self.events.push(SimEvent::HeliHit { heli: h, shooter: thrower, zone, point });
                }
            }
        }
    }
}

/// A part of something big that a grenade got to, and how hard.
#[derive(Clone, Copy)]
enum Struck {
    Vehicle(usize, VehicleZone, Vec3, Vec3, f32),
    Heli(usize, HeliZone, Vec3, f32),
}

impl Struck {
    fn same_part(&self, other: &Struck) -> bool {
        match (self, other) {
            (Struck::Vehicle(a, x, ..), Struck::Vehicle(b, y, ..)) => a == b && x == y,
            (Struck::Heli(a, x, ..), Struck::Heli(b, y, ..)) => a == b && x == y,
            _ => false,
        }
    }
}
