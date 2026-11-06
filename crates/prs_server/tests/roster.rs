//! Found by the soak: the match's roster reaches a client before all the soldiers on it do, and the
//! map and the markers look people up by what the roster says.

use prs_net::server::ServerConfig;
use prs_net::testkit::{Net, World};
use prs_net_sim::{SimClient, SimServer, SimServerConfig};
use prs_server::bots::{BotScript, bot_cmd};
use prs_sim::{Level, Sim};
use std::rc::Rc;

#[test]
fn everyone_on_the_roster_can_be_looked_up_as_soon_as_the_roster_arrives() {
    let server = SimServer::new("town", "aas", SimServerConfig::default()).expect("the map exists");
    let mut world: World<SimClient, SimServer> =
        World::new(server, ServerConfig::default(), |server: &SimServer, id: u16| server.sim.soldiers[id as usize].body.pos);
    let net = Net::datagram(50.0, 0.0, 0.0);
    for _ in 0..3 {
        world.join(
            Box::new(|map: &str, mode: &str| Some(SimClient::with_sim(Sim::new(Level::named(map)?), prs_mode::Mode::named(mode)?))),
            Rc::new(|who, tick| bot_cmd(BotScript::Idle, who, tick)),
            &net,
        );
    }
    let mut seen = 0;
    for _ in 0..120 {
        world.run(0.1);
        for player in &world.players {
            let Some(game) = player.session.game() else { continue };
            let Some(state) = &game.match_state else { continue };
            for id in state.players.iter().map(|p| p.soldier).chain(state.squads.iter().flat_map(|s| s.members.iter().copied())) {
                assert!(id < game.sim.soldiers.len(), "the roster names {id} and there are {}", game.sim.soldiers.len());
                seen += 1;
            }
        }
    }
    assert!(seen > 100, "should have looked at plenty: {seen}");
}
