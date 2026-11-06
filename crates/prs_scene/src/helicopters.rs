//! Helicopters as the sim has them, at the moment between ticks the runtime blends them to: rotors
//! turning and tilting, needles on their dials, and dust where the downwash reaches the ground.

use bevy::prelude::*;
use prs_models::{DIALS, HeliBlades, HeliBlur, HeliBody, HeliLayout, HeliModel, HeliNeedle, HeliRotor, HeliTail, build_heli_model, spawn_heli};
use prs_runtime::prs_sim::helicopter::{Reading, dial_angles};
use prs_runtime::{HeliFrames, SimResource};

use crate::vehicles::Puff;

/// Blades are a blur at any speed above this share of nominal, and go from solid to gone over the same.
const BLUR_FROM: f32 = 0.35;
const BLADES_TO: f32 = 0.6;
/// How solid the disc of a rotor at full speed looks.
const BLUR_ALPHA: f32 = 0.2;
/// The tail rotor turns this many times for each turn of the main one.
const TAIL_GEARING: f32 = 5.2;
/// As far down as a gauge will look for the ground.
const RADAR_REACH: f32 = 500.0;

/// One helicopter's own copy of the blur, because its alpha follows its rotor.
#[derive(Component)]
pub(crate) struct BlurOf(Handle<StandardMaterial>);

pub(crate) fn spawn_new_helicopters(
    mut commands: Commands,
    sim: Res<SimResource>,
    mut drawn: Local<usize>,
    mut model: Local<Option<HeliModel>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sim = &sim.0;
    for heli in *drawn..sim.helicopters.len() {
        let cfg = &sim.heli_cfg;
        let model = model.get_or_insert_with(|| build_heli_model(&mut meshes, &mut materials, cfg.rotor.radius, cfg.rotor.chord, cfg.tail.radius));
        let tail = cfg.tail_rotor_hub();
        let root = spawn_heli(&mut commands, model, heli, HeliLayout { hub: cfg.rotor.hub, tail });
        let blur = materials.get(&model.blur_paint).cloned().map(|paint| materials.add(paint)).unwrap_or_default();
        commands.entity(root).insert(BlurOf(blur));
    }
    *drawn = (*drawn).max(sim.helicopters.len());
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_helicopters(
    sim: Res<SimResource>,
    frames: Res<HeliFrames>,
    mut bodies: Query<(&HeliBody, &mut Transform, Option<&BlurOf>), (Without<HeliRotor>, Without<HeliTail>, Without<HeliNeedle>)>,
    mut rotors: Query<(&HeliRotor, &mut Transform), (Without<HeliBody>, Without<HeliTail>, Without<HeliNeedle>)>,
    mut tails: Query<(&HeliTail, &mut Transform, &mut Visibility), (Without<HeliBody>, Without<HeliRotor>, Without<HeliNeedle>)>,
    mut needles: Query<(&HeliNeedle, &mut Transform), (Without<HeliBody>, Without<HeliRotor>, Without<HeliTail>)>,
    mut blades: Query<(&HeliBlades, &mut Visibility), (Without<HeliBlur>, Without<HeliTail>)>,
    mut blurs: Query<(&HeliBlur, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), (Without<HeliBlades>, Without<HeliTail>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sim = &sim.0;
    let cfg = &sim.heli_cfg;
    for (body, mut transform, blur) in &mut bodies {
        let Some(frame) = frames.blended.get(body.0) else { continue };
        transform.translation = frame.pos;
        transform.rotation = frame.rot;
        let Some(heli) = sim.helicopters.get(body.0) else { continue };
        if let Some(BlurOf(handle)) = blur
            && let Some(mut paint) = materials.get_mut(handle)
        {
            let share = ((heli.state.rotor_fraction(cfg) - BLUR_FROM) / (1.0 - BLUR_FROM)).clamp(0.0, 1.0);
            paint.base_color.set_alpha(share * BLUR_ALPHA);
            for (part, _, mut material) in &mut blurs {
                if part.heli == body.0 && material.0 != *handle {
                    material.0 = handle.clone();
                }
            }
        }
    }
    for (part, mut transform) in &mut rotors {
        let Some(heli) = sim.helicopters.get(part.heli) else { continue };
        let state = &heli.state;
        let tilt = Quat::from_rotation_arc(Vec3::Y, state.disc_normal());
        transform.rotation = tilt * Quat::from_rotation_y(state.rotor_angle);
    }
    for (part, mut visible) in &mut blades {
        let Some(heli) = sim.helicopters.get(part.heli) else { continue };
        let fast = heli.state.rotor_fraction(cfg) > BLADES_TO;
        *visible = if heli.state.health.rotor <= 0.0 || fast { Visibility::Hidden } else { Visibility::Inherited };
    }
    for (part, mut visible, _) in &mut blurs {
        let Some(heli) = sim.helicopters.get(part.heli) else { continue };
        let slow = heli.state.rotor_fraction(cfg) < BLUR_FROM;
        *visible = if heli.state.health.rotor <= 0.0 || slow { Visibility::Hidden } else { Visibility::Inherited };
    }
    for (part, mut transform, mut visible) in &mut tails {
        let Some(heli) = sim.helicopters.get(part.heli) else { continue };
        *visible = if heli.state.health.tail <= 0.0 { Visibility::Hidden } else { Visibility::Inherited };
        transform.rotation = Quat::from_rotation_x(heli.state.rotor_angle * TAIL_GEARING);
    }
    for (part, mut transform) in &mut needles {
        let Some(heli) = sim.helicopters.get(part.heli) else { continue };
        if heli.state.health.cockpit <= 0.0 {
            continue;
        }
        let below = sim.world.raycast(heli.state.pos, Vec3::NEG_Y, RADAR_REACH).map_or(RADAR_REACH, |hit| hit.distance);
        let height = (below + cfg.gear.points[0].y).max(0.0);
        let angles = dial_angles(&cfg.dials, &Reading::of(&heli.state, cfg, height));
        let at = DIALS[part.dial];
        transform.translation = at + Vec3::Z * 0.016;
        transform.rotation = Quat::from_rotation_z(-angles[part.dial]);
    }
}

#[derive(Default)]
pub(crate) struct DustAssets {
    mesh: Option<Handle<Mesh>>,
    dust: Option<Handle<StandardMaterial>>,
    smoke: Option<Handle<StandardMaterial>>,
}

/// The downwash lifting dust off the ground in a ring below the rotor, which also shows where the air is
/// going, and smoke from a hurt engine.
pub(crate) fn blow_dust(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<SimResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<DustAssets>,
    mut owed: Local<Vec<(f32, f32)>>,
    mut seed: Local<u32>,
) {
    let sim = &sim.0;
    if sim.helicopters.is_empty() {
        return;
    }
    let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
    let dust = assets
        .dust
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.55, 0.48, 0.36, 0.3),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .clone();
    let smoke = assets
        .smoke
        .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.1, 0.1), perceptual_roughness: 1.0, ..default() }))
        .clone();
    let mut random = move || {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let dt = time.delta_secs();
    let cfg = &sim.heli_cfg;
    let wash = &cfg.wash;
    owed.resize(sim.helicopters.len(), (0.0, 0.0));
    for (h, heli) in sim.helicopters.iter().enumerate() {
        let state = &heli.state;
        let hub = state.pos + state.rot * cfg.rotor.hub;
        let weight = cfg.mass * cfg.gravity;
        let below = sim.world.raycast(hub, Vec3::NEG_Y, wash.reach);
        if let Some(hit) = below {
            let close = 1.0 - hit.distance / wash.reach;
            let strength = (state.thrust / weight).clamp(0.0, 1.0) * close;
            owed[h].0 += wash.rate * strength * dt;
            while owed[h].0 >= 1.0 {
                owed[h].0 -= 1.0;
                let angle = random() * std::f32::consts::TAU;
                let out = Vec3::new(angle.cos(), 0.0, angle.sin());
                let ground = Vec3::new(hub.x, hub.y - hit.distance + 0.2, hub.z);
                let at = ground + out * cfg.rotor.radius * wash.ring * (0.7 + 0.6 * random());
                let life = wash.life * (0.6 + 0.8 * random());
                commands.spawn((
                    Puff { velocity: out * wash.out_speed * (0.6 + 0.8 * random()) + Vec3::Y * wash.rise_speed, life, max_life: life, size: wash.size, grows: true, thins: None },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(dust.clone()),
                    Transform::from_translation(at).with_scale(Vec3::splat(wash.size)),
                ));
            }
        }
        let hurt = state.health.engine < cfg.damage.engine_hp * 0.5 || state.wrecked;
        if hurt {
            owed[h].1 += 10.0 * dt;
            while owed[h].1 >= 1.0 {
                owed[h].1 -= 1.0;
                let at = state.pos + state.rot * Vec3::new(0.0, 1.5, 2.5);
                commands.spawn((
                    Puff { velocity: state.vel * 0.3 + Vec3::Y * 1.5, life: 2.2, max_life: 2.2, size: 0.45, grows: true, thins: None },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(smoke.clone()),
                    Transform::from_translation(at).with_scale(Vec3::splat(0.45)),
                ));
            }
        }
    }
}
