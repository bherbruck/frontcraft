//! Where a gun physically is. One pure function from soldier state to world position, so the
//! shooter, the simulation and everyone else watching all agree on it.

use crate::WeaponConfig;
#[cfg(test)]
use crate::WeaponTable;
use glam::{EulerRot, Quat, Vec2, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct GunPoseInput {
    /// Wherever the eye really is, leaning included. The gun is carried along with it.
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// -1 = fully left, 1 = fully right. Rolls the hold about the line through the eye.
    pub lean: f32,
    /// Radians from the look direction: x = yaw, y = pitch.
    pub aim_offset: Vec2,
    /// 0 = hip, 1 = shouldered.
    pub ads: f32,
    /// 0 = lowered, 1 = ready.
    pub ready: f32,
    /// Metres an obstacle has pushed the gun back.
    pub retract: f32,
    /// 0 = in someone's hands, 1 = lying where they fell.
    pub fallen: f32,
    /// 0 = in the hands, 1 = hanging on its sling with the hands on something else.
    pub slung: f32,
    /// How far round onto the back it has gone on its sling, for leaning into something, 0 to 1.
    pub on_back: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GunPose {
    /// The receiver, which is the gun model's origin.
    pub origin: Vec3,
    pub rotation: Quat,
    pub muzzle: Vec3,
    pub forward: Vec3,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a sight's zero goes to yaw and pitch, as radians, on a gun rolled by this lean. The
/// sights sit above the bore in the gun's own frame, so the elevation has to follow it round for bullet
/// and sight line to meet at the zero range.
pub fn zeroed(cfg: &WeaponConfig, lean: f32, elevation: f32) -> Vec2 {
    let (sin, cos) = (lean * cfg.hold.lean_roll).sin_cos();
    Vec2::new(-sin, cos) * elevation
}

/// Where the eye has to be for this gun, looking this way, to have its fore-end on this point. It is
/// `gun_pose` read backwards, for a gun held up and not moved off by anything.
pub fn rested_eye(cfg: &WeaponConfig, pivot: Vec3, along: f32, yaw: f32, pitch: f32, ads: f32) -> Vec3 {
    let h = &cfg.hold;
    let aim = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    pivot - aim * (h.hip.lerp(h.ads, smooth(ads)) + Vec3::new(0.0, -cfg.rest.under, -along))
}

/// The pitch that puts the eye at this height for a gun on this pivot height, which is [`rested_eye`]
/// the other way round. A soldier pivoting a gun aims up by getting lower.
pub fn pitch_for_eye(cfg: &WeaponConfig, pivot_y: f32, along: f32, ads: f32, eye_y: f32) -> f32 {
    let h = &cfg.hold;
    let hold = h.hip.lerp(h.ads, smooth(ads));
    let (up, ahead) = (hold.y - cfg.rest.under, -hold.z + along);
    let reach = (up * up + ahead * ahead).sqrt();
    ((pivot_y - eye_y) / reach).clamp(-1.0, 1.0).asin() - up.atan2(ahead)
}

/// The gun turns about the eye, so at the shoulder the line from the eye through the sights is the
/// gun's axis, and the front sight on screen marks where it points. A lean rolls it about that same
/// line, so the sights stay on it however far over it goes.
pub fn gun_pose(cfg: &WeaponConfig, input: &GunPoseInput) -> GunPose {
    let h = &cfg.hold;
    let aim = Quat::from_euler(
        EulerRot::YXZ,
        input.yaw + input.aim_offset.x,
        input.pitch + input.aim_offset.y,
        -input.lean * h.lean_roll,
    );
    let lowered = 1.0 - smooth(input.ready);
    let fallen = smooth(input.fallen);
    let slung = smooth(input.slung);
    let hold = h.hip.lerp(h.ads, smooth(input.ads))
        + h.lowered_offset * lowered
        + h.slung_offset * slung
        + h.fallen_offset * fallen
        + Vec3::Z * input.retract;
    let tilt = (h.lowered_rotation * lowered).lerp(h.slung_rotation, slung).lerp(h.fallen_rotation, fallen);
    let rotation = aim * Quat::from_euler(EulerRot::YXZ, tilt.y, tilt.x, tilt.z);
    let origin = input.eye + aim * hold;
    // On the back it goes with the body and not the look, so looking about does not swing it through the shoulders.
    let back = smooth(input.on_back);
    let level = Quat::from_rotation_y(input.yaw);
    let (b, r) = (h.back_offset, h.back_rotation);
    let origin = origin.lerp(input.eye + level * b, back);
    let rotation = rotation.slerp(level * Quat::from_euler(EulerRot::YXZ, r.y, r.x, r.z), back);
    let forward = rotation * Vec3::NEG_Z;
    GunPose { origin, rotation, muzzle: origin + forward * h.muzzle_forward, forward }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> GunPoseInput {
        GunPoseInput {
            eye: Vec3::new(3.0, 1.6, -2.0),
            yaw: 0.7,
            pitch: -0.2,
            lean: 0.0,
            aim_offset: Vec2::ZERO,
            ads: 1.0,
            ready: 1.0,
            retract: 0.0,
            fallen: 0.0,
            slung: 0.0,
            on_back: 0.0,
        }
    }

    /// Distance from a point to the line through the eye along the gun's axis.
    fn off_line(eye: Vec3, forward: Vec3, point: Vec3) -> f32 {
        let d = point - eye;
        (d - forward * d.dot(forward)).length()
    }

    #[test]
    fn a_gun_on_its_sling_hangs_low_and_points_at_the_ground() {
        let cfg = WeaponConfig::assault_rifle();
        let held = gun_pose(&cfg, &GunPoseInput { ads: 0.0, ready: 0.0, ..input() });
        let slung = gun_pose(&cfg, &GunPoseInput { ads: 0.0, ready: 0.0, slung: 1.0, ..input() });
        assert!(slung.origin.y < held.origin.y - 0.2, "{} against {}", slung.origin, held.origin);
        assert!(slung.forward.y < -0.5, "{}", slung.forward);
    }

    #[test]
    fn the_pitch_for_a_height_is_the_height_for_a_pitch_turned_round() {
        let cfg = WeaponConfig::assault_rifle();
        for pitch in [-0.5, -0.1, 0.0, 0.2, 0.45] {
            for ads in [0.0, 1.0] {
                let eye = rested_eye(&cfg, Vec3::new(1.0, 1.3, 2.0), 0.3, 0.4, pitch, ads);
                let back = pitch_for_eye(&cfg, 1.3, 0.3, ads, eye.y);
                assert!((back - pitch).abs() < 1e-4, "{back} for {pitch}");
            }
        }
    }

    #[test]
    fn shouldered_sights_line_up_with_the_eye_wherever_the_gun_points() {
        let cfg = WeaponConfig::assault_rifle();
        for aim_offset in [Vec2::ZERO, Vec2::new(0.03, 0.05), Vec2::new(-0.08, 0.01)] {
            let i = GunPoseInput { aim_offset, ..input() };
            let pose = gun_pose(&cfg, &i);
            let sight = pose.origin + pose.rotation * Vec3::new(0.0, cfg.hold.sight_height, -cfg.hold.front_sight);
            assert!(off_line(i.eye, pose.forward, sight) < 0.001);
        }
    }

    #[test]
    fn every_gun_lines_up_with_the_eye_at_the_shoulder_and_fits_its_own_sights() {
        for cfg in WeaponTable::default().weapons {
            let pose = gun_pose(&cfg, &input());
            for along in [cfg.hold.front_sight, cfg.hold.rear_sight] {
                let sight = pose.origin + pose.rotation * Vec3::new(0.0, cfg.hold.sight_height, -along);
                assert!(off_line(input().eye, pose.forward, sight) < 0.001, "{}", cfg.name);
            }
            assert!(cfg.hold.front_sight < cfg.hold.muzzle_forward, "{}: front sight off the barrel", cfg.name);
            assert!(cfg.hold.max_retract < cfg.hold.blocked_retract, "{}", cfg.name);
        }
    }

    #[test]
    fn the_muzzle_is_a_gun_length_ahead_of_the_receiver() {
        let cfg = WeaponConfig::assault_rifle();
        let pose = gun_pose(&cfg, &input());
        assert!((pose.muzzle.distance(pose.origin) - cfg.hold.muzzle_forward).abs() < 0.001);
        assert!(pose.muzzle.distance(input().eye) > 0.6, "muzzle should be well clear of the eye");
    }

    #[test]
    fn hip_carries_the_gun_off_to_the_side_and_low() {
        let cfg = WeaponConfig::assault_rifle();
        let i = GunPoseInput { yaw: 0.0, pitch: 0.0, ads: 0.0, ..input() };
        let pose = gun_pose(&cfg, &i);
        assert!(pose.origin.x > i.eye.x + 0.1);
        assert!(pose.origin.y < i.eye.y - 0.1);
    }

    #[test]
    fn a_lean_rolls_the_gun_about_the_eye_and_leaves_it_pointing_the_same_way() {
        for cfg in WeaponTable::default().weapons {
            let straight = gun_pose(&cfg, &input());
            for lean in [-1.0, -0.4, 1.0] {
                let i = GunPoseInput { lean, ..input() };
                let pose = gun_pose(&cfg, &i);
                assert!(pose.forward.distance(straight.forward) < 1e-4, "{}", cfg.name);
                let sight = pose.origin + pose.rotation * Vec3::new(0.0, cfg.hold.sight_height, -cfg.hold.front_sight);
                assert!(off_line(i.eye, pose.forward, sight) < 0.001, "{}: sights left the eye", cfg.name);
            }
        }
    }

    #[test]
    fn leaning_right_tips_the_top_of_the_gun_to_the_right() {
        let cfg = WeaponConfig::assault_rifle();
        let i = GunPoseInput { yaw: 0.0, pitch: 0.0, lean: 1.0, ..input() };
        let up = gun_pose(&cfg, &i).rotation * Vec3::Y;
        assert!(up.x > 0.05, "{up}");
        let left = gun_pose(&cfg, &GunPoseInput { lean: -1.0, ..i }).rotation * Vec3::Y;
        assert!((left.x + up.x).abs() < 1e-4);
    }

    #[test]
    fn a_zero_follows_the_roll_of_the_gun_and_keeps_its_size() {
        let cfg = WeaponConfig::assault_rifle();
        assert_eq!(zeroed(&cfg, 0.0, 0.002).y, 0.002);
        let leaned = zeroed(&cfg, 1.0, 0.002);
        // Leaning right the top of the gun goes right, and yaw is positive to the left.
        assert!(leaned.x < 0.0);
        assert!((leaned.length() - 0.002).abs() < 1e-6);
    }

    #[test]
    fn retracting_pulls_the_muzzle_back_along_the_gun() {
        let cfg = WeaponConfig::assault_rifle();
        let free = gun_pose(&cfg, &input());
        let pushed = gun_pose(&cfg, &GunPoseInput { retract: 0.2, ..input() });
        assert!((free.muzzle.distance(pushed.muzzle) - 0.2).abs() < 0.001);
    }

    #[test]
    fn lowering_points_the_gun_away_from_the_look_direction() {
        let cfg = WeaponConfig::assault_rifle();
        let ready = gun_pose(&cfg, &input());
        let down = gun_pose(&cfg, &GunPoseInput { ready: 0.0, ..input() });
        assert!(ready.forward.dot(down.forward) < 0.95);
    }
}
