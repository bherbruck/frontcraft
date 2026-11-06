//! Your system on a bad network, in a test, in a few lines: a real server sim and real client sims
//! with a simulated network between them, on a virtual clock, so it is fast and exactly repeatable.
//!
//! ```ignore
//! use prs_net_sim::testkit::*;
//!
//! let mut world = world_on("range");
//! let net = Net::datagram(50.0, 10.0, 0.02); // ms one way, ms jitter, loss. Or Net::tcp(..).
//! let me = world.join(client_on("range"), script(|who, tick| walk_forward(tick)), &net);
//! world.run(10.0);
//! assert_eq!(world.players[me].reconciles(), 0);
//! let server_side = &world.server.game.sim;  // the truth, to change or check
//! let my_copy = &world.players[me].session.game().unwrap().sim;
//! ```

pub use prs_net::server::ServerConfig;
pub use prs_net::testkit::{Net, Observer, World};

use glam::{Vec2, Vec3};
use prs_net::client::GameFactory;
use prs_sim::protocol::PlayerCmd;
use std::rc::Rc;

use crate::{SimClient, SimServer, SimServerConfig};

pub type SimWorld = World<SimClient, SimServer>;
pub type Script = Rc<dyn Fn(u32, u32) -> PlayerCmd>;

pub fn position_of(server: &SimServer, id: u16) -> Vec3 {
    server.sim.soldiers[id as usize].body.pos
}

/// A server playing this map, with the usual settings.
pub fn world_on(map: &str) -> SimWorld {
    world_with(map, ServerConfig::default())
}

pub fn world_with(map: &str, cfg: ServerConfig) -> SimWorld {
    let game = SimServer::new(map, "practice", SimServerConfig::default()).expect("a map by that name");
    World::new(game, cfg, position_of)
}

/// A client that sets itself up however the server says, as a real one does.
pub fn client_on(_map: &str) -> GameFactory<SimClient> {
    Box::new(|map: &str, mode: &str| SimClient::setup(map, mode))
}

/// A player's input: the command for (player number, tick).
pub fn script(f: impl Fn(u32, u32) -> PlayerCmd + 'static) -> Script {
    Rc::new(f)
}

pub fn walk_forward(tick: u32) -> PlayerCmd {
    PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_in_the_header_works() {
        let mut world = world_on("range");
        let net = Net::datagram(50.0, 10.0, 0.0);
        let me = world.join(client_on("range"), script(|_, tick| walk_forward(tick)), &net);
        world.run(5.0);
        assert_eq!(world.players[me].reconciles(), 0);
    }
}
