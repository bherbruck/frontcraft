//! A player's own soldier on the wire: all of it, to the bit, or prediction would disagree with the
//! server on every tick from the first mistake on. To keep that cheap only what has changed since
//! something the client already holds is sent, which even mid-firefight is about half of it.
//!
//! The destructures here list every field with no `..`, so a new field on a soldier stops this
//! compiling until someone decides how it goes on the wire.

use glam::{Vec2, Vec3};
use prs_net::wire::{Reader, WireError, Writer};
use prs_sim::body::BodyAnim;
use prs_sim::health::Health;
use prs_sim::suppression::Suppression;
use prs_sim::ThrowHold;
use prs_sim::inventory::{Inventory, Loadout, Slot, Stash};
use prs_sim::movement::{Climb, ClimbKind, Lean, MoveState, Perch};
use prs_sim::protocol::Buttons;
use prs_sim::weapons::{FireMode, Gun, Rng, WeaponId, WeaponState};
use prs_sim::seats::{Transit, TransitKind};
use prs_sim::body::{CareKit, CareSite};
use prs_sim::{Care, Dressed, GunRest, Soldier, Surface, Treatment};

use crate::events::{surface_from_u8, surface_to_u8};
use crate::remote::{life_from_bits, life_to_bits, stance_from_bits, stance_to_bits};

const MAX_LIST: usize = 256;

const CARE_MEDKIT: u8 = 1 << 4;

pub(crate) fn care_site_to_bits(site: CareSite) -> u8 {
    match site {
        CareSite::Chest => 0,
        CareSite::Arm => 1,
        CareSite::Leg => 2,
        CareSite::Ally => 3,
    }
}

pub(crate) fn care_site_from_bits(bits: u8) -> CareSite {
    match bits & 3 {
        1 => CareSite::Arm,
        2 => CareSite::Leg,
        3 => CareSite::Ally,
        _ => CareSite::Chest,
    }
}

/// Goes over every float a soldier has, in a fixed order.
fn each_float(s: &mut Soldier, f: &mut impl FnMut(&mut f32)) {
    let Soldier {
        body,
        weapon,
        yaw,
        pitch,
        gun_retract,
        rest,
        anim,
        team: _,
        health,
        suppression,
        throw,
        inventory: _,
        treatment: _,
        lifting: _,
        pushing: _,
        lift_refused,
        dressed: _,
        attacker: _,
        home,
        home_yaw,
        loadout: _,
        seed: _,
        lives: _,
        hold_fire: _,
        last_buttons: _,
        present: _,
        seat: _,
        transit: _,
        deck: _,
    } = s;
    let v3 = |v: &mut Vec3, f: &mut dyn FnMut(&mut f32)| {
        f(&mut v.x);
        f(&mut v.y);
        f(&mut v.z);
    };
    let MoveState {
        pos,
        vel,
        stance: _,
        eye_height,
        grounded: _,
        ground_normal,
        ground_surface: _,
        sprinting: _,
        stamina,
        stance_timer,
        stride,
        coyote,
        jump_buffered,
        jump_cooldown,
        landing_timer,
        stamina_delay,
        sprint_locked: _,
        eye_rate,
        lean,
        climb,
        climb_cooldown,
        perch,
        boosted: _,
        held_eye,
    } = body;
    f(held_eye);
    v3(pos, f);
    v3(vel, f);
    v3(ground_normal, f);
    v3(home, f);
    let GunRest { on: _, pivot, along, yaw: rest_yaw, pitch: rest_pitch, feet, amount: rest_amount, settle, eye: rest_eye, reach, height, low, high, refused } = rest;
    v3(pivot, f);
    v3(feet, f);
    v3(rest_eye, f);
    v3(reach, f);
    for v in [along, rest_yaw, rest_pitch, rest_amount, settle, height, low, high, refused] {
        f(v);
    }
    let Lean { side: _, amount, rate, eye } = lean;
    f(&mut eye.x);
    f(&mut eye.y);
    f(amount);
    f(rate);
    let Climb { boosted: _, kind: _, on: _, from, to, top, hands, yaw: climb_yaw, t, duration, rise_end, over_from, fall_from, eye_from, eye_to, end: _, squeeze, squeeze_until, stand_share } = climb;
    v3(from, f);
    v3(to, f);
    v3(hands, f);
    let Perch { on: _, spot } = perch;
    v3(spot, f);
    for v in [top, climb_yaw, t, duration, rise_end, over_from, fall_from, eye_from, eye_to, squeeze, squeeze_until, stand_share, climb_cooldown] {
        f(v);
    }
    for v in [
        eye_height, stamina, stance_timer, stride, coyote, jump_buffered, jump_cooldown, landing_timer,
        stamina_delay, eye_rate, yaw, pitch, gun_retract, home_yaw,
    ] {
        f(v);
    }
    let v2 = |v: &mut Vec2, f: &mut dyn FnMut(&mut f32)| {
        f(&mut v.x);
        f(&mut v.y);
    };
    let WeaponState {
        weapon: _,
        ammo: _,
        magazines: _,
        mode: _,
        drawing: _,
        ads,
        ready,
        reload_timer,
        recoil,
        recoil_vel,
        heat,
        instability,
        shot_shake,
        wander,
        wander_phase,
        wander_seed,
        sway,
        sway_phase,
        sway_amp,
        breath,
        breath_spent: _,
        breath_scale,
        cooldown,
        trigger_was_down: _,
        ads_was_up: _,
        reload_was_empty: _,
        rng: _,
    } = weapon;
    v2(recoil, f);
    v2(recoil_vel, f);
    v2(wander, f);
    v2(sway, f);
    for v in wander_seed {
        f(v);
    }
    for v in [
        ads, ready, reload_timer, heat, instability, shot_shake, wander_phase, sway_phase, sway_amp, breath,
        breath_scale, cooldown,
    ] {
        f(v);
    }
    let BodyAnim {
        hip_yaw,
        turning: _,
        turn_phase,
        turn_weight,
        moving,
        travel,
        air,
        land,
        land_rate,
        last_fall,
        collapse,
        fall_yaw,
        brace,
        slung,
        heave,
        care,
        care_progress,
        care_site: _,
        care_kit: _,
    } = anim;
    v2(travel, f);
    for v in [hip_yaw, turn_phase, turn_weight, moving, air, land, land_rate, last_fall, collapse, fall_yaw, brace, slung, heave, care, care_progress] {
        f(v);
    }
    f(lift_refused);
    let Health { life: _, health, bleed, arm_wound, leg_wound, clock } = health;
    for v in [health, bleed, arm_wound, leg_wound, clock] {
        f(v);
    }
    let Suppression { level, hold, last_shooter: _ } = suppression;
    f(level);
    f(hold);
    let ThrowHold { held, underarm: _, thrown: _ } = throw;
    f(held);
}

fn floats_of(soldier: &Soldier) -> Vec<f32> {
    let mut copy = soldier.clone();
    let mut out = Vec::with_capacity(96);
    each_float(&mut copy, &mut |v| out.push(*v));
    out
}

fn write_gun(gun: &Gun, w: &mut Writer) {
    let Gun { weapon, ammo, mode, magazines } = gun;
    w.u16(weapon.0);
    w.varu(*ammo as u64);
    w.u8(mode_to_u8(*mode));
    write_magazines(magazines, w);
}

fn read_gun(r: &mut Reader) -> Result<Gun, WireError> {
    Ok(Gun {
        weapon: WeaponId(r.u16()?),
        ammo: r.varu32()?,
        mode: mode_from_u8(r.u8()?),
        magazines: read_magazines(r)?,
    })
}

fn write_magazines(magazines: &[u32], w: &mut Writer) {
    w.varu(magazines.len() as u64);
    for &m in magazines {
        w.varu(m as u64);
    }
}

fn read_magazines(r: &mut Reader) -> Result<Vec<u32>, WireError> {
    let n = r.count(MAX_LIST)?;
    let mut magazines = Vec::with_capacity(n);
    for _ in 0..n {
        magazines.push(r.varu32()?);
    }
    Ok(magazines)
}

fn mode_to_u8(mode: FireMode) -> u8 {
    match mode {
        FireMode::Semi => 0,
        FireMode::Auto => 1,
    }
}

fn mode_from_u8(v: u8) -> FireMode {
    if v == 1 { FireMode::Auto } else { FireMode::Semi }
}

fn slot_to_u8(slot: Slot) -> u8 {
    match slot {
        Slot::Primary => 0,
        Slot::Sidearm => 1,
        Slot::Bandage => 2,
        Slot::Medkit => 3,
        Slot::Frag => 4,
        Slot::Smoke => 5,
    }
}

fn slot_from_u8(v: u8) -> Slot {
    match v {
        1 => Slot::Sidearm,
        2 => Slot::Bandage,
        3 => Slot::Medkit,
        4 => Slot::Frag,
        5 => Slot::Smoke,
        _ => Slot::Primary,
    }
}

pub(crate) fn write_stash(stash: &Stash, w: &mut Writer) {
    let Stash { guns, bandages, medkits, frags, smokes } = stash;
    for gun in guns {
        w.u8(gun.is_some() as u8);
        if let Some(gun) = gun {
            write_gun(gun, w);
        }
    }
    w.varu(*bandages as u64);
    w.varu(*medkits as u64);
    w.varu(*frags as u64);
    w.varu(*smokes as u64);
}

pub(crate) fn read_stash(r: &mut Reader) -> Result<Stash, WireError> {
    let mut stash = Stash::default();
    for gun in &mut stash.guns {
        if r.u8()? != 0 {
            *gun = Some(read_gun(r)?);
        }
    }
    stash.bandages = r.varu32()?;
    stash.medkits = r.varu32()?;
    stash.frags = r.varu32()?;
    stash.smokes = r.varu32()?;
    Ok(stash)
}

pub(crate) fn climb_kind_to_bits(kind: ClimbKind) -> u8 {
    match kind {
        ClimbKind::None => 0,
        ClimbKind::Vault => 1,
        ClimbKind::Mantle => 2,
        ClimbKind::PullUp => 3,
        ClimbKind::Stool => 4,
    }
}

pub(crate) fn climb_kind_from_bits(bits: u8) -> ClimbKind {
    match bits {
        1 => ClimbKind::Vault,
        2 => ClimbKind::Mantle,
        3 => ClimbKind::PullUp,
        4 => ClimbKind::Stool,
        _ => ClimbKind::None,
    }
}

fn bits(flags: &[bool]) -> u16 {
    flags.iter().enumerate().fold(0, |all, (i, &on)| all | (on as u16) << i)
}

/// Everything that isn't a float, in its own encoding, so it can be compared and left out as one.
fn write_discrete(s: &Soldier, w: &mut Writer) {
    let Soldier {
        body,
        weapon,
        yaw: _,
        pitch: _,
        gun_retract: _,
        rest,
        anim,
        team,
        health,
        suppression,
        throw,
        inventory,
        treatment,
        lifting,
        pushing,
        lift_refused: _,
        dressed,
        attacker,
        home: _,
        home_yaw: _,
        loadout,
        seed,
        lives,
        hold_fire,
        last_buttons,
        present,
        seat,
        transit: _,
        deck: _,
    } = s;
    w.u8(stance_to_bits(body.stance));
    w.u8(surface_to_u8(body.ground_surface));
    w.u8(life_to_bits(health.life));
    w.u8(body.lean.side as u8);
    w.u8(climb_kind_to_bits(body.climb.kind));
    w.u8(stance_to_bits(body.climb.end));
    w.varu(body.climb.on.map_or(0, |on| on as u64 + 1));
    w.varu(body.perch.on.map_or(0, |on| on as u64 + 1));
    w.u16(bits(&[
        body.grounded,
        body.sprinting,
        body.sprint_locked,
        weapon.drawing,
        weapon.breath_spent,
        weapon.trigger_was_down,
        weapon.ads_was_up,
        weapon.reload_was_empty,
        anim.turning,
        *hold_fire,
        *present,
        body.boosted,
        body.climb.boosted,
        rest.on,
    ]));
    w.u16(weapon.weapon.0);
    w.varu(weapon.ammo as u64);
    write_magazines(&weapon.magazines, w);
    w.u8(mode_to_u8(weapon.mode));
    w.varu(weapon.rng.0 as u64);
    w.u8(*team);
    let Inventory { stash, held, selected } = inventory;
    write_stash(stash, w);
    w.u8(slot_to_u8(*held));
    w.u8(slot_to_u8(*selected));
    w.varu(lifting.map_or(0, |v| v as u64 + 1));
    w.varu(pushing.map_or(0, |(v, _)| v as u64 + 1));
    w.u8(pushing.map_or(0, |(_, spot)| spot));
    w.u8(treatment.is_some() as u8);
    if let Some(Treatment { care, patient, left, total }) = treatment {
        w.u8(matches!(care, Care::Medkit) as u8);
        w.varu(*patient as u64);
        w.f32(*left);
        w.f32(*total);
    }
    w.u8(dressed.arm as u8 | (dressed.leg as u8) << 1 | care_site_to_bits(anim.care_site) << 2 | (anim.care_kit == CareKit::Medkit) as u8 * CARE_MEDKIT);
    w.varu(attacker.map_or(0, |a| a as u64 + 1));
    w.varu(suppression.last_shooter.map_or(0, |a| a as u64 + 1));
    w.u8(throw.underarm as u8);
    w.varu(throw.thrown as u64);
    let Loadout { primary, sidearm, bandages, medkits, frags, smokes } = loadout;
    w.varu(*frags as u64);
    w.varu(*smokes as u64);
    w.u16(primary.0);
    w.varu(sidearm.map_or(0, |id| id.0 as u64 + 1));
    w.varu(*bandages as u64);
    w.varu(*medkits as u64);
    w.varu(*seed as u64);
    w.varu(*lives as u64);
    w.varu(last_buttons.0 as u64);
    match seat {
        Some(at) => {
            w.varu(at.carrier as u64 + 1);
            w.u8(at.seat);
        }
        None => w.varu(0),
    }
}

fn read_discrete(s: &mut Soldier, r: &mut Reader) -> Result<(), WireError> {
    s.body.stance = stance_from_bits(r.u8()?);
    s.body.ground_surface = surface_from_u8(r.u8()?)?;
    s.health.life = life_from_bits(r.u8()?);
    s.body.lean.side = r.u8()? as i8;
    s.body.climb.kind = climb_kind_from_bits(r.u8()?);
    s.body.climb.end = stance_from_bits(r.u8()?);
    s.body.climb.on = r.varu32()?.checked_sub(1);
    s.body.perch.on = r.varu32()?.checked_sub(1);
    let flag = r.u16()?;
    let on = |bit: u32| flag & (1 << bit) != 0;
    s.body.grounded = on(0);
    s.body.sprinting = on(1);
    s.body.sprint_locked = on(2);
    s.weapon.drawing = on(3);
    s.weapon.breath_spent = on(4);
    s.weapon.trigger_was_down = on(5);
    s.weapon.ads_was_up = on(6);
    s.weapon.reload_was_empty = on(7);
    s.anim.turning = on(8);
    s.hold_fire = on(9);
    s.present = on(10);
    s.body.boosted = on(11);
    s.body.climb.boosted = on(12);
    s.rest.on = on(13);
    s.weapon.weapon = WeaponId(r.u16()?);
    s.weapon.ammo = r.varu32()?;
    s.weapon.magazines = read_magazines(r)?;
    s.weapon.mode = mode_from_u8(r.u8()?);
    s.weapon.rng = Rng(r.varu32()?);
    s.team = r.u8()?;
    s.inventory.stash = read_stash(r)?;
    s.inventory.held = slot_from_u8(r.u8()?);
    s.inventory.selected = slot_from_u8(r.u8()?);
    s.lifting = r.varu32()?.checked_sub(1);
    let pushed = r.varu32()?.checked_sub(1);
    let spot = r.u8()?;
    s.pushing = pushed.map(|v| (v, spot));
    s.treatment = if r.u8()? != 0 {
        Some(Treatment {
            care: if r.u8()? != 0 { Care::Medkit } else { Care::Bandage },
            patient: r.varu32()? as usize,
            left: r.f32()?,
            total: r.f32()?,
        })
    } else {
        None
    };
    let care = r.u8()?;
    s.dressed = Dressed { arm: care & 1 != 0, leg: care & 2 != 0 };
    s.anim.care_site = care_site_from_bits(care >> 2);
    s.anim.care_kit = if care & CARE_MEDKIT != 0 { CareKit::Medkit } else { CareKit::Bandage };
    s.attacker = r.varu32()?.checked_sub(1).map(|a| a as usize);
    s.suppression.last_shooter = r.varu32()?.checked_sub(1).map(|a| a as usize);
    s.throw.underarm = r.u8()? != 0;
    s.throw.thrown = r.varu32()?;
    s.loadout.frags = r.varu32()?;
    s.loadout.smokes = r.varu32()?;
    s.loadout.primary = WeaponId(r.u16()?);
    s.loadout.sidearm = r.varu32()?.checked_sub(1).map(|id| WeaponId(id as u16));
    s.loadout.bandages = r.varu32()?;
    s.loadout.medkits = r.varu32()?;
    s.seed = r.varu32()?;
    s.lives = r.varu32()?;
    s.last_buttons = Buttons(r.varu32()?);
    s.seat = match r.varu32()? {
        0 => None,
        carrier => Some(prs_sim::seats::SeatedAt { carrier: carrier - 1, seat: r.u8()? }),
    };
    Ok(())
}

fn discrete_of(soldier: &Soldier) -> Vec<u8> {
    let mut w = Writer::default();
    write_discrete(soldier, &mut w);
    w.buf
}

/// What a client starts from with nothing to go on. Every float and discrete value comes off the wire.
fn blank() -> Soldier {
    Soldier {
        body: MoveState {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            stance: Default::default(),
            eye_height: 0.0,
            grounded: false,
            ground_normal: Vec3::ZERO,
            ground_surface: Surface::Dirt,
            sprinting: false,
            stamina: 0.0,
            stance_timer: 0.0,
            stride: 0.0,
            coyote: 0.0,
            jump_buffered: 0.0,
            jump_cooldown: 0.0,
            landing_timer: 0.0,
            stamina_delay: 0.0,
            sprint_locked: false,
            eye_rate: 0.0,
            lean: Lean::default(),
            climb: Climb::default(),
            climb_cooldown: 0.0,
            perch: Perch::default(),
            boosted: false,
            held_eye: 0.0,
        },
        weapon: WeaponState {
            weapon: WeaponId(0),
            ammo: 0,
            magazines: Vec::new(),
            mode: FireMode::Semi,
            drawing: false,
            ads: 0.0,
            ready: 0.0,
            reload_timer: 0.0,
            recoil: Vec2::ZERO,
            recoil_vel: Vec2::ZERO,
            heat: 0.0,
            instability: 0.0,
            shot_shake: 0.0,
            wander: Vec2::ZERO,
            wander_phase: 0.0,
            wander_seed: [0.0; 4],
            sway: Vec2::ZERO,
            sway_phase: 0.0,
            sway_amp: 0.0,
            breath: 0.0,
            breath_spent: false,
            breath_scale: 0.0,
            cooldown: 0.0,
            trigger_was_down: false,
            ads_was_up: false,
            reload_was_empty: false,
            rng: Rng(0),
        },
        yaw: 0.0,
        pitch: 0.0,
        gun_retract: 0.0,
        rest: GunRest::default(),
        anim: BodyAnim {
            hip_yaw: 0.0,
            turning: false,
            turn_phase: 0.0,
            turn_weight: 0.0,
            moving: 0.0,
            travel: Vec2::ZERO,
            air: 0.0,
            land: 0.0,
            land_rate: 0.0,
            last_fall: 0.0,
            collapse: 0.0,
            fall_yaw: 0.0,
            brace: 0.0,
            slung: 0.0,
            heave: 0.0,
            care: 0.0,
            care_progress: 0.0,
            care_site: CareSite::Chest,
            care_kit: CareKit::Bandage,
        },
        team: 0,
        health: Health { life: prs_sim::health::Life::Up, health: 0.0, bleed: 0.0, arm_wound: 0.0, leg_wound: 0.0, clock: 0.0 },
        suppression: Suppression::default(),
        throw: ThrowHold::default(),
        inventory: Inventory::default(),
        treatment: None,
        lifting: None,
        pushing: None,
        lift_refused: 0.0,
        dressed: Dressed::default(),
        attacker: None,
        home: Vec3::ZERO,
        home_yaw: 0.0,
        loadout: Loadout { primary: WeaponId(0), sidearm: None, bandages: 0, medkits: 0, frags: 0, smokes: 0 },
        seed: 0,
        lives: 0,
        hold_fire: false,
        last_buttons: Buttons::default(),
        present: true,
        seat: None,
        transit: None,
        deck: None,
    }
}


/// The floats that differ from `was`, as a mask and then the values to the bit. Shared with anything
/// else that is sent whole to its owner.
pub(crate) fn write_changed(now: &[f32], was: &[f32], w: &mut Writer) {
    let mut mask = vec![0u64; now.len().div_ceil(64)];
    for (i, (a, b)) in now.iter().zip(was).enumerate() {
        if a.to_bits() != b.to_bits() {
            mask[i / 64] |= 1 << (i % 64);
        }
    }
    for word in mask {
        w.varu(word);
    }
    for (a, b) in now.iter().zip(was) {
        if a.to_bits() != b.to_bits() {
            w.f32(*a);
        }
    }
}

/// Reads what [`write_changed`] wrote over the `count` floats a visitor goes through.
pub(crate) fn read_changed(
    count: usize,
    r: &mut Reader,
    each: impl FnOnce(&mut dyn FnMut(&mut f32)),
) -> Result<(), WireError> {
    let mut mask = vec![0u64; count.div_ceil(64)];
    for word in &mut mask {
        *word = r.varu()?;
    }
    let mut i = 0;
    let mut failed = None;
    each(&mut |v: &mut f32| {
        if mask[i / 64] & (1 << (i % 64)) != 0 {
            match r.f32() {
                Ok(value) => *v = value,
                Err(e) => failed = Some(e),
            }
        }
        i += 1;
    });
    failed.map_or(Ok(()), Err)
}

/// Whether they are at the same point of the same door and the same place on the same hull.
fn places_agree(a: &Soldier, b: &Soldier, epsilon: f32) -> bool {
    let transit = match (a.transit, b.transit) {
        (None, None) => true,
        (Some(x), Some(y)) => x.kind == y.kind && x.back == y.back && (x.t - y.t).abs() <= epsilon,
        _ => false,
    };
    let deck = match (a.deck, b.deck) {
        (None, None) => true,
        (Some(x), Some(y)) => x.vehicle == y.vehicle && x.local.distance(y.local) <= epsilon,
        _ => false,
    };
    transit && deck
}

/// Going through a door and standing on a hull come and go, so they are not among the floats, whose
/// number has to be the same on both sides before either knows the rest.
fn write_places(s: &Soldier, w: &mut Writer) {
    match s.transit {
        Some(Transit { kind, t, back }) => {
            let (tag, index) = match kind {
                TransitKind::In { entry } => (1, entry),
                TransitKind::Out { entry } => (2, entry),
                TransitKind::Across { from } => (3, from),
            };
            w.u8(tag | (back as u8) << 2);
            w.u8(index);
            w.f32(t);
        }
        None => w.u8(0),
    }
    match s.deck {
        Some(deck) => {
            w.varu(deck.vehicle as u64 + 1);
            w.vec3(deck.local);
        }
        None => w.varu(0),
    }
}

fn read_places(s: &mut Soldier, r: &mut Reader) -> Result<(), WireError> {
    let tag = r.u8()?;
    s.transit = if tag == 0 {
        None
    } else {
        let index = r.u8()?;
        let kind = match tag & 3 {
            1 => TransitKind::In { entry: index },
            2 => TransitKind::Out { entry: index },
            _ => TransitKind::Across { from: index },
        };
        Some(Transit { kind, t: r.f32()?, back: tag & 4 != 0 })
    };
    s.deck = match r.varu32()? {
        0 => None,
        vehicle => Some(prs_sim::OnDeck { vehicle: vehicle - 1, local: r.vec3()? }),
    };
    Ok(())
}

pub(crate) fn write(soldier: &Soldier, base: Option<&Soldier>, w: &mut Writer) {
    write_places(soldier, w);
    let now = floats_of(soldier);
    let was = base.map_or_else(|| floats_of(&blank()), floats_of);
    write_changed(&now, &was, w);
    let discrete = discrete_of(soldier);
    let same = base.is_some_and(|b| discrete_of(b) == discrete);
    w.u8(!same as u8);
    if !same {
        w.buf.extend_from_slice(&discrete);
    }
}

pub(crate) fn read(base: Option<&Soldier>, r: &mut Reader) -> Result<Soldier, WireError> {
    let mut soldier = base.map_or_else(blank, Soldier::clone);
    read_places(&mut soldier, r)?;
    let count = floats_of(&soldier).len();
    read_changed(count, r, |each| each_float(&mut soldier, &mut { each }))?;
    if r.u8()? != 0 {
        read_discrete(&mut soldier, r)?;
    }
    Ok(soldier)
}

/// Whether two states agree, to `epsilon` on anything continuous and exactly on anything discrete.
pub(crate) fn matches(a: &Soldier, b: &Soldier, epsilon: f32) -> bool {
    floats_of(a).iter().zip(floats_of(b)).all(|(x, y)| (x - y).abs() <= epsilon) && discrete_of(a) == discrete_of(b) && places_agree(a, b, epsilon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_sim::movement::Stance;
    use prs_sim::protocol::PlayerCmd;
    use prs_sim::{Level, Sim};

    /// A soldier who has been through a bit of everything, so nearly every field has a real value.
    fn lived_in() -> (Sim, Soldier) {
        let mut sim = Sim::new(Level::flat(200.0));
        let id = sim.spawn_soldier(77);
        let other = sim.spawn_soldier(78);
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, true);
        buttons.set(Buttons::ADS, true);
        for i in 0..200u32 {
            let cmd = PlayerCmd {
                tick: i + 1,
                wish_move: Vec2::new(0.3, 0.9),
                yaw: i as f32 * 0.02,
                pitch: 0.1,
                buttons,
                ..Default::default()
            };
            sim.step(&[(id, cmd)]);
        }
        sim.hurt(id, prs_sim::health::Zone::Leg, 20.0);
        sim.hurt(id, prs_sim::health::Zone::Arm, 20.0);
        sim.step(&[]);
        sim.soldiers[id].attacker = Some(other);
        sim.soldiers[id].suppression = Suppression { level: 0.61, hold: 0.47, last_shooter: Some(other) };
        sim.soldiers[id].treatment =
            Some(Treatment { care: Care::Medkit, patient: other, left: 1.5, total: 3.25 });
        let soldier = sim.soldiers[id].clone();
        (sim, soldier)
    }

    fn round_trip(soldier: &Soldier, base: Option<&Soldier>) -> (Soldier, usize) {
        let mut w = Writer::default();
        write(soldier, base, &mut w);
        (read(base, &mut Reader::new(&w.buf)).unwrap(), w.buf.len())
    }

    #[test]
    fn a_soldier_comes_through_exactly_with_nothing_to_go_on() {
        let (_, soldier) = lived_in();
        assert_eq!(round_trip(&soldier, None).0, soldier);
    }

    #[test]
    fn a_soldier_comes_through_exactly_against_a_baseline_and_cheaper() {
        let (mut sim, _) = lived_in();
        let id = 0;
        let walk = PlayerCmd { wish_move: Vec2::new(0.0, 1.0), yaw: 1.0, ..Default::default() };
        sim.step(&[(id, walk)]);
        let was = sim.soldiers[id].clone();
        let (_, full) = round_trip(&was, None);
        sim.step(&[(id, walk)]);
        let now = sim.soldiers[id].clone();
        let (got, delta) = round_trip(&now, Some(&was));
        assert_eq!(got, now);
        assert!(delta * 3 < full * 2, "{delta} against {full}");
    }

    /// `PartialEq` on floats can't tell 0.0 from -0.0, and they step differently from there.
    #[test]
    fn every_float_keeps_its_bits() {
        let (_, mut soldier) = lived_in();
        each_float(&mut soldier, &mut |v| *v = -0.0);
        let (got, _) = round_trip(&soldier, None);
        assert!(floats_of(&got).iter().all(|v| v.to_bits() == (-0.0f32).to_bits()));
    }

    /// Every float the visitor touches moves the encoding, so none is quietly left out or shared.
    #[test]
    fn every_float_is_distinct_and_is_sent() {
        let (_, soldier) = lived_in();
        let mut numbered = soldier.clone();
        let mut n = 0.0;
        each_float(&mut numbered, &mut |v| {
            n += 1.0;
            *v = n;
        });
        let got = floats_of(&round_trip(&numbered, None).0);
        for (i, v) in got.iter().enumerate() {
            assert_eq!(*v, (i + 1) as f32);
        }
    }

    /// The same for what isn't a float: change each thing, and the encoding sees it.
    #[test]
    fn every_discrete_change_is_seen() {
        let (_, soldier) = lived_in();
        type Change = fn(&mut Soldier);
        let changes: &[Change] = &[
            |s| s.body.stance = prs_sim::movement::Stance::Prone,
            |s| s.body.grounded ^= true,
            |s| s.body.sprinting ^= true,
            |s| s.body.sprint_locked ^= true,
            |s| s.body.lean.side -= 1,
            |s| s.body.climb.kind = ClimbKind::PullUp,
            |s| s.body.climb.end = Stance::Crouch,
            |s| s.body.climb.on = Some(3),
            |s| s.body.perch.on = Some(4),
            |s| s.body.boosted ^= true,
            |s| s.rest.on ^= true,
            |s| s.body.climb.boosted ^= true,
            |s| s.body.ground_surface = Surface::Metal,
            |s| s.weapon.weapon = WeaponId(3),
            |s| s.weapon.ammo += 1,
            |s| s.weapon.magazines.push(1),
            |s| s.weapon.mode = if s.weapon.mode == FireMode::Auto { FireMode::Semi } else { FireMode::Auto },
            |s| s.weapon.drawing ^= true,
            |s| s.weapon.breath_spent ^= true,
            |s| s.weapon.trigger_was_down ^= true,
            |s| s.weapon.ads_was_up ^= true,
            |s| s.weapon.reload_was_empty ^= true,
            |s| s.weapon.rng = Rng(s.weapon.rng.0 ^ 1),
            |s| s.anim.turning ^= true,
            |s| s.team += 1,
            |s| s.health.life = prs_sim::health::Life::Dead,
            |s| s.suppression.last_shooter = None,
            |s| s.inventory.stash.bandages += 1,
            |s| s.inventory.stash.frags += 1,
            |s| s.inventory.stash.smokes += 1,
            |s| s.throw.underarm ^= true,
            |s| s.throw.thrown += 1,
            |s| s.loadout.frags += 1,
            |s| s.loadout.smokes += 1,
            |s| s.inventory.held = Slot::Sidearm,
            |s| s.inventory.selected = Slot::Medkit,
            |s| s.treatment = None,
            |s| s.dressed.arm ^= true,
            |s| s.dressed.leg ^= true,
            |s| s.anim.care_site = CareSite::Leg,
            |s| s.anim.care_kit = CareKit::Medkit,
            |s| s.attacker = None,
            |s| s.loadout.medkits += 1,
            |s| s.seed += 1,
            |s| s.lives += 1,
            |s| s.hold_fire ^= true,
            |s| s.last_buttons = Buttons(s.last_buttons.0 ^ 1),
            |s| s.present ^= true,
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut changed = soldier.clone();
            change(&mut changed);
            assert_ne!(discrete_of(&changed), discrete_of(&soldier), "change {i}");
            assert_eq!(round_trip(&changed, Some(&soldier)).0, changed, "change {i}");
        }
    }

    #[test]
    fn a_tiny_float_difference_is_no_disagreement_but_a_real_one_is() {
        let (_, soldier) = lived_in();
        let mut close = soldier.clone();
        close.body.pos.x += 1e-5;
        assert!(matches(&soldier, &close, 1e-3));
        close.body.pos.x += 0.01;
        assert!(!matches(&soldier, &close, 1e-3));
        let mut other = soldier.clone();
        other.weapon.rng = Rng(other.weapon.rng.0 ^ 1);
        assert!(!matches(&soldier, &other, 1e-3));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let (_, soldier) = lived_in();
        let mut w = Writer::default();
        write(&soldier, None, &mut w);
        for cut in 0..w.buf.len() {
            assert!(read(None, &mut Reader::new(&w.buf[..cut])).is_err(), "cut at {cut}");
        }
    }
}
