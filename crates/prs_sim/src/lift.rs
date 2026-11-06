//! Righting a vehicle by hand. Hold F with your hands on the hull and it rolls, as a real body against the ground
//! and whatever else is in the way, for as long as somebody keeps at it.

use glam::{Vec2, Vec3};
use prs_movement::Stance;
use prs_protocol::Buttons;
use prs_vehicles::{DriveInput, VehicleEvent, VehicleState};

use crate::{Sim, SoldierId, TICK_DT, view_dir};

/// The nearest point of a hull to somewhere.
fn hull_point(vehicle: &VehicleState, hulls: &[prs_vehicles::HullBox], at: Vec3) -> Vec3 {
    let mut best = Vec3::ZERO;
    let mut nearest = f32::MAX;
    let from = vehicle.rot.inverse() * (at - vehicle.pos);
    for hull in hulls {
        let local = (from - hull.center).clamp(-hull.half, hull.half) + hull.center;
        let d = local.distance_squared(from);
        if d < nearest {
            nearest = d;
            best = local;
        }
    }
    vehicle.pos + vehicle.rot * best
}

impl Sim {
    fn up_of(&self, v: usize) -> f32 {
        (self.vehicles[v].rot * Vec3::Y).y
    }

    /// The nearest of a vehicle's hull to here.
    pub fn nearest_of_hull(&self, v: usize, at: Vec3) -> Vec3 {
        hull_point(&self.vehicles[v], &self.vehicle_cfgs[self.vehicles[v].kind].hull, at)
    }

    /// The vehicle on its side or roof that they could put their hands to looking this way, and how squarely.
    pub(crate) fn lift_in_reach(&self, id: SoldierId, look: Vec3) -> Option<(usize, f32)> {
        let soldier = &self.soldiers[id];
        if soldier.seat.is_some() || soldier.body.stance == Stance::Prone || !soldier.health.is_up() {
            return None;
        }
        let eye = soldier.eye();
        let mut best: Option<(usize, f32)> = None;
        for v in 0..self.vehicles.len() {
            let cfg = &self.vehicle_cfgs[self.vehicles[v].kind];
            if self.vehicles[v].damage.wrecked || self.up_of(v) >= cfg.lift.start_below {
                continue;
            }
            let point = self.nearest_of_hull(v, eye);
            let facing = (point - eye).normalize_or(look).dot(look);
            if eye.distance(point) <= cfg.lift.reach && facing >= cfg.lift.facing && best.is_none_or(|(_, was)| facing > was) {
                best = Some((v, facing));
            }
        }
        best
    }

    /// F on one: they have their hands on it until something stops them.
    pub(crate) fn start_lifting(&mut self, id: SoldierId, v: usize) {
        if self.soldiers[id].lift_refused > 0.0 {
            return;
        }
        self.interrupt_care(id);
        self.soldiers[id].lifting = Some(v as u32);
    }

    /// Whether they are still at it, and what it costs them. F let go, a jump, having gone down, nothing left, or it
    /// having got away from them, are all an end to it. Moving is not: they are held within reach of it.
    pub(crate) fn step_lift(&mut self, id: SoldierId, buttons: Buttons) {
        let soldier = &mut self.soldiers[id];
        soldier.lift_refused = (soldier.lift_refused - TICK_DT).max(0.0);
        let Some(v) = soldier.lifting.map(|v| v as usize) else {
            return;
        };
        let Some(cfg) = self.vehicles.get(v).map(|vehicle| self.vehicle_cfgs[vehicle.kind].lift) else {
            soldier.lifting = None;
            return;
        };
        let soldier = &self.soldiers[id];
        let at_it = soldier.health.is_up()
            && buttons.contains(Buttons::USE)
            && !buttons.contains(Buttons::JUMP)
            && soldier.seat.is_none()
            && !soldier.body.climb.active()
            && soldier.body.stance != Stance::Prone
            && soldier.body.stamina > 0.0
            && !self.vehicles[v].damage.wrecked
            && self.up_of(v) < cfg.done_above
            && soldier.eye().distance(self.nearest_of_hull(v, soldier.eye())) <= cfg.hold_reach;
        let regen_delay = self.move_cfg.stamina_regen_delay;
        let soldier = &mut self.soldiers[id];
        if at_it {
            soldier.body.stamina = (soldier.body.stamina - cfg.stamina_drain * TICK_DT).max(0.0);
            soldier.body.stamina_delay = regen_delay;
        } else {
            soldier.lifting = None;
        }
    }

    /// Those on a vehicle and where they are, on average.
    fn lifters_of(&self, v: usize) -> (u32, Vec3) {
        let (mut count, mut sum) = (0, Vec3::ZERO);
        for soldier in &self.soldiers {
            if soldier.present && soldier.lifting == Some(v as u32) {
                count += 1;
                sum += soldier.body.pos;
            }
        }
        (count, sum / count.max(1) as f32)
    }

    /// What those lifting a vehicle add to what its driver is asking of it, and whether there is enough of them.
    pub(crate) fn add_lift(&self, v: usize, input: &mut DriveInput) -> bool {
        let (count, from) = self.lifters_of(v);
        let cfg = &self.vehicle_cfgs[self.vehicles[v].kind].lift;
        if count < cfg.crew.max(1) || self.up_of(v) >= cfg.done_above {
            return false;
        }
        input.lift = cfg.rate_per_soldier * count as f32;
        input.lift_from = from;
        true
    }

    /// Wherever their own feet took them, still within reach of the hull, the way a rested gun stops at the end of
    /// its arc and does not come off.
    pub(crate) fn hold_lifter(&mut self, id: SoldierId) {
        let Some(v) = self.soldiers[id].lifting.map(|v| v as usize).filter(|&v| v < self.vehicles.len()) else {
            return;
        };
        let cfg = self.vehicle_cfgs[self.vehicles[v].kind].lift;
        let anchor = self.nearest_of_hull(v, self.soldiers[id].body.pos + Vec3::Y * 0.5);
        self.hold_within(id, anchor, cfg.hold_within, cfg.pull_rate, Vec3::ZERO);
    }

    /// Keeps someone's feet no further across the ground from `anchor` than `reach`, after their own moving and
    /// whatever has carried them along. Past it, they are drawn back at `pull` m/s and kept from going further
    /// out, but never let go of: the same as a rested gun at the end of its arc.
    pub(crate) fn hold_within(&mut self, id: SoldierId, anchor: Vec3, reach: f32, pull: f32, carried: Vec3) {
        let body = &self.soldiers[id].body;
        let moved = Vec2::new(body.pos.x + carried.x * TICK_DT, body.pos.z + carried.z * TICK_DT);
        let from = Vec2::new(anchor.x, anchor.z);
        let out = moved - from;
        let past = out.length() - reach;
        let held = if past > 0.0 { from + out.normalize_or_zero() * (reach + (past - pull * TICK_DT).max(0.0)) } else { moved };
        let top = Vec3::new(held.x, body.pos.y + 1.0, held.y);
        let ground = self.world.raycast(top, Vec3::NEG_Y, 3.0).map_or(body.pos.y, |hit| top.y - hit.distance);
        let body = &mut self.soldiers[id].body;
        if past > 0.0 {
            // Nothing more of their own outwards; whatever they were doing along the edge goes on.
            let away = out.normalize_or_zero();
            let outward = Vec2::new(body.vel.x, body.vel.z).dot(away).max(0.0);
            body.vel -= Vec3::new(away.x, 0.0, away.y) * outward;
        }
        body.pos = Vec3::new(held.x, if carried == Vec3::ZERO { body.pos.y } else { ground }, held.y);
    }

    /// Before a vehicle moves: one being worked on is not to be written off, and it groans.
    pub(crate) fn before_lift(&mut self, v: usize, lifted: bool) {
        if !lifted {
            return;
        }
        self.vehicles[v].abandoned_for = 0.0;
        if self.vehicle_cfgs[self.vehicles[v].kind].lift.start_below <= (self.vehicles[v].rot * Vec3::Y).y {
            return;
        }
        let every = (self.vehicle_cfgs[self.vehicles[v].kind].lift.groan_every / TICK_DT).max(1.0) as u32;
        if self.tick % every == 0 {
            let event = VehicleEvent::Heave { point: self.vehicles[v].pos };
            self.events.push(crate::SimEvent::Vehicle { vehicle: v, event });
        }
    }

    /// It has stopped dead under them. They let go, and the prompt says why.
    pub(crate) fn lift_blocked(&mut self, v: usize) {
        let refused = self.vehicle_cfgs[self.vehicles[v].kind].lift.refused_for;
        for soldier in &mut self.soldiers {
            if soldier.lifting == Some(v as u32) {
                soldier.lifting = None;
                soldier.lift_refused = refused;
            }
        }
    }

    /// Where the hands go, left then right: on the hull, in front of the shoulders.
    pub fn lift_hands(&self, id: SoldierId, v: usize) -> [Vec3; 2] {
        let soldier = &self.soldiers[id];
        let c = &self.body_cfg.lift;
        let forward = view_dir(soldier.yaw, 0.0);
        let right = forward.cross(Vec3::Y);
        let chest = soldier.eye() - Vec3::Y * c.chest_below_eye + forward * c.ahead;
        [-1.0, 1.0].map(|side| self.nearest_of_hull(v, chest + right * side * c.apart))
    }

    /// For someone else's client, which is told they are lifting and not what: whatever is over that is nearest.
    pub fn lifted_near(&self, at: Vec3) -> Option<u32> {
        (0..self.vehicles.len())
            .filter(|&v| self.up_of(v) < self.vehicle_cfgs[self.vehicles[v].kind].lift.done_above)
            .min_by(|&a, &b| {
                let distance = |v: usize| self.nearest_of_hull(v, at).distance_squared(at);
                distance(a).total_cmp(&distance(b))
            })
            .map(|v| v as u32)
    }
}
