//! A reload heard in step with the hands. The sounds come at the same points of the reload that move the
//! off hand, read from the same settings, so a mod that changes one keeps the other with it. They follow
//! the reload as you see it, which is the progress your own body is drawn from.

use bevy::prelude::*;
use prs_runtime::{LocalView, SimResource};
use prs_synth::{Moved, SoundSet};

use crate::hearing::Acoustics;
use crate::{AudioLevels, moved, touched};

/// The points of a reload that make a sound: the magazine coming off the gun, the new one seating, and the
/// hand going back to the foregrip, or after an empty magazine, to the charging handle.
const POINTS: usize = 3;

/// Which of a reload's sounds have played. Each plays once a reload, whichever way its progress jumps.
#[derive(Default)]
pub(crate) struct ReloadEars {
    last: f32,
    heard: [bool; POINTS],
}

impl ReloadEars {
    /// The sounds due now that a reload is `progress` of the way through, given the points each one plays at.
    /// Progress going back, as it can when a prediction is corrected, plays nothing again, and only a reload
    /// ending or being abandoned, which brings progress back to 0, lets them play for the next one.
    fn hear(&mut self, progress: f32, points: [f32; POINTS]) -> [bool; POINTS] {
        let mut due = [false; POINTS];
        if progress <= 0.0 {
            *self = Self::default();
            return due;
        }
        for (i, &at) in points.iter().enumerate() {
            if !self.heard[i] && self.last < at && progress >= at {
                self.heard[i] = true;
                due[i] = true;
            }
        }
        self.last = self.last.max(progress);
        due
    }
}

pub(crate) fn reload_sounds(
    levels: Res<AudioLevels>,
    mut commands: Commands,
    view: Res<LocalView>,
    sim: Res<SimResource>,
    acoustics: Res<Acoustics>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut ears: Local<ReloadEars>,
    mut seed: Local<u32>,
) {
    let state = &view.blended;
    let Some(gun) = sim.0.weapons.weapons.get(state.weapon.0 as usize) else {
        return;
    };
    // A reload from empty leaves the rounds where they were until it is done, so an empty gun reloading is
    // the long kind.
    let empty = state.ammo == 0;
    let seconds = if empty { gun.reload_empty_time } else { gun.reload_time }.max(0.01);
    let set = levels.sound_set();
    let next = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    let hand = &sim.0.body_cfg.reload;
    // The new magazine's sound starts early by as much as it takes to seat in it.
    let seat_after = match set {
        SoundSet::New => prs_synth::first_touch_seconds(next, &acoustics.contacts.mag_in).unwrap_or(0.0),
        SoundSet::Current => acoustics.slides.mag_in.swell * acoustics.slides.mag_in.seconds,
    };
    let points = [hand.at_magazine, (hand.back_at_magazine - seat_after / seconds).max(hand.at_magazine), hand.leave_magazine];
    let due = ears.hear(state.reload_progress, points);
    for (i, _) in due.iter().enumerate().filter(|(_, due)| **due) {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let this = if i == 1 { next } else { *seed };
        let wobble = 1.0 + ((this >> 8) & 0xff) as f32 / 255.0 * 0.06 - 0.03;
        match i {
            0 => moved(&mut commands, &mut sources, &acoustics, set, this, Moved::MagOut, 1.0, wobble),
            1 => moved(&mut commands, &mut sources, &acoustics, set, this, Moved::MagIn, 1.0, wobble),
            // Only the New set has a charging handle. Otherwise it is the hand settling back on the gun.
            _ if empty && set == SoundSet::New => touched(&mut commands, &mut sources, this, &acoustics.contacts.charge, 1.0, wobble),
            _ => moved(&mut commands, &mut sources, &acoustics, set, this, Moved::Handling, acoustics.reload_settle, wobble),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINTS_AT: [f32; POINTS] = [0.12, 0.6, 0.84];

    fn run(ears: &mut ReloadEars, progress: &[f32]) -> Vec<usize> {
        progress.iter().flat_map(|&p| ears.hear(p, POINTS_AT).into_iter().enumerate().filter(|(_, due)| *due).map(|(i, _)| i).collect::<Vec<_>>()).collect()
    }

    #[test]
    fn each_sound_plays_once_at_its_point() {
        let mut ears = ReloadEars::default();
        assert_eq!(run(&mut ears, &[0.0, 0.05, 0.11, 0.12, 0.3, 0.59, 0.61, 0.9, 1.0]), vec![0, 1, 2]);
    }

    #[test]
    fn a_corrected_prediction_going_back_plays_nothing_twice() {
        let mut ears = ReloadEars::default();
        assert_eq!(run(&mut ears, &[0.1, 0.13, 0.08, 0.14, 0.62, 0.55, 0.65, 0.9, 0.7, 0.95]), vec![0, 1, 2]);
    }

    #[test]
    fn a_big_jump_plays_everything_it_passed_over() {
        let mut ears = ReloadEars::default();
        assert_eq!(run(&mut ears, &[0.05, 0.7]), vec![0, 1]);
    }

    #[test]
    fn an_abandoned_reload_stops_and_the_next_starts_afresh() {
        let mut ears = ReloadEars::default();
        assert_eq!(run(&mut ears, &[0.05, 0.2, 0.4, 0.0, 0.0]), vec![0]);
        assert_eq!(run(&mut ears, &[0.05, 0.2, 0.7, 0.9, 0.0]), vec![0, 1, 2]);
    }
}
