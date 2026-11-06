//! Character movement: pure state in, state out.
//!
//! The model is Source-style acceleration on a heavy soldier: quick to respond, slow to stop
//! being committed. Jump spam and bunny hopping are designed out through landing recovery and
//! stamina cost rather than by making the normal movement feel sluggish.

use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld, Surface};

mod climb;
mod crowd;
mod lean;
pub use climb::{Climb, ClimbConfig, ClimbKind, Perch, StoolConfig, StoolOffer, abort_climb, climb_limit, plan_climb};
pub use crowd::{CrowdConfig, Crowding, Neighbour};
pub use lean::{Lean, LeanConfig, LeanInput, lean_eye, step_lean};

const SKIN: f32 = 0.015;
const MAX_SLIDES: usize = 4;
const GROUND_PROBE: f32 = 0.06;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stance {
    #[default]
    Stand,
    Crouch,
    Prone,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MoveConfig {
    pub radius: f32,
    pub stand_height: f32,
    pub crouch_height: f32,
    pub prone_height: f32,
    pub stand_eye: f32,
    pub crouch_eye: f32,
    pub prone_eye: f32,

    pub run_speed: f32,
    pub sprint_speed: f32,
    pub ads_speed: f32,
    pub crouch_speed: f32,
    pub prone_speed: f32,
    pub backward_scale: f32,
    pub strafe_scale: f32,

    pub ground_accel: f32,
    pub ground_friction: f32,
    /// Below this speed friction acts as if moving at this speed, so stops feel decisive.
    pub stop_speed: f32,
    pub air_accel: f32,
    pub air_max_speed: f32,
    pub gravity: f32,
    pub jump_height: f32,
    pub max_slope_cos: f32,
    pub step_height: f32,

    pub coyote_time: f32,
    pub jump_buffer: f32,
    pub jump_cooldown: f32,
    /// Seconds of slowed movement after a normal landing; grows with impact speed.
    pub landing_recovery: f32,
    pub landing_slow_scale: f32,
    /// Landing slower than this takes nothing, and recovery grows with speed beyond it, up to the most.
    pub landing_min_speed: f32,
    pub landing_full_speed: f32,
    pub landing_most: f32,
    /// Recovery at the slightest landing, as a share of `landing_recovery`.
    pub landing_least: f32,
    /// Share of speed kept while changing stance.
    pub stance_change_scale: f32,

    pub stamina_sprint_drain: f32,
    pub stamina_jump_cost: f32,
    pub stamina_regen: f32,
    pub stamina_regen_moving: f32,
    pub stamina_regen_delay: f32,
    /// After emptying stamina, sprint stays locked until it has recovered this far.
    pub stamina_sprint_unlock: f32,

    pub stand_crouch_time: f32,
    pub crouch_prone_time: f32,
    /// Going down from a wound, as the fall takes it rather than as the soldier chooses.
    pub collapse_time: f32,
    pub stride_run: f32,
    pub stride_sprint: f32,
    pub stride_crouch: f32,
    /// Footsteps per stride, one for each foot.
    pub footsteps_per_stride: f32,
    pub lean: LeanConfig,
    pub climb: ClimbConfig,
    pub stool: StoolConfig,
    pub crowd: CrowdConfig,
}

impl Default for MoveConfig {
    fn default() -> Self {
        Self {
            radius: 0.3,
            stand_height: 1.8,
            crouch_height: 1.2,
            prone_height: 0.7,
            stand_eye: 1.62,
            crouch_eye: 1.02,
            prone_eye: 0.38,

            run_speed: 3.9,
            sprint_speed: 6.2,
            ads_speed: 1.7,
            crouch_speed: 2.0,
            prone_speed: 0.75,
            backward_scale: 0.7,
            strafe_scale: 0.88,

            ground_accel: 11.0,
            ground_friction: 7.5,
            stop_speed: 1.6,
            air_accel: 1.1,
            air_max_speed: 1.0,
            gravity: 17.0,
            jump_height: 0.6,
            max_slope_cos: 0.68,
            step_height: 0.42,

            coyote_time: 0.09,
            jump_buffer: 0.12,
            jump_cooldown: 0.55,
            landing_recovery: 0.32,
            landing_slow_scale: 0.45,
            landing_min_speed: 1.0,
            landing_full_speed: 8.0,
            landing_most: 1.5,
            landing_least: 0.5,
            stance_change_scale: 0.45,

            stamina_sprint_drain: 1.0 / 14.0,
            stamina_jump_cost: 0.14,
            stamina_regen: 1.0 / 10.0,
            stamina_regen_moving: 1.0 / 22.0,
            stamina_regen_delay: 0.9,
            stamina_sprint_unlock: 0.2,

            stand_crouch_time: 0.28,
            crouch_prone_time: 0.7,
            collapse_time: 1.1,
            stride_run: 2.1,
            stride_sprint: 2.9,
            stride_crouch: 1.5,
            footsteps_per_stride: 2.0,
            lean: LeanConfig::default(),
            climb: ClimbConfig::default(),
            stool: StoolConfig::default(),
            crowd: CrowdConfig::default(),
        }
    }
}

impl MoveConfig {
    pub fn height(&self, stance: Stance) -> f32 {
        match stance {
            Stance::Stand => self.stand_height,
            Stance::Crouch => self.crouch_height,
            Stance::Prone => self.prone_height,
        }
    }

    pub fn eye(&self, stance: Stance) -> f32 {
        match stance {
            Stance::Stand => self.stand_eye,
            Stance::Crouch => self.crouch_eye,
            Stance::Prone => self.prone_eye,
        }
    }

    /// Ground covered by one stride right now.
    pub fn stride_length(&self, stance: Stance, sprinting: bool) -> f32 {
        match stance {
            _ if sprinting => self.stride_sprint,
            Stance::Stand => self.stride_run,
            Stance::Crouch | Stance::Prone => self.stride_crouch,
        }
    }

    pub fn capsule(&self, stance: Stance) -> Capsule {
        Capsule {
            height: self.height(stance),
            radius: self.radius,
        }
    }

    fn transition_time(&self, from: Stance, to: Stance) -> f32 {
        match (from, to) {
            (Stance::Stand, Stance::Crouch) | (Stance::Crouch, Stance::Stand) => {
                self.stand_crouch_time
            }
            (Stance::Prone, _) | (_, Stance::Prone) => self.crouch_prone_time,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MoveInput {
    /// x = strafe right, y = forward.
    pub wish: Vec2,
    pub yaw: f32,
    pub jump_pressed: bool,
    pub sprint_held: bool,
    pub crouch_pressed: bool,
    pub prone_pressed: bool,
    /// The weapon is raised to the sights, which forces walking pace.
    pub aiming: bool,
    /// Share of speed lost to wounds or weight, 0 to 1.
    pub slowed: f32,
    /// Sprinting drains stamina this share faster.
    pub extra_drain: f32,
    /// The legs will not sprint at all.
    pub no_sprint: bool,
    /// The soldier can no longer stand and goes down whatever else is happening.
    pub collapsing: bool,
    /// How hurt the limbs a climb needs are, 0 to 1, and the share of speed weight has cost.
    pub hurt: f32,
    pub burden: f32,
    /// Hands are taken up with something, and will not go to a ledge until they are free.
    pub busy_hands: bool,
    /// A crouched soldier ahead to climb on, if it came to that.
    pub stool: Option<StoolOffer>,
    /// There is someone on their back, so they can only shuffle.
    pub carrying: bool,
    /// Whoever they are bumping into.
    pub crowd: Crowding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MoveEvent {
    Jumped,
    Landed { speed: f32 },
    Footstep { intensity: f32, surface: Surface },
    /// Vertical pop from a stair step or snap; the camera smooths it away.
    Stepped { height: f32 },
    StanceChanged { stance: Stance },
    Climbed { kind: ClimbKind },
}

/// Every field is public: a client has to restore all of it to replay from what the server says.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveState {
    pub pos: Vec3,
    pub vel: Vec3,
    pub stance: Stance,
    pub eye_height: f32,
    pub grounded: bool,
    pub ground_normal: Vec3,
    pub ground_surface: Surface,
    pub sprinting: bool,
    /// 0..1, drained by sprinting and jumping. Weapons read it for exertion sway.
    pub stamina: f32,
    /// Seconds left in the current stance change.
    pub stance_timer: f32,
    /// Total ground distance walked, measured in strides. Bob and footsteps share it.
    pub stride: f32,
    pub coyote: f32,
    pub jump_buffered: f32,
    pub jump_cooldown: f32,
    pub landing_timer: f32,
    pub stamina_delay: f32,
    pub sprint_locked: bool,
    pub eye_rate: f32,
    pub lean: Lean,
    /// A climb under way. Its `kind` says if there is one.
    pub climb: Climb,
    pub climb_cooldown: f32,
    /// Whose back they are standing on, if they are.
    pub perch: Perch,
    /// Got somewhere no one soldier could have, off someone's back, and has not yet come back down to
    /// the land. The sim says when they have. A game mode may want to know.
    pub boosted: bool,
    /// Eye over feet, while something outside walking is holding them at a height in between the
    /// stances, as a gun rested on something does. Zero otherwise.
    pub held_eye: f32,
}

impl MoveState {
    pub fn new(pos: Vec3, cfg: &MoveConfig) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            stance: Stance::Stand,
            eye_height: cfg.stand_eye,
            grounded: false,
            ground_normal: Vec3::Y,
            ground_surface: Surface::Dirt,
            sprinting: false,
            stamina: 1.0,
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
        }
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.z).length()
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::Y * self.eye_height
    }
}

/// What a body held with its eye at this height takes up: down to the eye and a little over it.
pub fn held_capsule(cfg: &MoveConfig, eye: f32) -> Capsule {
    let height = eye + (cfg.stand_height - cfg.stand_eye);
    Capsule { height: height.max(cfg.radius * 2.0 + 0.01), radius: cfg.radius }
}

/// Forward and right on the ground plane for a yaw. Yaw 0 looks down -Z, matching the camera.
pub fn yaw_basis(yaw: f32) -> (Vec3, Vec3) {
    let (s, c) = yaw.sin_cos();
    (Vec3::new(-s, 0.0, -c), Vec3::new(c, 0.0, -s))
}

pub fn step(
    state: &mut MoveState,
    cfg: &MoveConfig,
    input: &MoveInput,
    world: &CollisionWorld,
    dt: f32,
    events: &mut Vec<MoveEvent>,
) {
    tick_timers(state, input, dt);
    if climb::step_climbing(state, cfg, input, world, dt) || climb::try_start(state, cfg, input, world, events) {
        return;
    }
    update_stance(state, cfg, input, world, dt, events);
    if climb::step_perched(state, cfg, input, world, dt) {
        update_sprint(state, cfg, &MoveInput { sprint_held: false, ..*input }, dt);
        return;
    }

    let capsule = if state.held_eye > 0.0 { held_capsule(cfg, state.held_eye) } else { cfg.capsule(state.stance) };
    state.pos += world.depenetration(state.pos, capsule, SKIN);

    let was_grounded = state.grounded;
    probe_ground(state, cfg, capsule, world);
    if state.grounded {
        state.coyote = cfg.coyote_time;
    }

    let jumped = try_jump(state, cfg, events);
    update_sprint(state, cfg, input, dt);

    let wish = wish_velocity(state, cfg, input);
    let fall_speed = -state.vel.y;

    if state.grounded && !jumped {
        state.vel.y = 0.0;
        apply_friction(state, cfg, wish.length() > 0.001, dt);
        accelerate(state, wish, cfg.ground_accel, dt);
    } else {
        state.vel.y -= cfg.gravity * dt;
        air_accelerate(state, wish, cfg, dt);
    }
    apply_brake(state, &input.crowd);

    let start = state.pos;
    let moved_on_ground = state.grounded && !jumped;
    move_with_steps(state, cfg, capsule, world, dt, moved_on_ground, events);
    apply_push(state, cfg, capsule, world, dt, moved_on_ground, input.crowd.push);

    let was_airborne_or_jumped = !moved_on_ground;
    probe_ground(state, cfg, capsule, world);
    if jumped || state.vel.y > 0.5 && !moved_on_ground {
        state.grounded = false;
    }

    if moved_on_ground && !state.grounded {
        snap_down(state, cfg, capsule, world, events);
    }

    if state.grounded && was_airborne_or_jumped && !was_grounded && fall_speed > cfg.landing_min_speed {
        let severity = ((fall_speed - cfg.landing_min_speed) / cfg.landing_full_speed).clamp(0.0, cfg.landing_most);
        state.landing_timer = cfg.landing_recovery * (cfg.landing_least + severity);
        events.push(MoveEvent::Landed { speed: fall_speed });
    }

    advance_stride(state, cfg, start, events);
}

fn tick_timers(state: &mut MoveState, input: &MoveInput, dt: f32) {
    state.coyote = (state.coyote - dt).max(0.0);
    state.jump_cooldown = (state.jump_cooldown - dt).max(0.0);
    state.landing_timer = (state.landing_timer - dt).max(0.0);
    state.stance_timer = (state.stance_timer - dt).max(0.0);
    state.climb_cooldown = (state.climb_cooldown - dt).max(0.0);
    state.jump_buffered = (state.jump_buffered - dt).max(0.0);
    if input.jump_pressed {
        state.jump_buffered = f32::MAX;
    }
}

fn update_stance(
    state: &mut MoveState,
    cfg: &MoveConfig,
    input: &MoveInput,
    world: &CollisionWorld,
    dt: f32,
    events: &mut Vec<MoveEvent>,
) {
    let mut desired = state.stance;
    if input.crouch_pressed {
        desired = if state.stance == Stance::Crouch { Stance::Stand } else { Stance::Crouch };
    }
    if input.prone_pressed {
        desired = if state.stance == Stance::Prone { Stance::Stand } else { Stance::Prone };
    }
    // There is no lying down on someone's back.
    if state.perch.on.is_some() && desired == Stance::Prone {
        desired = state.stance;
    }
    if input.jump_pressed && state.stance != Stance::Stand {
        desired = if state.stance == Stance::Prone { Stance::Crouch } else { Stance::Stand };
        state.jump_buffered = 0.0;
    }
    if input.sprint_held && !input.aiming && input.wish.y > 0.5 && state.stance == Stance::Crouch {
        desired = Stance::Stand;
    }

    if input.collapsing && state.stance != Stance::Prone {
        let time = cfg.collapse_time.max(0.001);
        state.eye_rate = (cfg.prone_eye - state.eye_height).abs() / time;
        state.stance_timer = time;
        state.stance = Stance::Prone;
        events.push(MoveEvent::StanceChanged { stance: Stance::Prone });
    }

    // Getting up needs headroom; lowering never does.
    let raising = cfg.height(desired) > cfg.height(state.stance);
    let can_change = state.stance_timer == 0.0
        && (state.grounded || desired != Stance::Prone)
        && (!raising || world.capsule_is_clear(state.pos, cfg.capsule(desired)));
    if desired != state.stance && can_change {
        let time = cfg.transition_time(state.stance, desired);
        state.eye_rate = (cfg.eye(desired) - state.eye_height).abs() / time.max(0.001);
        state.stance_timer = time;
        state.stance = desired;
        events.push(MoveEvent::StanceChanged { stance: desired });
    }

    let target = if state.held_eye > 0.0 { state.held_eye } else { cfg.eye(state.stance) };
    let max_delta = state.eye_rate.max(1.0) * dt;
    state.eye_height += (target - state.eye_height).clamp(-max_delta, max_delta);
}

fn probe_ground(state: &mut MoveState, cfg: &MoveConfig, capsule: Capsule, world: &CollisionWorld) {
    state.grounded = false;
    if state.vel.y > 2.0 {
        return;
    }
    if let Some(hit) = world.cast_capsule(state.pos, capsule, Vec3::NEG_Y, GROUND_PROBE + SKIN)
        && standable(state.pos, cfg, &hit, world)
    {
        state.grounded = true;
        state.ground_normal = if hit.normal.y >= cfg.max_slope_cos { hit.normal } else { Vec3::Y };
        state.ground_surface = hit.surface;
    }
}

/// Whether a downward capsule hit is something to stand on. The rounded bottom of a capsule
/// resting on a ledge reports a steep normal from the corner, so look at the surface itself.
fn standable(feet: Vec3, cfg: &MoveConfig, hit: &prs_collision::CastHit, world: &CollisionWorld) -> bool {
    if hit.normal.y >= cfg.max_slope_cos {
        return true;
    }
    if hit.normal.y < 0.02 {
        return false;
    }
    let contact = feet + Vec3::Y * (cfg.radius - hit.distance) - hit.normal * cfg.radius;
    let inward = -horizontal(hit.normal).normalize_or_zero();
    world
        .walk_ray(contact + inward * 0.03 + Vec3::Y * 0.15, Vec3::NEG_Y, 0.3)
        .is_some_and(|below| below.normal.y >= cfg.max_slope_cos)
}

/// A brake only ever takes speed away.
fn apply_brake(state: &mut MoveState, crowd: &Crowding) {
    if crowd.depth <= 0.0 {
        return;
    }
    let speed = state.horizontal_speed();
    if speed > crowd.limit {
        let share = crowd.limit / speed;
        state.vel.x *= share;
        state.vel.z *= share;
    }
}

/// Moves the soldier by a push, as far as walls allow, and leaves their speed as it was.
fn apply_push(state: &mut MoveState, cfg: &MoveConfig, capsule: Capsule, world: &CollisionWorld, dt: f32, on_ground: bool, push: Vec2) {
    if push == Vec2::ZERO {
        return;
    }
    let mut vel = Vec3::new(push.x, 0.0, push.y);
    slide(&mut state.pos, &mut vel, capsule, cfg, world, dt, on_ground);
}

fn try_jump(state: &mut MoveState, cfg: &MoveConfig, events: &mut Vec<MoveEvent>) -> bool {
    let allowed = state.jump_buffered > 0.0
        && state.coyote > 0.0
        && state.jump_cooldown == 0.0
        && state.stance == Stance::Stand
        && state.stance_timer == 0.0
        && state.stamina >= cfg.stamina_jump_cost;
    if !allowed {
        // A press only stays buffered for a short window, whichever way it fails.
        if state.jump_buffered > cfg.jump_buffer {
            state.jump_buffered = cfg.jump_buffer;
        }
        return false;
    }
    state.vel.y = (2.0 * cfg.gravity * cfg.jump_height).sqrt();
    state.grounded = false;
    state.coyote = 0.0;
    state.jump_buffered = 0.0;
    state.jump_cooldown = cfg.jump_cooldown;
    state.stamina -= cfg.stamina_jump_cost;
    state.stamina_delay = cfg.stamina_regen_delay;
    events.push(MoveEvent::Jumped);
    true
}

fn update_sprint(state: &mut MoveState, cfg: &MoveConfig, input: &MoveInput, dt: f32) {
    if state.stamina >= cfg.stamina_sprint_unlock {
        state.sprint_locked = false;
    }
    let moving = input.wish.length_squared() > 0.01;
    let wants = input.sprint_held && !input.aiming && !input.no_sprint && input.wish.y > 0.5;
    let on_feet = state.stance == Stance::Stand && state.stance_timer == 0.0;
    state.sprinting = wants && on_feet && !state.sprint_locked && state.stamina > 0.0;

    if state.sprinting {
        let drain = cfg.stamina_sprint_drain * (1.0 + input.extra_drain);
        state.stamina = (state.stamina - drain * dt).max(0.0);
        state.stamina_delay = cfg.stamina_regen_delay;
        if state.stamina == 0.0 {
            state.sprint_locked = true;
            state.sprinting = false;
        }
    } else if state.stamina_delay > 0.0 {
        state.stamina_delay -= dt;
    } else {
        let regen = if moving { cfg.stamina_regen_moving } else { cfg.stamina_regen };
        state.stamina = (state.stamina + regen * dt).min(1.0);
    }
}

fn wish_velocity(state: &MoveState, cfg: &MoveConfig, input: &MoveInput) -> Vec3 {
    let wish = input.wish.clamp_length_max(1.0);
    if wish == Vec2::ZERO {
        return Vec3::ZERO;
    }
    let (forward, right) = yaw_basis(input.yaw);
    let dir = (forward * wish.y + right * wish.x).normalize_or_zero();

    let base = match state.stance {
        Stance::Stand if state.sprinting => cfg.sprint_speed,
        Stance::Stand if input.aiming => cfg.ads_speed,
        Stance::Stand => cfg.run_speed,
        Stance::Crouch if input.aiming => cfg.ads_speed.min(cfg.crouch_speed),
        Stance::Crouch => cfg.crouch_speed,
        Stance::Prone => cfg.prone_speed,
    };
    // Leaning caps the pace as far as the lean has gone, and walking slower than that is untouched.
    let base = base + (base.min(cfg.lean.max_speed) - base) * state.lean.amount.abs().min(1.0);
    let base = if input.carrying { base.min(cfg.stool.carry_speed) } else { base };
    // Scale is blended across the input direction so the speed circle has no corners.
    let n = wish.normalize();
    let along = if n.y >= 0.0 { 1.0 } else { cfg.backward_scale };
    let scale = along * n.y * n.y + cfg.strafe_scale * n.x * n.x;
    let settling = if state.stance_timer > 0.0 { cfg.stance_change_scale } else { 1.0 };
    let landing = if state.landing_timer > 0.0 { cfg.landing_slow_scale } else { 1.0 };
    dir * base * wish.length() * scale * settling * landing * (1.0 - input.slowed.clamp(0.0, 1.0))
}

fn apply_friction(state: &mut MoveState, cfg: &MoveConfig, wants_to_move: bool, dt: f32) {
    let speed = state.horizontal_speed();
    if speed < 0.01 {
        state.vel.x = 0.0;
        state.vel.z = 0.0;
        return;
    }
    // The extra bite at low speed is for coming to a stop. Against someone meaning to go slowly, such as a
    // crawl, it would take away more each tick than they could put back.
    let control = if wants_to_move { speed } else { speed.max(cfg.stop_speed) };
    let new_speed = (speed - control * cfg.ground_friction * dt).max(0.0);
    let k = new_speed / speed;
    state.vel.x *= k;
    state.vel.z *= k;
}

fn accelerate(state: &mut MoveState, wish: Vec3, accel: f32, dt: f32) {
    let wish_speed = wish.length();
    if wish_speed < 0.001 {
        return;
    }
    let dir = wish / wish_speed;
    let current = state.vel.dot(dir);
    let add = (wish_speed - current).clamp(0.0, accel * wish_speed * dt);
    state.vel += dir * add;
}

fn air_accelerate(state: &mut MoveState, wish: Vec3, cfg: &MoveConfig, dt: f32) {
    let wish_speed = wish.length();
    if wish_speed < 0.001 {
        return;
    }
    let dir = wish / wish_speed;
    let capped = wish_speed.min(cfg.air_max_speed);
    let current = state.vel.dot(dir);
    let add = (capped - current).clamp(0.0, cfg.air_accel * wish_speed * dt);
    state.vel += dir * add;
}

fn move_with_steps(
    state: &mut MoveState,
    cfg: &MoveConfig,
    capsule: Capsule,
    world: &CollisionWorld,
    dt: f32,
    on_ground: bool,
    events: &mut Vec<MoveEvent>,
) {
    let start_pos = state.pos;
    let start_vel = state.vel;
    let blocked = slide(&mut state.pos, &mut state.vel, capsule, cfg, world, dt, on_ground);
    if !on_ground || !blocked {
        return;
    }

    // Blocked while walking: retry from step height and keep it if that gets further.
    let up = world
        .cast_capsule(start_pos, capsule, Vec3::Y, cfg.step_height + SKIN)
        .map_or(cfg.step_height, |hit| (hit.distance - SKIN).max(0.0));
    if up < 0.02 {
        return;
    }
    let mut pos = start_pos + Vec3::Y * up;
    let mut vel = start_vel;
    slide(&mut pos, &mut vel, capsule, cfg, world, dt, false);
    let Some(floor) = world.cast_capsule(pos, capsule, Vec3::NEG_Y, up + SKIN + GROUND_PROBE)
    else {
        return;
    };
    if !standable(pos, cfg, &floor, world) {
        return;
    }
    pos.y -= (floor.distance - SKIN).max(0.0);

    let plain = horizontal(state.pos - start_pos).length_squared();
    let stepped = horizontal(pos - start_pos).length_squared();
    if stepped > plain + 1e-5 {
        let height = pos.y - state.pos.y;
        state.pos = pos;
        state.vel = Vec3::new(vel.x, 0.0, vel.z);
        if height > 0.02 {
            events.push(MoveEvent::Stepped { height });
        }
    }
}

fn horizontal(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Quake-style collide and slide. Returns whether something steep got in the way.
/// Walking into a slope: as slow as the slope makes it, but still going where you were headed, as someone
/// walking does. Taking only the part going into the slope off the velocity would turn them along the hill.
fn up_the_slope(vel: Vec3, normal: Vec3) -> Vec3 {
    let speed = horizontal(clip_to_planes(vel, &[normal])).length();
    let along = horizontal(vel).normalize_or_zero() * speed;
    along.with_y(-(normal.x * along.x + normal.z * along.z) / normal.y)
}

fn slide(
    pos: &mut Vec3,
    vel: &mut Vec3,
    capsule: Capsule,
    cfg: &MoveConfig,
    world: &CollisionWorld,
    dt: f32,
    on_ground: bool,
) -> bool {
    let original = *vel;
    let mut remaining = dt;
    let mut planes: [Vec3; MAX_SLIDES] = [Vec3::ZERO; MAX_SLIDES];
    let mut plane_count = 0;
    let mut blocked = false;

    for _ in 0..MAX_SLIDES {
        let distance = vel.length() * remaining;
        if distance < 1e-5 {
            break;
        }
        let dir = *vel / vel.length();
        let Some(hit) = world.cast_capsule(*pos, capsule, dir, distance + SKIN) else {
            *pos += dir * distance;
            break;
        };

        let travel = (hit.distance - SKIN).max(0.0);
        *pos += dir * travel;
        remaining *= 1.0 - (travel / distance).min(1.0);

        let walkable = hit.normal.y >= cfg.max_slope_cos;
        if !walkable {
            blocked = true;
        }
        // A wall must not lift a walking soldier: clip against it as if it were vertical.
        let normal = if on_ground && !walkable {
            horizontal(hit.normal).normalize_or(hit.normal)
        } else {
            hit.normal
        };

        planes[plane_count] = normal;
        plane_count += 1;
        *vel = if on_ground && walkable && plane_count == 1 {
            up_the_slope(*vel, normal)
        } else {
            clip_to_planes(*vel, &planes[..plane_count])
        };
        if vel.dot(original) <= 0.0 {
            *vel = Vec3::ZERO;
            break;
        }
    }
    blocked
}

fn clip_to_planes(vel: Vec3, planes: &[Vec3]) -> Vec3 {
    let mut clipped = vel;
    for &n in planes {
        let into = clipped.dot(n);
        if into < 0.0 {
            clipped -= n * into;
        }
    }
    // Two planes pinch into a crease: only motion along it survives.
    if let [.., a, b] = planes
        && clipped.dot(*a) < -1e-4
    {
        let crease = a.cross(*b).normalize_or_zero();
        return crease * vel.dot(crease);
    }
    clipped
}

fn snap_down(
    state: &mut MoveState,
    cfg: &MoveConfig,
    capsule: Capsule,
    world: &CollisionWorld,
    events: &mut Vec<MoveEvent>,
) {
    let Some(hit) = world.cast_capsule(state.pos, capsule, Vec3::NEG_Y, cfg.step_height + SKIN)
    else {
        return;
    };
    if !standable(state.pos, cfg, &hit, world) {
        return;
    }
    let drop = (hit.distance - SKIN).max(0.0);
    state.pos.y -= drop;
    state.grounded = true;
    state.ground_normal = if hit.normal.y >= cfg.max_slope_cos { hit.normal } else { Vec3::Y };
    state.ground_surface = hit.surface;
    state.vel.y = 0.0;
    if drop > 0.05 {
        events.push(MoveEvent::Stepped { height: -drop });
    }
}

fn advance_stride(state: &mut MoveState, cfg: &MoveConfig, start: Vec3, events: &mut Vec<MoveEvent>) {
    if !state.grounded {
        return;
    }
    let length = cfg.stride_length(state.stance, state.sprinting);
    let before = state.stride;
    state.stride += horizontal(state.pos - start).length() / length;
    let steps = cfg.footsteps_per_stride;
    if (state.stride * steps).floor() != (before * steps).floor() {
        let speed = state.horizontal_speed();
        events.push(MoveEvent::Footstep {
            intensity: (speed / cfg.sprint_speed).clamp(0.15, 1.0),
            surface: state.ground_surface,
        });
    }
}

#[cfg(test)]
mod tests;
