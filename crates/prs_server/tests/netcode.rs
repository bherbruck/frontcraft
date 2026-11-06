//! The proof that the netcode works with the real game: a real server sim and real client sims on a
//! simulated network with a virtual clock, so these are exactly repeatable. `prs_net`'s tests cover
//! the same loop with a toy; these are about what only the real simulation can get wrong.
//!
//! The reports print their numbers:
//! `cargo test --release -p prs_server --test netcode report -- --ignored --nocapture --test-threads=1`

use glam::{Vec2, Vec3};
use prs_net::client::{ClientConfig, ClientSession, ClientState, GameFactory};
use prs_net::game::ServerGame;
use prs_net::server::{GameServer, InterestConfig, ServerConfig};
use prs_net::testkit::{Hidden, Net, Observer, World};
use prs_net::wire::{ClientMsg, CmdPacket, PROTOCOL_VERSION, RejectReason};
use prs_mode::{Mode, ModeCmd, ModeEvent};
use prs_net_sim::mode::encode_cmd;
use prs_net_sim::{AnyPose, SimClient, SimServer, SimServerConfig, WireEvent};
use prs_server::bots::{BotScript, bot_cmd};
use prs_sim::health::Life;
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::{Level, Sim, SimEvent};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

const MAP: &str = "range";

type Game = World<SimClient, SimServer>;

fn factory() -> GameFactory<SimClient> {
    Box::new(|map: &str, mode: &str| SimClient::setup(map, mode))
}

fn server_game() -> SimServer {
    SimServer::new(MAP, "practice", SimServerConfig::default()).expect("the map exists")
}

fn position_of(server: &SimServer, id: u16) -> Vec3 {
    server.sim.soldiers[id as usize].body.pos
}

fn world(cfg: ServerConfig) -> Game {
    World::new(server_game(), cfg, position_of)
}

fn script(s: BotScript) -> Rc<dyn Fn(u32, u32) -> PlayerCmd> {
    Rc::new(move |who, tick| bot_cmd(s, who, tick))
}

fn stats(world: &Game, p: usize) -> prs_net::predict::PredictStats {
    world.players[p].session.predictor().expect("joined").stats()
}

/// Nothing is shooting past anyone here. A near miss raises suppression and a hit hurts, which is news
/// a client could not have predicted, so they do reconcile, and rightly.
#[test]
fn prediction_agrees_to_the_bit_when_nothing_disagrees() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    world.join(factory(), script(BotScript::Busy), &net);
    world.run(30.0);
    for (p, player) in world.players.iter().enumerate() {
        let stats = stats(&world, p);
        let counters = world.server.counters(player.id).unwrap();
        assert!(stats.compared > 800, "compared {}", stats.compared);
        assert_eq!(stats.reconciles, 0, "no reconciliation at 0 loss: {stats:?}");
        assert_eq!(stats.max_error, 0.0, "identical to the bit: {stats:?}");
        assert_eq!(counters.cmds_repeated, 0, "{counters:?}");
        let fired = player.predicted_events.iter().filter(|e| matches!(e, SimEvent::ShotFired { .. })).count();
        assert!(fired > 0, "that included recoil, sprinting, stance and reloads");
    }
}

#[test]
fn prediction_agrees_for_several_players_who_are_not_shooting() {
    // Far enough apart never to touch: bumping into someone seen in the past is a real disagreement,
    // and `prs_net_sim`'s `climb_tests` measure that one.
    let spread = SimServerConfig { spawn_spacing: 30.0, ..SimServerConfig::default() };
    let game = SimServer::new(MAP, "practice", spread).expect("the map exists");
    let mut world: Game = World::new(game, ServerConfig::default(), position_of);
    let net = Net::datagram(50.0, 0.0, 0.0);
    for _ in 0..4 {
        world.join(factory(), script(BotScript::Patrol), &net);
    }
    world.run(30.0);
    for p in 0..4 {
        let result = stats(&world, p);
        assert!(result.compared > 800, "{result:?}");
        assert_eq!(result.reconciles, 0, "{result:?}");
        assert_eq!(result.max_error, 0.0, "{result:?}");
    }
}

#[test]
fn prediction_still_agrees_with_jitter() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 20.0, 0.0);
    world.join(factory(), script(BotScript::Busy), &net);
    world.run(30.0);
    for (p, player) in world.players.iter().enumerate() {
        assert_eq!(stats(&world, p).reconciles, 0, "{:?}", world.server.counters(player.id));
    }
}

#[test]
fn a_misprediction_is_fixed_once_and_never_seen_as_a_pop() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    let p = world.join(factory(), script(BotScript::Patrol), &net);
    world.run(5.0);
    assert_eq!(stats(&world, p).reconciles, 0);

    let id = world.player_id(p) as usize;
    world.server.game.sim.soldiers[id].body.pos += Vec3::new(0.5, 0.0, 0.0);
    let from = world.players[p].frames.len();
    world.run(1.5);

    let result = stats(&world, p);
    assert_eq!(result.reconciles, 1, "the one correction fixes all later ticks too: {result:?}");
    assert!(result.max_correction > 0.3, "{result:?}");
    let run_step = world.server.game.sim.move_cfg.run_speed * TICK_DT;
    let frames = &world.players[p].frames[from..];
    let worst = frames.windows(2).map(|w| w[0].view.distance(w[1].view)).fold(0.0, f32::max);
    assert!(worst < run_step * 1.5 + 0.5 * 0.2, "worst frame step {worst} m");
    let offset = world.players[p].session.predictor().unwrap().visual_offset().length();
    assert!(offset < 0.01, "{offset}");

    world.run(5.0);
    assert_eq!(stats(&world, p).reconciles, 1, "and stays right");
}

/// The test that needs every private timer, phase and random number: a lost command leaves them all
/// slightly different. Once told, the client must agree again rather than disagree from then on.
#[test]
fn losing_commands_is_put_right_for_good() {
    let mut world = world(ServerConfig::default());
    let good = Net::datagram(50.0, 0.0, 0.0);
    let p = world.join(factory(), script(BotScript::Busy), &good);
    world.run(5.0);

    let up = world.players[p].socket.tx.clone();
    up.borrow_mut().set_config(prs_net::simnet::LinkConfig { loss: 1.0, ..good.up });
    world.run(0.25);
    up.borrow_mut().set_config(good.up);
    world.run(1.0);
    assert!(world.server.counters(world.players[p].id).unwrap().cmds_repeated > 0);
    assert!(stats(&world, p).reconciles > 0, "there really was a disagreement");
    let settled = stats(&world, p).reconciles;
    world.run(15.0);
    assert_eq!(stats(&world, p).reconciles, settled, "no disagreement once it is over");
}

#[test]
fn under_loss_prediction_stays_bounded() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.05);
    for _ in 0..4 {
        world.join(factory(), script(BotScript::Busy), &net);
    }
    world.run(60.0);
    for (p, player) in world.players.iter().enumerate() {
        let result = stats(&world, p);
        assert_eq!(player.session.stats().bad_frames, 0);
        assert_eq!(player.session.stats().missing_baselines, 0, "acked baselines survive loss");
        assert!(result.max_correction < 0.5, "{result:?}");
    }
}

/// What the watcher's copy of a soldier says, and what the server had at that moment.
#[derive(Default)]
struct GunWatch {
    truth: HashMap<u32, (Vec3, Vec3, f32)>,
    worst_muzzle: f32,
    worst_aim: f32,
    worst_hip: f32,
    frames: u32,
    shooter: usize,
    watcher: u32,
    skip_until: u32,
}

impl Observer<SimClient, SimServer> for GunWatch {
    fn after_tick(&mut self, server: &GameServer<SimServer>) {
        let sim = &server.game.sim;
        if let Some(soldier) = sim.soldiers.get(self.shooter) {
            let gun = soldier.gun_pose(&sim.weapons);
            self.truth.insert(sim.tick, (gun.muzzle, gun.forward, soldier.anim.hip_yaw));
        }
    }

    fn after_frame(&mut self, who: u32, session: &ClientSession<SimClient>) {
        if who != self.watcher {
            return;
        }
        let (Some(game), Some(interp)) = (session.game(), session.interp()) else { return };
        let tick = interp.play_tick();
        let (a, b) = (self.truth.get(&(tick.floor() as u32)), self.truth.get(&(tick.ceil() as u32)));
        let (Some(a), Some(b)) = (a, b) else { return };
        if tick < self.skip_until as f64 || session.remotes().is_empty() {
            return;
        }
        let t = tick.fract() as f32;
        let (muzzle, forward) = (a.0.lerp(b.0, t), a.1.lerp(b.1, t).normalize());
        let seen = &game.sim.soldiers[self.shooter];
        let gun = seen.gun_pose(&game.sim.weapons);
        self.worst_muzzle = self.worst_muzzle.max(gun.muzzle.distance(muzzle));
        self.worst_aim = self.worst_aim.max(gun.forward.angle_between(forward));
        self.worst_hip = self.worst_hip.max((seen.anim.hip_yaw - a.2).abs());
        self.frames += 1;
    }
}

#[test]
fn the_gun_seen_by_others_is_where_the_server_has_it() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(100.0, 10.0, 0.0);
    let shooter = world.join(factory(), script(BotScript::Busy), &net);
    let watcher = world.join(factory(), script(BotScript::Idle), &net);
    world.run(1.0);
    let mut watch = GunWatch {
        shooter: world.player_id(shooter) as usize,
        watcher: world.players[watcher].who,
        skip_until: world.server.game.tick() + 120,
        ..Default::default()
    };
    world.run_observed(30.0, &mut watch);
    println!(
        "gun as seen by another player over {} frames: muzzle {:.4} m, aim {:.3} mrad",
        watch.frames,
        watch.worst_muzzle,
        watch.worst_aim * 1000.0,
    );
    assert!(watch.frames > 1000);
    // A bot sprinting, jumping and firing, drawn between 30 Hz snapshots.
    assert!(watch.worst_muzzle < 0.15, "muzzle off by {} m", watch.worst_muzzle);
    assert!(watch.worst_aim < 0.02, "aim off by {} mrad", watch.worst_aim * 1000.0);
}

#[test]
fn remote_soldiers_stay_smooth_and_accurate_at_100ms_with_jitter() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(100.0, 25.0, 0.0);
    let mover = world.join(factory(), script(BotScript::Patrol), &net);
    let watcher = world.join(factory(), script(BotScript::Idle), &net);
    world.run(5.0);
    let starved_settling = world.players[watcher].session.interp().unwrap().stats().starved_frames;
    world.run(25.0);
    let id = world.player_id(mover);
    let run_step = world.server.game.sim.move_cfg.run_speed * TICK_DT;
    let frames = &world.players[watcher].frames;
    let (mut worst_error, mut worst_step, mut last) = (0.0f32, 0.0f32, None);
    for frame in &frames[frames.len() / 4..] {
        let Some(&(_, at)) = frame.remotes.iter().find(|r| r.0 == id as u32) else { continue };
        worst_error = worst_error.max(at.distance(world.truth_at(id, frame.play_tick).expect("recorded")));
        if let Some(previous) = last {
            worst_step = worst_step.max(at.distance(previous));
        }
        last = Some(at);
    }
    let interp = world.players[watcher].session.interp().unwrap().stats();
    println!("remote at 100 +- 25 ms: {worst_error:.4} m off the true path, biggest frame step {worst_step:.4} m");
    println!("starved frames settling: {starved_settling}, then {}", interp.starved_frames - starved_settling);
    assert_eq!(interp.starved_frames - starved_settling, 0, "{interp:?}");
    assert!(worst_error < 0.05, "off the true path by {worst_error} m");
    assert!(worst_step < run_step * 1.15, "a frame moved {worst_step} m, a full run is {run_step}");
}

#[test]
fn other_players_shots_are_heard_once_and_your_own_are_not_echoed() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.10);
    let shooter = world.join(factory(), script(BotScript::Busy), &net);
    let watcher = world.join(factory(), script(BotScript::Idle), &net);
    let mut shots = 0;
    struct Count<'a>(&'a mut usize, usize);
    impl Observer<SimClient, SimServer> for Count<'_> {
        fn after_tick(&mut self, server: &GameServer<SimServer>) {
            let fired = |e: &&SimEvent| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == self.1);
            *self.0 += server.game.sim.events.iter().filter(fired).count();
        }
    }
    world.run(0.5);
    let id = world.player_id(shooter) as usize;
    world.run_observed(30.0, &mut Count(&mut shots, id));
    let heard_shot = |e: &&WireEvent| matches!(e, WireEvent::Sim(SimEvent::ShotFired { .. }));
    let is_shot = |e: &&SimEvent| matches!(e, SimEvent::ShotFired { .. });
    let heard = world.players[watcher].server_events.iter().filter(heard_shot).count();
    assert!(shots > 5, "fired {shots}");
    assert!(heard <= shots && heard + 5 >= shots, "fired {shots}, heard {heard}");
    assert_eq!(world.players[shooter].server_events.iter().filter(heard_shot).count(), 0);
    let predicted = world.players[shooter].predicted_events.iter().filter(is_shot).count();
    assert!(predicted >= shots, "predicted {predicted}, fired {shots}: your shot is immediate");
}

/// Being on the wrong end of a bullet is the server's word alone: the client's own bullet going past
/// its own sim's copy of someone does nothing to them there.
#[test]
fn only_the_server_decides_what_a_bullet_hits() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(20.0, 0.0, 0.0);
    let cmd_of = Rc::new(Cell::new(PlayerCmd::default()));
    let held = cmd_of.clone();
    let input: Rc<dyn Fn(u32, u32) -> PlayerCmd> = Rc::new(move |_, tick| PlayerCmd { tick, ..held.get() });
    let shooter = world.join(factory(), input, &net);
    let target = world.join(factory(), script(BotScript::Idle), &net);
    world.run(2.0);

    // Aim at the other soldier. The client's copy of them is right there, and it must not care.
    let (a, b) = (world.player_id(shooter) as usize, world.player_id(target) as usize);
    let sim = &world.server.game.sim;
    let to = sim.soldiers[b].body.pos + Vec3::Y * 1.3 - sim.soldiers[a].body.eye();
    let mut buttons = Buttons::default();
    buttons.set(Buttons::FIRE, true);
    cmd_of.set(PlayerCmd {
        yaw: (-to.x).atan2(-to.z),
        pitch: (to.y / to.length()).asin(),
        buttons,
        ..Default::default()
    });
    let sees = |world: &Game| world.players[shooter].session.game().unwrap().sim.soldiers[b].health.health;
    let before = sees(&world);
    world.run(0.2);
    assert_eq!(sees(&world), before, "the shooter's client hurt nobody");
}

#[test]
fn two_clients_really_can_shoot_each_other() {
    let mut world = world(ServerConfig::default());
    let net = Net::tcp(40.0, 5.0, 0.0, 200.0);
    let aim = Rc::new(Cell::new((0.0f32, 0.0f32, false)));
    let held = aim.clone();
    let input: Rc<dyn Fn(u32, u32) -> PlayerCmd> = Rc::new(move |_, tick| {
        let (yaw, pitch, fire) = held.get();
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, fire && tick % 8 == 0);
        PlayerCmd { tick, yaw, pitch, buttons, wish_move: Vec2::ZERO, ..Default::default() }
    });
    let shooter = world.join(factory(), input, &net);
    let target = world.join(factory(), script(BotScript::Idle), &net);
    world.run(2.0);

    let (a, b) = (world.player_id(shooter) as usize, world.player_id(target) as usize);
    let to = world.server.game.sim.soldiers[b].body.pos + Vec3::Y * 1.2 - world.server.game.sim.soldiers[a].body.eye();
    aim.set(((-to.x).atan2(-to.z), (to.y / to.length()).asin(), true));
    world.run(6.0);

    let hit = world.players[target]
        .server_events
        .iter()
        .any(|e| matches!(e, WireEvent::Sim(SimEvent::SoldierHit { soldier, shooter: from, .. }) if *soldier == b && *from == a));
    assert!(hit, "the victim was told");
    let told = world.players[shooter]
        .server_events
        .iter()
        .any(|e| matches!(e, WireEvent::Sim(SimEvent::SoldierHit { soldier, .. }) if *soldier == b));
    assert!(told, "and so was the shooter");
    let on_server = world.server.game.sim.soldiers[b].health;
    let on_victim = world.players[target].session.game().unwrap().sim.soldiers[b].health;
    // Six seconds of hits can kill, and a practice range brings the dead straight back.
    let came_back = world.players[target]
        .server_events
        .iter()
        .any(|e| matches!(e, WireEvent::Sim(SimEvent::Respawned { soldier }) if *soldier == b));
    assert!(on_server.health < 100.0 || on_server.life != Life::Up || came_back, "{on_server:?}");
    assert_eq!(on_victim.life, on_server.life, "the victim knows what state they are in");
}

/// Shooter aims exactly at what it draws of someone running across, and fires. With rewinding those
/// are hits. With it off, at this latency, the same shots land behind the target.
fn crossing_shots(net: Net, max_rewind_ticks: u32) -> (u32, u32) {
    let cfg = ServerConfig { max_rewind_ticks, ..Default::default() };
    let game = SimServer::with_sim(
        Sim::new(Level::flat(400.0)),
        "flat",
        "practice",
        Mode::free_practice(),
        SimServerConfig { rewind_ticks: 40, ..Default::default() },
    );
    let mut world: Game = World::new(game, cfg, position_of);
    let flat: fn() -> GameFactory<SimClient> =
        || Box::new(|_: &str, _: &str| Some(SimClient::with_sim(Sim::new(Level::flat(400.0)), Mode::free_practice())));

    let aim = Rc::new(Cell::new((0.0f32, 0.0f32, false)));
    let held = aim.clone();
    let shooter_input: Rc<dyn Fn(u32, u32) -> PlayerCmd> = Rc::new(move |_, tick| {
        let (yaw, pitch, fire) = held.get();
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, fire && tick % 30 == 0);
        PlayerCmd { tick, yaw, pitch, buttons, wish_move: Vec2::ZERO, ..Default::default() }
    });
    // Back and forth across the line of fire, 15 m away, faster than the bullet's flight matters.
    let runner_input: Rc<dyn Fn(u32, u32) -> PlayerCmd> = Rc::new(|_, tick| {
        let side = if (tick / 120) % 2 == 0 { 1.0 } else { -1.0 };
        PlayerCmd { tick, wish_move: Vec2::new(side, 0.0), ..Default::default() }
    });
    let shooter = world.join(flat(), shooter_input, &net);
    let runner = world.join(flat(), runner_input, &net);
    world.run(1.0);
    let (a, b) = (world.player_id(shooter) as usize, world.player_id(runner) as usize);
    world.server.game.sim.set_home(a, Vec3::new(0.0, 0.1, 15.0), 0.0);
    world.server.game.sim.set_home(b, Vec3::new(0.0, 0.1, 0.0), 0.0);
    world.run(1.0);
    assert!(world.server.game.sim.soldiers[b].health.is_up());

    struct Aim {
        aim: Rc<Cell<(f32, f32, bool)>>,
        who: u32,
        target: usize,
        hits: u32,
        fired: u32,
        victim: usize,
    }
    impl Observer<SimClient, SimServer> for Aim {
        fn after_tick(&mut self, server: &GameServer<SimServer>) {
            for event in &server.game.sim.events {
                match event {
                    SimEvent::ShotFired { .. } => self.fired += 1,
                    SimEvent::SoldierHit { soldier, .. } if *soldier == self.victim => self.hits += 1,
                    _ => {}
                }
            }
        }
        fn after_frame(&mut self, who: u32, session: &ClientSession<SimClient>) {
            if who != self.who {
                return;
            }
            let Some(game) = session.game() else { return };
            let me = &game.sim.soldiers[game.local];
            let them = game.sim.soldiers[self.target].body.pos + Vec3::Y * 1.2;
            // From the muzzle, which is off to the side of the eye when firing from the hip.
            let to = them - me.gun_pose(&game.sim.weapons).muzzle;
            let gun = me.weapon.aim_offset();
            self.aim.set(((-to.x).atan2(-to.z) - gun.x, (to.y / to.length()).asin() - gun.y, true));
        }
    }
    let mut observer = Aim { aim, who: world.players[shooter].who, target: b, hits: 0, fired: 0, victim: b };
    // Short, so nobody dies of it, and repeated by healing up between goes.
    for _ in 0..8 {
        world.server.game.sim.soldiers[b].health = prs_sim::health::Health::new(&prs_sim::health::HealthConfig::default());
        world.run_observed(1.0, &mut observer);
    }
    (observer.hits, observer.fired)
}

#[test]
fn lag_compensation_hits_what_the_shooter_saw() {
    let net = Net::tcp(60.0, 0.0, 0.0, 200.0);
    let (with, fired) = crossing_shots(net.clone(), 36);
    let (without, fired_without) = crossing_shots(net, 0);
    println!("at 120 ms RTT, aimed at what is drawn: {with}/{fired} hit with rewinding, {without}/{fired_without} without");
    assert!(fired >= 6, "fired {fired}");
    assert!(with * 10 >= fired * 7, "{with}/{fired}: what was on the sights should hit");
    assert!(without * 3 < with, "{without} without against {with} with");
}

// ---- Vehicles and helicopters.

/// Puts the first player at the wheel of the first vehicle, and whoever else in the seats behind.
fn seat(world: &mut Game, soldier: usize, carrier: u32, seat: u8) {
    let id = world.player_id(soldier) as usize;
    world.server.game.sim.sit_in(id, prs_sim::seats::SeatedAt { carrier, seat });
}

fn driving(_: u32, tick: u32) -> PlayerCmd {
    let t = tick as f32 / 60.0;
    PlayerCmd { tick, wish_move: Vec2::new((t * 0.5).sin() * 0.01, 1.0), ..Default::default() }
}

#[test]
fn a_driver_predicts_their_vehicle_to_the_bit() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.0);
    let driver = world.join(factory(), Rc::new(driving), &net);
    world.run(1.0);
    seat(&mut world, driver, 0, 0);
    world.run(2.0);
    assert_eq!(world.server.game.sim.soldiers[world.player_id(driver) as usize].seat.map(|s| s.carrier), Some(0));
    let settled = stats(&world, driver).reconciles;
    assert!(settled >= 1, "getting put in a seat was news");
    let start = world.server.game.sim.vehicles[0].pos;
    let before = stats(&world, driver).compared;
    world.run(20.0);
    let result = stats(&world, driver);
    let vehicle = &world.server.game.sim.vehicles[0];
    println!("driving 20 s at 100 ms RTT: {} compared, {} reconciles, went {:.0} m", result.compared - before, result.reconciles - settled, vehicle.pos.distance(start));
    assert!(vehicle.pos.distance(start) > 20.0, "it really drove");
    assert_eq!(result.reconciles, settled, "{result:?}");
    let on_client = &world.players[driver].session.game().unwrap().sim.vehicles[0];
    let lead = world.players[driver].session.predictor().unwrap().tick() - world.server.game.tick();
    assert!(on_client.pos.distance(vehicle.pos) < vehicle.vel.length() * (lead as f32 + 2.0) * TICK_DT + 0.1, "the client's is ahead by its lead");
}

#[test]
fn a_driver_on_a_lossy_link_is_put_right_without_a_jump() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.05);
    let driver = world.join(factory(), Rc::new(driving), &net);
    world.run(1.0);
    seat(&mut world, driver, 0, 0);
    world.run(30.0);
    let result = stats(&world, driver);
    println!("driving 30 s at 5% loss: {} compared, {} reconciles, biggest correction {:.3} m", result.compared, result.reconciles, result.max_correction);
    assert_eq!(world.players[driver].session.stats().missing_baselines, 0);
    assert!(result.reconciles <= 2 + result.compared / 50, "{result:?}");
}

#[test]
fn everyone_else_sees_a_vehicle_and_who_is_in_it_move_together() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.0);
    let driver = world.join(factory(), Rc::new(driving), &net);
    let passenger = world.join(factory(), script(BotScript::Idle), &net);
    let watcher = world.join(factory(), script(BotScript::Idle), &net);
    world.run(1.0);
    seat(&mut world, driver, 0, 0);
    seat(&mut world, passenger, 0, 1);
    world.run(10.0);

    let passenger_id = world.player_id(passenger) as usize;
    let seen = &world.players[watcher].session.game().unwrap().sim;
    assert_eq!(seen.soldiers[passenger_id].seat.map(|s| s.seat), Some(1), "they know which seat");
    let frame = seen.carrier_frame(0);
    let local = frame.to_local(seen.soldiers[passenger_id].body.pos);
    let truth = &world.server.game.sim;
    let true_local = truth.carrier_frame(0).to_local(truth.soldiers[passenger_id].body.pos);
    assert!(local.distance(true_local) < 0.05, "in the seat they are really in: {local} vs {true_local}");
    assert!(frame.pos.distance(truth.vehicles[0].pos) > 0.05, "and it really is moving");
    assert!(seen.vehicles[0].occupants.0[1].is_some(), "the vehicle knows who is in it");

    // Riding as drawn, in the past, is no disagreement with a server that has them in the present.
    let reconciles = stats(&world, passenger).reconciles;
    world.run(10.0);
    let extra = stats(&world, passenger).reconciles - reconciles;
    println!("a passenger over 10 s of driving: {extra} reconciles");
    assert!(extra <= 5, "a passenger agrees nearly all the way: {extra}");
}

#[test]
fn getting_out_hands_the_vehicle_back_to_everyone() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(30.0, 0.0, 0.0);
    let driver = world.join(factory(), Rc::new(driving), &net);
    world.run(1.0);
    seat(&mut world, driver, 0, 0);
    world.run(5.0);
    let id = world.player_id(driver) as usize;
    world.server.game.sim.soldiers[id].seat = None;
    world.server.game.sim.vehicles[0].occupants.leave(id as u32);
    world.run(5.0);
    let client = world.players[driver].session.game().unwrap();
    let server = &world.server.game.sim;
    assert!(client.sim.soldiers[id].seat.is_none());
    let drawn_behind = server.vehicles[0].vel.length() * 0.3 + 0.05;
    assert!(client.sim.vehicles[0].pos.distance(server.vehicles[0].pos) < drawn_behind, "it is drawn from the server again");
}

#[test]
fn a_pilot_predicts_their_helicopter_to_the_bit() {
    let mut world = world(ServerConfig::default());
    let net = Net::datagram(50.0, 10.0, 0.0);
    let flying: Rc<dyn Fn(u32, u32) -> PlayerCmd> = Rc::new(|_, tick| {
        let t = tick as f32 / 60.0;
        let mut buttons = Buttons::default();
        buttons.set(Buttons::JUMP, tick % 3000 == 300);
        // On the skids until the engine has run up, then up and away.
        let collective = if t < 25.0 { 0.0 } else { 0.8 };
        PlayerCmd { tick, wish_move: Vec2::new((t * 0.4).sin() * 0.05, collective), stick: [(t.sin() * 15.0) as i8, 8], buttons, ..Default::default() }
    });
    let pilot = world.join(factory(), flying, &net);
    world.run(1.0);
    seat(&mut world, pilot, prs_sim::copters::HELI_CARRIERS, 0);
    world.run(1.0);
    let settled = stats(&world, pilot).reconciles;
    let before = stats(&world, pilot).compared;
    world.run(40.0);
    let result = stats(&world, pilot);
    let heli = &world.server.game.sim.helicopters[0].state;
    println!("flying 40 s at 100 ms RTT: {} compared, {} reconciles, height {:.1} m, rotor {:.1}", result.compared - before, result.reconciles - settled, heli.pos.y, heli.rotor_speed);
    assert!(heli.rotor_speed > 0.0 && heli.pos.y > 5.0, "it really flew");
    assert_eq!(result.reconciles, settled, "{result:?}");
}

/// Passenger aiming at a soldier standing beside the road, from a truck going past, at real latency.
#[test]
fn a_passengers_shot_starts_from_where_they_saw_themselves() {
    let cfg = ServerConfig::default();
    let mut world = world(cfg);
    let net = Net::datagram(60.0, 0.0, 0.0);
    let driver = world.join(factory(), Rc::new(driving), &net);
    let passenger = world.join(factory(), script(BotScript::Idle), &net);
    world.run(1.0);
    seat(&mut world, driver, 0, 0);
    seat(&mut world, passenger, 0, 1);
    world.run(4.0);
    let id = world.player_id(passenger) as usize;
    let sim = &world.server.game.sim;
    let speed = sim.vehicles[0].vel.length();
    assert!(speed > 2.0, "going {speed} m/s");

    // Twelve ticks of lag: the muzzle is where the seat was then, not where it is now.
    let then = sim.past_carrier_frame(0, 12).expect("on record");
    let now = sim.carrier_frame(0);
    let muzzle = sim.soldiers[id].gun_pose(&sim.weapons).muzzle;
    let origin = sim.shot_origin(id, muzzle, 12);
    assert!(origin.distance(then.point(now.to_local(muzzle))) < 1e-4);
    assert!(origin.distance(muzzle) > speed * 12.0 * TICK_DT * 0.7, "moved back along the road: {} m", origin.distance(muzzle));

    // The driver is in the present, whatever they say about what they see.
    let driver_id = world.player_id(driver) as usize;
    let sim = &world.server.game.sim;
    let muzzle = sim.soldiers[driver_id].gun_pose(&sim.weapons).muzzle;
    assert_eq!(sim.shot_origin(driver_id, muzzle, 12), muzzle);
}

// ---- A real match.

fn aas_world() -> Game {
    let game = SimServer::new("town", "aas", SimServerConfig::default()).expect("the map and mode exist");
    World::new(game, ServerConfig::default(), position_of)
}

#[test]
fn on_a_real_match_a_player_waits_on_the_spawn_screen_and_deploys_by_asking() {
    let mut world = aas_world();
    let net = Net::tcp(40.0, 5.0, 0.0, 200.0);
    let me = world.join(factory(), script(BotScript::Patrol), &net);
    world.run(3.0);
    let id = world.player_id(me) as usize;
    assert!(!world.server.game.sim.soldiers[id].present, "still on the spawn screen");
    let held = world.server.game.sim.soldiers[id].body.pos;
    assert_eq!(stats(&world, me).reconciles, 0, "and that is what the client has too");

    world.players[me].session.send_request(encode_cmd(&ModeCmd::Deploy));
    world.run(25.0);
    let soldier = &world.server.game.sim.soldiers[id];
    assert!(soldier.present, "in the game after asking");
    assert!(soldier.body.pos.distance(held) > 1.0, "somewhere else entirely");

    let client = world.players[me].session.game().unwrap();
    assert!(client.sim.soldiers[id].present, "and the client knows");
    assert!(client.sim.soldiers[id].body.pos.distance(soldier.body.pos) < 2.0);
    let shown = client.match_state.as_ref().expect("the client has been told about the match");
    assert_eq!(*shown.player(id).map(|p| &p.deployed).unwrap(), true);
    let told = world.players[me].server_events.iter().any(|e| matches!(e, WireEvent::Mode(ModeEvent::Deployed { soldier }) if *soldier == id));
    assert!(told, "the client heard it happen");
}

#[test]
fn a_nonsense_request_from_a_client_is_turned_down_and_nothing_breaks() {
    let mut world = aas_world();
    let net = Net::tcp(40.0, 5.0, 0.0, 200.0);
    let me = world.join(factory(), script(BotScript::Idle), &net);
    world.run(1.0);
    world.players[me].session.send_request(encode_cmd(&ModeCmd::SetKit(999_999)));
    world.players[me].session.send_request(encode_cmd(&ModeCmd::JoinSquad(424242)));
    world.players[me].session.send_request(vec![250, 1, 2, 3]);
    world.run(2.0);
    let refused = world.players[me].server_events.iter().filter(|e| matches!(e, WireEvent::Mode(ModeEvent::Refused { .. }))).count();
    assert!(refused >= 1, "told why");
    assert_eq!(world.players[me].session.stats().bad_frames, 0);
}

#[test]
fn bots_on_a_real_match_reach_everyone_and_cost_what_they_cost() {
    let mut world = aas_world();
    let net = Net::tcp(40.0, 5.0, 0.0, 200.0);
    let me = world.join(factory(), script(BotScript::Idle), &net);
    world.run(20.0);
    let soldiers = world.players[me].session.remotes().iter().filter(|(_, pose)| matches!(pose, AnyPose::Soldier(_))).count();
    assert!(soldiers >= 8, "{soldiers} bots seen");
    let before = world.players[me].session.stats().bytes_down;
    world.run(5.0);
    let rate = (world.players[me].session.stats().bytes_down - before) as f64 / 5.0;
    println!("watching {soldiers} bots play on town: {rate:.0} B/s");
}

// ---- Cheats. These talk to the server directly: a hacked client doesn't use our client.

struct Attacker {
    server: GameServer<SimServer>,
    id: u32,
    soldier: usize,
    start: Vec3,
}

fn walk(tick: u32) -> PlayerCmd {
    PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), ..Default::default() }
}

impl Attacker {
    fn new() -> Self {
        let mut server = GameServer::new(server_game(), ServerConfig::default());
        let id = server.connect(true);
        let rules = server.game.rules_hash();
        server.receive(id, &ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode());
        server.receive(id, &ClientMsg::Join { rules, name: String::new() }.encode());
        let soldier = server.player_of(id).unwrap() as usize;
        let mut attacker = Self { server, id, soldier, start: Vec3::ZERO };
        // Land before starting anything, so that speed is ground speed.
        for _ in 0..60 {
            let cmd = PlayerCmd { tick: attacker.next(), ..Default::default() };
            attacker.send(vec![cmd]);
            attacker.server.tick();
        }
        attacker.start = attacker.pos();
        attacker
    }

    fn next(&self) -> u32 {
        self.server.game.tick() + 1
    }

    fn send(&mut self, cmds: Vec<PlayerCmd>) {
        let frame = ClientMsg::Cmds(CmdPacket { cmds, ..Default::default() }).encode();
        self.server.receive(self.id, &frame);
    }

    fn pos(&self) -> Vec3 {
        self.server.game.sim.soldiers[self.soldier].body.pos
    }

    fn honest_distance(ticks: u32) -> f32 {
        let mut honest = Attacker::new();
        for _ in 0..ticks {
            let cmd = walk(honest.next());
            honest.send(vec![cmd]);
            honest.server.tick();
        }
        honest.pos().distance(honest.start)
    }
}

const WINDOW_TICKS: u32 = 120;

#[test]
fn a_huge_move_goes_no_faster_than_an_honest_one() {
    let honest = Attacker::honest_distance(WINDOW_TICKS);
    let mut cheat = Attacker::new();
    for _ in 0..WINDOW_TICKS {
        let cmd = PlayerCmd { wish_move: Vec2::new(0.0, 1e9), ..walk(cheat.next()) };
        cheat.send(vec![cmd]);
        cheat.server.tick();
    }
    let went = cheat.pos().distance(cheat.start);
    assert!(went <= honest * 1.002, "cheat {went}, honest {honest}");
    assert!(went > honest * 0.9, "a clamped command still works: {went} vs {honest}");
}

#[test]
fn a_speed_hack_gains_nothing() {
    let honest = Attacker::honest_distance(WINDOW_TICKS);
    let mut cheat = Attacker::new();
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
    let went = cheat.pos().distance(cheat.start);
    assert!(went <= honest * 1.002, "cheat {went}, honest {honest}");
    assert!(cheat.server.counters(cheat.id).unwrap().cmds_future > 0);
}

#[test]
fn a_tick_from_the_future_is_refused() {
    let mut cheat = Attacker::new();
    let far = cheat.next() + 100_000;
    cheat.send(vec![walk(far)]);
    assert_eq!(cheat.server.counters(cheat.id).unwrap().cmds_future, 1);
}

#[test]
fn extreme_and_broken_aim_cannot_break_a_soldier() {
    let mut cheat = Attacker::new();
    let max = cheat.server.config().limits.max_turn_per_tick;
    for (i, yaw) in [1e30f32, -1e30, f32::MAX, f32::NAN, f32::INFINITY, 1e12].into_iter().enumerate() {
        let before = cheat.server.game.sim.soldiers[cheat.soldier].yaw;
        let pitch = if i % 2 == 0 { 1e30 } else { f32::NAN };
        let cmd = PlayerCmd { yaw, pitch, ..walk(cheat.next()) };
        cheat.send(vec![cmd]);
        cheat.server.tick();
        let soldier = &cheat.server.game.sim.soldiers[cheat.soldier];
        assert!(soldier.yaw.is_finite() && soldier.pitch.is_finite(), "{}, {}", soldier.yaw, soldier.pitch);
        assert!((soldier.yaw - before).abs() <= max + 1e-3, "turned {}", soldier.yaw - before);
        assert!(soldier.body.pos.is_finite());
        assert!(soldier.weapon.aim_offset().is_finite());
    }
    let counters = cheat.server.counters(cheat.id).unwrap();
    assert!(counters.turns_clamped > 0, "{counters:?}");
    assert!(counters.cmds_not_finite > 0, "{counters:?}");
}

#[test]
fn a_fast_trigger_fires_no_faster_than_the_gun_and_runs_out() {
    let mut cheat = Attacker::new();
    let mut shots = 0;
    for _ in 0..600 {
        let tick = cheat.next();
        let mut buttons = Buttons::default();
        buttons.set(Buttons::FIRE, tick % 2 == 0);
        cheat.send(vec![PlayerCmd { tick, buttons, ..Default::default() }; 8]);
        cheat.server.tick();
        let fired = |e: &&SimEvent| matches!(e, SimEvent::ShotFired { .. });
        shots += cheat.server.game.sim.events.iter().filter(fired).count();
    }
    let sim = &cheat.server.game.sim;
    let cfg = sim.soldiers[cheat.soldier].held_config(&sim.weapons);
    assert!(shots as u32 <= cfg.magazine, "{shots} from a magazine of {}", cfg.magazine);
}

#[test]
fn asking_for_more_rewind_than_allowed_gets_the_limit() {
    let mut cheat = Attacker::new();
    let limit = cheat.server.config().max_rewind_ticks;
    for _ in 0..30 {
        let tick = cheat.next();
        let packet = CmdPacket { view_tick: 1, cmds: vec![walk(tick)], ..Default::default() };
        cheat.server.receive(cheat.id, &ClientMsg::Cmds(packet).encode());
        cheat.server.tick();
    }
    // Fire, and see how far back the bullet will look.
    let tick = cheat.next();
    let mut buttons = Buttons::default();
    buttons.set(Buttons::FIRE, true);
    let packet = CmdPacket { view_tick: 1, cmds: vec![PlayerCmd { tick, buttons, ..Default::default() }], ..Default::default() };
    cheat.server.receive(cheat.id, &ClientMsg::Cmds(packet).encode());
    cheat.server.tick();
    let bullet = cheat.server.game.sim.bullets.last().expect("a shot was fired");
    assert_eq!(bullet.rewind, limit);
}

#[test]
fn a_client_with_other_tuning_or_no_such_map_is_told_so() {
    fn handshake(session: &mut ClientSession<SimClient>, server: &mut GameServer<SimServer>) {
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
    let mut server = GameServer::new(server_game(), ServerConfig::default());
    let tuned = |map: &str, mode: &str| {
        let mut sim = Sim::new(Level::named(map)?);
        sim.move_cfg.run_speed *= 2.0;
        Some(SimClient::with_sim(sim, Mode::named(mode)?))
    };
    let mut session = ClientSession::new(Box::new(tuned), ClientConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(session.state(), ClientState::Rejected(RejectReason::Rules));

    let mut session = ClientSession::new(Box::new(|_: &str, _: &str| None::<SimClient>), ClientConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(session.state(), ClientState::Rejected(RejectReason::UnknownMap));

    let mut session = ClientSession::new(factory(), ClientConfig::default());
    handshake(&mut session, &mut server);
    assert_eq!(session.state(), ClientState::Playing);
}

#[test]
fn the_client_plays_the_map_the_server_says() {
    let game = SimServer::new("town", "aas", SimServerConfig::default()).expect("a map");
    let mut server = GameServer::new(game, ServerConfig::default());
    let id = server.connect(true);
    let told = Rc::new(std::cell::RefCell::new(String::new()));
    let told_too = told.clone();
    let factory: GameFactory<SimClient> = Box::new(move |map: &str, mode: &str| {
        *told_too.borrow_mut() = format!("{map} {mode}");
        factory()(map, mode)
    });
    let mut session = ClientSession::new(factory, ClientConfig::default());
    for _ in 0..3 {
        for frame in session.drain_outgoing() {
            server.receive(id, &frame);
        }
        for (_, frame) in server.drain_outgoing() {
            session.receive(&frame);
        }
    }
    assert_eq!(*told.borrow(), "town aas");
    assert_eq!(session.state(), ClientState::Playing);
}

// ---- Measurements.

/// IP, TCP and the WebSocket frame around each message: 20 + 20 + 4 from the server, plus a 4 byte
/// mask from a client. Timestamps and any bare ACKs are on top of this.
const OVERHEAD_DOWN: f64 = 44.0;
const OVERHEAD_UP: f64 = 48.0;

struct Row {
    down: f64,
    down_max: f64,
    up: f64,
    frames_down: f64,
    frames_up: f64,
}

fn measure(players: usize, s: BotScript, cfg: ServerConfig, seconds: f64) -> Row {
    let mut world = world(cfg);
    world.record = false;
    let net = Net::tcp(50.0, 5.0, 0.0, 200.0);
    for _ in 0..players {
        world.join(factory(), script(s), &net);
    }
    world.run(3.0);
    let before: Vec<_> = world.players.iter().map(|p| p.session.stats()).collect();
    world.run(seconds);
    let mut row = Row { down: 0.0, down_max: 0.0, up: 0.0, frames_down: 0.0, frames_up: 0.0 };
    for (player, was) in world.players.iter().zip(before) {
        let now = player.session.stats();
        let down = (now.bytes_down - was.bytes_down) as f64 / seconds;
        row.down += down / players as f64;
        row.down_max = row.down_max.max(down);
        row.up += (now.bytes_up - was.bytes_up) as f64 / seconds / players as f64;
        row.frames_down += (now.frames_down - was.frames_down) as f64 / seconds / players as f64;
        row.frames_up += (now.frames_up - was.frames_up) as f64 / seconds / players as f64;
    }
    row
}

#[test]
#[ignore = "report"]
fn report_bandwidth() {
    println!("players script snap/s | down payload B/s (max) | down wire KB/s | up payload B/s | up wire KB/s");
    for &(players, s, name, snapshot_every) in &[
        (10, BotScript::Patrol, "patrol", 2),
        (10, BotScript::Busy, "busy", 2),
        (32, BotScript::Idle, "idle", 2),
        (32, BotScript::Patrol, "patrol", 2),
        (32, BotScript::Busy, "busy", 2),
        (32, BotScript::Busy, "busy", 3),
    ] {
        let cfg = ServerConfig { snapshot_every, ..Default::default() };
        let row = measure(players, s, cfg, 10.0);
        let down_wire = row.down + row.frames_down * OVERHEAD_DOWN;
        let up_wire = row.up + row.frames_up * OVERHEAD_UP;
        println!(
            "{players:>7} {name:<6} {:>6.0} | {:>10.0} ({:>7.0}) | {:>14.1} | {:>14.0} | {:>12.1}",
            60.0 / snapshot_every as f64,
            row.down,
            row.down_max,
            down_wire / 1000.0,
            row.up,
            up_wire / 1000.0,
        );
    }
}

/// Everyone walking around a kilometre square, or everyone in one place.
fn measure_map(players: usize, spread: f32, interest: InterestConfig) -> (f64, f64, f32) {
    let flat: fn() -> GameFactory<SimClient> =
        || Box::new(|_: &str, _: &str| Some(SimClient::with_sim(Sim::new(Level::flat(1200.0)), Mode::free_practice())));
    let game = SimServer::with_sim(Sim::new(Level::flat(1200.0)), "flat", "practice", Mode::free_practice(), SimServerConfig::default());
    let mut world: Game = World::new(game, ServerConfig { interest, ..Default::default() }, position_of);
    let net = Net::tcp(50.0, 5.0, 0.0, 200.0);
    for _ in 0..players {
        world.join(flat(), script(BotScript::Patrol), &net);
    }
    world.run(1.0);
    let mut seed = 0x2545_F491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    };
    for p in 0..players {
        let id = world.player_id(p) as usize;
        let at = Vec3::new(next() * spread, 0.1, next() * spread);
        world.server.game.sim.set_home(id, at, 0.0);
    }
    world.run(4.0);
    let before: Vec<_> = world.players.iter().map(|p| p.session.stats()).collect();
    let seconds = 10.0;
    world.run(seconds);
    let (mut down, mut most) = (0.0, 0.0f64);
    for (player, was) in world.players.iter().zip(before) {
        let now = player.session.stats();
        let payload = (now.bytes_down - was.bytes_down) as f64 / seconds;
        let frames = (now.frames_down - was.frames_down) as f64 / seconds;
        let wire = payload + frames * OVERHEAD_DOWN;
        down += wire / players as f64;
        most = most.max(wire);
    }
    // How far anyone is drawn from anywhere the server has had them lately.
    let mut worst = 0.0f32;
    for player in &world.players {
        let Some(frame) = player.frames.last() else { continue };
        for (key, at) in &frame.remotes {
            let Some(history) = world.truth.get(&(*key as u16)) else { continue };
            let tick = frame.play_tick as u32;
            let nearest = (0..120).filter_map(|back| history.get(&tick.saturating_sub(back))).map(|t| t.distance(*at)).fold(f32::MAX, f32::min);
            if nearest < f32::MAX {
                worst = worst.max(nearest);
            }
        }
    }
    (down, most, worst)
}

#[test]
#[ignore = "report"]
fn report_interest() {
    let everything = InterestConfig { snapshot_budget: usize::MAX, full_rate_within: 1e6, ..Default::default() };
    println!("players  where         interest        | down wire KB/s avg (most) | worst off any recent path m");
    for &(players, spread, place) in &[(32, 1000.0, "1 km square"), (64, 1000.0, "1 km square"), (32, 60.0, "60 m square"), (64, 60.0, "60 m square")] {
        for (name, interest) in [
            ("none", everything.clone()),
            ("default", InterestConfig::default()),
            ("budget 400 B", InterestConfig { snapshot_budget: 400, ..Default::default() }),
        ] {
            let (down, most, worst) = measure_map(players, spread, interest);
            println!(
                "{players:>7}  {place:<13} {name:<15} | {:>14.1} ({:>5.1})     | {worst:>6.2}",
                down / 1000.0,
                most / 1000.0,
            );
        }
    }
}

#[test]
#[ignore = "report"]
fn report_transport_cost() {
    println!("link              | lead | repeats/s | reconciles/s | max corr m | starved % | worst remote err m");
    for (name, net) in [
        ("datagram 0%", Net::datagram(50.0, 5.0, 0.0)),
        ("datagram 5%", Net::datagram(50.0, 5.0, 0.05)),
        ("tcp 0%", Net::tcp(50.0, 5.0, 0.0, 200.0)),
        ("tcp 0.1% rto 200", Net::tcp(50.0, 5.0, 0.001, 200.0)),
        ("tcp 1% rto 200", Net::tcp(50.0, 5.0, 0.01, 200.0)),
        ("tcp 5% rto 200", Net::tcp(50.0, 5.0, 0.05, 200.0)),
    ] {
        let mut game = world(ServerConfig::default());
        let mover = game.join(factory(), script(BotScript::Busy), &net);
        let watcher = game.join(factory(), script(BotScript::Idle), &net);
        game.run(5.0);
        let from = game.players[watcher].frames.len();
        let starved = game.players[watcher].session.interp().unwrap().stats().starved_frames;
        let reconciles = stats(&game, mover).reconciles;
        let repeats = game.server.counters(game.players[mover].id).unwrap().cmds_repeated;
        let seconds = 30.0;
        game.run(seconds);
        let id = game.player_id(mover);
        let result = stats(&game, mover);
        let lead = game.players[mover].session.predictor().unwrap().tick() as i64 - game.server.game.tick() as i64;
        let w = &game.players[watcher];
        let frames = &w.frames[from..];
        let worst = frames
            .iter()
            .filter_map(|f| {
                let r = f.remotes.iter().find(|r| r.0 == id as u32)?;
                Some(r.1.distance(game.truth_at(id, f.play_tick)?))
            })
            .fold(0.0, f32::max);
        println!(
            "{name:<17} | {lead:>4} | {:>9.2} | {:>12.2} | {:>10.3} | {:>8.2}% | {worst:>8.3}",
            (game.server.counters(game.players[mover].id).unwrap().cmds_repeated - repeats) as f64 / seconds,
            (result.reconciles - reconciles) as f64 / seconds,
            result.max_correction,
            (w.session.interp().unwrap().stats().starved_frames - starved) as f64 * 100.0 / frames.len() as f64,
        );
    }
}


/// The same game over a WebSocket and over WebTransport, on a long, lossy link. TCP's resend timer and
/// QUIC's are taken as the same, a round trip and a bit, so the difference is only what has to wait.
#[test]
#[ignore = "report"]
fn report_websocket_against_webtransport() {
    const ONE_WAY_MS: f64 = 100.0;
    const JITTER_MS: f64 = 5.0;
    const RESEND_MS: f64 = 250.0;
    const PLAYERS: usize = 8;
    println!("link     | lead ms avg | lead ms max | extrapolated % | reconciles/s | down B/s | up B/s");
    for loss in [0.01, 0.02, 0.05] {
        for (name, net) in [
            ("ws", Net::tcp(ONE_WAY_MS, JITTER_MS, loss, RESEND_MS)),
            ("wt", Net::quic(ONE_WAY_MS, JITTER_MS, loss, RESEND_MS)),
        ] {
            let mut game = world(ServerConfig::default());
            let mover = game.join(factory(), script(BotScript::Busy), &net);
            let watcher = game.join(factory(), script(BotScript::Idle), &net);
            for _ in 2..PLAYERS {
                game.join(factory(), script(BotScript::Busy), &net);
            }
            game.run(5.0);
            let from = game.players[watcher].frames.len();
            let starved = game.players[watcher].session.interp().unwrap().stats().starved_frames;
            let reconciles = stats(&game, mover).reconciles;
            let link = |game: &Game| (game.players[mover].socket.rx.borrow().stats(), game.players[mover].socket.tx.borrow().stats());
            let (down, up) = link(&game);
            let seconds = 30;
            let mut leads = Vec::new();
            for _ in 0..seconds * 10 {
                game.run(0.1);
                let lead = game.players[mover].session.predictor().unwrap().tick() as f64 - game.server.game.tick() as f64;
                leads.push(lead * TICK_DT as f64 * 1000.0);
            }
            let (down_now, up_now) = link(&game);
            let w = &game.players[watcher];
            let frames = w.frames.len() - from;
            println!(
                "{name} {:>2.0}% | {:>11.0} | {:>11.0} | {:>13.1}% | {:>12.2} | {:>8.0} | {:>6.0}",
                loss * 100.0,
                leads.iter().sum::<f64>() / leads.len() as f64,
                leads.iter().copied().fold(0.0, f64::max),
                (w.session.interp().unwrap().stats().starved_frames - starved) as f64 * 100.0 / frames as f64,
                (stats(&game, mover).reconciles - reconciles) as f64 / seconds as f64,
                (down_now.bytes - down.bytes) as f64 / seconds as f64,
                (up_now.bytes - up.bytes) as f64 / seconds as f64,
            );
        }
    }
}

/// How long a player takes to play properly again after their tab was out of sight: their lead back near
/// where it was and no corrections, for a whole second. "Before" turns off what fixed it.
#[test]
#[ignore = "report"]
fn report_coming_back_to_a_hidden_tab() {
    use prs_net::clock::ClockConfig;
    const GIVE_UP_S: f64 = 300.0;
    const STEP_S: f64 = 0.05;
    let before = ClientConfig {
        clock: ClockConfig { jump_settle_ms: 0.0, ..Default::default() },
        away_after_s: f32::INFINITY,
        ..Default::default()
    };
    println!("away  | frames while away | before: steady after | now: steady after");
    for (away, throttled) in [(5.0, None), (30.0, None), (120.0, None), (30.0, Some(1000.0)), (360.0, Some(60_000.0))] {
        let mut results = Vec::new();
        for client in [before.clone(), ClientConfig::default()] {
            let net = Net { client, ..Net::tcp(40.0, 5.0, 0.0, 200.0) };
            let mut game = world(ServerConfig::default());
            let mover = game.join(factory(), script(BotScript::Busy), &net);
            game.join(factory(), script(BotScript::Busy), &net);
            game.run(5.0);
            let lead_of = |game: &Game| {
                game.players[mover].session.predictor().map_or(0, |p| p.tick() as i64 - game.server.game.tick() as i64)
            };
            let usual = lead_of(&game);
            let now = game.players[mover].socket.clock_ms.get();
            game.players[mover].hidden = Some(Hidden { until_ms: now + away * 1000.0, frame_every_ms: throttled });
            game.run(away);
            let (mut t, mut calm_since, mut reconciles) = (0.0, 0.0, stats(&game, mover).reconciles);
            while t < GIVE_UP_S && t - calm_since < 1.0 {
                game.run(STEP_S);
                t += STEP_S;
                let now_reconciles = stats(&game, mover).reconciles;
                if (lead_of(&game) - usual).abs() > 2 || now_reconciles != reconciles {
                    calm_since = t;
                }
                reconciles = now_reconciles;
            }
            results.push(if t >= GIVE_UP_S { format!("not in {GIVE_UP_S} s") } else { format!("{calm_since:.2} s") });
        }
        let frames = throttled.map_or("none".to_owned(), |every| format!("every {} s", every / 1000.0));
        println!("{away:>4} s | {frames:<17} | {:>20} | {:>17}", results[0], results[1]);
    }
}

/// What an idle player on town costs, where most of what is in view is parked and never changes.
#[test]
#[ignore = "report"]
fn report_what_standing_still_costs_on_town() {
    for (name, net) in [("datagram", Net::datagram(50.0, 5.0, 0.0)), ("tcp", Net::tcp(50.0, 5.0, 0.0, 200.0))] {
        let game = SimServer::new("town", "aas", SimServerConfig { bots_per_team: Some(0), ..Default::default() }).expect("town");
        let parked = game.sim.vehicles.len();
        let mut world: Game = World::new(game, ServerConfig::default(), position_of);
        world.record = false;
        let idle = world.join(factory(), script(BotScript::Idle), &net);
        world.run(5.0);
        let before = world.players[idle].session.stats();
        let seconds = 30.0;
        world.run(seconds);
        let after = world.players[idle].session.stats();
        println!(
            "{name:<8} {parked} parked: down {:.0} B/s, {} missing baselines",
            (after.bytes_down - before.bytes_down) as f64 / seconds,
            after.missing_baselines,
        );
    }
}

/// Anything at all from a client: rubbish, well-formed nonsense, and everything out of order.
#[test]
fn a_client_sending_anything_cannot_bring_the_server_down() {
    let game = SimServer::new("town", "aas", SimServerConfig::default()).expect("the map and mode exist");
    let mut server = GameServer::new(game, ServerConfig::default());
    let rules = server.game.rules_hash();
    let mut seed = 0x9E37_79B9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut clients: Vec<u32> = Vec::new();
    for tick in 0..3000u32 {
        if tick % 100 == 0 {
            let id = server.connect(true);
            server.receive(id, &ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode());
            server.receive(id, &ClientMsg::Join { rules, name: String::new() }.encode());
            clients.push(id);
        }
        let who = clients[next() as usize % clients.len()];
        match next() % 5 {
            0 => {
                let junk: Vec<u8> = (0..next() % 300).map(|_| next() as u8).collect();
                server.receive(who, &junk);
            }
            1 => {
                let junk: Vec<u8> = (0..next() % 16).map(|_| next() as u8).collect();
                let packet = CmdPacket { requests: vec![(next() % 4, junk)], ..Default::default() };
                server.receive(who, &ClientMsg::Cmds(packet).encode());
            }
            2 => {
                let cmd = PlayerCmd {
                    tick: server.game.tick() + next() % 8,
                    wish_move: Vec2::new(f32::from_bits(next()), f32::from_bits(next())),
                    yaw: f32::from_bits(next()),
                    pitch: f32::from_bits(next()),
                    buttons: Buttons(next()),
                    stick: [next() as i8, next() as i8],
                };
                server.receive(who, &ClientMsg::Cmds(CmdPacket { cmds: vec![cmd], ack_snapshot: next(), ack_bits: next(), view_tick: next(), ..Default::default() }).encode());
            }
            3 => server.receive(who, &ClientMsg::Hello { protocol: next() as u16 }.encode()),
            _ => server.receive(who, &ClientMsg::Join { rules: next() as u64, name: String::new() }.encode()),
        }
        for kicked in server.drain_kicked() {
            clients.retain(|&id| id != kicked);
            server.disconnect(kicked);
        }
        if clients.is_empty() {
            let id = server.connect(true);
            server.receive(id, &ClientMsg::Hello { protocol: PROTOCOL_VERSION }.encode());
            server.receive(id, &ClientMsg::Join { rules, name: String::new() }.encode());
            clients.push(id);
        }
        server.tick();
        server.drain_outgoing();
        for soldier in &server.game.sim.soldiers {
            assert!(soldier.body.pos.is_finite() && soldier.yaw.is_finite(), "tick {tick}");
        }
    }
}
