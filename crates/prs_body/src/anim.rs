//! The little a body has to remember between ticks. Stepped by the simulation at its fixed tick from
//! its own state, so everyone who runs the same sim gets the same body.

use crate::config::BodyConfig;
use glam::{Vec2, Vec3};

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

/// What animation memory needs from a soldier each tick.
#[derive(Clone, Copy, Debug)]
pub struct AnimInput {
    pub yaw: f32,
    pub velocity: Vec3,
    pub grounded: bool,
    /// 0 = standing, 1 = fully crouched.
    pub crouch: f32,
    /// 0 = at most crouched, 1 = lying flat.
    pub prone: f32,
    /// Too hurt to stay up: the body gives way.
    pub down: bool,
}

/// What is being treated, which decides where the hands go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CareSite {
    #[default]
    Chest,
    Arm,
    Leg,
    /// Somebody else, in front of them.
    Ally,
}

/// What it is being treated with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CareKit {
    #[default]
    Bandage,
    Medkit,
}

/// A treatment under way, as a body needs to know it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarePose {
    pub site: CareSite,
    pub kit: CareKit,
    /// 0 at the start, 1 when it is done.
    pub progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyAnim {
    /// Which way the hips and legs face. The upper body faces the look, and the spine twists between.
    pub hip_yaw: f32,
    /// Whether the hips are on their way back to the look.
    pub turning: bool,
    /// Advanced by hip rotation, to step the feet round when turning on the spot.
    pub turn_phase: f32,
    /// 0 to 1: how much of a turn-on-the-spot step to show.
    pub turn_weight: f32,
    /// 0 = standing still, 1 = walking.
    pub moving: f32,
    /// Ground direction of travel. Held after stopping, so the legs settle along the line they had.
    pub travel: Vec2,
    /// 0 = on the ground, 1 = legs fully in the air pose.
    pub air: f32,
    /// How far the hips have dipped on landing.
    pub land: f32,
    pub land_rate: f32,
    pub last_fall: f32,
    /// 0 = holding themselves up, 1 = lying where they fell.
    pub collapse: f32,
    /// Which way they went down, which stays as it was however the look moves afterwards.
    pub fall_yaw: f32,
    /// 0 = their own weight, 1 = braced under someone standing on their back.
    pub brace: f32,
    /// How far the gun has gone onto its sling for a climb, 0 to 1.
    pub slung: f32,
    /// How far the hands are on a hull they are heaving at, 0 to 1.
    pub heave: f32,
    /// How far the hands are on a treatment rather than the gun, 0 to 1.
    pub care: f32,
    /// The last treatment's progress, site and kit, kept for the hands on their way back to the gun.
    pub care_progress: f32,
    pub care_site: CareSite,
    pub care_kit: CareKit,
}

impl BodyAnim {
    pub fn new(yaw: f32) -> Self {
        Self {
            hip_yaw: yaw,
            turning: false,
            turn_phase: 0.0,
            turn_weight: 0.0,
            moving: 0.0,
            travel: Vec2::new(0.0, -1.0),
            air: 0.0,
            land: 0.0,
            land_rate: 0.0,
            last_fall: 0.0,
            collapse: 0.0,
            fall_yaw: yaw,
            brace: 0.0,
            slung: 0.0,
            heave: 0.0,
            care: 0.0,
            care_progress: 0.0,
            care_site: CareSite::Chest,
            care_kit: CareKit::Bandage,
        }
    }

    pub fn step(&mut self, cfg: &BodyConfig, input: &AnimInput, dt: f32) {
        let ground = Vec2::new(input.velocity.x, input.velocity.z);
        let speed = ground.length();
        let g = &cfg.gait;
        if speed > g.still_speed {
            self.travel = ground / speed;
        }
        let span = (g.moving_speed - g.still_speed).max(1e-4);
        let moving_goal = ((speed - g.still_speed) / span).clamp(0.0, 1.0);
        self.moving += (moving_goal - self.moving) * (1.0 - (-g.blend_rate * dt).exp());

        self.step_hips(cfg, input, dt);
        self.step_air(cfg, input, dt);
        self.step_collapse(cfg, input, dt);
    }

    /// Whether someone is on their back. It only moves the body and nothing else asks about it.
    pub fn step_brace(&mut self, cfg: &BodyConfig, carrying: bool, dt: f32) {
        let reach = dt / cfg.look.brace_time.max(1e-3);
        self.brace += (if carrying { 1.0 } else { 0.0 } - self.brace).clamp(-reach, reach);
    }

    /// The gun going onto its sling, or coming back off it, whichever this asks for.
    pub fn step_sling(&mut self, cfg: &BodyConfig, goal: f32, dt: f32) {
        let reach = dt / cfg.look.sling_time.max(1e-3);
        self.slung += (goal - self.slung).clamp(-reach, reach);
    }

    /// The hands going to a hull, or back to the gun.
    pub fn step_heave(&mut self, cfg: &BodyConfig, heaving: bool, dt: f32) {
        let time = if heaving { cfg.lift.reach_time } else { cfg.lift.return_time };
        let reach = dt / time.max(1e-3);
        self.heave += (if heaving { 1.0 } else { 0.0 } - self.heave).clamp(-reach, reach);
    }

    /// The hands going to a treatment under way, or back to the gun once there is none, whatever ended it.
    pub fn step_care(&mut self, cfg: &BodyConfig, treating: Option<CarePose>, dt: f32) {
        let c = &cfg.care;
        let (goal, time) = if treating.is_some() { (1.0, c.reach_time) } else { (0.0, c.return_time) };
        let reach = dt / time.max(1e-3);
        self.care += (goal - self.care).clamp(-reach, reach);
        if let Some(pose) = treating {
            self.care_progress = pose.progress;
            self.care_site = pose.site;
            self.care_kit = pose.kit;
        }
    }

    fn step_collapse(&mut self, cfg: &BodyConfig, input: &AnimInput, dt: f32) {
        if !input.down && self.collapse == 0.0 {
            self.fall_yaw = input.yaw;
        }
        let (goal, time) = if input.down { (1.0, cfg.collapse.fall_time) } else { (0.0, cfg.collapse.rise_time) };
        let reach = dt / time.max(1e-3);
        self.collapse += (goal - self.collapse).clamp(-reach, reach);
    }

    fn step_hips(&mut self, cfg: &BodyConfig, input: &AnimInput, dt: f32) {
        let t = &cfg.turn;
        let limit = cfg.stand.lerp(&cfg.crouch, input.crouch).lerp(&cfg.prone, input.prone).twist_limit;
        let off = wrap_angle(input.yaw - self.hip_yaw);

        // Walking points the hips somewhere between the look and the direction of travel. Going
        // backwards they face the look and the legs run in reverse, rather than turning round.
        let mut goal_off = 0.0;
        if self.moving > 0.05 {
            let travel_yaw = (-self.travel.x).atan2(-self.travel.y);
            let mut across = wrap_angle(travel_yaw - input.yaw);
            if across.abs() > PI * 0.5 {
                across = wrap_angle(across - PI);
            }
            goal_off = (across * t.hip_follow).clamp(-t.hip_follow_max, t.hip_follow_max) * (1.0 - input.prone);
        }

        let before = self.hip_yaw;
        if self.moving > 0.05 {
            self.turning = false;
            let error = wrap_angle(input.yaw + goal_off - self.hip_yaw);
            self.hip_yaw += error * (1.0 - (-t.rate * dt).exp());
        } else {
            let start = t.start.min(limit * 0.8);
            if off.abs() > start {
                self.turning = true;
            }
            if self.turning {
                self.hip_yaw += off * (1.0 - (-t.rate * dt).exp());
                if wrap_angle(input.yaw - self.hip_yaw).abs() < t.stop {
                    self.turning = false;
                }
            }
        }

        // However fast the look turned, the spine only twists so far. Hips are never left behind.
        let off = wrap_angle(input.yaw - self.hip_yaw);
        if off.abs() > limit {
            self.hip_yaw = input.yaw - off.signum() * limit;
        }

        let moved = wrap_angle(self.hip_yaw - before).abs();
        self.turn_phase += moved / t.step_angle.max(1e-3);
        let turning_on_the_spot = (moved / dt.max(1e-4) > 0.6) && self.moving < 0.2 && input.grounded;
        let goal = if turning_on_the_spot { 1.0 } else { 0.0 };
        self.turn_weight += (goal - self.turn_weight) * (1.0 - (-14.0 * dt).exp());
    }

    fn step_air(&mut self, cfg: &BodyConfig, input: &AnimInput, dt: f32) {
        let a = &cfg.air;
        let goal = if input.grounded { 0.0 } else { 1.0 };
        self.air += (goal - self.air) * (1.0 - (-a.rate * dt).exp());

        let fall = -input.velocity.y;
        if input.grounded && self.last_fall > a.land_min_speed {
            let kick = (self.last_fall * a.land_dip).min(a.land_dip_max) * a.land_stiffness.sqrt();
            self.land_rate += kick;
        }
        self.last_fall = if input.grounded { 0.0 } else { fall };

        // Integrated exactly enough for a stiff spring at the tick rate.
        let accel = -self.land * a.land_stiffness - self.land_rate * a.land_damping;
        self.land_rate += accel * dt;
        self.land = (self.land + self.land_rate * dt).clamp(0.0, a.land_dip_max);
    }

    /// Between two ticks, for rendering faster than the sim runs.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            hip_yaw: self.hip_yaw + wrap_angle(other.hip_yaw - self.hip_yaw) * t,
            turning: other.turning,
            turn_phase: self.turn_phase + (other.turn_phase - self.turn_phase) * t,
            turn_weight: self.turn_weight + (other.turn_weight - self.turn_weight) * t,
            moving: self.moving + (other.moving - self.moving) * t,
            travel: self.travel.lerp(other.travel, t).normalize_or(other.travel),
            air: self.air + (other.air - self.air) * t,
            land: self.land + (other.land - self.land) * t,
            land_rate: other.land_rate,
            last_fall: other.last_fall,
            collapse: self.collapse + (other.collapse - self.collapse) * t,
            fall_yaw: other.fall_yaw,
            brace: self.brace + (other.brace - self.brace) * t,
            slung: self.slung + (other.slung - self.slung) * t,
            heave: self.heave + (other.heave - self.heave) * t,
            care: self.care + (other.care - self.care) * t,
            // A new treatment starts from nothing, which is not something to slide back to.
            care_progress: if other.care_progress >= self.care_progress { self.care_progress + (other.care_progress - self.care_progress) * t } else { other.care_progress },
            care_site: other.care_site,
            care_kit: other.care_kit,
        }
    }
}

/// Into -PI..PI. Look yaw is unwrapped, so turning on the spot can pile up any number of turns.
pub fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}
