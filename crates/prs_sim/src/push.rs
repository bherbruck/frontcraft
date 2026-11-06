//! Pushing a vehicle by hand. Hold F at one end or a corner of it and you lean in: a real force where your hands
//! are, through the vehicle's own body, so it rolls, or rocks off whatever it is stuck on, if there are enough of you.

use glam::{Vec2, Vec3};
use prs_movement::Stance;
use prs_protocol::Buttons;
use prs_vehicles::DriveInput;

use crate::{Sim, SoldierId, TICK_DT};

impl Sim {
    /// The spot on an upright vehicle they could put their hands to looking this way: which, and how squarely.
    pub(crate) fn push_in_reach(&self, id: SoldierId, look: Vec3) -> Option<(usize, u8, f32)> {
        let soldier = &self.soldiers[id];
        if soldier.seat.is_some() || soldier.body.stance == Stance::Prone || !soldier.health.is_up() || soldier.body.climb.active() {
            return None;
        }
        let eye = soldier.eye();
        let mut best: Option<(usize, u8, f32)> = None;
        for (v, vehicle) in self.vehicles.iter().enumerate() {
            let cfg = &self.vehicle_cfgs[vehicle.kind];
            if vehicle.damage.wrecked || (vehicle.rot * Vec3::Y).y < cfg.lift.start_below {
                continue;
            }
            for (s, spot) in cfg.push.spots.iter().enumerate() {
                let at = vehicle.pos + vehicle.rot * *spot;
                let facing = (at - eye).normalize_or(look).dot(look);
                if eye.distance(at) <= cfg.push.reach && facing >= cfg.push.facing && best.is_none_or(|(_, _, was)| facing > was) {
                    best = Some((v, s as u8, facing));
                }
            }
        }
        best
    }

    /// F at one: they lean in until something stops them.
    pub(crate) fn start_pushing(&mut self, id: SoldierId, v: usize, spot: u8) {
        self.interrupt_care(id);
        self.soldiers[id].pushing = Some((v as u32, spot));
    }

    /// Where a pusher's hands are on it, in the world.
    fn push_spot(&self, v: usize, spot: u8) -> Option<Vec3> {
        let vehicle = self.vehicles.get(v)?;
        let at = self.vehicle_cfgs[vehicle.kind].push.spots.get(spot as usize)?;
        Some(vehicle.pos + vehicle.rot * *at)
    }

    /// Whether they are still at it, and what it costs them. F let go, a jump, going prone or down, having nothing
    /// left, it getting away from them, or it coming at them, are all an end to it.
    pub(crate) fn step_push(&mut self, id: SoldierId, buttons: Buttons) {
        let Some((v, spot)) = self.soldiers[id].pushing else {
            return;
        };
        let (v, regen_delay) = (v as usize, self.move_cfg.stamina_regen_delay);
        let Some(at) = self.push_spot(v, spot) else {
            self.soldiers[id].pushing = None;
            return;
        };
        let vehicle = &self.vehicles[v];
        let cfg = &self.vehicle_cfgs[vehicle.kind];
        let soldier = &self.soldiers[id];
        let away = Vec2::new(at.x - soldier.body.pos.x, at.z - soldier.body.pos.z).normalize_or_zero();
        let coming = (vehicle.vel + vehicle.ang_vel.cross(at - vehicle.pos)).dot(-Vec3::new(away.x, 0.0, away.y));
        let going = -coming;
        let at_it = soldier.health.is_up()
            && buttons.contains(Buttons::USE)
            && !buttons.contains(Buttons::JUMP)
            && soldier.seat.is_none()
            && !soldier.body.climb.active()
            && soldier.body.stance != Stance::Prone
            && soldier.body.stamina > 0.0
            && !vehicle.damage.wrecked
            && (vehicle.rot * Vec3::Y).y >= cfg.lift.start_below
            && coming < cfg.push.stop_speed
            && going < cfg.push.keep_up * 1.2
            && soldier.eye().distance(at) <= cfg.push.hold_reach;
        let drain = cfg.push.stamina_drain;
        let soldier = &mut self.soldiers[id];
        if at_it {
            soldier.body.stamina = (soldier.body.stamina - drain * TICK_DT).max(0.0);
            soldier.body.stamina_delay = regen_delay;
        } else {
            soldier.pushing = None;
        }
    }

    /// Someone leaning into a vehicle goes with it, and their own feet go where they like as long as their hands still
    /// reach the spot, the furthest being nearly straight arms at its height. Nothing of theirs pushes into the hull,
    /// so a client a little behind the server never finds them somewhere else.
    pub(crate) fn hold_pusher(&mut self, id: SoldierId) {
        let Some((v, spot)) = self.soldiers[id].pushing else {
            return;
        };
        let Some(at) = self.push_spot(v as usize, spot) else {
            return;
        };
        let vehicle = &self.vehicles[v as usize];
        let cfg = &self.vehicle_cfgs[vehicle.kind].push;
        let carried = vehicle.vel + vehicle.ang_vel.cross(at - vehicle.pos);
        let pull = cfg.pull_rate;
        let (pose, bones) = (&self.body_cfg.lift, &self.body_cfg.bones);
        let shoulder = self.soldiers[id].body.pos.y + pose.push_eye - pose.shoulder_drop;
        let reach = (bones.upper_arm + bones.forearm + bones.hand) * pose.arm_bend;
        let level = (reach * reach - (shoulder - at.y).powi(2)).max(0.0).sqrt() + pose.shoulder_ahead;
        let eye = pose.push_eye;
        self.hold_within(id, at, level, pull, carried);
        self.soldiers[id].body.eye_height = eye;
    }

    /// Both hands flat on it at the spot, side by side across it.
    pub fn push_hands(&self, v: usize, spot: u8) -> Option<[Vec3; 2]> {
        let at = self.push_spot(v, spot)?;
        let across = self.vehicles[v].rot * Vec3::X * self.body_cfg.lift.apart;
        Some([at - across, at + across])
    }

    /// Everyone pushing a vehicle, as forces where their hands are. More than there is room for go in with the last.
    pub(crate) fn add_push(&self, v: usize, input: &mut DriveInput) -> bool {
        let cfg = &self.vehicle_cfgs[self.vehicles[v].kind].push;
        let mut n = 0;
        for soldier in &self.soldiers {
            let Some((on, spot)) = soldier.pushing.filter(|(on, _)| soldier.present && *on as usize == v) else { continue };
            let Some(at) = self.push_spot(on as usize, spot) else { continue };
            let away = Vec2::new(at.x - soldier.body.pos.x, at.z - soldier.body.pos.z).normalize_or_zero();
            // Nobody pushes harder than they can keep up with.
            let vehicle = &self.vehicles[v];
            let going = (vehicle.vel + vehicle.ang_vel.cross(at - vehicle.pos)).dot(Vec3::new(away.x, 0.0, away.y));
            let keeping_up = (1.0 - going / cfg.keep_up.max(1e-3)).clamp(0.0, 1.0);
            let force = Vec3::new(away.x, cfg.lift_share, away.y) * cfg.force * keeping_up;
            let slot = n.min(input.pushes.len() - 1);
            input.pushes[slot] = (input.pushes[slot].0 + force, at);
            n += 1;
        }
        n > 0
    }

    /// For someone else's client, which is told they are pushing and not what: the nearest spot on anything upright.
    pub fn pushed_near(&self, at: Vec3) -> Option<(u32, u8)> {
        let mut best = None;
        let mut nearest = f32::MAX;
        for (v, vehicle) in self.vehicles.iter().enumerate() {
            for (s, spot) in self.vehicle_cfgs[vehicle.kind].push.spots.iter().enumerate() {
                let d = (vehicle.pos + vehicle.rot * *spot).distance(at);
                if d < nearest {
                    nearest = d;
                    best = Some((v as u32, s as u8));
                }
            }
        }
        best
    }
}
