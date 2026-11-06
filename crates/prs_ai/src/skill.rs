//! How good a soldier is, as data. A difficulty setting or a mod changes this and nothing else. None of
//! it reaches into the sim: a better shot still holds the same gun, with the same recoil, sway and wander,
//! and is only quicker to see, steadier in the head and more patient about pulling the trigger.

use glam::Vec2;

use crate::sight::SightConfig;

/// Lengths are metres, times are seconds and angles are radians. A pair is the least and the most.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkillConfig {
    /// How long from first seeing someone to doing anything about it.
    pub reaction: Vec2,
    /// Half the width of what they notice without turning, as a cosine.
    pub view_cos: f32,
    /// Someone this close is noticed whichever way they are facing.
    pub notice_close: f32,
    /// Further than this nobody is seen at all.
    pub view_range: f32,
    /// Someone upright and moving within this is always spotted. Past it the chance falls with the square.
    pub spot_range: f32,
    /// Chance of spotting someone crouched and someone prone, compared to standing.
    pub spot_crouched: f32,
    pub spot_prone: f32,
    /// Chance of spotting someone standing still, compared to moving.
    pub spot_still: f32,
    /// How fast they become aware of someone in plain sight up close, per second, and how fast that fades
    /// with them out of sight. At full awareness they have been seen for what they are.
    pub notice_rate: f32,
    pub forget_rate: f32,
    /// How much a sliver counts: the rate goes with the share of the body that shows to this power.
    pub visible_power: f32,
    /// Someone firing is this much quicker to pick out, and counts as at least this much of them showing.
    pub firing_boost: f32,
    pub firing_share: f32,
    /// Below this share of them showing, someone being watched is lost to sight and only remembered.
    pub track_share: f32,
    /// Past this only someone firing is picked out at all, and past `still_hidden_beyond` nobody keeping
    /// still and down.
    pub quiet_range: f32,
    pub still_hidden_beyond: f32,
    /// How far gunfire is heard, and what share of the distance they are out by in placing it.
    pub hear_range: f32,
    pub hear_error: f32,
    /// How fast they can swing the gun round, and how quickly it closes on what they aim at, per second.
    pub aim_speed: f32,
    pub aim_rate: f32,
    /// How far off their idea of the aim point wanders, as an angle, and how long each wander takes.
    pub tracking_noise: f32,
    pub tracking_time: Vec2,
    /// How old their sight of their own front sight is, and how much of what it shows they correct for.
    /// A person cannot follow recoil closely either, which is what keeps this from being a laser.
    pub sight_lag: f32,
    pub sight_correct: f32,
    /// Share of a target's movement during the bullet's flight they allow for.
    pub lead: f32,
    /// Share of the range they can be out by in judging it for elevation.
    pub range_error: f32,
    /// How many target widths off the sights can be for them to take the shot, and how long they wait for that.
    pub fire_tolerance: f32,
    pub patience: f32,
    /// Bursts in rounds up close, in the middle and far off, and which distances those change at.
    pub burst_close: (u32, u32),
    pub burst_mid: (u32, u32),
    pub burst_far: (u32, u32),
    pub close_range: f32,
    pub far_range: f32,
    /// Waits between bursts up close and far off, to let the gun settle.
    pub pause_close: Vec2,
    pub pause_far: Vec2,
    /// Fired at from the hip this close, rather than waiting for the sights.
    pub hip_range: f32,
    /// Crouches to shoot past this, if they can see from down there.
    pub crouch_beyond: f32,
    /// What being shot at does. Near misses and hits add to pressure, and it drains over `calm_time`.
    pub crack_radius: f32,
    pub crack_pressure: f32,
    pub hit_pressure: f32,
    pub calm_time: f32,
    /// How much pressure the sim's suppression level counts for.
    pub suppression_weight: f32,
    /// From this much they break for cover, and until it is below that much they stay there.
    pub cover_at: f32,
    pub pinned_at: f32,
}

impl SkillConfig {
    /// A soldier who has been shot at before. A harder bot than the usual, for a mod to choose.
    pub fn regular() -> Self {
        Self {
            reaction: Vec2::new(0.35, 0.9),
            view_cos: 0.35,
            notice_close: 10.0,
            view_range: 420.0,
            spot_range: 140.0,
            spot_crouched: 0.65,
            spot_prone: 0.35,
            spot_still: 0.6,
            notice_rate: 5.0,
            forget_rate: 0.35,
            visible_power: 2.5,
            firing_boost: 5.0,
            firing_share: 0.5,
            track_share: 0.15,
            quiet_range: 350.0,
            still_hidden_beyond: 160.0,
            hear_range: 280.0,
            hear_error: 0.12,
            aim_speed: 2.6,
            aim_rate: 7.0,
            tracking_noise: 0.0075,
            tracking_time: Vec2::new(0.35, 1.0),
            sight_lag: 0.22,
            sight_correct: 0.45,
            lead: 0.7,
            range_error: 0.12,
            fire_tolerance: 1.3,
            patience: 2.2,
            burst_close: (3, 6),
            burst_mid: (2, 3),
            burst_far: (1, 1),
            close_range: 30.0,
            far_range: 110.0,
            pause_close: Vec2::new(0.25, 0.6),
            pause_far: Vec2::new(0.7, 1.8),
            hip_range: 9.0,
            crouch_beyond: 30.0,
            crack_radius: 4.0,
            crack_pressure: 0.3,
            hit_pressure: 1.0,
            calm_time: 3.0,
            suppression_weight: 1.2,
            cover_at: 0.55,
            pinned_at: 0.3,
        }
    }

    /// What a bot is unless a mod says otherwise: slow to see someone and slower to react, with an unsteady
    /// idea of where to aim and little feel for recoil, range or lead, and in no hurry to shoot. About half as
    /// dangerous as a regular at middle range, and still dangerous up close.
    pub fn easy() -> Self {
        Self {
            reaction: Vec2::new(0.8, 1.7),
            notice_rate: 3.2,
            spot_range: 100.0,
            spot_crouched: 0.45,
            spot_prone: 0.2,
            spot_still: 0.4,
            aim_speed: 1.8,
            aim_rate: 4.0,
            tracking_noise: 0.018,
            tracking_time: Vec2::new(0.5, 1.4),
            sight_lag: 0.32,
            sight_correct: 0.2,
            lead: 0.35,
            range_error: 0.25,
            fire_tolerance: 2.0,
            patience: 3.0,
            burst_close: (2, 5),
            burst_mid: (1, 2),
            burst_far: (1, 1),
            pause_close: Vec2::new(0.4, 0.9),
            pause_far: Vec2::new(1.1, 2.4),
            ..Self::regular()
        }
    }

    /// Slow to see, slow to aim, shoots too soon and scares easily.
    pub fn recruit() -> Self {
        Self {
            reaction: Vec2::new(0.7, 1.6),
            notice_rate: 3.0,
            spot_range: 90.0,
            aim_speed: 1.8,
            aim_rate: 4.5,
            tracking_noise: 0.013,
            sight_correct: 0.2,
            lead: 0.3,
            range_error: 0.25,
            fire_tolerance: 2.5,
            patience: 1.0,
            cover_at: 0.35,
            pinned_at: 0.2,
            ..Self::regular()
        }
    }

    /// Quick, patient, and takes more to get their head down.
    pub fn veteran() -> Self {
        Self {
            reaction: Vec2::new(0.22, 0.5),
            notice_rate: 7.0,
            spot_range: 200.0,
            aim_speed: 3.4,
            aim_rate: 10.0,
            tracking_noise: 0.005,
            sight_lag: 0.16,
            sight_correct: 0.65,
            lead: 0.9,
            range_error: 0.06,
            fire_tolerance: 0.9,
            patience: 3.5,
            cover_at: 0.8,
            pinned_at: 0.45,
            ..Self::regular()
        }
    }
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self::easy()
    }
}

/// What a fighting soldier does, apart from how good they are at it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FightConfig {
    pub skill: SkillConfig,
    pub sight: SightConfig,
    /// Ticks between looking around. Each soldier has their own moment, so rays are spread out.
    pub think_ticks: u32,
    /// How long someone not seen stays worth thinking about, and for how long to keep firing where they were.
    pub forget_after: f32,
    pub suppress_memory: f32,
    /// A friend this close to the line of fire holds the trigger.
    pub friend_clearance: f32,
    /// Tries at finding somewhere to hide, each with this many feels around, and how far off they feel.
    pub cover_tries: u32,
    pub cover_spots: u32,
    pub cover_reach: f32,
    /// How far from a wall to stop.
    pub cover_stand_off: f32,
    /// Whatever hides you has to be this close, or it is just the far side of a hill.
    pub cover_hug: f32,
    /// Height of a crouched eye above the feet, for judging cover.
    pub cover_eye: f32,
    /// How long to stay down before looking out again, and how long to look.
    pub hide_time: Vec2,
    pub peek_time: Vec2,
    /// How long to look around on hearing something, before getting on with it.
    pub alert_time: Vec2,
    /// How long after that the same thing can stop them again.
    pub alert_rest: f32,
    /// Sideways from the enemy a flank goes, as a share of the range between its limits, and how long it may take.
    pub flank_share: f32,
    pub flank_reach: Vec2,
    pub flank_time: f32,
    /// Keeps going for the flank past this range, rather than stopping to shoot at whoever shows up.
    pub flank_commit: f32,
    /// Thrown things: how long one soldier and one squad leave between them, and how long to give a throw.
    pub throw_rest: f32,
    pub squad_throw_rest: f32,
    pub throw_time: f32,
    /// Chance of deciding to, each time it could be worth it.
    pub frag_chance: f32,
    pub smoke_chance: f32,
    /// How far a frag gets thrown at someone, how fresh the sighting has to be, and how far friends have to be from it.
    pub frag_range: Vec2,
    pub frag_memory: f32,
    pub frag_friends: f32,
    /// Someone moving slower than this, even in sight, is worth a frag.
    pub frag_still: f32,
    /// How far ahead smoke goes to cover moving, and the most it is thrown at all.
    pub smoke_ahead: f32,
    pub smoke_reach: f32,
    /// Share of the range they land wide, and how far off the angle of the throw they go. Nobody's throw is perfect.
    pub throw_error: f32,
    pub throw_angle_error: f32,
    /// A throw needs this much open air ahead of it.
    pub throw_room: f32,
    /// How many times the usual pressure it takes to stop someone going round.
    pub flank_nerve: f32,
    /// How far from the squad's middle a soldier lets themselves get, a medic, and how far ahead of it on
    /// the way somewhere before waiting. Past `squad_group` they are not with the squad at all yet.
    pub squad_keep: f32,
    pub medic_keep: f32,
    pub squad_ahead: f32,
    pub squad_group: f32,
    /// Longest wait for them, and how long it takes to be willing to wait all over again.
    pub squad_wait: f32,
    pub squad_wait_recover: f32,
    /// Runs to a friend who is down within this, and treats them from this far.
    pub medic_reach: f32,
    pub medic_close: f32,
    /// Share of a magazine left at which to change it while nobody is in sight.
    pub reload_below: f32,
    /// Pressure low enough to stop and bandage or get someone up.
    pub calm_enough: f32,
    /// An enemy in sight further off than this does not stop a medic.
    pub medic_ignores_beyond: f32,
    /// Nearly there, only being shot at stops a medic.
    pub medic_committed: f32,
    /// Further from them than this a medic runs flat out.
    pub medic_sprint: f32,
    /// Most rounds per magazine one soldier will spend on where someone was, as a share.
    pub suppress_share: f32,
}

impl Default for FightConfig {
    fn default() -> Self {
        Self {
            skill: SkillConfig::default(),
            sight: SightConfig::default(),
            think_ticks: 12,
            forget_after: 18.0,
            suppress_memory: 7.0,
            friend_clearance: 1.6,
            cover_tries: 6,
            cover_spots: 3,
            cover_reach: 14.0,
            cover_stand_off: 0.6,
            cover_hug: 3.5,
            cover_eye: 0.95,
            hide_time: Vec2::new(1.5, 4.0),
            peek_time: Vec2::new(2.5, 5.0),
            alert_time: Vec2::new(1.5, 3.5),
            alert_rest: 8.0,
            flank_share: 0.55,
            flank_reach: Vec2::new(18.0, 70.0),
            flank_time: 35.0,
            flank_commit: 45.0,
            flank_nerve: 1.8,
            throw_rest: 30.0,
            squad_throw_rest: 10.0,
            throw_time: 5.0,
            frag_chance: 0.7,
            smoke_chance: 0.17,
            frag_range: Vec2::new(9.0, 40.0),
            frag_memory: 14.0,
            frag_friends: 9.0,
            frag_still: 1.0,
            smoke_ahead: 12.0,
            smoke_reach: 32.0,
            throw_error: 0.15,
            throw_angle_error: 0.05,
            throw_room: 5.0,
            squad_keep: 28.0,
            medic_keep: 12.0,
            squad_ahead: 35.0,
            squad_group: 90.0,
            squad_wait: 6.0,
            squad_wait_recover: 20.0,
            medic_reach: 150.0,
            medic_close: 1.5,
            reload_below: 0.3,
            calm_enough: 0.3,
            medic_ignores_beyond: 35.0,
            medic_committed: 15.0,
            medic_sprint: 12.0,
            suppress_share: 0.5,
        }
    }
}
