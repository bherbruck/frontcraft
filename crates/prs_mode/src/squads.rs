//! Squads and their rally points.

use glam::{Vec2, Vec3};
use prs_sim::movement::Stance;
use prs_sim::{Sim, SoldierId};

use crate::state::{ModeEvent, Rally, RallyLoss, Refusal, Squad};
use crate::{Match, base_spot, seconds_to_ticks};

/// Golden angle in radians, which spreads soldiers around a point however many there are.
const SPIRAL_TURN: f32 = 2.399_963;

impl Match {
    /// Into the first squad with room, or a new one if there is none and that is allowed.
    pub(crate) fn auto_squad(&mut self, soldier: SoldierId) {
        let team = self.state.player(soldier).map_or(0, |p| p.team);
        let room = self.mode.cfg.squads.max_members;
        let open = self.state.squads.iter().find(|s| s.team == team && s.members.len() < room).map(|s| s.id);
        let result = match open {
            Some(id) => self.join_squad(soldier, id),
            None => self.create_squad_for(soldier),
        };
        // No room anywhere just means a soldier with no squad.
        let _ = result;
    }

    pub(crate) fn create_squad_for(&mut self, soldier: SoldierId) -> Result<(), Refusal> {
        let team = self.state.player(soldier).map_or(0, |p| p.team);
        let squads = self.state.squads.iter().filter(|s| s.team == team).count();
        if squads >= self.mode.cfg.squads.max_per_team {
            return Err(Refusal::TooManySquads);
        }
        self.leave_squad(soldier);
        let id = self.state.next_squad;
        self.state.next_squad += 1;
        self.state.squads.push(Squad { id, team, leader: soldier, members: vec![soldier], rally: None, rally_ready_at: 0 });
        self.state.player_mut(soldier).expect("in the match").squad = Some(id);
        Ok(())
    }

    pub(crate) fn join_squad(&mut self, soldier: SoldierId, id: u32) -> Result<(), Refusal> {
        let player = self.state.player(soldier).expect("in the match");
        let (team, human) = (player.team, !player.bot);
        if player.squad == Some(id) {
            return Ok(());
        }
        let squad = self.state.squad(id).ok_or(Refusal::NoSuchSquad)?;
        if squad.team != team {
            return Err(Refusal::OtherTeam);
        }
        if squad.members.len() >= self.mode.cfg.squads.max_members {
            return Err(Refusal::SquadFull);
        }
        self.leave_squad(soldier);
        // A player takes over from a bot, because bots do not lead.
        let bot_leads = {
            let squad = self.state.squad(id).expect("just checked");
            self.state.player(squad.leader).is_none_or(|p| p.bot)
        };
        let squad = self.state.squads.iter_mut().find(|s| s.id == id).expect("just checked");
        squad.members.push(soldier);
        if human && bot_leads {
            squad.leader = soldier;
        }
        self.state.player_mut(soldier).expect("in the match").squad = Some(id);
        Ok(())
    }

    /// Out of whatever squad they are in. A squad left without a leader takes the next player, and
    /// an empty one is gone along with its rally point.
    pub(crate) fn leave_squad(&mut self, soldier: SoldierId) {
        let Some(player) = self.state.player_mut(soldier) else {
            return;
        };
        let Some(id) = player.squad.take() else {
            return;
        };
        let squad = self.state.squads.iter_mut().find(|s| s.id == id).expect("a player's squad exists");
        squad.members.retain(|&m| m != soldier);
        if squad.members.is_empty() {
            if squad.rally.is_some() {
                self.events.push(ModeEvent::RallyLost { squad: id, why: RallyLoss::SquadGone });
            }
            self.state.squads.retain(|s| s.id != id);
            return;
        }
        if squad.leader == soldier {
            let members = squad.members.clone();
            let human = members.iter().copied().find(|&m| self.state.player(m).is_some_and(|p| !p.bot));
            let squad = self.state.squads.iter_mut().find(|s| s.id == id).expect("still there");
            squad.leader = human.unwrap_or(members[0]);
        }
    }

    /// All the rules for putting one down, in one place. Each of these is a reason a leader is told no.
    pub(crate) fn place_rally(&mut self, sim: &Sim, soldier: SoldierId) -> Result<(), Refusal> {
        let rules = self.mode.cfg.rally.clone().ok_or(Refusal::NoRallyRules)?;
        if self.state.ended() {
            return Err(Refusal::RoundOver);
        }
        let team = self.state.player(soldier).expect("in the match").team;
        let squad = self.state.squad_of(soldier).ok_or(Refusal::NotLeader)?;
        let id = squad.id;
        if squad.leader != soldier {
            return Err(Refusal::NotLeader);
        }
        let leader = &sim.soldiers[soldier];
        if !leader.present || !leader.health.is_up() {
            return Err(Refusal::NotUp);
        }
        if !leader.body.grounded {
            return Err(Refusal::NotOnGround);
        }
        if sim.tick < squad.rally_ready_at {
            return Err(Refusal::RallyCooling);
        }
        let at = leader.body.pos;
        let near_mates = squad
            .members
            .iter()
            .filter(|&&m| m != soldier && sim.soldiers[m].present && sim.soldiers[m].health.is_up())
            .filter(|&&m| sim.soldiers[m].body.pos.distance(at) <= rules.nearby_radius)
            .count();
        if near_mates < rules.needs_nearby {
            return Err(Refusal::NoSquadmateNear);
        }
        if self.count_near(sim, at, rules.enemy_radius, |t| t != team) > 0 {
            return Err(Refusal::EnemyNear);
        }
        let (base, _) = base_spot(sim, team, 0);
        if sim.level.teams.get(team as usize).is_some() && flat_distance(base, at) < rules.min_from_base {
            return Err(Refusal::TooCloseToBase);
        }
        if self.enemy_base_at(sim, at, team, 0.0).is_some() {
            return Err(Refusal::TooCloseToBase);
        }
        if rules.clear_of_zones && sim.level.capture_points.iter().any(|z| flat_distance(z.center, at) <= z.radius) {
            return Err(Refusal::InAZone);
        }
        let has_tickets = self.mode.cfg.tickets.is_some();
        if has_tickets && self.state.teams[team as usize].tickets <= rules.ticket_cost {
            return Err(Refusal::NoTickets);
        }
        if has_tickets {
            self.state.teams[team as usize].tickets -= rules.ticket_cost;
        }
        let squad = self.state.squads.iter_mut().find(|s| s.id == id).expect("just looked at it");
        if squad.rally.is_some() {
            self.events.push(ModeEvent::RallyLost { squad: id, why: RallyLoss::Replaced });
        }
        let expires_at = sim.tick + seconds_to_ticks(rules.lifetime);
        squad.rally = Some(Rally { pos: at, yaw: leader.yaw, spawns_left: rules.spawns, expires_at, overrun_since: None });
        squad.rally_ready_at = sim.tick + seconds_to_ticks(rules.cooldown);
        self.events.push(ModeEvent::RallyPlaced { squad: id });
        Ok(())
    }

    /// How many soldiers of the teams asked for are on their feet this close.
    fn count_near(&self, sim: &Sim, at: Vec3, radius: f32, wanted: impl Fn(u8) -> bool) -> usize {
        let close = |p: &&crate::PlayerState| {
            let soldier = &sim.soldiers[p.soldier];
            soldier.present && soldier.health.is_up() && soldier.body.pos.distance(at) <= radius
        };
        self.state.players.iter().filter(|p| wanted(p.team)).filter(close).count()
    }

    /// Clocks, and whether any have been overrun or have run out.
    pub(crate) fn step_rallies(&mut self, sim: &Sim) {
        let Some(rules) = self.mode.cfg.rally.clone() else {
            return;
        };
        for index in 0..self.state.squads.len() {
            let squad = &self.state.squads[index];
            let (id, team) = (squad.id, squad.team);
            let rally = squad.rally;
            let lost = rally.and_then(|mut rally| {
                let enemies = self.count_near(sim, rally.pos, rules.overrun_radius, |t| t != team);
                let friends = self.count_near(sim, rally.pos, rules.overrun_radius, |t| t == team);
                if (enemies > friends) != rally.overrun_since.is_some() {
                    rally.overrun_since = (enemies > friends).then_some(sim.tick);
                    self.state.squads[index].rally = Some(rally);
                }
                if rally.overrun_since.is_some_and(|since| sim.tick >= since + seconds_to_ticks(rules.overrun_seconds)) {
                    Some(RallyLoss::Overrun)
                } else if sim.tick >= rally.expires_at {
                    Some(RallyLoss::Expired)
                } else {
                    None
                }
            });
            let squad = &mut self.state.squads[index];
            if let Some(why) = lost {
                squad.rally = None;
                self.events.push(ModeEvent::RallyLost { squad: id, why });
            }
        }
    }

    pub(crate) fn use_rally(&mut self, soldier: SoldierId) {
        let Some(id) = self.state.player(soldier).and_then(|p| p.squad) else {
            return;
        };
        let squad = self.state.squads.iter_mut().find(|s| s.id == id).expect("a player's squad exists");
        let Some(rally) = &mut squad.rally else {
            return;
        };
        rally.spawns_left -= 1;
        if rally.spawns_left == 0 {
            squad.rally = None;
            self.events.push(ModeEvent::RallyLost { squad: id, why: RallyLoss::UsedUp });
        }
    }

    /// Clear ground near it, a different way each time it is used, or on top of it if there is none.
    pub(crate) fn rally_spot(&self, sim: &Sim, rally: &Rally) -> Vec3 {
        let Some(rules) = self.mode.cfg.rally.as_ref() else {
            return rally.pos;
        };
        let capsule = sim.move_cfg.capsule(Stance::Stand);
        let start = (rules.spawns - rally.spawns_left) as f32;
        for attempt in 0..rules.spawn_tries {
            let angle = (start + attempt as f32) * SPIRAL_TURN;
            let share = (attempt as f32 + 1.0) / rules.spawn_tries.max(1) as f32;
            let offset = Vec2::from_angle(angle) * rules.spawn_spread * share.sqrt();
            let top = rally.pos + Vec3::new(offset.x, rules.ground_probe, offset.y);
            let Some(hit) = sim.world.raycast(top, Vec3::NEG_Y, rules.ground_probe * 2.0) else {
                continue;
            };
            let feet = top + Vec3::NEG_Y * (hit.distance - rules.spawn_lift);
            if hit.normal.y >= sim.move_cfg.max_slope_cos && sim.world.capsule_is_clear(feet, capsule) {
                return feet;
            }
        }
        rally.pos + Vec3::Y * rules.spawn_lift
    }
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}
