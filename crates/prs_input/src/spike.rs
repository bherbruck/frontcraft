//! Rejects one-frame mouse glitches. Browsers and remote desktop layers occasionally report a huge
//! bogus delta, which would otherwise snap the view to a random direction.

use bevy::prelude::*;

/// Speed that always counts as real, however still the mouse was a moment ago.
const ALWAYS_REAL_SPEED: f32 = 15_000.0;
const ALWAYS_REAL_PIXELS: f32 = 60.0;
/// How many times faster than the last frame a mouse can plausibly get in one frame.
const MAX_SPEED_UP: f32 = 4.0;

#[derive(Default, Debug)]
pub(crate) struct SpikeFilter {
    last_delta: Vec2,
    last_speed: f32,
    suspect: Option<Vec2>,
    pub dropped: u32,
    pub largest_dropped: f32,
}

impl SpikeFilter {
    pub fn filter(&mut self, delta: Vec2, dt: f32) -> Vec2 {
        let dt = dt.max(0.0001);
        let size = delta.length();
        let limit = (self.last_speed * dt * MAX_SPEED_UP)
            .max(ALWAYS_REAL_SPEED * dt)
            .max(ALWAYS_REAL_PIXELS);

        // A glitch lasts one frame. Two big frames in the same direction is a genuine hard flick.
        let confirmed = self.suspect.is_some_and(|s| s.dot(delta) > 0.0);
        if size > limit && !confirmed {
            self.suspect = Some(delta);
            self.dropped += 1;
            self.largest_dropped = self.largest_dropped.max(size);
            return self.last_delta;
        }
        self.suspect = None;
        self.last_delta = delta;
        self.last_speed = size / dt;
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 144.0;

    #[test]
    fn a_hard_flick_that_builds_up_passes_untouched() {
        let mut f = SpikeFilter::default();
        for px in [3.0, 12.0, 40.0, 110.0, 260.0, 500.0, 300.0, 90.0, 10.0] {
            let d = Vec2::new(px, -px * 0.2);
            assert_eq!(f.filter(d, DT), d);
        }
        assert_eq!(f.dropped, 0);
    }

    #[test]
    fn a_one_frame_spike_is_replaced_with_the_previous_motion() {
        let mut f = SpikeFilter::default();
        let normal = Vec2::new(20.0, 4.0);
        f.filter(normal, DT);
        assert_eq!(f.filter(Vec2::new(-1900.0, 700.0), DT), normal);
        assert_eq!(f.filter(normal, DT), normal);
        assert_eq!(f.dropped, 1);
        assert!(f.largest_dropped > 1900.0);
    }

    #[test]
    fn a_sudden_real_flick_loses_at_most_one_frame() {
        let mut f = SpikeFilter::default();
        f.filter(Vec2::ZERO, DT);
        let fast = Vec2::new(600.0, 0.0);
        f.filter(fast, DT);
        assert_eq!(f.filter(fast, DT), fast);
        assert_eq!(f.filter(fast, DT), fast);
    }

    #[test]
    fn low_frame_rates_allow_proportionally_more_motion() {
        let mut f = SpikeFilter::default();
        let d = Vec2::new(700.0, 0.0);
        assert_eq!(f.filter(d, 0.1), d);
    }
}
