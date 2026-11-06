use super::*;
use glam::{EulerRot, Quat, Vec2, Vec3};

fn flat(_: Vec3) -> Option<GroundHit> {
    Some(GroundHit { height: 0.0, normal: Vec3::Y })
}

const GRIPS: Grips = Grips {
    trigger: Vec3::new(0.0, -0.075, 0.07),
    trigger_wrist: Vec3::new(0.012, -0.092, 0.145),
    support: Vec3::new(0.0, -0.038, -0.22),
    support_wrist: Vec3::new(-0.03, -0.062, -0.15),
    magazine: Vec3::new(0.0, -0.13, -0.05),
};

/// The same rule the sim's `gun_pose` follows, so these tests need nothing above this crate.
fn gun(eye: Vec3, yaw: f32, pitch: f32, hold: Vec3) -> GunHold {
    let aim = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    GunHold { origin: eye + aim * hold, rotation: aim, grips: GRIPS }
}

const HIP: Vec3 = Vec3::new(0.14, -0.145, -0.34);
const ADS: Vec3 = Vec3::new(0.0, -0.058, -0.235);

#[derive(Clone, Copy)]
struct Case {
    eye_height: f32,
    crouch: f32,
    prone: f32,
    yaw: f32,
    pitch: f32,
    ads: f32,
}

impl Case {
    fn input(self) -> BodyInput {
        let feet = Vec3::new(1.0, 0.0, -2.0);
        let eye = feet + Vec3::Y * self.eye_height;
        let hold = HIP.lerp(ADS, self.ads);
        BodyInput {
            feet,
            eye,
            yaw: self.yaw,
            pitch: self.pitch,
            head_turn: Vec2::ZERO,
            lean: 0.0,
            throw: 0.0,
            hand_at: None,
            velocity: Vec3::ZERO,
            grounded: true,
            stride: 0.0,
            stride_length: 2.1,
            crouch: self.crouch,
            prone: self.prone,
            ads: self.ads,
            ready: 1.0,
            reload: 0.0,
            limp: 0.0,
            gun: gun(eye, self.yaw, self.pitch, hold),
            anim: BodyAnim::new(self.yaw),
            sit: None,
            ledge: None,
            perched: false,
            first_person: false,
            push: None,
        }
    }
}

const STAND: Case = Case { eye_height: 1.62, crouch: 0.0, prone: 0.0, yaw: 0.0, pitch: 0.0, ads: 0.0 };
const CROUCH: Case = Case { eye_height: 1.02, crouch: 1.0, ..STAND };
const PRONE: Case = Case { eye_height: 0.38, crouch: 1.0, prone: 1.0, ..STAND };

fn cases() -> Vec<Case> {
    let mut all = Vec::new();
    for stance in [STAND, CROUCH, PRONE] {
        for pitch in [-1.54, -1.0, -0.5, 0.0, 0.5, 1.0, 1.54] {
            for ads in [0.0, 1.0] {
                for yaw in [0.0, 1.1, -2.7] {
                    all.push(Case { pitch, ads, yaw, ..stance });
                }
            }
        }
    }
    all
}

fn hands_gap(pose: &Pose, input: &BodyInput) -> (f32, f32) {
    let g = &input.gun;
    let world = |p: Vec3| g.origin + g.rotation * p;
    let right = pose[Bone::HandR].start.distance(world(g.grips.trigger_wrist));
    let left = pose[Bone::HandL].start.distance(world(g.grips.support_wrist));
    (right, left)
}

/// How far `point` is outside a bone's box, ignoring its taper. Zero inside.
fn outside(pose: &Pose, bone: Bone, cfg: &BodyConfig, point: Vec3) -> f32 {
    let b = pose[bone];
    let local = b.rotation.inverse() * (point - b.start);
    let size = bone.size(cfg);
    let d = Vec3::new(
        local.x.abs() - size.x,
        (local.y - b.length * 0.5).abs() - b.length * 0.5,
        local.z.abs() - size.y,
    );
    d.max(Vec3::ZERO).length()
}

#[test]
fn every_bone_keeps_its_length_and_joins_its_parent() {
    let cfg = BodyConfig::default();
    for case in cases() {
        let input = case.input();
        let pose = solve(&cfg, &input, &flat);
        for bone in Bone::ALL {
            let b = pose[bone];
            assert!((b.length - bone.length(&cfg)).abs() < 1e-6);
            assert!(b.rotation.is_normalized(), "{bone:?}");
            if let Some(parent) = bone.parent() {
                let p = pose[parent];
                // Thighs and shoulders root on the side of their parent, everything else at its end.
                let (at, slack) = match bone {
                    Bone::ThighL | Bone::ThighR => (p.start, cfg.bones.hip_width),
                    Bone::ShoulderL | Bone::ShoulderR => (p.end(), cfg.bones.shoulder_root.length()),
                    _ => (p.end(), 0.0),
                };
                let gap = b.start.distance(at);
                assert!(gap < slack + 0.004, "{bone:?} is {gap} from its parent at pitch {}", case.pitch);
            }
        }
    }
}

#[test]
fn hands_stay_on_the_gun_at_any_sensible_aim() {
    let cfg = BodyConfig::default();
    for case in cases() {
        if case.pitch.abs() > 1.05 {
            continue;
        }
        let input = case.input();
        let pose = solve(&cfg, &input, &flat);
        let (right, left) = hands_gap(&pose, &input);
        assert!(right < 0.01, "trigger hand {right} off at {:?} pitch {}", case.eye_height, case.pitch);
        // The support hand may slide along the handguard, but it must stay on the gun.
        assert!(left <= cfg.look.support_slide + 0.03, "off hand {left} off at {} pitch {} ads {}", case.eye_height, case.pitch, case.ads);
    }
}

#[test]
fn hands_never_end_up_far_from_the_gun_even_looking_straight_up_or_down() {
    let cfg = BodyConfig::default();
    for case in cases() {
        let input = case.input();
        let pose = solve(&cfg, &input, &flat);
        let (right, left) = hands_gap(&pose, &input);
        assert!(right < 0.12, "trigger hand {right} off at eye {} pitch {}", case.eye_height, case.pitch);
        assert!(left < cfg.look.support_slide + 0.16, "off hand {left} off at eye {} pitch {}", case.eye_height, case.pitch);
    }
}

#[test]
fn feet_are_on_the_ground_at_rest() {
    let cfg = BodyConfig::default();
    for stance in [STAND, CROUCH] {
        for pitch in [-0.5, 0.0, 0.5] {
            let pose = solve(&cfg, &Case { pitch, ..stance }.input(), &flat);
            for foot in [Bone::FootL, Bone::FootR] {
                let ankle = pose[foot].start.y;
                let wanted = cfg.bones.ankle_height + 0.05;
                assert!(ankle >= cfg.bones.ankle_height - 0.005 && ankle < wanted + 0.05, "{foot:?} ankle at {ankle}");
            }
        }
    }
}

#[test]
fn looking_up_and_down_leaves_the_feet_where_they_are() {
    let cfg = BodyConfig::default();
    let level = solve(&cfg, &STAND.input(), &flat);
    // Far enough down the front foot may go up on its toes, which has its own test.
    for pitch in [-0.7, 1.0, 1.54] {
        let bent = solve(&cfg, &Case { pitch, ..STAND }.input(), &flat);
        for foot in [Bone::FootL, Bone::FootR] {
            let a = level[foot].start;
            let b = bent[foot].start;
            assert!(Vec2::new(a.x - b.x, a.z - b.z).length() < 1e-3, "{foot:?} moved with pitch {pitch}");
        }
    }
}

#[test]
fn nothing_goes_through_a_flat_floor() {
    let cfg = BodyConfig::default();
    for case in cases() {
        let pose = solve(&cfg, &case.input(), &flat);
        for bone in Bone::ALL {
            let b = pose[bone];
            // Arms and hands holding a gun low can get as low as the gun does, and lying down and
            // looking far down `gun_pose` puts the gun itself in the ground.
            let lying_arm = case.prone > 0.5 && case.pitch < -1.2 && matches!(bone, Bone::UpperArmL | Bone::UpperArmR);
            if lying_arm || matches!(bone, Bone::HandL | Bone::HandR | Bone::ForearmL | Bone::ForearmR) {
                continue;
            }
            let lowest = b.start.y.min(b.end().y);
            assert!(lowest > -0.005, "{bone:?} at {lowest}, eye {} pitch {}", case.eye_height, case.pitch);
        }
    }
}

#[test]
fn extreme_input_never_gives_nan() {
    let cfg = BodyConfig::default();
    for pitch in [-3.0, -1.57, 1.57, 3.0] {
        for stance in [STAND, CROUCH, PRONE] {
            for velocity in [Vec3::ZERO, Vec3::new(50.0, -30.0, 2.0)] {
                let mut input = Case { pitch, yaw: 1000.0, ..stance }.input();
                input.velocity = velocity;
                input.grounded = velocity == Vec3::ZERO;
                input.stride = 1234.5;
                input.reload = 0.4;
                input.head_turn = Vec2::new(9.0, -9.0);
                input.anim.moving = 1.0;
                input.anim.air = 0.5;
                let pose = solve(&cfg, &input, &flat);
                for b in pose.bones {
                    assert!(b.start.is_finite() && b.rotation.is_finite());
                }
            }
        }
    }
}

#[test]
fn the_same_state_always_gives_the_same_pose() {
    let cfg = BodyConfig::default();
    for case in cases().into_iter().step_by(7) {
        let input = case.input();
        assert_eq!(solve(&cfg, &input, &flat), solve(&cfg, &input, &flat));
    }
}

/// A camera at the eye has a near plane, so the visible body has to stay this far from it.
const NEAR_CLEARANCE: f32 = 0.05;

#[test]
fn nothing_visible_touches_the_camera_in_any_stance_or_look() {
    let cfg = BodyConfig::default();
    for case in cases() {
        let input = case.input();
        let pose = solve(&cfg, &input, &flat);
        for bone in Bone::ALL {
            // These two are not drawn for the person inside them.
            if matches!(bone, Bone::Head | Bone::Neck) {
                continue;
            }
            let clear = outside(&pose, bone, &cfg, input.eye);
            assert!(clear > NEAR_CLEARANCE, "{bone:?} is {clear} from the eye at eye {} pitch {} ads {}", case.eye_height, case.pitch, case.ads);
        }
    }
}

#[test]
fn looking_ahead_only_the_arms_can_be_on_screen() {
    let cfg = BodyConfig::default();
    let half_fov = 42.0_f32.to_radians(); // Well beyond the widest setting's vertical half.
    for stance in [STAND, CROUCH, PRONE] {
        for ads in [0.0, 1.0] {
            let input = Case { ads, ..stance }.input();
            let pose = solve(&cfg, &input, &flat);
            for bone in Bone::ALL {
                if matches!(bone, Bone::Head | Bone::Neck | Bone::UpperArmL | Bone::UpperArmR | Bone::ForearmL | Bone::ForearmR | Bone::HandL | Bone::HandR) {
                    continue;
                }
                for t in [0.0, 0.5, 1.0] {
                    let p = pose[bone].start.lerp(pose[bone].end(), t) - input.eye;
                    let ahead = -p.z;
                    if ahead <= 0.0 {
                        continue;
                    }
                    let up = (p.y + bone.size(&cfg).y).atan2(ahead);
                    assert!(up < -half_fov, "{bone:?} is {} deg below the view centre at eye {}", up.to_degrees(), stance.eye_height);
                }
            }
        }
    }
}

#[test]
fn nothing_gets_near_the_line_of_sight_through_the_sights() {
    let cfg = BodyConfig::default();
    for stance in [STAND, CROUCH, PRONE] {
        let input = Case { ads: 1.0, ..stance }.input();
        let pose = solve(&cfg, &input, &flat);
        for bone in Bone::ALL {
            if matches!(bone, Bone::Head | Bone::Neck) {
                continue;
            }
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let p = pose[bone].start.lerp(pose[bone].end(), t) - input.eye;
                if p.z < -0.1 {
                    let above = p.y + bone.size(&cfg).y;
                    assert!(above < -0.0, "{bone:?} reaches {above} above the line of sight at eye {}", stance.eye_height);
                }
            }
        }
    }
}

#[test]
fn free_look_only_turns_the_head() {
    let cfg = BodyConfig::default();
    let still = STAND.input();
    let looking = BodyInput { head_turn: Vec2::new(1.0, -0.5), ..still };
    let (a, b) = (solve(&cfg, &still, &flat), solve(&cfg, &looking, &flat));
    for bone in Bone::ALL {
        if bone != Bone::Head {
            assert_eq!(a[bone], b[bone], "{bone:?} moved");
        }
    }
    assert_ne!(a[Bone::Head].rotation, b[Bone::Head].rotation);
}

#[test]
fn a_planted_foot_does_not_skate() {
    let cfg = BodyConfig::default();
    let speed = 3.9;
    let stride_length = 2.1;
    let dt = 1.0 / 60.0;
    let mut input = STAND.input();
    input.velocity = Vec3::new(0.0, 0.0, -speed);
    input.anim.moving = 1.0;
    input.stride_length = stride_length;
    let mut planted_run = 0;
    let mut worst: f32 = 0.0;
    let mut last: Option<Vec3> = None;
    for tick in 0..240 {
        let distance = speed * dt * tick as f32;
        input.feet = Vec3::new(1.0, 0.0, -2.0 - distance);
        input.eye = input.feet + Vec3::Y * 1.62;
        input.stride = distance / stride_length;
        input.gun = gun(input.eye, 0.0, 0.0, HIP);
        let pose = solve(&cfg, &input, &flat);
        let ankle = pose[Bone::FootL].start;
        let on_floor = ankle.y < cfg.bones.ankle_height + 0.0005;
        match (on_floor, last) {
            (true, Some(before)) => {
                worst = worst.max(Vec2::new(ankle.x - before.x, ankle.z - before.z).length());
                planted_run += 1;
                last = Some(ankle);
            }
            (true, None) => last = Some(ankle),
            (false, _) => last = None,
        }
    }
    assert!(planted_run > 20, "the foot should spend time on the ground, got {planted_run} ticks");
    assert!(worst < 0.012, "a planted foot moved {worst} m in one tick");
}

#[test]
fn feet_follow_the_ground_under_them() {
    let cfg = BodyConfig::default();
    let hill = |at: Vec3| Some(GroundHit { height: at.x * 0.1, normal: Vec3::new(-0.1, 1.0, 0.0).normalize() });
    let input = Case { pitch: 0.0, ..STAND }.input();
    let pose = solve(&cfg, &input, &hill);
    let a = pose[Bone::FootR].start;
    let expect = a.x * 0.1 + cfg.bones.ankle_height;
    assert!((a.y - expect).abs() < 0.03, "ankle {} on ground wanting {expect}", a.y);
}

#[test]
fn the_hips_lag_the_look_and_catch_up() {
    let cfg = BodyConfig::default();
    let mut anim = BodyAnim::new(0.0);
    let input = |yaw| AnimInput { yaw, velocity: Vec3::ZERO, grounded: true, crouch: 0.0, prone: 0.0, down: false };
    for _ in 0..30 {
        anim.step(&cfg, &input(0.4), 1.0 / 60.0);
    }
    assert_eq!(anim.hip_yaw, 0.0, "a small look should not move the hips");
    for _ in 0..120 {
        anim.step(&cfg, &input(1.4), 1.0 / 60.0);
    }
    assert!((anim.hip_yaw - 1.4).abs() < cfg.turn.stop + 0.01, "hips at {}", anim.hip_yaw);
}

#[test]
fn a_look_flick_never_overtwists_the_spine() {
    let cfg = BodyConfig::default();
    let mut anim = BodyAnim::new(0.0);
    anim.step(&cfg, &AnimInput { yaw: 3.0, velocity: Vec3::ZERO, grounded: true, crouch: 0.0, prone: 0.0, down: false }, 1.0 / 60.0);
    assert!(wrap_angle(3.0 - anim.hip_yaw).abs() <= cfg.stand.twist_limit + 1e-3);
}

#[test]
fn landing_dips_the_hips_and_then_recovers() {
    let cfg = BodyConfig::default();
    let mut anim = BodyAnim::new(0.0);
    let dt = 1.0 / 60.0;
    let air = AnimInput { yaw: 0.0, velocity: Vec3::new(0.0, -5.0, 0.0), grounded: false, crouch: 0.0, prone: 0.0, down: false };
    let down = AnimInput { velocity: Vec3::ZERO, grounded: true, ..air };
    for _ in 0..20 {
        anim.step(&cfg, &air, dt);
    }
    assert!(anim.air > 0.9);
    let mut deepest: f32 = 0.0;
    for _ in 0..240 {
        anim.step(&cfg, &down, dt);
        deepest = deepest.max(anim.land);
    }
    assert!(deepest > 0.04, "a hard landing should show, got {deepest}");
    assert!(anim.land < 0.002 && anim.air < 0.01, "should have recovered");
}

#[test]
fn reloading_takes_the_off_hand_to_the_magazine_and_back() {
    let cfg = BodyConfig::default();
    let r = &cfg.reload;
    let input = STAND.input();
    let world = |p: Vec3| input.gun.origin + input.gun.rotation * p;
    let mid_magazine = (r.back_at_magazine + r.leave_magazine) * 0.5;
    let at = |t: f32| solve(&cfg, &BodyInput { reload: t, ..input }, &flat)[Bone::HandL].end();
    let tolerance = cfg.bones.hand + 0.02;
    assert!(at(r.at_magazine).distance(world(GRIPS.magazine)) < tolerance);
    assert!(at(mid_magazine).distance(world(GRIPS.magazine)) < tolerance);
    let off = at((r.at_pouch + r.leave_pouch) * 0.5);
    assert!(off.distance(world(GRIPS.magazine)) > 0.1, "should have gone to the pouch");
    let done = solve(&cfg, &input, &flat);
    assert!(at(0.9999).distance(done[Bone::HandL].end()) < 0.02, "should be back on the foregrip");
}

#[test]
fn a_gun_pushed_back_by_a_wall_keeps_the_arms_whole() {
    let cfg = BodyConfig::default();
    for stance in [STAND, CROUCH, PRONE] {
        for ads in [0.0, 1.0] {
            let mut input = Case { ads, ..stance }.input();
            input.gun.origin += input.gun.rotation * Vec3::Z * 0.5;
            let pose = solve(&cfg, &input, &flat);
            for bone in [Bone::UpperArmL, Bone::ForearmL, Bone::UpperArmR, Bone::ForearmR] {
                assert!(pose[bone].start.is_finite());
                assert!(pose[bone].start.distance(pose[bone].end()) - bone.length(&cfg) < 1e-3);
            }
            let (right, left) = hands_gap(&pose, &input);
            assert!(right < 0.02 && left < 0.02, "hands {right} {left} off a pulled-back gun");
        }
    }
}

#[test]
fn lying_on_a_slope_the_whole_body_follows_the_ground() {
    let cfg = BodyConfig::default();
    // Rising to the south, so behind a soldier facing north the ground is higher.
    let hill = |at: Vec3| Some(GroundHit { height: (at.z + 2.0) * 0.12, normal: Vec3::new(0.0, 1.0, -0.12).normalize() });
    let input = PRONE.input();
    let pose = solve(&cfg, &input, &hill);
    let hip_ground = (pose[Bone::Pelvis].start.z + 2.0) * 0.12;
    assert!(pose[Bone::Pelvis].start.y > hip_ground + 0.05, "hips under the hill");
    for foot in [Bone::FootL, Bone::FootR] {
        let ankle = pose[foot].start;
        let ground = (ankle.z + 2.0) * 0.12;
        assert!(ankle.y > ground + 0.05, "{foot:?} under the hill");
    }
}

#[test]
fn elbows_rest_on_the_floor_and_do_not_go_through_it_when_lying_down() {
    let cfg = BodyConfig::default();
    for ads in [0.0, 1.0] {
        for pitch in [-0.1, 0.0, 0.2] {
            let pose = solve(&cfg, &Case { ads, pitch, ..PRONE }.input(), &flat);
            for arm in [Bone::ForearmL, Bone::ForearmR] {
                let elbow = pose[arm].start.y;
                assert!(elbow > 0.0, "{arm:?} elbow at {elbow} with ads {ads} pitch {pitch}");
            }
        }
    }
}

#[test]
fn walking_never_brings_a_knee_up_into_the_view() {
    let cfg = BodyConfig::default();
    let half_fov = 42.0_f32.to_radians(); // The widest vertical field of view on offer.
    let gaits = [(STAND, 1.7, 2.1), (STAND, 3.9, 2.1), (STAND, 6.2, 2.9), (CROUCH, 1.0, 1.5), (CROUCH, 2.0, 1.5), (PRONE, 0.75, 1.5)];
    for (stance, speed, stride_length) in gaits {
        for step in 0..40 {
            let mut input = stance.input();
            input.velocity = Vec3::new(0.0, 0.0, -speed);
            input.stride = step as f32 / 40.0;
            input.stride_length = stride_length;
            input.anim.moving = 1.0;
            let pose = solve(&cfg, &input, &flat);
            for bone in [Bone::Pelvis, Bone::Spine, Bone::Chest, Bone::ThighL, Bone::ThighR, Bone::ShinL, Bone::ShinR, Bone::FootL, Bone::FootR] {
                for point in [pose[bone].start, pose[bone].end()] {
                    let p = point - input.eye;
                    let ahead = -p.z;
                    if ahead > 0.0 {
                        let up = (p.y + bone.size(&cfg).y).atan2(ahead);
                        assert!(up < -half_fov, "{bone:?} at {} deg when moving {speed} m/s at step {step}", up.to_degrees());
                    }
                }
            }
        }
    }
}

#[test]
fn elbows_stay_below_the_view_and_the_chest_stays_upright_at_every_gait() {
    let cfg = BodyConfig::default();
    let half_fov = 42.0_f32.to_radians();
    let gaits = [(STAND, 0.0, 2.1), (STAND, 1.7, 2.1), (STAND, 3.9, 2.1), (STAND, 6.2, 2.9), (CROUCH, 0.0, 1.5), (CROUCH, 1.0, 1.5), (CROUCH, 2.0, 1.5), (PRONE, 0.0, 1.5), (PRONE, 0.75, 1.5)];
    for (stance, speed, stride_length) in gaits {
        for ads in [0.0, 1.0] {
            for step in 0..20 {
                let mut input = Case { ads, ..stance }.input();
                input.velocity = Vec3::new(0.0, 0.0, -speed);
                input.stride = step as f32 / 20.0;
                input.stride_length = stride_length;
                input.anim.moving = if speed > 0.0 { 1.0 } else { 0.0 };
                let pose = solve(&cfg, &input, &flat);
                for forearm in [Bone::ForearmL, Bone::ForearmR] {
                    let p = pose[forearm].start - input.eye;
                    if -p.z > 0.0 {
                        let angle = (p.y + forearm.size(&cfg).y).atan2(-p.z);
                        // Lying behind the sights the off elbow is low and ahead, so only the very
                        // widest settings show it, at the bottom edge beside the arm already there.
                        let limit = if stance.prone > 0.5 { 36.0_f32.to_radians() } else { half_fov };
                        assert!(angle < -limit, "{forearm:?} elbow at {} deg, eye {} speed {speed} ads {ads}", angle.to_degrees(), stance.eye_height);
                    }
                }
                let chest = pose[Bone::Chest];
                assert!(chest.end().y > chest.start.y - 0.02, "chest pointing down at eye {} speed {speed}: {} to {}", stance.eye_height, chest.start.y, chest.end().y);
            }
        }
    }
}

/// Angle in degrees between two bones' directions.
fn bend(pose: &Pose, a: Bone, b: Bone) -> f32 {
    (pose[a].rotation * Vec3::Y).angle_between(pose[b].rotation * Vec3::Y).to_degrees()
}

#[test]
fn the_back_stays_straight_in_every_stance_and_gait() {
    let cfg = BodyConfig::default();
    let gaits = [(STAND, 0.0, 2.1), (STAND, 1.7, 2.1), (STAND, 3.9, 2.1), (STAND, 6.2, 2.9), (CROUCH, 0.0, 1.5), (CROUCH, 1.0, 1.5), (CROUCH, 2.0, 1.5), (PRONE, 0.0, 1.5), (PRONE, 0.75, 1.5)];
    for (stance, speed, stride_length) in gaits {
        for pitch in [-1.54, -0.8, 0.0, 0.8, 1.54] {
            for step in 0..10 {
                let mut input = Case { pitch, ..stance }.input();
                input.velocity = Vec3::new(0.0, 0.0, -speed);
                input.stride = step as f32 / 10.0;
                input.stride_length = stride_length;
                input.anim.moving = if speed > 0.0 { 1.0 } else { 0.0 };
                let pose = solve(&cfg, &input, &flat);
                let waist = bend(&pose, Bone::Spine, Bone::Chest);
                assert!(waist < 22.0, "bent {waist} deg at the waist: eye {} speed {speed} pitch {pitch}", stance.eye_height);
            }
        }
    }
}

#[test]
fn looking_down_the_chest_stays_off_the_gun() {
    let cfg = BodyConfig::default();
    for pitch in [-0.6, -1.0, -1.25, -1.54] {
        let input = Case { pitch, ..STAND }.input();
        let pose = solve(&cfg, &input, &flat);
        let g = &input.gun;
        for point in [g.grips.support, g.grips.trigger, g.grips.magazine] {
            let clear = outside(&pose, Bone::Chest, &cfg, g.origin + g.rotation * point);
            assert!(clear > 0.0, "{point} on the gun is inside the chest at pitch {pitch}");
        }
    }
}

#[test]
fn looking_straight_down_the_feet_rise_on_their_toes_and_never_leave_the_floor() {
    let cfg = BodyConfig::default();
    for pitch in [-0.7, -1.0, -1.25, -1.54] {
        let pose = solve(&cfg, &Case { pitch, ..STAND }.input(), &flat);
        for foot in [Bone::FootL, Bone::FootR] {
            let toes = pose[foot].end().y;
            assert!((toes - cfg.bones.ankle_height).abs() < 0.01, "{foot:?} toes {toes} high at pitch {pitch}");
            let lifted = pose[foot].start.y - cfg.bones.ankle_height;
            assert!(lifted < 0.03, "{foot:?} heel up {lifted} at pitch {pitch}");
        }
    }
}

#[test]
fn looking_at_your_feet_the_chest_is_out_of_the_way_and_both_boots_are_in_it() {
    let cfg = BodyConfig::default();
    let half_fov = 31.0_f32.to_radians(); // The default vertical field of view.
    for stance in [STAND, CROUCH] {
        let pitch = -1.45;
        let input = Case { pitch, ..stance }.input();
        let pose = solve(&cfg, &input, &flat);
        let view = glam::Quat::from_euler(EulerRot::YXZ, 0.0, pitch, 0.0) * Vec3::NEG_Z;
        let off_view = |point: Vec3| (point - input.eye).angle_between(view);
        // The chest's centre line, so its near edge can just show at the bottom of the frame.
        for point in [pose[Bone::Chest].start, pose[Bone::Chest].end()] {
            assert!(off_view(point) > half_fov, "chest {} deg off the view, eye {}", off_view(point).to_degrees(), stance.eye_height);
        }
        if stance.crouch == 0.0 {
            for foot in [Bone::FootL, Bone::FootR] {
                assert!(off_view(pose[foot].end()) < half_fov, "{foot:?} toes out of view");
            }
        }
    }
}

fn fallen(yaw: f32) -> BodyInput {
    let cfg = BodyConfig::default();
    let mut anim = BodyAnim::new(yaw);
    let down = AnimInput { yaw, velocity: Vec3::ZERO, grounded: true, crouch: 1.0, prone: 1.0, down: true };
    for _ in 0..200 {
        anim.step(&cfg, &down, 1.0 / 60.0);
    }
    assert_eq!(anim.collapse, 1.0);
    let case = Case { eye_height: 0.38, crouch: 1.0, prone: 1.0, yaw, pitch: 0.0, ads: 0.0 };
    BodyInput { anim, ..case.input() }
}

#[test]
fn a_fallen_soldier_lies_on_the_ground_stretched_out_behind_where_they_stood() {
    let cfg = BodyConfig::default();
    let pose = solve(&cfg, &fallen(0.0), &flat);
    for bone in Bone::ALL {
        let (start, end) = (pose[bone].start, pose[bone].end());
        assert!(start.y.max(end.y) < 0.4, "{bone:?} is standing up at {start} to {end}");
        assert!(start.y.min(end.y) > -0.2, "{bone:?} is through the floor at {start} to {end}");
    }
    assert!(pose[Bone::Head].start.z < pose[Bone::Pelvis].start.z, "the head should be ahead of the hips");
    assert!(pose[Bone::FootL].start.z > pose[Bone::Pelvis].start.z, "the feet should be behind them");
    assert!(pose[Bone::HandR].start.x > pose[Bone::Chest].start.x);
    assert!(pose[Bone::HandL].start.x < pose[Bone::Chest].start.x);
}

#[test]
fn a_fallen_soldier_does_not_follow_the_look() {
    let cfg = BodyConfig::default();
    let mut input = fallen(0.0);
    let before = solve(&cfg, &input, &flat);
    input.yaw = 2.5;
    input.pitch = 0.8;
    input.eye += Vec3::new(0.0, 0.3, 0.0);
    assert_eq!(before, solve(&cfg, &input, &flat));
}

#[test]
fn going_down_takes_time_and_coming_round_undoes_it() {
    let cfg = BodyConfig::default();
    let mut anim = BodyAnim::new(0.0);
    let mut input = AnimInput { yaw: 0.0, velocity: Vec3::ZERO, grounded: true, crouch: 0.0, prone: 0.0, down: true };
    anim.step(&cfg, &input, cfg.collapse.fall_time * 0.5);
    assert!(anim.collapse > 0.3 && anim.collapse < 0.7, "{}", anim.collapse);
    anim.step(&cfg, &input, cfg.collapse.fall_time);
    input.yaw = 1.0;
    anim.step(&cfg, &input, 1.0 / 60.0);
    assert_eq!(anim.fall_yaw, 0.0, "the way they fell stays as it was");
    input.down = false;
    anim.step(&cfg, &input, cfg.collapse.rise_time + 0.1);
    assert_eq!(anim.collapse, 0.0);
}

/// A soldier leaned over as the sim would have them: the eye and gun gone sideways, the gun rolled.
fn leaning(case: Case, lean: f32) -> BodyInput {
    let mut input = case.input();
    let aim = Quat::from_euler(EulerRot::YXZ, case.yaw, case.pitch, -lean * 0.2);
    let hold = HIP.lerp(ADS, case.ads);
    let right = Quat::from_rotation_y(case.yaw) * Vec3::X;
    input.eye += right * lean * 0.38 - Vec3::Y * lean * lean * 0.07;
    input.lean = lean;
    input.gun = GunHold { origin: input.eye + aim * hold, rotation: aim, grips: GRIPS };
    input
}

#[test]
fn a_lean_takes_the_shoulders_over_and_leaves_the_feet_alone() {
    let cfg = BodyConfig::default();
    for case in [STAND, CROUCH] {
        let upright = solve(&cfg, &case.input(), &flat);
        for lean in [-1.0_f32, 1.0] {
            let input = leaning(case, lean);
            let pose = solve(&cfg, &input, &flat);
            let right = Quat::from_rotation_y(case.yaw) * Vec3::X;
            let moved = |bone: Bone| (pose[bone].start - upright[bone].start).dot(right) * lean;
            assert!(moved(Bone::Head) > 0.25, "the head went {}", moved(Bone::Head));
            assert!(moved(Bone::Chest) > 0.1, "the chest went {}", moved(Bone::Chest));
            let (across, level) = (pose[Bone::Chest].rotation * Vec3::X, upright[Bone::Chest].rotation * Vec3::X);
            assert!((across.y - level.y) * -lean > 0.2, "the shoulders should tip over the way it leans: {across}");
            for foot in [Bone::FootL, Bone::FootR] {
                assert!(pose[foot].start.distance(upright[foot].start) < 0.05, "{foot:?} moved");
            }
            assert!(pose[Bone::Pelvis].start.y < upright[Bone::Pelvis].start.y + 0.04, "the hips rose");
            for (hand, grip) in [(Bone::HandR, GRIPS.trigger), (Bone::HandL, GRIPS.support)] {
                let off = pose[hand].end().distance(input.gun.origin + input.gun.rotation * grip);
                assert!(off < 0.12, "{hand:?} came {off} off the gun");
            }
        }
    }
}

/// A climb takes the hands off the gun and puts them on the ledge, bones whole and all.
#[test]
fn hands_go_to_a_ledge_and_back() {
    let cfg = BodyConfig::default();
    let case = Case { eye_height: 1.5, crouch: 0.2, prone: 0.0, yaw: 0.0, pitch: 0.0, ads: 0.0 };
    let still = case.input();
    let ledge = still.feet + Vec3::new(0.0, 1.2, -0.45);
    let hold = LedgeHold { ledge, yaw: 0.0, grip: 1.0, both: true };
    let on = solve(&cfg, &BodyInput { ledge: Some(hold), ..still }, &flat);
    for (hand, side) in [(Bone::HandL, -1.0), (Bone::HandR, 1.0)] {
        let wanted = ledge + Vec3::X * side * cfg.look.ledge_hands_apart;
        assert!(on[hand].end().distance(wanted) < 0.1, "{hand:?} at {} for {wanted}", on[hand].end());
    }
    let one_hand = solve(&cfg, &BodyInput { ledge: Some(LedgeHold { both: false, ..hold }), ..still }, &flat);
    let none = solve(&cfg, &still, &flat);
    assert!(one_hand[Bone::HandL].end().distance(ledge) < 0.35);
    assert!(one_hand[Bone::HandR].end().distance(none[Bone::HandR].end()) < 1e-3, "a hand stays on the gun");
    let off = solve(&cfg, &BodyInput { ledge: Some(LedgeHold { grip: 0.0, ..hold }), ..still }, &flat);
    assert!(off[Bone::HandL].end().distance(none[Bone::HandL].end()) < 1e-3);
}

/// Under someone they are lower and flatter across the back, with the eye and the gun where they were.
#[test]
fn bracing_under_someone_only_moves_the_body() {
    let cfg = BodyConfig::default();
    let case = Case { eye_height: 1.02, crouch: 1.0, prone: 0.0, yaw: 0.0, pitch: 0.0, ads: 0.0 };
    let still = case.input();
    let mut anim = still.anim;
    anim.brace = 1.0;
    let braced = solve(&cfg, &BodyInput { anim, ..still }, &flat);
    let free = solve(&cfg, &still, &flat);
    let across = |pose: &Pose| pose[Bone::Chest].end().y - pose[Bone::Pelvis].start.y;
    assert!(across(&braced) < across(&free), "{} against {}", across(&braced), across(&free));
    assert!(braced[Bone::Head].start.distance(free[Bone::Head].start) < 0.01);
}

/// Someone part-way through a treatment, with the hands all the way on it.
fn treating(case: Case, site: CareSite, progress: f32) -> BodyInput {
    let mut input = case.input();
    input.anim.care = 1.0;
    input.anim.care_site = site;
    input.anim.care_progress = progress;
    input
}

const SITES: [CareSite; 4] = [CareSite::Chest, CareSite::Arm, CareSite::Leg, CareSite::Ally];

#[test]
fn with_no_treatment_the_hands_stay_exactly_on_the_gun() {
    let cfg = BodyConfig::default();
    for site in SITES {
        let mut input = treating(STAND, site, 0.5);
        input.anim.care = 0.0;
        assert_eq!(solve(&cfg, &input, &flat), solve(&cfg, &STAND.input(), &flat), "{site:?}");
    }
}

#[test]
fn a_treatment_only_moves_the_arms() {
    let cfg = BodyConfig::default();
    for stance in [STAND, CROUCH, PRONE] {
        for site in SITES {
            let (still, busy) = (solve(&cfg, &stance.input(), &flat), solve(&cfg, &treating(stance, site, 0.4), &flat));
            for bone in [Bone::Pelvis, Bone::Spine, Bone::Chest, Bone::Neck, Bone::Head, Bone::ThighL, Bone::ThighR, Bone::ShinL, Bone::ShinR, Bone::FootL, Bone::FootR] {
                assert_eq!(still[bone], busy[bone], "{bone:?} at {site:?}");
            }
        }
    }
}

#[test]
fn hands_go_to_the_wound_and_stay_a_whole_arm_all_the_way_through() {
    let cfg = BodyConfig::default();
    for case in cases().into_iter().step_by(5) {
        for site in SITES {
            for first_person in [false, true] {
                for progress in [0.0, 0.2, 0.5, 0.77, 1.0] {
                    let input = BodyInput { first_person, ..treating(case, site, progress) };
                    let pose = solve(&cfg, &input, &flat);
                    for bones in [[Bone::ShoulderL, Bone::UpperArmL, Bone::ForearmL, Bone::HandL], [Bone::ShoulderR, Bone::UpperArmR, Bone::ForearmR, Bone::HandR]] {
                        for pair in bones.windows(2) {
                            let gap = pose[pair[0]].end().distance(pose[pair[1]].start);
                            assert!(gap < 0.004, "{:?} is {gap} from {:?}", pair[1], pair[0]);
                        }
                        for bone in bones {
                            assert!(pose[bone].start.is_finite() && pose[bone].rotation.is_normalized());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn each_wound_gets_the_hands_where_it_is() {
    let cfg = BodyConfig::default();
    let c = &cfg.care;
    let tight = |pose: &Pose, at: Vec3, radius: f32| pose[Bone::HandR].end().distance(at) < radius + c.hand_gap + c.slide + 0.04;

    let pose = solve(&cfg, &treating(STAND, CareSite::Arm, 0.5), &flat);
    let forearm = pose[Bone::ForearmL];
    assert!(tight(&pose, forearm.start.lerp(forearm.end(), c.arm_at), cfg.bones.forearm_size.x), "on the forearm");

    let pose = solve(&cfg, &treating(CROUCH, CareSite::Leg, 0.5), &flat);
    let thigh = pose[Bone::ThighL];
    assert!(tight(&pose, thigh.start.lerp(thigh.end(), c.thigh_at), cfg.bones.thigh_size.x), "on the thigh");

    let pose = solve(&cfg, &treating(STAND, CareSite::Leg, 0.5), &flat);
    let thigh = pose[Bone::ThighL];
    let at = thigh.start.lerp(thigh.end(), c.thigh_at);
    assert!(pose[Bone::HandR].end().distance(at) < 0.2, "a standing soldier can reach their own thigh: {}", pose[Bone::HandR].end().distance(at));
}

#[test]
fn a_wound_you_could_not_see_is_treated_where_you_can() {
    let cfg = BodyConfig::default();
    let half_fov = 31.0_f32.to_radians();
    for stance in [STAND, CROUCH, PRONE] {
        for site in [CareSite::Chest, CareSite::Leg] {
            let input = BodyInput { first_person: true, ..treating(stance, site, 0.5) };
            let pose = solve(&cfg, &input, &flat);
            let forearm = pose[Bone::ForearmL];
            let spot = forearm.start.lerp(forearm.end(), cfg.care.arm_at);
            let off = (spot - input.eye).angle_between(Vec3::NEG_Z);
            assert!(off < half_fov, "{site:?} shows the forearm {} deg off the view at eye {}", off.to_degrees(), stance.eye_height);
        }
    }
}

#[test]
fn treating_never_brings_anything_up_over_the_view_or_into_the_camera() {
    let cfg = BodyConfig::default();
    for stance in [STAND, CROUCH, PRONE] {
        for site in SITES {
            for step in 0..=40 {
                let input = BodyInput { first_person: true, ..treating(stance, site, step as f32 / 40.0) };
                let pose = solve(&cfg, &input, &flat);
                for bone in [Bone::UpperArmL, Bone::ForearmL, Bone::HandL, Bone::UpperArmR, Bone::ForearmR, Bone::HandR] {
                    for point in [pose[bone].start, pose[bone].end()] {
                        let p = point - input.eye;
                        assert!(p.y < -0.03, "{bone:?} {} from the eye on {site:?} at {step}, eye {}", p.y, stance.eye_height);
                    }
                    let clear = outside(&pose, bone, &cfg, input.eye);
                    assert!(clear > NEAR_CLEARANCE, "{bone:?} is {clear} from the eye on {site:?} at {step}, eye {}", stance.eye_height);
                }
            }
        }
    }
}

#[test]
fn the_roll_moves_with_the_treatment_and_is_still_at_each_end() {
    let cfg = BodyConfig::default();
    let c = &cfg.care;
    let at = |progress: f32| solve(&cfg, &treating(STAND, CareSite::Arm, progress), &flat)[Bone::HandR].end();
    assert!(at(0.0).distance(at(c.start_still * 0.9)) < 1e-4, "getting it out");
    assert!(at(1.0).distance(at(1.0 - c.end_still * 0.9)) < 1e-4, "tying it off");
    let span = 1.0 - c.start_still - c.end_still;
    let quarter = c.start_still + span * 0.25 / c.passes;
    assert!(at(c.start_still).distance(at(quarter)) > 0.04, "a pass should go round the limb");
}

#[test]
fn hands_reach_a_treatment_and_come_back_in_the_time_given_whatever_ended_it() {
    let cfg = BodyConfig::default();
    let dt = 1.0 / 60.0;
    let mut anim = BodyAnim::new(0.0);
    let pose = CarePose { site: CareSite::Leg, kit: CareKit::Medkit, progress: 0.25 };
    for _ in 0..(cfg.care.reach_time / dt) as usize + 1 {
        anim.step_care(&cfg, Some(pose), dt);
    }
    assert_eq!(anim.care, 1.0);
    assert_eq!((anim.care_progress, anim.care_site, anim.care_kit), (0.25, CareSite::Leg, CareKit::Medkit));

    anim.step_care(&cfg, None, dt);
    assert!(anim.care < 1.0 && anim.care > 0.9, "let go, the hands ease off: {}", anim.care);
    assert_eq!(anim.care_progress, 0.25, "they keep what they were doing on the way back");
    for _ in 0..(cfg.care.return_time / dt) as usize + 1 {
        anim.step_care(&cfg, None, dt);
    }
    assert_eq!(anim.care, 0.0);
}

#[test]
fn in_a_seat_the_hands_leave_the_wheel_for_a_treatment_and_go_back() {
    let cfg = BodyConfig::default();
    let hands = [Vec3::new(0.8, 1.2, -2.5), Vec3::new(1.2, 1.2, -2.5)];
    let sit = SitPose { hips: Vec3::new(1.0, 0.8, -1.95), feet: Vec3::new(1.0, 0.2, -2.5), yaw: 0.0, hands: Some(hands) };
    let driving = BodyInput { sit: Some(sit), ..STAND.input() };
    let at_wheel = solve(&cfg, &driving, &flat);
    assert_eq!(at_wheel, solve(&cfg, &BodyInput { anim: BodyAnim { care: 0.0, ..driving.anim }, ..driving }, &flat));
    for site in SITES {
        let mut anim = driving.anim;
        anim.care = 1.0;
        anim.care_site = site;
        anim.care_progress = 0.5;
        let busy = solve(&cfg, &BodyInput { anim, ..driving }, &flat);
        assert!(busy[Bone::HandR].end().distance(at_wheel[Bone::HandR].end()) > 0.05, "{site:?}");
        for bone in Bone::ALL {
            assert!(busy[bone].start.is_finite(), "{bone:?} on {site:?}");
        }
    }
}

/// Walking on with a treatment under way, at this speed, this far through a stride.
fn treating_on_the_move(case: Case, site: CareSite, speed: f32, stride: f32, progress: f32) -> BodyInput {
    let mut input = treating(case, site, progress);
    input.velocity = Vec3::new(0.0, 0.0, -speed);
    input.stride = stride;
    input.stride_length = 1.5;
    input.anim.moving = 1.0;
    input
}

const WALKS: [(Case, f32); 3] = [(STAND, 1.7), (CROUCH, 1.7), (PRONE, 0.75)];

#[test]
fn walking_on_the_hands_stay_with_the_dressing_and_the_legs_keep_walking() {
    let cfg = BodyConfig::default();
    for (case, speed) in WALKS {
        for site in [CareSite::Arm, CareSite::Leg, CareSite::Chest] {
            for first_person in [false, true] {
                for step in 0..24 {
                    let input = BodyInput { first_person, ..treating_on_the_move(case, site, speed, step as f32 / 24.0, 0.5) };
                    let pose = solve(&cfg, &input, &flat);
                    let walking_only = BodyInput { anim: BodyAnim { care: 0.0, ..input.anim }, ..input };
                    let legs = solve(&cfg, &walking_only, &flat);
                    for bone in [Bone::ThighL, Bone::ThighR, Bone::ShinL, Bone::ShinR, Bone::FootL, Bone::FootR] {
                        assert_eq!(pose[bone], legs[bone], "{bone:?}");
                    }
                    // The one holding the dressing is on the limb being wrapped, wherever that has got to.
                    if site != CareSite::Chest || first_person {
                        let forearm = pose[Bone::ForearmL];
                        let on = forearm.start.lerp(forearm.end(), cfg.care.arm_at);
                        let gap = pose[Bone::HandR].end().distance(on);
                        assert!(gap < cfg.bones.forearm_size.x + cfg.care.hand_gap + cfg.care.slide + 0.04, "{gap} off the forearm on {site:?} at eye {}", case.eye_height);
                    }
                }
            }
        }
    }
}

#[test]
fn walking_on_a_leg_wound_does_not_drag_the_arms_about_with_the_thigh() {
    let cfg = BodyConfig::default();
    for first_person in [false, true] {
        let hand_at = |step: f32| {
            let input = BodyInput { first_person, ..treating_on_the_move(STAND, CareSite::Leg, 1.7, step / 24.0, 0.0) };
            solve(&cfg, &input, &flat)[Bone::HandL].start
        };
        let (mut lowest, mut highest) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for step in 0..24 {
            lowest = lowest.min(hand_at(step as f32));
            highest = highest.max(hand_at(step as f32));
        }
        assert!((highest - lowest).length() < 0.03, "the hand wanders {} over a stride", (highest - lowest).length());
    }
}

#[test]
fn walking_on_the_arms_stay_out_of_the_body_and_the_view() {
    let cfg = BodyConfig::default();
    for (case, speed) in WALKS {
        for site in [CareSite::Arm, CareSite::Leg] {
            for step in 0..24 {
                let input = BodyInput { first_person: true, ..treating_on_the_move(case, site, speed, step as f32 / 24.0, step as f32 / 24.0) };
                let pose = solve(&cfg, &input, &flat);
                for point in [pose[Bone::HandL].start, pose[Bone::HandL].end(), pose[Bone::HandR].start, pose[Bone::HandR].end()] {
                    assert!(point.y < input.eye.y - 0.03, "a hand is up at the eye");
                    for bone in [Bone::Pelvis, Bone::Spine, Bone::Chest, Bone::ThighL, Bone::ThighR] {
                        assert!(outside(&pose, bone, &cfg, point) > 0.0, "a hand is inside {bone:?} on {site:?} at eye {}, step {step}", case.eye_height);
                    }
                }
            }
        }
    }
}
