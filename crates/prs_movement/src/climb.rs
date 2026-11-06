//! Getting up and over things with your hands: a vault, a mantle, a pull-up, or up onto someone's back.
//!
//! A climb is planned once and then is a fixed path through the air, so it is the same on every machine
//! and needs nothing from the server but the press that started it. Everything about whether it can be
//! done is looked at in one place, [`plan_climb`], with the same path it will really follow. Each tick
//! walks one step along it and looks again, and if something has got in the way the soldier just lets go.
//!
//! Only an abrupt rise counts as a ledge, so a slope, however steep, is never climbed: that is what holds
//! the map's rim, and it means pressing jump on a hillside is still a jump.
//!
//! The path eases at both ends of every part of it, with the height and the eye moving on quintic curves,
//! so the view never gets a jolt. There is no speed to carry out of it: you stand where it left you.

use crate::{MoveConfig, MoveEvent, MoveInput, MoveState, SKIN, Stance, horizontal, standable, yaw_basis};
use glam::Vec3;
use prs_collision::{Capsule, CollisionWorld, Surface};
use std::f32::consts::PI;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ClimbConfig {
    /// How far forward the stick has to be, so jumping on the spot beside a wall is still a jump.
    pub forward_wish: f32,
    /// How far ahead of the feet an edge can be and still be got to.
    pub reach: f32,
    /// How far apart to look along the ground for an edge, before narrowing it down.
    pub probe_step: f32,
    /// How far above the limit of reach and below the feet to look.
    pub probe_above: f32,
    pub probe_below: f32,
    /// How high up that reach to look from: all the way, then lower if that is inside something.
    pub probe_shares: [f32; 3],
    /// Tops up to here are a quick hop over. A thin wall can be gone over up to `mantle_height`.
    pub vault_height: f32,
    /// Up to about the chin. Above it, hanging by the arms, is a pull-up.
    pub mantle_height: f32,
    /// As high as a soldier can get with empty hands and a fresh body.
    pub max_height: f32,
    /// How much further someone standing on a back reaches: the one below gives a shove.
    pub perched_boost: f32,
    /// How far either side of the feet the top has to carry on, to be something to stand on. A thinner
    /// wall than twice this is only something to go over.
    pub support: f32,
    /// How much the top can rise or fall across that and still be one thing.
    pub top_tolerance: f32,
    /// How far past the edge the feet come to rest on top, past the body's own radius.
    pub on_top_margin: f32,
    /// The thickest top that can be gone over rather than climbed onto.
    pub over_depth: f32,
    /// How far past the far side the feet come down.
    pub over_beyond: f32,
    /// The furthest down the far side can be from the top.
    pub over_drop: f32,
    /// What the feet clear a top by on the way across.
    pub clearance: f32,
    /// Nothing of these can be climbed. A hedge is not something you can stand on.
    pub no_top: Vec<Surface>,
    /// Seconds a climb takes: this, and this much per metre up.
    pub base_time: f32,
    pub time_per_metre: f32,
    /// Share of the climb spent on the way up, and how much before that the feet start across.
    pub rise_share: f32,
    pub over_blend: f32,
    /// How far through the feet start settling down onto a top.
    pub settle_from: f32,
    /// How far the eye tucks in on the way, at the middle of the climb.
    pub duck: f32,
    /// Going through a gap too low to go through upright, as a window is, the body goes down flat over
    /// the sill: how high over the feet the eye is while it is in the gap, the seconds more it takes, and
    /// of that the seconds at the end for getting up again on the far side. It goes down on the way up to
    /// the sill and comes back up once clear of the far side. Zero never squeezes through anything.
    pub squeeze: f32,
    pub squeeze_time: f32,
    pub squeeze_stand: f32,
    /// How often to look along a path when planning it, in metres.
    pub path_step: f32,
    pub path_margin: f32,
    /// Stamina it takes, on top of the same drain as sprinting. Short of this it still happens, and leaves none.
    pub stamina_base: f32,
    pub stamina_per_metre: f32,
    /// How much longer it takes with none of that stamina to give: 1 is twice as long, 0 is no different.
    pub winded_time: f32,
    /// Seconds before the next climb and the slowed walk afterwards, so a run of ledges is slower than stairs.
    pub cooldown: f32,
    pub exit_slow: f32,
    /// How much a wound takes off the height, at its worst, and how much longer it takes.
    pub wound_reach: f32,
    pub wound_time: f32,
    /// A wounded arm counts for this share of a wounded leg.
    pub arm_share: f32,
    /// The same for weight: per share of speed that weight has already cost.
    pub burden_reach: f32,
    pub burden_time: f32,
    /// How far into a climb the hands reach the ledge and how far from the end they let go.
    pub grip_in: f32,
    pub grip_out: f32,
}

impl Default for ClimbConfig {
    fn default() -> Self {
        Self {
            forward_wish: 0.3,
            reach: 0.95,
            probe_step: 0.1,
            probe_above: 0.3,
            probe_below: 3.0,
            probe_shares: [1.0, 0.7, 0.45],
            vault_height: 1.15,
            mantle_height: 1.6,
            max_height: 2.25,
            perched_boost: 0.5,
            support: 0.22,
            top_tolerance: 0.25,
            on_top_margin: 0.06,
            over_depth: 0.75,
            over_beyond: 0.35,
            over_drop: 2.7,
            clearance: 0.03,
            no_top: vec![Surface::Grass],
            base_time: 0.2,
            time_per_metre: 0.65,
            rise_share: 0.6,
            over_blend: 0.2,
            settle_from: 0.8,
            duck: 0.18,
            squeeze: 0.5,
            squeeze_time: 1.0,
            squeeze_stand: 0.7,
            path_step: 0.05,
            path_margin: 0.012,
            stamina_base: 0.08,
            stamina_per_metre: 0.07,
            winded_time: 1.0,
            cooldown: 0.6,
            exit_slow: 0.25,
            wound_reach: 1.5,
            wound_time: 1.5,
            arm_share: 0.6,
            burden_reach: 3.0,
            burden_time: 2.5,
            grip_in: 0.2,
            grip_out: 0.15,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoolConfig {
    /// How far ahead a crouched soldier can be, and how far off dead ahead, to climb onto.
    pub reach: f32,
    pub cone_cos: f32,
    /// How far above or below your feet theirs can be.
    pub level: f32,
    /// Where the feet go on them: above their feet, and behind them, the way they are facing.
    pub back_height: f32,
    pub back_offset: f32,
    /// How close to the crouched eye height they have to be, to be settled in it.
    pub settled: f32,
    /// Fastest someone with a rider on them can shuffle, and how far they can be from where the rider
    /// got on before the rider goes.
    pub carry_speed: f32,
    pub slack: f32,
    /// How far a rider can shuffle about on a back, how fast, and what they leave with when thrown off.
    pub radius: f32,
    pub walk_speed: f32,
    pub shrug_speed: f32,
    /// What a place on someone's back is rounded to, so that a client and the server get the very same
    /// numbers from someone who is standing still. It is the grid the wire sends a soldier on.
    pub grid: f32,
    /// Feet this close above the land count as being back down on it.
    pub on_the_land: f32,
}

impl Default for StoolConfig {
    fn default() -> Self {
        Self {
            reach: 1.1,
            cone_cos: 0.7,
            level: 0.5,
            back_height: 0.8,
            back_offset: 0.3,
            settled: 0.03,
            carry_speed: 0.5,
            slack: 0.25,
            radius: 0.2,
            walk_speed: 0.5,
            shrug_speed: 0.9,
            grid: 512.0,
            on_the_land: 0.6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClimbKind {
    /// Not climbing.
    #[default]
    None,
    Vault,
    Mantle,
    PullUp,
    /// Up onto a crouched soldier's back.
    Stool,
}

/// A way up that was found, or the one being gone through. Every field is public: a client has to
/// restore all of it to replay from what the server says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Climb {
    pub kind: ClimbKind,
    /// Whose back it is, for one that ends on someone.
    pub on: Option<u32>,
    pub from: Vec3,
    pub to: Vec3,
    /// Where the feet pass over: the height to get up to on the way.
    pub top: f32,
    /// The ledge under the hands.
    pub hands: Vec3,
    /// Which way it goes, which stays as it was wherever the player looks.
    pub yaw: f32,
    /// How far through, 0 to 1, and how many seconds all of it takes.
    pub t: f32,
    pub duration: f32,
    /// The share of the way through when the feet get up to the top, start across and start down.
    pub rise_end: f32,
    pub over_from: f32,
    pub fall_from: f32,
    /// Eye above the feet at the start and the end.
    pub eye_from: f32,
    pub eye_to: f32,
    /// The stance it ends in.
    pub end: Stance,
    /// Whether it ends somewhere that takes a back to reach.
    pub boosted: bool,
    /// How high over the feet the eye goes while crossing, to get through a low gap, zero for an open
    /// top, and how far through it is before the body is clear of the far side and coming back up.
    pub squeeze: f32,
    pub squeeze_until: f32,
    /// The share of it at the end that is spent getting up where it lands, after going across.
    pub stand_share: f32,
}

/// Standing on someone. Nothing here follows them, so a rider's place depends on nothing but this:
/// when they move too far the rider simply stops being held up. It is what lets a client predict it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Perch {
    pub on: Option<u32>,
    pub spot: Vec3,
}

/// A crouched soldier in front of someone asking to climb, offered by whoever knows about soldiers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoolOffer {
    pub on: u32,
    /// Where feet go on their back, on the grid.
    pub spot: Vec3,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

impl Climb {
    pub fn active(&self) -> bool {
        self.kind != ClimbKind::None
    }

    /// The feet, this far through.
    pub fn feet_at(&self, t: f32) -> Vec3 {
        let rise = ease(t / self.rise_end.max(1e-3));
        let fall = ease((t - self.fall_from) / (1.0 - self.fall_from).max(1e-3));
        let flat = self.from + (self.to - self.from) * self.along_at(t);
        Vec3::new(flat.x, self.from.y + (self.top - self.from.y) * rise + (self.to.y - self.top) * fall, flat.z)
    }

    /// How far across it has gone, 0 to 1, this far through.
    fn along_at(&self, t: f32) -> f32 {
        ease((t - self.over_from) / (1.0 - self.stand_share - self.over_from).max(1e-3))
    }

    /// How far through it is by the time this share of the way across has gone.
    fn when_across(&self, share: f32) -> f32 {
        let mut t = self.over_from;
        while t < 0.98 && self.along_at(t) < share {
            t += 0.01;
        }
        t.min(0.98)
    }

    /// The eye above the feet, this far through.
    pub fn eye_at(&self, t: f32, cfg: &MoveConfig) -> f32 {
        let tuck = (PI * t).sin();
        // Settled by the time the feet are going in under whatever is over the top.
        let settled = self.eye_from + (self.eye_to - self.eye_from) * ease(t / self.rise_end.max(1e-3)) - cfg.climb.duck * tuck * tuck;
        if self.squeeze <= 0.0 {
            return settled;
        }
        // Down on the way up to the sill, flat over it for as long as the body is in the gap, and up again
        // once the far side is behind them.
        let down = ease(t / self.over_from.max(1e-3));
        let up = ease((1.0 - t) / (1.0 - self.squeeze_until).max(1e-3));
        settled + (self.squeeze.min(settled) - settled) * down.min(up)
    }

    /// What a soldier takes up at this eye height: down to their eye and a little over it.
    fn capsule_at(&self, t: f32, cfg: &MoveConfig) -> Capsule {
        let height = self.eye_at(t, cfg) + (cfg.stand_height - cfg.stand_eye);
        Capsule { height: height.max(cfg.radius * 2.0 + 0.01), radius: cfg.radius }
    }

    /// How far the hands are on the ledge rather than the gun, 0 to 1.
    pub fn grip(&self, cfg: &ClimbConfig) -> f32 {
        let on = (self.t / cfg.grip_in.max(1e-3)).clamp(0.0, 1.0);
        let off = ((1.0 - self.t) / cfg.grip_out.max(1e-3)).clamp(0.0, 1.0);
        ease(on.min(off))
    }

    fn path_is_clear(&self, cfg: &MoveConfig, world: &CollisionWorld) -> bool {
        // The longest way round for how many looks to take, since it does not go in a straight line.
        let length = (self.top - self.from.y).abs() + (self.to.y - self.top).abs() + horizontal(self.to - self.from).length();
        let count = (length / cfg.climb.path_step.max(1e-2)).ceil().clamp(4.0, 128.0) as usize;
        (1..=count).all(|i| {
            let t = i as f32 / count as f32;
            // A shade fatter than the body, so that nothing can graze it between one look and the next.
            self.fits_at(t, cfg, world, cfg.climb.path_margin)
        })
    }

    /// Whether the body fits this far along, this much fatter. Straight up from where they stand, up against
    /// a wall as they may be, all that is asked is not getting closer to anything than a body can be.
    fn fits_at(&self, t: f32, cfg: &MoveConfig, world: &CollisionWorld, margin: f32) -> bool {
        let body = self.capsule_at(t, cfg);
        let feet = self.feet_at(t);
        let radius = if horizontal(feet - self.from).length() < SKIN { body.radius - SKIN } else { body.radius + margin };
        world.capsule_is_clear(feet, Capsule { radius, height: body.height + margin })
    }
}

/// A vertical look at the ground ahead.
struct Probe {
    height: f32,
    walkable: bool,
    surface: Surface,
}

struct Looking<'a> {
    cfg: &'a MoveConfig,
    world: &'a CollisionWorld,
    feet: Vec3,
    forward: Vec3,
    right: Vec3,
    /// The height the rays start at.
    above: f32,
    down: f32,
}

impl Looking<'_> {
    fn ahead(&self, d: f32) -> Vec3 {
        self.feet + self.forward * d
    }

    /// The first thing going down from above the limit of reach, or None if nothing is there. If that is
    /// inside a slab there may still be room under it, so a little lower is tried, and only if all of
    /// those are inside something is it solid here, which is the Err.
    fn probe(&self, at: Vec3) -> Result<Option<Probe>, ()> {
        for share in self.cfg.climb.probe_shares {
            let start = at.with_y(self.feet.y + (self.above - self.feet.y) * share);
            match self.world.walk_ray(start, Vec3::NEG_Y, self.down) {
                Some(hit) if hit.distance <= 0.0 => continue,
                Some(hit) => {
                    return Ok(Some(Probe {
                        height: start.y - hit.distance,
                        walkable: hit.normal.y >= self.cfg.max_slope_cos,
                        surface: hit.surface,
                    }));
                }
                None => return Ok(None),
            }
        }
        Err(())
    }

    /// The nearest abrupt rise ahead that is more than a step: how far, and how high its top is.
    fn edge(&self, limit: f32) -> Option<(f32, Probe)> {
        let c = &self.cfg.climb;
        let mut last = self.feet.y;
        let mut last_d = 0.0;
        let mut d = c.probe_step;
        while d <= c.reach {
            let probe = self.probe(self.ahead(d)).ok()?;
            if let Some(found) = probe {
                let rise = found.height - self.feet.y;
                if found.height - last > self.cfg.step_height && rise > self.cfg.step_height {
                    if rise > limit {
                        return None;
                    }
                    return Some(self.narrow(last_d, d, last, found));
                }
                last = found.height;
            }
            last_d = d;
            d += c.probe_step;
        }
        None
    }

    /// Closes in on where a rise found between two looks really starts.
    fn narrow(&self, mut near: f32, mut far: f32, low: f32, mut top: Probe) -> (f32, Probe) {
        for _ in 0..4 {
            let middle = (near + far) * 0.5;
            match self.probe(self.ahead(middle)) {
                Ok(Some(probe)) if probe.height - low > self.cfg.step_height => {
                    far = middle;
                    top = probe;
                }
                _ => near = middle,
            }
        }
        (far, top)
    }

    /// Whether the top carries on all round this point, for feet to rest on it.
    fn supports(&self, at: Vec3, height: f32) -> bool {
        let c = &self.cfg.climb;
        [Vec3::ZERO, self.forward, -self.forward, self.right, -self.right].iter().all(|offset| {
            matches!(self.probe(at + *offset * c.support), Ok(Some(p))
                if p.walkable && !c.no_top.contains(&p.surface) && (p.height - height).abs() <= c.top_tolerance)
        })
    }

    /// Where the far side of a top is, if it is thin enough to go over.
    fn far_side(&self, edge: f32, height: f32) -> Option<f32> {
        let c = &self.cfg.climb;
        let mut d = c.probe_step;
        while d <= c.over_depth {
            let dropped = match self.probe(self.ahead(edge + d)) {
                Ok(Some(p)) => p.height < height - c.top_tolerance,
                Ok(None) => true,
                Err(()) => return None,
            };
            if dropped {
                return Some(edge + d);
            }
            d += c.probe_step;
        }
        None
    }

    /// Where feet come to rest coming down at this point from `from`, no further than `max_drop`. Looked
    /// at with the crouched body, as what fits is worked out afterwards.
    fn rest(&self, at: Vec3, from: f32, max_drop: f32) -> Option<Vec3> {
        let body = self.cfg.capsule(Stance::Crouch);
        let high = Vec3::new(at.x, from, at.z);
        if !self.world.capsule_is_clear(high, body) {
            return None;
        }
        let hit = self.world.cast_capsule(high, body, Vec3::NEG_Y, max_drop)?;
        let feet = high - Vec3::Y * (hit.distance - SKIN);
        standable(feet, self.cfg, &hit, self.world).then_some(feet)
    }
}

/// How high this soldier can get, as they are.
pub fn climb_limit(cfg: &MoveConfig, hurt: f32, burden: f32, perched: bool) -> f32 {
    let c = &cfg.climb;
    let boost = if perched { c.perched_boost } else { 0.0 };
    c.max_height + boost - hurt.clamp(0.0, 1.0) * c.wound_reach - burden.max(0.0) * c.burden_reach
}

/// What stance a body can end up in at this spot, given the one it is after. None if nothing fits.
fn fitting_stance(cfg: &MoveConfig, world: &CollisionWorld, feet: Vec3, wanted: Stance) -> Option<Stance> {
    if wanted == Stance::Stand && world.capsule_is_clear(feet, cfg.capsule(Stance::Stand)) {
        return Some(Stance::Stand);
    }
    world.capsule_is_clear(feet, cfg.capsule(Stance::Crouch)).then_some(Stance::Crouch)
}

/// The climb this soldier would get by asking now, if there is one. It only looks, so it can be asked
/// by anything, such as a bot wondering whether a bit of cover can be gone over.
pub fn plan_climb(state: &MoveState, cfg: &MoveConfig, input: &MoveInput, world: &CollisionWorld) -> Option<Climb> {
    let c = &cfg.climb;
    let perched = state.perch.on.is_some();
    let (forward, right) = yaw_basis(input.yaw);
    // In the air, what counts is how high it is from the ground below, so a jump never adds to the reach.
    let lifted = if state.grounded || perched { 0.0 } else { world.walk_ray(state.pos, Vec3::NEG_Y, c.probe_below)?.distance };
    let limit = climb_limit(cfg, input.hurt, input.burden, perched) - lifted;
    if limit <= 0.0 {
        return None;
    }
    let wanted = if state.stance == Stance::Crouch { Stance::Crouch } else { Stance::Stand };
    let mut climb = Climb {
        from: state.pos,
        yaw: input.yaw,
        eye_from: state.eye_height,
        rise_end: c.rise_share,
        ..Climb::default()
    };

    let over = if let Some(offer) = input.stool {
        let rise = offer.spot.y - state.pos.y;
        if rise <= 0.0 || rise > limit {
            return None;
        }
        climb.kind = ClimbKind::Stool;
        climb.on = Some(offer.on);
        climb.to = offer.spot;
        climb.top = offer.spot.y + c.clearance;
        climb.hands = offer.spot + forward * c.on_top_margin;
        climb.end = fitting_stance(cfg, world, offer.spot, wanted)?;
        None
    } else {
        let look = Looking {
            cfg,
            world,
            feet: state.pos,
            forward,
            right,
            above: state.pos.y + limit + c.probe_above,
            down: limit + c.probe_above + c.probe_below,
        };
        let (edge, top) = look.edge(limit)?;
        if c.no_top.contains(&top.surface) || !top.walkable {
            return None;
        }
        let rise = top.height - state.pos.y + lifted;
        climb.hands = look.ahead(edge).with_y(top.height);
        let on_top = look.ahead(edge + cfg.radius + c.on_top_margin);
        // From a little above, in case it rises away from the edge, or from just over it, if there is no
        // room for that.
        let landing = look.supports(on_top, top.height).then(|| {
            let drop = c.top_tolerance * 2.0 + c.clearance;
            look.rest(on_top, top.height + c.top_tolerance + c.clearance, drop).or_else(|| look.rest(on_top, top.height + c.clearance, drop))
        });
        let landing = landing.flatten();
        match landing {
            Some(feet) => {
                climb.kind = if rise <= c.vault_height {
                    ClimbKind::Vault
                } else if rise <= c.mantle_height {
                    ClimbKind::Mantle
                } else {
                    ClimbKind::PullUp
                };
                let goal = if climb.kind == ClimbKind::PullUp { Stance::Crouch } else { wanted };
                climb.to = feet;
                climb.top = top.height.max(feet.y) + c.clearance;
                climb.end = fitting_stance(cfg, world, feet, goal)?;
                None
            }
            None => {
                // Nothing to stand on, so it is only something to go over, and only up to the chin.
                if rise > c.mantle_height {
                    return None;
                }
                let far = look.far_side(edge, top.height)?;
                let down = look.ahead(far + cfg.radius + c.over_beyond);
                let top_y = top.height + c.clearance;
                let feet = look.rest(down, top_y, c.over_drop)?;
                climb.kind = if rise <= c.vault_height { ClimbKind::Vault } else { ClimbKind::Mantle };
                climb.to = feet;
                climb.top = top_y;
                climb.end = fitting_stance(cfg, world, feet, wanted)?;
                Some((look.ahead(far + cfg.radius + c.clearance) - state.pos).length())
            }
        }
    };

    climb.eye_to = cfg.eye(climb.end);
    climb.boosted = perched && climb.to.y - (state.pos.y - cfg.stool.back_height) > c.max_height;
    let up = (climb.top - state.pos.y).max(0.0);
    let down = (climb.top - climb.to.y).max(0.0);
    climb.duration = (c.base_time + c.time_per_metre * (up + down * 0.5))
        * (1.0 + input.hurt.clamp(0.0, 1.0) * c.wound_time + input.burden.max(0.0) * c.burden_time);

    // Starting across as the feet come up to the top looks far better, if the wall lets it. Otherwise
    // all the way up and then across.
    let total = horizontal(climb.to - climb.from).length().max(1e-3);
    // Upright if there is room, and if not, flat through the gap, as anyone gets through a window.
    let upright = climb.duration;
    for squeeze in [0.0, c.squeeze] {
        climb.squeeze = squeeze;
        // The extra time squeezing goes on getting down, before going across, and getting up after, and the
        // going across itself takes as long as ever.
        climb.duration = if squeeze > 0.0 { upright + c.squeeze_time } else { upright };
        climb.stand_share = if squeeze > 0.0 { c.squeeze_stand / climb.duration } else { 0.0 };
        let getting_down = if squeeze > 0.0 { (c.squeeze_time - c.squeeze_stand).max(0.0) } else { 0.0 };
        climb.rise_end = (c.rise_share * upright + getting_down) / climb.duration;
        for over_from in [climb.rise_end * (1.0 - c.over_blend), climb.rise_end] {
            climb.over_from = over_from;
            climb.fall_from = match over {
                Some(clear) => climb.when_across(clear / total),
                None => c.settle_from,
            };
            // Clear of the far side once the body's back has gone past it.
            let behind = over.map_or(total, |clear| clear + cfg.radius);
            climb.squeeze_until = climb.when_across((behind / total).min(1.0));
            if climb.path_is_clear(cfg, world) {
                return Some(climb);
            }
        }
    }
    None
}

pub(crate) fn stamina_cost(climb: &Climb, cfg: &MoveConfig, drain: f32) -> f32 {
    let rise = (climb.top - climb.from.y).max(0.0);
    (cfg.climb.stamina_base + cfg.climb.stamina_per_metre * rise) * (1.0 + drain.max(0.0))
}

/// Starts a climb, if the jump just pressed was asking for one. Otherwise nothing has happened.
pub(crate) fn try_start(state: &mut MoveState, cfg: &MoveConfig, input: &MoveInput, world: &CollisionWorld, events: &mut Vec<MoveEvent>) -> bool {
    let footing = state.grounded || state.coyote > 0.0 || state.perch.on.is_some();
    // A jump that hasn't come down yet is still reaching for whatever it meets, so pressing a little too
    // far from a wall isn't a press wasted.
    let jumping = !footing && state.jump_cooldown > 0.0;
    let ready = (state.jump_buffered > 0.0 || jumping)
        && input.wish.y > cfg.climb.forward_wish
        && state.stance != Stance::Prone
        && state.stance_timer == 0.0
        && state.climb_cooldown == 0.0
        && (state.jump_cooldown == 0.0 || jumping)
        && !input.collapsing
        && !input.busy_hands;
    if !ready {
        return false;
    }
    let Some(mut climb) = plan_climb(state, cfg, input, world) else {
        return false;
    };
    let cost = stamina_cost(&climb, cfg, input.extra_drain);
    let short = 1.0 - (state.stamina / cost.max(1e-3)).min(1.0);
    climb.duration *= 1.0 + cfg.climb.winded_time * short;
    state.stamina = (state.stamina - cost).max(0.0);
    state.stamina_delay = cfg.stamina_regen_delay;
    state.jump_buffered = 0.0;
    state.sprinting = false;
    state.perch = Perch::default();
    state.lean.side = 0;
    if state.stance != climb.end {
        state.stance = climb.end;
        events.push(MoveEvent::StanceChanged { stance: climb.end });
    }
    state.climb = climb;
    events.push(MoveEvent::Climbed { kind: climb.kind });
    true
}

/// One tick of a climb under way. False if there isn't one, or if there no longer is, and the soldier
/// should move as anyone does.
pub(crate) fn step_climbing(state: &mut MoveState, cfg: &MoveConfig, input: &MoveInput, world: &CollisionWorld, dt: f32) -> bool {
    if !state.climb.active() {
        return false;
    }
    if input.collapsing {
        abort_climb(state, cfg, world);
        return false;
    }
    let climb = state.climb;
    let t = (climb.t + dt / climb.duration.max(1e-3)).min(1.0);
    let feet = climb.feet_at(t);
    // The only thing that can have changed is whatever else the world has in it. Static geometry was
    // all looked at when this was planned, so this is what stops a gap closing on someone.
    if !climb.fits_at(t, cfg, world, 0.0) {
        abort_climb(state, cfg, world);
        return false;
    }
    state.jump_buffered = 0.0;
    state.coyote = 0.0;
    state.grounded = false;
    state.sprinting = false;
    state.stamina_delay = cfg.stamina_regen_delay;
    state.vel = (feet - state.pos) / dt;
    state.pos = feet;
    state.eye_height = climb.eye_at(t, cfg);
    state.climb.t = t;
    if t >= 1.0 {
        finish(state, cfg, world);
    }
    true
}

fn finish(state: &mut MoveState, cfg: &MoveConfig, world: &CollisionWorld) {
    let climb = state.climb;
    state.pos = climb.to;
    state.vel = Vec3::ZERO;
    state.eye_height = climb.eye_to;
    state.eye_rate = 0.0;
    state.stance = climb.end;
    state.climb = Climb::default();
    state.climb_cooldown = cfg.climb.cooldown;
    state.jump_cooldown = state.jump_cooldown.max(cfg.jump_cooldown);
    state.landing_timer = cfg.climb.exit_slow;
    state.boosted |= climb.boosted;
    if let Some(on) = climb.on {
        state.perch = Perch { on: Some(on), spot: climb.to };
        state.grounded = true;
    } else {
        crate::probe_ground(state, cfg, cfg.capsule(climb.end), world);
    }
}

/// Lets go, wherever that is, with no speed in any direction. The soldier falls from there as anyone
/// would, and it costs the same wait as finishing, or a run of attempts would be free.
pub fn abort_climb(state: &mut MoveState, cfg: &MoveConfig, world: &CollisionWorld) {
    state.climb = Climb::default();
    state.perch = Perch::default();
    state.vel = Vec3::ZERO;
    state.grounded = false;
    state.climb_cooldown = cfg.climb.cooldown;
    state.jump_cooldown = state.jump_cooldown.max(cfg.jump_cooldown);
    state.eye_rate = 0.0;
    if !world.capsule_is_clear(state.pos, cfg.capsule(state.stance)) {
        state.stance = Stance::Crouch;
    }
}

/// One tick on someone's back. They can shuffle about up there and step off the edge, and that is it.
pub(crate) fn step_perched(state: &mut MoveState, cfg: &MoveConfig, input: &MoveInput, world: &CollisionWorld, dt: f32) -> bool {
    if state.perch.on.is_none() {
        return false;
    }
    let s = &cfg.stool;
    let (forward, right) = yaw_basis(input.yaw);
    let wish = input.wish.clamp_length_max(1.0);
    let vel = (forward * wish.y + right * wish.x) * s.walk_speed;
    let spot = state.perch.spot;
    let next = state.pos + vel * dt;
    state.vel = Vec3::ZERO;
    state.grounded = true;
    state.ground_normal = Vec3::Y;
    state.ground_surface = Surface::Flesh;
    state.coyote = cfg.coyote_time;
    state.sprinting = false;
    if state.jump_buffered > cfg.jump_buffer {
        state.jump_buffered = cfg.jump_buffer;
    }
    if world.capsule_is_clear(next, cfg.capsule(state.stance)) {
        state.vel = vel;
        state.pos = next;
        if horizontal(next - spot).length() > s.radius {
            state.perch = Perch::default();
            state.grounded = false;
            return false;
        }
    }
    true
}
