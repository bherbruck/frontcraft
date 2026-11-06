//! What players are called, from what they type to what everyone else is shown, over the real wire.

use prs_net::client::{ClientConfig, ClientSession};
use prs_net::server::{ClientId, GameServer, ServerConfig};
use prs_net_sim::{SimClient, SimServer, SimServerConfig};
use prs_sim::protocol::{PlayerCmd, TICK_DT};

struct Player {
    id: ClientId,
    session: ClientSession<SimClient>,
}

struct Lobby {
    server: GameServer<SimServer>,
    players: Vec<Player>,
}

impl Lobby {
    fn new() -> Self {
        let game = SimServer::new("range", "practice", SimServerConfig::default()).unwrap();
        Self { server: GameServer::new(game, ServerConfig::default()), players: Vec::new() }
    }

    fn join(&mut self, name: &str) {
        let factory = Box::new(|map: &str, mode: &str| SimClient::setup(map, mode));
        let session = ClientSession::new(factory, ClientConfig { name: name.into(), ..Default::default() });
        self.players.push(Player { id: self.server.connect(true), session });
    }

    /// A perfect network: everything arrives the same tick.
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            for player in &mut self.players {
                player.session.update(TICK_DT, |tick| PlayerCmd { tick, ..Default::default() });
                for frame in player.session.drain_outgoing() {
                    self.server.receive(player.id, &frame);
                }
            }
            self.server.tick();
            for (id, frame) in self.server.drain_outgoing() {
                if let Some(player) = self.players.iter_mut().find(|p| p.id == id) {
                    player.session.receive(&frame);
                }
            }
        }
    }

    /// Everyone's names as one player has been told them, by player number.
    fn seen_by(&self, who: usize) -> Vec<(u16, String)> {
        let mut names = self.players[who].session.game().map(|game| game.names.clone()).unwrap_or_default();
        names.sort();
        names
    }

    fn number(&self, who: usize) -> u16 {
        self.server.player_of(self.players[who].id).unwrap()
    }
}

#[test]
fn everyone_sees_everyone_else_by_name_including_whoever_joins_late() {
    let mut lobby = Lobby::new();
    lobby.join("Hawk");
    lobby.join("  Big   Bear ");
    lobby.run(120);
    let both = vec![(lobby.number(0), "Hawk".to_string()), (lobby.number(1), "Big Bear".to_string())];
    let mut expected = both.clone();
    expected.sort();
    assert_eq!(lobby.seen_by(0), expected);
    assert_eq!(lobby.seen_by(1), expected);

    lobby.join("Hawk");
    lobby.run(120);
    let mut expected = both;
    expected.push((lobby.number(2), "Hawk 2".to_string()));
    expected.sort();
    for who in 0..3 {
        assert_eq!(lobby.seen_by(who), expected, "as player {who} sees it");
    }
}

#[test]
fn a_name_goes_when_its_player_does() {
    let mut lobby = Lobby::new();
    lobby.join("Hawk");
    lobby.join("Wren");
    lobby.run(120);
    let gone = lobby.players.remove(1);
    lobby.server.disconnect(gone.id);
    lobby.run(120);
    assert_eq!(lobby.seen_by(0), [(lobby.number(0), "Hawk".to_string())]);
}

#[test]
fn a_bad_name_is_cleaned_up_and_an_empty_one_gets_a_number() {
    let mut lobby = Lobby::new();
    lobby.join("Bot 1");
    lobby.join("\u{202E}Wr\u{200B}en\n");
    lobby.run(120);
    let first = lobby.number(0);
    let mut expected = vec![(first, format!("Player {first}")), (lobby.number(1), "Wren".to_string())];
    expected.sort();
    assert_eq!(lobby.seen_by(1), expected);
}

#[test]
fn names_are_not_part_of_the_rules() {
    let mut lobby = Lobby::new();
    let before = prs_net::game::ServerGame::rules_hash(&lobby.server.game);
    lobby.join("Hawk");
    lobby.run(60);
    assert_eq!(prs_net::game::ServerGame::rules_hash(&lobby.server.game), before);
    assert!(lobby.players[0].session.game().is_some(), "joined, so the client's rules matched");
}
