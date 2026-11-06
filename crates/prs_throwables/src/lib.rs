//! Things that get thrown: what they weigh in the air, when they go off and what is left. Nothing here
//! knows about soldiers or Bevy, so a server runs it as it is and a client flies its own copy of an arc
//! from where it started, the way bullets are done. Who gets hurt is the sim's, since it has the bones.

use glam::Vec3;
use prs_collision::CollisionWorld;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThrownKind {
    #[default]
    Frag,
    Smoke,
}

/// All lengths are metres, times are seconds and angles are radians.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThrowConfig {
    /// How long winding up takes to reach full strength, and how much of it a quick flick gets.
    pub windup: f32,
    pub least_strength: f32,
    pub overarm_speed: f32,
    pub underarm_speed: f32,
    /// How far above the look the hand sends it.
    pub overarm_lift: f32,
    pub underarm_lift: f32,
    /// Where the hand lets go, from the eye in the look's frame: x = right, y = up, -z = forward.
    pub overarm_hand: Vec3,
    pub underarm_hand: Vec3,
    /// Share of the soldier's own speed it keeps.
    pub inherit: f32,
    pub gravity: f32,
    pub radius: f32,
    /// Share of speed kept off a surface, square on and along it.
    pub bounce: f32,
    pub slide: f32,
    /// Slower than this off something flat and it stays there.
    pub rest_speed: f32,
    /// How long a copy on a client waits to hear it went off before giving up on it.
    pub client_patience: f32,
    pub frag: FragConfig,
    pub smoke: SmokeConfig,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FragConfig {
    /// From the pin coming out, so holding on to it shortens what is left.
    pub fuse: f32,
    pub fragments: u32,
    /// How far a fragment does anything, what it does right up close, and how that falls with distance.
    pub fragment_reach: f32,
    pub fragment_damage: f32,
    pub fragment_falloff: f32,
    /// Share of what it does to a person that it does to a vehicle.
    pub vehicle_share: f32,
    /// The blast itself, to anything with a clear line from it.
    pub blast_radius: f32,
    pub blast_damage: f32,
    /// How far it rattles anyone with a clear line, and how much right beside it.
    pub suppress_reach: f32,
    pub suppress: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SmokeConfig {
    /// From leaving the hand. It carries on falling and pouring out smoke from wherever that finds it.
    pub fuse: f32,
    /// The longest it can go on falling after that, in case it never comes to rest.
    pub fall_time: f32,
    /// How long it takes to fill out, how long it hangs there and how long it takes to thin away.
    pub grow_time: f32,
    pub hold_time: f32,
    pub fade_time: f32,
    pub radius: f32,
    /// How far above the ground its middle sits, as a share of the radius.
    pub lift: f32,
    /// This much of a line inside it and there is no seeing along that line.
    pub opaque_after: f32,
}

impl Default for ThrowConfig {
    fn default() -> Self {
        Self {
            windup: 0.6,
            least_strength: 0.45,
            overarm_speed: 20.0,
            underarm_speed: 7.0,
            overarm_lift: 0.14,
            underarm_lift: 0.3,
            overarm_hand: Vec3::new(0.22, 0.12, -0.2),
            underarm_hand: Vec3::new(0.25, -0.6, -0.3),
            inherit: 0.6,
            gravity: 9.81,
            radius: 0.05,
            bounce: 0.32,
            slide: 0.6,
            rest_speed: 0.9,
            client_patience: 2.0,
            frag: FragConfig {
                fuse: 4.5,
                fragments: 260,
                fragment_reach: 20.0,
                fragment_damage: 45.0,
                fragment_falloff: 1.2,
                vehicle_share: 0.25,
                blast_radius: 5.0,
                blast_damage: 240.0,
                suppress_reach: 22.0,
                suppress: 0.9,
            },
            smoke: SmokeConfig {
                fuse: 1.6,
                fall_time: 6.0,
                grow_time: 6.0,
                hold_time: 28.0,
                fade_time: 8.0,
                radius: 7.0,
                lift: 0.35,
                opaque_after: 2.0,
            },
        }
    }
}

/// One in the air or on the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thrown {
    pub id: u32,
    pub kind: ThrownKind,
    pub thrower: usize,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Seconds until it goes off. On a client it can go below zero, waiting to hear.
    pub fuse: f32,
    pub at_rest: bool,
}

/// Where it hit something, and how hard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounce {
    pub point: Vec3,
    pub speed: f32,
}

impl Thrown {
    /// One tick in the air: under gravity, and off whatever it meets with one ray.
    pub fn fly(&mut self, cfg: &ThrowConfig, world: &CollisionWorld, dt: f32) -> Option<Bounce> {
        self.fuse -= dt;
        if self.at_rest {
            return None;
        }
        self.vel.y -= cfg.gravity * dt;
        let step = self.vel * dt;
        let length = step.length();
        if length < 1e-5 {
            return None;
        }
        let dir = step / length;
        let Some(hit) = world.raycast(self.pos, dir, length + cfg.radius) else {
            self.pos += step;
            return None;
        };
        self.pos += dir * (hit.distance - cfg.radius).max(0.0);
        let into = self.vel.dot(hit.normal);
        let speed = -into;
        self.vel = (self.vel - hit.normal * into) * cfg.slide - hit.normal * into * cfg.bounce;
        if self.vel.length() < cfg.rest_speed && hit.normal.y > 0.5 {
            self.vel = Vec3::ZERO;
            self.at_rest = true;
        }
        Some(Bounce { point: self.pos, speed })
    }
}

/// A smoke grenade's cloud, one rough ball. Only when it started changes, so it costs little to tell a client.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cloud {
    pub id: u32,
    pub pos: Vec3,
    pub age: f32,
}

impl Cloud {
    pub fn new(id: u32, ground: Vec3, cfg: &SmokeConfig) -> Self {
        Self { id, pos: ground + Vec3::Y * cfg.radius * cfg.lift, age: 0.0 }
    }

    /// The grenade it comes from is still falling, and the cloud goes with it, but doesn't bounce when it does.
    pub fn over(&mut self, ground: Vec3, cfg: &SmokeConfig) {
        let at = ground + Vec3::Y * cfg.radius * cfg.lift;
        self.pos = at.with_y(at.y.min(self.pos.y));
    }

    pub fn radius(&self, cfg: &SmokeConfig) -> f32 {
        let grown = (self.age / cfg.grow_time.max(1e-3)).min(1.0);
        // Quick to start and slower to finish, as gas does.
        let rise = 1.0 - (1.0 - grown) * (1.0 - grown);
        let gone = ((self.age - cfg.grow_time - cfg.hold_time) / cfg.fade_time.max(1e-3)).clamp(0.0, 1.0);
        cfg.radius * rise * (1.0 - gone)
    }

    /// How hard the grenade under it is still pouring, from 1 down to nothing as the cloud thins away.
    pub fn pouring(&self, cfg: &SmokeConfig) -> f32 {
        1.0 - ((self.age - cfg.grow_time - cfg.hold_time) / cfg.fade_time.max(1e-3)).clamp(0.0, 1.0)
    }

    /// Where the grenade came to rest, which is what its middle was lifted from.
    pub fn grenade_at(&self, cfg: &SmokeConfig) -> Vec3 {
        self.pos - Vec3::Y * cfg.radius * cfg.lift
    }

    pub fn finished(&self, cfg: &SmokeConfig) -> bool {
        self.age >= cfg.grow_time + cfg.hold_time + cfg.fade_time
    }

    /// Length of the stretch of this line inside the cloud.
    pub fn line_inside(&self, a: Vec3, b: Vec3, cfg: &SmokeConfig) -> f32 {
        let radius = self.radius(cfg);
        let span = b - a;
        let length = span.length();
        if length < 1e-4 || radius <= 0.0 {
            return 0.0;
        }
        let dir = span / length;
        let along = (self.pos - a).dot(dir);
        let off = (self.pos - a - dir * along).length_squared();
        let half = radius * radius - off;
        if half <= 0.0 {
            return 0.0;
        }
        let half = half.sqrt();
        ((along + half).min(length) - (along - half).max(0.0)).max(0.0)
    }
}

/// Whether smoke stops anyone seeing along this line. All sight in the game goes through one question,
/// so a person and a bot are stopped by the same thing.
pub fn smoke_blocks(clouds: &[Cloud], cfg: &SmokeConfig, a: Vec3, b: Vec3) -> bool {
    clouds.iter().map(|cloud| cloud.line_inside(a, b, cfg)).sum::<f32>() >= cfg.opaque_after
}

/// Which way one fragment goes. They spread evenly over the sphere, and the seed turns the whole pattern
/// and shakes each one, so that no two grenades leave the same safe gaps.
pub fn fragment_dir(seed: u32, index: u32, count: u32) -> Vec3 {
    const GOLDEN: f32 = 2.399_963;
    let mix = |v: u32| {
        let mut x = v.wrapping_mul(2654435761).wrapping_add(0x9E37_79B1);
        x ^= x >> 15;
        x = x.wrapping_mul(2246822519);
        x ^= x >> 13;
        (x >> 8) as f32 / (1u32 << 24) as f32
    };
    let shake = 0.5 / (count.max(1) as f32).sqrt();
    let y = (1.0 - (index as f32 + 0.5) * 2.0 / count.max(1) as f32 + (mix(seed ^ index) - 0.5) * shake).clamp(-1.0, 1.0);
    let flat = (1.0 - y * y).max(0.0).sqrt();
    let angle = index as f32 * GOLDEN + mix(seed) * std::f32::consts::TAU + (mix(seed ^ (index << 8)) - 0.5) * shake;
    Vec3::new(flat * angle.cos(), y, flat * angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use prs_collision::Surface;

    fn floor() -> CollisionWorld {
        let mut world = CollisionWorld::default();
        world.add_box(Vec3::new(0.0, -1.0, 0.0), Vec3::new(9.0, 1.0, 9.0), Quat::IDENTITY, Surface::Concrete);
        world
    }

    fn thrown(pos: Vec3, vel: Vec3) -> Thrown {
        Thrown { id: 1, kind: ThrownKind::Frag, thrower: 0, pos, vel, fuse: 5.0, at_rest: false }
    }

    #[test]
    fn it_bounces_lower_each_time_and_comes_to_rest_on_the_floor() {
        let (cfg, world) = (ThrowConfig::default(), floor());
        let mut t = thrown(Vec3::new(0.0, 2.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        let mut bounces = Vec::new();
        for _ in 0..600 {
            bounces.extend(t.fly(&cfg, &world, 1.0 / 60.0));
        }
        assert!(t.at_rest, "still going at {} with {}", t.pos, t.vel);
        assert!(t.pos.y >= 0.0 && t.pos.y < cfg.radius * 2.0, "{}", t.pos);
        assert!(bounces.len() >= 2 && bounces[1].speed < bounces[0].speed, "{bounces:?}");
    }

    #[test]
    fn it_does_not_go_through_a_wall() {
        let cfg = ThrowConfig::default();
        let mut world = floor();
        world.add_box(Vec3::new(3.0, 1.0, 0.0), Vec3::new(0.1, 3.0, 9.0), Quat::IDENTITY, Surface::Concrete);
        let mut t = thrown(Vec3::new(0.0, 1.0, 0.0), Vec3::new(30.0, 0.0, 0.0));
        for _ in 0..300 {
            t.fly(&cfg, &world, 1.0 / 60.0);
            assert!(t.pos.x < 3.0, "through the wall to {}", t.pos);
        }
    }

    #[test]
    fn a_cloud_grows_hangs_and_is_gone_and_only_a_thick_enough_line_is_blocked() {
        let cfg = ThrowConfig::default().smoke;
        let mut cloud = Cloud::new(1, Vec3::ZERO, &cfg);
        assert_eq!(cloud.radius(&cfg), 0.0);
        cloud.age = cfg.grow_time;
        let full = cloud.radius(&cfg);
        assert_eq!(full, cfg.radius);
        let across = |at: f32| (Vec3::new(-30.0, cloud.pos.y + at, 0.0), Vec3::new(30.0, cloud.pos.y + at, 0.0));
        let (a, b) = across(0.0);
        assert!(smoke_blocks(&[cloud], &cfg, a, b));
        let (a, b) = across(full + 0.5);
        assert!(!smoke_blocks(&[cloud], &cfg, a, b), "passes above it");
        // From inside it looking out there is still a good length of it in the way.
        assert!(smoke_blocks(&[cloud], &cfg, cloud.pos, Vec3::new(50.0, cloud.pos.y, 0.0)));
        cloud.age = cfg.grow_time + cfg.hold_time + cfg.fade_time;
        assert!(cloud.finished(&cfg));
        assert_eq!(cloud.radius(&cfg), 0.0);
    }

    #[test]
    fn fragments_spread_all_round_and_differ_by_seed() {
        let n = 72;
        let dirs: Vec<Vec3> = (0..n).map(|i| fragment_dir(5, i, n)).collect();
        assert!(dirs.iter().all(|d| (d.length() - 1.0).abs() < 1e-3));
        let mean = dirs.iter().sum::<Vec3>() / n as f32;
        assert!(mean.length() < 0.15, "lopsided: {mean}");
        assert_ne!(dirs[3], fragment_dir(6, 3, n));
        assert_eq!(dirs[3], fragment_dir(5, 3, n));
    }
}
