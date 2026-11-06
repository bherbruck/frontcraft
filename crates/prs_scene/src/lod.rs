//! Less detail with distance, so a 4 km map costs about what a 1 km one did. Only the drawing changes:
//! what stops a bullet or a player is the same everywhere.
//!
//! Each 128 m chunk of the level is drawn several ways: the full terrain near you and a coarse grid
//! beyond; every box with its baked light near you and plain boxes for the bigger ones beyond; whole
//! trees near you and one cheap shape each beyond. Farther still, a whole 512 m block's ground is one
//! coarse mesh, which keeps the number of draw calls down on WebGL2. Past the view distance nothing is
//! drawn. Each thing is kept at as few levels as it needs, since in a browser what is stored on the GPU
//! runs out before what is drawn does: ground too rough for any coarse grid keeps only its full mesh.
//!
//! One system picks what shows, by distances measured from fixed middles: a chunk's choice from its
//! own middle, and whether a block stands in for its chunks from the block's middle. So the pieces of
//! one chunk always agree, and a block and its chunks are never drawn together or missing together.
//!
//! Full detail for boxes and trees is only built for chunks near the camera, a few at a time within
//! a budget of milliseconds per frame, and dropped again once the camera has gone well past, so what a
//! map stores stays about the same however many buildings it has. Until a chunk's full detail is ready
//! its plain boxes and far trees stand in. Looking through a scope counts as being nearer, along the
//! direction you look.
//!
//! Wherever a soldier or a vehicle stands, the ground under it is the full mesh at any distance, and its
//! block never stands in for its chunks. A coarse grid can sit up to a metre off the true ground, which
//! through a scope would show someone floating or sunk to the knees.

use bevy::prelude::*;
use prs_level::{Level, Paint, Solid, Terrain, Tree, TreeKind};
use bevy::platform::time::Instant;
use prs_view::{ViewDistance, ViewSettings, WorldCamera};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::bake::RoomLight;
use crate::statics::{self, Batch, CHUNK, Chunks};

/// How the level's detail falls off. Distances are from the middle of a chunk or block, in metres.
#[derive(Resource, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Detail {
    /// Nearer than this, the full terrain mesh. Beyond it, a regular grid per chunk: the coarsest of
    /// `terrain_cells` that keeps within `terrain_error` metres of the full mesh, or the full mesh if
    /// none does. So open fields go coarse and a river's banks keep their shape.
    pub terrain_full_until: f32,
    pub terrain_cells: Vec<f32>,
    pub terrain_error: f32,
    /// Nearer than this, boxes carry the baked light of rooms and wall feet. Beyond it they are plain,
    /// and those smaller than `small_solid` along every side are left out.
    pub solids_full_until: f32,
    pub small_solid: f32,
    /// Nearer than this, whole trees. Beyond it, one simple shape each, until `trees_until`.
    pub trees_full_until: f32,
    pub trees_until: f32,
    /// Beyond this, a whole block's ground is one coarse mesh instead of its chunks'.
    pub far_from: f32,
    /// Side of a block, in metres: a whole number of chunks.
    pub far_block: f32,
    /// The same for a whole block, which is only ever seen from `far_from` or more.
    pub far_terrain_cells: Vec<f32>,
    pub far_terrain_error: f32,
    /// How far a coarse grid's edge reaches down, to hide cracks where it meets a finer neighbour.
    pub skirt: f32,
    /// Full detail is built this much before it is needed, and dropped this much farther out again, so
    /// that a chunk at the edge is not built and dropped over and over.
    pub full_build_ahead: f32,
    pub full_drop_behind: f32,
    /// Milliseconds a frame may spend building full detail. The first fill after loading has no limit.
    pub build_ms_per_frame: f32,
    /// A chunk's boxes or trees with more triangles than this are cut into 64 m squares, and those into
    /// 32 m ones if still over, so a dense souq is culled in smaller pieces. Detail is still chosen for
    /// the whole chunk at once, so its pieces always agree.
    pub split_triangles: u32,
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            terrain_full_until: 450.0,
            terrain_cells: vec![32.0, 16.0, 8.0],
            terrain_error: 1.0,
            solids_full_until: 450.0,
            small_solid: 2.5,
            trees_full_until: 300.0,
            trees_until: 2500.0,
            far_from: 1500.0,
            far_block: CHUNK * 4.0,
            far_terrain_cells: vec![64.0, 32.0, 16.0],
            far_terrain_error: 2.5,
            skirt: 6.0,
            full_build_ahead: 150.0,
            full_drop_behind: 250.0,
            build_ms_per_frame: 2.0,
            split_triangles: 12_000,
        }
    }
}

/// Which of a chunk's or block's pieces this is, and so when it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    TerrainFull,
    TerrainCoarse,
    /// Ground no coarse grid can follow closely enough, kept at full detail at every distance.
    TerrainKept,
    SolidsFull,
    SolidsPlain,
    TreesFull,
    TreesFar,
    /// Water over a canal's or river's bed, at one detail until its block stands in for it.
    Water,
    /// A whole block, standing in for its chunks.
    Block,
}

/// A drawn piece of the level: what it is, and the middle its distance is measured from.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Piece {
    role: Role,
    middle: Vec3,
    /// The chunk it covers, or the corner chunk of its block.
    chunk: Key,
    /// The block this piece is part of, as an index into [`Blocks`].
    block: usize,
}

/// The middle of every block, so a block's choice is worked out once per frame for all its chunks.
#[derive(Resource, Default)]
pub(crate) struct Blocks {
    middles: Vec<Vec3>,
    radius: f32,
}

/// Whether a piece shows, given its own distance and whether its block stands in for it.
///
/// `away` is how far off it really is, which decides whether it is drawn at all. `distance` is how near it
/// looks, which decides its detail: less than `away` when it is under a scope.
fn shows(role: Role, away: f32, distance: f32, block_far: bool, detail: &Detail, view: f32) -> bool {
    let reach = CHUNK * std::f32::consts::FRAC_1_SQRT_2;
    if role == Role::Block {
        return block_far;
    }
    if away > view + reach {
        return false;
    }
    // A block stands in for its chunks' ground only. Plain boxes and far trees exist once, per chunk.
    if block_far && matches!(role, Role::TerrainFull | Role::TerrainCoarse | Role::TerrainKept | Role::Water) {
        return false;
    }
    match role {
        Role::TerrainFull => distance < detail.terrain_full_until,
        Role::TerrainCoarse => distance >= detail.terrain_full_until,
        Role::TerrainKept => true,
        Role::SolidsFull => distance < detail.solids_full_until,
        Role::SolidsPlain => distance >= detail.solids_full_until,
        Role::TreesFull => distance < detail.trees_full_until.min(detail.trees_until),
        Role::TreesFar => distance >= detail.trees_full_until.min(detail.trees_until) && distance < detail.trees_until,
        Role::Water => true,
        Role::Block => unreachable!(),
    }
}

/// Whether a block is drawn whole instead of as its chunks: far enough off, and not past the view.
fn block_is_far(distance: f32, radius: f32, detail: &Detail, view: f32) -> bool {
    distance >= detail.far_from && distance <= view + radius
}

/// Where the camera is and how it looks, for how near each piece looks from it.
pub(crate) struct Eye {
    at: Vec3,
    forward: Vec3,
    /// Pieces within this cone of the view direction count as nearer by `zoom`.
    cone_cos: f32,
    zoom: f32,
}

impl Eye {
    fn new(camera: &GlobalTransform, projection: &Projection, settings: &ViewSettings) -> Self {
        let (fov, base) = match projection {
            Projection::Perspective(p) => (p.fov, settings.fov().to_radians()),
            _ => (1.0, 1.0),
        };
        let zoom = ((fov * 0.5).tan() / (base * 0.5).tan().max(1e-3)).clamp(0.01, 1.0);
        // Generous, so the edge of a scope's picture and a quick turn are covered too.
        Self { at: camera.translation(), forward: camera.forward().as_vec3(), cone_cos: fov.min(1.5).cos(), zoom }
    }

    fn away(&self, p: Vec3) -> f32 {
        p.distance(self.at)
    }

    /// How near something looks: its distance, divided by the scope's magnification if it is in view.
    fn near(&self, p: Vec3) -> f32 {
        let d = self.away(p);
        if self.zoom < 1.0 && d > f32::EPSILON && (p - self.at).dot(self.forward) / d >= self.cone_cos {
            d * self.zoom
        } else {
            d
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn choose_detail(
    detail: Res<Detail>,
    distance: Res<ViewDistance>,
    blocks: Res<Blocks>,
    settings: Res<ViewSettings>,
    sim: Res<prs_runtime::SimResource>,
    camera: Query<(&GlobalTransform, &Projection), With<WorldCamera>>,
    mut pieces: Query<(&Piece, &mut Visibility)>,
    mut far: Local<Vec<bool>>,
    mut occupied: Local<HashSet<Key>>,
    resident: Res<Resident>,
) {
    let Ok((camera, projection)) = camera.single() else {
        return;
    };
    let eye = Eye::new(camera, projection, &settings);
    let view = distance.metres;
    let sim = &sim.0;
    occupied.clear();
    let standing = sim.soldiers.iter().map(|s| s.body.pos).chain(sim.vehicles.iter().map(|v| v.pos)).chain(sim.helicopters.iter().map(|h| h.state.pos));
    occupied.extend(standing.map(chunk_key));
    far.clear();
    far.extend(blocks.middles.iter().map(|m| block_is_far(eye.near(*m), blocks.radius, &detail, view) && eye.away(*m) <= view + blocks.radius));
    // A block with anyone in it is drawn as its chunks, so their ground can be the full mesh.
    for (piece, _) in &pieces {
        if piece.role == Role::TerrainFull
            && occupied.contains(&piece.chunk)
            && let Some(f) = far.get_mut(piece.block)
        {
            *f = false;
        }
    }
    // Plain boxes and far trees stand in for a chunk's full detail until all of it exists.
    let ready = |k: &Key| resident.chunks.get(k).is_some_and(|(_, ready)| *ready);
    let reach = CHUNK * std::f32::consts::FRAC_1_SQRT_2;
    for (piece, mut visibility) in &mut pieces {
        let block_far = far.get(piece.block).copied().unwrap_or(false);
        let (away, near) = (eye.away(piece.middle), eye.near(piece.middle));
        let mut show = shows(piece.role, away, near, block_far, &detail, view);
        let in_view = away <= view + reach;
        match piece.role {
            Role::TerrainFull if occupied.contains(&piece.chunk) && !block_far && in_view => show = true,
            Role::TerrainCoarse if occupied.contains(&piece.chunk) && !block_far => show = false,
            Role::SolidsFull | Role::TreesFull if !ready(&piece.chunk) => show = false,
            Role::SolidsPlain if !ready(&piece.chunk) => show = in_view,
            Role::TreesFar if !ready(&piece.chunk) => show = in_view && near < detail.trees_until,
            _ => {}
        }
        visibility.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// What a chunk's full detail is built from when the camera comes near: which boxes and trees, where
/// its middle is, and what lights it.
#[derive(Resource)]
pub(crate) struct FullParts {
    solids: HashMap<Key, Vec<u32>>,
    trees: HashMap<Key, Vec<u32>>,
    places: HashMap<Key, (Vec3, usize)>,
    openings: Vec<Vec<crate::bake::Opening>>,
    pub(crate) material: Handle<StandardMaterial>,
}

/// The chunks whose full detail is built, and the entities it is drawn by.
#[derive(Resource, Default)]
pub(crate) struct Resident {
    /// Each built chunk's entities, and whether they exist yet: they are spawned at the end of the frame
    /// that finishes them, so a chunk only stands in for its plain boxes from the next.
    chunks: HashMap<Key, (Vec<Entity>, bool)>,
    /// A chunk part way through being built, so that no frame spends much more than its budget.
    pending: Option<Pending>,
    filled: bool,
}

struct Pending {
    key: Key,
    /// Boxes and trees built so far, sorted into the chunk's 32 m squares by where each stands.
    solids: [Batch; CELLS],
    trees: [Batch; CELLS],
    next_solid: usize,
    next_tree: usize,
    /// Once every box and tree is in: which squares go into each mesh, still to spawn one at a time.
    to_spawn: Option<Vec<(Vec<usize>, Role)>>,
    entities: Vec<Entity>,
}

/// A chunk is built in 32 m squares, four by four, so a dense one can be drawn in smaller pieces.
const CELLS_ACROSS: usize = 4;
const CELLS: usize = CELLS_ACROSS * CELLS_ACROSS;

/// Which of a chunk's 32 m squares a point is in.
fn cell_of(chunk: Key, at: Vec3) -> usize {
    let corner = Vec2::new(chunk.0 as f32 * CHUNK, chunk.1 as f32 * CHUNK);
    let cell = ((at.xz() - corner) / (CHUNK / CELLS_ACROSS as f32)).floor().clamp(Vec2::ZERO, Vec2::splat(CELLS_ACROSS as f32 - 1.0));
    cell.x as usize + CELLS_ACROSS * cell.y as usize
}

/// Which squares go into each mesh: the whole chunk if it is within `cap` triangles, else each 64 m
/// quarter that is, else its 32 m squares one by one.
fn group_cells(triangles: &[usize; CELLS], cap: u32) -> Vec<Vec<usize>> {
    let cap = cap.max(1) as usize;
    if triangles.iter().sum::<usize>() <= cap {
        return vec![(0..CELLS).collect()];
    }
    let mut groups = Vec::new();
    for qy in 0..2 {
        for qx in 0..2 {
            let quarter: Vec<usize> = (0..4).map(|i| (qx * 2 + i % 2) + CELLS_ACROSS * (qy * 2 + i / 2)).collect();
            if quarter.iter().map(|&c| triangles[c]).sum::<usize>() <= cap {
                groups.push(quarter);
            } else {
                groups.extend(quarter.into_iter().map(|c| vec![c]));
            }
        }
    }
    groups
}

fn append(into: &mut Batch, from: Batch) {
    let base = into.positions.len() as u32;
    into.positions.extend(from.positions);
    into.normals.extend(from.normals);
    into.colors.extend(from.colors);
    into.indices.extend(from.indices.into_iter().map(|i| i + base));
}

/// How building full detail is going, for reports.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct DetailStats {
    /// Chunks built and dropped so far.
    pub built: u32,
    pub dropped: u32,
    /// The most milliseconds one frame spent building, after the first fill.
    pub worst_frame_ms: f32,
    /// How long the first fill after loading took.
    pub first_fill_ms: f32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_near(
    mut commands: Commands,
    detail: Res<Detail>,
    settings: Res<ViewSettings>,
    sim: Res<prs_runtime::SimResource>,
    atmosphere: Res<prs_atmosphere::Atmosphere>,
    parts: Res<FullParts>,
    mut resident: ResMut<Resident>,
    mut stats: ResMut<DetailStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    camera: Query<(&GlobalTransform, &Projection), With<WorldCamera>>,
) {
    let Ok((camera, projection)) = camera.single() else {
        return;
    };
    let eye = Eye::new(camera, projection, &settings);
    let build_within = detail.solids_full_until.max(detail.trees_full_until.min(detail.trees_until)) + detail.full_build_ahead;
    let drop_beyond = build_within + detail.full_drop_behind.max(0.0);
    let near = |k: &Key| parts.places.get(k).map_or(f32::INFINITY, |(middle, _)| eye.near(*middle));

    let gone: Vec<Key> = resident.chunks.keys().filter(|k| near(k) > drop_beyond).copied().collect();
    for key in gone {
        for entity in resident.chunks.remove(&key).map(|(entities, _)| entities).unwrap_or_default() {
            commands.entity(entity).despawn();
        }
        stats.dropped += 1;
    }
    // What was spawned last frame exists now.
    for (_, ready) in resident.chunks.values_mut() {
        *ready = true;
    }
    if resident.pending.as_ref().is_some_and(|p| near(&p.key) > drop_beyond) {
        for entity in resident.pending.take().map(|p| p.entities).unwrap_or_default() {
            commands.entity(entity).despawn();
        }
    }
    let pending_key = resident.pending.as_ref().map(|p| p.key);
    let mut wanted: Vec<(f32, Key)> = parts
        .places
        .keys()
        .filter(|k| !resident.chunks.contains_key(k) && Some(**k) != pending_key)
        .map(|k| (near(k), *k))
        .filter(|(d, _)| *d < build_within)
        .collect();
    if wanted.is_empty() && resident.pending.is_none() {
        resident.filled = true;
        return;
    }
    // Nearest last, to pop.
    wanted.sort_by(|a, b| b.0.total_cmp(&a.0));
    let level = &sim.0.level;
    let light = RoomLight::with_openings(level, atmosphere.bake, &parts.openings);
    let started = Instant::now();
    let budget = detail.build_ms_per_frame.max(0.0);
    let over_budget = |filled: bool| filled && started.elapsed().as_secs_f32() * 1000.0 >= budget;
    let none: &[u32] = &[];
    loop {
        let mut pending = match resident.pending.take() {
            Some(pending) => pending,
            None => match wanted.pop() {
                Some((_, key)) => Pending {
                    key,
                    solids: Default::default(),
                    trees: Default::default(),
                    next_solid: 0,
                    next_tree: 0,
                    to_spawn: None,
                    entities: Vec::new(),
                },
                None => break,
            },
        };
        let solids = parts.solids.get(&pending.key).map_or(none, |v| v.as_slice());
        let trees = parts.trees.get(&pending.key).map_or(none, |v| v.as_slice());
        // One box or tree at a time, looking at the clock between them.
        while pending.next_solid < solids.len() || pending.next_tree < trees.len() {
            if over_budget(resident.filled) {
                break;
            }
            if let Some(&i) = solids.get(pending.next_solid) {
                let solid = &level.solids[i as usize];
                statics::add_solid(&mut pending.solids[cell_of(pending.key, solid.center)], solid, &light);
                pending.next_solid += 1;
            } else {
                let tree = &level.trees[trees[pending.next_tree] as usize];
                let cell = &mut pending.trees[cell_of(pending.key, tree.base)];
                statics::add_trunk(cell, tree, &light);
                statics::add_canopy(cell, tree, &light);
                pending.next_tree += 1;
            }
        }
        if pending.next_solid < solids.len() || pending.next_tree < trees.len() {
            resident.pending = Some(pending);
            break;
        }
        let (middle, block) = parts.places[&pending.key];
        if pending.to_spawn.is_none() {
            let mut plan = Vec::new();
            for role in [Role::SolidsFull, Role::TreesFull] {
                let cells = if role == Role::SolidsFull { &pending.solids } else { &pending.trees };
                let triangles: [usize; CELLS] = std::array::from_fn(|c| cells[c].indices.len() / 3);
                plan.extend(group_cells(&triangles, detail.split_triangles).into_iter().map(|group| (group, role)));
            }
            pending.to_spawn = Some(plan);
        }
        let mut stopped = false;
        while let Some((group, role)) = pending.to_spawn.as_mut().and_then(Vec::pop) {
            let cells = if role == Role::SolidsFull { &mut pending.solids } else { &mut pending.trees };
            let mut batch = Batch::default();
            for c in group {
                append(&mut batch, std::mem::take(&mut cells[c]));
            }
            if batch.indices.is_empty() {
                continue;
            }
            if atmosphere.bake.face_tint > 0.0 {
                batch.tint_by_face(|normal| atmosphere.face_tint(normal));
            }
            let piece = Piece { role, middle, chunk: pending.key, block };
            let triangles = crate::Triangles(batch.indices.len() as u32 / 3);
            pending.entities.push(commands.spawn((Mesh3d(meshes.add(batch.mesh())), MeshMaterial3d(parts.material.clone()), triangles, piece, Visibility::Hidden)).id());
            if over_budget(resident.filled) {
                stopped = true;
                break;
            }
        }
        if stopped && pending.to_spawn.as_ref().is_some_and(|left| !left.is_empty()) {
            resident.pending = Some(pending);
            break;
        }
        resident.chunks.insert(pending.key, (pending.entities, false));
        stats.built += 1;
        if over_budget(resident.filled) {
            break;
        }
    }
    let spent = started.elapsed().as_secs_f32() * 1000.0;
    if resident.filled {
        stats.worst_frame_ms = stats.worst_frame_ms.max(spent);
    } else {
        stats.first_fill_ms += spent;
        resident.filled = resident.pending.is_none();
    }
}

/// Every piece of the level, with where its distance is measured from.
pub(crate) struct Detailed {
    pub terrain: Vec<(Batch, Piece)>,
    pub solids: Vec<(Batch, Piece)>,
    pub water: Vec<(Batch, Piece)>,
    pub parts: FullParts,
    pub blocks: Blocks,
}

type Key = (i32, i32);

fn chunk_key(at: Vec3) -> Key {
    ((at.x / CHUNK).floor() as i32, (at.z / CHUNK).floor() as i32)
}

pub(crate) fn detailed_meshes(level: &Level, detail: &Detail) -> Detailed {
    let sampler = TerrainSampler::new(&level.terrain);
    let block = (detail.far_block / CHUNK).round().max(1.0) * CHUNK;
    let per_block = (block / CHUNK) as i32;
    let block_of = |chunk: Key| (chunk.0.div_euclid(per_block), chunk.1.div_euclid(per_block));
    // Middles sit on the ground, so a helicopter overhead measures its height as distance too.
    let ground = |x: f32, z: f32| sampler.at(Vec2::new(x, z)).map_or(0.0, |(y, _)| y);
    let chunk_middle = |k: Key| {
        let (x, z) = ((k.0 as f32 + 0.5) * CHUNK, (k.1 as f32 + 0.5) * CHUNK);
        Vec3::new(x, ground(x, z), z)
    };

    let mut block_index: HashMap<Key, usize> = HashMap::new();
    let mut middles = Vec::new();
    let mut index_of = |b: Key| {
        *block_index.entry(b).or_insert_with(|| {
            let (x, z) = ((b.0 as f32 + 0.5) * block, (b.1 as f32 + 0.5) * block);
            middles.push(Vec3::new(x, ground(x, z), z));
            middles.len() - 1
        })
    };

    let mut terrain = Vec::new();
    let mut solids = Vec::new();
    let piece = |role: Role, k: Key, index_of: &mut dyn FnMut(Key) -> usize| Piece {
        role,
        middle: chunk_middle(k),
        chunk: k,
        block: index_of(block_of(k)),
    };

    let coarse = coarse_terrain(&sampler, &detail.terrain_cells, detail.terrain_error, CHUNK, detail.skirt);
    for (k, batch) in full_terrain(&level.terrain, detail.skirt) {
        let role = if coarse.contains_key(&k) { Role::TerrainFull } else { Role::TerrainKept };
        terrain.push((batch, piece(role, k, &mut index_of)));
    }
    for (k, batch) in coarse {
        terrain.push((batch, piece(Role::TerrainCoarse, k, &mut index_of)));
    }

    let mut plain = Chunks::new();
    let (mut solid_parts, mut tree_parts): (HashMap<Key, Vec<u32>>, HashMap<Key, Vec<u32>>) = Default::default();
    for (i, solid) in level.solids.iter().enumerate() {
        solid_parts.entry(chunk_key(solid.center)).or_default().push(i as u32);
        if !is_small(solid, detail.small_solid) {
            add_plain_box(statics::chunk_of(&mut plain, solid.center), solid);
        }
    }
    let mut trees_far = Chunks::new();
    for (i, tree) in level.trees.iter().enumerate() {
        tree_parts.entry(chunk_key(tree.base)).or_default().push(i as u32);
        add_far_tree(statics::chunk_of(&mut trees_far, tree.base), tree);
    }
    let places = solid_parts.keys().chain(tree_parts.keys()).map(|&k| (k, (chunk_middle(k), index_of(block_of(k))))).collect();
    for (chunks, role) in [(plain, Role::SolidsPlain), (trees_far, Role::TreesFar)] {
        for (k, batch) in chunks {
            for part in split_to_cap(batch, k, detail.split_triangles) {
                if !part.indices.is_empty() {
                    solids.push((part, piece(role, k, &mut index_of)));
                }
            }
        }
    }
    let mut water = Vec::new();
    for (k, batch) in statics::water_chunks(&level.terrain) {
        if !batch.indices.is_empty() {
            water.push((batch, piece(Role::Water, k, &mut index_of)));
        }
    }
    let middle_of_block = |b: Key, index_of: &mut dyn FnMut(Key) -> usize| {
        let index = index_of(b);
        Piece { role: Role::Block, middle: Vec3::ZERO, chunk: (b.0 * per_block, b.1 * per_block), block: index }
    };
    for (b, batch) in coarse_terrain(&sampler, &detail.far_terrain_cells, detail.far_terrain_error, block, detail.skirt) {
        let block = (b.0.div_euclid(per_block), b.1.div_euclid(per_block));
        terrain.push((batch, middle_of_block(block, &mut index_of)));
    }
    // A block's own pieces measure from its middle, which is only known once every block has one.
    for (_, piece) in terrain.iter_mut().chain(solids.iter_mut()) {
        if piece.role == Role::Block {
            piece.middle = middles[piece.block];
        }
    }
    let parts = FullParts { solids: solid_parts, trees: tree_parts, places, openings: RoomLight::openings(level), material: Handle::default() };
    Detailed { terrain, solids, water, parts, blocks: Blocks { middles, radius: block * std::f32::consts::FRAC_1_SQRT_2 } }
}

/// A chunk's batch, cut into 64 m squares if it has more than `cap` triangles, and any of those still
/// over it into 32 m squares.
fn split_to_cap(batch: Batch, chunk: Key, cap: u32) -> Vec<Batch> {
    const SMALLEST: f32 = 32.0;
    let corner = Vec2::new(chunk.0 as f32 * CHUNK, chunk.1 as f32 * CHUNK);
    let mut out = Vec::new();
    let mut todo = vec![(batch, corner, CHUNK)];
    while let Some((batch, corner, side)) = todo.pop() {
        if batch.indices.len() / 3 <= cap.max(1) as usize || side <= SMALLEST {
            out.push(batch);
            continue;
        }
        let half = side * 0.5;
        for (i, part) in split_in_four(&batch, corner, half).into_iter().enumerate() {
            let at = corner + Vec2::new((i % 2) as f32 * half, (i / 2) as f32 * half);
            if !part.indices.is_empty() {
                todo.push((part, at, half));
            }
        }
    }
    out
}

/// Cuts a batch into the four squares of `half` from `corner`, by where each triangle's middle is. A
/// triangle stays whole, so a box across a line goes with one side, which only moves its culling.
fn split_in_four(batch: &Batch, corner: Vec2, half: f32) -> [Batch; 4] {
    let mut parts: [Batch; 4] = Default::default();
    let mut remap: [Vec<u32>; 4] = Default::default();
    for map in &mut remap {
        *map = vec![u32::MAX; batch.positions.len()];
    }
    for tri in batch.indices.chunks(3) {
        let middle = tri.iter().map(|&i| Vec3::from_array(batch.positions[i as usize])).sum::<Vec3>() / 3.0;
        let along = ((middle.xz() - corner) / half).floor().clamp(Vec2::ZERO, Vec2::ONE);
        let which = along.x as usize + 2 * along.y as usize;
        let (part, map) = (&mut parts[which], &mut remap[which]);
        for &i in tri {
            if map[i as usize] == u32::MAX {
                map[i as usize] = part.positions.len() as u32;
                part.positions.push(batch.positions[i as usize]);
                part.normals.push(batch.normals[i as usize]);
                part.colors.push(batch.colors[i as usize]);
            }
            part.indices.push(map[i as usize]);
        }
    }
    parts
}

/// Whether a box is too small to be worth drawing from far away.
fn is_small(solid: &Solid, under: f32) -> bool {
    solid.half.max_element() * 2.0 < under
}

/// A box with one flat colour per face and no underside: what is left of it from a few hundred metres.
fn add_plain_box(batch: &mut Batch, solid: &Solid) {
    let (color, vary) = statics::palette(solid.paint);
    let color = color * (1.0 - vary + statics::grain(solid.center) * vary * 2.0);
    let corner = |x: f32, y: f32, z: f32| solid.center + solid.rotation * (solid.half * Vec3::new(x, y, z));
    let faces: [([Vec3; 4], Vec3); 5] = [
        ([corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.), corner(1., -1., 1.)], Vec3::X),
        ([corner(-1., -1., 1.), corner(-1., 1., 1.), corner(-1., 1., -1.), corner(-1., -1., -1.)], Vec3::NEG_X),
        ([corner(-1., 1., -1.), corner(-1., 1., 1.), corner(1., 1., 1.), corner(1., 1., -1.)], Vec3::Y),
        ([corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], Vec3::Z),
        ([corner(1., -1., -1.), corner(-1., -1., -1.), corner(-1., 1., -1.), corner(1., 1., -1.)], Vec3::NEG_Z),
    ];
    for (corners, normal) in faces {
        batch.quad(corners, [color; 4], solid.rotation * normal);
    }
}

/// One shape for a whole tree: a four-sided cone for a fir, a diamond on a stalk for the rest.
fn add_far_tree(batch: &mut Batch, tree: &Tree) {
    let (h, r) = (tree.height, tree.radius);
    let vary = 0.88 + statics::grain(tree.base) * 0.24;
    let turn = Quat::from_rotation_y(statics::grain(tree.base + Vec3::X) * std::f32::consts::TAU);
    let side = |i: usize, radius: f32, y: f32| {
        let a = i as f32 * std::f32::consts::FRAC_PI_2;
        tree.base + turn * Vec3::new(a.cos() * radius, y, a.sin() * radius)
    };
    let (color, low, high) = match tree.kind {
        TreeKind::Fir => (Vec3::new(0.13, 0.3, 0.17), h * 0.12, h),
        TreeKind::Broadleaf => (Vec3::new(0.24, 0.42, 0.14), h * 0.35, h),
        TreeKind::Poplar => (Vec3::new(0.27, 0.43, 0.15), h * 0.2, h),
        TreeKind::Fruit => (Vec3::new(0.33, 0.45, 0.17), h * 0.3, h * 0.95),
        TreeKind::Palm => (Vec3::new(0.3, 0.42, 0.16), h * 0.8, h * 0.98),
    };
    let color = color * vary;
    let top = tree.base + Vec3::Y * high;
    if tree.kind == TreeKind::Fir {
        for i in 0..4 {
            let (a, b) = (side(i, r, low), side(i + 1, r, low));
            batch.triangle([top, b, a], [statics::leaf(color, 0.72, a + b); 3]);
        }
        return;
    }
    let waist = low + (high - low) * 0.4;
    let bottom = tree.base + Vec3::Y * low;
    for i in 0..4 {
        let (a, b) = (side(i, r, waist), side(i + 1, r, waist));
        batch.triangle([top, b, a], [statics::leaf(color, 0.75, a + b); 3]);
        batch.triangle([bottom, a, b], [statics::leaf(color, 0.3, a + b); 3]);
    }
    if low > 1.0 {
        let trunk = Solid {
            center: tree.base + Vec3::Y * (low * 0.5),
            half: Vec3::new(prs_level::TRUNK_HALF_WIDTH, low * 0.5, prs_level::TRUNK_HALF_WIDTH),
            rotation: Quat::IDENTITY,
            paint: Paint::Trunk,
        };
        add_plain_box(batch, &trunk);
    }
}

/// Finds the terrain's height and colour anywhere, from the triangles of the full mesh.
struct TerrainSampler<'a> {
    terrain: &'a Terrain,
    cells: HashMap<Key, Vec<u32>>,
}

impl<'a> TerrainSampler<'a> {
    const CELL: f32 = 16.0;

    fn new(terrain: &'a Terrain) -> Self {
        let mut cells: HashMap<Key, Vec<u32>> = HashMap::new();
        for (i, tri) in terrain.indices.iter().enumerate() {
            let corners = tri.map(|v| terrain.vertices[v as usize].xz());
            let (lo, hi) = corners.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let (x0, z0) = ((lo.x / Self::CELL).floor() as i32, (lo.y / Self::CELL).floor() as i32);
            let (x1, z1) = ((hi.x / Self::CELL).floor() as i32, (hi.y / Self::CELL).floor() as i32);
            for x in x0..=x1 {
                for z in z0..=z1 {
                    cells.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        Self { terrain, cells }
    }

    /// Height and colour at a point, or `None` off the terrain.
    fn at(&self, p: Vec2) -> Option<(f32, Vec3)> {
        let key = ((p.x / Self::CELL).floor() as i32, (p.y / Self::CELL).floor() as i32);
        let terrain = self.terrain;
        for &i in self.cells.get(&key)? {
            let [a, b, c] = terrain.indices[i as usize].map(|v| terrain.vertices[v as usize]);
            let Some([u, v, w]) = barycentric(p, a.xz(), b.xz(), c.xz()) else {
                continue;
            };
            let colors = statics::triangle_colors(terrain, i as usize);
            return Some((u * a.y + v * b.y + w * c.y, colors[0] * u + colors[1] * v + colors[2] * w));
        }
        None
    }
}

fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<[f32; 3]> {
    let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if d.abs() < 1e-9 {
        return None;
    }
    let u = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / d;
    let v = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / d;
    let w = 1.0 - u - v;
    const SLACK: f32 = -1e-4;
    (u >= SLACK && v >= SLACK && w >= SLACK).then_some([u, v, w])
}

/// Where grid lines fall between `from` and `to`: every `cell` from the world's origin, plus both ends,
/// so that neighbouring pieces share their edges exactly.
fn grid_lines(from: f32, to: f32, cell: f32) -> Vec<f32> {
    let mut lines = vec![from];
    let mut k = (from / cell).floor() + 1.0;
    while k * cell < to - cell * 0.01 {
        lines.push(k * cell);
        k += 1.0;
    }
    lines.push(to);
    lines
}

/// The terrain as one regular grid per square of `side`, keyed by which chunk its corner is in, each
/// with a skirt round it. Each square takes the coarsest of `cells` whose surface stays within `error`
/// of the full mesh's corners inside it. If none does, a chunk-sized square is left out, for its full
/// mesh to stand in, and a bigger one takes the finest.
fn coarse_terrain(sampler: &TerrainSampler, cells: &[f32], error: f32, side: f32, skirt: f32) -> BTreeMap<Key, Batch> {
    let terrain = sampler.terrain;
    let half = terrain.size * 0.5;
    let first = (-half / side).floor() as i32;
    let last = (half / side).ceil() as i32;
    let square_of = |p: Vec3| ((p.x / side).floor() as i32, (p.z / side).floor() as i32);
    let mut corners: HashMap<Key, Vec<Vec3>> = HashMap::new();
    for &v in &terrain.vertices {
        corners.entry(square_of(v)).or_default().push(v);
    }
    let mut cells: Vec<f32> = cells.iter().copied().filter(|c| *c >= 1.0).collect();
    cells.sort_by(|a, b| b.total_cmp(a));
    let mut out = BTreeMap::new();
    for bx in first..last {
        for bz in first..last {
            let (x0, x1) = ((bx as f32 * side).max(-half), ((bx + 1) as f32 * side).min(half));
            let (z0, z1) = ((bz as f32 * side).max(-half), ((bz + 1) as f32 * side).min(half));
            if x1 - x0 < 0.5 || z1 - z0 < 0.5 {
                continue;
            }
            let key = chunk_key(Vec3::new(x0 + 0.01, 0.0, z0 + 0.01));
            let inside = corners.get(&(bx, bz)).map_or(&[][..], |c| c.as_slice());
            let grid = cells.iter().find_map(|&cell| {
                let grid = Grid::sample(sampler, x0, x1, z0, z1, cell);
                (grid.worst_error(inside) <= error).then_some(grid)
            });
            match grid {
                Some(grid) => {
                    out.insert(key, grid.batch(skirt));
                }
                // Nothing coarser will do here, and the chunk's full mesh is already stored.
                None if (side - CHUNK).abs() < 0.5 => {}
                None => {
                    let finest = cells.last().copied().unwrap_or(side);
                    out.insert(key, Grid::sample(sampler, x0, x1, z0, z1, finest).batch(skirt));
                }
            }
        }
    }
    out
}

/// Heights and colours on a regular grid over one square.
struct Grid {
    xs: Vec<f32>,
    zs: Vec<f32>,
    points: Vec<(Vec3, Vec3)>,
}

impl Grid {
    fn sample(sampler: &TerrainSampler, x0: f32, x1: f32, z0: f32, z1: f32, cell: f32) -> Self {
        let (xs, zs) = (grid_lines(x0, x1, cell), grid_lines(z0, z1, cell));
        let mut points = Vec::with_capacity(xs.len() * zs.len());
        for &z in &zs {
            for &x in &xs {
                let (y, color) = sampler.at(Vec2::new(x, z)).unwrap_or((0.0, Vec3::splat(0.4)));
                points.push((Vec3::new(x, y, z), color));
            }
        }
        Self { xs, zs, points }
    }

    fn at(&self, i: usize, j: usize) -> (Vec3, Vec3) {
        self.points[j * self.xs.len() + i]
    }

    /// The two triangles of a grid cell, with the diagonal alternating as the full terrain's does.
    fn cell_triangles(&self, i: usize, j: usize) -> [[(Vec3, Vec3); 3]; 2] {
        let (p00, p10, p01, p11) = (self.at(i, j), self.at(i + 1, j), self.at(i, j + 1), self.at(i + 1, j + 1));
        if (i + j).is_multiple_of(2) { [[p00, p01, p10], [p10, p01, p11]] } else { [[p00, p01, p11], [p00, p11, p10]] }
    }

    /// The farthest any of `corners` is above or below this grid's surface, in metres.
    fn worst_error(&self, corners: &[Vec3]) -> f32 {
        let mut worst: f32 = 0.0;
        for p in corners {
            let i = self.xs.partition_point(|x| *x <= p.x).clamp(1, self.xs.len() - 1) - 1;
            let j = self.zs.partition_point(|z| *z <= p.z).clamp(1, self.zs.len() - 1) - 1;
            for [a, b, c] in self.cell_triangles(i, j) {
                if let Some([u, v, w]) = barycentric(p.xz(), a.0.xz(), b.0.xz(), c.0.xz()) {
                    worst = worst.max((u * a.0.y + v * b.0.y + w * c.0.y - p.y).abs());
                    break;
                }
            }
        }
        worst
    }

    fn batch(&self, skirt: f32) -> Batch {
        let mut batch = Batch::default();
        let (row, top) = (self.xs.len(), self.zs.len() - 1);
        for j in 0..top {
            for i in 0..row - 1 {
                for t in self.cell_triangles(i, j) {
                    batch.triangle([t[0].0, t[1].0, t[2].0], [t[0].1, t[1].1, t[2].1]);
                }
            }
        }
        for i in 0..row - 1 {
            add_skirt(&mut batch, self.at(i, 0), self.at(i + 1, 0), Vec3::NEG_Z, skirt);
            add_skirt(&mut batch, self.at(i, top), self.at(i + 1, top), Vec3::Z, skirt);
        }
        for j in 0..top {
            add_skirt(&mut batch, self.at(0, j), self.at(0, j + 1), Vec3::NEG_X, skirt);
            add_skirt(&mut batch, self.at(row - 1, j), self.at(row - 1, j + 1), Vec3::X, skirt);
        }
        batch
    }
}

/// A strip hanging `depth` below the edge from `a` to `b`, facing `out`.
fn add_skirt(batch: &mut Batch, a: (Vec3, Vec3), b: (Vec3, Vec3), out: Vec3, depth: f32) {
    let down = Vec3::Y * depth;
    let (a, b) = if (b.0 - a.0).cross(-down).dot(out) >= 0.0 { (a, b) } else { (b, a) };
    batch.triangle([a.0, b.0, b.0 - down], [a.1, b.1, b.1]);
    batch.triangle([a.0, b.0 - down, a.0 - down], [a.1, b.1, a.1]);
}

/// The full terrain mesh, with every triangle that crosses a chunk's edge cut along it, so that each
/// chunk is an exact square and can carry a skirt where it meets a coarser neighbour.
fn full_terrain(terrain: &Terrain, skirt: f32) -> BTreeMap<Key, Batch> {
    let mut out: BTreeMap<Key, Batch> = BTreeMap::new();
    for (i, tri) in terrain.indices.iter().enumerate() {
        let corners = tri.map(|v| terrain.vertices[v as usize]);
        let colors = statics::triangle_colors(terrain, i);
        let polygon: Vec<(Vec3, Vec3)> = corners.into_iter().zip(colors).collect();
        let (lo, hi) = corners.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(p.xz()), hi.max(p.xz())));
        // A point on a chunk's edge belongs to both chunks; a sliver of slack keeps such triangles whole.
        const EDGE: f32 = 1e-3;
        let first = chunk_key(Vec3::new(lo.x + EDGE, 0.0, lo.y + EDGE));
        let last = chunk_key(Vec3::new(hi.x - EDGE, 0.0, hi.y - EDGE));
        for kx in first.0..=last.0.max(first.0) {
            for kz in first.1..=last.1.max(first.1) {
                let square = Rect::new(kx as f32 * CHUNK, kz as f32 * CHUNK, (kx + 1) as f32 * CHUNK, (kz + 1) as f32 * CHUNK);
                let piece = if first == last { polygon.clone() } else { clip(&polygon, square) };
                if piece.len() < 3 {
                    continue;
                }
                let batch = out.entry((kx, kz)).or_default();
                for k in 1..piece.len() - 1 {
                    let (a, b, c) = (piece[0], piece[k], piece[k + 1]);
                    batch.triangle([a.0, b.0, c.0], [a.1, b.1, c.1]);
                }
                for k in 0..piece.len() {
                    let (a, b) = (piece[k], piece[(k + 1) % piece.len()]);
                    if let Some(out) = edge_side(a.0, b.0, square) {
                        add_skirt(batch, a, b, out, skirt);
                    }
                }
            }
        }
    }
    out
}

/// Which way out of `square` an edge faces, if it lies along one of the square's sides.
fn edge_side(a: Vec3, b: Vec3, square: Rect) -> Option<Vec3> {
    const ON: f32 = 1e-3;
    let on = |p: f32, line: f32| (p - line).abs() < ON;
    if on(a.x, square.min.x) && on(b.x, square.min.x) {
        Some(Vec3::NEG_X)
    } else if on(a.x, square.max.x) && on(b.x, square.max.x) {
        Some(Vec3::X)
    } else if on(a.z, square.min.y) && on(b.z, square.min.y) {
        Some(Vec3::NEG_Z)
    } else if on(a.z, square.max.y) && on(b.z, square.max.y) {
        Some(Vec3::Z)
    } else {
        None
    }
}

/// Cuts a convex polygon to the inside of `square` (Sutherland and Hodgman), carrying colour along.
fn clip(polygon: &[(Vec3, Vec3)], square: Rect) -> Vec<(Vec3, Vec3)> {
    // Each side as (whether it is a line of x, where, which side is in).
    let planes = [(true, square.min.x, 1.0), (true, square.max.x, -1.0), (false, square.min.y, 1.0), (false, square.max.y, -1.0)];
    let mut points = polygon.to_vec();
    for (is_x, line, sign) in planes {
        let coord = |p: Vec3| if is_x { p.x } else { p.z };
        let inside = |p: Vec3| (coord(p) - line) * sign >= 0.0;
        let mut kept = Vec::with_capacity(points.len() + 2);
        for k in 0..points.len() {
            let (a, b) = (points[k], points[(k + 1) % points.len()]);
            if inside(a.0) {
                kept.push(a);
            }
            if inside(a.0) != inside(b.0) {
                let t = (line - coord(a.0)) / (coord(b.0) - coord(a.0));
                let mut p = a.0.lerp(b.0, t);
                // Exactly on the line, so the edge test for skirts finds it.
                if is_x { p.x = line } else { p.z = line }
                kept.push((p, a.1.lerp(b.1, t)));
            }
        }
        points = kept;
        if points.is_empty() {
            break;
        }
    }
    points
}

/// Spawns the level's pieces, hidden until [`choose_detail`] has looked at where the camera is.
pub(crate) fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, pieces: Vec<(Batch, Piece)>, material: &Handle<StandardMaterial>) {
    for (batch, piece) in pieces {
        let triangles = crate::Triangles(batch.indices.len() as u32 / 3);
        commands.spawn((Mesh3d(meshes.add(batch.mesh())), MeshMaterial3d(material.clone()), triangles, piece, Visibility::Hidden));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lines_share_edges_and_include_both_ends() {
        assert_eq!(grid_lines(0.0, 128.0, 32.0), vec![0.0, 32.0, 64.0, 96.0, 128.0]);
        assert_eq!(grid_lines(-500.0, -384.0, 32.0), vec![-500.0, -480.0, -448.0, -416.0, -384.0]);
    }

    #[test]
    fn coarse_terrain_follows_the_ground() {
        let terrain = Terrain::generate(512.0, 64, |p| (p.x * 0.02).sin() * 6.0 + p.y * 0.01);
        let sampler = TerrainSampler::new(&terrain);
        let squares = coarse_terrain(&sampler, &[16.0], f32::INFINITY, 128.0, 6.0);
        assert_eq!(squares.len(), 16);
        for batch in squares.values() {
            for &i in &batch.indices {
                let p = Vec3::from_array(batch.positions[i as usize]);
                let ground = sampler.at(p.xz()).unwrap().0;
                // Grid points sit on the ground, and skirts hang below it by the skirt, never above.
                assert!(p.y <= ground + 0.01 && p.y >= ground - 6.5, "{p} vs {ground}");
            }
        }
    }

    /// At every distance exactly one terrain piece covers a place, and a block never doubles up with
    /// its chunks: walking the distances through every boundary finds no gap and no overlap.
    /// Flat ground goes to the coarsest grid; a narrow ditch keeps enough detail to stay a ditch.
    #[test]
    fn rough_squares_keep_more_detail_than_flat_ones() {
        let ditch = |p: Vec2| if (p.x - 40.0).abs() < 5.0 { -3.0 } else { 0.0 };
        let terrain = Terrain::generate(256.0, 64, ditch);
        let sampler = TerrainSampler::new(&terrain);
        let squares = coarse_terrain(&sampler, &[32.0, 16.0, 8.0, 4.0], 0.6, 128.0, 6.0);
        let count = |k: Key| squares[&k].indices.len();
        // (-1, 0) is open ground; (0, 0) holds the ditch at x = 40.
        assert!(count((0, 0)) > count((-1, 0)) * 4, "{} vs {}", count((0, 0)), count((-1, 0)));
        // Asked to follow the ditch to a centimetre, no grid will do, and the full mesh is left to stand in.
        let strict = coarse_terrain(&sampler, &[32.0, 16.0], 0.01, 128.0, 6.0);
        assert!(!strict.contains_key(&(0, 0)) && strict.contains_key(&(-1, 0)));
    }

    #[test]
    fn full_terrain_is_cut_into_exact_squares_with_skirts() {
        let terrain = Terrain::generate(512.0, 37, |p| (p.x * 0.02).sin() * 6.0);
        let chunks = full_terrain(&terrain, 6.0);
        // Upward faces cover the ground exactly once, as the uncut mesh does; skirts only add walls.
        let up_area = |t: [Vec3; 3]| {
            let n = (t[1] - t[0]).as_dvec3().cross((t[2] - t[0]).as_dvec3());
            if n.normalize().y > 0.5 { n.y.abs() * 0.5 } else { 0.0 }
        };
        let cut: f64 = chunks
            .values()
            .flat_map(|b| b.indices.chunks(3).map(|t| [0, 1, 2].map(|k| Vec3::from_array(b.positions[t[k] as usize]))))
            .map(up_area)
            .sum();
        let whole: f64 = terrain.indices.iter().map(|t| up_area(t.map(|v| terrain.vertices[v as usize]))).sum();
        assert!((cut - whole).abs() < 0.5, "{cut} vs {whole}");
        for (&(kx, kz), batch) in &chunks {
            for p in &batch.positions {
                assert!(p[0] >= kx as f32 * CHUNK - 1e-3 && p[0] <= (kx + 1) as f32 * CHUNK + 1e-3);
                assert!(p[2] >= kz as f32 * CHUNK - 1e-3 && p[2] <= (kz + 1) as f32 * CHUNK + 1e-3);
            }
        }
    }

    #[test]
    fn exactly_one_terrain_piece_shows_at_every_distance() {
        let detail = Detail::default();
        let view = 4000.0;
        let radius = detail.far_block * std::f32::consts::FRAC_1_SQRT_2;
        for step in 0..1200 {
            let chunk_distance = step as f32 * 3.0;
            // The block's middle can be nearer or farther than the chunk's by up to its radius.
            for offset in [-radius, -100.0, 0.0, 100.0, radius] {
                let block_distance = (chunk_distance + offset).max(0.0);
                let far = block_is_far(block_distance, radius, &detail, view);
                let past_view = chunk_distance > view + CHUNK * std::f32::consts::FRAC_1_SQRT_2 && !far;
                // A chunk has either a full and a coarse mesh, or only its full mesh kept for every distance.
                for set in [&[Role::TerrainFull, Role::TerrainCoarse, Role::Block][..], &[Role::TerrainKept, Role::Block][..]] {
                    let shown = set.iter().filter(|&&role| shows(role, chunk_distance, chunk_distance, far, &detail, view)).count();
                    assert_eq!(shown, usize::from(!past_view), "{chunk_distance} m, block at {block_distance} m, {set:?}");
                }
            }
        }
    }

    /// Through a scope, what is in front looks as near as the magnification makes it, and what is
    /// behind or off to the side does not.
    #[test]
    fn a_scope_brings_detail_nearer_only_where_you_look() {
        let eye = Eye { at: Vec3::ZERO, forward: Vec3::NEG_Z, cone_cos: 0.5_f32.cos(), zoom: 0.25 };
        assert!((eye.near(Vec3::new(0.0, 0.0, -800.0)) - 200.0).abs() < 1e-3);
        assert_eq!(eye.near(Vec3::new(0.0, 0.0, 800.0)), 800.0);
        assert_eq!(eye.near(Vec3::new(800.0, 0.0, 0.0)), 800.0);
        let unzoomed = Eye { zoom: 1.0, ..eye };
        assert_eq!(unzoomed.near(Vec3::new(0.0, 0.0, -800.0)), 800.0);
    }

    /// A dense chunk is cut into smaller squares, every triangle kept exactly once and in its square.
    #[test]
    fn squares_are_grouped_up_to_the_cap() {
        assert_eq!(group_cells(&[100; CELLS], 5_000), vec![(0..CELLS).collect::<Vec<_>>()]);
        let quarters = group_cells(&[1_000; CELLS], 5_000);
        assert_eq!(quarters.len(), 4);
        assert!(quarters.iter().all(|q| q.len() == 4));
        assert_eq!(group_cells(&[2_000; CELLS], 5_000).len(), CELLS);
        let mut all: Vec<usize> = quarters.concat();
        all.sort();
        assert_eq!(all, (0..CELLS).collect::<Vec<_>>());
        assert_eq!(cell_of((0, 0), Vec3::new(5.0, 0.0, 5.0)), 0);
        assert_eq!(cell_of((0, 0), Vec3::new(127.0, 0.0, 127.0)), CELLS - 1);
        assert_eq!(cell_of((-1, 0), Vec3::new(-1.0, 0.0, 40.0)), 3 + CELLS_ACROSS);
    }

    #[test]
    fn dense_chunks_are_cut_into_smaller_squares() {
        let mut batch = Batch::default();
        for i in 0..64 {
            for j in 0..64 {
                let p = Vec3::new(128.0 + i as f32 * 2.0 + 0.5, 0.0, -128.0 + j as f32 * 2.0 + 0.5);
                batch.triangle([p, p + Vec3::X, p + Vec3::Z], [Vec3::ONE; 3]);
            }
        }
        let parts = split_to_cap(batch, (1, -1), 300);
        assert_eq!(parts.len(), 16, "4096 triangles at 300 a part go down to 32 m squares");
        assert_eq!(parts.iter().map(|p| p.indices.len() / 3).sum::<usize>(), 4096);
        for part in &parts {
            let xs: Vec<f32> = part.positions.iter().map(|p| p[0]).collect();
            let span = xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min);
            assert!(span < 33.0, "{span}");
        }
        let few = Batch::default();
        assert_eq!(split_to_cap(few, (0, 0), 300).len(), 1);
    }

    #[test]
    fn nothing_shows_past_the_view_distance() {
        let detail = Detail::default();
        let far = block_is_far(9000.0, 362.0, &detail, 1000.0);
        assert!(!far);
        for role in [Role::TerrainCoarse, Role::SolidsPlain, Role::TreesFar] {
            assert!(!shows(role, 1200.0, 1200.0, false, &detail, 1000.0));
        }
    }
}
