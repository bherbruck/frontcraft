//! The whole loop on a simulated network with a toy game: server session and client sessions, real
//! in everything but the game. `prs_server`'s tests run the real simulation through the same harness.
//!
//! The report prints what TCP costs:
//! `cargo test -p prs_net --test loop report -- --ignored --nocapture`

mod common;

use common::*;
use glam::{Vec2, Vec3};
use prs_net::client::{ClientConfig, ClientSession, ClientState};
use prs_net::entity::MAX_FIELDS;
use prs_net::game::ServerGame;
use prs_net::server::{ClientId, GameServer, InterestConfig, ServerConfig};
use prs_net::simnet::{LinkConfig, LinkMode};
use prs_net::testkit::{Hidden, Net};
use prs_net::validate::CmdLimits;
use prs_net::wire::{BaseRef, Baselines, ClientMsg, CmdPacket, PROTOCOL_VERSION, RejectReason, ServerMsg};
use prs_protocol::{Buttons, PlayerCmd, TICK_DT};

const WALK_STEP: f32 = RUN_SPEED * TICK_DT;

#[test]
fn prediction_agrees_exactly_when_nothing_disagrees() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    for _ in 0..4 {
        world.join(ToyClient::factory(), busy(), &net);
    }
    world.run(30.0);
    for player in &world.players {
        let stats = player.session.predictor().unwrap().stats();
        let counters = world.server.counters(player.id).unwrap();
        assert!(stats.compared > 800, "compared {}", stats.compared);
        assert_eq!(stats.reconciles, 0, "{stats:?}");
        assert_eq!(stats.max_error, 0.0, "identical to the bit: {stats:?}");
        assert_eq!(counters.cmds_repeated, 0, "{counters:?}");
        assert_eq!(counters.cmds_late, 0, "{counters:?}");
        assert!(!player.predicted_events.is_empty(), "it really did fire");
    }
}

#[test]
fn prediction_still_agrees_with_jitter_because_the_clock_leads_enough() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 20.0, 0.0);
    for _ in 0..4 {
        world.join(ToyClient::factory(), busy(), &net);
    }
    world.run(30.0);
    for player in &world.players {
        let counters = world.server.counters(player.id).unwrap();
        assert_eq!(player.reconciles(), 0, "{counters:?}");
        assert_eq!(counters.cmds_repeated, 0, "{counters:?}");
    }
}

#[test]
fn clients_on_fast_and_slow_clocks_hold_their_lead() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(40.0, 5.0, 0.0);
    for skew in [1.02, 0.98, 1.0] {
        let p = world.join(ToyClient::factory(), patrol(), &net);
        world.players[p].clock_skew = skew;
    }
    // Let the lead settle, then hold it to account.
    world.run(10.0);
    let settled: Vec<_> = world.players.iter().map(|p| world.server.counters(p.id).unwrap()).collect();
    world.run(60.0);
    for (player, before) in world.players.iter().zip(settled) {
        let now = world.server.counters(player.id).unwrap();
        assert_eq!(now.cmds_late, before.cmds_late, "skew {}", player.clock_skew);
        assert_eq!(now.cmds_repeated, before.cmds_repeated, "skew {}", player.clock_skew);
        let lead = player.session.predictor().unwrap().tick() as i64 - world.server.game.tick() as i64;
        // 40 ms is about 2.5 ticks, plus margin. A clock that ran away would be far past this.
        assert!((3..=14).contains(&lead), "lead {lead} at skew {}", player.clock_skew);
    }
}

#[test]
fn a_misprediction_is_fixed_once_and_never_seen_as_a_pop() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    let p = world.join(ToyClient::factory(), patrol(), &net);
    world.run(5.0);
    assert_eq!(world.players[p].reconciles(), 0);

    // Something the client could not have known: an explosion, a vehicle. Half a metre sideways.
    let id = world.player_id(p);
    world.server.game.player_mut(id).pos += Vec3::new(0.5, 0.0, 0.0);
    let from = world.players[p].frames.len();
    world.run(1.0);

    let player = &world.players[p];
    let stats = player.session.predictor().unwrap().stats();
    assert_eq!(stats.reconciles, 1, "the one correction fixes all later ticks too: {stats:?}");
    assert!((stats.max_correction - 0.5).abs() < 0.01, "{stats:?}");

    // Never a jump on screen: the fastest it moves is walking plus a share of the correction.
    let frames = &player.frames[from..];
    let worst = frames.windows(2).map(|w| w[0].view.distance(w[1].view)).fold(0.0, f32::max);
    let share = 1.0 - (-12.0f32 * TICK_DT).exp();
    assert!(worst < WALK_STEP + 0.5 * share + 0.01, "worst frame step {worst}");

    // And it arrives: a second later the picture is back on the player.
    let offset = player.session.predictor().unwrap().visual_offset().length();
    assert!(offset < 0.01, "{offset}");

    world.run(1.0);
    assert_eq!(world.players[p].reconciles(), 1, "and stays right");
}

/// A command the server never got leaves private state different as well. Once the client has
/// been told, it must agree again rather than disagree on every tick from then on.
#[test]
fn losing_commands_is_put_right_for_good() {
    let mut world = toy_world(ServerConfig::default());
    let good = Net::datagram(50.0, 0.0, 0.0);
    let p = world.join(ToyClient::factory(), busy(), &good);
    world.run(5.0);

    // Every command for a quarter of a second vanishes on the way up.
    let up = world.players[p].socket.tx.clone();
    up.borrow_mut().set_config(LinkConfig { loss: 1.0, ..good.up });
    world.run(0.25);
    up.borrow_mut().set_config(good.up);
    world.run(1.0);
    let repeats = world.server.counters(world.players[p].id).unwrap().cmds_repeated;
    assert!(repeats > 0, "the outage really cost commands");
    let settled = world.players[p].reconciles();

    world.run(10.0);
    assert_eq!(world.players[p].reconciles(), settled, "no disagreement after it is over");
}

#[test]
fn under_loss_prediction_stays_bounded_and_nothing_piles_up() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.05);
    for _ in 0..4 {
        world.join(ToyClient::factory(), patrol(), &net);
    }
    world.run(60.0);
    for player in &world.players {
        let stats = player.session.predictor().unwrap().stats();
        let client = player.session.stats();
        assert_eq!(client.bad_frames, 0);
        assert_eq!(client.missing_baselines, 0, "acked baselines survive loss");
        assert!(stats.max_correction < 0.5, "{stats:?}");
        let offset = player.session.predictor().unwrap().visual_offset().length();
        assert!(offset < 0.2, "visual offset {offset}");
    }
}

#[test]
fn an_outage_recovers() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 5.0, 0.0);
    let mover = world.join(ToyClient::factory(), patrol(), &net);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(5.0);

    let dead = LinkConfig { latency_ms: 50.0, jitter_ms: 5.0, loss: 1.0, mode: LinkMode::Datagram };
    let set = |world: &ToyWorld, cfg: LinkConfig| {
        world.players[mover].socket.tx.borrow_mut().set_config(cfg);
        world.players[mover].socket.rx.borrow_mut().set_config(cfg);
    };
    set(&world, dead);
    world.run(2.0);
    set(&world, LinkConfig { loss: 0.0, ..dead });
    world.run(5.0);

    let id = world.player_id(mover);
    let predictor = world.players[mover].session.predictor().unwrap();
    let lead_ticks = predictor.tick() - world.server.game.tick();
    let allowed = WALK_STEP * (lead_ticks as f32 + 2.0);
    let apart = world.server.game.position_of(id).distance(predictor.game.me.pos);
    assert!(apart < allowed, "{apart} m apart, lead allows {allowed}");
    assert!(lead_ticks < 20, "the lead came back down: {lead_ticks}");
    assert_eq!(world.players[watcher].session.remotes().len(), 1);
}

#[test]
fn remote_players_stay_smooth_and_accurate_at_100ms_with_jitter() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(100.0, 25.0, 0.0);
    let mover = world.join(ToyClient::factory(), patrol(), &net);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(30.0);

    let id = world.player_id(mover);
    let frames = &world.players[watcher].frames;
    let mut worst_error = 0.0f32;
    let mut worst_step = 0.0f32;
    let mut last = None;
    for frame in &frames[frames.len() / 4..] {
        let Some(&(_, at)) = frame.remotes.iter().find(|r| r.0 == id as u32) else { continue };
        worst_error = worst_error.max(at.distance(world.truth_at(id, frame.play_tick).expect("recorded")));
        if let Some(previous) = last {
            worst_step = worst_step.max(at.distance(previous));
        }
        last = Some(at);
    }
    let interp = world.players[watcher].session.interp().unwrap().stats();
    assert_eq!(interp.starved_frames, 0, "jitter is within the 100 ms delay: {interp:?}");
    assert_eq!(interp.snaps, 0, "{interp:?}");
    assert!(worst_error < 0.03, "off the true path by {worst_error} m");
    assert!(worst_step < WALK_STEP * 1.15, "a frame moved {worst_step} m, true is {WALK_STEP}");
}

/// A tab out of sight gets no frames while snapshots pile up, then comes back with one long frame. The
/// clock used to jump at every report from before its first jump, and swung by thousands of ticks for
/// minutes.
#[test]
fn coming_back_to_a_hidden_tab_is_steady_within_a_second() {
    for (away, net) in [(30.0, Net::tcp(40.0, 5.0, 0.0, 200.0)), (120.0, Net::datagram(40.0, 5.0, 0.0))] {
        let mut world = toy_world(ServerConfig::default());
        let mover = world.join(ToyClient::factory(), patrol(), &net);
        world.join(ToyClient::factory(), idle(), &net);
        world.run(5.0);
        let lead_of = |world: &ToyWorld| world.players[mover].session.predictor().unwrap().tick() as i64 - world.server.game.tick() as i64;
        let usual = lead_of(&world);
        let now = world.players[mover].socket.clock_ms.get();
        world.players[mover].hidden = Some(Hidden { until_ms: now + away * 1000.0, frame_every_ms: None });
        world.run(away + 1.0);
        let reconciles = world.players[mover].reconciles();
        for _ in 0..40 {
            world.run(0.1);
            assert!((lead_of(&world) - usual).abs() <= 2, "lead {} against {usual}, {away} s away", lead_of(&world));
        }
        assert_eq!(world.players[mover].reconciles(), reconciles, "{away} s away");
    }
}

/// Frames this slow look like coming back from away each time, but a machine that is always this slow
/// is still playing, and has to send its commands.
#[test]
fn a_machine_that_is_always_slow_is_not_taken_for_away() {
    let mut world = toy_world(ServerConfig::default());
    let slow = world.join(ToyClient::factory(), patrol(), &Net::tcp(40.0, 0.0, 0.0, 200.0));
    world.players[slow].frame_ms = 2500.0;
    world.run(20.0);
    let session = &world.players[slow].session;
    assert_eq!(
        world.server.player_count(),
        1,
        "its commands reached the server: {:?} {:?} {:?}",
        session.state(),
        session.stats(),
        session.clock().map(|c| (c.rtt_ms(), c.jumps())),
    );
    assert!(world.players[slow].session.clock().is_some_and(|c| c.rtt_ms() < 5000.0));
}

#[test]
fn a_long_frame_does_not_leave_everyone_else_stuck() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    world.join(ToyClient::factory(), patrol(), &net);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(3.0);
    world.players[watcher].session.draw(0.5);
    world.run(1.0);
    let before = world.players[watcher].session.interp().unwrap().stats().starved_frames;
    world.run(1.0);
    let after = world.players[watcher].session.interp().unwrap().stats().starved_frames;
    assert_eq!(after, before, "a second on, it is drawing from real snapshots again");
}

#[test]
fn every_shot_is_heard_once_even_over_a_lossy_link() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.10);
    let shooter = world.join(ToyClient::factory(), busy(), &net);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(30.0);
    let fired = world.server.game.shots[world.player_id(shooter) as usize] as usize;
    assert!(fired > 5, "fired {fired}");
    let heard = world.players[watcher].server_events.len();
    // The last few might still be in flight.
    assert!(heard <= fired && heard + 3 >= fired, "fired {fired}, heard {heard}");
    assert!(world.players[shooter].server_events.is_empty(), "your own shots are predicted, not echoed");
}

#[test]
fn players_come_and_go() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::tcp(20.0, 0.0, 0.0, 200.0);
    let a = world.join(ToyClient::factory(), patrol(), &net);
    let b = world.join(ToyClient::factory(), patrol(), &net);
    let c = world.join(ToyClient::factory(), idle(), &net);
    world.run(3.0);
    assert_eq!(world.players[c].session.remotes().len(), 2);

    let gone = world.players[b].id;
    let slot = world.server.player_of(gone).unwrap();
    world.server.disconnect(gone);
    world.run(1.0);
    assert_eq!(world.players[c].session.remotes().len(), 1, "someone left");
    assert_eq!(world.server.player_count(), 2);

    let d = world.join(ToyClient::factory(), patrol(), &net);
    world.run(1.0);
    assert_eq!(world.server.player_of(world.players[d].id), Some(slot), "the slot is reused");
    assert_eq!(world.players[c].session.remotes().len(), 2);
    assert_eq!(world.players[a].session.state(), ClientState::Playing);
    assert_eq!(world.players[d].reconciles(), 0, "a reused slot starts exactly fresh");
}

#[test]
fn a_backlogged_client_skips_snapshots_and_recovers() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::tcp(30.0, 0.0, 0.0, 200.0);
    let mover = world.join(ToyClient::factory(), patrol(), &net);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(3.0);
    let id = world.players[watcher].id;
    world.server.set_backlogged(id, true);
    world.run(1.0);
    world.server.set_backlogged(id, false);
    world.run(1.0);
    let session = &world.players[watcher].session;
    assert_eq!(session.stats().missing_baselines, 0);
    let truth = world.truth_at(world.player_id(mover), session.interp().unwrap().play_tick()).unwrap();
    let drawn = session.remotes()[0].1.pos;
    assert!(drawn.distance(truth) < 0.1, "{drawn} vs {truth}");
}

// ---- Cheats. These talk to the server directly: a hacked client doesn't use our client.

struct Attacker {
    server: GameServer<ToyServer>,
    id: ClientId,
    player: u16,
    start: Vec3,
}

fn walk(tick: u32) -> PlayerCmd {
    PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), ..Default::default() }
}

impl Attacker {
    fn new(cfg: ServerConfig) -> Self {
        let mut server = GameServer::new(ToyServer::new(), cfg);
        let id = server.connect(true);
        let rules = server.game.rules_hash();
        server.receive(id, &ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode());
        server.receive(id, &ClientMsg::Join { rules, name: String::new() }.encode());
        let player = server.player_of(id).unwrap();
        let start = server.game.position_of(player);
        Self { server, id, player, start }
    }

    fn next(&self) -> u32 {
        self.server.game.tick() + 1
    }

    fn send(&mut self, cmds: Vec<PlayerCmd>) {
        let frame = ClientMsg::Cmds(CmdPacket { cmds, ..Default::default() }).encode();
        self.server.receive(self.id, &frame);
    }

    fn distance(&self) -> f32 {
        self.server.game.position_of(self.player).distance(self.start)
    }

    fn toy(&self) -> &Toy {
        self.server.game.player(self.player)
    }

    /// How far an honest client gets in `ticks`.
    fn honest_distance(ticks: u32) -> f32 {
        let mut honest = Attacker::new(ServerConfig::default());
        for _ in 0..ticks {
            honest.send(vec![walk(honest.next())]);
            honest.server.tick();
        }
        honest.distance()
    }
}

const WINDOW_TICKS: u32 = 120;

#[test]
fn a_huge_move_goes_no_faster_than_an_honest_one() {
    let honest = Attacker::honest_distance(WINDOW_TICKS);
    let mut cheat = Attacker::new(ServerConfig::default());
    for _ in 0..WINDOW_TICKS {
        let cmd = PlayerCmd { wish_move: Vec2::new(0.0, 1e9), ..walk(cheat.next()) };
        cheat.send(vec![cmd]);
        cheat.server.tick();
    }
    let went = cheat.distance();
    assert!((went - honest).abs() < honest * 0.002, "cheat {went}, honest {honest}");
}

#[test]
fn sending_commands_faster_gains_nothing() {
    // Speed hack: a client clock at five times real speed, sending every command it makes.
    let honest = Attacker::honest_distance(WINDOW_TICKS);
    let mut cheat = Attacker::new(ServerConfig::default());
    let mut fake_tick = cheat.server.game.tick();
    for _ in 0..WINDOW_TICKS {
        let cmds = (0..5)
            .map(|_| {
                fake_tick += 1;
                walk(fake_tick)
            })
            .collect();
        cheat.send(cmds);
        cheat.server.tick();
    }
    let counters = cheat.server.counters(cheat.id).unwrap();
    assert!(cheat.distance() <= honest * 1.002, "cheat {}, honest {honest}", cheat.distance());
    assert!(counters.cmds_future > 0, "the fake clock ran out of the window: {counters:?}");
}

#[test]
fn replaying_old_ticks_gains_nothing() {
    let honest = Attacker::honest_distance(WINDOW_TICKS);
    let mut cheat = Attacker::new(ServerConfig::default());
    for _ in 0..WINDOW_TICKS {
        let tick = cheat.next();
        let cmds = (0..40).rev().map(|back| walk(tick.saturating_sub(back).max(1))).collect();
        cheat.send(cmds);
        cheat.server.tick();
    }
    assert!(cheat.distance() <= honest * 1.002, "cheat {}, honest {honest}", cheat.distance());
}

#[test]
fn commands_from_the_future_are_refused() {
    let limits = CmdLimits { max_future_ticks: 30, ..Default::default() };
    let mut cheat = Attacker::new(ServerConfig { limits, ..Default::default() });
    let tick = cheat.next() + 1000;
    cheat.send(vec![walk(tick)]);
    for _ in 0..120 {
        cheat.server.tick();
    }
    assert_eq!(cheat.server.counters(cheat.id).unwrap().cmds_future, 1);
    assert_eq!(cheat.distance(), 0.0);
}

#[test]
fn extreme_and_broken_aim_cannot_break_a_player() {
    let mut cheat = Attacker::new(ServerConfig::default());
    let max = cheat.server.config().limits.max_turn_per_tick;
    for (i, yaw) in [1e30f32, -1e30, f32::MAX, f32::NAN, f32::INFINITY, 1e12].into_iter().enumerate() {
        let before = cheat.toy().yaw;
        let pitch = if i % 2 == 0 { 1e30 } else { f32::NAN };
        let cmd = PlayerCmd { yaw, pitch, ..walk(cheat.next()) };
        cheat.send(vec![cmd]);
        cheat.server.tick();
        let toy = cheat.toy();
        assert!(toy.yaw.is_finite() && toy.pitch.is_finite(), "{}, {}", toy.yaw, toy.pitch);
        assert!((toy.yaw - before).abs() <= max + 1e-3, "turned {}", toy.yaw - before);
        assert!(toy.pos.is_finite());
    }
    let counters = cheat.server.counters(cheat.id).unwrap();
    assert!(counters.turns_clamped > 0, "{counters:?}");
    assert!(counters.cmds_not_finite > 0, "{counters:?}");
}

#[test]
fn a_fast_trigger_fires_no_faster_than_the_gun() {
    let mut cheat = Attacker::new(ServerConfig::default());
    for _ in 0..600 {
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, true);
        let cmd = PlayerCmd { buttons, ..walk(cheat.next()) };
        let copies = vec![cmd; 8];
        cheat.send(copies);
        cheat.server.tick();
    }
    let shots = cheat.server.game.shots[cheat.player as usize];
    // 10 seconds at one shot per 0.1 s, but only a magazine's worth.
    assert_eq!(shots, 30, "the server counts the rounds");
}

#[test]
fn flooding_with_packets_and_garbage_does_nothing() {
    let limits = CmdLimits { max_packets_per_second: 240, ..Default::default() };
    let mut cheat = Attacker::new(ServerConfig { limits, ..Default::default() });
    for i in 0..5000u32 {
        cheat.server.receive(cheat.id, &i.to_le_bytes());
    }
    let tick = cheat.next();
    for _ in 0..5000 {
        cheat.send(vec![walk(tick)]);
    }
    cheat.server.tick();
    let counters = cheat.server.counters(cheat.id).unwrap();
    assert!(counters.bad_frames > 4000, "{counters:?}");
    assert!(counters.packets_over_rate > 4000, "{counters:?}");
    assert!(cheat.distance() < WALK_STEP * 2.0, "one tick's worth at most");
}

struct NoBaselines;
impl Baselines<ToySchema> for NoBaselines {
    fn base_at(&self, _: u32) -> Option<BaseRef<'_, ToySchema>> {
        None
    }

    fn entity_at(&self, _: u32, _: u32) -> Option<[i32; MAX_FIELDS]> {
        None
    }
}

fn is_reject(frame: &[u8], reason: RejectReason) -> bool {
    matches!(ServerMsg::<ToySchema>::decode(frame, &NoBaselines), Ok(ServerMsg::Reject(r, _)) if r == reason)
}

#[test]
fn a_client_that_keeps_breaking_the_rules_is_dropped() {
    let limits = CmdLimits { kick_after_rejects_per_second: 10, ..Default::default() };
    let mut cheat = Attacker::new(ServerConfig { limits, ..Default::default() });
    for _ in 0..20 {
        let cmd = PlayerCmd { yaw: f32::NAN, ..walk(cheat.next()) };
        cheat.send(vec![cmd]);
    }
    assert_eq!(cheat.server.drain_kicked(), vec![cheat.id]);
    assert!(cheat.server.drain_outgoing().iter().any(|(_, f)| is_reject(f, RejectReason::Kicked)));
}

#[test]
fn clients_on_other_rules_or_versions_or_when_full_are_turned_away() {
    let cfg = ServerConfig { max_players: 1, ..Default::default() };
    let mut server = GameServer::new(ToyServer::new(), cfg);
    let rules = server.game.rules_hash();
    let reply = |server: &mut GameServer<ToyServer>, frame: Vec<u8>| {
        let id = server.connect(true);
        server.receive(id, &frame);
        server.drain_outgoing().pop().unwrap().1
    };
    let hello = ClientMsg::Hello { protocol: PROTOCOL_VERSION + 1 }.encode();
    assert!(is_reject(&reply(&mut server, hello), RejectReason::Protocol));
    assert!(is_reject(&reply(&mut server, ClientMsg::Join { rules: rules ^ 1, name: String::new() }.encode()), RejectReason::Rules));
    let ok = reply(&mut server, ClientMsg::Join { rules, name: String::new() }.encode());
    assert!(matches!(ServerMsg::<ToySchema>::decode(&ok, &NoBaselines), Ok(ServerMsg::Welcome(_))));
    assert!(is_reject(&reply(&mut server, ClientMsg::Join { rules, name: String::new() }.encode()), RejectReason::Full));
}

fn handshake(session: &mut ClientSession<ToyClient>, server: &mut GameServer<ToyServer>) {
    let id = server.connect(true);
    for _ in 0..3 {
        for frame in session.drain_outgoing() {
            server.receive(id, &frame);
        }
        for (_, frame) in server.drain_outgoing() {
            session.receive(&frame);
        }
    }
}

#[test]
fn a_client_with_different_rules_is_told_so() {
    let mut session = ClientSession::new(ToyClient::with_other_rules(), ClientConfig::default());
    let mut server = GameServer::new(ToyServer::new(), ServerConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(session.state(), ClientState::Rejected(RejectReason::Rules));
}

#[test]
fn a_client_is_told_which_map_and_can_say_it_does_not_have_it() {
    let map = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let told = map.clone();
    let factory = Box::new(move |name: &str, mode: &str| {
        *told.borrow_mut() = format!("{name} {mode}");
        Some(ToyClient::new())
    });
    let mut session = ClientSession::new(factory, ClientConfig::default());
    let mut server = GameServer::new(ToyServer::new(), ServerConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(*map.borrow(), "toyland toymode");
    assert_eq!(session.state(), ClientState::Playing);

    let mut session = ClientSession::new(Box::new(|_: &str, _: &str| None::<ToyClient>), ClientConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(session.state(), ClientState::Rejected(RejectReason::UnknownMap));
    assert_eq!(server.game.players.iter().flatten().count(), 1, "and it never took a place in the game");
}

#[test]
fn rarely_changing_world_state_reaches_everyone_and_costs_nothing_otherwise() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(30.0, 5.0, 0.05);
    let p = world.join(ToyClient::factory(), idle(), &net);
    world.run(2.0);
    assert_eq!(world.players[p].session.game().unwrap().shared, vec![1, 2, 3]);
    let quiet = world.players[p].session.stats();
    world.run(2.0);
    let later = world.players[p].session.stats();
    let per_frame = (later.bytes_down - quiet.bytes_down) as f64 / (later.frames_down - quiet.frames_down) as f64;
    world.server.game.shared = vec![9; 500];
    world.run(0.5);
    assert_eq!(world.players[p].session.game().unwrap().shared, vec![9; 500]);
    assert!(per_frame < 100.0, "it was not being resent: {per_frame} bytes a frame");
}

#[test]
fn requests_of_the_game_arrive_once_and_in_order_over_a_lossy_link() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(30.0, 5.0, 0.3);
    let p = world.join(ToyClient::factory(), idle(), &net);
    world.run(3.0);
    for i in 0..20u8 {
        world.players[p].session.send_request(vec![i]);
        world.run(0.05);
    }
    world.run(3.0);
    let got: Vec<u8> = world.server.game.requests.iter().map(|(_, bytes)| bytes[0]).collect();
    assert_eq!(got, (0..20).collect::<Vec<u8>>());
}

#[test]
fn a_shooter_reports_how_far_in_the_past_it_sees() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::datagram(100.0, 0.0, 0.0);
    let p = world.join(ToyClient::factory(), patrol(), &net);
    world.join(ToyClient::factory(), idle(), &net);
    world.run(5.0);
    let lag = world.server.game.view_lag[world.player_id(p) as usize];
    // 100 ms each way is 12 ticks, the view is 6 to 7 behind what has arrived, and the lead has a margin.
    assert!((16..=28).contains(&lag), "{lag} ticks");
}

#[test]
fn nobody_can_ask_for_more_rewind_than_the_server_allows() {
    let cfg = ServerConfig { max_rewind_ticks: 10, ..Default::default() };
    let mut cheat = Attacker::new(cfg);
    let frame = ClientMsg::Cmds(CmdPacket {
        view_tick: 1,
        cmds: vec![walk(cheat.next() + 500)],
        ..Default::default()
    });
    cheat.server.receive(cheat.id, &frame.encode());
    for _ in 0..100 {
        let tick = cheat.next();
        let frame = ClientMsg::Cmds(CmdPacket { view_tick: 1, cmds: vec![walk(tick)], ..Default::default() });
        cheat.server.receive(cheat.id, &frame.encode());
        cheat.server.tick();
    }
    assert_eq!(cheat.server.game.view_lag[cheat.player as usize], 10);
}

/// A browser that stalls just as the answer to joining arrives thinks the round trip took that long.
#[test]
fn a_long_stall_while_joining_does_not_leave_a_player_stuck_outside_the_game() {
    let mut server = GameServer::new(ToyServer::new(), ServerConfig::default());
    let id = server.connect(true);
    let mut session = ClientSession::new(ToyClient::factory(), ClientConfig::default());
    let script = patrol();
    let pass = |session: &mut ClientSession<ToyClient>, server: &mut GameServer<ToyServer>| {
        for frame in session.drain_outgoing() {
            server.receive(id, &frame);
        }
        for (_, frame) in server.drain_outgoing() {
            session.receive(&frame);
        }
    };
    pass(&mut session, &mut server);
    // The Join is out, and then nothing happens for half a minute before its answer is read.
    for frame in session.drain_outgoing() {
        server.receive(id, &frame);
    }
    session.update(30.0, |tick| script(0, tick));
    session.drain_outgoing();
    for (_, frame) in server.drain_outgoing() {
        session.receive(&frame);
    }
    assert_eq!(session.state(), ClientState::Playing);
    assert_eq!(server.player_count(), 0);
    for _ in 0..180 {
        session.update(TICK_DT, |tick| script(0, tick));
        pass(&mut session, &mut server);
        server.tick();
        pass(&mut session, &mut server);
    }
    assert_eq!(server.player_count(), 1, "their commands got through: {:?}", server.counters(id));
    let lead = session.predictor().unwrap().tick() as i64 - server.game.tick() as i64;
    assert!((0..=20).contains(&lead), "and the clock came right: {lead}");
}

// ---- Who gets sent what.

fn spread_world(cfg: ServerConfig, players: usize, spacing: f32) -> ToyWorld {
    let mut world = toy_world(cfg);
    let net = Net::tcp(30.0, 0.0, 0.0, 200.0);
    for _ in 0..players {
        world.join(ToyClient::factory(), patrol(), &net);
    }
    world.run(0.5);
    for p in 0..players {
        let id = world.player_id(p);
        world.server.game.player_mut(id).pos = Vec3::new(p as f32 * spacing, 0.0, 0.0);
    }
    world
}

fn down_rate(world: &mut ToyWorld, p: usize, seconds: f64) -> f64 {
    let before = world.players[p].session.stats().bytes_down;
    world.run(seconds);
    (world.players[p].session.stats().bytes_down - before) as f64 / seconds
}

#[test]
fn something_that_is_not_changing_costs_nothing_and_is_still_there() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::tcp(30.0, 0.0, 0.0, 200.0);
    let watcher = world.join(ToyClient::factory(), idle(), &net);
    world.run(1.0);
    let mover = world.join(ToyClient::factory(), idle(), &net);
    world.run(3.0);
    let quiet = down_rate(&mut world, watcher, 2.0);
    assert_eq!(world.players[watcher].session.remotes().len(), 1, "still there");
    // An idle toy changes only its wobble, and that is all that should be going.
    assert!(quiet < 2500.0, "{quiet} B/s for one idle player");
    let _ = mover;
}

#[test]
fn far_things_are_sent_less_often_and_still_drawn_near_the_truth() {
    let cfg = ServerConfig { interest: InterestConfig { full_rate_within: 50.0, always_within: 10.0, ..Default::default() }, ..Default::default() };
    let mut near = spread_world(cfg.clone(), 8, 5.0);
    let mut far = spread_world(cfg, 8, 200.0);
    near.run(2.0);
    far.run(2.0);
    let near_rate = down_rate(&mut near, 0, 5.0);
    let far_rate = down_rate(&mut far, 0, 5.0);
    println!("8 players, seen by the first: {near_rate:.0} B/s close together, {far_rate:.0} B/s spread out");
    assert!(far_rate < near_rate * 0.7, "{far_rate} against {near_rate}");

    // Slower but not wrong: the furthest is still drawn, on its true path.
    let farthest = far.player_id(7);
    let frames = &far.players[0].frames;
    let tail = &frames[frames.len() - 60..];
    let worst = tail
        .iter()
        .filter_map(|f| {
            let at = f.remotes.iter().find(|r| r.0 == farthest as u32)?.1;
            let error = (0..30).filter_map(|back| far.truth_at(farthest, f.play_tick - back as f64)).map(|t| t.distance(at)).fold(f32::MAX, f32::min);
            Some(error)
        })
        .fold(0.0, f32::max);
    assert!(worst < 0.5, "drawn {worst} m off anywhere it has really been");
}

#[test]
fn something_in_the_sights_goes_at_full_rate_however_far() {
    let cfg = ServerConfig { interest: InterestConfig { full_rate_within: 10.0, always_within: 1.0, slowest_every: 10.0, aim_cone: 0.99, ..Default::default() }, ..Default::default() };
    let mut world = toy_world(cfg);
    let net = Net::tcp(30.0, 0.0, 0.0, 200.0);
    let looker = world.join(ToyClient::factory(), idle(), &net);
    let target = world.join(ToyClient::factory(), patrol(), &net);
    world.run(0.5);
    let (a, b) = (world.player_id(looker), world.player_id(target));
    world.server.game.player_mut(a).pos = Vec3::ZERO;
    // Idle looks down -Z, so 300 m down -Z is dead centre and 300 m along +X is not.
    world.server.game.player_mut(b).pos = Vec3::new(0.0, 0.0, -300.0);
    world.server.game.player_mut(b).vel = Vec3::ZERO;
    world.run(0.3);
    let aimed = down_rate(&mut world, looker, 0.1);
    world.server.game.player_mut(b).pos = Vec3::new(300.0, 0.0, 0.0);
    world.run(0.3);
    let aside = down_rate(&mut world, looker, 0.1);
    println!("300 m away: {aimed:.0} B/s in the sights, {aside:.0} B/s off to the side");
    assert!(aimed > aside, "{aimed} against {aside}");
}

#[test]
fn the_budget_holds_and_nothing_starves() {
    let budget = 150;
    let cfg = ServerConfig { interest: InterestConfig { snapshot_budget: budget, always_within: 0.0, full_rate_within: 10_000.0, ..Default::default() }, ..Default::default() };
    let mut world = spread_world(cfg, 32, 3.0);
    world.run(1.0);
    let watcher = 0;
    let (frames_before, bytes_before) = {
        let stats = world.players[watcher].session.stats();
        (stats.frames_down, stats.bytes_down)
    };
    world.run(5.0);
    let stats = world.players[watcher].session.stats();
    let per_snapshot = (stats.bytes_down - bytes_before) as f64 / (stats.frames_down - frames_before) as f64;
    println!("32 close together on a budget of {budget}: {per_snapshot:.0} B a snapshot");
    // The budget is for entities. Their own state and the headers come on top of it, plus one overshoot.
    assert!(per_snapshot < budget as f64 + 120.0, "{per_snapshot}");
    assert_eq!(world.players[watcher].session.remotes().len(), 31, "everyone is known");
    // And everybody got a turn: all are near where the server has them.
    let frame = world.players[watcher].frames.last().unwrap();
    let worst = frame
        .remotes
        .iter()
        .filter_map(|(key, at)| {
            let truth = world.server.game.position_of(*key as u16);
            Some(truth.distance(*at))
        })
        .fold(0.0, f32::max);
    assert!(worst < 5.0, "someone has been left {worst} m behind");
}

#[test]
fn out_of_range_is_forgotten_and_coming_back_needs_no_baseline() {
    let mut world = toy_world(ServerConfig::default());
    world.server.game.max_range = 100.0;
    let net = Net::datagram(30.0, 5.0, 0.1);
    let a = world.join(ToyClient::factory(), idle(), &net);
    let b = world.join(ToyClient::factory(), patrol(), &net);
    world.run(2.0);
    assert_eq!(world.players[a].session.remotes().len(), 1);
    let id = world.player_id(b);
    world.server.game.player_mut(id).pos = Vec3::new(5000.0, 0.0, 0.0);
    world.run(1.0);
    assert_eq!(world.players[a].session.remotes().len(), 0, "forgotten");
    world.server.game.player_mut(id).pos = Vec3::new(10.0, 0.0, 0.0);
    world.run(1.0);
    assert_eq!(world.players[a].session.remotes().len(), 1, "back");
    assert_eq!(world.players[a].session.stats().bad_frames, 0);
}

#[test]
fn a_shot_too_far_off_to_matter_is_not_sent() {
    let mut world = toy_world(ServerConfig::default());
    let net = Net::tcp(30.0, 0.0, 0.0, 200.0);
    let shooter = world.join(ToyClient::factory(), busy(), &net);
    let near = world.join(ToyClient::factory(), idle(), &net);
    let far = world.join(ToyClient::factory(), idle(), &net);
    world.run(0.5);
    let id = world.player_id(far);
    world.server.game.player_mut(id).pos = Vec3::new(TOY_SHOT_REACH * 3.0, 0.0, 0.0);
    world.run(15.0);
    assert!(world.server.game.shots[world.player_id(shooter) as usize] > 0);
    assert!(!world.players[near].server_events.is_empty());
    assert!(world.players[far].server_events.is_empty(), "they would not have heard it");
}

// ---- Measurements.

#[test]
#[ignore = "report"]
fn report_transport_cost() {
    println!("link                      | lead ticks | repeats/s | reconciles/s | max corr m | starved % | worst remote err m");
    let cases: Vec<(&str, Net)> = vec![
        ("datagram 0%", Net::datagram(50.0, 5.0, 0.0)),
        ("datagram 1%", Net::datagram(50.0, 5.0, 0.01)),
        ("datagram 5%", Net::datagram(50.0, 5.0, 0.05)),
        ("tcp 0%", Net::tcp(50.0, 5.0, 0.0, 200.0)),
        ("tcp 0.1% rto 200", Net::tcp(50.0, 5.0, 0.001, 200.0)),
        ("tcp 1% rto 200", Net::tcp(50.0, 5.0, 0.01, 200.0)),
        ("tcp 5% rto 200", Net::tcp(50.0, 5.0, 0.05, 200.0)),
        ("tcp 1% rexmit 120", Net::tcp(50.0, 5.0, 0.01, 120.0)),
    ];
    for (name, net) in cases {
        let mut world = toy_world(ServerConfig::default());
        let mover = world.join(ToyClient::factory(), patrol(), &net);
        let watcher = world.join(ToyClient::factory(), idle(), &net);
        world.run(5.0);
        let frames_from = world.players[watcher].frames.len();
        let starved_before = world.players[watcher].session.interp().unwrap().stats().starved_frames;
        let reconciles_before = world.players[mover].reconciles();
        let repeats_before = world.server.counters(world.players[mover].id).unwrap().cmds_repeated;
        let seconds = 60.0;
        world.run(seconds);
        let id = world.player_id(mover);
        let player = &world.players[mover];
        let predictor = player.session.predictor().unwrap();
        let lead = predictor.tick() as i64 - world.server.game.tick() as i64;
        let repeats = world.server.counters(player.id).unwrap().cmds_repeated - repeats_before;
        let reconciles = player.reconciles() - reconciles_before;
        let w = &world.players[watcher];
        let starved = w.session.interp().unwrap().stats().starved_frames - starved_before;
        let frames = &w.frames[frames_from..];
        let worst = frames
            .iter()
            .filter_map(|f| {
                let r = f.remotes.iter().find(|r| r.0 == id as u32)?;
                Some(r.1.distance(world.truth_at(id, f.play_tick)?))
            })
            .fold(0.0, f32::max);
        println!(
            "{name:<25} | {lead:>10} | {:>9.2} | {:>12.2} | {:>10.3} | {:>8.2}% | {worst:>8.3}",
            repeats as f64 / seconds,
            reconciles as f64 / seconds,
            predictor.stats().max_correction,
            starved as f64 * 100.0 / frames.len() as f64,
        );
    }
}
