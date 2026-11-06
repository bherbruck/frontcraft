//! A harbour town on a bay, two kilometres across, on land surveyed by NASA's SRTM (public domain) around
//! Muttrah in Oman. The heights are real; everything on them is made up for the game. The sea is to the
//! north, a port at its west end and a dhow harbour at its east, and between them the corniche along the
//! bay with the souq packed in behind it, the old quarter climbing the foot of the rocky hills, a wadi
//! running back into them, and a fort on the ridge over the east end of the bay.
//!
//! The two sides come in along the shore, West from the port and East from beyond the dhow harbour, and
//! fight over the town between.

use crate::atlas::{Dice, Kind, Quarter, Site, base, fort, gate, line, spawn_rows, survey};
use crate::spec::*;
use crate::{Paint, TreeKind};
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI};

const SIZE: f32 = 2048.0;

fn road(points: &[(f32, f32)], width: f32, ground: Ground) -> Road {
    Road { points: line(points), width, shoulder: 4.0, ground }
}

const CORNICHE: &[(f32, f32)] = &[
    (-818.0, -700.0),
    (-760.0, -740.0),
    (-700.0, -768.0),
    (-580.0, -768.0),
    (-520.0, -800.0),
    (-440.0, -808.0),
    (-330.0, -768.0),
    (-200.0, -733.0),
    (-60.0, -713.0),
    (80.0, -713.0),
    (220.0, -733.0),
    (340.0, -768.0),
    (420.0, -803.0),
    (480.0, -762.0),
    (520.0, -748.0),
    (680.0, -748.0),
    (780.0, -700.0),
    (828.0, -660.0),
];

/// The shore from west to east, then round by the north edge of the map, where the sea wall at the edge
/// of the map stands in it. Inside is sea.
const SEA: &[(f32, f32)] = &[
    (-1016.0, -860.0),
    (-700.0, -860.0),
    (-700.0, -790.0),
    (-600.0, -790.0),
    (-600.0, -860.0),
    (-520.0, -860.0),
    (-440.0, -830.0),
    (-330.0, -790.0),
    (-200.0, -755.0),
    (-60.0, -735.0),
    (80.0, -735.0),
    (220.0, -755.0),
    (340.0, -790.0),
    (420.0, -825.0),
    (500.0, -850.0),
    (510.0, -770.0),
    (680.0, -770.0),
    (690.0, -850.0),
    (690.0, -930.0),
    (715.0, -930.0),
    (715.0, -865.0),
    (800.0, -880.0),
    (1016.0, -895.0),
    (1016.0, -1016.0),
    (-1016.0, -1016.0),
];

const WEST_BASE: (f32, f32) = (-850.0, -700.0);
const EAST_BASE: (f32, f32) = (860.0, -660.0);

impl MapSpec {
    pub fn muttrah() -> Self {
        let style = BuildStyle::default();
        let mut spec = Self {
            seed: 2358,
            terrain: land(),
            style,
            roads: roads(),
            ditches: Vec::new(),
            zones: zones(),
            compounds: Vec::new(),
            walls: Vec::new(),
            rubble: rocks(),
            props: Vec::new(),
            woods: woods(),
            teams: vec![
                TeamSpec { name: "West".into(), spawns: spawn_rows((WEST_BASE.0 + 2.0, WEST_BASE.1 + 6.0), -FRAC_PI_2), base_radius: 60.0 },
                TeamSpec { name: "East".into(), spawns: spawn_rows((EAST_BASE.0 - 2.0, EAST_BASE.1 - 6.0), FRAC_PI_2), base_radius: 60.0 },
            ],
            capture_points: points(),
            // Outside the west base, firing west over open ground towards the edge of the map.
            sentry: SpawnSpec { at: Vec2::new(-905.0, -700.0), yaw: FRAC_PI_2 },
            helipads: vec![SpawnSpec { at: Vec2::new(-920.0, -560.0), yaw: -FRAC_PI_2 }, SpawnSpec { at: Vec2::new(925.0, -540.0), yaw: FRAC_PI_2 }],
            vehicles: motor_pools(),
            waters: vec![Water {
                points: line(SEA),
                bed: -3.5,
                bank: 2.0,
                ground: Ground::Water,
                quay: Some(Quay { beyond: 0.6, over_bed: 0.5, length: 6.0, paint: Paint::Stone }),
            }],
            bridges: Vec::new(),
            steps: landings(),
            landmarks: Vec::new(),
        };
        spec.compounds = town(&spec);
        spec
    }
}

/// Steps down from the quays into the water every so often, so whoever goes in can get out.
fn landings() -> Vec<Steps> {
    // Which stretches of shore, by the index of the point they start from.
    [1usize, 3, 6, 8, 10, 12, 15, 21]
        .into_iter()
        .map(|k| {
            let (a, b) = (Vec2::from(SEA[k]), Vec2::from(SEA[k + 1]));
            let across = (b - a).perp().normalize();
            let out = if across.y < 0.0 { across } else { -across };
            let mid = a.lerp(b, 0.5);
            Steps { from: mid - out * 4.0, to: mid + out * 7.0, width: 2.4, paint: Paint::Stone }
        })
        .collect()
}

fn land() -> TerrainSpec {
    TerrainSpec {
        size: SIZE,
        base_cells: 64,
        max_splits: 4,
        paint_splits: 3,
        tolerance: 0.25,
        open_tolerance: None,
        open_paint_splits: None,
        detail: Vec::new(),
        waves: Vec::new(),
        hills: Vec::new(),
        // The strip along the bay is quieted, so the town stands on gentle ground and the quays are level.
        calm: vec![
            Calm { at: Vec2::new(0.0, -1000.0), radius: 650.0, fade: 1000.0, keep: 0.3 },
            // Flatter still under the souq, so its alleys are level from one block to the next.
            Calm { at: Vec2::new(0.0, -590.0), radius: 250.0, fade: 380.0, keep: 0.05 },
        ],
        rim: Rim { inset: 45.0, height: 3.0, face: 3.0, face_height: 4.5, wander: 6.0 },
        survey: Some(survey(include_str!("survey/muttrah.txt"), SIZE, 0.0)),
    }
}

fn roads() -> Vec<Road> {
    vec![
        road(CORNICHE, 10.0, Ground::Road),
        // The back street behind the souq, round through the wadi and over the saddle to the east base.
        road(
            &[
                (-850.0, -676.0),
                (-830.0, -560.0),
                (-700.0, -480.0),
                (-400.0, -470.0),
                (-100.0, -460.0),
                (150.0, -470.0),
                (240.0, -400.0),
                (290.0, -260.0),
                (320.0, -120.0),
                (370.0, 20.0),
                (520.0, 90.0),
                (700.0, 40.0),
                (830.0, -80.0),
                (870.0, -400.0),
                (860.0, -636.0),
            ],
            8.0,
            Ground::Road,
        ),
        // Up the ridge to the fort's gate.
        road(&[(320.0, -150.0), (400.0, -120.0), (450.0, -140.0), (488.0, -168.0)], 5.0, Ground::Track),
        // Lanes from the corniche up through the town to the back street.
        road(&[(-330.0, -768.0), (-310.0, -600.0), (-300.0, -470.0)], 6.0, Ground::Road),
        road(&[(220.0, -733.0), (200.0, -600.0), (160.0, -470.0)], 6.0, Ground::Road),
        road(&[(-580.0, -768.0), (-560.0, -620.0), (-540.0, -476.0)], 6.0, Ground::Road),
        // Into the west hills, past the hillside houses.
        road(&[(-560.0, -476.0), (-470.0, -320.0), (-420.0, -200.0), (-360.0, -60.0)], 5.0, Ground::Track),
    ]
}

fn zones() -> Vec<Zone> {
    let zone = |at: (f32, f32), half: (f32, f32), yaw: f32, ground| Zone { at: at.into(), half: half.into(), yaw, ground };
    vec![
        // Bare rock everywhere the town and the shore do not reach, then sand along the shore and in the wadi.
        zone((0.0, 0.0), (1024.0, 1024.0), 0.0, Ground::Rock),
        zone((0.0, -640.0), (1024.0, 400.0), 0.0, Ground::Sand),
        zone((-890.0, 0.0), (140.0, 260.0), 0.0, Ground::Sand),
        zone((330.0, -60.0), (110.0, 230.0), 0.3, Ground::Sand),
        zone((560.0, 120.0), (90.0, 60.0), -0.2, Ground::Sand),
        zone((830.0, -120.0), (60.0, 220.0), 0.0, Ground::Sand),
        // The paved port and the quay round the dhow harbour.
        zone((-780.0, -700.0), (200.0, 150.0), 0.0, Ground::Paving),
        zone((600.0, -790.0), (110.0, 50.0), 0.0, Ground::Paving),
    ]
}

fn points() -> Vec<CapturePointSpec> {
    [
        ("Port Gate", (-580.0, -710.0), 30.0),
        ("Corniche", (-120.0, -710.0), 28.0),
        ("Hillside", (-400.0, -220.0), 28.0),
        ("Souq", (0.0, -590.0), 30.0),
        ("Fish Market", (295.0, -722.0), 28.0),
        ("Fort", (515.0, -175.0), 15.0),
        ("Dhow Harbour", (590.0, -738.0), 28.0),
    ]
    .into_iter()
    .map(|(name, at, radius)| CapturePointSpec { name: name.into(), at: at.into(), radius })
    .collect()
}

/// Each base's trucks, armoured cars and jeeps side by side on a level pool beside it, pointing along open
/// ground. The east pool is the west one turned half round.
const WEST_POOL: (f32, f32) = (-866.0, -622.0);
const EAST_POOL: (f32, f32) = (876.0, -600.0);

fn motor_pools() -> Vec<(u8, SpawnSpec)> {
    let kinds = [0u8, 1, 2, 0, 2, 1];
    let mut pools = Vec::new();
    for (k, kind) in kinds.iter().enumerate() {
        let along = (k as f32 - 2.5) * 6.0;
        pools.push((*kind, SpawnSpec { at: Vec2::new(WEST_POOL.0, WEST_POOL.1 + along), yaw: -FRAC_PI_2 }));
    }
    for (k, kind) in kinds.iter().enumerate() {
        let along = (k as f32 - 2.5) * 6.0;
        pools.push((*kind, SpawnSpec { at: Vec2::new(EAST_POOL.0, EAST_POOL.1 - along), yaw: FRAC_PI_2 }));
    }
    pools
}

fn rocks() -> Vec<Rubble> {
    let rock = |at: (f32, f32), radius: f32, count: u32| Rubble { at: at.into(), radius, count, size: (0.7, 2.2), paint: Paint::Rock };
    vec![
        rock((-300.0, 150.0), 60.0, 30),
        rock((-600.0, 300.0), 70.0, 30),
        rock((-200.0, 450.0), 80.0, 30),
        rock((0.0, 250.0), 60.0, 24),
        rock((150.0, 600.0), 90.0, 30),
        rock((-450.0, 700.0), 90.0, 30),
        rock((550.0, 300.0), 70.0, 26),
        rock((800.0, 150.0), 60.0, 22),
        rock((700.0, 650.0), 90.0, 30),
        rock((450.0, -300.0), 30.0, 14),
        rock((560.0, -90.0), 30.0, 14),
        rock((-800.0, 100.0), 60.0, 20),
    ]
}

fn woods() -> Vec<Woodland> {
    let palms = |points: &[(f32, f32)], spacing: f32| Woodland {
        kind: TreeKind::Palm,
        shape: WoodShape::Line { points: line(points), spacing, jitter: 1.0 },
        height: (7.0, 10.0),
        radius: (1.8, 2.6),
    };
    // Along the corniche on the sea side, set back from the kerb.
    let promenade: Vec<(f32, f32)> = CORNICHE[4..13].iter().map(|&(x, z)| (x, z - 11.5)).collect();
    vec![
        palms(&promenade, 16.0),
        // Gardens in the wadi, and scattered trees in the hills.
        Woodland { kind: TreeKind::Palm, shape: WoodShape::Clump { at: Vec2::new(330.0, -40.0), half: Vec2::new(40.0, 70.0), yaw: 0.3, count: 30 }, height: (6.0, 9.0), radius: (1.8, 2.6) },
        Woodland { kind: TreeKind::Broadleaf, shape: WoodShape::Clump { at: Vec2::new(-300.0, 300.0), half: Vec2::new(300.0, 300.0), yaw: 0.0, count: 25 }, height: (3.0, 5.0), radius: (1.5, 2.4) },
        Woodland { kind: TreeKind::Broadleaf, shape: WoodShape::Clump { at: Vec2::new(500.0, 400.0), half: Vec2::new(300.0, 300.0), yaw: 0.0, count: 25 }, height: (3.0, 5.0), radius: (1.5, 2.4) },
    ]
}

/// Level paving for a motor pool, so nothing parked on it leans.
fn pool(at: (f32, f32)) -> Compound {
    Compound { at: at.into(), yaw: 0.0, half: Vec2::new(10.0, 20.0), lift: 0.0, ground: Ground::Paving, wall: None, buildings: Vec::new(), props: Vec::new() }
}

/// Everything built: the bases and the fort placed by hand, then each quarter filled around them.
fn town(spec: &MapSpec) -> Vec<Compound> {
    let s = &spec.style;
    let mut dice = Dice::new(spec.seed);
    let mut built = vec![
        base(s, WEST_BASE.into(), 0.0, &[gate(Side::E, 0.0, 10.0), gate(Side::S, 0.0, 8.0)]),
        base(s, EAST_BASE.into(), PI, &[gate(Side::E, 0.0, 10.0), gate(Side::N, 0.0, 8.0)]),
        pool(WEST_POOL),
        pool(EAST_POOL),
        fort(s, Vec2::new(515.0, -175.0), -0.9, Vec2::new(24.0, 16.0), 4.2, &[gate(Side::S, -10.0, 4.0), gate(Side::W, 4.0, 3.0)]),
    ];
    let mut with_these = spec.clone();
    with_these.compounds = built.clone();
    let mut site = Site::new(&with_these);
    // Room to fight at each point, and the strips the motor pools drive out along.
    for point in &spec.capture_points {
        site.reserve(point.at, 0.0, Vec2::splat(point.radius + 4.0));
    }
    site.reserve(Vec2::new(WEST_POOL.0 + 90.0, WEST_POOL.1), 0.0, Vec2::new(110.0, 24.0));
    site.reserve(Vec2::new(EAST_POOL.0 - 90.0, EAST_POOL.1), 0.0, Vec2::new(110.0, 24.0));
    site.reserve(Vec2::new(-920.0, -560.0), 0.0, Vec2::splat(22.0));
    site.reserve(Vec2::new(925.0, -540.0), 0.0, Vec2::splat(22.0));
    site.reserve(Vec2::new(-950.0, -700.0), 0.0, Vec2::new(60.0, 10.0));
    let quarters = [
        // The souq: blocks shoulder to shoulder from the corniche back to the back street.
        Quarter { at: Vec2::new(0.0, -590.0), yaw: 0.0, half: Vec2::new(230.0, 100.0), block: Vec2::new(30.0, 22.0), street: 5.0, kind: Kind::Souq, density: 0.9, drop: 2.5, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre, Paint::Limewash] },
        // Warehouses on the port.
        Quarter { at: Vec2::new(-690.0, -680.0), yaw: 0.0, half: Vec2::new(110.0, 80.0), block: Vec2::new(44.0, 30.0), street: 9.0, kind: Kind::Works, density: 1.0, drop: 2.5, paints: &[Paint::Cement, Paint::Concrete, Paint::Brick] },
        // The old quarter, walled houses up the foot of the hills.
        Quarter { at: Vec2::new(-470.0, -470.0), yaw: 0.05, half: Vec2::new(180.0, 150.0), block: Vec2::new(26.0, 22.0), street: 6.0, kind: Kind::Courtyard, density: 0.7, drop: 3.0, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre, Paint::Stone] },
        Quarter { at: Vec2::new(-80.0, -330.0), yaw: -0.04, half: Vec2::new(230.0, 110.0), block: Vec2::new(26.0, 22.0), street: 6.0, kind: Kind::Courtyard, density: 0.6, drop: 3.0, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre, Paint::Stone] },
        Quarter { at: Vec2::new(-450.0, -170.0), yaw: 0.1, half: Vec2::new(150.0, 80.0), block: Vec2::new(26.0, 22.0), street: 8.0, kind: Kind::Courtyard, density: 0.5, drop: 4.0, paints: &[Paint::Limewash, Paint::Plaster, Paint::Stone] },
        // The wadi, and the east end behind the dhow harbour.
        Quarter { at: Vec2::new(300.0, -170.0), yaw: 0.3, half: Vec2::new(110.0, 170.0), block: Vec2::new(26.0, 22.0), street: 7.0, kind: Kind::Courtyard, density: 0.6, drop: 3.5, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre] },
        Quarter { at: Vec2::new(720.0, -480.0), yaw: -0.1, half: Vec2::new(120.0, 140.0), block: Vec2::new(26.0, 22.0), street: 7.0, kind: Kind::Courtyard, density: 0.5, drop: 3.5, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre] },
        Quarter { at: Vec2::new(600.0, -650.0), yaw: 0.0, half: Vec2::new(100.0, 60.0), block: Vec2::new(30.0, 22.0), street: 6.0, kind: Kind::Souq, density: 0.8, drop: 2.5, paints: &[Paint::Limewash, Paint::Plaster, Paint::Ochre] },
        Quarter { at: Vec2::new(760.0, -815.0), yaw: -0.1, half: Vec2::new(45.0, 30.0), block: Vec2::new(40.0, 26.0), street: 6.0, kind: Kind::Works, density: 1.0, drop: 3.0, paints: &[Paint::Cement, Paint::Brick] },
    ];
    for quarter in &quarters {
        built.extend(quarter.fill(s, &mut dice, &mut site));
    }
    // Something to fight round at the market point, in the middle of the souq.
    let mut market = crate::atlas::square(Vec2::new(0.0, -590.0), 0.0, Vec2::new(20.0, 14.0), &mut dice);
    market.props.retain(|p| p.at.length() > 6.0);
    built.push(market);
    built
}
