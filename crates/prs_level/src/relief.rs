//! The shape and colour of the land, worked out from a [`MapSpec`], and the mesh that follows it.

use crate::spec::{Compound, Ditch, Ground, Hill, MapSpec};
use crate::{Terrain, smoothstep};
use glam::Vec2;

/// The closest point on a polyline to `p`, and how far away it is, among the segments given by their first
/// point. [`Near`] gives the segments that come within reach of `p`, which are the only ones that can matter.
fn nearest(points: &[Vec2], segments: &[(u32, u32)], p: Vec2) -> Option<(Vec2, f32)> {
    let mut best = None;
    for &(_, k) in segments {
        let (Some(&a), Some(&b)) = (points.get(k as usize), points.get(k as usize + 1)) else { continue };
        let along = b - a;
        let t = ((p - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
        let on = a + along * t;
        let d = on.distance_squared(p);
        if best.is_none_or(|(_, best_d)| d < best_d) {
            best = Some((on, d));
        }
    }
    best.map(|(on, d)| (on, d.sqrt()))
}

/// The entries for each line, a line at a time, in the order of the lines.
fn by_line(entries: &[(u32, u32)]) -> impl Iterator<Item = (usize, &[(u32, u32)])> {
    entries.chunk_by(|a, b| a.0 == b.0).map(|run| (run[0].0 as usize, run))
}

/// What comes near one square of the map: road and ditch segments as (line, segment), and pads, steps and zones,
/// each in the order the map lists them, so that working through them gives what working through them all would.
#[derive(Default)]
struct Nearby {
    roads: Vec<(u32, u32)>,
    ditches: Vec<(u32, u32)>,
    pads: Vec<u32>,
    steps: Vec<u32>,
    zones: Vec<u32>,
}

impl Nearby {
    fn len(&self) -> usize {
        self.roads.len() + self.ditches.len() + self.pads.len() + self.steps.len() + self.zones.len()
    }
}

/// A grid over the map saying what is near each square of it, so a height or a colour looks only at what is
/// near and not at everything on the map. Anywhere off the grid gets everything.
struct Near {
    origin: Vec2,
    cell: f32,
    n: u32,
    cells: Vec<Nearby>,
    everything: Nearby,
}

impl Near {
    /// Squares this many to a side at most, and no smaller than this.
    const MOST_CELLS: f32 = 256.0;
    const LEAST_CELL: f32 = 4.0;

    fn new(size: f32) -> Self {
        let cell = (size / Self::MOST_CELLS).max(Self::LEAST_CELL);
        let n = (size / cell).ceil().max(1.0) as u32;
        let cells = (0..n * n).map(|_| Nearby::default()).collect();
        Self { origin: Vec2::splat(-size * 0.5), cell, n, cells, everything: Nearby::default() }
    }

    /// Every square a box from `min` to `max` touches, for each to be told about something in it.
    fn squares(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = usize> + use<> {
        let at = |v: f32, o: f32| ((v - o) / self.cell).floor();
        let n = self.n as f32;
        let (x0, x1) = (at(min.x, self.origin.x).max(0.0), at(max.x, self.origin.x).min(n - 1.0));
        let (z0, z1) = (at(min.y, self.origin.y).max(0.0), at(max.y, self.origin.y).min(n - 1.0));
        let (row, xs) = (self.n as usize, x0 as usize..=x1 as usize);
        // A box off the grid, or not a box at all, touches nothing. Off the grid, everything is looked at anyway.
        let any = x0 <= x1 && z0 <= z1;
        (z0 as usize..=z1 as usize).filter(move |_| any).flat_map(move |z| xs.clone().map(move |x| z * row + x))
    }

    fn at(&self, p: Vec2) -> &Nearby {
        let (x, z) = (((p.x - self.origin.x) / self.cell).floor(), ((p.y - self.origin.y) / self.cell).floor());
        if x >= 0.0 && z >= 0.0 && x < self.n as f32 && z < self.n as f32 {
            &self.cells[z as usize * self.n as usize + x as usize]
        } else {
            &self.everything
        }
    }
}

/// The bounds of a box turned by `axis`, with `reach` all round.
fn turned_bounds(at: Vec2, axis: Vec2, half: Vec2, reach: f32) -> (Vec2, Vec2) {
    let extent = Vec2::new(axis.x.abs() * half.x + axis.y.abs() * half.y, axis.y.abs() * half.x + axis.x.abs() * half.y) + reach;
    (at - extent, at + extent)
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

/// How far either side of a flight of steps, and how far below its line, the land is cut away for it.
const STEP_CUT_MARGIN: f32 = 0.6;
const STEP_CUT_DEPTH: f32 = 0.3;
/// Nothing grows this close to a road or a compound.
const GROWING_CLEARANCE: f32 = 1.0;
/// How far round a flight of steps nothing grows, so there is room to step on and off.
const STEP_CLEARANCE: f32 = 2.5;

pub(crate) struct Relief<'a> {
    spec: &'a MapSpec,
    /// Features looked at so far in working out heights and ground, and how many may be. Past that every
    /// question gets a flat answer at once, and whoever is building gives up.
    work: std::cell::Cell<u64>,
    limit: u64,
    near: Near,
    pub pads: Vec<Pad>,
    /// The height of the land at each end of each flight of steps, before the steps are cut into it.
    pub step_ends: Vec<(f32, f32)>,
}

impl<'a> Relief<'a> {
    #[cfg(test)]
    pub fn new(spec: &'a MapSpec) -> Self {
        Self::within(spec, u64::MAX)
    }

    /// One that gives up past `limit` features looked at, whatever the spec asks for. Telling the grid what is
    /// near each square counts too.
    pub fn within(spec: &'a MapSpec, limit: u64) -> Self {
        let work = std::cell::Cell::new(0);
        let near = Near::new(spec.terrain.size);
        let mut relief = Self { spec, work, limit, near, pads: Vec::new(), step_ends: Vec::new() };
        relief.pads = spec.compounds.iter().map(|c| relief.pad(c)).collect();
        relief.near = relief.index();
        relief.step_ends = spec.steps.iter().map(|s| (relief.uncut(s.from), relief.uncut(s.to))).collect();
        relief
    }

    /// Which roads, ditches, pads, steps and zones can matter to each square, out as far as any question asks:
    /// past that, none of them changes the height, the colour or whether anything can grow there.
    fn index(&self) -> Near {
        let spec = self.spec;
        let mut near = Near::new(spec.terrain.size);
        let add = |near: &mut Near, (min, max): (Vec2, Vec2), put: &dyn Fn(&mut Nearby)| {
            put(&mut near.everything);
            for square in near.squares(min, max).collect::<Vec<_>>() {
                if !self.spend(0) {
                    return;
                }
                put(&mut near.cells[square]);
            }
        };
        let segments = |points: &[Vec2], reach: f32| -> Vec<(Vec2, Vec2)> {
            points.windows(2).map(|w| (w[0].min(w[1]) - reach, w[0].max(w[1]) + reach)).collect()
        };
        for (i, road) in spec.roads.iter().enumerate() {
            let reach = road.width * 0.5 + road.shoulder.max(GROWING_CLEARANCE);
            for (k, bounds) in segments(&road.points, reach).into_iter().enumerate() {
                add(&mut near, bounds, &|cell| cell.roads.push((i as u32, k as u32)));
            }
        }
        for (i, ditch) in spec.ditches.iter().enumerate() {
            for (k, bounds) in segments(&ditch.points, ditch.width * 0.5).into_iter().enumerate() {
                add(&mut near, bounds, &|cell| cell.ditches.push((i as u32, k as u32)));
            }
        }
        let fade = spec.style.pad_fade.max(GROWING_CLEARANCE);
        for (i, pad) in self.pads.iter().enumerate() {
            add(&mut near, turned_bounds(pad.at, pad.axis, pad.half, fade), &|cell| cell.pads.push(i as u32));
        }
        for (i, steps) in spec.steps.iter().enumerate() {
            // Room to get on and off reaches half a flight past each end.
            let reach = steps.width * 0.5 + STEP_CUT_MARGIN.max(STEP_CLEARANCE) + steps.from.distance(steps.to) * 0.5;
            add(&mut near, (steps.from.min(steps.to) - reach, steps.from.max(steps.to) + reach), &|cell| cell.steps.push(i as u32));
        }
        for (i, zone) in spec.zones.iter().enumerate() {
            add(&mut near, turned_bounds(zone.at, Vec2::from_angle(-zone.yaw), zone.half, 0.0), &|cell| cell.zones.push(i as u32));
        }
        near
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

    /// Counts the cost of a question. False once past the limit.
    fn spend(&self, cost: usize) -> bool {
        let work = self.work.get().saturating_add(cost as u64 + 1);
        self.work.set(work);
        work <= self.limit
    }

    /// Whether working out the land took more than it was allowed.
    pub fn spent(&self) -> bool {
        self.work.get() > self.limit
    }

    /// The land before anything is cut into it or levelled on it.
    fn natural(&self, p: Vec2) -> f32 {
        let terrain = &self.spec.terrain;
        if !self.spend(terrain.calm.len() + terrain.waves.len() + terrain.hills.len()) {
            return 0.0;
        }
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
        let near = self.near.at(p);
        if !self.spend(near.steps.len()) {
            return 0.0;
        }
        let mut h = self.uncut(p);
        // Steps are let into a bank, so no tread is buried under a slope steeper than they are.
        for &i in &near.steps {
            let (steps, (start, end)) = (&self.spec.steps[i as usize], self.step_ends[i as usize]);
            let along = steps.to - steps.from;
            let length = along.length();
            if length < 0.1 {
                continue;
            }
            let dir = along / length;
            let d = p - steps.from;
            let (t, across) = (d.dot(dir) / length, d.dot(dir.perp()).abs());
            if (0.0..=1.0).contains(&t) && across < steps.width * 0.5 + STEP_CUT_MARGIN {
                h = h.min(start + (end - start) * t - STEP_CUT_DEPTH);
            }
        }
        h
    }

    fn uncut(&self, p: Vec2) -> f32 {
        let near = self.near.at(p);
        if !self.spend(near.len()) {
            return 0.0;
        }
        let spec = self.spec;
        let natural = self.natural(p);
        let mut h = natural;
        let cut = |ditch: &Ditch, d: f32| {
            let bank = ditch.width * 0.5;
            let bed = ditch.bed_width * 0.5;
            ditch.depth * (1.0 - ((d - bed) / (bank - bed)).clamp(0.0, 1.0))
        };
        let ditches = || by_line(&near.ditches).map(|(i, run)| (&spec.ditches[i], run));
        for (ditch, run) in ditches().filter(|(d, _)| !d.cuts_roads) {
            if let Some((_, d)) = nearest(&ditch.points, run, p) {
                h -= cut(ditch, d);
            }
        }
        for (i, run) in by_line(&near.roads) {
            let road = &spec.roads[i];
            if let Some((on, d)) = nearest(&road.points, run, p) {
                let edge = road.width * 0.5;
                let w = 1.0 - smoothstep(edge, edge + road.shoulder, d);
                if w > 0.0 {
                    h += (self.natural(on) - h) * w;
                }
            }
        }
        // A canal goes through roads, so a road over it needs a bridge. Only where the canal is: past its banks
        // the land is as the roads and everything else left it.
        for (ditch, run) in ditches().filter(|(d, _)| d.cuts_roads) {
            if let Some((_, d)) = nearest(&ditch.points, run, p).filter(|&(_, d)| d < ditch.width * 0.5) {
                h = h.min(natural - cut(ditch, d));
            }
        }
        // Nearest pad last, so that a pad is level right up to its edge whatever is next door.
        let fade = spec.style.pad_fade;
        let mut levels: Vec<(f32, f32)> = near.pads.iter().map(|&i| &self.pads[i as usize]).filter_map(|pad| {
            let d = pad.outside(p);
            (d < fade).then_some((1.0 - smoothstep(0.0, fade, d), pad.level))
        }).collect();
        levels.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (weight, level) in levels {
            h += (level - h) * weight;
        }
        h
    }

    pub fn ground(&self, p: Vec2) -> Ground {
        let near = self.near.at(p);
        if !self.spend(near.len()) {
            return Ground::Meadow;
        }
        let spec = self.spec;
        if let Some(pad) = near.pads.iter().map(|&i| &self.pads[i as usize]).find(|pad| pad.outside(p) < 0.5) {
            return pad.ground;
        }
        let within = |points: &[Vec2], run: &[(u32, u32)], width: f32| nearest(points, run, p).is_some_and(|(_, d)| d < width * 0.5);
        let ditches = || by_line(&near.ditches).map(|(i, run)| (&spec.ditches[i], run));
        if let Some((ditch, _)) = ditches().filter(|(d, _)| d.cuts_roads).find(|(d, run)| within(&d.points, run, d.width)) {
            return ditch.ground;
        }
        if let Some(road) = by_line(&near.roads).map(|(i, run)| (&spec.roads[i], run)).find(|(r, run)| within(&r.points, run, r.width)) {
            return road.0.ground;
        }
        if let Some((ditch, _)) = ditches().find(|(d, run)| within(&d.points, run, d.width)) {
            return ditch.ground;
        }
        for &i in near.zones.iter().rev() {
            let zone = &spec.zones[i as usize];
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
        let near = self.near.at(p);
        let spec = self.spec;
        // Past the limit, everywhere is taken, so nothing more is put anywhere.
        !self.spend(near.len())
            || near.pads.iter().any(|&i| self.pads[i as usize].outside(p) < GROWING_CLEARANCE)
            || by_line(&near.roads).any(|(i, run)| nearest(&spec.roads[i].points, run, p).is_some_and(|(_, d)| d < spec.roads[i].width * 0.5 + GROWING_CLEARANCE))
            || by_line(&near.ditches).any(|(i, run)| nearest(&spec.ditches[i].points, run, p).is_some_and(|(_, d)| d < spec.ditches[i].width * 0.5))
            || near.steps.iter().map(|&i| &spec.steps[i as usize]).any(|s| {
                // The steps, and room to get on and off them at either end.
                let along = s.to - s.from;
                let t = ((p - s.from).dot(along) / along.length_squared().max(1e-6)).clamp(-0.5, 1.5);
                (s.from + along * t).distance(p) < s.width * 0.5 + STEP_CLEARANCE
            })
    }

    /// The mesh, unless it would be more than `max_cells` cells or shaping it would take more than allowed.
    pub fn terrain(&self, max_cells: u64) -> Result<Terrain, String> {
        let t = &self.spec.terrain;
        let detail = |p: Vec2| {
            // Past the limit, everywhere is detailed: the strict tolerance, and no more looking.
            let open = self.spend(t.detail.len()) && !t.detail.iter().any(|(at, radius)| at.distance(p) < *radius);
            match (open, t.open_tolerance) {
                (true, Some(tolerance)) => (tolerance, t.open_paint_splits.unwrap_or(t.paint_splits).min(t.paint_splits)),
                _ => (t.tolerance, t.paint_splits),
            }
        };
        let terrain = Terrain::adaptive(t.size, t.base_cells, t.max_splits, detail, max_cells, |p| self.height(p), |p| self.ground(p))?;
        if self.spent() {
            return Err(format!("more than {} looks at the land to shape it", self.limit));
        }
        Ok(terrain)
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
    /// cells differ by one split at most, and larger ones fan out to meet them without any crack. Refused if
    /// that comes to more than `max_cells` cells, before any of the mesh is made.
    #[allow(clippy::too_many_arguments)]
    pub fn adaptive(
        size: f32,
        base_cells: u32,
        max_splits: u32,
        detail: impl Fn(Vec2) -> (f32, u32),
        max_cells: u64,
        height: impl Fn(Vec2) -> f32,
        ground: impl Fn(Vec2) -> Ground,
    ) -> Result<Self, String> {
        let grid = Grid::new(size, base_cells, max_splits, height, ground);
        let mut depth = vec![0u8; (grid.n * grid.n) as usize];
        let base = 1 << max_splits;
        for j in 0..base_cells {
            for i in 0..base_cells {
                grid.refine(&mut depth, i * base, j * base, base, 0, max_splits, &detail);
            }
        }
        balance(&mut depth, grid.n, max_splits);
        // Each finest cell is a share of the cell that covers it: a quarter of it for each split short of the finest.
        let cells: f64 = depth.iter().map(|&d| 0.25_f64.powi((max_splits - (d as u32).min(max_splits)) as i32)).sum();
        if !(cells <= max_cells as f64) {
            return Err(format!("ground of {cells:.0} cells"));
        }
        Ok(grid.mesh(&depth, max_splits))
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
    /// `detail` says, for a place, the height error a cell there may leave and how many times it may split to
    /// follow the ground's colour.
    fn refine(&self, depth: &mut [u8], i: u32, j: u32, cell: u32, level: u32, max_splits: u32, detail: &impl Fn(Vec2) -> (f32, u32)) {
        let (tolerance, paint_splits) = detail(self.point(i + cell / 2, j + cell / 2));
        let split = level < max_splits
            && (self.height_error(i, j, cell) > tolerance || (level < paint_splits && self.paint_changes(i, j, cell, level + 1 < paint_splits)));
        if split {
            let half = cell / 2;
            for (a, b) in [(0, 0), (half, 0), (0, half), (half, half)] {
                self.refine(depth, i + a, j + b, half, level + 1, max_splits, detail);
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

#[cfg(test)]
mod near_tests {
    use super::*;

    /// Looking only at what the grid says is near gives exactly what looking at everything does, everywhere.
    #[test]
    fn the_grid_changes_nothing_but_how_much_is_looked_at() {
        for spec in [MapSpec::town(), MapSpec::claudabad()] {
            let near = Relief::new(&spec);
            let mut everywhere = Relief::new(&spec);
            // With no squares, every question is off the grid and looks at everything.
            everywhere.near.n = 0;
            let half = spec.terrain.size * 0.5;
            let step = spec.terrain.size / 397.0;
            let mut asked = 0;
            let mut y = -half;
            while y <= half {
                let mut x = -half;
                while x <= half {
                    let p = Vec2::new(x, y);
                    assert_eq!(near.height(p).to_bits(), everywhere.height(p).to_bits(), "height at {p}");
                    assert_eq!(near.ground(p), everywhere.ground(p), "ground at {p}");
                    assert_eq!(near.occupied(p), everywhere.occupied(p), "room at {p}");
                    asked += 1;
                    x += step;
                }
                y += step;
            }
            assert!(asked > 150_000);
            assert!(near.work.get() * 3 < everywhere.work.get(), "{} against {}", near.work.get(), everywhere.work.get());
        }
    }
}
