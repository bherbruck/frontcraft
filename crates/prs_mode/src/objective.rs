//! What a mode is about. The harness handles sides, squads, spawning and tickets, and an [`Objective`]
//! adds whatever makes one mode different from another. Anything it has to remember goes in
//! [`MatchState`], so a server can send it and an export can restore it. To add a mode, implement this
//! or combine what is here with different data in a [`crate::ModeConfig`].

use glam::{Vec2, Vec3};
use prs_sim::{CapturePoint, Level, Sim, Soldier, heli_of};
use prs_sim::protocol::TICK_DT;
use std::fmt::Debug;

use crate::config::CaptureRules;
use crate::state::{MatchState, ModeEvent, PointState, TEAMS};

/// What a squad of a team should be doing. For bots, and for anyone else who wants to know.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    pub point: usize,
    pub pos: Vec3,
    pub radius: f32,
    pub attack: bool,
}

pub trait Objective: Debug + Send + Sync {
    /// Sets up its part of a new round.
    fn begin(&self, level: &Level, state: &mut MatchState);
    /// One tick. Only called while the round is being played.
    fn step(&self, sim: &Sim, state: &mut MatchState, events: &mut Vec<ModeEvent>);
    /// Where a squad should be heading, if there is anywhere in particular.
    fn order(&self, level: &Level, state: &MatchState, team: u8, squad: u32) -> Option<Order>;
    /// Which zone this spot is in and how that zone stands for this team, if there are zones.
    fn zone_status(&self, _level: &Level, _state: &MatchState, _soldier: &Soldier) -> Option<(usize, ZoneStatus)> {
        None
    }
}

/// Nothing to do but fight.
#[derive(Clone, Copy, Debug)]
pub struct FightOnly;

impl Objective for FightOnly {
    fn begin(&self, _level: &Level, _state: &mut MatchState) {}

    fn step(&self, _sim: &Sim, _state: &mut MatchState, _events: &mut Vec<ModeEvent>) {}

    fn order(&self, _level: &Level, _state: &MatchState, _team: u8, _squad: u32) -> Option<Order> {
        None
    }
}

/// Points in a chain, taken in order, with tickets bleeding from the side that holds fewer.
#[derive(Clone, Debug)]
pub struct AdvanceAndSecure(pub CaptureRules);

/// How a point stands from where one team's soldier is standing, for showing them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneStatus {
    /// Nothing going on and nothing you can do: it is yours and quiet.
    Held,
    /// Your side is pushing its progress up.
    Taking,
    /// Your side is bringing someone else's hold down.
    Neutralising,
    /// The other side is winning it.
    Losing,
    /// Even numbers, so nothing moves.
    Contested,
    /// It is not the next one in the chain for you.
    NotNext,
    /// You are in it but not enough of you to count.
    TooFew,
    /// You are in it, but not in a way that counts: up on a roof or in the air.
    NotCounted,
}

/// The order of the chain: as given, or by how much nearer each point is to one base than the other.
/// That beats going by a straight line between them, because it keeps a point out on a flank next to
/// whatever it is really nearest, and it works for a map bent any way.
fn chain_order(level: &Level, rules: &CaptureRules) -> Vec<usize> {
    if let Some(chain) = &rules.chain {
        return chain.clone();
    }
    let flat = |p: Vec3| Vec2::new(p.x, p.z);
    let base = |team: usize| level.teams.get(team).and_then(|t| t.spawns.first()).map(|s| flat(s.pos));
    let start = base(0).unwrap_or_else(|| flat(level.spawn));
    let end = base(1).unwrap_or(start + Vec2::NEG_Y * level.terrain.size);
    let along = |i: usize| {
        let at = flat(level.capture_points[i].center);
        let (from, to) = (at.distance(start), at.distance(end));
        from / (from + to).max(1e-3)
    };
    let mut chain: Vec<usize> = (0..level.capture_points.len()).collect();
    chain.sort_by(|a, b| along(*a).total_cmp(&along(*b)));
    chain
}

impl AdvanceAndSecure {
    fn may_attack(state: &MatchState, point: usize, team: u8) -> bool {
        state.attack_target(team) == Some(point)
    }

    fn in_zone(&self, zone: &CapturePoint, at: Vec3) -> bool {
        let offset = at - zone.center;
        Vec2::new(offset.x, offset.z).length() <= zone.radius && offset.y.abs() <= self.0.zone_height
    }

    /// Whether being in a zone the way this soldier is counts for anything. Not on a roof they needed
    /// someone's back to reach, and not from the air.
    fn counts(&self, soldier: &Soldier) -> bool {
        let boosted = soldier.body.boosted && !self.0.boosted_soldiers_count;
        let flying = soldier.seat.is_some_and(|at| heli_of(at.carrier).is_some()) && !self.0.aircraft_count;
        !boosted && !flying
    }

    /// How fast a side with this many more than the other moves a point, as a share per second.
    fn rate(&self, advantage: u32) -> f32 {
        let rules = &self.0;
        let extra = advantage.saturating_sub(1) as f32 * rules.per_extra_soldier;
        (1.0 + extra.min(rules.max_speedup)) / rules.capture_seconds.max(1e-3)
    }

    /// Who is where. Only someone on their feet holds ground, and only ground someone else could walk to.
    fn count_inside(&self, sim: &Sim, state: &mut MatchState) {
        let MatchState { points, players, .. } = state;
        for (index, point) in points.iter_mut().enumerate() {
            let zone = &sim.level.capture_points[index];
            point.inside = [0; TEAMS];
            for player in players.iter() {
                let soldier = &sim.soldiers[player.soldier];
                if !soldier.present || !soldier.health.is_up() || !self.counts(soldier) {
                    continue;
                }
                if self.in_zone(zone, soldier.body.pos) && (player.team as usize) < TEAMS {
                    point.inside[player.team as usize] += 1;
                }
            }
        }
    }

    /// Which way a point should be moving now, and who is doing it. All the rules of capturing are here,
    /// and all [`Self::step_point`] does is make that happen, so that nothing changes until this does.
    fn wanted_motion(&self, point: &PointState, may_attack: [bool; TEAMS]) -> Motion {
        let [a, b] = point.inside;
        let enough = |count: u32| count >= self.0.min_soldiers;
        let leader = match (a > b && enough(a), b > a && enough(b)) {
            (true, _) => Some(0u8),
            (_, true) => Some(1u8),
            _ => None,
        };
        let drift = self.0.drift_per_second * TICK_DT;
        let Some(team) = leader else {
            if a != 0 || b != 0 {
                return Motion::Still;
            }
            return match (point.owner, point.claimant) {
                (Some(_), _) => Motion::Up(drift),
                (None, Some(_)) => Motion::Down(drift),
                (None, None) => Motion::Still,
            };
        };
        let speed = self.rate(a.abs_diff(b)) * TICK_DT;
        if point.owner == Some(team) {
            return Motion::Up(speed);
        }
        if !may_attack[team as usize] {
            return Motion::Still;
        }
        match point.claimant {
            Some(claimant) if claimant == team => Motion::Up(speed),
            None => Motion::Claim(team, speed),
            Some(_) => Motion::Down(speed),
        }
    }

    fn step_point(&self, state: &mut MatchState, index: usize, now: u32, events: &mut Vec<ModeEvent>) {
        let may_attack = [Self::may_attack(state, index, 0), Self::may_attack(state, index, 1)];
        let point = &mut state.points[index];
        let here = point.progress_at(now);
        // Moving, and arrived, one way or the other.
        if point.rate > 0.0 && here >= 1.0 {
            *point = PointState { progress: 1.0, rate: 0.0, since: now, ..*point };
            if point.owner != point.claimant {
                point.owner = point.claimant;
                if let Some(team) = point.claimant {
                    events.push(ModeEvent::PointCaptured { point: index, team });
                }
            }
        } else if point.rate < 0.0 && here <= 0.0 {
            let held_by = point.owner;
            *point = PointState { owner: None, claimant: None, progress: 0.0, rate: 0.0, since: now, ..*point };
            if let Some(owner) = held_by {
                events.push(ModeEvent::PointNeutralised { point: index, by: 1 - owner });
            }
        }
        let rate = match self.wanted_motion(point, may_attack) {
            Motion::Still => 0.0,
            Motion::Up(rate) if here < 1.0 => rate,
            Motion::Down(rate) if here > 0.0 => -rate,
            Motion::Up(_) | Motion::Down(_) => 0.0,
            Motion::Claim(team, rate) => {
                point.claimant = Some(team);
                rate
            }
        };
        if rate != point.rate {
            point.progress = here;
            point.rate = rate;
            point.since = now;
        }
    }

    /// A ticket goes every so many ticks from whoever holds fewer points. A change in how far behind
    /// they are keeps what was already owed towards the next one.
    fn bleed(&self, state: &mut MatchState, now: u32) {
        let held = [state.points_held(0), state.points_held(1)];
        for team in 0..TEAMS {
            let behind = held[1 - team].saturating_sub(held[team]) as f32;
            let per_tick = behind * self.0.bleed_per_point_per_minute / 60.0 * TICK_DT;
            let every = if per_tick > 0.0 { (1.0 / per_tick).round().max(1.0) as u32 } else { 0 };
            let side = &mut state.teams[team];
            if every != side.bleed_every {
                let owed = if side.bleed_every > 0 { side.next_bleed.saturating_sub(now) as f32 / side.bleed_every as f32 } else { 1.0 };
                side.bleed_every = every;
                side.next_bleed = now + (owed * every as f32).round() as u32;
            }
            if side.bleed_every > 0 && now >= side.next_bleed {
                side.tickets -= 1;
                side.next_bleed = now + side.bleed_every;
            }
        }
    }
}

/// What a point wants to be doing, per tick.
enum Motion {
    Still,
    Up(f32),
    Down(f32),
    Claim(u8, f32),
}

/// How a point stands for someone of this team who is in it.
fn status_of(state: &MatchState, index: usize, team: u8, min_soldiers: u32) -> ZoneStatus {
    let point = &state.points[index];
    let mine = point.inside[team as usize];
    let theirs = point.inside[1 - team as usize];
    if mine == theirs && mine > 0 {
        return ZoneStatus::Contested;
    }
    if theirs > mine {
        let they_may = state.attack_target(1 - team) == Some(index);
        return if they_may && theirs >= min_soldiers { ZoneStatus::Losing } else { ZoneStatus::Contested };
    }
    if mine < min_soldiers {
        return ZoneStatus::TooFew;
    }
    if point.owner == Some(team) {
        return ZoneStatus::Held;
    }
    if state.attack_target(team) != Some(index) {
        return ZoneStatus::NotNext;
    }
    match point.claimant {
        Some(claimant) if claimant != team => ZoneStatus::Neutralising,
        _ => ZoneStatus::Taking,
    }
}

impl Objective for AdvanceAndSecure {
    fn begin(&self, level: &Level, state: &mut MatchState) {
        state.chain = chain_order(level, &self.0);
        state.points = vec![PointState { since: state.started_at, ..Default::default() }; level.capture_points.len()];
        // Never so many that the two ends would meet, so there is always something to fight over.
        let held = self.0.start_held.min(state.chain.len().saturating_sub(1) / 2);
        for team in 0..TEAMS {
            for step in 0..held {
                let at = if team == 0 { step } else { state.chain.len() - 1 - step };
                let point = &mut state.points[state.chain[at]];
                point.owner = Some(team as u8);
                point.claimant = Some(team as u8);
                point.progress = 1.0;
            }
        }
    }

    fn step(&self, sim: &Sim, state: &mut MatchState, events: &mut Vec<ModeEvent>) {
        self.count_inside(sim, state);
        for index in 0..state.points.len() {
            self.step_point(state, index, sim.tick, events);
        }
        self.bleed(state, sim.tick);
    }

    fn zone_status(&self, level: &Level, state: &MatchState, soldier: &Soldier) -> Option<(usize, ZoneStatus)> {
        let index = level.capture_points.iter().position(|zone| self.in_zone(zone, soldier.body.pos))?;
        if !self.counts(soldier) {
            return Some((index, ZoneStatus::NotCounted));
        }
        Some((index, status_of(state, index, soldier.team, self.0.min_soldiers)))
    }

    /// Every other squad of a side takes the front point back when it is being lost, and otherwise everyone
    /// goes for the next point along. Squad ids run across both sides, so it goes by a squad's place in its own.
    fn order(&self, level: &Level, state: &MatchState, team: u8, squad: u32) -> Option<Order> {
        let place = state.squads.iter().filter(|s| s.team == team).position(|s| s.id == squad).unwrap_or(0);
        let front = state.front_point(team);
        let threatened = front.filter(|&i| state.points[i].rate < 0.0 || state.points[i].progress < 1.0 || state.points[i].inside[1 - team as usize] > 0);
        let (point, attack) = match (threatened, state.attack_target(team)) {
            (Some(front), _) if place % 2 == 1 => (front, false),
            (_, Some(target)) => (target, true),
            _ => (front?, false),
        };
        let zone = level.capture_points.get(point)?;
        Some(Order { point, pos: zone.center, radius: zone.radius, attack })
    }
}
