//! What a map is made of, as plain data. Collision and rendering both build from this one
//! description, so what you see is what you collide with.

mod archetypes;
mod build;
mod relief;
pub mod spec;
mod town;

use glam::{Quat, Vec2, Vec3};
use prs_collision::{CollisionWorld, Surface};
pub use spec::{Ground, MapSpec};

/// From the spawn on the range, beside the firing line.
const RANGE_SENTRY: Vec3 = Vec3::new(5.0, 0.0, -3.0);
/// Behind the firing line and off to the left.
const RANGE_HELIPAD: (f32, f32) = (-42.0, 14.0);

/// A trunk is a square post this wide, and this much of the tree's height on each side of its middle.
pub const TRUNK_HALF_WIDTH: f32 = 0.22;
pub const TRUNK_HALF_HEIGHT: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Concrete,
    Wood,
    Sandbag,
    Rock,
    Metal,
    Trunk,
    Limewash,
    Mudbrick,
    Ochre,
    Cement,
    Brick,
    Stone,
    Plaster,
    FloorTile,
    Packed,
    RoofTile,
    Tin,
    Rust,
    Canvas,
    Hay,
    Hedge,
    Planks,
}

impl Paint {
    fn surface(self) -> Surface {
        match self {
            Paint::Concrete
            | Paint::Rock
            | Paint::Limewash
            | Paint::Mudbrick
            | Paint::Ochre
            | Paint::Cement
            | Paint::Brick
            | Paint::Stone
            | Paint::Plaster
            | Paint::FloorTile
            | Paint::RoofTile => Surface::Concrete,
            Paint::Wood | Paint::Trunk | Paint::Planks => Surface::Wood,
            Paint::Hay | Paint::Hedge => Surface::Grass,
            Paint::Packed | Paint::Canvas | Paint::Sandbag => Surface::Dirt,
            Paint::Metal | Paint::Tin | Paint::Rust => Surface::Metal,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Solid {
    pub center: Vec3,
    pub half: Vec3,
    pub rotation: Quat,
    pub paint: Paint,
}

/// Canopy with no collision. Bullets and players pass through it.
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    pub base: Vec3,
    pub height: f32,
    pub radius: f32,
    pub kind: TreeKind,
}

impl Tree {
    /// The leaves as an upright ellipsoid, (middle, half extents): near enough to what is drawn for working
    /// out what they hide. A fir's lowest branches start clear of a standing soldier's eyes.
    pub fn canopy(&self, fir_clearance: f32) -> (Vec3, Vec3) {
        let (h, r) = (self.height, self.radius);
        let (at, half) = match self.kind {
            TreeKind::Fir => (h * 0.55 + (fir_clearance - h * 0.21).max(0.0), Vec3::new(r * 0.85, h * 0.45, r * 0.85)),
            TreeKind::Broadleaf => (h * 0.68, Vec3::new(r, h * 0.33, r)),
            TreeKind::Poplar => (h * 0.62, Vec3::new(r, h * 0.42, r)),
            TreeKind::Fruit => (h * 0.66, Vec3::new(r, h * 0.36, r)),
        };
        (self.base + Vec3::Y * at, half)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TreeKind {
    #[default]
    Fir,
    Broadleaf,
    Poplar,
    Fruit,
}

#[derive(Clone, Debug)]
pub struct Terrain {
    pub size: f32,
    pub cells: u32,
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
    /// What each vertex's ground is, for its colour.
    pub ground: Vec<Ground>,
}

/// Where one team starts from.
#[derive(Clone, Debug)]
pub struct Team {
    pub name: String,
    pub spawns: Vec<SpawnPoint>,
    /// How far from the middle of its spawns its main base reaches, in metres, which a mode can keep
    /// the other side out of.
    pub base_radius: f32,
}

impl Team {
    /// The middle of its spawns.
    pub fn base_center(&self) -> Vec3 {
        self.spawns.iter().map(|s| s.pos).sum::<Vec3>() / self.spawns.len().max(1) as f32
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpawnPoint {
    pub pos: Vec3,
    pub yaw: f32,
}

/// Where a vehicle starts, on the ground and facing `yaw`. `kind` is a row in the game's table of vehicles.
#[derive(Clone, Copy, Debug)]
pub struct VehicleSpawn {
    pub kind: u8,
    pub pos: Vec3,
    pub yaw: f32,
}

/// A place worth holding. What it means to hold it is up to the game mode.
#[derive(Clone, Debug)]
pub struct CapturePoint {
    pub name: String,
    pub center: Vec3,
    pub radius: f32,
}

/// The inside of one storey of a building.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub center: Vec3,
    pub half: Vec3,
    pub rotation: Quat,
}

impl Room {
    pub fn contains(&self, p: Vec3) -> bool {
        let local = self.rotation.conjugate() * (p - self.center);
        local.abs().cmple(self.half).all()
    }
}

/// An opening to walk through, at floor level in the middle of the wall.
#[derive(Clone, Copy, Debug)]
pub struct Doorway {
    pub at: Vec3,
    pub outward: Vec3,
    pub width: f32,
}

/// An opening to shoot through, at its centre in the middle of the wall.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub at: Vec3,
    pub outward: Vec3,
    pub width: f32,
    pub height: f32,
}

/// A way up: where to stand at the bottom, then the points to walk through to get to the top.
#[derive(Clone, Debug)]
pub struct Staircase {
    pub path: Vec<Vec3>,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub terrain: Terrain,
    pub solids: Vec<Solid>,
    pub trees: Vec<Tree>,
    pub targets: Vec<Vec3>,
    pub spawn: Vec3,
    pub spawn_yaw: f32,
    pub teams: Vec<Team>,
    pub capture_points: Vec<CapturePoint>,
    pub rooms: Vec<Room>,
    pub doorways: Vec<Doorway>,
    pub windows: Vec<Window>,
    pub staircases: Vec<Staircase>,
    /// Somewhere a soldier can stand and fire with a clear line, for a stand-in with nothing else to do.
    pub sentry: SpawnPoint,
    /// Clear flat places for a helicopter, on the ground and facing `yaw`.
    pub helipads: Vec<SpawnPoint>,
    pub vehicles: Vec<VehicleSpawn>,
}

impl Level {
    /// The map with this name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "range" => Some(Self::training_range()),
            "town" => Some(MapSpec::town().build()),
            _ => None,
        }
    }

    /// Flat open ground and nothing else, `size` metres across.
    pub fn flat(size: f32) -> Self {
        Level {
            terrain: Terrain::generate(size, 2, |_| 0.0),
            solids: Vec::new(),
            trees: Vec::new(),
            targets: Vec::new(),
            spawn: Vec3::new(0.0, 0.1, 0.0),
            spawn_yaw: 0.0,
            teams: Vec::new(),
            capture_points: Vec::new(),
            rooms: Vec::new(),
            doorways: Vec::new(),
            windows: Vec::new(),
            staircases: Vec::new(),
            sentry: SpawnPoint { pos: Vec3::new(0.0, 0.1, 0.0), yaw: 0.0 },
            helipads: Vec::new(),
            vehicles: Vec::new(),
        }
    }

    pub fn collision(&self) -> CollisionWorld {
        let mut world = CollisionWorld::default();
        let corners = (0..self.terrain.indices.len()).map(|i| self.terrain.corner_surfaces(i)).collect();
        world.add_varied_mesh(self.terrain.vertices.clone(), self.terrain.indices.clone(), corners);
        for s in &self.solids {
            world.add_box(s.center, s.half, s.rotation, s.paint.surface());
        }
        for t in &self.trees {
            let half = Vec3::new(TRUNK_HALF_WIDTH, t.height * TRUNK_HALF_HEIGHT, TRUNK_HALF_WIDTH);
            world.add_box(t.base + Vec3::Y * half.y, half, Quat::IDENTITY, Surface::Wood);
        }
        world
    }

    /// The crowns of the trees, which only something as big as a rotor cares about.
    pub fn canopies(&self) -> CollisionWorld {
        let mut world = CollisionWorld::default();
        for t in &self.trees {
            let half = Vec3::new(t.radius, t.height * TRUNK_HALF_HEIGHT, t.radius);
            world.add_box(t.base + Vec3::Y * (t.height - half.y), half, Quat::IDENTITY, Surface::Wood);
        }
        world
    }

    /// A firing range looking down -Z from the spawn, with cover to practise from, a squad of
    /// targets to prove you cannot mow them down, and a small obstacle course off to the side.
    pub fn training_range() -> Self {
        let terrain = Terrain::generate(640.0, 96, range_height);
        let spawn = Vec3::new(0.0, range_height(Vec2::ZERO) + 0.1, 0.0);
        let mut level = Level {
            terrain,
            solids: Vec::new(),
            trees: Vec::new(),
            targets: Vec::new(),
            spawn,
            spawn_yaw: 0.0,
            teams: Vec::new(),
            capture_points: Vec::new(),
            rooms: Vec::new(),
            doorways: Vec::new(),
            windows: Vec::new(),
            staircases: Vec::new(),
            sentry: SpawnPoint { pos: spawn + RANGE_SENTRY, yaw: 0.05 },
            helipads: Vec::new(),
            vehicles: Vec::new(),
        };
        for (kind, x, z) in [(0, 14.0, 7.0), (0, -14.0, 7.0), (1, -22.0, 7.0)] {
            level.vehicles.push(VehicleSpawn { kind, pos: Vec3::new(x, level.ground(x, z), z), yaw: 0.0 });
        }
        level.add_targets();
        level.add_firing_line();
        level.add_obstacles();
        level.add_compound();
        level.add_scenery();
        level.add_sides();
        level
    }

    /// Two sides and a line of points down the lanes, so any mode can be played here too.
    fn add_sides(&mut self) {
        let row = |z: f32, yaw: f32| {
            let spots = (0..8).map(|i| {
                let x = 8.0 + i as f32 * 3.0;
                SpawnPoint { pos: Vec3::new(x, range_height(Vec2::new(x, z)) + 0.3, z), yaw }
            });
            spots.collect::<Vec<_>>()
        };
        self.teams.push(Team { name: "Near".into(), spawns: row(12.0, 0.0), base_radius: 13.0 });
        self.teams.push(Team { name: "Far".into(), spawns: row(-285.0, std::f32::consts::PI), base_radius: 13.0 });
        for (name, x, z) in [("Line", 12.0, -20.0), ("Bank", 40.0, -110.0), ("Gully", 30.0, -200.0), ("Far End", 20.0, -250.0)] {
            let center = Vec3::new(x, range_height(Vec2::new(x, z)), z);
            self.capture_points.push(CapturePoint { name: name.into(), center, radius: 18.0 });
        }
        let (x, z) = RANGE_HELIPAD;
        self.helipads.push(SpawnPoint { pos: Vec3::new(x, range_height(Vec2::new(x, z)), z), yaw: 0.0 });
    }

    fn ground(&self, x: f32, z: f32) -> f32 {
        range_height(Vec2::new(x, z))
    }

    fn add(&mut self, center: Vec3, half: Vec3, rotation: Quat, paint: Paint) {
        self.solids.push(Solid { center, half, rotation, paint });
    }

    /// A box sitting on the ground at (x, z).
    fn place(&mut self, x: f32, z: f32, half: Vec3, yaw: f32, paint: Paint) {
        let y = self.ground(x, z) + half.y - 0.05;
        self.add(Vec3::new(x, y, z), half, Quat::from_rotation_y(yaw), paint);
    }

    fn add_targets(&mut self) {
        for (range, count) in [(25.0, 3), (50.0, 3), (100.0, 3), (200.0, 3), (300.0, 2)] {
            for i in 0..count {
                let x = (i as f32 - (count - 1) as f32 * 0.5) * (4.0 + range * 0.06);
                self.targets.push(Vec3::new(x, self.ground(x, -range), -range));
            }
        }
        // A loose squad, close enough to tempt a mag dump.
        for (x, z) in [(-22.0, -60.0), (-19.0, -63.0), (-16.0, -58.0), (-13.0, -62.0), (-10.0, -59.0)] {
            self.targets.push(Vec3::new(x, self.ground(x, z), z));
        }
    }

    fn add_firing_line(&mut self) {
        // Waist-high sandbags to fire over, either crouched or standing.
        for i in 0..5 {
            let x = -6.0 + i as f32 * 3.0;
            self.place(x, -6.5, Vec3::new(1.4, 0.45, 0.35), 0.03 * i as f32, Paint::Sandbag);
        }
        self.place(0.0, 5.0, Vec3::new(5.0, 0.03, 8.0), 0.0, Paint::Concrete);
    }

    fn add_obstacles(&mut self) {
        let x = 26.0;
        let z = 2.0;
        let g = self.ground(x, z);

        // Stairs up to a platform you can shoot from.
        for i in 0..8 {
            let h = 0.28 * (i + 1) as f32;
            self.add(
                Vec3::new(x, g + h * 0.5, z - i as f32 * 0.45),
                Vec3::new(1.5, h * 0.5, 0.225),
                Quat::IDENTITY,
                Paint::Concrete,
            );
        }
        let top = g + 0.28 * 8.0;
        self.add(Vec3::new(x, g + top * 0.0 + 0.28 * 4.0, z - 6.0), Vec3::new(3.0, 0.28 * 4.0, 2.4), Quat::IDENTITY, Paint::Concrete);

        // A walkable ramp and a slope too steep to climb.
        self.add(
            Vec3::new(x + 8.0, g + 1.0, z - 3.0),
            Vec3::new(2.0, 0.2, 6.0),
            Quat::from_rotation_x(0.42),
            Paint::Wood,
        );
        self.add(
            Vec3::new(x + 14.0, g + 2.0, z - 3.0),
            Vec3::new(2.0, 0.2, 4.0),
            Quat::from_rotation_x(1.05),
            Paint::Rock,
        );

        // A low tunnel: crouch height, then prone height.
        let (tx, tz) = (x + 20.0, z - 3.0);
        self.add(Vec3::new(tx, g + 1.65, tz), Vec3::new(1.5, 0.3, 3.0), Quat::IDENTITY, Paint::Concrete);
        self.add(Vec3::new(tx, g + 1.15, tz - 6.0), Vec3::new(1.5, 0.3, 3.0), Quat::IDENTITY, Paint::Concrete);
        for side in [-1.8_f32, 1.8] {
            self.add(Vec3::new(tx + side, g + 0.75, tz - 3.0), Vec3::new(0.3, 1.2, 6.0), Quat::IDENTITY, Paint::Concrete);
        }
    }

    fn add_compound(&mut self) {
        // A roofed building with a doorway, standing in for the urban fights to come.
        let (x, z) = (-34.0, -24.0);
        let g = self.ground(x, z);
        let h = 1.5;
        let wall = |dx: f32, dz: f32, half: Vec3| (Vec3::new(x + dx, g + h, z + dz), half);
        for (center, half) in [
            wall(0.0, -6.0, Vec3::new(6.0, h, 0.2)),
            wall(-6.0, 0.0, Vec3::new(0.2, h, 6.0)),
            wall(6.0, 0.0, Vec3::new(0.2, h, 6.0)),
            wall(-3.75, 6.0, Vec3::new(2.25, h, 0.2)),
            wall(3.75, 6.0, Vec3::new(2.25, h, 0.2)),
        ] {
            self.add(center, half, Quat::IDENTITY, Paint::Concrete);
        }
        self.add(Vec3::new(x, g + 2.6, z + 6.0), Vec3::new(1.5, 0.4, 0.2), Quat::IDENTITY, Paint::Concrete);
        self.add(Vec3::new(x, g + 3.1, z), Vec3::new(6.4, 0.12, 6.4), Quat::IDENTITY, Paint::Concrete);

        for (dx, dz, half, yaw) in [
            (-8.5, 9.0, Vec3::new(0.6, 0.6, 0.6), 0.3),
            (-8.0, 10.4, Vec3::new(0.5, 0.5, 0.5), 0.8),
            (9.5, 8.0, Vec3::new(1.2, 1.1, 2.6), 0.1),
            (1.0, 13.0, Vec3::new(3.0, 0.5, 0.35), 0.0),
        ] {
            let paint = if half.y > 1.0 { Paint::Metal } else { Paint::Wood };
            self.place(x + dx, z + dz, half, yaw, paint);
        }
    }

    fn add_scenery(&mut self) {
        let mut rng = Scatter(7);
        let mut placed = 0;
        while placed < 170 {
            let x = rng.range(-300.0, 300.0);
            let z = rng.range(-300.0, 300.0);
            // Keep the firing lanes and play area open.
            let in_lane = x.abs() < 30.0 && z < 10.0;
            let near_spawn = Vec2::new(x, z).length() < 22.0;
            let near_buildings = (-60.0..60.0).contains(&x) && (-40.0..20.0).contains(&z);
            if in_lane || near_spawn || near_buildings {
                continue;
            }
            placed += 1;
            if rng.range(0.0, 1.0) < 0.72 {
                self.trees.push(Tree {
                    base: Vec3::new(x, self.ground(x, z) - 0.1, z),
                    height: rng.range(5.0, 10.0),
                    radius: rng.range(1.4, 2.6),
                    kind: TreeKind::Fir,
                });
            } else {
                let s = rng.range(0.5, 1.9);
                let half = Vec3::new(s, s * rng.range(0.5, 0.9), s * rng.range(0.7, 1.2));
                let rotation = Quat::from_euler(
                    glam::EulerRot::YXZ,
                    rng.range(0.0, std::f32::consts::TAU),
                    rng.range(-0.35, 0.35),
                    rng.range(-0.35, 0.35),
                );
                let center = Vec3::new(x, self.ground(x, z) + half.y * 0.4, z);
                self.add(center, half, rotation, Paint::Rock);
            }
        }
    }
}

impl Terrain {
    /// How far a triangle has gone from growing things to bare earth, from 0 to 1, by its slope.
    pub fn bareness(&self, triangle: usize) -> f32 {
        let [a, b, c] = self.indices[triangle].map(|v| self.vertices[v as usize]);
        let normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
        (1.0 - normal.y).clamp(0.0, 0.2) * 5.0
    }

    /// What a triangle sounds like at each corner, from the same data that colours it: bare earth
    /// where the slope has it drawn that way, otherwise whatever the ground is there.
    pub fn corner_surfaces(&self, triangle: usize) -> [Surface; 3] {
        if self.bareness(triangle) > 0.5 {
            return [Surface::Dirt; 3];
        }
        self.indices[triangle].map(|v| self.ground[v as usize].surface())
    }

    pub fn generate(size: f32, cells: u32, height: impl Fn(Vec2) -> f32) -> Self {
        let step = size / cells as f32;
        let mut vertices = Vec::with_capacity(((cells + 1) * (cells + 1)) as usize);
        for j in 0..=cells {
            for i in 0..=cells {
                let p = Vec2::new(i as f32 * step - size * 0.5, j as f32 * step - size * 0.5);
                vertices.push(Vec3::new(p.x, height(p), p.y));
            }
        }
        let row = cells + 1;
        let mut indices = Vec::with_capacity((cells * cells * 2) as usize);
        for j in 0..cells {
            for i in 0..cells {
                let a = j * row + i;
                let (b, c, d) = (a + 1, a + row, a + row + 1);
                // Alternate the diagonal so facets do not line up into visible stripes.
                if (i + j) % 2 == 0 {
                    indices.push([a, c, b]);
                    indices.push([b, c, d]);
                } else {
                    indices.push([a, c, d]);
                    indices.push([a, d, b]);
                }
            }
        }
        let ground = vec![Ground::Natural; vertices.len()];
        Self { size, cells, vertices, indices, ground }
    }
}

/// Flat where you shoot from and at, rolling everywhere else.
fn range_height(p: Vec2) -> f32 {
    let waves = (p.x * 0.021 + 1.3).sin() * 3.2
        + (p.y * 0.017 - 0.4).sin() * 3.6
        + (p.x * 0.053 + p.y * 0.041).sin() * 1.3
        + (p.x * 0.11 - p.y * 0.097 + 2.0).sin() * 0.42;

    // Blend towards a level lane, so targets stay visible at range.
    let lane = smoothstep(26.0, 70.0, p.x.abs()).max(smoothstep(30.0, 90.0, p.y)).max(smoothstep(340.0, 420.0, -p.y));
    let base = 0.0;
    base + waves * lane + berm(p)
}

/// The backstop behind the furthest targets.
fn berm(p: Vec2) -> f32 {
    smoothstep(-300.0, -318.0, p.y) * 10.0
}

pub(crate) fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Scatter(u32);

impl Scatter {
    fn range(&mut self, min: f32, max: f32) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        let u = (self.0 >> 8) as f32 / (1u32 << 24) as f32;
        min + (max - min) * u
    }
}

#[cfg(test)]
mod town_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use prs_collision::Capsule;

    #[test]
    fn the_range_has_clear_spawns_for_both_sides_and_a_chain_of_points() {
        let level = Level::training_range();
        let world = level.collision();
        let body = Capsule { height: 1.8, radius: 0.3 };
        assert_eq!(level.teams.len(), 2);
        assert!(level.capture_points.len() >= 3);
        for spawn in level.teams.iter().flat_map(|t| &t.spawns) {
            assert!(world.capsule_is_clear(spawn.pos, body), "{}", spawn.pos);
        }
        for point in &level.capture_points {
            assert!(world.capsule_is_clear(point.center + Vec3::Y * 0.1, body), "{}", point.name);
        }
    }

    #[test]
    fn spawn_is_clear_with_ground_below() {
        let level = Level::training_range();
        let world = level.collision();
        let body = Capsule { height: 1.8, radius: 0.3 };
        assert!(world.capsule_is_clear(level.spawn, body));
        let hit = world.cast_capsule(level.spawn, body, Vec3::NEG_Y, 5.0).unwrap();
        assert!(hit.normal.y > 0.9);
    }

    #[test]
    fn every_target_can_be_seen_from_the_firing_line() {
        let level = Level::training_range();
        let world = level.collision();
        let eye = level.spawn + Vec3::Y * 1.62;
        for &t in &level.targets {
            let chest = t + Vec3::Y * 1.3;
            let to = chest - eye;
            let hit = world.raycast(eye, to.normalize(), to.length());
            assert!(hit.is_none(), "target at {t} is blocked: {hit:?}");
        }
    }
}

#[cfg(test)]
mod climb_tests;
