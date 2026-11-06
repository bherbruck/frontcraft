//! Climbing and bumping into people on a real network, at 100 ms each way. A climb up a wall is yours
//! alone and is never wrong. Anything to do with another soldier is seen in the past and can be, so what
//! is measured there is how little it shows.

use crate::testkit::*;
use glam::{Quat, Vec2, Vec3};
use prs_net::predict::PredictStats;
use prs_sim::movement::{ClimbKind, Stance};
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::{Sim, SoldierId, Surface};
use std::cell::Cell;
use std::rc::Rc;

const LATENCY: f64 = 100.0;

fn net() -> Net {
    Net::datagram(LATENCY, 0.0, 0.0)
}

fn stats(world: &SimWorld, p: usize) -> PredictStats {
    world.players[p].session.predictor().expect("joined").stats()
}

/// Gets a command from who a player is and what tick it is, starting from a tick well into the game.
fn cmd(tick: u32, wish: Vec2, buttons: &[Buttons]) -> PlayerCmd {
    let mut held = Buttons::default();
    for &b in buttons {
        held.set(b, true);
    }
    PlayerCmd { tick, wish_move: wish, buttons: held, ..Default::default() }
}

fn sims(world: &mut SimWorld, p: usize, change: impl Fn(&mut Sim)) {
    change(&mut world.server.game.sim);
    let client = world.players[p].session.game_mut().expect("joined");
    change(&mut client.sim);
}

fn ground_block(sim: &mut Sim, height: f32, at: Vec3) {
    sim.world.add_box(at + Vec3::Y * (height * 0.5), Vec3::new(3.0, height * 0.5, 2.0), Quat::IDENTITY, Surface::Concrete);
}

/// The worst jump the view makes between frames, beyond what the steady pace of a walk would account for.
fn worst_step(world: &SimWorld, p: usize, from: usize) -> f32 {
    world.players[p].frames[from..].windows(2).map(|w| w[0].view.distance(w[1].view)).fold(0.0, f32::max)
}

fn sim_contact(world: &SimWorld) -> f32 {
    world.server.game.sim.move_cfg.crowd.contact
}

/// Pressing jump at a wall every so often, all the way up a stack of ledges. Nothing else in it, so no
/// tick of it may ever disagree.
#[test]
fn climbing_a_wall_never_reconciles() {
    let mut world = world_on("range");
    let me = world.join(client_on("range"), script(|_, tick| cmd(tick, Vec2::Y, if tick % 40 == 0 { &[Buttons::JUMP] } else { &[] })), &net());
    world.run(1.0);
    let id = world.player_id(me) as SoldierId;
    let at = world.server.game.sim.soldiers[id].body.pos;
    let forward = prs_sim::movement::yaw_basis(world.server.game.sim.soldiers[id].yaw).0;
    assert!(forward.length() > 0.5);
    for (i, height) in [1.0, 1.4, 2.1, 0.9].iter().enumerate() {
        let spot = at + Vec3::new(0.0, i as f32 * 2.0, -4.0 - i as f32 * 4.0);
        sims(&mut world, me, |sim| ground_block(sim, *height, spot));
    }
    let before = stats(&world, me);
    let mut climbed = [false; 5];
    for _ in 0..(12.0 * 60.0) as usize {
        world.run(1.0 / 60.0);
        let kind = world.server.game.sim.soldiers[id].body.climb.kind;
        climbed[prs_climb_slot(kind)] = true;
    }
    let result = stats(&world, me);
    assert_eq!(result.reconciles, before.reconciles, "{result:?}");
    assert_eq!(result.max_error, 0.0, "{result:?}");
    assert!(climbed[1] || climbed[2] || climbed[3], "never climbed anything: {:?}", world.server.game.sim.soldiers[id].body.pos);
}

fn prs_climb_slot(kind: ClimbKind) -> usize {
    match kind {
        ClimbKind::None => 0,
        ClimbKind::Vault => 1,
        ClimbKind::Mantle => 2,
        ClimbKind::PullUp => 3,
        ClimbKind::Stool => 4,
    }
}

struct Pair {
    world: SimWorld,
    me: usize,
    id: SoldierId,
    stool: SoldierId,
}

/// A player stood behind a crouched soldier the server moves itself, in the open.
fn pair(cmds: impl Fn(u32, u32) -> PlayerCmd + 'static) -> Pair {
    let mut world = world_on("range");
    let me = world.join(client_on("range"), script(cmds), &net());
    world.run(0.5);
    let id = world.player_id(me) as SoldierId;
    let stool = world.server.game.sim.spawn_soldier(99);
    let at = world.server.game.sim.soldiers[id].body.pos;
    let sim = &mut world.server.game.sim;
    sim.set_home(stool, at + Vec3::new(0.0, 0.0, -0.8), 0.0);
    sim.soldiers[stool].body.stance = Stance::Crouch;
    sim.soldiers[stool].body.eye_height = sim.move_cfg.crouch_eye;
    world.run(0.6);
    Pair { world, me, id, stool }
}

/// Getting onto a back that is keeping still is agreed on, as the place on it comes from the wire's own grid.
#[test]
fn climbing_onto_a_still_back_agrees() {
    let first_press = 90;
    let Pair { mut world, me, id, stool } = pair(move |_, tick| match tick {
        t if t < first_press => cmd(t, Vec2::ZERO, &[]),
        t if t == first_press => cmd(t, Vec2::Y, &[Buttons::JUMP]),
        t if t < first_press + 60 => cmd(t, Vec2::Y, &[]),
        t => cmd(t, Vec2::ZERO, &[]),
    });
    let from = world.players[me].frames.len();
    world.run(4.0);
    let server = &world.server.game.sim;
    assert_eq!(server.soldiers[id].body.perch.on, Some(stool as u32), "at {}", server.soldiers[id].body.pos);
    let result = stats(&world, me);
    assert!(result.reconciles <= 1, "{result:?}");
    assert!(result.max_correction < 0.02, "{result:?}");
    let client = world.players[me].session.game().unwrap();
    assert_eq!(client.sim.soldiers[id].body.perch.on, Some(stool as u32));
    assert!(worst_step(&world, me, from) < 0.15, "{}", worst_step(&world, me, from));
}

/// The back stands up under them halfway through the climb. It is news to them for a round trip, and
/// then they are where the server says, without a teleport or being thrown anywhere.
#[test]
fn the_back_standing_up_mid_climb_resolves_cleanly() {
    let first_press = 90;
    let Pair { mut world, me, id, stool } = pair(move |_, tick| match tick {
        t if t == first_press => cmd(t, Vec2::Y, &[Buttons::JUMP]),
        t if t > first_press && t < first_press + 60 => cmd(t, Vec2::Y, &[]),
        t => cmd(t, Vec2::ZERO, &[]),
    });
    let from = world.players[me].frames.len();
    let mut stood = false;
    let (mut highest_after, mut fastest): (f32, f32) = (f32::MIN, 0.0);
    for _ in 0..(4.0 * 60.0) as usize {
        world.run(1.0 / 60.0);
        let server = &mut world.server.game.sim;
        let climb = server.soldiers[id].body.climb;
        assert!(!stood || climb.t < 0.6, "still climbing onto someone who stood up");
        if !stood && climb.kind == ClimbKind::Stool && climb.t > 0.5 {
            stood = true;
            let body = &mut server.soldiers[stool].body;
            body.stance = Stance::Stand;
            body.eye_height = server.move_cfg.stand_eye;
        }
        if stood && !server.soldiers[id].body.climb.active() {
            let body = &server.soldiers[id].body;
            highest_after = highest_after.max(body.vel.y);
            fastest = fastest.max(body.horizontal_speed());
        }
    }
    assert!(stood, "never saw them climbing");
    let server = &world.server.game.sim;
    let body = &server.soldiers[id].body;
    assert_eq!(body.perch.on, None);
    assert!(body.grounded, "{}", body.pos);
    assert!(highest_after <= 1e-3, "launched: {highest_after}");
    assert!(fastest <= server.move_cfg.run_speed, "{fastest}");
    let result = stats(&world, me);
    assert!(result.max_correction < 1.0, "{result:?}");
    // Corrections are blended, so nothing on screen jumps by much more than the correction allows in a frame.
    assert!(worst_step(&world, me, from) < 0.3, "{}", worst_step(&world, me, from));
    let client = world.players[me].session.game().unwrap();
    assert!(client.sim.soldiers[id].body.pos.distance(body.pos) < 0.01, "{}", client.sim.soldiers[id].body.pos);
    assert_eq!(client.sim.soldiers[id].body.perch.on, None);
}

/// The same with someone already up there. They find out late and fall a bit late, and it fades in.
#[test]
fn being_shrugged_off_is_a_bounded_correction() {
    let first_press = 90;
    let Pair { mut world, me, id, stool } = pair(move |_, tick| match tick {
        t if t == first_press => cmd(t, Vec2::Y, &[Buttons::JUMP]),
        t if t > first_press && t < first_press + 60 => cmd(t, Vec2::Y, &[]),
        t => cmd(t, Vec2::ZERO, &[]),
    });
    world.run(3.0);
    assert_eq!(world.server.game.sim.soldiers[id].body.perch.on, Some(stool as u32));
    let before = stats(&world, me);
    let from = world.players[me].frames.len();
    let server = &mut world.server.game.sim;
    let stand_eye = server.move_cfg.stand_eye;
    server.soldiers[stool].body.stance = Stance::Stand;
    server.soldiers[stool].body.eye_height = stand_eye;
    world.run(2.0);
    let result = stats(&world, me);
    assert!(result.reconciles - before.reconciles <= 8, "{result:?}");
    // At worst they had already come all the way down before they heard about it.
    let fall = world.server.game.sim.move_cfg.stool.back_height + 0.05;
    assert!(result.max_correction < fall, "{result:?}");
    assert!(worst_step(&world, me, from) < 0.3, "{}", worst_step(&world, me, from));
    let body = &world.server.game.sim.soldiers[id].body;
    assert_eq!(body.perch.on, None);
    assert!(body.grounded);
    let client = world.players[me].session.game().unwrap();
    assert!(client.sim.soldiers[id].body.pos.distance(body.pos) < 0.01);
}

/// Walking into a soldier who is keeping still is the same on both sides, to the bit, up to them being
/// moved aside, which is the part only the server knows about at first.
#[test]
fn walking_into_someone_stays_a_gentle_correction() {
    let start = 180;
    let Pair { mut world, me, id, stool } = pair(move |_, tick| if tick < start { cmd(tick, Vec2::ZERO, &[]) } else { cmd(tick, Vec2::Y, &[Buttons::SPRINT]) });
    let sim = &mut world.server.game.sim;
    sim.soldiers[stool].body.stance = Stance::Stand;
    sim.soldiers[stool].body.eye_height = sim.move_cfg.stand_eye;
    let front = sim.soldiers[id].body.pos + Vec3::new(0.0, 0.0, -4.0);
    sim.set_home(stool, front, 0.0);
    let from = world.players[me].frames.len();
    world.run(6.0);
    assert!(world.server.game.sim.soldiers[id].body.pos.z < front.z, "never got by: {}", world.server.game.sim.soldiers[id].body.pos);
    let result = stats(&world, me);
    println!("into someone standing: {result:?}, worst frame {}", worst_step(&world, me, from));
    assert!(result.max_correction < 0.3, "{result:?}");
    assert!(result.max_error < 0.1, "{result:?}");
    // A sprint is a tenth of a metre a tick, and that much again on top of it is the most it ever adds.
    assert!(worst_step(&world, me, from) < 0.2, "{}", worst_step(&world, me, from));
}

/// Two players running into each other head on, each only seeing the other in the past: the hard case.
#[test]
fn two_players_running_into_each_other_stays_a_gentle_correction() {
    let mut world = world_on("range");
    let start = 180;
    let forward = move |who: u32, tick: u32| {
        let wish = if tick < start { Vec2::ZERO } else { Vec2::Y };
        PlayerCmd { yaw: if who == 0 { 0.0 } else { std::f32::consts::PI }, ..cmd(tick, wish, &[]) }
    };
    let a = world.join(client_on("range"), script(forward), &net());
    let b = world.join(client_on("range"), script(forward), &net());
    world.run(1.0);
    let (ia, ib) = (world.player_id(a) as SoldierId, world.player_id(b) as SoldierId);
    let at = world.server.game.sim.soldiers[ia].body.pos;
    let sim = &mut world.server.game.sim;
    // Being put there is a correction of their own, so it is the other one who is measured.
    sim.soldiers[ib].body.pos = at + Vec3::new(0.05, 0.0, -3.5);
    let from = world.players[a].frames.len();
    let mut closest = f32::MAX;
    for _ in 0..(8.0 * 60.0) as usize {
        world.run(1.0 / 60.0);
        let sim = &world.server.game.sim;
        closest = closest.min(sim.soldiers[ia].body.pos.distance(sim.soldiers[ib].body.pos));
    }
    assert!(closest < sim_contact(&world), "never touched: {closest}");
    let result = stats(&world, a);
    println!("running into each other: {result:?}, worst frame {}", worst_step(&world, a, from));
    assert!(result.max_error < 0.1, "{result:?}");
    // Each on the far side of the other sooner or later than the other's guess, and it is on screen for
    // as long as it fades, so what matters is the frame below: scarcely more than a walking step.
    assert!(result.max_correction < 0.6, "{result:?}");
    assert!(worst_step(&world, a, from) < 0.15, "{}", worst_step(&world, a, from));
    let sim = &world.server.game.sim;
    assert!(sim.soldiers[ia].body.pos.z < sim.soldiers[ib].body.pos.z, "still in each other's way: {} and {}", sim.soldiers[ia].body.pos, sim.soldiers[ib].body.pos);
}

/// Everyone else sees the climb, hands on the ledge and gun lowered, and who is on whose back.
#[test]
fn others_see_it_as_it_is() {
    let first_press = 90;
    let Pair { mut world, me, id, stool } = pair(move |_, tick| match tick {
        t if t == first_press => cmd(t, Vec2::Y, &[Buttons::JUMP]),
        t if t > first_press && t < first_press + 60 => cmd(t, Vec2::Y, &[]),
        t => cmd(t, Vec2::ZERO, &[]),
    });
    let watcher = world.join(client_on("range"), script(|_, tick| cmd(tick, Vec2::ZERO, &[])), &net());
    let (mut saw_hands, mut saw_gun_down) = (false, false);
    for _ in 0..(4.0 * 60.0) as usize {
        world.run(1.0 / 60.0);
        let Some(client) = world.players[watcher].session.game() else { continue };
        if client.sim.soldiers.len() <= id {
            continue;
        }
        let input = client.sim.body_input(id);
        if let Some(hold) = input.ledge {
            saw_hands |= hold.grip > 0.5;
            saw_gun_down |= input.ready < 0.4;
        }
    }
    assert!(saw_hands && saw_gun_down, "hands {saw_hands}, gun {saw_gun_down}");
    let client = world.players[watcher].session.game().unwrap();
    assert_eq!(client.sim.soldiers[id].body.perch.on, Some(stool as u32));
    assert!(client.sim.soldiers[stool].anim.brace > 0.9, "{}", client.sim.soldiers[stool].anim.brace);
    assert!(client.sim.body_input(id).perched);
    let _ = me;
}

/// Being in contact for seconds on end, as in a crowd, and not only the moment of passing: four players
/// going round in tight circles, penned in together so that they can't get away. However much they disagree it has to stay a nudge on
/// screen, so this is the frame-to-frame number that a player sees.
#[test]
fn sustained_contact_stays_a_nudge_on_screen() {
    let mut world = world_on("range");
    let circling = |who: u32, tick: u32| PlayerCmd { yaw: who as f32 * 1.3 + tick as f32 / 60.0 * 7.0, ..cmd(tick, Vec2::Y, &[]) };
    let players: Vec<usize> = (0..4).map(|_| world.join(client_on("range"), script(circling), &net())).collect();
    world.run(1.0);
    let ids: Vec<SoldierId> = players.iter().map(|&p| world.player_id(p) as SoldierId).collect();
    let middle = world.server.game.sim.soldiers[ids[0]].body.pos;
    let wall = |sim: &mut Sim| {
        for (at, half) in [(Vec3::X, Vec3::new(0.2, 2.0, 1.2)), (Vec3::NEG_X, Vec3::new(0.2, 2.0, 1.2)), (Vec3::Z, Vec3::new(1.2, 2.0, 0.2)), (Vec3::NEG_Z, Vec3::new(1.2, 2.0, 0.2))] {
            sim.world.add_box(middle + at * 1.0, half, Quat::IDENTITY, Surface::Concrete);
        }
    };
    wall(&mut world.server.game.sim);
    for &p in &players {
        wall(&mut world.players[p].session.game_mut().expect("joined").sim);
    }
    for (i, &id) in ids.iter().enumerate() {
        world.server.game.sim.soldiers[id].body.pos = middle + Vec3::new(i as f32 * 0.2 - 0.3, 0.05, 0.0);
    }
    // Joining and being put there are jumps of their own, and have nothing to do with this.
    world.run(2.0);
    let from: Vec<usize> = players.iter().map(|&p| world.players[p].frames.len()).collect();
    let contact = sim_contact(&world);
    let mut touching = [0u32; 4];
    let seconds = 30.0;
    for _ in 0..(seconds * 60.0) as usize {
        world.run(1.0 / 60.0);
        let sim = &world.server.game.sim;
        for (i, &a) in ids.iter().enumerate() {
            let near = ids.iter().any(|&b| b != a && sim.soldiers[a].body.pos.distance(sim.soldiers[b].body.pos) < contact);
            touching[i] += u32::from(near);
        }
    }
    let run_step = world.server.game.sim.move_cfg.run_speed / 60.0;
    for (i, &p) in players.iter().enumerate() {
        let worst = worst_step(&world, p, from[i]);
        let result = stats(&world, p);
        println!("sustained contact: player {i} touching {:.1} s, worst frame {worst:.3} m against a step of {run_step:.3}, {result:?}", touching[i] as f32 / 60.0);
        assert!(touching[i] > 60 * 3, "only touched for {} ticks, so this proves nothing", touching[i]);
        assert!(worst < run_step + 0.05, "worst frame {worst} m");
        // Largest error and correction count being put in the pen as well, so they say little. This does not.
        assert!(result.reconciles * 10 < result.compared * 6, "{result:?}");
    }
}

/// Setting a gun down and lifting it is all the player's own, in a world that isn't moving, so it never disagrees.
#[test]
fn resting_a_gun_in_and_out_never_reconciles() {
    let mut world = world_on("range");
    let rest_key = |tick: u32| tick % 240 == 60 || tick % 240 == 200;
    let me = world.join(
        client_on("range"),
        script(move |_, tick| {
            let mut held = Buttons::default();
            held.set(Buttons::REST, rest_key(tick));
            held.set(Buttons::ADS, tick % 240 > 90 && tick % 240 < 190);
            held.set(Buttons::FIRE, tick % 240 == 150);
            let swing = ((tick % 240) as f32 / 240.0 * std::f32::consts::TAU).sin() * 0.3;
            PlayerCmd { tick, yaw: swing, pitch: swing * 0.3, buttons: held, ..Default::default() }
        }),
        &net(),
    );
    world.run(1.0);
    let id = world.player_id(me) as SoldierId;
    let at = world.server.game.sim.soldiers[id].body.pos;
    // Chest high, with its near face a short way in front of them.
    sims(&mut world, me, |sim| {
        sim.world.add_box(at + Vec3::new(0.0, 0.65, -0.75), Vec3::new(3.0, 0.65, 0.3), Quat::IDENTITY, Surface::Concrete);
    });
    let before = stats(&world, me);
    let (mut rested, mut lifted) = (0, 0);
    let mut was = false;
    for _ in 0..(16.0 * 60.0) as usize {
        world.run(1.0 / 60.0);
        let on = world.server.game.sim.soldiers[id].rest.on;
        rested += u32::from(on && !was);
        lifted += u32::from(!on && was);
        was = on;
    }
    assert!(rested >= 3 && lifted >= 3, "only rested {rested} and lifted {lifted}");
    let result = stats(&world, me);
    assert_eq!(result.reconciles, before.reconciles, "{result:?}");
    assert_eq!(result.max_error, before.max_error, "{result:?}");
    assert!(result.compared > before.compared + 400, "{result:?}");
}

/// Everyone else sees the gun where it really is, set down on the same point.
#[test]
fn a_gun_set_down_is_where_it_really_is_for_everyone_else() {
    let mut world = world_on("range");
    let me = world.join(client_on("range"), script(|_, tick| {
        let mut held = Buttons::default();
        held.set(Buttons::REST, tick == 120);
        held.set(Buttons::ADS, tick > 180);
        PlayerCmd { tick, buttons: held, ..Default::default() }
    }), &net());
    let them = world.join(client_on("range"), script(|_, tick| PlayerCmd { tick, ..Default::default() }), &net());
    world.run(1.0);
    let id = world.player_id(me) as SoldierId;
    let at = world.server.game.sim.soldiers[id].body.pos;
    sims(&mut world, me, |sim| {
        sim.world.add_box(at + Vec3::new(0.0, 0.65, -0.75), Vec3::new(3.0, 0.65, 0.3), Quat::IDENTITY, Surface::Concrete);
    });
    world.run(4.0);
    let sim = &world.server.game.sim;
    let real = &sim.soldiers[id];
    assert!(real.rest.on && real.rest.amount >= 1.0, "should be rested");
    assert!((real.body.eye_height - sim.move_cfg.stand_eye).abs() > 0.05, "the body should have come down to it: {}", real.body.eye_height);
    let seen = &world.players[them].session.game().unwrap().sim;
    let (a, b) = (seen.soldiers[id].gun_pose(&sim.weapons), real.gun_pose(&sim.weapons));
    assert!(a.muzzle.distance(b.muzzle) < 0.02, "{} against {}", a.muzzle, b.muzzle);
    assert!(seen.soldiers[id].eye().distance(real.eye()) < 0.01);
}

/// A look that goes through the same stop a client's mouse does, a tick at a time, read from the client's
/// own idea of the rest. The script picks it up from here.
struct Mouse {
    look: Rc<Cell<(f32, f32)>>,
}

impl Mouse {
    fn push(&self, world: &SimWorld, p: usize, id: SoldierId, by: Vec2, stop: bool) {
        let (yaw, pitch) = self.look.get();
        let sim = &world.players[p].session.game().unwrap().sim;
        let soldier = &sim.soldiers[id];
        let wanted = Vec2::new(yaw + by.x, pitch + by.y);
        let got = match sim.rest_arc(id, wanted.x).filter(|_| stop) {
            Some(arc) => soldier.held_config(&sim.weapons).rest.stop_look(&arc, Vec2::new(yaw, pitch), wanted, 1.0 / 60.0),
            None => wanted,
        };
        self.look.set((got.x, got.y));
    }
}

/// A player in front of a chest high wall, with C pressed once on `rest_tick`, and a mouse to push.
fn rested_with_a_mouse(rest_tick: u32) -> (SimWorld, usize, SoldierId, Mouse) {
    let look = Rc::new(Cell::new((0.0, 0.0)));
    let held = look.clone();
    let mut world = world_on("range");
    let me = world.join(
        client_on("range"),
        script(move |_, tick| {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::REST, tick == rest_tick);
            buttons.set(Buttons::ADS, tick > rest_tick + 30);
            let (yaw, pitch) = held.get();
            PlayerCmd { tick, yaw, pitch, buttons, ..Default::default() }
        }),
        &net(),
    );
    world.run(1.0);
    let id = world.player_id(me) as SoldierId;
    let at = world.server.game.sim.soldiers[id].body.pos;
    sims(&mut world, me, |sim| {
        sim.world.add_box(at + Vec3::new(0.0, 0.65, -0.75), Vec3::new(3.0, 0.65, 0.3), Quat::IDENTITY, Surface::Concrete);
    });
    (world, me, id, Mouse { look })
}

/// Pushing as hard as a mouse can at each end of the arc for seconds on end: still rested, nothing to
/// correct, and the first tick back goes back.
#[test]
fn pushing_past_the_end_of_the_arc_stays_rested_and_never_reconciles() {
    let (mut world, me, id, mouse) = rested_with_a_mouse(150);
    world.run(3.0);
    assert!(world.server.game.sim.soldiers[id].rest.on, "should be rested");
    let before = stats(&world, me);
    let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
    for by in [Vec2::new(0.1, 0.0), Vec2::new(-0.1, 0.0), Vec2::new(0.0, 0.1), Vec2::new(0.0, -0.1)] {
        let from = mouse.look.get();
        for _ in 0..(3.0 * 60.0) as usize {
            mouse.push(&world, me, id, by, true);
            world.run(1.0 / 60.0);
            assert!(world.server.game.sim.soldiers[id].rest.on, "came off pushing {by}");
            let height = world.server.game.sim.soldiers[id].body.eye_height;
            lowest = lowest.min(height);
            highest = highest.max(height);
        }
        let at_end = mouse.look.get();
        assert_ne!(at_end, from, "should have got somewhere");
        // A small move the other way is all there on the very next tick.
        let back = -by.normalize() * 0.01;
        mouse.push(&world, me, id, back, true);
        let moved = Vec2::new(mouse.look.get().0 - at_end.0, mouse.look.get().1 - at_end.1);
        assert!(moved.distance(back) < 1e-4, "moved {moved} for {back}");
        for _ in 0..60 {
            mouse.push(&world, me, id, -by * 0.03, true);
            world.run(1.0 / 60.0);
        }
    }
    assert!(highest - lowest > 0.25, "the body should have gone up and down with it: {lowest} to {highest}");
    let result = stats(&world, me);
    assert_eq!(result.reconciles, before.reconciles, "{result:?}");
    assert_eq!(result.max_error, before.max_error, "{result:?}");
}

/// With the player's setting off it is the sim's own rule: on past the arc and it comes off, and that
/// is predicted as well as anything.
#[test]
fn with_the_stop_turned_off_it_comes_off_and_never_reconciles() {
    let (mut world, me, id, mouse) = rested_with_a_mouse(150);
    world.run(3.0);
    assert!(world.server.game.sim.soldiers[id].rest.on);
    let before = stats(&world, me);
    for _ in 0..120 {
        mouse.push(&world, me, id, Vec2::new(0.02, 0.0), false);
        world.run(1.0 / 60.0);
    }
    world.run(0.5);
    assert!(!world.server.game.sim.soldiers[id].rest.on, "should have come off");
    let result = stats(&world, me);
    assert_eq!(result.reconciles, before.reconciles, "{result:?}");
}

/// A truck under the gun driving off while the mouse is hard against the arc. It is news from the server,
/// so it may well take a correction, but the rest just comes off, and nobody ends up anywhere odd.
#[test]
fn a_truck_driving_off_from_under_a_rest_held_at_the_arc_just_lifts_it() {
    // Somewhere beside the truck a crouched soldier can rest on it, found by trying.
    let probe = |sim: &mut Sim, id: SoldierId, feet: Vec3, yaw: f32| {
        sim.set_home(id, feet, yaw);
        for _ in 0..12 {
            sim.step(&[(id, PlayerCmd { tick: sim.tick + 1, yaw, ..Default::default() })]);
        }
        let mut buttons = Buttons::default();
        buttons.set(Buttons::REST, true);
        sim.step(&[(id, PlayerCmd { tick: sim.tick + 1, yaw, buttons, ..Default::default() })]);
        let on = sim.soldiers[id].rest.on;
        sim.soldiers[id].rest = Default::default();
        sim.step(&[(id, PlayerCmd { tick: sim.tick + 1, yaw, ..Default::default() })]);
        on
    };
    let mut sim = Sim::new(prs_sim::Level::named("range").unwrap());
    let id = sim.spawn_soldier(1);
    let mut crouch = Buttons::default();
    crouch.set(Buttons::CROUCH, true);
    sim.step(&[(id, PlayerCmd { tick: sim.tick + 1, buttons: crouch, ..Default::default() })]);
    for _ in 0..40 {
        sim.step(&[]);
    }
    let center = sim.vehicles[0].pos;
    let spot = (0..16)
        .flat_map(|turn| [1.6, 1.8, 2.0, 2.4, 2.8, 3.2, 3.6].map(move |away| (turn as f32 * std::f32::consts::TAU / 16.0, away)))
        .find_map(|(angle, away): (f32, f32)| {
            let at = center + Vec3::new(angle.sin(), 0.0, angle.cos()) * away;
            let top = Vec3::new(at.x, center.y + 8.0, at.z);
            let hit = sim.world.raycast(top, Vec3::NEG_Y, 30.0)?;
            let feet = top - Vec3::Y * (hit.distance - 0.03);
            let clear = sim.world.capsule_is_clear(feet, sim.move_cfg.capsule(Stance::Stand));
            (clear && probe(&mut sim, id, feet, angle)).then_some((feet, angle))
        });
    let (feet, yaw) = spot.expect("somewhere round the truck to rest on");

    let look = Rc::new(Cell::new((yaw, 0.0)));
    let held = look.clone();
    let start = 120;
    let mut world = world_on("range");
    let me = world.join(
        client_on("range"),
        script(move |_, tick| {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::CROUCH, tick == start);
            buttons.set(Buttons::REST, tick == start + 60);
            let (yaw, pitch) = held.get();
            PlayerCmd { tick, yaw, pitch, buttons, ..Default::default() }
        }),
        &net(),
    );
    let mouse = Mouse { look };
    world.run(1.0);
    let id = world.player_id(me) as SoldierId;
    world.server.game.sim.soldiers[id].body.pos = feet;
    world.server.game.sim.soldiers[id].home = feet;
    world.run(3.0);
    assert!(world.server.game.sim.soldiers[id].rest.on, "should be rested on the truck at {feet}");
    let from = world.players[me].frames.len();
    for _ in 0..60 {
        mouse.push(&world, me, id, Vec2::new(0.1, 0.0), true);
        world.run(1.0 / 60.0);
    }
    assert!(world.server.game.sim.soldiers[id].rest.on, "held against the arc");
    for _ in 0..(3.0 * 60.0) as usize {
        mouse.push(&world, me, id, Vec2::new(0.1, 0.0), true);
        // Straight away from them, or it is them being run over that is being tested.
        world.server.game.sim.vehicles[0].pos += (center - feet).with_y(0.0).normalize() * (3.0 / 60.0);
        world.server.game.sim.vehicles[0].asleep = false;
        world.run(1.0 / 60.0);
    }
    let body_pos = world.server.game.sim.soldiers[id].body.pos;
    assert!(!world.server.game.sim.soldiers[id].rest.on, "the truck has gone");
    world.run(1.0);
    let client = &world.players[me].session.game().unwrap().sim;
    assert!(!client.soldiers[id].rest.on, "and on the client");
    assert_eq!(client.soldiers[id].rest.eye, Vec3::ZERO);
    assert!(client.soldiers[id].body.pos.distance(world.server.game.sim.soldiers[id].body.pos) < 0.05);
    assert!(body_pos.distance(feet) < 1.0, "should not have gone anywhere: {body_pos}");
    assert!(worst_step(&world, me, from) < 0.3, "{}", worst_step(&world, me, from));
}
