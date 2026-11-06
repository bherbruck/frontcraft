//! One tick of a chassis on its wheels. Forces go in as impulses at the point they act, and tyres and
//! hull are solved as velocity limits rather than as springs, so nothing has to be stiff and a parked
//! vehicle comes exactly to rest instead of hunting about it.

use glam::{Mat3, Quat, Vec2, Vec3};
use prs_collision::{BoxContact, CollisionWorld, box_vs_box};

use crate::{VehicleConfig, VehicleState};

/// What the driver is asking for: both from -1 to 1, throttle forwards and steering to the right.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriveInput {
    pub throttle: f32,
    pub steer: f32,
    /// Whether anyone is at the controls. Without them the engine is off and the brake is on.
    pub manned: bool,
    /// The handbrake: the back wheels, hard enough to lock them, whatever else is asked.
    pub handbrake: bool,
    /// A change of gear by hand this tick: 1 up, -1 down, 0 none.
    pub shift: i8,
    /// Soldiers on foot rolling it upright: radians a second, and where they are, on average.
    pub lift: f32,
    pub lift_from: Vec3,
    /// Soldiers on foot pushing it: newtons, and where, in the world. Unused ones are nothing.
    pub pushes: [(Vec3, Vec3); 4],
}

impl DriveInput {
    pub fn pushed(&self) -> bool {
        self.pushes.iter().any(|(force, _)| *force != Vec3::ZERO)
    }
}

/// Something that happened to a vehicle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VehicleEvent {
    /// Something is in the way of those righting it.
    LiftBlocked,
    /// It is coming up, and it sounds like it.
    Heave { point: Vec3 },
    /// Back on its wheels, however it got there.
    Landed { point: Vec3, speed: f32 },
    /// It hit something, this hard in m/s of lost speed.
    Crashed { point: glam::Vec3, speed: f32 },
    EngineDead,
    TyreBurst { wheel: u8 },
    OnFire,
    Exploded { point: glam::Vec3 },
    Respawned,
}

/// The rigid body, in the form the impulses want it.
struct Body {
    inv_mass: f32,
    inv_inertia: Mat3,
    vel: Vec3,
    ang_vel: Vec3,
}

impl Body {
    fn new(state: &VehicleState, cfg: &VehicleConfig) -> Self {
        let r = Mat3::from_quat(state.rot);
        let inv_local = Mat3::from_diagonal(cfg.inertia.recip());
        Self { inv_mass: 1.0 / cfg.mass, inv_inertia: r * inv_local * r.transpose(), vel: state.vel, ang_vel: state.ang_vel }
    }

    fn velocity_at(&self, r: Vec3) -> Vec3 {
        self.vel + self.ang_vel.cross(r)
    }

    fn apply(&mut self, r: Vec3, impulse: Vec3) {
        self.vel += impulse * self.inv_mass;
        self.ang_vel += self.inv_inertia * r.cross(impulse);
    }

    /// How much mass this point seems to have, pushed this way.
    fn mass_at(&self, r: Vec3, along: Vec3) -> f32 {
        let turn = (self.inv_inertia * r.cross(along)).cross(r);
        1.0 / (self.inv_mass + along.dot(turn)).max(1e-6)
    }
}

/// One wheel on the ground, for this substep.
struct Patch {
    wheel: usize,
    r: Vec3,
    normal: Vec3,
    forward: Vec3,
    right: Vec3,
    grip: f32,
    /// Newtons of push from the road holding it down, along the normal.
    load: f32,
    drive: f32,
    /// Newtons of brake and rolling drag it can apply, whichever way it is rolling.
    retard: f32,
    /// The share of its grip the way it is sliding leaves it to corner with.
    share: f32,
    /// A wheel locked by its brake is not rolling, so its grip all goes against the way it is sliding, whichever
    /// way it is turned: these shares of it along the tyre and across it.
    locked: Option<(f32, f32)>,
    /// Impulses so far: the brake's share, and the whole along the tyre and across it.
    retarding: f32,
    long: f32,
    lat: f32,
}

/// One tick of one vehicle. A parked one with nothing asked of it does nothing here at all.
pub fn step(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, world: &CollisionWorld, dt: f32, events: &mut Vec<VehicleEvent>) {
    step_engine_idle(state, cfg, input, dt);
    if state.asleep {
        if !wants_to_move(input) {
            return;
        }
        state.asleep = false;
        state.still_for = 0.0;
    }
    let dead = state.damage.wrecked;
    let input = if dead { DriveInput::default() } else { *input };
    update_steering(state, cfg, &input, dt);
    let drive = update_engine(state, cfg, &input, dt);
    let substep = dt / cfg.substeps.max(1) as f32;
    let mut contacts = Vec::new();
    for _ in 0..cfg.substeps.max(1) {
        substep_body(state, cfg, &input, &drive, world, substep, &mut contacts, events);
    }
    update_lift(state, cfg, &input, dt, events);
    update_sleep(state, cfg, &input, dt);
}

/// Which way it rolls to come upright. With no way it wants to go, the side nearest those lifting comes up.
fn lift_axis(state: &VehicleState, cfg: &VehicleConfig, from: Vec3) -> Vec3 {
    let axis = (state.rot * Vec3::Y).cross(Vec3::Y);
    if axis.length() > cfg.lift.flat_below {
        return axis.normalize();
    }
    let along = state.rot * Vec3::NEG_Z;
    if along.cross(from - state.pos).y >= 0.0 { along } else { -along }
}

/// However many it takes, all they can do is bring it round this fast against what holds it down. They never
/// hold it back, so past its balance it goes on by itself, and the moment they stop gravity has it.
fn apply_lift(body: &mut Body, state: &VehicleState, cfg: &VehicleConfig, input: &DriveInput) {
    if input.lift > 0.0 {
        let axis = lift_axis(state, cfg, input.lift_from);
        let short = input.lift - body.ang_vel.dot(axis);
        if short > 0.0 {
            body.ang_vel += axis * short;
        }
    }
}

/// Whether what they are doing is getting anywhere, and whether it has come down on its wheels.
fn update_lift(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, dt: f32, events: &mut Vec<VehicleEvent>) {
    let moving = input.lift > 0.0 && state.ang_vel.dot(lift_axis(state, cfg, input.lift_from)) >= input.lift * cfg.lift.stall_share;
    state.lift_stall = if input.lift > 0.0 && !moving { state.lift_stall + dt } else { 0.0 };
    if state.lift_stall >= cfg.lift.stall_after {
        state.lift_stall = 0.0;
        events.push(VehicleEvent::LiftBlocked);
    }
    if (state.rot * Vec3::Y).y < cfg.damage.righted_above {
        state.was_over = true;
    } else if state.was_over && state.wheels.iter().any(|w| w.on_ground) {
        state.was_over = false;
        events.push(VehicleEvent::Landed { point: state.pos, speed: state.vel.length().max(state.ang_vel.length()) });
    }
}

fn wants_to_move(input: &DriveInput) -> bool {
    input.lift > 0.0 || input.pushed() || (input.manned && (input.throttle != 0.0 || input.steer != 0.0 || input.shift != 0))
}

/// Anything that has just been struck wakes up.
pub fn wake(state: &mut VehicleState) {
    state.asleep = false;
    state.still_for = 0.0;
}

fn step_engine_idle(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, dt: f32) {
    // On its side or its roof, with the oil and the fuel in the wrong place, it stops.
    let upright = (state.rot * Vec3::Y).y >= cfg.damage.righted_above;
    let alive = state.damage.engine > 0.0 && !state.damage.wrecked && upright;
    state.engine.running = input.manned && alive;
    // Whoever takes it next starts in automatic.
    if !input.manned {
        state.engine.manual = false;
    }
    if !state.engine.running {
        state.engine.load = 0.0;
        state.engine.rpm = (state.engine.rpm - state.engine.rpm * cfg.engine.rpm_rate * dt).max(0.0);
        if state.engine.rpm < 1.0 {
            state.engine.rpm = 0.0;
        }
    } else if state.asleep {
        state.engine.load = 0.0;
        state.engine.rpm += (cfg.engine.idle_rpm - state.engine.rpm) * (cfg.engine.rpm_rate * dt).min(1.0);
    }
}

fn update_steering(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, dt: f32) {
    let s = &cfg.steering;
    // Full lock whatever the speed: how hard it really turns is up to the tyres.
    let want = input.steer.clamp(-1.0, 1.0) * s.max_angle;
    // Coming back through the middle goes at the speed of letting go, so opposite lock is there in time.
    let rate = if input.steer * state.steer <= 0.0 && state.steer != 0.0 { s.return_rate } else { s.rate };
    state.steer += (want - state.steer).clamp(-rate * dt, rate * dt);
}

/// How much pull the engine gives each driven wheel this tick, and what the pedals do at each wheel.
struct Drive {
    wheel_force: f32,
    brake: f32,
    parked: bool,
}

fn update_engine(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, dt: f32) -> Drive {
    let e = &cfg.engine;
    let speed = state.forward_speed();
    let wish = input.throttle.clamp(-1.0, 1.0);
    let stopped = speed.abs() < cfg.brakes.reverse_below;
    // Throttle against the way it is rolling is the brake, until it is nearly stopped.
    let wanted_gear_sign = if wish > 0.0 { 1 } else if wish < 0.0 { -1 } else { 0 };
    let current_sign = if state.engine.gear < 0 { -1 } else { 1 };
    let against = wanted_gear_sign != 0 && wanted_gear_sign != current_sign;
    let mut brake = 0.0;
    let mut throttle = 0.0;
    if against {
        let rolling_with_gear = if current_sign > 0 { speed > -cfg.brakes.reverse_below } else { speed < cfg.brakes.reverse_below };
        if stopped || !rolling_with_gear {
            state.engine.gear = if wanted_gear_sign > 0 { 1 } else { -1 };
            state.engine.shift_left = e.shift_time;
            throttle = wish.abs();
        } else {
            brake = wish.abs();
        }
    } else {
        throttle = wish.abs();
    }
    if !state.engine.running {
        throttle = 0.0;
    }
    if input.handbrake {
        throttle = 0.0;
    }
    // Slow however it is going: sideways through a slide, its speed along itself can be nothing at all.
    // Somebody pushing has it out of gear and off its brakes, unless whoever is at the wheel has a foot on them.
    let parked = !input.pushed() && (!input.manned || ((wish == 0.0 || input.handbrake) && state.vel.length() < cfg.brakes.hold_below));
    if e.manual_until_stop && state.engine.manual && input.shift == 0 && speed.abs() < cfg.brakes.reverse_below {
        state.engine.manual = false;
    }
    // Foot off for the change, so a gear can be heard going in.
    let wanted = if state.engine.shift_left > 0.0 { 0.0 } else { throttle };

    let pedal = (wanted - state.engine.load).clamp(-e.throttle_rate * dt, e.throttle_rate * dt);
    state.engine.load += pedal;

    // The engine turns with the driven wheels, slipping the clutch to keep going from a standstill.
    let driven: Vec<f32> = cfg.wheels.iter().zip(&state.wheels).filter(|(w, _)| w.driven).map(|(_, s)| s.spin.abs()).collect();
    let wheel_rate = driven.iter().sum::<f32>() / driven.len().max(1) as f32;
    let to_rpm = 60.0 / std::f32::consts::TAU;
    let ratio = e.ratio(state.engine.gear);
    let from_wheels = wheel_rate * ratio * to_rpm;
    if state.engine.running {
        let launch = e.idle_rpm + (e.launch_rpm - e.idle_rpm) * state.engine.load;
        let goal = from_wheels.max(launch).min(e.redline_rpm);
        state.engine.rpm += (goal - state.engine.rpm) * (e.rpm_rate * dt).min(1.0);
    }

    state.engine.shift_left = (state.engine.shift_left - dt).max(0.0);
    if input.shift != 0 {
        shift_by_hand(state, cfg, input.shift, wheel_rate);
    }
    if state.engine.gear > 0 && state.engine.shift_left <= 0.0 && !state.engine.manual {
        let top = e.gears.len() as i8;
        let next = if from_wheels > e.shift_up_rpm && state.engine.gear < top && throttle > 0.0 {
            Some(state.engine.gear + 1)
        } else if from_wheels < e.shift_down_rpm && state.engine.gear > 1 {
            Some(state.engine.gear - 1)
        } else {
            None
        };
        if let Some(gear) = next {
            state.engine.gear = gear;
            state.engine.shift_left = e.shift_time;
        }
    }

    let power = state.damage.power_share(&cfg.damage);
    let through = if state.engine.shift_left > 0.0 { 0.0 } else { 1.0 };
    let torque = e.torque_at(state.engine.rpm.max(from_wheels)) * state.engine.load * power * through;
    let pull = torque * ratio / cfg.wheels.iter().find(|w| w.driven).map_or(1.0, |w| w.radius);
    let sign = if state.engine.gear < 0 { -1.0 } else { 1.0 };
    Drive { wheel_force: sign * pull / driven.len().max(1) as f32, brake, parked }
}

/// One gear up or down the box, which leaves the box to the driver from then on. Not into a gear that would
/// take the engine past its redline, and backwards and forwards only happen by way of the throttle, stopped.
fn shift_by_hand(state: &mut VehicleState, cfg: &VehicleConfig, shift: i8, wheel_rate: f32) {
    let e = &cfg.engine;
    if state.engine.gear < 0 {
        return;
    }
    let next = state.engine.gear + shift;
    if next < 1 || next > e.gears.len() as i8 {
        return;
    }
    let revs = wheel_rate * e.ratio(next) * 60.0 / std::f32::consts::TAU;
    if revs <= e.redline_rpm {
        state.engine.manual = true;
        state.engine.gear = next;
        state.engine.shift_left = e.shift_time;
    }
}

#[allow(clippy::too_many_arguments)]
fn substep_body(
    state: &mut VehicleState,
    cfg: &VehicleConfig,
    input: &DriveInput,
    drive: &Drive,
    world: &CollisionWorld,
    dt: f32,
    contacts: &mut Vec<BoxContact>,
    events: &mut Vec<VehicleEvent>,
) {
    let mut body = Body::new(state, cfg);
    body.vel.y -= cfg.gravity * dt;
    body.vel -= body.vel * body.vel.length() * cfg.aero.drag * body.inv_mass * dt;
    body.ang_vel *= (-cfg.aero.angular_drag * dt).exp();

    let frame = state.frame();
    let up = state.rot * Vec3::Y;
    let mut patches: Vec<Patch> = Vec::with_capacity(cfg.wheels.len());
    let mut lift = Vec3::ZERO;
    let previous: Vec<f32> = state.wheels.iter().map(|w| w.compression).collect();
    for (i, wheel) in cfg.wheels.iter().enumerate() {
        let flat = state.wheels[i].tyre <= 0.0;
        let radius = if flat { wheel.radius - cfg.tyres.flat_drop } else { wheel.radius };
        let start = frame.point(wheel.mount);
        let reach = wheel.travel + radius;
        let hit = world.raycast(start, -up, reach).filter(|hit| hit.distance > 0.0 && hit.normal.dot(up) > 0.2);
        let w = &mut state.wheels[i];
        let Some(hit) = hit else {
            w.compression = 0.0;
            w.on_ground = false;
            w.load = 0.0;
            w.slip = 0.0;
            continue;
        };
        let raw = reach - hit.distance;
        w.compression = raw.clamp(0.0, wheel.travel);
        w.on_ground = true;
        w.surface = hit.surface;
        let contact = start - up * hit.distance;
        let r = contact - state.pos;
        let rate = (w.compression - previous[i]) / dt;
        let spring = cfg.suspension.stiffness * w.compression + cfg.suspension.damping * rate;
        // Bottomed out, the wheel is a solid thing between the road and the chassis.
        let depth = raw - wheel.travel;
        if depth > 0.0 {
            let v = body.velocity_at(r).dot(hit.normal);
            if v < 0.0 {
                let mass = body.mass_at(r, hit.normal);
                body.apply(r, hit.normal * (-v * mass));
            }
            push_out(&mut lift, hit.normal, depth * cfg.suspension.bump_correction);
        }
        w.load = spring.max(0.0);

        let turn = if wheel.steers { state.steer } else { 0.0 };
        let heading = state.rot * Quat::from_rotation_y(-turn) * Vec3::NEG_Z;
        let forward = (heading - hit.normal * heading.dot(hit.normal)).normalize_or(heading);
        let right = forward.cross(hit.normal).normalize_or(state.rot * Vec3::X);
        let ground = cfg.tyres.on(hit.surface);
        let grip = ground.grip * if flat { cfg.tyres.flat_grip } else { 1.0 };
        let rolling = ground.rolling + if flat { cfg.tyres.flat_rolling } else { 0.0 };
        let rolling_share = if input.pushed() { (state.vel.length() / cfg.push.roll_from.max(1e-3)).min(1.0) } else { 1.0 };
        let hand = if !input.handbrake {
            0.0
        } else if wheel.steers {
            cfg.brakes.handbrake_front
        } else {
            cfg.brakes.handbrake_rear
        };
        let foot = cfg.brakes.per_wheel * drive.brake * if wheel.steers { 1.0 } else { cfg.brakes.rear_share };
        let brakes = if drive.parked { cfg.brakes.parked } else { foot + hand };
        let engine_drag = if wheel.driven && input.throttle == 0.0 && input.manned { cfg.engine.engine_brake / cfg.wheels.len() as f32 } else { 0.0 };
        patches.push(Patch {
            wheel: i,
            r,
            normal: hit.normal,
            forward,
            right,
            grip,
            load: 0.0,
            drive: if wheel.driven { drive.wheel_force } else { 0.0 },
            retard: brakes + engine_drag + rolling * w.load * rolling_share,
            share: 1.0,
            locked: None,
            retarding: 0.0,
            long: 0.0,
            lat: 0.0,
        });
    }
    // Anti-roll bars: whichever side of an axle is pressed further up takes more of the load.
    let s = &cfg.suspension;
    for (a, b) in s.axles {
        let (a, b) = (a as usize, b as usize);
        if a >= state.wheels.len() || b >= state.wheels.len() {
            continue;
        }
        let shift = s.anti_roll * (state.wheels[a].compression - state.wheels[b].compression);
        if state.wheels[a].on_ground {
            state.wheels[a].load = (state.wheels[a].load + shift).max(0.0);
        }
        if state.wheels[b].on_ground {
            state.wheels[b].load = (state.wheels[b].load - shift).max(0.0);
        }
    }
    for p in &mut patches {
        p.load = state.wheels[p.wheel].load;
        body.apply(p.r, p.normal * (p.load * dt));
    }

    apply_lift(&mut body, state, cfg, input);
    for &(force, at) in &input.pushes {
        if force != Vec3::ZERO {
            body.apply(at - state.pos, force * dt);
        }
    }
    solve_tyres(&mut body, cfg, &mut patches, dt);
    finish_wheels(state, cfg, &patches, drive, dt, &body);
    solve_hull(&mut body, state, cfg, world, contacts, events, &mut lift);

    state.vel = body.vel;
    state.ang_vel = body.ang_vel;
    state.pos += state.vel * dt + lift;
    let turn = state.ang_vel * dt;
    state.rot = (Quat::from_xyzw(turn.x * 0.5, turn.y * 0.5, turn.z * 0.5, 0.0) * state.rot + state.rot).normalize();
}

/// Grip a tyre has sideways at this slip, as a share of the most it can have. At walking pace it holds
/// outright, and past the peak of the curve it falls away and slides.
fn lateral_share(cfg: &VehicleConfig, v_long: f32, v_lat: f32) -> f32 {
    let t = &cfg.tyres;
    let speed = Vec2::new(v_long, v_lat).length();
    if speed <= t.static_speed {
        return 1.0;
    }
    let slip = v_lat.abs().atan2(v_long.abs());
    let curve = (t.slip_shape * (t.slip_stiffness * slip).atan()).sin();
    let blend = ((speed - t.static_speed) / t.static_speed).min(1.0);
    1.0 + (curve - 1.0) * blend
}

fn solve_tyres(body: &mut Body, cfg: &VehicleConfig, patches: &mut [Patch], dt: f32) {
    // How far each tyre has let go is how it is sliding now. Measured again between iterations it would see
    // the slip its own force has just taken out, find it has no grip and take the force back.
    for p in patches.iter_mut() {
        let v = body.velocity_at(p.r);
        let (along, across) = (v.dot(p.forward), v.dot(p.right));
        p.share = lateral_share(cfg, along, across);
        let speed = Vec2::new(along, across).length();
        // Sliding across, a brake needs less to stop it turning altogether: only what is left along the tyre.
        let moving = speed > cfg.tyres.static_speed;
        p.locked = (moving && p.retard * speed >= p.grip * p.load * along.abs()).then(|| (along.abs() / speed, across.abs() / speed));
    }
    for _ in 0..cfg.iterations.max(1) {
        for p in patches.iter_mut() {
            let most = p.grip * p.load * dt;
            let v = body.velocity_at(p.r);

            // The engine's pull as far as grip allows, and brakes and drag take off what they can.
            let v_long = v.dot(p.forward);
            let mass = body.mass_at(p.r, p.forward);
            let most_along = p.locked.map_or(most, |(along, _)| most * along);
            let limit = p.retard * dt;
            let retarding = (p.retarding - v_long * mass).clamp(-limit, limit);
            let long = (p.drive * dt + retarding).clamp(-most_along, most_along);
            body.apply(p.r, p.forward * (long - p.long));
            p.long = long;
            p.retarding = retarding;

            let v = body.velocity_at(p.r);
            let v_lat = v.dot(p.right);
            let left = (most * most - p.long * p.long).max(0.0).sqrt().max(most * cfg.tyres.lateral_reserve);
            let side_limit = p.locked.map_or((most * p.share).min(left), |(_, across)| most * across);
            let mass = body.mass_at(p.r, p.right);
            let lat = (p.lat - v_lat * mass).clamp(-side_limit, side_limit);
            body.apply(p.r, p.right * (lat - p.lat));
            p.lat = lat;
        }
    }
}

/// Wheels' own spin, which nothing else depends on but the engine note and how they are drawn.
fn finish_wheels(state: &mut VehicleState, cfg: &VehicleConfig, patches: &[Patch], drive: &Drive, dt: f32, body: &Body) {
    for (i, wheel) in cfg.wheels.iter().enumerate() {
        let w = &mut state.wheels[i];
        if let Some(p) = patches.iter().find(|p| p.wheel == i) {
            let v_long = body.velocity_at(p.r).dot(p.forward);
            let over = (p.drive.abs() - p.long.abs() / dt).max(0.0);
            w.slip = (over / (p.grip * p.load + 1.0)).min(1.0);
            let spun = over * cfg.tyres.spin_gain * p.drive.signum();
            w.spin = (v_long + spun) / wheel.radius;
        } else {
            let torque = if wheel.driven { drive.wheel_force * wheel.radius } else { 0.0 };
            w.spin += torque / cfg.engine.wheel_inertia * dt;
            w.spin *= (-cfg.engine.wheel_air_drag * dt).exp();
            if drive.parked {
                w.spin = 0.0;
            }
        }
        w.angle = (w.angle + w.spin * dt).rem_euclid(std::f32::consts::TAU);
    }
}

#[allow(clippy::too_many_arguments)]
fn solve_hull(
    body: &mut Body,
    state: &VehicleState,
    cfg: &VehicleConfig,
    world: &CollisionWorld,
    contacts: &mut Vec<BoxContact>,
    events: &mut Vec<VehicleEvent>,
    lift: &mut Vec3,
) {
    contacts.clear();
    for hull in &cfg.hull {
        world.box_contacts(state.pos + state.rot * hull.center, hull.half, state.rot, contacts);
    }
    if contacts.is_empty() {
        return;
    }
    let c = &cfg.contact;
    let mut hardest = 0.0_f32;
    let mut at = state.pos;
    for _ in 0..cfg.iterations.max(1) {
        for contact in contacts.iter() {
            let r = contact.point - state.pos;
            let v = body.velocity_at(r);
            let closing = v.dot(contact.normal);
            if closing >= 0.0 {
                continue;
            }
            if -closing > hardest {
                hardest = -closing;
                at = contact.point;
            }
            let bounce = if -closing > cfg.damage.crash_from { c.bounce } else { 0.0 };
            let mass = body.mass_at(r, contact.normal);
            let push = -closing * (1.0 + bounce) * mass;
            body.apply(r, contact.normal * push);
            let v = body.velocity_at(r);
            let slide = v - contact.normal * v.dot(contact.normal);
            let speed = slide.length();
            if speed > 1e-4 {
                let dir = slide / speed;
                let mass = body.mass_at(r, dir);
                let rub = (speed * mass).min(c.friction * push);
                body.apply(r, -dir * rub);
            }
        }
    }
    for contact in contacts.iter() {
        push_out(lift, contact.normal, (contact.depth - c.slop).max(0.0) * c.correction);
    }
    if hardest > cfg.damage.crash_from {
        events.push(VehicleEvent::Crashed { point: at, speed: hardest });
    }
}

/// Adds to a correction only as much as this contact wants beyond what it already does for it, so
/// several contacts asking for the same thing do not add up.
fn push_out(lift: &mut Vec3, normal: Vec3, wanted: f32) {
    let already = lift.dot(normal);
    if already < wanted {
        *lift += normal * (wanted - already);
    }
}

fn update_sleep(state: &mut VehicleState, cfg: &VehicleConfig, input: &DriveInput, dt: f32) {
    let mut moving = state.vel;
    // Lying on its hull, with no springs under it, it rests by falling a hair and being put back.
    if !state.wheels.iter().any(|w| w.on_ground) && moving.y.abs() < cfg.gravity * dt * 2.0 {
        moving.y = 0.0;
    }
    let settled = moving.length() < cfg.sleep.speed && state.ang_vel.length() < cfg.sleep.spin;
    if settled && !wants_to_move(input) {
        state.still_for += dt;
        if state.still_for >= cfg.sleep.after {
            state.asleep = true;
            state.vel = Vec3::ZERO;
            state.ang_vel = Vec3::ZERO;
        }
    } else {
        state.still_for = 0.0;
    }
}

/// Two vehicles running into each other. Whichever parts of them touch push apart, each by its own share.
pub fn collide(a: &mut VehicleState, cfg_a: &VehicleConfig, b: &mut VehicleState, cfg_b: &VehicleConfig, events: &mut Vec<(bool, VehicleEvent)>) {
    if a.pos.distance_squared(b.pos) > (cfg_a.reach() + cfg_b.reach()).powi(2) {
        return;
    }
    for hull_a in &cfg_a.hull {
        for hull_b in &cfg_b.hull {
            let box_a = (a.pos + a.rot * hull_a.center, hull_a.half, a.rot);
            let box_b = (b.pos + b.rot * hull_b.center, hull_b.half, b.rot);
            let Some(contact) = box_vs_box(box_a, box_b) else {
                continue;
            };
            let (mut one, mut two) = (Body::new(a, cfg_a), Body::new(b, cfg_b));
            let (r1, r2) = (contact.point - a.pos, contact.point - b.pos);
            let n = contact.normal;
            let closing = (one.velocity_at(r1) - two.velocity_at(r2)).dot(n);
            if closing < 0.0 {
                let m1 = 1.0 / one.mass_at(r1, n);
                let m2 = 1.0 / two.mass_at(r2, n);
                let bounce = cfg_a.contact.bounce.min(cfg_b.contact.bounce);
                let push = -closing * (1.0 + bounce) / (m1 + m2);
                one.apply(r1, n * push);
                two.apply(r2, -n * push);
                if -closing > cfg_a.damage.crash_from {
                    events.push((false, VehicleEvent::Crashed { point: contact.point, speed: -closing }));
                    events.push((true, VehicleEvent::Crashed { point: contact.point, speed: -closing }));
                }
            }
            let share = cfg_b.mass / (cfg_a.mass + cfg_b.mass);
            let over = (contact.depth - cfg_a.contact.slop).max(0.0) * cfg_a.contact.correction;
            a.pos += n * over * share;
            b.pos -= n * over * (1.0 - share);
            a.vel = one.vel;
            a.ang_vel = one.ang_vel;
            b.vel = two.vel;
            b.ang_vel = two.ang_vel;
            wake(a);
            wake(b);
        }
    }
}

#[cfg(test)]
mod tyre_tests {
    use super::*;
    use crate::VehicleTable;
    use prs_seats::Frame;

    /// A tyre pushes against the way it is sliding, at every wheel, at any slip, wheel angle or spin, on
    /// both sides of its peak.
    #[test]
    fn a_tyres_force_is_always_against_its_slide() {
        let table = VehicleTable::default();
        let cfg = &table[VehicleTable::UTILITY_TRUCK];
        for beta in [-170.0_f32, -120.0, -90.0, -60.0, -30.0, -8.0, -2.0, 2.0, 8.0, 30.0, 60.0, 90.0, 120.0, 170.0] {
            for steer in [-0.6_f32, -0.2, 0.0, 0.2, 0.6] {
                for yaw_rate in [-1.5, 0.0, 1.5] {
                    let mut state = VehicleState::new(VehicleTable::UTILITY_TRUCK, cfg, Frame { pos: Vec3::ZERO, rot: Quat::IDENTITY });
                    let direction = Quat::from_rotation_y(beta.to_radians()) * Vec3::NEG_Z;
                    state.vel = direction * 18.0;
                    state.ang_vel = Vec3::Y * yaw_rate;
                    let mut body = Body::new(&state, cfg);
                    let mut patches: Vec<Patch> = cfg.wheels.iter().enumerate().map(|(i, wheel)| {
                        let turn = if wheel.steers { steer } else { 0.0 };
                        let forward = Quat::from_rotation_y(-turn) * Vec3::NEG_Z;
                        Patch { wheel: i, r: wheel.mount, normal: Vec3::Y, forward, right: forward.cross(Vec3::Y), grip: 0.6, load: 5500.0, drive: 0.0, retard: 0.0, share: 1.0, locked: None, retarding: 0.0, long: 0.0, lat: 0.0 }
                    }).collect();
                    let slips: Vec<f32> = patches.iter().map(|p| body.velocity_at(p.r).dot(p.right)).collect();
                    solve_tyres(&mut body, cfg, &mut patches, 1.0 / 120.0);
                    for (p, slip) in patches.iter().zip(slips) {
                        if slip.abs() > 0.5 {
                            assert!(p.lat * slip < 0.0, "beta {beta} steer {steer} yaw {yaw_rate}, wheel {}: slip {slip} and impulse {}", p.wheel, p.lat);
                            assert!(p.lat.abs() > 0.5 * 0.6 * 5500.0 / 120.0, "it should hold on past its peak: {}", p.lat);
                        }
                    }
                }
            }
        }
    }
}
