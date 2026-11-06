//! A limiter on everything one-off that plays at once, so a firefight full of far shots landing together
//! cannot add up past full scale. Bevy mixes each sound on its own and offers no master bus to put an
//! effect on, but every one-off sound here is rendered before it plays, so how loud the mix is going to
//! be is known ahead: each new sound is turned down by as much as it would push the mix over the top.
//! A sound on its own below full scale is never touched.

use std::collections::VecDeque;

use bevy::prelude::*;

/// How the mix is held under full scale, for a mod to change.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Limiter {
    /// Where it starts to hold the mix down, in dB of full scale.
    pub threshold_db: f32,
    /// How much less what goes past the threshold comes out: 6 is 6 dB over in, 1 dB over out.
    pub ratio: f32,
    /// The mix is never let past this, in dB of full scale.
    pub ceiling_db: f32,
    /// The most it ever turns one sound down, in dB, so that a far shot in a loud moment is still there.
    pub most_db: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self { threshold_db: -3.0, ratio: 6.0, ceiling_db: -0.3, most_db: 15.0 }
    }
}

/// The mix to come of the sounds already started, sample by sample, as one channel.
#[derive(Resource, Default)]
pub struct Bus {
    /// Seconds at its first sample.
    since: f64,
    ahead: VecDeque<f32>,
}

fn loudest(mix: &VecDeque<f32>, offset: usize, samples: &[f32], volume: f32) -> f32 {
    samples.iter().enumerate().map(|(i, s)| (mix.get(offset + i).copied().unwrap_or(0.0) + s * volume).abs()).fold(0.0, f32::max)
}

impl Bus {
    /// How loud to play `samples`, which start at `now` seconds at `volume`: `volume` itself unless the
    /// mix would go over the threshold, and less by the limiter's law if it would. A sound no louder than
    /// it is on its own is left alone, as it is what adds up that is held down.
    pub fn admit(&mut self, cfg: &Limiter, now: f64, samples: &[f32], volume: f32) -> f32 {
        let offset = self.catch_up(now);

        let own = samples.iter().fold(0.0_f32, |most, s| most.max(s.abs())) * volume;
        let (threshold, ceiling) = (10f32.powf(cfg.threshold_db / 20.0), 10f32.powf(cfg.ceiling_db / 20.0));
        let together = loudest(&self.ahead, offset, samples, volume);
        let mut gain = 1.0;
        if together > threshold.max(own) && together > 0.0 {
            let over = 20.0 * (together / threshold).log10();
            let wanted = (threshold * 10f32.powf(over / cfg.ratio.max(1.0) / 20.0)).min(ceiling).max(own.min(threshold));
            let least = 10f32.powf(-cfg.most_db / 20.0);
            // The peak of the sum is near enough in proportion to this sound's share of it.
            for _ in 0..4 {
                let now_loudest = loudest(&self.ahead, offset, samples, volume * gain);
                if now_loudest <= wanted * 1.01 {
                    break;
                }
                gain = (gain * wanted / now_loudest).clamp(least, 1.0);
            }
        }
        self.mix_in(offset, samples, volume * gain);
        volume * gain
    }

    /// A sound that plays as loud as it is, your own shot, which the sounds after it make room for.
    pub fn add(&mut self, now: f64, samples: &[f32], volume: f32) {
        let offset = self.catch_up(now);
        self.mix_in(offset, samples, volume);
    }

    /// Forgets what has played, and gives where `now` is in what is left.
    fn catch_up(&mut self, now: f64) -> usize {
        let rate = prs_synth::RATE as f64;
        let gone = (((now - self.since) * rate).floor().max(0.0) as usize).min(self.ahead.len());
        self.ahead.drain(..gone);
        self.since = if self.ahead.is_empty() { now } else { self.since + gone as f64 / rate };
        ((now - self.since) * rate).floor().max(0.0) as usize
    }

    fn mix_in(&mut self, offset: usize, samples: &[f32], volume: f32) {
        if self.ahead.len() < offset + samples.len() {
            self.ahead.resize(offset + samples.len(), 0.0);
        }
        for (i, s) in samples.iter().enumerate() {
            self.ahead[offset + i] += s * volume;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hearing::{Acoustics, Carry};
    use crate::synth;
    use prs_runtime::prs_sim::weapons::WeaponConfig;
    use prs_synth::{Noise, RATE};

    #[test]
    fn a_sound_on_its_own_is_never_touched() {
        let mut bus = Bus::default();
        let shot = synth::gun_shot(7, 1.0, &WeaponConfig::assault_rifle().report);
        assert_eq!(bus.admit(&Limiter::default(), 0.0, &shot, 1.0), 1.0);
        // Nor one after it has finished.
        assert_eq!(bus.admit(&Limiter::default(), 2.0, &shot, 1.0), 1.0);
    }

    #[test]
    fn two_loud_sounds_together_are_held_down_but_never_lost() {
        let cfg = Limiter::default();
        let mut bus = Bus::default();
        let shot = synth::gun_shot(7, 1.0, &WeaponConfig::assault_rifle().report);
        assert_eq!(bus.admit(&cfg, 0.0, &shot, 1.0), 1.0);
        let second = bus.admit(&cfg, 0.01, &shot, 1.0);
        assert!(second < 1.0 && second >= 10f32.powf(-cfg.most_db / 20.0) - 1e-6, "{second}");
    }

    /// The firefight: `shooters` rifles somewhere from 50 to 600 m off on open ground, each firing now and
    /// then for `seconds`, mixed as rodio would, with or without the limiter. The loudest sample and how
    /// many went past full scale.
    fn firefight(shooters: usize, seconds: f32, limited: bool) -> (f32, usize) {
        firefight_with(shooters, seconds, limited, Acoustics::default().shot)
    }

    fn firefight_with(shooters: usize, seconds: f32, limited: bool, carry: Carry) -> (f32, usize) {
        let a = Acoustics::default();
        let report = WeaponConfig::assault_rifle().report;
        let mut noise = Noise(0xF1F1);
        let mut mix = vec![0.0_f32; ((seconds + 1.5) * RATE as f32) as usize];
        let mut shots: Vec<(f32, f32, f32)> = Vec::new();
        for _ in 0..shooters {
            let distance = 50.0 + (noise.next() + 1.0) * 0.5 * 550.0;
            let brightness = (noise.next() + 1.0) * 0.5;
            let mut t = (noise.next() + 1.0) * 0.5;
            while t < seconds {
                shots.push((t + distance / a.speed_of_sound, distance, brightness));
                // Bursts of three, then a pause.
                t += if (noise.next() + 1.0) * 0.5 < 0.66 { 0.09 } else { 0.6 + (noise.next() + 1.0) * 0.6 };
            }
        }
        shots.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut bus = Bus::default();
        for (i, (at, distance, brightness)) in shots.into_iter().enumerate() {
            let samples = synth::gun_shot(i as u32 * 977 + 1, brightness, &report);
            let volume = carry.volume(distance) * a.shot_loudness(brightness);
            let volume = if limited { bus.admit(&a.limiter, at as f64, &samples, volume) } else { volume };
            let start = (at * RATE as f32) as usize;
            for (j, s) in samples.iter().enumerate() {
                if let Some(slot) = mix.get_mut(start + j) {
                    *slot += s * volume;
                }
            }
        }
        (mix.iter().fold(0.0_f32, |most, s| most.max(s.abs())), mix.iter().filter(|s| s.abs() > 1.0).count())
    }

    /// Your own shot in the middle of it all is as loud as ever, and what comes after makes room.
    #[test]
    fn your_own_shot_is_never_turned_down() {
        let cfg = Limiter::default();
        let mut bus = Bus::default();
        let shot = synth::gun_shot(7, 1.0, &WeaponConfig::assault_rifle().report);
        bus.add(0.0, &shot, 1.0);
        bus.add(0.002, &shot, 1.0);
        assert!(bus.admit(&cfg, 0.004, &shot, 0.3) < 0.3);
    }

    #[test]
    fn a_busy_firefight_does_not_go_past_full_scale() {
        let (peak, over) = firefight(30, 10.0, true);
        assert!(over == 0 && peak <= 1.0, "peak {peak}, {over} samples over");
    }

    #[test]
    #[ignore = "prints the loudest a busy firefight gets, with and without the limiter"]
    fn firefight_report() {
        for shooters in [10, 30, 60] {
            let (old, over_old) = firefight_with(shooters, 10.0, false, Carry::new(1.0, 10.0));
            let (before, over_before) = firefight(shooters, 10.0, false);
            let (after, over_after) = firefight(shooters, 10.0, true);
            println!(
                "{shooters:3} shooters at 50 to 600 m for 10 s: old fade {:5.1} dBFS, {over_old} over | new fade, no limiter {:5.1} dBFS, {over_before} over | with limiter {:5.1} dBFS, {over_after} over",
                20.0 * old.log10(),
                20.0 * before.log10(),
                20.0 * after.log10()
            );
        }
    }
}
