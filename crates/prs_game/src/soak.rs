//! The real game with no window and no graphics card, a seeded monkey at the keys and mouse, and the
//! clock run as fast as the machine can go. `PRS_SOAK=<simulated seconds>[:seed]`, along with `PRS_MAP`,
//! `PRS_MODE`, `PRS_WALKERS`, `PRS_MOD` and `PRS_SERVER`. Any panic is a bug, and exits 101 with where.
//!
//! Input goes in where the operating system's does, as keyboard and mouse messages, so everything from
//! there on is what a player runs. `PRS_SOAK_WINDOW=1` keeps the window and the renderer and runs in real time. Now and then the monkey is also picked up and put by something
//! interesting, as it would take a long time to wander into a helicopter's door on its own.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::time::TimeUpdateStrategy;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitPlugin;
use prs_input::Captured;
use prs_runtime::prs_sim::health::HealthEvent;
use prs_runtime::prs_sim::{CarryEvent, SeatEvent, SimEvent};
use prs_runtime::prs_sim::weapons::WeaponEvent;
use prs_runtime::{LocalSoldier, PendingInput, SimMessage, SimResource};
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_nanos(16_666_667);
/// Keys it knows, with the ones that do the most held down the most.
const KEYS: [(KeyCode, u32); 31] = [
    (KeyCode::KeyB, 3),
    (KeyCode::KeyG, 3),
    (KeyCode::KeyW, 20),
    (KeyCode::KeyA, 6),
    (KeyCode::KeyS, 3),
    (KeyCode::KeyD, 6),
    (KeyCode::ShiftLeft, 8),
    (KeyCode::Space, 6),
    (KeyCode::KeyX, 2),
    (KeyCode::KeyZ, 2),
    (KeyCode::KeyQ, 2),
    (KeyCode::KeyE, 2),
    (KeyCode::AltLeft, 3),
    (KeyCode::KeyR, 3),
    (KeyCode::KeyV, 2),
    (KeyCode::Digit1, 2),
    (KeyCode::Digit2, 2),
    (KeyCode::Digit3, 2),
    (KeyCode::Digit4, 2),
    (KeyCode::KeyF, 5),
    (KeyCode::KeyT, 1),
    (KeyCode::KeyM, 1),
    (KeyCode::KeyI, 1),
    (KeyCode::Tab, 1),
    (KeyCode::Enter, 4),
    (KeyCode::Escape, 1),
    (KeyCode::F3, 1),
    (KeyCode::F4, 1),
    (KeyCode::BracketLeft, 1),
    (KeyCode::BracketRight, 1),
    (KeyCode::Digit5, 1),
];

pub struct Spec {
    seconds: f32,
    seed: u32,
}

pub fn requested() -> Option<Spec> {
    let text = std::env::var("PRS_SOAK").ok()?;
    let mut parts = text.split(':');
    let seconds = parts.next()?.parse().ok()?;
    let seed = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    Some(Spec { seconds, seed })
}

pub fn requested_with_window() -> Option<Spec> {
    std::env::var("PRS_SOAK_WINDOW").ok()?;
    requested()
}

/// The engine with everything but a window and a renderer.
pub fn engine(app: &mut App) -> &mut App {
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>()
            .set(RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(WgpuSettings { backends: None, ..default() })), ..default() }),
    );
    app
}

#[derive(Default, Debug)]
struct Tally {
    shots: u32,
    hit: u32,
    down: u32,
    died: u32,
    respawned: u32,
    climbs: u32,
    flying: u32,
    seats: u32,
    seat_swaps: u32,
    kits_taken: u32,
    reloads: u32,
    treatments: u32,
    near_misses: u32,
}

struct Dice(u32);

impl Dice {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }

    fn chance(&mut self, one_in: u32) -> bool {
        self.below(one_in) == 0
    }

    fn signed(&mut self, most: f32) -> f32 {
        (self.below(2001) as f32 / 1000.0 - 1.0) * most
    }
}

#[derive(Resource)]
struct Monkey {
    dice: Dice,
    held: Vec<KeyCode>,
    left: bool,
    right: bool,
    aim: Vec2,
    window: Option<Entity>,
    frames: u64,
    /// What it has actually got up to, so a clean run says something.
    tally: Tally,
    /// Whether V went down since the last tick was looked at, for checking the fire mode only ever
    /// changes because someone asked.
    v_since: u32,
}

fn key(code: KeyCode, state: ButtonState, window: Entity) -> KeyboardInput {
    KeyboardInput { key_code: code, logical_key: Key::Dead(None), state, text: None, repeat: false, window }
}

/// One frame's worth of a person who has no idea what any of this does.
fn poke(world: &mut World) {
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next();
    let mut monkey = world.remove_resource::<Monkey>().expect("inserted at the start");
    monkey.window = window.or(monkey.window);
    let window = monkey.window.expect("a primary window");
    monkey.frames += 1;

    // Keys go down, and each comes up again on its own time.
    let total: u32 = KEYS.iter().map(|(_, weight)| weight).sum();
    if monkey.dice.chance(6) {
        let mut roll = monkey.dice.below(total);
        for (code, weight) in KEYS {
            if roll < weight {
                if !monkey.held.contains(&code) {
                    monkey.held.push(code);
                    if code == KeyCode::KeyV {
                        // A second is well past any tick that key could have gone out in, even when a
                        // client has slowed down to let a server catch up.
                        monkey.v_since = 60;
                    }
                    world.write_message(key(code, ButtonState::Pressed, window));
                }
                break;
            }
            roll -= weight;
        }
    }
    let mut i = 0;
    while i < monkey.held.len() {
        if monkey.dice.chance(14) {
            let code = monkey.held.swap_remove(i);
            world.write_message(key(code, ButtonState::Released, window));
        } else {
            i += 1;
        }
    }
    for (button, is_left) in [(MouseButton::Left, true), (MouseButton::Right, false)] {
        let down = if is_left { monkey.left } else { monkey.right };
        let flip = monkey.dice.chance(if down { 25 } else { 40 });
        if flip {
            let state = if down { ButtonState::Released } else { ButtonState::Pressed };
            world.write_message(MouseButtonInput { button, state, window });
            if is_left {
                monkey.left = !down;
            } else {
                monkey.right = !down;
            }
        }
    }
    // A mouse that wanders, with now and then the kind of jump browsers produce.
    if monkey.dice.chance(40) {
        monkey.aim = Vec2::new(monkey.dice.signed(60.0), monkey.dice.signed(20.0));
    }
    let spike = monkey.dice.chance(500);
    let delta = if spike { Vec2::new(monkey.dice.signed(4000.0), monkey.dice.signed(4000.0)) } else { monkey.aim };
    world.write_message(MouseMotion { delta });

    // A monkey at the controls wants to fly: engine on, and pull.
    if world.resource::<PendingInput>().at_controls && monkey.dice.chance(3) {
        let local = world.resource::<LocalSoldier>().0;
        let sim = &world.resource::<SimResource>().0;
        let heli = sim.soldiers[local].seat.and_then(|at| prs_runtime::prs_sim::heli_of(at.carrier)).and_then(|h| sim.helicopters.get(h));
        let stopped = heli.is_some_and(|h| h.state.engine == prs_runtime::prs_sim::helicopter::Engine::Off);
        let code = if stopped { KeyCode::Space } else { KeyCode::KeyW };
        if !monkey.held.contains(&code) {
            monkey.held.push(code);
            world.write_message(key(code, ButtonState::Pressed, window));
        }
    }
    // Put it somewhere worth being, looking at it, with a finger on F.
    if monkey.dice.chance(240) {
        let (roll, a, b) = (monkey.dice.next(), monkey.dice.signed(3.0), monkey.dice.signed(3.0));
        if place(world, roll, Vec2::new(a, b)) && !monkey.held.contains(&KeyCode::KeyF) {
            monkey.held.push(KeyCode::KeyF);
            world.write_message(key(KeyCode::KeyF, ButtonState::Pressed, window));
        }
    }
    world.insert_resource(monkey);
}

/// At a door of something, by a kit or by another soldier. Returns whether it moved them.
fn place(world: &mut World, roll: u32, off: Vec2) -> bool {
    let local = world.resource::<LocalSoldier>().0;
    let sim = &mut world.resource_mut::<SimResource>().into_inner().0;
    let mut spots: Vec<(Vec3, Option<Vec3>)> = Vec::new();
    let carriers: Vec<u32> = (0..sim.vehicles.len() as u32).chain((0..sim.helicopters.len()).map(prs_runtime::prs_sim::heli_carrier)).collect();
    for carrier in carriers {
        let frame = sim.carrier_frame(carrier);
        for door in sim.carrier_entries(carrier) {
            spots.push((frame.point(door.stand), Some(frame.point(door.hand))));
        }
    }
    spots.extend(sim.kits.iter().map(|k| (k.pos + off.extend(0.0).xzy() * 0.3, Some(k.pos))));
    spots.extend(sim.soldiers.iter().filter(|s| s.present).map(|s| (s.body.pos + off.extend(0.0).xzy(), None)));
    let Some(soldier) = sim.soldiers.get(local) else { return false };
    if spots.is_empty() || !soldier.present || soldier.seat.is_some() || !soldier.health.is_up() || soldier.body.climb.active() {
        return false;
    }
    let (spot, look_at) = spots[roll as usize % spots.len()];
    let ground = sim.world.raycast(spot + Vec3::Y * 8.0, Vec3::NEG_Y, 16.0).map_or(spot.y, |hit| spot.y + 8.0 - hit.distance);
    sim.soldiers[local].body.pos = Vec3::new(spot.x, ground + 0.05, spot.z);
    sim.soldiers[local].body.vel = Vec3::ZERO;
    if let Some(at) = look_at {
        let to = at - sim.soldiers[local].eye();
        let mut input = world.resource_mut::<PendingInput>();
        input.yaw = (-to.x).atan2(-to.z);
        input.pitch = (to.y / to.length()).asin();
    }
    true
}

/// The stray report of a rifle on AUTO with nobody touching V: the mode only moves if V went down.
fn check_fire_mode(mut messages: MessageReader<SimMessage>, local: Res<LocalSoldier>, sim: Res<SimResource>, mut monkey: ResMut<Monkey>) {
    for SimMessage(event) in messages.read() {
        if let SimEvent::Weapon { soldier, event: WeaponEvent::ModeChanged(mode) } = event
            && *soldier == local.0
        {
            assert!(monkey.v_since > 0, "fire mode went to {mode:?} with nobody pressing V, frame {}", monkey.frames);
        }
        let tally = &mut monkey.tally;
        match *event {
            SimEvent::ShotFired { soldier, .. } if soldier == local.0 => tally.shots += 1,
            SimEvent::SoldierHit { soldier, .. } if soldier == local.0 => tally.hit += 1,
            SimEvent::BulletPassed { soldier, .. } if soldier == local.0 => tally.near_misses += 1,
            SimEvent::Respawned { soldier } if soldier == local.0 => tally.respawned += 1,
            SimEvent::Health { soldier, event: HealthEvent::Downed } if soldier == local.0 => tally.down += 1,
            SimEvent::Health { soldier, event: HealthEvent::Died } if soldier == local.0 => tally.died += 1,
            SimEvent::Weapon { soldier, event: WeaponEvent::ReloadFinished } if soldier == local.0 => tally.reloads += 1,
            SimEvent::Carry { soldier, event: CarryEvent::KitTaken { .. } } if soldier == local.0 => tally.kits_taken += 1,
            SimEvent::Carry { soldier, event: CarryEvent::TreatmentStarted { .. } } if soldier == local.0 => tally.treatments += 1,
            SimEvent::Seat { soldier, event: SeatEvent::Entered { .. } } if soldier == local.0 => tally.seats += 1,
            SimEvent::Seat { soldier, event: SeatEvent::Swapped { .. } } if soldier == local.0 => tally.seat_swaps += 1,
            _ => {}
        }
    }
    monkey.v_since = monkey.v_since.saturating_sub(1);
    let soldier = sim.0.soldiers.get(local.0);
    if soldier.is_some_and(|s| s.body.climb.active()) {
        monkey.tally.climbs += 1;
    }
    if sim.0.helicopters.iter().any(|h| !h.state.on_ground) {
        monkey.tally.flying += 1;
    }
}

/// The same monkey with a real window and a real renderer, in real time, for what only drawing can get
/// wrong. Use with `PRS_SOAK_WINDOW=1`. It ends itself after the seconds asked for.
pub fn watch(mut app: App, spec: Spec) {
    let seed = spec.seed.wrapping_mul(2_654_435_761).max(1);
    let seconds = spec.seconds;
    app.insert_resource(Monkey { dice: Dice(seed), held: Vec::new(), left: false, right: false, aim: Vec2::ZERO, window: None, frames: 0, v_since: 0, tally: Tally::default() })
        .add_systems(First, poke)
        .add_systems(Update, (check_fire_mode, move |time: Res<Time<Real>>, monkey: Res<Monkey>, mut captured: ResMut<Captured>, mut exit: MessageWriter<AppExit>| {
            if monkey.frames == 2 {
                captured.0 = true;
            }
            if time.elapsed_secs() > seconds {
                println!("soak: {:?}", monkey.tally);
                println!("soak: finished {} frames in {seconds:.0} s with no panic", monkey.frames);
                exit.write(AppExit::Success);
            }
        }));
    app.run();
}

pub fn run(mut app: App, spec: Spec) {
    let seed = spec.seed.wrapping_mul(2_654_435_761).max(1);
    // Against a server the clock has to be the real one. Alone it goes as fast as the machine can.
    let real_time = std::env::var("PRS_SERVER").is_ok();
    if !real_time {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    }
    app.insert_resource(Monkey { dice: Dice(seed), held: Vec::new(), left: false, right: false, aim: Vec2::ZERO, window: None, frames: 0, v_since: 0, tally: Tally::default() })
        .add_systems(First, poke)
        .add_systems(Update, check_fire_mode);
    app.finish();
    app.cleanup();
    let started = Instant::now();
    let frames = (spec.seconds / FRAME.as_secs_f32()) as u64;
    for frame in 0..frames {
        if real_time {
            std::thread::sleep(FRAME.saturating_sub(started.elapsed().saturating_sub(FRAME * frame as u32)));
        }
        app.update();
        if frame == 0 {
            app.world_mut().resource_mut::<Captured>().0 = true;
        }
        if frame % 3600 == 0 {
            let sim = &app.world().resource::<SimResource>().0;
            println!(
                "soak: {:.0} s simulated, {:.0} frames a second, {} soldiers, {} bullets, {} kits",
                frame as f32 * FRAME.as_secs_f32(),
                frame as f32 / started.elapsed().as_secs_f32().max(1e-3),
                sim.soldiers.iter().filter(|s| s.present).count(),
                sim.bullets.len(),
                sim.kits.len(),
            );
        }
    }
    println!("soak: {:?}", app.world().resource::<Monkey>().tally);
    println!("soak: finished {frames} frames in {:.0} s with no panic", started.elapsed().as_secs_f32());
}
