//! A match over a bad network: a real server and two real clients on the simulated network. The server
//! decides everything, clients only ask, and what they are told agrees with what happened.

use prs_mode::{Mode, ModeCmd, ModeEvent, Refusal, SpawnAt};
use prs_net::server::ServerConfig;
use prs_net_sim::mode::encode_cmd;
use prs_net_sim::testkit::{Net, SimWorld, position_of};
use prs_net_sim::{SimClient, SimServer, SimServerConfig, WireEvent};
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::{Level, Sim};
use std::cell::Cell;
use std::rc::Rc;

const MAP: &str = "range";

fn mode() -> Mode {
    let mut mode = Mode::advance_and_secure();
    // Just the two players, so nobody else gets a say.
    mode.cfg.bots_per_team = Some(0);
    mode
}

fn world() -> SimWorld {
    let sim = Sim::new(Level::named(MAP).unwrap());
    let game = SimServer::with_sim(sim, MAP, "aas", mode(), SimServerConfig::default());
    SimWorld::new(game, ServerConfig::default(), position_of)
}

fn join(world: &mut SimWorld, net: &Net, yaw: f32, walking: Rc<Cell<bool>>) -> usize {
    let factory = Box::new(|map: &str, _mode: &str| Some(SimClient::with_sim(Sim::new(Level::named(map)?), mode())));
    let input = Rc::new(move |_who: u32, tick: u32| {
        let wish = if walking.get() { glam::Vec2::Y } else { glam::Vec2::ZERO };
        PlayerCmd { tick, wish_move: wish, yaw, buttons: Buttons::default(), ..Default::default() }
    });
    world.join(factory, input, net)
}

fn ask(world: &mut SimWorld, player: usize, cmd: ModeCmd) {
    world.players[player].session.send_request(encode_cmd(&cmd));
}

fn id_of(world: &SimWorld, player: usize) -> usize {
    world.player_id(player) as usize
}

/// The match as this client has been told it.
fn told(world: &SimWorld, player: usize) -> prs_mode::MatchState {
    world.players[player].session.game().unwrap().match_state.clone().expect("told about the match by now")
}

fn mode_events(world: &SimWorld, player: usize) -> Vec<ModeEvent> {
    world.players[player]
        .server_events
        .iter()
        .filter_map(|event| match event {
            WireEvent::Mode(event) => Some(*event),
            WireEvent::Sim(_) => None,
        })
        .collect()
}

fn progress(world: &SimWorld, point: usize) -> f32 {
    let sim = &world.server.game.sim;
    world.server.game.game.state.points[point].progress_at(sim.tick)
}

/// Two clients on a poor connection: one walks to the next point and takes it, for real, with
/// the other side showing up to stall it for a while. Everyone agrees all the way.
fn a_capture_over(net: Net) {
    let mut world = world();
    let walking = Rc::new(Cell::new(false));
    // Near the first spawn of the near side, looking at the next point along.
    let target = world.server.game.game.state.attack_target(0).unwrap();
    let level = &world.server.game.sim.level;
    let start = level.teams[0].spawns[0].pos;
    let goal = level.capture_points[target].center;
    let yaw = (-(goal.x - start.x)).atan2(-(goal.z - start.z));

    let attacker = join(&mut world, &net, yaw, walking.clone());
    let defender = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    world.run(2.0);
    let (a, d) = (id_of(&world, attacker), id_of(&world, defender));
    let state = &world.server.game.game.state;
    assert_eq!(state.player(a).unwrap().team, 0, "the first to come is on the first side");
    assert_eq!(state.player(d).unwrap().team, 1, "and the next on the other");
    assert!(!world.server.game.sim.soldiers[a].present, "nobody is on the field until they ask");

    // Nothing the client says about itself counts: only what the server does.
    ask(&mut world, attacker, ModeCmd::SetKit(1));
    ask(&mut world, attacker, ModeCmd::Deploy);
    ask(&mut world, defender, ModeCmd::Deploy);
    world.run(2.0);
    assert!(world.server.game.sim.soldiers[a].present);
    assert!(world.server.game.sim.soldiers[d].present);
    assert_eq!(world.server.game.sim.soldiers[a].inventory.stash.medkits, 2, "the kit asked for");
    assert_eq!(told(&world, attacker).player(a).unwrap().kit, 1);

    walking.set(true);
    let sim_limit = 120;
    for _ in 0..sim_limit * 2 {
        world.run(0.5);
        if world.server.game.game.state.points[target].inside[0] > 0 {
            break;
        }
    }
    assert_eq!(world.server.game.game.state.points[target].inside[0], 1, "should have walked into the zone");
    walking.set(false);
    world.run(8.0);
    let so_far = progress(&world, target);
    assert!(so_far > 0.1, "{so_far}");
    let client_says = told(&world, defender).points[target];
    assert_eq!(client_says.claimant, Some(0), "the other side's client can see it being taken");
    assert!(client_says.rate > 0.0);

    // The other side turns up and it stalls, and both clients see that.
    let at = world.server.game.sim.soldiers[a].body.pos;
    world.server.game.sim.soldiers[d].body.pos = at + glam::Vec3::X * 2.0;
    world.run(3.0);
    let stalled = progress(&world, target);
    world.run(3.0);
    assert_eq!(progress(&world, target), stalled, "even numbers");
    for player in [attacker, defender] {
        let point = told(&world, player).points[target];
        assert_eq!(point.inside, [1, 1]);
        assert_eq!(point.rate, 0.0);
    }

    // They leave, and it goes on to the end.
    world.server.game.sim.soldiers[d].body.pos = world.server.game.sim.level.teams[1].spawns[0].pos;
    for _ in 0..120 {
        world.run(0.5);
        if world.server.game.game.state.points[target].owner == Some(0) {
            break;
        }
    }
    assert_eq!(world.server.game.game.state.points[target].owner, Some(0), "taken on the server");
    world.run(2.0);
    for player in [attacker, defender] {
        assert_eq!(told(&world, player).points[target].owner, Some(0), "and both clients know");
        assert!(
            mode_events(&world, player).contains(&ModeEvent::PointCaptured { point: target, team: 0 }),
            "and were told: {:?}",
            mode_events(&world, player)
        );
    }
    assert_eq!(world.server.game.game.state.player(a).unwrap().score.captures, 1);
    assert_eq!(told(&world, defender).player(a).unwrap().score.captures, 1);
}

#[test]
fn two_clients_complete_a_capture_on_a_lossy_datagram_network() {
    a_capture_over(Net::datagram(60.0, 15.0, 0.03));
}

#[test]
fn two_clients_complete_a_capture_on_a_lossy_tcp_network() {
    a_capture_over(Net::tcp(60.0, 15.0, 0.01, 200.0));
}

#[test]
fn what_a_hacked_client_asks_for_is_refused_by_the_server() {
    let mut world = world();
    let net = Net::datagram(40.0, 5.0, 0.0);
    let cheat = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    let other = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    world.run(2.0);
    let (c, o) = (id_of(&world, cheat), id_of(&world, other));
    let before_tickets = world.server.game.game.state.teams[0].tickets;
    let their_squad = world.server.game.game.state.squad_of(o).unwrap().id;
    let own_squad = world.server.game.game.state.squad_of(c).unwrap().id;

    ask(&mut world, cheat, ModeCmd::SetKit(9999));
    world.run(1.0);
    assert_eq!(world.server.game.game.state.player(c).unwrap().kit, 0, "no such kit");
    ask(&mut world, cheat, ModeCmd::JoinSquad(their_squad));
    ask(&mut world, cheat, ModeCmd::SetSpawn(SpawnAt::Rally));
    ask(&mut world, cheat, ModeCmd::PlaceRally);
    ask(&mut world, cheat, ModeCmd::JoinTeam(200));
    ask(&mut world, cheat, ModeCmd::Deploy);
    world.run(2.0);
    let state = &world.server.game.game.state;
    let player = state.player(c).unwrap();
    assert_eq!(player.squad, Some(own_squad), "not a squad on the other side");
    assert_eq!(player.spawn, SpawnAt::Base, "no rally to spawn on");
    assert_eq!(player.team, 0);
    assert!(state.squads.iter().all(|s| s.rally.is_none()), "not alone and off the field");
    assert_eq!(state.teams[0].tickets, before_tickets);
    let refused = mode_events(&world, cheat);
    assert!(refused.contains(&ModeEvent::Refused { soldier: c, why: Refusal::NoSuchKit }), "{refused:?}");
    assert!(refused.contains(&ModeEvent::Refused { soldier: c, why: Refusal::OtherTeam }), "{refused:?}");

    // Bytes that mean nothing are dropped, and asking someone else's business is not possible:
    // a request is always taken as coming from the player who sent it.
    world.players[cheat].session.send_request(vec![255, 255, 255]);
    world.players[cheat].session.send_request(Vec::new());
    world.run(1.0);
    assert!(world.server.game.sim.soldiers[o].inventory.stash.medkits == 0 || !world.server.game.sim.soldiers[o].present);
    assert_eq!(world.server.game.game.state.player(o).unwrap().kit, 0);
}

#[test]
fn a_match_at_rest_sends_no_match_state_at_all() {
    let mut world = world();
    let net = Net::datagram(40.0, 5.0, 0.0);
    let one = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    let two = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    ask(&mut world, one, ModeCmd::Deploy);
    ask(&mut world, two, ModeCmd::Deploy);
    world.run(5.0);
    let settled = world.server.game.game.state.clone();
    let shared = prs_net::game::ServerGame::shared(&world.server.game);
    world.run(10.0);
    assert_eq!(world.server.game.game.state, settled);
    assert_eq!(prs_net::game::ServerGame::shared(&world.server.game), shared, "same bytes, so nothing is sent");
}

#[test]
fn the_server_kills_a_client_in_the_enemy_base_and_the_client_is_told_in_time() {
    let mut world = world();
    let net = Net::datagram(60.0, 15.0, 0.03);
    let cheat = join(&mut world, &net, 0.0, Rc::new(Cell::new(false)));
    world.run(2.0);
    let c = id_of(&world, cheat);
    ask(&mut world, cheat, ModeCmd::Deploy);
    world.run(2.0);
    let warning = world.server.game.game.mode.cfg.base_protection.as_ref().unwrap().warning_seconds as f64;
    let base = world.server.game.sim.level.teams[1].spawns[3].pos;
    world.server.game.sim.soldiers[c].body.pos = base;
    world.run(2.0);
    let since = world.server.game.game.state.player(c).unwrap().trespass_since.expect("the server saw it");
    assert_eq!(told(&world, cheat).player(c).unwrap().trespass_since, Some(since), "and the client has the clock");
    assert!(world.server.game.sim.soldiers[c].health.is_up());
    world.run(warning);
    assert!(world.server.game.sim.soldiers[c].health.is_dead());
    world.run(1.0);
    assert_eq!(told(&world, cheat).player(c).unwrap().score.deaths, 1);
    assert_eq!(told(&world, cheat).player(c).unwrap().trespass_since, None);
}
