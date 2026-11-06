//! Claudabad: four kilometres of dusty river country fought over between a northern and a southern team,
//! with the town at its heart. A canal runs
//! across the middle and parts round an island, where a terracotta sunburst stands over the square for
//! everyone to steer by. Six bridges cross it: a road bridge at each end and two onto the island from
//! each bank. North of the canal is the old quarter, close-packed compounds round a covered bazaar, with
//! a brickworks beyond. South of it is the newer town under its water tower, and the rail yard. Palm groves
//! and fields line the canal outside the town, and a low ridge hides the town from the country beyond.
//!
//! Out there the canal widens into a river, crossed by a highway on each flank, each going over a ring road
//! on an overpass. A hilltop fort with a radio mast watches over each side, dry wadis cut the open ground,
//! hamlets stand among their orchards, and each base has an airstrip beside it.
//!
//! Each side's half is the other's turned half way round the middle of the map, so both sides have as
//! far to go, and what stands there differs.

use crate::archetypes::{barn, hall, house, shed, shop_row, tower, townhouse};
use crate::spec::*;
use crate::town::{clump, compound, gate, hill, line, placed, rubble, spawn_rows, tall, trees, wave, zone};
use crate::{Paint, Scatter, TreeKind};
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI};

/// The canal: deep enough that you cannot climb out, with steep sides.
const CANAL_WIDTH: f32 = 16.0;
const CANAL_BED: f32 = 12.5;
const CANAL_DEPTH: f32 = 3.6;
/// From the middle of the canal to where a bridge starts on either bank, and a flight of steps runs from.
const BRIDGE_REACH: f32 = 12.0;
const STEPS_FROM: f32 = 10.0;
const STEPS_TO: f32 = 4.0;
/// Steps go down this far along the bank from a bridge, and are this wide.
const STEPS_BESIDE: f32 = 7.0;
const STEPS_WIDTH: f32 = 2.4;
const ROAD_BRIDGE: (f32, f32) = (8.0, 0.9);
const FOOT_BRIDGE: (f32, f32) = (3.2, 1.0);
const BRIDGE_THICKNESS: f32 = 0.6;
const BRIDGE_SPAN: f32 = 12.0;

/// The canal's stone banks: how far past the top each block reaches (short of the parapet), how far over the
/// bed (a cell of the finest ground, which is as far as the bank's slope can show past the bed's edge), and how
/// long each block is.
const QUAY_BEYOND: f32 = 0.9;
const QUAY_OVER_BED: f32 = 2.0;
const QUAY_LENGTH: f32 = 6.0;

/// More steps out of the canal, between the bridges, so nobody who falls in has far to walk.
const STEPS_EVERY: f32 = 120.0;
/// A low wall along both banks through the town, so nothing drives in by accident, open at each bridge and
/// at the top of each flight of steps.
const PARAPET: (f32, f32) = (0.85, 0.4);
const PARAPET_OUT: f32 = 1.2;
const PARAPET_REACH: f32 = 265.0;
const PARAPET_GAP: f32 = 2.0;

/// Kept this far clear of a road's middle and the canal's when filling a quarter with compounds.
const ROAD_CLEARANCE: f32 = 9.0;
const CANAL_CLEARANCE: f32 = 16.0;
/// A block of a quarter: half its size, and the alley between it and the next.
const BLOCK: (f32, f32) = (14.0, 12.0);
const ALLEY: f32 = 4.0;

/// The south base, with the airfield to its east. The north one is this turned round.
const BASE: (f32, f32) = (300.0, 1750.0);
/// The river the canal becomes outside the town: wider and a little deeper.
const RIVER: (f32, f32, f32) = (30.0, 22.0, 4.2);
const RIVER_REACH: f32 = 20.0;
/// How far an overpass's embankments stand either side of the road it crosses, how long and wide they are,
/// and how high.
const EMBANKMENT: (f32, f32, f32, f32) = (45.0, 40.0, 14.0, 6.0);
const OVERPASS_REACH: f32 = 28.0;

/// The point half way round the middle of the map from `p`.
fn turned(p: (f32, f32)) -> (f32, f32) {
    (-p.0, -p.1)
}

fn both(points: &[(f32, f32)]) -> [Vec<(f32, f32)>; 2] {
    [points.to_vec(), points.iter().map(|&p| turned(p)).collect()]
}

impl MapSpec {
    pub fn claudabad() -> Self {
        let style = BuildStyle::default();
        let roads = roads();
        let canal = canal();
        let mut compounds = places(&style);
        compounds.extend(country(&style));
        let taken: Vec<(Vec2, Vec2)> = compounds.iter().map(|c| (c.at, c.half + Vec2::splat(ALLEY))).collect();
        compounds.extend(quarters(&style, &roads, &canal, &taken));
        let (mut bridges, steps) = crossings(&canal);
        bridges.extend(highway_bridges());
        let mut walls = rails();
        walls.extend(parapets(&canal, &bridges, &steps));
        let mut ditches = canal;
        ditches.extend(irrigation());
        ditches.extend(wadis());
        Self {
            seed: 4242,
            terrain: land(),
            style,
            roads,
            ditches,
            zones: grounds(),
            compounds,
            walls,
            rubble: heaps(),
            props: street_props(),
            woods: woods(),
            teams: vec![
                TeamSpec { name: "South".into(), spawns: spawn_rows((BASE.0, BASE.1 - 2.0), 0.0), base_radius: 60.0 },
                TeamSpec { name: "North".into(), spawns: spawn_rows(turned((BASE.0, BASE.1 + 1.5)), PI), base_radius: 60.0 },
            ],
            // Each side's three are the other's turned round, so the walks and drives match, and the square
            // is in the middle.
            capture_points: [
                ("Orchard Hamlet", (-650.0, 1150.0), 40.0),
                ("Hill Fort", (900.0, 450.0), 36.0),
                ("New Town", (-130.0, 120.0), 34.0),
                // As far from New Town as from the Bazaar, beside the sunburst.
                ("Sunburst Square", (8.0, 9.0), 30.0),
                ("Bazaar", (130.0, -120.0), 34.0),
                ("Ridge Fort", (-900.0, -450.0), 36.0),
                ("Kiln Hamlet", (650.0, -1150.0), 40.0),
            ]
            .into_iter()
            .map(|(name, at, radius)| CapturePointSpec { name: name.into(), at: at.into(), radius })
            .collect(),
            // In the southern base, firing out of its west gate across the empty ground beyond.
            sentry: SpawnSpec { at: Vec2::new(BASE.0 - 36.0, BASE.1), yaw: FRAC_PI_2 },
            // On the airstrip's apron.
            helipads: vec![SpawnSpec { at: Vec2::new(BASE.0 + 120.0, BASE.1 + 50.0), yaw: 0.0 }, SpawnSpec { at: turned((BASE.0 + 120.0, BASE.1 + 50.0)).into(), yaw: PI }],
            vehicles: vehicles(),
            bridges,
            steps,
            landmarks: vec![Sunburst {
                at: Vec2::ZERO,
                yaw: 0.35,
                plinth: glam::Vec3::new(2.4, 0.5, 2.4),
                column: glam::Vec3::new(0.55, 4.5, 0.55),
                hub: 0.9,
                rays: 12,
                ray_length: 4.2,
                ray_width: 0.55,
                ray_depth: 0.4,
                short_every: 2,
                short_share: 0.62,
                paint: Paint::Terracotta,
                stone: Paint::Stone,
            }],
            waters: Vec::new(),
        }
    }
}

fn land() -> TerrainSpec {
    TerrainSpec {
        size: 4096.0,
        base_cells: 128,
        max_splits: 4,
        paint_splits: 4,
        tolerance: 0.16,
        // Open country is smooth enough to need far fewer triangles; the town, hamlets, forts and bases are not.
        open_tolerance: Some(0.9),
        open_paint_splits: Some(2),
        detail: [((0.0, 0.0), 400.0), ((-650.0, 1150.0), 140.0), ((900.0, 450.0), 90.0), ((1300.0, 1250.0), 110.0), (BASE, 90.0), ((BASE.0 + 120.0, BASE.1), 90.0)]
            .into_iter()
            .flat_map(|(at, r)| [(Vec2::from(at), r), (Vec2::from(turned(at)), r)])
            .collect(),
        waves: vec![wave(0.017, 0.008, 0.4, 1.6), wave(0.006, 0.021, 1.7, 2.0), wave(0.047, 0.035, 3.1, 0.5), wave(0.0021, 0.0017, 0.9, 9.0), wave(0.0013, 0.0031, 2.3, 7.0)],
        hills: vec![
            // The ridges that hide each base from the town, with a gap where the road goes over.
            hill((0.0, 305.0), (470.0, 46.0), 0.02, 13.0),
            hill((55.0, 302.0), (44.0, 40.0), 0.0, -8.0),
            hill((0.0, -305.0), (470.0, 46.0), 0.02, 13.0),
            hill((-55.0, -302.0), (44.0, 40.0), 0.0, -8.0),
            // Knolls out on the flanks, for a long look across the palms.
            hill((-380.0, 170.0), (80.0, 60.0), 0.3, 9.0),
            hill((380.0, -170.0), (80.0, 60.0), 0.3, 9.0),
            hill((360.0, 210.0), (70.0, 55.0), -0.4, 7.0),
            hill((-360.0, -210.0), (70.0, 55.0), -0.4, 7.0),
            // The fort hills, each a long look over its side's half.
            hill((900.0, 450.0), (170.0, 140.0), 0.4, 30.0),
            hill((-900.0, -450.0), (170.0, 140.0), 0.4, 30.0),
            // High ground out in the country, to break up the long views.
            hill((-1500.0, 600.0), (260.0, 180.0), 0.3, 26.0),
            hill((1500.0, -600.0), (260.0, 180.0), 0.3, 26.0),
            hill((1100.0, 1500.0), (220.0, 140.0), -0.2, 20.0),
            hill((-1100.0, -1500.0), (220.0, 140.0), -0.2, 20.0),
            hill((-300.0, 1450.0), (200.0, 120.0), 0.1, 16.0),
            hill((300.0, -1450.0), (200.0, 120.0), 0.1, 16.0),
            // The embankments each highway climbs to go over a ring road.
            hill((-1150.0, 700.0 - EMBANKMENT.0), (EMBANKMENT.2, EMBANKMENT.1), 0.0, EMBANKMENT.3),
            hill((-1150.0, 700.0 + EMBANKMENT.0), (EMBANKMENT.2, EMBANKMENT.1), 0.0, EMBANKMENT.3),
            hill((1150.0, -700.0 - EMBANKMENT.0), (EMBANKMENT.2, EMBANKMENT.1), 0.0, EMBANKMENT.3),
            hill((1150.0, -700.0 + EMBANKMENT.0), (EMBANKMENT.2, EMBANKMENT.1), 0.0, EMBANKMENT.3),
        ],
        // The valley floor the town sits on.
        calm: vec![Calm { at: Vec2::ZERO, radius: 330.0, fade: 600.0, keep: 0.12 }],
        rim: Rim { inset: 70.0, height: 20.0, face: 12.0, face_height: 16.0, wander: 12.0 },
        survey: None,
    }
}

fn canal() -> Vec<Ditch> {
    let canal = |points: &[(f32, f32)]| Ditch {
        points: line(points),
        width: CANAL_WIDTH,
        bed_width: CANAL_BED,
        depth: CANAL_DEPTH,
        ground: Ground::Water,
        cuts_roads: true,
        quay: Some(Quay { beyond: QUAY_BEYOND, over_bed: QUAY_OVER_BED, length: QUAY_LENGTH, paint: Paint::Stone }),
    };
    let west = [(-530.0, -34.0), (-380.0, -20.0), (-250.0, -8.0), (-150.0, 0.0), (-95.0, 0.0)];
    let north_arm = [(-95.0, 0.0), (-60.0, -36.0), (0.0, -44.0), (60.0, -36.0), (95.0, 0.0)];
    let mut all = Vec::new();
    for half in [both(&west), both(&north_arm)] {
        for points in half {
            all.push(canal(&points));
        }
    }
    let river = [(-520.0, -34.0), (-900.0, -60.0), (-1400.0, -20.0), (-1800.0, 30.0), (-2100.0, 40.0)];
    for points in both(&river) {
        let (width, bed_width, depth) = RIVER;
        // Banked up in mud brick, the colour of the land.
        let quay = Quay { beyond: QUAY_BEYOND, over_bed: QUAY_OVER_BED, length: QUAY_LENGTH, paint: Paint::Mudbrick };
        all.push(Ditch { points: line(&points), width, bed_width, depth, ground: Ground::Water, cuts_roads: true, quay: Some(quay) });
    }
    all
}

/// Dry wadis across the open ground on each flank, deep enough to drive along out of sight, with sides
/// gentle enough to get in and out of anywhere.
fn wadis() -> Vec<Ditch> {
    let wadi = |points: &[(f32, f32)]| Ditch { points: line(points), width: 24.0, bed_width: 8.0, depth: 4.0, ground: Ground::Shingle, cuts_roads: false, quay: None };
    let mut all = Vec::new();
    for points in [
        vec![(-1900.0, 1450.0), (-1500.0, 1150.0), (-1100.0, 900.0), (-800.0, 520.0), (-600.0, 180.0), (-580.0, 10.0)],
        vec![(1900.0, 1550.0), (1550.0, 1250.0), (1250.0, 1000.0), (1150.0, 750.0), (700.0, 300.0), (610.0, 80.0)],
        vec![(-1100.0, 900.0), (-700.0, 1000.0), (-400.0, 900.0)],
    ] {
        for half in both(&points) {
            all.push(wadi(&half));
        }
    }
    all
}

/// The highways' bridges over the river, and their overpasses over the ring roads.
fn highway_bridges() -> Vec<Bridge> {
    let (width, parapet) = ROAD_BRIDGE;
    let deck = |from: (f32, f32), to: (f32, f32)| Bridge { from: from.into(), to: to.into(), width, thickness: BRIDGE_THICKNESS, parapet, span: BRIDGE_SPAN, paint: Paint::Cement };
    let mut all = Vec::new();
    for (from, to) in [((-1150.0, -40.0 - RIVER_REACH), (-1150.0, -40.0 + RIVER_REACH)), ((-1150.0, 700.0 - OVERPASS_REACH), (-1150.0, 700.0 + OVERPASS_REACH))] {
        all.push(deck(from, to));
        all.push(deck(turned(from), turned(to)));
    }
    all
}

/// Ditches feeding the fields off the canal, on both sides of it at each end of the town.
fn irrigation() -> Vec<Ditch> {
    let ditch = |points: &[(f32, f32)]| Ditch { points: line(points), width: 4.8, bed_width: 1.4, depth: 1.25, ground: Ground::DitchBed, cuts_roads: false, quay: None };
    let mut all = Vec::new();
    for points in [
        vec![(-420.0, -38.0), (-430.0, -150.0), (-470.0, -260.0)],
        vec![(-330.0, -30.0), (-320.0, -140.0)],
        vec![(-430.0, -150.0), (-320.0, -140.0), (-270.0, -150.0)],
        vec![(-400.0, 0.0), (-410.0, 110.0)],
    ] {
        for half in both(&points) {
            all.push(ditch(&half));
        }
    }
    all
}

fn roads() -> Vec<Road> {
    let road = |points: &[(f32, f32)], width: f32, ground: Ground| Road { points: line(points), width, shoulder: 5.0, ground };
    let mut all = Vec::new();
    for (points, width, ground) in [
        // From the south base, across the country, over the ridge gap and down to the island bridge.
        (vec![(BASE.0 + 16.0, BASE.1 - 26.0), (300.0, 1500.0), (200.0, 1150.0), (110.0, 800.0), (60.0, 520.0), (55.0, 300.0), (30.0, 160.0), (-30.0, 72.0), (-30.0, 52.0)], 7.0, Ground::Road),
        // The western highway, the whole height of the map, over the ring road and the river.
        (vec![(-1100.0, 1960.0), (-1150.0, 1200.0), (-1150.0, 700.0), (-1160.0, 200.0), (-1150.0, -40.0), (-1180.0, -900.0), (-1120.0, -1960.0)], 9.0, Ground::Road),
        // The southern ring road, under the western highway's overpass.
        (vec![(-1960.0, 720.0), (-1150.0, 700.0), (-600.0, 680.0), (110.0, 760.0), (900.0, 700.0), (1960.0, 690.0)], 7.0, Ground::Road),
        // Lanes to the hamlets and the fort, and from the base to the airstrip.
        (vec![(-600.0, 680.0), (-650.0, 1100.0)], 4.0, Ground::Track),
        (vec![(900.0, 700.0), (900.0, 560.0)], 4.0, Ground::Track),
        (vec![(1150.0, 1250.0), (900.0, 700.0)], 4.0, Ground::Track),
        (vec![(BASE.0 + 40.0, BASE.1), (BASE.0 + 120.0, BASE.1 + 30.0)], 5.0, Ground::Track),
        // Across the island, round the square.
        (vec![(-30.0, 28.0), (-28.0, 21.5)], 7.0, Ground::Road),
        // Along the south bank from the island road to the east bridge, and on to the east edge.
        (vec![(-30.0, 72.0), (60.0, 80.0), (150.0, 60.0), (250.0, 30.0), (300.0, 24.0)], 6.0, Ground::Road),
        (vec![(300.0, 1.0), (380.0, 48.0), (530.0, 60.0)], 6.0, Ground::Road),
        // Lanes out to the rail yard and the new town's square.
        (vec![(150.0, 60.0), (188.0, 150.0), (198.0, 192.0)], 4.0, Ground::Track),
        (vec![(-30.0, 72.0), (-96.0, 108.0)], 4.0, Ground::Track),
        // The south bank's footbridge lane.
        (vec![(45.0, 50.0), (60.0, 80.0)], 4.0, Ground::Track),
    ] {
        for half in both(&points) {
            all.push(road(&half, width, ground));
        }
    }
    all
}

/// Bridges, and steps down into the canal beside each and every so often between them, so that falling in
/// is not the end of you: (middle of the canal under the bridge, whether it carries a road).
fn crossings(canal: &[Ditch]) -> (Vec<Bridge>, Vec<Steps>) {
    let sites = [((-300.0, -12.6), true), ((30.0, -40.0), true), ((-45.0, -38.0), false)];
    let (mut bridges, mut steps) = (Vec::new(), Vec::new());
    for (center, road) in sites {
        for c in [center, turned(center)] {
            let c = Vec2::from(c);
            let (width, parapet) = if road { ROAD_BRIDGE } else { FOOT_BRIDGE };
            bridges.push(Bridge {
                from: c - Vec2::Y * BRIDGE_REACH,
                to: c + Vec2::Y * BRIDGE_REACH,
                width,
                thickness: BRIDGE_THICKNESS,
                parapet,
                span: BRIDGE_SPAN,
                paint: Paint::Stone,
            });
            let beside = Vec2::X * (width * 0.5 + STEPS_BESIDE * 0.5 + 1.0);
            for bank in [-1.0, 1.0] {
                steps.push(Steps {
                    from: c + beside + Vec2::Y * bank * STEPS_FROM,
                    to: c + beside + Vec2::Y * bank * STEPS_TO,
                    width: STEPS_WIDTH,
                    paint: Paint::Stone,
                });
            }
        }
    }
    let distance = |points: &[Vec2], p: Vec2| {
        points.windows(2).map(|w| {
            let t = ((p - w[0]).dot(w[1] - w[0]) / (w[1] - w[0]).length_squared()).clamp(0.0, 1.0);
            (w[0] + (w[1] - w[0]) * t).distance(p)
        }).fold(f32::MAX, f32::min)
    };
    for (index, ditch) in canal.iter().enumerate() {
        for (at, across) in along(&ditch.points, STEPS_EVERY) {
            if bridges.iter().any(|b| b.from.lerp(b.to, 0.5).distance(at) < STEPS_EVERY * 0.5) {
                continue;
            }
            // From just over the bank's top to just onto the bed, however wide this stretch is.
            let (from, to) = (ditch.width * 0.5 + STEPS_FROM - CANAL_WIDTH * 0.5, ditch.bed_width * 0.5 + STEPS_TO - CANAL_BED * 0.5);
            for bank in [-1.0, 1.0] {
                let top = at + across * bank * from;
                // Not where another stretch of water joins this one and the bank is somewhere else.
                let joined = canal.iter().enumerate().any(|(other, d)| other != index && distance(&d.points, top) < d.width * 0.5 + 2.0);
                if !joined {
                    steps.push(Steps { from: top, to: at + across * bank * to, width: STEPS_WIDTH, paint: Paint::Stone });
                }
            }
        }
    }
    (bridges, steps)
}

/// Points every so far along a line, each with the way across it, starting half that from its start.
fn along(points: &[Vec2], every: f32) -> Vec<(Vec2, Vec2)> {
    let mut out = Vec::new();
    let mut next = every * 0.5;
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(b);
        let dir = (b - a) / length;
        while next <= walked + length {
            out.push((a + dir * (next - walked), dir.perp()));
            next += every;
        }
        walked += length;
    }
    out
}

/// The canal's parapets: both banks of every stretch within reach of the middle of the town, broken where a
/// bridge or a flight of steps meets the bank.
fn parapets(canal: &[Ditch], bridges: &[Bridge], steps: &[Steps]) -> Vec<FieldWall> {
    let out = CANAL_WIDTH * 0.5 + PARAPET_OUT;
    let open = |p: Vec2| {
        let bridged = bridges.iter().any(|b| {
            let mid = b.from.lerp(b.to, 0.5);
            let dir = (b.to - b.from).normalize();
            let d = p - mid;
            d.dot(dir.perp()).abs() < b.width * 0.5 + PARAPET_GAP && d.dot(dir).abs() < BRIDGE_REACH + 2.0
        });
        let stepped = steps.iter().any(|s| s.from.distance(p) < STEPS_WIDTH * 0.5 + PARAPET_GAP + 1.5);
        bridged || stepped || p.x.abs() > PARAPET_REACH
    };
    let mut walls = Vec::new();
    for ditch in canal {
        for side in [-1.0, 1.0] {
            let mut run: Vec<(f32, f32)> = Vec::new();
            for (at, across) in along(&ditch.points, 2.0) {
                let p = at + across * side * out;
                if open(p) {
                    if run.len() > 1 {
                        walls.push(std::mem::take(&mut run));
                    }
                    run.clear();
                } else {
                    run.push((p.x, p.y));
                }
            }
            if run.len() > 1 {
                walls.push(run);
            }
        }
    }
    let s = BuildStyle::default();
    walls
        .into_iter()
        .map(|points| FieldWall { points: line(&points), height: PARAPET.0, unevenness: 0.0, thickness: PARAPET.1, segment: s.wall_segment, wander: 0.0, paint: Paint::Stone, gaps: Vec::new() })
        .collect()
}

fn grounds() -> Vec<Zone> {
    let mut all = vec![
        // Dusty scrub everywhere, then the worn ground of the town, either side of the canal.
        zone((0.0, 0.0), (2048.0, 2048.0), 0.0, Ground::Scrub),
        zone((0.0, 0.0), (270.0, 230.0), 0.0, Ground::Dirt),
    ];
    // Each base's apron, and the fields round each hamlet.
    for (at, half, ground) in [
        ((BASE.0 + 120.0, BASE.1 + 50.0), (40.0, 18.0), Ground::Paving),
        ((-650.0, 1290.0), (140.0, 50.0), Ground::Wheat),
        ((-820.0, 1150.0), (50.0, 110.0), Ground::Plough),
        ((1300.0, 1360.0), (110.0, 40.0), Ground::Pasture),
    ] {
        all.push(zone(at, half, 0.0, ground));
        all.push(zone(turned(at), half, 0.0, ground));
    }
    for (at, half, ground) in [
        ((-420.0, -90.0), (60.0, 50.0), Ground::Wheat),
        ((-330.0, -90.0), (30.0, 50.0), Ground::Plough),
        ((-440.0, -210.0), (40.0, 50.0), Ground::Plough),
        ((-330.0, -200.0), (50.0, 50.0), Ground::Pasture),
        ((-430.0, 60.0), (50.0, 45.0), Ground::Wheat),
        ((-330.0, 70.0), (40.0, 40.0), Ground::Orchard),
    ] {
        all.push(zone(at, half, 0.0, ground));
        all.push(zone(turned(at), half, 0.0, ground));
    }
    all
}

/// The rail line along the south of the town into the yard, as two low rails that follow the ground.
fn rails() -> Vec<FieldWall> {
    let rail = |points: Vec<(f32, f32)>| FieldWall {
        points: line(&points),
        height: 0.16,
        unevenness: 0.0,
        thickness: 0.12,
        segment: 6.0,
        wander: 0.0,
        paint: Paint::Metal,
        gaps: Vec::new(),
    };
    let mut all = Vec::new();
    for gauge in [-0.75, 0.75] {
        all.push(rail(vec![(-470.0, 216.0 + gauge), (170.0, 216.0 + gauge)]));
        for siding in [224.0, 236.0, 248.0] {
            all.push(rail(vec![(175.0, siding + gauge), (250.0, siding + gauge)]));
        }
        all.push(rail(vec![(255.0, 230.0 + gauge), (470.0, 244.0 + gauge)]));
    }
    all
}

/// The named places: the bases, the square, the bazaar, the brickworks, the new town and the rail yard.
fn places(s: &BuildStyle) -> Vec<Compound> {
    let (mud, lime, ochre, cement, brick) = (Paint::Mudbrick, Paint::Limewash, Paint::Ochre, Paint::Cement, Paint::Brick);
    let flat = Roof::Flat { paint: Paint::Packed };
    let mut all = Vec::new();

    // The square on the island, round the sunburst, with a cafe at each end.
    all.push(Compound {
        ground: Ground::Paving,
        ..compound(
            (0.0, 0.0),
            0.0,
            (32.0, 19.0),
            None,
            Vec::new(),
            vec![
                placed(Prop::Barrier, (-12.0, -10.0), 0.2),
                placed(Prop::Barrier, (14.0, 11.0), -0.3),
                placed(Prop::Stall, (-20.0, 8.0), 0.1),
                placed(Prop::Stall, (21.0, -8.0), PI + 0.1),
                placed(Prop::Well, (-24.0, -4.0), 0.0),
            ],
        )
    });
    for at in [(-62.0, 0.0), turned((-62.0, 0.0))] {
        all.push(compound(at, if at.0 < 0.0 { 0.0 } else { PI }, (9.0, 7.0), None, vec![house(s, (0.0, -1.0), 0.0, (6.0, 3.6), lime)], vec![placed(Prop::Drums, (6.0, 5.0), 0.3)]));
    }

    // The bazaar: shops round a market under canvas.
    let mut market = vec![placed(Prop::Well, (0.0, 0.0), 0.0)];
    for (x, z) in [(-14.0, -4.0), (-14.0, 6.0), (-4.0, -4.0), (-4.0, 6.0), (6.0, -4.0), (6.0, 6.0), (16.0, 1.0)] {
        market.push(placed(Prop::Canopy, (x, z), 0.0));
    }
    for (x, z, yaw) in [(-18.0, 1.0, FRAC_PI_2), (11.0, -9.0, 0.0), (1.0, 11.0, PI), (-9.0, 11.0, PI)] {
        market.push(placed(Prop::Stall, (x, z), yaw));
    }
    all.push(Compound {
        ground: Ground::Paving,
        ..compound(
            (130.0, -120.0),
            0.06,
            (30.0, 26.0),
            None,
            vec![shop_row(s, (0.0, -21.0), 0.0, (22.0, 4.2), ochre, 4), shop_row(s, (0.0, 21.0), PI, (22.0, 4.2), mud, 4)],
            market,
        )
    });

    // The brickworks: kilns and stacks of brick behind a low wall.
    all.push(compound(
        (-210.0, -230.0),
        0.1,
        (40.0, 28.0),
        tall(mud, s.crouch_cover, &[gate(Side::S, 6.0, 6.0), gate(Side::E, -4.0, 4.0), gate(Side::N, -10.0, 3.0)]),
        vec![barn(s, (-22.0, -14.0), 0.0, (10.0, 6.5), brick, Paint::Rust), shed((24.0, 16.0), PI, (6.0, 4.0), mud, Paint::Tin)],
        vec![
            placed(Prop::Kiln, (6.0, -16.0), 0.0),
            placed(Prop::Kiln, (22.0, -14.0), 0.1),
            placed(Prop::Kiln, (-24.0, 10.0), FRAC_PI_2),
            placed(Prop::CrateStack, (2.0, 10.0), 0.4),
            placed(Prop::Truck, (12.0, 4.0), 1.2),
        ],
    ));

    // The new town's square, under the water tower.
    all.push(Compound {
        ground: Ground::Paving,
        ..compound(
            (-130.0, 120.0),
            -0.05,
            (22.0, 18.0),
            None,
            vec![hall(s, (0.0, -10.0), 0.0, (13.0, 6.5), cement)],
            vec![placed(Prop::WaterTower, (14.0, 9.0), 0.2), placed(Prop::Wreck, (-12.0, 9.0), 1.1), placed(Prop::Barrier, (2.0, 12.0), 0.1)],
        )
    });

    // The rail yard: two goods sheds, a signal tower, and wagons standing on the sidings.
    all.push(compound(
        (212.0, 230.0),
        0.0,
        (46.0, 30.0),
        None,
        vec![
            barn(s, (-32.0, -23.0), 0.0, (11.0, 7.0), brick, Paint::Tin),
            barn(s, (34.0, 18.0), PI, (10.0, 6.5), cement, Paint::Rust),
            tower(s, (36.0, -18.0), 0.0, (2.9, 4.4), brick),
        ],
        vec![
            placed(Prop::Boxcar, (-14.0, -6.0), FRAC_PI_2),
            placed(Prop::Boxcar, (8.0, 6.0), FRAC_PI_2),
            placed(Prop::Boxcar, (-22.0, 18.0), FRAC_PI_2),
            placed(Prop::Boxcar, (6.0, 18.0), FRAC_PI_2),
            placed(Prop::Drums, (-2.0, -10.0), 0.4),
            placed(Prop::CrateStack, (22.0, -8.0), 0.2),
        ],
    ));

    // Two townhouses by each island bridge, looking over it.
    for (at, yaw) in [((62.0, -66.0), 0.05), (turned((62.0, -66.0)), PI + 0.05)] {
        all.push(compound(at, yaw, (11.0, 7.0), None, vec![townhouse(s, (0.0, 0.0), 0.0, (8.0, 4.5), lime, flat)], Vec::new()));
    }
    all
}

/// Out in the country: the bases with their airfields, the hill forts and the hamlets' squares. Each one on
/// the south side, and the same turned round for the north.
fn country(s: &BuildStyle) -> Vec<Compound> {
    let mut south = vec![
        compound(
            BASE,
            0.0,
            (30.0, 22.0),
            tall(Paint::Sandbag, s.high_wall, &[gate(Side::W, 0.0, 7.0), gate(Side::N, 16.0, 18.0), gate(Side::E, 0.0, 7.0)]),
            vec![shed((-16.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas), shed((0.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas)],
            vec![placed(Prop::CrateStack, (-21.0, 14.0), 0.2), placed(Prop::Hesco, (-20.0, 4.0), FRAC_PI_2)],
        ),
        // Two hangars and a tower at the head of the airstrip.
        compound(
            (BASE.0 + 120.0, BASE.1 - 10.0),
            0.0,
            (60.0, 18.0),
            None,
            vec![barn(s, (-30.0, -4.0), 0.0, (14.0, 9.0), Paint::Tin, Paint::Tin), barn(s, (2.0, -4.0), 0.0, (14.0, 9.0), Paint::Tin, Paint::Tin), tower(s, (40.0, -6.0), 0.0, (2.9, 4.4), Paint::Cement)],
            vec![placed(Prop::Drums, (24.0, 10.0), 0.3), placed(Prop::Truck, (50.0, 10.0), 0.0)],
        ),
        // The airstrip itself, levelled.
        Compound { ground: Ground::Paving, ..compound((BASE.0 + 360.0, BASE.1 + 50.0), 0.0, (300.0, 15.0), None, Vec::new(), Vec::new()) },
        // The fort on its hill, with the radio mast.
        compound(
            (900.0, 450.0),
            0.3,
            (26.0, 20.0),
            tall(Paint::Stone, s.high_wall, &[gate(Side::N, 0.0, 4.0), gate(Side::S, -8.0, 3.0)]),
            vec![tower(s, (-18.0, -12.0), 0.0, (2.9, 4.4), Paint::Stone), tower(s, (18.0, 12.0), PI, (2.9, 4.4), Paint::Stone), house(s, (-10.0, 10.0), PI, (6.0, 3.8), Paint::Stone)],
            vec![placed(Prop::Mast, (8.0, -10.0), 0.0), placed(Prop::Sandbags, (-2.0, 4.0), 0.0), placed(Prop::Hesco, (12.0, 4.0), FRAC_PI_2)],
        ),
        // The hamlets' squares, with their wells.
        compound((-650.0, 1150.0), 0.1, (16.0, 14.0), None, vec![townhouse(s, (0.0, -8.0), 0.0, (8.0, 4.5), Paint::Mudbrick, Roof::Flat { paint: Paint::Packed })], vec![placed(Prop::Well, (-6.0, 6.0), 0.0), placed(Prop::Stall, (6.0, 5.0), 0.2)]),
        compound((1300.0, 1250.0), -0.2, (14.0, 12.0), None, vec![house(s, (0.0, -6.0), 0.0, (7.0, 4.0), Paint::Ochre).roof_stairs(s)], vec![placed(Prop::Well, (-4.0, 6.0), 0.0)]),
    ];
    let north: Vec<Compound> = south.iter().map(|c| Compound { at: Vec2::from(turned((c.at.x, c.at.y))), yaw: c.yaw + PI, ..c.clone() }).collect();
    south.extend(north);
    south
}

/// Fills both banks with walled compounds and the alleys between them, wherever nothing else is: the old
/// quarter thick on the north bank, the new town thinner on the south.
fn quarters(s: &BuildStyle, roads: &[Road], canal: &[Ditch], taken: &[(Vec2, Vec2)]) -> Vec<Compound> {
    let near = |p: Vec2, points: &[Vec2], within: f32| {
        points.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            (a + (b - a) * t).distance(p) < within
        })
    };
    let half = Vec2::from(BLOCK);
    let clear = |at: Vec2| {
        let corners = [Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)].map(|c| at + c * half);
        let off_roads = roads.iter().all(|r| corners.iter().all(|&c| !near(c, &r.points, r.width * 0.5 + ROAD_CLEARANCE)));
        let off_canal = canal.iter().filter(|d| d.cuts_roads).all(|d| corners.iter().all(|&c| !near(c, &d.points, CANAL_CLEARANCE)));
        let off_places = taken.iter().all(|(t, h)| (at - *t).abs().cmpgt(*h + half).any());
        off_roads && off_canal && off_places
    };
    let mut rng = Scatter(77);
    let mut all = Vec::new();
    let pitch = half * 2.0 + Vec2::splat(ALLEY);
    // (min, max, how many of the free blocks get a compound).
    let hamlets = [((-650.0, 1150.0), (120.0, 90.0)), ((1300.0, 1250.0), (90.0, 70.0))];
    let mut regions = vec![(Vec2::new(-260.0, -205.0), Vec2::new(260.0, -60.0), 0.92), (Vec2::new(-260.0, 60.0), Vec2::new(260.0, 205.0), 0.6)];
    for (at, half) in hamlets {
        for at in [at, turned(at)] {
            let (at, half) = (Vec2::from(at), Vec2::from(half));
            regions.push((at - half, at + half, 0.55));
        }
    }
    for (min, max, share) in regions {
        let mut z = min.y + half.y;
        while z + half.y <= max.y {
            let mut x = min.x + half.x;
            while x + half.x <= max.x {
                let at = Vec2::new(x, z);
                let lucky = rng.range(0.0, 1.0) < share;
                if lucky && clear(at) {
                    all.push(walled_house(s, &mut rng, at));
                }
                x += pitch.x;
            }
            z += pitch.y;
        }
    }
    all
}

/// One block's compound: a house along the back wall, sometimes a smaller one across the yard, and gates
/// in the side walls onto the alleys.
fn walled_house(s: &BuildStyle, rng: &mut Scatter, at: Vec2) -> Compound {
    let paints = [Paint::Mudbrick, Paint::Ochre, Paint::Limewash, Paint::Mudbrick];
    let pick = |rng: &mut Scatter, n: usize| ((rng.range(0.0, 1.0) * n as f32) as usize).min(n - 1);
    let walls = paints[pick(rng, paints.len())];
    let yard = paints[pick(rng, paints.len())];
    let (bx, bz) = BLOCK;
    let main_half = (rng.range(6.0, 8.0), 4.5);
    let main_at = (-2.0, -bz + main_half.1 + 1.6);
    let main = match pick(rng, 3) {
        0 => house(s, main_at, 0.0, main_half, walls).roof_stairs(s),
        1 => townhouse(s, main_at, 0.0, main_half, walls, Roof::Flat { paint: Paint::Packed }),
        _ => house(s, main_at, 0.0, main_half, walls),
    };
    let mut buildings = vec![main];
    if rng.range(0.0, 1.0) < 0.5 {
        let half = (5.0, 3.6);
        buildings.push(house(s, (rng.range(-5.0, 5.0), bz - half.1 - 1.6), PI, half, walls));
    }
    let mut gates = vec![gate(if rng.range(0.0, 1.0) < 0.5 { Side::E } else { Side::W }, 0.6, 2.4)];
    if rng.range(0.0, 1.0) < 0.4 {
        gates.push(gate(Side::E, 0.6, 2.4));
        gates.push(gate(Side::W, 0.6, 2.4));
        gates.truncate(2);
    }
    let props = if rng.range(0.0, 1.0) < 0.5 { vec![placed(Prop::Drums, (bx - 2.5, 0.5), rng.range(0.0, 3.0))] } else { Vec::new() };
    compound((at.x, at.y), rng.range(-0.06, 0.06), (bx, bz), tall(yard, s.high_wall, &gates), buildings, props)
}

fn vehicles() -> Vec<(u8, SpawnSpec)> {
    let mut all = Vec::new();
    let (x, z) = BASE;
    // Inside the base pointing out of its wide gate, and more outside it, for the long drive in.
    for (kind, at, yaw) in [
        (0, (x + 10.0, z - 14.0), 0.0),
        (2, (x + 14.0, z - 14.0), 0.0),
        (1, (x + 20.0, z - 14.0), 0.0),
        (0, (x + 24.0, z - 14.0), 0.0),
        (2, (x + 8.0, z - 34.0), 0.0),
        (2, (x + 26.0, z - 34.0), 0.0),
        (0, (x + 34.0, z - 40.0), 0.0),
        (1, (x - 8.0, z - 40.0), 0.0),
    ] {
        all.push((kind, SpawnSpec { at: Vec2::from(at), yaw }));
        all.push((kind, SpawnSpec { at: Vec2::from(turned(at)), yaw: yaw + PI }));
    }
    all
}

fn heaps() -> Vec<Rubble> {
    let mut all = vec![
        rubble((-180.0, -246.0), 4.0, 18, Paint::Brick),
        rubble((-232.0, -214.0), 3.5, 14, Paint::Brick),
        Rubble { at: Vec2::new(-380.0, 176.0), radius: 30.0, count: 12, size: (0.6, 1.6), paint: Paint::Rock },
        Rubble { at: Vec2::new(380.0, -176.0), radius: 30.0, count: 12, size: (0.6, 1.6), paint: Paint::Rock },
    ];
    for at in [(-70.0, -64.0), (160.0, -80.0), (-200.0, -90.0)] {
        all.push(rubble(at, 3.0, 10, Paint::Mudbrick));
        all.push(rubble(turned(at), 3.0, 10, Paint::Limewash));
    }
    // Cover out on the long approaches: outcrops to run between.
    for at in [(400.0, 1200.0), (-200.0, 1000.0), (700.0, 900.0), (-1000.0, 1300.0), (1400.0, 600.0), (-400.0, 600.0), (1100.0, 200.0), (-1300.0, 300.0), (200.0, 600.0)] {
        for at in [at, turned(at)] {
            all.push(Rubble { at: Vec2::from(at), radius: 25.0, count: 10, size: (0.7, 1.8), paint: Paint::Rock });
        }
    }
    all
}

fn street_props() -> Vec<Placed> {
    let mut all = Vec::new();
    for (prop, at, yaw) in [
        (Prop::Wreck, (18.0, 140.0), 0.3),
        (Prop::Barrier, (-24.0, 66.0), 0.2),
        (Prop::Sandbags, (-36.0, 64.0), 0.0),
        (Prop::Wreck, (260.0, 40.0), 1.4),
        // Wrecks along the highway and the ring road.
        (Prop::Wreck, (-1145.0, 1000.0), 0.2),
        (Prop::Truck, (-1165.0, 380.0), 3.0),
        (Prop::Wreck, (-300.0, 690.0), 1.7),
        (Prop::Wreck, (500.0, 735.0), 1.5),
        (Prop::Barrier, (-1150.0, 820.0), 0.1),
    ] {
        all.push(placed(prop, at, yaw));
        all.push(placed(prop, turned(at), yaw + PI));
    }
    all
}

fn woods() -> Vec<Woodland> {
    use TreeKind::{Broadleaf, Palm};
    let palm = ((9.0, 13.0), (2.2, 3.0));
    let mut all = Vec::new();
    for (points, spacing) in [
        // Palms along the canal outside the town.
        (vec![(-500.0, -48.0), (-380.0, -34.0), (-270.0, -24.0)], 9.0),
        (vec![(-500.0, -18.0), (-380.0, -6.0), (-270.0, 6.0)], 11.0),
        // Along the road out of town.
        (vec![(-30.0, 90.0), (-90.0, 116.0)], 12.0),
    ] {
        for half in both(&points) {
            all.push(trees(&half, spacing, Palm, palm.0, palm.1));
        }
    }
    for (at, half, count) in [((-440.0, 20.0), (40.0, 20.0), 40), ((-330.0, 130.0), (30.0, 25.0), 24), ((-460.0, -150.0), (25.0, 40.0), 20)] {
        all.push(clump(at, half, count, Palm, palm.0, palm.1));
        all.push(clump(turned(at), half, count, Palm, palm.0, palm.1));
    }
    // Palm groves along the river out in the country, and the hamlets' orchards.
    for (at, half, count) in [((-800.0, -100.0), (120.0, 35.0), 60), ((-1300.0, 20.0), (150.0, 40.0), 70), ((-1750.0, 80.0), (120.0, 40.0), 50), ((-1000.0, 40.0), (80.0, 25.0), 30)] {
        all.push(clump(at, half, count, Palm, palm.0, palm.1));
        all.push(clump(turned(at), half, count, Palm, palm.0, palm.1));
    }
    for (at, half) in [((-560.0, 1150.0), (40.0, 60.0)), ((1300.0, 1150.0), (50.0, 30.0))] {
        for (at, yaw) in [(at, 0.1), (turned(at), 0.1 + PI)] {
            all.push(Woodland { kind: TreeKind::Fruit, shape: WoodShape::Rows { at: Vec2::from(at), half: Vec2::from(half), yaw, spacing: 9.0 }, height: (4.0, 5.5), radius: (1.6, 2.2) });
        }
    }
    // Woods on the high ground.
    for (at, half, count) in [((-1500.0, 650.0), (200.0, 100.0), 90), ((1100.0, 1520.0), (160.0, 70.0), 60), ((-300.0, -330.0), (90.0, 30.0), 30), ((260.0, -330.0), (70.0, 25.0), 22)] {
        all.push(clump(at, half, count, Broadleaf, (8.0, 12.0), (2.6, 3.8)));
        all.push(clump(turned(at), half, count, Broadleaf, (8.0, 12.0), (2.6, 3.8)));
    }
    all
}
