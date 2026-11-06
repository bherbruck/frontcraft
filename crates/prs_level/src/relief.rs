//! The shape and colour of the land, worked out from a [`MapSpec`], and the mesh that follows it.

use crate::spec::{Compound, Ground, Hill, MapSpec};
use crate::{Terrain, smoothstep};
use glam::Vec2;

/// A polyline with its bounds, so that most points never measure a distance to it.
struct Line<'a> {
    points: &'a [Vec2],
    min: Vec2,
    max: Vec2,
}

impl<'a> Line<'a> {
    fn new(points: &'a [Vec2], reach: f32) -> Self {
        let min = points.iter().fold(Vec2::splat(f32::MAX), |m, p| m.min(*p)) - reach;
        let max = points.iter().fold(Vec2::splat(f32::MIN), |m, p| m.max(*p)) + reach;
        Self { points, min, max }
    }

    /// The closest point on the line and how far away it is, if `p` is within the line's reach.
    fn nearest(&self, p: Vec2) -> Option<(Vec2, f32)> {
        if p.x < self.min.x || p.x > self.max.x || p.y < self.min.y || p.y > self.max.y {
            return None;
        }
        let mut best = (self.points[0], f32::MAX);
        for pair in self.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let along = b - a;
            let t = ((p - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
            let on = a + along * t;
            let d = on.distance_squared(p);
            if d < best.1 {
                best = (on, d);
            }
        }
        Some((best.0, best.1.sqrt()))
    }
}

/// A level patch of ground under a compound, easing out into the land around it.
pub(crate) struct Pad {
    pub at: Vec2,
    pub axis: Vec2,
    pub half: Vec2,
    pub level: f32,
    pub ground: Ground,
}

impl Pad {
    fn outside(&self, p: Vec2) -> f32 {
        let d = p - self.at;
        let local = Vec2::new(d.dot(self.axis), d.dot(self.axis.perp()));
        (local.abs() - self.half).max(Vec2::ZERO).length()
    }
}

pub(crate) struct Relief<'a> {
    spec: &'a MapSpec,
    roads: Vec<Line<'a>>,
    ditches: Vec<Line<'a>>,
    pub pads: Vec<Pad>,
}

impl<'a> Relief<'a> {
    pub fn new(spec: &'a MapSpec) -> Self {
        let roads = spec.roads.iter().map(|r| Line::new(&r.points, r.width * 0.5 + r.shoulder)).collect();
        let ditches = spec.ditches.iter().map(|d| Line::new(&d.points, d.width * 0.5)).collect();
        let mut relief = Self { spec, roads, ditches, pads: Vec::new() };
        relief.pads = spec.compounds.iter().map(|c| relief.pad(c)).collect();
        relief
    }

    fn pad(&self, c: &Compound) -> Pad {
        Pad {
            at: c.at,
            axis: Vec2::from_angle(-c.yaw),
            half: c.half + self.spec.style.yard_margin,
            level: self.natural(c.at) + c.lift,
            ground: c.ground,
        }
    }

    /// The land before anything is cut into it or levelled on it.
    fn natural(&self, p: Vec2) -> f32 {
        let terrain = &self.spec.terrain;
        let quiet = terrain.calm.iter().map(|c| c.keep + (1.0 - c.keep) * smoothstep(c.radius, c.fade, c.at.distance(p))).fold(1.0, f32::min);
        let waves: f32 = terrain.waves.iter().map(|w| (w.frequency.dot(p) + w.phase).sin() * w.height).sum();
        let hills: f32 = terrain.hills.iter().map(|h| hill(h, p)).sum();
        (waves + hills) * quiet + self.rim(p)
    }

    fn rim(&self, p: Vec2) -> f32 {
        let terrain = &self.spec.terrain;
        let rim = &terrain.rim;
        let edge = terrain.size * 0.5;
        let reach = p.x.abs().max(p.y.abs());
        let wander = ((p.x + p.y) * 0.013).sin() * rim.wander + ((p.x - p.y) * 0.031).sin() * rim.wander * 0.4;
        let face_from = edge - rim.face;
        let rise = smoothstep(edge - rim.inset + wander, face_from, reach) * rim.height;
        let face = ((reach - face_from) / rim.face).clamp(0.0, 1.0) * rim.face_height;
        rise + face
    }

    pub fn height(&self, p: Vec2) -> f32 {
        let natural = self.natural(p);
        let mut h = natural;
        for (spec, line) in self.spec.ditches.iter().zip(&self.ditches) {
            if let Some((_, d)) = line.nearest(p) {
                let bank = spec.width * 0.5;
                let bed = spec.bed_width * 0.5;
                h -= spec.depth * (1.0 - ((d - bed) / (bank - bed)).clamp(0.0, 1.0));
            }
        }
        for (spec, line) in self.spec.roads.iter().zip(&self.roads) {
            if let Some((on, d)) = line.nearest(p) {
                let edge = spec.width * 0.5;
                let w = 1.0 - smoothstep(edge, edge + spec.shoulder, d);
                if w > 0.0 {
                    h += (self.natural(on) - h) * w;
                }
            }
        }
        // Nearest pad last, so that a pad is level right up to its edge whatever is next door.
        let fade = self.spec.style.pad_fade;
        let mut near: Vec<(f32, f32)> = self.pads.iter().filter_map(|pad| {
            let d = pad.outside(p);
            (d < fade).then_some((1.0 - smoothstep(0.0, fade, d), pad.level))
        }).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (weight, level) in near {
            h += (level - h) * weight;
        }
        h
    }

    pub fn ground(&self, p: Vec2) -> Ground {
        for pad in &self.pads {
            if pad.outside(p) < 0.5 {
                return pad.ground;
            }
        }
        for (spec, line) in self.spec.roads.iter().zip(&self.roads) {
            if line.nearest(p).is_some_and(|(_, d)| d < spec.width * 0.5) {
                return spec.ground;
            }
        }
        for (spec, line) in self.spec.ditches.iter().zip(&self.ditches) {
            if line.nearest(p).is_some_and(|(_, d)| d < spec.width * 0.5) {
                return spec.ground;
            }
        }
        for zone in self.spec.zones.iter().rev() {
            let axis = Vec2::from_angle(-zone.yaw);
            let d = p - zone.at;
            let local = Vec2::new(d.dot(axis), d.dot(axis.perp()));
            if local.x.abs() < zone.half.x && local.y.abs() < zone.half.y {
                return zone.ground;
            }
        }
        Ground::Meadow
    }

    /// Whether something growing at `p` would be in the way of a road, a ditch or a compound.
    pub fn occupied(&self, p: Vec2) -> bool {
        self.pads.iter().any(|pad| pad.outside(p) < 1.0)
            || self.spec.roads.iter().zip(&self.roads).any(|(s, l)| l.nearest(p).is_some_and(|(_, d)| d < s.width * 0.5 + 1.0))
            || self.spec.ditches.iter().zip(&self.ditches).any(|(s, l)| l.nearest(p).is_some_and(|(_, d)| d < s.width * 0.5))
    }

    pub fn terrain(&self) -> Terrain {
        let t = &self.spec.terrain;
        Terrain::adaptive(t.size, t.base_cells, t.max_splits, t.paint_splits, t.tolerance, |p| self.height(p), |p| self.ground(p))
    }
}

fn hill(hill: &Hill, p: Vec2) -> f32 {
    let d = Vec2::from_angle(-hill.yaw).rotate(p - hill.at) / hill.radius;
    let square = d.length_squared();
    if square >= 1.0 { 0.0 } else { hill.height * (1.0 - square) * (1.0 - square) }
}

impl Terrain {
    /// A mesh that is coarse where the land is smooth and fine where it is not: cells split until
    /// they follow `height` to within `tolerance`, and to follow changes of `ground`. Neighbouring
    /// cells differ by one split at most, and larger ones fan out to meet them without any crack.
    pub fn adaptive(
        size: f32,
        base_cells: u32,
        max_splits: u32,
        paint_splits: u32,
        tolerance: f32,
        height: impl Fn(Vec2) -> f32,
        ground: impl Fn(Vec2) -> Ground,
    ) -> Self {
        let grid = Grid::new(size, base_cells, max_splits, height, ground);
        let mut depth = vec![0u8; (grid.n * grid.n) as usize];
        let base = 1 << max_splits;
        for j in 0..base_cells {
            for i in 0..base_cells {
                grid.refine(&mut depth, i * base, j * base, base, 0, max_splits, paint_splits, tolerance);
            }
        }
        balance(&mut depth, grid.n, max_splits);
        grid.mesh(&depth, max_splits)
    }
}

/// The finest lattice a map could be cut to. Heights are asked for once at each corner, at most.
struct Grid<H, G> {
    size: f32,
    n: u32,
    step: f32,
    height_fn: H,
    ground_fn: G,
    heights: std::cell::RefCell<Vec<f32>>,
}

impl<H: Fn(Vec2) -> f32, G: Fn(Vec2) -> Ground> Grid<H, G> {
    fn new(size: f32, base_cells: u32, max_splits: u32, height_fn: H, ground_fn: G) -> Self {
        let n = base_cells << max_splits;
        let heights = std::cell::RefCell::new(vec![f32::NAN; ((n + 1) * (n + 1)) as usize]);
        Self { size, n, step: size / n as f32, height_fn, ground_fn, heights }
    }

    fn point(&self, i: u32, j: u32) -> Vec2 {
        Vec2::new(i as f32 * self.step - self.size * 0.5, j as f32 * self.step - self.size * 0.5)
    }

    fn height(&self, i: u32, j: u32) -> f32 {
        let at = (j * (self.n + 1) + i) as usize;
        let cached = self.heights.borrow()[at];
        if !cached.is_nan() {
            return cached;
        }
        let h = (self.height_fn)(self.point(i, j));
        self.heights.borrow_mut()[at] = h;
        h
    }

    #[allow(clippy::too_many_arguments)]
    fn refine(&self, depth: &mut [u8], i: u32, j: u32, cell: u32, level: u32, max_splits: u32, paint_splits: u32, tolerance: f32) {
        let split = level < max_splits
            && (self.height_error(i, j, cell) > tolerance || (level < paint_splits && self.paint_changes(i, j, cell, level + 1 < paint_splits)));
        if split {
            let half = cell / 2;
            for (a, b) in [(0, 0), (half, 0), (0, half), (half, half)] {
                self.refine(depth, i + a, j + b, half, level + 1, max_splits, paint_splits, tolerance);
            }
        } else {
            for b in j..j + cell {
                for a in i..i + cell {
                    depth[(b * self.n + a) as usize] = level as u8;
                }
            }
        }
    }

    fn height_error(&self, i: u32, j: u32, cell: u32) -> f32 {
        let corners = [self.height(i, j), self.height(i + cell, j), self.height(i, j + cell), self.height(i + cell, j + cell)];
        let mut worst: f32 = 0.0;
        let stride = (cell / 4).max(1);
        for b in (0..=cell).step_by(stride as usize) {
            for a in (0..=cell).step_by(stride as usize) {
                let (u, v) = (a as f32 / cell as f32, b as f32 / cell as f32);
                let flat = (corners[0] * (1.0 - u) + corners[1] * u) * (1.0 - v) + (corners[2] * (1.0 - u) + corners[3] * u) * v;
                worst = worst.max((self.height(i + a, j + b) - flat).abs());
            }
        }
        worst
    }

    /// Whether the ground changes colour in this cell. On the last split only the edges of built
    /// ground count, since a road needs a sharper edge than a field does.
    fn paint_changes(&self, i: u32, j: u32, cell: u32, any_edge: bool) -> bool {
        let first = (self.ground_fn)(self.point(i, j));
        let stride = (cell / 4).max(1) as usize;
        let mut changes = false;
        let mut built = first.is_built();
        for b in (0..=cell).step_by(stride) {
            for a in (0..=cell).step_by(stride) {
                let ground = (self.ground_fn)(self.point(i + a, j + b));
                changes |= ground != first;
                built |= ground.is_built();
            }
        }
        changes && (any_edge || built)
    }

    fn mesh(&self, depth: &[u8], max_splits: u32) -> Terrain {
        let n = self.n;
        let mut index = vec![u32::MAX; ((n + 1) * (n + 1)) as usize];
        let mut vertices = Vec::new();
        let mut ground = Vec::new();
        let mut indices = Vec::new();
        let mut vertex = |i: u32, j: u32| {
            let at = (j * (n + 1) + i) as usize;
            if index[at] == u32::MAX {
                index[at] = vertices.len() as u32;
                let p = self.point(i, j);
                vertices.push(glam::Vec3::new(p.x, self.height(i, j), p.y));
                ground.push((self.ground_fn)(p));
            }
            index[at]
        };
        let at = |i: i64, j: i64| -> Option<u8> {
            (i >= 0 && j >= 0 && i < n as i64 && j < n as i64).then(|| depth[(j * n as i64 + i) as usize])
        };
        let mut j = 0;
        while j < n {
            let mut i = 0;
            while i < n {
                // Cells are visited from their top-left finest square, recognised by alignment.
                let level = depth[(j * n + i) as usize] as u32;
                let cell = 1u32 << (max_splits - level);
                if i % cell != 0 || j % cell != 0 {
                    i += 1;
                    continue;
                }
                let (x0, z0, x1, z1) = (i, j, i + cell, j + cell);
                let deeper = |a: i64, b: i64| at(a, b).is_some_and(|d| d as u32 > level);
                let mid = cell / 2;
                let west = cell > 1 && deeper(x0 as i64 - 1, z0 as i64);
                let south = cell > 1 && deeper(x0 as i64, z1 as i64);
                let east = cell > 1 && deeper(x1 as i64, z0 as i64);
                let north = cell > 1 && deeper(x0 as i64, z0 as i64 - 1);
                if !(west || south || east || north) {
                    let (a, b, c, d) = (vertex(x0, z0), vertex(x1, z0), vertex(x0, z1), vertex(x1, z1));
                    // Alternate the diagonal so facets do not line up into visible stripes.
                    if ((x0 + z0) / cell).is_multiple_of(2) {
                        indices.push([a, c, b]);
                        indices.push([b, c, d]);
                    } else {
                        indices.push([a, c, d]);
                        indices.push([a, d, b]);
                    }
                } else {
                    let centre = vertex(x0 + mid, z0 + mid);
                    let mut ring = vec![vertex(x0, z0)];
                    if west {
                        ring.push(vertex(x0, z0 + mid));
                    }
                    ring.push(vertex(x0, z1));
                    if south {
                        ring.push(vertex(x0 + mid, z1));
                    }
                    ring.push(vertex(x1, z1));
                    if east {
                        ring.push(vertex(x1, z0 + mid));
                    }
                    ring.push(vertex(x1, z0));
                    if north {
                        ring.push(vertex(x0 + mid, z0));
                    }
                    for k in 0..ring.len() {
                        indices.push([centre, ring[k], ring[(k + 1) % ring.len()]]);
                    }
                }
                i += cell;
            }
            j += 1;
        }
        Terrain { size: self.size, cells: n, vertices, indices, ground }
    }
}

/// Splits any cell with a neighbour more than one split finer, so a side only ever has to meet one
/// extra vertex.
fn balance(depth: &mut [u8], n: u32, max_splits: u32) {
    loop {
        let mut changed = false;
        for j in 0..n {
            for i in 0..n {
                let level = depth[(j * n + i) as usize] as u32;
                if level == 0 || level > max_splits {
                    continue;
                }
                let cell = 1u32 << (max_splits - level);
                if !i.is_multiple_of(cell) || !j.is_multiple_of(cell) {
                    continue;
                }
                // Whoever is this deep forces its neighbours to within one level of it.
                let need = level as u8 - 1;
                let (ci, cj) = (i as i64, j as i64);
                let c = cell as i64;
                for (x, z) in [(ci - c, cj), (ci + c, cj), (ci, cj - c), (ci, cj + c)] {
                    if x < 0 || z < 0 || x >= n as i64 || z >= n as i64 {
                        continue;
                    }
                    let (x, z) = (x as u32, z as u32);
                    if depth[(z * n + x) as usize] < need {
                        let old = depth[(z * n + x) as usize] as u32;
                        let parent = 1u32 << (max_splits - old);
                        let (px, pz) = (x / parent * parent, z / parent * parent);
                        let new = old as u8 + 1;
                        for b in pz..pz + parent {
                            for a in px..px + parent {
                                depth[(b * n + a) as usize] = new;
                            }
                        }
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return;
        }
    }
}
