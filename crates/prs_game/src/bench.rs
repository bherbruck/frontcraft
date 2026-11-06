//! A fixed flight over the map that reports how long frames took, for comparing builds and maps.
//!
//! `PRS_BENCH=1`, or `?bench=1` on the web, takes the camera off the player once the level is drawn: a
//! slow turn at head height on the first team's spawn, a wide circle at helicopter height round the
//! middle, then a low pass straight across. It prints one `BENCH` line with load time, frame times per
//! leg and what was drawn, and ends the program. On the web the line goes to the console and the title.
//! `PRS_BENCH_SHOTS=folder` also saves a screenshot half way through each leg, to see what was drawn.

use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use prs_runtime::{LevelReady, SimResource};
use prs_view::WorldCamera;

/// How the flight goes. The defaults are what reports are compared at, so change them only together.
#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Route {
    /// Frames to let shaders compile and buffers fill before timing starts.
    settle_frames: u32,
    settle_seconds: f32,
    eye_height: f32,
    turn_seconds: f32,
    orbit_seconds: f32,
    orbit_height: f32,
    /// Radius of the circle, as a share of the map's size.
    orbit_radius: f32,
    pass_seconds: f32,
    pass_height: f32,
}

impl Default for Route {
    fn default() -> Self {
        Self {
            settle_frames: 30,
            settle_seconds: 4.0,
            eye_height: 1.7,
            turn_seconds: 12.0,
            orbit_seconds: 16.0,
            orbit_height: 300.0,
            orbit_radius: 0.35,
            pass_seconds: 14.0,
            pass_height: 60.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Leg {
    Turn,
    Orbit,
    Pass,
}

const LEGS: [Leg; 3] = [Leg::Turn, Leg::Orbit, Leg::Pass];

#[derive(Default)]
struct LegStats {
    frames_ms: Vec<f32>,
    meshes: Vec<u32>,
    triangles: Vec<u64>,
}

#[derive(Resource)]
struct Bench {
    started: Instant,
    loaded_at: Option<f32>,
    frames_since_load: u32,
    flight_started: Option<Instant>,
    legs: [LegStats; 3],
    shots_taken: [bool; 3],
    done: bool,
}

/// When the program started, so load time includes building the map. `main` sets it first thing.
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn mark_start() {
    STARTED.get_or_init(Instant::now);
}

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        if !requested() {
            return;
        }
        let route = prs_runtime::prs_sim::prs_mods::tuned("bench", Route::default());
        app.insert_resource(route)
            .insert_resource(Bench {
                started: *STARTED.get_or_init(Instant::now),
                loaded_at: None,
                frames_since_load: 0,
                flight_started: None,
                legs: Default::default(),
                shots_taken: [false; 3],
                done: false,
            })
            .add_systems(Update, (time_load, record).chain().run_if(resource_exists::<LevelReady>))
            .add_systems(PostUpdate, fly.before(TransformSystems::Propagate).run_if(resource_exists::<LevelReady>));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn requested() -> bool {
    std::env::var("PRS_BENCH").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[cfg(target_arch = "wasm32")]
fn requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .and_then(|params| params.get("bench"))
        .is_some_and(|v| !v.is_empty() && v != "0")
}

/// The level counts as loaded on the first frame after its meshes exist, which is when they reach the GPU.
fn time_load(mut bench: ResMut<Bench>, meshes: Query<(), With<Mesh3d>>) {
    if bench.loaded_at.is_none() {
        if meshes.is_empty() {
            return;
        }
        bench.frames_since_load += 1;
        if bench.frames_since_load >= 2 {
            bench.loaded_at = Some(bench.started.elapsed().as_secs_f32());
            bench.frames_since_load = 0;
        }
        return;
    }
    bench.frames_since_load += 1;
}

/// Which leg the flight is on and how far through it, or `None` while settling or once it is over.
fn leg_at(bench: &Bench, route: &Route) -> Option<(Leg, f32)> {
    let elapsed = bench.flight_started?.elapsed().as_secs_f32();
    let lengths = [route.turn_seconds, route.orbit_seconds, route.pass_seconds];
    let mut from = 0.0;
    for (leg, length) in LEGS.into_iter().zip(lengths) {
        if elapsed < from + length {
            return Some((leg, (elapsed - from) / length.max(f32::EPSILON)));
        }
        from += length;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    route: Res<Route>,
    time: Res<Time<Real>>,
    sim: Res<SimResource>,
    drawn: Query<(&ViewVisibility, Option<&prs_scene::Triangles>), With<Mesh3d>>,
    all: Query<&prs_scene::Triangles>,
    mut exit: MessageWriter<AppExit>,
) {
    if bench.done || bench.loaded_at.is_none() {
        return;
    }
    if bench.flight_started.is_none() {
        let settled = bench.frames_since_load >= route.settle_frames
            && bench.started.elapsed().as_secs_f32() >= bench.loaded_at.unwrap_or(0.0) + route.settle_seconds;
        if settled {
            bench.flight_started = Some(Instant::now());
        }
        return;
    }
    let Some((leg, through)) = leg_at(&bench, &route) else {
        bench.done = true;
        report(&bench, &sim, all.iter().map(|t| t.0 as u64).sum());
        exit.write(AppExit::Success);
        return;
    };
    let index = LEGS.iter().position(|l| *l == leg).unwrap_or(0);
    if through >= 0.5 && !bench.shots_taken[index] {
        bench.shots_taken[index] = true;
        shoot(&mut commands, leg);
    }
    let frame_ms = time.delta_secs() * 1000.0;
    let stats = &mut bench.legs[index];
    stats.frames_ms.push(frame_ms);
    // Counting what was drawn costs a walk over every mesh, so only every tenth frame.
    if stats.frames_ms.len() % 10 == 1 {
        let (mut count, mut triangles) = (0u32, 0u64);
        for (visibility, counted) in &drawn {
            if visibility.get() {
                count += 1;
                triangles += counted.map_or(0, |t| t.0 as u64);
            }
        }
        stats.meshes.push(count);
        stats.triangles.push(triangles);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn shoot(commands: &mut Commands, leg: Leg) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Ok(folder) = std::env::var("PRS_BENCH_SHOTS") else {
        return;
    };
    let path = std::path::Path::new(&folder).join(format!("{leg:?}.png").to_lowercase());
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}

#[cfg(target_arch = "wasm32")]
fn shoot(_: &mut Commands, _: Leg) {}

fn fly(
    bench: Res<Bench>,
    route: Res<Route>,
    sim: Res<SimResource>,
    mut captured: ResMut<prs_input::Captured>,
    mut camera: Query<&mut Transform, With<WorldCamera>>,
) {
    // As if playing, so the controls and the pause menu are not drawn over what is measured.
    captured.0 = true;
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let level = &sim.0.level;
    let size = level.terrain.size;
    let spawn = level.teams.first().and_then(|t| t.spawns.first()).map_or(Vec3::ZERO, |s| s.pos);
    let ground = |p: Vec3| ground_height(level, p);
    let (leg, t) = match leg_at(&bench, &route) {
        Some(at) => at,
        None if bench.flight_started.is_none() => (Leg::Turn, 0.0),
        None => return,
    };
    *transform = match leg {
        Leg::Turn => {
            let eye = Vec3::new(spawn.x, ground(spawn) + route.eye_height, spawn.z);
            let yaw = t * std::f32::consts::TAU;
            Transform::from_translation(eye).with_rotation(Quat::from_rotation_y(yaw))
        }
        Leg::Orbit => {
            let angle = t * std::f32::consts::TAU;
            let radius = size * route.orbit_radius;
            let at = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
            let eye = Vec3::new(at.x, ground(at) + route.orbit_height, at.z);
            Transform::from_translation(eye).looking_at(Vec3::new(0.0, ground(Vec3::ZERO), 0.0), Vec3::Y)
        }
        Leg::Pass => {
            let half = size * 0.45;
            let at = Vec3::new(-half + t * 2.0 * half, 0.0, half * 0.2);
            let eye = Vec3::new(at.x, ground(at) + route.pass_height, at.z);
            Transform::from_translation(eye).looking_to(Vec3::new(1.0, -0.12, 0.0), Vec3::Y)
        }
    };
}

/// Height of the terrain under `p`, from the triangle that holds it. Slow, but only the camera asks.
fn ground_height(level: &prs_runtime::prs_sim::Level, p: Vec3) -> f32 {
    let terrain = &level.terrain;
    let q = Vec2::new(p.x, p.z);
    for tri in &terrain.indices {
        let [a, b, c] = tri.map(|v| terrain.vertices[v as usize]);
        let (a2, b2, c2) = (a.xz(), b.xz(), c.xz());
        let d = (b2.y - c2.y) * (a2.x - c2.x) + (c2.x - b2.x) * (a2.y - c2.y);
        if d.abs() < f32::EPSILON {
            continue;
        }
        let u = ((b2.y - c2.y) * (q.x - c2.x) + (c2.x - b2.x) * (q.y - c2.y)) / d;
        let v = ((c2.y - a2.y) * (q.x - c2.x) + (a2.x - c2.x) * (q.y - c2.y)) / d;
        let w = 1.0 - u - v;
        if u >= -1e-4 && v >= -1e-4 && w >= -1e-4 {
            return u * a.y + v * b.y + w * c.y;
        }
    }
    0.0
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn report(bench: &Bench, sim: &SimResource, stored: u64) {
    let level = &sim.0.level;
    let mut legs = Vec::new();
    for (leg, stats) in LEGS.iter().zip(&bench.legs) {
        let mut sorted = stats.frames_ms.clone();
        sorted.sort_by(f32::total_cmp);
        let mean = sorted.iter().sum::<f32>() / sorted.len().max(1) as f32;
        let average = |v: &[u32]| v.iter().map(|&x| x as f64).sum::<f64>() / v.len().max(1) as f64;
        let tris = stats.triangles.iter().sum::<u64>() as f64 / stats.triangles.len().max(1) as f64;
        legs.push(format!(
            "\"{leg:?}\":{{\"frames\":{},\"mean_ms\":{mean:.2},\"p50_ms\":{:.2},\"p95_ms\":{:.2},\"max_ms\":{:.2},\"meshes\":{:.0},\"triangles\":{tris:.0}}}",
            sorted.len(),
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.95),
            sorted.last().copied().unwrap_or(0.0),
            average(&stats.meshes),
        ));
    }
    let line = format!(
        "BENCH {{\"map_size\":{},\"stored_triangles\":{stored},\"terrain_triangles\":{},\"solids\":{},\"trees\":{},\"load_s\":{:.2},{}}}",
        level.terrain.size,
        level.terrain.indices.len(),
        level.solids.len(),
        level.trees.len(),
        bench.loaded_at.unwrap_or(0.0),
        legs.join(","),
    );
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&line.clone().into());
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            document.set_title(&line);
        }
    }
}
