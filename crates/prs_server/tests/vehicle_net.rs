//! A vehicle on a bad network: driven by nothing but commands on one client and watched from another,
//! against the real server.

use glam::{Quat, Vec2, Vec3};
use prs_net::server::{InterestConfig, ServerConfig};
use prs_net::testkit::Net;
use prs_net_sim::testkit::{SimWorld, client_on, world_with};
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::seats::SeatedAt;
use prs_sim::vehicles::VehicleEvent;
use prs_sim::{Level, Sim, SimEvent};
use std::collections::VecDeque;
use std::cell::Cell;
use std::rc::Rc;

/// Going, turning hard one way, braking to nearly nothing, off again and turning the other way.
fn drive_about(_: u32, tick: u32) -> PlayerCmd {
    let t = tick as f32 * TICK_DT;
    let phase = t % 12.0;
    let wish = match phase {
        p if p < 3.0 => Vec2::new(0.0, 1.0),
        p if p < 3.9 => Vec2::new(-1.0, 1.0).clamp_length_max(1.0),
        p if p < 6.0 => Vec2::new(0.0, -1.0),
        p if p < 8.0 => Vec2::new(0.0, 1.0),
        p if p < 8.9 => Vec2::new(1.0, 1.0).clamp_length_max(1.0),
        p if p < 11.0 => Vec2::new(0.0, 0.4),
        p if p < 11.5 => Vec2::new(0.0, -1.0),
        _ => Vec2::ZERO,
    };
    let mut buttons = Buttons::default();
    buttons.set(Buttons::JUMP, (11.0..11.5).contains(&phase));
    PlayerCmd { tick, wish_move: wish, buttons, ..Default::default() }
}

/// The ticks over which to judge how smoothly the driver's view moves.
const WINDOW: usize = 6;

struct Drive {
    world: SimWorld,
    driver: usize,
    watcher: usize,
    worst_offset: f32,
    worst_slide: f32,
    worst_drawn: f32,
    crashes: u32,
    track: Vec<(u32, Vec3)>,
    /// Times the gear changed, on the server and in what the driver's client predicted.
    server_shifts: u32,
    client_shifts: u32,
    server_gear: i8,
    client_gear: i8,
}

fn out_driving(net: &Net) -> Drive {
    out_driving_by(net, drive_about)
}

/// The same route, changing gear by hand: up twice on the way out of each corner, down on the way into the next.
fn shifting_about(id: u32, tick: u32) -> PlayerCmd {
    let mut cmd = drive_about(id, tick);
    let phase = tick % (12.0 / TICK_DT) as u32;
    let press = |at: f32| (0..3).contains(&phase.wrapping_sub((at / TICK_DT) as u32));
    cmd.buttons.set(Buttons::LEAN_RIGHT, press(1.0) || press(2.2) || press(7.0));
    cmd.buttons.set(Buttons::LEAN_LEFT, press(4.0) || press(5.0));
    cmd
}

fn out_driving_by(net: &Net, script: impl Fn(u32, u32) -> PlayerCmd + 'static) -> Drive {
    // Far things are drawn further back on purpose, and this is about how true the drawing is.
    let interest = InterestConfig { full_rate_within: 1e6, ..Default::default() };
    let mut world = world_with("range", ServerConfig { interest, ..Default::default() });
    let driver = world.join(client_on("range"), Rc::new(script), net);
    let watcher = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), net);
    world.run(1.5);
    let id = world.player_id(driver) as usize;
    world.server.game.sim.sit_in(id, SeatedAt { carrier: 0, seat: 0 });
    world.run(1.0);
    Drive { world, driver, watcher, worst_offset: 0.0, worst_slide: 0.0, worst_drawn: 0.0, crashes: 0, track: Vec::new(), server_shifts: 0, client_shifts: 0, server_gear: 1, client_gear: 1 }
}

impl Drive {
    fn run(&mut self, seconds: f32) {
        let mut views: VecDeque<(Vec3, Vec3)> = VecDeque::new();
        for _ in 0..(seconds / TICK_DT) as u32 {
            self.world.run(TICK_DT as f64);
            let server = &self.world.server.game.sim;
            self.track.push((server.tick, server.vehicles[0].pos));
            self.server_shifts += (server.vehicles[0].engine.gear != self.server_gear) as u32;
            self.server_gear = server.vehicles[0].engine.gear;
            self.crashes += server.events.iter().filter(|e| matches!(e, SimEvent::Vehicle { event: VehicleEvent::Crashed { .. }, .. })).count() as u32;
            let session = &self.world.players[self.driver].session;
            if let (Some(game), Some(predictor)) = (session.game(), session.predictor()) {
                self.client_shifts += (game.sim.vehicles[0].engine.gear != self.client_gear) as u32;
                self.client_gear = game.sim.vehicles[0].engine.gear;
                self.worst_offset = self.worst_offset.max(predictor.visual_offset().length());
                views.push_back((game.sim.vehicles[0].pos + predictor.visual_offset(), game.sim.vehicles[0].vel));
                if views.len() > WINDOW {
                    let (from, _) = views.pop_front().expect("just checked");
                    let to = views.back().expect("just pushed").0;
                    let n = views.len() as f32;
                    let vel = views.iter().fold(Vec3::ZERO, |sum, view| sum + view.1 / n);
                    self.worst_slide = self.worst_slide.max(((to - from) / (WINDOW as f32 * TICK_DT) - vel).length());
                }
            }
            let watching = &self.world.players[self.watcher].session;
            if let (Some(game), Some(interp)) = (watching.game(), watching.interp()) {
                let play = interp.play_tick();
                if let Some(at) = self.track.iter().position(|(tick, _)| *tick as f64 >= play) {
                    self.worst_drawn = self.worst_drawn.max(game.sim.vehicles[0].pos.distance(self.track[at].1));
                }
            }
        }
    }

    fn reconciles(&self) -> u32 {
        self.world.players[self.driver].session.predictor().map_or(0, |p| p.stats().reconciles)
    }
}

/// 100 ms each way, some jitter and nothing lost: turning, braking and all, the driver's own prediction
/// holds, so what they see moves as the truck does, and anyone watching sees it where it was.
#[test]
fn turning_and_braking_at_100_ms_stays_smooth_for_the_driver_and_true_for_the_watcher() {
    let mut drive = out_driving(&Net::datagram(100.0, 10.0, 0.0));
    let settled = drive.reconciles();
    let start = drive.world.server.game.sim.vehicles[0].pos;
    drive.run(36.0);
    let server = &drive.world.server.game.sim;
    let vehicle = &server.vehicles[0];
    println!(
        "36 s at 100 ms: went {:.0} m, {} crashes, {} reconciles, offset up to {:.3} m, view off the truck's speed by up to {:.3} m/s, watcher off by up to {:.3} m",
        vehicle.pos.distance(start),
        drive.crashes,
        drive.reconciles() - settled,
        drive.worst_offset,
        drive.worst_slide,
        drive.worst_drawn
    );
    assert!(vehicle.pos.distance(start) > 25.0, "it really drove");
    assert_eq!(drive.crashes, 0, "the route should be clear, so this tests driving");
    assert!(drive.reconciles() - settled <= 2, "{}", drive.reconciles() - settled);
    assert!(drive.worst_offset < 0.2, "{}", drive.worst_offset);
    // Sampled by the tick, a client running one tick more or fewer than the server in a window of six
    // is a tick of speed over that window: at this speed nearly 3 m/s without anything being wrong.
    assert!(drive.worst_slide < 3.5, "{}", drive.worst_slide);
    assert!(drive.worst_drawn < 1.0, "{}", drive.worst_drawn);
}

/// Changing gear is a command like any other: predicted the tick it is pressed, so the same 100 ms stays
/// within the same limits, and a gear that differed from the server's would be a reconcile.
#[test]
fn changing_gear_by_hand_at_100_ms_stays_within_the_same_limits() {
    let mut drive = out_driving_by(&Net::datagram(100.0, 10.0, 0.0), shifting_about);
    let settled = drive.reconciles();
    drive.run(36.0);
    let manual = drive.world.server.game.sim.vehicles[0].engine.manual;
    println!(
        "36 s shifting at 100 ms: {} changes on the server, {} on the client, {} crashes, {} reconciles, offset up to {:.3} m, view off by up to {:.3} m/s, watcher off by up to {:.3} m",
        drive.server_shifts,
        drive.client_shifts,
        drive.crashes,
        drive.reconciles() - settled,
        drive.worst_offset,
        drive.worst_slide,
        drive.worst_drawn
    );
    assert!(drive.server_shifts >= 10, "{}", drive.server_shifts);
    assert!(manual);
    assert!(drive.client_shifts.abs_diff(drive.server_shifts) <= 1, "{} against {}", drive.client_shifts, drive.server_shifts);
    assert_eq!(drive.crashes, 0, "the route should be clear, so this tests driving");
    assert!(drive.reconciles() - settled <= 2, "{}", drive.reconciles() - settled);
    assert!(drive.worst_offset < 0.2, "{}", drive.worst_offset);
    assert!(drive.worst_slide < 3.5, "{}", drive.worst_slide);
    assert!(drive.worst_drawn < 1.0, "{}", drive.worst_drawn);
}

/// A passenger on a client whose copy of the truck runs behind the server's, with somebody else driving it hard, keeping
/// their look on one spot inside as the real client does: it goes out in the world, carried by the truck they can see.
#[test]
fn a_passenger_at_100_ms_keeps_their_look_on_the_seat_while_someone_else_drives_hard() {
    let net = Net::datagram(100.0, 10.0, 0.0);
    // What the passenger's client knows of the truck, for the command it sends next.
    let seen = Rc::new(Cell::new(0.0_f32));
    let sent = seen.clone();
    let mut world = world_with("range", ServerConfig { interest: InterestConfig { full_rate_within: 1e6, ..Default::default() }, ..Default::default() });
    let driver = world.join(client_on("range"), Rc::new(drive_about), &net);
    let passenger = world.join(
        client_on("range"),
        Rc::new(move |_, tick| PlayerCmd { tick, yaw: sent.get() - std::f32::consts::FRAC_PI_2, pitch: 0.02, ..Default::default() }),
        &net,
    );
    let watcher = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    world.run(1.5);
    let (d, p) = (world.player_id(driver) as usize, world.player_id(passenger) as usize);
    world.server.game.sim.sit_in(d, SeatedAt { carrier: 0, seat: 0 });
    world.server.game.sim.sit_in(p, SeatedAt { carrier: 0, seat: 3 });
    world.run(2.0);
    let stats = |world: &SimWorld| world.players[passenger].session.predictor().map_or(0, |pred| pred.stats().reconciles);
    let settled = stats(&world);
    let (mut worst_view, mut worst_seen, mut worst_watched, mut turned, mut last) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, None);
    let relative = |sim: &Sim| {
        let frame = sim.vehicles[0].frame();
        let off = sim.soldiers[p].yaw - frame.yaw();
        (off + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
    };
    for _ in 0..(24.0 / TICK_DT) as u32 {
        let frame_yaw = world.players[passenger].session.game().map_or(0.0, |game| game.sim.vehicles[0].frame().yaw());
        seen.set(frame_yaw);
        world.run(TICK_DT as f64);
        worst_view = worst_view.max(world.players[passenger].session.predictor().map_or(0.0, |pred| pred.visual_offset().length()));
        worst_seen = worst_seen.max((relative(&world.players[passenger].session.game().unwrap().sim) + std::f32::consts::FRAC_PI_2).abs());
        worst_watched = worst_watched.max((relative(&world.players[watcher].session.game().unwrap().sim) + std::f32::consts::FRAC_PI_2).abs());
        let yaw = world.server.game.sim.vehicles[0].frame().yaw();
        if let Some(last) = last {
            let delta: f32 = yaw - last;
            turned += ((delta + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs();
        }
        last = Some(yaw);
    }
    println!(
        "24 s a passenger at 100 ms while it turned {:.0} degrees: {} reconciles, view off by {:.3} m, own look off the seat by {:.2} deg, watcher sees {:.2} deg off",
        turned.to_degrees(),
        stats(&world) - settled,
        worst_view,
        worst_seen.to_degrees(),
        worst_watched.to_degrees()
    );
    assert!(turned.to_degrees() > 90.0);
    // The ride shakes a gun a little differently on a truck 100 ms in the past (`PASSENGER_SLACK`), so now and then
    // the gun is put right. Their view is the seat's, so none of this moves it.
    assert!(stats(&world) - settled <= 40, "{}", stats(&world) - settled);
    assert!(worst_view < 0.3, "{worst_view}");
    assert!(worst_seen.to_degrees() < 3.0, "{}", worst_seen.to_degrees());
    // Their look reaches anyone else later than the truck does, as far as it turns in that time.
    assert!(worst_watched.to_degrees() < 15.0, "{}", worst_watched.to_degrees());
}

/// A handbrake turn is a command like any other, so at 100 ms the driver's client has it to the bit.
#[test]
fn a_handbrake_turn_at_100_ms_stays_within_the_same_limits() {
    let script = |id: u32, tick: u32| {
        let mut cmd = drive_about(id, tick);
        let phase = (tick as f32 * TICK_DT) % 6.0;
        if phase > 3.0 {
            cmd.wish_move = Vec2::new(1.0, 0.0);
        }
        cmd.buttons.set(Buttons::JUMP, (3.0..4.2).contains(&phase));
        cmd
    };
    let mut drive = out_driving_by(&Net::datagram(100.0, 10.0, 0.0), script);
    let settled = drive.reconciles();
    drive.run(24.0);
    println!("24 s of handbrake turns at 100 ms: {} reconciles, offset up to {:.3} m, watcher off by up to {:.3} m", drive.reconciles() - settled, drive.worst_offset, drive.worst_drawn);
    assert!(drive.reconciles() - settled <= 2, "{}", drive.reconciles() - settled);
    assert!(drive.worst_offset < 0.2, "{}", drive.worst_offset);
    assert!(drive.worst_drawn < 1.0, "{}", drive.worst_drawn);
}

/// Two on foot pushing a truck along at 100 ms, walking with it as it goes: what they see of themselves never jumps.
#[test]
fn pushing_a_truck_at_100_ms_never_jerks_the_pushers_about() {
    let net = Net::datagram(100.0, 10.0, 0.0);
    let aims = [Rc::new(Cell::new((0.0_f32, 0.0_f32, false))), Rc::new(Cell::new((0.0_f32, 0.0_f32, false)))];
    let mut world = world_with("range", ServerConfig { interest: InterestConfig { full_rate_within: 1e6, ..Default::default() }, ..Default::default() });
    let pushers: Vec<usize> = aims
        .iter()
        .map(|aim| {
            let held = aim.clone();
            world.join(
                client_on("range"),
                Rc::new(move |_, tick| {
                    let (yaw, pitch, on) = held.get();
                    let mut buttons = Buttons::default();
                    buttons.set(Buttons::USE, on);
                    PlayerCmd { tick, yaw, pitch, buttons, wish_move: if on { Vec2::Y } else { Vec2::ZERO }, ..Default::default() }
                }),
                &net,
            )
        })
        .collect();
    world.run(1.5);
    let ids: Vec<usize> = pushers.iter().map(|&p| world.player_id(p) as usize).collect();
    let sim = &mut world.server.game.sim;
    let truck = sim.vehicles[0].frame();
    let spots = [3usize, 5];
    for (&id, &spot) in ids.iter().zip(&spots) {
        let at = truck.point(sim.vehicle_cfgs[sim.vehicles[0].kind].push.spots[spot]);
        let back = truck.rot * Vec3::Z * 0.75;
        let feet = Vec3::new(at.x + back.x, 0.0, at.z + back.z);
        let top = feet + Vec3::Y * 10.0;
        let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 30.0).map_or(10.0, |hit| hit.distance);
        sim.soldiers[id].body.pos = Vec3::new(feet.x, ground + 0.02, feet.z);
        sim.soldiers[id].body.vel = Vec3::ZERO;
    }
    world.run(1.0);
    for ((&id, &spot), aim) in ids.iter().zip(&spots).zip(&aims) {
        let sim = &world.server.game.sim;
        let at = sim.vehicles[0].frame().point(sim.vehicle_cfgs[sim.vehicles[0].kind].push.spots[spot]);
        let to = (at - sim.soldiers[id].eye()).normalize();
        aim.set(((-to.x).atan2(-to.z), to.y.asin(), true));
    }
    let settled: Vec<u32> = pushers.iter().map(|&p| world.players[p].session.predictor().map_or(0, |pred| pred.stats().reconciles)).collect();
    let start = world.server.game.sim.vehicles[0].pos;
    let mut worst = 0.0_f32;
    for _ in 0..(8.0 / TICK_DT) as u32 {
        world.run(TICK_DT as f64);
        for &p in &pushers {
            worst = worst.max(world.players[p].session.predictor().map_or(0.0, |pred| pred.visual_offset().length()));
        }
    }
    let went = world.server.game.sim.vehicles[0].pos.distance(start);
    let reconciles: Vec<u32> = pushers.iter().zip(&settled).map(|(&p, was)| world.players[p].session.predictor().map_or(0, |pred| pred.stats().reconciles) - was).collect();
    println!("8 s of two pushing at 100 ms: the truck went {went:.1} m, {reconciles:?} reconciles, view off by up to {worst:.3} m");
    assert!(went > 3.0, "{went}");
    assert!(ids.iter().all(|&id| world.server.game.sim.soldiers[id].pushing.is_some()), "still at it");
    assert!(worst < 0.1, "{worst}");
}

/// One soldier on foot righting a truck at 100 ms, shot part way through. Everyone sees the same thing: the hands on
/// it, the truck coming up and falling back, including someone who arrives while it is going on.
#[test]
fn righting_a_truck_at_100_ms_and_being_shot_doing_it_looks_the_same_to_everyone() {
    let net = Net::datagram(100.0, 10.0, 0.0);
    let aim = Rc::new(Cell::new((0.0_f32, 0.0_f32, false)));
    let held = aim.clone();
    let mut world = world_with("range", ServerConfig { interest: InterestConfig { full_rate_within: 1e6, ..Default::default() }, ..Default::default() });
    let lifter = world.join(
        client_on("range"),
        Rc::new(move |_, tick| {
            let (yaw, pitch, on) = held.get();
            let mut buttons = Buttons::default();
            buttons.set(Buttons::USE, on);
            PlayerCmd { tick, yaw, pitch, buttons, ..Default::default() }
        }),
        &net,
    );
    let watcher = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    world.run(1.5);
    let id = world.player_id(lifter) as usize;
    let sim = &mut world.server.game.sim;
    let side = {
        let truck = &mut sim.vehicles[0];
        truck.rot *= Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        truck.pos.y += 1.2;
        prs_sim::vehicles::wake(truck);
        truck.rot * Vec3::Y
    };
    world.run(4.0);
    let sim = &mut world.server.game.sim;
    let up = |sim: &prs_sim::Sim| (sim.vehicles[0].rot * Vec3::Y).y;
    let truck = sim.vehicles[0].pos;
    let top = truck + side * 2.2 + Vec3::Y * 10.0;
    let ground = top.y - sim.world.raycast(top, Vec3::NEG_Y, 30.0).map_or(10.0, |hit| hit.distance);
    sim.soldiers[id].body.pos = Vec3::new(top.x, ground + 0.02, top.z);
    sim.soldiers[id].body.vel = Vec3::ZERO;
    world.run(1.0);
    let sim = &world.server.game.sim;
    let eye = sim.soldiers[id].eye();
    let to = (sim.nearest_of_hull(0, eye) - eye).normalize_or(Vec3::NEG_Z);
    aim.set(((-to.x).atan2(-to.z), to.y.asin(), false));
    world.run(0.5);
    let settled = world.players[lifter].session.predictor().map_or(0, |p| p.stats().reconciles);
    aim.set(((-to.x).atan2(-to.z), to.y.asin(), true));
    world.run(2.5);

    // Part way up, and somebody turns up.
    let late = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    world.run(1.0);
    let up_before = up(&world.server.game.sim);
    assert!(up_before > 0.2, "it should be coming up: {up_before}");
    assert!(world.server.game.sim.soldiers[id].lifting.is_some());
    for (name, who) in [("the lifter", lifter), ("a watcher", watcher), ("someone new", late)] {
        let seen = world.players[who].session.game().expect("in the game");
        assert!((up(&seen.sim) - up_before).abs() < 0.1, "{name} sees {} and the server has {up_before}", up(&seen.sim));
        assert_eq!(seen.sim.soldiers[id].lifting, Some(0), "{name} sees the hands on it");
    }
    assert!(world.players[watcher].session.game().unwrap().sim.soldiers[id].anim.heave > 0.5);

    let while_lifting = world.players[lifter].session.predictor().map_or(0, |p| p.stats().reconciles) - settled;
    assert_eq!(while_lifting, 0, "lifting is all predicted");

    world.server.game.sim.knock(id, 5.0);
    assert_eq!(world.server.game.sim.soldiers[id].lifting, None);
    aim.set(((-to.x).atan2(-to.z), to.y.asin(), false));
    // It comes down against them, and a client's hull is a moment behind the server's, so the shove is put right
    // as it happens. That has to stay something nobody sees.
    let mut worst_offset = 0.0_f32;
    for _ in 0..(4.0 / TICK_DT) as u32 {
        world.run(TICK_DT as f64);
        worst_offset = worst_offset.max(world.players[lifter].session.predictor().map_or(0.0, |p| p.visual_offset().length()));
    }
    let down = up(&world.server.game.sim);
    assert!(down < 0.1, "it should have fallen back: {down}");
    let stats = world.players[lifter].session.predictor().map(|p| p.stats()).expect("playing");
    println!("up to {up_before:.2}, fell to {down:.2}, {} reconciles while it fell, view off by {worst_offset:.3} m at most", stats.reconciles - settled);
    assert!(worst_offset < 0.25, "{worst_offset}");
    for (name, who) in [("the lifter", lifter), ("a watcher", watcher), ("someone new", late)] {
        let seen = world.players[who].session.game().expect("in the game");
        assert!((up(&seen.sim) - down).abs() < 0.02, "{name} sees {} and the server has {down}", up(&seen.sim));
        assert!(seen.sim.vehicles[0].pos.distance(world.server.game.sim.vehicles[0].pos) < 0.1, "{name}");
        assert_eq!(seen.sim.soldiers[id].lifting, None, "{name}");
    }
}

/// Lost packets force real corrections, and the view has to take those without jumping.
#[test]
fn a_correction_on_a_bad_link_never_jumps_the_view() {
    let mut drive = out_driving(&Net::datagram(100.0, 20.0, 0.05));
    drive.run(40.0);
    println!(
        "40 s at 100 ms and 5% loss: {} reconciles, offset up to {:.3} m, view off the truck's speed by up to {:.3} m/s",
        drive.reconciles(),
        drive.worst_offset,
        drive.worst_slide
    );
    assert!(drive.worst_slide < 4.0, "{}", drive.worst_slide);
}

/// The server has the truck somewhere else all of a sudden, as a crash the driver did not foresee would.
/// They get put right, without the view lurching, and it all fades away.
#[test]
fn a_big_correction_fades_in_without_a_lurch() {
    let mut drive = out_driving(&Net::datagram(100.0, 0.0, 0.0));
    drive.run(5.0);
    let before = drive.reconciles();
    assert_eq!(before, drive.reconciles());
    drive.world.server.game.sim.vehicles[0].pos.x += 1.5;
    drive.worst_slide = 0.0;
    drive.worst_offset = 0.0;
    drive.run(5.0);
    println!("1.5 m shove: {} reconciles, offset up to {:.3} m, view off by up to {:.3} m/s", drive.reconciles() - before, drive.worst_offset, drive.worst_slide);
    assert!(drive.reconciles() > before, "that should have been news");
    assert!(drive.worst_offset > 1.0, "the correction should be drawn as an offset, and was {}", drive.worst_offset);
    assert!(drive.worst_slide < 6.5, "the most it fades at, on top of tick sampling: {}", drive.worst_slide);
    let session = &drive.world.players[drive.driver].session;
    assert!(session.predictor().unwrap().visual_offset().length() < 0.02, "and it has faded away by now");
}

/// Standing on a hull someone else is driving. Nobody can predict the driver, so the soldier predicts
/// against the hull as drawn, in the past, and the server has them on it in the present.
#[test]
fn standing_on_a_moving_deck_stays_on_it_and_corrections_stay_small() {
    let net = Net::datagram(100.0, 10.0, 0.0);
    let mut world = world_with("range", ServerConfig::default());
    let car = world.server.game.sim.vehicles.iter().position(|v| v.kind == prs_sim::vehicles::VehicleTable::ARMOURED_CAR).expect("a car on the range");
    let ride_gently = |_: u32, tick: u32| PlayerCmd { tick, wish_move: Vec2::new(0.0, if tick % 240 < 90 { 0.5 } else { 0.0 }), ..Default::default() };
    let driver = world.join(client_on("range"), Rc::new(ride_gently), &net);
    let rider = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    world.run(1.5);
    let (driver_id, rider_id) = (world.player_id(driver) as usize, world.player_id(rider) as usize);
    let server = &mut world.server.game.sim;
    server.sit_in(driver_id, SeatedAt { carrier: car as u32, seat: 0 });
    let deck = server.carrier_frame(car as u32).point(Vec3::new(0.0, 1.05, 1.6));
    server.soldiers[rider_id].body.pos = deck;
    world.run(3.0);
    let predictor = |world: &SimWorld| world.players[rider].session.predictor().unwrap().stats();
    let before = predictor(&world);
    let start = world.server.game.sim.vehicles[car].pos;
    world.run(16.0);
    let server = &world.server.game.sim;
    let moved = server.vehicles[car].pos.distance(start);
    let local = server.carrier_frame(car as u32).to_local(server.soldiers[rider_id].body.pos);
    let after = predictor(&world);
    println!("on a deck for 16 s over {moved:.0} m: {} reconciles of {} compared, biggest correction {:.3} m, ends at {local} on the hull", after.reconciles - before.reconciles, after.compared - before.compared, after.max_correction);
    assert!(moved > 5.0, "it should have gone somewhere: {moved}");
    assert!(local.y > 0.9 && local.y < 1.2, "should still be stood on the deck: {local}");
    assert!(after.reconciles - before.reconciles <= (after.compared - before.compared) / 20, "{after:?}");
}

/// A client can only ask. Pressing F from across the map gets it nothing, and the server's word is final.
#[test]
fn no_client_gets_into_anything_it_is_not_standing_at() {
    let mut world = world_with("range", ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    let press_use = |_: u32, tick: u32| {
        let mut buttons = Buttons::default();
        buttons.set(Buttons::USE, tick % 2 == 0);
        PlayerCmd { tick, buttons, ..Default::default() }
    };
    let hacker = world.join(client_on("range"), Rc::new(press_use), &net);
    world.run(5.0);
    let id = world.player_id(hacker) as usize;
    let server = &world.server.game.sim;
    assert_eq!(server.soldiers[id].seat, None);
    assert!(server.vehicles.iter().all(|vehicle| vehicle.occupants.is_empty()));
    let own = &world.players[hacker].session.game().unwrap().sim;
    assert_eq!(own.soldiers[id].seat, None);
}

/// Someone else climbing in is seen climbing in, with the same motion and in the same place.
#[test]
fn climbing_into_a_seat_is_seen_by_everyone_on_the_way() {
    let mut world = world_with("range", ServerConfig::default());
    let net = Net::datagram(50.0, 0.0, 0.0);
    let climber = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    let watcher = world.join(client_on("range"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &net);
    world.run(1.5);
    let id = world.player_id(climber) as usize;
    let server = &mut world.server.game.sim;
    server.sit_in(id, SeatedAt { carrier: 0, seat: 1 });
    server.soldiers[id].transit = Some(prs_sim::seats::Transit::new(prs_sim::seats::TransitKind::In { entry: 1 }));
    world.run(0.5);
    let seen = &world.players[watcher].session.game().unwrap().sim;
    let transit = seen.soldiers[id].transit.expect("seen on the way in");
    assert!(transit.t > 0.0 && transit.t < 1.0, "{transit:?}");
    let eye = seen.soldiers[id].eye();
    let truth = world.server.game.sim.soldiers[id].eye();
    assert!(eye.distance(truth) < 0.4, "{eye} vs {truth}");
    world.run(1.5);
    let seen = &world.players[watcher].session.game().unwrap().sim;
    assert_eq!(seen.soldiers[id].transit, None);
    assert_eq!(seen.soldiers[id].seat, Some(SeatedAt { carrier: 0, seat: 1 }));
}

/// What a client has to pay when it reconciles: replaying commands from an old state.
#[test]
#[ignore]
fn replay_cost_report() {
    for map in ["range", "town"] {
        let mut sim = Sim::new(Level::named(map).unwrap());
        for _ in 0..120 {
            sim.step(&[]);
        }
        let id = sim.spawn_soldier(1);
        sim.sit_in(id, SeatedAt { carrier: 0, seat: 0 });
        for tick in 0..240 {
            sim.step(&[(id, PlayerCmd { tick, wish_move: Vec2::new(0.0, 1.0), ..Default::default() })]);
        }
        let saved = sim.export_state();
        let start = std::time::Instant::now();
        let rounds = 200;
        for _ in 0..rounds {
            sim.import_state(&saved);
            for tick in 0..20 {
                let cmd = PlayerCmd { tick: saved.tick + tick + 1, wish_move: Vec2::new((tick as f32 * 0.1).sin(), 1.0), ..Default::default() };
                sim.step_predicted(id, &cmd);
            }
        }
        println!("{map}: {:?} to restore and replay 20 ticks of a driver", start.elapsed() / rounds);
    }
}

/// `cargo test -p prs_server --test vehicle_net parked_cost_report -- --ignored --nocapture`. What the parked vehicles
/// on the town cost: bytes a second to one idle player, and sim time a tick, with them parked and with them woken.
#[test]
#[ignore]
fn parked_cost_report() {
    for (woken, none) in [(false, true), (false, false), (true, false)] {
        let mut world = world_with("town", ServerConfig::default());
        if none {
            world.server.game.sim.vehicles.clear();
            world.server.game.sim.vehicle_doors.clear();
        }
        let me = world.join(client_on("town"), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), &Net::datagram(50.0, 0.0, 0.0));
        world.run(4.0);
        let was = world.players[me].session.stats();
        let seconds = 10.0_f64;
        for _ in 0..(seconds / TICK_DT as f64) as u32 {
            if woken {
                world.server.game.sim.vehicles.iter_mut().for_each(prs_sim::vehicles::wake);
            }
            world.run(TICK_DT as f64);
        }
        let now = world.players[me].session.stats();
        let mut sim = Sim::new(Level::named("town").unwrap());
        for _ in 0..240 {
            sim.step(&[]);
        }
        let start = std::time::Instant::now();
        for _ in 0..600 {
            if woken {
                sim.vehicles.iter_mut().for_each(prs_sim::vehicles::wake);
            }
            sim.step(&[]);
        }
        println!(
            "{} vehicles {}: {:.0} B/s down to an idle player, sim step {:.3} ms",
            if none { 0 } else { sim.vehicles.len() },
            if woken { "woken every tick" } else { "parked" },
            (now.bytes_down - was.bytes_down) as f64 / seconds,
            start.elapsed().as_secs_f64() * 1000.0 / 600.0
        );
    }
}
