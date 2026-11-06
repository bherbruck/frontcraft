//! Your own suppression over a real server and client. Nobody predicts other people's bullets, so it
//! arrives from the server a little late and the client takes its word and goes on from there.
//! `cargo test -p prs_server --test suppression -- --nocapture` prints how it behaves.

use glam::Vec3;
use prs_net::server::ServerConfig;
use prs_net::testkit::{Net, World};
use prs_net_sim::{SimClient, SimServer, SimServerConfig};
use prs_server::bots::{BotScript, bot_cmd};
use prs_sim::protocol::{PlayerCmd, TICK_DT};
use prs_sim::{Bullet, FlyingBullet, Level, Sim};
use std::rc::Rc;

type Game = World<SimClient, SimServer>;

fn world() -> Game {
    let server = SimServer::new("range", "practice", SimServerConfig::default()).expect("the map exists");
    World::new(server, ServerConfig::default(), |server: &SimServer, id: u16| server.sim.soldiers[id as usize].body.pos)
}

fn still() -> Rc<dyn Fn(u32, u32) -> PlayerCmd> {
    Rc::new(|who, tick| bot_cmd(BotScript::Idle, who, tick))
}

/// A rifle round passing a metre from this soldier, from far enough away to count.
fn near_miss(world: &mut Game, shooter: usize, soldier: usize) {
    let sim = &mut world.server.game.sim;
    let eye = sim.soldiers[soldier].eye();
    let at = eye + Vec3::X;
    let from = at + Vec3::new(0.0, 0.0, 12.0);
    let weapon = sim.soldiers[shooter].weapon.weapon;
    let bullet = Bullet::new(from, (at - from).normalize(), &sim.weapons[weapon].ballistics);
    sim.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
}

#[test]
fn at_100_ms_your_own_suppression_arrives_late_and_then_follows_the_server() {
    let mut world = world();
    let net = Net::datagram(50.0, 0.0, 0.0);
    world.join(Box::new(|map: &str, mode: &str| SimClient::setup(map, mode)), still(), &net);
    world.join(Box::new(|map: &str, mode: &str| SimClient::setup(map, mode)), still(), &net);
    world.run(3.0);
    let (me, other) = (world.player_id(0) as usize, world.player_id(1) as usize);
    let before = world.players[0].session.predictor().unwrap().stats();

    near_miss(&mut world, other, me);
    let (mut on_server, mut on_client) = (None, None);
    let client_level = |world: &Game| world.players[0].session.game().unwrap().sim.soldiers[me].suppression.level;
    for step in 0..60 {
        world.run(TICK_DT as f64);
        let now = step as f32 * TICK_DT;
        if on_server.is_none() && world.server.game.sim.soldiers[me].suppression.level > 0.0 {
            on_server = Some(now);
        }
        if on_client.is_none() && client_level(&world) > 0.0 {
            on_client = Some(now);
        }
    }
    let (on_server, on_client) = (on_server.expect("the server saw it"), on_client.expect("the client heard"));
    let late = on_client - on_server;
    let stats = world.players[0].session.predictor().unwrap().stats();
    println!(
        "at 100 ms: on the client {:.0} ms after the server, {} reconciles, {:.4} m moved",
        late * 1000.0,
        stats.reconciles - before.reconciles,
        stats.max_correction,
    );
    assert!(late < 0.2, "took {late} s to arrive");
    assert!(stats.reconciles - before.reconciles <= 2, "one near miss is one correction: {stats:?}");
    assert!(stats.max_correction < 0.01, "being shot at must not move you: {stats:?}");

    // From then on the client predicts it wearing off, and agrees.
    let settled = world.players[0].session.predictor().unwrap().stats().reconciles;
    world.run(1.0);
    let (server, client) = (world.server.game.sim.soldiers[me].suppression.level, client_level(&world));
    assert!(server > 0.0 && (server - client).abs() < 0.02, "server {server} client {client}");
    let after = world.players[0].session.predictor().unwrap().stats().reconciles;
    assert_eq!(after, settled, "wearing off needs no correcting");
}
