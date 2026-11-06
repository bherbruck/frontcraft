//! Two-bone inverse kinematics and building rotations from directions. Both are written to give a
//! sane answer for any input, because a soldier's pose is drawn every frame and cannot fail.

use glam::{Mat3, Quat, Vec3};

const EPS: f32 = 1e-4;

/// Any direction square to `dir`, preferring `hint`.
fn perpendicular(dir: Vec3, hint: Vec3) -> Vec3 {
    let along = hint - dir * hint.dot(dir);
    if along.length_squared() > EPS {
        return along.normalize();
    }
    dir.any_orthonormal_vector()
}

/// Where the middle joint and the end go for a limb rooted at `root` reaching for `target`, bending
/// towards `pole`. Bone lengths are exact: an unreachable target gets the nearest pose that fits.
pub fn two_bone(root: Vec3, target: Vec3, upper: f32, lower: f32, pole: Vec3) -> (Vec3, Vec3) {
    let reach = upper + lower;
    let shortest = (upper - lower).abs() + 0.001;
    let to = target - root;
    let length = to.length();
    let dir = if length > EPS { to / length } else { pole.normalize_or(Vec3::NEG_Y) };
    let d = length.clamp(shortest, (reach - 0.001).max(shortest));
    let along = (upper * upper - lower * lower + d * d) / (2.0 * d);
    let out = (upper * upper - along * along).max(0.0).sqrt();
    let mid = root + dir * along + perpendicular(dir, pole) * out;
    let end = root + dir * d;
    // Numerics can leave the second bone a hair off length, and the lengths are a promise.
    let lower_dir = (end - mid).normalize_or(dir);
    (mid, mid + lower_dir * lower)
}

/// Rotation whose +Y is along `y` and whose +X is as close to `x_hint` as it can be. With +Y up the
/// bone and +X to the right, -Z is the front of the segment.
pub fn frame(y: Vec3, x_hint: Vec3) -> Quat {
    let y = y.normalize_or(Vec3::Y);
    let x = perpendicular(y, x_hint);
    let z = x.cross(y);
    Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reachable_target_is_reached_with_exact_lengths() {
        let root = Vec3::new(0.2, 1.0, -0.3);
        let target = root + Vec3::new(0.3, -0.2, 0.1);
        let (mid, end) = two_bone(root, target, 0.3, 0.27, Vec3::NEG_Y);
        assert!(end.distance(target) < 1e-3);
        assert!((mid.distance(root) - 0.3).abs() < 1e-3);
        assert!((mid.distance(end) - 0.27).abs() < 1e-3);
    }

    #[test]
    fn a_far_target_straightens_the_limb_without_stretching_it() {
        let (mid, end) = two_bone(Vec3::ZERO, Vec3::X * 5.0, 0.3, 0.27, Vec3::NEG_Y);
        assert!((mid.length() - 0.3).abs() < 1e-3);
        assert!((mid.distance(end) - 0.27).abs() < 1e-3);
        assert!(end.x > 0.56);
    }

    #[test]
    fn the_joint_bends_towards_the_pole() {
        let (mid, _) = two_bone(Vec3::ZERO, Vec3::X * 0.4, 0.3, 0.3, Vec3::Y);
        assert!(mid.y > 0.1);
    }

    #[test]
    fn degenerate_input_gives_finite_answers() {
        for (target, pole) in [(Vec3::ZERO, Vec3::Y), (Vec3::X, Vec3::X), (Vec3::ZERO, Vec3::ZERO)] {
            let (mid, end) = two_bone(Vec3::ZERO, target, 0.3, 0.3, pole);
            assert!(mid.is_finite() && end.is_finite());
        }
    }

    #[test]
    fn frames_are_orthonormal_even_when_the_hint_is_parallel() {
        for (y, hint) in [(Vec3::Y, Vec3::X), (Vec3::X, Vec3::X), (Vec3::ZERO, Vec3::ZERO)] {
            let q = frame(y, hint);
            assert!(q.is_normalized());
        }
        let q = frame(Vec3::Y, Vec3::X);
        assert!((q * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-4, "-Z is the front");
    }
}
