//! A client with a different mod from the server's is told so, and one with the same mod plays.

use prs_mods::Mod;
use prs_net::client::{ClientState, GameFactory};
use prs_net::server::ServerConfig;
use prs_net::testkit::{Net, World};
use prs_net::wire::RejectReason;
use prs_net_sim::{SimClient, SimServer, SimServerConfig, rules_hash};
use prs_server::bots::{BotScript, bot_cmd};
use prs_sim::{Level, Sim};
use std::rc::Rc;

const ARCADE: &str = include_str!("../../../mods/arcade.toml");

fn modded(text: &str, name: &str) -> Mod {
    Mod::parse(name, text).unwrap_or_else(|e| panic!("{e}"))
}

fn server(active: &Mod) -> World<SimClient, SimServer> {
    let sim = Sim::with_mod(Level::named("range").unwrap(), active).unwrap();
    let mode = prs_mode::Mode::named("practice").unwrap();
    let game = SimServer::with_sim(sim, "range", "practice", mode, SimServerConfig::default());
    let cfg = ServerConfig { rules_note: prs_mods::describe(&active.name), ..Default::default() };
    World::new(game, cfg, |server: &SimServer, id: u16| server.sim.soldiers[id as usize].body.pos)
}

fn client(active: Mod) -> GameFactory<SimClient> {
    Box::new(move |map: &str, mode: &str| {
        let sim = Sim::with_mod(Level::named(map)?, &active).ok()?;
        Some(SimClient::with_sim(sim, prs_mode::Mode::named(mode)?))
    })
}

fn still() -> Rc<dyn Fn(u32, u32) -> prs_sim::protocol::PlayerCmd> {
    Rc::new(|who, tick| bot_cmd(BotScript::Idle, who, tick))
}

#[test]
fn a_mod_changes_the_rules_hash_and_the_same_mod_does_not() {
    let hash = |active: &Mod| {
        let sim = Sim::with_mod(Level::named("range").unwrap(), active).unwrap();
        rules_hash(&sim, &prs_mode::Mode::named("practice").unwrap())
    };
    assert_ne!(hash(&Mod::default()), hash(&modded(ARCADE, "arcade")));
    assert_eq!(hash(&modded(ARCADE, "arcade")), hash(&modded(ARCADE, "arcade")));
    assert_eq!(hash(&Mod::default()), hash(&modded("", "empty")), "a mod that says nothing is no mod");
}

#[test]
fn a_server_refuses_a_client_with_another_mod_and_says_which_it_runs() {
    let mut world = server(&modded(ARCADE, "arcade"));
    let net = Net::datagram(10.0, 0.0, 0.0);
    world.join(client(Mod::default()), still(), &net);
    world.run(1.0);
    let session = &world.players[0].session;
    assert_eq!(session.state(), ClientState::Rejected(RejectReason::Rules));
    assert_eq!(session.reject_note(), "mod 'arcade'");
}

#[test]
fn a_client_with_the_same_mod_plays_and_gets_the_modded_gun() {
    let arcade = modded(ARCADE, "arcade");
    let mut world = server(&arcade);
    let net = Net::datagram(10.0, 0.0, 0.0);
    world.join(client(arcade), still(), &net);
    world.run(1.0);
    let session = &world.players[0].session;
    assert_eq!(session.state(), ClientState::Playing);
    let sim = &session.game().unwrap().sim;
    assert!(sim.weapons.weapons[0].ads_time <= 0.02);
}
