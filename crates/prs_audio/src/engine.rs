//! Engines that keep running. One looped voice for each copy of the sound, faded and pitched as the
//! vehicle does, placed by direction like every other sound, and muffled through walls by crossfading
//! to a dull copy of itself, since a live loop cannot be filtered.

use bevy::audio::{SpatialAudioSink, Volume};
use bevy::prelude::*;
use prs_runtime::{LocalView, SimResource};

use crate::hearing::Acoustics;
use crate::{EMITTER_RADIUS, ear_position, synth};

/// How many cylinders the loops are rendered for.
const CYLINDERS: f32 = 4.0;

#[derive(Resource)]
pub(crate) struct EngineLoops {
    clear: Handle<AudioSource>,
    dull: Handle<AudioSource>,
}

pub(crate) fn build_engine_loops(mut commands: Commands, acoustics: Res<Acoustics>, mut sources: ResMut<Assets<AudioSource>>) {
    let rpm = acoustics.engine.base_rpm;
    let mut add = |brightness: f32, seed: u32| {
        let samples = synth::engine_loop(seed, rpm, CYLINDERS, brightness);
        sources.add(AudioSource { bytes: std::sync::Arc::from(synth::encode(&samples)) })
    };
    let loops = EngineLoops { clear: add(1.0, 0x61), dull: add(0.0, 0x62) };
    commands.insert_resource(loops);
}

/// One looping copy of one vehicle's engine.
#[derive(Component)]
pub(crate) struct EngineVoice {
    vehicle: usize,
    clear: bool,
    volume: f32,
}

/// Whether each running vehicle has something in the way, worked out now and then.
#[derive(Default)]
pub(crate) struct Occlusion {
    wait: f32,
    muffled: Vec<Option<crate::hearing::MuffleLevel>>,
}

pub(crate) fn run_engines(
    levels: Res<crate::AudioLevels>,
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<SimResource>,
    view: Res<LocalView>,
    acoustics: Res<Acoustics>,
    loops: Option<Res<EngineLoops>>,
    mut occlusion: Local<Occlusion>,
    mut voices: Query<(Entity, &mut EngineVoice, &mut Transform, Option<&mut SpatialAudioSink>)>,
) {
    let Some(loops) = loops else {
        return;
    };
    let sim = &sim.0;
    let e = &acoustics.engine;
    let ear = ear_position(&view);
    let dt = time.delta_secs();

    occlusion.muffled.resize(sim.vehicles.len(), None);
    occlusion.wait -= dt;
    if occlusion.wait <= 0.0 {
        occlusion.wait = e.occlusion_every;
        let world = &sim.world;
        let blocked = |from: Vec3, to: Vec3| {
            let path = to - from;
            world.raycast(from, path.normalize_or(Vec3::Y), path.length()).is_some()
        };
        for (v, vehicle) in sim.vehicles.iter().enumerate() {
            occlusion.muffled[v] = if vehicle.engine.running { acoustics.muffling(vehicle.pos, ear, blocked) } else { None };
        }
    }

    let inside = view.blended.seat.map(|at| at.carrier as usize);
    let mut sounding = vec![[false; 2]; sim.vehicles.len()];
    let fade = (e.mix_rate * dt).min(1.0);
    for (entity, mut voice, mut transform, sink) in &mut voices {
        let Some(vehicle) = sim.vehicles.get(voice.vehicle) else {
            commands.entity(entity).despawn();
            continue;
        };
        let distance = vehicle.pos.distance(ear);
        let heard = vehicle.engine.running && distance < e.out_of_earshot;
        if !heard && voice.volume <= 1e-3 {
            commands.entity(entity).despawn();
            continue;
        }
        sounding[voice.vehicle][voice.clear as usize] = true;
        let rpm = vehicle.engine.rpm;
        let mix = e.mix(rpm, vehicle.engine.load, distance, inside == Some(voice.vehicle), occlusion.muffled[voice.vehicle]);
        let wanted = if !heard { 0.0 } else if voice.clear { mix.clear } else { mix.dull };
        voice.volume += (wanted - voice.volume) * fade;
        transform.translation = (vehicle.pos - ear).normalize_or_zero() * EMITTER_RADIUS;
        if let Some(mut sink) = sink {
            sink.set_speed(mix.speed);
            sink.set_volume(Volume::Linear(voice.volume * levels.master()));
        }
    }
    for (v, vehicle) in sim.vehicles.iter().enumerate() {
        if !vehicle.engine.running || vehicle.pos.distance(ear) >= e.out_of_earshot {
            continue;
        }
        for (clear, handle) in [(false, &loops.dull), (true, &loops.clear)] {
            if !sounding[v][clear as usize] {
                commands.spawn((
                    EngineVoice { vehicle: v, clear, volume: 0.0 },
                    AudioPlayer(handle.clone()),
                    PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)).with_spatial(true),
                    Transform::from_translation((vehicle.pos - ear).normalize_or_zero() * EMITTER_RADIUS),
                ));
            }
        }
    }
}
