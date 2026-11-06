//! The New sound set: one model for everything that touches something else. An object is a few decaying
//! modes. A touch is a soft-shouldered push on them, and a rub is many tiny random touches, so a magazine
//! seating and a sleeve dragging along a wall come from the same sums and no two are alike.

use serde::{Deserialize, Serialize};

use crate::{LowPass, Noise, RATE};

/// Which sounds play: today's or the ones built on [`ContactSound`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SoundSet {
    #[default]
    New,
    Current,
}

/// How a step differs from the one before, each as a share either way of the tuned value at its centre.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct StepVary {
    pub scuff_hz: f32,
    pub scuff_rate: f32,
    pub scuff_top_hz: f32,
}

impl Default for StepVary {
    fn default() -> Self {
        Self { scuff_hz: 0.15, scuff_rate: 0.15, scuff_top_hz: 0.15 }
    }
}

/// One thing touching another. Nothing here is a sample: every field is a sum, and every event draws its
/// own modes, touches and grains from its seed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ContactSound {
    /// How loud it plays, or for something heard from afar, how much of its carry it uses.
    pub volume: f32,
    pub seconds: f32,
    /// How much one differs from the next, as a share of each thing that can differ.
    pub vary: f32,

    /// The object: how many modes it has, from 1 to 8, the lowest, and how far each is above the last.
    pub modes: u32,
    pub lowest_hz: f32,
    pub spacing: f32,
    /// How fast the lowest mode dies, per second, and how much faster the higher ones go, 1 being in step
    /// with their pitch. Damping is most of what tells metal from wood.
    pub damping: f32,
    pub damping_rise: f32,
    /// How much each mode is of the one below it.
    pub top_tilt: f32,

    /// The touch: its level against the rub, 0 for none, and how many landings it takes.
    pub touch_level: f32,
    pub touches: u32,
    /// When it comes, as a share of the length.
    pub touch_at: f32,
    /// How long the contact lasts in milliseconds. Longer is softer, because it stops pushing the high modes.
    pub touch_ms: f32,
    /// Seconds to the first bounce. Each gap is shorter than the last.
    pub touch_gap: f32,

    /// The rub: its level against the touch, 0 for none, and how much of the length it takes from the start.
    pub rub_level: f32,
    pub rub_share: f32,
    /// How much of the rub it takes to reach full speed.
    pub swell: f32,
    /// Tiny touches a second at full speed, how long each lasts in milliseconds, and how unequal they are:
    /// 0 is even, which is a rub, and 1 is a few hard ones among many soft, which is a scratch.
    pub density: f32,
    pub grain_ms: f32,
    pub rough: f32,
    /// How broad each mode is under a rub, as a share of its pitch. Narrow is pitched and wide is airy.
    pub width: f32,
    /// How far the modes rise with speed, as a share of their pitch.
    pub sweep: f32,
    /// How much it catches and lets go, from 0 to 1, and how fast, per second.
    pub drag: f32,
    pub drag_hz: f32,

    /// What is below this is taken out, which keeps it from being a thump.
    pub bottom_hz: f32,
}

/// What a body does with its hands and kit. The same things in both sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    Stance,
    Climb,
    SightsUp,
    SightsDown,
    GunOnWall,
    GunChange,
    Handling,
    Crawl,
    MagOut,
    MagIn,
    Gear,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ContactSounds {
    pub stance: ContactSound,
    pub climb: ContactSound,
    pub sights_up: ContactSound,
    pub sights_down: ContactSound,
    pub gun_on_wall: ContactSound,
    pub gun_change: ContactSound,
    pub handling: ContactSound,
    pub crawl: ContactSound,
    pub mag_out: ContactSound,
    pub mag_in: ContactSound,
    pub gear: ContactSound,
    /// Working the charging handle after an empty magazine: back, then forward softly. Only the New set has it.
    pub charge: ContactSound,
    pub selector: ContactSound,
    pub dry: ContactSound,
    pub zip: ContactSound,
    pub tear: ContactSound,
    pub metal_hit: ContactSound,
    pub clink: ContactSound,
    /// The metal part of a vehicle crash. The thud under it is the same in both sets.
    pub crash_metal: ContactSound,
    pub impact_dirt: ContactSound,
    pub impact_grass: ContactSound,
    pub impact_gravel: ContactSound,
    pub impact_wood: ContactSound,
    pub impact_concrete: ContactSound,
    pub impact_metal: ContactSound,
}

impl ContactSounds {
    pub fn of(&self, what: Moved) -> &ContactSound {
        match what {
            Moved::Stance => &self.stance,
            Moved::Climb => &self.climb,
            Moved::SightsUp => &self.sights_up,
            Moved::SightsDown => &self.sights_down,
            Moved::GunOnWall => &self.gun_on_wall,
            Moved::GunChange => &self.gun_change,
            Moved::Handling => &self.handling,
            Moved::Crawl => &self.crawl,
            Moved::MagOut => &self.mag_out,
            Moved::MagIn => &self.mag_in,
            Moved::Gear => &self.gear,
        }
    }
}

impl Default for ContactSounds {
    /// Each `volume` is set so that it plays about as loud as the Current sound it stands in for, which
    /// `each_new_sound_is_about_as_loud_as_the_current_one` holds to.
    fn default() -> Self {
        let cloth = ContactSound {
            volume: 0.026,
            seconds: 0.3,
            vary: 0.15,
            modes: 4,
            lowest_hz: 360.0,
            spacing: 1.6,
            damping: 120.0,
            damping_rise: 0.8,
            top_tilt: 0.7,
            touch_level: 0.0,
            touches: 0,
            touch_at: 0.1,
            touch_ms: 3.0,
            touch_gap: 0.012,
            rub_level: 1.0,
            rub_share: 1.0,
            swell: 0.35,
            density: 2200.0,
            grain_ms: 1.5,
            rough: 0.2,
            width: 1.0,
            sweep: 0.35,
            drag: 0.35,
            drag_hz: 30.0,
            bottom_hz: 240.0,
        };
        let steel = ContactSound { modes: 5, lowest_hz: 620.0, spacing: 1.7, damping: 170.0, damping_rise: 0.7, top_tilt: 0.6, width: 0.6, rough: 0.35, bottom_hz: 320.0, ..cloth };
        let hit = ContactSound { seconds: 0.2, swell: 0.1, rub_share: 0.3, density: 7000.0, grain_ms: 0.8, rough: 0.5, drag: 0.0, sweep: 0.0, bottom_hz: 100.0, ..cloth };
        Self {
            stance: ContactSound { volume: 0.0373, ..cloth },
            climb: ContactSound { volume: 0.0406, seconds: 0.5, swell: 0.45, density: 1700.0, rough: 0.3, drag: 0.5, drag_hz: 22.0, ..cloth },
            sights_up: ContactSound { volume: 0.0287, seconds: 0.16, lowest_hz: 520.0, sweep: 0.5, rough: 0.12, bottom_hz: 420.0, ..cloth },
            sights_down: ContactSound { volume: 0.0282, seconds: 0.15, lowest_hz: 480.0, sweep: 0.3, rough: 0.12, bottom_hz: 420.0, ..cloth },
            gun_on_wall: ContactSound { volume: 0.0404, seconds: 0.26, swell: 0.25, density: 900.0, rough: 0.6, drag: 0.65, drag_hz: 60.0, touch_level: 0.3, touches: 1, touch_at: 0.04, touch_ms: 3.0, ..steel },
            gun_change: ContactSound { volume: 0.0378, seconds: 0.32, lowest_hz: 440.0, density: 1800.0, rough: 0.3, drag: 0.4, drag_hz: 40.0, ..cloth },
            handling: ContactSound { volume: 0.0359, seconds: 0.22, lowest_hz: 400.0, ..cloth },
            crawl: ContactSound { volume: 0.0331, seconds: 0.8, vary: 0.4, swell: 0.45, lowest_hz: 300.0, density: 1500.0, rough: 0.35, sweep: 0.25, drag: 0.55, drag_hz: 20.0, bottom_hz: 200.0, ..cloth },
            // The latch letting go, then the magazine sliding out.
            mag_out: ContactSound { volume: 0.0767, seconds: 0.28, swell: 0.55, density: 1400.0, touch_level: 0.3, touches: 1, touch_at: 0.08, touch_ms: 3.0, ..steel },
            // It slides in and seats softly: a long contact, so it has little top end.
            mag_in: ContactSound { volume: 0.0819, seconds: 0.26, rub_share: 0.68, swell: 0.7, density: 1500.0, touch_level: 0.45, touches: 2, touch_at: 0.68, touch_ms: 4.5, touch_gap: 0.009, ..steel },
            gear: ContactSound { volume: 0.0232, seconds: 0.26, vary: 0.4, swell: 0.4, lowest_hz: 480.0, density: 1500.0, rough: 0.45, drag: 0.55, drag_hz: 38.0, bottom_hz: 400.0, ..cloth },
            // Drawn back along the receiver, then let forward to seat softly, about as loud as a magazine.
            charge: ContactSound { volume: 0.09, seconds: 0.32, rub_share: 0.62, swell: 0.4, density: 1300.0, touch_level: 0.4, touches: 2, touch_at: 0.64, touch_ms: 3.5, touch_gap: 0.01, ..steel },
            selector: ContactSound { volume: 0.27, seconds: 0.1, rub_level: 0.0, touch_level: 1.0, touches: 2, touch_at: 0.08, touch_ms: 1.8, touch_gap: 0.014, modes: 4, lowest_hz: 950.0, damping: 240.0, ..steel },
            dry: ContactSound { volume: 0.3212, seconds: 0.08, rub_level: 0.0, touch_level: 1.0, touches: 1, touch_at: 0.1, touch_ms: 1.6, modes: 4, lowest_hz: 800.0, damping: 300.0, ..steel },
            zip: ContactSound { volume: 0.1373, seconds: 0.34, swell: 0.5, lowest_hz: 1300.0, spacing: 1.5, width: 0.7, density: 2600.0, grain_ms: 0.7, rough: 0.5, drag: 0.5, drag_hz: 70.0, bottom_hz: 700.0, ..cloth },
            tear: ContactSound { volume: 0.3821, seconds: 0.5, swell: 0.12, lowest_hz: 1100.0, spacing: 1.6, density: 800.0, grain_ms: 1.0, rough: 0.9, drag: 0.8, drag_hz: 60.0, bottom_hz: 500.0, ..cloth },
            metal_hit: ContactSound { volume: 0.6928, seconds: 0.25, modes: 6, lowest_hz: 1000.0, spacing: 1.55, damping: 30.0, damping_rise: 0.7, top_tilt: 0.75, touch_level: 1.0, touches: 1, touch_at: 0.01, touch_ms: 0.4, rub_level: 0.3, rub_share: 0.12, swell: 0.1, density: 9000.0, grain_ms: 0.5, rough: 0.6, drag: 0.0, sweep: 0.0, width: 1.0, bottom_hz: 200.0, ..cloth },
            clink: ContactSound { volume: 0.8261, seconds: 0.25, modes: 5, lowest_hz: 1500.0, spacing: 1.6, damping: 70.0, damping_rise: 0.7, top_tilt: 0.65, touch_level: 1.0, touches: 2, touch_at: 0.02, touch_ms: 0.8, touch_gap: 0.04, rub_level: 0.0, bottom_hz: 300.0, ..cloth },
            crash_metal: ContactSound { volume: 1.0, seconds: 0.65, modes: 8, lowest_hz: 240.0, spacing: 1.42, damping: 9.0, damping_rise: 0.6, top_tilt: 0.85, touch_level: 1.0, touches: 5, touch_at: 0.02, touch_ms: 2.0, touch_gap: 0.09, rub_level: 0.8, swell: 0.1, density: 1100.0, grain_ms: 2.0, rough: 0.9, width: 0.45, sweep: 0.1, drag: 0.7, drag_hz: 24.0, bottom_hz: 180.0, ..cloth },
            impact_dirt: ContactSound { volume: 0.6078, seconds: 0.16, lowest_hz: 260.0, ..hit },
            impact_grass: ContactSound { volume: 0.6862, seconds: 0.12, lowest_hz: 220.0, top_tilt: 0.5, ..hit },
            impact_gravel: ContactSound { volume: 0.5669, seconds: 0.22, lowest_hz: 520.0, density: 2500.0, rough: 0.95, ..hit },
            impact_wood: ContactSound { volume: 0.635, seconds: 0.16, modes: 5, lowest_hz: 430.0, spacing: 1.7, damping: 110.0, damping_rise: 0.9, touch_level: 1.0, touches: 1, touch_at: 0.02, touch_ms: 0.6, rub_level: 0.35, rub_share: 0.2, ..hit },
            impact_concrete: ContactSound { volume: 0.5377, seconds: 0.14, modes: 5, lowest_hz: 850.0, damping: 320.0, damping_rise: 0.9, touch_level: 0.8, touches: 1, touch_at: 0.02, touch_ms: 0.35, rub_level: 0.8, rub_share: 0.55, rough: 0.8, ..hit },
            impact_metal: ContactSound { volume: 0.4861, seconds: 0.25, modes: 6, lowest_hz: 1000.0, spacing: 1.55, damping: 30.0, damping_rise: 0.7, top_tilt: 0.75, touch_level: 1.0, touches: 1, touch_at: 0.01, touch_ms: 0.4, rub_level: 0.3, rub_share: 0.12, swell: 0.1, density: 9000.0, grain_ms: 0.5, rough: 0.6, bottom_hz: 200.0, ..hit },
        }
    }
}

/// A step with its grit a little different from the last, around the tuned values. The rest is [`crate::footstep`].
pub fn varied_footstep(seed: u32, sound: &crate::StepSound, vary: &StepVary) -> Vec<f32> {
    let mut noise = Noise(seed.rotate_left(11) | 1);
    for _ in 0..8 {
        noise.next();
    }
    let mut around = |share: f32| 1.0 + noise.next() * share;
    let varied = crate::StepSound {
        scuff_hz: sound.scuff_hz * around(vary.scuff_hz),
        scuff_rate: sound.scuff_rate * around(vary.scuff_rate),
        scuff_top_hz: sound.scuff_top_hz * around(vary.scuff_top_hz),
        ..*sound
    };
    crate::footstep(seed, &varied)
}

/// One decaying mode. Two samples of memory, and a unit push rings at unit height whatever its pitch.
#[derive(Default)]
struct Mode {
    pull: f32,
    settle: f32,
    gain: f32,
    last: f32,
    before: f32,
}

impl Mode {
    fn tune(&mut self, hz: f32, decay: f32) {
        let radius = (-decay / RATE as f32).exp();
        let turn = std::f32::consts::TAU * hz.min(RATE as f32 * 0.45) / RATE as f32;
        self.pull = 2.0 * radius * turn.cos();
        self.settle = -radius * radius;
        self.gain = turn.sin();
    }

    fn step(&mut self, push: f32) -> f32 {
        let now = self.gain * push + self.pull * self.last + self.settle * self.before;
        self.before = self.last;
        self.last = now;
        now
    }
}

/// How long to wait before anything starts, so that even the shortest touch has silence to rise out of.
const LEAD_SECONDS: f32 = 0.003;
/// Samples between moving a rub's modes with its speed, about 1.5 ms.
const SWEEP_INTERVAL: usize = 64;
/// Loudness over this many seconds is what a sound is held to, since a single sample says little about a grain.
const LOUDNESS_WINDOW: f32 = 0.01;
/// How loud the loudest moment of any contact is before its `volume`, safely short of the limiter's ceiling.
const LOUDEST: f32 = 0.55;
const MOST_MODES: usize = 8;

fn loudest_moment(samples: &[f32]) -> f32 {
    let window = ((LOUDNESS_WINDOW * RATE as f32) as usize).clamp(1, samples.len().max(1));
    let (mut sum, mut most) = (0.0_f32, 0.0_f32);
    for i in 0..samples.len() {
        sum += samples[i] * samples[i];
        if i >= window {
            sum -= samples[i - window] * samples[i - window];
        }
        most = most.max(sum);
    }
    (most.max(0.0) / window as f32).sqrt()
}

/// Up to the same loudness whatever was done to it, so that `volume` means what it says.
fn held_to(samples: &mut [f32], level: f32) {
    let loudest = loudest_moment(samples);
    if loudest > 1e-9 {
        for sample in samples.iter_mut() {
            *sample *= level / loudest;
        }
    }
}

/// The first thing [`contact`] draws for a seed, so that when its touch lands can be known before it plays.
fn length_and_noise(seed: u32, sound: &ContactSound) -> (f32, Noise) {
    let mut noise = Noise(seed | 1);
    for _ in 0..8 {
        noise.next();
    }
    let seconds = (sound.seconds * (1.0 + noise.next() * sound.vary.clamp(0.0, 1.0))).max(0.02);
    (seconds, noise)
}

/// Seconds into this seed's render that its first touch lands, if it has one, so that it can be started
/// early enough to land on time.
pub fn first_touch_seconds(seed: u32, sound: &ContactSound) -> Option<f32> {
    (sound.touch_level > 0.0 && sound.touches > 0).then(|| LEAD_SECONDS + sound.touch_at.clamp(0.0, 1.0) * length_and_noise(seed, sound).0)
}

/// One touch or rub, rendered per event so that no two are the same.
pub fn contact(seed: u32, sound: &ContactSound) -> Vec<f32> {
    let (seconds, mut noise) = length_and_noise(seed, sound);
    let share = sound.vary.clamp(0.0, 1.0);
    let mut vary = |by: f32| 1.0 + noise.next() * by * share;
    let lowest = sound.lowest_hz.max(20.0) * vary(0.8);
    let level = vary(0.6);
    let density = sound.density * vary(1.0);
    let swell = (sound.swell * vary(1.0)).clamp(0.02, 0.98);
    let sweep = sound.sweep * vary(1.0);
    let count = sound.modes.clamp(1, MOST_MODES as u32) as usize;
    let mut pitches = [0.0; MOST_MODES];
    let mut heights = [0.0; MOST_MODES];
    for i in 0..count {
        // Each mode is a little off where it should be, and where the touch lands sets how hard each is pushed.
        pitches[i] = lowest * sound.spacing.max(1.01).powi(i as i32) * vary(0.25);
        heights[i] = sound.top_tilt.powi(i as i32) * vary(1.5).max(0.1);
    }
    let length = (seconds * RATE as f32) as usize;

    let mut rubbed = vec![0.0; length];
    if sound.rub_level > 0.0 {
        rub(&mut rubbed, &mut noise, sound, &pitches[..count], &heights[..count], density, swell, sweep);
        held_to(&mut rubbed, 1.0);
    }
    let mut touched = vec![0.0; length];
    if sound.touch_level > 0.0 && sound.touches > 0 {
        touch(&mut touched, &mut noise, sound, seconds, &pitches[..count], &heights[..count]);
        held_to(&mut touched, 1.0);
    }

    let mut out: Vec<f32> = rubbed.iter().zip(&touched).map(|(r, t)| r * sound.rub_level + t * sound.touch_level).collect();
    held_to(&mut out, LOUDEST * level);
    let (mut bottom, bottom_step) = (LowPass::default(), LowPass::step_size(sound.bottom_hz.max(1.0)));
    // Half a cosine out, for whatever is still ringing when the length runs out.
    let fade = (length as f32 * 0.2).max(1.0);
    for (i, sample) in out.iter_mut().enumerate() {
        let left = (length - i) as f32;
        let shape = if left < fade { 0.5 - 0.5 * (std::f32::consts::PI * left / fade).cos() } else { 1.0 };
        let cut = (*sample - bottom.step_by(*sample, bottom_step)) * shape;
        *sample = cut / (1.0 + cut * cut).sqrt();
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn rub(out: &mut [f32], noise: &mut Noise, sound: &ContactSound, pitches: &[f32], heights: &[f32], density: f32, swell: f32, sweep: f32) {
    let span = (out.len() as f32 * sound.rub_share.clamp(0.02, 1.0)).max(2.0);
    let swell_at = span * swell;
    let mut bank: Vec<Mode> = pitches.iter().map(|_| Mode::default()).collect();
    let grain_fade = (-1000.0 / (sound.grain_ms.max(0.05) * RATE as f32)).exp();
    let (mut grain, mut catches) = (0.0_f32, LowPass::default());
    let catch_step = LowPass::step_size(sound.drag_hz.max(0.5));
    let (drag, rough) = (sound.drag.clamp(0.0, 1.0), sound.rough.clamp(0.0, 1.0));
    let width = sound.width.max(0.02);
    for (i, sample) in out.iter_mut().enumerate() {
        let at = i as f32;
        // Half a cosine each way, the speed of the hand, which starts and stops with no corner.
        let speed = if at >= span {
            0.0
        } else if at < swell_at {
            0.5 - 0.5 * (std::f32::consts::PI * at / swell_at).cos()
        } else {
            0.5 + 0.5 * (std::f32::consts::PI * (at - swell_at) / (span - swell_at)).cos()
        };
        if i % SWEEP_INTERVAL == 0 {
            for (mode, &pitch) in bank.iter_mut().zip(pitches) {
                let hz = pitch * (1.0 + sweep * (speed - 0.5));
                mode.tune(hz, std::f32::consts::PI * hz * width);
            }
        }
        let catching = (catches.step_by(noise.next(), catch_step) * 6.0).abs().min(1.0);
        let pressure = speed * (1.0 - drag * (1.0 - catching));
        if (noise.next() + 1.0) * 0.5 < density * pressure / RATE as f32 {
            let chance = (noise.next() + 1.0) * 0.5;
            // Even when smooth. The cube makes most of them small and a few large.
            let unequal = 1.0 + rough * (3.0 * chance * chance * chance - 1.0);
            grain += unequal * speed.max(0.05);
        }
        grain *= grain_fade;
        let push = noise.next() * grain;
        *sample = bank.iter_mut().zip(heights).map(|(mode, height)| mode.step(push) * height).sum();
    }
}

fn touch(out: &mut [f32], noise: &mut Noise, sound: &ContactSound, seconds: f32, pitches: &[f32], heights: &[f32]) {
    let mut bank: Vec<Mode> = pitches.iter().map(|_| Mode::default()).collect();
    let lowest = pitches[0];
    for (mode, &pitch) in bank.iter_mut().zip(pitches) {
        mode.tune(pitch, sound.damping.max(0.1) * (pitch / lowest).powf(sound.damping_rise));
    }
    let contact = ((sound.touch_ms.max(0.05) * 0.001 * RATE as f32) as usize).max(2);
    let mut starts = Vec::with_capacity(sound.touches as usize);
    let (mut at, mut gap, mut hardness) = (LEAD_SECONDS + sound.touch_at.clamp(0.0, 1.0) * seconds, sound.touch_gap.max(0.001), 1.0);
    for _ in 0..sound.touches {
        starts.push(((at * RATE as f32) as usize, hardness));
        at += gap * (1.0 + noise.next() * 0.2);
        gap *= 0.65 + noise.next() * 0.1;
        hardness *= 0.55 + noise.next() * 0.15;
    }
    for (i, sample) in out.iter_mut().enumerate() {
        let mut force = 0.0;
        for &(start, hardness) in &starts {
            if i >= start && i < start + contact {
                // The same push whatever its length, so longer only takes off the top.
                let along = (i - start) as f32 / contact as f32;
                force += hardness * (1.0 - (std::f32::consts::TAU * along).cos()) / contact as f32;
            }
        }
        *sample = bank.iter_mut().zip(heights).map(|(mode, height)| mode.step(force) * height).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_sound() -> Vec<(&'static str, ContactSound)> {
        let c = ContactSounds::default();
        vec![
            ("stance", c.stance), ("climb", c.climb), ("sights_up", c.sights_up), ("sights_down", c.sights_down), ("gun_on_wall", c.gun_on_wall),
            ("gun_change", c.gun_change), ("handling", c.handling), ("crawl", c.crawl), ("mag_out", c.mag_out), ("mag_in", c.mag_in), ("gear", c.gear), ("charge", c.charge),
            ("selector", c.selector), ("dry", c.dry), ("zip", c.zip), ("tear", c.tear), ("metal_hit", c.metal_hit), ("clink", c.clink), ("crash_metal", c.crash_metal),
            ("impact_dirt", c.impact_dirt), ("impact_grass", c.impact_grass), ("impact_gravel", c.impact_gravel), ("impact_wood", c.impact_wood),
            ("impact_concrete", c.impact_concrete), ("impact_metal", c.impact_metal),
        ]
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |most, s| most.max(s.abs()))
    }

    #[test]
    fn nothing_pins_the_output_or_goes_wrong_in_the_sums() {
        for (name, sound) in every_sound() {
            for seed in 0..30 {
                let samples = contact(seed * 131 + 1, &sound);
                assert!(samples.iter().all(|s| s.is_finite()), "{name}");
                let top = peak(&samples);
                assert!(top > 0.05 && top < 0.97, "{name} seed {seed}: peak {top}");
                let mean = samples.iter().sum::<f32>() / samples.len() as f32;
                assert!(mean.abs() < 0.03, "{name}: it sits off centre by {mean}");
            }
        }
    }

    #[test]
    fn every_contact_starts_and_ends_with_no_click() {
        for (name, sound) in every_sound() {
            for seed in 0..20 {
                let samples = contact(seed * 97 + 1, &sound);
                let top = peak(&samples);
                let edge = RATE as usize / 500;
                assert!(peak(&samples[..edge]) < top * 0.5, "{name} starts with a click");
                assert!(peak(&samples[samples.len() - edge..]) < top * 0.5, "{name} stops with a click");
                assert!(samples[0].abs() < 0.01 && samples[samples.len() - 1].abs() < 0.01, "{name} ends off zero");
            }
        }
    }

    #[test]
    fn no_two_are_alike_and_they_differ_in_more_than_their_noise() {
        for (name, sound) in every_sound() {
            let (a, b) = (contact(11, &sound), contact(29, &sound));
            assert_ne!(a, b, "{name}");
            // Their length changes too, and what is in them.
            let lengths: std::collections::BTreeSet<usize> = (0..20).map(|seed| contact(seed * 77 + 1, &sound).len()).collect();
            assert!(lengths.len() > 5, "{name}: {lengths:?}");
        }
    }

    /// Soft contact is what keeps it from being a tick: the same object, touched for longer, has less top end.
    #[test]
    fn a_longer_touch_has_less_top_end() {
        let top = |ms: f32| {
            let sound = ContactSound { rub_level: 0.0, touch_level: 1.0, touches: 1, vary: 0.0, touch_ms: ms, ..ContactSounds::default().selector };
            let samples = contact(5, &sound);
            let changes: f32 = samples.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
            changes / samples.iter().map(|s| s * s).sum::<f32>()
        };
        assert!(top(0.3) > top(6.0) * 1.3, "{} {}", top(0.3), top(6.0));
    }

    #[test]
    fn more_damping_dies_sooner() {
        let ring = |damping: f32| {
            let sound = ContactSound { rub_level: 0.0, touch_level: 1.0, touches: 1, vary: 0.0, seconds: 0.4, damping, ..ContactSounds::default().metal_hit };
            let samples = contact(5, &sound);
            let quarter = samples.len() / 4;
            samples[quarter..].iter().map(|s| s * s).sum::<f32>() / samples.iter().map(|s| s * s).sum::<f32>()
        };
        assert!(ring(20.0) > ring(200.0) * 3.0, "{} {}", ring(20.0), ring(200.0));
    }

    /// A rub is many small even touches and a scratch a few big ones among them, which shows as peaks.
    #[test]
    fn roughness_turns_a_rub_towards_a_scratch() {
        let spiky = |rough: f32| {
            let sound = ContactSound { rough, density: 500.0, vary: 0.0, drag: 0.0, ..ContactSounds::default().stance };
            (0..30)
                .map(|seed| {
                    let samples = contact(seed * 41 + 3, &sound);
                    peak(&samples) / loudest_moment(&samples)
                })
                .sum::<f32>()
        };
        assert!(spiky(1.0) > spiky(0.0), "{} {}", spiky(1.0), spiky(0.0));
    }

    /// The loudest 30 ms, averaged over a lot of them, at the volume it plays at, in dB.
    fn heard(render: impl Fn(u32) -> Vec<f32>, volume: f32) -> f32 {
        let window = RATE as usize * 3 / 100;
        let mut total = 0.0;
        for seed in 0..40 {
            let samples = render(seed * 977 + 13);
            let most = samples.windows(window.min(samples.len())).step_by(32).map(|w| w.iter().map(|s| s * s).sum::<f32>()).fold(0.0, f32::max);
            total += (most / window as f32).sqrt() * volume;
        }
        20.0 * (total / 40.0).log10()
    }

    fn pairs() -> Vec<(&'static str, f32, f32)> {
        use crate::*;
        let (new, old) = (ContactSounds::default(), SlideSounds::default());
        let mut out = Vec::new();
        for (name, what) in [("stance", Moved::Stance), ("climb", Moved::Climb), ("sights_up", Moved::SightsUp), ("sights_down", Moved::SightsDown), ("gun_on_wall", Moved::GunOnWall), ("gun_change", Moved::GunChange), ("handling", Moved::Handling), ("crawl", Moved::Crawl), ("mag_out", Moved::MagOut), ("mag_in", Moved::MagIn), ("gear", Moved::Gear)] {
            let (n, o) = (*new.of(what), *old.of(what));
            out.push((name, heard(|seed| slide(seed, &o), o.volume), heard(|seed| contact(seed, &n), n.volume)));
        }
        let new_one = |sound: ContactSound| heard(move |seed| contact(seed, &sound), sound.volume);
        out.push(("selector", heard(|_| current_selector(), CURRENT_SELECTOR_VOLUME), new_one(new.selector)));
        out.push(("zip", heard(|_| current_zip(), CURRENT_ZIP_VOLUME), new_one(new.zip)));
        out.push(("dry", heard(|_| dry_click(), CURRENT_DRY_VOLUME), new_one(new.dry)));
        out.push(("tear", heard(|_| tear(), CURRENT_TEAR_VOLUME), new_one(new.tear)));
        out.push(("metal_hit", heard(metal_hit, 1.0), new_one(new.metal_hit)));
        out.push(("clink", heard(metal_hit, 1.0), new_one(new.clink)));
        out.push(("impact_dirt", heard(impact, 1.0), new_one(new.impact_dirt)));
        out.push(("impact_grass", heard(impact, 1.0), new_one(new.impact_grass)));
        out.push(("impact_gravel", heard(impact, 1.0), new_one(new.impact_gravel)));
        out.push(("impact_wood", heard(impact, 1.0), new_one(new.impact_wood)));
        out.push(("impact_concrete", heard(impact, 1.0), new_one(new.impact_concrete)));
        out.push(("impact_metal", heard(impact, 1.0), new_one(new.impact_metal)));
        out.push(("crash", heard(crash, 1.0), heard(|seed| contact_crash(seed, &new.crash_metal), 1.0)));
        out
    }

    /// A sound can be started early so that its touch lands on a moment, which needs to know when it lands.
    #[test]
    fn the_first_touch_lands_when_it_is_said_to() {
        let sound = ContactSound { rub_level: 0.0, ..ContactSounds::default().mag_in };
        for seed in 0..30 {
            let samples = contact(seed * 53 + 7, &sound);
            let loudest = samples.iter().enumerate().fold((0, 0.0_f32), |most, (i, s)| if s.abs() > most.1 { (i, s.abs()) } else { most }).0;
            let said = first_touch_seconds(seed * 53 + 7, &sound).unwrap();
            let heard = loudest as f32 / RATE as f32;
            assert!(heard >= said && heard - said < 0.012, "seed {seed}: said {said}, loudest at {heard}");
        }
        assert_eq!(first_touch_seconds(1, &ContactSounds::default().stance), None);
    }

    #[test]
    fn charging_is_about_as_loud_as_a_magazine() {
        let c = ContactSounds::default();
        let (charge, mag_in) = (heard(|seed| contact(seed, &c.charge), c.charge.volume), heard(|seed| contact(seed, &c.mag_in), c.mag_in.volume));
        assert!((charge - mag_in).abs() < 2.0, "{charge:.1} against {mag_in:.1}");
    }

    /// The switch is meant to change what a sound is like and not how loud it is.
    #[test]
    fn each_new_sound_is_about_as_loud_as_the_current_one() {
        for (name, current, new) in pairs() {
            assert!((new - current).abs() < 2.0, "{name}: {new:.1} dB against {current:.1} dB");
        }
    }

    /// Equally loud, but a rub full of spikes would still seem harsher going from one set to the other.
    #[test]
    fn a_new_rub_is_not_much_spikier_than_the_current_one() {
        use crate::*;
        let (new, old) = (ContactSounds::default(), SlideSounds::default());
        let top = |render: &dyn Fn(u32) -> Vec<f32>, volume: f32| 20.0 * ((0..40).map(|seed| peak(&render(seed * 977 + 13))).sum::<f32>() / 40.0 * volume).log10();
        for (name, what) in [("stance", Moved::Stance), ("climb", Moved::Climb), ("sights_up", Moved::SightsUp), ("sights_down", Moved::SightsDown), ("gun_on_wall", Moved::GunOnWall), ("gun_change", Moved::GunChange), ("handling", Moved::Handling), ("crawl", Moved::Crawl), ("mag_out", Moved::MagOut), ("mag_in", Moved::MagIn), ("gear", Moved::Gear)] {
            let (n, o) = (*new.of(what), *old.of(what));
            let (current, new) = (top(&|seed| slide(seed, &o), o.volume), top(&|seed| contact(seed, &n), n.volume));
            assert!(new - current < 5.0, "{name}: peak {new:.1} dB against {current:.1} dB");
        }
    }

    #[test]
    #[ignore = "prints the level of each sound in both sets"]
    fn level_report() {
        for (name, current, new) in pairs() {
            println!("{name:16} current {current:6.1} new {new:6.1} diff {:5.1}  scale {:.3}", new - current, 10f32.powf((current - new) / 20.0));
        }
    }

    #[test]
    fn a_step_varies_around_the_tuned_values_and_not_away_from_them() {
        let sound = crate::StepSounds::default().dirt;
        let none = StepVary { scuff_hz: 0.0, scuff_rate: 0.0, scuff_top_hz: 0.0 };
        assert_eq!(varied_footstep(9, &sound, &none), crate::footstep(9, &sound));
        assert_ne!(varied_footstep(9, &sound, &StepVary::default()), crate::footstep(9, &sound));
        let tone = |samples: &[f32]| samples.iter().map(|s| s * s).sum::<f32>().sqrt();
        let (tuned, varied): (f32, f32) = (1..200u32).map(|seed| (tone(&crate::footstep(seed * 37, &sound)), tone(&varied_footstep(seed * 37, &sound, &StepVary::default())))).fold((0.0, 0.0), |sum, each| (sum.0 + each.0, sum.1 + each.1));
        assert!((varied / tuned - 1.0).abs() < 0.05, "{varied} around {tuned}");
    }
}
