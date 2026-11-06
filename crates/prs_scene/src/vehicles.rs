//! Vehicles as the sim has them: drawn where the runtime blends them between ticks, wheels turning and
//! steering, suspension moving, and smoke and fire when they are in a bad way.

use bevy::prelude::*;
use prs_models::{VehicleBody, VehicleDoor, VehicleModel, VehiclePaint, VehicleRoot, VehicleSteeringWheel, VehicleStrut, VehicleWheel, spawn_vehicle};
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::prs_sim::vehicles::{PanelZone, VehicleEvent};
use prs_runtime::{SimMessage, SimResource, VehicleFrames};
use std::collections::HashMap;

/// How one drifting puff or flame looks. All of it is tuning for the eye and does not touch the sim.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct FireLook {
    /// Puffs a second out of something smoking, and out of something on fire.
    pub smoke_rate: f32,
    pub fire_rate: f32,
    pub puff_life: f32,
    pub puff_size: f32,
    pub puff_rise: f32,
    /// Where it comes from, up from the chassis's centre.
    pub above: f32,
    /// The burst when it goes up: how many pieces, how fast and how big.
    pub blast_pieces: usize,
    pub blast_speed: f32,
    pub blast_size: f32,
    /// Never more than this many in the air.
    pub max_puffs: usize,
}

impl Default for FireLook {
    fn default() -> Self {
        Self {
            smoke_rate: 7.0,
            fire_rate: 22.0,
            puff_life: 3.0,
            puff_size: 0.2,
            puff_rise: 3.6,
            above: 0.2,
            blast_pieces: 34,
            blast_speed: 9.0,
            blast_size: 0.4,
            max_puffs: 140,
        }
    }
}

#[derive(Component)]
pub(crate) struct Puff {
    pub(crate) velocity: Vec3,
    pub(crate) life: f32,
    pub(crate) max_life: f32,
    pub(crate) size: f32,
    /// Hot things fade away and do not grow. Smoke does.
    pub(crate) grows: bool,
    /// Smoke thins out in steps as it ages, thickest first, by swapping to a thinner copy.
    pub(crate) thins: Option<[Handle<StandardMaterial>; 3]>,
}

/// Each kind of vehicle's model, built the first time one is seen.
#[derive(Resource, Default)]
pub(crate) struct VehicleModels(HashMap<u8, VehicleModel>);

pub(crate) fn spawn_new_vehicles(
    mut commands: Commands,
    sim: Res<SimResource>,
    paint: Res<VehiclePaint>,
    mut drawn: Local<usize>,
    mut models: ResMut<VehicleModels>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sim = &sim.0;
    for vehicle in *drawn..sim.vehicles.len() {
        let kind = sim.vehicles[vehicle].kind;
        let model = models.0.entry(kind.0).or_insert_with(|| VehicleModel::new(&sim.vehicle_cfgs[kind], &paint, &mut meshes, &mut materials));
        spawn_vehicle(&mut commands, model, vehicle);
    }
    *drawn = (*drawn).max(sim.vehicles.len());
}

pub(crate) fn update_vehicles(
    sim: Res<SimResource>,
    frames: Res<VehicleFrames>,
    mut roots: Query<(&VehicleRoot, &mut Transform), (Without<VehicleWheel>, Without<VehicleStrut>)>,
    mut wheels: Query<(&VehicleWheel, &mut Transform), (Without<VehicleRoot>, Without<VehicleStrut>)>,
    mut struts: Query<(&VehicleStrut, &mut Transform), (Without<VehicleRoot>, Without<VehicleWheel>)>,
    mut steering: Query<(&VehicleSteeringWheel, &mut Transform), (Without<VehicleRoot>, Without<VehicleWheel>, Without<VehicleStrut>)>,
    mut doors: Query<(&VehicleDoor, &mut Transform), (Without<VehicleRoot>, Without<VehicleWheel>, Without<VehicleStrut>, Without<VehicleSteeringWheel>)>,
) {
    let sim = &sim.0;
    for (root, mut transform) in &mut roots {
        if let Some(frame) = frames.blended.get(root.vehicle) {
            transform.translation = frame.pos;
            transform.rotation = frame.rot;
        }
    }
    for (part, mut transform) in &mut wheels {
        let Some(vehicle) = sim.vehicles.get(part.vehicle) else { continue };
        let cfg = &sim.vehicle_cfgs[vehicle.kind];
        let wheel = &cfg.wheels[part.wheel];
        let state = &vehicle.wheels[part.wheel];
        transform.translation = vehicle.axle(cfg, part.wheel);
        let steer = if wheel.steers { vehicle.steer } else { 0.0 };
        // Rolling forwards a wheel turns top towards -Z, which is backwards about +X.
        transform.rotation = Quat::from_rotation_y(-steer) * Quat::from_rotation_x(-state.angle);
        let squash = if state.tyre <= 0.0 { (wheel.radius - cfg.tyres.flat_drop) / wheel.radius } else { 1.0 };
        transform.scale = Vec3::new(1.0, squash, squash);
    }
    for (part, mut transform) in &mut doors {
        let Some(vehicle) = sim.vehicles.get(part.vehicle) else { continue };
        let Some(door) = sim.vehicle_cfgs[vehicle.kind].entries[part.entry].door else { continue };
        let share = sim.vehicle_doors.get(part.vehicle).and_then(|doors| doors.get(part.entry)).copied().unwrap_or(0.0);
        transform.rotation = Quat::from_rotation_y(door.open * share);
    }
    for (part, mut transform) in &mut steering {
        let Some(vehicle) = sim.vehicles.get(part.vehicle) else { continue };
        transform.rotation = sim.vehicle_cfgs[vehicle.kind].steering_wheel_turn(vehicle.steer);
    }
    for (part, mut transform) in &mut struts {
        let Some(vehicle) = sim.vehicles.get(part.vehicle) else { continue };
        let cfg = &sim.vehicle_cfgs[vehicle.kind];
        let wheel = &cfg.wheels[part.wheel];
        let axle = vehicle.axle(cfg, part.wheel);
        let top = wheel.mount + Vec3::Y * wheel.radius * 0.5 - Vec3::X * wheel.mount.x.signum() * 0.12;
        let span = top - axle;
        transform.translation = top;
        transform.rotation = Quat::from_rotation_arc(Vec3::Y, span.normalize_or(Vec3::Y));
        transform.scale = Vec3::new(1.0, span.length(), 1.0);
    }
}

/// Swaps a wreck's paint for the burnt look, and back on a new one.
pub(crate) fn scorch_wrecks(
    sim: Res<SimResource>,
    models: Res<VehicleModels>,
    mut bodies: Query<(&VehicleBody, &mut MeshMaterial3d<StandardMaterial>), Without<VehicleDoor>>,
    mut doors: Query<(&VehicleDoor, &mut MeshMaterial3d<StandardMaterial>), Without<VehicleBody>>,
) {
    let sim = &sim.0;
    for (door, mut material) in &mut doors {
        let Some(vehicle) = sim.vehicles.get(door.vehicle) else { continue };
        let Some(model) = models.0.get(&vehicle.kind.0) else { continue };
        let wanted = if vehicle.damage.wrecked { &model.burnt } else { &model.painted };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
    for (body, mut material) in &mut bodies {
        let Some(vehicle) = sim.vehicles.get(body.vehicle) else { continue };
        let Some(model) = models.0.get(&vehicle.kind.0) else { continue };
        let wanted = if vehicle.damage.wrecked { &model.burnt } else { &model.painted };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}

#[derive(Default)]
pub(crate) struct PuffAssets {
    mesh: Option<Handle<Mesh>>,
    smoke: Option<[Handle<StandardMaterial>; 3]>,
    black: Option<[Handle<StandardMaterial>; 3]>,
    flame: Option<Handle<StandardMaterial>>,
}

/// Pours smoke and flame out of whatever is hurt enough.
pub(crate) fn emit_puffs(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<SimResource>,
    look: Res<FireLook>,
    mut messages: MessageReader<SimMessage>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<PuffAssets>,
    mut owed: Local<HashMap<usize, f32>>,
    mut seed: Local<u32>,
    existing: Query<(), With<Puff>>,
) {
    // Faceted blobs, the low-poly way of drawing something round, one size across.
    let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Sphere::new(0.5).mesh().ico(1).expect("a small icosphere"))).clone();
    let mut thinning = |slot: &mut Option<[Handle<StandardMaterial>; 3]>, shade: f32, thickest: f32| {
        slot.get_or_insert_with(|| {
            [1.0, 0.6, 0.28].map(|share| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(shade, shade, shade * 0.98, thickest * share),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    ..default()
                })
            })
        })
        .clone()
    };
    let smoke = thinning(&mut assets.smoke, 0.45, 0.55);
    let black = thinning(&mut assets.black, 0.09, 0.72);
    let flame = assets
        .flame
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.55, 0.15), emissive: LinearRgba::rgb(7.0, 2.6, 0.4), perceptual_roughness: 1.0, ..default() }))
        .clone();
    let mut random = move || {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut count = existing.iter().count();
    let dt = time.delta_secs();
    let sim = &sim.0;
    for (v, vehicle) in sim.vehicles.iter().enumerate() {
        let d = &sim.vehicle_cfgs[vehicle.kind].damage;
        let rate = if vehicle.damage.burning.is_some() {
            look.fire_rate
        } else if vehicle.damage.smoking(d) || vehicle.damage.wrecked {
            look.smoke_rate
        } else {
            owed.remove(&v);
            continue;
        };
        let due = owed.entry(v).or_default();
        *due += rate * dt;
        while *due >= 1.0 {
            *due -= 1.0;
            if count >= look.max_puffs {
                continue;
            }
            count += 1;
            let hot = vehicle.damage.burning.is_some() && random() < 0.45;
            let thick = vehicle.damage.burning.is_some() || vehicle.damage.wrecked;
            let (handle, thins, life, size) = if hot {
                (flame.clone(), None, look.puff_life * 0.16, look.puff_size * 1.2)
            } else if thick {
                (black[0].clone(), Some(black.clone()), look.puff_life, look.puff_size * 1.5)
            } else {
                (smoke[0].clone(), Some(smoke.clone()), look.puff_life * 0.7, look.puff_size)
            };
            // Out of the engine bay, which is where it starts.
            let bay = sim.vehicle_cfgs[vehicle.kind].panels.iter().find(|p| p.zone == PanelZone::Engine);
            let (middle, half) = bay.map_or((Vec3::ZERO, Vec3::ONE), |p| (p.center + Vec3::Y * p.half.y, p.half));
            let spread = Vec3::new(random() - 0.5, 0.0, random() - 0.5);
            let at = vehicle.pos + vehicle.rot * (middle + spread * half * 1.6) + Vec3::Y * look.above;
            let velocity = Vec3::Y * look.puff_rise * (0.7 + random() * 0.6) + spread * 1.6 + vehicle.vel * 0.5;
            commands.spawn((
                Puff { velocity, life, max_life: life, size, grows: !hot, thins },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(handle),
                Transform::from_translation(at).with_scale(Vec3::splat(size)),
            ));
        }
    }
    for SimMessage(event) in messages.read() {
        if let SimEvent::Vehicle { event: VehicleEvent::Exploded { point }, .. } = *event {
            for i in 0..look.blast_pieces {
                let dir = Vec3::new(random() - 0.5, random() * 0.8, random() - 0.5).normalize_or(Vec3::Y);
                let velocity = dir * look.blast_speed * (0.4 + random());
                let hot = i % 2 == 0;
                let life = look.puff_life * if hot { 0.35 } else { 0.8 };
                commands.spawn((
                    Puff { velocity, life, max_life: life, size: look.blast_size, grows: !hot, thins: (!hot).then(|| black.clone()) },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(if hot { flame.clone() } else { black[0].clone() }),
                    Transform::from_translation(point + Vec3::Y * look.above).with_scale(Vec3::splat(look.blast_size)),
                ));
            }
        }
    }
}

pub(crate) fn update_puffs(mut commands: Commands, time: Res<Time>, mut puffs: Query<(Entity, &mut Puff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>) {
    let dt = time.delta_secs();
    for (entity, mut puff, mut transform, mut material) in &mut puffs {
        puff.life -= dt;
        if puff.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        puff.velocity *= (-1.4 * dt).exp();
        let step = puff.velocity * dt;
        transform.translation += step;
        transform.rotate_y(dt * 0.8);
        let t = puff.life / puff.max_life;
        let swell = if puff.grows { 1.0 + (1.0 - t) * 2.2 } else { 0.4 + t * 0.6 };
        transform.scale = Vec3::splat(puff.size * swell * t.powf(0.3));
        if let Some(steps) = &puff.thins {
            let step = if t > 0.6 { 0 } else if t > 0.3 { 1 } else { 2 };
            if material.0 != steps[step] {
                material.0 = steps[step].clone();
            }
        }
    }
}
