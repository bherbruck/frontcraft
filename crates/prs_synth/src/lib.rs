//! Sound from nothing but sums, with no engine underneath, so the game and the tuner page at `/tune/`
//! make exactly the same noise from the same settings.

use serde::{Deserialize, Serialize};

mod contact;

pub use contact::{ContactSound, ContactSounds, Moved, SoundSet, StepVary, contact, first_touch_seconds, varied_footstep};

pub const RATE: u32 = 44_100;

pub struct Noise(pub u32);

impl Noise {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// One-pole low-pass, the basic tool for turning white noise into something with body.
#[derive(Default)]
pub struct LowPass(f32);

impl LowPass {
    pub fn step_size(cutoff_hz: f32) -> f32 {
        1.0 - (-std::f32::consts::TAU * cutoff_hz / RATE as f32).exp()
    }

    pub fn step(&mut self, input: f32, cutoff_hz: f32) -> f32 {
        self.step_by(input, Self::step_size(cutoff_hz))
    }

    pub fn step_by(&mut self, input: f32, step_size: f32) -> f32 {
        self.0 += (input - self.0) * step_size;
        self.0
    }
}

pub fn render(seconds: f32, mut sample: impl FnMut(f32) -> f32) -> Vec<f32> {
    let count = (seconds * RATE as f32) as usize;
    (0..count).map(|i| sample(i as f32 / RATE as f32)).collect()
}

/// Your footsteps: volume when barely moving, and how much a full sprint adds to it.
pub const STEP_VOLUME: (f32, f32) = (0.5, 1.25);

/// How a boot sounds on one kind of ground. Mostly weight, so the thud carries the level and the
/// rest stays well below it, or footsteps turn into hissing.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct StepSound {
    /// The heel landing: a falling low tone.
    pub thud_hz: f32,
    pub thud_level: f32,
    pub thud_rate: f32,
    /// How far above `thud_hz` the heel starts, as a share of it, and how fast it falls from there.
    pub thud_sweep: f32,
    pub thud_sweep_rate: f32,
    /// How much the heel's pitch differs from one step to the next, as a share of it.
    pub thud_vary: f32,
    /// Grit under the sole: filtered noise that follows the heel.
    pub scuff_hz: f32,
    pub scuff_level: f32,
    pub scuff_rate: f32,
    /// Where the top of the grit is rolled off.
    pub scuff_top_hz: f32,
    /// When the sole rolls down after the heel, in seconds.
    pub roll_delay: f32,
    /// Separate crunches, for loose ground. 0 for a smooth scuff.
    pub grains: u32,
    pub grain_rate: f32,
    /// What the floor itself does: boards booming or a plate ringing.
    pub ring_hz: f32,
    pub ring_level: f32,
    pub ring_rate: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct StepSounds {
    pub dirt: StepSound,
    pub grass: StepSound,
    pub gravel: StepSound,
    pub wood: StepSound,
    pub concrete: StepSound,
    pub metal: StepSound,
}

impl Default for StepSounds {
    /// Tuned by ear at `/tune/`: nearly all sole and hardly any heel.
    fn default() -> Self {
        let dirt = StepSound {
            thud_hz: 30.0,
            thud_level: 0.02,
            thud_rate: 15.0,
            thud_sweep: 0.18,
            thud_sweep_rate: 32.0,
            thud_vary: 0.09,
            scuff_hz: 100.0,
            scuff_level: 0.11,
            scuff_rate: 46.1539,
            scuff_top_hz: 4000.0,
            roll_delay: 0.079,
            grains: 0,
            grain_rate: 90.0,
            ring_hz: 0.0,
            ring_level: 0.0,
            ring_rate: 3.6144,
        };
        Self {
            dirt,
            grass: StepSound { thud_level: 0.0, thud_rate: 17.3084, scuff_hz: 145.1613, scuff_level: 0.0733, scuff_rate: 24.6159, roll_delay: 0.0518, ..dirt },
            gravel: StepSound { thud_level: 0.0, scuff_hz: 209.6774, scuff_level: 0.202, scuff_rate: 20.0, grains: 9, ..dirt },
            wood: StepSound { thud_hz: 34.8388, thud_level: 0.0, scuff_hz: 112.9032, scuff_level: 0.0513, ring_hz: 61.7424, ring_level: 0.0926, ring_rate: 86.7712, ..dirt },
            concrete: StepSound { thud_hz: 40.6448, thud_level: 0.0, thud_rate: 24.2309, scuff_hz: 161.2903, scuff_level: 0.1173, scuff_rate: 115.3841, roll_delay: 0.0874, ..dirt },
            metal: StepSound { thud_hz: 37.7422, thud_level: 0.0, scuff_hz: 145.1613, scuff_level: 0.0513, scuff_rate: 92.3079, ring_hz: 116.0, ring_level: 0.05, ring_rate: 47.0, ..dirt },
        }
    }
}

/// A boot coming down: rendered per step, so that no two steps are the same and each suits the ground.
pub fn footstep(seed: u32, sound: &StepSound) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    for _ in 0..8 {
        noise.next();
    }
    let mut vary = |amount: f32| 1.0 + noise.next() * amount;
    let (thud_hz, roll_delay, scuff_level) = (sound.thud_hz * vary(sound.thud_vary), sound.roll_delay * vary(0.3), sound.scuff_level * vary(0.25));
    let grain_at: Vec<f32> = (0..sound.grains).map(|_| roll_delay + (vary(1.0) - 1.0).abs() * 0.14).collect();
    let (mut grit, mut smooth) = (LowPass::default(), LowPass::default());
    let scuff_step = LowPass::step_size(sound.scuff_hz);
    let mut phase = 0.0;
    render(0.3, |t| {
        phase += std::f32::consts::TAU * thud_hz * (1.0 + sound.thud_sweep * (-t * sound.thud_sweep_rate).exp()) / RATE as f32;
        let thud = phase.sin() * (-t * sound.thud_rate).exp() * sound.thud_level;
        let ring = (std::f32::consts::TAU * sound.ring_hz * t).sin() * (-t * sound.ring_rate).exp() * sound.ring_level;
        let grit_in = grit.step_by(noise.next(), scuff_step) * 3.0;
        let since_roll = t - roll_delay;
        let scuff_shape = if since_roll < 0.0 {
            0.0
        } else if sound.grains == 0 {
            (since_roll * 500.0).min(1.0) * (-since_roll * sound.scuff_rate).exp()
        } else {
            let crunches: f32 = grain_at.iter().filter(|&&at| t >= at).map(|&at| (-(t - at) * sound.grain_rate).exp()).sum();
            crunches.min(1.0) * (-since_roll * sound.scuff_rate).exp()
        };
        let sample = thud + ring + smooth.step(grit_in * scuff_shape * scuff_level, sound.scuff_top_hz);
        sample / (1.0 + sample * sample).sqrt()
    })
}

/// Something sliding: cloth over webbing, a sling, a barrel along a wall. Band-limited noise that eases in
/// and out, with no thump or click at either end, and a roughness that makes it drag rather than hiss.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SlideSound {
    /// How loud it plays.
    pub volume: f32,
    pub seconds: f32,
    /// How much of its length it takes to swell up, from 0 to 1.
    pub swell: f32,
    /// The brightest part of it, at the start and at the end, so it can rise or fall as it goes.
    pub top_hz_from: f32,
    pub top_hz_to: f32,
    /// What is below this is taken out, which is what keeps it from being a thump.
    pub bottom_hz: f32,
    /// How unevenly it drags: 0 is a smooth hiss, 1 catches and lets go.
    pub drag: f32,
    /// How fast it catches, per second.
    pub drag_hz: f32,
    /// How much one differs from the next in length, brightness, roughness and level, as a share of each,
    /// and past a third of that it can also run the other way, as an arm and a leg do.
    pub vary: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SlideSounds {
    pub stance: SlideSound,
    pub climb: SlideSound,
    pub sights_up: SlideSound,
    pub sights_down: SlideSound,
    pub gun_on_wall: SlideSound,
    pub gun_change: SlideSound,
    /// Everything else done with the hands: picking something out, a seat, setting the gun down on something.
    pub handling: SlideSound,
    /// Dragging yourself along the ground, in place of footsteps when prone.
    pub crawl: SlideSound,
    /// A magazine drawn out of the gun and pushed back in.
    pub mag_out: SlideSound,
    pub mag_in: SlideSound,
    /// Webbing and kit shifting with each step, as loud as this at a sprint.
    pub gear: SlideSound,
    /// A heavy hull coming up off the ground, metal against metal and against the ground.
    pub hull_groan: SlideSound,
}

impl SlideSounds {
    pub fn of(&self, what: Moved) -> &SlideSound {
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

impl Default for SlideSounds {
    fn default() -> Self {
        let stance = SlideSound { volume: 0.026, seconds: 0.3, swell: 0.3, top_hz_from: 1500.0, top_hz_to: 700.0, bottom_hz: 260.0, drag: 0.35, drag_hz: 38.0, vary: 0.12 };
        Self {
            stance,
            hull_groan: SlideSound { volume: 0.5, seconds: 1.3, swell: 0.4, top_hz_from: 520.0, top_hz_to: 240.0, bottom_hz: 60.0, drag: 0.85, drag_hz: 11.0, vary: 0.2 },
            climb: SlideSound { volume: 0.029, seconds: 0.5, swell: 0.45, top_hz_from: 1000.0, top_hz_to: 1700.0, drag: 0.5, drag_hz: 26.0, ..stance },
            sights_up: SlideSound { volume: 0.016, seconds: 0.16, top_hz_from: 1300.0, top_hz_to: 2300.0, bottom_hz: 450.0, drag: 0.2, ..stance },
            sights_down: SlideSound { volume: 0.016, seconds: 0.15, top_hz_from: 1900.0, top_hz_to: 1100.0, bottom_hz: 450.0, drag: 0.2, ..stance },
            gun_on_wall: SlideSound { volume: 0.029, seconds: 0.24, swell: 0.2, top_hz_from: 3400.0, top_hz_to: 2000.0, bottom_hz: 750.0, drag: 0.75, drag_hz: 70.0, ..stance },
            gun_change: SlideSound { volume: 0.026, seconds: 0.32, top_hz_from: 1200.0, top_hz_to: 2200.0, bottom_hz: 380.0, drag: 0.45, drag_hz: 45.0, ..stance },
            crawl: SlideSound { volume: 0.03, seconds: 0.8, swell: 0.45, top_hz_from: 1100.0, top_hz_to: 650.0, bottom_hz: 220.0, drag: 0.6, drag_hz: 24.0, vary: 0.45 },
            mag_out: SlideSound { volume: 0.05, seconds: 0.26, swell: 0.55, top_hz_from: 1700.0, top_hz_to: 3000.0, bottom_hz: 500.0, drag: 0.55, drag_hz: 55.0, vary: 0.2 },
            mag_in: SlideSound { volume: 0.06, seconds: 0.2, swell: 0.75, top_hz_from: 2600.0, top_hz_to: 1300.0, bottom_hz: 420.0, drag: 0.55, drag_hz: 60.0, vary: 0.2 },
            gear: SlideSound { volume: 0.016, seconds: 0.26, swell: 0.4, top_hz_from: 1500.0, top_hz_to: 2300.0, bottom_hz: 450.0, drag: 0.6, drag_hz: 42.0, vary: 0.45 },
            handling: SlideSound { volume: 0.025, seconds: 0.22, top_hz_from: 1600.0, top_hz_to: 1000.0, bottom_hz: 320.0, ..stance },
        }
    }
}

/// One slide, rendered per event so that no two are the same.
pub fn slide(seed: u32, sound: &SlideSound) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    for _ in 0..8 {
        noise.next();
    }
    let mut vary = |share: f32| 1.0 + noise.next() * share * sound.vary;
    let seconds = (sound.seconds * vary(1.0)).max(0.01);
    let swell = (sound.swell * vary(1.0)).clamp(0.01, 0.99) * seconds;
    let (mut top_from, mut top_to) = (sound.top_hz_from * vary(1.0), sound.top_hz_to * vary(1.0));
    let (bottom_hz, drag_hz, drag_amount, level) = (sound.bottom_hz * vary(0.5), sound.drag_hz * vary(1.0), sound.drag * vary(0.5), vary(1.0));
    if vary(1.0) > 1.0 + sound.vary / 3.0 {
        std::mem::swap(&mut top_from, &mut top_to);
    }
    let (mut top, mut bottom, mut catches) = (LowPass::default(), LowPass::default(), LowPass::default());
    let catch_step = LowPass::step_size(drag_hz);
    let bottom_step = LowPass::step_size(bottom_hz);
    render(seconds, |t| {
        let share = t / seconds;
        let n = noise.next();
        let bright = top.step(n, top_from + (top_to - top_from) * share);
        let band = bright - bottom.step_by(bright, bottom_step);
        // Slow noise, stretched back up to full size, for how hard it is dragging just now.
        let catching = (catches.step_by(noise.next(), catch_step) * 6.0).abs().min(1.0);
        let drag = 1.0 - drag_amount.clamp(0.0, 1.0) * (1.0 - catching);
        // Half a cosine each way, which starts and stops with no corner to be heard as a click.
        let shape = if t < swell { 0.5 - 0.5 * (std::f32::consts::PI * t / swell).cos() } else { 0.5 + 0.5 * (std::f32::consts::PI * (t - swell) / (seconds - swell)).cos() };
        let sample = band * drag * shape * level * 5.0;
        sample / (1.0 + sample * sample).sqrt()
    })
}

/// A smoke grenade pouring: rough airy noise that goes on for as long as the smoke does. It is made a stretch
/// at a time as it plays, each one new and fading into the next, so that it never comes round again.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SmokeSound {
    /// How loud it plays right up close.
    pub volume: f32,
    /// How long each stretch is, and how much of that it shares with the next.
    pub stretch_seconds: f32,
    pub overlap_seconds: f32,
    /// The top and bottom of the hiss. What is below the bottom is taken out, so it has no rumble.
    pub top_hz: f32,
    pub bottom_hz: f32,
    /// How far the brightness and the level wander, as a share of each, and how many times a second.
    pub top_wander: f32,
    pub level_wander: f32,
    pub wander_hz: f32,
    /// How unevenly it comes out: 0 is smooth, 1 sputters. And how fast, per second.
    pub rough: f32,
    pub rough_hz: f32,
    /// The fizz as it starts: how long it takes to ease in, how much louder it starts than it goes on, how
    /// bright, and how long until that has mostly settled.
    pub fizz_in_seconds: f32,
    pub fizz_level: f32,
    pub fizz_top_hz: f32,
    pub fizz_seconds: f32,
}

impl Default for SmokeSound {
    fn default() -> Self {
        Self {
            volume: 0.03,
            stretch_seconds: 3.0,
            overlap_seconds: 0.8,
            top_hz: 4200.0,
            bottom_hz: 800.0,
            top_wander: 0.35,
            level_wander: 0.3,
            wander_hz: 0.9,
            rough: 0.5,
            rough_hz: 34.0,
            fizz_in_seconds: 0.05,
            fizz_level: 0.9,
            fizz_top_hz: 8500.0,
            fizz_seconds: 0.5,
        }
    }
}

impl SmokeSound {
    /// How long after one stretch starts the next one should.
    pub fn every(&self) -> f32 {
        (self.stretch_seconds - self.overlap_seconds).max(0.05)
    }
}

/// A value going smoothly from one random place to the next, from -1 to 1.
struct Wander {
    from: f32,
    to: f32,
    at: f32,
    step: f32,
}

impl Wander {
    fn new(noise: &mut Noise, hz: f32) -> Self {
        Self { from: noise.next(), to: noise.next(), at: 0.0, step: hz.max(0.01) / RATE as f32 }
    }

    fn next(&mut self, noise: &mut Noise) -> f32 {
        self.at += self.step;
        if self.at >= 1.0 {
            self.at -= 1.0;
            self.from = self.to;
            self.to = noise.next();
        }
        self.from + (self.to - self.from) * (0.5 - 0.5 * (std::f32::consts::PI * self.at).cos())
    }
}

/// One stretch of the hiss, with `starting` for the first, which fizzes. Its ends are made to overlap another
/// one's: their power adds up to the same all the way across, and with nothing to overlap it fades to nothing.
pub fn smoke_hiss(seed: u32, sound: &SmokeSound, starting: bool) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    for _ in 0..8 {
        noise.next();
    }
    let seconds = sound.stretch_seconds.max(0.1);
    let overlap = sound.overlap_seconds.clamp(0.01, seconds * 0.5);
    let ease_in = if starting { sound.fizz_in_seconds.clamp(0.01, overlap) } else { overlap };
    let mut top_wander = Wander::new(&mut noise, sound.wander_hz);
    let mut level_wander = Wander::new(&mut noise, sound.wander_hz * 0.7);
    let mut catches = Wander::new(&mut noise, sound.rough_hz);
    let (mut top, mut bottom) = (LowPass::default(), LowPass::default());
    let bottom_step = LowPass::step_size(sound.bottom_hz);
    let quarter = std::f32::consts::FRAC_PI_2;
    render(seconds, |t| {
        let fizz = if starting { (-t / sound.fizz_seconds.max(1e-3)).exp() } else { 0.0 };
        let top_hz = sound.top_hz * (1.0 + sound.top_wander.clamp(0.0, 0.9) * top_wander.next(&mut noise));
        let bright = top.step(noise.next(), top_hz + (sound.fizz_top_hz - top_hz) * fizz);
        let band = bright - bottom.step_by(bright, bottom_step);
        let drag = 1.0 - sound.rough.clamp(0.0, 1.0) * (0.5 + 0.5 * catches.next(&mut noise));
        let level = (1.0 + sound.level_wander.clamp(0.0, 0.9) * level_wander.next(&mut noise)) * (1.0 + sound.fizz_level * fizz);
        // A quarter of a sine each way, for when two uncorrelated noises fade across each other.
        let shape = (quarter * (t / ease_in).min(1.0)).sin() * (quarter * ((seconds - t) / overlap).min(1.0)).sin();
        let sample = band * drag * level * shape * 2.5;
        sample / (1.0 + sample * sample).sqrt()
    })
}

/// A whole run of the hiss as the game would play it, for the tuner and for listening to it across its joins.
pub fn smoke_run(seed: u32, sound: &SmokeSound, seconds: f32) -> Vec<f32> {
    let every = sound.every();
    let stretches = (seconds / every).ceil().max(1.0) as u32;
    let mut out: Vec<f32> = Vec::new();
    for i in 0..stretches {
        let stretch = smoke_hiss(seed.wrapping_add(i.wrapping_mul(7919)), sound, i == 0);
        let from = (i as f32 * every * RATE as f32) as usize;
        out.resize(out.len().max(from + stretch.len()), 0.0);
        for (at, sample) in out[from..].iter_mut().zip(stretch) {
            *at += sample;
        }
    }
    out
}

// The sounds below are the Current set as they were, kept so that a player can go back to them.

/// How loud the one-off sounds of the Current set play, which have no settings of their own.
pub const CURRENT_DRY_VOLUME: f32 = 0.7;
pub const CURRENT_SELECTOR_VOLUME: f32 = 0.6;
pub const CURRENT_ZIP_VOLUME: f32 = 0.5;
pub const CURRENT_TEAR_VOLUME: f32 = 0.45;

/// How loud things heard from afar are right up close, in both sets. New ones scale this by their `volume`.
pub const IMPACT_LOUDNESS: f32 = 0.5;
pub const METAL_HIT_LOUDNESS: f32 = 0.7;
pub const CLINK_LOUDNESS: f32 = 0.35;
pub const CRASH_LOUDNESS: f32 = 1.0;

/// What the frozen renders of the Current set are made from.
pub fn current_selector() -> Vec<f32> {
    clack(0x53, 3100.0, 0.35)
}

pub fn current_zip() -> Vec<f32> {
    clack(0x56, 2200.0, 0.3)
}

/// Exponential decay from 1, a multiplication per sample rather than an `exp` per sample.
pub struct Fade {
    level: f32,
    factor: f32,
}

impl Fade {
    pub fn new(rate: f32) -> Self {
        Self { level: 1.0, factor: (-rate / RATE as f32).exp() }
    }

    pub fn next(&mut self) -> f32 {
        let level = self.level;
        self.level *= self.factor;
        level
    }
}

/// A strip of field dressing torn open.
pub fn tear() -> Vec<f32> {
    let mut noise = Noise(0x7A11);
    let mut f = LowPass::default();
    render(0.5, |t| {
        // Rips in little bursts, each one a touch later and quieter than the one before.
        let flutter = 0.55 + 0.45 * (t * 130.0).sin().abs();
        f.step(noise.next(), 5200.0) * flutter * (-t * 6.0).exp() * (t * 60.0).min(1.0) * 0.8
    })
}

pub fn dry_click() -> Vec<f32> {
    let mut noise = Noise(0x4123);
    let mut f = LowPass::default();
    render(0.05, |t| f.step(noise.next(), 5000.0) * (-t * 170.0).exp() * 0.8)
}

/// Metal on metal, for magazines, bolts and selectors. `pitch` shifts it per use.
pub fn clack(seed: u32, pitch: f32, level: f32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let mut f = LowPass::default();
    render(0.12, |t| {
        let tone = (std::f32::consts::TAU * pitch * t).sin() * 0.5
            + (std::f32::consts::TAU * pitch * 2.76 * t).sin() * 0.3;
        (f.step(noise.next(), 3500.0) * 0.8 + tone) * (-t * 55.0).exp() * level
    })
}

/// Bullet striking dirt or concrete, heard from wherever it landed.
pub fn impact(seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let mut f = LowPass::default();
    render(0.14, |t| {
        let n = noise.next();
        f.step(n, 2300.0) * (-t * 44.0).exp() * 1.2
    })
}

/// A round going into sheet metal: a sharp tick, a ring with a few inharmonic partials, a little rattle.
pub fn metal_hit(seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let pitch = 1.0 + (seed >> 16 & 0xff) as f32 / 255.0 * 0.35;
    let partials = [(1150.0, 0.5, 34.0), (2710.0, 0.32, 48.0), (4380.0, 0.2, 70.0), (6650.0, 0.1, 110.0)];
    let length = (RATE as f32 * 0.22) as usize;
    let mut tick = Fade::new(420.0);
    (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let ring: f32 = partials
                .iter()
                .map(|&(hz, level, decay)| (t * hz * pitch * std::f32::consts::TAU).sin() * level * (-t * decay).exp())
                .sum();
            ring * 0.8 + noise.next() * tick.next() * 0.7
        })
        .collect()
}

/// A vehicle hitting something: a low thud with metal crumpling over it.
pub fn crash(seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let (mut thud, mut crumple) = (LowPass::default(), LowPass::default());
    let (mut body, mut rattle) = (Fade::new(9.0), Fade::new(6.0));
    let length = (RATE as f32 * 0.65) as usize;
    (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let n = noise.next();
            let low = thud.step(n, 180.0) * 5.0 * body.next();
            let bang = (t * 62.0 * std::f32::consts::TAU).sin() * (-t * 16.0).exp() * 0.8;
            // Bursts of metal, on a beat of its own so no two crashes ring alike.
            let gate = (((t * 37.0 + (seed & 7) as f32).sin() * 3.0).max(0.0)).min(1.0);
            let high = (n - crumple.step(n, 1400.0)) * gate * rattle.next() * 0.55;
            (low + bang + high).clamp(-1.0, 1.0)
        })
        .collect()
}

/// The same low thud, with the metal on it from [`contact`] in place of filtered noise.
pub fn contact_crash(seed: u32, metal: &ContactSound) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let mut thud = LowPass::default();
    let mut body = Fade::new(9.0);
    let sound_volume = metal.volume;
    let metal = contact(seed.rotate_left(5), metal);
    let length = (RATE as f32 * 0.65) as usize;
    (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let low = thud.step(noise.next(), 180.0) * 5.0 * body.next();
            let bang = (t * 62.0 * std::f32::consts::TAU).sin() * (-t * 16.0).exp() * 0.8;
            let sample = low + bang + metal.get(i).copied().unwrap_or(0.0) * sound_volume;
            sample.clamp(-1.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Today's sounds, as measured before the New set was added, so moving them about cannot change them.
    #[test]
    fn the_current_set_is_what_it_always_was() {
        let (steps, slides) = (StepSounds::default(), SlideSounds::default());
        let check = |name: &str, samples: Vec<f32>, length: usize, rms: f64, third: f32, two_thirds: f32| {
            let measured = (samples.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / samples.len() as f64).sqrt();
            assert_eq!(samples.len(), length, "{name}");
            assert!((measured - rms).abs() < 1e-3, "{name}: {measured}");
            assert!((samples[length / 3] - third).abs() < 1e-3 && (samples[length * 2 / 3] - two_thirds).abs() < 1e-3, "{name}");
        };
        check("footstep_dirt", footstep(1234, &steps.dirt), 13230, 0.005611, 0.012597, 0.000264);
        check("footstep_gravel", footstep(1234, &steps.gravel), 13230, 0.010214, 0.012136, 0.000414);
        check("footstep_metal", footstep(1234, &steps.metal), 13230, 0.006787, 0.001210, 0.000004);
        check("slide_stance", slide(1234, &slides.stance), 14076, 0.249634, -0.037229, 0.024355);
        check("slide_crawl", slide(1234, &slides.crawl), 43748, 0.113070, 0.110324, 0.000232);
        check("slide_mag_in", slide(1234, &slides.mag_in), 9760, 0.226326, 0.252587, -0.497929);
        check("dry_click", dry_click(), 2205, 0.064720, -0.019959, -0.000671);
        check("selector", current_selector(), 5292, 0.043257, 0.001038, 0.000458);
        check("zip", current_zip(), 5292, 0.039268, -0.010376, -0.001556);
        check("tear", tear(), 22050, 0.088810, -0.096622, -0.051943);
        check("impact", impact(77), 6174, 0.079708, -0.030585, -0.010355);
        check("metal_hit", metal_hit(77), 9702, 0.089459, 0.021965, -0.002315);
        check("crash", crash(77), 28664, 0.169751, 0.018473, -0.009697);
    }

    fn power(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn a_hiss_has_no_click_at_either_end_of_a_stretch_or_of_a_whole_run() {
        let sound = SmokeSound::default();
        for seed in 0..20 {
            let middle = smoke_hiss(seed * 97 + 1, &sound, false);
            let first = smoke_hiss(seed * 97 + 1, &sound, true);
            let run = smoke_run(seed * 97 + 1, &sound, 8.0);
            for samples in [&middle, &first, &run] {
                let peak = loudest(samples);
                let edge = RATE as usize / 1000;
                assert!(peak > 0.1 && peak <= 2.0, "{peak}");
                assert!(samples[0].abs() < 0.01, "it starts on a step: {}", samples[0]);
                assert!(samples.last().unwrap().abs() < 0.01, "it stops on a step");
                assert!(loudest(&samples[..edge]) < peak * 0.5, "it starts with a click");
                assert!(loudest(&samples[samples.len() - edge..]) < peak * 0.25, "it stops with a click");
            }
        }
    }

    #[test]
    fn a_hiss_is_air_with_no_thump_in_it_and_the_fizz_is_louder_and_brighter_and_soon_over() {
        let sound = SmokeSound::default();
        let run = smoke_run(3, &sound, 6.0);
        let mut low = LowPass::default();
        let (mut under, mut all) = (0.0, 0.0);
        for &s in &run {
            let l = low.step(s, 200.0);
            under += l * l;
            all += s * s;
        }
        assert!(under / all < 0.05, "a thump in it: {}", under / all);
        let at = |from: f32, to: f32| power(&run[(from * RATE as f32) as usize..(to * RATE as f32) as usize]);
        let (fizz, settled) = (at(0.06, 0.3), at(2.5, 5.5));
        assert!(fizz > settled * 1.2, "the fizz has to be heard: {fizz} against {settled}");
        assert!(fizz < settled * 3.0, "and not be a bang: {fizz} against {settled}");
    }

    /// Stretches change over with no dip or bump a person would hear as a beat every couple of seconds.
    #[test]
    fn a_long_hiss_keeps_changing_and_has_no_beat_where_stretches_meet() {
        let sound = SmokeSound::default();
        let run = smoke_run(11, &sound, 30.0);
        let window = RATE as usize / 4;
        let levels: Vec<f32> = run[RATE as usize * 2..run.len() - RATE as usize * 2].chunks(window).map(power).collect();
        let mean = levels.iter().sum::<f32>() / levels.len() as f32;
        let (least, most) = levels.iter().fold((f32::MAX, 0.0_f32), |(l, m), &v| (l.min(v), m.max(v)));
        assert!(least > mean * 0.35 && most < mean * 2.2, "{least} to {most} around {mean}");
        assert!(most > least * 1.3, "it should wander: {least} to {most}");
        // Against itself one and two stretches on, it should not match at all.
        let every = (sound.every() * RATE as f32) as usize;
        for lag in [every, every * 2, every * 5] {
            let (a, b) = (&run[RATE as usize * 3..RATE as usize * 8], &run[RATE as usize * 3 + lag..RATE as usize * 8 + lag]);
            let match_up = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / (a.len() as f32 * power(a) * power(b));
            assert!(match_up.abs() < 0.05, "it repeats after {lag}: {match_up}");
        }
    }

    #[test]
    fn no_two_hisses_are_the_same() {
        let sound = SmokeSound::default();
        assert_ne!(smoke_hiss(1, &sound, false), smoke_hiss(2, &sound, false));
        assert_ne!(smoke_run(1, &sound, 5.0), smoke_run(2, &sound, 5.0));
    }

    #[test]
    fn a_mod_can_take_the_hiss_to_its_limits_without_anything_breaking() {
        let flat = SmokeSound { rough: 0.0, top_wander: 0.0, level_wander: 0.0, wander_hz: 0.0, fizz_level: 0.0, fizz_seconds: 0.0, fizz_in_seconds: 0.0, ..SmokeSound::default() };
        let tiny = SmokeSound { stretch_seconds: 0.0, overlap_seconds: 0.0, ..flat };
        let huge = SmokeSound { overlap_seconds: 100.0, rough: 5.0, top_wander: 5.0, level_wander: 5.0, ..flat };
        for sound in [flat, tiny, huge] {
            assert!(smoke_run(1, &sound, 2.0).iter().all(|s| s.is_finite() && s.abs() < 40.0));
        }
    }

    fn loudest(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |most, s| most.max(s.abs()))
    }

    #[test]
    fn a_slide_starts_and_ends_with_no_click_and_has_no_thump() {
        let slides = SlideSounds::default();
        for sound in [slides.stance, slides.climb, slides.sights_up, slides.sights_down, slides.gun_on_wall, slides.gun_change, slides.handling, slides.crawl, slides.mag_out, slides.mag_in, slides.gear] {
            for seed in 0..20 {
                let samples = slide(seed * 97 + 1, &sound);
                let peak = loudest(&samples);
                let edge = (RATE as usize) / 500;
                assert!(peak > 0.05 && peak <= 1.0, "{sound:?}: {peak}");
                assert!(loudest(&samples[..edge]) < peak * 0.5, "{sound:?} starts with a click");
                assert!(loudest(&samples[samples.len() - edge..]) < peak * 0.5, "{sound:?} stops with a click");
                // Mostly above where a body's thump would be.
                let mut low = LowPass::default();
                let (mut under, mut all) = (0.0, 0.0);
                for &s in &samples {
                    let l = low.step(s, 120.0);
                    under += l * l;
                    all += s * s;
                }
                assert!(under / all < 0.25, "{sound:?} has a thump in it: {}", under / all);
            }
        }
    }

    /// A crawl goes on for a long time, so it must not sound like one sample over and over.
    #[test]
    fn a_crawl_keeps_changing() {
        let sound = SlideSounds::default().crawl;
        let (mut shortest, mut longest, mut quietest, mut loudest_seen) = (usize::MAX, 0, 1.0_f32, 0.0_f32);
        for seed in 1..200 {
            let samples = slide(seed * 131, &sound);
            shortest = shortest.min(samples.len());
            longest = longest.max(samples.len());
            quietest = quietest.min(loudest(&samples));
            loudest_seen = loudest_seen.max(loudest(&samples));
        }
        assert!(longest as f32 > shortest as f32 * 1.5, "{shortest} to {longest}");
        assert!(loudest_seen > quietest * 1.3, "{quietest} to {loudest_seen}");
    }

    #[test]
    fn no_two_slides_are_the_same() {
        let sound = SlideSounds::default().stance;
        assert_ne!(slide(1, &sound), slide(2, &sound));
    }
}
