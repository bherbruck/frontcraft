//! A city on a river, four kilometres across, on land surveyed by NASA's SRTM (public domain) around
//! Fallujah in Iraq. The heights are real; everything on them is made up for the game. The river runs down
//! the west side with a bridge across it in each half of the map, and the city of walled compounds fills the
//! east bank between farmland and palm groves to the north and open desert and a depot to the south.
//!
//! North comes in from the farms and South from the desert. Each side's half is the other's mirrored across
//! the middle of the map, from the river and bridges to the points, so the walks up the chain match.

use crate::atlas::{Dice, Kind, Quarter, Site, base, gate, line, spawn_rows, survey};
use crate::spec::*;
use crate::{Paint, TreeKind};
use glam::Vec2;
use std::f32::consts::PI;

const SIZE: f32 = 4096.0;
/// The height the survey is measured from: about the level of the river's banks.
const DATUM: f32 = 42.0;

/// The middle of the river, north to south. Mirrored across the middle of the map, as everything is.
const RIVER: &[(f32, f32)] = &[(-1250.0, -2040.0), (-1130.0, -1500.0), (-1030.0, -900.0), (-1000.0, -300.0), (-1000.0, 300.0), (-1030.0, 900.0), (-1130.0, 1500.0), (-1250.0, 2040.0)];
const RIVER_HALF: f32 = 95.0;

const NORTH_BASE: (f32, f32) = (900.0, -1800.0);
/// Where each bridge crosses, in the north half. The south one is its mirror.
const BRIDGE_Z: f32 = -700.0;

/// Where the city is thickest, and how far out from there it has thinned to its outskirts.
const HEART: (f32, f32) = (-100.0, 0.0);
const HEART_REACH: f32 = 250.0;

/// How much of each quarter is built, all together. The city's size against what the web build draws.
const DENSITY: f32 = 1.0;

/// Mirrors a point across the middle of the map, north for south.
fn south(p: (f32, f32)) -> (f32, f32) {
    (p.0, -p.1)
}

fn both(points: &[(f32, f32)]) -> [Vec<(f32, f32)>; 2] {
    [points.to_vec(), points.iter().map(|&p| south(p)).collect()]
}

fn road(points: &[(f32, f32)], width: f32, ground: Ground) -> Road {
    Road { points: line(points), width, shoulder: 4.0, ground }
}

/// The river's middle at a distance along the map from north to south.
fn river_at(z: f32) -> f32 {
    let pair = RIVER.windows(2).find(|p| z >= p[0].1 && z <= p[1].1).unwrap_or(&RIVER[..2]);
    let t = (z - pair[0].1) / (pair[1].1 - pair[0].1);
    pair[0].0 + (pair[1].0 - pair[0].0) * t
}

impl MapSpec {
    pub fn fallujah() -> Self {
        let style = BuildStyle::default();
        let mut spec = Self {
            seed: 3343,
            terrain: land(),
            style,
            roads: roads(),
            ditches: ditches(),
            zones: zones(),
            compounds: Vec::new(),
            walls: Vec::new(),
            rubble: rubble(),
            props: Vec::new(),
            woods: woods(),
            teams: vec![
                TeamSpec { name: "North".into(), spawns: spawn_rows((NORTH_BASE.0, NORTH_BASE.1 + 2.0), PI), base_radius: 60.0 },
                TeamSpec { name: "South".into(), spawns: spawn_rows((NORTH_BASE.0, -NORTH_BASE.1 - 2.0), 0.0), base_radius: 60.0 },
            ],
            capture_points: points(),
            // In the north base, firing east out over the fields.
            sentry: SpawnSpec { at: Vec2::new(NORTH_BASE.0 + 50.0, NORTH_BASE.1 - 30.0), yaw: -PI * 0.5 },
            helipads: vec![
                SpawnSpec { at: Vec2::new(NORTH_BASE.0 - 80.0, NORTH_BASE.1), yaw: PI },
                SpawnSpec { at: Vec2::new(NORTH_BASE.0 - 80.0, -NORTH_BASE.1), yaw: 0.0 },
            ],
            vehicles: motor_pools(),
            waters: vec![Water { points: river(), bed: 36.0 - DATUM, bank: 16.0, ground: Ground::Water, quay: None }],
            bridges: bridges(),
            steps: Vec::new(),
            landmarks: Vec::new(),
        };
        spec.compounds = city(&spec);
        spec
    }
}

fn land() -> TerrainSpec {
    TerrainSpec {
        size: SIZE,
        base_cells: 64,
        // Two metres at the finest in the city and round the points, and coarser out in the fields and desert.
        max_splits: 5,
        paint_splits: 4,
        tolerance: 0.25,
        open_tolerance: Some(0.6),
        open_paint_splits: Some(2),
        detail: [((-100.0, 0.0), 1250.0), ((-1550.0, -420.0), 200.0), ((-1550.0, 420.0), 200.0), ((900.0, -1300.0), 260.0), ((900.0, 1300.0), 300.0)]
            .into_iter()
            .chain([((NORTH_BASE.0, NORTH_BASE.1), 160.0), ((NORTH_BASE.0, -NORTH_BASE.1), 160.0)])
            .map(|(at, radius)| (Vec2::from(at), radius))
            .collect(),
        waves: Vec::new(),
        // Low berms out in the desert and across the fields, the only cover between the city and the bases.
        hills: vec![
            Hill { at: Vec2::new(1400.0, -1100.0), radius: Vec2::new(260.0, 50.0), yaw: 0.3, height: 4.0 },
            Hill { at: Vec2::new(1400.0, 1100.0), radius: Vec2::new(260.0, 50.0), yaw: -0.3, height: 4.0 },
            Hill { at: Vec2::new(300.0, -1500.0), radius: Vec2::new(200.0, 40.0), yaw: -0.1, height: 3.0 },
            Hill { at: Vec2::new(300.0, 1500.0), radius: Vec2::new(200.0, 40.0), yaw: 0.1, height: 3.0 },
            Hill { at: Vec2::new(1700.0, 0.0), radius: Vec2::new(60.0, 380.0), yaw: 0.0, height: 5.0 },
        ],
        // The city's ground quieted, so that neighbouring compounds stand at nearly the same level.
        calm: vec![Calm { at: Vec2::new(-100.0, 0.0), radius: 1000.0, fade: 1500.0, keep: 0.35 }],
        rim: Rim { inset: 60.0, height: 4.0, face: 3.0, face_height: 4.5, wander: 10.0 },
        survey: Some(survey(include_str!("survey/fallujah.txt"), SIZE, DATUM)),
    }
}

/// The river's banks as one outline, down the east bank and back up the west.
fn river() -> Vec<Vec2> {
    let edge = SIZE * 0.5 - 8.0;
    let mut points: Vec<Vec2> = Vec::new();
    let mut west = Vec::new();
    for &(x, z) in RIVER {
        let z = z.clamp(-edge, edge);
        points.push(Vec2::new(x + RIVER_HALF, z));
        west.push(Vec2::new(x - RIVER_HALF, z));
    }
    points.extend(west.into_iter().rev());
    points
}

fn bridges() -> Vec<Bridge> {
    [BRIDGE_Z, -BRIDGE_Z]
        .into_iter()
        .map(|z| {
            let x = river_at(z);
            let reach = RIVER_HALF + 22.0;
            Bridge { from: Vec2::new(x - reach, z), to: Vec2::new(x + reach, z), width: 9.0, thickness: 0.8, parapet: 1.0, span: 28.0, paint: Paint::Concrete }
        })
        .collect()
}

fn roads() -> Vec<Road> {
    let mut all = Vec::new();
    let bridge_x = |z: f32| river_at(z);
    for half in [1.0f32, -1.0] {
        let p = |x: f32, z: f32| (x, z * half);
        // From the base round the fields or the desert and into the city.
        all.push(road(&[p(NORTH_BASE.0, NORTH_BASE.1 + 30.0), p(880.0, -1500.0), p(780.0, -1250.0), p(600.0, -1000.0)], 9.0, Ground::Road));
        all.push(road(&[p(NORTH_BASE.0 - 40.0, NORTH_BASE.1), p(300.0, -1750.0), p(-300.0, -1500.0), p(-700.0, -1250.0), p(-900.0, -1000.0)], 7.0, Ground::Track));
        // To the bridge and across, and down the west bank.
        let bx = bridge_x(BRIDGE_Z * half.abs());
        all.push(road(&[p(-150.0, -700.0), p(-600.0, -700.0), p(bx + RIVER_HALF + 22.0, -700.0)], 9.0, Ground::Road));
        all.push(road(&[p(bx - RIVER_HALF - 22.0, -700.0), p(-1500.0, -700.0), p(-1600.0, -1300.0), p(-1700.0, -1900.0)], 7.0, Ground::Road));
        // The city's own grid.
        all.push(road(&[p(-930.0, -1150.0), p(900.0, -1150.0)], 8.0, Ground::Road));
        all.push(road(&[p(-930.0, -360.0), p(1000.0, -360.0)], 8.0, Ground::Road));
        all.push(road(&[p(1300.0, -1700.0), p(1050.0, -900.0), p(1000.0, -360.0)], 7.0, Ground::Track));
    }
    // Through the middle, north to south and east to west, and down the east bank and the west.
    all.push(road(&[(-150.0, -1150.0), (-150.0, 1150.0)], 10.0, Ground::Road));
    all.push(road(&[(-930.0, 0.0), (1600.0, 0.0)], 10.0, Ground::Road));
    all.push(road(&[(-920.0, -1150.0), (-890.0, -300.0), (-890.0, 300.0), (-920.0, 1150.0)], 7.0, Ground::Road));
    all.push(road(&[(500.0, -1150.0), (500.0, 1150.0)], 8.0, Ground::Road));
    all.push(road(&[(-1500.0, -700.0), (-1400.0, 0.0), (-1500.0, 700.0)], 7.0, Ground::Road));
    all
}

/// Irrigation ditches across the fields, and their mirrors.
fn ditches() -> Vec<Ditch> {
    let ditch = |points: &[(f32, f32)]| Ditch { points: line(points), width: 5.0, bed_width: 1.4, depth: 1.4, ground: Ground::DitchBed, cuts_roads: false, quay: None };
    let mut all = Vec::new();
    for half in both(&[(-860.0, -1350.0), (200.0, -1380.0), (1100.0, -1400.0)]) {
        all.push(ditch(&half));
    }
    for half in both(&[(-1850.0, -1100.0), (-1300.0, -1050.0)]) {
        all.push(ditch(&half));
    }
    for half in both(&[(-1850.0, -300.0), (-1250.0, -320.0)]) {
        all.push(ditch(&half));
    }
    all
}

fn zones() -> Vec<Zone> {
    let zone = |at: (f32, f32), half: (f32, f32), yaw: f32, ground, ragged| Zone { at: at.into(), half: half.into(), yaw, ground, ragged };
    let mut all = vec![
        // Desert everywhere, then grazing along the west bank.
        zone((0.0, 0.0), (2048.0, 2048.0), 0.0, Ground::Sand, 0.0),
        zone((-1600.0, 0.0), (420.0, 2048.0), 0.0, Ground::Pasture, 50.0),
    ];
    // Farmland as a patchwork of fields, each its own crop or left fallow, a little crooked, with worn edges.
    let mut dice = Dice::new(41);
    let crops = [Ground::Wheat, Ground::Plough, Ground::Stubble, Ground::Wheat, Ground::Pasture, Ground::Plough];
    for (at, half) in [
        ((-1600.0, -1500.0), (300.0, 250.0)),
        ((-1600.0, -600.0), (280.0, 300.0)),
        ((-1650.0, 150.0), (240.0, 140.0)),
        ((-400.0, -1600.0), (450.0, 200.0)),
        ((300.0, -1700.0), (250.0, 150.0)),
        ((1200.0, -1600.0), (300.0, 250.0)),
    ] {
        let (nx, nz) = ((half.0 / 90.0) as i32, (half.1 / 70.0) as i32);
        let field = (half.0 * 2.0 / nx as f32, half.1 * 2.0 / nz as f32);
        for j in 0..nz {
            for i in 0..nx {
                if dice.chance(0.15) {
                    continue;
                }
                let centre = (at.0 - half.0 + field.0 * (i as f32 + 0.5), at.1 - half.1 + field.1 * (j as f32 + 0.5));
                let size = (field.0 * 0.5 - dice.range(2.0, 6.0), field.1 * 0.5 - dice.range(2.0, 6.0));
                let (ground, yaw) = (dice.pick(&crops), dice.range(-0.06, 0.06));
                all.push(zone(centre, size, yaw, ground, 6.0));
                all.push(zone(south(centre), size, -yaw, ground, 6.0));
            }
        }
    }
    // Orchard ground under the date groves.
    for (at, half) in [((-1280.0, -1300.0), (80.0, 170.0)), ((-1270.0, -150.0), (80.0, 170.0)), ((-860.0, -1500.0), (80.0, 170.0))] {
        all.push(zone(at, half, 0.05, Ground::Orchard, 15.0));
        all.push(zone(south(at), half, 0.05, Ground::Orchard, 15.0));
    }
    all
}

fn points() -> Vec<CapturePointSpec> {
    let north = [("Farmstead", (900.0, -1300.0), 30.0), ("Mill", (-560.0, -1060.0), 28.0), ("Market", (300.0, -560.0), 30.0)];
    let southern = [("Depot", 30.0), ("Pump House", 28.0), ("Bus Station", 30.0)];
    let mut all: Vec<CapturePointSpec> = north.iter().map(|&(name, at, radius)| CapturePointSpec { name: name.into(), at: at.into(), radius }).collect();
    all.push(CapturePointSpec { name: "Central Square".into(), at: Vec2::new(-100.0, 0.0), radius: 32.0 });
    for (k, (name, radius)) in southern.iter().enumerate() {
        all.push(CapturePointSpec { name: (*name).into(), at: south(north[k].1).into(), radius: *radius });
    }
    all
}

const POOL: (f32, f32) = (NORTH_BASE.0 + 75.0, NORTH_BASE.1 + 10.0);

/// Each base's vehicles side by side on a level pool, pointing out along open ground. The south pool is the
/// north one mirrored.
fn motor_pools() -> Vec<(u8, SpawnSpec)> {
    let kinds = [0u8, 1, 2, 0, 2, 1];
    let mut pools = Vec::new();
    for (k, kind) in kinds.iter().enumerate() {
        let along = (k as f32 - 2.5) * 6.0;
        pools.push((*kind, SpawnSpec { at: Vec2::new(POOL.0 + along, POOL.1), yaw: PI }));
    }
    for (k, kind) in kinds.iter().enumerate() {
        let along = (k as f32 - 2.5) * 6.0;
        pools.push((*kind, SpawnSpec { at: Vec2::new(POOL.0 + along, -POOL.1), yaw: 0.0 }));
    }
    pools
}

fn rubble() -> Vec<Rubble> {
    let heap = |at: (f32, f32), radius: f32, count: u32, paint| Rubble { at: at.into(), radius, count, size: (0.3, 0.9), paint };
    let mut all = Vec::new();
    for at in [(-400.0, -800.0), (150.0, -300.0), (650.0, -820.0), (-700.0, -450.0), (300.0, -1000.0)] {
        all.push(heap(at, 4.0, 14, Paint::Mudbrick));
        all.push(heap(south(at), 4.0, 14, Paint::Cement));
    }
    all
}

fn woods() -> Vec<Woodland> {
    let palms = |at: (f32, f32), half: (f32, f32), spacing: f32| Woodland {
        kind: TreeKind::Palm,
        shape: WoodShape::Rows { at: at.into(), half: half.into(), yaw: 0.05, spacing },
        height: (8.0, 12.0),
        radius: (2.0, 2.8),
    };
    let mut all = Vec::new();
    // Date groves along both banks of the river, and orchards on the west bank.
    for at in [(-1280.0, -1300.0), (-1270.0, -150.0), (-860.0, -1500.0)] {
        all.push(palms(at, (70.0, 160.0), 14.0));
        all.push(palms(south(at), (70.0, 160.0), 14.0));
    }
    for at in [(-1800.0, -900.0), (-1750.0, -250.0)] {
        all.push(Woodland { kind: TreeKind::Fruit, shape: WoodShape::Rows { at: at.into(), half: Vec2::new(110.0, 90.0), yaw: 0.0, spacing: 11.0 }, height: (4.0, 5.5), radius: (1.8, 2.4) });
        all.push(Woodland { kind: TreeKind::Fruit, shape: WoodShape::Rows { at: south(at).into(), half: Vec2::new(110.0, 90.0), yaw: 0.0, spacing: 11.0 }, height: (4.0, 5.5), radius: (1.8, 2.4) });
    }
    // Lines of palms along the main roads.
    all.push(Woodland { kind: TreeKind::Palm, shape: WoodShape::Line { points: line(&[(-880.0, -14.0), (900.0, -14.0)]), spacing: 22.0, jitter: 1.0 }, height: (8.0, 11.0), radius: (2.0, 2.6) });
    all.push(Woodland { kind: TreeKind::Palm, shape: WoodShape::Line { points: line(&[(-880.0, 14.0), (900.0, 14.0)]), spacing: 22.0, jitter: 1.0 }, height: (8.0, 11.0), radius: (2.0, 2.6) });
    all
}

/// The bases and pools placed by hand, then the city filled quarter by quarter, each north quarter with its
/// southern mirror.
fn city(spec: &MapSpec) -> Vec<Compound> {
    let s = &spec.style;
    let mut dice = Dice::new(spec.seed);
    let pool = |at: (f32, f32)| Compound { at: at.into(), yaw: 0.0, half: Vec2::new(20.0, 10.0), lift: 0.0, ground: Ground::Paving, wall: None, buildings: Vec::new(), props: Vec::new() };
    let mut built = vec![
        // Each with its back to the edge of the map and its wide gate towards the city.
        base(s, NORTH_BASE.into(), 0.0, &[gate(Side::S, 0.0, 10.0), gate(Side::E, 0.0, 8.0)]),
        base(s, south(NORTH_BASE).into(), PI, &[gate(Side::S, 0.0, 10.0), gate(Side::W, 0.0, 8.0)]),
        pool(POOL),
        pool(south(POOL)),
    ];
    let mut with_these = spec.clone();
    with_these.compounds = built.clone();
    let mut site = Site::new(&with_these);
    for point in &spec.capture_points {
        site.reserve(point.at, 0.0, Vec2::splat(point.radius + 4.0));
    }
    for helipad in &spec.helipads {
        site.reserve(helipad.at, 0.0, Vec2::splat(22.0));
    }
    // The ground each pool drives out over, towards the middle of the map, and the sentry's line of fire.
    for z in [POOL.1, -POOL.1] {
        site.reserve(Vec2::new(POOL.0, z - z.signum() * 60.0), 0.0, Vec2::new(26.0, 70.0));
    }
    site.reserve(Vec2::new(NORTH_BASE.0 + 95.0, NORTH_BASE.1 - 30.0), 0.0, Vec2::new(45.0, 6.0));
    let homes: &'static [Paint] = &[Paint::Mudbrick, Paint::Ochre, Paint::Plaster, Paint::Cement, Paint::Mudbrick];
    let works: &'static [Paint] = &[Paint::Cement, Paint::Concrete, Paint::Brick];
    let quarter = |at: (f32, f32), half: (f32, f32), kind: Kind, density: f32, paints: &'static [Paint]| Quarter {
        at: at.into(),
        yaw: 0.0,
        half: half.into(),
        block: match kind {
            Kind::Souq => Vec2::new(30.0, 22.0),
            Kind::Courtyard => Vec2::new(28.0, 26.0),
            Kind::Works => Vec2::new(46.0, 32.0),
        },
        street: match kind {
            Kind::Souq => 5.0,
            Kind::Courtyard => 6.0,
            Kind::Works => 10.0,
        },
        kind,
        density: (density * DENSITY).min(1.0),
        drop: 3.0,
        paints,
        heart: None,
    };
    let city = |q: Quarter| Quarter { heart: Some((HEART.into(), HEART_REACH)), ..q };
    // Each north quarter beside its southern mirror. The heart of the city round the square is packed, the
    // quarters out from it thin out towards the edge, and the farms to the north answer the depot to the south.
    let pairs = [
        (quarter((300.0, -560.0), (110.0, 70.0), Kind::Souq, 0.6, homes), None),
        // The city's half, packed round the square and along its roads, thinning out towards the fields.
        (city(quarter((-50.0, -560.0), (650.0, 520.0), Kind::Courtyard, 1.0, homes)), None),
        (quarter((-600.0, -1060.0), (200.0, 70.0), Kind::Works, 0.6, works), None),
        (
            quarter((900.0, -1220.0), (220.0, 120.0), Kind::Courtyard, 0.2, homes),
            Some(quarter((900.0, 1200.0), (220.0, 130.0), Kind::Works, 0.5, works)),
        ),
        // A village on the west bank.
        (quarter((-1550.0, -420.0), (150.0, 140.0), Kind::Courtyard, 0.3, homes), None),
    ];
    for (north, southern) in pairs {
        built.extend(north.fill(s, &mut dice, &mut site));
        let southern = southern.unwrap_or(Quarter { at: Vec2::new(north.at.x, -north.at.y), yaw: PI, ..north });
        built.extend(southern.fill(s, &mut dice, &mut site));
    }
    // Something to fight round at the market and the bus station, and in the square.
    for at in [(300.0, -560.0), (300.0, 560.0), (-100.0, 0.0)] {
        let mut square = crate::atlas::square(at.into(), 0.0, Vec2::new(20.0, 16.0), &mut dice);
        square.props.retain(|p| p.at.length() > 6.0);
        built.push(square);
    }
    built
}
