//! Claudabad: a dusty river town fought over between a northern and a southern team. A canal runs
//! across the middle and parts round an island, where a terracotta sunburst stands over the square for
//! everyone to steer by. Six bridges cross it: a road bridge at each end and two onto the island from
//! each bank. North of the canal is the old quarter, close-packed compounds round a covered bazaar, with
//! a brickworks beyond. South of it is the newer town under its water tower, and the rail yard. Palm groves
//! and fields line the canal outside the town, and a low ridge hides each base.
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

/// Kept this far clear of a road's middle and the canal's when filling a quarter with compounds.
const ROAD_CLEARANCE: f32 = 9.0;
const CANAL_CLEARANCE: f32 = 16.0;
/// A block of a quarter: half its size, and the alley between it and the next.
const BLOCK: (f32, f32) = (14.0, 12.0);
const ALLEY: f32 = 4.0;

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
        let taken: Vec<(Vec2, Vec2)> = compounds.iter().map(|c| (c.at, c.half + Vec2::splat(ALLEY))).collect();
        compounds.extend(quarters(&style, &roads, &canal, &taken));
        let (bridges, steps) = crossings();
        let mut ditches = canal;
        ditches.extend(irrigation());
        Self {
            seed: 4242,
            terrain: land(),
            style,
            roads,
            ditches,
            zones: grounds(),
            compounds,
            walls: rails(),
            rubble: heaps(),
            props: street_props(),
            woods: woods(),
            teams: vec![
                TeamSpec { name: "South".into(), spawns: spawn_rows((40.0, 428.0), 0.0), base_radius: 60.0 },
                TeamSpec { name: "North".into(), spawns: spawn_rows(turned((40.0, 431.5)), PI), base_radius: 60.0 },
            ],
            // Each side's two are the other's turned round, so the walks match, and the square is in the middle.
            capture_points: [
                ("Rail Yard", (210.0, 230.0), 36.0),
                ("New Town", (-130.0, 120.0), 34.0),
                // As far from New Town as from the Bazaar, beside the sunburst.
                ("Sunburst Square", (8.0, 9.0), 30.0),
                ("Bazaar", (130.0, -120.0), 34.0),
                ("Brickworks", (-210.0, -230.0), 36.0),
            ]
            .into_iter()
            .map(|(name, at, radius)| CapturePointSpec { name: name.into(), at: at.into(), radius })
            .collect(),
            // In the southern base, firing out of its west gate across the empty ground beyond.
            sentry: SpawnSpec { at: Vec2::new(14.0, 430.0), yaw: FRAC_PI_2 },
            helipads: vec![SpawnSpec { at: Vec2::new(60.0, 432.0), yaw: 0.0 }, SpawnSpec { at: turned((60.0, 432.0)).into(), yaw: PI }],
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
        waves: vec![wave(0.017, 0.008, 0.4, 1.6), wave(0.006, 0.021, 1.7, 2.0), wave(0.047, 0.035, 3.1, 0.5)],
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
        ],
        // The valley floor the town sits on.
        calm: vec![Calm { at: Vec2::ZERO, radius: 260.0, fade: 380.0, keep: 0.12 }],
        rim: Rim { inset: 70.0, height: 20.0, face: 12.0, face_height: 16.0, wander: 12.0 },
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
    };
    let west = [(-530.0, -34.0), (-380.0, -20.0), (-250.0, -8.0), (-150.0, 0.0), (-95.0, 0.0)];
    let north_arm = [(-95.0, 0.0), (-60.0, -36.0), (0.0, -44.0), (60.0, -36.0), (95.0, 0.0)];
    let mut all = Vec::new();
    for half in [both(&west), both(&north_arm)] {
        for points in half {
            all.push(canal(&points));
        }
    }
    all
}

/// Ditches feeding the fields off the canal, on both sides of it at each end of the town.
fn irrigation() -> Vec<Ditch> {
    let ditch = |points: &[(f32, f32)]| Ditch { points: line(points), width: 4.8, bed_width: 1.4, depth: 1.25, ground: Ground::DitchBed, cuts_roads: false };
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
        // From the south base over the ridge gap and down to the island bridge.
        (vec![(55.0, 404.0), (55.0, 300.0), (30.0, 160.0), (-30.0, 72.0), (-30.0, 52.0)], 7.0, Ground::Road),
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

/// Bridges, and steps down into the canal beside each so that falling in is not the end of you: (middle
/// of the canal under the bridge, which way across it, whether it carries a road).
fn crossings() -> (Vec<Bridge>, Vec<Steps>) {
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
    (bridges, steps)
}

fn grounds() -> Vec<Zone> {
    let mut all = vec![
        // The worn ground of the town, either side of the canal.
        zone((0.0, 0.0), (270.0, 230.0), 0.0, Ground::Dirt),
    ];
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

    // The bases, the north one the south turned half round.
    for (at, yaw) in [((40.0, 430.0), 0.0), (turned((40.0, 430.0)), PI)] {
        all.push(compound(
            at,
            yaw,
            (30.0, 22.0),
            tall(Paint::Sandbag, s.high_wall, &[gate(Side::W, 0.0, 7.0), gate(Side::N, 16.0, 18.0)]),
            vec![shed((-16.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas), shed((0.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas)],
            vec![placed(Prop::CrateStack, (-21.0, 14.0), 0.2), placed(Prop::Hesco, (-20.0, 4.0), FRAC_PI_2)],
        ));
    }

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
    for (min, max, share) in [(Vec2::new(-260.0, -205.0), Vec2::new(260.0, -60.0), 0.92), (Vec2::new(-260.0, 60.0), Vec2::new(260.0, 205.0), 0.6)] {
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
    for (kind, at, yaw) in [(0, (50.0, 416.0), 0.0), (2, (54.0, 416.0), 0.0), (1, (60.0, 416.0), 0.0), (0, (64.0, 416.0), 0.0), (2, (48.0, 396.0), 0.0), (2, (66.0, 396.0), 0.0)] {
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
    all
}

fn street_props() -> Vec<Placed> {
    let mut all = Vec::new();
    for (prop, at, yaw) in [(Prop::Wreck, (18.0, 140.0), 0.3), (Prop::Barrier, (-24.0, 66.0), 0.2), (Prop::Sandbags, (-36.0, 64.0), 0.0), (Prop::Wreck, (260.0, 40.0), 1.4)] {
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
    for (at, half, count) in [((-300.0, -330.0), (90.0, 30.0), 30), ((260.0, -330.0), (70.0, 25.0), 22)] {
        all.push(clump(at, half, count, Broadleaf, (8.0, 12.0), (2.6, 3.8)));
        all.push(clump(turned(at), half, count, Broadleaf, (8.0, 12.0), (2.6, 3.8)));
    }
    all
}
