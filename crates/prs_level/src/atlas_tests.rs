//! Holds the maps of real places to what any map promises: everyone can get everywhere, on foot and by road,
//! nothing is built through anything else, and every stair and door works.

use crate::atlas::{apart, corners};
use crate::relief::Relief;
use crate::spec::*;
use crate::{Level, MapData, MapSpec};
use glam::{Vec2, Vec3};
use prs_collision::{Capsule, CollisionWorld};
use prs_movement::{MoveConfig, MoveEvent, MoveInput, MoveState, step};
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Instant;

const DT: f32 = 1.0 / 60.0;

pub(crate) struct Map {
    pub name: &'static str,
    pub spec: MapSpec,
    pub level: Level,
    pub world: CollisionWorld,
}

pub(crate) fn muttrah() -> &'static Map {
    static MAP: OnceLock<Map> = OnceLock::new();
    MAP.get_or_init(|| load("muttrah", MapSpec::muttrah()))
}

pub(crate) fn fallujah() -> &'static Map {
    static MAP: OnceLock<Map> = OnceLock::new();
    MAP.get_or_init(|| load("fallujah", MapSpec::fallujah()))
}

fn load(name: &'static str, spec: MapSpec) -> Map {
    let level = Level::named(name).expect("the map is built in and fits the usual limits");
    let world = level.collision();
    Map { name, spec, level, world }
}

fn cfg() -> MoveConfig {
    MoveConfig::default()
}

fn body(c: &MoveConfig) -> Capsule {
    Capsule { height: c.stand_height, radius: c.radius }
}

/// Walks a soldier towards each point in turn. Returns where they got stuck, if they did.
fn walk(start: Vec3, path: &[Vec3], c: &MoveConfig, w: &CollisionWorld) -> Result<MoveState, String> {
    let mut state = MoveState::new(start + Vec3::Y * 0.05, c);
    let mut events: Vec<MoveEvent> = Vec::new();
    for _ in 0..60 {
        step(&mut state, c, &MoveInput::default(), w, DT, &mut events);
    }
    for target in path {
        let mut ticks = 0;
        loop {
            let to = *target - state.pos;
            let flat = Vec2::new(to.x, to.z);
            if flat.length() < 0.35 && to.y.abs() < 0.6 {
                break;
            }
            ticks += 1;
            if ticks > 60 * 25 {
                return Err(format!("stuck at {} on the way to {target}", state.pos));
            }
            let yaw = (-flat.x).atan2(-flat.y);
            let input = MoveInput { wish: Vec2::Y, yaw, ..MoveInput::default() };
            events.clear();
            step(&mut state, c, &input, w, DT, &mut events);
        }
    }
    Ok(state)
}

/// Every `n`th of a list, so a big map's thousands of doors are sampled evenly.
fn every<T>(items: &[T], most: usize) -> impl Iterator<Item = (usize, &T)> {
    let n = items.len().div_ceil(most.max(1)).max(1);
    items.iter().enumerate().step_by(n)
}

/// A grid over the map of where a soldier can stand, joined where one can walk from cell to cell, searched
/// from a start. `None` for cells never reached, or the metres walked to get there.
pub(crate) struct Reach {
    cell: f32,
    half: f32,
    n: i32,
    metres: Vec<Option<f32>>,
}

impl Reach {
    fn from(map: &Map, start: Vec2, cell: f32, slope: f32, clearance: f32, dip: f32) -> Self {
        let relief = Relief::new(&map.spec);
        let half = map.spec.terrain.size * 0.5 - cell;
        let n = (half * 2.0 / cell) as i32;
        let index = |i: i32, j: i32| (j * n + i) as usize;
        let point = |i: i32, j: i32| Vec2::new(i as f32 * cell - half, j as f32 * cell - half);
        let probe = Capsule { height: 1.8, radius: clearance };
        let heights: Vec<f32> = (0..n * n).map(|k| relief.height(point(k % n, k / n))).collect();
        let open: Vec<bool> = (0..n * n)
            .map(|k| {
                let p = point(k % n, k / n);
                map.world.capsule_is_clear(Vec3::new(p.x, heights[k as usize] + 0.45, p.y), probe)
            })
            .collect();
        let mut metres = vec![None; (n * n) as usize];
        let at = ((start + Vec2::splat(half)) / cell).round();
        let (si, sj) = ((at.x as i32).clamp(0, n - 1), (at.y as i32).clamp(0, n - 1));
        let mut queue = VecDeque::new();
        // Starting from anywhere open near the start, in case it is on the edge of something.
        for (a, b) in (-2..=2).flat_map(|a| (-2..=2).map(move |b| (a, b))) {
            let (i, j) = (si + a, sj + b);
            if i >= 0 && j >= 0 && i < n && j < n && open[index(i, j)] {
                metres[index(i, j)] = Some(0.0);
                queue.push_back((i, j));
            }
        }
        while let Some((i, j)) = queue.pop_front() {
            let here = metres[index(i, j)].unwrap();
            let y = heights[index(i, j)];
            for (a, b) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if a < 1 || b < 1 || a >= n - 1 || b >= n - 1 || metres[index(a, b)].is_some() || !open[index(a, b)] {
                    continue;
                }
                let ny = heights[index(a, b)];
                if (ny - y).abs() > slope * cell {
                    continue;
                }
                // Into a dip deeper than a wheel will go.
                let (pa, pb) = (heights[index(2 * a - i, 2 * b - j)], y);
                if (pa + pb) * 0.5 - ny > dip {
                    continue;
                }
                metres[index(a, b)] = Some(here + cell);
                queue.push_back((a, b));
            }
        }
        Self { cell, half, n, metres }
    }

    /// On foot: the slope a soldier can walk, and their own width.
    pub fn on_foot(map: &Map, start: Vec2) -> Self {
        let c = cfg();
        let slope = (1.0 - c.max_slope_cos * c.max_slope_cos).sqrt() / c.max_slope_cos;
        Self::from(map, start, 2.0, slope, c.radius, f32::INFINITY)
    }

    /// By truck, as the bots' convoys judge it: room either side, a climb a truck makes, and no ditches.
    pub fn by_road(map: &Map, start: Vec2) -> Self {
        Self::from(map, start, 2.5, 0.4, 1.15, 0.3)
    }

    /// Metres to the nearest reached cell within `radius` of `p`.
    pub fn to(&self, p: Vec2, radius: f32) -> Option<f32> {
        let at = (p + Vec2::splat(self.half)) / self.cell;
        let r = (radius / self.cell).ceil() as i32;
        let (ci, cj) = (at.x.round() as i32, at.y.round() as i32);
        let mut best: Option<f32> = None;
        for j in cj - r..=cj + r {
            for i in ci - r..=ci + r {
                if i < 0 || j < 0 || i >= self.n || j >= self.n {
                    continue;
                }
                if let Some(m) = self.metres[(j * self.n + i) as usize] {
                    best = Some(best.map_or(m, |b: f32| b.min(m)));
                }
            }
        }
        best
    }
}

fn base_of(map: &Map, team: usize) -> Vec2 {
    let c = map.level.teams[team].base_center();
    Vec2::new(c.x, c.z)
}

fn flat(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

pub(crate) fn report(map: &Map) {
    let l = &map.level;
    let buildings: usize = map.spec.compounds.iter().map(|c| c.buildings.len()).sum();
    let text = MapData::built_in(map.name).unwrap().text;
    println!(
        "{}: file {} KB, compounds {}, buildings {buildings}, solids {}, trees {}, rooms {}, doors {}, windows {}, stairs {}, terrain tris {}, verts {}",
        map.name,
        text.len() / 1024,
        map.spec.compounds.len(),
        l.solids.len(),
        l.trees.len(),
        l.rooms.len(),
        l.doorways.len(),
        l.windows.len(),
        l.staircases.len(),
        l.terrain.indices.len(),
        l.terrain.vertices.len()
    );
    let start = Instant::now();
    let _ = MapData::built_in(map.name).unwrap().level().unwrap();
    println!("load and build from the file: {:?}", start.elapsed());
    let start = Instant::now();
    let _ = l.collision();
    println!("build collision: {:?}", start.elapsed());
    let walk = Reach::on_foot(map, base_of(map, 0));
    let road = Reach::by_road(map, base_of(map, 0));
    let walk_east = Reach::on_foot(map, base_of(map, 1));
    let road_east = Reach::by_road(map, base_of(map, 1));
    for point in &l.capture_points {
        let p = flat(point.center);
        let show = |m: Option<f32>| m.map_or("-".to_owned(), |m| format!("{m:.0}"));
        println!(
            "  {:14} walk {:>5} / {:>5}  drive {:>5} / {:>5}",
            point.name,
            show(walk.to(p, point.radius)),
            show(walk_east.to(p, point.radius)),
            show(road.to(p, point.radius)),
            show(road_east.to(p, point.radius))
        );
    }
}

#[test]
#[ignore]
fn muttrah_report() {
    report(muttrah());
}

#[test]
#[ignore]
fn fallujah_report() {
    report(fallujah());
}

/// The checks every map of a real place passes.
pub(crate) fn holds_up(map: &Map) {
    let c = cfg();
    let w = &map.world;
    let l = &map.level;
    let mut failed = Vec::new();
    // Spawns: clear, on the ground, not steep.
    assert_eq!(l.teams.len(), 2);
    for team in &l.teams {
        assert!(team.spawns.len() >= 8, "{}", team.name);
        for spawn in &team.spawns {
            if !w.capsule_is_clear(spawn.pos, body(&c)) {
                failed.push(format!("{} spawn at {} is blocked", team.name, spawn.pos));
                continue;
            }
            match w.cast_capsule(spawn.pos, body(&c), Vec3::NEG_Y, 2.0) {
                Some(hit) if hit.distance < 0.3 && hit.normal.y > c.max_slope_cos => {}
                hit => failed.push(format!("{} spawn at {} stands badly: {hit:?}", team.name, spawn.pos)),
            }
        }
    }
    // Points: in the open, with nothing overhead.
    assert!((5..=7).contains(&l.capture_points.len()), "{} points", l.capture_points.len());
    for point in &l.capture_points {
        let feet = point.center + Vec3::Y * 0.1;
        if !w.capsule_is_clear(feet, body(&c)) || w.raycast(feet + Vec3::Y * 0.2, Vec3::Y, 60.0).is_some() {
            failed.push(format!("{} is not in the open", point.name));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Nothing is built through anything else: no two compounds, and no two buildings in one, overlap.
pub(crate) fn nothing_overlaps(spec: &MapSpec) {
    let mut clashes = Vec::new();
    let boxes: Vec<[Vec2; 4]> = spec.compounds.iter().map(|c| corners(c.at, c.yaw, c.half)).collect();
    for (i, a) in boxes.iter().enumerate() {
        for (j, b) in boxes.iter().enumerate().skip(i + 1) {
            if !apart(a, b) {
                clashes.push(format!("compounds at {} and {}", spec.compounds[i].at, spec.compounds[j].at));
            }
        }
    }
    for compound in &spec.compounds {
        let footprint = |b: &Building| {
            let stairs = if b.flights.iter().any(|f| f.from.is_none()) { spec.style.stair_width } else { 0.0 };
            corners(b.at + Vec2::new(stairs * 0.5, 0.0), b.yaw, b.half + Vec2::new(stairs * 0.5, 0.0))
        };
        for (i, a) in compound.buildings.iter().enumerate() {
            for b in compound.buildings.iter().skip(i + 1) {
                if !apart(&footprint(a), &footprint(b)) {
                    clashes.push(format!("buildings at {} and {} in the compound at {}", a.at, b.at, compound.at));
                }
            }
            if !footprint(a).iter().all(|c| c.x.abs() < compound.half.x && c.y.abs() < compound.half.y) {
                clashes.push(format!("building at {} outside the compound at {}", a.at, compound.at));
            }
        }
    }
    assert!(clashes.is_empty(), "{} clashes:\n{}", clashes.len(), clashes.join("\n"));
}

pub(crate) fn stairs_climb(map: &Map, most: usize) {
    let c = cfg();
    let mut failed = Vec::new();
    let all = &map.level.staircases;
    assert!(!all.is_empty());
    for (i, stairs) in every(all, most) {
        let end = *stairs.path.last().unwrap();
        match walk(stairs.path[0], &stairs.path[1..], &c, &map.world) {
            Ok(state) if (state.pos.y - end.y).abs() <= 0.3 => {}
            Ok(state) => failed.push(format!("stairs {i}: ended at {} not {end}", state.pos)),
            Err(why) => failed.push(format!("stairs {i} at {}: {why}", stairs.path[0])),
        }
    }
    assert!(failed.is_empty(), "{} of {}:\n{}", failed.len(), all.len(), failed.join("\n"));
}

pub(crate) fn doors_work(map: &Map, most: usize) {
    let c = cfg();
    let mut failed = Vec::new();
    for (i, door) in every(&map.level.doorways, most) {
        // A lip of ground at a gate, where the land's mesh is coarser than the wall, is stepped over like a kerb.
        if !map.world.capsule_is_clear(door.at + Vec3::Y * 0.2, body(&c)) {
            failed.push(format!("door {i} at {} is blocked", door.at));
            continue;
        }
        let (out, inside) = (door.at + door.outward * 1.2, door.at - door.outward * 1.2);
        for (from, to) in [(out, inside), (inside, out)] {
            if let Err(why) = walk(from + Vec3::Y * 0.3, &[to], &c, &map.world) {
                failed.push(format!("door {i} at {}: {why}", door.at));
                break;
            }
        }
    }
    assert!(failed.is_empty(), "{} of {}:\n{}", failed.len(), map.level.doorways.len(), failed.join("\n"));
}

pub(crate) fn helipads_are_clear(map: &Map) {
    let w = &map.world;
    assert_eq!(map.level.helipads.len(), 2);
    for pad in &map.level.helipads {
        let height = |x: f32, z: f32| {
            let top = Vec3::new(pad.pos.x + x, pad.pos.y + 20.0, pad.pos.z + z);
            top.y - w.raycast(top, Vec3::NEG_Y, 40.0).expect("ground under a helipad").distance
        };
        for (x, z) in [(-1.3, -2.0), (1.3, -2.0), (-1.3, 1.7), (1.3, 1.7), (0.0, 0.0)] {
            assert!((height(x, z) - pad.pos.y).abs() < 0.35, "not flat at {} near {x} {z}", pad.pos);
        }
        for i in 0..48 {
            let angle = i as f32 / 48.0 * std::f32::consts::TAU;
            let out = Vec3::new(angle.cos(), 0.0, angle.sin());
            for above in [2.5, 4.0, 5.5] {
                assert!(w.raycast(pad.pos + Vec3::Y * above, out, 9.5).is_none(), "something {above} m up near the pad at {}", pad.pos);
            }
        }
    }
}

/// Both bases get to every point on foot and by road, and to each other. That both sides have as far to walk
/// up the chain is held by `prs_ai`'s test of the bots' own way across the map.
pub(crate) fn everyone_gets_everywhere(map: &Map) {
    let l = &map.level;
    let mut missing = Vec::new();
    for team in 0..2 {
        let walk = Reach::on_foot(map, base_of(map, team));
        let road = Reach::by_road(map, base_of(map, team));
        for point in &l.capture_points {
            let p = flat(point.center);
            if walk.to(p, point.radius).is_none() {
                missing.push(format!("{} can't walk to {}", l.teams[team].name, point.name));
            }
            // A truck gets to within a short walk.
            if road.to(p, point.radius + 60.0).is_none() {
                missing.push(format!("{} can't drive to {}", l.teams[team].name, point.name));
            }
        }
        for spawn in &l.teams[1 - team].spawns {
            if walk.to(flat(spawn.pos), 4.0).is_none() {
                missing.push(format!("{} can't walk to the other base's spawn at {}", l.teams[team].name, spawn.pos));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// Every door at ground level, every spawn and every point join up on foot.
pub(crate) fn doors_join_up(map: &Map) {
    let relief = Relief::new(&map.spec);
    let walk = Reach::on_foot(map, base_of(map, 0));
    let mut missing = Vec::new();
    let mut doors = 0;
    for door in map.level.doorways.iter().filter(|d| d.at.y < relief.height(flat(d.at)) + 0.5) {
        doors += 1;
        let (out, inside) = (door.at + door.outward * 3.0, door.at - door.outward * 3.0);
        if walk.to(flat(out), 3.0).is_none() && walk.to(flat(inside), 3.0).is_none() {
            missing.push(format!("door {}", door.at));
        }
    }
    assert!(missing.len() * 100 <= doors, "{} of {doors} doors can't be walked to:\n{}", missing.len(), missing.join("\n"));
}

/// Every flight of steps goes up and down, which for steps into water is the way out.
pub(crate) fn steps_go_both_ways(map: &Map) {
    let c = cfg();
    let relief = Relief::new(&map.spec);
    let lift = |p: Vec2| Vec3::new(p.x, relief.height(p) + 0.3, p.y);
    let mut failed = Vec::new();
    for steps in &map.spec.steps {
        let (top, bottom) = (lift(steps.from), lift(steps.to));
        for (a, b) in [(bottom, top), (top, bottom)] {
            if let Err(why) = walk(a, &[b], &c, &map.world) {
                failed.push(format!("steps from {} to {}: {why}", steps.from, steps.to));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Walking straight at the edge of the map, from anywhere along it, stops at the edge.
pub(crate) fn nobody_walks_off(map: &Map) {
    let c = cfg();
    let relief = Relief::new(&map.spec);
    let edge = map.spec.terrain.size * 0.5;
    let mut out = Vec::new();
    for k in -4..=4 {
        let along = k as f32 * edge * 0.2;
        for (start, outward) in [
            (Vec2::new(edge - 60.0, along), Vec2::X),
            (Vec2::new(-edge + 60.0, along), Vec2::NEG_X),
            (Vec2::new(along, edge - 60.0), Vec2::Y),
            (Vec2::new(along, -edge + 60.0), Vec2::NEG_Y),
        ] {
            let mut state = MoveState::new(Vec3::new(start.x, relief.height(start) + 0.3, start.y), &c);
            let input = MoveInput { wish: Vec2::Y, yaw: (-outward.x).atan2(-outward.y), ..MoveInput::default() };
            let mut events = Vec::new();
            for _ in 0..60 * 20 {
                step(&mut state, &c, &input, &map.world, DT, &mut events);
                events.clear();
            }
            if state.pos.x.abs() >= edge || state.pos.z.abs() >= edge || state.pos.y < -20.0 {
                out.push(format!("from {start} out to {}", state.pos));
            }
        }
    }
    assert!(out.is_empty(), "{}", out.join("\n"));
}

#[test]
fn muttrah_nobody_walks_off() {
    nobody_walks_off(muttrah());
}

#[test]
fn muttrah_steps_go_both_ways() {
    steps_go_both_ways(muttrah());
}

#[test]
fn muttrah_holds_up() {
    holds_up(muttrah());
}

#[test]
fn muttrah_builds_nothing_through_anything() {
    nothing_overlaps(&muttrah().spec);
}

#[test]
fn muttrah_stairs_climb() {
    stairs_climb(muttrah(), 400);
}

#[test]
fn muttrah_doors_work() {
    doors_work(muttrah(), 100_000);
}

#[test]
fn muttrah_helipads_are_clear() {
    helipads_are_clear(muttrah());
}

#[test]
fn muttrah_everyone_gets_everywhere() {
    everyone_gets_everywhere(muttrah());
}

#[test]
fn muttrah_doors_join_up() {
    doors_join_up(muttrah());
}

/// A plan of the map from straight above, as a PPM at `PRS_PLAN`, a pixel to `PRS_PLAN_SCALE` metres: land
/// shaded by its slope, water, roofs and walls by their height, trees, and the points and bases ringed.
pub(crate) fn plan(map: &Map) {
    let Ok(path) = std::env::var("PRS_PLAN") else { return };
    let scale: f32 = std::env::var("PRS_PLAN_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let relief = Relief::new(&map.spec);
    let size = map.spec.terrain.size;
    let n = (size / scale) as usize;
    let mut out = vec![0u8; n * n * 3];
    let at = |i: usize, j: usize| Vec2::new(i as f32 * scale - size * 0.5, j as f32 * scale - size * 0.5);
    for j in 0..n {
        for i in 0..n {
            let p = at(i, j);
            let h = relief.height(p);
            let slope = (relief.height(p + Vec2::X * scale) - h) / scale;
            let shade = (0.75 - slope * 1.5).clamp(0.3, 1.1);
            let base: [f32; 3] = match relief.ground(p) {
                g if relief.wet(p) && g != Ground::Paving => [60.0, 90.0, 140.0],
                Ground::Road => [150.0, 145.0, 135.0],
                Ground::Track => [170.0, 150.0, 110.0],
                Ground::Paving => [185.0, 180.0, 170.0],
                Ground::Yard | Ground::Dirt => [190.0, 170.0, 130.0],
                Ground::Scrub => [160.0, 150.0, 110.0],
                _ => [175.0, 165.0, 120.0],
            };
            let band = if (h / 10.0).fract() < 0.06 { 0.8 } else { 1.0 };
            let k = (j * n + i) * 3;
            for c in 0..3 {
                out[k + c] = (base[c] * shade * band).clamp(0.0, 255.0) as u8;
            }
        }
    }
    let mut paint = |p: Vec2, rgb: [u8; 3]| {
        let (i, j) = (((p.x + size * 0.5) / scale) as i64, ((p.y + size * 0.5) / scale) as i64);
        if i >= 0 && j >= 0 && (i as usize) < n && (j as usize) < n {
            let k = (j as usize * n + i as usize) * 3;
            out[k..k + 3].copy_from_slice(&rgb);
        }
    };
    let mut solids: Vec<&crate::Solid> = map.level.solids.iter().collect();
    solids.sort_by(|a, b| (a.center.y + a.half.y).total_cmp(&(b.center.y + b.half.y)));
    for solid in solids {
        let top = solid.center.y + solid.half.y - relief.height(flat(solid.center));
        let v = (120.0 + top * 12.0).clamp(80.0, 255.0) as u8;
        let rgb = match solid.paint {
            crate::Paint::Rock => [110, 100, 90],
            crate::Paint::Sandbag => [150, 140, 90],
            _ => [v, v.saturating_sub(10), v.saturating_sub(25)],
        };
        let axis_x = solid.rotation * Vec3::X * solid.half.x;
        let axis_z = solid.rotation * Vec3::Z * solid.half.z;
        let steps_x = (solid.half.x * 2.0 / scale).ceil().max(1.0) as i32;
        let steps_z = (solid.half.z * 2.0 / scale).ceil().max(1.0) as i32;
        for a in 0..=steps_x {
            for b in 0..=steps_z {
                let p = solid.center + axis_x * (a as f32 / steps_x as f32 * 2.0 - 1.0) + axis_z * (b as f32 / steps_z as f32 * 2.0 - 1.0);
                paint(flat(p), rgb);
            }
        }
    }
    for tree in &map.level.trees {
        for k in 0..12 {
            let a = k as f32 / 12.0 * std::f32::consts::TAU;
            paint(flat(tree.base) + Vec2::from_angle(a) * tree.radius * 0.6, [60, 110, 50]);
        }
    }
    let ring = |paint: &mut dyn FnMut(Vec2, [u8; 3]), c: Vec2, r: f32, rgb: [u8; 3]| {
        for k in 0..(r * 8.0) as i32 {
            let a = k as f32 / (r * 8.0) * std::f32::consts::TAU;
            paint(c + Vec2::from_angle(a) * r, rgb);
        }
    };
    for point in &map.level.capture_points {
        ring(&mut paint, flat(point.center), point.radius, [230, 40, 40]);
    }
    for team in &map.level.teams {
        ring(&mut paint, base_of(map, team.spawns.len().min(0)), team.base_radius, [40, 40, 230]);
        ring(&mut paint, flat(team.base_center()), team.base_radius, [40, 40, 230]);
    }
    for v in &map.level.vehicles {
        ring(&mut paint, flat(v.pos), 2.0, [250, 250, 0]);
    }
    for pad in &map.level.helipads {
        ring(&mut paint, flat(pad.pos), 9.5, [255, 140, 0]);
    }
    let mut file = format!("P6\n{n} {n}\n255\n").into_bytes();
    file.extend_from_slice(&out);
    std::fs::write(path, file).unwrap();
}

#[test]
#[ignore]
fn muttrah_plan() {
    plan(muttrah());
}

#[test]
#[ignore]
fn fallujah_plan() {
    plan(fallujah());
}

/// What stands near `PRS_NEAR=x,z` on a map, to see why something is in the way.
#[test]
#[ignore]
fn near() {
    let at: Vec<f32> = std::env::var("PRS_NEAR").unwrap_or_default().split(',').filter_map(|v| v.parse().ok()).collect();
    let p = Vec2::new(at[0], at[1]);
    let map = if std::env::var("PRS_NEAR_MAP").as_deref() == Ok("fallujah") { fallujah() } else { muttrah() };
    for c in map.spec.compounds.iter().filter(|c| c.at.distance(p) < c.half.length() + 3.0) {
        println!("compound at {} yaw {} half {}: {:?}", c.at, c.yaw, c.half, c.buildings.iter().map(|b| (b.at, b.half, b.storeys, b.openings.len())).collect::<Vec<_>>());
    }
    for s in map.level.solids.iter().filter(|s| flat(s.center).distance(p) < s.half.length() + 2.0) {
        println!("{:?} {} half {}", s.paint, s.center, s.half);
    }
}

/// A walled yard can be got into over its wall as well as through its gate: alone where the wall is low,
/// and from a friend's back where it is not.
pub(crate) fn yard_walls_can_be_climbed(map: &Map, most: usize) {
    use prs_movement::{Perch, Stance, plan_climb};
    let c = cfg();
    let relief = Relief::new(&map.spec);
    let walled: Vec<&Compound> = map.spec.compounds.iter().filter(|k| k.wall.as_ref().is_some_and(|w| w.height <= 2.45)).collect();
    let (mut tried, mut climbed, mut failed) = (0, 0, Vec::new());
    for (_, compound) in every(&walled, most) {
        let wall = compound.wall.as_ref().unwrap();
        let rotation = glam::Quat::from_rotation_y(compound.yaw);
        let half = compound.half;
        // Somewhere along any of its walls, clear of the gates, as whoever climbs it chooses.
        let spots = [Side::N, Side::S, Side::E, Side::W].into_iter().flat_map(|side| (-4..=4).map(move |k| (side, k as f32 * 0.2)));
        let mut stood = false;
        let mut made_it = false;
        for (side, share) in spots {
            let (local, along_half) = match side {
                Side::N => (Vec3::new(share * half.x, 0.0, -half.y), half.x),
                Side::S => (Vec3::new(share * half.x, 0.0, half.y), half.x),
                Side::E => (Vec3::new(half.x, 0.0, share * half.y), half.y),
                Side::W => (Vec3::new(-half.x, 0.0, share * half.y), half.y),
            };
            if wall.gates.iter().any(|g| g.side == side && (g.along - share * along_half).abs() < g.width + 1.0) {
                continue;
            }
            let normal = side.normal();
            let outward = rotation * Vec3::new(normal.x, 0.0, normal.y);
            let spot = Vec3::new(compound.at.x, 0.0, compound.at.y) + rotation * local + outward * (c.radius + 0.3);
            let top = Vec3::new(spot.x, relief.height(flat(spot)) + 10.0, spot.z);
            let Some(hit) = map.world.raycast(top, Vec3::NEG_Y, 20.0) else { continue };
            let feet = top - Vec3::Y * (hit.distance - 0.02);
            if feet.y - relief.height(flat(feet)) > 0.4 || !map.world.capsule_is_clear(feet, c.capsule(Stance::Stand)) {
                continue;
            }
            stood = true;
            let input = MoveInput { yaw: outward.x.atan2(outward.z), ..MoveInput::default() };
            let mut state = MoveState::new(feet, &c);
            state.grounded = true;
            let alone = plan_climb(&state, &c, &input, &map.world);
            let back = feet + Vec3::Y * c.stool.back_height;
            state.pos = back;
            state.perch = Perch { on: Some(1), spot: back };
            let helped = map.world.capsule_is_clear(back, c.capsule(Stance::Stand)).then(|| plan_climb(&state, &c, &input, &map.world)).flatten();
            if alone.is_some() || helped.is_some() {
                made_it = true;
                break;
            }
        }
        if !stood {
            continue;
        }
        tried += 1;
        if made_it {
            climbed += 1;
        } else {
            failed.push(format!("{} m wall of the compound at {}", wall.height, compound.at));
        }
    }
    assert!(tried >= most.min(walled.len()) / 2, "only {tried} walls could be stood beside");
    assert!(climbed * 10 >= tried * 9, "{climbed} of {tried} climbed:\n{}", failed.join("\n"));
}

#[test]
fn muttrah_yard_walls_can_be_climbed() {
    yard_walls_can_be_climbed(muttrah(), 60);
}

/// The ground height at each of `PRS_HEIGHTS=x,z;x,z` on `PRS_HEIGHTS_MAP`, for placing a camera.
#[test]
#[ignore]
fn heights() {
    let spec = match std::env::var("PRS_HEIGHTS_MAP").as_deref() {
        Ok("fallujah") => MapSpec::fallujah(),
        _ => MapSpec::muttrah(),
    };
    let relief = Relief::new(&spec);
    for pair in std::env::var("PRS_HEIGHTS").unwrap_or_default().split(';') {
        let v: Vec<f32> = pair.split(',').filter_map(|s| s.parse().ok()).collect();
        if let [x, z] = v[..] {
            println!("{x},{z}: {:.2}", relief.height(Vec2::new(x, z)));
        }
    }
}

#[test]
fn muttrah_fits_the_build_budget_with_room_to_spare() {
    assert!(muttrah().spec.build_within(crate::maps::MapLimits::default().max_build_steps / 2).is_ok());
}

/// Every bridge takes a soldier across and back without falling off.
pub(crate) fn bridges_can_be_walked(map: &Map) {
    let c = cfg();
    let relief = Relief::new(&map.spec);
    let ground = |p: Vec2| Vec3::new(p.x, relief.height(p) + 0.3, p.y);
    assert!(!map.spec.bridges.is_empty());
    for bridge in &map.spec.bridges {
        let dir = (bridge.to - bridge.from).normalize();
        let (from, to) = (bridge.from - dir * 4.0, bridge.to + dir * 4.0);
        for (a, b) in [(from, to), (to, from)] {
            // A long bridge in stages, each a walk of a few seconds, along the deck.
            let stages = (a.distance(b) / 20.0).ceil() as usize;
            let deck = |t: f32| {
                let p = a.lerp(b, t);
                let top = Vec3::new(p.x, 200.0, p.y);
                map.world.raycast(top, Vec3::NEG_Y, 400.0).map_or(ground(p), |hit| top - Vec3::Y * (hit.distance - 0.3))
            };
            let path: Vec<Vec3> = (1..=stages).map(|k| deck(k as f32 / stages as f32)).collect();
            let state = walk(ground(a), &path, &c, &map.world).unwrap_or_else(|why| panic!("bridge at {}: {why}", bridge.from));
            assert!(state.pos.y > ground(b).y - 0.8, "fell off the bridge at {} to {}", bridge.from, state.pos);
        }
    }
}

#[test]
fn fallujah_holds_up() {
    holds_up(fallujah());
}

#[test]
fn fallujah_builds_nothing_through_anything() {
    nothing_overlaps(&fallujah().spec);
}

#[test]
fn fallujah_stairs_climb() {
    stairs_climb(fallujah(), 300);
}

#[test]
fn fallujah_doors_work() {
    doors_work(fallujah(), 600);
}

#[test]
fn fallujah_helipads_are_clear() {
    helipads_are_clear(fallujah());
}

#[test]
fn fallujah_everyone_gets_everywhere() {
    everyone_gets_everywhere(fallujah());
}

#[test]
fn fallujah_doors_join_up() {
    doors_join_up(fallujah());
}

#[test]
fn fallujah_yard_walls_can_be_climbed() {
    yard_walls_can_be_climbed(fallujah(), 60);
}

#[test]
fn fallujah_bridges_can_be_walked() {
    bridges_can_be_walked(fallujah());
}

#[test]
fn fallujah_nobody_walks_off() {
    nobody_walks_off(fallujah());
}

#[test]
fn fallujah_fits_the_build_budget_with_room_to_spare() {
    assert!(fallujah().spec.build_within(crate::maps::MapLimits::default().max_build_steps / 2).is_ok());
}
