//! Static world geometry and the handful of queries gameplay needs from it.
//!
//! Wraps parry so no other crate sees a physics library. Runs without Bevy, which is what lets
//! the simulation run in unit tests and, later, on a headless server.

use glam::{Quat, Vec3};
use std::collections::HashMap;
use parry3d::math::Pose;
use parry3d::query::{self, PointQueryWithLocation, Ray, RayCast, ShapeCastOptions};
use parry3d::shape::{Ball, Capsule as ParryCapsule, Cuboid, SharedShape};

/// Parry's GJK casts lose f32 precision against large shapes, to the point of missing a floor the
/// capsule is touching. Boxes bigger than this are split into tiles.
const MAX_BOX_HALF_EXTENT: f32 = 10.0;
const TILE_OVERLAP: f32 = 0.001;
const CELL: f32 = 16.0;
/// More cells than this and it is quicker to check everything: a collider goes in the wide list, and
/// a query walks the whole collider list.
const MAX_CELLS: i64 = 64;
/// How far from a capsule's contact point to look for the triangle it touched.
const SURFACE_SEARCH: f32 = 0.5;
/// How big a box's corners are when asking which of them are in something.
const CORNER_RADIUS: f32 = 0.02;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Surface {
    #[default]
    Dirt,
    Grass,
    Gravel,
    Wood,
    Concrete,
    Metal,
    Flesh,
}

/// Upright capsule, described by its total height so callers can think in body sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    pub height: f32,
    pub radius: f32,
}

impl Capsule {
    fn shape(self) -> ParryCapsule {
        ParryCapsule::new_y((self.height * 0.5 - self.radius).max(0.001), self.radius)
    }

    fn center(self, feet: Vec3) -> Vec3 {
        feet + Vec3::Y * (self.height * 0.5)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CastHit {
    pub distance: f32,
    /// Points away from the obstacle, towards the capsule.
    pub normal: Vec3,
    pub surface: Surface,
}

/// A box touching something, as a solid body wants to hear about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxContact {
    pub point: Vec3,
    /// Points away from the obstacle, the way the box has to go to get out.
    pub normal: Vec3,
    pub depth: f32,
    pub surface: Surface,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub distance: f32,
    pub normal: Vec3,
    pub surface: Surface,
}

struct Collider {
    pose: Pose,
    shape: SharedShape,
    surface: Surface,
    /// For a mesh whose surface varies: what each triangle is at each of its corners.
    corners: Option<Vec<[Surface; 3]>>,
    min: Vec3,
    max: Vec3,
}

impl Collider {
    /// The surface at a point on triangle `triangle` of a mesh: whichever covers the most of what
    /// is blended there, so a change of ground falls where its colour does.
    fn surface_on(&self, triangle: u32, point: Vec3) -> Surface {
        let (Some(corners), Some(mesh)) = (&self.corners, self.shape.as_trimesh()) else {
            return self.surface;
        };
        let [a, b, c] = corners[triangle as usize];
        if a == b && b == c {
            return a;
        }
        let t = mesh.triangle(triangle);
        let area = |p: Vec3, q: Vec3| (p - point).cross(q - point).length();
        let weights = [area(t.b, t.c), area(t.c, t.a), area(t.a, t.b)];
        let weight_of = |surface: Surface| {
            [a, b, c].iter().zip(weights).filter(|(s, _)| **s == surface).map(|(_, w)| w).sum::<f32>()
        };
        [a, b, c].into_iter().max_by(|x, y| weight_of(*x).total_cmp(&weight_of(*y))).unwrap_or(a)
    }

    /// The surface at a point a capsule touched, which does not say which triangle it was.
    fn surface_near(&self, point: Vec3) -> Surface {
        let Some(mesh) = self.corners.as_ref().and(self.shape.as_trimesh()) else {
            return self.surface;
        };
        mesh.project_local_point_and_get_location_with_max_dist(point, false, SURFACE_SEARCH)
            .map_or(self.surface, |(_, (triangle, _))| self.surface_on(triangle, point))
    }
}

/// A box that moves, like the hull of a vehicle. Bodies walk into it, stand on it and climb it, and it
/// is put here fresh by whoever moves it. Rays do not see one unless they ask: a bullet and a sound have
/// their own idea of a vehicle, and a vehicle's wheels must not find itself.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mover {
    pose: Pose,
    shape: Cuboid,
    reach: f32,
}

#[derive(Default)]
pub struct CollisionWorld {
    colliders: Vec<Collider>,
    /// Which colliders touch each cell of a grid across the ground, so that a query only looks at
    /// what is nearby however big the map is.
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Colliders too big for cells, like the terrain.
    wide: Vec<u32>,
    movers: Vec<Mover>,
}

impl CollisionWorld {
    pub fn clear_movers(&mut self) {
        self.movers.clear();
    }

    pub fn add_mover(&mut self, center: Vec3, half: Vec3, rotation: Quat) {
        self.movers.push(Mover { pose: Pose::from_parts(center, rotation), shape: Cuboid::new(half), reach: half.length() });
    }

    /// The boxes that could touch a capsule's sweep from `center` over `span`.
    fn movers_near(&self, center: Vec3, span: f32, capsule: Capsule) -> impl Iterator<Item = &Mover> {
        let room = span + capsule.height;
        self.movers.iter().filter(move |m| m.pose.translation.distance_squared(center) < (m.reach + room).powi(2))
    }

    /// How high the land itself is at this point, with whatever is built on it left out. None off the
    /// edge of the map.
    pub fn land_height(&self, x: f32, z: f32) -> Option<f32> {
        const ABOVE: f32 = 10_000.0;
        let ray = Ray::new(Vec3::new(x, ABOVE, z), Vec3::NEG_Y);
        self.wide
            .iter()
            .map(|&id| &self.colliders[id as usize])
            .filter(|c| c.shape.as_trimesh().is_some())
            .filter_map(|c| c.shape.cast_ray(&c.pose, &ray, ABOVE * 2.0, true))
            .map(|distance| ABOVE - distance)
            .max_by(f32::total_cmp)
    }

    /// A ray as a walking body feels the world: everything, with moving boxes too.
    pub fn walk_ray(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<RayHit> {
        let mut best = self.raycast(origin, dir, max_distance);
        let ray = Ray::new(origin, dir);
        for m in &self.movers {
            if let Some(hit) = m.shape.cast_ray_and_get_normal(&m.pose, &ray, max_distance, true)
                && best.is_none_or(|b| hit.time_of_impact < b.distance)
            {
                best = Some(RayHit { distance: hit.time_of_impact, normal: hit.normal, surface: Surface::Metal });
            }
        }
        best
    }

    pub fn add_box(&mut self, center: Vec3, half_extents: Vec3, rotation: Quat, surface: Surface) {
        let longest = half_extents.max_element();
        if longest > MAX_BOX_HALF_EXTENT {
            let axis = if longest == half_extents.x {
                Vec3::X
            } else if longest == half_extents.y {
                Vec3::Y
            } else {
                Vec3::Z
            };
            // Tiles overlap a hair: a ray exactly along a shared face could slip between them.
            let half = half_extents - axis * (longest * 0.5 - TILE_OVERLAP);
            let shift = rotation * (axis * longest * 0.5);
            self.add_box(center - shift, half, rotation, surface);
            self.add_box(center + shift, half, rotation, surface);
            return;
        }
        self.add(
            Pose::from_parts(center, rotation),
            SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z),
            surface,
            None,
        );
    }

    /// Panics on an invalid mesh: level data is authored, so that is a programming error.
    pub fn add_mesh(&mut self, vertices: Vec<Vec3>, indices: Vec<[u32; 3]>, surface: Surface) {
        let shape = SharedShape::trimesh(vertices, indices).expect("invalid collision mesh");
        self.add(Pose::IDENTITY, shape, surface, None);
    }

    /// A mesh made of different surfaces, given for each corner of each triangle. Panics on an
    /// invalid mesh, like [`Self::add_mesh`].
    pub fn add_varied_mesh(&mut self, vertices: Vec<Vec3>, indices: Vec<[u32; 3]>, corners: Vec<[Surface; 3]>) {
        assert_eq!(indices.len(), corners.len(), "a surface for each triangle");
        let shape = SharedShape::trimesh(vertices, indices).expect("invalid collision mesh");
        self.add(Pose::IDENTITY, shape, Surface::default(), Some(corners));
    }

    fn add(&mut self, pose: Pose, shape: SharedShape, surface: Surface, corners: Option<Vec<[Surface; 3]>>) {
        let aabb = shape.compute_aabb(&pose);
        let (min, max) = (aabb.mins, aabb.maxs);
        let id = self.colliders.len() as u32;
        self.colliders.push(Collider { pose, shape, surface, corners, min, max });
        let ((x0, x1), (z0, z1)) = (cell_range(min.x, max.x), cell_range(min.z, max.z));
        if cell_count(x0, x1, z0, z1) > MAX_CELLS {
            self.wide.push(id);
            return;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.cells.entry((x, z)).or_default().push(id);
            }
        }
    }

    fn near(&self, min: Vec3, max: Vec3) -> impl Iterator<Item = &Collider> {
        let overlaps = move |c: &&Collider| {
            c.min.x <= max.x && c.max.x >= min.x && c.min.y <= max.y && c.max.y >= min.y && c.min.z <= max.z && c.max.z >= min.z
        };
        let ((x0, x1), (z0, z1)) = (cell_range(min.x, max.x), cell_range(min.z, max.z));
        let local = cell_count(x0, x1, z0, z1) <= MAX_CELLS;
        let everything: &[Collider] = if local { &[] } else { &self.colliders };
        let wide: &[u32] = if local { &self.wide } else { &[] };
        let cells_x = x0..=if local { x1 } else { x0 - 1 };
        let cells_z = z0..=z1;
        let gridded = cells_x
            .flat_map(move |x| cells_z.clone().map(move |z| (x, z)))
            .filter_map(|cell| self.cells.get(&cell).map(|ids| (cell, ids)))
            .flat_map(move |(cell, ids)| {
                // A collider in several cells counts only in the first one both boxes share.
                ids.iter().map(|&id| &self.colliders[id as usize]).filter(move |c| {
                    overlaps(c) && cell == (cell_of(c.min.x.max(min.x)), cell_of(c.min.z.max(min.z)))
                })
            });
        everything
            .iter()
            .chain(wide.iter().map(|&id| &self.colliders[id as usize]))
            .filter(overlaps)
            .chain(gridded)
    }

    /// Sweeps a capsule standing at `feet` along unit vector `dir`. Motion that leaves an existing
    /// overlap is not reported, so a slightly embedded capsule can still walk free.
    pub fn cast_capsule(
        &self,
        feet: Vec3,
        capsule: Capsule,
        dir: Vec3,
        max_distance: f32,
    ) -> Option<CastHit> {
        let shape = capsule.shape();
        let center = capsule.center(feet);
        let pose = Pose::from_translation(center);
        let reach = Vec3::new(capsule.radius, capsule.height * 0.5, capsule.radius);
        let end = center + dir * max_distance;
        let options = ShapeCastOptions {
            max_time_of_impact: max_distance,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };

        let mut best: Option<(&Collider, Vec3, CastHit)> = None;
        for c in self.near(center.min(end) - reach, center.max(end) + reach) {
            let Ok(Some(hit)) =
                query::cast_shapes(&pose, dir, &shape, &c.pose, Vec3::ZERO, c.shape.as_ref(), options)
            else {
                continue;
            };
            if best.is_none_or(|(_, _, b)| hit.time_of_impact < b.distance) {
                let found = CastHit { distance: hit.time_of_impact, normal: -hit.normal1, surface: c.surface };
                best = Some((c, hit.witness2, found));
            }
        }
        let found = best.map(|(c, contact, hit)| CastHit { surface: c.surface_near(contact), ..hit });
        let mut found = found;
        for m in self.movers_near(center, max_distance, capsule) {
            let Ok(Some(hit)) = query::cast_shapes(&pose, dir, &shape, &m.pose, Vec3::ZERO, &m.shape, options) else {
                continue;
            };
            if found.is_none_or(|b: CastHit| hit.time_of_impact < b.distance) {
                found = Some(CastHit { distance: hit.time_of_impact, normal: -hit.normal1, surface: Surface::Metal });
            }
        }
        found
    }

    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<RayHit> {
        let ray = Ray::new(origin, dir);
        let end = origin + dir * max_distance;
        let mut best: Option<RayHit> = None;
        for c in self.near(origin.min(end), origin.max(end)) {
            let Some(hit) = c.shape.cast_ray_and_get_normal(&c.pose, &ray, max_distance, true)
            else {
                continue;
            };
            if best.is_none_or(|b| hit.time_of_impact < b.distance) {
                best = Some(RayHit {
                    distance: hit.time_of_impact,
                    normal: hit.normal,
                    surface: c.surface_on(hit.subshape, origin + dir * hit.time_of_impact),
                });
            }
        }
        best
    }

    /// Offset that moves the capsule out of anything it is embedded in, or zero if it is clear.
    pub fn depenetration(&self, feet: Vec3, capsule: Capsule, skin: f32) -> Vec3 {
        let mut offset = Vec3::ZERO;
        let shape = capsule.shape();
        for _ in 0..3 {
            let center = capsule.center(feet + offset);
            let pose = Pose::from_translation(center);
            let reach = Vec3::new(capsule.radius, capsule.height * 0.5, capsule.radius) + skin;
            let mut pushed = false;
            for c in self.near(center - reach, center + reach) {
                if let Ok(Some(contact)) = query::contact(&pose, &shape, &c.pose, c.shape.as_ref(), 0.0)
                    && contact.dist < 0.0
                {
                    offset += contact.normal1 * (contact.dist - skin);
                    pushed = true;
                    break;
                }
            }
            for m in self.movers_near(center, skin, capsule) {
                if pushed {
                    break;
                }
                if let Ok(Some(contact)) = query::contact(&pose, &shape, &m.pose, &m.shape, 0.0)
                    && contact.dist < 0.0
                {
                    offset += contact.normal1 * (contact.dist - skin);
                    pushed = true;
                }
            }
            if !pushed {
                break;
            }
        }
        offset
    }

    /// Everything a turned box is pushing into, added to `out`: the deepest point against each thing it
    /// touches, and then any of its corners that are in that thing, so a body lying flat is held up
    /// all over rather than rocking about one point at a time.
    pub fn box_contacts(&self, center: Vec3, half: Vec3, rotation: Quat, out: &mut Vec<BoxContact>) {
        let pose = Pose::from_parts(center, rotation);
        let shape = Cuboid::new(half);
        let reach = (rotation * half.with_y(0.0).with_z(0.0)).abs()
            + (rotation * Vec3::new(0.0, half.y, 0.0)).abs()
            + (rotation * Vec3::new(0.0, 0.0, half.z)).abs();
        let ball = Ball::new(CORNER_RADIUS);
        for c in self.near(center - reach, center + reach) {
            let Ok(Some(deepest)) = query::contact(&pose, &shape, &c.pose, c.shape.as_ref(), 0.0) else {
                continue;
            };
            if deepest.dist >= 0.0 {
                continue;
            }
            let mut add = |point: Vec3, normal: Vec3, depth: f32| {
                // Ground is only ever got out of upwards, however deep something has gone into it.
                let normal = if c.shape.as_trimesh().is_some() && normal.y < 0.0 { -normal } else { normal };
                out.push(BoxContact { point, normal, depth, surface: c.surface_near(point) });
            };
            add(deepest.point2, -deepest.normal1, -deepest.dist);
            for sign in [Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, -1.0, -1.0), Vec3::new(-1.0, 1.0, -1.0), Vec3::new(1.0, 1.0, -1.0), Vec3::new(-1.0, -1.0, 1.0), Vec3::new(1.0, -1.0, 1.0), Vec3::new(-1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 1.0)] {
                let corner = center + rotation * (half * sign);
                let at = Pose::from_translation(corner);
                if let Ok(Some(touch)) = query::contact(&at, &ball, &c.pose, c.shape.as_ref(), 0.0)
                    && touch.dist < 0.0
                {
                    add(touch.point2, -touch.normal1, -touch.dist);
                }
            }
        }
    }

    pub fn capsule_is_clear(&self, feet: Vec3, capsule: Capsule) -> bool {
        let shape = capsule.shape();
        let center = capsule.center(feet);
        let pose = Pose::from_translation(center);
        let reach = Vec3::new(capsule.radius, capsule.height * 0.5, capsule.radius);
        let walls = self.near(center - reach, center + reach).all(|c| {
            !matches!(
                query::contact(&pose, &shape, &c.pose, c.shape.as_ref(), 0.0),
                Ok(Some(contact)) if contact.dist < 0.0
            )
        });
        walls
            && self.movers_near(center, 0.0, capsule).all(|m| {
                !matches!(query::contact(&pose, &shape, &m.pose, &m.shape, 0.0), Ok(Some(contact)) if contact.dist < 0.0)
            })
    }
}

fn cell_of(at: f32) -> i32 {
    (at / CELL).floor() as i32
}

fn cell_range(min: f32, max: f32) -> (i32, i32) {
    (cell_of(min), cell_of(max))
}

fn cell_count(x0: i32, x1: i32, z0: i32, z1: i32) -> i64 {
    (x1 as i64 - x0 as i64 + 1) * (z1 as i64 - z0 as i64 + 1)
}

/// Distance along a ray to an upright capsule standing at `feet`.
pub fn ray_vs_capsule(
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
    feet: Vec3,
    capsule: Capsule,
) -> Option<f32> {
    let pose = Pose::from_translation(capsule.center(feet));
    capsule
        .shape()
        .cast_ray(&pose, &Ray::new(origin, dir), max_distance, true)
}

/// Where a ray enters a capsule running from `a` to `b`, however that lies. Each bone of a body is one
/// of these. A ray that starts inside counts as hitting at once.
pub fn ray_vs_bone(origin: Vec3, dir: Vec3, max_distance: f32, a: Vec3, b: Vec3, radius: f32) -> Option<f32> {
    parry3d::shape::Capsule::new(a, b, radius).cast_ray(&Pose::identity(), &Ray::new(origin, dir), max_distance, true)
}

/// How two turned boxes touch, if they do. The contact pushes the first one out of the second.
pub fn box_vs_box(a: (Vec3, Vec3, Quat), b: (Vec3, Vec3, Quat)) -> Option<BoxContact> {
    let (pose_a, pose_b) = (Pose::from_parts(a.0, a.2), Pose::from_parts(b.0, b.2));
    let contact = query::contact(&pose_a, &Cuboid::new(a.1), &pose_b, &Cuboid::new(b.1), 0.0).ok().flatten()?;
    (contact.dist < 0.0).then(|| BoxContact {
        point: contact.point2,
        normal: -contact.normal1,
        depth: -contact.dist,
        surface: Surface::Metal,
    })
}

/// Where a ray enters a turned box, and the normal there. A ray that starts inside counts as hitting at once.
pub fn ray_vs_box(origin: Vec3, dir: Vec3, max_distance: f32, center: Vec3, half: Vec3, rotation: Quat) -> Option<(f32, Vec3)> {
    let pose = Pose::from_parts(center, rotation);
    let hit = Cuboid::new(half).cast_ray_and_get_normal(&pose, &Ray::new(origin, dir), max_distance, true)?;
    Some((hit.time_of_impact, hit.normal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_on_the_ground_touches_it_at_its_corners_and_is_pushed_up() {
        let mut out = Vec::new();
        floor().box_contacts(Vec3::new(0.0, 0.45, 0.0), Vec3::new(1.0, 0.5, 2.0), Quat::IDENTITY, &mut out);
        assert!(out.len() >= 4, "{out:?}");
        assert!(out.iter().all(|c| c.normal.y > 0.99 && c.depth > 0.0), "{out:?}");
        out.clear();
        floor().box_contacts(Vec3::new(0.0, 0.6, 0.0), Vec3::new(1.0, 0.5, 2.0), Quat::IDENTITY, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_capsule_walks_on_and_into_a_mover_but_rays_do_not_see_one_unless_they_ask() {
        let mut w = floor();
        w.add_mover(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 2.0), Quat::IDENTITY);
        let top = Vec3::new(0.0, 4.0, 0.0);
        let hit = w.cast_capsule(top, BODY, Vec3::NEG_Y, 5.0).expect("the deck");
        assert!((hit.distance - 2.0).abs() < 0.01, "{hit:?}");
        assert_eq!(hit.surface, Surface::Metal);
        assert!(!w.capsule_is_clear(Vec3::new(0.0, 0.5, 0.0), BODY));
        assert!(w.capsule_is_clear(Vec3::new(4.0, 0.05, 0.0), BODY));
        assert!(w.depenetration(Vec3::new(0.9, 0.5, 0.0), BODY, 0.01).x > 0.1);
        assert!((w.raycast(top, Vec3::NEG_Y, 9.0).unwrap().distance - 4.0).abs() < 0.01, "only the floor");
        assert!((w.walk_ray(top, Vec3::NEG_Y, 9.0).unwrap().distance - 2.0).abs() < 0.01, "the deck");
        w.clear_movers();
        assert!(w.capsule_is_clear(Vec3::new(0.0, 0.5, 0.0), BODY));
    }

    #[test]
    fn deep_in_the_ground_a_box_still_comes_out_upwards() {
        let mut w = CollisionWorld::default();
        w.add_mesh(
            vec![Vec3::new(-50.0, 0.0, -50.0), Vec3::new(-50.0, 0.0, 50.0), Vec3::new(50.0, 0.0, 50.0), Vec3::new(50.0, 0.0, -50.0)],
            vec![[0, 1, 2], [0, 2, 3]],
            Surface::Dirt,
        );
        let mut out = Vec::new();
        w.box_contacts(Vec3::new(0.0, -0.3, 0.0), Vec3::new(1.0, 0.5, 2.0), Quat::IDENTITY, &mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|c| c.normal.y > 0.0), "{out:?}");
    }

    #[test]
    fn turned_boxes_are_hit_where_they_are() {
        let turned = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let (at, half) = (Vec3::new(0.0, 0.0, -5.0), Vec3::new(1.0, 1.0, 1.0));
        let (d, _) = ray_vs_box(Vec3::ZERO, Vec3::NEG_Z, 10.0, at, half, turned).unwrap();
        assert!((d - (5.0 - std::f32::consts::SQRT_2)).abs() < 1e-3, "{d}");
        assert!(ray_vs_box(Vec3::new(1.6, 0.0, 0.0), Vec3::NEG_Z, 10.0, at, half, turned).is_none());
        let contact = box_vs_box((Vec3::new(1.9, 0.0, -5.0), half, Quat::IDENTITY), (at, half, Quat::IDENTITY)).unwrap();
        assert!(contact.normal.x > 0.9 && (contact.depth - 0.1).abs() < 1e-3, "{contact:?}");
    }

    #[test]
    fn a_bone_is_hit_where_it_lies_and_missed_where_it_does_not() {
        let (a, b) = (Vec3::new(0.0, 1.0, -5.0), Vec3::new(1.0, 1.0, -5.0));
        let hit = ray_vs_bone(Vec3::new(0.5, 1.0, 0.0), Vec3::NEG_Z, 10.0, a, b, 0.1).unwrap();
        assert!((hit - 4.9).abs() < 1e-3, "{hit}");
        assert!(ray_vs_bone(Vec3::new(0.5, 1.5, 0.0), Vec3::NEG_Z, 10.0, a, b, 0.1).is_none());
        assert!(ray_vs_bone(Vec3::new(0.5, 1.0, 0.0), Vec3::NEG_Z, 3.0, a, b, 0.1).is_none());
    }

    const BODY: Capsule = Capsule {
        height: 1.8,
        radius: 0.3,
    };

    fn floor() -> CollisionWorld {
        let mut w = CollisionWorld::default();
        w.add_box(Vec3::new(0.0, -1.0, 0.0), Vec3::new(50.0, 1.0, 50.0), Quat::IDENTITY, Surface::Dirt);
        w
    }

    #[test]
    fn capsule_cast_down_finds_floor_with_upward_normal() {
        let hit = floor()
            .cast_capsule(Vec3::new(0.0, 2.0, 0.0), BODY, Vec3::NEG_Y, 10.0)
            .unwrap();
        assert!((hit.distance - 2.0).abs() < 0.01, "{hit:?}");
        assert!(hit.normal.y > 0.99, "{hit:?}");
    }

    #[test]
    fn capsule_cast_hits_triangle_mesh() {
        let mut w = CollisionWorld::default();
        w.add_mesh(
            vec![
                Vec3::new(-50.0, 0.0, -50.0),
                Vec3::new(-50.0, 0.0, 50.0),
                Vec3::new(50.0, 0.0, 50.0),
                Vec3::new(50.0, 0.0, -50.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Surface::Dirt,
        );
        let hit = w
            .cast_capsule(Vec3::new(1.0, 2.0, 1.0), BODY, Vec3::NEG_Y, 10.0)
            .unwrap();
        assert!((hit.distance - 2.0).abs() < 0.01, "{hit:?}");
        assert!(hit.normal.y > 0.99, "{hit:?}");
    }

    #[test]
    fn embedded_capsule_gets_pushed_out() {
        let w = floor();
        let feet = Vec3::new(0.0, -0.2, 0.0);
        assert!(!w.capsule_is_clear(feet, BODY));
        let out = w.depenetration(feet, BODY, 0.01);
        assert!(out.y > 0.19, "{out:?}");
        assert!(w.capsule_is_clear(feet + out, BODY));
    }

    /// Regression: an untiled 500 m box made casts miss a floor the capsule was nearly touching.
    #[test]
    fn huge_boxes_stay_reliable_right_at_contact() {
        let mut w = CollisionWorld::default();
        w.add_box(Vec3::new(0.0, -1.0, 0.0), Vec3::new(500.0, 1.0, 500.0), Quat::IDENTITY, Surface::Dirt);
        for i in 0..2000 {
            let f = i as f32;
            let feet = Vec3::new((f * 0.37).sin() * 400.0, 0.0005 + (i % 60) as f32 * 0.0025, (f * 0.11).cos() * 400.0);
            let hit = w.cast_capsule(feet, BODY, Vec3::NEG_Y, 0.2).expect("missed the floor");
            assert!((hit.distance - feet.y).abs() < 0.004, "{feet} {hit:?}");
            assert!(hit.normal.y > 0.99, "{feet} {hit:?}");
        }
        let out = w.depenetration(Vec3::new(1.0, -0.01, 1.0), BODY, 0.0);
        assert!(out.y > 0.005, "{out:?}");
    }

    /// Half grass and half gravel, split down the middle of a square at x = 0.
    fn two_surfaces() -> CollisionWorld {
        let mut w = CollisionWorld::default();
        let (g, s) = (Surface::Grass, Surface::Gravel);
        w.add_varied_mesh(
            vec![
                Vec3::new(-10.0, 0.0, -10.0),
                Vec3::new(-10.0, 0.0, 10.0),
                Vec3::new(0.0, 0.0, -10.0),
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(10.0, 0.0, -10.0),
                Vec3::new(10.0, 0.0, 10.0),
            ],
            vec![[0, 1, 2], [2, 1, 3], [2, 3, 4], [4, 3, 5]],
            vec![[g; 3], [g; 3], [s; 3], [s; 3]],
        );
        w
    }

    #[test]
    fn rays_and_capsules_report_the_surface_of_the_triangle_they_hit() {
        let w = two_surfaces();
        for (x, surface) in [(-5.0, Surface::Grass), (5.0, Surface::Gravel)] {
            let from = Vec3::new(x, 3.0, 1.0);
            assert_eq!(w.raycast(from, Vec3::NEG_Y, 5.0).unwrap().surface, surface);
            assert_eq!(w.cast_capsule(from, BODY, Vec3::NEG_Y, 5.0).unwrap().surface, surface);
        }
    }

    /// Within a blended triangle, the surface changes where the blend passes halfway.
    #[test]
    fn a_blended_triangle_is_whichever_surface_covers_most_at_that_point() {
        let (g, s) = (Surface::Grass, Surface::Gravel);
        let mut w = CollisionWorld::default();
        w.add_varied_mesh(
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 0.0)],
            vec![[0, 1, 2]],
            vec![[g, g, s]],
        );
        let at = |x: f32| w.raycast(Vec3::new(x, 1.0, 1.0), Vec3::NEG_Y, 2.0).unwrap().surface;
        assert_eq!(at(1.0), g);
        assert_eq!(at(8.0), s);
    }

    /// However a query falls across the cells, it finds each collider it touches exactly once.
    #[test]
    fn queries_find_everything_nearby_once() {
        let mut w = CollisionWorld::default();
        let positions = [(0.0, 0.0), (15.9, 16.1), (-40.0, 33.0), (200.0, -300.0)];
        for (x, z) in positions {
            w.add_box(Vec3::new(x, 0.0, z), Vec3::new(9.0, 1.0, 9.0), Quat::IDENTITY, Surface::Wood);
        }
        w.add_mesh(
            vec![Vec3::new(-900.0, -5.0, -900.0), Vec3::new(-900.0, -5.0, 900.0), Vec3::new(900.0, -5.0, 900.0)],
            vec![[0, 1, 2]],
            Surface::Dirt,
        );
        let count = |min: Vec3, max: Vec3| w.near(min, max).count();
        // A box, with the mesh below them all only found when the query reaches down to it.
        assert_eq!(count(Vec3::new(-1.0, -1.0, -1.0), Vec3::splat(1.0)), 1);
        assert_eq!(count(Vec3::new(-1.0, -6.0, -1.0), Vec3::splat(1.0)), 2);
        // Across a cell boundary, touching two boxes that each lie in several cells.
        assert_eq!(count(Vec3::new(5.0, -1.0, 5.0), Vec3::new(7.0, 1.0, 10.0)), 2);
        assert_eq!(count(Vec3::new(100.0, -6.0, 100.0), Vec3::new(101.0, 1.0, 101.0)), 1);
        // Too big for the grid, so it falls back on checking them all.
        assert_eq!(count(Vec3::new(-500.0, -9.0, -500.0), Vec3::new(500.0, 9.0, 500.0)), 5);
        assert_eq!(count(Vec3::new(-500.0, 3.0, -500.0), Vec3::new(500.0, 9.0, 500.0)), 0);
    }

    #[test]
    fn ray_reports_nearest_hit_and_normal() {
        let hit = floor().raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((hit.distance - 5.0).abs() < 0.001);
        assert!(hit.normal.y > 0.99);
    }
}
