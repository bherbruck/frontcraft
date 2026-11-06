//! One button, two jobs, like Arma: a tap flips the sights and holding it zooms. A press only
//! counts as a tap once it is released, so zooming in never also flips the sights.

#[derive(Default, Debug)]
pub struct AimButton {
    pub sights: bool,
    pub zooming: bool,
    down: bool,
    held_for: f32,
}

impl AimButton {
    pub fn update(&mut self, just_pressed: bool, pressed: bool, dt: f32, hold_seconds: f32) {
        if just_pressed {
            self.down = true;
            self.held_for = 0.0;
        } else if self.down {
            self.held_for += dt;
        }
        if !self.down {
            return;
        }
        let held = self.held_for >= hold_seconds;
        self.zooming = pressed && held;
        if !pressed {
            self.sights ^= !held;
            self.down = false;
        }
    }
}

impl AimButton {
    /// The button means something else for now. The sights stay as they were, and this press is forgotten.
    pub fn set_aside(&mut self) {
        self.zooming = false;
        self.down = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.01;
    const HOLD: f32 = 0.18;

    /// Presses for `seconds`, then releases. Returns whether it was zooming just before release.
    fn press(button: &mut AimButton, seconds: f32) -> bool {
        button.update(true, true, DT, HOLD);
        for _ in 0..(seconds / DT) as u32 {
            button.update(false, true, DT, HOLD);
        }
        let zoomed = button.zooming;
        button.update(false, false, DT, HOLD);
        zoomed
    }

    #[test]
    fn a_tap_flips_the_sights_without_zooming() {
        let mut button = AimButton::default();
        assert!(!press(&mut button, 0.08));
        assert!(button.sights);
        assert!(!press(&mut button, 0.08));
        assert!(!button.sights);
    }

    #[test]
    fn a_hold_zooms_until_release_and_leaves_the_sights_alone() {
        for sights in [false, true] {
            let mut button = AimButton { sights, ..Default::default() };
            assert!(press(&mut button, 0.5));
            assert!(!button.zooming);
            assert_eq!(button.sights, sights);
        }
    }

    #[test]
    fn a_tap_inside_one_slow_frame_still_counts() {
        let mut button = AimButton::default();
        button.update(true, false, 0.05, HOLD);
        assert!(button.sights);
        assert!(!button.zooming);
    }
}
