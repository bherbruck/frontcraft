//! Proportions and animation tuning. Lengths are metres and angles are radians throughout.

use glam::{Vec2, Vec3};

/// Heaving at a hull with both hands.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LiftPoseConfig {
    pub reach_time: f32,
    pub return_time: f32,
    /// Where the hands go for on the hull, from the eye: down to the chest, ahead of it, and each side of it.
    pub chest_below_eye: f32,
    pub ahead: f32,
    pub apart: f32,
    pub elbow: Vec3,
    /// Leaning into a push: how high the eye comes down to, where the shoulders are from it, and how much of an
    /// arm's length is used, so the elbows keep a little bend. Where they stand follows from these.
    pub push_eye: f32,
    pub shoulder_drop: f32,
    pub shoulder_ahead: f32,
    pub arm_bend: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BodyConfig {
    pub bones: Proportions,
    pub look: LookConfig,
    pub turn: TurnConfig,
    pub gait: GaitConfig,
    pub air: AirConfig,
    pub reload: ReloadConfig,
    pub care: CareConfig,
    pub lift: LiftPoseConfig,
    pub collapse: CollapseConfig,
    pub stand: StancePose,
    pub crouch: StancePose,
    pub prone: StancePose,
    /// Crouched with someone standing on the back: flatter and lower, and feet well apart.
    pub brace: StancePose,
    /// Leaning into something with both hands on it: weight forward from the ankles, the back leg long.
    pub heave: StancePose,
}

/// Bone lengths and how thick each segment is. Everything that draws or tests a body reads these.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Proportions {
    pub pelvis: f32,
    pub spine: f32,
    pub chest: f32,
    pub neck: f32,
    pub head: f32,
    pub shoulder: f32,
    pub upper_arm: f32,
    pub forearm: f32,
    pub hand: f32,
    pub thigh: f32,
    pub shin: f32,
    pub foot: f32,
    /// Where the arms root on the chest: x = out from the middle, y = down from the top of the chest.
    pub shoulder_root: Vec2,
    /// Hip joint out from the middle of the pelvis.
    pub hip_width: f32,
    /// Sole to ankle joint.
    pub ankle_height: f32,
    /// The skull joint from the eye, looking straight ahead: x = right, y = up, -z = forward.
    pub skull_from_eye: Vec3,
    /// Half width and half depth of each segment at its thickest.
    pub pelvis_size: Vec2,
    pub spine_size: Vec2,
    pub chest_size: Vec2,
    pub neck_size: Vec2,
    pub head_size: Vec2,
    pub shoulder_size: Vec2,
    pub upper_arm_size: Vec2,
    pub forearm_size: Vec2,
    pub hand_size: Vec2,
    pub thigh_size: Vec2,
    pub shin_size: Vec2,
    pub foot_size: Vec2,
}

/// How looking around moves the upper body. The eye never moves: the body arranges itself below it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LookConfig {
    /// How far the chest turns from the way a seat faces, in radians.
    pub sit_twist: f32,
    /// How far each foot is out from the middle, sitting.
    pub sit_feet_apart: f32,
    /// How far above a hand on a wheel its wrist is.
    pub hand_on_wheel: f32,
    /// How far either side of the middle the hands go on a ledge, how far back of the hand a wrist is, and
    /// how far a ledge is above where the hand holds.
    pub ledge_hands_apart: f32,
    pub ledge_wrist_back: f32,
    pub ledge_wrist_up: f32,
    /// How close together the feet are on someone's back, as a share of how they usually stand.
    pub perched_feet: f32,
    /// Seconds to get braced when someone climbs on.
    pub brace_time: f32,
    /// Seconds for a gun to go onto its sling for a climb, or to come back up.
    pub sling_time: f32,
    /// How much of the look pitch swings the skull around the eye. The rest is taken by the head
    /// nodding on its joint, which keeps the neck from being hauled up when looking down.
    pub skull_pitch_share: f32,
    /// How far the neck leans with the head rather than along the torso, 0 to 1.
    pub neck_share: f32,
    /// The same looking well down, which swings the chest back from under the eye and off the gun.
    pub neck_share_down: f32,
    /// Looking far down the head tucks in under and behind the eye, taking the chest with it: x = back,
    /// y = down. That leaves your legs and rifle in view and saves the hips rising under a fixed eye.
    pub tuck: Vec2,
    /// Look-down pitch over which the tuck comes in: x = start, y = full.
    pub tuck_pitch: Vec2,
    /// How far an arm's centre line keeps from the eye, to stay clear of the camera's near plane.
    pub arm_eye_clearance: f32,
    /// How far the neck stands up towards vertical lying down, to come up off a flat torso.
    pub neck_share_prone: f32,
    /// Hips travel back per radian of looking down, which is what bending over does.
    pub hips_back_down: f32,
    /// Hips travel forward per radian of looking up.
    pub hips_forward_up: f32,
    /// Limit on how far free look turns the head from the gun: x = yaw, y = pitch.
    pub head_turn_limit: Vec2,
    /// How much of its length the spine is kept stretched to, 0 to 1. The hips move back rather than
    /// let it fold, so this is what keeps a back straight. One is a dead straight line.
    pub spine_stretch: f32,
    /// Most the shoulder may swing towards a grip that is out of reach.
    pub shoulder_reach: f32,
    /// Most the support hand may slide back along the handguard to stay on a gun that is too far.
    pub support_slide: f32,
    /// How far the hips go over with the weight at full lean. The spine takes the rest of the way to the eye.
    pub lean_hips: f32,
    /// How far the head rolls at full lean, radians. It stays closer to level than the shoulders, as a head does.
    pub lean_head_roll: f32,
    /// Where the wrist goes winding up to throw, from the eye: x = right, y = up, -z = forward.
    pub throw_hand: Vec3,
    /// The wrist from something held out in front of you, in the look's frame.
    pub hand_at_wrist: Vec3,
    /// Elbow directions turned with the chest but kept upright: x = right, y = up, -z = forward.
    pub elbow_trigger: Vec3,
    pub elbow_support: Vec3,
}

/// How the lower body catches up with where the upper body looks.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TurnConfig {
    /// Standing still, the hips leave the look this far before they start to follow.
    pub start: f32,
    /// And they stop following once this close.
    pub stop: f32,
    /// Share of the look-to-travel angle the hips turn when walking off the look direction.
    pub hip_follow: f32,
    /// Most the hips turn towards the travel direction.
    pub hip_follow_max: f32,
    /// Rate the hips chase their target, per second.
    pub rate: f32,
    /// Radians the hips turn for one in-place step.
    pub step_angle: f32,
    pub step_lift: f32,
}

/// One point on the speed curve. Values between points are interpolated.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct GaitPoint {
    pub speed: f32,
    /// Ground a foot covers while planted.
    pub stance_length: f32,
    pub swing_height: f32,
    /// Hips travel back from the eye, the forward lean of moving fast.
    pub lean: f32,
    /// Hips lower than the stance's rest height.
    pub hip_drop: f32,
    /// How far the hips bob each step.
    pub bounce: f32,
    /// How far the hips swing with each step, radians. The chest counters it.
    pub hip_swing: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GaitConfig {
    /// Full leg cycles per `stride`. One means the left foot lands on every footstep sound.
    pub cycles_per_stride: f32,
    /// Below this speed the feet settle into the stance's rest position.
    pub still_speed: f32,
    /// Speed at which the walk is fully blended in.
    pub moving_speed: f32,
    /// How quickly the walk blends in and out, per second.
    pub blend_rate: f32,
    pub stand: [GaitPoint; 3],
    pub crouch: [GaitPoint; 2],
    /// Share of the cycle a foot stays planted when crawling. Crawling slides on the belly, so unlike
    /// walking this is not tied to distance.
    pub crawl_duty: f32,
    pub crawl_stroke: f32,
    pub crawl_wriggle: f32,
    pub crawl_lift: f32,
    /// Half the distance between the feet while walking.
    pub track_width: f32,
    /// How much more the hips bob and swing on a fully wounded leg, as fractions.
    pub limp_bounce: f32,
    pub limp_swing: f32,
    /// Toe pitch at the start of stance, and at push-off.
    pub heel_strike: f32,
    pub toe_off: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AirConfig {
    /// How quickly the legs tuck on leaving the ground, per second.
    pub rate: f32,
    /// Ankles from the hip: x = out, y = up, -z = forward. Rising, and then falling.
    pub rise_front: Vec3,
    pub rise_back: Vec3,
    pub fall_front: Vec3,
    pub fall_back: Vec3,
    /// Fall speed over which the legs go from tucked to reaching for the ground.
    pub fall_blend_speed: f32,
    /// Dip of the hips per metre per second of landing speed.
    pub land_dip: f32,
    pub land_dip_max: f32,
    pub land_stiffness: f32,
    pub land_damping: f32,
    /// Slower than this and there is no landing to absorb.
    pub land_min_speed: f32,
}

/// Off-hand path while reloading, as fractions of the reload. It goes foregrip, magazine, pouch,
/// magazine, foregrip.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReloadConfig {
    pub at_magazine: f32,
    pub at_pouch: f32,
    pub leave_pouch: f32,
    pub back_at_magazine: f32,
    pub leave_magazine: f32,
    /// Spare magazines, in the chest's frame from its top: x = right, y = up, -z = forward.
    pub pouch: Vec3,
}

impl ReloadConfig {
    /// Whether the magazine is off the gun and in the hand at this point of a reload. The old one
    /// goes in the pouch and the hand comes back with the new one.
    pub fn magazine_in_hand(&self, t: f32) -> bool {
        t > self.at_magazine && t < self.back_at_magazine
    }
}

/// Putting a dressing on: the hands leave the gun, one holds the dressing to the wound and the other
/// takes the roll back and forth over it. How far along is the treatment's own progress, so the motion
/// lasts exactly as long as the treatment does.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CareConfig {
    /// Seconds for the hands to come off the gun and get to the wound.
    pub reach_time: f32,
    /// Seconds for them to go back, finished or interrupted.
    pub return_time: f32,
    /// How many passes of the roll a treatment takes, however long it lasts.
    pub passes: f32,
    /// How far round the limb the roll goes each side of the top of it.
    pub arc: f32,
    /// Share of the treatment at each end with the hands still: getting the dressing out, tying it off.
    pub start_still: f32,
    pub end_still: f32,
    /// How far the roll works its way along the limb, each side of the wound.
    pub slide: f32,
    /// Between the surface of whatever is being wrapped and the middle of the hand.
    pub hand_gap: f32,
    /// The hand holding the dressing, along the limb from the wound.
    pub hold_along: f32,
    /// Off wrist with a forearm held up to be wrapped, where you can see it: from the eye along the look.
    pub arm_wrist: Vec3,
    /// The same lying down, where the working arm comes across close under the eye.
    pub arc_prone: f32,
    /// The same lying down, closer in, as the shoulders are further behind the eye.
    pub arm_wrist_prone: Vec3,
    /// How that elbow goes, turned with the chest: x = right, y = up, -z = forward.
    pub arm_elbow: Vec3,
    /// Where on the forearm and thigh a dressing goes, as a share of the bone from its start.
    pub arm_at: f32,
    pub thigh_at: f32,
    /// A wound on the trunk, in the chest's frame from its top.
    pub chest_at: Vec3,
    /// Where the hands go to treat someone else, from the eye along the look.
    pub ally_at: Vec3,
    /// Radians off the look at which a wound goes out of your own sight. Between these two, your hands
    /// go from the wound itself over to your forearm held up in view.
    pub out_of_sight: Vec2,
    /// Beyond the arm's reach by this much, as lying on your front and reaching for your leg is, the
    /// hands go to the forearm too.
    pub out_of_reach: f32,
    /// Where the working forearm comes up from, in the chest's frame from its top.
    pub right_arm_from: Vec3,
    /// How much the working hand turns out to the side of the limb it is on, against lying along the arm.
    pub hand_turn: f32,
    /// How much of its radius the roll has left at the end.
    pub roll_left: f32,
}

/// Lying where they fell: face down, the head level with where the eye was and the body stretched out
/// behind it, arms and legs slack.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CollapseConfig {
    /// Seconds to go down, which matches the time the eye takes on its way to the ground.
    pub fall_time: f32,
    /// Seconds to come back to a normal prone position when revived.
    pub rise_time: f32,
    /// How far above the ground the middle of the trunk and the limbs lie.
    pub trunk_height: f32,
    pub limb_height: f32,
    /// Forward from the eye to the top of the head, to bring the head off the gun.
    pub head_ahead: f32,
    /// The head rolled over to one side.
    pub head_roll: f32,
    /// Upper arm and forearm directions, in radians from straight out to the side, towards the feet.
    pub upper_arm_back: f32,
    pub forearm_back: f32,
    /// Thighs and shins out from straight back.
    pub thigh_out: f32,
    pub shin_out: f32,
    /// Toes pointing this far down from straight back.
    pub foot_down: f32,
    /// Added to the trunk's fall yaw, so a body does not lie perfectly along the look.
    pub lie_twist: f32,
}

/// A held position. Soldiers blend between these by how far down their eye has gone, which the sim
/// already eases, so stance changes need no animation state of their own.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct StancePose {
    pub hip_height: f32,
    /// Hips behind the eye along the lower body's forward.
    pub hip_back: f32,
    /// Pelvis tipped forward from upright. A quarter turn is lying flat.
    pub pelvis_tilt: f32,
    /// Ankles relative to the hips along the ground: x = right, y = forward.
    pub foot_left: Vec2,
    pub foot_right: Vec2,
    /// Toes turned out from straight ahead.
    pub foot_splay: f32,
    /// Toe pitch of each foot: negative points the toes down.
    pub toe_left: f32,
    pub toe_right: f32,
    /// How far above its usual height each ankle sits, to stay clear when the foot is pitched.
    pub ankle_left: f32,
    pub ankle_right: f32,
    /// Chest turned off the look, so the left shoulder leads: x = from the hip, y = at the sights.
    pub blade: Vec2,
    /// How far the look can twist off the hips before they are dragged along.
    pub twist_limit: f32,
    /// Knee direction: blends from forward at 0 to outward and down at 1.
    pub knee_down: f32,
    /// How far out from the leg the knees point.
    pub knee_out: f32,
    /// Elbows pushed out and away from the ground, 0 to 1.
    pub elbow_out: f32,
}

impl StancePose {
    pub fn lerp(&self, o: &Self, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        Self {
            hip_height: f(self.hip_height, o.hip_height),
            hip_back: f(self.hip_back, o.hip_back),
            pelvis_tilt: f(self.pelvis_tilt, o.pelvis_tilt),
            foot_left: self.foot_left.lerp(o.foot_left, t),
            foot_right: self.foot_right.lerp(o.foot_right, t),
            foot_splay: f(self.foot_splay, o.foot_splay),
            toe_left: f(self.toe_left, o.toe_left),
            toe_right: f(self.toe_right, o.toe_right),
            ankle_left: f(self.ankle_left, o.ankle_left),
            ankle_right: f(self.ankle_right, o.ankle_right),
            blade: self.blade.lerp(o.blade, t),
            twist_limit: f(self.twist_limit, o.twist_limit),
            knee_down: f(self.knee_down, o.knee_down),
            knee_out: f(self.knee_out, o.knee_out),
            elbow_out: f(self.elbow_out, o.elbow_out),
        }
    }
}

impl Default for BodyConfig {
    fn default() -> Self {
        Self {
            bones: Proportions {
                pelvis: 0.11,
                spine: 0.2,
                chest: 0.24,
                neck: 0.1,
                head: 0.2,
                shoulder: 0.15,
                upper_arm: 0.32,
                forearm: 0.27,
                hand: 0.09,
                thigh: 0.46,
                shin: 0.44,
                foot: 0.2,
                shoulder_root: Vec2::new(0.025, 0.035),
                hip_width: 0.1,
                ankle_height: 0.085,
                skull_from_eye: Vec3::new(0.0, -0.09, 0.085),
                pelvis_size: Vec2::new(0.165, 0.11),
                spine_size: Vec2::new(0.15, 0.1),
                chest_size: Vec2::new(0.175, 0.115),
                neck_size: Vec2::new(0.05, 0.055),
                head_size: Vec2::new(0.085, 0.1),
                shoulder_size: Vec2::new(0.055, 0.055),
                upper_arm_size: Vec2::new(0.052, 0.052),
                forearm_size: Vec2::new(0.042, 0.042),
                hand_size: Vec2::new(0.026, 0.044),
                thigh_size: Vec2::new(0.075, 0.08),
                shin_size: Vec2::new(0.055, 0.06),
                foot_size: Vec2::new(0.048, 0.04),
            },
            look: LookConfig {
                sit_twist: 1.1,
                sit_feet_apart: 0.14,
                hand_on_wheel: 0.05,
                ledge_hands_apart: 0.2,
                ledge_wrist_back: 0.08,
                ledge_wrist_up: 0.04,
                perched_feet: 0.55,
                brace_time: 0.3,
                sling_time: 0.35,
                skull_pitch_share: 0.2,
                neck_share: 0.3,
                neck_share_down: 0.6,
                tuck: Vec2::new(0.17, 0.05),
                tuck_pitch: Vec2::new(0.7, 1.4),
                arm_eye_clearance: 0.125,
                neck_share_prone: 0.8,
                hips_back_down: 0.1,
                hips_forward_up: 0.06,
                head_turn_limit: Vec2::new(1.3, 0.8),
                spine_stretch: 0.995,
                shoulder_reach: 0.7,
                support_slide: 0.13,
                lean_hips: 0.13,
                lean_head_roll: 0.3,
                throw_hand: Vec3::new(0.3, -0.05, 0.28),
                hand_at_wrist: Vec3::new(0.02, -0.06, 0.1),
                elbow_trigger: Vec3::new(0.95, -1.0, 0.3),
                elbow_support: Vec3::new(-0.45, -1.0, 0.1),
            },
            turn: TurnConfig {
                start: 0.75,
                stop: 0.12,
                hip_follow: 0.6,
                hip_follow_max: 0.95,
                rate: 7.0,
                step_angle: 0.9,
                step_lift: 0.06,
            },
            gait: GaitConfig {
                cycles_per_stride: 1.0,
                still_speed: 0.08,
                moving_speed: 0.7,
                blend_rate: 9.0,
                stand: [
                    GaitPoint { speed: 1.7, stance_length: 0.7, swing_height: 0.1, lean: 0.015, hip_drop: 0.012, bounce: 0.012, hip_swing: 0.1 },
                    GaitPoint { speed: 3.9, stance_length: 0.72, swing_height: 0.2, lean: 0.06, hip_drop: 0.035, bounce: 0.02, hip_swing: 0.14 },
                    GaitPoint { speed: 6.2, stance_length: 0.82, swing_height: 0.3, lean: 0.17, hip_drop: 0.06, bounce: 0.03, hip_swing: 0.18 },
                ],
                crouch: [
                    GaitPoint { speed: 1.0, stance_length: 0.55, swing_height: 0.08, lean: 0.08, hip_drop: -0.05, bounce: 0.012, hip_swing: 0.08 },
                    GaitPoint { speed: 2.0, stance_length: 0.62, swing_height: 0.1, lean: 0.14, hip_drop: -0.08, bounce: 0.018, hip_swing: 0.12 },
                ],
                crawl_duty: 0.5,
                crawl_stroke: 0.3,
                crawl_wriggle: 0.07,
                crawl_lift: 0.03,
                track_width: 0.1,
                limp_bounce: 1.6,
                limp_swing: 0.9,
                heel_strike: 0.3,
                toe_off: -0.6,
            },
            air: AirConfig {
                rate: 12.0,
                rise_front: Vec3::new(0.11, -0.56, -0.2),
                rise_back: Vec3::new(0.11, -0.66, 0.2),
                fall_front: Vec3::new(0.12, -0.8, -0.12),
                fall_back: Vec3::new(0.12, -0.76, 0.14),
                fall_blend_speed: 3.5,
                land_dip: 0.028,
                land_dip_max: 0.18,
                land_stiffness: 120.0,
                land_damping: 14.0,
                land_min_speed: 1.0,
            },
            reload: ReloadConfig {
                at_magazine: 0.12,
                at_pouch: 0.32,
                leave_pouch: 0.46,
                back_at_magazine: 0.68,
                leave_magazine: 0.84,
                pouch: Vec3::new(-0.08, -0.27, -0.14),
            },
            lift: LiftPoseConfig { reach_time: 0.35, return_time: 0.4, chest_below_eye: 0.35, ahead: 0.25, apart: 0.27, elbow: Vec3::new(0.7, -1.0, 0.25), push_eye: 1.2, shoulder_drop: 0.3, shoulder_ahead: -0.2, arm_bend: 0.8 },
            care: CareConfig {
                reach_time: 0.3,
                return_time: 0.4,
                passes: 3.0,
                arc: 1.2,
                start_still: 0.08,
                end_still: 0.1,
                slide: 0.035,
                hand_gap: 0.03,
                hold_along: 0.09,
                arm_wrist: Vec3::new(0.03, -0.12, -0.46),
                arc_prone: 0.55,
                arm_wrist_prone: Vec3::new(0.03, -0.12, -0.36),
                arm_elbow: Vec3::new(-1.0, -0.35, 0.1),
                arm_at: 0.68,
                thigh_at: 0.28,
                chest_at: Vec3::new(-0.03, -0.17, -0.17),
                ally_at: Vec3::new(0.0, -0.2, -0.5),
                out_of_sight: Vec2::new(0.42, 0.62),
                out_of_reach: 0.1,
                right_arm_from: Vec3::new(0.3, -0.35, -0.05),
                hand_turn: 0.4,
                roll_left: 0.55,
            },
            collapse: CollapseConfig {
                fall_time: 1.1,
                rise_time: 1.2,
                trunk_height: 0.14,
                limb_height: 0.07,
                head_ahead: 0.0,
                head_roll: 1.1,
                upper_arm_back: 0.65,
                forearm_back: 1.25,
                thigh_out: 0.13,
                shin_out: 0.2,
                foot_down: 1.0,
                lie_twist: 0.0,
            },
            stand: StancePose {
                hip_height: 0.895,
                hip_back: 0.14,
                pelvis_tilt: 0.08,
                foot_left: Vec2::new(-0.13, 0.21),
                foot_right: Vec2::new(0.14, 0.0),
                foot_splay: 0.3,
                toe_left: 0.0,
                toe_right: 0.0,
                ankle_left: 0.0,
                ankle_right: 0.0,
                blade: Vec2::new(0.6, 0.42),
                twist_limit: 1.1,
                knee_down: 0.0,
                knee_out: 0.15,
                elbow_out: 0.0,
            },
            heave: StancePose {
                hip_height: 0.78,
                hip_back: 0.6,
                pelvis_tilt: 0.65,
                foot_left: Vec2::new(-0.13, 0.12),
                foot_right: Vec2::new(0.14, -0.5),
                foot_splay: 0.15,
                toe_left: 0.0,
                toe_right: -0.35,
                ankle_left: 0.0,
                ankle_right: 0.04,
                blade: Vec2::new(0.0, 0.0),
                twist_limit: 0.5,
                knee_down: 0.0,
                knee_out: 0.1,
                elbow_out: 0.3,
            },
            brace: StancePose {
                hip_height: 0.52,
                hip_back: 0.5,
                pelvis_tilt: 0.75,
                foot_left: Vec2::new(-0.2, 0.3),
                foot_right: Vec2::new(0.2, -0.1),
                foot_splay: 0.3,
                toe_left: 0.0,
                toe_right: -0.5,
                ankle_left: 0.0,
                ankle_right: 0.04,
                blade: Vec2::new(0.3, 0.3),
                twist_limit: 1.0,
                knee_down: 0.0,
                knee_out: 0.25,
                elbow_out: 0.2,
            },
            crouch: StancePose {
                hip_height: 0.4,
                hip_back: 0.42,
                pelvis_tilt: 0.3,
                foot_left: Vec2::new(-0.14, 0.38),
                foot_right: Vec2::new(0.15, -0.14),
                foot_splay: 0.25,
                toe_left: 0.0,
                toe_right: -0.75,
                ankle_left: 0.0,
                ankle_right: 0.055,
                blade: Vec2::new(0.55, 0.42),
                twist_limit: 1.0,
                knee_down: 0.0,
                knee_out: 0.2,
                elbow_out: 0.1,
            },
            prone: StancePose {
                hip_height: 0.16,
                hip_back: 0.7,
                pelvis_tilt: 1.5,
                foot_left: Vec2::new(-0.17, -0.87),
                foot_right: Vec2::new(0.17, -0.87),
                foot_splay: 0.15,
                toe_left: -1.0,
                toe_right: -1.0,
                ankle_left: 0.09,
                ankle_right: 0.09,
                blade: Vec2::new(0.3, 0.2),
                twist_limit: 0.4,
                knee_down: 1.0,
                knee_out: 0.7,
                elbow_out: 0.9,
            },
        }
    }
}
