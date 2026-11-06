//! Procedural sound effects, so the game needs no audio assets. Everything renders to 16-bit WAV
//! bytes in the format Bevy already knows how to play: shots and footsteps per event, the rest once.

use prs_runtime::prs_sim::weapons::ReportConfig;
use prs_synth::{Fade, LowPass, Noise, RATE, render};
pub use prs_synth::{contact, contact_crash, crash, footstep, impact, metal_hit, slide, varied_footstep};

const SHOT_DRIVE: f32 = 3.2;
const SHOT_CEILING: f32 = 0.98;

/// Samples between updates of a filter sweep, about 1.5 ms.
const SWEEP_INTERVAL: usize = 64;

/// The rifle report: supersonic crack, chest thump, and a smooth roll off across the range. Rendered
/// for every shot, so the seed varies its character as well as its noise. `brightness` is how much
/// of the sharp front end reaches this listener, from 0 behind the barrel to 1 in front of it.
/// `report` is the character of this particular gun.
pub fn gun_shot(seed: u32, brightness: f32, report: &ReportConfig) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    for _ in 0..8 {
        noise.next();
    }
    let mut vary = |amount: f32| 1.0 + noise.next() * amount;
    let (thump_hz, blast_rate, tail_rate, tail_level) =
        (report.thump_hz * vary(0.12), report.blast_rate * vary(0.2), report.tail_rate * vary(0.2), report.tail_level * vary(0.2));
    let crack_level = report.crack_level * (0.15 + 0.85 * brightness);
    let blast_tone = 0.45 + 0.55 * brightness;

    let (mut body, mut tail, mut thump_filter) = (LowPass::default(), LowPass::default(), LowPass::default());
    let (mut crack_fade, mut blast_fade, mut thump_fade, mut tail_fade) = (Fade::new(300.0), Fade::new(blast_rate), Fade::new(24.0), Fade::new(tail_rate));
    let (mut blast_sweep, mut tail_sweep, mut pitch_sweep) = (Fade::new(7.0), Fade::new(3.0), Fade::new(60.0));
    let thump_filter_step = LowPass::step_size(700.0);
    let (mut blast_step, mut tail_step, mut phase) = (0.0, 0.0, 0.0);

    let count = (1.3 * RATE as f32) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // Filter sweeps are slow enough to update now and then, which saves most of the work.
        let sweep = i % SWEEP_INTERVAL == 0;
        let (blast_amount, tail_amount) = (blast_sweep.next(), tail_sweep.next());
        if sweep {
            blast_step = LowPass::step_size((3400.0 * blast_amount + 500.0) * blast_tone);
            tail_step = LowPass::step_size(1400.0 * tail_amount + 250.0);
        }
        let n = noise.next();
        let pitch = thump_hz * (1.0 + 2.5 * pitch_sweep.next());
        phase += std::f32::consts::TAU * pitch / RATE as f32;
        let thump_level = thump_fade.next() * 0.85;
        let thump = if thump_level > 0.001 { phase.sin() * thump_level } else { 0.0 };

        let sample = n * crack_fade.next() * crack_level
            + body.step_by(n, blast_step) * blast_fade.next() * 2.2
            + thump_filter.step_by(thump, thump_filter_step)
            + tail.step_by(n, tail_step) * tail_fade.next() * tail_level;
        // Pushed hard into the limiter: a gunshot is about as loud as sound output can get.
        let driven = sample * SHOT_DRIVE;
        out.push(driven / (1.0 + driven * driven).sqrt() * SHOT_CEILING);
    }
    out
}

/// What is left of a sound after passing through a wall: the highs go, then its edge.
pub fn muffled(samples: &[f32], cutoff_hz: f32) -> Vec<f32> {
    let step = LowPass::step_size(cutoff_hz);
    let (mut first, mut second) = (LowPass::default(), LowPass::default());
    samples.iter().map(|&s| second.step_by(first.step_by(s, step), step)).collect()
}

pub fn tear() -> Vec<u8> {
    encode(&prs_synth::tear())
}

/// A bullet going past. A supersonic one is a sharp crack, its shock wave, gone in a few milliseconds
/// with a little of the air ringing after it. A slower one is a whizz that falls in pitch as it goes.
/// `closeness` is 1 right by the ear.
pub fn bullet_passing(seed: u32, closeness: f32, cracked: bool) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let closeness = closeness.clamp(0.0, 1.0);
    if cracked {
        let (mut low, mut ring) = (LowPass::default(), LowPass::default());
        let edge = 900.0 + 3000.0 * closeness;
        return render(0.09, |t| {
            let n = noise.next();
            let bright = n - low.step(n, edge);
            let snap = bright * (-t * 900.0).exp() * 2.2;
            let air = ring.step(n, 2400.0) * (-t * 55.0).exp() * 0.3;
            (snap + air).clamp(-1.0, 1.0)
        });
    }
    let (mut low, mut high) = (LowPass::default(), LowPass::default());
    const WHIZZ: f32 = 0.2;
    render(WHIZZ, |t| {
        let u = t / WHIZZ;
        let pitch = 2600.0 - 1500.0 * u;
        let n = noise.next();
        let band = low.step(n, pitch * 1.4) - high.step(n, pitch * 0.7);
        band * (u * std::f32::consts::PI).sin().powi(2) * 1.5
    })
}

/// One heartbeat as you hear it through your own head: a low lub and a softer dub just after it.
pub fn heartbeat() -> Vec<u8> {
    let thump = |t: f32, hz: f32, level: f32| {
        if t < 0.0 {
            return 0.0;
        }
        // Starts soft, so that it is a thud in the chest rather than a click.
        (std::f32::consts::TAU * hz * t).sin() * (-t * 22.0).exp() * (t * 160.0).min(1.0) * level
    };
    encode(&render(0.42, |t| thump(t, 58.0, 0.9) + thump(t - 0.17, 72.0, 0.55)))
}

pub fn dry_click() -> Vec<u8> {
    encode(&prs_synth::dry_click())
}

pub fn clack(seed: u32, pitch: f32, level: f32) -> Vec<u8> {
    encode(&prs_synth::clack(seed, pitch, level))
}

/// A hit on a body: a dull slap you can pick out at range, which is your only hit confirmation.
pub fn body_hit(seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let (mut low, mut mid) = (LowPass::default(), LowPass::default());
    render(0.2, |t| {
        let n = noise.next();
        low.step(n, 380.0) * (-t * 30.0).exp() * 3.0 + mid.step(n, 1500.0) * (-t * 90.0).exp() * 0.9
    })
}

/// A vehicle going up: a blast with a long dark rumble after it.
pub fn blast(seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let (mut boom, mut roar) = (LowPass::default(), LowPass::default());
    let (mut crack, mut tail) = (Fade::new(38.0), Fade::new(1.9));
    let length = (RATE as f32 * 2.4) as usize;
    (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let n = noise.next();
            let low = boom.step(n, 95.0) * 9.0 * tail.next();
            let mid = roar.step(n, 900.0) * 1.6 * crack.next();
            let thump = (t * 44.0 * std::f32::consts::TAU).sin() * (-t * 7.0).exp();
            (low + mid + thump).clamp(-1.0, 1.0)
        })
        .collect()
}

/// One second of an engine at `rpm`, ending where it starts so it can loop. Firing pulses on a low
/// fundamental, with some rasp from the exhaust. `brightness` from 0 to 1 is how much of the top end
/// is left in: the dull copy is what comes through a wall or at idle.
pub fn engine_loop(seed: u32, rpm: f32, cylinders: f32, brightness: f32) -> Vec<f32> {
    let length = RATE as usize;
    // A whole number of firings in the loop, so its end joins its start.
    let firing = (rpm / 60.0 * cylinders * 0.5).round().max(1.0);
    let mut noise = Noise(seed | 1);
    let mut rasp = LowPass::default();
    let wobble = [1.0, 0.78, 1.12, 0.9];
    let hiss: Vec<f32> = (0..length)
        .map(|_| {
            let n = noise.next();
            rasp.step(n, 500.0 + 3500.0 * brightness) * (0.5 + 0.8 * brightness)
        })
        .collect();
    let harmonics = 2.0 + 8.0 * brightness;
    let mut out: Vec<f32> = (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let phase = t * firing;
            let beat = wobble[((phase.floor() as usize) % wobble.len()).min(wobble.len() - 1)];
            // A sharp pulse per firing, from a sum of harmonics that all finish whole cycles in the loop.
            let mut pulse = 0.0;
            let mut k = 1.0;
            while k <= harmonics {
                pulse += (phase * k * std::f32::consts::TAU).sin() / k.powf(1.1 - 0.35 * brightness);
                k += 1.0;
            }
            let rumble = (phase * 0.5 * std::f32::consts::TAU).sin() * 0.5;
            (pulse * 0.38 * beat + rumble) * 0.55
        })
        .collect();
    // The noise only joins up if it is crossfaded over its own start.
    let overlap = RATE as usize / 8;
    for i in 0..length {
        let n = if i < overlap {
            let mix = i as f32 / overlap as f32;
            hiss[i] * mix + hiss[length - overlap + i] * (1.0 - mix)
        } else {
            hiss[i]
        };
        let beat_envelope = 0.7 + 0.3 * (i as f32 / RATE as f32 * firing * std::f32::consts::TAU).sin();
        out[i] = (out[i] + n * beat_envelope * 0.8).clamp(-1.0, 1.0);
    }
    out
}

/// Rendered a little too long and folded back on itself, so that what comes after the end of a loop
/// lands at its start and it can go round for ever without a click.
fn folded(length: usize, tail: usize, mut sample: impl FnMut(usize) -> f32) -> Vec<f32> {
    let mut out: Vec<f32> = (0..length + tail).map(&mut sample).collect();
    for i in 0..tail {
        let share = i as f32 / tail as f32;
        // The fade-in of what follows against the fade-out of what is already there.
        out[i] = out[i] * share + out[length + i] * (1.0 - share);
    }
    out.truncate(length);
    out
}

/// How often a blade comes past at nominal rotor speed, and how many of them a loop holds. They all
/// differ a little, and a whole number of them fits, so the beat goes round unbroken.
const THUMP_HZ: f32 = 10.8;
const THUMPS: usize = 12;
/// In the New set: long enough for its noise not to be heard coming round, over six seconds.
pub const LONG_THUMPS: usize = 65;
/// Seconds of turbine in the New set, a whole number so its tones finish whole cycles.
pub const LONG_WHINE: usize = 6;

/// The main rotor: a slap for every blade going by, every other one a little stronger because no two
/// blades track quite alike, over a rumble. Played faster and slower with the rotor.
pub fn rotor_thump() -> Vec<f32> {
    rotor_thump_of(THUMPS)
}

pub fn rotor_thump_of(thumps: usize) -> Vec<f32> {
    let period = RATE as f32 / THUMP_HZ;
    let length = (period * thumps as f32) as usize;
    let mut noise = Noise(0x4EE1);
    let levels: Vec<f32> = (0..thumps).map(|i| (if i % 2 == 0 { 1.0 } else { 0.72 }) * (1.0 + noise.next() * 0.08)).collect();
    let (mut slap, mut rumble) = (LowPass::default(), LowPass::default());
    folded(length, RATE as usize / 8, |i| {
        let n = noise.next();
        let beat = i as f32 / period;
        let which = beat as usize;
        let t = (beat - which as f32) / THUMP_HZ;
        let level = levels[which % thumps];
        let hit = (-32.0 * t).exp() * level;
        let body = (std::f32::consts::TAU * 62.0 * t).sin() * hit * 0.8;
        let crack = slap.step(n, 900.0) * (-70.0 * t).exp() * level * 2.6;
        let under = rumble.step(n, 130.0) * 0.8;
        ((body + crack + under) * 1.4).clamp(-1.0, 1.0)
    })
}

/// The turbine: a whine of a few steady tones over hiss, with the buzz of the tail rotor under it.
pub fn turbine_whine() -> Vec<f32> {
    turbine_whine_of(1)
}

pub fn turbine_whine_of(seconds: usize) -> Vec<f32> {
    let length = RATE as usize * seconds;
    let mut noise = Noise(0x7B1E);
    let mut hiss = LowPass::default();
    folded(length, RATE as usize / 8, |i| {
        let t = i as f32 / RATE as f32;
        let tone = |hz: f32| (std::f32::consts::TAU * hz * t).sin();
        let n = noise.next();
        // Noise less what a filter keeps of it leaves the top end.
        let high = n - hiss.step(n, 2600.0);
        let buzz = (std::f32::consts::TAU * 27.0 * t).sin().powi(15);
        (tone(2950.0) * 0.16 + tone(4410.0) * 0.1 + tone(6230.0) * 0.05 + high * 0.1 + buzz * tone(420.0) * 0.32).clamp(-1.0, 1.0)
    })
}

pub fn encode(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_synth::StepSounds;
    use prs_runtime::prs_sim::weapons::WeaponTable;

    #[test]
    fn wav_header_describes_the_payload() {
        let bytes = dry_click();
        assert_eq!(&bytes[..4], b"RIFF");
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 44 + data);
    }

    use prs_runtime::prs_sim::weapons::WeaponConfig;

    fn rifle_shot(seed: u32, brightness: f32) -> Vec<f32> {
        gun_shot(seed, brightness, &WeaponConfig::assault_rifle().report)
    }

    #[test]
    fn each_gun_sounds_like_itself() {
        let shot = |cfg: WeaponConfig| gun_shot(7, 1.0, &cfg.report);
        let (rifle, pistol, lmg) = (shot(WeaponConfig::assault_rifle()), shot(WeaponConfig::pistol()), shot(WeaponConfig::machine_gun()));
        assert!(sharpness(&pistol) < sharpness(&rifle), "a slow round has less crack");
        assert_ne!(rifle, lmg);
        for cfg in [WeaponConfig::pistol(), WeaponConfig::machine_gun(), WeaponConfig::marksman_rifle()] {
            for seed in 0..10 {
                let peak = gun_shot(seed * 977, 1.0, &cfg.report).iter().fold(0.0_f32, |p, s| p.max(s.abs()));
                assert!(peak > 0.5 && peak < 1.0, "{}: {peak}", cfg.name);
            }
        }
    }

    /// Share of a sound's energy that is low enough to feel like weight rather than hiss.
    fn low_share(samples: &[f32], cutoff_hz: f32) -> f32 {
        let mut low = LowPass::default();
        let (mut under, mut all) = (0.0, 0.0);
        for &s in samples {
            let l = low.step(s, cutoff_hz);
            under += l * l;
            all += s * s;
        }
        under / all
    }

    /// Writes the rifle the tuner page plays to compare loudness against. Run it if the rifle's sound changes.
    #[test]
    #[ignore = "writes a file"]
    fn write_tuner_reference_shot() {
        let shot = gun_shot(1, 1.0, &WeaponConfig::assault_rifle().report);
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/tune/rifle.wav"), encode(&shot)).unwrap();
    }

    #[test]
    fn footsteps_are_mostly_low_on_every_surface() {
        let steps = StepSounds::default();
        for (surface, sound) in [("dirt", steps.dirt), ("grass", steps.grass), ("gravel", steps.gravel), ("concrete", steps.concrete), ("wood", steps.wood), ("metal", steps.metal)] {
            for seed in 0..20 {
                let step = footstep(seed * 131, &sound);
                let share = low_share(&step, 500.0);
                assert!(share > 0.5, "{surface} seed {seed}: only {share} below 500 Hz");
                assert!(step.iter().all(|s| s.abs() <= 1.0));
                let varied = varied_footstep(seed * 131, &sound, &prs_synth::StepVary::default());
                let share = low_share(&varied, 500.0);
                assert!(share > 0.5, "{surface} seed {seed} varied: only {share} below 500 Hz");
            }
        }
    }

    #[test]
    fn each_surface_sounds_different_and_no_two_steps_match() {
        let steps = StepSounds::default();
        let dirt = footstep(7, &steps.dirt);
        let gravel = footstep(7, &steps.gravel);
        let wood = footstep(7, &steps.wood);
        assert!(sharpness(&gravel) > sharpness(&dirt) * 1.2, "gravel crunches more than dirt");
        assert!(low_share(&wood, 300.0) > 0.0 && wood != dirt && gravel != dirt);
        assert_ne!(footstep(1, &steps.dirt), footstep(2, &steps.dirt));
    }

    #[test]
    fn a_gunshot_is_loud_and_keeps_going_past_the_first_instant() {
        for cfg in &WeaponTable::default().weapons {
            let shot = gun_shot(7, 0.95, &cfg.report);
            let rms = |w: &[f32]| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt();
            assert!(rms(&shot[..RATE as usize * 150 / 1000]) > 0.4, "not loud enough up front");
        }
    }

    #[test]
    #[ignore = "prints how loud each gun is"]
    fn loudness_report() {
        for cfg in &WeaponTable::default().weapons {
            let shot = gun_shot(7, 0.95, &cfg.report);
            let n = |ms: usize| RATE as usize * ms / 1000;
            let rms = |w: &[f32]| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt();
            let peak = shot.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
            println!("thump {:>5.0}: peak {peak:.2} rms 0-50ms {:.3} 0-150ms {:.3} 150-500ms {:.3}", cfg.report.thump_hz, rms(&shot[..n(50)]), rms(&shot[..n(150)]), rms(&shot[n(150)..n(500)]));
        }
    }

    #[test]
    fn a_heartbeat_is_low_soft_and_ends_in_silence() {
        let bytes = heartbeat();
        let samples: Vec<f32> = bytes[44..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32).collect();
        let peak = samples.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
        assert!(peak > 0.4 && peak <= 1.0, "{peak}");
        // No sample leaps far from the last, which is what makes it a thud and not a click.
        assert!(samples.windows(2).all(|w| (w[1] - w[0]).abs() < 0.03));
        assert!(samples.last().unwrap().abs() < 0.02, "it must not stop with a snap");
    }

    #[test]
    fn a_crack_is_short_and_sharp_and_a_whizz_is_longer_and_softer() {
        let crack = bullet_passing(5, 0.8, true);
        let whizz = bullet_passing(5, 0.8, false);
        assert!(crack.len() < whizz.len());
        let start = RATE as usize / 200;
        let sharpest_whizz = whizz.chunks(start).map(sharpness).fold(0.0_f32, f32::max);
        assert!(sharpness(&crack[..start]) > sharpest_whizz, "{} vs {sharpest_whizz}", sharpness(&crack[..start]));
        for sound in [&crack, &whizz] {
            let peak = sound.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
            assert!(peak > 0.2 && peak <= 1.0, "{peak}");
        }
        // Most of a crack is over almost at once.
        let energy = |s: &[f32]| s.iter().map(|v| v * v).sum::<f32>();
        assert!(energy(&crack[..RATE as usize / 100]) > energy(&crack) * 0.7);
    }

    #[test]
    fn shots_are_loud_but_not_flat_topped() {
        for seed in 0..40 {
            let peak = rifle_shot(seed * 977, 1.0).iter().fold(0.0_f32, |p, s| p.max(s.abs()));
            assert!(peak > 0.6, "too quiet: {peak}");
            assert!(peak < 1.0, "clipping");
        }
    }

    /// Loudness over time, in 20 ms steps.
    fn envelope(samples: &[f32]) -> Vec<f32> {
        samples.chunks(RATE as usize / 50).map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt()).collect()
    }

    /// A separate echo shows up as the sound getting louder again after it has started to fade.
    #[test]
    fn a_shot_fades_without_separate_echoes() {
        for seed in 0..40 {
            let mut quietest = f32::MAX;
            for (i, level) in envelope(&rifle_shot(seed * 977, 1.0)).into_iter().enumerate() {
                assert!(level < quietest.max(0.001) * 1.6, "seed {seed}: louder again at {} ms: {level} after {quietest}", i * 20);
                quietest = quietest.min(level);
            }
        }
    }

    /// How fast a sound changes from one sample to the next, which is mostly its high frequencies.
    fn sharpness(samples: &[f32]) -> f32 {
        samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum()
    }

    #[test]
    fn no_two_shots_are_alike() {
        assert_ne!(rifle_shot(1, 1.0), rifle_shot(2, 1.0));
    }

    #[test]
    fn a_shot_from_behind_the_barrel_is_duller() {
        assert!(sharpness(&rifle_shot(7, 1.0)) > sharpness(&rifle_shot(7, 0.0)) * 1.5);
    }

    #[test]
    fn muffling_takes_the_edge_off() {
        let shot = rifle_shot(7, 1.0);
        let through_wall = muffled(&shot, 600.0);
        let sealed = muffled(&shot, 260.0);
        assert_eq!(through_wall.len(), shot.len());
        assert!(sharpness(&through_wall) < sharpness(&shot) * 0.3);
        assert!(sharpness(&sealed) < sharpness(&through_wall));
    }
}

#[cfg(test)]
mod vehicle_sound_tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn an_engine_loop_joins_up_without_a_click_and_is_louder_than_nothing() {
        for brightness in [0.0, 1.0] {
            let looped = engine_loop(7, 1800.0, 4.0, brightness);
            assert_eq!(looped.len(), RATE as usize);
            assert!(rms(&looped) > 0.1, "{}", rms(&looped));
            let typical_step = looped.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / looped.len() as f32;
            let join = (looped[0] - looped[looped.len() - 1]).abs();
            assert!(join < (typical_step * 25.0).max(0.25), "the loop point jumps by {join} where a normal step is {typical_step}");
        }
    }

    #[test]
    fn the_dull_copy_has_less_top_end_than_the_clear_one() {
        let top = |samples: &[f32]| rms(&samples.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(top(&engine_loop(7, 1800.0, 4.0, 1.0)) > top(&engine_loop(7, 1800.0, 4.0, 0.0)) * 1.5);
    }

    #[test]
    fn metal_rings_brighter_than_a_thud_and_a_blast_is_long_and_low() {
        let top = |samples: &[f32]| rms(&samples.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(samples).max(1e-3);
        let (metal, thud, boom) = (metal_hit(3), body_hit(3), blast(3));
        assert!(top(&metal) > top(&thud) * 1.25, "{} {}", top(&metal), top(&thud));
        assert!(boom.len() > metal.len() * 5);
        assert!(top(&boom) < top(&metal));
    }
}

#[cfg(test)]
mod rotor_tests {
    use super::*;

    #[test]
    fn the_rotor_loops_go_round_without_a_click() {
        for sound in [rotor_thump(), turbine_whine(), rotor_thump_of(LONG_THUMPS), turbine_whine_of(LONG_WHINE)] {
            let step = |a: f32, b: f32| (a - b).abs();
            let seam = step(sound[0], sound[sound.len() - 1]);
            // Nothing at the join jumps further than the sound moves from sample to sample anyway.
            let biggest = sound.windows(2).map(|w| step(w[0], w[1])).fold(0.0, f32::max);
            assert!(seam <= biggest, "{seam} at the seam against {biggest}");
            assert!(sound.iter().all(|s| s.abs() <= 1.0));
        }
    }

    /// Noise that comes round again within a second or two is heard to, so the New set's loops are longer.
    #[test]
    fn the_long_loops_do_not_repeat_within_five_seconds_and_keep_the_short_ones_as_they_were() {
        for sound in [rotor_thump_of(LONG_THUMPS), turbine_whine_of(LONG_WHINE)] {
            assert!(sound.len() >= RATE as usize * 5, "{}", sound.len());
        }
        let long = rotor_thump_of(LONG_THUMPS);
        let period = (RATE as f32 / THUMP_HZ) as usize;
        // A beat a whole loop of the short version further on is not the same noise.
        let (a, b) = (&long[period * 2..period * 3], &long[period * 14..period * 15]);
        assert!(a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() > 1.0);
        assert_eq!(rotor_thump().len(), 49_000);
        assert_eq!(turbine_whine().len(), RATE as usize);
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        assert!((rms(&long) / rms(&rotor_thump()) - 1.0).abs() < 0.1, "the long one is as loud");
        assert!((rms(&rotor_thump()) - 0.303223).abs() < 1e-3 && (rms(&turbine_whine()) - 0.168059).abs() < 1e-3);
    }

    #[test]
    fn the_beat_comes_at_the_rate_of_the_blades() {
        let thump = rotor_thump();
        let period = (RATE as f32 / THUMP_HZ) as usize;
        let energy = |from: usize, to: usize| thump[from..to].iter().map(|s| s * s).sum::<f32>();
        let (early, late) = (energy(period * 2, period * 2 + period / 4), energy(period * 2 + period * 3 / 4, period * 3));
        assert!(early > late * 2.0, "{early} right after a blade and {late} before the next");
    }
}
