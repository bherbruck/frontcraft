//! How badly you are bleeding, as everything that shows it needs it: the red at the edge of the screen,
//! the readout and the heartbeat all go by this, so they are always in time and always agree.

use bevy::prelude::*;
use prs_sim::Care;
use prs_sim::health::{BleedLook, BleedSeverity, Life, seconds_until_down};

use crate::LocalView;

#[derive(Resource, Clone, Debug)]
pub struct BleedShown {
    pub look: BleedLook,
    pub severity: BleedSeverity,
    /// Until the blood running out puts you down, if it is running out.
    pub seconds_left: Option<f32>,
    /// How far a bandage on yourself has got, from 0 to 1.
    pub bandaging: f32,
    /// The strength of the red right now, on its way to where the severity says.
    pub strength: f32,
    /// How big the frame of red is right now.
    pub frame: f32,
    /// The colour of the readout right now.
    pub colour: [f32; 3],
    /// Where in a heartbeat this is, where 0 is the top of a beat and 1 is the next.
    pub phase: f32,
    /// How many have gone by. Whatever makes the sound listens for this changing.
    pub beats: u32,
    /// Seconds left of saying the bleeding has stopped.
    pub stopped_for: f32,
    pub beats_per_minute: f32,
}

impl BleedShown {
    pub fn new(look: BleedLook) -> Self {
        Self {
            colour: look.light.colour,
            frame: look.light.frame,
            beats_per_minute: look.light.beats_per_minute,
            look,
            severity: BleedSeverity::None,
            seconds_left: None,
            bandaging: 0.0,
            strength: 0.0,
            phase: 0.0,
            beats: 0,
            stopped_for: 0.0,
        }
    }

    /// What is left after what a bandage going on has taken away, so you can see it working.
    pub fn relieved(&self) -> f32 {
        1.0 - self.look.bandage_relief * self.bandaging
    }

    /// How opaque the red is this instant. It swells and eases on a plain wave, the gentlest there is for
    /// how far it moves, and never goes dark between beats, so it is a pulse and not a flash.
    pub fn alpha(&self) -> f32 {
        let beat = (self.phase * std::f32::consts::TAU).cos() * 0.5 + 0.5;
        self.strength * (1.0 - self.look.pulse_depth * (1.0 - beat))
    }

    fn step(&mut self, life: Life, health: f32, bleed: f32, bandaging: f32, dt: f32) {
        let was = self.severity;
        self.seconds_left = seconds_until_down(life, health, bleed);
        self.severity = self.look.severity(self.seconds_left);
        self.bandaging = bandaging;
        if was != BleedSeverity::None && self.severity == BleedSeverity::None && life == Life::Up {
            self.stopped_for = self.look.stopped_note_seconds;
        }
        self.stopped_for = (self.stopped_for - dt).max(0.0);

        let band = self.look.band(self.severity).copied();
        let target = band.map_or(0.0, |band| band.strength * self.relieved());
        let rate = if target > self.strength { self.look.rise_rate } else { self.look.fall_rate };
        let ease = 1.0 - (-rate * dt).exp();
        self.strength += (target - self.strength) * ease;
        // While it fades out it keeps the shape and colour it had.
        if let Some(band) = band {
            self.frame += (band.frame - self.frame) * ease;
            self.beats_per_minute += (band.beats_per_minute - self.beats_per_minute) * ease;
            for (colour, wanted) in self.colour.iter_mut().zip(band.colour) {
                *colour += (wanted - *colour) * ease;
            }
            self.phase += dt * self.beats_per_minute / 60.0;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.beats = self.beats.wrapping_add(1);
            }
        }
    }
}

pub(crate) fn update_bleed_shown(time: Res<Time>, view: Res<LocalView>, mut shown: ResMut<BleedShown>) {
    let s = &view.blended;
    let bandaging = match s.treating {
        Some((Care::Bandage, progress)) if s.treating_yourself => progress,
        _ => 0.0,
    };
    shown.step(s.life, s.health, s.bleed, bandaging, time.delta_secs());
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(shown: &mut BleedShown, health: f32, bleed: f32, bandaging: f32, seconds: f32) {
        for _ in 0..(seconds / DT) as usize {
            shown.step(Life::Up, health, bleed, bandaging, DT);
        }
    }

    #[test]
    fn calm_at_rest_nothing_shows_and_nothing_beats() {
        let mut shown = BleedShown::new(BleedLook::default());
        run(&mut shown, 100.0, 0.0, 0.0, 5.0);
        assert_eq!(shown.severity, BleedSeverity::None);
        assert_eq!(shown.alpha(), 0.0);
        assert_eq!(shown.beats, 0);
        // Hurt but no longer bleeding is calm too. The bar says how much blood is left.
        run(&mut shown, 20.0, 0.0, 0.0, 5.0);
        assert_eq!(shown.alpha(), 0.0);
    }

    #[test]
    fn worse_bleeds_are_stronger_and_beat_faster_and_it_settles_where_the_look_says() {
        let mut seen = Vec::new();
        for (health, bleed, severity) in [(90.0, 1.0, BleedSeverity::Light), (60.0, 2.0, BleedSeverity::Heavy), (30.0, 4.0, BleedSeverity::Critical)] {
            let mut shown = BleedShown::new(BleedLook::default());
            run(&mut shown, health, bleed, 0.0, 10.0);
            assert_eq!(shown.severity, severity);
            let band = *shown.look.band(severity).unwrap();
            assert!((shown.strength - band.strength).abs() < 0.01);
            assert!((shown.frame - band.frame).abs() < 0.01);
            let beats = shown.beats as f32;
            assert!((beats - band.beats_per_minute / 6.0).abs() < 1.5, "{beats} beats in 10 s at {} a minute", band.beats_per_minute);
            seen.push((shown.strength, beats));
        }
        assert!(seen[0].0 < seen[1].0 && seen[1].0 < seen[2].0);
        assert!(seen[0].1 < seen[1].1 && seen[1].1 < seen[2].1);
    }

    #[test]
    fn it_swells_and_eases_and_changes_gently_from_one_frame_to_the_next() {
        let mut shown = BleedShown::new(BleedLook::default());
        run(&mut shown, 30.0, 4.0, 0.0, 5.0);
        let (mut top, mut bottom, mut last, mut biggest_step) = (0.0_f32, 1.0_f32, shown.alpha(), 0.0_f32);
        for _ in 0..300 {
            shown.step(Life::Up, 30.0, 4.0, 0.0, DT);
            let now = shown.alpha();
            top = top.max(now);
            bottom = bottom.min(now);
            biggest_step = biggest_step.max((now - last).abs());
            last = now;
        }
        assert!(top - bottom > 0.2, "it has to be seen to beat: {bottom} to {top}");
        assert!(bottom > top * 0.4, "and never go dark between beats: {bottom} to {top}");
        assert!(biggest_step < 0.04, "no jumps, even at the worst: {biggest_step}");
    }

    #[test]
    fn a_bandage_going_on_visibly_eases_it_and_when_it_is_on_it_fades_and_says_so() {
        let mut shown = BleedShown::new(BleedLook::default());
        run(&mut shown, 60.0, 2.0, 0.0, 5.0);
        let before = shown.strength;
        run(&mut shown, 60.0, 2.0, 0.9, 3.0);
        assert!(shown.strength < before * 0.55, "{} from {before}", shown.strength);
        assert!(shown.relieved() < 0.5);

        shown.step(Life::Up, 60.0, 0.0, 0.0, DT);
        assert_eq!(shown.severity, BleedSeverity::None);
        assert!(shown.stopped_for > 0.0, "the readout says it has stopped");
        let beats = shown.beats;
        run(&mut shown, 60.0, 0.0, 0.0, 4.0);
        assert!(shown.strength < 0.01, "and the red is gone");
        assert_eq!(shown.stopped_for, 0.0);
        assert_eq!(shown.beats, beats, "and so is the heartbeat");
    }

    #[test]
    fn a_mod_can_turn_every_part_of_it_off_without_anything_breaking() {
        let mut look = BleedLook { pulse_depth: 0.0, rise_rate: 0.0, fall_rate: 0.0, bandage_relief: 0.0, stopped_note_seconds: 0.0, heavy_within: 0.0, critical_within: 0.0, ..BleedLook::default() };
        for band in [&mut look.light, &mut look.heavy, &mut look.critical] {
            band.strength = 0.0;
            band.beats_per_minute = 0.0;
        }
        let mut shown = BleedShown::new(look);
        run(&mut shown, 50.0, 2.0, 0.5, 2.0);
        assert_eq!(shown.alpha(), 0.0);
        assert_eq!(shown.beats, 0);
        // At no health and with a huge bleed it is still a number.
        run(&mut shown, 0.0, 1e6, 0.0, 0.1);
        assert!(shown.seconds_left.unwrap().is_finite() && shown.alpha().is_finite());
    }

    #[test]
    fn one_line_of_a_mod_changes_one_setting_of_one_severity() {
        let parsed = prs_sim::prs_mods::Mod::parse("test", "[view.bleed]\ncritical_within = 5.0\n[view.bleed.heavy]\nstrength = 0.2\n").unwrap();
        let look = parsed.over("view.bleed", BleedLook::default()).unwrap();
        assert_eq!(look.severity(Some(10.0)), BleedSeverity::Heavy);
        assert_eq!(look.heavy.strength, 0.2);
        assert_eq!(look.critical, BleedLook::default().critical);
    }

    #[test]
    fn going_down_is_not_a_bleeding_that_stopped() {
        let mut shown = BleedShown::new(BleedLook::default());
        run(&mut shown, 10.0, 4.0, 0.0, 1.0);
        shown.step(Life::Down, 0.0, 4.0, 0.0, DT);
        assert_eq!(shown.severity, BleedSeverity::None);
        assert_eq!(shown.stopped_for, 0.0);
    }
}
