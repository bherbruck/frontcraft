//! Bullets against what is in the world: walls, range targets and soldiers. A soldier is hit on the
//! bones of the pose they are actually in, so someone lying down or behind a wall is hit exactly
//! where they look. Bones are worked out only for soldiers a bullet comes near.

use crate::*;
use prs_body::Bone;
use prs_collision::{ray_vs_bone, ray_vs_capsule};
use prs_vehicles::Strike;

/// Any bone of a soldier in any pose is within this of the middle of their standing height, in metres.
/// `every_bone_stays_within_the_reach_a_bullet_checks` holds this to the truth.
pub(crate) const BODY_REACH: f32 = 2.6;
pub(crate) const BODY_MIDDLE: f32 = 0.9;

/// Which part of the body a bone belongs to.
pub fn zone_of(bone: Bone) -> Zone {
    match bone {
        Bone::Head | Bone::Neck => Zone::Head,
        Bone::Chest | Bone::ShoulderL | Bone::ShoulderR => Zone::Chest,
        Bone::Pelvis | Bone::Spine => Zone::Abdomen,
        Bone::UpperArmL | Bone::ForearmL | Bone::HandL | Bone::UpperArmR | Bone::ForearmR | Bone::HandR => Zone::Arm,
        Bone::ThighL | Bone::ShinL | Bone::FootL | Bone::ThighR | Bone::ShinR | Bone::FootR => Zone::Leg,
    }
}

/// How thick a bone is to a bullet: halfway between its width and depth.
pub(crate) fn bone_radius(cfg: &BodyConfig, bone: Bone) -> f32 {
    let size = bone.size(cfg);
    (size.x + size.y) * 0.5
}

/// Whether a bullet's path comes close enough to a soldier to bother working out their bones.
fn passes_near(from: Vec3, dir: Vec3, length: f32, feet: Vec3) -> bool {
    let middle = feet + Vec3::Y * BODY_MIDDLE;
    let along = (middle - from).dot(dir).clamp(0.0, length);
    (from + dir * along).distance_squared(middle) < BODY_REACH * BODY_REACH
}

struct SoldierStrike {
    soldier: SoldierId,
    zone: Zone,
    distance: f32,
}

impl Sim {
    pub(crate) fn step_bullets(&mut self) {
        let mut poses = std::mem::take(&mut self.hit_poses);
        poses.clear();
        poses.resize(self.soldiers.len(), None);
        let mut i = 0;
        while i < self.bullets.len() {
            let (from, to) = self.bullets[i].bullet.advance(TICK_DT);
            let mut flying = self.bullets[i];
            let events_before = self.events.len();
            let hit = self.resolve_bullet_segment(from, to, &mut flying, &mut poses);
            let end = if hit { self.last_strike_point(events_before).unwrap_or(to) } else { to };
            self.bullet_near_misses(from, end, hit, &flying);
            if hit || flying.bullet.expired() {
                self.bullets.swap_remove(i);
            } else {
                self.bullets[i] = flying;
                i += 1;
            }
        }
        self.hit_poses = poses;
    }

    /// Where the bullet that just stopped did so, from what it left in the events.
    fn last_strike_point(&self, since: usize) -> Option<Vec3> {
        self.events[since..].iter().rev().find_map(|event| match *event {
            SimEvent::Impact { point, .. }
            | SimEvent::TargetHit { point, .. }
            | SimEvent::SoldierHit { point, .. }
            | SimEvent::VehicleHit { point, .. }
            | SimEvent::HeliHit { point, .. } => Some(point),
            _ => None,
        })
    }

    /// Rattles anyone this stretch of flight came close to, once per bullet: a soldier belongs to the
    /// stretch whose span they are level with, or to the last one if the bullet stopped short of them.
    /// Your own bullets do not count, and neither does anything still close to whoever fired it.
    fn bullet_near_misses(&mut self, from: Vec3, to: Vec3, stopped: bool, flying: &FlyingBullet) {
        let cfg = &self.suppression_cfg;
        let span = to - from;
        let length = span.length();
        if !cfg.enabled || length <= 0.0 {
            return;
        }
        let dir = span / length;
        let speed = flying.bullet.vel.length();
        let flown = flying.bullet.age * speed;
        for id in 0..self.soldiers.len() {
            let soldier = &self.soldiers[id];
            if id == flying.shooter || !soldier.present || soldier.health.is_dead() {
                continue;
            }
            let eye = soldier.eye();
            let along = (eye - from).dot(dir);
            if along < 0.0 || (along >= length && !stopped) {
                continue;
            }
            let along = along.min(length);
            if flown - (length - along) < cfg.min_flown {
                continue;
            }
            let point = from + dir * along;
            let distance = point.distance(eye);
            if distance >= cfg.reach {
                continue;
            }
            // From just off the point, as a bullet that stopped is touching whatever stopped it.
            let to_eye = (eye - point).normalize_or_zero();
            let clearance = 0.05_f32.min(distance * 0.5);
            let covered = self.world.raycast(point + to_eye * clearance, to_eye, (distance - clearance).max(0.0)).is_some();
            let round = self.weapons[flying.weapon].round_size;
            let amount = cfg.near_miss(distance, round, speed, covered);
            self.soldiers[id].suppression.add(cfg, amount, flying.shooter);
            self.events.push(SimEvent::BulletPassed {
                soldier: id,
                shooter: flying.shooter,
                point,
                weapon: flying.weapon,
                distance,
                cracked: speed >= cfg.crack_speed,
                amount,
            });
        }
    }

    /// The nearest bone of anyone this stretch of a bullet's flight touches. Your own bullet cannot
    /// find you on its way out of your own muzzle.
    fn strike_soldier(
        &self,
        from: Vec3,
        dir: Vec3,
        length: f32,
        flying: &FlyingBullet,
        poses: &mut [Option<Pose>],
    ) -> Option<SoldierStrike> {
        let mut nearest: Option<SoldierStrike> = None;
        for (id, soldier) in self.soldiers.iter().enumerate() {
            if !soldier.present || soldier.health.is_dead() {
                continue;
            }
            if id == flying.shooter && flying.bullet.age < self.health_cfg.self_hit_grace {
                continue;
            }
            // Everyone else is where the shooter saw them. You are always where you are.
            let seen = if id == flying.shooter { None } else { self.past.seen(id, self.tick, flying.rewind) };
            let feet = seen.map_or(soldier.body.pos, |input| input.feet);
            if !passes_near(from, dir, length, feet) {
                continue;
            }
            let past_pose;
            let pose = match seen {
                Some(input) => {
                    past_pose = self.body_pose(input);
                    &past_pose
                }
                None => &*poses[id].get_or_insert_with(|| self.body_pose(&self.body_input(id))),
            };
            for bone in Bone::ALL {
                let (a, b) = (pose[bone].start, pose[bone].end());
                let radius = bone_radius(&self.body_cfg, bone);
                if let Some(distance) = ray_vs_bone(from, dir, length, a, b, radius)
                    && nearest.as_ref().is_none_or(|n| distance < n.distance)
                {
                    nearest = Some(SoldierStrike { soldier: id, zone: zone_of(bone), distance });
                }
            }
        }
        nearest
    }

    /// The first part of any vehicle this stretch of flight touches, with vehicles where the shooter saw them.
    fn strike_vehicle(&self, from: Vec3, dir: Vec3, length: f32, flying: &FlyingBullet) -> Option<(usize, Strike)> {
        let mut nearest: Option<(usize, Strike)> = None;
        for (v, vehicle) in self.vehicles.iter().enumerate() {
            let frame = self.past.vehicle_seen(v, self.tick, flying.rewind).unwrap_or_else(|| vehicle.frame());
            if let Some(found) = prs_vehicles::strike(vehicle, &self.vehicle_cfgs[vehicle.kind], &self.vehicle_doors[v], &frame, from, dir, length)
                && nearest.as_ref().is_none_or(|n| found.distance < n.1.distance)
            {
                nearest = Some((v, found));
            }
        }
        nearest
    }

    /// Returns whether the bullet was stopped.
    fn resolve_bullet_segment(&mut self, from: Vec3, to: Vec3, flying: &mut FlyingBullet, poses: &mut [Option<Pose>]) -> bool {
        let span = to - from;
        let length = span.length();
        let dir = span / length;

        let wall = self.world.raycast(from, dir, length);
        let wall_distance = wall.map_or(f32::MAX, |w| w.distance);

        let target_hit = self
            .targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_up())
            .filter_map(|(id, t)| {
                ray_vs_capsule(from, dir, length, t.pos, TARGET_BODY).map(|d| (id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let target_distance = target_hit.map_or(f32::MAX, |t| t.1);
        let strike = self.strike_soldier(from, dir, length, flying, poses);
        let soldier_distance = strike.as_ref().map_or(f32::MAX, |s| s.distance);
        let vehicle = self.strike_vehicle(from, dir, length, flying);
        if let Some((v, found)) = vehicle
            && found.distance < wall_distance
            && found.distance < target_distance
            && found.distance < soldier_distance
        {
            let point = from + dir * found.distance;
            self.bullet_into_vehicle(v, found.zone, self.bullet_damage(flying));
            self.events.push(SimEvent::VehicleHit { vehicle: v, shooter: flying.shooter, zone: found.zone, point, normal: found.normal });
            if found.pass <= 0.0 {
                return true;
            }
            // Through the panel, slower, and on from its far side to whatever else is on the way.
            flying.bullet.vel *= found.pass;
            let on = found.distance + self.world_cfg.panel_through;
            return on < length && self.resolve_bullet_segment(from + dir * on, to, flying, poses);
        }

        if let Some((h, zone, distance)) = self.strike_helicopter(from, dir, length, flying)
            && distance < wall_distance
            && distance < target_distance
            && distance < soldier_distance
        {
            let point = from + dir * distance;
            self.bullet_into_helicopter(h, zone, self.bullet_damage(flying));
            self.events.push(SimEvent::HeliHit { heli: h, shooter: flying.shooter, zone, point });
            let Some(pass) = prs_helicopter::damage::passes(&self.heli_cfg.damage, zone) else {
                return true;
            };
            flying.bullet.vel *= pass;
            let on = distance + self.world_cfg.panel_through;
            return on < length && self.resolve_bullet_segment(from + dir * on, to, flying, poses);
        }

        if soldier_distance < wall_distance && soldier_distance < target_distance {
            let strike = strike.expect("nothing else is nearer");
            self.hit_soldier(strike, from + dir * soldier_distance, &*flying);
            return true;
        }
        match target_hit {
            Some((id, distance)) if distance < wall_distance => {
                let point = from + dir * distance;
                let target = &mut self.targets[id];
                let in_room = self.level.target_mark(id).in_room;
                target.down_for = if in_room { self.world_cfg.room_target_down_time } else { self.world_cfg.target_down_time };
                target.hits += 1;
                let headshot = point.y - target.pos.y > self.world_cfg.target_head_height;
                self.events.push(SimEvent::TargetHit { target: id, point, headshot });
                true
            }
            _ => {
                if let Some(wall) = wall {
                    self.events.push(SimEvent::Impact {
                        point: from + dir * wall.distance,
                        normal: wall.normal,
                        surface: wall.surface,
                    });
                    true
                } else {
                    false
                }
            }
        }
    }

    /// What a bullet does on arrival, by how much speed it has left.
    fn bullet_damage(&self, flying: &FlyingBullet) -> f32 {
        let weapon = &self.weapons[flying.weapon];
        let left = flying.bullet.vel.length() / weapon.ballistics.muzzle_velocity.max(1.0);
        weapon.damage * left.powf(weapon.damage_falloff)
    }

    fn hit_soldier(&mut self, strike: SoldierStrike, point: Vec3, flying: &FlyingBullet) {
        let raw = self.bullet_damage(flying);
        let id = strike.soldier;
        let soldier = &mut self.soldiers[id];
        soldier.attacker = Some(flying.shooter);
        self.health_events.clear();
        let result = soldier.health.hit(&self.health_cfg, strike.zone, raw, &mut self.health_events);
        self.events.push(SimEvent::SoldierHit {
            soldier: id,
            shooter: flying.shooter,
            zone: strike.zone,
            point,
            damage: result.damage,
        });
        // Being hit breaks your concentration on whoever you were treating, and stops you in a doorway.
        self.interrupt_hands(id);
        self.interrupt_transit(id);
        self.settle_health(id);
    }
}
