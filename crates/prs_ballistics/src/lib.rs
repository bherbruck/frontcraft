//! Bullet flight. Knows nothing about what it might hit; callers test the returned segments.

use glam::Vec3;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct BulletConfig {
    pub muzzle_velocity: f32,
    /// Fraction of speed lost per metre travelled.
    pub drag: f32,
    pub max_age: f32,
    /// How fast it falls, in metres per second per second.
    pub gravity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bullet {
    pub pos: Vec3,
    pub vel: Vec3,
    pub age: f32,
    pub drag: f32,
    pub max_age: f32,
    pub gravity: f32,
}

impl Bullet {
    pub fn new(origin: Vec3, dir: Vec3, cfg: &BulletConfig) -> Self {
        Self {
            pos: origin,
            vel: dir * cfg.muzzle_velocity,
            age: 0.0,
            drag: cfg.drag,
            max_age: cfg.max_age,
            gravity: cfg.gravity,
        }
    }

    /// Moves the bullet and returns the segment it covered.
    pub fn advance(&mut self, dt: f32) -> (Vec3, Vec3) {
        let from = self.pos;
        let speed = self.vel.length();
        self.vel += (-self.vel * self.drag * speed + Vec3::NEG_Y * self.gravity) * dt;
        self.pos += self.vel * dt;
        self.age += dt;
        (from, self.pos)
    }

    pub fn expired(&self) -> bool {
        self.age >= self.max_age
    }
}

/// Elevation in radians that puts a bullet back on the line of sight at `range` metres. The sights
/// sit `sight_height` metres above the bore, so the bullet has to climb that far as well.
pub fn zero_elevation(cfg: &BulletConfig, range: f32, sight_height: f32) -> f32 {
    let mut elevation: f32 = 0.0;
    for _ in 0..4 {
        let mut b = Bullet::new(
            Vec3::ZERO,
            Vec3::new(0.0, elevation.sin(), -elevation.cos()),
            cfg,
        );
        while -b.pos.z < range && !b.expired() {
            b.advance(1.0 / 240.0);
        }
        elevation -= ((b.pos.y - sight_height) / range).atan();
    }
    elevation
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFLE: BulletConfig = BulletConfig {
        muzzle_velocity: 900.0,
        drag: 0.0009,
        max_age: 4.0,
        gravity: 9.81,
    };

    fn height_at(range: f32, elevation: f32) -> f32 {
        let dir = Vec3::new(0.0, elevation.sin(), -elevation.cos());
        let mut b = Bullet::new(Vec3::ZERO, dir, &RIFLE);
        while -b.pos.z < range {
            b.advance(1.0 / 60.0);
        }
        b.pos.y
    }

    #[test]
    fn zeroing_climbs_to_meet_sights_above_the_bore() {
        let e = zero_elevation(&RIFLE, 100.0, 0.06);
        assert!((height_at(100.0, e) - 0.06).abs() < 0.08, "{}", height_at(100.0, e));
        assert!(e > zero_elevation(&RIFLE, 100.0, 0.0));
    }

    #[test]
    fn slows_down_and_drops() {
        let mut b = Bullet::new(Vec3::ZERO, Vec3::NEG_Z, &RIFLE);
        for _ in 0..30 {
            b.advance(1.0 / 60.0);
        }
        assert!(b.vel.length() < 800.0, "{}", b.vel.length());
        assert!(b.pos.y < -0.5, "{}", b.pos.y);
    }

    #[test]
    fn zeroed_rifle_shoots_flat_at_zero_range_and_drops_beyond() {
        let e = zero_elevation(&RIFLE, 100.0, 0.0);
        assert!(height_at(100.0, e).abs() < 0.08, "{}", height_at(100.0, e));
        assert!(height_at(300.0, e) < -0.4, "{}", height_at(300.0, e));
    }
}
