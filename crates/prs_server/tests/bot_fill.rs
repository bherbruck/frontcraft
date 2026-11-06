//! Bots making room for people on a real server, mid-round: they leave as players join a side and come
//! back as players go, disconnects included, and nobody on the field vanishes or jumps while it happens.

use prs_net::server::ServerConfig;
use prs_net::testkit::{Net, World};
use prs_net_sim::{SimClient, SimServer, SimServerConfig};
use prs_sim::protocol::PlayerCmd;
use std::rc::Rc;

/// Further than anyone goes in a tenth of a second on their own feet.
const JUMP: f32 = 2.5;

type Game = World<SimClient, SimServer>;

fn world(per_team: usize) -> Game {
    let cfg = SimServerConfig { bots_per_team: Some(per_team), ..Default::default() };
    let server = SimServer::new("town", "aas", cfg).expect("the map and mode exist");
    World::new(server, ServerConfig::default(), |server: &SimServer, id: u16| server.sim.soldiers[id as usize].body.pos)
}

fn join(world: &mut Game) -> usize {
    let idle = Rc::new(|_who: u32, tick: u32| PlayerCmd { tick, ..Default::default() });
    let factory = Box::new(|map: &str, mode: &str| SimClient::setup(map, mode));
    world.join(factory, idle, &Net::datagram(40.0, 5.0, 0.0))
}

/// (players, bots) on each side, as the server has it.
fn sides(world: &Game) -> [(usize, usize); 2] {
    let state = &world.server.game.game.state;
    [0, 1].map(|team| {
        let on = state.players.iter().filter(|p| p.team == team);
        let bots = on.clone().filter(|p| p.bot).count();
        (on.count() - bots, bots)
    })
}

/// Runs, checking every tenth of a second that no bot still in the match moved further than it could have.
fn run_watching(world: &mut Game, seconds: f64) {
    for _ in 0..(seconds * 10.0) as u32 {
        let before: Vec<_> = world.server.game.game.state.players.iter().filter(|p| p.bot).map(|p| {
            let soldier = &world.server.game.sim.soldiers[p.soldier];
            (p.soldier, soldier.body.pos, soldier.lives, soldier.present)
        }).collect();
        world.run(0.1);
        let sim = &world.server.game.sim;
        for (id, pos, lives, present) in before {
            let soldier = &sim.soldiers[id];
            let still_here = world.server.game.game.state.player(id).is_some();
            // Coming back after dying is a jump to the base, and that is meant to be one.
            if still_here && present && soldier.present && soldier.lives == lives {
                assert!(soldier.body.pos.distance(pos) < JUMP, "bot {id} jumped {} m", soldier.body.pos.distance(pos));
            }
        }
    }
}

#[test]
fn bots_make_way_for_players_mid_round_and_come_back_when_they_go() {
    let mut world = world(4);
    world.run(1.0);
    assert_eq!(sides(&world), [(0, 4), (0, 4)]);
    let round = world.server.game.game.state.round;

    let first = join(&mut world);
    run_watching(&mut world, 2.0);
    assert_eq!(sides(&world), [(1, 3), (0, 4)], "the first player takes a bot's place on the first side");
    let second = join(&mut world);
    run_watching(&mut world, 2.0);
    assert_eq!(sides(&world), [(1, 3), (1, 3)], "the next goes to the side with fewer players");

    // A third tips it: the other side gets a bot more to stay the same size.
    let third = join(&mut world);
    run_watching(&mut world, 2.0);
    let [a, b] = sides(&world);
    assert_eq!(a.0 + a.1, b.0 + b.1, "the sides stay the same size: {a:?} {b:?}");

    // Gone without a word: the server drops them, and their place fills again.
    let soldiers: Vec<usize> = [first, second, third].iter().map(|&p| world.player_id(p) as usize).collect();
    for player in [first, second, third] {
        let id = world.players[player].id;
        world.server.disconnect(id);
    }
    run_watching(&mut world, 3.0);
    assert_eq!(sides(&world), [(0, 4), (0, 4)], "nobody left behind holding a place");
    assert_eq!(world.server.game.game.state.round, round, "all of it in one round");
    for soldier in soldiers {
        assert!(!world.server.game.sim.soldiers[soldier].present, "a disconnected soldier is off the field");
    }
}
