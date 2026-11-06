//! From a soldier's state to world-space bones. The chain runs from the fixed points inwards: the eye
//! places the head and chest, the ground places the feet, the gun places the hands, and the joints
//! between them work out how to fit.

use crate::anim::{BodyAnim, CareSite, wrap_angle};
use crate::config::{BodyConfig, GaitPoint, StancePose};
use crate::ik::{frame, two_bone};
use glam::{EulerRot, Quat, Vec2, Vec3};
use std::ops::Index;

const PI: f32 = std::f32::consts::PI;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bone {
    Pelvis,
    Spine,
    Chest,
    Neck,
    Head,
    ShoulderL,
    UpperArmL,
    ForearmL,
    HandL,
    ShoulderR,
    UpperArmR,
    ForearmR,
    HandR,
    ThighL,
    ShinL,
    FootL,
    ThighR,
    ShinR,
    FootR,
}

impl Bone {
    pub const COUNT: usize = 19;
    pub const ALL: [Bone; Self::COUNT] = [
        Bone::Pelvis,
        Bone::Spine,
        Bone::Chest,
        Bone::Neck,
        Bone::Head,
        Bone::ShoulderL,
        Bone::UpperArmL,
        Bone::ForearmL,
        Bone::HandL,
        Bone::ShoulderR,
        Bone::UpperArmR,
        Bone::ForearmR,
        Bone::HandR,
        Bone::ThighL,
        Bone::ShinL,
        Bone::FootL,
        Bone::ThighR,
        Bone::ShinR,
        Bone::FootR,
    ];

    /// The hierarchy. The pelvis is the root.
    pub fn parent(self) -> Option<Bone> {
        Some(match self {
            Bone::Pelvis => return None,
            Bone::Spine | Bone::ThighL | Bone::ThighR => Bone::Pelvis,
            Bone::Chest => Bone::Spine,
            Bone::Neck | Bone::ShoulderL | Bone::ShoulderR => Bone::Chest,
            Bone::Head => Bone::Neck,
            Bone::UpperArmL => Bone::ShoulderL,
            Bone::ForearmL => Bone::UpperArmL,
            Bone::HandL => Bone::ForearmL,
            Bone::UpperArmR => Bone::ShoulderR,
            Bone::ForearmR => Bone::UpperArmR,
            Bone::HandR => Bone::ForearmR,
            Bone::ShinL => Bone::ThighL,
            Bone::FootL => Bone::ShinL,
            Bone::ShinR => Bone::ThighR,
            Bone::FootR => Bone::ShinR,
        })
    }

    pub fn length(self, cfg: &BodyConfig) -> f32 {
        let b = &cfg.bones;
        match self {
            Bone::Pelvis => b.pelvis,
            Bone::Spine => b.spine,
            Bone::Chest => b.chest,
            Bone::Neck => b.neck,
            Bone::Head => b.head,
            Bone::ShoulderL | Bone::ShoulderR => b.shoulder,
            Bone::UpperArmL | Bone::UpperArmR => b.upper_arm,
            Bone::ForearmL | Bone::ForearmR => b.forearm,
            Bone::HandL | Bone::HandR => b.hand,
            Bone::ThighL | Bone::ThighR => b.thigh,
            Bone::ShinL | Bone::ShinR => b.shin,
            Bone::FootL | Bone::FootR => b.foot,
        }
    }

    /// Half width and half depth of the segment at its thickest.
    pub fn size(self, cfg: &BodyConfig) -> Vec2 {
        let b = &cfg.bones;
        match self {
            Bone::Pelvis => b.pelvis_size,
            Bone::Spine => b.spine_size,
            Bone::Chest => b.chest_size,
            Bone::Neck => b.neck_size,
            Bone::Head => b.head_size,
            Bone::ShoulderL | Bone::ShoulderR => b.shoulder_size,
            Bone::UpperArmL | Bone::UpperArmR => b.upper_arm_size,
            Bone::ForearmL | Bone::ForearmR => b.forearm_size,
            Bone::HandL | Bone::HandR => b.hand_size,
            Bone::ThighL | Bone::ThighR => b.thigh_size,
            Bone::ShinL | Bone::ShinR => b.shin_size,
            Bone::FootL | Bone::FootR => b.foot_size,
        }
    }
}

/// One bone in the world. Its +Y runs from the joint at `start` towards its child, and -Z is its
/// front, the same way a person is drawn facing the camera's forward.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoneTransform {
    pub start: Vec3,
    pub rotation: Quat,
    pub length: f32,
}

impl BoneTransform {
    pub fn end(&self) -> Vec3 {
        self.start + self.rotation * Vec3::Y * self.length
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub bones: [BoneTransform; Bone::COUNT],
}

impl Index<Bone> for Pose {
    type Output = BoneTransform;
    fn index(&self, bone: Bone) -> &BoneTransform {
        &self.bones[bone as usize]
    }
}

/// Points on the gun the hands go to, in the gun's frame: x = right, y = up, -z = towards the muzzle.
#[derive(Clone, Copy, Debug)]
pub struct Grips {
    pub trigger: Vec3,
    pub trigger_wrist: Vec3,
    pub support: Vec3,
    pub support_wrist: Vec3,
    pub magazine: Vec3,
}

/// The gun as it is, wherever that came from. Everyone else's is `gun_pose` straight from the sim,
/// and your own is whatever the view actually drew, so hands never come off it.
#[derive(Clone, Copy, Debug)]
pub struct GunHold {
    pub origin: Vec3,
    pub rotation: Quat,
    pub grips: Grips,
}

impl GunHold {
    fn world(&self, point: Vec3) -> Vec3 {
        self.origin + self.rotation * point
    }
}

/// Sitting in something. Where the seat is, in the world, in place of the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SitPose {
    pub hips: Vec3,
    /// Halfway between the feet, at the ankles.
    pub feet: Vec3,
    /// Which way the seat faces. The body twists from here to look.
    pub yaw: f32,
    /// On the controls, left then right. Otherwise the hands stay on the gun.
    pub hands: Option<[Vec3; 2]>,
}

/// Hands going to a ledge on the way up something. The gun stays where the sim says it is, lowered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgeHold {
    /// The edge of the ledge, in the middle.
    pub ledge: Vec3,
    /// Which way the climb is going.
    pub yaw: f32,
    /// How far the hands are on the ledge rather than the gun, 0 to 1.
    pub grip: f32,
    /// A quick hop over only takes the one hand.
    pub both: bool,
}

/// A soldier as the body sees it.
#[derive(Clone, Copy, Debug)]
pub struct BodyInput {
    /// The ground reference, directly below the eye when not leaning.
    pub feet: Vec3,
    /// Fixed. The body fits itself to this and never the other way round.
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Head turned off the look. Only the head moves, and the sim knows nothing of it.
    pub head_turn: Vec2,
    /// -1 = fully left, 1 = fully right. The eye and gun have already gone over: this is the hips and head.
    pub lean: f32,
    /// How far the throwing arm has gone back, 0 to 1.
    pub throw: f32,
    /// Where the throwing hand closes, instead, for someone looking out of these eyes at it.
    pub hand_at: Option<Vec3>,
    pub velocity: Vec3,
    pub grounded: bool,
    pub stride: f32,
    /// Ground covered by one `stride` at the moment.
    pub stride_length: f32,
    /// 0 = standing, 1 = fully crouched.
    pub crouch: f32,
    /// 0 = at most crouched, 1 = lying flat.
    pub prone: f32,
    /// 0 = hip, 1 = shouldered.
    pub ads: f32,
    /// 0 = lowered, 1 = ready.
    pub ready: f32,
    /// How far through a reload, or 0 when not reloading.
    pub reload: f32,
    /// How badly the legs are hurt, 0 to 1, which puts a limp in the walk.
    pub limp: f32,
    pub gun: GunHold,
    pub anim: BodyAnim,
    pub sit: Option<SitPose>,
    pub ledge: Option<LedgeHold>,
    /// Standing on someone's back, so the feet are close together and there is no ground under them.
    pub perched: bool,
    /// Someone is looking out of these eyes, so a treatment they could not see goes where they can.
    pub first_person: bool,
    /// Heaving at something: where the hands go on it, left then right.
    pub push: Option<[Vec3; 2]>,
}

/// A spot of ground under a foot or the hips.
#[derive(Clone, Copy, Debug)]
pub struct GroundHit {
    pub height: f32,
    pub normal: Vec3,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn gait_point_lerp(a: GaitPoint, b: GaitPoint, t: f32) -> GaitPoint {
    GaitPoint {
        speed: lerp(a.speed, b.speed, t),
        stance_length: lerp(a.stance_length, b.stance_length, t),
        swing_height: lerp(a.swing_height, b.swing_height, t),
        lean: lerp(a.lean, b.lean, t),
        hip_drop: lerp(a.hip_drop, b.hip_drop, t),
        bounce: lerp(a.bounce, b.bounce, t),
        hip_swing: lerp(a.hip_swing, b.hip_swing, t),
    }
}

/// Reads a curve of gait points by speed, holding at both ends.
fn sample_curve(points: &[GaitPoint], speed: f32) -> GaitPoint {
    let mut current = points[0];
    for pair in points.windows(2) {
        if speed >= pair[0].speed {
            let span = (pair[1].speed - pair[0].speed).max(1e-4);
            current = gait_point_lerp(pair[0], pair[1], ((speed - pair[0].speed) / span).min(1.0));
        }
    }
    current
}

fn nearest_on_segment(a: Vec3, b: Vec3, point: Vec3) -> Vec3 {
    let along = b - a;
    let t = (point - a).dot(along) / along.length_squared().max(1e-6);
    a + along * t.clamp(0.0, 1.0)
}

/// `dir` with any part along `axis` taken out, as a unit vector.
fn perpendicular_to(dir: Vec3, axis: Vec3) -> Vec3 {
    let off = dir - axis * dir.dot(axis);
    off.normalize_or(axis.any_orthonormal_vector())
}

fn yaw_rotation(yaw: f32) -> Quat {
    Quat::from_rotation_y(yaw)
}

fn look_rotation(yaw: f32, pitch: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

fn bone(cfg: &BodyConfig, which: Bone, start: Vec3, rotation: Quat) -> BoneTransform {
    BoneTransform { start, rotation, length: which.length(cfg) }
}

/// The body's pose this instant. `ground` answers for the terrain under a point, and None means
/// nothing is there.
pub fn solve(cfg: &BodyConfig, input: &BodyInput, ground: &dyn Fn(Vec3) -> Option<GroundHit>) -> Pose {
    let solver = Solver::new(cfg, input, ground);
    let mut pose = Pose::default();
    let torso = solver.torso(&mut pose);
    solver.arms(&torso, &mut pose);
    solver.legs(&torso, &mut pose);
    solver.care(&torso, &mut pose);
    solver.heave(&torso, &mut pose);
    if input.anim.collapse > 0.0 {
        let lying = solver.lying();
        if input.anim.collapse >= 1.0 {
            return lying;
        }
        let t = smooth(input.anim.collapse);
        for (bone, end) in pose.bones.iter_mut().zip(lying.bones) {
            bone.start = bone.start.lerp(end.start, t);
            bone.rotation = bone.rotation.slerp(end.rotation, t);
        }
    }
    pose
}

struct Solver<'a> {
    cfg: &'a BodyConfig,
    input: &'a BodyInput,
    ground: &'a dyn Fn(Vec3) -> Option<GroundHit>,
    stance: StancePose,
    gait: GaitPoint,
    hip_fwd: Vec3,
    hip_right: Vec3,
    /// Where the legs' cycle is: 0 to 1 is one full cycle of the left leg.
    phase: f32,
}

/// What arms and legs need to know about the torso they attach to.
struct Torso {
    hips: Vec3,
    /// Hips on the ground plane before look pitch shifted them, so looking down leaves the feet alone.
    settled: Vec3,
    pelvis_right: Vec3,
    chest_end: Vec3,
    chest_rotation: Quat,
    /// The way the chest faces, ignoring how far it leans. Elbows hang from this, so a soldier
    /// bending forward does not swing them up in front of the face.
    chest_facing: Quat,
    floor: f32,
}

impl<'a> Solver<'a> {
    fn new(cfg: &'a BodyConfig, input: &'a BodyInput, ground: &'a dyn Fn(Vec3) -> Option<GroundHit>) -> Self {
        let (crouch, prone) = (input.crouch.clamp(0.0, 1.0), input.prone.clamp(0.0, 1.0));
        let stance = cfg.stand.lerp(&cfg.crouch, crouch).lerp(&cfg.prone, prone).lerp(&cfg.brace, input.anim.brace * crouch);
        let stance = if input.push.is_some() { stance.lerp(&cfg.heave, smooth(input.anim.heave) * (1.0 - prone)) } else { stance };
        let speed = Vec2::new(input.velocity.x, input.velocity.z).length();
        let stand = sample_curve(&cfg.gait.stand, speed);
        let low = sample_curve(&cfg.gait.crouch, speed);
        let upright = gait_point_lerp(stand, low, crouch);
        let still = GaitPoint { speed, stance_length: cfg.gait.crawl_stroke, swing_height: cfg.gait.crawl_lift, lean: 0.0, hip_drop: 0.0, bounce: 0.0, hip_swing: cfg.gait.crawl_wriggle };
        let mut gait = gait_point_lerp(upright, still, prone);
        gait.bounce *= 1.0 + input.limp * cfg.gait.limp_bounce;
        gait.hip_swing *= 1.0 + input.limp * cfg.gait.limp_swing;
        let hip = yaw_rotation(input.anim.hip_yaw);
        Self {
            cfg,
            input,
            ground,
            stance,
            gait,
            hip_fwd: hip * Vec3::NEG_Z,
            hip_right: hip * Vec3::X,
            phase: input.stride * cfg.gait.cycles_per_stride,
        }
    }

    /// Ground under a point, ignoring anything too far from where the soldier stands to be reached.
    fn ground_at(&self, at: Vec3) -> Option<GroundHit> {
        (self.ground)(at).filter(|hit| (hit.height - self.input.feet.y).abs() < 0.45)
    }

    fn torso(&self, pose: &mut Pose) -> Torso {
        let (cfg, input, anim) = (self.cfg, self.input, &self.input.anim);
        let (look, b) = (&cfg.look, &cfg.bones);
        let moving = anim.moving;
        let on_ground = 1.0 - anim.air;

        let cycle = self.phase.rem_euclid(1.0);
        let settled_back = self.stance.hip_back + self.gait.lean * moving;
        // From the feet and not the eye, so the view's bob cannot make a planted foot slip.
        let settled = input.feet - self.hip_fwd * settled_back;
        // Bending over pushes the hips back. Lying down there is nothing to bend.
        let pitch_shift = if input.pitch < 0.0 { -input.pitch * look.hips_back_down } else { -input.pitch * look.hips_forward_up }
            * (1.0 - input.prone);
        let hips_ground = settled - self.hip_fwd * pitch_shift;

        // Lying on a slope, the hips are on their own bit of ground and not at the height of the eye.
        let floor = match self.ground_at(hips_ground) {
            Some(hit) => lerp(input.feet.y, hit.height, input.prone * on_ground),
            None => input.feet.y,
        };
        let bob = self.gait.bounce * (cycle * TAU * 2.0).cos() * moving;
        let height = self.stance.hip_height - self.gait.hip_drop * moving - anim.land + bob;
        let look_right = yaw_rotation(input.yaw) * Vec3::X;
        let mut hips = Vec3::new(hips_ground.x, floor + height, hips_ground.z) + look_right * input.lean * look.lean_hips;
        if let Some(sit) = input.sit {
            hips = sit.hips;
        }

        let tilt = self.stance.pelvis_tilt + self.gait.lean * moving;
        let pelvis_up = Vec3::Y * tilt.cos() + self.hip_fwd * tilt.sin();
        let swing = self.gait.hip_swing * (cycle * TAU).sin() * moving;
        let pelvis_right = yaw_rotation(anim.hip_yaw + swing) * Vec3::X;

        // Down from the eye: skull, neck, then whatever the spine has to do to meet the hips.
        let roll = -input.lean * look.lean_head_roll;
        let nod = Quat::from_euler(EulerRot::YXZ, input.yaw, input.pitch * look.skull_pitch_share, roll);
        let tuck_span = (look.tuck_pitch.y - look.tuck_pitch.x).max(1e-3);
        let tuck = smooth((-input.pitch - look.tuck_pitch.x) / tuck_span) * (1.0 - input.prone);
        let look_flat = yaw_rotation(input.yaw) * Vec3::NEG_Z;
        let skull = input.eye + nod * b.skull_from_eye - (look_flat * look.tuck.x + Vec3::Y * look.tuck.y) * tuck;
        let look_rot = look_rotation(input.yaw, input.pitch);
        let mut lumbar = hips + pelvis_up * b.pelvis;
        let lean_axis = (skull - lumbar).normalize_or(Vec3::Y);
        let neck_axis = lean_axis
            .lerp(look_rot * Vec3::Y, lerp(look.neck_share, look.neck_share_down, smooth(-input.pitch)))
            .lerp(Vec3::Y, look.neck_share_prone * input.prone)
            .normalize_or(Vec3::Y);
        let neck_base = skull - neck_axis * b.neck;

        // A back is straight, so if the hips are too close to the neck they move back until the spine
        // is stretched, which leans the torso forward from the hips instead of folding it at the waist.
        let spine_length = b.spine + b.chest;
        let stretched = spine_length * look.spine_stretch;
        let to_neck = neck_base - lumbar;
        if to_neck.length() < stretched {
            let along = to_neck.dot(self.hip_fwd);
            let back = -along + (along * along - to_neck.length_squared() + stretched * stretched).sqrt();
            lumbar -= self.hip_fwd * back;
            hips -= self.hip_fwd * back;
        }

        // The spine is all there is between the fixed eye and the hips, so if it cannot stretch that
        // far the hips come up. Legs have give, and the eye does not.
        let longest = spine_length * 0.998;
        if lumbar.distance(neck_base) > longest {
            lumbar = neck_base + (lumbar - neck_base).normalize_or(Vec3::NEG_Y) * longest;
            hips = lumbar - pelvis_up * b.pelvis;
        }

        let back = Vec3::Y * tilt.sin() - self.hip_fwd * tilt.cos();
        let (chest_start, chest_end) = two_bone(lumbar, neck_base, b.spine, b.chest, back);

        let ads = smooth(input.ads);
        let blade = lerp(self.stance.blade.x, self.stance.blade.y, ads) * smooth(input.ready);
        let chest_yaw = match input.sit {
            // A belted body only turns so far. The head does the rest.
            Some(sit) => sit.yaw + wrap_angle(input.yaw - blade - sit.yaw).clamp(-look.sit_twist, look.sit_twist),
            None => input.yaw - blade - swing * 0.6,
        };
        let chest_right = yaw_rotation(chest_yaw) * Vec3::X;
        let spine_yaw = anim.hip_yaw + swing + wrap_angle(chest_yaw - anim.hip_yaw - swing) * 0.5;
        let chest_rotation = frame(chest_end - chest_start, chest_right);

        pose.bones[Bone::Pelvis as usize] = bone(cfg, Bone::Pelvis, hips, frame(pelvis_up, pelvis_right));
        pose.bones[Bone::Spine as usize] =
            bone(cfg, Bone::Spine, lumbar, frame(chest_start - lumbar, yaw_rotation(spine_yaw) * Vec3::X));
        pose.bones[Bone::Chest as usize] = bone(cfg, Bone::Chest, chest_start, chest_rotation);
        pose.bones[Bone::Neck as usize] = bone(cfg, Bone::Neck, chest_end, frame(skull - chest_end, chest_right));

        let limit = look.head_turn_limit;
        let turn = input.head_turn.clamp(-limit, limit);
        let head_start = pose[Bone::Neck].end();
        pose.bones[Bone::Head as usize] =
            bone(cfg, Bone::Head, head_start, Quat::from_euler(EulerRot::YXZ, input.yaw + turn.x, input.pitch + turn.y, roll));

        Torso { hips, settled, pelvis_right, chest_end, chest_rotation, chest_facing: yaw_rotation(chest_yaw), floor }
    }

    fn arms(&self, torso: &Torso, pose: &mut Pose) {
        let (cfg, input) = (self.cfg, self.input);
        let gun = &input.gun;
        let g = &gun.grips;

        if let Some([left, right]) = input.sit.and_then(|sit| sit.hands) {
            let wrist = Vec3::Y * cfg.look.hand_on_wheel;
            self.arm(torso, pose, 1.0, right + wrist, right, cfg.look.elbow_trigger, [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR], false);
            self.arm(torso, pose, -1.0, left + wrist, left, cfg.look.elbow_support, [Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL], false);
            return;
        }
        let mut trigger = (gun.world(g.trigger), gun.world(g.trigger_wrist));
        if input.throw > 0.0 {
            let back = input.eye + yaw_rotation(input.yaw) * cfg.look.throw_hand;
            let t = smooth(input.throw);
            trigger = (trigger.0.lerp(back + Vec3::Y * cfg.bones.hand, t), trigger.1.lerp(back, t));
        }
        if let Some(grip) = input.hand_at {
            trigger = (grip, grip + look_rotation(input.yaw, input.pitch) * cfg.look.hand_at_wrist);
        }
        let (mut support, mut support_wrist) = self.support_hand(torso);
        if let Some(hold) = input.ledge {
            let turn = yaw_rotation(hold.yaw);
            let share = smooth(hold.grip);
            let on_ledge = |side: f32| {
                let hand = hold.ledge + turn * Vec3::X * side * cfg.look.ledge_hands_apart;
                (hand, hand + turn * Vec3::Z * cfg.look.ledge_wrist_back + Vec3::Y * cfg.look.ledge_wrist_up)
            };
            let (hand, wrist) = on_ledge(-1.0);
            support = support.lerp(hand, share);
            support_wrist = support_wrist.lerp(wrist, share);
            if hold.both {
                let (hand, wrist) = on_ledge(1.0);
                trigger = (trigger.0.lerp(hand, share), trigger.1.lerp(wrist, share));
            }
        }
        self.arm(torso, pose, 1.0, trigger.1, trigger.0, cfg.look.elbow_trigger, [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR], false);
        self.arm(torso, pose, -1.0, support_wrist, support, cfg.look.elbow_support, [Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL], true);
    }

    /// Where the off hand wants to be: on the foregrip, or on its way round a reload.
    fn support_hand(&self, torso: &Torso) -> (Vec3, Vec3) {
        let (cfg, input) = (self.cfg, self.input);
        let gun = &input.gun;
        let g = &gun.grips;
        let wrist_offset = g.support_wrist - g.support;
        let foregrip = g.support;
        let at_magazine = gun.world(g.magazine);
        let at_foregrip = gun.world(foregrip);

        let r = &cfg.reload;
        let t = input.reload;
        if t <= 0.0 {
            return (at_foregrip, gun.world(g.support_wrist));
        }
        let pouch = torso.chest_end + torso.chest_rotation * r.pouch;
        let point = if t < r.at_magazine {
            at_foregrip.lerp(at_magazine, smooth(t / r.at_magazine))
        } else if t < r.at_pouch {
            at_magazine.lerp(pouch, smooth((t - r.at_magazine) / (r.at_pouch - r.at_magazine)))
        } else if t < r.leave_pouch {
            pouch
        } else if t < r.back_at_magazine {
            pouch.lerp(at_magazine, smooth((t - r.leave_pouch) / (r.back_at_magazine - r.leave_pouch)))
        } else if t < r.leave_magazine {
            at_magazine
        } else {
            at_magazine.lerp(at_foregrip, smooth((t - r.leave_magazine) / (1.0 - r.leave_magazine)))
        };
        (point, point + gun.rotation * wrist_offset)
    }

    /// One arm, from the chest to a hand on `grip`. `side` is 1 on the right and -1 on the left.
    #[allow(clippy::too_many_arguments)]
    fn arm(&self, torso: &Torso, pose: &mut Pose, side: f32, wrist: Vec3, grip: Vec3, elbow: Vec3, bones: [Bone; 4], slides: bool) {
        let (cfg, input) = (self.cfg, self.input);
        let (b, look) = (&cfg.bones, &cfg.look);
        let chest = torso.chest_rotation;
        let root = torso.chest_end + chest * Vec3::new(side * b.shoulder_root.x, -b.shoulder_root.y, 0.0);
        let rest = chest * Vec3::new(side, -0.1, 0.0).normalize();
        let reach = b.upper_arm + b.forearm;

        // A gun held far away slides the off hand back along the handguard before anything else.
        let (mut wrist, mut grip) = (wrist, grip);
        if slides {
            let over = (root + rest * b.shoulder).distance(wrist) - reach * 0.97;
            if over > 0.0 {
                let back = input.gun.rotation * Vec3::Z * over.min(look.support_slide);
                wrist += back;
                grip += back;
            }
        }

        // Then the shoulder comes round. It only swings as far as it needs to.
        let to_wrist = (wrist - root).normalize_or(rest);
        let over = (root + rest * b.shoulder).distance(wrist) - reach * 0.97;
        let swing = if over > 0.0 { (over / b.shoulder).min(look.shoulder_reach) } else { 0.0 };
        let axis = rest.cross(to_wrist).normalize_or_zero();
        let shoulder_dir = Quat::from_axis_angle(axis, swing.min(rest.angle_between(to_wrist))) * rest;
        let shoulder_end = root + shoulder_dir * b.shoulder;

        let outward = torso.chest_facing * Vec3::new(side, -0.15, 0.0);
        let hanging = torso.chest_facing * elbow;
        let pole = hanging.normalize_or(Vec3::NEG_Y).lerp(outward.normalize_or(Vec3::NEG_Y), self.stance.elbow_out);
        let (mut mid, mut end) = two_bone(shoulder_end, wrist, b.upper_arm, b.forearm, pole);
        // Lying down the elbow rests on the ground and does not go through it.
        let lowest = torso.floor + b.forearm_size.y;
        if mid.y < lowest {
            let flat = Vec3::new(pole.x, 0.4, pole.z);
            (mid, end) = two_bone(shoulder_end, wrist, b.upper_arm, b.forearm, flat);
        }
        // A gun held overhead can bring the arm across the face. Bend it away from the camera, and if
        // it is too straight to bend, let the hand fall just short of the gun rather than clip.
        let closest = |mid: Vec3, end: Vec3| {
            let upper = nearest_on_segment(shoulder_end, mid, input.eye);
            let lower = nearest_on_segment(mid, end, input.eye);
            if upper.distance(input.eye) < lower.distance(input.eye) { upper } else { lower }
        };
        let near = closest(mid, end);
        if near.distance(input.eye) < look.arm_eye_clearance {
            (mid, end) = two_bone(shoulder_end, wrist, b.upper_arm, b.forearm, near - input.eye);
            let near = closest(mid, end);
            let short = look.arm_eye_clearance - near.distance(input.eye);
            if short > 0.0 {
                // The arm pivots at the shoulder, so the wrist has to move further than the arm does.
                let lever = wrist.distance(shoulder_end) / near.distance(shoulder_end).max(0.05);
                wrist += (near - input.eye).normalize_or_zero() * short * lever;
                (mid, end) = two_bone(shoulder_end, wrist, b.upper_arm, b.forearm, near - input.eye);
            }
        }

        let chest_right = chest * Vec3::X;
        let shoulder_hint = chest * Vec3::NEG_Z;
        pose.bones[bones[0] as usize] = bone(cfg, bones[0], root, frame(shoulder_dir, shoulder_hint));
        pose.bones[bones[1] as usize] = bone(cfg, bones[1], shoulder_end, frame(mid - shoulder_end, chest_right));
        pose.bones[bones[2] as usize] = bone(cfg, bones[2], mid, frame(end - mid, chest_right));
        pose.bones[bones[3] as usize] = bone(cfg, bones[3], end, frame(grip - end, input.gun.rotation * Vec3::X));
    }

    fn legs(&self, torso: &Torso, pose: &mut Pose) {
        self.leg(torso, pose, -1.0, 0.0, self.stance.foot_left, self.stance.toe_left, self.stance.ankle_left, [Bone::ThighL, Bone::ShinL, Bone::FootL]);
        self.leg(torso, pose, 1.0, 0.5, self.stance.foot_right, self.stance.toe_right, self.stance.ankle_right, [Bone::ThighR, Bone::ShinR, Bone::FootR]);
    }

    /// Where a foot is in its cycle: how far along the line of travel from under the hips, how high
    /// it has lifted, and the toe pitch that goes with it.
    fn gait_foot(&self, offset: f32) -> (f32, f32, f32) {
        let cfg = self.cfg;
        let g = &cfg.gait;
        let input = self.input;
        let cycle_length = (input.stride_length / g.cycles_per_stride.max(1e-3)).max(1e-3);
        // Planted, a foot travels back past the hips at exactly walking speed, which is the whole
        // trick to feet that do not skate: the planted share is the stride it covers over the cycle.
        let walk_duty = (self.gait.stance_length / cycle_length).clamp(0.15, 0.85);
        let duty = lerp(walk_duty, g.crawl_duty, input.prone);
        let t = (self.phase + offset).rem_euclid(1.0);
        let stance = self.gait.stance_length;
        if t < duty {
            let u = t / duty;
            let toe = if u < 0.2 {
                g.heel_strike * (1.0 - u / 0.2)
            } else if u > 0.6 {
                g.toe_off * ((u - 0.6) / 0.4)
            } else {
                0.0
            };
            (stance * (0.5 - u), 0.0, toe)
        } else {
            let u = (t - duty) / (1.0 - duty);
            let along = stance * (smooth(u) - 0.5);
            let lift = (u * PI).sin() * self.gait.swing_height;
            (along, lift, lerp(g.toe_off, g.heel_strike, u))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn leg(&self, torso: &Torso, pose: &mut Pose, side: f32, offset: f32, rest: Vec2, rest_toe: f32, rest_ankle: f32, bones: [Bone; 3]) {
        let (cfg, input, anim) = (self.cfg, self.input, &self.input.anim);
        let b = &cfg.bones;
        let stance = &self.stance;
        let (along, gait_lift, gait_toe) = self.gait_foot(offset);

        let spread = if input.perched { cfg.look.perched_feet } else { 1.0 };
        let rest_offset = (self.hip_right * rest.x + self.hip_fwd * rest.y) * spread;
        let travel = Vec3::new(anim.travel.x, 0.0, anim.travel.y);
        let walking = self.hip_right * side * cfg.gait.track_width + travel * along;
        let crawling = rest_offset + self.hip_fwd * along;
        let moved = walking.lerp(crawling, input.prone);
        let flat = torso.settled + rest_offset.lerp(moved, anim.moving);

        let turn_step = (((anim.turn_phase + offset) * TAU).sin()).max(0.0) * cfg.turn.step_lift * anim.turn_weight;
        let lift = gait_lift * anim.moving + turn_step;
        let hit = if input.sit.is_some() { None } else { self.ground_at(flat) };
        let floor = hit.map_or(input.feet.y, |h| h.height);
        let on_ground = match input.sit {
            Some(sit) => sit.feet + self.hip_right * side * cfg.look.sit_feet_apart,
            None => Vec3::new(flat.x, floor + b.ankle_height + rest_ankle + lift, flat.z),
        };

        let a = &cfg.air;
        let fall = self.fall_blend();
        let (front, back_leg) = (a.rise_front.lerp(a.fall_front, fall), a.rise_back.lerp(a.fall_back, fall));
        let air = if side < 0.0 { front } else { back_leg };
        let in_air = torso.hips + self.hip_right * side * air.x + Vec3::Y * air.y - self.hip_fwd * air.z;
        let ankle_target = on_ground.lerp(in_air, anim.air);

        let hip = torso.hips + torso.pelvis_right * side * b.hip_width;
        let out = self.hip_right * side * stance.knee_out;
        let standing_knee = self.hip_fwd + out;
        let lying_knee = Vec3::NEG_Y + out;
        let knee_pole = standing_knee.lerp(lying_knee, stance.knee_down);
        let (knee, ankle) = two_bone(hip, ankle_target, b.thigh, b.shin, knee_pole);

        // A leg that cannot reach the ground goes up on its toes rather than leaving the floor.
        let pulled_up = (ankle.y - ankle_target.y).max(0.0) * (1.0 - anim.air) * (1.0 - input.prone);
        let tiptoe = (pulled_up / b.foot).min(1.0).asin();
        let toe_pitch = rest_toe * (1.0 - anim.moving * (1.0 - input.prone)) + gait_toe * anim.moving * (1.0 - input.prone) - tiptoe;
        let splay = stance.foot_splay * (1.0 - anim.moving * (1.0 - input.prone)) * -side;
        let heading = yaw_rotation(anim.hip_yaw + splay) * Vec3::NEG_Z;
        let normal = if anim.air < 0.5 { hit.map_or(Vec3::Y, |h| h.normal) } else { Vec3::Y };
        let level = (heading - normal * heading.dot(normal)).normalize_or(heading);
        let foot_right = level.cross(normal).normalize_or(self.hip_right);
        let toe = level * toe_pitch.cos() + normal * toe_pitch.sin();

        let thigh_right = -self.hip_right;
        pose.bones[bones[0] as usize] = bone(cfg, bones[0], hip, frame(knee - hip, thigh_right));
        pose.bones[bones[1] as usize] = bone(cfg, bones[1], knee, frame(ankle - knee, thigh_right));
        pose.bones[bones[2] as usize] = bone(cfg, bones[2], ankle, frame(toe, -foot_right));
    }

    /// Both hands up on a hull, arms braced, whatever the gun is doing on its sling.
    fn heave(&self, torso: &Torso, pose: &mut Pose) {
        let (cfg, input) = (self.cfg, self.input);
        let Some(hands) = input.push.filter(|_| input.anim.heave > 0.0) else {
            return;
        };
        let amount = smooth(input.anim.heave);
        let b = &cfg.bones;
        let sides = [
            (-1.0, hands[0], Bone::HandL, [Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL]),
            (1.0, hands[1], Bone::HandR, [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR]),
        ];
        for (side, grip, hand, bones) in sides {
            let was = (pose[hand].start, pose[hand].end());
            let shoulder = torso.chest_end + torso.chest_rotation * Vec3::new(side * b.shoulder_root.x, -b.shoulder_root.y, 0.0);
            let reach = b.upper_arm + b.forearm + b.hand;
            // On the hull if it is in reach, and as far towards it as an arm goes if it has got away from them.
            let out = (grip - shoulder).clamp_length_max(reach);
            let (grip, wrist) = (shoulder + out, shoulder + out - out.normalize_or(Vec3::NEG_Z) * b.hand);
            self.arm(torso, pose, side, was.0.lerp(wrist, amount), was.1.lerp(grip, amount), cfg.lift.elbow, bones, false);
        }
    }

    /// The hands on a treatment. This goes last, since it can be the legs being treated, and takes the
    /// hands from wherever everything else put them, so at nothing it changes nothing.
    fn care(&self, torso: &Torso, pose: &mut Pose) {
        let (cfg, input, anim) = (self.cfg, self.input, &self.input.anim);
        if anim.care <= 0.0 {
            return;
        }
        let (c, b) = (&cfg.care, &cfg.bones);
        let amount = smooth(anim.care);
        let look = look_rotation(input.yaw, input.pitch);
        let left_bones = [Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL];
        let right_bones = [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR];
        let on_gun = |pose: &Pose, hand: Bone| (pose[hand].start, pose[hand].end());
        let (left_was, right_was) = (on_gun(pose, Bone::HandL), on_gun(pose, Bone::HandR));
        let mix = |was: (Vec3, Vec3), wrist: Vec3, grip: Vec3| (was.0.lerp(wrist, amount), was.1.lerp(grip, amount));

        // The wound itself: a spot, the way a bandage runs round it, and how far out the surface is.
        let right_of = torso.chest_rotation * Vec3::X;
        let wound = match anim.care_site {
            CareSite::Leg => {
                let thigh = pose[Bone::ThighL];
                Some((thigh.start.lerp(thigh.end(), c.thigh_at), thigh.rotation * Vec3::Y, b.thigh_size.x))
            }
            CareSite::Chest => Some((torso.chest_end + torso.chest_rotation * c.chest_at, right_of, 0.0)),
            CareSite::Ally => Some((input.eye + look * c.ally_at, look * Vec3::X, 0.0)),
            CareSite::Arm => None,
        };
        // How much the hands go to the wound rather than to the forearm, which they do when it cannot be
        // reached, or when it is yours and out of your sight.
        let share = wound.map_or(0.0, |(at, _, _)| {
            let shoulder = torso.chest_end + torso.chest_rotation * Vec3::new(-b.shoulder, -b.shoulder_root.y, 0.0);
            let reach = b.upper_arm + b.forearm + b.hand;
            let over = (at.distance(shoulder) - reach) / c.out_of_reach.max(1e-3);
            let within_reach = 1.0 - smooth(over);
            let within_sight = if input.first_person && anim.care_site != CareSite::Ally {
                let off = (at - input.eye).angle_between(look * Vec3::NEG_Z);
                1.0 - smooth((off - c.out_of_sight.x) / (c.out_of_sight.y - c.out_of_sight.x).max(1e-3))
            } else {
                1.0
            };
            // Lying down nobody gets a hand to their own leg or under their own chest, and nobody holds a
            // dressing to a thigh that is swinging, so walking on they work on the forearm too.
            let lying = if anim.care_site == CareSite::Ally { 0.0 } else { input.prone };
            let walking = if anim.care_site == CareSite::Leg { anim.moving } else { 0.0 };
            within_reach * within_sight * (1.0 - lying) * (1.0 - walking)
        });

        // The off hand first, as it may be the forearm that it belongs to that is being wrapped.
        let held_up = input.eye + look * c.arm_wrist.lerp(c.arm_wrist_prone, input.prone);
        let up_grip = held_up + (held_up - input.eye).normalize_or(look * Vec3::NEG_Z) * b.hand;
        let (wrist, grip) = match wound {
            Some((at, along, radius)) if share > 0.0 => {
                let toward = perpendicular_to(input.eye - at, along);
                let grip = at - along * c.hold_along + toward * (radius + c.hand_gap);
                let wrist = grip + (toward + (torso.chest_end - grip).normalize_or_zero()).normalize_or(toward) * b.hand;
                (held_up.lerp(wrist, share), up_grip.lerp(grip, share))
            }
            _ => (held_up, up_grip),
        };
        let (wrist, grip) = mix(left_was, wrist, grip);
        let elbow = cfg.look.elbow_support.lerp(c.arm_elbow, amount * (1.0 - share));
        self.arm(torso, pose, -1.0, wrist, grip, elbow, left_bones, false);

        let forearm = pose[Bone::ForearmL];
        let on_forearm = (forearm.start.lerp(forearm.end(), c.arm_at), forearm.rotation * Vec3::Y, b.forearm_size.x);
        let (at, along, radius) = match wound {
            Some((at, along, radius)) => (on_forearm.0.lerp(at, share), on_forearm.1.lerp(along, share).normalize_or(along), on_forearm.2 + (radius - on_forearm.2) * share),
            None => on_forearm,
        };

        // The roll goes back and forth over the side you can see, and holds still at either end.
        let span = (1.0 - c.start_still - c.end_still).max(1e-3);
        let working = ((anim.care_progress - c.start_still) / span).clamp(0.0, 1.0);
        let pass = (working * c.passes * TAU).sin();
        let toward = perpendicular_to(input.eye - at, along);
        let round = along.cross(toward);
        let angle = pass * lerp(c.arc, c.arc_prone, input.prone);
        let out = toward * angle.cos() + round * angle.sin();
        let grip = at + along * (working * 2.0 - 1.0) * c.slide + out * (radius + c.hand_gap);
        // The forearm comes up from the right shoulder and low, so it stays under the view with the hand
        // on the end of it, whichever side of the limb the roll is on.
        let from = torso.chest_end + torso.chest_rotation * c.right_arm_from;
        let wrist = grip + ((from - grip).normalize_or_zero() + out * c.hand_turn).normalize_or(out) * b.hand;
        let (wrist, grip) = mix(right_was, wrist, grip);
        self.arm(torso, pose, 1.0, wrist, grip, cfg.look.elbow_trigger, right_bones, false);
    }

    /// How a soldier lies after going down. Nothing here listens to the look or the gun, so it stays
    /// put whatever the player does with their view. Each joint finds its own bit of ground.
    fn lying(&self) -> Pose {
        let (cfg, input) = (self.cfg, self.input);
        let (c, b) = (&cfg.collapse, &cfg.bones);
        let yaw = input.anim.fall_yaw + c.lie_twist;
        let turn = yaw_rotation(yaw);
        let (ahead, right) = (turn * Vec3::NEG_Z, turn * Vec3::X);
        let lay = |at: Vec3, height: f32| {
            let floor = self.ground_at(at).map_or(input.feet.y, |hit| hit.height);
            Vec3::new(at.x, floor + height, at.z)
        };
        let mut pose = Pose::default();

        // The trunk first, laid out from the head back, so the head lies under the eye.
        let trunk = b.pelvis + b.spine + b.chest + b.neck + b.head;
        let head_top = lay(input.feet + ahead * c.head_ahead, c.trunk_height);
        let hips = lay(head_top - ahead * trunk, c.trunk_height);
        let along = (head_top - hips).normalize_or(ahead);
        let mut at = hips;
        for which in [Bone::Pelvis, Bone::Spine, Bone::Chest, Bone::Neck] {
            let placed = bone(cfg, which, at, frame(along, right));
            at = placed.end();
            pose.bones[which as usize] = placed;
        }
        // Rolled over onto one cheek.
        let head = frame(along, right) * Quat::from_rotation_y(c.head_roll);
        pose.bones[Bone::Head as usize] = bone(cfg, Bone::Head, at, head);

        let chest_end = pose[Bone::Chest].end();
        let limb = |pose: &mut Pose, which: Bone, start: Vec3, dir: Vec3, hint: Vec3| {
            let end = lay(start + dir * which.length(cfg), c.limb_height);
            let placed = bone(cfg, which, start, frame(end - start, hint));
            pose.bones[which as usize] = placed;
            placed.end()
        };
        for (side, bones) in [
            (1.0, [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR]),
            (-1.0, [Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL]),
        ] {
            let out = right * side;
            let root = chest_end + out * b.shoulder_root.x - along * b.shoulder_root.y;
            let shoulder = limb(&mut pose, bones[0], root, out, ahead);
            let swung = |angle: f32| out * angle.cos() - ahead * angle.sin();
            let elbow = limb(&mut pose, bones[1], shoulder, swung(c.upper_arm_back), right);
            let wrist = limb(&mut pose, bones[2], elbow, swung(c.forearm_back), right);
            limb(&mut pose, bones[3], wrist, swung(c.forearm_back), right);
        }
        for (side, bones) in [(1.0, [Bone::ThighR, Bone::ShinR, Bone::FootR]), (-1.0, [Bone::ThighL, Bone::ShinL, Bone::FootL])] {
            let out = right * side;
            let splayed = |amount: f32| (-ahead + out * amount).normalize_or(-ahead);
            let hip = hips + out * b.hip_width;
            let knee = limb(&mut pose, bones[0], hip, splayed(c.thigh_out), -right);
            let ankle = limb(&mut pose, bones[1], knee, splayed(c.shin_out), -right);
            let toes = -ahead * c.foot_down.cos() + Vec3::NEG_Y * c.foot_down.sin();
            pose.bones[bones[2] as usize] = bone(cfg, bones[2], ankle, frame(toes, right));
        }
        pose
    }

    fn fall_blend(&self) -> f32 {
        smooth((-self.input.velocity.y / self.cfg.air.fall_blend_speed.max(1e-3)).max(0.0))
    }
}
