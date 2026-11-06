//! The rules of a match, laid over the simulation: sides, squads, spawning, tickets and how a round is
//! won. Nothing here knows about Bevy, a screen or a network. A host calls [`Match::step`] straight after
//! `Sim::step`, with whatever players have asked of the mode that tick. It reads what happened from the
//! sim's events and soldiers, and brings soldiers in through the sim, so bullets, wounds and everything
//! else about a soldier work the same in any mode.

mod config;
mod objective;
mod squads;
mod state;

pub use config::{BaseRules, CaptureRules, Deployment, IdentifyRules, Kit, Mode, ModeConfig, RallyRules, SquadRules, TicketRules, standard_kits};
pub use objective::{AdvanceAndSecure, FightOnly, Objective, Order, ZoneStatus};
pub use state::{
    MatchState, ModeCmd, ModeEvent, Phase, PlayerState, PointState, Rally, RallyLoss, Refusal, Score, SpawnAt, Squad, TEAMS, TeamState,
};

use glam::Vec3;
use prs_sim::health::{HealthEvent, Zone};
use prs_sim::protocol::TICK_DT;
use prs_sim::{CarryEvent, Care, Sim, SimEvent, SoldierId};

/// One match on one sim.
pub struct Match {
    pub mode: Mode,
    pub state: MatchState,
    /// Filled during [`Self::step`] and by joining and leaving, cleared at the start of the next step.
    pub events: Vec<ModeEvent>,
    /// Slots bots have left, to use again rather than growing the sim for ever.
    free_bot_slots: Vec<SoldierId>,
}

impl Match {
    /// A round ready to begin. The dead now wait for this to bring them back.
    pub fn new(mode: Mode, sim: &mut Sim) -> Self {
        sim.hold_respawns = true;
        let state = MatchState {
            round: 0,
            started_at: sim.tick,
            phase: Phase::Playing,
            teams: Vec::new(),
            points: Vec::new(),
            chain: Vec::new(),
            squads: Vec::new(),
            players: Vec::new(),
            next_squad: 0,
        };
        let mut game = Self { mode, state, events: Vec::new(), free_bot_slots: Vec::new() };
        game.begin_round(sim);
        game
    }

    pub fn export_state(&self) -> MatchState {
        self.state.clone()
    }

    pub fn import_state(&mut self, state: &MatchState) {
        self.state.clone_from(state);
    }

    /// Where a squad should be heading. See [`Objective::order`].
    pub fn order_for(&self, sim: &Sim, soldier: SoldierId) -> Option<Order> {
        let player = self.state.player(soldier)?;
        self.mode.objective.order(&sim.level, &self.state, player.team, player.squad.unwrap_or(0))
    }

    /// Puts a soldier the sim already has into the match. With no team asked for they go on the
    /// smaller side, and then into the first squad with room.
    pub fn join(&mut self, sim: &mut Sim, soldier: SoldierId, team: Option<u8>, bot: bool) {
        if self.state.player(soldier).is_some() {
            return;
        }
        let automatic = self.mode.cfg.deployment == Deployment::Automatic;
        let team = if automatic { sim.soldiers[soldier].team } else { team.unwrap_or_else(|| self.smaller_team()) };
        let team = team.min(TEAMS as u8 - 1);
        self.state.players.push(PlayerState {
            soldier,
            team,
            squad: None,
            kit: 0,
            spawn: SpawnAt::Base,
            ready: false,
            bot,
            deployed: automatic,
            back_at: sim.tick,
            down_since: None,
            charged: false,
            trespass_since: None,
            score: Score::default(),
        });
        if !automatic {
            self.park(sim, soldier);
            self.auto_squad(soldier);
            if bot {
                self.state.player_mut(soldier).expect("just added").kit = self.kit_for_bot(soldier);
            }
        }
    }

    /// Takes someone out of the match. Their slot in the sim stays, empty.
    pub fn leave(&mut self, sim: &mut Sim, soldier: SoldierId) {
        self.leave_squad(soldier);
        self.state.players.retain(|p| p.soldier != soldier);
        sim.soldiers[soldier].present = false;
    }

    /// Keeps the sides the same size with bots: each up to `bots_per_team`, or up to the bigger side's players
    /// if that is more, so the side with fewer players gets the bots. Runs every tick, so bots come and go as
    /// players do. A bot only makes way where nobody sees it go: see [`Self::bot_to_send_home`].
    pub fn fill_bots(&mut self, sim: &mut Sim) {
        let cfg = &self.mode.cfg;
        let Some(per_team) = cfg.bots_per_team else {
            return;
        };
        let humans = [self.state.humans(0), self.state.humans(1)];
        let players = humans[0] + humans[1];
        let side = if cfg.no_bots_from_players.is_some_and(|from| players >= from) { 0 } else { per_team.max(humans[0].max(humans[1])) };
        let mut wanted = [side.saturating_sub(humans[0]), side.saturating_sub(humans[1])];
        if let Some(most) = cfg.bots_most {
            // Off the side with more, so what there is stays as even as it can.
            while wanted[0] + wanted[1] > most {
                let more = usize::from(wanted[1] > wanted[0]);
                wanted[more] -= 1;
            }
        }
        for team in 0..TEAMS as u8 {
            let bots = |state: &MatchState| state.players.iter().filter(|p| p.team == team && p.bot).count();
            while bots(&self.state) < wanted[team as usize] {
                let slot = self.free_bot_slots.pop().unwrap_or_else(|| {
                    let seed = 0xB07u32.wrapping_add(sim.soldiers.len() as u32).wrapping_mul(0x9E37_79B1);
                    let id = sim.spawn_soldier_with(seed, team, sim.inventory_cfg.loadout);
                    sim.soldiers[id].present = false;
                    id
                });
                self.join(sim, slot, Some(team), true);
            }
            while bots(&self.state) > wanted[team as usize] {
                let Some(bot) = self.bot_to_send_home(sim, team) else {
                    break;
                };
                self.leave(sim, bot);
                self.free_bot_slots.push(bot);
            }
        }
    }

    /// The bot on this side that can go without anyone seeing it: one off the field first, then one dead, then
    /// one on its feet with no player near. One in a squad with a player in it goes last. None if every bot
    /// is on its feet in front of someone, in which case one goes as soon as it can.
    fn bot_to_send_home(&self, sim: &Sim, team: u8) -> Option<SoldierId> {
        let near = self.mode.cfg.bot_unseen_distance;
        let seen = |at: glam::Vec3| {
            self.state.players.iter().filter(|p| !p.bot).any(|p| {
                let soldier = &sim.soldiers[p.soldier];
                soldier.present && soldier.body.pos.distance(at) < near
            })
        };
        let with_a_player = |player: &PlayerState| {
            self.state.squad_of(player.soldier).is_some_and(|squad| squad.members.iter().any(|&m| self.state.player(m).is_some_and(|p| !p.bot)))
        };
        self.state
            .players
            .iter()
            .filter(|p| p.team == team && p.bot)
            .filter_map(|p| {
                let soldier = &sim.soldiers[p.soldier];
                let rank = if !p.deployed || !soldier.present {
                    0
                } else if soldier.health.is_dead() {
                    1
                } else if !seen(soldier.body.pos) {
                    2
                } else {
                    return None;
                };
                Some(((rank, with_a_player(p), std::cmp::Reverse(p.soldier)), p.soldier))
            })
            .min_by_key(|(key, _)| *key)
            .map(|(_, soldier)| soldier)
    }

    /// One tick of the rules. Call it straight after `Sim::step`, because it reads that step's events.
    pub fn step(&mut self, sim: &mut Sim, cmds: &[(SoldierId, ModeCmd)]) {
        self.events.clear();
        for &(soldier, cmd) in cmds {
            if let Err(why) = self.command(sim, soldier, cmd) {
                self.refuse(soldier, why);
            }
        }
        self.fill_bots(sim);
        match self.state.phase {
            Phase::Playing => {
                self.step_bases(sim);
                self.read_sim_events(sim);
                self.step_players(sim);
                self.step_rallies(sim);
                let Self { mode, state, events, .. } = self;
                mode.objective.step(sim, state, events);
                self.credit_captures(sim);
                self.check_for_winner(sim.tick);
            }
            Phase::Ended { restart_at, .. } => {
                if sim.tick >= restart_at {
                    self.begin_round(sim);
                }
            }
        }
    }

    /// Everything back to how a round starts. Sides and squads stay together, and the field is cleared.
    fn begin_round(&mut self, sim: &mut Sim) {
        self.state.round += 1;
        self.state.started_at = sim.tick;
        self.state.phase = Phase::Playing;
        let start = self.mode.cfg.tickets.as_ref().map_or(0, |t| t.start);
        self.state.teams = vec![TeamState { tickets: start, ..Default::default() }; TEAMS];
        self.mode.objective.begin(&sim.level, &mut self.state);
        for squad in &mut self.state.squads {
            squad.rally = None;
            squad.rally_ready_at = sim.tick;
        }
        sim.bullets.clear();
        sim.reset_rides();
        for kit in sim.kits.drain(..) {
            sim.events.push(SimEvent::KitGone { kit: kit.id });
        }
        for index in 0..self.state.players.len() {
            let soldier = self.state.players[index].soldier;
            let player = &mut self.state.players[index];
            player.ready = false;
            player.back_at = sim.tick;
            player.down_since = None;
            player.trespass_since = None;
            player.charged = false;
            player.score = Score::default();
            player.spawn = SpawnAt::Base;
            if self.mode.cfg.deployment == Deployment::Automatic {
                sim.respawn(soldier);
            } else {
                player.deployed = false;
                self.park(sim, soldier);
            }
        }
        self.events.push(ModeEvent::RoundStarted { round: self.state.round });
    }

    /// Off the field, at their base, ready to be brought in.
    fn park(&mut self, sim: &mut Sim, soldier: SoldierId) {
        let team = self.state.player(soldier).map_or(0, |p| p.team);
        let (pos, yaw) = base_spot(sim, team, soldier);
        let loadout = self.chosen_loadout(sim, soldier);
        sim.respawn_at(soldier, pos, yaw, loadout);
        sim.soldiers[soldier].team = team;
        sim.soldiers[soldier].present = false;
    }

    fn chosen_loadout(&self, sim: &Sim, soldier: SoldierId) -> prs_sim::inventory::Loadout {
        let kit = self.state.player(soldier).map_or(0, |p| p.kit);
        self.mode.cfg.kits.get(kit).map_or(sim.inventory_cfg.loadout, |k| k.loadout)
    }

    /// The side with fewer real players, since bots make way for them anyway. Bots break a tie.
    fn smaller_team(&self) -> u8 {
        let size = |team: u8| (self.state.humans(team), self.state.team_size(team));
        if size(1) < size(0) { 1 } else { 0 }
    }

    fn command(&mut self, sim: &mut Sim, soldier: SoldierId, cmd: ModeCmd) -> Result<(), Refusal> {
        if self.state.player(soldier).is_none() {
            return Ok(());
        }
        match cmd {
            ModeCmd::SetKit(kit) => {
                if !self.kit_allowed(soldier, kit)? {
                    return Err(Refusal::KitTaken);
                }
                self.state.player_mut(soldier).expect("checked").kit = kit;
            }
            ModeCmd::SetSpawn(at) => {
                if at == SpawnAt::Rally && self.state.squad_of(soldier).is_none_or(|s| s.rally.is_none()) {
                    return Err(Refusal::NoRally);
                }
                self.state.player_mut(soldier).expect("checked").spawn = at;
            }
            ModeCmd::Deploy => {
                if self.state.ended() {
                    return Err(Refusal::RoundOver);
                }
                self.state.player_mut(soldier).expect("checked").ready = true;
            }
            ModeCmd::CancelDeploy => self.state.player_mut(soldier).expect("checked").ready = false,
            ModeCmd::JoinTeam(team) => self.switch_team(sim, soldier, team)?,
            ModeCmd::CreateSquad => self.create_squad_for(soldier)?,
            ModeCmd::JoinSquad(id) => self.join_squad(soldier, id)?,
            ModeCmd::LeaveSquad => self.leave_squad(soldier),
            ModeCmd::PlaceRally => self.place_rally(sim, soldier)?,
            ModeCmd::GiveUp => self.give_up(sim, soldier)?,
        }
        Ok(())
    }

    /// Whether a kit can be taken, given who else in the squad has it.
    pub(crate) fn kit_allowed(&self, soldier: SoldierId, kit: usize) -> Result<bool, Refusal> {
        let def = self.mode.cfg.kits.get(kit).ok_or(Refusal::NoSuchKit)?;
        let (Some(limit), Some(squad)) = (def.per_squad, self.state.squad_of(soldier)) else {
            return Ok(true);
        };
        let taken = squad.members.iter().filter(|&&m| m != soldier && self.state.player(m).is_some_and(|p| p.kit == kit)).count();
        Ok((taken as u32) < limit)
    }

    /// The first kit they could take, going on from the last bot's, so a side comes out mixed.
    fn kit_for_bot(&self, soldier: SoldierId) -> usize {
        let kits = self.mode.cfg.kits.len();
        (0..kits).map(|i| (soldier + i) % kits.max(1)).find(|&kit| self.kit_allowed(soldier, kit) == Ok(true)).unwrap_or(0)
    }

    fn switch_team(&mut self, sim: &mut Sim, soldier: SoldierId, team: u8) -> Result<(), Refusal> {
        let player = self.state.player(soldier).expect("checked");
        if player.team == team || team as usize >= TEAMS {
            return Ok(());
        }
        if player.deployed && !sim.soldiers[soldier].health.is_dead() {
            return Err(Refusal::InTheField);
        }
        let humans = |team: u8| self.state.humans(team);
        let players = humans(0) + humans(1);
        // With only a few players they can sort themselves out however they like.
        let kept_even = players >= self.mode.cfg.balance_from_players;
        if kept_even && humans(team) + 1 > humans(player.team).saturating_sub(1) + self.mode.cfg.team_balance_margin {
            return Err(Refusal::TeamsUneven);
        }
        self.leave_squad(soldier);
        let player = self.state.player_mut(soldier).expect("checked");
        player.team = team;
        player.spawn = SpawnAt::Base;
        player.ready = false;
        sim.soldiers[soldier].team = team;
        self.auto_squad(soldier);
        if !self.state.player(soldier).expect("checked").deployed {
            self.park(sim, soldier);
        }
        Ok(())
    }

    fn give_up(&mut self, sim: &mut Sim, soldier: SoldierId) -> Result<(), Refusal> {
        let health = &sim.soldiers[soldier].health;
        if health.is_up() || health.is_dead() {
            return Err(Refusal::NotUp);
        }
        let down_since = self.state.player(soldier).expect("checked").down_since.unwrap_or(sim.tick);
        if sim.tick < down_since + seconds_to_ticks(self.mode.cfg.give_up_after) {
            return Err(Refusal::NotDownLongEnough);
        }
        // Just hard enough, since a body part's multiplier is rules too.
        let raw = sim.health_cfg.down_kill_damage / sim.health_cfg.zone(Zone::Head).damage.max(1e-3);
        sim.hurt(soldier, Zone::Head, raw * 1.01);
        Ok(())
    }

    /// The middle of the main base of the other side that this spot is inside, if any.
    pub fn enemy_base_at(&self, sim: &Sim, at: Vec3, team: u8, margin: f32) -> Option<Vec3> {
        let rules = self.mode.cfg.base_protection.as_ref()?;
        sim.level.teams.iter().enumerate().filter(|(index, _)| *index != team as usize).find_map(|(_, base)| {
            let center = base.base_center();
            let offset = at - center;
            let inside = glam::Vec2::new(offset.x, offset.z).length() <= base.base_radius + margin && offset.y <= rules.ceiling;
            inside.then_some(center)
        })
    }

    /// Who has gone into the other side's main base, and the end for anyone still there when time is up.
    /// Runs before the sim's events are read, so that a death here costs what any other death does.
    fn step_bases(&mut self, sim: &mut Sim) {
        let Some(warning) = self.mode.cfg.base_protection.as_ref().map(|rules| seconds_to_ticks(rules.warning_seconds)) else {
            return;
        };
        for index in 0..self.state.players.len() {
            let (id, team) = (self.state.players[index].soldier, self.state.players[index].team);
            let soldier = &sim.soldiers[id];
            let on_field = soldier.present && soldier.health.is_up();
            let inside = on_field && self.enemy_base_at(sim, soldier.body.pos, team, 0.0).is_some();
            let player = &mut self.state.players[index];
            if inside != player.trespass_since.is_some() {
                player.trespass_since = inside.then_some(sim.tick);
            }
            if player.trespass_since.is_some_and(|since| sim.tick >= since + warning) {
                player.trespass_since = None;
                // Nobody's kill. The hit is just hard enough, since a body part's multiplier is rules too.
                sim.soldiers[id].attacker = None;
                let raw = sim.health_cfg.head_kill_damage / sim.health_cfg.zone(Zone::Head).damage.max(1e-3);
                sim.hurt(id, Zone::Head, raw * 1.01);
            }
        }
    }

    /// Tickets and scores from what the sim says just happened to whom.
    fn read_sim_events(&mut self, sim: &Sim) {
        for event in &sim.events {
            let SimEvent::Health { soldier, event } = *event else {
                continue;
            };
            match event {
                HealthEvent::Downed => self.charge(sim, soldier),
                HealthEvent::Died => {
                    self.charge(sim, soldier);
                    let back_at = sim.tick + seconds_to_ticks(self.mode.cfg.respawn_seconds);
                    if let Some(player) = self.state.player_mut(soldier) {
                        player.score.deaths += 1;
                        player.back_at = back_at;
                        player.ready = player.bot;
                    }
                }
                HealthEvent::Revived => self.refund(sim, soldier),
                _ => {}
            }
        }
    }

    /// This life has cost the team, once, however it goes on to end.
    fn charge(&mut self, sim: &Sim, soldier: SoldierId) {
        let Some(victim) = self.state.player_mut(soldier) else {
            return;
        };
        if victim.charged {
            return;
        }
        victim.charged = true;
        let team = victim.team as usize;
        let rules = self.mode.cfg.tickets.clone();
        let cost = rules.as_ref().map_or(0, |t| t.per_death);
        self.state.teams[team].tickets -= cost;
        let Some(killer) = sim.soldiers[soldier].attacker.filter(|&a| a != soldier) else {
            return;
        };
        let friendly = self.state.player(killer).is_some_and(|k| k.team as usize == team);
        if friendly {
            self.state.teams[team].tickets -= rules.as_ref().map_or(0, |t| t.teamkill_penalty);
            self.events.push(ModeEvent::TeamKill { killer, victim: soldier });
        }
        if let Some(killer) = self.state.player_mut(killer) {
            if friendly {
                killer.score.team_kills += 1;
            } else {
                killer.score.kills += 1;
            }
        }
    }

    /// Got up again before it came to dying, so the team has its ticket back and the medic the credit.
    fn refund(&mut self, sim: &Sim, soldier: SoldierId) {
        let Some(player) = self.state.player_mut(soldier) else {
            return;
        };
        if !player.charged {
            return;
        }
        player.charged = false;
        let team = player.team as usize;
        self.state.teams[team].tickets += self.mode.cfg.tickets.as_ref().map_or(0, |t| t.revive_refund);
        let medic = sim.events.iter().find_map(|event| match *event {
            SimEvent::Carry { soldier: medic, event: CarryEvent::TreatmentFinished { care: Care::Medkit, patient } } if patient == soldier => {
                Some(medic)
            }
            _ => None,
        });
        if let Some(medic) = medic.and_then(|m| self.state.player_mut(m)) {
            medic.score.revives += 1;
        }
    }

    /// Clocks for everyone off the field, and bringing in whoever's time has come.
    fn step_players(&mut self, sim: &mut Sim) {
        let now = sim.tick;
        for index in 0..self.state.players.len() {
            let player = &mut self.state.players[index];
            let soldier = player.soldier;
            let health = &sim.soldiers[soldier].health;
            let down = !health.is_up() && !health.is_dead();
            if down != player.down_since.is_some() {
                player.down_since = down.then_some(now);
            }
            if player.deployed && !health.is_dead() {
                continue;
            }
            if self.mode.cfg.deployment == Deployment::Automatic {
                if now >= player.back_at {
                    sim.respawn(soldier);
                    self.after_coming_back(soldier);
                }
                continue;
            }
            if player.bot {
                let rally = self.state.squad_of(soldier).is_some_and(|s| s.rally.is_some());
                let player = &mut self.state.players[index];
                player.ready = true;
                player.spawn = if rally { SpawnAt::Rally } else { SpawnAt::Base };
            }
            self.forget_lost_rally(index);
            let player = &self.state.players[index];
            if !player.ready {
                continue;
            }
            match player.spawn {
                SpawnAt::Base if now >= player.back_at => self.deploy_at_base(sim, soldier),
                SpawnAt::Rally => {
                    let extra = self.mode.cfg.rally.as_ref().map_or(0.0, |r| r.extra_wait);
                    if now >= player.back_at + seconds_to_ticks(extra) {
                        self.deploy_at_rally(sim, soldier);
                    }
                }
                _ => {}
            }
        }
    }

    fn deploy_at_base(&mut self, sim: &mut Sim, soldier: SoldierId) {
        if !self.can_take_kit(soldier) {
            return;
        }
        let team = self.state.player(soldier).expect("in the list").team;
        let state = &mut self.state.teams[team as usize];
        let (pos, yaw) = base_spot(sim, team, state.next_spawn);
        state.next_spawn = state.next_spawn.wrapping_add(1);
        self.bring_in(sim, soldier, pos, yaw);
    }

    fn deploy_at_rally(&mut self, sim: &mut Sim, soldier: SoldierId) {
        if !self.can_take_kit(soldier) {
            return;
        }
        let Some(rally) = self.state.squad_of(soldier).and_then(|s| s.rally) else {
            return;
        };
        let pos = self.rally_spot(sim, &rally);
        self.bring_in(sim, soldier, pos, rally.yaw);
        self.use_rally(soldier);
    }

    /// A rally point that has gone is no longer a choice, and whoever was waiting for it chooses again.
    fn forget_lost_rally(&mut self, index: usize) {
        let soldier = self.state.players[index].soldier;
        let has_rally = self.state.squad_of(soldier).is_some_and(|s| s.rally.is_some());
        let player = &mut self.state.players[index];
        if player.spawn == SpawnAt::Rally && !has_rally {
            player.spawn = SpawnAt::Base;
            if player.ready {
                player.ready = false;
                self.refuse(soldier, Refusal::NoRally);
            }
        }
    }

    /// Someone else in the squad may have taken the kit while they waited. They choose again.
    fn can_take_kit(&mut self, soldier: SoldierId) -> bool {
        let kit = self.state.player(soldier).expect("in the list").kit;
        if self.kit_allowed(soldier, kit) == Ok(true) || self.mode.cfg.kits.is_empty() {
            return true;
        }
        let other = self.kit_for_bot(soldier);
        let player = self.state.player_mut(soldier).expect("in the list");
        if player.bot {
            player.kit = other;
            return true;
        }
        player.ready = false;
        self.refuse(soldier, Refusal::KitTaken);
        false
    }

    fn bring_in(&mut self, sim: &mut Sim, soldier: SoldierId, pos: Vec3, yaw: f32) {
        let loadout = self.chosen_loadout(sim, soldier);
        sim.respawn_at(soldier, pos, yaw, loadout);
        self.after_coming_back(soldier);
    }

    fn after_coming_back(&mut self, soldier: SoldierId) {
        let player = self.state.player_mut(soldier).expect("in the list");
        player.deployed = true;
        player.ready = false;
        player.charged = false;
        player.down_since = None;
        player.trespass_since = None;
        self.events.push(ModeEvent::Deployed { soldier });
    }

    /// Whoever is in a zone as it falls gets the credit.
    fn credit_captures(&mut self, sim: &Sim) {
        for i in 0..self.events.len() {
            let ModeEvent::PointCaptured { point, team } = self.events[i] else {
                continue;
            };
            let zone = &sim.level.capture_points[point];
            for player in self.state.players.iter_mut().filter(|p| p.team == team) {
                let soldier = &sim.soldiers[player.soldier];
                let near = (soldier.body.pos - zone.center).length() <= zone.radius * 2.0;
                if soldier.present && soldier.health.is_up() && near {
                    player.score.captures += 1;
                }
            }
        }
    }

    fn refuse(&mut self, soldier: SoldierId, why: Refusal) {
        self.events.push(ModeEvent::Refused { soldier, why });
    }

    fn check_for_winner(&mut self, now: u32) {
        let [a, b] = [self.state.teams[0].tickets, self.state.teams[1].tickets];
        let out_of_tickets = self.mode.cfg.tickets.is_some() && (a <= 0 || b <= 0);
        let time_up = self.mode.cfg.time_limit.is_some_and(|limit| now >= self.state.started_at + seconds_to_ticks(limit));
        if !out_of_tickets && !time_up {
            return;
        }
        let winner = match a.cmp(&b) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        };
        self.state.phase = Phase::Ended { winner, restart_at: now + seconds_to_ticks(self.mode.cfg.restart_seconds) };
        self.events.push(ModeEvent::RoundEnded { winner });
    }
}

pub(crate) fn seconds_to_ticks(seconds: f32) -> u32 {
    (seconds / TICK_DT).round().max(0.0) as u32
}

/// One of a team's spawns, or wherever the level starts people if it does not say.
fn base_spot(sim: &Sim, team: u8, index: usize) -> (Vec3, f32) {
    let spawns = sim.level.teams.get(team as usize).map(|t| &t.spawns[..]).unwrap_or(&[]);
    match spawns.get(index % spawns.len().max(1)) {
        Some(spawn) => (spawn.pos, spawn.yaw),
        None => (sim.level.spawn, sim.level.spawn_yaw),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod roof_tests;
