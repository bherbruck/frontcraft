//! Tuning data. Angles are degrees and times are seconds throughout.

use glam::{Vec2, Vec3};
use prs_ballistics::BulletConfig;
use std::borrow::Cow;
use std::ops::Index;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FireMode {
    Semi,
    Auto,
}

/// A row in the [`WeaponTable`]. Plain data, so it can go over the network as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WeaponId(pub u16);

/// Every gun in the game. Soldiers refer to a row, so what a gun does lives here and nowhere else.
#[derive(Clone, Debug)]
pub struct WeaponTable {
    pub weapons: Vec<WeaponConfig>,
}

/// In a mod a gun goes by its `key`, as `weapons.m4`. Its place in the list is its [`WeaponId`], which
/// is why a table keeps the order it was written in.
impl serde::Serialize for WeaponTable {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.weapons.iter().map(|w| (w.key.as_ref(), w)))
    }
}

impl<'de> serde::Deserialize<'de> for WeaponTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Guns;
        impl<'de> serde::de::Visitor<'de> for Guns {
            type Value = WeaponTable;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a table of guns by name")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<WeaponTable, A::Error> {
                let mut weapons = Vec::new();
                while let Some((key, mut gun)) = map.next_entry::<String, WeaponConfig>()? {
                    gun.key = key.into();
                    weapons.push(gun);
                }
                Ok(WeaponTable { weapons })
            }
        }
        d.deserialize_map(Guns)
    }
}

impl WeaponTable {
    pub const ASSAULT_RIFLE: WeaponId = WeaponId(0);
    pub const MACHINE_GUN: WeaponId = WeaponId(1);
    pub const MARKSMAN_RIFLE: WeaponId = WeaponId(2);
    pub const PISTOL: WeaponId = WeaponId(3);

    pub fn ids(&self) -> impl Iterator<Item = WeaponId> + use<> {
        (0..self.weapons.len() as u16).map(WeaponId)
    }
}

impl Default for WeaponTable {
    fn default() -> Self {
        Self {
            weapons: vec![
                WeaponConfig::assault_rifle(),
                WeaponConfig::machine_gun(),
                WeaponConfig::marksman_rifle(),
                WeaponConfig::pistol(),
            ],
        }
    }
}

impl Index<WeaponId> for WeaponTable {
    type Output = WeaponConfig;
    fn index(&self, id: WeaponId) -> &WeaponConfig {
        &self.weapons[id.0 as usize]
    }
}

/// Which low-poly shape a gun is drawn with. The numbers on it come from its [`HoldConfig`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GunShape {
    Carbine,
    MachineGun,
    MarksmanRifle,
    Pistol,
}

/// The character of a gun's report. The synth renders each shot from this.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReportConfig {
    /// Pitch of the chest thump, hertz. Bigger rounds thump lower.
    pub thump_hz: f32,
    /// How fast the muzzle blast dies away, per second.
    pub blast_rate: f32,
    /// How fast the tail dies away, per second.
    pub tail_rate: f32,
    pub tail_level: f32,
    /// Sharpness of the supersonic crack. Subsonic rounds have little.
    pub crack_level: f32,
}

/// The burst at the muzzle. Starts at the muzzle and blooms forward, so nothing of it is ever behind
/// the end of the barrel.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct FlashConfig {
    /// Metres forward from the muzzle.
    pub length: f32,
    /// Metres across at its widest.
    pub width: f32,
    /// Seconds on screen.
    pub time: f32,
    /// Lumens of the light it throws on what is around it: a quick warm flicker on nearby surfaces,
    /// so a wall a metre or two away lights up and does not go white.
    pub light: f32,
    /// Metres that light reaches.
    pub light_range: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WeaponConfig {
    /// What a mod calls it.
    #[serde(skip)]
    pub key: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub shape: GunShape,
    pub report: ReportConfig,
    pub flash: FlashConfig,
    /// Kilograms, unloaded.
    pub weight: f32,
    /// Kilograms per round carried, belt or magazine included.
    pub round_weight: f32,
    /// How much a passing round rattles whoever it just missed, with a rifle round being 1.
    pub round_size: f32,
    /// How hard a round hits at muzzle velocity, before the body part it lands on.
    pub damage: f32,
    /// How damage falls with speed: `damage * (speed / muzzle_velocity) ^ damage_falloff`.
    pub damage_falloff: f32,
    pub ballistics: BulletConfig,
    /// Metres at which the sights and the bullet meet.
    pub zero_range: f32,
    pub rounds_per_minute: f32,
    /// Mechanical accuracy: the diameter of the shot group, in minutes of angle. A fixed property of
    /// the weapon and ammunition. Firing, moving and stance never change it.
    pub dispersion_moa: f32,
    pub magazine: u32,
    pub spare_magazines: u32,
    pub modes: Vec<FireMode>,
    pub default_mode: FireMode,
    pub reload_time: f32,
    pub reload_empty_time: f32,
    pub ads_time: f32,
    pub raise_time: f32,
    pub lower_time: f32,
    /// Bringing the gun down to change to something else.
    pub stow_time: f32,
    /// Bringing it up when it has just been drawn.
    pub draw_time: f32,
    pub hold: HoldConfig,
    #[serde(default)]
    pub rest: RestConfig,
    pub recoil: RecoilConfig,
    pub stability: StabilityConfig,
    pub sway: SwayConfig,
}

/// How the gun is carried and how big it is, in metres from the eye in the look direction's frame:
/// Setting a gun down on something. All of it per gun, so a mod can make it instant or take it away.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RestConfig {
    /// How far along the gun from the receiver to the muzzle it can touch, as shares of that, and how
    /// many places to look between.
    pub near: f32,
    pub far: f32,
    pub samples: u32,
    /// How far past the barrel at the body's highest and lowest something can be and still be found.
    pub above: f32,
    pub below: f32,
    /// How far under the bore the underside of the fore-end is, and how far that sits above the contact.
    pub under: f32,
    pub clearance: f32,
    /// How level it has to be.
    pub flat_cos: f32,
    /// How high the eye gets over the feet at its lowest, kneeling, and lying down at its highest. Standing
    /// tall is the top otherwise, and `MoveConfig::prone_eye` the bottom for someone lying down.
    pub low_eye: f32,
    pub prone_high_eye: f32,
    /// How far round the feet to look for something that stops a knee going down, how far above the feet
    /// it counts from, and how much room to keep from a ceiling.
    pub knee_reach: f32,
    pub knee_rise: f32,
    pub head_room: f32,
    /// How far along past the contact the barrel has to be clear of what it is on, how far to look between
    /// places, how far up something can be before it is not the rest but something in the way, how far
    /// down before the rest has ended, how much to clear it by, and radians over for the gun's own wander.
    pub clear_reach: f32,
    pub clear_step: f32,
    pub clear_rise: f32,
    pub clear_drop: f32,
    pub clear_margin: f32,
    pub clear_wander: f32,
    /// How far over the bore to look back along the gun for a window frame or the like. The part of the
    /// gun behind the contact swings out sideways, and it may go over what it is resting on.
    pub frame_lift: f32,
    /// Radians past the body's or the support's limit before it comes off, for a client that lets it.
    pub limit_slack: f32,
    /// Radians a second a look left outside where it can be is drawn back.
    pub pull_rate: f32,
    /// How far the barrel can swing off the way it went down, radians, before it comes off.
    pub arc_yaw: f32,
    pub arc_up: f32,
    pub arc_down: f32,
    /// For a client stopping the look at the arc rather than going past it: how far inside the arc it
    /// stops, how far from there the look starts to drag, and the share of the mouse left at the very end.
    pub arc_margin: f32,
    pub arc_soft: f32,
    pub arc_drag: f32,
    /// How far from the upright eye the eye will go to stay behind the sights, and the room it keeps.
    pub max_eye_shift: f32,
    pub eye_clearance: f32,
    /// How far they can have moved, how fast they can be going to set it down, and how far either side
    /// of the contact to look for it still being there.
    pub max_drift: f32,
    pub max_speed: f32,
    pub support_gap: f32,
    /// Seconds to get over to it or back off it, and to settle once there.
    pub ease_time: f32,
    pub settle_time: f32,
    /// Seconds more for each metre the head has to go, and the fastest it ever goes, metres a second. A
    /// swing faster than that leaves the gun a little off the contact until the head catches up.
    pub ease_per_metre: f32,
    pub eye_speed: f32,
    /// How much of the settling each shot knocks back.
    pub shot_unsettles: f32,
    /// What is left, fully settled, of sway, wander, kick and what firing does to the wander.
    pub steady_sway: f32,
    pub steady_wander: f32,
    pub steady_recoil: f32,
    pub steady_shake: f32,
    /// Seconds to say that there was nothing to rest on.
    pub refused_time: f32,
}

/// Where a look can go on a rest, right now, at some yaw: the arc, the body and what is under the gun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RestArc {
    pub yaw_low: f32,
    pub yaw_high: f32,
    pub pitch_low: f32,
    pub pitch_high: f32,
}

impl RestConfig {
    /// Where a look wanting to go from `now` to `wanted` gets to, all as x = yaw and y = pitch. It drags near
    /// the end and goes no further, never past what the sim allows, and coming back there is nothing to
    /// unwind, as what was asked past the end has not been kept. If the end has moved past the look, the
    /// look is drawn back to it at `pull_rate` rather than all at once, or the view would jump.
    pub fn stop_look(&self, arc: &RestArc, now: Vec2, wanted: Vec2, dt: f32) -> Vec2 {
        let axis = |now: f32, wanted: f32, low: f32, high: f32| {
            let (low, high) = (low + self.arc_margin, (high - self.arc_margin).max(low + self.arc_margin));
            let pull = self.pull_rate * dt;
            if now < low {
                return wanted.max(now + pull).min(low.max(wanted.min(high)));
            }
            if now > high {
                return wanted.min(now - pull).max(high.min(wanted.max(low)));
            }
            let room = if wanted > now { high - now } else { now - low };
            let share = (room / self.arc_soft.max(1e-4)).clamp(self.arc_drag, 1.0);
            (now + (wanted - now) * share).clamp(low, high)
        };
        Vec2::new(axis(now.x, wanted.x, arc.yaw_low, arc.yaw_high), axis(now.y, wanted.y, arc.pitch_low, arc.pitch_high))
    }
}

impl Default for RestConfig {
    fn default() -> Self {
        Self {
            near: 0.1,
            far: 0.7,
            samples: 8,
            above: 0.04,
            below: 0.12,
            under: 0.03,
            clearance: 0.03,
            flat_cos: 0.7,
            low_eye: 0.92,
            prone_high_eye: 0.6,
            knee_reach: 0.3,
            knee_rise: 0.15,
            head_room: 0.02,
            clear_reach: 2.6,
            clear_step: 0.06,
            clear_rise: 0.22,
            clear_drop: 0.5,
            clear_margin: 0.015,
            clear_wander: 0.008,
            frame_lift: 0.1,
            limit_slack: 0.03,
            pull_rate: 0.9,
            arc_yaw: 0.6,
            arc_up: 0.55,
            arc_down: 0.55,
            arc_margin: 0.002,
            arc_soft: 0.07,
            arc_drag: 0.2,
            max_eye_shift: 0.75,
            eye_clearance: 0.09,
            max_drift: 0.25,
            max_speed: 1.0,
            support_gap: 0.04,
            ease_time: 0.45,
            settle_time: 0.9,
            ease_per_metre: 0.9,
            eye_speed: 1.4,
            shot_unsettles: 0.18,
            steady_sway: 0.3,
            steady_wander: 0.4,
            steady_recoil: 0.5,
            steady_shake: 0.5,
            refused_time: 1.5,
        }
    }
}

/// x = right, y = up, -z = forward. The gun's origin is its receiver.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HoldConfig {
    pub hip: Vec3,
    pub ads: Vec3,
    /// Added to the hold when the gun is lowered for sprinting or by an obstacle.
    pub lowered_offset: Vec3,
    /// Radians: x = pitch, y = yaw, z = roll.
    pub lowered_rotation: Vec3,
    /// Where it ends up lying next to someone who has gone down, added to the hip hold.
    pub fallen_offset: Vec3,
    /// How it lies there, instead of how it is lowered.
    pub fallen_rotation: Vec3,
    /// Where it hangs with both hands off it, added to the hip hold, and how.
    #[serde(default)]
    pub slung_offset: Vec3,
    #[serde(default)]
    pub slung_rotation: Vec3,
    /// Slung round onto the back instead, for leaning into something: from the eye along the ground's level
    /// rather than the look, and how it lies there.
    #[serde(default)]
    pub back_offset: Vec3,
    #[serde(default)]
    pub back_rotation: Vec3,
    /// Receiver to muzzle.
    pub muzzle_forward: f32,
    /// Bore to line of sight.
    pub sight_height: f32,
    /// Receiver to the front and rear sights, along the barrel. Rear is negative: behind the receiver.
    pub front_sight: f32,
    pub rear_sight: f32,
    /// How far an obstacle can push the gun back before it has to be lowered.
    pub max_retract: f32,
    /// How far back a gun sits once it has been lowered out of the way.
    pub blocked_retract: f32,
    pub retract_speed: f32,
    /// Radians the gun rolls with the shoulders at full lean, about the line through the eye.
    pub lean_roll: f32,
    pub grips: GripConfig,
}

/// Where hands go on this gun, in its own frame: x = right, y = up, -z = towards the muzzle.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct GripConfig {
    pub trigger: Vec3,
    /// The wrist behind each hand, which sets the angle the hand comes in at.
    pub trigger_wrist: Vec3,
    pub support: Vec3,
    pub support_wrist: Vec3,
    pub magazine: Vec3,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecoilConfig {
    /// Upward sight velocity added per shot, degrees/second.
    pub kick_up: f32,
    /// Fraction of `kick_up` randomly added or removed per shot.
    pub kick_up_spread: f32,
    /// Largest sideways sight velocity added per shot, either direction.
    pub kick_side: f32,
    /// Spring rate of the sights returning, per second. Higher is snappier.
    pub recovery_rate: f32,
    /// Extra kick per unit of heat, as a fraction.
    pub heat_kick: f32,
    pub heat_kick_max: f32,
    pub heat_decay: f32,
    /// Recoil multiplier firing from the hip rather than the shoulder.
    pub hip_scale: f32,
    pub stand_scale: f32,
    pub crouch_scale: f32,
    pub prone_scale: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StabilityConfig {
    pub hip_stand: f32,
    pub hip_crouch: f32,
    pub hip_prone: f32,
    pub ads_stand: f32,
    pub ads_crouch: f32,
    pub ads_prone: f32,
    /// Added at running speed.
    pub moving: f32,
    /// Added while the rifle is coming up to the shoulder.
    pub raising: f32,
    pub airborne: f32,
    pub changing_stance: f32,
    /// Added per unit of lean a second, so a lean on its way over unsettles the gun and a lean that has
    /// arrived does not.
    pub changing_lean: f32,
    pub turning: f32,
    pub turn_rate_full: f32,
    /// Added at empty stamina.
    pub exertion: f32,
    /// Added at a fully wounded arm, which cannot hold the gun still.
    pub arm_wound: f32,
    /// Added at no blood left. Weakness shakes the real gun too.
    pub blood_loss: f32,
    /// Added at full suppression. Being shot at shakes the real gun too.
    pub suppression: f32,
    /// Instability added per shot.
    pub shot_shake: f32,
    /// Extra shot shake per unit of heat, as a fraction.
    pub heat_shake: f32,
    pub max: f32,
    pub rise_time: f32,
    /// Time constant of settling. Roughly three of these to get steady again.
    pub settle_time: f32,
    /// How fast the gun wanders when steady, radians of phase per second.
    pub wander_rate: f32,
    /// Extra wander speed per degree of instability, as a fraction, so a gun shakes faster the
    /// more unsettled it is and slows to a drift as it calms.
    pub wander_rate_unsteady: f32,
    /// How `moving` grows with speed: one is a straight line and more is gentle at a walk.
    pub moving_curve: f32,
    /// Fastest speed that counts, as a share of running.
    pub moving_max: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SwayConfig {
    pub stand: f32,
    pub crouch: f32,
    pub prone: f32,
    pub hip_scale: f32,
    pub rate: f32,
    pub exertion: f32,
    /// Extra sway at a fully wounded arm, at no blood left, and at full suppression, as fractions.
    pub arm_wound: f32,
    pub blood_loss: f32,
    pub suppression: f32,
    pub breath_steady: f32,
    pub breath_gasp: f32,
    pub breath_hold_time: f32,
    pub breath_recover_time: f32,
    pub breath_unlock: f32,
    /// How fast holding or gasping takes hold, per second.
    pub breath_rate: f32,
    /// How fast sway follows a change in what is causing it, per second.
    pub settle_rate: f32,
    /// How much faster you sway out of breath, as a fraction at empty stamina.
    pub winded_rate: f32,
}

impl WeaponConfig {
    /// Half-angle of the dispersion cone, radians.
    pub fn dispersion_radius(&self) -> f32 {
        (self.dispersion_moa / 60.0).to_radians() * 0.5
    }

    pub fn assault_rifle() -> Self {
        Self {
            key: "m4".into(),
            name: "M4 Carbine".into(),
            shape: GunShape::Carbine,
            report: ReportConfig { thump_hz: 92.0, blast_rate: 22.0, tail_rate: 4.5, tail_level: 0.34, crack_level: 0.9 },
            flash: FlashConfig { length: 0.2, width: 0.075, time: 0.045, light: 16_000.0, light_range: 10.0 },
            round_size: 1.0,
            weight: 3.4,
            round_weight: 0.012,
            damage: 55.0,
            damage_falloff: 1.5,
            ballistics: BulletConfig { muzzle_velocity: 880.0, drag: 0.0009, max_age: 4.0, gravity: 9.81 },
            zero_range: 100.0,
            rounds_per_minute: 750.0,
            dispersion_moa: 4.0,
            magazine: 30,
            spare_magazines: 5,
            modes: vec![FireMode::Semi, FireMode::Auto],
            default_mode: FireMode::Semi,
            reload_time: 2.5,
            reload_empty_time: 3.1,
            ads_time: 0.26,
            raise_time: 0.32,
            lower_time: 0.18,
            stow_time: 0.45,
            draw_time: 0.55,
            rest: RestConfig::default(),
            hold: HoldConfig {
                hip: Vec3::new(0.14, -0.145, -0.34),
                ads: Vec3::new(0.0, -0.058, -0.235),
                lowered_offset: Vec3::new(-0.05, 0.01, 0.03),
                lowered_rotation: Vec3::new(-0.1, 0.5, 0.35),
                fallen_offset: Vec3::new(0.12, -0.19, 0.0),
                fallen_rotation: Vec3::new(0.0, 0.5, 1.5),
                slung_offset: Vec3::new(-0.04, -0.3, 0.2),
                slung_rotation: Vec3::new(-1.15, 0.45, 0.5),
                back_offset: Vec3::new(0.05, -0.15, 0.4),
                back_rotation: Vec3::new(-0.55, 2.65, 0.3),
                muzzle_forward: 0.515,
                sight_height: 0.058,
                front_sight: 0.27,
                rear_sight: -0.05,
                max_retract: 0.38,
                blocked_retract: 0.5,
                retract_speed: 3.0,
                lean_roll: 0.2,
                grips: GripConfig {
                    trigger: Vec3::new(0.0, -0.075, 0.07),
                    trigger_wrist: Vec3::new(0.012, -0.094, 0.15),
                    support: Vec3::new(0.0, -0.036, -0.22),
                    support_wrist: Vec3::new(-0.03, -0.062, -0.15),
                    magazine: Vec3::new(0.0, -0.13, -0.05),
                },
            },
            recoil: RecoilConfig {
                kick_up: 26.0,
                kick_up_spread: 0.45,
                kick_side: 26.0,
                recovery_rate: 9.5,
                heat_kick: 0.32,
                heat_kick_max: 3.5,
                heat_decay: 0.4,
                hip_scale: 1.8,
                stand_scale: 1.0,
                crouch_scale: 0.78,
                prone_scale: 0.55,
            },
            stability: StabilityConfig {
                hip_stand: 0.22,
                hip_crouch: 0.18,
                hip_prone: 0.12,
                ads_stand: 0.03,
                ads_crouch: 0.02,
                ads_prone: 0.01,
                raising: 0.7,
                moving: 2.2,
                airborne: 3.0,
                changing_stance: 2.0,
                changing_lean: 0.320,
                turning: 0.3,
                turn_rate_full: 180.0,
                exertion: 0.3,
                arm_wound: 1.4,
                blood_loss: 0.9,
                suppression: 1.8,
                shot_shake: 0.4,
                heat_shake: 0.4,
                max: 7.0,
                rise_time: 0.25,
                settle_time: 0.7,
                wander_rate: 0.9,
                wander_rate_unsteady: 0.9,
                moving_curve: 1.2,
                moving_max: 1.5,
            },
            sway: SwayConfig {
                stand: 0.22,
                crouch: 0.12,
                prone: 0.04,
                hip_scale: 0.5,
                rate: 0.8,
                exertion: 2.2,
                arm_wound: 1.0,
                blood_loss: 1.5,
                suppression: 2.0,
                breath_steady: 0.18,
                breath_gasp: 1.7,
                breath_hold_time: 5.0,
                breath_recover_time: 7.0,
                breath_unlock: 0.45,
                breath_rate: 3.0,
                settle_rate: 3.0,
                winded_rate: 0.8,
            },
        }
    }

    /// Heavy with a big belt. Terrible from the hip or on your feet, and a different animal lying down
    /// even with no bipod: the ground takes the weight and most of the kick.
    pub fn machine_gun() -> Self {
        let base = Self::assault_rifle();
        Self {
            key: "m249".into(),
            name: "M249 SAW".into(),
            shape: GunShape::MachineGun,
            report: ReportConfig { thump_hz: 74.0, blast_rate: 19.0, tail_rate: 3.8, tail_level: 0.42, crack_level: 1.0 },
            flash: FlashConfig { length: 0.34, width: 0.12, time: 0.05, light: 24_000.0, light_range: 12.0 },
            round_size: 1.15,
            weight: 7.5,
            round_weight: 0.03,
            damage: 60.0,
            ballistics: BulletConfig { muzzle_velocity: 915.0, drag: 0.0008, max_age: 4.0, gravity: 9.81 },
            rounds_per_minute: 720.0,
            dispersion_moa: 6.0,
            magazine: 100,
            spare_magazines: 2,
            modes: vec![FireMode::Auto],
            default_mode: FireMode::Auto,
            reload_time: 6.5,
            reload_empty_time: 6.5,
            ads_time: 0.5,
            raise_time: 0.55,
            lower_time: 0.32,
            stow_time: 0.85,
            draw_time: 1.05,
            rest: RestConfig::default(),
            hold: HoldConfig {
                hip: Vec3::new(0.16, -0.19, -0.36),
                ads: Vec3::new(0.0, -0.075, -0.25),
                lowered_offset: Vec3::new(-0.05, -0.03, 0.03),
                lowered_rotation: Vec3::new(-0.2, 0.5, 0.35),
                fallen_offset: Vec3::new(0.12, -0.13, 0.0),
                fallen_rotation: Vec3::new(0.0, 0.5, 1.5),
                slung_offset: Vec3::new(-0.04, -0.3, 0.2),
                slung_rotation: Vec3::new(-1.15, 0.45, 0.5),
                back_offset: Vec3::new(0.05, -0.15, 0.4),
                back_rotation: Vec3::new(-0.55, 2.65, 0.3),
                muzzle_forward: 0.78,
                sight_height: 0.075,
                front_sight: 0.5,
                rear_sight: -0.02,
                max_retract: 0.5,
                blocked_retract: 0.66,
                retract_speed: 2.4,
                lean_roll: 0.14,
                grips: GripConfig {
                    trigger: Vec3::new(0.0, -0.09, 0.09),
                    trigger_wrist: Vec3::new(0.012, -0.11, 0.17),
                    support: Vec3::new(0.0, -0.052, -0.3),
                    support_wrist: Vec3::new(-0.03, -0.08, -0.23),
                    magazine: Vec3::new(0.0, -0.13, -0.08),
                },
            },
            recoil: RecoilConfig {
                kick_up: 30.0,
                kick_side: 32.0,
                recovery_rate: 9.0,
                heat_decay: 0.5,
                hip_scale: 3.2,
                stand_scale: 1.25,
                crouch_scale: 0.8,
                prone_scale: 0.2,
                ..base.recoil
            },
            stability: StabilityConfig {
                hip_stand: 0.7,
                hip_crouch: 0.45,
                hip_prone: 0.14,
                ads_stand: 0.11,
                ads_crouch: 0.06,
                ads_prone: 0.008,
                moving: 3.2,
                raising: 1.0,
                turning: 0.5,
                exertion: 0.5,
                shot_shake: 0.5,
                ..base.stability
            },
            sway: SwayConfig { stand: 0.38, crouch: 0.2, prone: 0.03, exertion: 3.0, ..base.sway },
            ..base
        }
    }

    /// Semi-automatic and slow to fire, with little dispersion and a hard hit. Rewards one good shot.
    pub fn marksman_rifle() -> Self {
        let base = Self::assault_rifle();
        Self {
            key: "m14".into(),
            name: "M14 EBR".into(),
            shape: GunShape::MarksmanRifle,
            report: ReportConfig { thump_hz: 78.0, blast_rate: 16.0, tail_rate: 3.4, tail_level: 0.44, crack_level: 1.0 },
            flash: FlashConfig { length: 0.28, width: 0.1, time: 0.05, light: 20_000.0, light_range: 11.0 },
            round_size: 1.4,
            weight: 5.0,
            round_weight: 0.028,
            damage: 85.0,
            damage_falloff: 1.3,
            ballistics: BulletConfig { muzzle_velocity: 850.0, drag: 0.0006, max_age: 4.0, gravity: 9.81 },
            zero_range: 150.0,
            rounds_per_minute: 110.0,
            dispersion_moa: 1.0,
            magazine: 20,
            spare_magazines: 4,
            modes: vec![FireMode::Semi],
            default_mode: FireMode::Semi,
            reload_time: 3.0,
            reload_empty_time: 3.6,
            ads_time: 0.36,
            raise_time: 0.4,
            lower_time: 0.24,
            stow_time: 0.55,
            draw_time: 0.7,
            rest: RestConfig::default(),
            hold: HoldConfig {
                hip: Vec3::new(0.14, -0.15, -0.36),
                ads: Vec3::new(0.0, -0.062, -0.25),
                muzzle_forward: 0.82,
                sight_height: 0.062,
                front_sight: 0.56,
                rear_sight: -0.04,
                max_retract: 0.55,
                blocked_retract: 0.72,
                retract_speed: 2.6,
                lean_roll: 0.18,
                grips: GripConfig {
                    trigger: Vec3::new(0.0, -0.075, 0.07),
                    trigger_wrist: Vec3::new(0.012, -0.094, 0.15),
                    support: Vec3::new(0.0, -0.036, -0.3),
                    support_wrist: Vec3::new(-0.03, -0.062, -0.23),
                    magazine: Vec3::new(0.0, -0.12, -0.05),
                },
                ..base.hold
            },
            recoil: RecoilConfig {
                kick_up: 52.0,
                kick_up_spread: 0.25,
                kick_side: 16.0,
                recovery_rate: 7.5,
                hip_scale: 2.2,
                ..base.recoil
            },
            stability: StabilityConfig {
                hip_stand: 0.32,
                hip_crouch: 0.24,
                ads_stand: 0.025,
                ads_crouch: 0.015,
                ads_prone: 0.006,
                moving: 2.6,
                shot_shake: 0.7,
                ..base.stability
            },
            sway: SwayConfig { stand: 0.2, crouch: 0.1, prone: 0.03, ..base.sway },
            ..base
        }
    }

    /// Quick to bring up and quick in the hand, with a weak slow round and a short reach.
    pub fn pistol() -> Self {
        let base = Self::assault_rifle();
        Self {
            key: "m9".into(),
            name: "M9 Pistol".into(),
            shape: GunShape::Pistol,
            report: ReportConfig { thump_hz: 135.0, blast_rate: 34.0, tail_rate: 7.5, tail_level: 0.2, crack_level: 0.3 },
            flash: FlashConfig { length: 0.09, width: 0.05, time: 0.035, light: 7_000.0, light_range: 7.0 },
            round_size: 0.6,
            weight: 0.95,
            round_weight: 0.015,
            damage: 30.0,
            damage_falloff: 1.5,
            ballistics: BulletConfig { muzzle_velocity: 370.0, drag: 0.0018, max_age: 3.0, gravity: 9.81 },
            zero_range: 25.0,
            rounds_per_minute: 420.0,
            dispersion_moa: 12.0,
            magazine: 15,
            spare_magazines: 3,
            modes: vec![FireMode::Semi],
            default_mode: FireMode::Semi,
            reload_time: 1.8,
            reload_empty_time: 2.1,
            ads_time: 0.18,
            raise_time: 0.2,
            lower_time: 0.12,
            stow_time: 0.25,
            draw_time: 0.32,
            rest: RestConfig { steady_recoil: 0.7, ..RestConfig::default() },
            hold: HoldConfig {
                hip: Vec3::new(0.11, -0.2, -0.36),
                ads: Vec3::new(0.0, -0.036, -0.5),
                lowered_offset: Vec3::new(-0.03, -0.09, 0.06),
                lowered_rotation: Vec3::new(-0.75, 0.25, 0.1),
                fallen_offset: Vec3::new(0.15, -0.15, 0.0),
                fallen_rotation: Vec3::new(0.0, 0.5, 1.5),
                slung_offset: Vec3::new(-0.04, -0.3, 0.2),
                slung_rotation: Vec3::new(-1.15, 0.45, 0.5),
                back_offset: Vec3::new(0.05, -0.15, 0.4),
                back_rotation: Vec3::new(-0.55, 2.65, 0.3),
                muzzle_forward: 0.165,
                sight_height: 0.036,
                front_sight: 0.15,
                rear_sight: -0.02,
                max_retract: 0.2,
                blocked_retract: 0.28,
                retract_speed: 3.0,
                lean_roll: 0.1,
                grips: GripConfig {
                    trigger: Vec3::new(0.0, -0.08, 0.03),
                    trigger_wrist: Vec3::new(0.01, -0.125, 0.095),
                    support: Vec3::new(-0.018, -0.095, 0.02),
                    support_wrist: Vec3::new(-0.05, -0.145, 0.085),
                    magazine: Vec3::new(0.0, -0.12, 0.03),
                },
            },
            recoil: RecoilConfig {
                kick_up: 36.0,
                kick_up_spread: 0.4,
                kick_side: 26.0,
                recovery_rate: 11.0,
                hip_scale: 1.5,
                crouch_scale: 0.85,
                prone_scale: 0.7,
                ..base.recoil
            },
            stability: StabilityConfig {
                hip_stand: 0.3,
                hip_crouch: 0.26,
                hip_prone: 0.2,
                ads_stand: 0.07,
                ads_crouch: 0.055,
                ads_prone: 0.03,
                moving: 1.6,
                raising: 0.4,
                shot_shake: 0.3,
                ..base.stability
            },
            sway: SwayConfig { stand: 0.3, crouch: 0.22, prone: 0.12, rate: 1.0, ..base.sway },
            ..base
        }
    }
}
