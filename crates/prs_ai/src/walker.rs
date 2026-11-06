//! One soldier wandering. It knows where it is, what is in the way and what it felt like doing last.

use crate::WalkerConfig;
use crate::flow::{FlowField, FlowGrid};
use glam::{Vec2, Vec3};
use prs_sim::CollisionWorld;
use prs_sim::movement::yaw_basis;
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use std::f32::consts::{PI, TAU};

/// All a walker's luck, one soldier's own. A plain generator, so that it is the same on every machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dice(pub u32);

impl Dice {
    pub(crate) fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2654435761).max(1))
    }

    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn chance(&mut self, share: f32) -> bool {
        self.next() < share
    }

    pub(crate) fn between(&mut self, range: Vec2) -> f32 {
        range.x + (range.y - range.x) * self.next()
    }

    pub(crate) fn in_circle(&mut self) -> Vec2 {
        let (sin, cos) = (self.next() * TAU).sin_cos();
        Vec2::new(cos, sin) * self.next().sqrt()
    }
}

/// An opening to walk through, and which way is out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Door {
    pub at: Vec2,
    pub outward: Vec2,
}

/// Where a walker keeps to.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkArea {
    pub center: Vec2,
    pub radius: f32,
    pub doors: Vec<Door>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Walk,
    Jog,
    Stop,
}

/// As much of a soldier as a walker goes by.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub feet: Vec3,
    pub up: bool,
    pub crouched: bool,
    pub lean_side: i8,
}

/// What is around a walker.
pub struct Surroundings<'a> {
    pub world: &'a CollisionWorld,
    /// Ground less steep than this can be walked on.
    pub walkable_cos: f32,
    pub area: &'a WalkArea,
    /// Everyone on their feet, themselves included.
    pub others: &'a [Vec2],
    /// The way to this area from anywhere, for an area that has one worked out.
    pub way: Option<(&'a FlowGrid, &'a FlowField)>,
}

pub(crate) fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// The yaw that looks along a direction on the ground: yaw 0 looks down -Z.
pub(crate) fn yaw_of(dir: Vec2) -> f32 {
    (-dir.x).atan2(-dir.y)
}

fn flat(yaw: f32) -> Vec2 {
    let forward = yaw_basis(yaw).0;
    Vec2::new(forward.x, forward.z)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Walker {
    pub soldier: usize,
    pub area: usize,
    pub dice: Dice,
    /// Which of its think ticks this walker uses, so they do not all think at once.
    pub beat: u32,
    pub mood: Mood,
    /// Seconds until picking something else to do.
    pub mood_left: f32,
    pub crouched: bool,
    pub lean_side: i8,
    /// Where it is going now, then where after that, last first.
    pub goal: Vec2,
    pub route: Vec<Vec2>,
    pub goal_left: f32,
    /// The way it is walking, and the way its body and gun are turned, both as unwrapped yaw.
    pub heading: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// How far it means to glance off the heading: x = yaw, y = pitch.
    pub glance: Vec2,
    pub glance_left: f32,
    /// Which way round it prefers to go past things, 1 or -1, so it does not dither.
    pub side: f32,
    /// Where it was last thought to be getting anywhere, and how long ago.
    pub anchor: Vec2,
    pub stuck_for: f32,
    /// The same over a longer stretch, for going back and forth in one spot, which never looks stuck tick by tick.
    pub far_anchor: Vec2,
    pub milling_for: f32,
    /// Seconds left of going its own way with no thought for the goal, how many times running it has
    /// had to do that for being stuck, and how many times this goal has turned it aside.
    pub detour_left: f32,
    pub unsticks: u32,
    pub detours: u32,
    pub jump: bool,
    /// Someone it should catch up with, ahead of any ideas of its own.
    pub follow: Option<Vec2>,
    /// Waiting for others to catch up, or hurrying to catch up with them.
    pub hold: bool,
    pub hurry: bool,
    /// Rays spent looking where it is going, ever, to keep an eye on the budget.
    pub rays_spent: u32,
    /// Lost track of the soldier, because they went down: start again from wherever they next are.
    pub lost: bool,
}

impl Walker {
    pub(crate) fn new(soldier: usize, area: usize, dice: Dice, beat: u32, at: Vec2, yaw: f32) -> Self {
        Self {
            soldier,
            area,
            dice,
            beat,
            mood: Mood::Walk,
            mood_left: 0.0,
            crouched: false,
            lean_side: 0,
            goal: at,
            route: Vec::new(),
            goal_left: 0.0,
            heading: yaw,
            yaw,
            pitch: 0.0,
            glance: Vec2::ZERO,
            glance_left: 0.0,
            side: if beat % 2 == 0 { 1.0 } else { -1.0 },
            anchor: at,
            stuck_for: 0.0,
            far_anchor: at,
            milling_for: 0.0,
            detour_left: 0.0,
            unsticks: 0,
            detours: 0,
            jump: false,
            follow: None,
            hold: false,
            hurry: false,
            rays_spent: 0,
            lost: false,
        }
    }

    /// The command for the coming tick. Every tick it turns and walks, and now and then it thinks.
    pub fn step(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, tick: u32) -> PlayerCmd {
        if !seen.up {
            self.lost = true;
            return PlayerCmd { tick, yaw: self.yaw, pitch: self.pitch, ..Default::default() };
        }
        let at = Vec2::new(seen.feet.x, seen.feet.z);
        if self.lost {
            // Back from the dead somewhere else, with the sim having set them to their home facing.
            self.lost = false;
            self.anchor = at;
            self.stuck_for = 0.0;
            self.goal_left = 0.0;
            self.route.clear();
        }
        let every = cfg.think_ticks.max(1);
        if tick % every == self.beat % every {
            self.think(cfg, seen, around, at, every as f32 * TICK_DT);
        }

        let wanted = self.heading + self.glance.x;
        let turn = cfg.turn_rate * TICK_DT;
        self.yaw += wrap(wanted - self.yaw).clamp(-turn, turn);
        self.pitch += (self.glance.y - self.pitch).clamp(-turn, turn);

        let pace = match self.mood {
            _ if self.follow.is_some() || self.hurry => cfg.jog_pace,
            _ if self.hold => 0.0,
            Mood::Stop => 0.0,
            Mood::Walk => cfg.walk_pace,
            Mood::Jog => cfg.jog_pace,
        };
        // Relative to where it faces: yaw grows to the left, and wish is x = right, y = forward.
        let off = wrap(self.heading - self.yaw);
        let wish_move = Vec2::new(-off.sin(), off.cos()) * pace;

        let mut buttons = Buttons::default();
        // These all act on being pressed, so every other tick is pressing and the rest are letting go.
        let press = tick % 2 == 0;
        buttons.set(Buttons::CROUCH, press && seen.crouched != self.crouched);
        buttons.set(Buttons::LEAN_LEFT, press && seen.lean_side != self.lean_side && self.lean_side < 0);
        buttons.set(Buttons::LEAN_RIGHT, press && seen.lean_side != self.lean_side && self.lean_side > 0);
        if self.lean_side == 0 && seen.lean_side != 0 {
            buttons.set(if seen.lean_side < 0 { Buttons::LEAN_LEFT } else { Buttons::LEAN_RIGHT }, press);
        }
        buttons.set(Buttons::JUMP, std::mem::take(&mut self.jump));
        PlayerCmd { tick, wish_move, yaw: self.yaw, pitch: self.pitch, buttons, ..Default::default() }
    }

    fn think(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, at: Vec2, dt: f32) {
        self.notice_being_stuck(cfg, at, dt);
        self.choose_mood(cfg, dt);
        self.choose_glance(cfg, dt);
        self.choose_goal(cfg, seen, around, at, dt);
        if let Some(spot) = self.follow {
            self.steer(cfg, seen, around, at, spot);
        } else if (self.mood != Mood::Stop || self.hurry) && !self.hold {
            // Across the map by the worked-out way, and within the area wherever it fancies.
            let far = at.distance(around.area.center) > around.area.radius;
            let way = around.way.filter(|_| far).and_then(|(grid, field)| grid.toward(field, at));
            self.steer(cfg, seen, around, at, way.unwrap_or(self.goal));
        }
    }

    /// Steers for a spot it has been sent to, with no ideas of its own about where to go next.
    pub(crate) fn steer_to(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, at: Vec2, spot: Vec2, dt: f32) {
        self.notice_being_stuck(cfg, at, dt);
        self.goal = spot;
        self.mood = Mood::Jog;
        self.steer(cfg, seen, around, at, spot);
    }

    fn notice_being_stuck(&mut self, cfg: &WalkerConfig, at: Vec2, dt: f32) {
        self.detour_left = (self.detour_left - dt).max(0.0);
        let resting = (self.mood == Mood::Stop || self.hold) && self.follow.is_none();
        if at.distance(self.far_anchor) > cfg.mill_distance || resting {
            self.far_anchor = at;
            self.milling_for = 0.0;
        } else {
            self.milling_for += dt;
        }
        let milling = self.milling_for >= cfg.mill_time;
        if at.distance(self.anchor) > cfg.stuck_distance && !milling {
            self.anchor = at;
            self.stuck_for = 0.0;
            if self.detour_left == 0.0 {
                self.unsticks = 0;
            }
            return;
        }
        if resting {
            return;
        }
        self.stuck_for += dt;
        if self.stuck_for < cfg.stuck_time && !milling {
            return;
        }
        self.milling_for = 0.0;
        self.far_anchor = at;
        // Back the way it came, more or less, and a good way off from there. Standing up gets out from
        // under things, and if it keeps happening, it may be something to jump.
        self.stuck_for = 0.0;
        self.unsticks += 1;
        self.detour_left = cfg.unstick_time;
        self.heading += PI + self.dice.between(Vec2::new(-1.2, 1.2));
        self.side = -self.side;
        self.crouched = false;
        self.jump = self.unsticks >= cfg.jump_after;
        self.route.clear();
        self.goal_left = 0.0;
    }

    fn choose_mood(&mut self, cfg: &WalkerConfig, dt: f32) {
        self.mood_left -= dt;
        if self.mood_left > 0.0 {
            return;
        }
        let dice = &mut self.dice;
        let roll = dice.between(Vec2::new(0.0, 1.0));
        self.mood = if roll < cfg.stop_chance {
            Mood::Stop
        } else if roll < cfg.stop_chance + cfg.jog_chance {
            Mood::Jog
        } else {
            Mood::Walk
        };
        self.mood_left = dice.between(if self.mood == Mood::Stop { cfg.stop_time } else { cfg.mood_time });
        self.crouched = self.mood != Mood::Jog && dice.chance(cfg.crouch_chance);
        self.lean_side = 0;
        if self.mood == Mood::Stop && dice.chance(cfg.lean_chance) {
            self.lean_side = if dice.chance(0.5) { 1 } else { -1 };
        }
    }

    fn choose_glance(&mut self, cfg: &WalkerConfig, dt: f32) {
        self.glance_left -= dt;
        if self.glance_left > 0.0 {
            return;
        }
        let dice = &mut self.dice;
        self.glance_left = dice.between(cfg.glance_time);
        let reach = if self.mood == Mood::Stop { cfg.glance_stopped } else { cfg.glance_walking };
        self.glance = Vec2::new(dice.between(Vec2::new(-reach, reach)), dice.between(Vec2::new(-cfg.glance_pitch, cfg.glance_pitch)));
    }

    fn choose_goal(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, at: Vec2, dt: f32) {
        let area = around.area;
        self.goal_left -= dt;
        let there = at.distance(self.goal) < cfg.goal_reach;
        if !there && self.goal_left > 0.0 {
            return;
        }
        // Timed out on the way through a door gives up on the door as well.
        if !there {
            self.route.clear();
        }
        self.goal_left = cfg.goal_time;
        self.detours = 0;
        if let Some(next) = self.route.pop() {
            self.goal = next;
            return;
        }
        let door = if self.dice.chance(cfg.door_chance) { self.nearby_door(cfg, area, at) } else { None };
        if let Some(door) = door {
            let front = door.at + door.outward * cfg.door_lead;
            self.goal = front;
            self.route.push(front);
            self.route.push(door.at - door.outward * cfg.door_inside);
            return;
        }
        // Somewhere it can walk straight to. Steering cannot find the way round a long wall, so a spot
        // behind one has it pressing back and forth against the wall a few metres at a time.
        for _ in 0..cfg.goal_tries.max(1) {
            let dice = &mut self.dice;
            let wander = at + flat(dice.between(Vec2::new(0.0, TAU))) * dice.between(cfg.goal_range);
            self.goal = if wander.distance(area.center) < area.radius { wander } else { area.center + dice.in_circle() * area.radius * 0.7 };
            if self.straight_to(cfg, seen, around, at, self.goal) {
                return;
            }
        }
    }

    /// Whether nothing it could not walk up stands between here and there.
    fn straight_to(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, at: Vec2, to: Vec2) -> bool {
        let along = to - at;
        let length = along.length();
        if length < 0.01 {
            return true;
        }
        self.rays_spent += 1;
        let eye = seen.feet + Vec3::Y * cfg.probe_height;
        let dir = Vec3::new(along.x, 0.0, along.y) / length;
        around.world.raycast(eye, dir, length).is_none_or(|hit| hit.normal.y >= around.walkable_cos)
    }

    /// One door within reach, by chance. Looks at a few at most, to stay cheap in a town full of them.
    fn nearby_door(&mut self, cfg: &WalkerConfig, area: &WalkArea, at: Vec2) -> Option<Door> {
        if area.doors.is_empty() {
            return None;
        }
        (0..6).find_map(|_| {
            let pick = (self.dice.next() * area.doors.len() as f32) as usize;
            let door = area.doors[pick.min(area.doors.len() - 1)];
            (door.at.distance(at) < cfg.door_reach).then_some(door)
        })
    }

    fn steer(&mut self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, at: Vec2, goal: Vec2) {
        let detouring = self.detour_left > 0.0;
        let to_goal = goal - at;
        let mut wanted = if !detouring && to_goal.length_squared() > 0.01 { yaw_of(to_goal) } else { self.heading };
        // Somebody close in front gets walked around, whichever side this walker likes.
        let ahead = flat(wanted);
        for &other in around.others {
            let gap = other - at;
            let distance = gap.length();
            if distance > 0.05 && distance < cfg.personal_space && gap.dot(ahead) > 0.0 {
                wanted += self.side * cfg.give_way;
                break;
            }
        }
        let mut rays = cfg.max_rays;
        if self.can_go(cfg, seen, around, wanted, &mut rays) {
            self.heading += wrap(wanted - self.heading);
            self.rays_spent += cfg.max_rays - rays;
            return;
        }
        self.turned_aside(cfg);
        let mut step = 1.0;
        while rays >= 2 && step * cfg.fan_step < PI {
            for side in [self.side, -self.side] {
                let turned = wanted + side * step * cfg.fan_step;
                if rays >= 2 && self.can_go(cfg, seen, around, turned, &mut rays) {
                    self.heading += wrap(turned - self.heading);
                    self.side = side;
                    self.rays_spent += cfg.max_rays - rays;
                    return;
                }
            }
            step += 1.0;
        }
        self.rays_spent += cfg.max_rays - rays;
        // Nothing found for the rays it had. Turn right round rather than stand staring at a wall.
        self.heading += PI;
        self.side = -self.side;
    }

    fn turned_aside(&mut self, cfg: &WalkerConfig) {
        self.detour_left = self.dice.between(cfg.detour_time);
        self.detours += 1;
        if self.detours >= cfg.detours_per_goal {
            self.goal_left = 0.0;
            self.route.clear();
        }
    }

    /// Whether there is nothing in the way along this yaw and ground to walk on at the end of it.
    fn can_go(&self, cfg: &WalkerConfig, seen: &Seen, around: &Surroundings, yaw: f32, rays: &mut u32) -> bool {
        let forward = yaw_basis(yaw).0;
        let eye = seen.feet + Vec3::Y * cfg.probe_height;
        *rays = rays.saturating_sub(1);
        // A slope that can be walked up does not count as being in the way.
        if let Some(hit) = around.world.raycast(eye, forward, cfg.probe_ahead)
            && hit.normal.y < around.walkable_cos
        {
            return false;
        }
        *rays = rays.saturating_sub(1);
        let down = cfg.probe_height + cfg.ledge_drop;
        let ground = around.world.raycast(eye + forward * cfg.ledge_ahead, Vec3::NEG_Y, down);
        ground.is_some_and(|hit| hit.normal.y >= around.walkable_cos)
    }
}
