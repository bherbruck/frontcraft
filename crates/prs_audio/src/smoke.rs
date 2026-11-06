//! A smoke grenade pouring, which plays for as long as there is smoke coming out of it. Each stretch of the hiss
//! is rendered new when it is needed and fades into the next, so that it is never the same twice and each one
//! takes in whatever is between it and you by then, the way every other sound does. Between them, each voice
//! follows the grenade, you, and the smoke thinning.

use bevy::audio::{SpatialAudioSink, Volume};
use bevy::prelude::*;
use prs_runtime::{LocalView, SimResource};
use std::collections::HashMap;

use crate::hearing::{Acoustics, SmokeHearing};
use crate::{EMITTER_RADIUS, ear_position, rendered, synth};

/// One stretch of hiss playing.
#[derive(Component)]
pub struct SmokeVoice {
    cloud: u32,
    /// Seconds of it left.
    left: f32,
    /// What was in the way when it started.
    gain: f32,
    volume: f32,
}

/// When one grenade's next stretch is due.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Hiss {
    next_in: f32,
    stretches: u32,
}

impl Hiss {
    /// Whether a stretch should start now. Nothing starts with no smoke coming or nobody near enough to hear it.
    fn due(&mut self, dt: f32, every: f32, pouring: f32, heard: f32) -> bool {
        self.next_in -= dt;
        if self.next_in > 0.0 || pouring <= 0.0 || heard <= 0.0 {
            return false;
        }
        self.next_in = every;
        self.stretches = self.stretches.wrapping_add(1);
        true
    }
}

fn wanted_volume(hearing: &SmokeHearing, volume: f32, distance: f32, pouring: f32, gain: f32) -> f32 {
    hearing.volume(distance) * volume * pouring * gain
}

pub(crate) fn update_smoke_hiss(
    levels: Res<crate::AudioLevels>,
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<SimResource>,
    view: Res<LocalView>,
    acoustics: Res<Acoustics>,
    mut propagator: ResMut<crate::Propagator>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut hisses: Local<HashMap<u32, Hiss>>,
    mut counter: Local<u32>,
    mut voices: Query<(Entity, &mut SmokeVoice, &mut Transform, Option<&mut SpatialAudioSink>)>,
) {
    let sim = &sim.0;
    let cfg = &sim.throw_cfg.smoke;
    let (sound, hearing) = (&acoustics.smoke, &acoustics.smoke_hearing);
    let ear = ear_position(&view);
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let ease = (hearing.ease_rate * dt).min(1.0);

    for (entity, mut voice, mut transform, sink) in &mut voices {
        voice.left -= dt;
        let cloud = sim.clouds.iter().find(|c| c.id == voice.cloud);
        let wanted = cloud.map_or(0.0, |cloud| wanted_volume(hearing, sound.volume, sim.smoke_source(cloud).distance(ear), cloud.pouring(cfg), voice.gain));
        voice.volume += (wanted - voice.volume) * ease;
        if voice.left <= 0.0 || (cloud.is_none() && voice.volume <= 1e-3) {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(cloud) = cloud {
            transform.translation = (sim.smoke_source(cloud) - ear).normalize_or_zero() * EMITTER_RADIUS;
        }
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(voice.volume * levels.master() * levels.effects()));
        }
    }

    hisses.retain(|id, _| sim.clouds.iter().any(|c| c.id == *id));
    for cloud in &sim.clouds {
        let source = sim.smoke_source(cloud);
        let distance = source.distance(ear);
        let hiss = hisses.entry(cloud.id).or_default();
        if !hiss.due(dt, sound.every(), cloud.pouring(cfg), hearing.volume(distance)) {
            continue;
        }
        *counter = counter.wrapping_mul(1664525).wrapping_add(1013904223);
        // Only from the very start: someone walking up to one, or arriving late, does not hear it begin.
        let starting = hiss.stretches == 1 && cloud.age < sound.fizz_seconds;
        let mut samples = prs_synth::smoke_hiss(*counter ^ cloud.id, sound, starting);
        let mut gain = 1.0;
        let heard = propagator.heard(&acoustics.propagation, &sim.level, &sim.world, source, ear, now);
        if heard.cuts(&acoustics.propagation) {
            samples = synth::muffled(&samples, heard.cutoff_hz);
        }
        gain *= heard.gain;
        if let Some((cutoff, dulled)) = acoustics.suppressed.at(view.blended.suppression) {
            samples = synth::muffled(&samples, cutoff);
            gain *= dulled;
        }
        let left = samples.len() as f32 / prs_synth::RATE as f32;
        let volume = wanted_volume(hearing, sound.volume, distance, cloud.pouring(cfg), gain);
        commands.spawn((
            SmokeVoice { cloud: cloud.id, left, gain, volume },
            AudioPlayer(sources.add(rendered(&samples))),
            PlaybackSettings::ONCE.with_volume(Volume::Linear(volume)).with_spatial(true),
            Transform::from_translation((source - ear).normalize_or_zero() * EMITTER_RADIUS),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_runtime::prs_sim::throwables::{Cloud, SmokeConfig, ThrowConfig};

    const DT: f32 = 1.0 / 60.0;

    fn smoke_cfg() -> SmokeConfig {
        ThrowConfig::default().smoke
    }

    fn peak_db(samples: &[f32], volume: f32) -> f32 {
        20.0 * (samples.iter().fold(0.0_f32, |most, s| most.max(s.abs())) * volume).log10()
    }

    /// Goes through one whole cloud at a distance, with the times stretches started at.
    fn stretches_over_a_cloud(distance: f32) -> Vec<f32> {
        let (cfg, acoustics) = (smoke_cfg(), Acoustics::default());
        let mut cloud = Cloud::new(1, Vec3::ZERO, &cfg);
        let mut hiss = Hiss::default();
        let mut started = Vec::new();
        for _ in 0..((cfg.grow_time + cfg.hold_time + cfg.fade_time + 10.0) / DT) as usize {
            if hiss.due(DT, acoustics.smoke.every(), cloud.pouring(&cfg), acoustics.smoke_hearing.volume(distance)) {
                started.push(cloud.age);
            }
            cloud.age += DT;
        }
        started
    }

    #[test]
    fn it_starts_at_once_goes_on_with_no_gap_and_stops_when_the_cloud_is_done() {
        let (cfg, acoustics) = (smoke_cfg(), Acoustics::default());
        let started = stretches_over_a_cloud(3.0);
        assert!(started[0] < 0.1, "{}", started[0]);
        for pair in started.windows(2) {
            assert!(pair[1] - pair[0] < acoustics.smoke.stretch_seconds - 0.1, "a gap at {pair:?}");
        }
        let end = cfg.grow_time + cfg.hold_time + cfg.fade_time;
        assert!(*started.last().unwrap() < end, "nothing after the end of it");
        assert!(*started.last().unwrap() > end - acoustics.smoke.stretch_seconds, "and it goes on to the end");
    }

    #[test]
    fn it_fades_as_the_smoke_thins_and_is_silent_when_it_has_gone() {
        let cfg = smoke_cfg();
        let mut cloud = Cloud::new(1, Vec3::ZERO, &cfg);
        let hearing = Acoustics::default().smoke_hearing;
        let at = |cloud: &Cloud| wanted_volume(&hearing, 1.0, 3.0, cloud.pouring(&cfg), 1.0);
        let full = at(&cloud);
        cloud.age = cfg.grow_time + cfg.hold_time;
        assert_eq!(at(&cloud), full, "as loud until it starts to thin");
        cloud.age += cfg.fade_time * 0.5;
        assert!((at(&cloud) - full * 0.5).abs() < 0.01);
        cloud.age = cfg.grow_time + cfg.hold_time + cfg.fade_time;
        assert_eq!(at(&cloud), 0.0);
    }

    #[test]
    fn nobody_too_far_away_makes_it_do_any_work() {
        assert!(stretches_over_a_cloud(Acoustics::default().smoke_hearing.fades_out.1 + 1.0).is_empty());
    }

    /// Walking and slide sounds are about -39 dB at the ear. This should be well above that close by, about
    /// as loud as them a little way off, and gone not much further than that.
    #[test]
    fn it_is_plainly_heard_close_by_and_carries_only_a_short_way() {
        let acoustics = Acoustics::default();
        let (sound, hearing) = (acoustics.smoke, acoustics.smoke_hearing);
        let run = prs_synth::smoke_run(5, &sound, 12.0);
        let settled = &run[(3.0 * prs_synth::RATE as f32) as usize..];
        let at = |distance: f32| peak_db(settled, wanted_volume(&hearing, sound.volume, distance, 1.0, 1.0));
        assert!(at(2.0) > -33.0 && at(2.0) < -24.0, "at 2 m: {}", at(2.0));
        assert!((at(8.0) - at(0.0) + 6.0).abs() < 0.5, "half as loud at 8 m: {} {}", at(0.0), at(8.0));
        assert!(at(20.0) < -39.0, "at 20 m: {}", at(20.0));
        assert_eq!(hearing.volume(hearing.fades_out.1), 0.0);
    }

    /// Its loudest moment, the start, stays a soft fizz: not as loud as a bullet's impact is up close.
    #[test]
    fn the_start_is_a_fizz_and_not_a_bang() {
        let acoustics = Acoustics::default();
        let sound = acoustics.smoke;
        let run = prs_synth::smoke_run(5, &sound, 3.0);
        assert!(peak_db(&run, sound.volume) < -18.0, "{}", peak_db(&run, sound.volume));
    }
}
