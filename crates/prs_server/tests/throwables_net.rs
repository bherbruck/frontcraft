//! Something thrown over a network: the thrower sees it go at once, everyone else flies their own copy of the
//! arc from what they are told, and only the server says when it goes off and what it did.

use prs_net::testkit::Net;
use prs_net_sim::WireEvent;
use prs_net_sim::testkit::{client_on, script, world_on};
use prs_sim::SimEvent;
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::throwables::ThrownKind;

/// Takes out a smoke grenade or a frag, then winds up and lets go.
fn thrower(presses: u32) -> impl Fn(u32, u32) -> PlayerCmd {
    move |_who, tick| {
        let mut buttons = Buttons::default();
        for n in 0..presses {
            buttons.set(Buttons::GRENADE, buttons.contains(Buttons::GRENADE) || tick == 60 + n * 10);
        }
        buttons.set(Buttons::FIRE, (180..240).contains(&tick));
        PlayerCmd { tick, yaw: 0.0, pitch: 0.25, buttons, ..Default::default() }
    }
}

fn stands_by(_who: u32, tick: u32) -> PlayerCmd {
    PlayerCmd { tick, ..Default::default() }
}

#[test]
fn everyone_sees_a_throw_and_only_the_server_says_what_it_did() {
    let mut world = world_on("range");
    let net = Net::datagram(50.0, 5.0, 0.0);
    world.join(client_on("range"), script(thrower(2)), &net);
    world.join(client_on("range"), script(stands_by), &net);
    world.run(3.0);
    let on_screen = |world: &prs_net_sim::testkit::SimWorld, p: usize| world.players[p].session.game().map_or(0, |g| g.sim.thrown.len());
    let (mut thrower_first, mut watcher_saw) = (false, false);
    for _ in 0..80 {
        world.run(0.05);
        let (mine, theirs, server) = (on_screen(&world, 0), on_screen(&world, 1), world.server.game.sim.thrown.len());
        // Their own shows up ahead of the server's word.
        thrower_first |= mine > 0 && server == 0;
        watcher_saw |= theirs > 0;
    }
    assert!(thrower_first, "the thrower should have seen it go before the server had it");
    assert!(watcher_saw, "the other player should have seen it in the air");
    world.run(12.0);

    let server = &world.server.game.sim;
    assert_eq!(server.clouds.len(), 1, "the smoke should be up by now");
    let cloud = server.clouds[0];
    for p in 0..2 {
        let player = &world.players[p];
        let burst = player.server_events.iter().any(|e| matches!(e, WireEvent::Sim(SimEvent::ThrownBurst { kind: ThrownKind::Smoke, .. })));
        assert!(burst, "player {p} never heard it go off");
        let game = player.session.game().unwrap();
        assert!(game.sim.thrown.is_empty(), "player {p} is still flying it");
        assert_eq!(game.sim.clouds.len(), 1, "player {p} has no smoke");
        // Each flew its own copy of the arc, and it is the server's that says where.
        assert!(game.sim.clouds[0].pos.distance(cloud.pos) < 0.01, "player {p} has it at {} and the server at {}", game.sim.clouds[0].pos, cloud.pos);
    }
    let copy = world.players[1].session.game().unwrap().sim.clouds[0].age;
    assert!((copy - cloud.age).abs() < 1.0, "aged {copy} on a client and {} on the server", cloud.age);
}

#[test]
fn a_frag_hurts_only_by_the_servers_word() {
    let mut world = world_on("range");
    let net = Net::datagram(50.0, 5.0, 0.0);
    world.join(client_on("range"), script(thrower(1)), &net);
    world.join(client_on("range"), script(stands_by), &net);
    // Standing right where it will land, so that if a client's copy could hurt, here it would.
    world.run(12.0);
    let watcher = &world.players[1];
    let heard = watcher.server_events.iter().any(|e| matches!(e, WireEvent::Sim(SimEvent::ThrownBurst { kind: ThrownKind::Frag, .. })));
    assert!(heard, "the other player never heard it go off");
    assert!(world.server.game.sim.thrown.is_empty());
    for player in &world.players {
        let game = player.session.game().unwrap();
        assert!(game.sim.thrown.is_empty(), "a client is still flying it");
        assert!(game.sim.clouds.is_empty());
    }
}
