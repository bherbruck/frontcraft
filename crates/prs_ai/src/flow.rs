//! The way to somewhere across a whole map. Walls with a gate in them cannot be found by steering, so for
//! each place soldiers get sent, every spot on the ground gets told which way is shorter, once, at the start.
//! After that a soldier just reads a number, so it costs nothing however many of them there are.
//!
//! It is worked out from the level's data with no queries at all. Each solid marks the cells it stands in
//! for anyone on the ground, a little larger than it is, and so does any ground too steep to walk. A wall is
//! usually thinner than a cell, so what it really marks is the steps between cells that would go through it.

use glam::{Quat, Vec2, Vec3};
use prs_sim::{Level, Solid};
use std::collections::VecDeque;

const UNREACHED: u16 = u16::MAX;
const WALL_X: u8 = 1 << 0;
const WALL_Z: u8 = 1 << 1;
const BLOCKED: u8 = 1 << 2;
/// A soldier off in a marked cell looks this many cells around for the way.
const SEARCH: i32 = 3;
/// Goes for a spot this many cells down the way, so as not to zigzag from cell to cell.
const AHEAD: usize = 4;

/// What counts as in the way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowRules {
    pub cell: f32,
    /// Clearance around a solid: about a soldier's width.
    pub margin: f32,
    /// A solid no higher than this above the ground can be stepped on.
    pub step: f32,
    /// A solid all above this can be walked under.
    pub headroom: f32,
    /// Most height a cell's ground may change across it.
    pub max_rise: f32,
    /// Most a cell may lie below the ground either side of it. A ditch can have gentle slopes to it from
    /// a distance and still be what stops a truck. A soldier walks through one, so for them this is large.
    pub max_dip: f32,
}

/// Which cells can be walked in. Shared by every field on the map.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowGrid {
    pub rules: FlowRules,
    half: f32,
    width: i32,
    /// For each cell, whether a solid stands in the way of the step to +x ([`WALL_X`]) or +z
    /// ([`WALL_Z`]), and whether it can't be stood in at all ([`BLOCKED`]). One byte a cell, which on a
    /// map four kilometres across is still millions of them.
    cells: Vec<u8>,
}

impl FlowGrid {
    pub fn new(level: &Level, rules: FlowRules) -> Self {
        let half = level.terrain.size * 0.5;
        let width = (level.terrain.size / rules.cell).ceil() as i32;
        let mut grid = Self { rules, half, width, cells: vec![0; (width * width) as usize] };
        let ground = grid.ground_heights(level);
        let height = |i: i32, j: i32| ground[(j * width + i) as usize];
        for j in 0..width {
            for i in 0..width {
                let edge = i == 0 || j == 0 || i == width - 1 || j == width - 1;
                let steep = !edge && ((height(i + 1, j) - height(i - 1, j)).abs() > rules.max_rise * 2.0 || (height(i, j + 1) - height(i, j - 1)).abs() > rules.max_rise * 2.0);
                if edge || steep {
                    grid.cells[(j * width + i) as usize] |= BLOCKED;
                }
            }
        }
        // A dip counts for a cell around it as well, as anything that has a wheel on the edge of one is going in.
        if rules.max_dip.is_finite() {
            let dip = |a: f32, b: f32, here: f32| (a + b) * 0.5 - here > rules.max_dip;
            for j in 1..width - 1 {
                for i in 1..width - 1 {
                    let here = height(i, j);
                    if dip(height(i + 1, j), height(i - 1, j), here) || dip(height(i, j + 1), height(i, j - 1), here) {
                        for b in j - 1..=j + 1 {
                            for a in i - 1..=i + 1 {
                                grid.cells[(b * width + a) as usize] |= BLOCKED;
                            }
                        }
                    }
                }
            }
        }
        for solid in &level.solids {
            grid.mark(&ground, solid);
        }
        for tree in &level.trees {
            let half = Vec3::new(prs_sim::TRUNK_HALF_WIDTH, tree.height * prs_sim::TRUNK_HALF_HEIGHT, prs_sim::TRUNK_HALF_WIDTH);
            let trunk = Solid { center: tree.base + Vec3::Y * half.y, half, rotation: Quat::IDENTITY, paint: prs_sim::Paint::Trunk };
            grid.mark(&ground, &trunk);
        }
        grid
    }

    /// The height of the ground at the middle of every cell, from the terrain's triangles.
    fn ground_heights(&self, level: &Level) -> Vec<f32> {
        let mut ground = vec![0.0; self.cells.len()];
        let vertices = &level.terrain.vertices;
        for [a, b, c] in &level.terrain.indices {
            let (a, b, c) = (vertices[*a as usize], vertices[*b as usize], vertices[*c as usize]);
            let (pa, pb, pc) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z), Vec2::new(c.x, c.z));
            let (i0, j0) = self.cell_of(pa.min(pb).min(pc));
            let (i1, j1) = self.cell_of(pa.max(pb).max(pc));
            let area = (pb - pa).perp_dot(pc - pa);
            if area.abs() < 1e-6 {
                continue;
            }
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = self.center(i, j);
                    let (u, v) = ((p - pa).perp_dot(pc - pa) / area, (pb - pa).perp_dot(p - pa) / area);
                    if u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001 {
                        ground[(j * self.width + i) as usize] = a.y + (b.y - a.y) * u + (c.y - a.y) * v;
                    }
                }
            }
        }
        ground
    }

    fn bytes(&self) -> usize {
        self.cells.len()
    }

    fn center(&self, i: i32, j: i32) -> Vec2 {
        (Vec2::new(i as f32, j as f32) + Vec2::splat(0.5)) * self.rules.cell - Vec2::splat(self.half)
    }

    fn cell_of(&self, p: Vec2) -> (i32, i32) {
        let at = (p + Vec2::splat(self.half)) / self.rules.cell;
        ((at.x.floor() as i32).clamp(0, self.width - 1), (at.y.floor() as i32).clamp(0, self.width - 1))
    }

    fn index(&self, i: i32, j: i32) -> Option<usize> {
        (i >= 0 && j >= 0 && i < self.width && j < self.width).then(|| (j * self.width + i) as usize)
    }

    fn mark(&mut self, ground: &[f32], solid: &Solid) {
        let r = self.rules;
        let corners = [Vec3::new(1.0, 1.0, 1.0), Vec3::new(-1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, -1.0), Vec3::new(-1.0, 1.0, -1.0)];
        let reach = corners.iter().map(|c| (solid.rotation * (*c * solid.half)).abs()).fold(Vec3::ZERO, Vec3::max);
        let flat = Vec2::new(solid.center.x, solid.center.z);
        let spread = Vec2::new(reach.x, reach.z) + Vec2::splat(r.margin);
        let (i0, j0) = self.cell_of(flat - spread - Vec2::splat(r.cell));
        let (i1, j1) = self.cell_of(flat + spread);
        let inverse = solid.rotation.conjugate();
        let local_of = |p: Vec2| {
            let local = inverse * Vec3::new(p.x - solid.center.x, 0.0, p.y - solid.center.z);
            Vec2::new(local.x, local.z)
        };
        let room = Vec2::new(solid.half.x, solid.half.z) + Vec2::splat(r.margin);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = self.center(i, j);
                let floor = ground[(j * self.width + i) as usize];
                let (bottom, top) = (solid.center.y - reach.y, solid.center.y + reach.y);
                if top <= floor + r.step || bottom >= floor + r.headroom {
                    continue;
                }
                let local = local_of(p);
                let at = (j * self.width + i) as usize;
                if local.x.abs() <= room.x && local.y.abs() <= room.y {
                    self.cells[at] |= BLOCKED;
                }
                if crosses_box(local, local_of(p + Vec2::X * r.cell), room) {
                    self.cells[at] |= WALL_X;
                }
                if crosses_box(local, local_of(p + Vec2::Y * r.cell), room) {
                    self.cells[at] |= WALL_Z;
                }
            }
        }
    }

    /// Marks somewhere round in the way, like something parked.
    pub fn mark_round(&mut self, center: Vec2, radius: f32) {
        let spread = Vec2::splat(radius);
        let (i0, j0) = self.cell_of(center - spread);
        let (i1, j1) = self.cell_of(center + spread);
        for j in j0..=j1 {
            for i in i0..=i1 {
                if self.center(i, j).distance(center) <= radius {
                    let at = (j * self.width + i) as usize;
                    self.cells[at] |= BLOCKED;
                }
            }
        }
    }

    /// How far every cell is from a place, in cells.
    pub fn field_to(&self, center: Vec2, radius: f32) -> FlowField {
        FlowField::pack(&self.distances_to(center, radius), self.width)
    }

    /// The same as plain numbers, a cell at a time.
    fn distances_to(&self, center: Vec2, radius: f32) -> Vec<u16> {
        let mut distance = vec![UNREACHED; self.cells.len()];
        let mut queue = VecDeque::new();
        let (ci, cj) = self.cell_of(center);
        let cells = (radius / self.rules.cell).ceil() as i32;
        for j in cj - cells..=cj + cells {
            for i in ci - cells..=ci + cells {
                if let Some(at) = self.index(i, j)
                    && self.cells[at] & BLOCKED == 0
                    && self.center(i, j).distance(center) <= radius
                {
                    distance[at] = 0;
                    queue.push_back((i, j));
                }
            }
        }
        while let Some((i, j)) = queue.pop_front() {
            let here = distance[self.index(i, j).expect("was queued")];
            for (a, b) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if let Some(at) = self.index(a, b)
                    && self.cells[at] & BLOCKED == 0
                    && distance[at] == UNREACHED
                    && self.step_open((i, j), (a, b))
                {
                    distance[at] = here.saturating_add(1);
                    queue.push_back((a, b));
                }
            }
        }
        distance
    }

    /// Whether a step to a cell next door is clear of the wall between them.
    fn step_open(&self, from: (i32, i32), to: (i32, i32)) -> bool {
        let Some(at) = self.index(from.0.min(to.0), from.1.min(to.1)) else { return false };
        let Some(next) = self.index(to.0, to.1) else { return false };
        let bit = if from.1 == to.1 { 1 } else { 2 };
        self.cells[next] & BLOCKED == 0 && self.cells[at] & bit == 0
    }

    /// Whether a straight walk between two points stays on open cells and goes through no wall.
    fn line_open(&self, from: Vec2, to: Vec2) -> bool {
        let steps = (from.distance(to) / (self.rules.cell * 0.4)).ceil().max(1.0) as u32;
        let mut cell = self.cell_of(from);
        for n in 1..=steps {
            let next = self.cell_of(from.lerp(to, n as f32 / steps as f32));
            let corner = (next.0, cell.1);
            let other = (cell.0, next.1);
            let open = match (next.0 != cell.0, next.1 != cell.1) {
                (false, false) => true,
                (true, true) => self.step_open(cell, corner) && self.step_open(corner, next) && self.step_open(cell, other) && self.step_open(other, next),
                _ => self.step_open(cell, next),
            };
            if !open {
                return false;
            }
            cell = next;
        }
        true
    }

    /// A spot a few cells along the way from here, if this place can be reached from here at all: the
    /// furthest along that can be walked to in a straight line, so as not to clip a corner on the way.
    pub fn toward(&self, field: &FlowField, from: Vec2) -> Option<Vec2> {
        let (mut i, mut j) = self.nearest_open(field, from)?;
        let mut spot = self.center(i, j);
        for _ in 0..AHEAD {
            let here = field.get(self.index(i, j)?);
            let mut best = (i, j, here);
            for (da, db) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let Some(at) = self.index(i + da, j + db) else { continue };
                // No cutting a corner: both ways round a diagonal step have to be open.
                let open = if da == 0 || db == 0 {
                    self.step_open((i, j), (i + da, j + db))
                } else {
                    self.step_open((i, j), (i + da, j)) && self.step_open((i + da, j), (i + da, j + db)) && self.step_open((i, j), (i, j + db)) && self.step_open((i, j + db), (i + da, j + db))
                };
                if open && field.get(at) < best.2 {
                    best = (i + da, j + db, field.get(at));
                }
            }
            if best.2 == here {
                break;
            }
            (i, j) = (best.0, best.1);
            if !self.line_open(from, self.center(i, j)) {
                break;
            }
            spot = self.center(i, j);
        }
        Some(spot)
    }

    /// Metres to walk to a place from here, going the way [`Self::toward`] says.
    pub fn walk_length(&self, field: &FlowField, from: Vec2) -> Option<f32> {
        let mut at = from;
        let mut walked = 0.0;
        for _ in 0..self.cells.len() {
            let next = self.toward(field, at)?;
            let step = next.distance(at);
            if step < self.rules.cell {
                return Some(walked + step);
            }
            walked += step;
            at = next;
        }
        None
    }

    /// How far along the way to this place, in metres, from here.
    pub fn metres_to(&self, field: &FlowField, from: Vec2) -> Option<f32> {
        let (i, j) = self.nearest_open(field, from)?;
        Some(field.get(self.index(i, j)?) as f32 * self.rules.cell)
    }

    fn nearest_open(&self, field: &FlowField, from: Vec2) -> Option<(i32, i32)> {
        let (i, j) = self.cell_of(from);
        for reach in 0..=SEARCH {
            let mut best: Option<(i32, i32, u16)> = None;
            for b in j - reach..=j + reach {
                for a in i - reach..=i + reach {
                    if let Some(at) = self.index(a, b)
                        && field.get(at) != UNREACHED
                        && best.is_none_or(|x| field.get(at) < x.2)
                    {
                        best = Some((a, b, field.get(at)));
                    }
                }
            }
            if let Some((a, b, _)) = best {
                return Some((a, b));
            }
        }
        None
    }
}

/// Whether a line between two points crosses a box around the origin, by narrowing down along each axis.
fn crosses_box(a: Vec2, b: Vec2, half: Vec2) -> bool {
    let d = b - a;
    let (mut from, mut to) = (0.0_f32, 1.0_f32);
    for axis in 0..2 {
        if d[axis].abs() < 1e-6 {
            if a[axis].abs() > half[axis] {
                return false;
            }
            continue;
        }
        let (near, far) = ((-half[axis] - a[axis]) / d[axis], (half[axis] - a[axis]) / d[axis]);
        from = from.max(near.min(far));
        to = to.min(near.max(far));
    }
    from <= to
}

/// Cells a side of the squares a field is kept in.
const TILE: usize = 8;
const TILE_CELLS: usize = TILE * TILE;
/// In a square kept small, a cell that can't reach the place.
const SMALL_UNREACHED: u8 = u8::MAX;

/// How far each cell is from one place, in cells. Kept in squares of [`TILE`] cells, each as the least
/// distance in it and a byte a cell for how much more, since next door is only ever a step further. A
/// square that a wall makes differ by more than a byte can hold, which is rare, keeps whole numbers.
/// That is a little over half of two bytes a cell, for exactly the same distances.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowField {
    width: usize,
    tiles_across: usize,
    base: Vec<u16>,
    small: Vec<u8>,
    /// Squares kept whole, by number, in order.
    wide: Vec<(usize, Box<[u16; TILE_CELLS]>)>,
}

impl FlowField {
    fn pack(distance: &[u16], width: i32) -> Self {
        let width = width as usize;
        let tiles_across = width.div_ceil(TILE);
        let tiles = tiles_across * tiles_across;
        let mut field = Self { width, tiles_across, base: vec![UNREACHED; tiles], small: vec![SMALL_UNREACHED; tiles * TILE_CELLS], wide: Vec::new() };
        for tile in 0..tiles {
            let (ti, tj) = (tile % tiles_across, tile / tiles_across);
            let mut whole = [UNREACHED; TILE_CELLS];
            for (k, value) in whole.iter_mut().enumerate() {
                let (i, j) = (ti * TILE + k % TILE, tj * TILE + k / TILE);
                if i < width && j < width {
                    *value = distance[j * width + i];
                }
            }
            let reached = || whole.iter().copied().filter(|&d| d != UNREACHED);
            let (Some(least), Some(most)) = (reached().min(), reached().max()) else { continue };
            if most - least >= SMALL_UNREACHED as u16 {
                field.wide.push((tile, Box::new(whole)));
                continue;
            }
            field.base[tile] = least;
            for (k, value) in whole.iter().enumerate() {
                if *value != UNREACHED {
                    field.small[tile * TILE_CELLS + k] = (value - least) as u8;
                }
            }
        }
        field
    }

    /// The distance at a cell, by its index in the grid.
    fn get(&self, at: usize) -> u16 {
        let (i, j) = (at % self.width, at / self.width);
        let tile = (j / TILE) * self.tiles_across + i / TILE;
        let k = (j % TILE) * TILE + i % TILE;
        if !self.wide.is_empty()
            && let Ok(found) = self.wide.binary_search_by_key(&tile, |(number, _)| *number)
        {
            return self.wide[found].1[k];
        }
        match self.small[tile * TILE_CELLS + k] {
            SMALL_UNREACHED => UNREACHED,
            more => self.base[tile] + more as u16,
        }
    }

    fn bytes(&self) -> usize {
        self.base.len() * 2 + self.small.len() + self.wide.len() * (TILE_CELLS * 2 + 16)
    }
}

/// A map's grid and the way to each of the places on it, in the order of those places.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowMap {
    pub grid: FlowGrid,
    pub fields: Vec<FlowField>,
}

impl FlowMap {
    /// What it all takes up, near enough.
    pub fn bytes(&self) -> usize {
        self.grid.bytes() + self.fields.iter().map(FlowField::bytes).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packed_field_gives_back_exactly_what_went_in() {
        let mut distance: Vec<u16> = (0..37 * 37).map(|at| (at % 37 + at / 37) as u16).collect();
        distance[5] = UNREACHED;
        // A wall that makes next door a long way round, in one square only.
        distance[37 * 20 + 20] = 900;
        let field = FlowField::pack(&distance, 37);
        assert_eq!(field.wide.len(), 1);
        for (at, want) in distance.iter().enumerate() {
            assert_eq!(field.get(at), *want, "cell {at}");
        }
    }

    #[test]
    fn on_a_real_map_packing_changes_no_distance_and_saves_most_of_the_room() {
        let level = Level::named("town").expect("built in");
        let rules = FlowRules { cell: 2.5, margin: 0.4, step: 0.4, headroom: 1.8, max_rise: 1.5, max_dip: f32::INFINITY };
        let grid = FlowGrid::new(&level, rules);
        for point in &level.capture_points {
            let center = Vec2::new(point.center.x, point.center.z);
            let raw = grid.distances_to(center, point.radius);
            let field = grid.field_to(center, point.radius);
            assert!((0..raw.len()).all(|at| field.get(at) == raw[at]));
            assert!(field.bytes() < raw.len() * 2 * 6 / 10, "{} bytes against {}", field.bytes(), raw.len() * 2);
        }
    }
}
