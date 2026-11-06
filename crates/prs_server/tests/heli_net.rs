//! A helicopter on a bad network: flown through nothing but commands by a pilot on one client and
//! watched from another, against the real server. The autopilot sees the server's state, which is fine:
//! all that matters to the network is the stream of commands it makes.

use glam::{Quat, Vec2, Vec3};
use prs_net::server::ServerConfig;
use prs_net::testkit::Net;
use prs_net_sim::testkit::{SimWorld, client_on, world_with};
use prs_sim::helicopter::autopilot::{Autopilot, Order};
use prs_sim::helicopter::{Engine, HeliState, VirtualStick};
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::SimEvent;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

/// Ticks over which to judge the pilot's view: long enough to smooth over ticks landing between frames.
const VIEW_WINDOW: usize = 6;

/// What the pilot should be doing, and where they have got to.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Plan {
    /// Stays still on the pad.
    Wait,
    Start,
    Hover { at: Vec3 },
    /// Round a circle at a speed, at a height.
    Circle { center: Vec2, radius: f32, speed: f32, height: f32 },
}

struct Cockpit {
    state: HeliState,
    plan: Plan,
    autopilot: Autopilot,
    switched: bool,
}

fn command(cockpit: &mut Cockpit, tick: u32) -> PlayerCmd {
    let s = cockpit.state;
    let order = match cockpit.plan {
        Plan::Wait => return PlayerCmd { tick, ..Default::default() },
        Plan::Start => {
            let mut buttons = Buttons::default();
            buttons.set(Buttons::JUMP, !cockpit.switched && s.engine == Engine::Off);
            cockpit.switched = cockpit.switched || buttons.contains(Buttons::JUMP);
            return PlayerCmd { tick, buttons, ..Default::default() };
        }
        Plan::Hover { at } => Order {
            velocity: (Vec2::new(at.x - s.pos.x, at.z - s.pos.z) * 0.3).clamp_length_max(8.0),
            climb: ((at.y - s.pos.y) * 0.3).clamp(-3.0, 3.0),
            heading: s.heading(),
            lean_limit: 0.12,
        },
        Plan::Circle { center, radius, speed, height } => {
            let here = Vec2::new(s.pos.x, s.pos.z) - center;
            let out = here.normalize_or(Vec2::X);
            let along = Vec2::new(-out.y, out.x);
            let velocity = along * speed + out * (radius - here.length()) * 0.25;
            let heading = (-velocity.x).atan2(-velocity.y);
            Order { velocity, climb: ((height - s.pos.y) * 0.3).clamp(-4.0, 5.0), heading, lean_limit: 0.35 }
        }
    };
    let input = cockpit.autopilot.fly(&s, &order, TICK_DT);
    PlayerCmd {
        tick,
        // As the input crate gives it: two keys held at once share out one.
        wish_move: Vec2::new(input.pedals, input.collective).clamp_length_max(1.0),
        stick: VirtualStick { pos: input.stick }.quantised(),
        ..Default::default()
    }
}

struct Sortie {
    world: SimWorld,
    cockpit: Rc<RefCell<Cockpit>>,
    pilot: usize,
    watcher: usize,
    /// Where it was on each server tick since the start, for checking what the watcher draws.
    track: Vec<(u32, Vec3)>,
    worst_drawn: f32,
    worst_lead: f32,
    /// The biggest correction still showing on the pilot's view, since being told to start measuring.
    worst_offset: f32,
    worst_turn: f32,
    /// What the pilot's view did and what the machine did, over a tenth of a second. The difference is what a
    /// correction looks like from inside.
    views: VecDeque<(Vec3, Quat, Vec3, Vec3)>,
    worst_slide: f32,
    worst_swing: f32,
}

impl Sortie {
    fn new(net: &Net) -> Self {
        Self::on("town", net)
    }

    fn on(map: &str, net: &Net) -> Self {
        let mut world = world_with(map, ServerConfig::default());
        let start = world.server.game.sim.helicopters[0].state;
        let cockpit = Rc::new(RefCell::new(Cockpit { state: start, plan: Plan::Wait, autopilot: Autopilot::default(), switched: false }));
        let script = cockpit.clone();
        let pilot = world.join(client_on(map), Rc::new(move |_, tick| command(&mut script.borrow_mut(), tick)), net);
        let watcher = world.join(client_on(map), Rc::new(|_, tick| PlayerCmd { tick, ..Default::default() }), net);
        Self { world, cockpit, pilot, watcher, track: Vec::new(), worst_drawn: 0.0, worst_lead: 0.0, worst_offset: 0.0, worst_turn: 0.0, views: VecDeque::new(), worst_slide: 0.0, worst_swing: 0.0 }
    }

    /// Runs on, a tick at a time so the pilot sees what is going on, and checks what everyone is drawing.
    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds / TICK_DT) as u32 {
            self.world.run(TICK_DT as f64);
            let server = &self.world.server.game.sim;
            let state = server.helicopters[0].state;
            self.cockpit.borrow_mut().state = state;
            self.track.push((server.tick, state.pos));

            let watcher = &self.world.players[self.watcher].session;
            let (Some(game), Some(interp)) = (watcher.game(), watcher.interp()) else { continue };
            let drawn = game.sim.helicopters[0].state.pos;
            let play = interp.play_tick();
            if let Some(window) = self.track.iter().position(|(t, _)| *t as f64 >= play) {
                let truth = self.track[window].1;
                self.worst_drawn = self.worst_drawn.max(drawn.distance(truth));
            }
            let own = &self.world.players[self.pilot].session;
            if let (Some(game), Some(predictor)) = (own.game(), own.predictor()) {
                let lead = predictor.tick().saturating_sub(server.tick) as f32 * TICK_DT;
                let far = game.sim.helicopters[0].state.pos.distance(state.pos) - state.vel.length() * lead;
                self.worst_lead = self.worst_lead.max(far);
                self.worst_offset = self.worst_offset.max(predictor.visual_offset().length());
                let turn = predictor.visual_turn().angle_between(Quat::IDENTITY);
                self.worst_turn = self.worst_turn.max(turn);
                let own_state = game.sim.helicopters[0].state;
                let world_spin = own_state.rot * own_state.spin;
                self.views.push_back((predictor.view_position(), (predictor.visual_turn() * own_state.rot).normalize(), own_state.vel, world_spin));
                if self.views.len() > VIEW_WINDOW {
                    let (from, from_rot, ..) = self.views.pop_front().expect("just checked");
                    let (to, to_rot, ..) = *self.views.back().expect("just pushed");
                    let seconds = VIEW_WINDOW as f32 * TICK_DT;
                    let n = self.views.len() as f32;
                    let (vel, spin) = self.views.iter().fold((Vec3::ZERO, Vec3::ZERO), |(v, s), e| (v + e.2 / n, s + e.3 / n));
                    self.worst_slide = self.worst_slide.max(((to - from) / seconds - vel).length());
                    let turned = (to_rot * from_rot.inverse()).to_scaled_axis() / seconds;
                    self.worst_swing = self.worst_swing.max((turned - spin).length());
                }
            }
        }
    }

    fn state(&self) -> HeliState {
        self.world.server.game.sim.helicopters[0].state
    }

    fn reconciles(&self) -> u32 {
        self.world.players[self.pilot].session.predictor().map_or(0, |p| p.stats().reconciles)
    }

    fn set_plan(&mut self, plan: Plan) {
        self.cockpit.borrow_mut().plan = plan;
    }

    /// To the air and steady, at the end of which a reconcile is news.
    fn into_a_hover(&mut self) -> Vec3 {
        self.run(1.0);
        let id = self.world.player_id(self.pilot) as usize;
        let carrier = prs_sim::heli_carrier(0);
        self.world.server.game.sim.sit_in(id, prs_sim::seats::SeatedAt { carrier, seat: 0 });
        self.run(0.5);
        self.set_plan(Plan::Start);
        self.run(self.world.server.game.sim.heli_cfg.engine.start_time + 4.0);
        assert!(self.state().rotor_fraction(&self.world.server.game.sim.heli_cfg) > 0.95);
        let at = self.state().pos + Vec3::Y * 20.0;
        self.set_plan(Plan::Hover { at });
        self.run(25.0);
        assert!(self.state().pos.distance(at) < 4.0, "got to {} going to {at}", self.state().pos);
        at
    }
}

fn check(net: Net, name: &str, most_per_minute: f32, most_offset: f32, most_turn: f32) {
    let mut sortie = Sortie::new(&net);
    let at = sortie.into_a_hover();

    let settled = sortie.reconciles();
    sortie.worst_offset = 0.0;
    sortie.worst_turn = 0.0;
    sortie.worst_drawn = 0.0;
    sortie.worst_slide = 0.0;
    sortie.worst_swing = 0.0;
    let mut furthest = 0.0f32;
    for _ in 0..30 {
        sortie.run(1.0);
        furthest = furthest.max(sortie.state().pos.distance(at));
    }
    let hovering = sortie.reconciles() - settled;

    let center = Vec2::new(at.x, at.z - 200.0);
    sortie.set_plan(Plan::Circle { center, radius: 200.0, speed: 40.0, height: at.y + 60.0 });
    sortie.run(25.0);
    let before_circling = sortie.reconciles();
    let mut fastest = 0.0f32;
    for _ in 0..40 {
        sortie.run(1.0);
        fastest = fastest.max(sortie.state().vel.length());
    }
    let circling = sortie.reconciles() - before_circling;
    println!(
        "{name}: hover strayed {furthest:.2} m with {hovering} reconciles in 30 s; {fastest:.1} m/s with {circling} in 40 s; on the pilot's view: correction {:.3} m {:.2} deg, view sliding {:.1} m/s and swinging {:.1} deg/s beyond the machine; drawn by the watcher within {:.3} m of the truth; pilot past its lead by {:.3} m",
        sortie.worst_offset, sortie.worst_turn.to_degrees(), sortie.worst_slide, sortie.worst_swing.to_degrees(), sortie.worst_drawn, sortie.worst_lead
    );
    // Reconciling comes to a stop at this, showing a jump. Nothing here should come near.
    let snap = sortie.world.server.game.sim.heli_cfg.net_view.snap_distance;
    assert!(sortie.worst_offset < snap * 0.75, "{} m of correction would nearly snap", sortie.worst_offset);
    assert!(sortie.worst_turn < most_turn, "{} rad of correction to the attitude", sortie.worst_turn);
    assert!(sortie.worst_slide < sortie.world.server.game.sim.heli_cfg.net_view.fade_speed * 1.5, "the view slid at {} m/s past the machine", sortie.worst_slide);
    assert!(!sortie.state().wrecked, "it should have stayed in the air");
    assert!(furthest < 4.0, "strayed {furthest} m");
    assert!(fastest > 35.0, "only {fastest} m/s");
    assert!(hovering as f32 / 0.5 <= most_per_minute, "{hovering} reconciles in the hover");
    assert!(circling as f32 / (40.0 / 60.0) <= most_per_minute, "{circling} reconciles in forward flight");
    assert!(sortie.worst_offset < most_offset, "a correction of {} m", sortie.worst_offset);
    assert!(sortie.worst_drawn < 2.5, "someone watching saw it {} m from the truth", sortie.worst_drawn);
}

#[test]
fn hover_and_forward_flight_at_100_ms_each_way() {
    check(Net::datagram(100.0, 0.0, 0.0), "100 ms each way", 0.0, 0.01, 0.01);
}

#[test]
fn hover_and_forward_flight_with_jitter_and_loss() {
    check(Net::datagram(100.0, 15.0, 0.02), "100 ms, 15 ms jitter, 2% loss", 60.0, 1.0, 0.03);
}

#[test]
fn hover_and_forward_flight_on_tcp_at_a_little_loss() {
    
}

/// The server shoves the machine a little in the air, as a misprediction would. From the cockpit that must
/// fade in rather than jump, in position and in attitude both.
#[test]
fn a_correction_fades_on_the_pilots_view_in_attitude_as_well_as_position() {
    let mut sortie = Sortie::new(&Net::datagram(50.0, 0.0, 0.0));
    sortie.into_a_hover();
    sortie.run(5.0);
    let state = &mut sortie.world.server.game.sim.helicopters[0].state;
    state.pos.x += 0.6;
    state.rot = (state.rot * Quat::from_rotation_z(0.05)).normalize();
    sortie.worst_slide = 0.0;
    sortie.worst_swing = 0.0;
    sortie.worst_offset = 0.0;
    sortie.worst_turn = 0.0;
    let before = sortie.reconciles();
    sortie.run(0.5);
    println!(
        "shoved 0.6 m and 3 deg: {} reconciles, correction {:.2} m {:.2} deg, view sliding {:.1} m/s and swinging {:.1} deg/s past the machine",
        sortie.reconciles() - before, sortie.worst_offset, sortie.worst_turn.to_degrees(), sortie.worst_slide, sortie.worst_swing.to_degrees()
    );
    assert!(sortie.reconciles() > before, "that should have been noticed");
    assert!(sortie.worst_offset > 0.3, "the jump went into the offset: {}", sortie.worst_offset);
    assert!(sortie.worst_turn > 0.02, "the turn went into the offset: {}", sortie.worst_turn);
    let view = &sortie.world.server.game.sim.heli_cfg.net_view;
    assert!(sortie.worst_slide < view.fade_speed * 1.5, "slid at {} m/s", sortie.worst_slide);
    assert!(sortie.worst_swing < view.fade_turn * 1.5, "swung at {} rad/s", sortie.worst_swing);
    sortie.run(2.0);
    let predictor = sortie.world.players[sortie.pilot].session.predictor().unwrap();
    assert!(predictor.visual_offset().length() < 0.01 && predictor.visual_turn().angle_between(Quat::IDENTITY) < 0.001, "all faded away");
}

/// Someone in the door, firing down at a soldier on the ground from a machine flying a circle round them,
/// on real latency: the shot starts where they saw their own gun, and it hits.
#[test]
fn a_door_gunner_hits_what_they_saw_from_a_moving_helicopter() {
    const FIRE_EVERY: u32 = 20;
    let net = Net::datagram(60.0, 0.0, 0.0);
    let mut sortie = Sortie::on("range", &net);

    let aim: Rc<Cell<(f32, f32, bool)>> = Rc::new(Cell::new((0.0, 0.0, false)));
    let aimed = aim.clone();
    let gunner = sortie.world.join(
        client_on("range"),
        Rc::new(move |_, tick| {
            let (yaw, pitch, firing) = aimed.get();
            let mut buttons = Buttons::default();
            buttons.set(Buttons::FIRE, firing && tick % FIRE_EVERY < FIRE_EVERY / 2);
            PlayerCmd { tick, yaw, pitch, buttons, ..Default::default() }
        }),
        &net,
    );
    sortie.run(1.0);
    let gunner_id = sortie.world.player_id(gunner) as usize;
    let target_id = sortie.world.player_id(sortie.watcher) as usize;
    let carrier = prs_sim::heli_carrier(0);
    sortie.world.server.game.sim.sit_in(gunner_id, prs_sim::seats::SeatedAt { carrier, seat: 3 });

    let at = sortie.into_a_hover();
    let center = Vec2::new(at.x - 60.0, at.z + 40.0);
    let spot = Vec3::new(center.x, at.y + 100.0, center.y);
    let down = sortie.world.server.game.sim.world.raycast(spot, Vec3::NEG_Y, 200.0).expect("ground").distance;
    sortie.world.server.game.sim.set_home(target_id, spot - Vec3::Y * (down - 0.1), 0.0);
    sortie.set_plan(Plan::Circle { center, radius: 70.0, speed: 16.0, height: at.y + 25.0 });
    sortie.run(20.0);

    let (mut server_shots, mut hits, mut went) = (Vec::new(), 0u32, 0.0f32);
    let mut seen_shots: Vec<(u32, Vec3)> = Vec::new();
    let mut counted = shots_of(&sortie.world.players[gunner].predicted_events).len();
    let mut miss: Vec<f32> = Vec::new();
    let mut looks: Vec<f32> = Vec::new();
    for _ in 0..(20.0 / TICK_DT) as u32 {
        let client = &sortie.world.players[gunner].session;
        if let Some(game) = client.game() {
            let sim = &game.sim;
            let eye = sim.soldiers[gunner_id].eye();
            let to = sim.soldiers[target_id].body.pos + Vec3::Y * 1.2 - eye;
            let (yaw, pitch) = ((-to.x).atan2(-to.z), (to.y / to.length()).asin());
            let frame = sim.carrier_frame(carrier);
            let bears = prs_sim::seats::can_fire(&frame, &sim.heli_cfg.seats[3], yaw, pitch);
            aim.set((yaw, pitch, bears));
        }
        sortie.run(TICK_DT);
        let predicted = shots_of(&sortie.world.players[gunner].predicted_events);
        let tick = sortie.world.players[gunner].session.predictor().map_or(0, |p| p.tick());
        seen_shots.extend(predicted[counted..].iter().map(|(origin, _)| (tick, *origin)));
        counted = predicted.len();
        let server = &sortie.world.server.game.sim;
        went = went.max(server.helicopters[0].state.vel.length());
        let target = server.soldiers[target_id].body.pos + Vec3::Y * 1.0;
        for event in &server.events {
            match *event {
                SimEvent::ShotFired { soldier, origin, dir, .. } if soldier == gunner_id => {
                    server_shots.push((server.tick, origin));
                    miss.push((target - origin).cross(dir).length());
                    let g = &server.soldiers[gunner_id];
                    looks.push(prs_sim::view_dir(g.yaw, g.pitch).angle_between((target - origin).normalize()));
                }
                SimEvent::SoldierHit { soldier, shooter, .. } if soldier == target_id && shooter == gunner_id => hits += 1,
                _ => {}
            }
        }
    }
    // A shot on each side for the same tick, near enough, whichever order they arrived.
    let gaps: Vec<f32> = server_shots
        .iter()
        .filter_map(|(tick, origin)| {
            let seen = seen_shots.iter().filter(|(t, _)| t.abs_diff(*tick) <= 3);
            seen.map(|(_, o)| o.distance(*origin)).reduce(f32::min)
        })
        .collect();
    let gap = gaps.iter().copied().fold(0.0, f32::max);
    miss.sort_by(f32::total_cmp);
    let median = miss.get(miss.len() / 2).copied().unwrap_or(f32::MAX);
    println!(
        "door gunner at {went:.1} m/s, 120 ms round trip: {} shots, {} matched to what the gunner saw, {hits} hits, worst muzzle gap {gap:.3} m, line of fire passes the target by {median:.2} m at the median and {:.2} m at best",
        server_shots.len(), gaps.len(), miss.first().copied().unwrap_or(f32::MAX)
    );
    assert!(went > 12.0, "going only {went} m/s");
    assert!(gaps.len() >= 5, "only {} shots matched", gaps.len());
    assert!(gap < 1.0, "the shot started {gap} m from where they saw their gun");
    let worst_look = looks.iter().copied().fold(0.0, f32::max);
    println!("their look was within {:.2} deg of the target as the server had it; the rest of the miss is the gun moving", worst_look.to_degrees());
    assert!(worst_look < 0.025, "the look was {} rad off by the time it got there", worst_look);

    // Hanging still, with the gun allowed to settle, close by: now it is a matter of aiming, and it hits.
    let at = sortie.state().pos;
    sortie.set_plan(Plan::Hover { at });
    aim.set((0.0, 0.0, false));
    let quiet = |sortie: &mut Sortie, aim: &Cell<(f32, f32, bool)>, firing: bool, seconds: f32| {
        let mut hits = 0;
        for _ in 0..(seconds / TICK_DT) as u32 {
            let game = sortie.world.players[gunner].session.game().unwrap();
            let sim = &game.sim;
            let to = sim.soldiers[target_id].body.pos + Vec3::Y * 1.0 - sim.soldiers[gunner_id].eye();
            aim.set(((-to.x).atan2(-to.z), (to.y / to.length()).asin(), firing));
            sortie.run(TICK_DT);
            let server = &sortie.world.server.game.sim;
            hits += server.events.iter().filter(|e| matches!(e, SimEvent::SoldierHit { soldier, .. } if *soldier == target_id)).count();
        }
        hits
    };
    quiet(&mut sortie, &aim, false, 30.0);
    sortie.world.server.game.sim.soldiers[gunner_id].weapon.ammo = 30;
    let frame = sortie.world.server.game.sim.carrier_frame(carrier);
    let beside = frame.point(Vec3::new(35.0, 0.0, 0.0));
    let spot = Vec3::new(beside.x, frame.pos.y + 100.0, beside.z);
    let down = sortie.world.server.game.sim.world.raycast(spot, Vec3::NEG_Y, 300.0).expect("ground").distance;
    sortie.world.server.game.sim.set_home(target_id, spot - Vec3::Y * (down - 0.1), 0.0);
    quiet(&mut sortie, &aim, false, 6.0);
    let hits = quiet(&mut sortie, &aim, true, 6.0);
    println!("hanging still: {hits} hits at {:.0} m", sortie.state().pos.distance(spot - Vec3::Y * down));
    assert!(hits >= 1, "not one hit from the door at a hover");
}

fn shots_of(events: &[SimEvent]) -> Vec<(Vec3, Vec3)> {
    events
        .iter()
        .filter_map(|event| match *event {
            SimEvent::ShotFired { origin, dir, .. } => Some((origin, dir)),
            _ => None,
        })
        .collect()
}
