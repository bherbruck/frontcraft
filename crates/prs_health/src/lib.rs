//! Being hurt. Knows about health, bleeding and wounds, and nothing about guns, bodies or bullets:
//! the sim hands it a body part and how hard the hit was, and reads back what that does to the soldier.
//!
//! A hit takes health and starts bleeding. Bleeding drains health until someone stops it. At no health
//! a soldier is down: on the ground, unable to act, bleeding out, and able to be revived. Very hard
//! hits and bad head shots skip all that. A wound never makes a gun less accurate as such. It only
//! gives the sim numbers for how much the gun shakes and how far the legs carry.

/// Which part of the body a bullet found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Head,
    Chest,
    Abdomen,
    Arm,
    Leg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Life {
    Up,
    Down,
    Dead,
}

/// What each hit on one part of the body does.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct ZoneConfig {
    /// Times the bullet's damage.
    pub damage: f32,
    /// Health per second added to the bleeding.
    pub bleed: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HealthConfig {
    pub max_health: f32,
    pub head: ZoneConfig,
    pub chest: ZoneConfig,
    pub abdomen: ZoneConfig,
    pub arm: ZoneConfig,
    pub leg: ZoneConfig,
    /// Bleeding never gets faster than this, in health per second.
    pub max_bleed: f32,
    /// A head hit this hard, after the zone's multiplier, kills outright.
    pub head_kill_damage: f32,
    /// A hit taking health this far below zero kills instead of downing.
    pub overkill: f32,
    /// Seconds a downed soldier lasts unless someone helps.
    pub down_time: f32,
    /// How much slower a bandaged soldier bleeds out while down.
    pub down_bandaged_scale: f32,
    /// Hit this hard while down, a soldier dies. Any lesser hit only costs time.
    pub down_kill_damage: f32,
    /// Seconds off the clock per point of damage below `down_kill_damage`.
    pub down_time_per_damage: f32,
    /// Damage to an arm or leg that makes for a full wound.
    pub limb_full_wound: f32,
    /// Below this share of health there is blood loss to feel.
    pub blood_loss_from: f32,
    /// Most of a soldier's speed that a full leg wound takes.
    pub leg_slow: f32,
    /// A leg wound this bad means no more sprinting.
    pub leg_no_sprint: f32,
    /// Seconds to apply a bandage.
    pub bandage_time: f32,
    /// Seconds to use a medkit on someone who is up.
    pub medkit_time: f32,
    /// Seconds to bring round someone who is down.
    pub revive_time: f32,
    pub medkit_health: f32,
    /// How much of a limb wound a medkit takes away.
    pub medkit_wound_heal: f32,
    /// What a soldier gets up with when revived.
    pub revive_health: f32,
    /// Seconds between dying and coming back.
    pub respawn_time: f32,
    /// How near a patient has to be to treat them, in metres.
    pub care_reach: f32,
    /// How square-on you have to be to them, as the cosine of the angle off your look.
    pub care_facing: f32,
    /// Moving faster than this stops a treatment.
    pub care_max_speed: f32,
    /// Whether the bleeding waits while a bandage goes on, so nobody goes down halfway through one.
    pub bandage_holds_bleeding: bool,
    /// Whether a bandage on yourself carries on as you walk, at the pace of aiming, with `care_max_speed`
    /// left for treating someone else or using a medkit.
    pub bandage_on_the_move: bool,
    /// Seconds a bullet flies before it can hit whoever fired it.
    pub self_hit_grace: f32,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            max_health: 100.0,
            head: ZoneConfig { damage: 4.0, bleed: 5.0 },
            chest: ZoneConfig { damage: 1.0, bleed: 3.5 },
            abdomen: ZoneConfig { damage: 0.85, bleed: 3.0 },
            arm: ZoneConfig { damage: 0.4, bleed: 1.0 },
            leg: ZoneConfig { damage: 0.5, bleed: 1.5 },
            max_bleed: 8.0,
            head_kill_damage: 100.0,
            overkill: 45.0,
            down_time: 60.0,
            down_bandaged_scale: 0.5,
            down_kill_damage: 40.0,
            down_time_per_damage: 0.4,
            limb_full_wound: 30.0,
            blood_loss_from: 0.8,
            leg_slow: 0.45,
            leg_no_sprint: 0.3,
            bandage_time: 4.0,
            medkit_time: 8.0,
            revive_time: 10.0,
            medkit_health: 60.0,
            medkit_wound_heal: 0.7,
            revive_health: 30.0,
            respawn_time: 15.0,
            care_reach: 2.2,
            care_facing: 0.3,
            care_max_speed: 0.4,
            bandage_holds_bleeding: true,
            bandage_on_the_move: true,
            self_hit_grace: 0.25,
        }
    }
}

impl HealthConfig {
    pub fn zone(&self, zone: Zone) -> &ZoneConfig {
        match zone {
            Zone::Head => &self.head,
            Zone::Chest => &self.chest,
            Zone::Abdomen => &self.abdomen,
            Zone::Arm => &self.arm,
            Zone::Leg => &self.leg,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HealthEvent {
    BleedingStarted,
    BleedingStopped,
    ArmWounded,
    LegWounded,
    Downed,
    Died,
    Revived,
    Healed,
}

/// How a hit went, once the body part has had its say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitResult {
    pub damage: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Health {
    pub life: Life,
    pub health: f32,
    /// Health per second draining away. 0 once bandaged.
    pub bleed: f32,
    /// 0 to 1.
    pub arm_wound: f32,
    pub leg_wound: f32,
    /// Seconds left of being down, or until respawning when dead.
    pub clock: f32,
}

impl Health {
    pub fn new(cfg: &HealthConfig) -> Self {
        Self { life: Life::Up, health: cfg.max_health, bleed: 0.0, arm_wound: 0.0, leg_wound: 0.0, clock: 0.0 }
    }

    pub fn is_up(&self) -> bool {
        self.life == Life::Up
    }

    pub fn is_dead(&self) -> bool {
        self.life == Life::Dead
    }

    pub fn bleeding(&self) -> bool {
        self.bleed > 0.0
    }

    /// How much blood is gone, as the gun should feel it. 0 until there is enough loss to notice.
    pub fn blood_loss(&self, cfg: &HealthConfig) -> f32 {
        let from = cfg.blood_loss_from * cfg.max_health;
        ((from - self.health) / from.max(1e-3)).clamp(0.0, 1.0)
    }

    /// How long until the blood running out puts this soldier down, if it is running out.
    pub fn seconds_until_down(&self) -> Option<f32> {
        seconds_until_down(self.life, self.health, self.bleed)
    }

    /// Share of speed the legs have lost.
    pub fn slowed(&self, cfg: &HealthConfig) -> f32 {
        self.leg_wound * cfg.leg_slow
    }

    pub fn can_sprint(&self, cfg: &HealthConfig) -> bool {
        self.leg_wound < cfg.leg_no_sprint
    }

    /// Whether a bandage would do anything.
    pub fn wants_bandage(&self) -> bool {
        !self.is_dead() && self.bleeding()
    }

    /// Whether a medkit would do anything.
    pub fn wants_medkit(&self, cfg: &HealthConfig) -> bool {
        match self.life {
            Life::Dead => false,
            Life::Down => true,
            Life::Up => self.health < cfg.max_health || self.arm_wound > 0.0 || self.leg_wound > 0.0,
        }
    }

    /// A bullet arrives. `raw` is its damage before the body part it landed on.
    pub fn hit(&mut self, cfg: &HealthConfig, zone: Zone, raw: f32, events: &mut Vec<HealthEvent>) -> HitResult {
        let z = cfg.zone(zone);
        let damage = raw * z.damage;
        if self.is_dead() {
            return HitResult { damage };
        }
        if self.life == Life::Down {
            if damage >= cfg.down_kill_damage {
                self.die(cfg, events);
            } else {
                self.clock -= damage * cfg.down_time_per_damage;
            }
            return HitResult { damage };
        }

        self.health -= damage;
        if !self.bleeding() {
            events.push(HealthEvent::BleedingStarted);
        }
        self.bleed = (self.bleed + z.bleed).min(cfg.max_bleed);
        let wound = damage / cfg.limb_full_wound.max(1e-3);
        match zone {
            Zone::Arm => {
                self.arm_wound = (self.arm_wound + wound).min(1.0);
                events.push(HealthEvent::ArmWounded);
            }
            Zone::Leg => {
                self.leg_wound = (self.leg_wound + wound).min(1.0);
                events.push(HealthEvent::LegWounded);
            }
            _ => {}
        }

        let outright = (zone == Zone::Head && damage >= cfg.head_kill_damage) || self.health <= -cfg.overkill;
        if outright {
            self.die(cfg, events);
        } else if self.health <= 0.0 {
            self.go_down(cfg, events);
        }
        HitResult { damage }
    }

    /// Time passes: bleeding drains health, and the clock runs. Returns true when a respawn is due.
    /// A knock with nothing sharp in it, like a crash or a fall: it takes health and opens no wound.
    pub fn knock(&mut self, cfg: &HealthConfig, raw: f32, events: &mut Vec<HealthEvent>) {
        if self.is_dead() {
            return;
        }
        if self.life == Life::Down {
            self.clock -= raw * cfg.down_time_per_damage;
            return;
        }
        self.health -= raw;
        if self.health <= -cfg.overkill {
            self.die(cfg, events);
        } else if self.health <= 0.0 {
            self.go_down(cfg, events);
        }
    }

    pub fn step(&mut self, cfg: &HealthConfig, dt: f32, events: &mut Vec<HealthEvent>) -> bool {
        self.step_held(cfg, dt, false, events)
    }

    /// As [`Self::step`], with `held` saying someone is pressing a dressing on, which stops the bleeding from
    /// counting for as long as they do. It is still there if they stop.
    pub fn step_held(&mut self, cfg: &HealthConfig, dt: f32, held: bool, events: &mut Vec<HealthEvent>) -> bool {
        match self.life {
            Life::Up => {
                if !held {
                    self.health -= self.bleed * dt;
                }
                if self.health <= 0.0 {
                    self.go_down(cfg, events);
                }
            }
            Life::Down => {
                let rate = if self.bleeding() && !held { 1.0 } else { cfg.down_bandaged_scale };
                self.clock -= rate * dt;
                if self.clock <= 0.0 {
                    self.die(cfg, events);
                }
            }
            Life::Dead => {
                self.clock -= dt;
                return self.clock <= 0.0;
            }
        }
        false
    }

    pub fn bandage(&mut self, events: &mut Vec<HealthEvent>) {
        if self.bleeding() {
            self.bleed = 0.0;
            events.push(HealthEvent::BleedingStopped);
        }
    }

    /// Stops the bleeding, restores health and takes most of the wounds away. Gets someone who is
    /// down back up, with not much health to spare.
    pub fn medkit(&mut self, cfg: &HealthConfig, events: &mut Vec<HealthEvent>) {
        self.bandage(events);
        let heal = 1.0 - cfg.medkit_wound_heal;
        self.arm_wound *= heal;
        self.leg_wound *= heal;
        match self.life {
            Life::Dead => {}
            Life::Down => {
                self.life = Life::Up;
                self.health = cfg.revive_health;
                self.clock = 0.0;
                events.push(HealthEvent::Revived);
            }
            Life::Up => {
                self.health = (self.health + cfg.medkit_health).min(cfg.max_health);
                events.push(HealthEvent::Healed);
            }
        }
    }

    fn go_down(&mut self, cfg: &HealthConfig, events: &mut Vec<HealthEvent>) {
        self.life = Life::Down;
        self.health = 0.0;
        self.clock = cfg.down_time;
        events.push(HealthEvent::Downed);
    }

    fn die(&mut self, cfg: &HealthConfig, events: &mut Vec<HealthEvent>) {
        self.life = Life::Dead;
        self.health = 0.0;
        self.bleed = 0.0;
        self.clock = cfg.respawn_time;
        events.push(HealthEvent::Died);
    }
}

/// How long this much health lasts at this bleed, for anyone holding the numbers and not a whole [`Health`].
pub fn seconds_until_down(life: Life, health: f32, bleed: f32) -> Option<f32> {
    (life == Life::Up && bleed > 0.0).then(|| health.max(0.0) / bleed)
}

/// How bad a bleed is, by how long it leaves you. Each is meant to be told from the others at a glance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum BleedSeverity {
    #[default]
    None,
    Light,
    Heavy,
    Critical,
}

/// How one severity shows.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BleedBand {
    /// How strong the red at the edge of the screen is at the top of a beat, from 0 to 1.
    pub strength: f32,
    /// How big the frame of red is drawn against the screen, where smaller comes further in. Never below 1.
    pub frame: f32,
    /// How fast it beats. Keep it well under 180, or it stops being a pulse and starts being a strobe.
    pub beats_per_minute: f32,
    /// How loud each heartbeat is, from 0 to 1.
    pub heartbeat_volume: f32,
    /// The colour of the bar and the words, as red, green and blue.
    pub colour: [f32; 3],
}

/// How losing blood looks and sounds. It is the player's own view of the model and no part of the rules,
/// so a client can change all of this without a server minding.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BleedLook {
    /// With this many seconds or fewer until going down, a bleed is heavy, and then critical.
    pub heavy_within: f32,
    pub critical_within: f32,
    pub light: BleedBand,
    pub heavy: BleedBand,
    pub critical: BleedBand,
    /// The colour of the frame at the edge of the screen, at every severity, as red, green and blue.
    pub edge_colour: [f32; 3],
    /// How square the frame is. Two is a circle, and more follows the edges of the screen more closely.
    pub edge_squareness: f32,
    /// From this share of the way out to the edge of the frame the red starts, and at the edge it is full.
    pub edge_starts: f32,
    /// How far the red falls between beats, from 0 for not at all to 1 for all the way.
    pub pulse_depth: f32,
    /// How fast what is shown follows what is happening, per second, going up and going down.
    pub rise_rate: f32,
    pub fall_rate: f32,
    /// How much of the red is gone by the time a bandage is nearly on, from 0 to 1.
    pub bandage_relief: f32,
    /// Seconds the readout says the bleeding has stopped.
    pub stopped_note_seconds: f32,
}

impl Default for BleedLook {
    fn default() -> Self {
        Self {
            heavy_within: 40.0,
            critical_within: 15.0,
            light: BleedBand { strength: 0.4, frame: 1.16, beats_per_minute: 56.0, heartbeat_volume: 0.12, colour: [1.0, 0.82, 0.55] },
            heavy: BleedBand { strength: 0.6, frame: 1.08, beats_per_minute: 84.0, heartbeat_volume: 0.3, colour: [1.0, 0.56, 0.3] },
            critical: BleedBand { strength: 0.85, frame: 1.0, beats_per_minute: 112.0, heartbeat_volume: 0.55, colour: [1.0, 0.32, 0.25] },
            edge_colour: [0.85, 0.03, 0.02],
            edge_squareness: 5.0,
            edge_starts: 0.62,
            pulse_depth: 0.45,
            rise_rate: 4.0,
            fall_rate: 2.0,
            bandage_relief: 0.65,
            stopped_note_seconds: 3.0,
        }
    }
}

impl BleedLook {
    /// `seconds_left` is from [`Health::seconds_until_down`].
    pub fn severity(&self, seconds_left: Option<f32>) -> BleedSeverity {
        match seconds_left {
            None => BleedSeverity::None,
            Some(s) if s <= self.critical_within => BleedSeverity::Critical,
            Some(s) if s <= self.heavy_within => BleedSeverity::Heavy,
            Some(_) => BleedSeverity::Light,
        }
    }

    pub fn band(&self, severity: BleedSeverity) -> Option<&BleedBand> {
        match severity {
            BleedSeverity::None => None,
            BleedSeverity::Light => Some(&self.light),
            BleedSeverity::Heavy => Some(&self.heavy),
            BleedSeverity::Critical => Some(&self.critical),
        }
    }
}

/// Seconds as a person would say them: nobody needs to know 37 from 38, and everybody needs to know 4.
pub fn rough_seconds(seconds: f32) -> u32 {
    let whole = seconds.max(0.0).ceil() as u32;
    match whole {
        0..=10 => whole,
        11..=60 => whole.div_ceil(5) * 5,
        _ => whole.div_ceil(10) * 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(h: &mut Health, cfg: &HealthConfig, seconds: f32, events: &mut Vec<HealthEvent>) {
        for _ in 0..(seconds / DT) as usize {
            h.step(cfg, DT, events);
        }
    }

    #[test]
    fn how_long_is_left_is_the_blood_over_the_rate_and_only_while_it_runs() {
        let cfg = HealthConfig::default();
        let mut h = Health::new(&cfg);
        assert_eq!(h.seconds_until_down(), None, "not bleeding");
        h.health = 60.0;
        h.bleed = 3.0;
        assert_eq!(h.seconds_until_down(), Some(20.0));
        // And it is right: running the model for that long is what puts them down.
        let mut events = Vec::new();
        run(&mut h, &cfg, 19.5, &mut events);
        assert!(h.is_up());
        run(&mut h, &cfg, 1.0, &mut events);
        assert_eq!(h.life, Life::Down);
        assert_eq!(h.seconds_until_down(), None, "down has a clock of its own");
        h.life = Life::Up;
        h.bandage(&mut events);
        assert_eq!(h.seconds_until_down(), None, "a bandage ends it");
    }

    /// What real wounds come out as with the real numbers: an arm or leg can wait, a gut shot should not
    /// and a chest shot cannot.
    #[test]
    fn real_wounds_land_in_the_severity_you_would_hope() {
        let (cfg, look) = (HealthConfig::default(), BleedLook::default());
        let after = |zone: Zone, raw: f32| {
            let mut h = Health::new(&cfg);
            h.hit(&cfg, zone, raw, &mut Vec::new());
            look.severity(h.seconds_until_down())
        };
        const RIFLE: f32 = 55.0;
        assert_eq!(after(Zone::Arm, RIFLE), BleedSeverity::Light);
        assert_eq!(after(Zone::Leg, RIFLE), BleedSeverity::Light);
        assert_eq!(after(Zone::Abdomen, RIFLE), BleedSeverity::Heavy);
        assert_eq!(after(Zone::Chest, RIFLE), BleedSeverity::Critical);
    }

    #[test]
    fn a_light_bleed_left_alone_gets_worse_on_its_own() {
        let (cfg, look) = (HealthConfig::default(), BleedLook::default());
        let mut h = Health::new(&cfg);
        h.hit(&cfg, Zone::Leg, 20.0, &mut Vec::new());
        let mut seen = vec![look.severity(h.seconds_until_down())];
        let mut events = Vec::new();
        while h.is_up() {
            h.step(&cfg, DT, &mut events);
            let now = look.severity(h.seconds_until_down());
            if h.is_up() && seen.last() != Some(&now) {
                seen.push(now);
            }
        }
        assert_eq!(seen, [BleedSeverity::Light, BleedSeverity::Heavy, BleedSeverity::Critical]);
    }

    #[test]
    fn severities_change_exactly_at_their_limits() {
        let look = BleedLook::default();
        assert_eq!(look.severity(None), BleedSeverity::None);
        assert_eq!(look.severity(Some(look.heavy_within + 0.1)), BleedSeverity::Light);
        assert_eq!(look.severity(Some(look.heavy_within)), BleedSeverity::Heavy);
        assert_eq!(look.severity(Some(look.critical_within + 0.1)), BleedSeverity::Heavy);
        assert_eq!(look.severity(Some(look.critical_within)), BleedSeverity::Critical);
        assert_eq!(look.severity(Some(0.0)), BleedSeverity::Critical);
        assert!(look.band(BleedSeverity::None).is_none());
    }

    /// The three have to be obviously different in everything, and none of them may strobe.
    #[test]
    fn each_severity_is_plainly_worse_than_the_last_and_none_of_them_strobes() {
        let look = BleedLook::default();
        let bands = [look.light, look.heavy, look.critical];
        for pair in bands.windows(2) {
            assert!(pair[1].strength > pair[0].strength * 1.4, "{pair:?}");
            assert!(pair[1].beats_per_minute > pair[0].beats_per_minute * 1.2, "{pair:?}");
            assert!(pair[1].heartbeat_volume > pair[0].heartbeat_volume, "{pair:?}");
            assert!(pair[1].frame < pair[0].frame, "{pair:?}");
            assert_ne!(pair[1].colour, pair[0].colour);
        }
        for band in bands {
            // Under two beats a second, and the red never goes from nothing to everything.
            assert!(band.beats_per_minute < 120.0, "{band:?}");
            assert!(band.frame >= 1.0, "{band:?}");
            assert!(band.strength <= 1.0);
        }
        assert!(look.pulse_depth < 0.6, "a beat swells and never flashes");
    }

    #[test]
    fn a_time_is_given_as_roughly_as_it_needs_to_be() {
        assert_eq!(rough_seconds(0.0), 0);
        assert_eq!(rough_seconds(3.2), 4);
        assert_eq!(rough_seconds(9.5), 10);
        assert_eq!(rough_seconds(10.5), 15);
        assert_eq!(rough_seconds(37.0), 40);
        assert_eq!(rough_seconds(60.0), 60);
        assert_eq!(rough_seconds(61.0), 70);
        assert_eq!(rough_seconds(-3.0), 0);
    }

    #[test]
    fn a_hit_to_the_body_hurts_and_starts_bleeding() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Chest, 55.0, &mut events);
        assert!(h.is_up());
        assert_eq!(h.health, cfg.max_health - 55.0);
        assert!(h.bleeding());
        assert_eq!(events, vec![HealthEvent::BleedingStarted]);
    }

    #[test]
    fn the_same_bullet_is_worth_more_in_the_head_and_less_in_a_limb() {
        let cfg = HealthConfig::default();
        let mut events = Vec::new();
        let damage = |zone| Health::new(&cfg).hit(&cfg, zone, 50.0, &mut Vec::new()).damage;
        assert!(damage(Zone::Head) > damage(Zone::Chest));
        assert!(damage(Zone::Chest) > damage(Zone::Abdomen));
        assert!(damage(Zone::Abdomen) > damage(Zone::Leg));
        assert!(damage(Zone::Leg) >= damage(Zone::Arm));

        let mut h = Health::new(&cfg);
        h.hit(&cfg, Zone::Head, 50.0, &mut events);
        assert!(h.is_dead(), "a rifle round to the head should kill outright");
    }

    #[test]
    fn enough_damage_puts_you_down_rather_than_dead_and_a_harder_hit_kills() {
        let cfg = HealthConfig::default();
        let mut events = Vec::new();
        let mut h = Health::new(&cfg);
        h.hit(&cfg, Zone::Chest, 55.0, &mut events);
        h.hit(&cfg, Zone::Chest, 55.0, &mut events);
        assert_eq!(h.life, Life::Down);
        assert!(events.contains(&HealthEvent::Downed));

        let mut h = Health::new(&cfg);
        h.health = 10.0;
        h.hit(&cfg, Zone::Chest, 60.0, &mut events);
        assert!(h.is_dead(), "that far below zero is no longer survivable");
    }

    #[test]
    fn bleeding_takes_you_down_and_then_finishes_it() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Chest, 55.0, &mut events);
        run(&mut h, &cfg, (cfg.max_health - 55.0) / cfg.chest.bleed + 0.1, &mut events);
        assert_eq!(h.life, Life::Down);
        run(&mut h, &cfg, cfg.down_time + 0.1, &mut events);
        assert!(h.is_dead());
        assert!(events.contains(&HealthEvent::Died));
    }

    #[test]
    fn a_bandage_stops_the_bleeding_and_nothing_else() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Leg, 55.0, &mut events);
        let (health, wound) = (h.health, h.leg_wound);
        h.bandage(&mut events);
        assert!(!h.bleeding());
        run(&mut h, &cfg, 30.0, &mut events);
        assert_eq!((h.health, h.leg_wound), (health, wound));
    }

    #[test]
    fn bandaging_someone_who_is_down_buys_them_time() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Chest, 120.0, &mut events);
        assert_eq!(h.life, Life::Down);
        h.bandage(&mut events);
        run(&mut h, &cfg, cfg.down_time + 1.0, &mut events);
        assert_eq!(h.life, Life::Down, "a bandaged soldier should outlast the normal clock");
    }

    #[test]
    fn a_medkit_revives_with_little_health_and_heals_someone_who_is_up() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Chest, 120.0, &mut events);
        h.medkit(&cfg, &mut events);
        assert!(h.is_up());
        assert_eq!(h.health, cfg.revive_health);
        assert!(!h.bleeding());
        h.medkit(&cfg, &mut events);
        assert_eq!(h.health, cfg.revive_health + cfg.medkit_health);
    }

    #[test]
    fn nothing_helps_the_dead() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Head, 200.0, &mut events);
        h.medkit(&cfg, &mut events);
        assert!(h.is_dead());
        assert!(!h.wants_medkit(&cfg));
        assert!(!h.wants_bandage());
    }

    #[test]
    fn the_clock_after_dying_says_when_to_come_back() {
        let cfg = HealthConfig::default();
        let (mut h, mut events) = (Health::new(&cfg), Vec::new());
        h.hit(&cfg, Zone::Head, 200.0, &mut events);
        assert!(!h.step(&cfg, cfg.respawn_time * 0.5, &mut events));
        assert!(h.step(&cfg, cfg.respawn_time * 0.6, &mut events));
    }

    #[test]
    fn a_wounded_leg_slows_and_then_stops_sprinting_and_blood_loss_starts_late() {
        let cfg = HealthConfig::default();
        let mut h = Health::new(&cfg);
        assert_eq!(h.slowed(&cfg), 0.0);
        assert!(h.can_sprint(&cfg));
        h.hit(&cfg, Zone::Leg, 30.0, &mut Vec::new());
        assert!(h.slowed(&cfg) > 0.1);
        assert!(!h.can_sprint(&cfg));
        h.health = cfg.max_health * 0.9;
        assert_eq!(h.blood_loss(&cfg), 0.0);
        h.health = cfg.max_health * 0.4;
        assert!(h.blood_loss(&cfg) > 0.3);
    }
}
