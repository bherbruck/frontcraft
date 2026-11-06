//! Soldiers with nothing particular to do, walking about so the world has people in it.
//!
//! They are soldiers like anyone else: all they can do is send a [`PlayerCmd`], so they move, wear out,
//! bleed and die by the same rules as a player. What they want lives here and the sim never sees it.
//! There is no Bevy and no clock in here, and all chance comes from a seed, so a server can run them too
//! and two runs from one seed are the same.
//!
//! Rays are what cost, and this also runs in browsers. A walker thinks every few ticks rather than
//! every tick, each at its own moment, and on open ground spends two rays when it does. It only looks
//! around for a way past when the way ahead is blocked.

mod fighter;
mod flow;
mod platoon;
mod sight;
mod skill;
mod troops;
mod convoy;
mod walker;

pub use fighter::{Battle, Contact, Fighter, Plan, SquadBoard};
pub use flow::{FlowField, FlowGrid, FlowMap, FlowRules};
pub use platoon::{Holdover, Platoon};
pub use sight::{Foliage, SightConfig};
pub use skill::{FightConfig, SkillConfig};
pub use convoy::{Convoy, ConvoyConfig, Leg, Rider, Trip};
pub use troops::{Troops, TroopsConfig};
pub use walker::{Door, Mood, Seen, Surroundings, Walker, WalkArea};

use glam::{Vec2, Vec3};
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::weapons::{WeaponId, WeaponTable};
use prs_sim::{Sim, SoldierId};
use walker::Dice;

/// How bots fight in this game, with the active mod's say.
pub fn tuned_troops() -> TroopsConfig {
    prs_mods::tuned("ai.troops", TroopsConfig::default())
}

/// How the crowd walks about in this game, with the active mod's say.
pub fn tuned_walkers() -> WalkerConfig {
    prs_mods::tuned("ai.walkers", WalkerConfig::default())
}

/// Lengths are metres, times are seconds and angles are radians. A pair is the least and the most.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WalkerConfig {
    pub count: usize,
    pub seed: u32,
    /// Share of them around where a player starts, with the rest around the middle of the map, so
    /// there is someone to see from the start and a town with people in it further on.
    pub near_spawn_share: f32,
    /// How far they roam from the middle of the map, and from where a player starts.
    pub town_radius: f32,
    pub spawn_radius: f32,
    /// What they carry, one after another down the crowd.
    pub primaries: Vec<WeaponId>,
    /// Tries at finding a clear place to start before making do with the middle of the area.
    pub place_tries: u32,
    /// Ticks between looking where they are going.
    pub think_ticks: u32,
    /// Most rays a walker spends in one think.
    pub max_rays: u32,
    /// Height above the feet of the ray that looks ahead. Above anything that can be stepped on, and
    /// low enough to catch a low wall.
    pub probe_height: f32,
    pub probe_ahead: f32,
    /// How far ahead to check there is ground to walk on, and how far it may drop before it is a ledge.
    pub ledge_ahead: f32,
    pub ledge_drop: f32,
    /// Angle between the ways tried, either side of the way wanted, when that way is blocked.
    pub fan_step: f32,
    /// Once turned aside by something, how long to keep going that way before trying for the goal
    /// again. Without this it would turn back the moment the wall was out of the ray's sight, for ever.
    pub detour_time: Vec2,
    /// How many of those to take before deciding it was somewhere else it wanted to be.
    pub detours_per_goal: u32,
    /// Share of a run's pace.
    pub walk_pace: f32,
    pub jog_pace: f32,
    /// How fast they turn, radians a second.
    pub turn_rate: f32,
    pub goal_reach: f32,
    /// How far away the next place to go is.
    pub goal_range: Vec2,
    /// How many places to try for one it can walk straight to, before settling for the last.
    pub goal_tries: u32,
    /// Given up on a place that has not been reached in this long.
    pub goal_time: f32,
    /// Chance the next place to go is through a door nearby, in and out again.
    pub door_chance: f32,
    pub door_reach: f32,
    /// How far back from a door to line up with it, and how far past it to go in.
    pub door_lead: f32,
    pub door_inside: f32,
    /// How long they go on doing one thing, and the chance the next thing is each of these. Walking
    /// is whatever is left.
    pub mood_time: Vec2,
    pub stop_time: Vec2,
    pub jog_chance: f32,
    pub stop_chance: f32,
    /// Chance of doing that crouched, and if stopped of leaning out.
    pub crouch_chance: f32,
    pub lean_chance: f32,
    /// How far they glance off the way they are going, walking and stopped, and how long for.
    pub glance_walking: f32,
    pub glance_stopped: f32,
    pub glance_pitch: f32,
    pub glance_time: Vec2,
    /// Moved no further than this in this long, while meaning to move, is stuck.
    pub stuck_distance: f32,
    pub stuck_time: f32,
    /// Getting no further than this from one spot in this long, while meaning to move, is as good as stuck.
    pub mill_distance: f32,
    pub mill_time: f32,
    /// How long to strike out in a new direction after that, with no thought for the goal.
    pub unstick_time: f32,
    /// How many times running it has to happen before trying a jump.
    pub jump_after: u32,
    /// Keeps this far from anyone in front of them, and turns this far to do it.
    pub personal_space: f32,
    pub give_way: f32,
}

impl Default for WalkerConfig {
    fn default() -> Self {
        Self {
            count: 12,
            seed: 0xC0DA,
            near_spawn_share: 0.25,
            town_radius: 95.0,
            spawn_radius: 40.0,
            primaries: vec![
                WeaponTable::ASSAULT_RIFLE,
                WeaponTable::ASSAULT_RIFLE,
                WeaponTable::MACHINE_GUN,
                WeaponTable::ASSAULT_RIFLE,
                WeaponTable::MARKSMAN_RIFLE,
            ],
            place_tries: 40,
            think_ticks: 12,
            max_rays: 8,
            probe_height: 0.55,
            probe_ahead: 2.2,
            ledge_ahead: 1.2,
            ledge_drop: 1.3,
            fan_step: 0.6,
            detour_time: Vec2::new(1.5, 4.5),
            detours_per_goal: 4,
            walk_pace: 0.46,
            jog_pace: 1.0,
            turn_rate: 2.4,
            goal_reach: 1.2,
            goal_range: Vec2::new(12.0, 45.0),
            goal_tries: 4,
            goal_time: 30.0,
            door_chance: 0.3,
            door_reach: 45.0,
            door_lead: 1.6,
            door_inside: 2.2,
            mood_time: Vec2::new(5.0, 16.0),
            stop_time: Vec2::new(1.5, 7.0),
            jog_chance: 0.2,
            stop_chance: 0.25,
            crouch_chance: 0.15,
            lean_chance: 0.25,
            glance_walking: 0.35,
            glance_stopped: 1.2,
            glance_pitch: 0.12,
            glance_time: Vec2::new(1.2, 4.0),
            stuck_distance: 0.6,
            stuck_time: 2.5,
            mill_distance: 4.0,
            mill_time: 12.0,
            unstick_time: 1.6,
            jump_after: 2,
            personal_space: 1.5,
            give_way: 0.7,
        }
    }
}

/// Everyone walking about in one sim, and what they want. Plain data. Not part of the sim's state,
/// because no client ever has to agree with it: only their commands count.
#[derive(Clone, Debug, PartialEq)]
pub struct Crowd {
    pub cfg: WalkerConfig,
    pub areas: Vec<WalkArea>,
    pub walkers: Vec<Walker>,
    /// Where everyone is, and what they are told to do. Kept to save allocating them every tick.
    feet: Vec<Vec2>,
    cmds: Vec<(SoldierId, PlayerCmd)>,
}

fn doors_near(sim: &Sim, center: Vec2, radius: f32) -> Vec<Door> {
    sim.level
        .doorways
        .iter()
        .map(|d| Door { at: Vec2::new(d.at.x, d.at.z), outward: Vec2::new(d.outward.x, d.outward.z).normalize_or_zero() })
        .filter(|d| d.at.distance(center) < radius)
        .collect()
}

/// The capture point nearest the middle of the map is the middle of what goes on there, if it has any.
fn middle_of(sim: &Sim) -> Option<Vec2> {
    sim.level.capture_points.iter().map(|p| Vec2::new(p.center.x, p.center.z)).min_by(|a, b| a.length().total_cmp(&b.length()))
}

/// Ground for a soldier to stand on somewhere in an area: out in the open and not on top of anything.
fn place_in(sim: &Sim, area: &WalkArea, dice: &mut Dice, tries: u32) -> Option<Vec3> {
    const SKY: f32 = 400.0;
    let capsule = sim.move_cfg.capsule(prs_sim::movement::Stance::Stand);
    (0..tries).find_map(|_| {
        let at = area.center + dice.in_circle() * area.radius;
        let top = Vec3::new(at.x, SKY, at.y);
        let hit = sim.world.raycast(top, Vec3::NEG_Y, SKY * 2.0)?;
        let ground = top - Vec3::Y * hit.distance;
        let walkable = hit.normal.y >= sim.move_cfg.max_slope_cos;
        // Below a roof there is more of the world, and below the ground there is nothing.
        let on_top = sim.world.raycast(ground - Vec3::Y * 0.5, Vec3::NEG_Y, SKY).is_some();
        let feet = ground + Vec3::Y * 0.05;
        (walkable && !on_top && sim.world.capsule_is_clear(feet, capsule)).then_some(feet)
    })
}

impl Crowd {
    /// Adds them to a sim, half on each team, wherever on the map there is room.
    pub fn populate(sim: &mut Sim, cfg: WalkerConfig) -> Self {
        let spawn = Vec2::new(sim.level.spawn.x, sim.level.spawn.z);
        // A map with nothing to hold, like the range, has only somewhere to start from.
        let (middle, radius) = middle_of(sim).map_or((spawn, cfg.spawn_radius), |middle| (middle, cfg.town_radius));
        let areas = vec![
            WalkArea { center: middle, radius, doors: doors_near(sim, middle, radius) },
            WalkArea { center: spawn, radius: cfg.spawn_radius, doors: doors_near(sim, spawn, cfg.spawn_radius) },
        ];
        let mut walkers = Vec::with_capacity(cfg.count);
        for i in 0..cfg.count {
            let seed = cfg.seed.wrapping_add((i as u32 + 1).wrapping_mul(0x9E37_79B1));
            let mut dice = Dice::new(seed);
            let near_spawn = (i as f32 + 0.5) * cfg.near_spawn_share % 1.0 < cfg.near_spawn_share;
            let area = usize::from(near_spawn);
            let feet = place_in(sim, &areas[area], &mut dice, cfg.place_tries).unwrap_or(sim.level.spawn);
            let mut loadout = sim.inventory_cfg.loadout;
            if !cfg.primaries.is_empty() {
                loadout.primary = cfg.primaries[i % cfg.primaries.len()];
            }
            let id = sim.spawn_soldier_with(seed, (i % 2) as u8, loadout);
            let yaw = dice.between(Vec2::new(-std::f32::consts::PI, std::f32::consts::PI));
            sim.set_home(id, feet, yaw);
            walkers.push(Walker::new(id, area, dice, i as u32, Vec2::new(feet.x, feet.z), yaw));
        }
        Self { cfg, areas, walkers, feet: Vec::new(), cmds: Vec::new() }
    }

    /// What everyone wants to do on the coming tick. Pass all of it to `Sim::step` along with anyone else's.
    pub fn think(&mut self, sim: &Sim) -> &[(SoldierId, PlayerCmd)] {
        self.feet.clear();
        self.feet.extend(sim.soldiers.iter().filter(|s| s.present && s.health.is_up()).map(|s| Vec2::new(s.body.pos.x, s.body.pos.z)));
        self.cmds.clear();
        for walker in &mut self.walkers {
            let soldier = &sim.soldiers[walker.soldier];
            if !soldier.present {
                continue;
            }
            let seen = Seen {
                feet: soldier.body.pos,
                up: soldier.health.is_up(),
                crouched: soldier.body.stance == prs_sim::movement::Stance::Crouch,
                lean_side: soldier.body.lean.side,
            };
            let around = Surroundings {
                world: &sim.world,
                walkable_cos: sim.move_cfg.max_slope_cos,
                area: &self.areas[walker.area],
                others: &self.feet,
                way: None,
            };
            let mut cmd = walker.step(&self.cfg, &seen, &around, sim.tick);
            no_window_on_a_jump(sim, walker.soldier, &mut cmd);
            self.cmds.push((walker.soldier, cmd));
        }
        &self.cmds
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod skill_tests;
#[cfg(test)]
mod sight_tests;
#[cfg(test)]
mod range_report;
#[cfg(test)]
mod flow_report;

/// A jump to get unstuck is not a reason to go in through a window, where nothing says the way out, so it
/// stays a jump: a climb only starts walking forward, on the ground or on the way up.
pub(crate) fn no_window_on_a_jump(sim: &Sim, id: SoldierId, cmd: &mut PlayerCmd) {
    let body = &sim.soldiers[id].body;
    let jumping = cmd.buttons.contains(Buttons::JUMP) || (!body.grounded && body.jump_cooldown > 0.0);
    if jumping && sim.climb_plan_looking(id, cmd.yaw).is_some_and(|climb| climb.squeeze > 0.0) {
        cmd.wish_move.y = cmd.wish_move.y.min(0.0);
    }
}
