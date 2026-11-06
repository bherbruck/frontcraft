//! Turns a [`MapSpec`] into a [`Level`]: walls with their openings cut out, floors with their
//! stairwells, roofs, stairs, trees and everything else as plain boxes.

use crate::maps::MapLimits;
use crate::relief::Relief;
use crate::spec::*;
use crate::{CapturePoint, Doorway, Level, Paint, Room, Scatter, Solid, SpawnPoint, Staircase, TargetMark, Team, Tree, Window};
use glam::{Quat, Vec2, Vec3};

/// A place and a heading that local coordinates hang off.
#[derive(Clone, Copy)]
struct Frame {
    origin: Vec3,
    rotation: Quat,
}

impl Frame {
    fn child(&self, at: Vec2, yaw: f32) -> Frame {
        Frame { origin: self.point(Vec3::new(at.x, 0.0, at.y)), rotation: self.rotation * Quat::from_rotation_y(yaw) }
    }

    fn point(&self, p: Vec3) -> Vec3 {
        self.origin + self.rotation * p
    }

    fn direction(&self, d: Vec3) -> Vec3 {
        self.rotation * d
    }
}

/// A hole in a straight run of wall, measured along the run and up from its base.
#[derive(Clone, Copy)]
struct Hole {
    at: f32,
    width: f32,
    sill: f32,
    head: f32,
}

/// A wall along one axis of a frame.
#[derive(Clone, Copy)]
struct Run {
    along_x: bool,
    /// Where it sits on the other axis.
    across: f32,
    from: f32,
    to: f32,
    base: f32,
    height: f32,
    thickness: f32,
    /// How far the solid parts carry on below the base.
    foot: f32,
    paint: Paint,
}

#[derive(Clone, Copy)]
struct Rect {
    min: Vec2,
    max: Vec2,
}

/// How far a bridge's deck stands over the ground at its ends.
const DECK_LIP: f32 = 0.12;
/// How far a quay's top stands over the ground beside it, how far it stays from a bridge or steps, and the
/// sharpest bend its blocks run on round.
const QUAY_LIP: f32 = 0.05;
const QUAY_GAP: f32 = 0.5;
const QUAY_BEND: f32 = 2.0;

/// How far `p` is from the segment `a` to `b`.
fn near_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + along * t).distance(p)
}

/// The most one flat length of a bridge's deck stands above or below the next, and the shortest a length may
/// be, which bounds how many lengths a bridge takes whatever the ground under its ends.
const DECK_STEP: f32 = 0.15;
pub(crate) const DECK_LENGTH: f32 = 0.5;

/// How much building is left: one step for every turn of every loop the builder takes. A map that would take
/// more stops being built, whatever its spec says, and is refused.
pub(crate) struct Budget {
    left: u64,
    spent: bool,
}

impl Budget {
    pub fn new(steps: u64) -> Self {
        Self { left: steps, spent: false }
    }

    /// One more step. False once there are none left, and from then on every loop gives up.
    pub fn take(&mut self) -> bool {
        if self.left == 0 {
            self.spent = true;
            return false;
        }
        self.left -= 1;
        true
    }
}

impl MapSpec {
    /// The level, for a spec made in code, which is trusted to be a real map.
    pub fn build(&self) -> Level {
        self.build_within(u64::MAX).expect("a map made in code fits")
    }

    /// The level, unless building it would take more than `steps`.
    pub fn build_within(&self, steps: u64) -> Result<Level, String> {
        let limits = MapLimits { max_build_steps: steps, max_terrain_work: u64::MAX, max_terrain_cells: u64::MAX, ..MapLimits::default() };
        self.build_limited(&limits)
    }

    /// The level, unless shaping its land or building on it would take more than the limits allow, whatever
    /// the spec says.
    pub fn build_limited(&self, limits: &MapLimits) -> Result<Level, String> {
        let steps = limits.max_build_steps;
        let relief = Relief::within(self, limits.max_terrain_work);
        let terrain = relief.terrain(limits.max_terrain_cells)?;
        let mut level = Level {
            terrain,
            solids: Vec::new(),
            trees: Vec::new(),
            targets: Vec::new(),
            target_marks: Vec::new(),
            spawn: Vec3::ZERO,
            spawn_yaw: 0.0,
            teams: Vec::new(),
            capture_points: Vec::new(),
            rooms: Vec::new(),
            doorways: Vec::new(),
            windows: Vec::new(),
            staircases: Vec::new(),
            sentry: SpawnPoint { pos: Vec3::ZERO, yaw: 0.0 },
            helipads: Vec::new(),
            vehicles: Vec::new(),
            source: 0,
        };
        let mason = Mason { style: &self.style, level: &mut level, budget: Budget::new(steps) };
        let mut builder = Builder { spec: self, relief: &relief, mason, scatter: Scatter(self.seed) };
        builder.compounds();
        builder.bridges();
        builder.quays();
        builder.steps();
        builder.landmarks();
        builder.field_walls();
        builder.rubble();
        builder.props();
        builder.woods();
        builder.objectives();
        if builder.mason.budget.spent {
            return Err(format!("more than {steps} steps to build"));
        }
        if relief.spent() {
            return Err(format!("more than {} looks at the land to build on it", limits.max_terrain_work));
        }
        Ok(level)
    }
}

struct Builder<'a> {
    spec: &'a MapSpec,
    relief: &'a Relief<'a>,
    mason: Mason<'a>,
    scatter: Scatter,
}

/// Puts up buildings, walls and props on a level from their specs, wherever the level came from.
pub(crate) struct Mason<'a> {
    pub style: &'a BuildStyle,
    pub level: &'a mut Level,
    pub budget: Budget,
}

impl Builder<'_> {
    fn compounds(&mut self) {
        for (index, compound) in self.spec.compounds.iter().enumerate() {
            if !self.mason.budget.take() {
                return;
            }
            self.mason.compound(compound, self.relief.pads[index].level);
        }
    }

    fn field_walls(&mut self) {
        let s = *self.mason.style;
        for wall in &self.spec.walls {
            if !self.mason.budget.take() {
                return;
            }
            let mut walked = 0.0;
            for pair in wall.points.windows(2) {
                if !self.mason.budget.take() {
                    return;
                }
                let (a, b) = (pair[0], pair[1]);
                let length = a.distance(b);
                let dir = (b - a) / length;
                let pieces = (length / wall.segment).ceil().max(1.0) as u32;
                let piece = length / pieces as f32;
                for i in 0..pieces {
                    if !self.mason.budget.take() {
                        return;
                    }
                    let from = walked + piece * i as f32;
                    let to = from + piece;
                    let jitter = self.scatter.range(-wall.unevenness, wall.unevenness);
                    let stray = dir.perp() * self.scatter.range(-wall.wander, wall.wander);
                    if wall.gaps.iter().any(|(g0, g1)| from < *g1 && to > *g0) {
                        continue;
                    }
                    let mid = a + dir * (piece * (i as f32 + 0.5)) + stray;
                    let ends = [a + dir * (piece * i as f32), a + dir * (piece * (i + 1) as f32)];
                    let low = ends.iter().map(|p| self.relief.height(*p)).fold(f32::MAX, f32::min);
                    let bottom = low - s.foot;
                    let top = self.relief.height(mid) + wall.height + jitter;
                    let yaw = (-dir.y).atan2(dir.x);
                    self.mason.level.solids.push(Solid {
                        center: Vec3::new(mid.x, (top + bottom) * 0.5, mid.y),
                        half: Vec3::new(piece * 0.5 + 0.02, (top - bottom) * 0.5, wall.thickness * 0.5),
                        rotation: Quat::from_rotation_y(yaw),
                        paint: wall.paint,
                    });
                }
                walked += length;
            }
        }
    }

    fn rubble(&mut self) {
        for heap in &self.spec.rubble {
            if !self.mason.budget.take() {
                return;
            }
            for _ in 0..heap.count {
                if !self.mason.budget.take() {
                    return;
                }
                let angle = self.scatter.range(0.0, std::f32::consts::TAU);
                let reach = self.scatter.range(0.0, 1.0).sqrt() * heap.radius;
                let p = heap.at + Vec2::from_angle(angle) * reach;
                let s = self.scatter.range(heap.size.0, heap.size.1);
                // Larger towards the middle of the heap, so it builds up rather than scatters.
                let swell = 1.0 + (1.0 - reach / heap.radius) * 0.6;
                let half = Vec3::new(s, s * self.scatter.range(0.45, 0.85), s * self.scatter.range(0.7, 1.2)) * swell;
                let rotation = Quat::from_euler(
                    glam::EulerRot::YXZ,
                    self.scatter.range(0.0, std::f32::consts::TAU),
                    self.scatter.range(-0.4, 0.4),
                    self.scatter.range(-0.4, 0.4),
                );
                let center = Vec3::new(p.x, self.relief.height(p) + half.y * 0.35, p.y);
                self.mason.level.solids.push(Solid { center, half, rotation, paint: heap.paint });
            }
        }
    }

    fn quays(&mut self) {
        let spec = self.spec;
        for (index, ditch) in spec.ditches.iter().enumerate() {
            let Some(quay) = ditch.quay else {
                continue;
            };
            let (inner, outer) = (ditch.bed_width * 0.5 - quay.over_bed, ditch.width * 0.5 + quay.beyond);
            let dirs: Vec<Vec2> = ditch.points.windows(2).map(|w| (w[1] - w[0]).normalize_or_zero()).collect();
            // At a bend, the blocks on its outside run on past the corner far enough to meet.
            let reach = |from: Vec2, to: Vec2, side: f32| {
                let turn = from.perp_dot(to).atan2(from.dot(to));
                if side * turn < 0.0 { outer * (turn.abs().min(QUAY_BEND) * 0.5).tan() } else { 0.0 }
            };
            for (k, pair) in ditch.points.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let (length, dir) = (a.distance(b), dirs[k]);
                if length < 0.1 {
                    continue;
                }
                let blocks = (length / quay.length).ceil().max(1.0) as u32;
                let block = length / blocks as f32;
                let rotation = Quat::from_rotation_y((-dir.y).atan2(dir.x));
                for i in 0..blocks {
                    if !self.mason.budget.take() {
                        return;
                    }
                    for side in [-1.0, 1.0] {
                        let before = if i == 0 && k > 0 { reach(dirs[k - 1], dir, side) } else { 0.0 };
                        let after = if i + 1 == blocks && k + 1 < dirs.len() { reach(dir, dirs[k + 1], side) } else { 0.0 };
                        let (start, end) = (block * i as f32 - before, block * (i + 1) as f32 + after);
                        let mid = a + dir * ((start + end) * 0.5);
                        let across = dir.perp() * side;
                        let middle = mid + across * (inner + outer) * 0.5;
                        let clear = |from: Vec2, to: Vec2, width: f32| near_segment(middle, from, to) > width * 0.5 + (end - start) * 0.5 + QUAY_GAP;
                        // Open to a bridge or steps, and not across any other water.
                        let open = spec.bridges.iter().any(|b| !clear(b.from, b.to, b.width))
                            || spec.steps.iter().any(|s| !clear(s.from, s.to, s.width))
                            || spec.ditches.iter().enumerate().any(|(other, d)| other != index && d.cuts_roads && d.points.windows(2).any(|w| near_segment(middle, w[0], w[1]) < d.width * 0.5));
                        if open {
                            continue;
                        }
                        let top = self.relief.height(mid + across * outer) + QUAY_LIP;
                        let bottom = self.relief.height(mid) - self.mason.style.foot;
                        let center = Vec3::new(middle.x, (top + bottom) * 0.5, middle.y);
                        let half = Vec3::new((end - start) * 0.5 + 0.02, (top - bottom).max(0.1) * 0.5, (outer - inner) * 0.5);
                        self.mason.level.solids.push(Solid { center, half, rotation, paint: quay.paint });
                    }
                }
            }
        }
    }

    fn bridges(&mut self) {
        for bridge in &self.spec.bridges {
            if !self.mason.budget.take() {
                return;
            }
            let along = bridge.to - bridge.from;
            let length = along.length();
            if length < 0.1 || bridge.span <= 0.0 {
                continue;
            }
            let dir = along / length;
            // A lip over the ground at each end, so the deck reads as the deck there and not the road.
            let (start, end) = (self.relief.height(bridge.from) + DECK_LIP, self.relief.height(bridge.to) + DECK_LIP);
            let rotation = Quat::from_rotation_y((-dir.y).atan2(dir.x));
            // Each span in as many flat lengths as keep the step between them small enough to walk and drive.
            let spans = (length / bridge.span).ceil().max(1.0) as u32;
            // However steep, no length shorter than DECK_LENGTH: past that the steps grow instead.
            let fit = (length / spans as f32 / DECK_LENGTH).floor().max(1.0);
            let per_span = ((end - start).abs() / spans as f32 / DECK_STEP).ceil().clamp(1.0, fit) as u32;
            let Some(pieces) = spans.checked_mul(per_span) else {
                self.mason.budget.spent = true;
                return;
            };
            let piece = length / pieces as f32;
            let (t, side) = (bridge.thickness, bridge.width * 0.5);
            let rail = self.mason.style.partition;
            for i in 0..pieces {
                if !self.mason.budget.take() {
                    return;
                }
                let share = (i as f32 + 0.5) / pieces as f32;
                let mid = bridge.from + dir * (piece * (i as f32 + 0.5));
                let top = start + (end - start) * share;
                let at = |up: f32, out: f32| Vec3::new(mid.x, top + up, mid.y) + rotation * Vec3::new(0.0, 0.0, out);
                let solid = |center: Vec3, half: Vec3| Solid { center, half, rotation, paint: bridge.paint };
                self.mason.level.solids.push(solid(at(-t * 0.5, 0.0), Vec3::new(piece * 0.5 + 0.02, t * 0.5, side)));
                if bridge.parapet > 0.0 {
                    for out in [side - rail * 0.5, -side + rail * 0.5] {
                        self.mason.level.solids.push(solid(at(bridge.parapet * 0.5, out), Vec3::new(piece * 0.5 + 0.02, bridge.parapet * 0.5, rail * 0.5)));
                    }
                }
                // A pier under every span's join, down to whatever is below.
                if i > 0 && i % per_span == 0 {
                    let join = bridge.from + dir * (piece * i as f32);
                    let below = self.relief.height(join) - self.mason.style.foot;
                    let bottom = start + (end - start) * (i as f32 / pieces as f32) - t;
                    if bottom - below > 0.3 {
                        let center = Vec3::new(join.x, (bottom + below) * 0.5, join.y);
                        self.mason.level.solids.push(solid(center, Vec3::new(t, (bottom - below) * 0.5, side * 0.85)));
                    }
                }
            }
        }
    }

    fn steps(&mut self) {
        let style = *self.mason.style;
        for (steps, &(start, end)) in self.spec.steps.iter().zip(&self.relief.step_ends) {
            if !self.mason.budget.take() {
                return;
            }
            let along = steps.to - steps.from;
            let length = along.length();
            if length < 0.1 || style.stair_rise <= 0.0 {
                continue;
            }
            let dir = along / length;
            let rotation = Quat::from_rotation_y((-dir.y).atan2(dir.x));
            let count = ((end - start).abs() / style.stair_rise).ceil().max(1.0) as u32;
            let tread = length / count as f32;
            for i in 0..count {
                if !self.mason.budget.take() {
                    return;
                }
                let mid = steps.from + dir * (tread * (i as f32 + 0.5));
                let top = start + (end - start) * (i as f32 + 0.5) / count as f32;
                let bottom = self.relief.height(mid).min(top) - style.foot;
                let center = Vec3::new(mid.x, (top + bottom) * 0.5, mid.y);
                let half = Vec3::new(tread * 0.5 + 0.01, (top - bottom) * 0.5, steps.width * 0.5);
                self.mason.level.solids.push(Solid { center, half, rotation, paint: steps.paint });
            }
        }
    }

    fn landmarks(&mut self) {
        for mark in &self.spec.landmarks {
            if !self.mason.budget.take() {
                return;
            }
            let ground = self.relief.height(mark.at);
            let turn = Quat::from_rotation_y(mark.yaw);
            let base = Vec3::new(mark.at.x, ground, mark.at.y);
            let foot = self.mason.style.foot;
            let plinth = Vec3::new(mark.plinth.x, mark.plinth.y + foot * 0.5, mark.plinth.z);
            let solid = |center: Vec3, half: Vec3, rotation: Quat, paint: Paint| Solid { center, half, rotation, paint };
            self.mason.level.solids.push(solid(base + Vec3::Y * (plinth.y - foot), plinth, turn, mark.stone));
            let column_base = mark.plinth.y * 2.0;
            self.mason.level.solids.push(solid(base + Vec3::Y * (column_base + mark.column.y), mark.column, turn, mark.stone));
            let hub = base + Vec3::Y * (column_base + mark.column.y * 2.0 + mark.hub);
            let diamond = turn * Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
            self.mason.level.solids.push(solid(hub, Vec3::new(mark.hub, mark.hub, mark.ray_depth * 0.6), diamond, mark.paint));
            for k in 0..mark.rays {
                if !self.mason.budget.take() {
                    return;
                }
                let angle = k as f32 / mark.rays.max(1) as f32 * std::f32::consts::TAU;
                let short = mark.short_every > 0 && k % mark.short_every == 0;
                let length = mark.ray_length * if short { mark.short_share } else { 1.0 };
                let out = turn * Vec3::new(angle.cos(), angle.sin(), 0.0);
                let center = hub + out * (mark.hub + length * 0.5);
                let half = Vec3::new(length * 0.5, mark.ray_width * 0.5, mark.ray_depth * 0.5);
                self.mason.level.solids.push(solid(center, half, turn * Quat::from_rotation_z(angle), mark.paint));
            }
        }
    }

    fn props(&mut self) {
        for placed in &self.spec.props {
            if !self.mason.budget.take() {
                return;
            }
            let y = self.relief.height(placed.at);
            let frame = Frame { origin: Vec3::new(placed.at.x, y, placed.at.y), rotation: Quat::from_rotation_y(placed.yaw) };
            self.mason.place_prop(&frame, placed.prop);
        }
    }

    fn woods(&mut self) {
        for wood in &self.spec.woods {
            if !self.mason.budget.take() {
                return;
            }
            match &wood.shape {
                WoodShape::Line { points, spacing, jitter } => {
                    for pair in points.windows(2) {
                        if !self.mason.budget.take() {
                            return;
                        }
                        let (a, b) = (pair[0], pair[1]);
                        let count = (a.distance(b) / spacing).ceil().max(1.0) as u32;
                        for i in 0..count {
                            if !self.mason.budget.take() {
                                return;
                            }
                            let p = a.lerp(b, (i as f32 + self.scatter.range(0.2, 0.8)) / count as f32);
                            let shift = Vec2::new(self.scatter.range(-*jitter, *jitter), self.scatter.range(-*jitter, *jitter));
                            self.tree(wood, p + shift);
                        }
                    }
                }
                WoodShape::Clump { at, half, yaw, count } => {
                    for _ in 0..*count {
                        if !self.mason.budget.take() {
                            return;
                        }
                        let local = Vec2::new(self.scatter.range(-half.x, half.x), self.scatter.range(-half.y, half.y));
                        self.tree(wood, *at + Vec2::from_angle(-*yaw).rotate(local));
                    }
                }
                WoodShape::Rows { at, half, yaw, spacing } => {
                    let (nx, nz) = ((half.x * 2.0 / spacing) as i32, (half.y * 2.0 / spacing) as i32);
                    for j in 0..=nz {
                        if !self.mason.budget.take() {
                            return;
                        }
                        for i in 0..=nx {
                            if !self.mason.budget.take() {
                                return;
                            }
                            let local = Vec2::new(i as f32 * spacing - half.x, j as f32 * spacing - half.y);
                            self.tree(wood, *at + Vec2::from_angle(-*yaw).rotate(local));
                        }
                    }
                }
            }
        }
    }

    fn tree(&mut self, wood: &Woodland, p: Vec2) {
        let height = self.scatter.range(wood.height.0, wood.height.1);
        let radius = self.scatter.range(wood.radius.0, wood.radius.1);
        if self.relief.occupied(p) {
            return;
        }
        self.mason.level.trees.push(Tree { base: Vec3::new(p.x, self.relief.height(p) - 0.25, p.y), height, radius, kind: wood.kind });
    }

    fn objectives(&mut self) {
        for team in &self.spec.teams {
            if !self.mason.budget.take() {
                return;
            }
            let spawns = team
                .spawns
                .iter()
                .map(|s| SpawnPoint { pos: Vec3::new(s.at.x, self.relief.height(s.at) + 0.1, s.at.y), yaw: s.yaw })
                .collect();
            self.mason.level.teams.push(Team { name: team.name.clone(), spawns, base_radius: team.base_radius });
        }
        for point in &self.spec.capture_points {
            if !self.mason.budget.take() {
                return;
            }
            self.mason.level.capture_points.push(CapturePoint {
                name: point.name.clone(),
                center: Vec3::new(point.at.x, self.relief.height(point.at), point.at.y),
                radius: point.radius,
            });
        }
        let sentry = self.spec.sentry;
        self.mason.level.sentry = SpawnPoint { pos: Vec3::new(sentry.at.x, self.relief.height(sentry.at) + 0.1, sentry.at.y), yaw: sentry.yaw };
        for pad in &self.spec.helipads {
            if !self.mason.budget.take() {
                return;
            }
            self.mason.level.helipads.push(SpawnPoint { pos: Vec3::new(pad.at.x, self.relief.height(pad.at), pad.at.y), yaw: pad.yaw });
        }
        for &(kind, at) in &self.spec.vehicles {
            if !self.mason.budget.take() {
                return;
            }
            let pos = Vec3::new(at.at.x, self.relief.height(at.at), at.at.y);
            self.mason.level.vehicles.push(crate::VehicleSpawn { kind, pos, yaw: at.yaw });
        }
        if let Some(first) = self.mason.level.teams.first().and_then(|t| t.spawns.first()).copied() {
            self.mason.level.spawn = first.pos;
            self.mason.level.spawn_yaw = first.yaw;
        }
    }
}

impl Mason<'_> {
    fn solid(&mut self, frame: &Frame, center: Vec3, half: Vec3, paint: Paint) {
        self.level.solids.push(Solid { center: frame.point(center), half, rotation: frame.rotation, paint });
    }

    /// A compound standing on level ground at `ground`.
    pub fn compound(&mut self, compound: &Compound, ground: f32) {
        let at = compound.at;
        let frame = Frame { origin: Vec3::new(at.x, ground, at.y), rotation: Quat::from_rotation_y(compound.yaw) };
        if let Some(wall) = &compound.wall {
            self.yard_wall(&frame, compound.half, wall);
        }
        for building in &compound.buildings {
            if !self.budget.take() {
                return;
            }
            self.building(&frame, building);
        }
        for placed in &compound.props {
            if !self.budget.take() {
                return;
            }
            self.place_prop(&frame.child(placed.at, placed.yaw), placed.prop);
        }
    }

    fn yard_wall(&mut self, frame: &Frame, half: Vec2, wall: &YardWall) {
        let t = self.style.yard_wall;
        for side in [Side::N, Side::S, Side::E, Side::W] {
            let run = self.side_run(side, half, t, 0.0, wall.height, self.style.foot, wall.paint);
            let holes: Vec<Hole> = wall
                .gates
                .iter()
                .filter(|g| g.side == side)
                .map(|g| Hole { at: g.along, width: g.width, sill: 0.0, head: wall.height })
                .collect();
            self.wall_run(frame, run, &holes);
            for gate in wall.gates.iter().filter(|g| g.side == side) {
                if !self.budget.take() {
                    return;
                }
                self.doorway(frame, side, half, t, gate.along, 0.0, gate.width);
            }
        }
    }

    /// The run of wall along one outer side. North and south walls take the corners.
    #[allow(clippy::too_many_arguments)]
    fn side_run(&self, side: Side, half: Vec2, thickness: f32, base: f32, height: f32, foot: f32, paint: Paint) -> Run {
        let (along_x, across, from, to) = match side {
            Side::N => (true, -half.y + thickness * 0.5, -half.x, half.x),
            Side::S => (true, half.y - thickness * 0.5, -half.x, half.x),
            Side::E => (false, half.x - thickness * 0.5, -half.y + thickness, half.y - thickness),
            Side::W => (false, -half.x + thickness * 0.5, -half.y + thickness, half.y - thickness),
        };
        Run { along_x, across, from, to, base, height, thickness, foot, paint }
    }

    /// Cuts holes out of a run of wall: solid piers between them, with a sill below and a lintel above.
    fn wall_run(&mut self, frame: &Frame, run: Run, holes: &[Hole]) {
        let mut sorted = holes.to_vec();
        sorted.sort_by(|a, b| a.at.total_cmp(&b.at));
        let mut cursor = run.from;
        for hole in &sorted {
            if !self.budget.take() {
                return;
            }
            let (left, right) = (hole.at - hole.width * 0.5, hole.at + hole.width * 0.5);
            self.wall_piece(frame, run, cursor, left, run.base - run.foot, run.base + run.height);
            if hole.sill > 0.0 {
                self.wall_piece(frame, run, left, right, run.base - run.foot, run.base + hole.sill);
            }
            if hole.head < run.height {
                self.wall_piece(frame, run, left, right, run.base + hole.head, run.base + run.height);
            }
            cursor = right;
        }
        self.wall_piece(frame, run, cursor, run.to, run.base - run.foot, run.base + run.height);
    }

    fn wall_piece(&mut self, frame: &Frame, run: Run, from: f32, to: f32, bottom: f32, top: f32) {
        if to - from < 0.01 || top - bottom < 0.01 {
            return;
        }
        let (mid, half) = ((from + to) * 0.5, (to - from) * 0.5);
        let (y, half_y) = ((bottom + top) * 0.5, (top - bottom) * 0.5);
        let t = run.thickness * 0.5;
        if run.along_x {
            self.solid(frame, Vec3::new(mid, y, run.across), Vec3::new(half, half_y, t), run.paint);
        } else {
            self.solid(frame, Vec3::new(run.across, y, mid), Vec3::new(t, half_y, half), run.paint);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn doorway(&mut self, frame: &Frame, side: Side, half: Vec2, thickness: f32, along: f32, floor: f32, width: f32) {
        let n = side.normal();
        let p = match side {
            Side::N | Side::S => Vec3::new(along, floor, n.y * (half.y - thickness * 0.5)),
            Side::E | Side::W => Vec3::new(n.x * (half.x - thickness * 0.5), floor, along),
        };
        self.level.doorways.push(Doorway {
            at: frame.point(p),
            outward: frame.direction(Vec3::new(n.x, 0.0, n.y)),
            width,
        });
    }

    fn building(&mut self, parent: &Frame, b: &Building) {
        let frame = parent.child(b.at, b.yaw);
        let s = *self.style;
        let storey = b.storey_height.unwrap_or(s.storey);
        let floor = s.plinth;
        let top = floor + storey * b.storeys as f32;

        // The ground floor and the foundation under it, in one block.
        let bottom = -s.foot;
        self.solid(
            &frame,
            Vec3::Y * ((floor + bottom) * 0.5),
            Vec3::new(b.half.x + 0.04, (floor - bottom) * 0.5, b.half.y + 0.04),
            b.floors,
        );

        for level in 0..b.storeys {
            if !self.budget.take() {
                return;
            }
            let base = floor + storey * level as f32;
            let foot = if level == 0 { s.foot } else { 0.0 };
            for side in [Side::N, Side::S, Side::E, Side::W] {
                let run = self.side_run(side, b.half, s.wall, base, storey, foot, b.walls);
                let openings = b.openings.iter().filter(|o| o.side == side && o.storey == level);
                let holes: Vec<Hole> = openings.clone().map(|o| self.opening_hole(o, storey)).collect();
                self.wall_run(&frame, run, &holes);
                for opening in openings {
                    if !self.budget.take() {
                        return;
                    }
                    self.record_opening(&frame, b, opening, base);
                }
            }
            let inner = b.half - Vec2::splat(s.wall);
            let height = storey - if level + 1 == b.storeys && matches!(b.roof, Roof::Gable { .. }) { 0.0 } else { s.slab };
            self.level.rooms.push(Room {
                center: frame.point(Vec3::new(0.0, base + height * 0.5, 0.0)),
                half: Vec3::new(inner.x, height * 0.5, inner.y),
                rotation: frame.rotation,
            });
            if level + 1 < b.storeys {
                let holes = self.stairwells(b, level + 1, storey);
                self.slab(&frame, b, base + storey, inner, &holes);
            }
        }

        for p in &b.partitions {
            if !self.budget.take() {
                return;
            }
            self.partition(&frame, p, floor + storey * p.storey as f32, storey);
        }
        for flight in &b.flights {
            if !self.budget.take() {
                return;
            }
            self.flight(&frame, flight, storey, b.stairs);
        }
        self.place_parts(&frame, &b.fixtures);
        for t in &b.targets {
            if !self.budget.take() {
                return;
            }
            let pos = frame.point(Vec3::new(t.at.x, floor + storey * t.storey as f32, t.at.y));
            let forward = frame.direction(Vec3::new(t.facing.sin(), 0.0, t.facing.cos()));
            // Marks go by the same index as targets, and plain targets before these may have none.
            self.level.target_marks.resize(self.level.targets.len(), TargetMark::default());
            self.level.targets.push(pos);
            self.level.target_marks.push(TargetMark { yaw: forward.x.atan2(forward.z), kind: t.kind, in_room: true });
        }

        match b.roof {
            Roof::Flat { paint } => {
                let inner = b.half - Vec2::splat(s.wall);
                let holes = self.stairwells(b, b.storeys, storey);
                let roof = Building { floors: paint, ..b.clone() };
                self.slab(&frame, &roof, top, inner, &holes);
                self.parapet(&frame, b, top);
            }
            Roof::Gable { paint, pitch } => self.gable_roof(&frame, b, top, paint, pitch),
        }
    }

    fn opening_hole(&self, opening: &Opening, storey: f32) -> Hole {
        let s = self.style;
        let (width, sill, head) = match opening.kind {
            OpeningKind::Door => (s.door_width, 0.0, s.door_head),
            OpeningKind::WideDoor => (s.wide_door_width, 0.0, s.wide_door_head),
            OpeningKind::Window => (s.window_width, s.window_sill, s.window_head),
        };
        Hole { at: opening.along, width, sill, head: head.min(storey - s.slab) }
    }

    fn record_opening(&mut self, frame: &Frame, b: &Building, opening: &Opening, base: f32) {
        let s = *self.style;
        let width = if opening.kind == OpeningKind::WideDoor { s.wide_door_width } else { s.door_width };
        if opening.kind != OpeningKind::Window {
            self.doorway(frame, opening.side, b.half, s.wall, opening.along, base, width);
            return;
        }
        let n = opening.side.normal();
        let y = base + (s.window_sill + s.window_head) * 0.5;
        let p = match opening.side {
            Side::N | Side::S => Vec3::new(opening.along, y, n.y * (b.half.y - s.wall * 0.5)),
            Side::E | Side::W => Vec3::new(n.x * (b.half.x - s.wall * 0.5), y, opening.along),
        };
        self.level.windows.push(Window {
            at: frame.point(p),
            outward: frame.direction(Vec3::new(n.x, 0.0, n.y)),
            width: s.window_width,
            height: s.window_head - s.window_sill,
        });
    }

    /// Holes in the floor that flights come up through.
    fn stairwells(&self, b: &Building, floor: u32, storey: f32) -> Vec<Rect> {
        b.flights
            .iter()
            .filter(|f| f.to == floor && matches!(f.finish, FlightFinish::Through { .. }))
            .map(|f| self.stairwell(f, storey))
            .collect()
    }

    fn stairwell(&self, f: &Flight, storey: f32) -> Rect {
        let s = self.style;
        let dir = f.ascent.normal();
        let steps = self.step_count(storey);
        let run = steps as f32 * s.stair_run;
        let rise = storey / steps as f32;
        // Where someone climbing first needs the floor above out of their way.
        let free = ((storey - s.slab - s.headroom) / rise).floor().max(0.0) * s.stair_run;
        let a = f.foot + dir * free;
        let b = f.foot + dir * run;
        let side = dir.perp().abs() * (s.stair_width * 0.5 + 0.05);
        Rect { min: a.min(b) - side, max: a.max(b) + side }
    }

    fn step_count(&self, height: f32) -> u32 {
        (height / self.style.stair_rise).ceil() as u32
    }

    /// A floor plate with rectangular holes, cut into as few boxes as columns between the holes allow.
    fn slab(&mut self, frame: &Frame, b: &Building, top: f32, inner: Vec2, holes: &[Rect]) {
        let t = self.style.slab;
        let mut xs = vec![-inner.x, inner.x];
        for h in holes {
            if !self.budget.take() {
                return;
            }
            xs.push(h.min.x.clamp(-inner.x, inner.x));
            xs.push(h.max.x.clamp(-inner.x, inner.x));
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.windows(2) {
            if !self.budget.take() {
                return;
            }
            let (x0, x1) = (pair[0], pair[1]);
            if x1 - x0 < 0.01 {
                continue;
            }
            let mid = (x0 + x1) * 0.5;
            let mut cuts: Vec<(f32, f32)> = holes.iter().filter(|h| h.min.x < mid && h.max.x > mid).map(|h| (h.min.y, h.max.y)).collect();
            cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut z = -inner.y;
            for (from, to) in cuts {
                if !self.budget.take() {
                    return;
                }
                self.slab_piece(frame, b, x0, x1, z, from.clamp(-inner.y, inner.y), top, t);
                z = z.max(to.clamp(-inner.y, inner.y));
            }
            self.slab_piece(frame, b, x0, x1, z, inner.y, top, t);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn slab_piece(&mut self, frame: &Frame, b: &Building, x0: f32, x1: f32, z0: f32, z1: f32, top: f32, t: f32) {
        if z1 - z0 < 0.01 {
            return;
        }
        self.solid(
            frame,
            Vec3::new((x0 + x1) * 0.5, top - t * 0.5, (z0 + z1) * 0.5),
            Vec3::new((x1 - x0) * 0.5, t * 0.5, (z1 - z0) * 0.5),
            b.floors,
        );
    }

    fn partition(&mut self, frame: &Frame, p: &Partition, base: f32, storey: f32) {
        let s = *self.style;
        let along_x = (p.to.x - p.from.x).abs() > (p.to.y - p.from.y).abs();
        let (across, from, to) = if along_x {
            (p.from.y, p.from.x.min(p.to.x), p.from.x.max(p.to.x))
        } else {
            (p.from.x, p.from.y.min(p.to.y), p.from.y.max(p.to.y))
        };
        let run = Run {
            along_x,
            across,
            from,
            to,
            base,
            height: storey - s.slab,
            thickness: s.partition,
            foot: 0.0,
            paint: Paint::Plaster,
        };
        let holes: Vec<Hole> = p.doors.iter().map(|d| Hole { at: from + d, width: s.door_width, sill: 0.0, head: s.door_head }).collect();
        self.wall_run(frame, run, &holes);
        for d in &p.doors {
            if !self.budget.take() {
                return;
            }
            let local = if along_x { Vec3::new(from + d, base, across) } else { Vec3::new(across, base, from + d) };
            let out = if along_x { Vec3::Z } else { Vec3::X };
            self.level.doorways.push(Doorway {
                at: frame.point(local),
                outward: frame.direction(out),
                width: s.door_width,
            });
        }
    }

    fn flight(&mut self, frame: &Frame, f: &Flight, storey: f32, paint: Paint) {
        let s = *self.style;
        let floor0 = s.plinth;
        let base = f.from.map_or(0.0, |level| floor0 + storey * level as f32);
        let top = floor0 + storey * f.to as f32;
        let steps = self.step_count(top - base);
        let rise = (top - base) / steps as f32;
        let dir = f.ascent.normal();
        let run = steps as f32 * s.stair_run;
        let half_run = s.stair_run * 0.5;
        let half_width = s.stair_width * 0.5;
        let foot = if f.from.is_none() { s.foot } else { 0.0 };

        for i in 0..steps {
            if !self.budget.take() {
                return;
            }
            let tread = rise * (i + 1) as f32;
            let at = f.foot + dir * (s.stair_run * i as f32 + half_run);
            let half_y = (tread + foot) * 0.5;
            let half = if dir.x != 0.0 { Vec3::new(half_run, half_y, half_width) } else { Vec3::new(half_width, half_y, half_run) };
            self.solid(frame, Vec3::new(at.x, base + tread - half_y, at.y), half, paint);
        }

        let foot_stand = f.foot - dir * 1.0;
        let top_tread = f.foot + dir * (run - half_run);
        let mut path = vec![frame.point(Vec3::new(foot_stand.x, base, foot_stand.y)), frame.point(Vec3::new(top_tread.x, top, top_tread.y))];

        match f.finish {
            FlightFinish::Through { open_right, open_left } => {
                let beyond = f.foot + dir * (run + 0.9);
                path.push(frame.point(Vec3::new(beyond.x, top, beyond.y)));
                self.stair_rails(frame, f, storey, top, open_right, open_left);
            }
            FlightFinish::Landing { exit } => {
                let landing = f.foot + dir * (run + half_width);
                let half_y = (top - base + foot) * 0.5;
                self.solid(frame, Vec3::new(landing.x, base + (top - base) - half_y, landing.y), Vec3::new(half_width, half_y, half_width), paint);
                let onto = landing + exit.normal() * (s.stair_width + s.wall + 0.5);
                path.push(frame.point(Vec3::new(landing.x, top, landing.y)));
                path.push(frame.point(Vec3::new(onto.x, top, onto.y)));
            }
        }
        self.level.staircases.push(Staircase { path });
    }

    fn stair_rails(&mut self, frame: &Frame, f: &Flight, storey: f32, floor_y: f32, right: bool, left: bool) {
        let s = *self.style;
        let well = self.stairwell(f, storey);
        let h = s.rail * 0.5;
        let t = s.partition * 0.5;
        let y = floor_y + h;
        let dir = f.ascent.normal();
        // Seen climbing, with yaw zero facing -z, right is +x: so right is the ascent turned that way.
        let right_dir = Vec2::new(-dir.y, dir.x);
        let mid = (well.min + well.max) * 0.5;
        let half = (well.max - well.min) * 0.5;

        let edge = |this: &mut Self, at: Vec2, along_x: bool, length: f32| {
            let half_box = if along_x { Vec3::new(length, h, t) } else { Vec3::new(t, h, length) };
            this.solid(frame, Vec3::new(at.x, y, at.y), half_box, Paint::Plaster);
        };
        // Across the end you would otherwise walk off into the stairs.
        let back = mid - dir * dir.abs().dot(half);
        edge(self, back, dir.x == 0.0, if dir.x == 0.0 { half.x } else { half.y });
        for (open, sign) in [(right, 1.0_f32), (left, -1.0)] {
            if open {
                let at = mid + right_dir * sign * right_dir.abs().dot(half);
                edge(self, at, dir.x != 0.0, if dir.x != 0.0 { half.x } else { half.y });
            }
        }
    }

    fn parapet(&mut self, frame: &Frame, b: &Building, top: f32) {
        let s = *self.style;
        for side in [Side::N, Side::S, Side::E, Side::W] {
            let run = self.side_run(side, b.half, s.wall, top, s.parapet, 0.0, b.walls);
            let holes: Vec<Hole> = b
                .flights
                .iter()
                .filter_map(|f| match f.finish {
                    FlightFinish::Landing { exit } if exit.opposite() == side => {
                        let steps = self.step_count(top);
                        let landing = f.foot + f.ascent.normal() * (steps as f32 * s.stair_run + s.stair_width * 0.5);
                        let along = if matches!(side, Side::N | Side::S) { landing.x } else { landing.y };
                        Some(Hole { at: along, width: s.stair_width, sill: 0.0, head: s.parapet })
                    }
                    _ => None,
                })
                .collect();
            self.wall_run(frame, run, &holes);
        }
    }

    /// Two tilted plates meeting at a ridge along x, with the gable ends closed by plates of wall
    /// that follow the roof line down and stay inside the wall below it.
    fn gable_roof(&mut self, frame: &Frame, b: &Building, top: f32, paint: Paint, pitch: f32) {
        let s = *self.style;
        let (sin, cos) = pitch.sin_cos();
        let span = b.half.y;
        let ridge = Vec3::new(0.0, top + span * pitch.tan(), 0.0);
        let thick = s.roof_thickness;
        let overhang = s.roof_overhang;
        for sign in [1.0_f32, -1.0] {
            let tilt = Quat::from_rotation_x(pitch * sign);
            let down = Vec3::new(0.0, -sin, cos * sign);
            let up = Vec3::new(0.0, cos, sin * sign);
            let tilted = Frame { origin: frame.origin, rotation: frame.rotation * tilt };
            let length = (span + overhang) / cos;
            // Runs just past the ridge so the two plates close over it.
            let center = ridge + down * (length * 0.5 - 0.06) + up * thick;
            let local = tilt.conjugate() * center;
            self.solid(&tilted, local, Vec3::new(b.half.x + overhang, thick, length * 0.5 + 0.06), paint);

            let plate_length = span / cos;
            let plate_half = span * sin * 0.5;
            let t = s.wall * 0.5 - 0.015;
            for end in [1.0_f32, -1.0] {
                let x = end * (b.half.x - s.wall * 0.5);
                let center = Vec3::new(x, ridge.y, 0.0) + down * (plate_length * 0.5) - up * plate_half;
                let local = tilt.conjugate() * center;
                self.solid(&tilted, local, Vec3::new(t, plate_half, plate_length * 0.5), b.walls);
            }
        }
    }

    fn place_prop(&mut self, frame: &Frame, prop: Prop) {
        self.place_parts(frame, prop.parts());
    }

    fn place_parts(&mut self, frame: &Frame, parts: &[Part]) {
        for &(at, half, yaw, paint) in parts {
            if !self.budget.take() {
                return;
            }
            let part = frame.child(Vec2::ZERO, yaw);
            let center = frame.point(Vec3::from(at));
            self.level.solids.push(Solid { center, half: Vec3::from(half), rotation: part.rotation, paint });
        }
    }
}
