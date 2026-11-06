//! Who hears what when somebody talks, what the server throws away, and that none of it touches the
//! game. Everything goes through `GameServer` as frames, the way it does from a real socket.

use glam::Vec3;
use prs_net::server::{ClientId, GameServer, ServerConfig, VoiceConfig};
use prs_net::testkit::{Net, World};
use prs_net::wire::{ClientMsg, CmdPacket, PROTOCOL_VERSION, VoiceChannel, VoiceUp};
use prs_net_sim::{SimClient, SimServer, SimServerConfig};
use prs_server::bots::{BotScript, bot_cmd};
use prs_sim::protocol::PlayerCmd;
use prs_sim::SoldierId;
use std::rc::Rc;

const OPUS: [u8; 80] = [7; 80];
const TAG_VOICE: u8 = 20;

struct Table {
    server: GameServer<SimServer>,
    clients: Vec<(ClientId, u16)>,
}

impl Table {
    fn new(players: usize, voice: VoiceConfig) -> Self {
        let game = SimServer::new("town", "aas", SimServerConfig { bots_per_team: Some(0), ..Default::default() }).unwrap();
        let rules = prs_net::game::ServerGame::rules_hash(&game);
        let mut server = GameServer::new(game, ServerConfig { voice, ..Default::default() });
        let mut clients = Vec::new();
        for _ in 0..players {
            let id = server.connect(true);
            server.receive(id, &ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode());
            server.receive(id, &ClientMsg::Join { rules, name: String::new() }.encode());
            clients.push((id, server.player_of(id).unwrap()));
        }
        let mut table = Self { server, clients };
        table.tick(3);
        table
    }

    fn id(&self, who: usize) -> ClientId {
        self.clients[who].0
    }

    fn player(&self, who: usize) -> u16 {
        self.clients[who].1
    }

    /// Runs the game with everybody sending commands and throws away what it says.
    fn tick(&mut self, ticks: u32) {
        for _ in 0..ticks {
            let tick = prs_net::game::ServerGame::tick(&self.server.game) + 1;
            for who in 0..self.clients.len() {
                let packet = CmdPacket { cmds: vec![PlayerCmd { tick, ..Default::default() }], ..Default::default() };
                self.server.receive(self.id(who), &ClientMsg::Cmds(packet).encode());
            }
            self.server.tick();
            self.server.drain_outgoing();
        }
    }

    fn put(&mut self, who: usize, at: Vec3) {
        let soldier = self.player(who) as SoldierId;
        self.server.game.sim.soldiers[soldier].body.pos = at;
    }

    fn listen(&mut self, who: usize, muted: Vec<u16>) {
        self.server.receive(self.id(who), &ClientMsg::VoiceSet { listening: true, muted }.encode());
    }

    fn talk(&mut self, who: usize, channel: VoiceChannel) {
        let up = VoiceUp { channel, seq: 1, bytes: OPUS.to_vec() };
        self.server.receive(self.id(who), &ClientMsg::VoiceUp(up).encode());
    }

    /// How many frames of voice each client got out of what was waiting, by who they are.
    fn heard(&mut self) -> Vec<usize> {
        let out = self.server.drain_outgoing();
        (0..self.clients.len())
            .map(|who| out.iter().filter(|(id, frame)| *id == self.id(who) && frame[0] == TAG_VOICE).count())
            .collect()
    }
}

fn on_foot(table: &mut Table) {
    for who in 0..table.clients.len() {
        let soldier = table.player(who) as SoldierId;
        let state = &mut table.server.game.game.state;
        if let Some(player) = state.players.iter_mut().find(|p| p.soldier == soldier) {
            player.deployed = true;
            player.team = who as u8 % 2;
        }
        table.server.game.sim.soldiers[soldier].present = true;
        table.put(who, Vec3::new(who as f32, 2.0, 0.0));
    }
}

/// Players 0 and 2 on one team and 1 and 3 on the other, all standing together.
fn four() -> Table {
    let mut table = Table::new(4, VoiceConfig::default());
    on_foot(&mut table);
    for who in 0..4 {
        table.listen(who, Vec::new());
    }
    table.heard();
    table
}

fn squad_up(table: &mut Table, squad: u32, members: &[usize]) {
    for &who in members {
        let soldier = table.player(who) as SoldierId;
        let state = &mut table.server.game.game.state;
        state.players.iter_mut().find(|p| p.soldier == soldier).unwrap().squad = Some(squad);
    }
}

#[test]
fn talking_out_loud_reaches_anyone_close_on_either_team_and_not_the_talker() {
    let mut table = four();
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 1, 1, 1]);
}

#[test]
fn out_of_range_nothing_is_sent_at_all() {
    let mut table = four();
    let reach = SimServerConfig::default().reach.voice_heard;
    table.put(1, Vec3::new(reach - 1.0, 2.0, 0.0));
    table.put(2, Vec3::new(reach + 1.0, 2.0, 0.0));
    table.put(3, Vec3::new(0.0, 2.0, reach * 5.0));
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 1, 0, 0]);
}

#[test]
fn the_radio_reaches_only_the_squad_however_far() {
    let mut table = four();
    squad_up(&mut table, 1, &[0, 2]);
    // On the other team, with the same number, which is nobody's squad but looks like it.
    squad_up(&mut table, 1, &[1]);
    table.put(2, Vec3::new(3000.0, 2.0, 0.0));
    table.talk(0, VoiceChannel::Squad);
    assert_eq!(table.heard(), [0, 0, 1, 0]);
}

#[test]
fn no_squad_means_nobody_on_the_radio() {
    let mut table = four();
    for player in &mut table.server.game.game.state.players {
        player.squad = None;
    }
    table.talk(0, VoiceChannel::Squad);
    assert_eq!(table.heard(), [0, 0, 0, 0]);
}

#[test]
fn someone_on_the_deploy_screen_is_not_heard_out_loud() {
    let mut table = four();
    let soldier = table.player(0) as SoldierId;
    let state = &mut table.server.game.game.state;
    state.players.iter_mut().find(|p| p.soldier == soldier).unwrap().deployed = false;
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 0, 0, 0]);
}

#[test]
fn a_mute_stops_it_at_once_even_what_is_already_waiting() {
    let mut table = four();
    let talker = table.player(0);
    table.talk(0, VoiceChannel::Local);
    table.listen(1, vec![talker]);
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 0, 2, 2]);
    table.listen(1, Vec::new());
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 1, 1, 1]);
}

#[test]
fn nothing_goes_to_a_client_that_has_not_asked_for_voice() {
    let mut table = Table::new(2, VoiceConfig::default());
    on_foot(&mut table);
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 0]);
}

#[test]
fn a_server_with_voice_off_carries_none_and_says_so() {
    let mut table = Table::new(2, VoiceConfig { enabled: false, ..Default::default() });
    on_foot(&mut table);
    table.listen(1, Vec::new());
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 0]);
    assert_eq!(table.server.counters(table.id(0)).unwrap().voice_taken, 0);
}

#[test]
fn a_talker_over_the_rate_is_cut_off_until_the_next_second() {
    let cfg = VoiceConfig::default();
    let allowed = cfg.talker_frames_per_second as usize;
    let mut table = Table::new(2, cfg);
    on_foot(&mut table);
    table.listen(1, Vec::new());
    table.tick(70);
    let mut got = 0;
    for _ in 0..allowed * 3 {
        table.talk(0, VoiceChannel::Local);
        got += table.heard()[1];
    }
    assert!(got > 0 && got <= allowed, "{got} frames got through against a limit of {allowed}");
    assert!(table.server.counters(table.id(0)).unwrap().voice_refused > 0);
    table.tick(prs_sim::protocol::TICK_HZ as u32 + 1);
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard()[1], 1, "the next second starts afresh");
}

#[test]
fn a_frame_too_big_or_empty_is_refused() {
    let cfg = VoiceConfig::default();
    let biggest = cfg.max_frame_bytes;
    let mut table = Table::new(2, cfg);
    on_foot(&mut table);
    table.listen(1, Vec::new());
    for len in [biggest + 1, 0] {
        let up = VoiceUp { channel: VoiceChannel::Local, seq: 0, bytes: vec![1; len] };
        table.server.receive(table.id(0), &ClientMsg::VoiceUp(up).encode());
        assert_eq!(table.heard()[1], 0, "{len} bytes");
    }
    let up = VoiceUp { channel: VoiceChannel::Local, seq: 0, bytes: vec![1; biggest] };
    table.server.receive(table.id(0), &ClientMsg::VoiceUp(up).encode());
    assert_eq!(table.heard()[1], 1);
}

#[test]
fn garbage_that_says_it_is_voice_is_only_a_bad_frame() {
    let mut table = Table::new(2, VoiceConfig::default());
    for frame in [&[4u8][..], &[4, 9, 0, 0], &[4, 0, 0, 200, 200, 3], &[5, 1, 200, 200, 200, 3]] {
        table.server.receive(table.id(0), frame);
    }
    assert_eq!(table.server.counters(table.id(0)).unwrap().bad_frames, 4);
}

#[test]
fn a_listener_that_cannot_keep_up_loses_voice_and_nothing_else_waits() {
    let mut table = four();
    let waiting = table.id(1);
    table.talk(0, VoiceChannel::Local);
    table.server.set_backlogged(waiting, true);
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 0, 2, 2], "what was waiting and what came after are both gone");
    assert_eq!(table.server.counters(waiting).unwrap().voice_dropped, 2);
    table.server.set_backlogged(waiting, false);
    table.talk(0, VoiceChannel::Local);
    assert_eq!(table.heard(), [0, 1, 1, 1], "and it comes back when it can");
}

#[test]
fn the_queue_never_grows_past_its_limit() {
    // Three frames' worth, each of the bytes and the little in front of them.
    let cfg = VoiceConfig { queue_bytes: 3 * (OPUS.len() + 16), talker_frames_per_second: 1000, talker_bytes_per_second: 1_000_000, ..Default::default() };
    let mut table = Table::new(2, cfg);
    on_foot(&mut table);
    table.listen(1, Vec::new());
    table.tick(70);
    for _ in 0..10 {
        table.talk(0, VoiceChannel::Local);
    }
    assert_eq!(table.heard()[1], 3);
    assert_eq!(table.server.counters(table.id(1)).unwrap().voice_dropped, 7);
}

#[test]
fn voice_goes_after_the_game_in_every_batch() {
    let mut table = four();
    table.talk(0, VoiceChannel::Local);
    for _ in 0..4 {
        table.server.tick();
    }
    let tags: Vec<u8> = table.server.drain_outgoing().iter().filter(|(id, _)| *id == table.id(1)).map(|(_, f)| f[0]).collect();
    let first = tags.iter().position(|&tag| tag == TAG_VOICE).unwrap();
    assert!(tags[first..].iter().all(|&tag| tag == TAG_VOICE), "game state after voice: {tags:?}");
    assert!(first > 0, "the snapshots went first: {tags:?}");
}

/// The same game with and without people talking comes out the same, and the same on the wire.
#[test]
fn people_talking_changes_nothing_in_the_game() {
    let run = |talking: bool| {
        let mut table = Table::new(3, VoiceConfig { talker_frames_per_second: 1000, talker_bytes_per_second: 1_000_000, ..Default::default() });
        on_foot(&mut table);
        squad_up(&mut table, 1, &[0, 2]);
        for who in 0..3 {
            table.listen(who, Vec::new());
        }
        table.server.drain_outgoing();
        let mut game_frames = Vec::new();
        let mut voice_frames = 0;
        for step in 0..240u32 {
            let tick = prs_net::game::ServerGame::tick(&table.server.game) + 1;
            for who in 0..3 {
                let cmd = PlayerCmd { tick, yaw: step as f32 * 0.01, ..Default::default() };
                let packet = CmdPacket { cmds: vec![cmd], ..Default::default() };
                table.server.receive(table.id(who), &ClientMsg::Cmds(packet).encode());
                if talking {
                    table.talk(who, if who == 0 { VoiceChannel::Local } else { VoiceChannel::Squad });
                }
            }
            table.server.tick();
            let frames = table.server.drain_outgoing();
            voice_frames += frames.iter().filter(|(_, frame)| frame[0] == TAG_VOICE).count();
            game_frames.extend(frames.into_iter().filter(|(_, frame)| frame[0] != TAG_VOICE));
        }
        assert_eq!(voice_frames > 0, talking, "{voice_frames} frames of voice");
        let rules = prs_net::game::ServerGame::rules_hash(&table.server.game);
        (game_frames, rules)
    };
    let quiet = run(false);
    let noisy = run(true);
    assert_eq!(quiet.1, noisy.1, "the rules hash");
    assert_eq!(quiet.0.len(), noisy.0.len());
    assert!(quiet.0 == noisy.0, "the frames of game state are byte for byte the same");
}

#[test]
fn a_client_cannot_say_who_it_is_talking_as() {
    // There is no field for it: what is heard carries the speaker the server knows.
    let mut table = four();
    table.talk(2, VoiceChannel::Local);
    let out = table.server.drain_outgoing();
    let (_, frame) = out.iter().find(|(_, frame)| frame[0] == TAG_VOICE).unwrap();
    assert_eq!(u16::from_le_bytes([frame[1], frame[2]]), table.player(2));
}

/// Two players who never touch, with both talking at the full rate or not at all.
fn prediction_with(talking: bool) -> Vec<(u32, f32, u64)> {
    let spread = SimServerConfig { spawn_spacing: 30.0, ..SimServerConfig::default() };
    let game = SimServer::new("range", "practice", spread).unwrap();
    let mut world: World<SimClient, SimServer> = World::new(game, ServerConfig::default(), |game, id| game.sim.soldiers[id as usize].body.pos);
    let net = Net::datagram(50.0, 0.0, 0.0);
    for _ in 0..2 {
        let patrol = Rc::new(|who, tick| bot_cmd(BotScript::Patrol, who, tick));
        world.join(Box::new(|map: &str, mode: &str| SimClient::setup(map, mode)), patrol, &net);
    }
    world.run(1.0);
    let ids: Vec<ClientId> = world.players.iter().map(|player| player.id).collect();
    for id in &ids {
        world.server.receive(*id, &ClientMsg::VoiceSet { listening: talking, muted: Vec::new() }.encode());
    }
    // 25 frames a second, for half a minute, each in the middle of everything else.
    for _ in 0..750 {
        world.run(0.04);
        if talking {
            for id in &ids {
                let up = VoiceUp { channel: VoiceChannel::Local, seq: 0, bytes: OPUS.to_vec() };
                world.server.receive(*id, &ClientMsg::VoiceUp(up).encode());
            }
        }
    }
    let voice: u64 = ids.iter().map(|id| world.server.counters(*id).unwrap().voice_sent).sum();
    assert_eq!(voice > 1000, talking, "{voice} frames of voice sent");
    world
        .players
        .iter()
        .map(|player| {
            let stats = player.session.predictor().unwrap().stats();
            (stats.reconciles, stats.max_error, world.server.counters(player.id).unwrap().snapshots_sent)
        })
        .collect()
}

#[test]
fn voice_at_the_full_rate_leaves_prediction_and_snapshots_as_they_were() {
    let (quiet, noisy) = (prediction_with(false), prediction_with(true));
    assert_eq!(quiet, noisy);
    assert!(noisy.iter().all(|(reconciles, error, _)| *reconciles == 0 && *error == 0.0), "{noisy:?}");
}

/// What voice costs a server with every seat taken: run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn what_routing_costs_with_a_full_server() {
    let players = ServerConfig::default().max_players;
    let mut table = Table::new(players, VoiceConfig { talker_frames_per_second: 100_000, talker_bytes_per_second: u32::MAX, listener_bytes_per_second: u32::MAX, ..Default::default() });
    on_foot(&mut table);
    for who in 0..players {
        table.listen(who, Vec::new());
    }
    table.server.drain_outgoing();
    let frames = 20_000;
    let began = std::time::Instant::now();
    for i in 0..frames {
        table.talk(i % players, VoiceChannel::Local);
        table.server.drain_outgoing();
    }
    let each = began.elapsed().as_secs_f64() / frames as f64;
    println!("one frame to {} listeners: {:.1} us", players - 1, each * 1e6);
    for talkers in [4.0, players as f64] {
        // 25 frames a second each, over 60 ticks a second.
        println!("{talkers} talking at once: {:.3} ms a tick", talkers * 25.0 / 60.0 * each * 1e3);
    }
}
