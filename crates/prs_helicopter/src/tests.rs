use glam::{Quat, Vec2, Vec3};
use prs_collision::{CollisionWorld, Surface};

use super::*;
use crate::autopilot::{Autopilot, Order};

const DT: f32 = 1.0 / 60.0;

fn flat_world() -> (CollisionWorld, CollisionWorld) {
    let mut solid = CollisionWorld::default();
    solid.add_box(Vec3::new(0.0, -1.0, 0.0), Vec3::new(600.0, 1.0, 600.0), Quat::IDENTITY, Surface::Dirt);
    (solid, CollisionWorld::default())
}

struct Flight {
    cfg: HeliConfig,
    state: HeliState,
    solid: CollisionWorld,
    canopy: CollisionWorld,
    half_size: f32,
    events: Vec<HeliEvent>,
}

impl Flight {
    fn on_flat() -> Self {
        let cfg = HeliConfig::default();
        let (solid, canopy) = flat_world();
        let state = HeliState::parked_at(&cfg, Vec3::ZERO, 0.0);
        Self { cfg, state, solid, canopy, half_size: 500.0, events: Vec::new() }
    }

    fn tick(&mut self, input: &HeliInput) {
        let world = HeliWorld { solid: &self.solid, canopy: &self.canopy, half_size: self.half_size };
        step(&mut self.state, &self.cfg, input, &world, DT, &mut self.events);
    }

    fn run(&mut self, seconds: f32, input: HeliInput) {
        for _ in 0..(seconds / DT) as u32 {
            self.tick(&input);
        }
    }

    fn start_engine(&mut self) {
        self.tick(&HeliInput { engine_switch: true, ..Default::default() });
        self.run(self.cfg.engine.start_time + 5.0, HeliInput::default());
    }

    /// In the air with the rotor at speed, as if flown there.
    fn hovering_at(&mut self, height: f32) {
        self.state.pos.y = height;
        self.state.engine = Engine::Running;
        self.state.spool = 1.0;
        self.state.power = 0.6;
        self.state.rotor_speed = self.cfg.rotor.nominal_speed;
        self.state.induced = aero::hover_induced(&self.cfg.rotor, self.cfg.mass * self.cfg.gravity, self.cfg.air.density);
        self.state.collective = self.hover_collective();
        self.state.parked = false;
        self.state.on_ground = false;
        self.state.skids = 0;
    }

    /// Collective that balances the weight in free air at nominal speed, from the rotor sums.
    fn hover_collective(&self) -> f32 {
        let weight = self.cfg.mass * self.cfg.gravity;
        let mut flow = HoverFlow::for_hover(&self.cfg);
        let mut low = 0.0;
        let mut high = 1.0;
        for _ in 0..30 {
            let mid = 0.5 * (low + high);
            flow.collective = mid;
            flow.induced = aero::hover_induced(&self.cfg.rotor, weight, self.cfg.air.density);
            for _ in 0..50 {
                let out = aero::rotor(&self.cfg.rotor, &flow.input());
                flow.induced += (out.induced_target - flow.induced) * 0.2;
            }
            if aero::rotor(&self.cfg.rotor, &flow.input()).thrust > weight {
                high = mid;
            } else {
                low = mid;
            }
        }
        0.5 * (low + high)
    }
}

struct HoverFlow {
    speed: f32,
    collective: f32,
    induced: f32,
    density: f32,
}

impl HoverFlow {
    fn for_hover(cfg: &HeliConfig) -> Self {
        Self { speed: cfg.rotor.nominal_speed, collective: 0.5, induced: 8.0, density: cfg.air.density }
    }

    fn input(&self) -> aero::RotorInput {
        aero::RotorInput {
            speed: self.speed,
            collective: self.collective,
            induced: self.induced,
            axial: 0.0,
            edgewise: 0.0,
            density: self.density,
            cushion: 0.0,
            vortex: 0.0,
            health: 1.0,
        }
    }
}

#[test]
fn hover_numbers_are_those_of_the_real_machine() {
    let f = Flight::on_flat();
    let collective = f.hover_collective();
    let pitch = (f.cfg.rotor.max_pitch - f.cfg.rotor.min_pitch) * collective + f.cfg.rotor.min_pitch;
    let mut input = HoverFlow::for_hover(&f.cfg);
    input.collective = collective;
    for _ in 0..200 {
        let out = aero::rotor(&f.cfg.rotor, &input.input());
        input.induced += (out.induced_target - input.induced) * 0.2;
    }
    let out = aero::rotor(&f.cfg.rotor, &input.input());
    println!("hover: collective {collective:.3} pitch {:.1} deg, induced {:.1} m/s, power {:.0} kW", pitch.to_degrees(), input.induced, out.power / 1000.0);
    assert!((5.0..10.0).contains(&pitch.to_degrees()), "pitch {} deg", pitch.to_degrees());
    assert!((350e3..750e3).contains(&out.power), "power {}", out.power);
    assert!(out.power < f.cfg.engine.max_power, "must be able to hover out of ground effect");
}

#[test]
fn nothing_moves_on_the_ground_at_rest() {
    let mut f = Flight::on_flat();
    f.run(3.0, HeliInput::default());
    let start = f.state;
    let (mut most_pos, mut most_rot) = (0.0f32, 0.0f32);
    for _ in 0..600 {
        f.tick(&HeliInput::default());
        most_pos = most_pos.max(f.state.pos.distance(start.pos));
        most_rot = most_rot.max(f.state.rot.angle_between(start.rot));
    }
    assert_eq!(most_pos, 0.0);
    assert_eq!(most_rot, 0.0);
    assert!(f.state.on_ground);
}

#[test]
fn nothing_moves_on_the_ground_with_the_rotor_turning_and_the_collective_down() {
    let mut f = Flight::on_flat();
    f.start_engine();
    assert_eq!(f.state.engine, Engine::Running);
    assert!(f.state.rotor_fraction(&f.cfg) > 0.95, "rotor {}", f.state.rotor_fraction(&f.cfg));
    let start = f.state;
    for _ in 0..600 {
        f.tick(&HeliInput::default());
        assert!(f.state.pos.distance(start.pos) < 1e-3, "moved {}", f.state.pos.distance(start.pos));
        assert!(f.state.rot.angle_between(start.rot) < 1e-3);
    }
}

#[test]
fn dropped_on_to_its_skids_it_settles_without_jitter() {
    let mut f = Flight::on_flat();
    f.state.pos.y += 0.3;
    f.state.parked = false;
    f.state.skids = 0;
    f.run(3.0, HeliInput::default());
    assert!(f.state.parked, "still moving at {} m/s", f.state.vel.length());
    assert!((f.state.pos.y - 1.65).abs() < 0.05, "height {}", f.state.pos.y);
    let start = f.state.pos;
    f.run(2.0, HeliInput::default());
    assert_eq!(f.state.pos, start);
}

#[test]
fn engine_takes_time_to_start_and_the_rotor_comes_up_with_it() {
    let mut f = Flight::on_flat();
    f.tick(&HeliInput { engine_switch: true, ..Default::default() });
    f.run(5.0, HeliInput::default());
    assert!(f.state.rotor_fraction(&f.cfg) < 0.2, "rotor {}", f.state.rotor_fraction(&f.cfg));
    f.run(25.0, HeliInput::default());
    assert_eq!(f.state.engine, Engine::Running);
    assert!(f.state.rotor_fraction(&f.cfg) > 0.97, "rotor {}", f.state.rotor_fraction(&f.cfg));
}

#[test]
fn full_collective_at_low_rotor_speed_does_not_lift() {
    let mut f = Flight::on_flat();
    f.tick(&HeliInput { engine_switch: true, ..Default::default() });
    let start = f.state.pos.y;
    // With the collective already hauled all the way up, it cannot get going.
    f.state.collective = 1.0;
    let mut rose = 0.0f32;
    for _ in 0..((f.cfg.engine.start_time - 3.0) / DT) as u32 {
        f.tick(&HeliInput { collective: 1.0, ..Default::default() });
        rose = rose.max(f.state.pos.y - start);
    }
    println!("full collective: rotor {} rose {rose}", f.state.rotor_fraction(&f.cfg));
    assert!(f.state.rotor_fraction(&f.cfg) < 0.8, "rotor {}", f.state.rotor_fraction(&f.cfg));
    assert!(rose < 0.06, "rose {rose}, more than taking the weight off the skids");
}

#[test]
fn hauling_up_the_collective_droops_the_rotor() {
    let mut f = Flight::on_flat();
    f.hovering_at(50.0);
    let before = f.state.rotor_speed;
    f.run(4.0, HeliInput { collective: 1.0, ..Default::default() });
    assert!(f.state.rotor_speed < before * 0.93, "rotor {}", f.state.rotor_fraction(&f.cfg));
}

/// Lets the collective down from the hover by this much and watches it sink for a while.
fn let_down(cfg: HeliConfig, by: f32, forward: f32, seconds: f32) -> Flight {
    let mut f = Flight::on_flat();
    f.cfg = cfg;
    f.hovering_at(600.0);
    f.state.vel.z = -forward;
    f.state.collective -= by;
    f.run(seconds, HeliInput::default());
    f
}

#[test]
fn descending_fast_with_no_forward_speed_loses_lift_in_its_own_wake() {
    let cfg = HeliConfig::default();
    let mut none = cfg.clone();
    none.rotor.vrs_loss = 0.0;
    let wake = let_down(cfg.clone(), 0.1, 0.0, 9.0);
    let no_wake = let_down(none, 0.1, 0.0, 9.0);
    println!("sink {} with vortex {}, {} without", wake.state.vel.y, wake.state.vortex, no_wake.state.vel.y);
    assert!(wake.state.vortex > 0.5, "vortex {}", wake.state.vortex);
    assert!(wake.state.vel.y < no_wake.state.vel.y * 1.5, "sank at {} and {}", wake.state.vel.y, no_wake.state.vel.y);
    assert!(wake.state.thrust < cfg.mass * cfg.gravity, "thrust {}", wake.state.thrust);
}

#[test]
fn pulling_power_in_vortex_ring_does_not_get_you_out_but_flying_out_does() {
    let mut f = let_down(HeliConfig::default(), 0.1, 0.0, 9.0);
    let sink = f.state.vel.y;
    assert!(f.state.vortex > 0.5, "vortex {}", f.state.vortex);
    // A full second of collective, more than anyone would dare.
    f.run(1.0, HeliInput { collective: 1.0, ..Default::default() });
    assert!(f.state.vel.y < sink * 0.5, "power got it out: {} from {sink}", f.state.vel.y);

    let mut f = let_down(HeliConfig::default(), 0.1, 0.0, 9.0);
    f.state.vel.z = -22.0;
    f.run(4.0, HeliInput::default());
    assert!(f.state.vortex < 0.1, "vortex {}", f.state.vortex);
    assert!(f.state.vel.y > sink * 0.6, "still sinking at {} from {sink}", f.state.vel.y);
}

#[test]
fn forward_speed_gives_translational_lift() {
    let mut flow = HoverFlow::for_hover(&HeliConfig::default()).input();
    let cfg = HeliConfig::default();
    let settle = |flow: &mut aero::RotorInput| {
        for _ in 0..300 {
            let out = aero::rotor(&cfg.rotor, flow);
            flow.induced += (out.induced_target - flow.induced) * 0.2;
        }
        aero::rotor(&cfg.rotor, flow)
    };
    let hover = settle(&mut flow);
    flow.edgewise = 15.0;
    let moving = settle(&mut flow);
    assert!(moving.thrust > hover.thrust * 1.1, "{} from {}", moving.thrust, hover.thrust);
    assert!(moving.power < hover.power * 1.1, "{} from {}", moving.power, hover.power);
}

#[test]
fn the_ground_cushions_and_a_roof_counts_as_ground() {
    let cfg = HeliConfig::default();
    let mut f = Flight::on_flat();
    f.solid.add_box(Vec3::new(0.0, 8.0, 0.0), Vec3::new(9.0, 1.0, 9.0), Quat::IDENTITY, Surface::Concrete);
    f.hovering_at(14.0);
    let world = HeliWorld { solid: &f.solid, canopy: &f.canopy, half_size: 500.0 };
    let over_roof = ground::GroundSample::take(&f.state, &cfg, &world).cushion;
    f.state.pos.x = 100.0;
    let world = HeliWorld { solid: &f.solid, canopy: &f.canopy, half_size: 500.0 };
    let over_field = ground::GroundSample::take(&f.state, &cfg, &world).cushion;
    assert!(over_roof > 0.05, "cushion {over_roof}");
    assert!(over_field < over_roof * 0.3, "cushion {over_field}");
}

#[test]
fn power_changes_yaw_it() {
    let mut f = Flight::on_flat();
    f.hovering_at(200.0);
    f.run(2.0, HeliInput::default());
    let before = f.state.heading();
    f.run(3.0, HeliInput { collective: 1.0, ..Default::default() });
    let turned = autopilot::wrap(f.state.heading() - before);
    assert!(turned < -0.08, "turned {turned} rad");
}

#[test]
fn without_a_tail_rotor_it_spins() {
    let mut f = Flight::on_flat();
    f.hovering_at(300.0);
    f.run(1.0, HeliInput::default());
    damage::apply(&mut f.state, &HeliConfig::default(), HeliZone::TailRotor, 1000.0);
    f.run(5.0, HeliInput::default());
    assert!(f.state.spin.y < -1.0, "yaw rate {} rad/s", f.state.spin.y);
    assert!(f.events.contains(&HeliEvent::TailRotorLost));
}

/// Collective down and nose on the horizon, holding a glide.
fn glide(f: &mut Flight, pilot: &mut Autopilot, forward: f32, collective: f32) {
    let flat = Vec2::new(f.state.forward().x, f.state.forward().z).normalize();
    let order = Order { velocity: flat * forward, climb: 0.0, heading: f.state.heading(), lean_limit: 0.2 };
    let mut input = pilot.fly(&f.state, &order, DT);
    input.collective = (collective - f.state.collective).clamp(-1.0, 1.0);
    f.tick(&input);
}

#[test]
fn a_dead_engine_can_be_autorotated_and_keeps_the_rotor_turning() {
    let mut f = Flight::on_flat();
    f.half_size = 5000.0;
    f.hovering_at(600.0);
    f.state.vel = Vec3::new(0.0, 0.0, -30.0);
    damage::apply(&mut f.state, &HeliConfig::default(), HeliZone::Engine, 300.0);
    let mut pilot = Autopilot::default();
    // Down with the collective at once and it keeps its speed and comes down steadily.
    for _ in 0..(25.0 / DT) as u32 {
        glide(&mut f, &mut pilot, 28.0, 0.12);
    }
    let rotor = f.state.rotor_fraction(&f.cfg);
    println!("autorotating: rotor {rotor} descent {} height {}", f.state.vel.y, f.state.pos.y);
    assert_eq!(f.state.engine, Engine::Dead);
    assert!((0.85..=1.2).contains(&rotor), "rotor {rotor}");
    assert!((-25.0..-5.0).contains(&f.state.vel.y), "descent {}", f.state.vel.y);
    assert!(f.state.pos.y > 100.0, "height {}", f.state.pos.y);
}

#[test]
fn a_dead_engine_with_the_collective_left_up_loses_the_rotor() {
    let mut f = Flight::on_flat();
    f.hovering_at(600.0);
    damage::apply(&mut f.state, &HeliConfig::default(), HeliZone::Engine, 300.0);
    f.run(8.0, HeliInput::default());
    assert!(f.state.rotor_fraction(&f.cfg) < 0.8, "rotor {}", f.state.rotor_fraction(&f.cfg));
}

#[test]
fn landing_hard_hurts_and_a_fall_is_the_end() {
    let drop = |speed: f32| {
        let mut f = Flight::on_flat();
        f.state.pos.y = 2.0;
        f.state.vel.y = -speed;
        f.state.parked = false;
        f.state.skids = 0;
        f.run(2.0, HeliInput::default());
        (f.state.health.structure / f.cfg.damage.structure_hp, f.state.wrecked)
    };
    assert_eq!(drop(1.5), (1.0, false));
    let (hurt, wrecked) = drop(6.0);
    assert!(hurt < 0.9 && !wrecked, "hurt {hurt}");
    assert!(drop(12.0).1);
}

#[test]
fn a_blade_strike_takes_the_rotor() {
    let mut f = Flight::on_flat();
    f.hovering_at(10.0);
    f.state.collective = 0.5;
    f.solid.add_box(Vec3::new(6.0, 10.0, 0.0), Vec3::new(0.5, 6.0, 4.0), Quat::IDENTITY, Surface::Concrete);
    f.run(0.5, HeliInput::default());
    assert!(f.events.iter().any(|e| matches!(e, HeliEvent::RotorStrike { canopy: false, .. })));
    assert!(f.state.health.rotor < f.cfg.damage.rotor_hp * 0.5, "rotor hp {}", f.state.health.rotor);
}

#[test]
fn flying_into_a_wall_breaks_it() {
    let mut f = Flight::on_flat();
    f.hovering_at(30.0);
    f.state.vel = Vec3::new(0.0, 0.0, -40.0);
    f.solid.add_box(Vec3::new(0.0, 30.0, -20.0), Vec3::new(30.0, 30.0, 1.0), Quat::IDENTITY, Surface::Concrete);
    f.run(1.5, HeliInput::default());
    assert!(f.state.wrecked);
    assert!(f.state.pos.z > -22.0, "went through to {}", f.state.pos.z);
}

#[test]
fn the_wind_at_the_edge_of_the_map_turns_you_back_at_full_speed() {
    let mut f = Flight::on_flat();
    f.hovering_at(150.0);
    f.state.pos.x = 400.0;
    f.state.vel = Vec3::new(65.0, 0.0, 0.0);
    let mut furthest = 0.0f32;
    for _ in 0..1200 {
        f.tick(&HeliInput::default());
        furthest = furthest.max(f.state.pos.x);
    }
    assert!(furthest < f.half_size + 60.0, "got to {furthest}");
    assert!(f.state.pos.x < furthest);
}

#[test]
fn bullets_find_the_part_they_hit() {
    let cfg = HeliConfig::default();
    let state = HeliState::parked_at(&cfg, Vec3::ZERO, 0.0);
    let from_the_side = |at: Vec3| {
        let world_at = state.pos + at;
        hit_zone(&state, &cfg, world_at + Vec3::X * 20.0, Vec3::NEG_X, 40.0).map(|(z, _)| z)
    };
    assert_eq!(from_the_side(Vec3::new(0.0, 0.1, -2.6)), Some(HeliZone::Cockpit));
    assert_eq!(from_the_side(Vec3::new(0.0, 1.3, 1.3)), Some(HeliZone::Engine));
    assert_eq!(from_the_side(Vec3::new(0.0, 0.8, 9.0)), Some(HeliZone::TailRotor));
    assert_eq!(from_the_side(Vec3::new(0.0, 0.5, 5.0)), Some(HeliZone::Structure));
    assert_eq!(from_the_side(Vec3::new(0.0, 8.0, 0.0)), None);

    let (zone, _) = hit_zone(&state, &cfg, state.pos + cfg.rotor.hub + Vec3::new(5.0, 3.0, 0.0), Vec3::NEG_Y, 10.0).unwrap();
    assert_eq!(zone, HeliZone::MainRotor);
    assert!(hit_zone(&state, &cfg, state.pos + cfg.rotor.hub + Vec3::new(0.0, 3.0, -5.0), Vec3::NEG_Y, 10.0).is_none());
}

#[test]
fn it_is_deterministic() {
    let run = || {
        let mut f = Flight::on_flat();
        f.hovering_at(50.0);
        for i in 0..1200 {
            let t = i as f32 * DT;
            f.tick(&HeliInput {
                collective: (t * 1.3).sin(),
                pedals: (t * 0.7).cos(),
                stick: Vec2::new((t * 2.1).sin(), (t * 1.7).cos()) * 0.3,
                engine_switch: false,
            });
        }
        f.state
    };
    assert_eq!(run(), run());
}

fn trace(f: &Flight, i: u32) {
    let (s, c) = (&f.state, &f.cfg);
    println!(
        "t {:5.1} pos {:6.1} {:5.1} {:6.1} vel {:5.1} {:5.1} {:5.1} att up {:4.2} hdg {:5.2} spin {:5.2} {:5.2} {:5.2} T {:6.0} ind {:4.1} rotor {:.2} power {:.2} vortex {:.2} coll {:.2} disc {:5.1} {:5.1} ped {:.2} ground {} wreck {}",
        i as f32 * DT, s.pos.x, s.pos.y, s.pos.z, s.vel.x, s.vel.y, s.vel.z, s.up().y, s.heading(), s.spin.x, s.spin.y, s.spin.z, s.thrust, s.induced,
        s.rotor_fraction(c), s.power, s.vortex, s.collective, s.disc.x.to_degrees(), s.disc.y.to_degrees(), s.pedal, s.on_ground, s.wrecked
    );
}

#[test]
#[ignore = "prints a trace, for tuning"]
fn autorotation_report() {
    let mut f = Flight::on_flat();
    f.hovering_at(600.0);
    f.state.vel = Vec3::new(0.0, 0.0, -30.0);
    damage::apply(&mut f.state, &HeliConfig::default(), HeliZone::Engine, 300.0);
    let mut pilot = Autopilot::default();
    for i in 0..1800 {
        glide(&mut f, &mut pilot, 28.0, 0.12);
        if i % 120 == 0 {
            trace(&f, i);
        }
    }
}

/// Flies a whole sortie as a student would be asked to, from a start on the ground to a landing.
struct Sortie {
    flight: Flight,
    pilot: Autopilot,
    tick: u32,
    verbose: bool,
    hover_box: Vec3,
    top_speed: f32,
    touchdown: f32,
}

impl Sortie {
    fn new(flight: Flight) -> Self {
        Self { flight, pilot: Autopilot::default(), tick: 0, verbose: false, hover_box: Vec3::ZERO, top_speed: 0.0, touchdown: 0.0 }
    }

    fn agl(&self) -> f32 {
        let s = &self.flight.state;
        let world = &self.flight.solid;
        let foot = s.pos + Vec3::Y * self.flight.cfg.gear.points[0].y;
        world.raycast(foot + Vec3::Y, Vec3::NEG_Y, 400.0).map_or(400.0, |hit| hit.distance - 1.0)
    }

    fn fly(&mut self, order: Order) {
        let input = self.pilot.fly(&self.flight.state, &order, DT);
        self.flight.tick(&input);
        self.tick += 1;
        self.top_speed = self.top_speed.max(self.flight.state.vel.length());
        if self.verbose && self.tick % 120 == 0 {
            trace(&self.flight, self.tick);
        }
        for event in self.flight.events.drain(..) {
            if let HeliEvent::Touchdown { speed } = event {
                self.touchdown = self.touchdown.max(speed);
            }
        }
    }

    fn start(&mut self) {
        self.flight.tick(&HeliInput { engine_switch: true, ..Default::default() });
        while self.flight.state.rotor_fraction(&self.flight.cfg) < 0.97 {
            let input = HeliInput { stick: Vec2::ZERO, ..Default::default() };
            self.flight.tick(&input);
            self.tick += 1;
            assert!(self.tick < 60 * 40, "the rotor never came up: {}", self.flight.state.rotor_fraction(&self.flight.cfg));
        }
    }

    fn take_off_to(&mut self, height: f32) {
        let heading = self.flight.state.heading();
        while self.agl() < height {
            let climb = (0.5 + (height - self.agl()) * 0.4).min(2.5);
            self.fly(Order { velocity: Vec2::ZERO, climb, heading, lean_limit: 0.1 });
            assert!(self.tick < 60 * 120, "never got to height, at {}", self.agl());
        }
    }

    /// Holds still over a point for a while, and records the furthest it strayed.
    fn hover(&mut self, seconds: f32) {
        let at = self.flight.state.pos;
        let heading = self.flight.state.heading();
        self.hover_box = Vec3::ZERO;
        for i in 0..(seconds / DT) as u32 {
            let s = &self.flight.state;
            let order = Order {
                velocity: Vec2::new((at.x - s.pos.x) * 0.35, (at.z - s.pos.z) * 0.35),
                climb: (at.y - s.pos.y) * 0.25,
                heading,
                lean_limit: 0.12,
            };
            self.fly(order);
            if i as f32 * DT > 5.0 {
                self.hover_box = self.hover_box.max((self.flight.state.pos - at).abs());
            }
        }
    }

    /// Towards a point at a speed, keeping height over whatever is below, and slowing for the end of the
    /// leg if asked to. Done once within `near` of it.
    fn leg(&mut self, to: Vec2, speed: f32, height: f32, near: f32, slow_to: Option<f32>) {
        let started = self.tick;
        loop {
            let s = self.flight.state;
            let here = Vec2::new(s.pos.x, s.pos.z);
            let way = to - here;
            let left = way.length();
            if left < near {
                return;
            }
            let flat = way / left;
            let wanted = match slow_to {
                Some(end) => speed.min((end * end + 2.0 * 2.2 * left).sqrt()),
                None => speed,
            };
            let climb = ((height - self.agl()) * 0.35).clamp(-4.0, 6.0);
            let moving = Vec2::new(s.vel.x, s.vel.z);
            // Points where it is going once there is enough speed, and where it wants to go until then.
            let aim = if moving.length() > 8.0 { moving } else { way };
            let heading = (-aim.x).atan2(-aim.y);
            self.fly(Order { velocity: flat * wanted, climb, heading, lean_limit: 0.3 });
            assert!(!self.flight.state.wrecked, "crashed at {} tick {}", self.flight.state.pos, self.tick);
            assert!(self.tick - started < 60 * 120, "leg never finished, at {} going to {to}", self.flight.state.pos);
        }
    }

    fn land_at(&mut self, spot: Vec2) {
        let started = self.tick;
        let heading = self.flight.state.heading();
        while !(self.flight.state.on_ground && self.flight.state.skids == 0b1111) {
            let s = self.flight.state;
            let way = spot - Vec2::new(s.pos.x, s.pos.z);
            let agl = self.agl();
            // Over the spot first, and only then down.
            let off = way.length();
            let sink = if off > 1.5 && agl > 6.0 {
                0.0
            } else if agl > 8.0 {
                -2.0
            } else {
                -0.45 - agl * 0.15
            };
            let flat = way * 0.3;
            self.fly(Order { velocity: flat.clamp_length_max(4.0), climb: sink, heading, lean_limit: 0.1 });
            assert!(!self.flight.state.wrecked, "crashed landing at {}", self.flight.state.pos);
            assert!(self.tick - started < 60 * 120, "never landed, at {} {agl}", self.flight.state.pos);
        }
        for _ in 0..(5.0 / DT) as u32 {
            let input = HeliInput { collective: -1.0, ..Default::default() };
            self.flight.tick(&input);
        }
    }
}

#[test]
#[ignore = "prints a trace, for tuning"]
fn hover_report() {
    let mut sortie = Sortie::new(Flight::on_flat());
    sortie.verbose = true;
    sortie.start();
    sortie.take_off_to(15.0);
    sortie.hover(35.0);
}

#[test]
fn hovers_for_thirty_seconds_in_a_box() {
    let mut sortie = Sortie::new(Flight::on_flat());
    sortie.start();
    sortie.take_off_to(15.0);
    sortie.hover(35.0);
    println!("hover box {}", sortie.hover_box);
    assert!(sortie.hover_box.max_element() < 2.5, "strayed {}", sortie.hover_box);
}

#[test]
fn a_whole_sortie_over_flat_ground() {
    let mut sortie = Sortie::new(Flight::on_flat());
    sortie.verbose = std::env::var("PRS_TRACE").is_ok();
    sortie.start();
    sortie.take_off_to(15.0);
    sortie.hover(35.0);
    sortie.leg(Vec2::new(0.0, -400.0), 40.0, 50.0, 40.0, None);
    sortie.leg(Vec2::new(300.0, -300.0), 40.0, 50.0, 40.0, None);
    sortie.leg(Vec2::new(300.0, 100.0), 40.0, 40.0, 3.0, Some(0.0));
    sortie.land_at(Vec2::new(300.0, 100.0));
    let s = sortie.flight.state;
    println!("flat sortie: box {} top speed {} touchdown {} at {} after {} s", sortie.hover_box, sortie.top_speed, sortie.touchdown, s.pos, sortie.tick as f32 * DT);
    assert!(sortie.top_speed > 38.0);
    assert!(sortie.touchdown < sortie.flight.cfg.damage.landing_safe);
    assert_eq!(s.health.structure, sortie.flight.cfg.damage.structure_hp);
    assert!(s.parked);
}


/// Off the pad at the southern base, along a dogleg over the ridge and into the market square.
#[test]
fn a_sortie_over_the_town_ends_on_the_market_square() {
    let level = prs_level::Level::named("town").unwrap();
    let pad = level.helipads[0];
    let square = level.capture_points.iter().find(|p| p.name == "Market").unwrap().center;
    let spot = Vec2::new(square.x + 1.5, square.z + 3.0);

    let cfg = HeliConfig::default();
    let state = HeliState::parked_at(&cfg, pad.pos, pad.yaw);
    let flight = Flight { cfg, state, solid: level.collision(), canopy: level.canopies(), half_size: level.terrain.size * 0.5, events: Vec::new() };
    let mut sortie = Sortie::new(flight);
    sortie.verbose = std::env::var("PRS_TRACE").is_ok();
    sortie.start();
    sortie.take_off_to(14.0);
    sortie.hover(35.0);
    let hover_box = sortie.hover_box;
    sortie.leg(Vec2::new(320.0, 0.0), 40.0, 55.0, 30.0, None);
    sortie.leg(Vec2::new(150.0, -220.0), 40.0, 55.0, 30.0, None);
    sortie.leg(spot, 40.0, 30.0, 2.5, Some(0.0));
    sortie.land_at(spot);

    let s = sortie.flight.state;
    let off = Vec2::new(s.pos.x, s.pos.z).distance(spot);
    println!(
        "town sortie: hover box {hover_box}, top speed {:.1}, touchdown {:.2} m/s, {off:.2} m from the spot, {:.0} s, structure {}",
        sortie.top_speed, sortie.touchdown, sortie.tick as f32 * DT, s.health.structure
    );
    assert!(hover_box.max_element() < 2.5, "strayed {hover_box}");
    assert!(sortie.top_speed > 38.0 && sortie.top_speed < 50.0, "top speed {}", sortie.top_speed);
    assert!(sortie.touchdown < sortie.flight.cfg.damage.landing_safe, "touched down at {}", sortie.touchdown);
    assert!(off < 4.0, "{off} m from the spot");
    assert_eq!(s.health.rotor, sortie.flight.cfg.damage.rotor_hp, "no blade strikes");
    assert_eq!(s.health.structure, sortie.flight.cfg.damage.structure_hp, "no damage");
    assert!(s.parked);
}

#[test]
fn trees_catch_the_blades_but_not_as_hard_as_a_wall() {
    let mut f = Flight::on_flat();
    f.hovering_at(14.0);
    f.canopy.add_box(Vec3::new(6.5, 16.0, 0.0), Vec3::new(1.5, 4.0, 1.5), Quat::IDENTITY, Surface::Wood);
    f.tick(&HeliInput::default());
    f.run(0.2, HeliInput::default());
    assert!(f.events.iter().any(|e| matches!(e, HeliEvent::RotorStrike { canopy: true, .. })));
    let after_tree = f.state.health.rotor;

    let mut g = Flight::on_flat();
    g.hovering_at(14.0);
    g.solid.add_box(Vec3::new(6.5, 16.0, 0.0), Vec3::new(1.5, 4.0, 1.5), Quat::IDENTITY, Surface::Concrete);
    g.tick(&HeliInput::default());
    g.run(0.2, HeliInput::default());
    assert!(g.state.health.rotor < after_tree, "{} and {after_tree}", g.state.health.rotor);
}

/// Picks a glide, a flare and a cushion, the way an instructor teaches it, from an engine failure.
fn autorotate_to_the_ground(f: &mut Flight, height: f32) -> f32 {
    f.half_size = 5000.0;
    f.hovering_at(height);
    f.state.pos.z = 500.0;
    f.state.vel = Vec3::new(0.0, 0.0, -30.0);
    damage::apply(&mut f.state, &HeliConfig::default(), HeliZone::Engine, 300.0);
    let mut pilot = Autopilot::default();
    let mut hardest = 0.0f32;
    for _ in 0..(120.0 / DT) as u32 {
        let foot = f.state.pos.y + f.cfg.gear.points[0].y;
        let forward = Vec2::new(f.state.forward().x, f.state.forward().z).normalize_or(Vec2::NEG_Y);
        // Glide, then haul the nose up to trade speed for rotor speed, then level and cushion it on.
        let (speed, lean, collective) = if foot > 28.0 {
            (26.0, 0.2, 0.12)
        } else if foot > 5.0 {
            (0.0, 0.42, 0.15)
        } else {
            (0.0, 0.08, 1.0)
        };
        let order = Order { velocity: forward * speed, climb: 0.0, heading: f.state.heading(), lean_limit: lean };
        let mut input = pilot.fly(&f.state, &order, DT);
        input.collective = if collective >= 1.0 { 1.0 } else { (collective - f.state.collective).clamp(-1.0, 1.0) };
        f.tick(&input);
        for event in f.events.drain(..) {
            if let HeliEvent::Touchdown { speed } = event {
                hardest = hardest.max(speed);
            }
        }
        if f.state.on_ground {
            return hardest;
        }
    }
    panic!("never got down, at {}", f.state.pos);
}

#[test]
fn an_autorotation_can_be_landed_and_a_bad_one_cannot() {
    let mut f = Flight::on_flat();
    let landed_at = autorotate_to_the_ground(&mut f, 250.0);
    let rotor = f.state.rotor_fraction(&f.cfg);
    println!("autorotation: touched down at {landed_at} m/s with the rotor at {rotor}, {} m/s forward", f.state.vel.length());
    assert!(landed_at < f.cfg.damage.landing_fatal, "came down at {landed_at} m/s");
    assert!(!f.state.wrecked);

    // Collective left where it was, nothing done, nothing to save it.
    let mut g = Flight::on_flat();
    g.hovering_at(150.0);
    damage::apply(&mut g.state, &HeliConfig::default(), HeliZone::Engine, 300.0);
    g.run(30.0, HeliInput::default());
    assert!(g.state.wrecked || g.state.health.structure < g.cfg.damage.structure_hp, "walked away from that at {}", g.state.pos);
}

/// As a person flies: only looks every so often, sees things a moment late, and the stick is a mouse.
#[test]
fn a_slow_pilot_who_sees_things_late_can_still_hover_it() {
    const LOOK_EVERY: u32 = 15;
    const LATE: usize = 15;
    let mut f = Flight::on_flat();
    f.hovering_at(20.0);
    let mut pilot = Autopilot::default();
    let at = f.state.pos;
    let mut seen = vec![f.state; LATE];
    let mut input = HeliInput::default();
    let mut most = Vec3::ZERO;
    for i in 0..(60.0 / DT) as u32 {
        let late = seen[i as usize % LATE];
        seen[i as usize % LATE] = f.state;
        if i % LOOK_EVERY == 0 {
            let order = Order {
                velocity: Vec2::new((at.x - late.pos.x) * 0.2, (at.z - late.pos.z) * 0.2),
                climb: (at.y - late.pos.y) * 0.2,
                heading: 0.0,
                lean_limit: 0.1,
            };
            input = pilot.fly(&late, &order, LOOK_EVERY as f32 * DT);
        }
        f.tick(&input);
        if i as f32 * DT > 15.0 {
            most = most.max((f.state.pos - at).abs());
        }
    }
    println!("slow pilot: strayed {most}");
    assert!(most.max_element() < 8.0, "strayed {most}");
}

#[test]
#[ignore = "prints what it takes to change the numbers"]
fn handling_report() {
    let lean = |f: &Flight| (-f.state.forward().y.asin().to_degrees(), (f.state.rot * Vec3::X).y.asin().to_degrees());
    let mut f = Flight::on_flat();
    f.half_size = 1.0e5;
    f.hovering_at(300.0);
    let hover = f.state.collective;
    let mut pilot = Autopilot::default();
    let mut forward_at = |f: &mut Flight, speed: f32, lean_limit: f32, seconds: f32| {
        for _ in 0..(seconds / DT) as u32 {
            let climb = (300.0 - f.state.pos.y) * 0.2;
            let order = Order { velocity: Vec2::new(0.0, -speed), climb, heading: 0.0, lean_limit };
            let input = pilot.fly(&f.state, &order, DT);
            f.tick(&input);
            f.state.pos.z = f.state.pos.z.rem_euclid(1000.0) - 500.0;
        }
    };
    println!("hover: collective {hover:.2}");
    forward_at(&mut f, 0.0, 0.2, 10.0);
    println!("hover: power {:.2} lean {:?}", f.state.power, lean(&f));
    for speed in [20.0, 40.0, 55.0, 70.0, 90.0] {
        forward_at(&mut f, speed, 0.5, 30.0);
        println!(
            "wanted {speed}: {:.1} m/s, power {:.2}, collective {:.2}, nose down and left down {:?}, disc {:.1}",
            f.state.airspeed(), f.state.power, f.state.collective, lean(&f), f.state.disc.y.to_degrees()
        );
    }
    forward_at(&mut f, 40.0, 0.3, 20.0);
    let (nose, _) = lean(&f);
    for _ in 0..(10.0 / DT) as u32 {
        f.tick(&HeliInput::default());
        f.state.pos.z = f.state.pos.z.rem_euclid(1000.0) - 500.0;
        f.state.pos.y = f.state.pos.y.max(100.0);
    }
    println!("hands off for 10 s at 40 m/s: nose down {nose:.1} to {:?}, speed {:.1}, sinking {:.1}, yaw {:.2}", lean(&f), f.state.airspeed(), f.state.vel.y, f.state.heading());

    let mut f = Flight::on_flat();
    f.half_size = 1.0e5;
    f.hovering_at(200.0);
    f.run(15.0, HeliInput { collective: 1.0, ..Default::default() });
    println!("full collective from hover: climb {:.1} m/s, rotor {:.2}, power {:.2}", f.state.vel.y, f.state.rotor_fraction(&f.cfg), f.state.power);
    for (name, stick) in [("forward", Vec2::Y), ("right", Vec2::X)] {
        let mut f = Flight::on_flat();
        f.hovering_at(200.0);
        f.run(1.0, HeliInput { stick, ..Default::default() });
        println!("full stick {name} for 1 s: spin {} rad/s", f.state.spin);
    }
    let mut f = Flight::on_flat();
    f.hovering_at(200.0);
    f.run(2.0, HeliInput { pedals: 1.0, ..Default::default() });
    println!("right pedal for 2 s: yaw rate {:.2} rad/s", f.state.spin.y);
}

/// A sortie's worth of flying from a state nudged by what floats on another machine could differ by.
#[test]
fn small_errors_stay_small() {
    let run = |nudge: f32| {
        let mut f = Flight::on_flat();
        f.hovering_at(30.0);
        f.state.pos.x += nudge;
        f.state.rot = (f.state.rot * Quat::from_rotation_z(nudge * 0.1)).normalize();
        f.state.rotor_speed += nudge;
        let mut trace = Vec::new();
        for i in 0..(6.0 / DT) as u32 {
            let t = i as f32 * DT;
            f.tick(&HeliInput { stick: Vec2::new((t * 1.1).sin(), (t * 0.7).cos()) * 0.15, collective: (t * 0.5).sin() * 0.2, ..Default::default() });
            trace.push(f.state);
        }
        trace
    };
    let (a, b) = (run(0.0), run(1.0e-4));
    let t = NetTolerance::default();
    // Twenty ticks is about what a replay at 150 ms of round trip covers.
    assert!(a[20].close_to(&b[20], &t), "{:?} {:?}", a[20], b[20]);
    let worst = a.iter().zip(&b).map(|(x, y)| x.pos.distance(y.pos)).fold(0.0, f32::max);
    println!("a tenth of a millimetre grows to {worst} m in 6 s");
    assert!(worst < 0.02, "grew to {worst} m");
}

#[test]
#[ignore = "prints what a replay costs"]
fn replay_cost_report() {
    let level = prs_level::Level::named("town").unwrap();
    let cfg = HeliConfig::default();
    let pad = level.helipads[0];
    let mut state = HeliState::parked_at(&cfg, pad.pos, pad.yaw);
    let (solid, canopy) = (level.collision(), level.canopies());
    let world = HeliWorld { solid: &solid, canopy: &canopy, half_size: level.terrain.size * 0.5 };
    state.pos = Vec3::new(-10.0, 40.0, 20.0);
    state.engine = Engine::Running;
    state.spool = 1.0;
    state.rotor_speed = cfg.rotor.nominal_speed;
    state.collective = 0.45;
    state.induced = 8.0;
    state.parked = false;
    state.on_ground = false;
    state.skids = 0;
    let input = HeliInput::default();
    let mut events = Vec::new();
    let started = std::time::Instant::now();
    let replays = 200;
    for _ in 0..replays {
        let mut replay = state;
        for _ in 0..20 {
            step(&mut replay, &cfg, &input, &world, DT, &mut events);
        }
        events.clear();
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0 / replays as f64;
    println!("20 ticks over the town: {ms:.3} ms");
}
