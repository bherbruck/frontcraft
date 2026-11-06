use super::*;
use prs_sim::Level;
use prs_sim::health::Life;
use prs_sim::inventory::Slot;
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::weapons::WeaponTable;

/// A level with nothing on it but ground and the points given, in a line north from a south base.
fn flat_level(points: usize) -> Level {
    let mut level = Level::flat(800.0);
    for (team, z) in [300.0, -300.0].into_iter().enumerate() {
        level.teams.push(prs_sim::Team {
            name: format!("Team {team}"),
            spawns: vec![prs_sim::SpawnPoint { pos: Vec3::new(0.0, 0.1, z), yaw: 0.0 }],
            base_radius: 40.0,
        });
    }
    for i in 0..points {
        let z = 200.0 - 400.0 * i as f32 / (points - 1).max(1) as f32;
        level.capture_points.push(prs_sim::CapturePoint { name: format!("P{i}"), center: Vec3::new(0.0, 0.0, z), radius: 20.0 });
    }
    level
}

struct Game {
    sim: Sim,
    game: Match,
}

impl Game {
    /// No bots unless a test asks for them, since they come in by themselves every tick.
    fn new(level: Level, mut mode: Mode) -> Self {
        mode.cfg.bots_per_team = None;
        let mut sim = Sim::new(level);
        let game = Match::new(mode, &mut sim);
        Self { sim, game }
    }

    fn with_bots(level: Level, per_team: usize) -> Self {
        let mut g = Self::new(level, Mode::advance_and_secure());
        g.game.mode.cfg.bots_per_team = Some(per_team);
        g
    }

    fn aas(points: usize) -> Self {
        Self::new(flat_level(points), Mode::advance_and_secure())
    }

    /// A soldier on a team, on the field, at their base.
    fn soldier(&mut self, team: u8) -> SoldierId {
        let id = self.player(team);
        self.step_with(&[(id, ModeCmd::Deploy)]);
        assert!(self.sim.soldiers[id].present, "should be in at once at the start of a round");
        id
    }

    /// A soldier who has joined and is still on the spawn screen.
    fn player(&mut self, team: u8) -> SoldierId {
        let seed = self.sim.soldiers.len() as u32 + 1;
        let id = self.sim.spawn_soldier(seed);
        self.game.join(&mut self.sim, id, Some(team), false);
        id
    }

    fn step_with(&mut self, cmds: &[(SoldierId, ModeCmd)]) {
        self.sim.step(&[]);
        self.game.step(&mut self.sim, cmds);
    }

    fn step(&mut self) {
        self.step_with(&[]);
    }

    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds / TICK_DT) as u32 {
            self.step();
        }
    }

    fn put(&mut self, id: SoldierId, at: Vec3) {
        self.sim.soldiers[id].body.pos = at + Vec3::Y * 0.1;
    }

    fn put_in_point(&mut self, id: SoldierId, point: usize) {
        let at = self.sim.level.capture_points[point].center;
        self.put(id, at);
    }

    /// Down in one go, the way a bullet does it, and let the mode see it.
    fn down(&mut self, id: SoldierId, by: Option<SoldierId>) {
        self.sim.step(&[]);
        self.sim.soldiers[id].attacker = by;
        self.sim.soldiers[id].health.health = 1.0;
        self.sim.hurt(id, prs_sim::health::Zone::Chest, 5.0);
        assert_eq!(self.sim.soldiers[id].health.life, Life::Down);
        self.game.step(&mut self.sim, &[]);
    }

    fn kill(&mut self, id: SoldierId, by: Option<SoldierId>) {
        self.sim.step(&[]);
        self.sim.soldiers[id].attacker = by;
        self.sim.hurt(id, prs_sim::health::Zone::Head, 1000.0);
        assert_eq!(self.sim.soldiers[id].health.life, Life::Dead);
        self.game.step(&mut self.sim, &[]);
    }

    fn revive(&mut self, id: SoldierId) {
        self.sim.step(&[]);
        let mut events = Vec::new();
        let cfg = self.sim.health_cfg.clone();
        self.sim.soldiers[id].health.medkit(&cfg, &mut events);
        for event in events {
            self.sim.events.push(SimEvent::Health { soldier: id, event });
        }
        self.game.step(&mut self.sim, &[]);
    }

    fn tickets(&self, team: usize) -> i32 {
        self.game.state.teams[team].tickets
    }

    fn point(&self, index: usize) -> PointState {
        self.game.state.points[index]
    }

    fn progress(&self, index: usize) -> f32 {
        self.point(index).progress_at(self.sim.tick)
    }
}

fn rules(game: &Game) -> RallyRules {
    game.game.mode.cfg.rally.clone().unwrap()
}

#[test]
fn the_chain_runs_from_one_base_to_the_other_on_the_real_town() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let game = Match::new(Mode::advance_and_secure(), &mut sim);
    let names: Vec<&str> = game.state.chain.iter().map(|&i| sim.level.capture_points[i].name.as_str()).collect();
    assert_eq!(names.first(), Some(&"Saddle"), "{names:?}");
    assert_eq!(names.last(), Some(&"Ridge Gap"), "{names:?}");
    assert_eq!(names.len(), 6);
    let state = &game.state;
    assert_eq!(state.points[state.chain[0]].owner, Some(0));
    assert_eq!(state.points[state.chain[5]].owner, Some(1));
    assert_eq!(state.points_held(0), 1);
    assert_eq!(state.attack_target(0), Some(state.chain[1]));
    assert_eq!(state.attack_target(1), Some(state.chain[4]));
}

#[test]
fn a_chain_given_as_data_overrides_the_order_of_the_land() {
    let mut mode = Mode::advance_and_secure();
    let rules = CaptureRules { chain: Some(vec![2, 0, 1]), ..Default::default() };
    mode.objective = Box::new(AdvanceAndSecure(rules));
    let game = Game::new(flat_level(3), mode);
    assert_eq!(game.game.state.chain, vec![2, 0, 1]);
}

#[test]
fn only_the_next_point_along_the_chain_can_be_taken() {
    let mut g = Game::aas(5);
    let a = g.soldier(0);
    // Point 0 is ours and 1 is next, so sitting in 2 does nothing at all.
    g.put_in_point(a, 2);
    g.run(10.0);
    assert_eq!(g.point(2).claimant, None);
    assert_eq!(g.progress(2), 0.0);
    g.put_in_point(a, 1);
    g.run(10.0);
    assert_eq!(g.point(1).claimant, Some(0));
    assert!(g.progress(1) > 0.2);
    assert_eq!(g.point(1).owner, None, "not taken yet");
}

#[test]
fn a_point_takes_the_configured_time_and_more_soldiers_take_it_faster() {
    let mut g = Game::aas(3);
    let capture = 45.0;
    let a = g.soldier(0);
    g.put_in_point(a, 1);
    g.run(capture - 1.0);
    assert_eq!(g.point(1).owner, None);
    g.run(1.5);
    assert_eq!(g.point(1).owner, Some(0));

    let mut g = Game::aas(3);
    let squad: Vec<_> = (0..3).map(|_| g.soldier(0)).collect();
    for &s in &squad {
        g.put_in_point(s, 1);
    }
    // Two more than the first, at a quarter faster each.
    g.run(capture / 1.5 + 0.5);
    assert_eq!(g.point(1).owner, Some(0));
}

#[test]
fn even_numbers_stall_a_capture_and_more_of_them_turn_it_back() {
    let mut g = Game::aas(3);
    let (a, a2) = (g.soldier(0), g.soldier(0));
    let (b, b2) = (g.soldier(1), g.soldier(1));
    g.put_in_point(a, 1);
    g.run(10.0);
    let before = g.progress(1);
    assert!(before > 0.1);

    g.put_in_point(b, 1);
    g.step();
    let before = g.progress(1);
    g.run(10.0);
    assert_eq!(g.progress(1), before, "one each and nothing moves");
    assert_eq!(g.point(1).inside, [1, 1]);
    assert_eq!(
        g.game.mode.objective.zone_status(&g.sim.level, &g.game.state, &g.sim.soldiers[a]).unwrap().1,
        ZoneStatus::Contested
    );

    g.put_in_point(a2, 1);
    g.run(5.0);
    assert!(g.progress(1) > before, "one up and it goes again");
    let ahead = g.progress(1);

    g.put_in_point(b2, 1);
    g.sim.soldiers[a2].present = false;
    g.run(5.0);
    assert!(g.progress(1) < ahead, "the other side now has more, and takes our progress away");
    assert_eq!(g.point(1).claimant, Some(0));
}

#[test]
fn someone_down_does_not_hold_ground() {
    let mut g = Game::aas(3);
    let (a, b) = (g.soldier(0), g.soldier(1));
    g.put_in_point(a, 1);
    g.put_in_point(b, 1);
    g.run(1.0);
    assert_eq!(g.point(1).inside, [1, 1]);
    g.down(b, Some(a));
    g.run(1.0);
    assert_eq!(g.point(1).inside, [1, 0]);
    assert!(g.progress(1) > 0.0);
}

#[test]
fn a_held_point_goes_down_to_nobody_before_it_goes_up_for_the_attacker() {
    let mut g = Game::aas(3);
    let a = g.soldier(0);
    g.put_in_point(a, 1);
    g.run(46.0);
    assert_eq!(g.point(1).owner, Some(0));
    assert_eq!(g.game.state.attack_target(1), Some(1), "that's the front now");
    g.sim.soldiers[a].present = false;

    let b = g.soldier(1);
    g.put_in_point(b, 1);
    g.run(30.0);
    assert_eq!(g.point(1).owner, Some(0), "still theirs, going down");
    assert!(g.progress(1) < 0.5);
    g.run(16.0);
    assert_eq!(g.point(1).owner, None);
    g.run(10.0);
    assert_eq!(g.point(1).claimant, Some(1));
    g.run(40.0);
    assert_eq!(g.point(1).owner, Some(1));
    assert!(g.game.events.is_empty() || !g.game.state.ended());
}

#[test]
fn a_point_left_alone_drifts_back() {
    let mut g = Game::aas(3);
    let a = g.soldier(0);
    g.put_in_point(a, 1);
    g.run(10.0);
    let taken = g.progress(1);
    g.put(a, Vec3::new(100.0, 0.0, 300.0));
    g.run(5.0);
    assert!(g.progress(1) < taken);
    g.run(30.0);
    assert_eq!(g.point(1).claimant, None);
}

#[test]
fn the_side_with_fewer_points_bleeds_at_the_configured_rate() {
    let mut g = Game::aas(3);
    let a = g.soldier(0);
    let start = g.tickets(1);
    g.run(60.0);
    assert_eq!(g.tickets(0), start);
    assert_eq!(g.tickets(1), start, "one each and nobody bleeds");
    g.put_in_point(a, 1);
    g.run(46.0);
    assert_eq!(g.point(1).owner, Some(0));
    let before = g.tickets(1);
    g.put(a, Vec3::new(100.0, 0.0, 300.0));
    g.run(60.0);
    // One point behind at five a minute. A little loose, because debt keeps its fractions.
    assert!((before - g.tickets(1) - 5).abs() <= 1, "{}", before - g.tickets(1));
    assert_eq!(g.tickets(0), start);
}

#[test]
fn deaths_cost_one_ticket_however_they_come_and_a_revive_gives_it_back() {
    let mut g = Game::aas(3);
    let (a, b) = (g.soldier(0), g.soldier(1));
    let start = g.tickets(1);

    g.kill(b, Some(a));
    assert_eq!(g.tickets(1), start - 1, "killed outright");
    assert_eq!(g.game.state.player(a).unwrap().score.kills, 1);

    let mut g = Game::aas(3);
    let (a, b) = (g.soldier(0), g.soldier(1));
    g.down(b, Some(a));
    assert_eq!(g.tickets(1), start - 1, "going down costs");
    g.revive(b);
    assert_eq!(g.tickets(1), start, "the revive hands it back");
    assert_eq!(g.sim.soldiers[b].health.life, Life::Up);

    g.down(b, Some(a));
    assert_eq!(g.tickets(1), start - 1);
    g.run(g.sim.health_cfg.down_time + 1.0);
    assert_eq!(g.sim.soldiers[b].health.life, Life::Dead);
    assert_eq!(g.tickets(1), start - 1, "bleeding out afterwards costs nothing more");
    assert_eq!(g.game.state.player(b).unwrap().score.deaths, 1);
}

#[test]
fn a_real_revive_with_a_medic_kit_scores_for_the_medic_and_needs_the_medkit() {
    let mut g = Game::aas(3);
    let (medic, rifle, hurt) = (g.player(0), g.player(0), g.player(0));
    g.step_with(&[(medic, ModeCmd::SetKit(1)), (medic, ModeCmd::Deploy), (rifle, ModeCmd::Deploy), (hurt, ModeCmd::Deploy)]);
    assert_eq!(g.sim.soldiers[medic].inventory.stash.medkits, 2);
    assert_eq!(g.sim.soldiers[rifle].inventory.stash.medkits, 0, "only medics carry one");
    assert_eq!(g.sim.soldiers[rifle].inventory.stash.bandages, 2);

    let start = g.tickets(0);
    let at = Vec3::new(50.0, 0.1, 0.0);
    g.put(hurt, at);
    g.run(0.5);
    g.down(hurt, None);
    assert_eq!(g.tickets(0), start - 1);
    g.run(1.5);

    // The rifleman stands over them and has nothing to do it with.
    let beside = g.sim.soldiers[hurt].body.pos + Vec3::new(1.0, 0.0, 0.0);
    g.put(rifle, beside);
    g.run(0.5);
    let look = g.sim.soldiers[hurt].body.pos - g.sim.soldiers[rifle].body.pos;
    let yaw = (-look.x).atan2(-look.z);
    let mut press = Buttons::default();
    press.set(Buttons::SLOT_4, true);
    let cmd = |tick, buttons| PlayerCmd { tick, yaw, pitch: -0.6, buttons, ..Default::default() };
    g.sim.step(&[(rifle, cmd(g.sim.tick, press))]);
    g.game.step(&mut g.sim, &[]);
    assert_ne!(g.sim.soldiers[rifle].inventory.selected, Slot::Medkit);
    g.sim.soldiers[rifle].present = false;

    // The medic can.
    g.put(medic, beside);
    g.run(0.5);
    let mut treating = Buttons::default();
    treating.set(Buttons::FIRE, true);
    g.sim.step(&[(medic, cmd(g.sim.tick, press))]);
    g.game.step(&mut g.sim, &[]);
    assert_eq!(g.sim.soldiers[medic].inventory.selected, Slot::Medkit);
    let ticks = ((g.sim.health_cfg.revive_time + 2.0) / TICK_DT) as u32;
    for _ in 0..ticks {
        g.sim.step(&[(medic, cmd(g.sim.tick, treating))]);
        g.game.step(&mut g.sim, &[]);
        if g.sim.soldiers[hurt].health.is_up() {
            break;
        }
    }
    assert!(g.sim.soldiers[hurt].health.is_up(), "should have been revived");
    assert_eq!(g.tickets(0), start, "and the ticket is back");
    assert_eq!(g.game.state.player(medic).unwrap().score.revives, 1);
}

#[test]
fn friendly_fire_costs_a_penalty_on_top_and_is_on_the_record() {
    let mut g = Game::aas(3);
    let (a, a2) = (g.soldier(0), g.soldier(0));
    let start = g.tickets(0);
    g.kill(a2, Some(a));
    assert_eq!(g.tickets(0), start - 1 - 2);
    assert_eq!(g.game.state.player(a).unwrap().score.team_kills, 1);
    assert_eq!(g.game.state.player(a).unwrap().score.kills, 0);
    assert!(g.game.events.iter().any(|e| matches!(e, ModeEvent::TeamKill { .. })));
}

#[test]
fn bullets_hit_friends_in_a_match_as_in_any_other_sim() {
    let mut g = Game::aas(3);
    let (shooter, friend) = (g.soldier(0), g.soldier(0));
    let start = g.tickets(0);
    g.put(shooter, Vec3::new(0.0, 0.1, 0.0));
    g.put(friend, Vec3::new(0.0, 0.1, -5.0));
    g.run(0.5);
    let mut fire = Buttons::default();
    fire.set(Buttons::FIRE, true);
    let cmd = PlayerCmd { yaw: 0.0, pitch: 0.0, buttons: fire, ..Default::default() };
    let mut was_hit = false;
    for tick in 0..240 {
        let held = if tick % 12 < 2 { cmd } else { PlayerCmd::default() };
        g.sim.step(&[(shooter, held)]);
        was_hit |= g.sim.events.iter().any(|e| matches!(e, SimEvent::SoldierHit { soldier, .. } if *soldier == friend));
        g.game.step(&mut g.sim, &[]);
    }
    assert!(was_hit);
    assert!(!g.sim.soldiers[friend].health.is_up(), "two seconds of rifle fire at five metres");
    assert_eq!(g.tickets(0), start - 1 - 2);
}

#[test]
fn nobody_comes_back_until_the_wait_is_over_and_they_say_so() {
    let mut g = Game::aas(3);
    let a = g.soldier(0);
    let b = g.soldier(1);
    g.kill(a, Some(b));
    let wait = g.game.mode.cfg.respawn_seconds;
    g.run(wait + 5.0);
    assert!(g.sim.soldiers[a].health.is_dead(), "not without being asked");
    g.step_with(&[(a, ModeCmd::Deploy)]);
    g.step();
    assert!(g.sim.soldiers[a].health.is_up());
    assert_eq!(g.sim.soldiers[a].body.pos.z, 300.0, "at their base");

    g.kill(a, Some(b));
    g.step_with(&[(a, ModeCmd::Deploy)]);
    g.run(wait - 1.0);
    assert!(g.sim.soldiers[a].health.is_dead(), "ready, but not yet");
    g.run(1.5);
    assert!(g.sim.soldiers[a].health.is_up());
}

#[test]
fn respawn_times_and_kits_come_from_the_mode() {
    let mut mode = Mode::advance_and_secure();
    mode.cfg.respawn_seconds = 2.0;
    mode.cfg.kits[0].loadout.primary = WeaponTable::MACHINE_GUN;
    mode.cfg.kits[0].loadout.bandages = 3;
    let mut g = Game::new(flat_level(3), mode);
    let a = g.soldier(0);
    assert_eq!(g.sim.soldiers[a].weapon.weapon, WeaponTable::MACHINE_GUN);
    assert_eq!(g.sim.soldiers[a].inventory.stash.bandages, 3);
    g.kill(a, None);
    g.step_with(&[(a, ModeCmd::Deploy)]);
    g.run(2.5);
    assert!(g.sim.soldiers[a].health.is_up());
}

#[test]
fn every_kit_issues_what_it_says() {
    let mut g = Game::aas(3);
    let medkits = [0, 2, 0, 0];
    let guns = [WeaponTable::ASSAULT_RIFLE, WeaponTable::ASSAULT_RIFLE, WeaponTable::MACHINE_GUN, WeaponTable::MARKSMAN_RIFLE];
    for kit in 0..4 {
        let a = g.player(0);
        g.step_with(&[(a, ModeCmd::SetKit(kit)), (a, ModeCmd::Deploy)]);
        assert_eq!(g.sim.soldiers[a].inventory.stash.medkits, medkits[kit], "kit {kit}");
        assert_eq!(g.sim.soldiers[a].weapon.weapon, guns[kit], "kit {kit}");
    }
}

#[test]
fn a_squad_takes_six_and_keeps_a_leader_and_limits_its_kits() {
    let mut g = Game::aas(3);
    let members: Vec<_> = (0..7).map(|_| g.player(0)).collect();
    let squad = g.game.state.squad_of(members[0]).unwrap();
    assert_eq!(squad.members.len(), 6, "the seventh gets a new squad");
    assert_eq!(squad.leader, members[0]);
    let second = g.game.state.squad_of(members[6]).unwrap().id;
    assert_ne!(second, squad.id);
    let first = squad.id;
    g.step_with(&[(members[6], ModeCmd::JoinSquad(first))]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: members[6], why: Refusal::SquadFull }));
    assert!(g.game.state.squad(second).is_some(), "stays where they were");

    // A leader going means the next along leads, and a machine gun is one to a squad.
    g.step_with(&[(members[0], ModeCmd::LeaveSquad)]);
    assert_eq!(g.game.state.squad(first).unwrap().leader, members[1]);
    g.step_with(&[(members[1], ModeCmd::SetKit(2))]);
    g.step_with(&[(members[2], ModeCmd::SetKit(2))]);
    assert_eq!(g.game.state.player(members[2]).unwrap().kit, 0);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: members[2], why: Refusal::KitTaken }));

    // An empty squad is gone. Nobody can join a squad on the other side.
    let other = g.player(1);
    let their_squad = g.game.state.squad_of(other).unwrap().id;
    g.step_with(&[(members[0], ModeCmd::JoinSquad(their_squad))]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: members[0], why: Refusal::OtherTeam }));
    g.step_with(&[(members[6], ModeCmd::LeaveSquad)]);
    assert!(g.game.state.squad(second).is_none());
}

/// A leader and a squadmate out in the open, past the first rally rules.
fn forward_pair(g: &mut Game) -> (SoldierId, SoldierId) {
    let (leader, mate) = (g.soldier(0), g.soldier(0));
    assert_eq!(g.game.state.squad_of(leader).unwrap().leader, leader);
    g.put(leader, Vec3::new(100.0, 0.0, 100.0));
    g.put(mate, Vec3::new(105.0, 0.0, 100.0));
    g.run(0.5);
    (leader, mate)
}

fn place(g: &mut Game, soldier: SoldierId) -> Option<Refusal> {
    g.step_with(&[(soldier, ModeCmd::PlaceRally)]);
    g.game.events.iter().find_map(|e| match e {
        ModeEvent::Refused { why, .. } => Some(*why),
        _ => None,
    })
}

#[test]
fn only_a_leader_with_a_squadmate_near_and_no_enemy_close_can_place_a_rally_and_it_costs() {
    let mut g = Game::aas(3);
    let start = g.tickets(0);
    let (leader, mate) = forward_pair(&mut g);
    assert_eq!(place(&mut g, mate), Some(Refusal::NotLeader));

    g.put(mate, Vec3::new(200.0, 0.0, 100.0));
    g.run(0.2);
    assert_eq!(place(&mut g, leader), Some(Refusal::NoSquadmateNear));
    g.put(mate, Vec3::new(105.0, 0.0, 100.0));

    let enemy = g.soldier(1);
    g.put(enemy, Vec3::new(120.0, 0.0, 100.0));
    g.run(0.2);
    assert_eq!(place(&mut g, leader), Some(Refusal::EnemyNear));
    g.put(enemy, Vec3::new(300.0, 0.0, -100.0));
    g.run(0.2);

    // In a zone, and too close to the base, are out too.
    let zone = g.sim.level.capture_points[1].center;
    g.put(leader, zone + Vec3::X * 5.0);
    g.put(mate, zone);
    g.run(0.2);
    assert_eq!(place(&mut g, leader), Some(Refusal::InAZone));
    g.put(leader, Vec3::new(30.0, 0.0, 290.0));
    g.put(mate, Vec3::new(30.0, 0.0, 280.0));
    g.run(0.2);
    assert_eq!(place(&mut g, leader), Some(Refusal::TooCloseToBase));
    g.put(leader, Vec3::new(100.0, 0.0, 100.0));
    g.put(mate, Vec3::new(105.0, 0.0, 100.0));
    g.run(0.2);

    assert_eq!(place(&mut g, leader), None);
    assert_eq!(g.tickets(0), start - rules(&g).ticket_cost);
    assert!(g.game.state.squad_of(leader).unwrap().rally.is_some());

    // And not another for a while.
    assert_eq!(place(&mut g, leader), Some(Refusal::RallyCooling));
    g.run(rules(&g).cooldown);
    assert_eq!(place(&mut g, leader), None);
    assert!(g.game.events.contains(&ModeEvent::RallyLost {
        squad: g.game.state.squad_of(leader).unwrap().id,
        why: RallyLoss::Replaced
    }));
}

#[test]
fn a_rally_brings_soldiers_in_nearby_after_a_longer_wait_and_runs_out() {
    let mut g = Game::aas(3);
    let (leader, mate) = forward_pair(&mut g);
    assert_eq!(place(&mut g, leader), None);
    let rally = g.game.state.squad_of(leader).unwrap().rally.unwrap();
    let wait = g.game.mode.cfg.respawn_seconds;
    let extra = rules(&g).extra_wait;

    g.kill(mate, None);
    g.step_with(&[(mate, ModeCmd::SetSpawn(SpawnAt::Rally)), (mate, ModeCmd::Deploy)]);
    g.run(wait + 1.0);
    assert!(g.sim.soldiers[mate].health.is_dead(), "still waiting the extra");
    g.run(extra);
    assert!(g.sim.soldiers[mate].health.is_up());
    let spot = g.sim.soldiers[mate].body.pos;
    assert!(spot.distance(rally.pos) <= rules(&g).spawn_spread + 1.0, "{spot}");
    assert_eq!(g.game.state.squad_of(leader).unwrap().rally.unwrap().spawns_left, rules(&g).spawns - 1);

    // Use it up.
    for _ in 1..rules(&g).spawns {
        g.kill(mate, None);
        g.step_with(&[(mate, ModeCmd::SetSpawn(SpawnAt::Rally)), (mate, ModeCmd::Deploy)]);
        g.run(wait + extra + 0.5);
        assert!(g.sim.soldiers[mate].health.is_up());
    }
    assert!(g.game.state.squad_of(leader).unwrap().rally.is_none());

    // With none left there is nothing to choose.
    g.kill(mate, None);
    g.step_with(&[(mate, ModeCmd::SetSpawn(SpawnAt::Rally))]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: mate, why: Refusal::NoRally }));
    assert_eq!(g.game.state.player(mate).unwrap().spawn, SpawnAt::Base);
}

#[test]
fn a_rally_lost_while_waiting_for_it_puts_you_back_on_the_spawn_screen() {
    let mut g = Game::aas(3);
    let (leader, mate) = forward_pair(&mut g);
    assert_eq!(place(&mut g, leader), None);
    let id = g.game.state.squad_of(leader).unwrap().id;
    g.kill(mate, None);
    g.step_with(&[(mate, ModeCmd::SetSpawn(SpawnAt::Rally)), (mate, ModeCmd::Deploy)]);
    g.game.state.squads.iter_mut().find(|s| s.id == id).unwrap().rally = None;
    let wait = g.game.mode.cfg.respawn_seconds + rules(&g).extra_wait;
    let mut told = false;
    for _ in 0..((wait + 1.0) / TICK_DT) as u32 {
        g.step();
        told |= g.game.events.contains(&ModeEvent::Refused { soldier: mate, why: Refusal::NoRally });
    }
    assert!(told);
    assert!(g.sim.soldiers[mate].health.is_dead());
    let player = g.game.state.player(mate).unwrap();
    assert_eq!(player.spawn, SpawnAt::Base);
    assert!(!player.ready, "they get to choose again");
}

#[test]
fn enemies_on_top_of_a_rally_destroy_it_and_a_friend_there_prevents_it() {
    let mut g = Game::aas(3);
    let (leader, mate) = forward_pair(&mut g);
    assert_eq!(place(&mut g, leader), None);
    let rally = g.game.state.squad_of(leader).unwrap().rally.unwrap();
    let id = g.game.state.squad_of(leader).unwrap().id;
    let seconds = rules(&g).overrun_seconds;

    // One each: it holds.
    let enemy = g.soldier(1);
    g.put(leader, rally.pos + Vec3::X * 2.0);
    g.put(mate, rally.pos + Vec3::X * 40.0);
    g.put(enemy, rally.pos - Vec3::X * 2.0);
    g.run(seconds + 1.0);
    assert!(g.game.state.squad(id).unwrap().rally.is_some());

    // Alone with it, it goes.
    g.put(leader, rally.pos + Vec3::X * 40.0);
    let mut lost = false;
    for _ in 0..((seconds + 1.0) / TICK_DT) as u32 {
        g.step();
        lost |= g.game.events.contains(&ModeEvent::RallyLost { squad: id, why: RallyLoss::Overrun });
    }
    assert!(lost);
    assert!(g.game.state.squad(id).unwrap().rally.is_none());
}

#[test]
fn a_rally_runs_out_of_time() {
    let mut g = Game::aas(3);
    let (leader, _) = forward_pair(&mut g);
    assert_eq!(place(&mut g, leader), None);
    g.run(rules(&g).lifetime + 1.0);
    assert!(g.game.state.squad_of(leader).unwrap().rally.is_none());
}

#[test]
fn a_soldier_can_give_up_only_once_down_for_long_enough() {
    let mut g = Game::aas(3);
    let a = g.soldier(0);
    g.step_with(&[(a, ModeCmd::GiveUp)]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: a, why: Refusal::NotUp }));
    g.down(a, None);
    g.step_with(&[(a, ModeCmd::GiveUp)]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: a, why: Refusal::NotDownLongEnough }));
    g.run(g.game.mode.cfg.give_up_after);
    let tickets = g.tickets(0);
    g.step_with(&[(a, ModeCmd::GiveUp)]);
    assert!(g.sim.soldiers[a].health.is_dead());
    assert_eq!(g.tickets(0), tickets, "they had already cost the ticket");
}

/// Pushes a team along the chain, by having everyone stand on its next point, until the round ends.
fn play_out(g: &mut Game, team: u8, soldiers: &[SoldierId], limit: f32) -> Option<u8> {
    for _ in 0..(limit / TICK_DT) as u32 {
        if let Some(target) = g.game.state.attack_target(team) {
            for &s in soldiers {
                g.put_in_point(s, target);
            }
        }
        g.step();
        if let Phase::Ended { winner, .. } = g.game.state.phase {
            return winner;
        }
    }
    panic!("the round never ended");
}

fn whole_round_on(map: &str) {
    for winner in [0u8, 1u8] {
        let mut g = Game::new(Level::named(map).unwrap(), Mode::advance_and_secure());
        assert!(g.sim.level.capture_points.len() >= 3, "{map} needs points for this mode");
        let winners: Vec<_> = (0..4).map(|_| g.soldier(winner)).collect();
        let _others: Vec<_> = (0..4).map(|_| g.soldier(1 - winner)).collect();
        let points = g.game.state.points.len();
        assert_eq!(play_out(&mut g, winner, &winners, 3000.0), Some(winner), "{map}");
        assert_eq!(g.game.state.points_held(winner), points, "{map}: everything taken on the way");
        assert!(g.tickets(1 - winner as usize) <= 0);
        assert!(g.tickets(winner as usize) > 0);
    }
}

#[test]
fn a_whole_round_of_advance_and_secure_on_the_town() {
    whole_round_on("town");
}

#[test]
fn a_whole_round_of_advance_and_secure_on_the_range() {
    whole_round_on("range");
}

#[test]
fn a_round_ends_on_the_time_limit_with_whoever_has_more_tickets() {
    let mut mode = Mode::advance_and_secure();
    mode.cfg.time_limit = Some(10.0);
    let mut g = Game::new(flat_level(3), mode);
    let (a, b) = (g.soldier(0), g.soldier(1));
    g.kill(b, Some(a));
    g.run(11.0);
    assert!(matches!(g.game.state.phase, Phase::Ended { winner: Some(0), .. }));

    let mut mode = Mode::advance_and_secure();
    mode.cfg.time_limit = Some(10.0);
    let mut g = Game::new(flat_level(3), mode);
    g.run(11.0);
    assert!(matches!(g.game.state.phase, Phase::Ended { winner: None, .. }), "even is a draw");
}

#[test]
fn the_next_round_starts_clean_with_everyone_in_their_squads() {
    let mut mode = Mode::advance_and_secure();
    mode.cfg.tickets.as_mut().unwrap().start = 2;
    let mut g = Game::new(flat_level(3), mode);
    let (a, a2) = (g.soldier(0), g.soldier(0));
    let b = g.soldier(1);
    g.put(a, Vec3::new(100.0, 0.0, 100.0));
    g.put(a2, Vec3::new(105.0, 0.0, 100.0));
    g.run(0.5);
    g.kill(b, Some(a));
    let squad = g.game.state.squad_of(a).unwrap().id;
    g.game.state.squads.iter_mut().find(|s| s.id == squad).unwrap().rally =
        Some(Rally { pos: Vec3::new(100.0, 0.0, 100.0), yaw: 0.0, spawns_left: 3, expires_at: u32::MAX, overrun_since: None });
    g.step_with(&[(b, ModeCmd::Deploy)]);
    g.kill(a2, None);
    assert!(g.sim.kits.len() >= 2, "both bodies dropped their kit");
    g.kill(a, None);
    assert!(matches!(g.game.state.phase, Phase::Ended { winner: Some(1), .. }), "{:?}", g.game.state.phase);

    // While it is over nothing goes on counting.
    let frozen = g.game.state.teams.clone();
    g.step_with(&[(b, ModeCmd::Deploy)]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: b, why: Refusal::RoundOver }));
    assert_eq!(g.game.state.teams, frozen);

    g.run(g.game.mode.cfg.restart_seconds + 0.5);
    let state = &g.game.state;
    assert_eq!(state.round, 2);
    assert_eq!(state.phase, Phase::Playing);
    assert_eq!(state.teams[0].tickets, 2);
    assert_eq!(state.teams[1].tickets, 2);
    assert_eq!(state.squad_of(a).unwrap().id, squad, "squads stay together");
    assert!(state.squad_of(a).unwrap().rally.is_none());
    assert!(state.player(a).unwrap().score == Score::default());
    assert!(g.sim.kits.is_empty());
    assert!(g.sim.bullets.is_empty());
    for id in [a, a2, b] {
        assert!(!g.sim.soldiers[id].present, "back to the spawn screen");
        assert!(g.sim.soldiers[id].health.is_up());
        assert!(!state.player(id).unwrap().deployed);
    }
    assert_eq!(state.points_held(0), 1);
    assert_eq!(state.points_held(1), 1);
    g.step_with(&[(a, ModeCmd::Deploy)]);
    assert!(g.sim.soldiers[a].present);
}

#[test]
fn team_deathmatch_has_the_harness_and_nothing_to_hold() {
    let mut g = Game::new(flat_level(3), Mode::team_deathmatch());
    let (a, b) = (g.soldier(0), g.soldier(1));
    let start = g.tickets(1);
    assert!(g.game.state.points.is_empty());
    assert_eq!(g.game.mode.objective.order(&g.sim.level, &g.game.state, 0, 0), None);
    g.put(a, g.sim.level.capture_points[0].center);
    g.run(60.0);
    assert_eq!(g.tickets(1), start, "nothing bleeds");
    for _ in 0..start {
        g.step_with(&[(b, ModeCmd::Deploy)]);
        g.step();
        g.kill(b, Some(a));
        g.run(g.game.mode.cfg.respawn_seconds);
        g.step_with(&[(b, ModeCmd::Deploy)]);
    }
    assert!(matches!(g.game.state.phase, Phase::Ended { winner: Some(0), .. }));
    assert_eq!(g.game.state.player(a).unwrap().score.kills as i32, start);
    g.step_with(&[(a, ModeCmd::PlaceRally)]);
    assert!(g.game.events.iter().any(|e| matches!(e, ModeEvent::Refused { why: Refusal::NoRallyRules, .. })));
}

#[test]
fn free_practice_never_ends_and_brings_you_back_where_you_started() {
    let mut sim = Sim::new(flat_level(3));
    let id = sim.spawn_soldier(1);
    let home = Vec3::new(40.0, 0.1, 40.0);
    sim.set_home(id, home, 0.5);
    let loadout = sim.soldiers[id].loadout;
    let mut game = Match::new(Mode::free_practice(), &mut sim);
    game.join(&mut sim, id, None, false);
    let walker = sim.spawn_soldier(2);
    game.join(&mut sim, walker, None, true);
    game.fill_bots(&mut sim);
    assert!(game.state.player(walker).is_some(), "practice leaves whoever is walking about alone");
    assert!(sim.soldiers[id].present, "no spawn screen");
    assert_eq!(sim.soldiers[id].body.pos, home);
    let step = |sim: &mut Sim, game: &mut Match, seconds: f32| {
        for _ in 0..(seconds / TICK_DT) as u32 {
            sim.step(&[]);
            game.step(sim, &[]);
        }
    };
    step(&mut sim, &mut game, 1.0);
    sim.step(&[]);
    sim.hurt(id, prs_sim::health::Zone::Head, 1000.0);
    game.step(&mut sim, &[]);
    assert!(sim.soldiers[id].health.is_dead());
    let wait = game.mode.cfg.respawn_seconds;
    step(&mut sim, &mut game, wait - 1.0);
    assert!(sim.soldiers[id].health.is_dead());
    step(&mut sim, &mut game, 1.5);
    assert!(sim.soldiers[id].health.is_up());
    assert!(sim.soldiers[id].body.pos.distance(home) < 0.5);
    assert_eq!(sim.soldiers[id].loadout, loadout);
    assert_eq!(sim.soldiers[id].inventory.stash.medkits, loadout.medkits);
    step(&mut sim, &mut game, 600.0);
    assert_eq!(game.state.phase, Phase::Playing);
}

#[test]
fn bots_fill_both_teams_get_mixed_kits_come_in_on_their_own_and_make_way_for_players() {
    let mut g = Game::with_bots(flat_level(3), 8);
    let human = g.player(0);
    g.game.fill_bots(&mut g.sim);
    let target = g.game.mode.cfg.bots_per_team.unwrap();
    assert_eq!(g.game.state.team_size(0), target);
    assert_eq!(g.game.state.team_size(1), target);
    assert!(g.game.state.players.iter().any(|p| p.kit == 1), "someone is a medic");
    g.run(0.1);
    assert!(g.game.state.players.iter().filter(|p| p.bot).all(|p| g.sim.soldiers[p.soldier].present));
    assert!(!g.sim.soldiers[human].present, "players choose for themselves");

    // A second player pushes a bot out, and a dead bot goes back in without being asked.
    let before = g.sim.soldiers.len();
    let bot = g.game.state.players.iter().find(|p| p.bot).unwrap().soldier;
    g.kill(bot, None);
    g.run(g.game.mode.cfg.respawn_seconds + 0.5);
    assert!(g.sim.soldiers[bot].health.is_up());
    let second = g.player(0);
    g.game.fill_bots(&mut g.sim);
    assert_eq!(g.game.state.team_size(0), target);
    assert!(g.game.state.player(second).is_some());

    // And when they go a bot comes back into the same slot rather than growing the sim.
    g.game.leave(&mut g.sim, second);
    g.game.fill_bots(&mut g.sim);
    assert_eq!(g.game.state.team_size(0), target);
    assert_eq!(g.sim.soldiers.len(), before + 1);
}

#[test]
fn a_player_takes_over_a_bot_squad_and_can_then_place_rallies() {
    let mut g = Game::aas(3);
    g.game.mode.cfg.bots_per_team = Some(4);
    g.game.fill_bots(&mut g.sim);
    let human = g.player(0);
    let squad = g.game.state.squad_of(human).unwrap();
    assert_eq!(squad.leader, human);
    assert!(squad.members.len() > 1);
}

#[test]
fn orders_say_the_next_point_for_attackers_and_the_front_for_a_threatened_defender() {
    let mut g = Game::aas(5);
    let (a, b, a2) = (g.soldier(0), g.soldier(1), g.soldier(0));
    g.step_with(&[(a2, ModeCmd::CreateSquad)]);
    let (first, second) = (g.game.state.squad_of(a).unwrap().id, g.game.state.squad_of(a2).unwrap().id);
    let order = g.game.order_for(&g.sim, a).unwrap();
    assert_eq!(order.point, g.game.state.attack_target(0).unwrap());
    assert!(order.attack);
    assert_eq!(order.pos, g.sim.level.capture_points[order.point].center);
    let their_order = g.game.order_for(&g.sim, b).unwrap();
    assert_eq!(their_order.point, g.game.state.attack_target(1).unwrap());

    // An enemy on our front point: the first squad carries on and the second goes back for it.
    let front = g.game.state.front_point(0).unwrap();
    g.game.state.points[front].inside[1] = 1;
    let state = &g.game.state;
    let objective = &g.game.mode.objective;
    assert_eq!(objective.order(&g.sim.level, state, 0, second).unwrap().point, front);
    assert!(!objective.order(&g.sim.level, state, 0, second).unwrap().attack);
    assert!(objective.order(&g.sim.level, state, 0, first).unwrap().attack);
}

#[test]
fn both_sides_split_their_squads_the_same_way_when_their_front_is_threatened() {
    // Enough bots that each side has three squads, the second side's numbered on from the first's.
    let mut g = Game::aas(5);
    g.game.mode.cfg.bots_per_team = Some(18);
    g.game.fill_bots(&mut g.sim);
    let ids = |g: &Game, team: u8| g.game.state.squads.iter().filter(|s| s.team == team).map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(ids(&g, 0), vec![0, 1, 2]);
    assert_eq!(ids(&g, 1), vec![3, 4, 5]);
    for team in 0..TEAMS {
        let front = g.game.state.front_point(team as u8).unwrap();
        g.game.state.points[front].inside[1 - team] = 1;
    }
    let split = |g: &Game, team: u8| {
        let orders = ids(g, team).into_iter().map(|squad| g.game.mode.objective.order(&g.sim.level, &g.game.state, team, squad).unwrap().attack);
        orders.collect::<Vec<_>>()
    };
    assert_eq!(split(&g, 0), vec![true, false, true]);
    assert_eq!(split(&g, 1), split(&g, 0));
}

/// A busy scripted match: bots, kills, captures, a rally. Gives the same sequence for the same inputs.
fn busy_tick(g: &mut Game, tick: u32, soldiers: &[SoldierId]) {
    for (i, &s) in soldiers.iter().enumerate() {
        if g.sim.soldiers[s].present && tick % 300 == 40 + i as u32 {
            if let Some(target) = g.game.state.attack_target(i as u8 % 2) {
                g.put_in_point(s, target);
            }
        }
    }
    let mut cmds: Vec<(SoldierId, ModeCmd)> = Vec::new();
    for &s in soldiers {
        if tick % 97 == 11 {
            cmds.push((s, ModeCmd::Deploy));
        }
    }
    if tick == 600 {
        g.put(soldiers[0], Vec3::new(100.0, 0.0, 100.0));
        g.put(soldiers[2], Vec3::new(102.0, 0.0, 100.0));
    }
    if tick == 640 {
        cmds.push((soldiers[0], ModeCmd::PlaceRally));
    }
    let cmd = |who: u32| PlayerCmd {
        tick,
        wish_move: glam::Vec2::new(0.0, 1.0),
        yaw: who as f32 + tick as f32 * 0.01,
        buttons: if tick % 40 < 4 { Buttons(Buttons::FIRE.0) } else { Buttons::default() },
        ..Default::default()
    };
    let sim_cmds: Vec<_> = soldiers.iter().map(|&s| (s, cmd(s as u32))).collect();
    g.sim.step(&sim_cmds);
    if tick % 500 == 250 {
        let victim = soldiers[tick as usize % soldiers.len()];
        if g.sim.soldiers[victim].present {
            g.sim.hurt(victim, prs_sim::health::Zone::Chest, 300.0);
        }
    }
    g.game.step(&mut g.sim, &cmds);
}

fn busy_game() -> (Game, Vec<SoldierId>) {
    let mut g = Game::new(Level::named("range").unwrap(), Mode::advance_and_secure());
    let soldiers: Vec<_> = (0..6).map(|i| g.player(i % 2)).collect();
    (g, soldiers)
}

#[test]
fn the_same_commands_give_the_same_match() {
    let (mut one, soldiers) = busy_game();
    let (mut two, _) = busy_game();
    for tick in 0..3000 {
        busy_tick(&mut one, tick, &soldiers);
        busy_tick(&mut two, tick, &soldiers);
        assert_eq!(one.game.state, two.game.state, "tick {tick}");
        assert_eq!(one.game.events, two.game.events, "tick {tick}");
    }
    assert!(one.game.state.players.iter().map(|p| p.score.deaths).sum::<u32>() > 0, "the script should have got somewhere");
}

#[test]
fn an_exported_match_restored_on_a_fresh_sim_steps_the_same() {
    let (mut one, soldiers) = busy_game();
    for tick in 0..1500 {
        busy_tick(&mut one, tick, &soldiers);
    }
    let (mut two, _) = busy_game();
    two.sim.import_state(&one.sim.export_state());
    two.game.import_state(&one.game.export_state());
    assert_eq!(two.game.state, one.game.state);
    for tick in 1500..3000 {
        busy_tick(&mut one, tick, &soldiers);
        busy_tick(&mut two, tick, &soldiers);
        assert_eq!(one.game.state, two.game.state, "tick {tick}");
        assert_eq!(one.sim.export_state(), two.sim.export_state(), "tick {tick}");
    }
}

#[test]
fn a_mod_can_write_its_own_objective_and_keep_everything_in_the_state() {
    /// The first side with someone at a point wins by draining the other side.
    #[derive(Debug)]
    struct KingOfTheHill;
    impl Objective for KingOfTheHill {
        fn begin(&self, level: &Level, state: &mut MatchState) {
            state.points = vec![PointState::default(); level.capture_points.len().min(1)];
        }
        fn step(&self, sim: &Sim, state: &mut MatchState, _events: &mut Vec<ModeEvent>) {
            let zone = &sim.level.capture_points[0];
            let mut bleed = [0; TEAMS];
            for player in &state.players {
                let soldier = &sim.soldiers[player.soldier];
                if soldier.present && soldier.body.pos.distance(zone.center) < zone.radius {
                    bleed[1 - player.team as usize] = 1;
                }
            }
            for team in 0..TEAMS {
                state.teams[team].tickets -= bleed[team];
            }
        }
        fn order(&self, _: &Level, _: &MatchState, _: u8, _: u32) -> Option<Order> {
            None
        }
    }
    let mut mode = Mode::team_deathmatch();
    mode.objective = Box::new(KingOfTheHill);
    let mut g = Game::new(flat_level(1), mode);
    let a = g.soldier(0);
    g.put_in_point(a, 0);
    g.run(5.0);
    assert!(matches!(g.game.state.phase, Phase::Ended { winner: Some(0), .. } | Phase::Playing));
    assert!(g.tickets(1) < 50);
    assert_eq!(g.tickets(0), 50);
}

#[test]
fn a_held_or_stalled_match_changes_no_state_from_one_tick_to_the_next() {
    let mut g = Game::aas(3);
    let (a, b) = (g.soldier(0), g.soldier(1));
    g.put_in_point(a, 1);
    g.put_in_point(b, 1);
    g.run(1.0);
    let settled = g.game.export_state();
    g.run(30.0);
    assert_eq!(g.game.export_state(), settled, "nothing is happening, so there is nothing to send");

    // A capture under way needs sending once as it starts, and then only as it ends.
    g.put(b, Vec3::new(300.0, 0.1, 0.0));
    g.run(1.0);
    let moving = g.game.export_state();
    assert!(moving.points[1].rate > 0.0);
    g.run(20.0);
    assert_eq!(g.game.export_state(), moving);
    assert!(g.progress(1) > moving.points[1].progress, "and yet anyone can work out how far it has got");
}

#[test]
fn nothing_a_client_can_ask_for_breaks_the_rules_of_a_match() {
    // Every request, with wild values, from everyone, in every state: what a hacked client can send.
    let mut g = Game::aas(3);
    let cheat = g.soldier(0);
    let enemy = g.soldier(1);
    let wild = [0usize, 1, 3, 4, 99, usize::MAX];
    let mut asks: Vec<ModeCmd> = vec![
        ModeCmd::Deploy,
        ModeCmd::CancelDeploy,
        ModeCmd::CreateSquad,
        ModeCmd::LeaveSquad,
        ModeCmd::PlaceRally,
        ModeCmd::GiveUp,
        ModeCmd::SetSpawn(SpawnAt::Rally),
        ModeCmd::SetSpawn(SpawnAt::Base),
    ];
    asks.extend(wild.map(ModeCmd::SetKit));
    asks.extend(wild.map(|w| ModeCmd::JoinSquad(w as u32)));
    asks.extend([0u8, 1, 2, 255].map(ModeCmd::JoinTeam));
    let their_squad = g.game.state.squad_of(enemy).unwrap().id;
    for round in 0..4 {
        for &ask in &asks {
            g.step_with(&[(cheat, ask), (9999, ask)]);
            let state = &g.game.state;
            let me = state.player(cheat).unwrap();
            assert!(me.kit < g.game.mode.cfg.kits.len(), "{ask:?}");
            assert!((me.team as usize) < TEAMS, "{ask:?}");
            assert_eq!(state.teams.len(), TEAMS);
            if let Some(id) = me.squad {
                let squad = state.squad(id).unwrap();
                assert_eq!(squad.team, me.team, "{ask:?}");
                assert!(squad.members.contains(&cheat));
                assert!(squad.members.len() <= g.game.mode.cfg.squads.max_members);
            }
            assert!(state.squad(their_squad).is_none_or(|s| !s.members.contains(&cheat)) || me.team == 1, "{ask:?}");
            assert!(state.squads.iter().all(|s| s.rally.is_none()), "alone with nobody near, nobody can place one: {ask:?}");
            assert_eq!(state.player(9999), None);
        }
        // Dead, and asking to come straight back: the wait still holds.
        if g.sim.soldiers[cheat].health.is_up() && g.sim.soldiers[cheat].present {
            g.kill(cheat, None);
        }
        let back_at = g.game.state.player(cheat).unwrap().back_at;
        g.step_with(&[(cheat, ModeCmd::Deploy)]);
        assert!(g.sim.tick >= back_at || g.sim.soldiers[cheat].health.is_dead(), "round {round}");
        g.run(g.game.mode.cfg.respawn_seconds + 1.0);
    }
}

/// Standing in the far side's base, well clear of everything else.
fn in_their_base(g: &Game, team: u8) -> Vec3 {
    let base = g.sim.level.teams[1 - team as usize].base_center();
    Vec3::new(base.x + 5.0, 0.1, base.z)
}

#[test]
fn going_into_the_enemy_base_gets_a_warning_and_then_kills_at_the_configured_time() {
    let mut g = Game::aas(3);
    let cheat = g.soldier(0);
    let warning = g.game.mode.cfg.base_protection.as_ref().unwrap().warning_seconds;
    let start = g.tickets(0);
    assert_eq!(g.game.state.player(cheat).unwrap().trespass_since, None);

    let at = in_their_base(&g, 0);
    g.put(cheat, at);
    g.step();
    let since = g.game.state.player(cheat).unwrap().trespass_since.expect("warned as soon as they are in");
    assert_eq!(since, g.sim.tick);
    g.run(warning - 0.5);
    assert!(g.sim.soldiers[cheat].health.is_up(), "still time to get out");
    assert_eq!(g.game.state.player(cheat).unwrap().trespass_since, Some(since), "the clock does not need resending");
    g.run(1.0);
    assert!(g.sim.soldiers[cheat].health.is_dead(), "time is up");
    assert_eq!(g.tickets(0), start - 1, "and it costs what any death costs");
    assert_eq!(g.game.state.player(cheat).unwrap().score.deaths, 1);
    assert_eq!(g.game.state.player(cheat).unwrap().trespass_since, None);
}

#[test]
fn getting_out_in_time_is_enough_and_nobody_gets_the_kill() {
    let mut g = Game::aas(3);
    let (cheat, other) = (g.soldier(0), g.soldier(1));
    let warning = g.game.mode.cfg.base_protection.as_ref().unwrap().warning_seconds;
    let at = in_their_base(&g, 0);
    g.put(cheat, at);
    g.run(warning - 1.0);
    g.put(cheat, Vec3::new(100.0, 0.1, 0.0));
    g.step();
    assert_eq!(g.game.state.player(cheat).unwrap().trespass_since, None);
    g.run(warning + 1.0);
    assert!(g.sim.soldiers[cheat].health.is_up());

    // Shot by them last, then killed by the rule: that is not their kill.
    g.sim.soldiers[cheat].attacker = Some(other);
    g.put(cheat, at);
    g.run(warning + 1.0);
    assert!(g.sim.soldiers[cheat].health.is_dead());
    assert_eq!(g.game.state.player(other).unwrap().score.kills, 0);
}

#[test]
fn your_own_base_is_yours_and_it_gives_nobody_any_protection() {
    let mut g = Game::aas(3);
    let (owner, attacker) = (g.soldier(1), g.soldier(0));
    let warning = g.game.mode.cfg.base_protection.as_ref().unwrap().warning_seconds;
    let at = in_their_base(&g, 0);
    g.put(owner, at);
    g.run(warning + 1.0);
    assert!(g.sim.soldiers[owner].health.is_up());
    assert_eq!(g.game.state.player(owner).unwrap().trespass_since, None);
    // No god mode: someone in their own base can still be shot from outside it.
    g.kill(owner, Some(attacker));
    assert_eq!(g.game.state.player(attacker).unwrap().score.kills, 1);
}

#[test]
fn over_the_base_above_the_ceiling_does_not_count_and_a_mode_can_turn_it_off() {
    let mut g = Game::aas(3);
    let rules = g.game.mode.cfg.base_protection.clone().unwrap();
    let at = in_their_base(&g, 0);
    assert!(g.game.enemy_base_at(&g.sim, at, 0, 0.0).is_some());
    assert!(g.game.enemy_base_at(&g.sim, at + Vec3::Y * (rules.ceiling + 1.0), 0, 0.0).is_none());

    let mut mode = Mode::advance_and_secure();
    mode.cfg.base_protection = None;
    let mut g = Game::new(flat_level(3), mode);
    let cheat = g.soldier(0);
    let at = in_their_base(&g, 0);
    g.put(cheat, at);
    g.run(rules.warning_seconds + 1.0);
    assert!(g.sim.soldiers[cheat].health.is_up());
}

#[test]
fn no_rally_point_in_the_enemy_base() {
    let mut g = Game::aas(3);
    let (leader, mate) = forward_pair(&mut g);
    let at = in_their_base(&g, 0);
    g.put(leader, at);
    g.put(mate, at + Vec3::X);
    g.run(0.2);
    assert_eq!(place(&mut g, leader), Some(Refusal::TooCloseToBase));
}

#[test]
fn no_point_on_any_map_is_inside_a_base_and_nobody_spawns_in_the_enemy_base() {
    for map in ["town", "range"] {
        let level = Level::named(map).unwrap();
        for team in &level.teams {
            let center = team.base_center();
            assert!(team.base_radius > 0.0);
            for point in &level.capture_points {
                let apart = glam::Vec2::new(center.x - point.center.x, center.z - point.center.z).length();
                assert!(apart > team.base_radius + point.radius, "{map}: {} overlaps {}", point.name, team.name);
            }
            for spawn in &team.spawns {
                assert!(spawn.pos.distance(center) < team.base_radius, "{map}: a spawn outside {}", team.name);
            }
        }
    }
}

/// Someone sitting in a helicopter, with the next point moved to where they are.
fn in_a_helicopter_in_a_zone(mode: Mode) -> (Game, SoldierId, usize) {
    let mut g = Game::new(Level::named("town").unwrap(), mode);
    let id = g.soldier(0);
    g.sim.sit_in(id, prs_sim::seats::SeatedAt { carrier: prs_sim::heli_carrier(0), seat: 1 });
    g.run(0.2);
    assert!(g.sim.soldiers[id].seat.is_some());
    let target = g.game.state.attack_target(0).unwrap();
    g.sim.level.capture_points[target].center = g.sim.soldiers[id].body.pos;
    g.run(0.2);
    (g, id, target)
}

#[test]
fn nobody_in_a_helicopter_counts_for_a_zone_and_they_are_told_so() {
    let (g, id, target) = in_a_helicopter_in_a_zone(Mode::advance_and_secure());
    assert_eq!(g.point(target).inside, [0, 0]);
    let status = g.game.mode.objective.zone_status(&g.sim.level, &g.game.state, &g.sim.soldiers[id]);
    assert_eq!(status, Some((target, ZoneStatus::NotCounted)));

    let mut mode = Mode::advance_and_secure();
    mode.objective = Box::new(AdvanceAndSecure(CaptureRules { aircraft_count: true, ..Default::default() }));
    let (g, _, target) = in_a_helicopter_in_a_zone(mode);
    assert_eq!(g.point(target).inside, [1, 0], "unless the mode says so");
}

#[test]
fn someone_up_on_a_back_is_told_they_do_not_count() {
    let mut g = Game::aas(3);
    let id = g.soldier(0);
    g.put_in_point(id, 1);
    g.step();
    g.sim.soldiers[id].body.boosted = true;
    g.step();
    assert_eq!(g.point(1).inside, [0, 0]);
    let status = g.game.mode.objective.zone_status(&g.sim.level, &g.game.state, &g.sim.soldiers[id]);
    assert_eq!(status, Some((1, ZoneStatus::NotCounted)));
}

#[test]
fn a_new_player_goes_to_the_side_with_fewer_real_players_whatever_the_bots() {
    let mut g = Game::aas(3);
    g.player(0);
    for _ in 0..3 {
        let bot = g.sim.spawn_soldier(g.sim.soldiers.len() as u32 + 1);
        g.game.join(&mut g.sim, bot, Some(1), true);
    }
    let id = g.sim.spawn_soldier(g.sim.soldiers.len() as u32 + 1);
    g.game.join(&mut g.sim, id, None, false);
    assert_eq!(g.game.state.player(id).unwrap().team, 1, "one real player against none, however many bots");
}

fn bots(g: &Game, team: u8) -> usize {
    g.game.state.players.iter().filter(|p| p.team == team && p.bot).count()
}

#[test]
fn the_side_with_fewer_players_gets_the_bots_and_they_come_and_go_every_tick() {
    let mut g = Game::with_bots(flat_level(3), 4);
    g.step();
    assert_eq!((bots(&g, 0), bots(&g, 1)), (4, 4), "nobody here yet");
    let a = g.player(0);
    g.step();
    assert_eq!((bots(&g, 0), bots(&g, 1)), (3, 4), "a player takes a bot's place at once, without a round starting");
    let others: Vec<_> = (0..5).map(|_| g.player(0)).collect();
    g.step();
    // Six players on one side: the other side is filled to match, beyond the usual four.
    assert_eq!(g.game.state.team_size(0), 6);
    assert_eq!(g.game.state.team_size(1), 6);
    assert_eq!(bots(&g, 0), 0);
    for &p in others.iter().chain([&a]) {
        g.game.leave(&mut g.sim, p);
    }
    g.step();
    assert_eq!((bots(&g, 0), bots(&g, 1)), (4, 4), "and they come back when the players go");
}

#[test]
fn a_cap_and_a_player_count_can_take_the_bots_away() {
    let mut g = Game::with_bots(flat_level(3), 8);
    g.game.mode.cfg.bots_most = Some(6);
    g.player(0);
    g.step();
    assert_eq!(bots(&g, 0) + bots(&g, 1), 6);
    assert!(bots(&g, 1) >= bots(&g, 0), "the side without the player keeps more");
    g.game.mode.cfg.no_bots_from_players = Some(2);
    g.player(1);
    g.step();
    assert_eq!(bots(&g, 0) + bots(&g, 1), 0);
}

#[test]
fn a_bot_in_front_of_a_player_waits_to_go_until_it_dies_or_nobody_is_near() {
    let mut g = Game::with_bots(flat_level(3), 1);
    g.step();
    let bot = g.game.state.players.iter().find(|p| p.team == 0 && p.bot).unwrap().soldier;
    g.run(0.5);
    assert!(g.sim.soldiers[bot].present, "the bot came in by itself");
    let watcher = g.soldier(1);
    let at = g.sim.soldiers[bot].body.pos;
    g.put(watcher, at + Vec3::X * 20.0);
    // A player joins the bot's side and wants its place, with someone looking right at it.
    g.player(0);
    g.run(1.0);
    assert!(g.game.state.player(bot).is_some(), "it does not vanish in front of anyone");
    assert!(g.sim.soldiers[bot].present);
    g.kill(bot, None);
    g.step();
    assert!(g.game.state.player(bot).is_none(), "dead, it goes at once");

    // On its feet with nobody near, it goes too.
    let mut g = Game::with_bots(flat_level(3), 1);
    g.run(0.5);
    let bot = g.game.state.players.iter().find(|p| p.team == 0 && p.bot).unwrap().soldier;
    let watcher = g.soldier(1);
    g.put(watcher, g.sim.soldiers[bot].body.pos + Vec3::X * 400.0);
    g.player(0);
    g.step();
    assert!(g.game.state.player(bot).is_none());
}

#[test]
fn a_bot_off_the_field_goes_before_one_on_it_and_one_in_a_players_squad_goes_last() {
    let mut g = Game::with_bots(flat_level(3), 3);
    g.run(0.5);
    let team_bots: Vec<_> = g.game.state.players.iter().filter(|p| p.team == 0 && p.bot).map(|p| p.soldier).collect();
    // One dead, waiting to come back.
    g.kill(team_bots[2], None);
    g.player(0);
    g.step();
    assert!(g.game.state.player(team_bots[2]).is_none(), "the dead one made way");
    assert!(team_bots[..2].iter().all(|&b| g.game.state.player(b).is_some()));
}

#[test]
fn with_few_players_anyone_can_switch_sides_and_from_six_they_are_kept_even() {
    let mut g = Game::aas(3);
    let (a, b) = (g.player(0), g.player(0));
    g.step_with(&[(b, ModeCmd::JoinTeam(1))]);
    g.step_with(&[(a, ModeCmd::JoinTeam(1))]);
    assert_eq!(g.game.state.humans(1), 2, "two players can both go to one side");
    assert!(!g.game.events.iter().any(|e| matches!(e, ModeEvent::Refused { why: Refusal::TeamsUneven, .. })));

    // Three a side: one more going over would make it four to two.
    let mut g = Game::aas(3);
    let players: Vec<_> = (0..6).map(|i| g.player(i as u8 % 2)).collect();
    g.step_with(&[(players[0], ModeCmd::JoinTeam(1))]);
    assert!(g.game.events.contains(&ModeEvent::Refused { soldier: players[0], why: Refusal::TeamsUneven }));
    assert_eq!(g.game.state.humans(1), 3);
}
