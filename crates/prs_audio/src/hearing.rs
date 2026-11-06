//! What the world does to a sound on its way to your ear: distance, which way the gun was pointing,
//! and whatever walls and roofs stand in between. Knows nothing about Bevy's audio, only geometry.

use bevy::prelude::*;

use crate::propagation::{Heard, Propagation};
use prs_runtime::prs_sim::Surface;
use prs_synth::{ContactSound, ContactSounds, SlideSound, SlideSounds, SmokeSound, SoundSet, STEP_VOLUME, StepSound, StepSounds, StepVary};

/// How a sound fades with distance. `loudness` is its volume up close, and it is half as loud at
/// `reach` metres. Well beyond that it halves each time the distance doubles, as sound does outdoors,
/// unless it is `far` reaching.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Carry {
    pub loudness: f32,
    pub reach: f32,
    pub far: Option<FarFade>,
}

/// How a loud sound fades once it is a long way off: past `from` metres, only this many dB each time the
/// distance doubles. In life a shot is so much louder than anything near you that it is plain across a
/// valley, and a game that kept its true fall off from close up would bury it under your own footsteps.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct FarFade {
    pub from: f32,
    pub db_per_doubling: f32,
}

impl Carry {
    pub const fn new(loudness: f32, reach: f32) -> Self {
        Self { loudness, reach, far: None }
    }

    pub const fn reaching(self, from: f32, db_per_doubling: f32) -> Self {
        Self { far: Some(FarFade { from, db_per_doubling }), ..self }
    }

    pub fn volume(self, distance: f32) -> f32 {
        let near = |d: f32| self.loudness * self.reach / (self.reach + d);
        match self.far {
            Some(far) if distance > far.from && far.from > 0.0 => near(far.from) * 10f32.powf(-far.db_per_doubling * (distance / far.from).log2() / 20.0),
            _ => near(distance),
        }
    }
}

/// How far a smoke grenade's hiss gets. It is a small sound, so past a point it is not there at all.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SmokeHearing {
    /// Times the sound's own volume.
    pub carry: Carry,
    /// In metres, where it starts to fade away to nothing and where it has.
    pub fades_out: (f32, f32),
    /// How fast it follows you moving and the smoke thinning, per second, so that it never steps.
    pub ease_rate: f32,
}

impl SmokeHearing {
    pub fn volume(&self, distance: f32) -> f32 {
        let (from, to) = self.fades_out;
        self.carry.volume(distance) * ((to - distance) / (to - from).max(1e-3)).clamp(0.0, 1.0)
    }
}

/// Hearing dulled by being shot at, coming on from `from_level` and at its worst at full suppression.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SuppressedHearing {
    pub from_level: f32,
    /// Where the highs are cut, just past `from_level` and at full suppression.
    pub cutoff_hz: (f32, f32),
    /// How loud everything is at full suppression.
    pub gain: f32,
}

impl SuppressedHearing {
    /// The dulling at this suppression, if there is any: the cutoff and the gain.
    pub fn at(&self, level: f32) -> Option<(f32, f32)> {
        if level <= self.from_level {
            return None;
        }
        let t = ((level - self.from_level) / (1.0 - self.from_level).max(1e-3)).min(1.0);
        Some((self.cutoff_hz.0 + (self.cutoff_hz.1 - self.cutoff_hz.0) * t, 1.0 + (self.gain - 1.0) * t))
    }
}

#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
pub struct Acoustics {
    /// Which sounds play until a player chooses, so a mod can pick. The player's choice beats this.
    pub sound_set: SoundSet,
    /// The sounds of the New set. `steps`, `slides` and the rest stay as tuned and belong to Current.
    pub contacts: ContactSounds,
    /// How much a step's grit differs from the last under the New set.
    pub step_vary: StepVary,
    pub speed_of_sound: f32,
    pub own_shot_volume: f32,
    /// Your footsteps: volume when barely moving, and how much a full sprint adds to it.
    pub step_volume: (f32, f32),
    /// How a boot sounds on each kind of ground.
    pub steps: StepSounds,
    /// Coming down is a louder step on whatever is there: how much louder than a full sprint's step when
    /// barely dropping, how much that adds at `land_full_speed`, in m/s of falling, and beyond it.
    pub land_loudness: (f32, f32),
    pub land_full_speed: f32,
    /// Stances, sights, the gun along a wall and everything else that moves without a click.
    pub slides: SlideSounds,
    /// How loud the hand settling back on the gun at the end of a reload is, against handling anything else.
    pub reload_settle: f32,
    /// How fast the gun has to be pulling back from a wall to be heard, in metres a second, and how long
    /// before it can be heard again.
    pub gun_on_wall_speed: f32,
    pub gun_on_wall_wait: f32,
    /// Someone else's footsteps, and how far off they are worth rendering at all.
    pub other_steps: Carry,
    pub other_steps_within: f32,
    pub shot: Carry,
    pub impact: Carry,
    /// Carries further than it should, because it is your only hit confirmation.
    pub body_hit: Carry,
    /// How loud a shot is from directly behind the barrel, compared to in front of it.
    pub behind_barrel_loudness: f32,
    pub metal_hit: Carry,
    pub crash: Carry,
    /// How hard a crash has to be, in m/s lost, to be as loud as one gets.
    pub crash_full_speed: f32,
    /// How hard a vehicle coming down on its wheels sounds, as a crash at this many m/s.
    pub landed_speed: f32,
    pub blast: Carry,
    pub grenade: Carry,
    /// Something thrown landing, which only carries a little, and how hard it has to land to be that loud.
    pub clink: Carry,
    pub clink_full_speed: f32,
    /// A smoke grenade pouring, and how that carries.
    pub smoke: SmokeSound,
    pub smoke_hearing: SmokeHearing,
    pub engine: EngineSound,
    /// Sounds often start touching a surface. They are heard from this far in front of it.
    pub source_clearance: f32,
    /// What hills, buildings, rooms and the air do to a sound on its way. See [`Propagation`].
    pub propagation: Propagation,
    /// What keeps many sounds at once from adding up past full scale. See [`crate::bus`].
    pub limiter: crate::bus::Limiter,
    /// A bullet going past, at the far edge of what counts and right by the ear.
    pub bullet_passing_volume: (f32, f32),
    /// Metres at which a bullet going past is no longer heard as close.
    pub bullet_passing_reach: f32,
    /// Everything else gets hard to hear when you are being shot at hard enough.
    pub suppressed: SuppressedHearing,
    /// A helicopter. The thump of the blades carries a very long way, and the whine of the engine does not.
    pub rotor_thump: Carry,
    pub rotor_whine: Carry,
    /// Up to about this far you hear the sharp slap of the blades, beyond it only their dull beat.
    pub rotor_slap_reach: f32,
    /// How much of all of it gets through your helmet with you sitting underneath.
    pub rotor_inside: f32,
    /// Loudness of a rotor doing no work, of one fully loaded being 1.
    pub rotor_idle_share: f32,
}

impl Acoustics {
    pub fn landing_volume(&self, speed: f32) -> f32 {
        let hardness = (speed / self.land_full_speed.max(0.1)).clamp(0.0, 1.0);
        (self.step_volume.0 + self.step_volume.1) * (self.land_loudness.0 + hardness * self.land_loudness.1)
    }

    /// A crawl on this ground: as much brighter or duller than on dirt as its grit is under a boot.
    pub fn crawl_on(&self, surface: Surface) -> SlideSound {
        let tint = (self.step_on(surface).scuff_hz / self.steps.dirt.scuff_hz.max(1.0)).sqrt();
        let crawl = self.slides.crawl;
        SlideSound { top_hz_from: crawl.top_hz_from * tint, top_hz_to: crawl.top_hz_to * tint, ..crawl }
    }

    /// The same under the New set.
    pub fn contact_crawl_on(&self, surface: Surface) -> ContactSound {
        let tint = (self.step_on(surface).scuff_hz / self.steps.dirt.scuff_hz.max(1.0)).sqrt();
        ContactSound { lowest_hz: self.contacts.crawl.lowest_hz * tint, ..self.contacts.crawl }
    }

    /// A bullet striking this, under the New set.
    pub fn impact_on(&self, surface: Surface) -> &ContactSound {
        match surface {
            Surface::Dirt | Surface::Flesh => &self.contacts.impact_dirt,
            Surface::Grass => &self.contacts.impact_grass,
            Surface::Gravel => &self.contacts.impact_gravel,
            Surface::Wood => &self.contacts.impact_wood,
            Surface::Concrete => &self.contacts.impact_concrete,
            Surface::Metal => &self.contacts.impact_metal,
        }
    }

    pub fn step_on(&self, surface: Surface) -> &StepSound {
        match surface {
            Surface::Dirt | Surface::Flesh => &self.steps.dirt,
            Surface::Grass => &self.steps.grass,
            Surface::Gravel => &self.steps.gravel,
            Surface::Wood => &self.steps.wood,
            Surface::Concrete => &self.steps.concrete,
            Surface::Metal => &self.steps.metal,
        }
    }
}

impl Default for Acoustics {
    fn default() -> Self {
        Self {
            sound_set: SoundSet::default(),
            contacts: ContactSounds::default(),
            step_vary: StepVary::default(),
            speed_of_sound: 343.0,
            own_shot_volume: 1.0,
            step_volume: STEP_VOLUME,
            steps: StepSounds::default(),
            land_loudness: (1.0, 1.0),
            land_full_speed: 9.0,
            slides: SlideSounds::default(),
            reload_settle: 0.7,
            gun_on_wall_speed: 0.15,
            gun_on_wall_wait: 0.35,
            other_steps: Carry::new(0.5, 5.0),
            other_steps_within: 40.0,
            shot: Carry::new(1.0, 10.0).reaching(30.0, 1.7),
            impact: Carry::new(prs_synth::IMPACT_LOUDNESS, 8.0),
            body_hit: Carry::new(1.0, 15.0),
            behind_barrel_loudness: 0.45,
            metal_hit: Carry::new(prs_synth::METAL_HIT_LOUDNESS, 12.0),
            crash: Carry::new(prs_synth::CRASH_LOUDNESS, 22.0).reaching(40.0, 3.0),
            landed_speed: 9.0,
            crash_full_speed: 16.0,
            blast: Carry::new(1.0, 45.0).reaching(60.0, 1.5),
            grenade: Carry::new(0.9, 30.0).reaching(50.0, 1.5),
            clink: Carry::new(prs_synth::CLINK_LOUDNESS, 5.0),
            clink_full_speed: 10.0,
            smoke: SmokeSound::default(),
            smoke_hearing: SmokeHearing { carry: Carry::new(1.0, 8.0), fades_out: (14.0, 30.0), ease_rate: 6.0 },
            engine: EngineSound::default(),
            source_clearance: 0.3,
            propagation: Propagation::default(),
            limiter: crate::bus::Limiter::default(),
            bullet_passing_volume: (0.3, 0.95),
            bullet_passing_reach: 6.0,
            suppressed: SuppressedHearing { from_level: 0.55, cutoff_hz: (7000.0, 1100.0), gain: 0.55 },
            rotor_thump: Carry::new(0.9, 130.0).reaching(200.0, 3.0),
            rotor_whine: Carry::new(0.5, 25.0),
            rotor_slap_reach: 220.0,
            rotor_inside: 0.55,
            rotor_idle_share: 0.55,
        }
    }
}

/// A running engine: a loop played faster with revs, with a clear copy and a dull copy mixed by how
/// hard it is working and whether anything is in the way.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct EngineSound {
    /// The revs the loops are rendered at.
    pub base_rpm: f32,
    pub min_speed: f32,
    pub max_speed: f32,
    pub carry: Carry,
    /// How loud it is sitting in it, instead of what `carry` makes of the distance.
    pub inside: f32,
    /// Share of the volume at idle, the rest coming on with load and revs.
    pub idle_share: f32,
    /// Share of the clear copy at idle and at full load. The rest is the dull one.
    pub clear_idle: f32,
    pub clear_load: f32,
    /// Revs that count as high, for the volume and the clear copy.
    pub high_rpm: f32,
    /// How often to work out whether something is in the way, in seconds.
    pub occlusion_every: f32,
    /// How quickly the volumes follow, per second, so nothing clicks.
    pub mix_rate: f32,
    /// How far off to bother playing one at all.
    pub out_of_earshot: f32,
}

impl Default for EngineSound {
    fn default() -> Self {
        Self {
            base_rpm: 1800.0,
            min_speed: 0.4,
            max_speed: 2.8,
            carry: Carry::new(0.8, 28.0).reaching(40.0, 3.0),
            inside: 0.42,
            idle_share: 0.45,
            clear_idle: 0.2,
            clear_load: 0.85,
            high_rpm: 4200.0,
            occlusion_every: 0.25,
            mix_rate: 8.0,
            out_of_earshot: 900.0,
        }
    }
}

/// How one engine should be heard this moment.
#[derive(Clone, Copy, Debug)]
pub struct EngineMix {
    pub speed: f32,
    pub clear: f32,
    pub dull: f32,
}

impl EngineSound {
    /// The mix for an engine at these revs and throttle, `distance` away or with you inside it, with
    /// `heard` being what the way to you does to it. Its clear copy is only as much as the top end that gets through.
    pub fn mix(&self, rpm: f32, load: f32, distance: f32, inside: bool, heard: Heard, clarity_hz: (f32, f32)) -> EngineMix {
        let revs = (rpm / self.high_rpm).clamp(0.0, 1.0);
        let effort = (load.clamp(0.0, 1.0) * 0.7 + revs * 0.3).clamp(0.0, 1.0);
        let level = self.idle_share + (1.0 - self.idle_share) * effort;
        let near = if inside { self.inside } else { self.carry.volume(distance) };
        let clear_share = self.clear_idle + (self.clear_load - self.clear_idle) * effort;
        let (clear_share, gain) = if inside { (clear_share, 1.0) } else { (clear_share * heard.clarity(clarity_hz), heard.gain) };
        EngineMix {
            speed: (rpm / self.base_rpm).clamp(self.min_speed, self.max_speed),
            clear: near * level * gain * clear_share,
            dull: near * level * gain * (1.0 - clear_share),
        }
    }
}

impl Acoustics {
    /// How much of a shot's sharp front end reaches the ear, from 0 to 1. A muzzle blast is aimed:
    /// strongest down the barrel, dull behind it. Up close it wraps around whichever way it points.
    pub fn shot_brightness(&self, muzzle: Vec3, barrel: Vec3, ear: Vec3) -> f32 {
        let to_ear = ear - muzzle;
        let distance = to_ear.length();
        let facing = barrel.dot(to_ear.normalize_or_zero());
        let in_front = ((facing + 1.0) * 0.5).powi(2);
        let close = self.shot.reach / (self.shot.reach + distance);
        in_front + (1.0 - in_front) * close
    }

    pub fn shot_loudness(&self, brightness: f32) -> f32 {
        self.behind_barrel_loudness + (1.0 - self.behind_barrel_loudness) * brightness
    }

    /// How much faster a sound plays for its source and the ear closing on each other: 1 when neither
    /// is. Things going away are lower, and things coming at you higher.
    pub fn doppler(&self, source: Vec3, source_velocity: Vec3, ear: Vec3, ear_velocity: Vec3) -> f32 {
        let toward_ear = (ear - source).normalize_or_zero();
        let c = self.speed_of_sound;
        (c - ear_velocity.dot(toward_ear)) / (c - source_velocity.dot(toward_ear)).max(c * 0.25)
    }

    /// Where a moving thing was when the sound you can hear now left it, near enough.
    pub fn heard_from(&self, source: Vec3, source_velocity: Vec3, ear: Vec3) -> Vec3 {
        source - source_velocity * (ear.distance(source) / self.speed_of_sound)
    }

    /// Share of a helicopter's beat that still has its slap at this distance. The rest is the dull part.
    pub fn slap_share(&self, distance: f32) -> f32 {
        self.rotor_slap_reach / (self.rotor_slap_reach + distance)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_mod_can_change_what_things_sound_like() {
        let active = prs_mods::Mod::parse("test", "audio.speed_of_sound = 100\naudio.shot.reach = 2.0").unwrap();
        let (plain, modded) = (Acoustics::default(), active.over("audio", Acoustics::default()).unwrap());
        assert_eq!(modded.speed_of_sound, 100.0);
        assert_eq!(modded.shot.reach, 2.0);
        assert_eq!(modded.own_shot_volume, plain.own_shot_volume);
        assert!(active.over("audio.nope", 1.0_f32).is_ok(), "nothing said there, nothing to change");
    }


    #[test]
    fn being_shot_at_only_dulls_hearing_once_it_gets_heavy() {
        let hearing = Acoustics::default().suppressed;
        assert!(hearing.at(0.0).is_none());
        assert!(hearing.at(hearing.from_level).is_none());
        let (some, _) = hearing.at(hearing.from_level + 0.1).unwrap();
        let (full, gain) = hearing.at(1.0).unwrap();
        assert!(full < some, "{full} {some}");
        assert!(gain < 1.0);
    }

    use super::*;
    use crate::propagation::Heard;
    use prs_runtime::prs_sim::Level;

    const CLARITY: (f32, f32) = (500.0, 4000.0);

    #[test]
    fn nobody_elses_shot_is_louder_than_your_own() {
        let a = Acoustics::default();
        assert_eq!(a.shot.volume(0.0), a.own_shot_volume);
        assert!(a.shot.volume(5.0) < a.own_shot_volume * 0.75);
    }

    #[test]
    fn sounds_keep_getting_quieter_the_further_away_they_are() {
        let a = Acoustics::default();
        for carry in [a.shot, a.impact, a.body_hit, a.blast, a.engine.carry, a.other_steps] {
            for distance in [1.0, 5.0, 25.0, 100.0, 300.0, 1000.0] {
                assert!(carry.volume(distance * 2.0) < carry.volume(distance));
            }
        }
        for carry in [a.impact, a.body_hit, a.other_steps] {
            assert!(carry.volume(400.0) < carry.volume(200.0) * 0.55, "far off, double the distance is half as loud");
        }
    }

    /// A firefight across the map is plain to hear, a blast further still, and footsteps stay close.
    #[test]
    fn loud_things_carry_far_and_quiet_ones_do_not() {
        let a = Acoustics::default();
        let db = |v: f32| 20.0 * v.log10();
        let shot = db(a.shot.volume(600.0));
        assert!((-22.0..-18.0).contains(&shot), "a shot at 600 m: {shot}");
        assert!(db(a.blast.volume(1200.0)) > shot, "a blast twice as far is louder still");
        assert!(db(a.other_steps.volume(40.0)) < db(a.other_steps.loudness) - 15.0, "footsteps fade fast");
        // Nothing changes close in, where a firefight is loud already.
        assert!((a.shot.volume(20.0) - Carry::new(1.0, 10.0).volume(20.0)).abs() < 1e-6);
    }

    #[test]
    fn a_shot_aimed_at_you_is_brighter_and_louder_than_one_aimed_away() {
        let a = Acoustics::default();
        let (muzzle, ear) = (Vec3::ZERO, Vec3::new(0.0, 0.0, -100.0));
        let toward = a.shot_brightness(muzzle, Vec3::NEG_Z, ear);
        let sideways = a.shot_brightness(muzzle, Vec3::X, ear);
        let away = a.shot_brightness(muzzle, Vec3::Z, ear);
        assert!(toward > 0.99);
        assert!(toward > sideways && sideways > away);
        assert!(away < 0.1);
        assert!(a.shot_loudness(toward) > a.shot_loudness(away) * 1.8);
    }

    #[test]
    fn the_rifle_on_your_own_shoulder_stays_sharp() {
        let a = Acoustics::default();
        assert!(a.shot_brightness(Vec3::NEG_Z * 0.5, Vec3::NEG_Z, Vec3::ZERO) > 0.85);
    }

    #[test]
    fn an_engine_rises_in_pitch_and_weight_with_revs_and_load() {
        let e = EngineSound::default();
        let idle = e.mix(800.0, 0.0, 10.0, false, Heard::CLEAR, CLARITY);
        let working = e.mix(3500.0, 1.0, 10.0, false, Heard::CLEAR, CLARITY);
        assert!(working.speed > idle.speed * 2.0, "{idle:?} {working:?}");
        assert!(working.clear + working.dull > idle.clear + idle.dull);
        assert!(working.clear / working.dull > idle.clear / idle.dull);
        assert!(e.mix(3500.0, 1.0, 200.0, false, Heard::CLEAR, CLARITY).clear < working.clear * 0.3, "further off is quieter");
    }

    #[test]
    fn an_engine_through_a_wall_is_only_its_low_end() {
        let a = Acoustics::default();
        let p = &a.propagation;
        let wall = p.heard(&[30.0, 35.0, 40.0, 45.0, 45.0, 45.0, 45.0]);
        let open = a.engine.mix(3000.0, 1.0, 40.0, false, Heard::CLEAR, p.clarity_hz);
        let blocked = a.engine.mix(3000.0, 1.0, 40.0, false, wall, p.clarity_hz);
        assert_eq!(blocked.clear, 0.0);
        assert!(blocked.dull < open.clear + open.dull);
        assert!(blocked.dull > 0.0);
        // In the seat nothing is between you and it.
        assert!(a.engine.mix(3000.0, 1.0, 1.0, true, wall, p.clarity_hz).clear > 0.0);
    }

    #[test]
    fn a_helicopter_is_higher_coming_at_you_and_lower_going_away() {
        let a = Acoustics::default();
        let ear = Vec3::ZERO;
        let at = Vec3::new(0.0, 50.0, -400.0);
        let coming = a.doppler(at, Vec3::new(0.0, 0.0, 40.0), ear, Vec3::ZERO);
        let going = a.doppler(at, Vec3::new(0.0, 0.0, -40.0), ear, Vec3::ZERO);
        assert!((1.08..1.2).contains(&coming), "{coming}");
        assert!((0.85..0.95).contains(&going), "{going}");
        assert_eq!(a.doppler(at, Vec3::ZERO, ear, Vec3::ZERO), 1.0);
    }

    #[test]
    fn a_helicopter_carries_further_than_anything_else_and_loses_its_edge_first() {
        let a = Acoustics::default();
        for distance in [200.0, 500.0, 1000.0] {
            assert!(a.rotor_thump.volume(distance) > a.shot.volume(distance));
            assert!(a.rotor_thump.volume(distance) > a.rotor_whine.volume(distance) * 3.0);
        }
        assert!(a.rotor_thump.volume(800.0) > 0.1, "you can hear one coming from across the map");
        assert!(a.slap_share(30.0) > 0.8 && a.slap_share(900.0) < 0.3);
    }

    #[test]
    fn you_hear_a_fast_helicopter_where_it_was() {
        let a = Acoustics::default();
        let heard = a.heard_from(Vec3::new(343.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -40.0), Vec3::ZERO);
        assert!((heard.z - 40.0).abs() < 1e-3, "{heard}");
    }

    /// A room in the range's kill house with a door to the outside: where you stand in it, a place out in
    /// front of that door, and the floor.
    fn range() -> (Level, prs_runtime::prs_sim::CollisionWorld, Vec3, Vec3) {
        let level = Level::named("range").unwrap();
        let world = level.collision();
        let spaces = crate::propagation::Spaces::new(&level, &Propagation::default());
        let door = level
            .doorways
            .iter()
            .find(|d| spaces.space_of(d.at + Vec3::Y - d.outward * 0.4).is_some() && spaces.space_of(d.at + Vec3::Y + d.outward * 0.4).is_none())
            .expect("a door out of the kill house");
        let inside = d_in(door.at, door.outward, 1.5) + Vec3::Y * 1.6;
        let outside = d_in(door.at, door.outward, -12.0) + Vec3::Y * 1.6;
        (level, world, inside, outside)
    }

    fn d_in(at: Vec3, outward: Vec3, metres: f32) -> Vec3 {
        at - Vec3::new(outward.x, 0.0, outward.z).normalize() * metres
    }

    fn heard(level: &Level, world: &prs_runtime::prs_sim::CollisionWorld, source: Vec3, ear: Vec3) -> crate::propagation::Heard {
        let p = Propagation::default();
        p.heard(&crate::propagation::loss(&p, &crate::propagation::Spaces::new(level, &p), world, source, ear))
    }

    #[test]
    fn a_shot_outside_is_duller_and_quieter_indoors_than_out_in_the_field() {
        let (level, world, inside, outside) = range();
        let indoors = heard(&level, &world, outside, inside);
        let in_the_field = heard(&level, &world, outside, outside + (outside - inside).normalize() * 13.5);
        assert!(indoors.gain <= in_the_field.gain, "{indoors:?} {in_the_field:?}");
        assert!(indoors.gain > 0.3, "straight in through the open door: {indoors:?}");
    }

    #[test]
    fn landing_is_louder_than_a_sprinting_step_and_harder_is_louder() {
        let a = Acoustics::default();
        let sprint = a.step_volume.0 + a.step_volume.1;
        assert!(a.landing_volume(1.0) > sprint);
        assert!(a.landing_volume(4.5) > a.landing_volume(1.0));
        assert_eq!(a.landing_volume(a.land_full_speed), a.landing_volume(100.0));
    }

    #[test]
    fn a_crawl_over_gravel_is_brighter_than_over_dirt() {
        let a = Acoustics::default();
        assert!(a.crawl_on(Surface::Gravel).top_hz_from > a.crawl_on(Surface::Dirt).top_hz_from);
        assert_eq!(a.crawl_on(Surface::Dirt).top_hz_to, a.slides.crawl.top_hz_to);
    }

    #[test]
    fn a_mod_can_pick_the_sound_set_and_change_a_new_sound() {
        assert_eq!(Acoustics::default().sound_set, SoundSet::New);
        let tuned = prs_mods::Mod::parse("set", "audio.sound_set = \"current\"\n[audio.contacts.mag_in]\ntouch_ms = 9.0\n[audio.step_vary]\nscuff_hz = 0.0\n").unwrap().over("audio", Acoustics::default()).unwrap();
        assert_eq!(tuned.sound_set, SoundSet::Current);
        assert_eq!(tuned.contacts.mag_in.touch_ms, 9.0);
        assert_eq!(tuned.step_vary.scuff_hz, 0.0);
        assert_eq!(tuned.contacts.mag_out.touch_ms, Acoustics::default().contacts.mag_out.touch_ms);
        assert!(prs_mods::Mod::parse("bad", "audio.sound_set = \"loud\"").unwrap().over("audio", Acoustics::default()).is_err());
    }

    #[test]
    fn every_surface_has_its_own_impact_under_the_new_set() {
        let a = Acoustics::default();
        assert!(a.impact_on(Surface::Metal).damping < a.impact_on(Surface::Wood).damping, "metal rings on");
        assert_ne!(a.impact_on(Surface::Dirt).lowest_hz, a.impact_on(Surface::Gravel).lowest_hz);
        assert!(a.contact_crawl_on(Surface::Gravel).lowest_hz > a.contact_crawl_on(Surface::Dirt).lowest_hz);
    }

    #[test]
    fn a_mod_can_change_one_setting_of_one_surface() {
        let tuned = prs_mods::Mod::parse("steps", "[audio.steps.dirt]\nthud_level = 0.3\n").unwrap().over("audio", Acoustics::default()).unwrap();
        assert_eq!(tuned.steps.dirt.thud_level, 0.3);
        assert_eq!(tuned.steps.dirt.thud_hz, Acoustics::default().steps.dirt.thud_hz);
        assert_eq!(tuned.steps.gravel.thud_level, Acoustics::default().steps.gravel.thud_level);
    }

    #[test]
    fn two_people_in_the_same_room_hear_each_other_clearly() {
        let (level, world, inside, outside) = range();
        let across = (inside - outside).normalize().cross(Vec3::Y) * 0.4;
        let heard = heard(&level, &world, inside + across, inside - across);
        assert!(heard.gain > 0.99 && heard.cutoff_hz > 15_000.0, "{heard:?}");
    }
}
