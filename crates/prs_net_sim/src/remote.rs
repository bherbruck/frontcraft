//! Other soldiers on the wire: whole numbers, so a delta is small, holding what it takes to draw them.
//! All the inputs to `gun_pose` are here, so everyone sees a gun where it really is.
//!
//! Their body animation is not. It follows from the rest and costs bandwidth, so a client steps its
//! own copy. The scales here are part of the format rather than tuning: both sides must agree, and
//! each is finer than anyone could see at the range a soldier is drawn.

use glam::{Vec2, Vec3};
use prs_net::entity::NetEntity;
use prs_sim::health::Life;
use prs_sim::inventory::Slot;
use prs_sim::seats::{SeatedAt, Transit, TransitKind};
use prs_sim::movement::{ClimbKind, MoveConfig, Stance, lean_eye};
use prs_sim::weapons::{WeaponId, WeaponTable};
use prs_sim::body::{CareKit, CarePose};
use prs_sim::{Dressed, Sim, Soldier, SoldierId};

use crate::own::{care_site_from_bits, care_site_to_bits, climb_kind_from_bits, climb_kind_to_bits};

const POS_SCALE: f32 = 512.0;
const VEL_SCALE: f32 = 128.0;
/// About 0.06 mrad, so a centimetre at 160 m.
const ANGLE_SCALE: f32 = 16384.0;
/// The gun's aim offset gets more, as it is what the shot actually depends on.
const AIM_SCALE: f32 = 65536.0;
const EYE_SCALE: f32 = 1024.0;
const FRACTION_SCALE: f32 = 255.0;
const RETRACT_SCALE: f32 = 1024.0;
const STRIDE_SCALE: f32 = 256.0;
const LEAN_SCALE: f32 = 1024.0;
const CLIMB_T_SCALE: f32 = 1024.0;

const POS: usize = 0;
const VEL: usize = 3;
const YAW: usize = 6;
const PITCH: usize = 7;
const AIM: usize = 8;
const EYE: usize = 10;
const ADS: usize = 11;
const READY: usize = 12;
const RETRACT: usize = 13;
const STRIDE: usize = 14;
const RELOAD: usize = 15;
const FLAGS: usize = 16;
const LEG: usize = 17;
const WEAPON: usize = 18;
const TEAM: usize = 19;
const LEAN: usize = 20;
const SEAT: usize = 21;
const CLIMB_KIND: usize = 22;
const CLIMB_T: usize = 23;
const LEDGE: usize = 24;
const CLIMB_YAW: usize = 27;
const MOUNT: usize = 28;
const TRANSIT: usize = 29;
const TRANSIT_T: usize = 30;
/// How far down they have gone, which `gun_pose` takes as an input, so it is not left to work out.
const COLLAPSE: usize = 31;
/// Where a gun set down on something has put the eye, since `gun_pose` goes out from there.
const REST_EYE: usize = 32;
/// How far through a treatment they are, one more than the fraction so that nothing means none.
const CARE: usize = 35;
/// What it is on and with, and what dressings they have on.
const CARE_HOW: usize = 36;
/// How far the gun is on its sling, which `gun_pose` also takes as an input.
const SLUNG: usize = 37;

/// Fields that change together share a bit in the delta's mask: (first field, how many).
pub(crate) const GROUPS: &[(usize, usize)] = &[
    (POS, 3),
    (VEL, 3),
    (YAW, 2),
    (AIM, 2),
    (EYE, 1),
    (ADS, 2),
    (RETRACT, 1),
    (STRIDE, 1),
    (RELOAD, 1),
    (FLAGS, 1),
    (LEG, 1),
    (WEAPON, 2),
    (LEAN, 1),
    (SEAT, 1),
    (CLIMB_KIND, 2),
    (REST_EYE, 3),
    (LEDGE, 4),
    (MOUNT, 1),
    (TRANSIT, 2),
    (COLLAPSE, 1),
    (CARE, 2),
    (SLUNG, 1),
];

const SEAT_SHIFT: u32 = 8;
const STANCE_MASK: i32 = 3;
const GROUNDED: i32 = 1 << 2;
const SPRINTING: i32 = 1 << 3;
const LIFE_SHIFT: u32 = 4;
const LIFE_MASK: i32 = 3;
const CARRYING: i32 = 1 << 6;
const THROWING: i32 = 1 << 7;
/// What they are winding up with: a frag, or with this set, smoke.
const THROWING_SMOKE: i32 = 1 << 8;
const LIFTING: i32 = 1 << 9;
const PUSHING: i32 = 1 << 10;
const REMOTE_WINDUP: f32 = 10.0;
const HOW_SITE_MASK: i32 = 3;
const HOW_MEDKIT: i32 = 1 << 2;
const HOW_DRESSED_ARM: i32 = 1 << 3;
const HOW_DRESSED_LEG: i32 = 1 << 4;

fn quant(value: f32, scale: f32) -> i32 {
    (value * scale).round() as i32
}

pub(crate) fn stance_to_bits(stance: Stance) -> u8 {
    match stance {
        Stance::Stand => 0,
        Stance::Crouch => 1,
        Stance::Prone => 2,
    }
}

pub(crate) fn stance_from_bits(bits: u8) -> Stance {
    match bits {
        1 => Stance::Crouch,
        2 => Stance::Prone,
        _ => Stance::Stand,
    }
}

pub(crate) fn life_to_bits(life: Life) -> u8 {
    match life {
        Life::Up => 0,
        Life::Down => 1,
        Life::Dead => 2,
    }
}

pub(crate) fn life_from_bits(bits: u8) -> Life {
    match bits {
        1 => Life::Down,
        2 => Life::Dead,
        _ => Life::Up,
    }
}

pub fn capture(sim: &Sim, id: SoldierId) -> NetEntity {
    let soldier = &sim.soldiers[id];
    let (body, weapon) = (&soldier.body, &soldier.weapon);
    let aim = weapon.aim_offset();
    let mut entity = NetEntity::new(crate::KIND_SOLDIER, id as u32);
    let f = &mut entity.fields;
    // Someone seated is wherever their seat is, so all that is sent is which seat.
    if soldier.seat.is_none() {
        for axis in 0..3 {
            f[POS + axis] = quant(body.pos[axis], POS_SCALE);
            f[VEL + axis] = quant(body.vel[axis], VEL_SCALE);
        }
    }
    f[SEAT] = soldier.seat.map_or(0, |at| (at.carrier as i32 + 1) << SEAT_SHIFT | at.seat as i32);
    f[YAW] = quant(soldier.yaw, ANGLE_SCALE);
    f[PITCH] = quant(soldier.pitch, ANGLE_SCALE);
    f[AIM] = quant(aim.x, AIM_SCALE);
    f[AIM + 1] = quant(aim.y, AIM_SCALE);
    f[EYE] = quant(body.eye_height, EYE_SCALE);
    f[ADS] = quant(weapon.ads, FRACTION_SCALE);
    f[READY] = quant(weapon.ready, FRACTION_SCALE);
    f[RETRACT] = quant(soldier.gun_retract, RETRACT_SCALE);
    f[STRIDE] = quant(body.stride, STRIDE_SCALE);
    f[RELOAD] = quant(weapon.reload_progress(soldier.held_config(&sim.weapons)), FRACTION_SCALE);
    f[FLAGS] = stance_to_bits(body.stance) as i32
        | if body.grounded { GROUNDED } else { 0 }
        | if body.sprinting { SPRINTING } else { 0 }
        | (life_to_bits(soldier.health.life) as i32) << LIFE_SHIFT
        | if sim.is_carrying(id) { CARRYING } else { 0 }
        | if soldier.throw.held > 0.0 { THROWING } else { 0 }
        | if soldier.inventory.selected == Slot::Smoke { THROWING_SMOKE } else { 0 }
        | if soldier.lifting.is_some() { LIFTING } else { 0 }
        | if soldier.pushing.is_some() { PUSHING } else { 0 };
    f[LEG] = quant(soldier.health.leg_wound, FRACTION_SCALE);
    f[WEAPON] = weapon.weapon.0 as i32;
    f[TEAM] = soldier.team as i32;
    f[LEAN] = quant(body.lean.amount, LEAN_SCALE);
    for axis in 0..3 {
        f[REST_EYE + axis] = quant(soldier.rest.eye[axis], EYE_SCALE);
    }
    f[COLLAPSE] = quant(soldier.anim.collapse, FRACTION_SCALE);
    f[SLUNG] = quant(soldier.anim.slung, FRACTION_SCALE);
    let treating = sim.care_pose(id);
    f[CARE] = treating.map_or(0, |pose| 1 + quant(pose.progress, FRACTION_SCALE));
    f[CARE_HOW] = treating.map_or(0, |pose| {
        care_site_to_bits(pose.site) as i32 | if pose.kit == CareKit::Medkit { HOW_MEDKIT } else { 0 }
    }) | if soldier.dressed.arm { HOW_DRESSED_ARM } else { 0 }
        | if soldier.dressed.leg { HOW_DRESSED_LEG } else { 0 };
    if body.climb.active() {
        f[CLIMB_KIND] = climb_kind_to_bits(body.climb.kind) as i32;
        f[CLIMB_T] = quant(body.climb.t, CLIMB_T_SCALE);
        for axis in 0..3 {
            f[LEDGE + axis] = quant(body.climb.hands[axis], POS_SCALE);
        }
        f[CLIMB_YAW] = quant(body.climb.yaw, ANGLE_SCALE);
    }
    f[MOUNT] = body.climb.on.or(body.perch.on).map_or(0, |on| on as i32 + 1);
    if let Some(transit) = soldier.transit {
        let (tag, index) = match transit.kind {
            TransitKind::In { entry } => (1, entry),
            TransitKind::Out { entry } => (2, entry),
            TransitKind::Across { from } => (3, from),
        };
        f[TRANSIT] = tag | (transit.back as i32) << 2 | (index as i32) << 3;
        f[TRANSIT_T] = quant(transit.t, CLIMB_T_SCALE);
    }
    entity
}

/// A remote soldier in world units, as drawn at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemotePose {
    pub feet: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Radians: x = yaw, y = pitch. Recoil, sway and wander already added up.
    pub aim_offset: Vec2,
    pub eye_height: f32,
    pub ads: f32,
    pub ready: f32,
    pub retract: f32,
    pub stride: f32,
    pub reload: f32,
    pub leg_wound: f32,
    pub stance: Stance,
    pub grounded: bool,
    pub sprinting: bool,
    pub life: Life,
    pub weapon: WeaponId,
    pub team: u8,
    /// -1 = fully left, 1 = fully right. The eye and everything else about a lean follow from this.
    pub lean: f32,
    pub collapse: f32,
    pub slung: f32,
    pub seat: Option<SeatedAt>,
    /// From where the eye would be to where a gun set down on something has put it.
    pub rest_eye: Vec3,
    pub transit: Option<Transit>,
    pub climb: ClimbKind,
    /// How far through a climb, 0 to 1, with the ledge under their hands and the way it goes.
    pub climb_t: f32,
    pub ledge: Vec3,
    pub climb_yaw: f32,
    /// Whose back they are on or climbing onto, and whether anyone is on theirs.
    pub mount: Option<u32>,
    pub carrying: bool,
    /// With their hands on a vehicle, righting it.
    pub lifting: bool,
    /// Leaning into one, pushing it.
    pub pushing: bool,
    /// Winding up to throw something, and whether that is smoke rather than a frag.
    pub throwing: bool,
    pub throwing_smoke: bool,
    /// A treatment they are in the middle of, and the dressings they have on.
    pub care: Option<CarePose>,
    pub dressed: Dressed,
}

impl RemotePose {
    pub fn from_entity(entity: &NetEntity) -> Self {
        let f = &entity.fields;
        let v3 = |at: usize, scale: f32| Vec3::new(f[at] as f32, f[at + 1] as f32, f[at + 2] as f32) / scale;
        Self {
            feet: v3(POS, POS_SCALE),
            velocity: v3(VEL, VEL_SCALE),
            yaw: f[YAW] as f32 / ANGLE_SCALE,
            pitch: f[PITCH] as f32 / ANGLE_SCALE,
            aim_offset: Vec2::new(f[AIM] as f32, f[AIM + 1] as f32) / AIM_SCALE,
            eye_height: f[EYE] as f32 / EYE_SCALE,
            ads: f[ADS] as f32 / FRACTION_SCALE,
            ready: f[READY] as f32 / FRACTION_SCALE,
            retract: f[RETRACT] as f32 / RETRACT_SCALE,
            stride: f[STRIDE] as f32 / STRIDE_SCALE,
            reload: f[RELOAD] as f32 / FRACTION_SCALE,
            leg_wound: f[LEG] as f32 / FRACTION_SCALE,
            stance: stance_from_bits((f[FLAGS] & STANCE_MASK) as u8),
            grounded: f[FLAGS] & GROUNDED != 0,
            sprinting: f[FLAGS] & SPRINTING != 0,
            life: life_from_bits(((f[FLAGS] >> LIFE_SHIFT) & LIFE_MASK) as u8),
            weapon: WeaponId(f[WEAPON] as u16),
            team: f[TEAM] as u8,
            lean: f[LEAN] as f32 / LEAN_SCALE,
            collapse: f[COLLAPSE] as f32 / FRACTION_SCALE,
            slung: f[SLUNG] as f32 / FRACTION_SCALE,
            seat: (f[SEAT] != 0).then(|| SeatedAt {
                carrier: (f[SEAT] >> SEAT_SHIFT) as u32 - 1,
                seat: f[SEAT] as u8,
            }),
            rest_eye: v3(REST_EYE, EYE_SCALE),
            climb: climb_kind_from_bits(f[CLIMB_KIND] as u8),
            climb_t: f[CLIMB_T] as f32 / CLIMB_T_SCALE,
            ledge: v3(LEDGE, POS_SCALE),
            climb_yaw: f[CLIMB_YAW] as f32 / ANGLE_SCALE,
            mount: (f[MOUNT] != 0).then(|| (f[MOUNT] - 1) as u32),
            transit: (f[TRANSIT] != 0).then(|| {
                let index = (f[TRANSIT] >> 3) as u8;
                let kind = match f[TRANSIT] & 3 {
                    1 => TransitKind::In { entry: index },
                    2 => TransitKind::Out { entry: index },
                    _ => TransitKind::Across { from: index },
                };
                Transit { kind, t: f[TRANSIT_T] as f32 / CLIMB_T_SCALE, back: f[TRANSIT] & 4 != 0 }
            }),
            carrying: f[FLAGS] & CARRYING != 0,
            lifting: f[FLAGS] & LIFTING != 0,
            pushing: f[FLAGS] & PUSHING != 0,
            throwing: f[FLAGS] & THROWING != 0,
            throwing_smoke: f[FLAGS] & THROWING_SMOKE != 0,
            care: (f[CARE] != 0).then(|| CarePose {
                site: care_site_from_bits((f[CARE_HOW] & HOW_SITE_MASK) as u8),
                kit: if f[CARE_HOW] & HOW_MEDKIT != 0 { CareKit::Medkit } else { CareKit::Bandage },
                progress: (f[CARE] - 1) as f32 / FRACTION_SCALE,
            }),
            dressed: Dressed { arm: f[CARE_HOW] & HOW_DRESSED_ARM != 0, leg: f[CARE_HOW] & HOW_DRESSED_LEG != 0 },
        }
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            feet: self.feet.lerp(other.feet, t),
            velocity: self.velocity.lerp(other.velocity, t),
            yaw: mix(self.yaw, other.yaw),
            pitch: mix(self.pitch, other.pitch),
            aim_offset: self.aim_offset.lerp(other.aim_offset, t),
            eye_height: mix(self.eye_height, other.eye_height),
            ads: mix(self.ads, other.ads),
            ready: mix(self.ready, other.ready),
            retract: mix(self.retract, other.retract),
            stride: mix(self.stride, other.stride),
            reload: mix(self.reload, other.reload),
            leg_wound: mix(self.leg_wound, other.leg_wound),
            lean: mix(self.lean, other.lean),
            rest_eye: self.rest_eye.lerp(other.rest_eye, t),
            collapse: mix(self.collapse, other.collapse),
            slung: mix(self.slung, other.slung),
            climb_t: mix(self.climb_t, other.climb_t),
            transit: match (self.transit, other.transit) {
                (Some(a), Some(b)) if a.kind == b.kind => Some(Transit { t: mix(a.t, b.t), ..b }),
                (_, b) => b,
            },
            care: match (self.care, other.care) {
                (Some(a), Some(b)) => Some(CarePose { progress: mix(a.progress, b.progress), ..b }),
                (_, b) => b,
            },
            ..*other
        }
    }

    /// Puts this pose on a soldier in the client's sim, so `gun_pose` and `body_input` give the answer
    /// they would on the server, and whatever draws soldiers needs nothing new.
    pub fn apply(&self, soldier: &mut Soldier, weapons: &WeaponTable, move_cfg: &MoveConfig) {
        let body = &mut soldier.body;
        body.pos = self.feet;
        body.vel = self.velocity;
        body.eye_height = self.eye_height;
        body.stride = self.stride;
        body.stance = self.stance;
        body.grounded = self.grounded;
        body.sprinting = self.sprinting;
        body.lean.amount = self.lean;
        body.lean.eye = lean_eye(move_cfg, self.lean, self.eye_height);
        body.climb.kind = self.climb;
        body.climb.t = self.climb_t;
        body.climb.hands = self.ledge;
        body.climb.yaw = self.climb_yaw;
        let climbing = self.climb != ClimbKind::None;
        body.climb.on = self.mount.filter(|_| climbing);
        body.perch.on = self.mount.filter(|_| !climbing);
        soldier.yaw = self.yaw;
        soldier.pitch = self.pitch;
        soldier.gun_retract = self.retract;
        soldier.rest.eye = self.rest_eye;
        soldier.team = self.team;
        soldier.seat = self.seat;
        soldier.transit = self.transit;
        soldier.health.life = self.life;
        soldier.health.leg_wound = self.leg_wound;
        soldier.dressed = self.dressed;
        // Only that the arm is back, for drawing it. How long is the thrower's business.
        soldier.throw.held = if self.throwing { REMOTE_WINDUP } else { 0.0 };
        soldier.inventory.selected = match (self.throwing, self.throwing_smoke) {
            (true, true) => Slot::Smoke,
            (true, false) => Slot::Frag,
            _ => Slot::Primary,
        };
        let weapon = &mut soldier.weapon;
        weapon.weapon = self.weapon;
        weapon.ads = self.ads;
        weapon.ready = self.ready;
        // Reload progress is measured against the full time when `reload_was_empty` is false.
        weapon.reload_was_empty = false;
        weapon.reload_timer = if self.reload > 0.0 { (1.0 - self.reload) * weapons[self.weapon].reload_time } else { 0.0 };
        // Only the sum is sent, because only the sum moves the gun.
        weapon.recoil = self.aim_offset.map(f32::to_degrees);
        weapon.sway = Vec2::ZERO;
        weapon.wander = Vec2::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_sim::Level;
    use prs_sim::protocol::{Buttons, PlayerCmd};

    /// Winding up, everyone else sees what it is they have in their hand.
    #[test]
    fn what_someone_is_about_to_throw_reaches_everyone_else() {
        for (presses, slot) in [(1, Slot::Frag), (2, Slot::Smoke)] {
            let mut sim = Sim::new(Level::named("range").unwrap());
            let id = sim.spawn_soldier(1);
            let mut watcher = Sim::new(Level::named("range").unwrap());
            let seen = watcher.spawn_soldier(2);
            for tick in 0..200 {
                let mut buttons = Buttons::default();
                buttons.set(Buttons::GRENADE, (0..presses).any(|n| tick == 5 + n * 4));
                buttons.set(Buttons::FIRE, tick > 120);
                sim.step(&[(id, PlayerCmd { tick, buttons, ..Default::default() })]);
            }
            assert_eq!(sim.soldiers[id].inventory.selected, slot);
            assert!(sim.soldiers[id].throw.held > 0.0, "should be winding up");
            RemotePose::from_entity(&capture(&sim, id)).apply(&mut watcher.soldiers[seen], &sim.weapons, &sim.move_cfg);
            assert_eq!(watcher.soldiers[seen].inventory.selected, slot);
            assert!(watcher.soldiers[seen].throw.held > 0.0);
        }
    }

    /// Someone treating themselves looks it to everyone else, from the same state, and it ends for
    /// them the moment it ends for the one doing it.
    #[test]
    fn a_treatment_reaches_everyone_else_and_their_hands_go_with_it() {
        use prs_sim::health::Zone;
        let mut sim = Sim::new(Level::named("range").unwrap());
        let id = sim.spawn_soldier(1);
        let mut watcher = Sim::new(Level::named("range").unwrap());
        let seen = watcher.spawn_soldier(2);
        let dt = prs_sim::protocol::TICK_DT;
        let mut stepped = false;
        let mut tick = 0;
        let mut step = |sim: &mut Sim, watcher: &mut Sim, held: bool| {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::BANDAGE, held);
            sim.step(&[(id, PlayerCmd { tick, buttons, ..Default::default() })]);
            tick += 1;
            let pose = RemotePose::from_entity(&capture(sim, id));
            pose.apply(&mut watcher.soldiers[seen], &sim.weapons, &sim.move_cfg);
            let body_cfg = watcher.body_cfg.clone();
            let anim = &mut watcher.soldiers[seen].anim;
            anim.step_care(&body_cfg, pose.care, dt);
            pose
        };
        for _ in 0..30 {
            step(&mut sim, &mut watcher, false);
        }
        sim.hurt(id, Zone::Arm, 15.0);
        while sim.soldiers[id].treatment.is_none_or(|t| t.left > t.total * 0.5) {
            step(&mut sim, &mut watcher, true);
            stepped = true;
        }
        assert!(stepped);
        let pose = step(&mut sim, &mut watcher, true);
        let care = pose.care.expect("they should see it");
        assert_eq!(care.site, prs_sim::body::CareSite::Arm);
        let real = sim.care_pose(id).unwrap();
        assert!((care.progress - real.progress).abs() < 0.01, "{} against {}", care.progress, real.progress);
        let (there, here) = (&watcher.soldiers[seen].anim, &sim.soldiers[id].anim);
        assert!(there.care > 0.9 && (there.care - here.care).abs() < 0.01, "{} against {}", there.care, here.care);

        let finished = (0..400).find(|_| step(&mut sim, &mut watcher, true).care.is_none());
        assert!(finished.is_some(), "the treatment should finish");
        assert!(sim.soldiers[id].dressed.arm);
        assert!(watcher.soldiers[seen].dressed.arm, "the dressing should show");
    }

    /// Everyone else's gun is worked out from what is sent, so a lean has to come through whole.
    #[test]
    fn a_lean_reaches_everyone_else_and_their_gun_is_where_it_really_is() {
        let mut sim = Sim::new(Level::named("range").unwrap());
        let id = sim.spawn_soldier(1);
        let mut watcher = Sim::new(Level::named("range").unwrap());
        let seen = watcher.spawn_soldier(2);
        for tick in 0..120 {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::LEAN_RIGHT, tick == 5);
            sim.step(&[(id, PlayerCmd { tick, buttons, ..Default::default() })]);
        }
        let soldier = &sim.soldiers[id];
        assert!(soldier.body.lean.amount > 0.9);
        let pose = RemotePose::from_entity(&capture(&sim, id));
        pose.apply(&mut watcher.soldiers[seen], &sim.weapons, &sim.move_cfg);
        let there = &watcher.soldiers[seen];
        assert!(there.eye().distance(soldier.eye()) < 0.005, "{} against {}", there.eye(), soldier.eye());
        let (a, b) = (there.gun_pose(&sim.weapons), soldier.gun_pose(&sim.weapons));
        assert!(a.muzzle.distance(b.muzzle) < 0.01, "{} against {}", a.muzzle, b.muzzle);
    }
}
