//! Soldiers and each other: bumping, and standing on someone's back.
//!
//! This is the one place that knows both soldiers and what they feel of each other. Each tick starts by
//! taking down where everyone is, rounded to the grid they go on the wire by, and that is all anyone
//! reads for the rest of the tick. So it does not matter who steps first, and a client looking at a
//! soldier standing still gets the very same numbers as the server.
//!
//! The list is kept sorted along one axis, and a soldier looks only at the slice of it within reach.
//! On a map where a few dozen soldiers are spread out, that is hardly ever anyone.

use glam::{Vec2, Vec3};
use prs_protocol::TICK_DT;
use prs_movement::{ClimbKind, Crowding, MoveConfig, MoveInput, Neighbour, Stance, StoolOffer, abort_climb, plan_climb, yaw_basis};

use crate::{Sim, Soldier, SoldierId};

#[derive(Clone, Copy, Debug)]
struct Entry {
    id: SoldierId,
    feet: Vec3,
    vel: Vec2,
    height: f32,
    team: u8,
}

#[derive(Default)]
pub(crate) struct Crowd {
    /// Everyone who takes up room, by their `x`.
    bodies: Vec<Entry>,
    /// Who has someone on their back.
    carrying: Vec<bool>,
}

fn on_grid(at: Vec3, cfg: &MoveConfig) -> Vec3 {
    (at * cfg.stool.grid).round() / cfg.stool.grid
}

/// Whether two soldiers with this between them are the same one being climbed on and the one climbing.
fn mounted_on(soldier: &Soldier, other: SoldierId) -> bool {
    soldier.body.climb.on.or(soldier.body.perch.on) == Some(other as u32)
}

impl Crowd {
    /// `ahead` is the seconds to carry anyone moving on by, for a client seeing them in the past.
    fn take_down(&mut self, soldiers: &[Soldier], cfg: &MoveConfig, ahead: f32) {
        self.bodies.clear();
        self.carrying.clear();
        self.carrying.resize(soldiers.len(), false);
        for (id, soldier) in soldiers.iter().enumerate() {
            if !soldier.present {
                continue;
            }
            if let Some(on) = soldier.body.climb.on.or(soldier.body.perch.on)
                && let Some(carrying) = self.carrying.get_mut(on as usize)
            {
                *carrying = true;
            }
            // Someone lying down can be stepped over, like anything else that low.
            if soldier.seat.is_some() || !soldier.health.is_up() || soldier.body.stance == Stance::Prone {
                continue;
            }
            let height = cfg.height(soldier.body.stance);
            let vel = soldier.body.vel;
            let feet = on_grid(soldier.body.pos + Vec3::new(vel.x, 0.0, vel.z) * ahead, cfg);
            self.bodies.push(Entry { id, feet, vel: Vec2::new(vel.x, vel.z), height, team: soldier.team });
        }
        self.bodies.sort_by(|a, b| a.feet.x.total_cmp(&b.feet.x));
    }

    fn near(&self, x: f32, reach: f32) -> &[Entry] {
        let from = self.bodies.partition_point(|e| e.feet.x < x - reach);
        let to = self.bodies.partition_point(|e| e.feet.x <= x + reach);
        &self.bodies[from..to]
    }
}

impl Sim {
    /// Whether someone is on this soldier's back, or on their way up there, as of this tick.
    pub fn is_carrying(&self, id: SoldierId) -> bool {
        self.crowd.carrying.get(id).copied().unwrap_or(false)
    }

    pub(crate) fn take_down_crowd(&mut self, tick: u32) {
        let ahead = self.others_seen_at.map_or(0.0, |seen| (tick as f32 - seen).max(0.0) * TICK_DT);
        self.crowd.take_down(&self.soldiers, &self.move_cfg, ahead.min(self.move_cfg.crowd.look_ahead));
    }

    /// What everyone nearby does to this soldier this tick. Someone with their hands on a ledge, on a
    /// back, or with someone on theirs, is committed and does not get moved.
    pub(crate) fn crowding_of(&self, id: SoldierId) -> Crowding {
        let cfg = &self.move_cfg;
        let soldier = &self.soldiers[id];
        let body = &soldier.body;
        let mut crowding = Crowding::default();
        if body.climb.active() || body.perch.on.is_some() || body.stance == Stance::Prone || self.is_carrying(id) {
            return crowding;
        }
        for other in self.crowd.near(body.pos.x, cfg.crowd.contact) {
            if other.id == id || mounted_on(&self.soldiers[other.id], id) {
                continue;
            }
            let aside = if id < other.id { Vec2::X } else { Vec2::NEG_X };
            let neighbour = Neighbour { feet: other.feet, vel: other.vel, height: other.height, friend: other.team == soldier.team, aside };
            crowding.add(&cfg.crowd, body.pos, cfg.height(body.stance), body.horizontal_speed(), &neighbour, cfg.sprint_speed);
        }
        crowding
    }

    /// Whether this soldier can hold up a rider. Read off what goes on the wire, so a client gets the
    /// same answer about someone as the server does, once it has seen them do it.
    fn can_hold_up(&self, id: SoldierId) -> bool {
        let cfg = &self.move_cfg;
        let soldier = &self.soldiers[id];
        let body = &soldier.body;
        soldier.present
            && soldier.seat.is_none()
            && soldier.health.is_up()
            && body.stance == Stance::Crouch
            && (body.eye_height - cfg.crouch_eye).abs() <= cfg.stool.settled
            && body.grounded
            && !body.climb.active()
            && body.perch.on.is_none()
    }

    /// The crouched soldier straight ahead to climb on, if there is one and it would be allowed. Nobody
    /// on someone's back gets to climb on a third. Theirs is a place on the wire's grid, so a client
    /// looking at someone keeping still comes to exactly the server's answer.
    pub(crate) fn stool_ahead(&self, id: SoldierId, yaw: f32) -> Option<StoolOffer> {
        let cfg = &self.move_cfg;
        let s = &cfg.stool;
        let body = &self.soldiers[id].body;
        if body.perch.on.is_some() || body.climb.active() {
            return None;
        }
        let (forward, _) = yaw_basis(yaw);
        let forward = Vec2::new(forward.x, forward.z);
        let mut best: Option<(f32, SoldierId)> = None;
        for other in self.crowd.near(body.pos.x, s.reach) {
            if other.id == id || self.is_carrying(other.id) || !self.can_hold_up(other.id) {
                continue;
            }
            let to = Vec2::new(other.feet.x - body.pos.x, other.feet.z - body.pos.z);
            let distance = to.length();
            let ahead = to.dot(forward) >= s.cone_cos * distance;
            if distance > s.reach || !ahead || (other.feet.y - body.pos.y).abs() > s.level {
                continue;
            }
            if best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, other.id));
            }
        }
        let (_, on) = best?;
        Some(StoolOffer { on: on as u32, spot: self.spot_on(on) })
    }

    /// Where feet go on this soldier's back. On the grid, so it comes out the same on both sides.
    fn spot_on(&self, on: SoldierId) -> Vec3 {
        let cfg = &self.move_cfg;
        let stool = &self.soldiers[on];
        let (forward, _) = yaw_basis(stool.anim.hip_yaw);
        on_grid(on_grid(stool.body.pos, cfg) - forward * cfg.stool.back_offset + Vec3::Y * cfg.stool.back_height, cfg)
    }

    /// Whoever is on someone's back, or climbing up there, is let go as soon as that person moves. Not
    /// thrown: they slide off away from them, at no great speed, and drop.
    pub(crate) fn check_mount(&mut self, id: SoldierId) {
        let cfg = &self.move_cfg;
        let body = &self.soldiers[id].body;
        let Some(on) = body.climb.on.or(body.perch.on).map(|on| on as SoldierId) else {
            return;
        };
        let fine = on < self.soldiers.len() && on != id && self.can_hold_up(on) && {
            let spot = if body.climb.active() { body.climb.to } else { body.perch.spot };
            (spot - self.spot_on(on)).length() <= cfg.stool.slack
        };
        let seated = self.soldiers[id].seat.is_some();
        if fine && !seated && self.soldiers[id].health.is_up() {
            return;
        }
        let shrug = cfg.stool.shrug_speed;
        let from = self.soldiers.get(on).map_or(body.pos, |stool| stool.body.pos);
        let body = &mut self.soldiers[id].body;
        let climbing = body.climb.active();
        abort_climb(body, &self.move_cfg, &self.world);
        if !climbing && !seated {
            let away = Vec2::new(body.pos.x - from.x, body.pos.z - from.z).normalize_or(Vec2::X) * shrug;
            body.vel = Vec3::new(away.x, 0.0, away.y);
        }
    }

    /// Back on the land, or in a seat, from wherever a boost got them to.
    pub(crate) fn check_boosted(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        if !soldier.body.boosted {
            return;
        }
        let body = &soldier.body;
        let land = self.world.land_height(body.pos.x, body.pos.z);
        if soldier.seat.is_some() || (body.grounded && land.is_some_and(|land| body.pos.y - land <= self.move_cfg.stool.on_the_land)) {
            self.soldiers[id].body.boosted = false;
        }
    }

    /// The climb a soldier would get by pressing jump now, walking forward with this look. It costs a few
    /// rays. For a bot wondering what it could do with a bit of cover, or anyone else who wants to ask.
    pub fn climb_ahead(&self, id: SoldierId, yaw: f32) -> Option<ClimbKind> {
        let soldier = &self.soldiers[id];
        if soldier.seat.is_some() || !soldier.health.is_up() {
            return None;
        }
        let input = MoveInput {
            yaw,
            hurt: self.climb_hurt(soldier),
            burden: self.burden_of(soldier),
            stool: self.stool_ahead(id, yaw),
            ..MoveInput::default()
        };
        plan_climb(&soldier.body, &self.move_cfg, &input, &self.world).map(|climb| climb.kind)
    }

    /// The climb pressing jump with W held would start right now, looking the way they are, or None if it
    /// would start nothing. It asks everything the press does, cooldown and hands and all, of the same
    /// planner, so anything that shows it to a player is never wrong about it.
    pub fn climb_on_offer(&self, id: SoldierId) -> Option<prs_movement::Climb> {
        self.climb_on_offer_looking(id, self.soldiers.get(id)?.yaw)
    }

    /// The same as [`Self::climb_on_offer`] for some other way of looking, such as one a bot is about to send.
    pub fn climb_on_offer_looking(&self, id: SoldierId, yaw: f32) -> Option<prs_movement::Climb> {
        let soldier = self.soldiers.get(id)?;
        let body = &soldier.body;
        let footing = body.grounded || body.coyote > 0.0 || body.perch.on.is_some();
        let ready = soldier.present
            && soldier.seat.is_none()
            && soldier.transit.is_none()
            && soldier.health.is_up()
            && footing
            && !body.climb.active()
            && body.stance != Stance::Prone
            && body.stance_timer == 0.0
            && body.climb_cooldown == 0.0
            && body.jump_cooldown == 0.0
            && soldier.weapon.reload_timer <= 0.0
            && soldier.treatment.is_none();
        if !ready {
            return None;
        }
        self.climb_plan_looking(id, yaw)
    }

    /// The climb the planner would give this soldier looking this way, before asking whether they are in
    /// any state to start it: in the air on the way up to something, say.
    pub fn climb_plan_looking(&self, id: SoldierId, yaw: f32) -> Option<prs_movement::Climb> {
        let soldier = self.soldiers.get(id)?;
        let input = MoveInput {
            yaw,
            hurt: self.climb_hurt(soldier),
            burden: self.burden_of(soldier),
            stool: self.stool_ahead(id, yaw),
            ..MoveInput::default()
        };
        plan_climb(&soldier.body, &self.move_cfg, &input, &self.world)
    }

    /// How much wounds take off climbing, 0 to 1.
    pub(crate) fn climb_hurt(&self, soldier: &Soldier) -> f32 {
        soldier.health.leg_wound.max(soldier.health.arm_wound * self.move_cfg.climb.arm_share)
    }

    pub(crate) fn burden_of(&self, soldier: &Soldier) -> f32 {
        prs_inventory::encumbrance(soldier.load(&self.inventory_cfg, &self.weapons), &self.inventory_cfg).0
    }
}
