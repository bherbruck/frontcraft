//! The town: a kilometre of land fought over between a northern and a southern team. A village at
//! the crossroads, farms and fields to the west, an orchard and a mill to the east, and a ridge
//! across each approach, so the bases never see each other and the town is only seen once you crest.

use crate::archetypes::{barn, cottage, hall, house, shed, shop_row, tower, townhouse};
use crate::spec::*;
use crate::{Paint, TreeKind};
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn line(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&p| p.into()).collect()
}

pub(crate) fn road(points: &[(f32, f32)], width: f32, ground: Ground) -> Road {
    Road { points: line(points), width, shoulder: 5.0, ground }
}

pub(crate) fn ditch(points: &[(f32, f32)]) -> Ditch {
    Ditch { points: line(points), width: 5.2, bed_width: 1.4, depth: 1.35, ground: Ground::DitchBed, cuts_roads: false }
}

pub(crate) fn zone(at: (f32, f32), half: (f32, f32), yaw: f32, ground: Ground) -> Zone {
    Zone { at: at.into(), half: half.into(), yaw, ground }
}

pub(crate) fn hill(at: (f32, f32), radius: (f32, f32), yaw: f32, height: f32) -> Hill {
    Hill { at: at.into(), radius: radius.into(), yaw, height }
}

pub(crate) fn wave(fx: f32, fz: f32, phase: f32, height: f32) -> Wave {
    Wave { frequency: Vec2::new(fx, fz), phase, height }
}

pub(crate) fn gate(side: Side, along: f32, width: f32) -> Gate {
    Gate { side, along, width }
}

pub(crate) fn placed(prop: Prop, at: (f32, f32), yaw: f32) -> Placed {
    Placed { prop, at: at.into(), yaw }
}

pub(crate) fn tall(paint: Paint, height: f32, gates: &[Gate]) -> Option<YardWall> {
    Some(YardWall { height, paint, gates: gates.to_vec() })
}

/// A compound that takes whatever the land gives it.
pub(crate) fn compound(at: (f32, f32), yaw: f32, half: (f32, f32), wall: Option<YardWall>, buildings: Vec<Building>, props: Vec<Placed>) -> Compound {
    Compound { at: at.into(), yaw, half: half.into(), lift: 0.0, ground: Ground::Yard, wall, buildings, props }
}

pub(crate) fn field_wall(points: &[(f32, f32)], height: f32, unevenness: f32, paint: Paint, gaps: &[(f32, f32)]) -> FieldWall {
    let s = BuildStyle::default();
    let (thickness, segment, wander) = if paint == Paint::Hedge { (s.hedge_thickness, s.hedge_segment, 0.22) } else { (s.field_wall, s.wall_segment, 0.0) };
    FieldWall { points: line(points), height, unevenness, thickness, segment, wander, paint, gaps: gaps.to_vec() }
}

pub(crate) fn rubble(at: (f32, f32), radius: f32, count: u32, paint: Paint) -> Rubble {
    Rubble { at: at.into(), radius, count, size: (0.3, 0.8), paint }
}

pub(crate) fn trees(points: &[(f32, f32)], spacing: f32, kind: TreeKind, height: (f32, f32), radius: (f32, f32)) -> Woodland {
    Woodland { kind, shape: WoodShape::Line { points: line(points), spacing, jitter: spacing * 0.2 }, height, radius }
}

pub(crate) fn clump(at: (f32, f32), half: (f32, f32), count: u32, kind: TreeKind, height: (f32, f32), radius: (f32, f32)) -> Woodland {
    Woodland { kind, shape: WoodShape::Clump { at: at.into(), half: half.into(), yaw: 0.3, count }, height, radius }
}

/// Twelve spawn places in two rows, looking the way the team has to go.
pub(crate) fn spawn_rows(at: (f32, f32), yaw: f32) -> Vec<SpawnSpec> {
    let mut spawns = Vec::new();
    for row in 0..2 {
        for i in 0..6 {
            spawns.push(SpawnSpec { at: Vec2::new(at.0 + i as f32 * 3.5 - 8.75, at.1 + row as f32 * 3.5), yaw });
        }
    }
    spawns
}

impl MapSpec {
    pub fn town() -> Self {
        let style = BuildStyle::default();
        Self {
            seed: 1307,
            terrain: land(),
            style,
            roads: roads(),
            ditches: ditches(),
            zones: fields(),
            compounds: compounds(&style),
            walls: walls(&style),
            rubble: heaps(),
            props: street_props(),
            woods: woods(),
            teams: vec![
                TeamSpec { name: "South".into(), spawns: spawn_rows((95.0, 426.0), 0.0), base_radius: 60.0 },
                TeamSpec { name: "North".into(), spawns: spawn_rows((-5.0, -429.0), PI), base_radius: 60.0 },
            ],
            capture_points: [
                // Each flag stands in the open. Each side's points are the other's turned half way round
                // the middle of the map, since being nearer to where the fight is decides a match: Mill
                // for Hilltop, and East Quarter for Market, which the two sides then fight over.
                ("Saddle", (28.5, 255.4), 28.0),
                ("Market", (-30.0, 6.0), 38.0),
                ("East Quarter", (120.0, -14.0), 38.0),
                ("Mill", (280.0, 212.0), 36.0),
                ("Hilltop", (-200.8, -224.4), 30.0),
                ("Ridge Gap", (83.6, -260.4), 28.0),
            ]
            .into_iter()
            .map(|(name, at, radius)| CapturePointSpec { name: name.into(), at: at.into(), radius })
            .collect(),
            // In the southern base, firing out of its open west gate and away down the land.
            sentry: SpawnSpec { at: Vec2::new(79.0, 425.0), yaw: FRAC_PI_2 },
            // At the sides of the bases, clear of the spawns, the sheds and the way to the rear gates.
            helipads: vec![
                SpawnSpec { at: Vec2::new(115.0, 427.0), yaw: 0.0 },
                SpawnSpec { at: Vec2::new(-25.0, -427.0), yaw: PI },
            ],
            vehicles: vec![
                // Inside each base with a clear run out of its gate: two at the west gate, and two each side of the wide
                // one in front, pointing straight out of it with the middle left clear to walk through. Two jeeps wait
                // outside it. The north base is the south one turned half round.
                (0, SpawnSpec { at: Vec2::new(77.0, 434.0), yaw: 0.94 }),
                (1, SpawnSpec { at: Vec2::new(75.0, 418.5), yaw: 2.26 }),
                (0, SpawnSpec { at: Vec2::new(104.6, 414.0), yaw: 0.0 }),
                (2, SpawnSpec { at: Vec2::new(108.2, 414.0), yaw: 0.0 }),
                (1, SpawnSpec { at: Vec2::new(118.4, 414.0), yaw: 0.0 }),
                (0, SpawnSpec { at: Vec2::new(121.9, 414.0), yaw: 0.0 }),
                (2, SpawnSpec { at: Vec2::new(100.0, 394.0), yaw: 0.0 }),
                (2, SpawnSpec { at: Vec2::new(126.0, 394.0), yaw: 0.0 }),
                (0, SpawnSpec { at: Vec2::new(13.0, -434.0), yaw: 0.94 - PI }),
                (1, SpawnSpec { at: Vec2::new(15.0, -418.5), yaw: 2.26 - PI }),
                (0, SpawnSpec { at: Vec2::new(-14.6, -414.0), yaw: PI }),
                (2, SpawnSpec { at: Vec2::new(-18.2, -414.0), yaw: PI }),
                (1, SpawnSpec { at: Vec2::new(-28.4, -414.0), yaw: PI }),
                (0, SpawnSpec { at: Vec2::new(-31.9, -414.0), yaw: PI }),
                (2, SpawnSpec { at: Vec2::new(-10.0, -394.0), yaw: PI }),
                (2, SpawnSpec { at: Vec2::new(-36.0, -394.0), yaw: PI }),
            ],
            bridges: Vec::new(),
            steps: Vec::new(),
            landmarks: Vec::new(),
        }
    }
}

fn land() -> TerrainSpec {
    TerrainSpec {
        size: 1024.0,
        base_cells: 64,
        max_splits: 3,
        paint_splits: 3,
        tolerance: 0.16,
        waves: vec![
            wave(0.021, 0.006, 0.7, 1.9),
            wave(0.007, 0.019, 2.1, 2.3),
            wave(0.043, 0.031, 4.0, 0.65),
            wave(0.09, -0.07, 1.0, 0.2),
        ],
        hills: vec![
            // The ridges that hide each base from the town, with a gap where the road goes over.
            hill((0.0, 250.0), (430.0, 52.0), -0.03, 16.0),
            hill((-8.0, 247.0), (46.0, 44.0), 0.0, -8.0),
            hill((60.0, -255.0), (320.0, 56.0), 0.05, 18.0),
            hill((62.0, -252.0), (48.0, 44.0), 0.0, -9.0),
            // Knolls above the fields, each a long shot from somewhere you need to cross.
            hill((-200.0, -232.0), (92.0, 72.0), 0.4, 13.0),
            hill((310.0, -55.0), (95.0, 125.0), 0.2, 13.0),
            hill((340.0, 265.0), (120.0, 85.0), -0.3, 12.0),
            hill((-330.0, 300.0), (130.0, 90.0), 0.25, 15.0),
            hill((-345.0, -70.0), (110.0, 160.0), 0.1, 4.5),
            hill((-180.0, 105.0), (75.0, 55.0), -0.2, 3.5),
        ],
        calm: vec![Calm { at: Vec2::new(0.0, 12.0), radius: 115.0, fade: 215.0, keep: 0.3 }],
        rim: Rim { inset: 70.0, height: 22.0, face: 12.0, face_height: 16.0, wander: 12.0 },
    }
}

fn roads() -> Vec<Road> {
    vec![
        road(
            &[(40.0, 512.0), (46.0, 400.0), (14.0, 300.0), (-8.0, 245.0), (2.0, 170.0), (12.0, 100.0), (2.0, 40.0), (-6.0, -30.0), (6.0, -100.0), (30.0, -170.0), (55.0, -250.0), (40.0, -350.0), (52.0, -512.0)],
            7.0,
            Ground::Road,
        ),
        road(
            &[(-512.0, 52.0), (-380.0, 30.0), (-250.0, 40.0), (-120.0, 26.0), (-50.0, 34.0), (2.0, 36.0), (60.0, 28.0), (140.0, 30.0), (230.0, 50.0), (340.0, 30.0), (512.0, 54.0)],
            6.0,
            Ground::Road,
        ),
        // Lanes out to the farm, the mill and the two bases.
        road(&[(-285.0, 38.0), (-295.0, -8.0)], 4.0, Ground::Track),
        road(&[(250.0, 46.0), (268.0, 72.0), (280.0, 150.0), (285.6, 202.0)], 4.0, Ground::Track),
        road(&[(46.0, 412.0), (63.0, 424.0)], 5.0, Ground::Track),
        road(&[(40.0, -350.0), (42.0, -420.0), (27.0, -428.0)], 5.0, Ground::Track),
        // Back lanes around the edge of the village.
        road(&[(-120.0, 26.0), (-118.0, -70.0), (-70.0, -118.0), (6.0, -118.0)], 3.6, Ground::Track),
        road(&[(-120.0, 26.0), (-124.0, 100.0), (-60.0, 150.0), (6.0, 150.0)], 3.6, Ground::Track),
        road(&[(60.0, 28.0), (118.0, 34.0)], 3.6, Ground::Track),
        road(&[(118.0, 34.0), (122.0, -118.0), (6.0, -122.0)], 3.6, Ground::Track),
        road(&[(118.0, 34.0), (124.0, 150.0), (6.0, 148.0)], 3.6, Ground::Track),
    ]
}

fn ditches() -> Vec<Ditch> {
    let mut all = vec![
        ditch(&[(-132.0, -210.0), (-128.0, -60.0), (-133.0, 20.0)]),
        ditch(&[(-133.0, 45.0), (-126.0, 190.0)]),
        ditch(&[(-470.0, -112.0), (-300.0, -100.0), (-131.0, -108.0)]),
        ditch(&[(-224.0, -160.0), (-220.0, -100.0), (-226.0, 22.0)]),
        ditch(&[(-226.0, 50.0), (-218.0, 210.0)]),
        ditch(&[(-470.0, 122.0), (-300.0, 108.0), (-128.0, 114.0)]),
        // Across the open ground north and south of town.
        ditch(&[(-128.0, -168.0), (-40.0, -172.0), (15.0, -152.0), (90.0, -160.0), (162.0, -172.0)]),
        ditch(&[(-70.0, -128.0), (-76.0, -170.0)]),
        ditch(&[(78.0, -128.0), (84.0, -160.0)]),
        ditch(&[(-122.0, 190.0), (-30.0, 176.0), (60.0, 184.0), (170.0, 198.0)]),
        ditch(&[(70.0, 162.0), (60.0, 184.0)]),
        ditch(&[(-30.0, 158.0), (-30.0, 176.0)]),
        // East, between the orchard and the fields.
        ditch(&[(190.0, -210.0), (215.0, -130.0), (205.0, 20.0)]),
    ];
    // The dry wash east of town: deep enough to move a squad along unseen.
    all.push(Ditch {
        points: line(&[(185.0, -468.0), (150.0, -310.0), (172.0, -160.0), (140.0, -60.0), (150.0, 30.0), (186.0, 130.0), (160.0, 250.0), (215.0, 380.0), (225.0, 470.0)]),
        width: 20.0,
        bed_width: 7.0,
        depth: 3.4,
        ground: Ground::Shingle,
        cuts_roads: false,
    });
    all
}

fn fields() -> Vec<Zone> {
    vec![
        // The bare ground the village has worn into the land.
        zone((0.0, 10.0), (128.0, 138.0), 0.0, Ground::Dirt),
        // West farmland, a patchwork between the ditches.
        zone((-178.0, -48.0), (42.0, 55.0), 0.0, Ground::Wheat),
        zone((-178.0, -160.0), (42.0, 48.0), 0.0, Ground::Plough),
        zone((-285.0, -160.0), (58.0, 45.0), 0.03, Ground::Stubble),
        zone((-400.0, -170.0), (55.0, 55.0), 0.02, Ground::Wheat),
        zone((-410.0, -30.0), (55.0, 72.0), 0.05, Ground::Plough),
        zone((-300.0, 70.0), (68.0, 28.0), 0.0, Ground::Stubble),
        zone((-178.0, 75.0), (42.0, 36.0), -0.05, Ground::Plough),
        zone((-178.0, 165.0), (42.0, 44.0), 0.0, Ground::Wheat),
        zone((-300.0, 170.0), (68.0, 55.0), 0.03, Ground::Plough),
        zone((-415.0, 75.0), (45.0, 40.0), 0.0, Ground::Wheat),
        // Open ground across the road north and south of the village.
        zone((-70.0, -150.0), (55.0, 18.0), 0.0, Ground::Stubble),
        zone((85.0, -148.0), (48.0, 19.0), 0.0, Ground::Wheat),
        zone((-70.0, 172.0), (50.0, 16.0), 0.0, Ground::Plough),
        zone((85.0, 170.0), (52.0, 18.0), 0.0, Ground::Stubble),
        // The orchard and rough land on the east side.
        zone((268.0, -38.0), (70.0, 62.0), 0.0, Ground::Orchard),
        zone((330.0, 180.0), (90.0, 60.0), 0.0, Ground::Scrub),
    ]
}

fn compounds(s: &BuildStyle) -> Vec<Compound> {
    let flat = |paint| Roof::Flat { paint };
    let gable = |paint, pitch| Roof::Gable { paint, pitch };
    let (lime, mud, ochre, cement, brick) = (Paint::Limewash, Paint::Mudbrick, Paint::Ochre, Paint::Cement, Paint::Brick);
    let (tile, tin) = (Paint::RoofTile, Paint::Tin);
    let mut all = vec![
        // The public buildings, facing the crossroads.
        compound(
            (42.0, -8.0),
            0.03,
            (27.0, 19.0),
            tall(cement, 2.4, &[gate(Side::W, 2.0, 4.0), gate(Side::S, 10.0, 4.0)]),
            vec![hall(s, (3.0, -6.5), 0.0, (17.0, 7.5), cement), tower(s, (-21.0, -9.0), 0.0, (2.9, 4.4), brick)],
            vec![placed(Prop::Truck, (-14.0, 11.0), 1.3), placed(Prop::Sandbags, (2.0, 6.0), 0.0), placed(Prop::Sandbags, (-3.5, 13.0), 0.1), placed(Prop::CrateStack, (19.0, 10.0), 0.3)],
        ),
        // The market square.
        Compound {
            ground: Ground::Paving,
            ..compound(
                (-30.0, 6.0),
                0.0,
                (20.0, 14.0),
                None,
                vec![shop_row(s, (0.0, -9.0), 0.0, (15.0, 4.2), ochre, 3)],
                vec![
                    placed(Prop::Well, (-2.0, 5.0), 0.0),
                    placed(Prop::Stall, (-12.0, 2.5), 0.1),
                    placed(Prop::Stall, (9.0, 1.5), -0.2),
                    placed(Prop::Stall, (12.0, 9.0), 0.15),
                    placed(Prop::Barrier, (-14.0, 10.0), 0.4),
                    placed(Prop::Drums, (5.0, 11.0), 0.0),
                    placed(Prop::Wreck, (-7.0, 11.0), 1.2),
                ],
            )
        },
        // North-east quarter.
        compound(
            (38.0, -52.0),
            0.06,
            (18.0, 13.0),
            tall(mud, 2.3, &[gate(Side::W, 2.0, 3.4), gate(Side::E, -3.0, 2.2)]),
            vec![townhouse(s, (-3.0, -5.0), 0.0, (8.5, 4.5), lime, flat(Paint::Packed)), shed((11.0, 7.0), PI, (4.0, 3.0), mud, tin)],
            vec![placed(Prop::Crate, (-11.0, 8.0), 0.3), placed(Prop::Drums, (-6.0, 9.5), 0.5)],
        ),
        compound(
            (84.0, -48.0),
            -0.12,
            (15.0, 17.0),
            tall(lime, 2.2, &[gate(Side::W, -3.0, 3.2), gate(Side::S, 5.0, 2.2)]),
            vec![house(s, (-1.5, -9.0), 0.0, (8.0, 4.5), ochre).roof_stairs(s), cottage(s, (-4.0, 9.5), PI, (6.5, 4.0), lime, tile)],
            vec![placed(Prop::Well, (7.0, 3.0), 0.0)],
        ),
        compound(
            (34.0, -96.0),
            0.2,
            (16.0, 13.0),
            tall(mud, 2.3, &[gate(Side::W, 0.0, 3.2), gate(Side::E, 4.0, 2.2)]),
            vec![townhouse(s, (2.0, -5.0), 0.0, (7.5, 4.5), ochre, gable(tile, 0.46)), shed((-9.0, 7.5), FRAC_PI_2, (3.5, 3.0), mud, tin)],
            vec![placed(Prop::CrateStack, (9.0, 8.0), 0.2)],
        ),
        compound(
            (78.0, -90.0),
            0.05,
            (18.0, 12.0),
            tall(lime, 2.3, &[gate(Side::S, -6.0, 3.0), gate(Side::W, 3.0, 2.2)]),
            vec![house(s, (-5.0, -4.0), 0.0, (7.5, 4.2), lime).roof_stairs(s), cottage(s, (9.0, 5.0), PI, (5.5, 3.6), ochre, tile)],
            vec![placed(Prop::Wreck, (-10.0, 7.5), 1.45)],
        ),
        // South-east quarter.
        compound(
            (46.0, 70.0),
            -0.05,
            (18.0, 15.0),
            tall(mud, 2.3, &[gate(Side::W, 0.0, 3.4), gate(Side::N, -6.0, 2.2)]),
            vec![townhouse(s, (-3.0, -6.0), 0.0, (8.5, 5.0), mud, flat(Paint::Packed)), house(s, (9.0, 9.0), PI, (6.5, 3.8), lime)],
            vec![placed(Prop::HayBale, (-12.0, 8.0), 0.2), placed(Prop::Drums, (-4.0, 10.5), 0.0)],
        ),
        compound(
            (87.0, 67.0),
            0.1,
            (16.0, 18.0),
            tall(ochre, 2.2, &[gate(Side::W, 4.0, 3.2), gate(Side::N, 0.0, 2.4)]),
            vec![cottage(s, (2.0, -10.0), 0.0, (7.5, 4.2), ochre, tile), shed((-8.5, 9.0), FRAC_PI_2, (4.0, 3.0), lime, tin)],
            vec![placed(Prop::Tank, (9.0, 8.0), 0.0), placed(Prop::CrateStack, (3.0, 12.0), 1.1)],
        ),
        compound(
            (49.0, 118.0),
            0.15,
            (15.0, 13.0),
            tall(lime, 2.3, &[gate(Side::W, 0.0, 3.2), gate(Side::N, 5.0, 2.2)]),
            vec![house(s, (0.0, -5.0), 0.0, (7.5, 4.2), lime).roof_stairs(s)],
            vec![placed(Prop::Wreck, (-5.0, 7.0), 0.7), placed(Prop::Crate, (8.0, 8.0), 0.2)],
        ),
        compound(
            (96.0, 118.0),
            -0.08,
            (17.0, 14.0),
            tall(mud, 2.3, &[gate(Side::N, -4.0, 3.0), gate(Side::W, 4.0, 2.2)]),
            vec![townhouse(s, (0.0, -4.5), 0.0, (8.0, 4.5), brick, gable(tile, 0.46)), shed((9.0, 8.0), PI, (4.5, 3.0), mud, tin)],
            vec![placed(Prop::HayBale, (-10.0, 8.0), 0.9)],
        ),
        // North-west quarter.
        compound(
            (-36.0, -42.0),
            -0.05,
            (19.0, 14.0),
            tall(mud, 2.4, &[gate(Side::E, 0.0, 3.4), gate(Side::S, -8.0, 2.4)]),
            vec![townhouse(s, (-3.0, -5.0), 0.0, (9.0, 5.0), ochre, flat(Paint::Packed)), house(s, (9.0, 8.5), PI, (6.5, 3.8), mud)],
            vec![placed(Prop::Drums, (-13.0, 8.0), 0.1)],
        ),
        compound(
            (-80.0, -3.0),
            0.08,
            (18.0, 16.0),
            tall(lime, 2.3, &[gate(Side::E, 0.0, 3.2), gate(Side::N, 4.0, 2.2)]),
            vec![cottage(s, (-2.0, -8.0), 0.0, (8.0, 4.5), lime, tile), shed((9.0, 8.5), PI, (4.5, 3.0), lime, tin)],
            vec![placed(Prop::Well, (-8.0, 7.0), 0.0), placed(Prop::CrateStack, (-14.0, 11.0), 0.4)],
        ),
        compound(
            (-88.0, -50.0),
            -0.12,
            (17.0, 14.0),
            tall(mud, 2.3, &[gate(Side::E, 2.0, 3.2), gate(Side::S, 5.0, 2.2)]),
            vec![house(s, (-3.0, -5.0), 0.0, (8.0, 4.5), mud).roof_stairs(s)],
            vec![placed(Prop::HayBale, (-9.0, 8.0), 0.2), placed(Prop::Wreck, (9.0, 7.0), 0.4)],
        ),
        compound(
            (-36.0, -96.0),
            0.12,
            (16.0, 15.0),
            tall(ochre, 2.3, &[gate(Side::E, -2.0, 3.2), gate(Side::S, 4.0, 2.2)]),
            vec![townhouse(s, (0.0, -6.0), 0.0, (8.0, 4.5), lime, flat(Paint::Packed))],
            vec![placed(Prop::CrateStack, (-8.0, 8.0), 0.3), placed(Prop::Drums, (6.0, 9.0), 0.5)],
        ),
        // South-west quarter.
        compound(
            (-30.0, 72.0),
            0.06,
            (18.0, 16.0),
            tall(lime, 2.3, &[gate(Side::E, 0.0, 3.4), gate(Side::N, 5.0, 2.2)]),
            vec![townhouse(s, (-3.0, -6.0), 0.0, (8.0, 5.0), lime, gable(tile, 0.46)), house(s, (8.0, 9.5), PI, (6.5, 3.8), ochre)],
            vec![placed(Prop::Tank, (-11.0, 9.0), 0.0)],
        ),
        compound(
            (-70.0, 78.0),
            -0.1,
            (16.0, 18.0),
            tall(mud, 2.3, &[gate(Side::E, -2.0, 3.2), gate(Side::N, 2.0, 2.2)]),
            vec![house(s, (-1.5, -9.0), 0.0, (7.0, 4.2), mud).roof_stairs(s), shed((0.0, 10.0), PI, (5.0, 3.2), mud, tin)],
            vec![placed(Prop::HayBale, (9.0, 3.0), 1.2)],
        ),
        compound(
            (-26.0, 124.0),
            -0.15,
            (16.0, 12.0),
            tall(ochre, 2.2, &[gate(Side::E, 2.0, 3.0), gate(Side::N, -2.0, 2.2)]),
            vec![cottage(s, (0.0, -3.0), 0.0, (7.5, 4.2), ochre, tile)],
            vec![placed(Prop::Drums, (9.0, 7.0), 0.3), placed(Prop::Crate, (-9.0, 7.5), 0.8)],
        ),
        // The graveyard, walled low.
        Compound {
            ground: Ground::Scrub,
            ..compound(
                (-84.0, 136.0),
                0.05,
                (24.0, 18.0),
                tall(Paint::Stone, s.crouch_cover, &[gate(Side::E, -4.0, 2.6), gate(Side::N, 6.0, 2.6)]),
                Vec::new(),
                graves(),
            )
        },
        // The farm in the western fields.
        compound(
            (-305.0, -38.0),
            0.15,
            (30.0, 22.0),
            tall(Paint::Stone, s.crouch_cover, &[gate(Side::S, 5.0, 5.0), gate(Side::E, -4.0, 4.0), gate(Side::W, 3.0, 2.4)]),
            vec![
                barn(s, (-13.0, -9.0), 0.0, (10.5, 6.5), Paint::Brick, Paint::Rust),
                cottage(s, (12.0, -11.0), 0.0, (7.5, 4.2), lime, tile),
                shed((14.0, 11.0), PI, (5.0, 3.5), Paint::Wood, tin),
            ],
            vec![
                placed(Prop::HayBale, (-18.0, 7.0), 0.2),
                placed(Prop::HayBale, (-10.0, 12.0), 1.1),
                placed(Prop::Truck, (-2.0, 8.0), 0.3),
                placed(Prop::Drums, (5.0, 15.0), 0.2),
            ],
        ),
        // The mill on the flank of the south-east hill, in among the woods, with a lookout over the road up.
        compound(
            (282.1, 221.6),
            -0.2,
            (24.0, 18.0),
            tall(mud, 2.4, &[gate(Side::N, 0.0, 5.0), gate(Side::W, -3.0, 2.4), gate(Side::S, 12.0, 2.4)]),
            vec![
                barn(s, (8.0, 2.0), 0.0, (10.0, 6.5), Paint::Brick, Paint::Rust),
                tower(s, (-15.0, -3.0), 0.0, (2.9, 4.4), mud),
                house(s, (-10.0, 11.0), PI, (6.5, 3.8), lime),
            ],
            vec![placed(Prop::CrateStack, (20.0, -11.0), 0.2), placed(Prop::Wreck, (1.0, -12.0), 0.3), placed(Prop::Tank, (-19.0, 10.0), 0.0)],
        ),
        // The checkpoint in the south saddle.
        compound(
            (30.0, 250.0),
            -0.1,
            (13.0, 9.5),
            tall(Paint::Stone, s.crouch_cover, &[gate(Side::W, 0.0, 3.2), gate(Side::E, 3.0, 2.2)]),
            vec![house(s, (0.0, -3.0), 0.0, (6.0, 3.8), cement).roof_stairs(s)],
            vec![placed(Prop::Hesco, (-8.0, 5.5), 0.0), placed(Prop::Sandbags, (5.0, 5.0), 0.0), placed(Prop::Crate, (10.5, 5.5), 0.3)],
        ),
        // Its twin in the north ridge gap.
        compound(
            (85.0, -255.0),
            0.08,
            (13.0, 9.5),
            tall(Paint::Stone, s.crouch_cover, &[gate(Side::W, 0.0, 3.2), gate(Side::E, -3.0, 2.2)]),
            vec![house(s, (0.0, 3.0), PI, (6.0, 3.8), cement)],
            vec![placed(Prop::Hesco, (-8.0, -5.5), 0.0), placed(Prop::Sandbags, (5.0, -5.0), 0.0), placed(Prop::Crate, (10.5, -5.5), 0.3)],
        ),
        // The old post on the knoll, looking down over the west fields.
        compound(
            (-200.0, -232.0),
            0.3,
            (20.0, 15.0),
            tall(Paint::Stone, s.crouch_cover, &[gate(Side::S, -4.0, 3.0), gate(Side::E, 3.0, 2.4)]),
            vec![townhouse(s, (-3.0, -3.0), 0.0, (7.5, 4.5), Paint::Stone, flat(Paint::Cement))],
            vec![placed(Prop::Hesco, (9.0, -8.0), 0.3), placed(Prop::Sandbags, (9.0, 9.0), 0.0), placed(Prop::Sandbags, (-11.0, 9.0), 0.0)],
        ),
    ];
    all.push(compound(
        (92.0, -8.0),
        0.06,
        (16.0, 14.0),
        tall(ochre, 2.3, &[gate(Side::W, -4.0, 3.0), gate(Side::S, 3.0, 2.4)]),
        vec![townhouse(s, (-2.0, -4.0), 0.0, (8.5, 4.5), mud, gable(tile, 0.46)), house(s, (8.0, 9.0), PI, (5.5, 3.6), ochre)],
        vec![placed(Prop::Drums, (-10.0, 8.0), 0.2)],
    ));
    // Houses and shops shoulder to shoulder along the two roads, each row looking onto the road.
    use Lot::{Cottage, Flat, FlatStairs, Shops, Tall, TallGable};
    let rows: [Row; 15] = [
        ((9.5, -64.0), -FRAC_PI_2 - 0.17, 4.2, &[(Tall, 5.0, lime), (Flat, 4.5, mud), (Shops, 6.0, ochre), (Flat, 4.0, lime)]),
        ((-7.5, -79.0), FRAC_PI_2 - 0.17, 4.2, &[(FlatStairs, 4.5, ochre), (TallGable, 5.0, brick), (Flat, 4.5, mud), (Cottage, 4.5, lime)]),
        ((6.3, -7.0), -FRAC_PI_2 + 0.11, 4.2, &[(Shops, 7.0, lime), (Tall, 5.0, ochre), (Flat, 4.5, mud), (Shops, 5.0, cement)]),
        ((-14.0, -18.0), FRAC_PI_2 + 0.11, 4.0, &[(Flat, 3.8, lime), (Flat, 3.6, ochre)]),
        ((17.9, 77.0), -FRAC_PI_2 + 0.17, 4.2, &[(TallGable, 5.0, lime), (Shops, 6.0, mud), (Flat, 4.5, lime), (Cottage, 4.5, brick)]),
        ((-1.7, 76.0), FRAC_PI_2 + 0.17, 4.2, &[(Tall, 5.0, mud), (Flat, 4.5, lime), (FlatStairs, 4.5, ochre), (Cottage, 5.0, lime)]),
        ((21.5, 122.0), -FRAC_PI_2 - 0.14, 4.2, &[(Cottage, 5.0, ochre), (Flat, 4.5, mud), (Cottage, 4.5, lime)]),
        ((-1.1, 124.0), FRAC_PI_2 - 0.14, 4.2, &[(Flat, 4.5, mud), (TallGable, 5.0, ochre)]),
        ((44.0, 21.0), 0.14, 4.2, &[(Shops, 7.0, ochre), (Tall, 5.0, lime), (Flat, 4.5, mud), (Shops, 5.5, lime)]),
        ((40.0, 40.0), PI + 0.14, 4.2, &[(Flat, 4.5, lime), (Shops, 6.5, cement), (TallGable, 5.0, ochre), (Flat, 4.5, mud)]),
        ((95.0, 21.0), -0.025, 4.2, &[(Flat, 4.5, mud), (Cottage, 5.0, lime), (Tall, 5.0, ochre), (Flat, 4.0, lime)]),
        ((-78.0, 21.6), -0.11, 4.2, &[(Flat, 4.5, ochre), (Shops, 6.0, lime), (Flat, 4.5, mud), (Tall, 5.0, lime)]),
        ((88.0, 37.7), PI - 0.025, 4.0, &[(Flat, 4.5, lime), (Tall, 5.0, mud), (Cottage, 4.5, ochre)]),
        ((-78.0, 39.8), PI - 0.11, 4.0, &[(Cottage, 5.0, mud), (Flat, 4.5, lime), (TallGable, 5.0, brick), (Flat, 4.0, ochre)]),
        ((-30.0, 43.8), PI - 0.04, 4.0, &[(Flat, 4.5, mud), (Shops, 6.0, lime), (Flat, 4.0, ochre)]),
    ];
    for (at, yaw, depth, lots) in rows {
        all.push(street_row(s, at, yaw, depth, lots));
    }
    // The two bases.
    all.push(compound(
        (95.0, 425.0),
        0.0,
        (30.0, 22.0),
        tall(Paint::Sandbag, s.high_wall, &[gate(Side::W, 0.0, 7.0), gate(Side::N, 18.0, 20.0)]),
        vec![shed((-16.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas), shed((0.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas)],
        vec![placed(Prop::Truck, (20.0, 12.0), 0.1), placed(Prop::Truck, (15.0, 12.0), -0.1), placed(Prop::CrateStack, (-21.0, 13.0), 0.2)],
    ));
    all.push(compound(
        (-5.0, -425.0),
        0.0,
        (30.0, 22.0),
        tall(Paint::Sandbag, s.high_wall, &[gate(Side::E, 0.0, 7.0), gate(Side::S, -18.0, 20.0)]),
        vec![shed((16.0, 14.0), PI, (6.0, 4.0), Paint::Canvas, Paint::Canvas), shed((0.0, 14.0), PI, (6.0, 4.0), Paint::Canvas, Paint::Canvas)],
        vec![placed(Prop::Truck, (-20.0, -12.0), 0.1), placed(Prop::Truck, (-15.0, -12.0), -0.1), placed(Prop::CrateStack, (21.0, -13.0), 0.2)],
    ));
    all
}

/// Where a street row stands, which way it faces, how deep it is, and its lots from end to end.
type Row<'a> = ((f32, f32), f32, f32, &'a [(Lot, f32, Paint)]);

#[derive(Clone, Copy)]
enum Lot {
    Flat,
    FlatStairs,
    Tall,
    TallGable,
    Cottage,
    Shops,
}

/// Buildings side by side with an alley between each, their doors towards +z. Stepped in and out a
/// little, as a street built one house at a time is.
fn street_row(s: &BuildStyle, at: (f32, f32), yaw: f32, depth: f32, lots: &[(Lot, f32, Paint)]) -> Compound {
    const ALLEY: f32 = 2.6;
    let needs = |lot: &(Lot, f32, Paint)| lot.1 * 2.0 + if matches!(lot.0, Lot::FlatStairs) { s.stair_width + 0.1 } else { 0.0 };
    let total = lots.iter().map(needs).sum::<f32>() + ALLEY * (lots.len() - 1) as f32;
    let mut x = -total * 0.5;
    let mut buildings = Vec::new();
    for (i, lot) in lots.iter().enumerate() {
        let &(kind, width, paint) = lot;
        let center = (x + width, if i % 2 == 0 { 0.35 } else { -0.35 });
        let half = (width, depth);
        buildings.push(match kind {
            // Water tanks on some roofs: the skyline gets a shape, and a roof gets something to hide behind.
            Lot::Flat if i % 3 == 0 => house(s, center, 0.0, half, paint).on_roof(s, Prop::Tank, (width * 0.45, -depth * 0.35)),
            Lot::Flat => house(s, center, 0.0, half, paint),
            Lot::FlatStairs => house(s, center, 0.0, half, paint).roof_stairs(s),
            Lot::Tall => townhouse(s, center, 0.0, half, paint, Roof::Flat { paint: Paint::Packed }),
            Lot::TallGable => townhouse(s, center, 0.0, half, paint, Roof::Gable { paint: Paint::RoofTile, pitch: 0.46 }),
            Lot::Cottage => cottage(s, center, 0.0, half, paint, Paint::RoofTile),
            Lot::Shops => shop_row(s, center, 0.0, half, paint, 2),
        });
        x += needs(lot) + ALLEY;
    }
    compound(at, yaw, (total * 0.5 + 0.5, depth + 0.5), None, buildings, Vec::new())
}

fn graves() -> Vec<Placed> {
    let mut graves = Vec::new();
    for row in 0..5 {
        for i in 0..8 {
            // Crooked rows with a few missing, as an old graveyard has.
            if (row * 7 + i * 3) % 5 == 0 {
                continue;
            }
            let at = (-17.0 + i as f32 * 4.6 + (row % 2) as f32, -12.0 + row as f32 * 6.0);
            graves.push(placed(Prop::Tomb, at, ((row + i) as f32 * 1.7).sin() * 0.12));
        }
    }
    graves
}

fn walls(s: &BuildStyle) -> Vec<FieldWall> {
    vec![
        // Stone walls across the open ground in front of the village, low enough that you have to
        // get down behind them, and spaced so the gaps between are a run under fire.
        field_wall(&[(-115.0, -135.0), (-60.0, -140.0), (-12.0, -132.0)], s.crouch_cover, 0.18, Paint::Stone, &[(38.0, 44.0)]),
        field_wall(&[(22.0, -128.0), (70.0, -135.0), (130.0, -128.0)], s.crouch_cover, 0.18, Paint::Stone, &[(55.0, 61.0)]),
        field_wall(&[(-60.0, -205.0), (0.0, -200.0)], s.prone_cover, 0.08, Paint::Stone, &[]),
        field_wall(&[(45.0, -205.0), (120.0, -210.0)], s.prone_cover, 0.08, Paint::Stone, &[(35.0, 41.0)]),
        field_wall(&[(-112.0, 160.0), (-30.0, 196.0)], s.prone_cover, 0.08, Paint::Stone, &[(40.0, 46.0)]),
        field_wall(&[(22.0, 156.0), (130.0, 168.0)], s.crouch_cover, 0.2, Paint::Stone, &[(50.0, 56.0)]),
        field_wall(&[(-20.0, 212.0), (-90.0, 215.0)], s.crouch_cover, 0.2, Paint::Stone, &[]),
        field_wall(&[(30.0, 212.0), (120.0, 205.0)], s.prone_cover, 0.08, Paint::Stone, &[(44.0, 50.0)]),
        // Hedges on the western field boundaries: thick, and taller than a man.
        field_wall(&[(-136.0, -100.0), (-140.0, 15.0)], s.high_wall, 0.4, Paint::Hedge, &[(52.0, 60.0)]),
        field_wall(&[(-214.0, -95.0), (-218.0, 15.0)], s.high_wall, 0.4, Paint::Hedge, &[(30.0, 37.0), (84.0, 90.0)]),
        field_wall(&[(-300.0, 18.0), (-400.0, 12.0)], s.high_wall, 0.4, Paint::Hedge, &[(46.0, 54.0)]),
        field_wall(&[(-140.0, 62.0), (-218.0, 62.0)], s.high_wall, 0.4, Paint::Hedge, &[(36.0, 44.0)]),
        field_wall(&[(-232.0, 62.0), (-368.0, 60.0)], s.crouch_cover, 0.2, Paint::Stone, &[(60.0, 68.0)]),
        field_wall(&[(-140.0, -118.0), (-218.0, -122.0)], s.crouch_cover, 0.2, Paint::Stone, &[(30.0, 37.0)]),
        field_wall(&[(-232.0, -122.0), (-340.0, -128.0)], s.prone_cover, 0.08, Paint::Stone, &[(50.0, 56.0)]),
        // Orchard and mill walls.
        field_wall(&[(198.0, -102.0), (338.0, -102.0)], s.crouch_cover, 0.2, Paint::Stone, &[(66.0, 74.0)]),
        field_wall(&[(198.0, 20.0), (338.0, 22.0)], s.crouch_cover, 0.2, Paint::Stone, &[(40.0, 48.0), (100.0, 108.0)]),
        // Two broken-down houses, roofs long gone: rooms of rubble to fight through.
        field_wall(&[(-92.0, -92.0), (-78.0, -92.0), (-78.0, -104.0), (-92.0, -104.0), (-92.0, -92.0)], 2.0, 1.0, Paint::Mudbrick, &[(5.0, 8.0), (30.0, 33.0)]),
        field_wall(&[(-88.0, -92.0), (-88.0, -104.0)], 1.5, 0.8, Paint::Mudbrick, &[(4.0, 6.0)]),
        field_wall(&[(60.0, 160.0), (76.0, 158.0), (78.0, 146.0), (62.0, 148.0), (60.0, 160.0)], 2.2, 1.2, Paint::Limewash, &[(2.0, 5.0), (25.0, 29.0)]),
    ]
}

fn heaps() -> Vec<Rubble> {
    vec![
        rubble((-85.0, -98.0), 7.0, 30, Paint::Mudbrick),
        rubble((69.0, 153.0), 7.0, 30, Paint::Limewash),
        // Where walls have come down in the streets.
        rubble((15.0, 20.0), 2.6, 12, Paint::Cement),
        rubble((-13.5, -35.0), 2.4, 10, Paint::Mudbrick),
        rubble((64.0, 48.0), 3.0, 12, Paint::Mudbrick),
        rubble((-55.0, 50.0), 2.8, 10, Paint::Limewash),
        rubble((60.0, -70.0), 3.0, 11, Paint::Limewash),
        rubble((-64.0, -28.0), 2.5, 9, Paint::Ochre),
        rubble((112.0, 86.0), 3.0, 12, Paint::Mudbrick),
        rubble((-112.0, 44.0), 3.0, 10, Paint::Mudbrick),
        // Outcrops on the high ground.
        Rubble { at: Vec2::new(-200.0, -292.0), radius: 22.0, count: 14, size: (0.7, 1.8), paint: Paint::Rock },
        Rubble { at: Vec2::new(150.0, 250.0), radius: 40.0, count: 18, size: (0.7, 2.0), paint: Paint::Rock },
        Rubble { at: Vec2::new(-180.0, 248.0), radius: 45.0, count: 20, size: (0.7, 2.0), paint: Paint::Rock },
        Rubble { at: Vec2::new(250.0, -255.0), radius: 40.0, count: 16, size: (0.7, 1.9), paint: Paint::Rock },
        Rubble { at: Vec2::new(-90.0, -262.0), radius: 40.0, count: 18, size: (0.7, 1.9), paint: Paint::Rock },
        Rubble { at: Vec2::new(345.0, -60.0), radius: 40.0, count: 12, size: (0.6, 1.7), paint: Paint::Rock },
    ]
}

fn street_props() -> Vec<Placed> {
    vec![
        placed(Prop::Wreck, (9.5, 80.0), 0.1),
        placed(Prop::Wreck, (-2.0, 8.0), 3.0),
        placed(Prop::Barrier, (6.0, -102.0), 1.3),
        placed(Prop::Barrier, (3.0, -108.0), 0.15),
        placed(Prop::Sandbags, (9.0, -110.0), 0.2),
        placed(Prop::Barrier, (12.0, 118.0), 0.2),
        placed(Prop::Sandbags, (6.0, 122.0), -0.1),
        placed(Prop::Truck, (-30.0, 31.0), 1.5),
        placed(Prop::Wreck, (92.0, 31.0), 1.7),
        placed(Prop::Wreck, (-210.0, 42.0), 1.2),
        placed(Prop::Truck, (32.0, -172.0), 0.3),
        placed(Prop::Wreck, (195.0, 42.0), 1.4),
        placed(Prop::HayBale, (-170.0, -40.0), 0.3),
        placed(Prop::HayBale, (-190.0, -70.0), 1.2),
        placed(Prop::HayBale, (-280.0, -160.0), 0.7),
        placed(Prop::HayBale, (-185.0, 150.0), 0.1),
        placed(Prop::HayBale, (-310.0, 150.0), 1.9),
    ]
}

fn woods() -> Vec<Woodland> {
    use TreeKind::{Broadleaf, Fir, Fruit, Poplar};
    let big = (9.0, 14.0);
    let wide = (3.0, 4.6);
    vec![
        // Field boundaries in the west: long, gapped lines to move along or shoot from.
        trees(&[(-139.0, -205.0), (-136.0, -120.0)], 9.0, Broadleaf, big, wide),
        trees(&[(-139.0, 60.0), (-135.0, 185.0)], 10.0, Broadleaf, big, wide),
        trees(&[(-230.0, -150.0), (-228.0, -115.0)], 8.0, Poplar, (11.0, 15.0), (1.4, 2.0)),
        trees(&[(-232.0, 72.0), (-226.0, 200.0)], 9.0, Poplar, (11.0, 15.0), (1.4, 2.0)),
        trees(&[(-460.0, -120.0), (-350.0, -112.0)], 10.0, Broadleaf, big, wide),
        trees(&[(-455.0, 130.0), (-305.0, 118.0)], 10.0, Broadleaf, big, wide),
        // Along the roads, where people planted them.
        trees(&[(-300.0, 46.0), (-235.0, 50.0)], 13.0, Poplar, (11.0, 16.0), (1.3, 1.9)),
        trees(&[(-112.0, 40.0), (-70.0, 44.0)], 14.0, Poplar, (9.0, 12.0), (1.2, 1.7)),
        trees(&[(18.0, 165.0), (28.0, 200.0)], 13.0, Poplar, (11.0, 15.0), (1.3, 1.9)),
        trees(&[(-15.0, -150.0), (-8.0, -125.0)], 12.0, Poplar, (10.0, 14.0), (1.2, 1.8)),
        trees(&[(140.0, 42.0), (225.0, 62.0)], 13.0, Poplar, (10.0, 15.0), (1.3, 1.9)),
        // Shade in the village.
        clump((-112.0, -22.0), (5.0, 22.0), 7, Broadleaf, (8.0, 12.0), (2.8, 4.0)),
        clump((122.0, -20.0), (5.0, 30.0), 8, Broadleaf, (8.0, 12.0), (2.8, 4.0)),
        clump((122.0, 85.0), (5.0, 25.0), 7, Broadleaf, (8.0, 12.0), (2.8, 4.0)),
        clump((-120.0, 90.0), (5.0, 22.0), 6, Broadleaf, (8.0, 12.0), (2.8, 4.0)),
        clump((-80.0, 118.0), (20.0, 3.0), 4, Fir, (6.0, 9.0), (1.5, 2.2)),
        // Woods on the high ground.
        clump((-60.0, 265.0), (130.0, 28.0), 75, Fir, (8.0, 14.0), (2.2, 3.4)),
        clump((225.0, 262.0), (70.0, 28.0), 40, Fir, (8.0, 13.0), (2.2, 3.2)),
        clump((-20.0, -285.0), (110.0, 22.0), 60, Fir, (8.0, 14.0), (2.2, 3.4)),
        clump((230.0, -275.0), (80.0, 26.0), 45, Fir, (8.0, 14.0), (2.2, 3.4)),
        clump((-340.0, -270.0), (80.0, 50.0), 50, Broadleaf, big, wide),
        clump((-350.0, 320.0), (90.0, 55.0), 60, Fir, (9.0, 15.0), (2.4, 3.6)),
        clump((355.0, 290.0), (80.0, 55.0), 45, Broadleaf, big, wide),
        clump((370.0, -180.0), (80.0, 60.0), 40, Fir, (8.0, 14.0), (2.2, 3.4)),
        clump((390.0, 90.0), (50.0, 70.0), 30, Broadleaf, big, wide),
        // The orchard: straight rows with long lanes between.
        Woodland { kind: Fruit, shape: WoodShape::Rows { at: Vec2::new(268.0, -38.0), half: Vec2::new(66.0, 58.0), yaw: 0.04, spacing: 10.0 }, height: (4.2, 5.8), radius: (1.8, 2.5) },
    ]
}
