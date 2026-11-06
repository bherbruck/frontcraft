//! Resting a gun on something. C with the fore-end over or against something solid sets it down there,
//! and from then on that point stays where it is and the look swings the barrel about it.
//!
//! `gun_pose` is the same as ever, going out from the eye to the sights, so the sights stay on the eye and
//! nothing about a shot changes. What is different is where the eye is: it goes wherever puts the fore-end
//! just over the contact point, as the head follows the stock round the rest. Aiming up over the rest
//! takes the body down and aiming down over it brings the body up, so while rested a soldier's height is
//! whatever that comes to, and their stance only says which way they go when they come off. The eye and
//! the height are as real as any other, so a capsule and hit zones go with them and it is the same for
//! everybody watching.
//!
//! So the swing stops for what really stops it: the arc, how high and low the body will go on the ground
//! under it, and the barrel having to point clear of whatever it is resting on, all the way along. Sights
//! sit above a bore, so a line of sight just clearing a surface is a bullet into it. [`Sim::rest_arc`] says
//! where all that comes to, and it is the same sum for the client's mouse as for the sim's rule.
//!
//! What it buys comes through the same sway, wander and kick as everything else, and only once it has
//! settled. Firing knocks that back, so it is for picking off one target and not for holding the trigger.

use glam::{Vec2, Vec3};
use prs_collision::Capsule;
use prs_movement::{MoveConfig, Stance, held_capsule};
use prs_protocol::TICK_DT;
use prs_weapons::{RestArc, RestConfig, WeaponConfig, pitch_for_eye, rested_eye};

use crate::{CarryEvent, Sim, SimEvent, SoldierId};

/// Every field is public: a client has to restore all of it to replay from what the server says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GunRest {
    pub on: bool,
    /// The contact point, which does not move, and how far along the gun from the receiver it touches.
    pub pivot: Vec3,
    pub along: f32,
    /// Which way it was pointing when it went down. The arc is measured from here.
    pub yaw: f32,
    pub pitch: f32,
    /// Where the soldier was standing when it went down.
    pub feet: Vec3,
    /// How far over to the rest they are, 0 to 1, and how far it has settled, 0 to 1.
    pub amount: f32,
    pub settle: f32,
    /// From the eye they would otherwise have to the eye they do have, along the ground: right now and
    /// all the way over. Height is in `body.eye_height`.
    pub eye: Vec3,
    pub reach: Vec3,
    /// How high the eye is over the feet all the way over, and how high and low the body will have it here.
    pub height: f32,
    pub low: f32,
    pub high: f32,
    /// Seconds left of saying there was nothing to rest on.
    pub refused: f32,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A place to set the gun down, and how far along the gun it touches.
struct Found {
    pivot: Vec3,
    along: f32,
}

/// Which stance a height is nearest to, of those a soldier who is not lying down can be in.
fn nearest_stance(cfg: &MoveConfig, eye: f32) -> Stance {
    if eye > (cfg.stand_eye + cfg.crouch_eye) * 0.5 { Stance::Stand } else { Stance::Crouch }
}

impl Sim {
    /// Whether the eye can get from where it would be to here with a little room around it.
    fn eye_fits(&self, upright: Vec3, reach: Vec3, c: &RestConfig) -> bool {
        let length = reach.length();
        let ball = Capsule { height: c.eye_clearance * 2.0, radius: c.eye_clearance };
        length < 1e-4 || self.world.cast_capsule(upright - Vec3::Y * c.eye_clearance, ball, reach / length, length).is_none()
    }

    /// How high and low the body will have the eye, standing where it is. Not above standing tall and not
    /// down past a kneel, or a low crawl for someone already lying down. Something under a knee stops it
    /// going so low, and a ceiling stops it rising. None if there is no room for even the lowest of it.
    fn body_range(&self, id: SoldierId, c: &RestConfig) -> Option<(f32, f32)> {
        let m = &self.move_cfg;
        let body = &self.soldiers[id].body;
        let (mut low, high) = if body.stance == Stance::Prone { (m.prone_eye, c.prone_high_eye) } else { (c.low_eye, m.stand_eye) };
        if body.stance != Stance::Prone {
            let start = c.knee_reach + c.knee_rise;
            for side in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                let at = body.pos + side * c.knee_reach + Vec3::Y * start;
                if let Some(hit) = self.world.walk_ray(at, Vec3::NEG_Y, start + c.knee_rise)
                    && hit.distance > 0.0
                {
                    low = low.max(c.low_eye + (start - hit.distance - c.knee_reach).max(0.0));
                }
            }
        }
        let lowest = held_capsule(m, low);
        if !self.world.capsule_is_clear(body.pos, lowest) {
            return None;
        }
        let room = self.world.cast_capsule(body.pos, lowest, Vec3::Y, (high - low).max(0.0));
        let high = room.map_or(high, |hit| (low + hit.distance - c.head_room).max(low));
        Some((low, high))
    }

    /// The lowest the barrel can point at this yaw and clear what it is resting on, all the way along to
    /// where that ends. Anything rising past `clear_rise` is no part of the rest, and is only something
    /// in the way. It is for the bore and not the sights, and with a little over for the gun's own wander.
    fn support_floor(&self, c: &RestConfig, pivot: Vec3, yaw: f32) -> f32 {
        let bore = pivot.y + c.clearance + c.under;
        let forward = prs_movement::yaw_basis(yaw).0;
        let top = pivot.y + c.clear_rise;
        let mut floor = -std::f32::consts::FRAC_PI_2;
        let step = c.clear_step.max(0.02);
        let surface = |along: f32| {
            let at = pivot + forward * along;
            let hit = self.world.walk_ray(Vec3::new(at.x, top, at.z), Vec3::NEG_Y, c.clear_rise + c.clear_drop)?;
            (hit.distance > 0.0).then_some(top - hit.distance)
        };
        let mut limit = |along: f32, height: f32| floor = floor.max(((height + c.clear_margin - bore) / along).atan());
        let (mut along, mut last) = (step, 0.0);
        while along <= c.clear_reach {
            let Some(height) = surface(along) else {
                // Run in on where it ends, or the limit would jump as that went by a step at a time.
                let (mut on, mut off) = (last, along);
                for _ in 0..4 {
                    let middle = (on + off) * 0.5;
                    match surface(middle) {
                        Some(height) => {
                            limit(middle, height);
                            on = middle;
                        }
                        None => off = middle,
                    }
                }
                break;
            };
            limit(along, height);
            last = along;
            along += step;
        }
        floor + c.clear_wander
    }

    /// Where the look can go on this soldier's rest with this yaw, for anyone who wants to stop at the end
    /// of it rather than go past it. None if the gun is not down.
    pub fn rest_arc(&self, id: SoldierId, yaw: f32) -> Option<RestArc> {
        let soldier = self.soldiers.get(id)?;
        let rest = &soldier.rest;
        if !rest.on {
            return None;
        }
        Some(self.arc_of(id, soldier.held_config(&self.weapons), rest, yaw))
    }

    fn arc_of(&self, id: SoldierId, cfg: &WeaponConfig, rest: &GunRest, yaw: f32) -> RestArc {
        let c = &cfg.rest;
        let soldier = &self.soldiers[id];
        let feet = soldier.body.pos.y;
        let raised = rest.pivot.y + c.clearance;
        let ads = soldier.weapon.ads;
        // Lower eyes go with higher aim.
        let body_low = pitch_for_eye(cfg, raised, rest.along, ads, feet + rest.high);
        let body_high = pitch_for_eye(cfg, raised, rest.along, ads, feet + rest.low);
        let floor = self.support_floor(c, rest.pivot, yaw);
        let pitch_high = (rest.pitch + c.arc_up).min(body_high);
        let pitch_low = (rest.pitch - c.arc_down).max(body_low).max(floor).min(pitch_high);
        RestArc { yaw_low: rest.yaw - c.arc_yaw, yaw_high: rest.yaw + c.arc_yaw, pitch_low, pitch_high }
    }

    /// Looks under where the gun could be, at the shoulder, anywhere from as high as the body goes to as
    /// low as it goes, from the front of where it can rest to the back. That is from the gun's real place
    /// in the world, with no account of a wall having pushed it back, and at the shoulder so that raising
    /// the sights afterwards leaves the head where it would have been.
    fn find_rest(&self, id: SoldierId, cfg: &WeaponConfig, yaw: f32, low: f32, high: f32) -> Option<Found> {
        let soldier = &self.soldiers[id];
        let c = &cfg.rest;
        let h = &cfg.hold;
        let upright = soldier.body.eye();
        let forward = prs_movement::yaw_basis(yaw).0;
        let feet = soldier.body.pos.y;
        let top = feet + high + h.ads.y + c.above;
        let bottom = feet + low + h.ads.y - c.below;
        let count = c.samples.max(2);
        (0..count).find_map(|i| {
            let along = (c.far + (c.near - c.far) * i as f32 / (count - 1) as f32) * h.muzzle_forward;
            let start = Vec3::new(upright.x, top, upright.z) + forward * (-h.ads.z + along);
            let hit = self.world.walk_ray(start, Vec3::NEG_Y, (top - bottom).max(0.01))?;
            if hit.distance <= 0.0 || hit.normal.y < c.flat_cos {
                return None;
            }
            let pivot = start - Vec3::Y * hit.distance;
            // Reaching across a wall to whatever is on the far side is no rest at all.
            let over = pivot + Vec3::Y * (c.clearance + c.under);
            let from = Vec3::new(upright.x, over.y + (-h.ads.y), upright.z);
            let to = over - from;
            let across = self.world.walk_ray(from, to.normalize_or_zero(), to.length()).is_some();
            (!across).then_some(Found { pivot, along })
        })
    }

    /// One tick of a soldier's rest, after they have moved and before anything looks at the gun.
    /// `can_hold` says that their hands are on the gun and nothing else is going on.
    pub(crate) fn step_rest(&mut self, id: SoldierId, pressed: bool, yaw: f32, pitch: f32, can_hold: bool) {
        let soldier = &self.soldiers[id];
        let cfg = &self.weapons[soldier.weapon.weapon];
        let c = &cfg.rest;
        let m = &self.move_cfg;
        let body = &soldier.body;
        let mut rest = soldier.rest;
        if !rest.on && !pressed && rest.amount == 0.0 && rest.settle == 0.0 && rest.refused == 0.0 && rest.eye == Vec3::ZERO {
            return;
        }
        let steady = can_hold && body.grounded && !body.sprinting && body.stance_timer == 0.0 && body.lean.side == 0 && body.lean.amount == 0.0;
        rest.refused = (rest.refused - TICK_DT).max(0.0);
        let mut happened = None;
        if !rest.on && pressed {
            let range = (steady && body.horizontal_speed() <= c.max_speed).then(|| self.body_range(id, c)).flatten();
            let found = range.and_then(|(low, high)| self.find_rest(id, cfg, yaw, low, high).map(|found| (found, low, high)));
            match found {
                Some((found, low, high)) => {
                    let height = rest.height;
                    rest = GunRest { on: true, pivot: found.pivot, along: found.along, yaw, pitch, feet: body.pos, low, high, height, refused: 0.0, ..rest };
                    happened = Some(CarryEvent::Rested);
                }
                None => rest.refused = c.refused_time,
            }
        } else if rest.on && pressed {
            rest.on = false;
            happened = Some(CarryEvent::RestLifted);
        }
        // Where the gun on the rest wants the head to be, for as far as the look is any use for it.
        let mut stance = body.stance;
        if rest.on {
            let arc = self.arc_of(id, cfg, &rest, yaw);
            let arrived = rest.amount >= 1.0;
            let in_arc = yaw >= arc.yaw_low && yaw <= arc.yaw_high && (!arrived || (pitch >= arc.pitch_low - c.limit_slack && pitch <= arc.pitch_high + c.limit_slack));
            let moved = Vec2::new(body.pos.x - rest.feet.x, body.pos.z - rest.feet.z).length() > c.max_drift;
            let target = rested_eye(cfg, rest.pivot + Vec3::Y * c.clearance, rest.along, yaw, pitch.clamp(arc.pitch_low, arc.pitch_high), soldier.weapon.ads);
            let reach = (target - body.eye()).with_y(0.0);
            let height = (target.y - body.pos.y).clamp(rest.low, rest.high);
            // Whatever it was on has gone, as a truck does when it drives off.
            let above = rest.pivot + Vec3::Y * c.support_gap;
            let held_up = self.world.walk_ray(above, Vec3::NEG_Y, c.support_gap * 2.0).is_some();
            let top = body.pos + Vec3::Y * height;
            let fits = reach.length() <= c.max_eye_shift && self.eye_fits(top, reach, c) && self.world.capsule_is_clear(body.pos, held_capsule(m, height));
            if !steady || moved || !in_arc || !held_up || !fits {
                rest.on = false;
                happened = Some(CarryEvent::RestLifted);
            } else {
                rest.reach = reach;
                rest.height = height;
                if arrived && stance != Stance::Prone {
                    stance = nearest_stance(m, height);
                }
            }
        }
        // Coming off, to the nearest stance they fit in. A stance key is its own answer.
        if !rest.on && soldier.rest.on && body.stance_timer == 0.0 && stance != Stance::Prone {
            stance = nearest_stance(m, rest.height);
            if stance == Stance::Stand && !self.world.capsule_is_clear(body.pos, m.capsule(Stance::Stand)) {
                stance = Stance::Crouch;
            }
        }
        // However far it has to go, it goes over at a pace a head moves at.
        let home = m.eye(stance);
        let time = c.ease_time + (rest.height - home).abs() * c.ease_per_metre;
        let ease = TICK_DT / time.max(1e-3);
        rest.amount = (rest.amount + if rest.on { ease } else { -ease }).clamp(0.0, 1.0);
        let settling = rest.on && rest.amount >= 1.0;
        rest.settle = if settling { (rest.settle + TICK_DT / c.settle_time.max(1e-3)).min(1.0) } else { (rest.settle - ease).max(0.0) };
        let most = c.eye_speed * TICK_DT;
        let wanted = rest.reach * smooth(rest.amount);
        rest.eye += (wanted - rest.eye).clamp_length_max(most);
        let holding = rest.amount > 0.0 || rest.eye != Vec3::ZERO;
        let soldier = &mut self.soldiers[id];
        if holding {
            let height = home + (rest.height - home) * smooth(rest.amount);
            soldier.body.eye_height += (height - soldier.body.eye_height).clamp(-most, most);
            soldier.body.held_eye = soldier.body.eye_height;
        } else {
            soldier.body.held_eye = 0.0;
        }
        soldier.body.stance = stance;
        soldier.rest = rest;
        if let Some(event) = happened {
            self.events.push(SimEvent::Carry { soldier: id, event });
        }
    }
}
