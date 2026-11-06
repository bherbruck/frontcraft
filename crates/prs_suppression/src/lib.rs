//! Being shot at. A bullet going past close by leaves a soldier rattled, by how near it came, how big a
//! round it was and whether it cracked on the way, and that wears off slowly once it stops.
//!
//! This is one number and knows nothing about guns or the world: the sim says what went past, and hands
//! the level on to whatever reacts to it. It never touches where a bullet goes. It shakes the real gun
//! through the same inputs as a wound, it darkens the view, and bots can read it.

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SuppressionConfig {
    /// A mod can turn the whole thing off.
    pub enabled: bool,
    /// Metres a bullet has to pass inside to count at all.
    pub reach: f32,
    /// How sharply it falls off from a direct pass to `reach`. One is a straight line.
    pub falloff: f32,
    /// Added to the level by a rifle round passing right by. Other rounds scale this by their size.
    pub per_round: f32,
    /// Speed of sound in metres per second, above which a bullet cracks as it passes.
    pub crack_speed: f32,
    /// How much of the effect a bullet has that does not crack.
    pub subsonic: f32,
    /// How much of the effect a bullet has when something solid is between it and you.
    pub through_cover: f32,
    /// Metres a bullet has to have flown first, so firing past a mate's shoulder leaves them be.
    pub min_flown: f32,
    /// Seconds with no near misses before it starts to wear off.
    pub hold: f32,
    /// How fast it wears off after that, per second. From full, about ten seconds.
    pub decay: f32,
}

impl Default for SuppressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reach: 6.0,
            falloff: 1.5,
            per_round: 0.25,
            crack_speed: 343.0,
            subsonic: 0.55,
            through_cover: 0.35,
            min_flown: 6.0,
            hold: 1.2,
            decay: 0.1,
        }
    }
}

impl SuppressionConfig {
    /// What one bullet does. `round` is how big it is, with a rifle round being 1.
    pub fn near_miss(&self, distance: f32, round: f32, speed: f32, covered: bool) -> f32 {
        if !self.enabled || distance >= self.reach {
            return 0.0;
        }
        let closeness = (1.0 - distance / self.reach.max(1e-3)).clamp(0.0, 1.0).powf(self.falloff);
        let crack = if speed >= self.crack_speed { 1.0 } else { self.subsonic };
        let cover = if covered { self.through_cover } else { 1.0 };
        self.per_round * round * closeness * crack * cover
    }
}

/// How rattled a soldier is. Plain data, and the server's word goes: nobody predicts other people's
/// bullets, so a client only predicts this wearing off.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Suppression {
    /// 0 = calm, 1 = as bad as it gets.
    pub level: f32,
    /// Seconds before it starts to wear off.
    pub hold: f32,
    /// Whose bullet it was last, for bots working out who to shoot back at.
    pub last_shooter: Option<usize>,
}

impl Suppression {
    pub fn add(&mut self, cfg: &SuppressionConfig, amount: f32, shooter: usize) {
        if amount <= 0.0 {
            return;
        }
        self.level = (self.level + amount).min(1.0);
        self.hold = cfg.hold;
        self.last_shooter = Some(shooter);
    }

    pub fn step(&mut self, cfg: &SuppressionConfig, dt: f32) {
        if self.hold > 0.0 {
            self.hold = (self.hold - dt).max(0.0);
        } else if self.level > 0.0 {
            self.level = (self.level - cfg.decay * dt).max(0.0);
            if self.level == 0.0 {
                self.last_shooter = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFLE: f32 = 1.0;
    const FAST: f32 = 800.0;

    #[test]
    fn nearer_is_worse_and_past_the_reach_is_nothing() {
        let cfg = SuppressionConfig::default();
        let at = |d| cfg.near_miss(d, RIFLE, FAST, false);
        assert!(at(0.5) > at(1.0) && at(1.0) > at(3.0) && at(3.0) > 0.0);
        assert_eq!(at(cfg.reach), 0.0);
        assert_eq!(at(30.0), 0.0);
    }

    #[test]
    fn a_bigger_round_a_crack_and_open_ground_are_all_worse() {
        let cfg = SuppressionConfig::default();
        assert!(cfg.near_miss(1.0, 1.4, FAST, false) > cfg.near_miss(1.0, 0.6, FAST, false));
        assert!(cfg.near_miss(1.0, RIFLE, FAST, false) > cfg.near_miss(1.0, RIFLE, 300.0, false));
        assert!(cfg.near_miss(1.0, RIFLE, FAST, false) > cfg.near_miss(1.0, RIFLE, FAST, true));
    }

    #[test]
    fn a_few_close_rounds_fill_it_and_it_stops_at_full() {
        let cfg = SuppressionConfig::default();
        let mut s = Suppression::default();
        for _ in 0..4 {
            s.add(&cfg, cfg.near_miss(1.0, RIFLE, FAST, false), 3);
        }
        assert!(s.level > 0.5, "{}", s.level);
        for _ in 0..40 {
            s.add(&cfg, cfg.near_miss(0.1, RIFLE, FAST, false), 3);
        }
        assert_eq!(s.level, 1.0);
        assert_eq!(s.last_shooter, Some(3));
    }

    #[test]
    fn it_holds_for_a_moment_and_then_wears_off_slowly() {
        let cfg = SuppressionConfig::default();
        let mut s = Suppression::default();
        s.add(&cfg, 1.0, 1);
        s.step(&cfg, cfg.hold * 0.9);
        assert_eq!(s.level, 1.0);
        s.step(&cfg, cfg.hold * 0.1);
        s.step(&cfg, 5.0);
        assert!(s.level > 0.4 && s.level < 0.6, "{}", s.level);
        s.step(&cfg, 6.0);
        assert_eq!(s, Suppression::default());
    }

    #[test]
    fn turned_off_nothing_ever_counts() {
        let cfg = SuppressionConfig { enabled: false, ..Default::default() };
        assert_eq!(cfg.near_miss(0.1, 2.0, FAST, false), 0.0);
    }
}
