//! Firearm handling. This is where "no running and gunning" lives.
//!
//! A bullet leaves along the gun's axis, give or take the weapon's own mechanical dispersion: a
//! small fixed cone from its specs that nothing the shooter does can widen. So nearly every miss is
//! the gun visibly pointing somewhere else, and what the sights show is what the shot does. The gun
//! is pushed off target by three things:
//!
//! * **Recoil** is an impulse on a spring, a sharp kick and a recovery. The random part of each
//!   kick is larger than the predictable part and grows with sustained fire. Shots come faster
//!   than anyone can react, so a learned pull-down cannot hold a burst on target.
//! * **Instability** is the gun being unsettled. Moving, turning, changing stance and firing all
//!   make it wander, and it calms down slowly once you stop. Raised sights start out shaky.
//! * **Sway** is breathing. Slow and readable, reduced by stance and by holding your breath.
//!
//! Mastery is therefore about discipline: stop, get low, let it settle, time the breath, take
//! one shot, and wait for the sights to come back.

mod config;
mod pose;

pub use config::*;
pub use pose::*;

use glam::Vec2;

/// What the weapon needs to know about the body holding it. Deliberately not the movement
/// crate's types, so the two stay independent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeaponInput {
    pub fire_held: bool,
    pub ads_held: bool,
    pub reload_pressed: bool,
    pub mode_pressed: bool,
    pub breath_held: bool,
    /// View rotation speed in degrees per second, from the player's own mouse movement.
    pub turn_rate: f32,
    /// Something is in the way of the barrel, so the gun has to come down.
    pub muzzle_blocked: bool,
    /// The soldier is changing to something else, or can no longer hold it up.
    pub stowing: bool,
    pub body: BodyState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    pub posture: Posture,
    /// 0 = still, 1 = running speed.
    pub speed_fraction: f32,
    pub airborne: bool,
    pub sprinting: bool,
    pub changing_stance: bool,
    /// How quickly the lean is changing, in leans a second.
    pub lean_rate: f32,
    /// 0..1
    pub stamina: f32,
    /// How badly an arm is wounded, 0..1.
    pub arm_wound: f32,
    /// How much blood is gone, 0..1.
    pub blood_loss: f32,
    /// How far the gun has settled onto something it is rested on, 0..1.
    pub rest: f32,
    /// How much they are being shot at, 0..1.
    pub suppression: f32,
}

impl Default for BodyState {
    fn default() -> Self {
        Self {
            posture: Posture::Stand,
            speed_fraction: 0.0,
            airborne: false,
            sprinting: false,
            changing_stance: false,
            lean_rate: 0.0,
            stamina: 1.0,
            arm_wound: 0.0,
            blood_loss: 0.0,
            rest: 0.0,
            suppression: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Posture {
    #[default]
    Stand,
    Crouch,
    Prone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeaponEvent {
    /// Offsets are radians from the player's look direction: x = yaw, y = pitch. `gun_offset` is
    /// where the gun pointed as it fired, and `direction_offset` adds this round's dispersion.
    Fired { gun_offset: Vec2, direction_offset: Vec2 },
    DryFire,
    ReloadStarted,
    ReloadFinished,
    ModeChanged(FireMode),
    AdsRaised,
    AdsLowered,
}

/// One physical gun and the ammunition that goes with it: what stays the same when it is put away,
/// dropped or picked up by someone else.
#[derive(Clone, Debug, PartialEq)]
pub struct Gun {
    pub weapon: WeaponId,
    /// Rounds in the gun, the chamber included.
    pub ammo: u32,
    pub mode: FireMode,
    /// Rounds left in each spare magazine, tracked one by one.
    pub magazines: Vec<u32>,
}

impl Gun {
    /// As issued: loaded, with its full set of spare magazines.
    pub fn issued(weapon: WeaponId, cfg: &WeaponConfig) -> Self {
        Self {
            weapon,
            ammo: cfg.magazine,
            mode: cfg.default_mode,
            magazines: vec![cfg.magazine; cfg.spare_magazines as usize],
        }
    }

    pub fn rounds(&self) -> u32 {
        self.ammo + self.magazines.iter().sum::<u32>()
    }
}

/// The gun in a soldier's hands and how it is being held. What the hands are doing carries over from
/// one gun to the next, so swapping guns does not settle a shaking soldier. Every field is public: a
/// client has to restore all of it, the random generator too, to replay from what the server says.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponState {
    pub weapon: WeaponId,
    /// Rounds in the gun, the chamber included.
    pub ammo: u32,
    /// Rounds left in each spare magazine.
    pub magazines: Vec<u32>,
    pub mode: FireMode,
    /// Bringing a newly drawn gun up, which is slower than raising it from a run.
    pub drawing: bool,
    /// 0 = hip, 1 = looking through the sights.
    pub ads: f32,
    /// 0 = lowered for sprinting, 1 = ready to fire.
    pub ready: f32,
    /// Seconds left in a reload, 0 when not reloading.
    pub reload_timer: f32,
    /// Sights offset from recoil, degrees: x = yaw, y = pitch.
    pub recoil: Vec2,
    pub recoil_vel: Vec2,
    /// How hot the current burst is. Each shot adds 1, and it bleeds off when the trigger rests.
    pub heat: f32,
    /// How far the gun is currently wandering, degrees.
    pub instability: f32,
    pub shot_shake: f32,
    /// Sights offset from instability, degrees.
    pub wander: Vec2,
    pub wander_phase: f32,
    pub wander_seed: [f32; 4],
    /// Sights offset from breathing, degrees.
    pub sway: Vec2,
    pub sway_phase: f32,
    pub sway_amp: f32,
    /// 1 = full lungs. Holding breath spends it.
    pub breath: f32,
    pub breath_spent: bool,
    pub breath_scale: f32,
    pub cooldown: f32,
    pub trigger_was_down: bool,
    pub ads_was_up: bool,
    pub reload_was_empty: bool,
    pub rng: Rng,
}

impl WeaponState {
    pub fn new(cfg: &WeaponConfig, seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let wander_seed = [0; 4].map(|_| rng.next_f32() * std::f32::consts::TAU);
        Self {
            weapon: WeaponId::default(),
            ammo: cfg.magazine,
            magazines: vec![cfg.magazine; cfg.spare_magazines as usize],
            mode: cfg.default_mode,
            drawing: false,
            ads: 0.0,
            ready: 1.0,
            reload_timer: 0.0,
            recoil: Vec2::ZERO,
            recoil_vel: Vec2::ZERO,
            heat: 0.0,
            instability: cfg.stability.hip_stand,
            shot_shake: 0.0,
            wander: Vec2::ZERO,
            wander_phase: 0.0,
            wander_seed,
            sway: Vec2::ZERO,
            sway_phase: (seed % 628) as f32 * 0.01,
            sway_amp: 0.0,
            breath: 1.0,
            breath_spent: false,
            breath_scale: 1.0,
            cooldown: 0.0,
            trigger_was_down: false,
            ads_was_up: false,
            reload_was_empty: false,
            rng,
        }
    }

    /// Starts out holding this gun, ready to fire.
    pub fn holding(gun: Gun, cfg: &WeaponConfig, seed: u32) -> Self {
        let mut state = Self::new(cfg, seed);
        state.put_in_hands(gun);
        state.ready = 1.0;
        state.drawing = false;
        state
    }

    /// Changes the gun in the hands and returns the one that was there. The new one still has to be
    /// brought up, and a reload does not carry over.
    pub fn put_in_hands(&mut self, gun: Gun) -> Gun {
        let old = self.gun();
        self.weapon = gun.weapon;
        self.ammo = gun.ammo;
        self.mode = gun.mode;
        self.magazines = gun.magazines;
        self.reload_timer = 0.0;
        self.ready = 0.0;
        self.drawing = true;
        old
    }

    /// Every round of it, the chamber and spare magazines included.
    pub fn rounds(&self) -> u32 {
        self.ammo + self.magazines.iter().sum::<u32>()
    }

    /// Bringing a newly drawn gun up.
    pub fn drawing(&self) -> bool {
        self.drawing
    }

    /// The gun in the hands, as it would be if put away.
    pub fn gun(&self) -> Gun {
        Gun { weapon: self.weapon, ammo: self.ammo, mode: self.mode, magazines: self.magazines.clone() }
    }

    pub fn reloading(&self) -> bool {
        self.reload_timer > 0.0
    }

    /// How far through the current reload, from 0 at the start to 1 at the end. 0 when not reloading.
    pub fn reload_progress(&self, cfg: &WeaponConfig) -> f32 {
        if !self.reloading() {
            return 0.0;
        }
        let total = if self.reload_was_empty { cfg.reload_empty_time } else { cfg.reload_time };
        (1.0 - self.reload_timer / total).clamp(0.0, 1.0)
    }

    /// Where the gun points relative to the look direction, in radians: x = yaw, y = pitch.
    /// Bullets leave along exactly this line.
    pub fn aim_offset(&self) -> Vec2 {
        (self.recoil + self.sway + self.wander) * std::f32::consts::PI / 180.0
    }
}

pub fn step(
    state: &mut WeaponState,
    cfg: &WeaponConfig,
    input: &WeaponInput,
    dt: f32,
    events: &mut Vec<WeaponEvent>,
) {
    update_handling(state, cfg, input, dt, events);
    update_reload(state, cfg, input, dt, events);
    update_recoil(state, cfg, dt);
    update_sway(state, cfg, input, dt);
    update_stability(state, cfg, input, dt);
    update_trigger(state, cfg, input, dt, events);
}

fn approach(current: f32, target: f32, max_delta: f32) -> f32 {
    current + (target - current).clamp(-max_delta, max_delta)
}

fn update_handling(
    state: &mut WeaponState,
    cfg: &WeaponConfig,
    input: &WeaponInput,
    dt: f32,
    events: &mut Vec<WeaponEvent>,
) {
    let body = &input.body;
    let lowered = body.sprinting || input.muzzle_blocked || input.stowing;
    let ready_target = if lowered { 0.0 } else { 1.0 };
    let ready_time = if input.stowing {
        cfg.stow_time
    } else if lowered {
        cfg.lower_time
    } else if state.drawing {
        cfg.draw_time
    } else {
        cfg.raise_time
    };
    state.ready = approach(state.ready, ready_target, dt / ready_time);
    if state.ready >= 1.0 {
        state.drawing = false;
    }

    let can_aim = !lowered && !state.reloading() && !body.changing_stance && state.ready > 0.5;
    let ads_target = if input.ads_held && can_aim { 1.0 } else { 0.0 };
    state.ads = approach(state.ads, ads_target, dt / cfg.ads_time);

    let up = ads_target > 0.5;
    if up != state.ads_was_up {
        state.ads_was_up = up;
        events.push(if up { WeaponEvent::AdsRaised } else { WeaponEvent::AdsLowered });
    }

    if input.mode_pressed && cfg.modes.len() > 1 {
        let current = cfg.modes.iter().position(|m| *m == state.mode).unwrap_or(0);
        state.mode = cfg.modes[(current + 1) % cfg.modes.len()];
        events.push(WeaponEvent::ModeChanged(state.mode));
    }
}

fn update_reload(
    state: &mut WeaponState,
    cfg: &WeaponConfig,
    input: &WeaponInput,
    dt: f32,
    events: &mut Vec<WeaponEvent>,
) {
    // Putting the gun away abandons a reload. Nothing has moved until a reload finishes.
    if input.stowing {
        state.reload_timer = 0.0;
        return;
    }
    if state.reloading() {
        state.reload_timer -= dt;
        if state.reload_timer <= 0.0 {
            state.reload_timer = 0.0;
            // The fullest magazine goes in and the old one is kept with whatever it still holds. A
            // chambered round stays in the gun.
            let chambered = if state.reload_was_empty { 0 } else { 1 };
            if let Some(fullest) = fullest_magazine(&state.magazines) {
                let taken = state.magazines.swap_remove(fullest);
                let old = state.ammo.saturating_sub(chambered);
                if old > 0 {
                    state.magazines.push(old);
                }
                state.ammo = chambered + taken;
                events.push(WeaponEvent::ReloadFinished);
            }
        }
    } else if input.reload_pressed && worth_reloading(state) {
        state.reload_was_empty = state.ammo == 0;
        state.reload_timer =
            if state.reload_was_empty { cfg.reload_empty_time } else { cfg.reload_time };
        events.push(WeaponEvent::ReloadStarted);
    }
}

fn fullest_magazine(magazines: &[u32]) -> Option<usize> {
    magazines.iter().enumerate().max_by_key(|(i, rounds)| (**rounds, *i)).map(|(i, _)| i)
}

/// Only when some magazine holds more than the one in the gun. Otherwise it swaps for nothing.
fn worth_reloading(state: &WeaponState) -> bool {
    let in_magazine = state.ammo.saturating_sub(1);
    fullest_magazine(&state.magazines).is_some_and(|i| state.magazines[i] > in_magazine)
}

fn update_recoil(state: &mut WeaponState, cfg: &WeaponConfig, dt: f32) {
    // Critically damped spring, integrated exactly so it does not depend on the tick rate.
    let w = cfg.recoil.recovery_rate;
    let e = (-w * dt).exp();
    let (x, v) = (state.recoil, state.recoil_vel);
    let c = v + x * w;
    state.recoil = (x + c * dt) * e;
    state.recoil_vel = (v - c * w * dt) * e;

    state.heat *= (-dt / cfg.recoil.heat_decay).exp();
}

fn update_sway(state: &mut WeaponState, cfg: &WeaponConfig, input: &WeaponInput, dt: f32) {
    let s = &cfg.sway;
    let posture_amp = match input.body.posture {
        Posture::Stand => s.stand,
        Posture::Crouch => s.crouch,
        Posture::Prone => s.prone,
    };
    let exertion = 1.0
        + (1.0 - input.body.stamina) * s.exertion
        + input.body.arm_wound * s.arm_wound
        + input.body.blood_loss * s.blood_loss
        + input.body.suppression * s.suppression;

    let holding = input.breath_held && state.ads > 0.9 && !state.breath_spent;
    if holding {
        state.breath = (state.breath - dt / s.breath_hold_time).max(0.0);
        if state.breath == 0.0 {
            state.breath_spent = true;
        }
    } else {
        state.breath = (state.breath + dt / s.breath_recover_time).min(1.0);
        if state.breath >= s.breath_unlock {
            state.breath_spent = false;
        }
    }
    let breath_target = if holding {
        s.breath_steady
    } else if state.breath_spent {
        s.breath_gasp
    } else {
        1.0
    };
    state.breath_scale = approach(state.breath_scale, breath_target, s.breath_rate * dt);

    let aim_scale = s.hip_scale + (1.0 - s.hip_scale) * state.ads;
    let rested = 1.0 + (cfg.rest.steady_sway - 1.0) * input.body.rest;
    let target_amp = posture_amp * exertion * state.breath_scale * aim_scale * rested;
    state.sway_amp += (target_amp - state.sway_amp) * (1.0 - (-s.settle_rate * dt).exp());

    state.sway_phase += dt * s.rate * (1.0 + (1.0 - input.body.stamina) * s.winded_rate);
    let p = state.sway_phase;
    state.sway = Vec2::new(
        p.sin() + 0.45 * (2.3 * p + 1.0).sin(),
        0.7 * (1.7 * p + 0.5).sin() + 0.3 * (3.1 * p).sin(),
    ) * state.sway_amp;
}

fn update_stability(state: &mut WeaponState, cfg: &WeaponConfig, input: &WeaponInput, dt: f32) {
    let c = &cfg.stability;
    state.shot_shake *= (-dt / c.settle_time).exp();
    let target = (stability_target(state, cfg, input) + state.shot_shake).min(c.max);
    let tau = if target > state.instability { c.rise_time } else { c.settle_time };
    state.instability += (target - state.instability) * (1.0 - (-dt / tau).exp());

    // Layered sines with unrelated frequencies: smooth, so it reads as a held weapon, but with no
    // pattern to learn. The seed gives each soldier a different path.
    state.wander_phase += dt * c.wander_rate * (1.0 + state.instability * c.wander_rate_unsteady);
    let p = state.wander_phase;
    let s = state.wander_seed;
    let path = Vec2::new(
        (p + s[0]).sin() + 0.6 * (1.9 * p + s[1]).sin(),
        (1.3 * p + s[2]).sin() + 0.6 * (2.3 * p + s[3]).sin(),
    ) / 1.6;
    state.wander = path * state.instability;
}

fn stability_target(state: &WeaponState, cfg: &WeaponConfig, input: &WeaponInput) -> f32 {
    let c = &cfg.stability;
    let body = &input.body;
    let lerp = |hip: f32, ads: f32| hip + (ads - hip) * state.ads;

    let mut target = match body.posture {
        Posture::Stand => lerp(c.hip_stand, c.ads_stand),
        Posture::Crouch => lerp(c.hip_crouch, c.ads_crouch),
        Posture::Prone => lerp(c.hip_prone, c.ads_prone),
    };
    target += body.speed_fraction.clamp(0.0, c.moving_max).powf(c.moving_curve) * c.moving;
    if state.ads > 0.0 && state.ads < 1.0 {
        target += c.raising;
    }
    target += (input.turn_rate / c.turn_rate_full).clamp(0.0, 1.0) * c.turning;
    target += (1.0 - body.stamina) * c.exertion;
    target += body.arm_wound * c.arm_wound + body.blood_loss * c.blood_loss + body.suppression * c.suppression;
    if body.airborne {
        target += c.airborne;
    }
    if body.changing_stance {
        target += c.changing_stance;
    }
    target += body.lean_rate * c.changing_lean;
    target * (1.0 + (cfg.rest.steady_wander - 1.0) * body.rest)
}

fn update_trigger(
    state: &mut WeaponState,
    cfg: &WeaponConfig,
    input: &WeaponInput,
    dt: f32,
    events: &mut Vec<WeaponEvent>,
) {
    let pulled = input.fire_held && !state.trigger_was_down;
    let held = input.fire_held;
    state.trigger_was_down = held;

    if state.cooldown > 0.0 {
        state.cooldown -= dt;
    }
    let interval = 60.0 / cfg.rounds_per_minute;

    let wants_shot = match state.mode {
        FireMode::Semi => pulled,
        FireMode::Auto => held,
    };
    if !wants_shot || state.cooldown > 0.0 {
        return;
    }
    let body = &input.body;
    if state.reloading()
        || state.ready < 1.0
        || body.sprinting
        || body.changing_stance
        || input.muzzle_blocked
        || input.stowing
    {
        return;
    }
    if state.ammo == 0 {
        if pulled {
            events.push(WeaponEvent::DryFire);
        }
        return;
    }

    state.ammo -= 1;
    // Carry the remainder so the rate of fire does not depend on the tick length.
    state.cooldown = (state.cooldown + interval).max(interval * 0.5);

    let gun_offset = state.aim_offset();
    let direction_offset = gun_offset + sample_dispersion(state, cfg);
    kick(state, cfg, body);
    events.push(WeaponEvent::Fired { gun_offset, direction_offset });
}

/// Where in the weapon's fixed cone this round goes, radians. Uniform across the disc.
fn sample_dispersion(state: &mut WeaponState, cfg: &WeaponConfig) -> Vec2 {
    let angle = state.rng.next_f32() * std::f32::consts::TAU;
    let radius = cfg.dispersion_radius() * state.rng.next_f32().sqrt();
    Vec2::from_angle(angle) * radius
}

fn kick(state: &mut WeaponState, cfg: &WeaponConfig, body: &BodyState) {
    let r = &cfg.recoil;
    let burst = 1.0 + (state.heat * r.heat_kick).min(r.heat_kick_max);
    let posture = match body.posture {
        Posture::Stand => r.stand_scale,
        Posture::Crouch => r.crouch_scale,
        Posture::Prone => r.prone_scale,
    };
    // Unshouldered, the gun jumps much further.
    let braced = r.hip_scale + (1.0 - r.hip_scale) * state.ads;
    let rested = 1.0 + (cfg.rest.steady_recoil - 1.0) * body.rest;
    let scale = burst * posture * braced * rested;

    let up = r.kick_up * (1.0 + state.rng.next_signed() * r.kick_up_spread);
    let side = r.kick_side * state.rng.next_signed();
    state.recoil_vel += Vec2::new(side, up) * scale;

    let c = &cfg.stability;
    let rested = 1.0 + (cfg.rest.steady_shake - 1.0) * body.rest;
    state.shot_shake += c.shot_shake * (1.0 + state.heat * c.heat_shake) * posture * rested;
    state.heat += 1.0;
}

/// Small deterministic generator: client and server must roll the same numbers from the same
/// seed for prediction to work, and it keeps this crate free of dependencies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2654435761).max(1))
    }

    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests;
