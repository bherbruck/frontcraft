//! Bots live on the server. A client runs none of it: it gets soldiers in its snapshots like any others,
//! hears their shots like any others, and has no idea anyone but a person is at the controls.

use prs_net::client::ClientSession;
use prs_net::server::{GameServer, ServerConfig};
use prs_net::testkit::{Net, Observer, World};
use prs_net_sim::{AnyPose, SimClient, SimServer, SimServerConfig, WireEvent};
use prs_sim::SimEvent;
use prs_sim::protocol::PlayerCmd;
use std::collections::HashMap;
use std::rc::Rc;

/// Further than anyone goes in one tick without being put somewhere.
const JUMP: f32 = 2.0;
/// How near a jump in where the server has someone a frame can be and still be looking at the wrong side of it.
/// A snapshot is two ticks, and a jump between two of them is drawn at one or the other.
const JUMP_TICKS: i64 = 2;
/// Anything further off than this is written down, to be looked at once it is known where everyone jumped.
const WORTH_NOTING: f32 = 0.1;

/// How far a client's drawing of each bot is from where the server had it at that moment.
#[derive(Default)]
struct Watch {
    truth: HashMap<u32, Vec<glam::Vec3>>,
    frames: u32,
    moved: f32,
    first: HashMap<u32, glam::Vec3>,
    off: Vec<(u32, usize, f32)>,
    /// The worst of the rest, which were never going to be anywhere near a jump's size.
    worst_unnoted: f32,
}

impl Watch {
    /// Whether the server had this soldier jump, by coming back at the base, within a moment of this tick.
    fn jumped_near(&self, tick: u32, id: usize) -> bool {
        (-JUMP_TICKS..=JUMP_TICKS).any(|o| {
            let t = (tick as i64 + o) as u32;
            let at = |t: u32| self.truth.get(&t).and_then(|list| list.get(id));
            at(t).zip(at(t + 1)).is_some_and(|(a, b)| a.distance(*b) > JUMP)
        })
    }

    /// The furthest a bot's gun was drawn from where the server had it, leaving out only those moments.
    fn worst_gun(&self) -> f32 {
        self.off.iter().filter(|(tick, id, _)| !self.jumped_near(*tick, *id)).map(|o| o.2).fold(self.worst_unnoted, f32::max)
    }
}

impl Observer<SimClient, SimServer> for Watch {
    fn after_tick(&mut self, server: &GameServer<SimServer>) {
        let sim = &server.game.sim;
        self.truth.insert(sim.tick, sim.soldiers.iter().map(|s| s.gun_pose(&sim.weapons).muzzle).collect());
    }

    fn after_frame(&mut self, _who: u32, session: &ClientSession<SimClient>) {
        let (Some(game), Some(interp)) = (session.game(), session.interp()) else { return };
        let tick = interp.play_tick();
        // Against the two snapshots either side, which is all a client can know. A stair is a jump of up
        // to `step_height` inside one tick on the server, and between snapshots that can only be drawn as
        // a slope across them, half a step off at worst for that moment. That says nothing about whether
        // what was sent arrived right, and measured tick by tick it would swamp anything that did not.
        let every = server_snapshot_every();
        let before = (tick / every as f64).floor() as u32 * every;
        let (Some(a), Some(b)) = (self.truth.get(&before), self.truth.get(&(before + every))) else { return };
        let t = ((tick - before as f64) / every as f64) as f32;
        self.frames += 1;
        // The client's clock takes a moment to settle.
        if tick < 120.0 {
            return;
        }
        for (key, pose) in session.remotes() {
            let AnyPose::Soldier(pose) = pose else { continue };
            let id = *key as usize;
            let seen = &game.sim.soldiers[id];
            let (Some(a), Some(b)) = (a.get(id), b.get(id)) else { continue };
            let muzzle = seen.gun_pose(&game.sim.weapons).muzzle;
            let off = muzzle.distance(a.lerp(*b, t));
            if off > WORTH_NOTING {
                self.off.push((tick.floor() as u32, id, off));
            } else {
                self.worst_unnoted = self.worst_unnoted.max(off);
            }
            let start = *self.first.entry(*key).or_insert(pose.feet);
            self.moved = self.moved.max(start.distance(pose.feet));
        }
    }
}

fn server_snapshot_every() -> u32 {
    ServerConfig::default().snapshot_every
}

#[test]
fn a_client_sees_server_bots_as_soldiers_and_runs_none_of_them() {
    let game = SimServer::new("town", "tdm", SimServerConfig::default()).expect("a map and a mode");
    let bots = game.game.state.players.iter().filter(|p| p.bot).count();
    assert!(bots >= 8, "only {bots} bots");
    // Someone far off is sent less often and drawn further back on purpose. This is about being right, so all at full rate.
    let mut cfg = ServerConfig::default();
    cfg.interest.full_rate_within = 100_000.0;
    cfg.interest.snapshot_budget = 1_000_000;
    cfg.datagram_bytes = 1_000_000;
    let mut world: World<SimClient, SimServer> = World::new(game, cfg, |server, id| server.sim.soldiers[id as usize].body.pos);
    let idle = Rc::new(|_who: u32, tick: u32| PlayerCmd { tick, ..Default::default() });
    let factory = Box::new(|map: &str, mode: &str| SimClient::setup(map, mode));
    world.join(factory, idle, &Net::datagram(50.0, 5.0, 0.0));
    let mut watch = Watch::default();
    world.run_observed(360.0, &mut watch);

    let player = &world.players[0];
    let remotes = player.session.remotes().iter().filter(|(_, p)| matches!(p, AnyPose::Soldier(_))).count();
    // A player joining pushes one bot out.
    let now = world.server.game.game.state.players.iter().filter(|p| p.bot).count();
    assert_eq!(now, bots - 1);
    assert!(remotes >= now, "{remotes} soldiers drawn with {now} bots on the server");
    assert!(watch.moved > 30.0, "they hardly went anywhere: {}", watch.moved);
    let worst_gun = watch.worst_gun();
    assert!(worst_gun < 0.05, "a bot's gun drawn {worst_gun} m from where it was");

    let sim = &world.server.game.sim;
    let (mut shots, mut heard, mut bots_hit) = (0, 0, 0);
    for event in &player.server_events {
        if let WireEvent::Sim(SimEvent::ShotFired { soldier, .. }) = event {
            heard += 1;
            shots += usize::from(*soldier != player.session.game().unwrap().local);
        }
    }
    for event in &player.server_events {
        if let WireEvent::Sim(SimEvent::SoldierHit { soldier, .. }) = event {
            bots_hit += usize::from(*soldier != player.session.game().unwrap().local);
        }
    }
    println!("{bots_hit} bots hit, {remotes} drawn, moved {:.0} m, worst gun {:.3} m, {heard} shots heard, tick {}", watch.moved, worst_gun, sim.tick);
    assert!(shots > 0, "bots fought for four minutes and nobody heard it");
    assert!(bots_hit > 0, "{shots} shots heard and nobody was seen to be hit");
}
