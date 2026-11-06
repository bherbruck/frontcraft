//! The dome: one mesh that stays centred on the camera, so everything in it is at infinity. A smooth
//! gradient that lands on the fog's colour at the horizon, a sun, hazy ridges beyond the map, and
//! faceted clouds. None of it is lit or fogged: its colours are worked out here, once.

use crate::{Atmosphere, ViewReach, linear};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::TAU;

const DOME_RADIUS: f32 = 1300.0;
/// The dome's size as a share of how far the camera sees.
const DOME_IN_VIEW: f32 = 0.87;
const DOME_SIDES: usize = 96;
/// Elevations of the dome's rings in degrees, tightest where colour changes fastest.
const DOME_RINGS: [f32; 18] = [-90.0, -30.0, -6.0, -1.0, 0.0, 0.8, 2.0, 4.0, 7.0, 11.0, 16.0, 22.0, 30.0, 40.0, 52.0, 65.0, 78.0, 90.0];
/// Rings around the sun, as multiples of its radius: the disc's edge, then the halo fading out.
const HALO_RINGS: [f32; 6] = [1.0, 1.12, 1.6, 2.6, 4.2, 7.0];
const HALO_SIDES: usize = 32;
/// How far above the horizon, in degrees, the air gives way to open sky.
const HORIZON_BAND: f32 = 20.0;
const RIDGE_STEPS: usize = 180;
const CLOUD_PUFFS: u32 = 5;

const T: f32 = 1.618034;
pub const ICOSAHEDRON_CORNERS: [[f32; 3]; 12] = [
    [-1., T, 0.], [1., T, 0.], [-1., -T, 0.], [1., -T, 0.],
    [0., -1., T], [0., 1., T], [0., -1., -T], [0., 1., -T],
    [T, 0., -1.], [T, 0., 1.], [-T, 0., -1.], [-T, 0., 1.],
];
pub const ICOSAHEDRON_FACES: [[usize; 3]; 20] = [
    [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
    [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
    [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

#[derive(Component)]
pub(crate) struct SkyDome;

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A repeatable number from 0 to 1.
fn noise(i: f32) -> f32 {
    ((i * 12.9898).sin() * 43758.547).rem_euclid(1.0)
}

/// The colour of the empty sky in this direction, in the same scene light as the fog.
pub(crate) fn sky_color(a: &Atmosphere, dir: Vec3) -> Vec3 {
    let dir = dir.normalize();
    let elevation = dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    // Below the horizon there is only air: the haze of the level at the same bearing.
    let flat = Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::X);
    let haze = a.haze_towards(if elevation > 0.0 { dir } else { flat });
    let rise = smooth(elevation / HORIZON_BAND);
    let high = linear(a.sky.high).lerp(linear(a.sky.zenith), smooth((elevation - HORIZON_BAND) / 60.0));
    let sky = haze.lerp(high, rise);
    // The sun warms the air on its side, even high up.
    let to_sun = dir.dot(a.to_sun()).max(0.0);
    sky + linear(a.sky.glow) * to_sun.powi(10) * rise * 0.7
}

/// The colour the dome's mesh actually shows in this direction, between its vertices.
fn dome_color(a: &Atmosphere, dir: Vec3) -> Vec3 {
    let dir = dir.normalize();
    let elevation = dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    let upper = DOME_RINGS.iter().position(|ring| *ring >= elevation).unwrap_or(DOME_RINGS.len() - 1).max(1);
    let (low, high) = (DOME_RINGS[upper - 1], DOME_RINGS[upper]);
    let up = ((elevation - low) / (high - low)).clamp(0.0, 1.0);
    let bearing = dir.x.atan2(-dir.z).rem_euclid(TAU) / TAU * DOME_SIDES as f32;
    let side = bearing.floor();
    let at = |side: f32, ring: f32| sky_color(a, on_dome(side / DOME_SIDES as f32 * TAU, ring.to_radians()));
    let ring = |ring: f32| at(side, ring).lerp(at(side + 1.0, ring), bearing - side);
    ring(low).lerp(ring(high), up)
}

fn on_dome(bearing: f32, elevation: f32) -> Vec3 {
    let (up, flat) = elevation.sin_cos();
    Vec3::new(bearing.sin() * flat, up, -bearing.cos() * flat)
}

#[derive(Default)]
struct SkyMesh {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl SkyMesh {
    fn vertex(&mut self, at: Vec3, color: Vec3) -> u32 {
        self.positions.push(at.to_array());
        self.colors.push([color.x, color.y, color.z, 1.0]);
        self.positions.len() as u32 - 1
    }

    fn triangle(&mut self, corners: [Vec3; 3], color: Vec3) {
        for at in corners {
            let i = self.vertex(at, color);
            self.indices.push(i);
        }
    }

    fn dome(&mut self, a: &Atmosphere) {
        for elevation in DOME_RINGS {
            let e = elevation.to_radians();
            for side in 0..=DOME_SIDES {
                let dir = on_dome(side as f32 / DOME_SIDES as f32 * TAU, e);
                self.vertex(dir * DOME_RADIUS, sky_color(a, dir));
            }
        }
        let row = DOME_SIDES as u32 + 1;
        for ring in 0..DOME_RINGS.len() as u32 - 1 {
            for side in 0..DOME_SIDES as u32 {
                let (p, q) = (ring * row + side, (ring + 1) * row + side);
                self.indices.extend([p, p + 1, q + 1, p, q + 1, q]);
            }
        }
    }

    /// The disc and a halo around it, a hair in front of the dome.
    fn sun(&mut self, a: &Atmosphere) {
        let to_sun = a.to_sun();
        let radius = a.sky.sun_radius.to_radians().tan();
        let side = to_sun.cross(Vec3::Y).normalize_or(Vec3::X);
        let other = side.cross(to_sun);
        let at = |ring: f32, around: f32| (to_sun + (side * around.cos() + other * around.sin()) * radius * ring).normalize();
        let disc = linear(a.sky.sun_disc) * 3.0;
        let center = self.vertex(to_sun * DOME_RADIUS * 0.99, disc);
        let mut rims = Vec::new();
        for (r, ring) in HALO_RINGS.into_iter().enumerate() {
            let first = self.positions.len() as u32;
            rims.push(first);
            for i in 0..HALO_SIDES {
                let dir = at(ring, i as f32 / HALO_SIDES as f32 * TAU);
                // Lands on the dome's own colour at the outer ring, so the patch has no edge.
                let t = 1.0 - (r as f32 / (HALO_RINGS.len() - 1) as f32);
                let color = if r == 0 {
                    disc
                } else {
                    dome_color(a, dir) + linear(a.sky.halo) * t * t * t * 1.1
                };
                self.vertex(dir * DOME_RADIUS * 0.99, color);
            }
        }
        let sides = HALO_SIDES as u32;
        for i in 0..sides {
            let next = (i + 1) % sides;
            self.indices.extend([center, rims[0] + i, rims[0] + next]);
            for r in 0..rims.len() - 1 {
                let (p, q) = (rims[r], rims[r + 1]);
                self.indices.extend([p + i, q + i, q + next, p + i, q + next, p + next]);
            }
        }
    }

    /// Bands of far hills beyond the edge of the map, each more lost in the haze than the last.
    fn ridges(&mut self, a: &Atmosphere) {
        for (layer, (height, clear)) in a.sky.ridges.into_iter().enumerate() {
            let radius = DOME_RADIUS * (0.985 - 0.012 * (2 - layer) as f32);
            let seed = layer as f32 * 37.0 + 3.0;
            let peak = |i: usize| {
                let u = (i % RIDGE_STEPS) as f32 / RIDGE_STEPS as f32 * TAU;
                let roll = (u * 3.0 + seed).sin() * 0.3 + (u * 7.0 + seed * 2.1).sin() * 0.28 + (u * 17.0 + seed * 0.7).sin() * 0.14;
                let jag = noise((i % RIDGE_STEPS) as f32 + seed) * 0.2;
                (height * (0.58 + roll * 0.7 + jag)).max(0.15).to_radians()
            };
            for i in 0..RIDGE_STEPS {
                let (u0, u1) = (i as f32 / RIDGE_STEPS as f32 * TAU, (i + 1) as f32 / RIDGE_STEPS as f32 * TAU);
                let (top0, top1) = (on_dome(u0, peak(i)) * radius, on_dome(u1, peak(i + 1)) * radius);
                let (foot0, foot1) = (on_dome(u0, -0.02) * radius, on_dome(u1, -0.02) * radius);
                let dir = on_dome((u0 + u1) * 0.5, 0.01);
                // Facets lean towards or away from the sun, however little of that survives the haze.
                let slope = (peak(i + 1) - peak(i)).signum();
                let lit = (dir.cross(Vec3::Y).dot(a.to_sun()) * slope * 0.5 + 0.5).clamp(0.0, 1.0);
                let tint = linear(a.sky.ridge_tint) * (0.86 + lit * 0.3);
                let color = tint.lerp(a.haze_towards(dir), clear + (noise(i as f32 * 1.3 + seed) - 0.5) * 0.03);
                self.triangle([foot0, top0, top1], color);
                self.triangle([foot0, top1, foot1], color);
            }
        }
    }

    /// Heaps of flat-bottomed puffs, lit from the sun's side once, per facet.
    fn clouds(&mut self, a: &Atmosphere) {
        let to_sun = a.to_sun();
        let (lit, shade) = (linear(a.sky.cloud_lit) * 1.5, linear(a.sky.cloud_shade));
        let unit = (1.0 + T * T).sqrt();
        for cloud in 0..a.sky.clouds {
            let n = cloud as f32 * 3.17 + 0.5;
            let bearing = (cloud as f32 + noise(n) * 0.8) / a.sky.clouds as f32 * TAU * 3.0;
            let elevation = (7.0 + noise(n + 1.0).powi(2) * 34.0).to_radians();
            let center = on_dome(bearing, elevation) * DOME_RADIUS * 0.9;
            let width = 55.0 + noise(n + 2.0) * 95.0;
            let across = center.cross(Vec3::Y).normalize();
            let haze = a.haze_towards(center.normalize());
            let lost = 1.0 - smooth((elevation.to_degrees() - 3.0) / 22.0);
            for puff in 0..CLOUD_PUFFS {
                let m = n + puff as f32 * 5.3;
                let along = (puff as f32 / (CLOUD_PUFFS - 1) as f32 - 0.5) * 2.0 + (noise(m) - 0.5) * 0.3;
                let bulge = 1.0 - along * along * 0.75;
                let size = Vec3::new(width * (0.42 + noise(m + 1.0) * 0.24), width * 0.3 * bulge * (0.7 + noise(m + 2.0) * 0.6), width * 0.42);
                let middle = center + across * along * width;
                let turn = Quat::from_rotation_y(noise(m + 3.0) * TAU);
                let corner = |i: usize| {
                    let v = Vec3::from_array(ICOSAHEDRON_CORNERS[i]) / unit;
                    // The underside is cut flat, the way cumulus sit on their own layer of air.
                    middle + turn * (Vec3::new(v.x, v.y.max(-0.22), v.z) * size)
                };
                for [p, q, r] in ICOSAHEDRON_FACES {
                    let corners = [corner(p), corner(q), corner(r)];
                    let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize_or(Vec3::NEG_Y);
                    let light = (normal.dot(to_sun) * 0.5 + 0.5 + normal.y * 0.15).clamp(0.0, 1.0);
                    let color = shade.lerp(lit, light * light).lerp(haze, lost * 0.8);
                    self.triangle(corners, color);
                }
            }
        }
    }

    fn mesh(self) -> Mesh {
        let normals = vec![[0.0, 1.0, 0.0]; self.positions.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

pub(crate) fn spawn_sky(
    mut commands: Commands,
    atmosphere: Res<Atmosphere>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut sky = SkyMesh::default();
    sky.dome(&atmosphere);
    sky.sun(&atmosphere);
    sky.ridges(&atmosphere);
    sky.clouds(&atmosphere);
    commands.spawn((
        SkyDome,
        Mesh3d(meshes.add(sky.mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        NoFrustumCulling,
    ));
}

/// Kept on the camera, so you never get any closer to the sky, and just inside the far plane, so it is
/// behind everything that is drawn.
pub(crate) fn follow_camera(
    reach: Res<ViewReach>,
    camera: Query<&Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut dome: Query<&mut Transform, With<SkyDome>>,
) {
    if let (Ok(camera), Ok(mut dome)) = (camera.single(), dome.single_mut()) {
        dome.translation = camera.translation;
        dome.scale = Vec3::splat(reach.0 * DOME_IN_VIEW / DOME_RADIUS);
    }
}

