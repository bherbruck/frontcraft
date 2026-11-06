//! The level as a few large meshes rather than thousands of entities. Everything static is merged by
//! chunk, so one chunk is one draw call that can be culled as a whole, and all colour is baked into
//! vertices so that one material does for everything. Nothing here runs after startup; `lod` decides
//! which of a chunk's meshes show from where.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use crate::bake::{RoomLight, foot_shade, room_cuts, wall_rise_cuts};
use prs_atmosphere::{Atmosphere, Bake, ICOSAHEDRON_CORNERS, ICOSAHEDRON_FACES};
use prs_level::{Ground, Paint, Room, Solid, Terrain, Tree, TreeKind};
#[cfg(test)]
use prs_level::Level;
use prs_runtime::SimResource;
use std::collections::BTreeMap;

/// Side of a chunk of merged geometry, in metres.
pub(crate) const CHUNK: f32 = 128.0;
/// Inside walls are plastered, whatever the outside looks like.
const ROOM_PLASTER: Vec3 = Vec3::new(0.8, 0.74, 0.63);
/// A wall has a foot to darken if it starts this near its floor, stands this tall, and is this wide.
const FOOT_NEAR_FLOOR: f32 = 0.3;
const FOOT_MIN_HEIGHT: f32 = 1.0;
const FOOT_MIN_WIDTH: f32 = 0.7;
/// How near a room a box must be to count as part of its building.
const BUILDING_REACH: f32 = 11.0;
/// How deep water stands over a canal's bed, how far over the wet ground beside it a corner may stand and still be
/// bed, and its colour.
const WATER_DEPTH: f32 = 0.5;
const WATER_FLAT: f32 = 0.35;
const WATER_COLOR: Vec3 = Vec3::new(0.24, 0.42, 0.44);
const WATER_ALPHA: f32 = 0.78;
/// How many fronds a palm's crown has.
const PALM_FRONDS: usize = 5;
const CANOPY_SIDES: usize = 7;
const FIR_CLEARANCE: f32 = 2.4;
const LEAF_UNDER: Vec3 = Vec3::new(0.62, 0.72, 0.8);
const LEAF_TOP: Vec3 = Vec3::new(1.14, 1.08, 0.8);

/// Triangles with their own flat normals and vertex colours.
#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) normals: Vec<[f32; 3]>,
    pub(crate) colors: Vec<[f32; 4]>,
    pub(crate) indices: Vec<u32>,
}

impl Batch {
    pub(crate) fn triangle(&mut self, corners: [Vec3; 3], colors: [Vec3; 3]) {
        let [a, b, c] = corners;
        self.facing(corners, colors, (b - a).cross(c - a).normalize_or(Vec3::Y));
    }

    /// A triangle lit as if it faced `normal`, whichever way it really slopes.
    fn facing(&mut self, corners: [Vec3; 3], colors: [Vec3; 3], normal: Vec3) {
        for (p, color) in corners.iter().zip(colors) {
            self.indices.push(self.positions.len() as u32);
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear(color));
        }
    }

    pub(crate) fn quad(&mut self, corners: [Vec3; 4], colors: [Vec3; 4], normal: Vec3) {
        let base = self.positions.len() as u32;
        for (p, color) in corners.iter().zip(colors) {
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear(color));
        }
        self.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A quad cut into tiles along its first edge and its last, with a colour worked out at each corner.
    fn tiles(&mut self, corners: [Vec3; 4], along: &[f32], up: &[f32], normal: Vec3, twist_limit: f32, color: impl Fn(Vec3) -> Vec3) {
        let [a, b, c, d] = corners;
        let base = self.positions.len() as u32;
        for &v in up {
            let (from, to) = (a.lerp(d, v), b.lerp(c, v));
            for &u in along {
                let p = from.lerp(to, u);
                self.positions.push(p.to_array());
                self.normals.push(normal.to_array());
                self.colors.push(linear(color(p)));
            }
        }
        let row = along.len() as u32;
        let shade = |i: u32| Vec3::from_slice(&self.colors[i as usize]).dot(Vec3::new(0.2126, 0.7152, 0.0722));
        let mut centres = Vec::new();
        for j in 0..up.len() as u32 - 1 {
            for i in 0..row - 1 {
                let p = base + j * row + i;
                let (p00, p10, p11, p01) = (p, p + 1, p + row + 1, p + row);
                // Two triangles show a tile's light as if it changed along one diagonal. Where the
                // corners disagree, a middle vertex lit for itself takes the place of the guess.
                let twist = (shade(p00) + shade(p11) - shade(p10) - shade(p01)).abs();
                if twist > twist_limit {
                    centres.push([p00, p10, p11, p01]);
                } else {
                    self.indices.extend([p00, p10, p11, p00, p11, p01]);
                }
            }
        }
        for [p00, p10, p11, p01] in centres {
            let middle = [p00, p10, p11, p01].iter().map(|&i| Vec3::from_array(self.positions[i as usize])).sum::<Vec3>() * 0.25;
            let m = self.positions.len() as u32;
            self.positions.push(middle.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear(color(middle)));
            self.indices.extend([p00, p10, m, p10, p11, m, p11, p01, m, p01, p00, m]);
        }
    }

    /// Multiplies each vertex's colour by something depending on which way it faces.
    pub(crate) fn tint_by_face(&mut self, tint: impl Fn(Vec3) -> Vec3) {
        for (color, normal) in self.colors.iter_mut().zip(&self.normals) {
            let tinted = Vec3::new(color[0], color[1], color[2]) * tint(Vec3::from_array(*normal));
            *color = [tinted.x, tinted.y, tinted.z, color[3]];
        }
    }

    pub(crate) fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn linear(srgb: Vec3) -> [f32; 4] {
    LinearRgba::from(Color::srgb(srgb.x, srgb.y, srgb.z)).to_f32_array()
}

pub(crate) type Chunks = BTreeMap<(i32, i32), Batch>;

pub(crate) fn chunk_of(chunks: &mut Chunks, at: Vec3) -> &mut Batch {
    chunks.entry(((at.x / CHUNK).floor() as i32, (at.z / CHUNK).floor() as i32)).or_default()
}

/// Everything static, kept apart by what it does to a bullet, at full detail: what tests check against
/// the colliders.
#[cfg(test)]
pub(crate) struct StaticMeshes {
    pub terrain: Chunks,
    pub solids: Chunks,
    /// Tree canopies are one thing you can see and pass through.
    pub canopies: Chunks,
    /// Water is the other: a surface over the bed of a canal or river, waded through.
    pub water: Chunks,
}

#[cfg(test)]
pub(crate) fn static_meshes(level: &Level, bake: Bake) -> StaticMeshes {
    let mut solids = Chunks::new();
    let mut canopies = Chunks::new();
    let light = RoomLight::new(level, bake);
    for solid in &level.solids {
        add_solid(chunk_of(&mut solids, solid.center), solid, &light);
    }
    for tree in &level.trees {
        add_trunk(chunk_of(&mut solids, tree.base), tree, &light);
        add_canopy(chunk_of(&mut canopies, tree.base), tree, &light);
    }
    StaticMeshes { terrain: terrain_chunks(&level.terrain), solids, canopies, water: water_chunks(&level.terrain) }
}

/// A flat surface a little over each stretch of bed under water. Only the bed, so the surface stops where
/// the bank rises out of it, which reads as the water's edge.
pub(crate) fn water_chunks(terrain: &Terrain) -> Chunks {
    let water = |v: u32| terrain.ground[v as usize] == Ground::Water;
    let wet = |tri: &[u32; 3]| tri.iter().any(|&v| water(v));
    // The bed: every wet corner with nothing beside it much lower. A bank's corners all have.
    let mut lowest_beside = vec![f32::MAX; terrain.vertices.len()];
    for tri in terrain.indices.iter().filter(|tri| wet(tri)) {
        for &v in tri {
            for &w in tri {
                lowest_beside[v as usize] = lowest_beside[v as usize].min(terrain.vertices[w as usize].y);
            }
        }
    }
    let bed = |v: u32| water(v) && terrain.vertices[v as usize].y - lowest_beside[v as usize] <= WATER_FLAT;
    let mut chunks = Chunks::new();
    for tri in terrain.indices.iter().filter(|tri| wet(tri)) {
        // Only where its lowest corner is on the bed: where a bank rises through it the ground hides it.
        let low = *tri.iter().min_by(|a, b| terrain.vertices[**a as usize].y.total_cmp(&terrain.vertices[**b as usize].y)).expect("three corners");
        if !bed(low) {
            continue;
        }
        // Each corner at the bed beside it, so neighbouring triangles meet without a step.
        let surface = tri.map(|v| {
            let c = terrain.vertices[v as usize];
            Vec3::new(c.x, lowest_beside[v as usize].min(c.y) + WATER_DEPTH, c.z)
        });
        let corners = tri.map(|v| terrain.vertices[v as usize]);
        chunk_of(&mut chunks, (corners[0] + corners[1] + corners[2]) / 3.0).facing(surface, [WATER_COLOR; 3], Vec3::Y);
    }
    chunks
}

pub(crate) fn spawn_level(
    mut commands: Commands,
    sim: Res<SimResource>,
    atmosphere: Res<Atmosphere>,
    detail: Res<crate::lod::Detail>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let crate::lod::Detailed { mut terrain, mut solids, water, blocks } = crate::lod::detailed_meshes(&sim.0.level, atmosphere.bake, &detail);
    commands.insert_resource(blocks);
    let wet = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, WATER_ALPHA),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.15,
        reflectance: 0.5,
        ..default()
    });
    let ground = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        reflectance: 0.15,
        ..default()
    });
    let painted = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        reflectance: 0.2,
        ..default()
    });
    if atmosphere.bake.face_tint > 0.0 {
        for (batch, _) in terrain.iter_mut().chain(solids.iter_mut()) {
            batch.tint_by_face(|normal| atmosphere.face_tint(normal));
        }
    }
    crate::lod::spawn(&mut commands, &mut meshes, terrain, &ground);
    crate::lod::spawn(&mut commands, &mut meshes, solids, &painted);
    for (batch, piece) in water {
        let triangles = crate::Triangles(batch.indices.len() as u32 / 3);
        commands.spawn((Mesh3d(meshes.add(batch.mesh())), MeshMaterial3d(wet.clone()), bevy::light::NotShadowCaster, triangles, piece, Visibility::Hidden));
    }
}

/// Colour, and how much it varies from one box to the next.
pub(crate) fn palette(paint: Paint) -> (Vec3, f32) {
    let (rgb, vary) = match paint {
        Paint::Concrete => ([0.6, 0.59, 0.54], 0.0),
        Paint::Wood => ([0.5, 0.34, 0.18], 0.0),
        Paint::Sandbag => ([0.66, 0.57, 0.37], 0.0),
        Paint::Rock => ([0.48, 0.46, 0.43], 0.0),
        Paint::Metal => ([0.27, 0.37, 0.3], 0.0),
        Paint::Trunk => ([0.3, 0.2, 0.13], 0.0),
        Paint::Limewash => ([0.78, 0.72, 0.61], 0.03),
        Paint::Mudbrick => ([0.67, 0.46, 0.3], 0.035),
        Paint::Ochre => ([0.76, 0.58, 0.37], 0.03),
        Paint::Cement => ([0.63, 0.63, 0.61], 0.025),
        Paint::Brick => ([0.55, 0.31, 0.24], 0.03),
        Paint::Stone => ([0.59, 0.55, 0.47], 0.05),
        Paint::Plaster => ([0.82, 0.77, 0.66], 0.015),
        Paint::FloorTile => ([0.56, 0.41, 0.3], 0.02),
        Paint::Packed => ([0.63, 0.53, 0.39], 0.02),
        Paint::RoofTile => ([0.62, 0.31, 0.2], 0.025),
        Paint::Tin => ([0.56, 0.6, 0.62], 0.03),
        Paint::Rust => ([0.56, 0.3, 0.18], 0.04),
        Paint::Canvas => ([0.53, 0.49, 0.28], 0.015),
        Paint::Hay => ([0.85, 0.7, 0.32], 0.03),
        Paint::Hedge => ([0.2, 0.37, 0.14], 0.14),
        Paint::Planks => ([0.5, 0.37, 0.23], 0.025),
        Paint::Terracotta => ([0.8, 0.45, 0.3], 0.02),
    };
    (Vec3::from_array(rgb), vary)
}

/// A repeatable number from 0 to 1 for something at this place.
pub(crate) fn grain(at: Vec3) -> f32 {
    (at.dot(Vec3::new(12.9898, 4.1414, 78.233)).sin() * 43758.547).fract().abs()
}

/// The nearest building's ground floor, if this box is part of one: its colour varies from house to
/// house rather than from one wall panel to the next.
fn home_room<'a>(solid: &Solid, rooms: &'a [Room]) -> Option<&'a Room> {
    let flat = |p: Vec3| Vec2::new(p.x, p.z);
    rooms
        .iter()
        .map(|room| (room, flat(room.center).distance_squared(flat(solid.center))))
        .filter(|(_, d)| *d < BUILDING_REACH * BUILDING_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.center.y.total_cmp(&b.0.center.y)))
        .map(|(room, _)| room)
}

pub(crate) fn add_solid(batch: &mut Batch, solid: &Solid, light: &RoomLight) {
    let (color, vary) = palette(solid.paint);
    let home = home_room(solid, light.rooms());
    let tone = home.map_or(solid.center, |room| Vec3::new(room.center.x, 0.0, room.center.z));
    let color = color * (1.0 - vary + grain(tone) * vary * 2.0);
    let floor = home.map_or(solid.center.y - solid.half.y, |room| room.center.y - room.half.y);
    let corner = |x: f32, y: f32, z: f32| solid.center + solid.rotation * (solid.half * Vec3::new(x, y, z));
    // Each face, wound to face outward, with its normal in the box's own frame.
    let faces: [([Vec3; 4], Vec3); 6] = [
        ([corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.), corner(1., -1., 1.)], Vec3::X),
        ([corner(-1., -1., 1.), corner(-1., 1., 1.), corner(-1., 1., -1.), corner(-1., -1., -1.)], Vec3::NEG_X),
        ([corner(-1., 1., -1.), corner(-1., 1., 1.), corner(1., 1., 1.), corner(1., 1., -1.)], Vec3::Y),
        ([corner(-1., -1., 1.), corner(-1., -1., -1.), corner(1., -1., -1.), corner(1., -1., 1.)], Vec3::NEG_Y),
        ([corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], Vec3::Z),
        ([corner(1., -1., -1.), corner(-1., -1., -1.), corner(-1., 1., -1.), corner(1., 1., -1.)], Vec3::NEG_Z),
    ];
    for (corners, local_normal) in faces {
        let normal = solid.rotation * local_normal;
        let middle = corners.iter().sum::<Vec3>() * 0.25;
        if let Some(room) = light.room_facing(middle, normal) {
            let color = if normal.y.abs() < 0.5 { color.lerp(ROOM_PLASTER, 0.65) } else { color };
            let bake = light.bake();
            let cuts = |edge: Vec3| {
                if edge.y.abs() > edge.length() * 0.7 {
                    wall_rise_cuts(edge, bake.corner_reach)
                } else {
                    room_cuts(edge.length(), bake.corner_reach, bake.room_cell)
                }
            };
            let (along, up) = (cuts(corners[1] - corners[0]), cuts(corners[3] - corners[0]));
            batch.tiles(corners, &along, &up, normal, bake.tile_twist, |p| color * light.at(room, p, normal));
        } else if let Some(cuts) = foot_cuts(&corners, normal, floor, light.bake()) {
            let bake = light.bake();
            match cuts {
                FootCuts::Along(up) => batch.tiles(corners, &up, &[0.0, 1.0], normal, bake.tile_twist, |p| color * foot_shade(bake, p.y - floor)),
                FootCuts::Up(up) => batch.tiles(corners, &[0.0, 1.0], &up, normal, bake.tile_twist, |p| color * foot_shade(bake, p.y - floor)),
            }
        } else {
            batch.quad(corners, [color; 4], normal);
        }
    }
}

enum FootCuts {
    /// Up the wall is along the quad's first edge.
    Along([f32; 3]),
    /// Up the wall is along the quad's last edge.
    Up([f32; 3]),
}

/// If this face is a wall standing on the floor, where to cut it to give it a foot.
fn foot_cuts(corners: &[Vec3; 4], normal: Vec3, floor: f32, bake: &Bake) -> Option<FootCuts> {
    if normal.y.abs() > 0.1 {
        return None;
    }
    let (first, last) = (corners[1] - corners[0], corners[3] - corners[0]);
    let (height, width) = if first.y.abs() > last.y.abs() { (first.y.abs(), last.length()) } else { (last.y.abs(), first.length()) };
    let bottom = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    if height < FOOT_MIN_HEIGHT || width < FOOT_MIN_WIDTH || (bottom - floor).abs() > FOOT_NEAR_FLOOR {
        return None;
    }
    let cut = ((floor + bake.foot_height - bottom) / height).clamp(0.05, 0.95);
    // Which way up the first corner is decides which end of that edge the foot is at.
    Some(if first.y.abs() > last.y.abs() {
        FootCuts::Along([0.0, if first.y > 0.0 { cut } else { 1.0 - cut }, 1.0])
    } else {
        FootCuts::Up([0.0, if last.y > 0.0 { cut } else { 1.0 - cut }, 1.0])
    })
}

/// The trunk's collider is a square post, so that is what gets drawn.
pub(crate) fn add_trunk(batch: &mut Batch, tree: &Tree, light: &RoomLight) {
    let half = Vec3::new(prs_level::TRUNK_HALF_WIDTH, tree.height * prs_level::TRUNK_HALF_HEIGHT, prs_level::TRUNK_HALF_WIDTH);
    let solid = Solid {
        center: tree.base + Vec3::Y * half.y,
        half,
        rotation: Quat::IDENTITY,
        paint: Paint::Trunk,
    };
    add_solid(batch, &solid, &RoomLight::outdoors(light));
}

pub(crate) fn add_canopy(batch: &mut Batch, tree: &Tree, light: &RoomLight) {
    let vary = 0.88 + grain(tree.base) * 0.24;
    let tilt = Quat::from_rotation_y(grain(tree.base + Vec3::X) * std::f32::consts::TAU);
    let (h, r) = (tree.height, tree.radius);
    match tree.kind {
        TreeKind::Fir => {
            let color = Vec3::new(0.13, 0.3, 0.17) * vary;
            // Clear of a standing soldier's eyes, however short the tree.
            let lift = (FIR_CLEARANCE - h * 0.21).max(0.0);
            for (layer, (at, width)) in [(0.3, 1.0), (0.55, 0.78), (0.8, 0.52)].into_iter().enumerate() {
                let layer_height = h * 0.45;
                let center = tree.base + Vec3::Y * (h * at + layer_height * 0.3 + lift);
                let turn = tilt * Quat::from_rotation_y(layer as f32 * 0.7);
                add_cone(batch, center, Vec3::new(r * width, layer_height, r * width), turn, color);
            }
        }
        TreeKind::Broadleaf => {
            let color = Vec3::new(0.24, 0.42, 0.14) * vary;
            let center = tree.base + Vec3::Y * (h * 0.68);
            add_blob(batch, center, Vec3::new(r, h * 0.33, r * 0.92), tilt, color);
            let side = tilt * Vec3::X * r * 0.55 + Vec3::Y * h * 0.14;
            add_blob(batch, center + side, Vec3::new(r * 0.62, h * 0.2, r * 0.66), tilt.inverse(), color * 1.08);
        }
        TreeKind::Poplar => {
            let color = Vec3::new(0.27, 0.43, 0.15) * vary;
            add_blob(batch, tree.base + Vec3::Y * (h * 0.62), Vec3::new(r, h * 0.42, r), tilt, color);
        }
        TreeKind::Fruit => {
            let color = Vec3::new(0.33, 0.45, 0.17) * vary;
            add_blob(batch, tree.base + Vec3::Y * (h * 0.66), Vec3::new(r, h * 0.36, r), tilt, color);
        }
        TreeKind::Palm => {
            // The trunk above the part that stops bullets, then fronds drooping from the crown.
            let low = h * prs_level::TRUNK_HALF_HEIGHT * 2.0;
            let crown = h * 0.9;
            let stem = Solid {
                center: tree.base + Vec3::Y * ((low + crown) * 0.5),
                half: Vec3::new(prs_level::TRUNK_HALF_WIDTH * 0.8, (crown - low) * 0.5, prs_level::TRUNK_HALF_WIDTH * 0.8),
                rotation: Quat::IDENTITY,
                paint: Paint::Trunk,
            };
            add_solid(batch, &stem, &RoomLight::outdoors(light));
            let color = Vec3::new(0.3, 0.42, 0.16) * vary;
            let top = tree.base + Vec3::Y * crown;
            for k in 0..PALM_FRONDS {
                let turn = tilt * Quat::from_rotation_y(k as f32 / PALM_FRONDS as f32 * std::f32::consts::TAU) * Quat::from_rotation_z(-0.35);
                add_blob(batch, top + turn * Vec3::new(r * 0.5, 0.0, 0.0), Vec3::new(r * 0.55, h * 0.025, r * 0.16), turn, color);
            }
        }
    }
}

/// One flat colour for a facet of leaves: cool and dark underneath, warm where the top catches the sky,
/// and each a little different so that a canopy reads as facets at any distance.
pub(crate) fn leaf(color: Vec3, up: f32, place: Vec3) -> Vec3 {
    color * LEAF_UNDER.lerp(LEAF_TOP, up) * (0.9 + grain(place) * 0.2)
}

fn add_cone(batch: &mut Batch, center: Vec3, scale: Vec3, turn: Quat, color: Vec3) {
    let point = |x: f32, y: f32, z: f32| center + turn * (scale * Vec3::new(x, y, z));
    let rim = |i: usize| {
        let a = i as f32 / CANOPY_SIDES as f32 * std::f32::consts::TAU;
        point(a.cos(), -0.5, a.sin())
    };
    let tip = point(0.0, 0.5, 0.0);
    let middle = point(0.0, -0.5, 0.0);
    for i in 0..CANOPY_SIDES {
        let (a, b) = (rim(i), rim(i + 1));
        batch.triangle([tip, b, a], [leaf(color, 0.72, a + b); 3]);
        batch.triangle([middle, a, b], [leaf(color, 0.3, a + b); 3]);
    }
}

/// An icosahedron: as round as a tree top needs to be from any distance you see it at.
fn add_blob(batch: &mut Batch, center: Vec3, scale: Vec3, turn: Quat, color: Vec3) {
    const T: f32 = 1.618034;
    const CORNERS: [[f32; 3]; 12] = ICOSAHEDRON_CORNERS;
    const FACES: [[usize; 3]; 20] = ICOSAHEDRON_FACES;
    let unit = (1.0 + T * T).sqrt();
    let point = |i: usize| center + turn * (scale * Vec3::from_array(CORNERS[i]) / unit);
    for [a, b, c] in FACES {
        let up = (CORNERS[a][1] + CORNERS[b][1] + CORNERS[c][1]) / (3.0 * T);
        let corners = [point(a), point(b), point(c)];
        batch.triangle(corners, [leaf(color, up * 0.5 + 0.5, corners[0] + corners[1]); 3]);
    }
}

/// Faceted terrain: every triangle gets its own flat normal, and each corner a colour by what the
/// ground is there.
#[cfg(test)]
pub(crate) fn terrain_chunks(terrain: &Terrain) -> Chunks {
    let mut chunks = Chunks::new();
    for (i, tri) in terrain.indices.iter().enumerate() {
        let corners = tri.map(|v| terrain.vertices[v as usize]);
        let [a, b, c] = corners;
        chunk_of(&mut chunks, (a + b + c) / 3.0).triangle(corners, triangle_colors(terrain, i));
    }
    chunks
}

pub(crate) fn triangle_colors(terrain: &Terrain, triangle: usize) -> [Vec3; 3] {
    let tri = terrain.indices[triangle];
    let height = tri.iter().map(|v| terrain.vertices[*v as usize].y).sum::<f32>() / 3.0;
    let steep = terrain.bareness(triangle);
    let grain = ((triangle as f32 * 12.9898).sin() * 43758.547).fract().abs() * 0.05;
    tri.map(|v| ground_color(terrain.ground[v as usize], terrain.vertices[v as usize], height, steep, grain))
}

fn ground_color(ground: Ground, at: Vec3, height: f32, steep: f32, grain: f32) -> Vec3 {
    let tint = Vec3::new(grain, grain, grain * 0.5);
    let dirt = Vec3::new(0.46, 0.35, 0.22) + tint;
    // Slow drifts of colour, so a meadow is not one flat green.
    let drift = ((at.x * 0.031 + 0.5).sin() * (at.z * 0.027 - 1.3).sin() * 0.5 + 0.5) * 0.5
        + ((at.x * 0.11).sin() * (at.z * 0.13).cos() * 0.5 + 0.5) * 0.5;
    let base = match ground {
        Ground::Natural => {
            let grass = Vec3::new(0.33, 0.45, 0.18) + tint;
            let dry = Vec3::new(0.56, 0.5, 0.22) + tint;
            grass.lerp(dry, ((height + 1.0) / 7.0).clamp(0.0, 1.0))
        }
        Ground::Meadow => Vec3::new(0.34, 0.44, 0.18).lerp(Vec3::new(0.46, 0.47, 0.22), drift) + tint,
        Ground::Pasture => Vec3::new(0.39, 0.48, 0.19).lerp(Vec3::new(0.44, 0.5, 0.22), drift) + tint,
        Ground::Wheat => Vec3::new(0.72, 0.58, 0.2).lerp(Vec3::new(0.8, 0.64, 0.24), drift) + tint * 1.6,
        Ground::Stubble => Vec3::new(0.62, 0.52, 0.26).lerp(Vec3::new(0.55, 0.48, 0.24), drift) + tint * 1.6,
        Ground::Plough => Vec3::new(0.38, 0.25, 0.15).lerp(Vec3::new(0.33, 0.22, 0.13), drift) + tint * 1.6,
        Ground::Orchard => Vec3::new(0.38, 0.45, 0.19).lerp(Vec3::new(0.46, 0.46, 0.22), drift) + tint,
        Ground::Road => Vec3::new(0.47, 0.43, 0.37) + tint * 0.6,
        Ground::Track => Vec3::new(0.56, 0.44, 0.27) + tint,
        Ground::Dirt => Vec3::new(0.54, 0.46, 0.27).lerp(Vec3::new(0.46, 0.44, 0.23), drift) + tint,
        Ground::Yard => Vec3::new(0.58, 0.48, 0.33) + tint,
        Ground::Paving => Vec3::new(0.62, 0.59, 0.52) + tint * 0.5,
        Ground::DitchBed => Vec3::new(0.38, 0.31, 0.19) + tint,
        Ground::Shingle => Vec3::new(0.62, 0.58, 0.5).lerp(Vec3::new(0.56, 0.52, 0.44), drift) + tint * 0.6,
        Ground::Scrub => Vec3::new(0.56, 0.47, 0.29).lerp(Vec3::new(0.5, 0.45, 0.27), drift) + tint,
        Ground::Water => Vec3::new(0.25, 0.35, 0.33).lerp(Vec3::new(0.22, 0.32, 0.31), drift) + tint * 0.3,
        Ground::Sand => Vec3::new(0.74, 0.64, 0.46).lerp(Vec3::new(0.68, 0.58, 0.4), drift) + tint * 0.8,
        Ground::Rock => Vec3::new(0.42, 0.36, 0.3).lerp(Vec3::new(0.36, 0.32, 0.28), drift) + tint * 0.8,
    };
    base.lerp(dirt, steep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_runtime::prs_sim::Surface;

    fn triangles(batch: &Batch) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        batch.indices.chunks(3).map(|t| [0, 1, 2].map(|i| Vec3::from_array(batch.positions[t[i] as usize])))
    }

    /// Whatever you can see stops you and bullets. Tree canopies are the stated exception.
    fn every_drawn_surface_has_a_collider(level: &Level) {
        let world = level.collision();
        let meshes = static_meshes(level, Bake::default());
        let mut checked = 0;
        for batch in meshes.terrain.values().chain(meshes.solids.values()) {
            for [a, b, c] in triangles(batch) {
                let middle = (a + b + c) / 3.0;
                let normal = (b - a).cross(c - a).normalize();
                checked += 1;
                let hit = world.raycast(middle + normal * 0.02, -normal, 0.06);
                assert!(hit.is_some(), "nothing solid under the surface drawn at {middle}, facing {normal}");
            }
        }
        assert!(checked > 1000, "{checked}");
    }

    /// Walls are boxes and trees are cones, and all of it has to face out, or culling opens holes in it.
    #[test]
    fn what_stands_on_the_map_faces_outwards() {
        for level in [Level::training_range(), Level::named("town").unwrap()] {
            let meshes = static_meshes(&level, Bake::default());
            let batches = meshes.solids.values().step_by(7).take(6).chain(meshes.canopies.values().step_by(7).take(3));
            for batch in batches {
                let positions = batch.positions.clone();
                let (normals, indices) = (batch.normals.clone(), batch.indices.clone());
                let mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
                    .with_inserted_indices(bevy::mesh::Indices::U32(indices));
                let report = prs_models::mesh_check::look_all_round(&mesh, 24, 7);
                assert_eq!(report.normals_disagree, 0, "{report:?}");
                assert!(report.share_inside_out() < 0.02, "{report:?}");
            }
        }
    }

    #[test]
    fn range_is_drawn_as_it_collides() {
        every_drawn_surface_has_a_collider(&Level::training_range());
    }

    #[test]
    fn town_is_drawn_as_it_collides() {
        every_drawn_surface_has_a_collider(&Level::named("town").unwrap());
    }

    #[test]
    fn claudabad_is_drawn_as_it_collides() {
        every_drawn_surface_has_a_collider(&Level::named("claudabad").unwrap());
    }

    /// Every collider is drawn: boxes and trunks over their whole surface, however a face has been
    /// cut into tiles, and the land triangle for triangle.
    #[test]
    fn nothing_solid_goes_undrawn() {
        let level = Level::named("town").unwrap();
        let meshes = static_meshes(&level, Bake::default());
        let surface = |h: Vec3| 8.0 * (h.x * h.y + h.y * h.z + h.z * h.x);
        let boxes: f32 = level.solids.iter().map(|s| surface(s.half)).sum();
        let trunks: f32 = level
            .trees
            .iter()
            .map(|t| surface(Vec3::new(prs_level::TRUNK_HALF_WIDTH, t.height * prs_level::TRUNK_HALF_HEIGHT, prs_level::TRUNK_HALF_WIDTH)))
            .sum();
        let drawn: f32 = meshes.solids.values().flat_map(triangles).map(|[a, b, c]| (b - a).cross(c - a).length() * 0.5).sum();
        let want = boxes + trunks;
        assert!((drawn - want).abs() < want * 0.001, "drawn {drawn} of {want} square metres");
        assert!(meshes.solids.values().map(|b| b.indices.len() / 3).sum::<usize>() >= (level.solids.len() + level.trees.len()) * 12);
        let count = |chunks: &Chunks| chunks.values().map(|b| b.indices.len() / 3).sum::<usize>();
        assert_eq!(count(&meshes.terrain), level.terrain.indices.len());
    }

    /// A sample of ground points: the colour drawn there, matched against every kind of ground at
    /// that spot, must name the surface a bullet coming straight down reports. Where a triangle
    /// blends grounds the colour is a mix, so it is only matched against the grounds being blended.
    fn ground_sounds_as_it_looks(level: &Level, expect: &[Surface]) {
        const KINDS: [Ground; 18] = [
            Ground::Natural, Ground::Meadow, Ground::Pasture, Ground::Wheat, Ground::Stubble, Ground::Plough, Ground::Orchard, Ground::Road,
            Ground::Track, Ground::Dirt, Ground::Yard, Ground::Paving, Ground::DitchBed, Ground::Scrub, Ground::Shingle, Ground::Water,
            Ground::Sand, Ground::Rock,
        ];
        let terrain = &level.terrain;
        let world = level.collision();
        let (mut whole, mut blended, mut blended_right) = (0, 0, 0);
        let mut seen = Vec::new();
        for i in (0..terrain.indices.len()).step_by(3) {
            let tri = terrain.indices[i];
            let [a, b, c] = tri.map(|v| terrain.vertices[v as usize]);
            let at = (a + b + c) / 3.0;
            let lift = 0.4;
            let Some(hit) = world.raycast(at + Vec3::Y * lift, Vec3::NEG_Y, 1.0) else { continue };
            // Something stands here, so the ground is not what you would hit.
            if (hit.distance - lift).abs() > 0.003 {
                continue;
            }
            if !seen.contains(&hit.surface) {
                seen.push(hit.surface);
            }
            let drawn = triangle_colors(terrain, i).iter().sum::<Vec3>() / 3.0;
            let height = (a.y + b.y + c.y) / 3.0;
            let steep = terrain.bareness(i);
            let grain = ((i as f32 * 12.9898).sin() * 43758.547).fract().abs() * 0.05;
            let nearest_of = |kinds: &[Ground]| {
                let d = |g: Ground| ground_color(g, at, height, steep, grain).distance_squared(drawn);
                kinds.iter().copied().min_by(|x, y| d(*x).total_cmp(&d(*y))).unwrap()
            };
            let grounds = tri.map(|v| terrain.ground[v as usize]);
            let one_kind = grounds[0] == grounds[1] && grounds[1] == grounds[2];
            let nearest = if one_kind { nearest_of(&KINDS) } else { nearest_of(&grounds) };
            // On the steep the colour is mostly dirt whatever the ground, and so is the sound.
            let looks = if steep > 0.5 { Surface::Dirt } else { nearest.surface() };
            if one_kind {
                whole += 1;
                assert_eq!(looks, hit.surface, "drawn as {nearest:?} at {at}, but sounds like {:?}", hit.surface);
            } else {
                blended += 1;
                blended_right += (looks == hit.surface) as u32;
            }
        }
        assert!(whole > 100, "{whole}");
        assert!(blended_right * 100 >= blended * 97, "only {blended_right} of {blended} blended points match");
        for surface in expect {
            assert!(seen.contains(surface), "no {surface:?} ground, only {seen:?}");
        }
    }

    #[test]
    fn range_ground_sounds_as_it_looks() {
        ground_sounds_as_it_looks(&Level::training_range(), &[Surface::Grass, Surface::Dirt]);
    }

    #[test]
    fn town_ground_sounds_as_it_looks() {
        ground_sounds_as_it_looks(&Level::named("town").unwrap(), &[Surface::Grass, Surface::Dirt, Surface::Gravel, Surface::Concrete]);
    }

    #[test]
    fn claudabad_ground_sounds_as_it_looks() {
        ground_sounds_as_it_looks(&Level::named("claudabad").unwrap(), &[Surface::Grass, Surface::Dirt, Surface::Gravel, Surface::Concrete]);
    }

    #[test]
    #[ignore]
    fn draw_report() {
        for (name, level) in [("range", Level::training_range()), ("town", Level::named("town").unwrap()), ("claudabad", Level::named("claudabad").unwrap()), ("muttrah", Level::named("muttrah").unwrap()), ("fallujah", Level::named("fallujah").unwrap())] {
            let start = std::time::Instant::now();
            let meshes = static_meshes(&level, Bake::default());
            let elapsed = start.elapsed();
            let report = |what: &str, chunks: &Chunks| {
                let tris: usize = chunks.values().map(|b| b.indices.len() / 3).sum();
                let verts: usize = chunks.values().map(|b| b.positions.len()).sum();
                println!("{name} {what}: {} chunks, {tris} triangles, {verts} vertices", chunks.len());
            };
            report("terrain", &meshes.terrain);
            report("solids", &meshes.solids);
            report("canopies", &meshes.canopies);
            println!("{name} built in {elapsed:?}");
        }
    }
}
