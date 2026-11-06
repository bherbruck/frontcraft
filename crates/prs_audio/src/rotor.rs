//! The sound of helicopters, which plays all the time rather than being set off. Three loops per
//! machine, played faster and slower with what it is doing and panned by where it was when the sound
//! you can hear now left it. Close up you get the slap of the blades and the engine. From far away you
//! get a dull beat from over the hill, long before anything comes into sight.

use bevy::audio::Volume;
use bevy::prelude::*;
use prs_runtime::prs_sim::helicopter::Engine;
use prs_runtime::{LocalSoldier, LocalView, SimResource};
use std::sync::Arc;

use crate::hearing::Acoustics;
use crate::{EMITTER_RADIUS, SoundSet, ear_position, synth};

/// Seconds between looking for walls in the way of each helicopter. They do not move fast compared to this.
const MUFFLE_EVERY: f32 = 0.2;
/// Fast rotor speeds bend the pitch of a loop made at nominal. Slower than this is not worth hearing.
const LEAST_SPEED: f32 = 0.15;
/// Keeps a sink's volume and pitch from stepping as they change, per second.
const EASE_RATE: f32 = 8.0;
/// Quieter than this and it might as well not be playing.
const SILENT: f32 = 0.002;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Loop {
    Slap,
    Beat,
    Whine,
}

/// One playing loop of a helicopter.
#[derive(Component)]
pub struct RotorLoop {
    heli: usize,
    which: Loop,
    volume: f32,
}

#[derive(Resource)]
pub(crate) struct RotorBank {
    /// Slap, beat and whine in each set, which differ in how long they run before coming round.
    new: [Handle<AudioSource>; 3],
    current: [Handle<AudioSource>; 3],
}

pub(crate) fn build_rotor_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut set = |thump: Vec<f32>, whine: Vec<f32>| {
        let mut add = |samples: &[f32]| sources.add(AudioSource { bytes: Arc::from(synth::encode(samples)) });
        let beat = add(&synth::muffled(&thump, 240.0));
        [add(&thump), beat, add(&whine)]
    };
    let new = set(synth::rotor_thump_of(synth::LONG_THUMPS), synth::turbine_whine_of(synth::LONG_WHINE));
    let current = set(synth::rotor_thump(), synth::turbine_whine());
    commands.insert_resource(RotorBank { new, current });
}

/// How much walls are taking off a helicopter's sound, remembered between looks.
#[derive(Default)]
pub(crate) struct Muffles {
    since: f32,
    of: Vec<(f32, f32)>,
}

pub(crate) fn update_rotor_sounds(
    levels: Res<crate::AudioLevels>,
    mut commands: Commands,
    time: Res<Time>,
    bank: Option<Res<RotorBank>>,
    sim: Res<SimResource>,
    view: Res<LocalView>,
    local: Res<LocalSoldier>,
    acoustics: Res<Acoustics>,
    mut muffles: Local<Muffles>,
    mut spawned: Local<usize>,
    mut playing: Local<Option<SoundSet>>,
    mut loops: Query<(Entity, &mut RotorLoop, &mut Transform, Option<&mut SpatialAudioSink>)>,
) {
    let Some(bank) = bank else {
        return;
    };
    let sim = &sim.0;
    // The other set's loops take over at once, easing in as they do whenever one starts.
    let set = levels.sound_set();
    if *playing != Some(set) {
        if playing.is_some() {
            for (entity, ..) in &loops {
                commands.entity(entity).despawn();
            }
            *spawned = 0;
        }
        *playing = Some(set);
    }
    let handles = match set {
        SoundSet::New => &bank.new,
        SoundSet::Current => &bank.current,
    };
    for heli in *spawned..sim.helicopters.len() {
        for (which, sound) in [Loop::Slap, Loop::Beat, Loop::Whine].into_iter().zip(handles) {
            commands.spawn((
                RotorLoop { heli, which, volume: 0.0 },
                AudioPlayer(sound.clone()),
                PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)).with_spatial(true),
                Transform::from_translation(Vec3::NEG_Z * EMITTER_RADIUS),
            ));
        }
    }
    *spawned = (*spawned).max(sim.helicopters.len());

    let dt = time.delta_secs();
    let ear = ear_position(&view);
    let ear_velocity = view.blended.velocity;
    muffles.since += dt;
    let look_again = muffles.since >= MUFFLE_EVERY;
    if look_again {
        muffles.since = 0.0;
    }
    muffles.of.resize(sim.helicopters.len(), (1.0, 1.0));
    let blocked = |from: Vec3, to: Vec3| {
        let path = to - from;
        sim.world.raycast(from, path.normalize_or(Vec3::Y), path.length()).is_some()
    };
    let aboard = sim.soldiers[local.0].seat.map(|at| at.carrier);

    for (_, mut sound, mut transform, sink) in &mut loops {
        let Some(sink) = sink else {
            continue;
        };
        let Some(heli) = sim.helicopters.get(sound.heli) else {
            continue;
        };
        let state = &heli.state;
        let cfg = &sim.heli_cfg;
        let inside = aboard == Some(prs_runtime::prs_sim::heli_carrier(sound.heli));
        let source = if inside { ear + Vec3::Y } else { acoustics.heard_from(state.pos + state.up() * cfg.rotor.hub.y, state.vel, ear) };
        let distance = if inside { 0.0 } else { ear.distance(source) };
        if look_again && !inside && sound.which == Loop::Slap {
            muffles.of[sound.heli] = match acoustics.muffling(source, ear, &blocked) {
                Some(level) => (level.gain, 0.0),
                None => (1.0, 1.0),
            };
        }
        let (gain, sharpness) = if inside { (1.0, 1.0) } else { muffles.of[sound.heli] };
        let turning = (state.rotor_speed / cfg.rotor.nominal_speed).clamp(0.0, 1.3);
        let doppler = if inside { 1.0 } else { acoustics.doppler(source, state.vel, ear, ear_velocity) };
        let work = (state.thrust / (cfg.mass * cfg.gravity)).clamp(0.0, 1.2);
        let load = acoustics.rotor_idle_share + (1.0 - acoustics.rotor_idle_share) * work;
        let slap_share = acoustics.slap_share(distance) * sharpness;
        let thump = acoustics.rotor_thump.volume(distance) * turning * turning * load * gain;
        let running = matches!(state.engine, Engine::Running | Engine::Starting) || state.spool > 0.0;
        let (target, speed) = match sound.which {
            Loop::Slap => (thump * slap_share, turning),
            Loop::Beat => (thump * (1.0 - slap_share * 0.7), turning),
            Loop::Whine => (
                if running { acoustics.rotor_whine.volume(distance) * state.spool * gain * (0.5 + 0.5 * state.power) } else { 0.0 },
                0.45 + 0.55 * state.spool + 0.08 * state.power,
            ),
        };
        let target = if inside { target * acoustics.rotor_inside } else { target };
        sound.volume += (target - sound.volume) * (1.0 - (-EASE_RATE * dt).exp());
        let mut sink = sink;
        // A helicopter standing silent on its pad costs nothing.
        if target < SILENT && sound.volume < SILENT {
            sink.pause();
            continue;
        }
        sink.play();
        sink.set_volume(Volume::Linear(sound.volume * levels.master()));
        sink.set_speed((speed * doppler).max(LEAST_SPEED));
        transform.translation = (source - ear).normalize_or(Vec3::NEG_Z) * EMITTER_RADIUS;
    }
}
