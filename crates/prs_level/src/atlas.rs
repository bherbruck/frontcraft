//! What the maps of real places share: a survey of the land read from its file, and quarters of buildings
//! laid out block by block over whatever area is free of water, roads, steep ground and each other.

use crate::archetypes::{barn, hall, house, shed, shop_row, tower, townhouse};
use crate::maps::HeightGrid;
use crate::relief::Relief;
use crate::spec::*;
use crate::{Paint, Scatter};
use glam::Vec2;
use std::f32::consts::PI;

/// A survey written as whole decimetres, a row to a line from the north-west corner, `size` metres across,
/// with heights taken from `datum`, so that a calm quiets the land towards that height and not towards the sea.
pub(crate) fn survey(text: &str, size: f32, datum: f32) -> HeightGrid {
    let heights: Vec<f32> = text.split_whitespace().map(|v| v.parse::<i32>().expect("a survey is whole decimetres") as f32 * 0.1 - datum).collect();
    let row = (heights.len() as f64).sqrt().round() as usize;
    assert_eq!(row * row, heights.len(), "a survey is square");
    HeightGrid { size, cells: row as u32 - 1, heights }
}

pub(crate) fn line(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&p| p.into()).collect()
}

pub(crate) fn placed(prop: Prop, at: (f32, f32), yaw: f32) -> Placed {
    Placed { prop, at: at.into(), yaw }
}

pub(crate) fn gate(side: Side, along: f32, width: f32) -> Gate {
    Gate { side, along, width }
}

/// Twelve spawn places in two rows, looking the way the team has to go.
pub(crate) fn spawn_rows(at: (f32, f32), yaw: f32) -> Vec<SpawnSpec> {
    let (across, back) = (Vec2::from_angle(-yaw), Vec2::from_angle(-yaw).perp());
    let mut spawns = Vec::new();
    for row in 0..2 {
        for i in 0..6 {
            let local = across * (i as f32 * 3.5 - 8.75) - back * (row as f32 * 3.5);
            spawns.push(SpawnSpec { at: Vec2::from(at) + local, yaw });
        }
    }
    spawns
}

/// A seeded roll of the dice, so a quarter comes out the same every time it is laid out.
pub(crate) struct Dice(Scatter);

impl Dice {
    pub fn new(seed: u32) -> Self {
        Self(Scatter(seed))
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        self.0.range(min, max)
    }

    pub fn chance(&mut self, odds: f32) -> bool {
        self.0.range(0.0, 1.0) < odds
    }

    pub fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[((self.0.range(0.0, 1.0) * from.len() as f32) as usize).min(from.len() - 1)]
    }
}

/// The corners of a rotated rectangle.
pub(crate) fn corners(at: Vec2, yaw: f32, half: Vec2) -> [Vec2; 4] {
    let axis = Vec2::from_angle(-yaw);
    [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)].map(|(a, b)| at + axis.rotate(half * Vec2::new(a, b)))
}

/// Whether two convex outlines are apart, by looking for an axis that separates them.
pub(crate) fn apart(a: &[Vec2; 4], b: &[Vec2; 4]) -> bool {
    let edges = [a[1] - a[0], a[2] - a[1], b[1] - b[0], b[2] - b[1]];
    edges.iter().any(|e| {
        let n = e.perp().normalize();
        let range = |p: &[Vec2; 4]| p.iter().fold((f32::MAX, f32::MIN), |r, c| (r.0.min(c.dot(n)), r.1.max(c.dot(n))));
        let (ra, rb) = (range(a), range(b));
        ra.1 < rb.0 || rb.1 < ra.0
    })
}

fn to_line(p: Vec2, points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let along = b - a;
            let t = ((p - a).dot(along) / along.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + along * t).distance(p)
        })
        .fold(f32::MAX, f32::min)
}

/// Says where a compound may go: on land, off the roads, on ground level enough to build on, and clear of
/// everything placed so far.
pub(crate) struct Site<'a> {
    spec: &'a MapSpec,
    relief: Relief<'a>,
    taken: Vec<[Vec2; 4]>,
}

impl<'a> Site<'a> {
    /// For a spec with its land, water and roads already in. Its compounds so far are taken.
    pub fn new(spec: &'a MapSpec) -> Self {
        let taken = spec.compounds.iter().map(|c| corners(c.at, c.yaw, c.half + spec.style.yard_margin)).collect();
        Self { spec, relief: Relief::within(spec, u64::MAX), taken }
    }

    /// Keeps a rectangle free of anything placed after.
    pub fn reserve(&mut self, at: Vec2, yaw: f32, half: Vec2) {
        self.taken.push(corners(at, yaw, half));
    }

    /// Whether a compound fits here, with `room` to spare from roads and other compounds, and no more than
    /// `drop` between the highest and lowest ground under it.
    pub fn fits(&self, at: Vec2, yaw: f32, half: Vec2, room: f32, drop: f32) -> bool {
        let outline = corners(at, yaw, half + room);
        if self.taken.iter().any(|t| !apart(t, &outline)) {
            return false;
        }
        let mut probes = outline.to_vec();
        probes.push(at);
        for k in 0..4 {
            probes.push((outline[k] + outline[(k + 1) % 4]) * 0.5);
        }
        if probes.iter().any(|&p| self.relief.wet(p) || p.x.abs() > self.spec.terrain.size * 0.5 - 60.0 || p.y.abs() > self.spec.terrain.size * 0.5 - 60.0) {
            return false;
        }
        let reach = half.length() + room;
        for road in &self.spec.roads {
            let clear = road.width * 0.5 + room;
            if to_line(at, &road.points) < reach + clear && probes.iter().any(|&p| to_line(p, &road.points) < clear) {
                return false;
            }
            // A road can pass between the probes, so look along it too.
            for pair in road.points.windows(2) {
                let steps = (pair[0].distance(pair[1]) / 2.0).ceil().max(1.0) as usize;
                for s in 0..=steps {
                    let p = pair[0].lerp(pair[1], s as f32 / steps as f32);
                    if p.distance(at) < reach + clear && inside(p, at, yaw, half + room + road.width * 0.5) {
                        return false;
                    }
                }
            }
        }
        let heights: Vec<f32> = probes.iter().map(|&p| self.relief.height(p)).collect();
        let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |r, h| (r.0.min(*h), r.1.max(*h)));
        high - low <= drop
    }

    /// Whether a road passes within `reach` of `p`.
    pub fn by_a_road(&self, p: Vec2, reach: f32) -> bool {
        self.spec.roads.iter().any(|road| to_line(p, &road.points) < reach + road.width * 0.5)
    }

    pub fn take(&mut self, compound: &Compound) {
        self.taken.push(corners(compound.at, compound.yaw, compound.half + self.spec.style.yard_margin));
    }
}

fn inside(p: Vec2, at: Vec2, yaw: f32, half: Vec2) -> bool {
    let local = Vec2::from_angle(yaw).rotate(p - at);
    local.x.abs() < half.x && local.y.abs() < half.y
}

/// What a quarter is built of.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    /// Shops and houses back to back in blocks with alleys between, and the odd open square.
    Souq,
    /// Houses round a walled yard.
    Courtyard,
    /// Sheds and halls behind a wall, with things stacked in the yard.
    Works,
}

/// An area to fill with blocks on a grid: its middle, which way it runs, how far it reaches, the size of a
/// block and the width of the street around it.
pub(crate) struct Quarter {
    pub at: Vec2,
    pub yaw: f32,
    pub half: Vec2,
    pub block: Vec2,
    pub street: f32,
    pub kind: Kind,
    /// Of the blocks that fit, how many are built.
    pub density: f32,
    /// Most the ground may fall across a block.
    pub drop: f32,
    pub paints: &'static [Paint],
    /// Where the town is thickest and how far out it thins to almost nothing, so that it clusters there
    /// rather than scattering evenly. Blocks on a road are built before blocks off one, as a town grows.
    pub heart: Option<(Vec2, f32)>,
}

/// How much more likely a block on a road is to be built, and the least share of the density a block far
/// from the heart keeps.
const ON_A_ROAD: f32 = 1.4;
const OUTSKIRTS: f32 = 0.04;

impl Quarter {
    pub fn fill(&self, s: &BuildStyle, dice: &mut Dice, site: &mut Site) -> Vec<Compound> {
        let pitch = self.block + Vec2::splat(self.street);
        let (nx, nz) = ((self.half.x * 2.0 / pitch.x).floor() as i32, (self.half.y * 2.0 / pitch.y).floor() as i32);
        let axis = Vec2::from_angle(-self.yaw);
        let mut built = Vec::new();
        for j in 0..nz {
            for i in 0..nx {
                let local = Vec2::new((i as f32 + 0.5) * pitch.x - nx as f32 * pitch.x * 0.5, (j as f32 + 0.5) * pitch.y - nz as f32 * pitch.y * 0.5);
                let at = self.at + axis.rotate(local);
                let odds = match self.heart {
                    Some((heart, reach)) => {
                        let thinning = 1.0 - crate::smoothstep(reach * 0.25, reach, heart.distance(at));
                        let street = if site.by_a_road(at, pitch.max_element()) { ON_A_ROAD } else { 1.0 };
                        self.density * (OUTSKIRTS + (1.0 - OUTSKIRTS) * thinning) * street
                    }
                    None => self.density,
                };
                if !dice.chance(odds) {
                    continue;
                }
                let yaw = self.yaw + dice.range(-0.03, 0.03);
                let half = self.block * 0.5 - Vec2::new(dice.range(0.0, 1.5), dice.range(0.0, 1.0));
                if !site.fits(at, yaw, half, self.street * 0.5 - s.yard_margin + 0.5, self.drop) {
                    continue;
                }
                let compound = match self.kind {
                    Kind::Souq if dice.chance(0.08) => square(at, yaw, half, dice),
                    Kind::Souq => souq_block(s, at, yaw, half, self.paints, dice),
                    Kind::Courtyard => courtyard(s, at, yaw, half, self.paints, dice),
                    Kind::Works => works(s, at, yaw, half, self.paints, dice),
                };
                site.take(&compound);
                built.push(compound);
            }
        }
        built
    }
}

fn bare(at: Vec2, yaw: f32, half: Vec2, ground: Ground) -> Compound {
    Compound { at, yaw, half, lift: 0.0, ground, wall: None, buildings: Vec::new(), props: Vec::new() }
}

/// An open square with stalls, a well and whatever has been left about.
pub(crate) fn square(at: Vec2, yaw: f32, half: Vec2, dice: &mut Dice) -> Compound {
    let mut c = bare(at, yaw, half, Ground::Paving);
    let count = (half.x * half.y / 60.0) as u32 + 2;
    for k in 0..count {
        let p = (dice.range(-half.x + 3.0, half.x - 3.0), dice.range(-half.y + 3.0, half.y - 3.0));
        let prop = if k == 0 { Prop::Well } else { dice.pick(&[Prop::Stall, Prop::Stall, Prop::Stall, Prop::Crate, Prop::Drums, Prop::Barrier]) };
        // Never on top of each other: a stall is about four metres across.
        if c.props.iter().any(|q| q.at.distance(p.into()) < 5.0) {
            continue;
        }
        c.props.push(placed(prop, p, dice.range(-0.3, 0.3)));
    }
    c
}

/// One building of a souq block, sized to fit `width` by `depth`, its door towards +z.
fn lot(s: &BuildStyle, at: (f32, f32), yaw: f32, width: f32, depth: f32, paint: Paint, dice: &mut Dice) -> Building {
    let half = (width * 0.5, depth * 0.5);
    let roll = dice.range(0.0, 1.0);
    if width >= 10.5 && roll < 0.35 {
        shop_row(s, at, yaw, half, paint, 2)
    } else if width >= 8.0 && depth >= 9.0 && roll < 0.75 {
        townhouse(s, at, yaw, half, paint, Roof::Flat { paint: Paint::Packed })
    } else if roll < 0.9 {
        let house = house(s, at, yaw, half, paint);
        if dice.chance(0.3) { house.on_roof(s, Prop::Tank, (half.0 * 0.45, -half.1 * 0.35)) } else { house }
    } else {
        townhouse(s, at, yaw, half, paint, Roof::Flat { paint: Paint::Cement })
    }
}

/// Two rows of buildings back to back, each looking out onto its own street, shoulder to shoulder along it.
pub(crate) fn souq_block(s: &BuildStyle, at: Vec2, yaw: f32, half: Vec2, paints: &[Paint], dice: &mut Dice) -> Compound {
    let mut c = bare(at, yaw, half, Ground::Paving);
    let depth = (half.y - 0.8).min(10.0);
    for (row, facing) in [(1.0, 0.0), (-1.0, PI)] {
        let z = row * (half.y - 0.3 - depth * 0.5);
        let mut x = -half.x + 0.3;
        let end = half.x - 0.3;
        while end - x >= 8.0 {
            let mut width = dice.range(8.0, 13.0).min(end - x);
            if end - x - width < 8.0 {
                width = end - x;
            }
            // Only the front row gets stairs up its outside, as only there do they face the street.
            let stairs = row > 0.0 && width >= 11.0 && dice.chance(0.35);
            let body = if stairs { width - s.stair_width - 0.2 } else { width };
            let centre = x + body * 0.5;
            let paint = dice.pick(paints);
            let building = if stairs {
                house(s, (centre, z), facing, (body * 0.5, depth * 0.5), paint).roof_stairs(s)
            } else {
                lot(s, (centre, z), facing, body, depth, paint, dice)
            };
            c.buildings.push(building);
            x += width + 0.4;
        }
    }
    c
}

/// A house at the back of a walled yard, a gate in front, often a shed and a second way out.
pub(crate) fn courtyard(s: &BuildStyle, at: Vec2, yaw: f32, half: Vec2, paints: &[Paint], dice: &mut Dice) -> Compound {
    let paint = dice.pick(paints);
    let mut c = bare(at, yaw, half, Ground::Yard);
    let height = dice.pick(&[2.2, 2.3, 2.4, s.crouch_cover + 0.4]);
    let mut gates = vec![gate(Side::S, dice.range(-half.x * 0.4, half.x * 0.4), dice.pick(&[2.4, 3.0, 3.4]))];
    if dice.chance(0.6) {
        // On the same side as the front gate, as any shed goes in the other front corner.
        gates.push(gate(if gates[0].along > 0.0 { Side::E } else { Side::W }, half.y * 0.45, 2.2));
    }
    c.wall = Some(YardWall { height, paint: dice.pick(paints), gates });
    let depth = dice.range(8.4, 10.0).min(half.y);
    let width = (half.x * 2.0 - 2.0).min(dice.range(10.0, 17.0));
    let z = -half.y + 0.6 + depth * 0.5;
    let house_at = (dice.range(-(half.x - 0.6 - width * 0.5), half.x - 0.6 - width * 0.5).min(half.x - 2.0 - width * 0.5), z);
    let main = if dice.chance(0.55) {
        townhouse(s, house_at, 0.0, (width * 0.5, depth * 0.5), paint, Roof::Flat { paint: Paint::Packed })
    } else if width + s.stair_width + 0.4 < half.x * 2.0 - 1.2 && dice.chance(0.5) {
        house(s, house_at, 0.0, (width * 0.5, depth * 0.5), paint).roof_stairs(s)
    } else {
        house(s, house_at, 0.0, (width * 0.5, depth * 0.5), paint)
    };
    c.buildings.push(main);
    // A shed in a front corner, if the yard is deep enough to leave the gate clear.
    let yard = half.y * 2.0 - depth - 1.2;
    if yard > 9.0 && dice.chance(0.5) {
        let side = if c.wall.as_ref().is_some_and(|w| w.gates[0].along > 0.0) { -1.0 } else { 1.0 };
        let shed_half = (3.2, 2.6);
        c.buildings.push(shed((side * (half.x - 0.8 - shed_half.0), half.y - 0.8 - shed_half.1), PI, shed_half, paint, Paint::Tin));
    }
    let yard_mid = (z + depth * 0.5 + half.y) * 0.5;
    if dice.chance(0.6) {
        let prop = dice.pick(&[Prop::Drums, Prop::Crate, Prop::Tank, Prop::Wreck, Prop::CrateStack]);
        c.props.push(placed(prop, (dice.range(-half.x * 0.3, half.x * 0.3), yard_mid), dice.range(0.0, PI)));
    }
    c
}

/// A yard of sheds and stacked goods behind a wall with a wide gate.
pub(crate) fn works(s: &BuildStyle, at: Vec2, yaw: f32, half: Vec2, paints: &[Paint], dice: &mut Dice) -> Compound {
    let paint = dice.pick(paints);
    let mut c = bare(at, yaw, half, Ground::Dirt);
    c.wall = Some(YardWall { height: dice.pick(&[s.crouch_cover, 2.3, 2.4]), paint, gates: vec![gate(Side::S, 0.0, 6.0), gate(Side::E, half.y * 0.5, 2.4)] });
    let width = (half.x * 2.0 - 3.0).min(dice.range(16.0, 34.0));
    // A barn has a door at the back as well, so leave a way round behind.
    let depth = (half.y - 3.0).min(dice.range(9.0, 13.0));
    let z = -half.y + 3.5 + depth * 0.5;
    // A hall's stairs run past the doors off its corridor unless it is long enough.
    let big = if width < 27.0 || dice.chance(0.5) {
        barn(s, (0.0, z), 0.0, (width * 0.5, depth * 0.5), paint, dice.pick(&[Paint::Tin, Paint::Rust]))
    } else {
        hall(s, (0.0, z), 0.0, (width * 0.5, depth * 0.5), paint)
    };
    c.buildings.push(big);
    for k in 0..3 {
        let x = -half.x * 0.6 + k as f32 * half.x * 0.6;
        let prop = dice.pick(&[Prop::CrateStack, Prop::Drums, Prop::Truck, Prop::Wreck, Prop::Crate]);
        c.props.push(placed(prop, (x + dice.range(-1.0, 1.0), half.y - 3.5), dice.range(-0.2, 0.2) + if prop == Prop::Truck { PI * 0.5 } else { 0.0 }));
    }
    c
}

/// A walled fort: towers in its corners, a keep, and a gate.
pub(crate) fn fort(s: &BuildStyle, at: Vec2, yaw: f32, half: Vec2, wall_height: f32, gates: &[Gate]) -> Compound {
    let mut c = bare(at, yaw, half, Ground::Dirt);
    c.wall = Some(YardWall { height: wall_height, paint: Paint::Stone, gates: gates.to_vec() });
    let t = (2.9, 4.4);
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0)] {
        c.buildings.push(tower(s, (x * (half.x - 1.0 - t.0), z * (half.y - 1.0 - t.1)), 0.0, t, Paint::Stone));
    }
    c.buildings.push(townhouse(s, (0.0, -half.y + 1.0 + 4.6), 0.0, (8.0, 4.6), Paint::Stone, Roof::Flat { paint: Paint::Cement }));
    c.props.push(placed(Prop::Well, (half.x * 0.3, half.y * 0.2), 0.0));
    c
}

/// A main base: a sandbagged square with sheds along the back, gates where the map says.
pub(crate) fn base(s: &BuildStyle, at: Vec2, yaw: f32, gates: &[Gate]) -> Compound {
    let mut c = bare(at, yaw, Vec2::new(30.0, 22.0), Ground::Yard);
    c.wall = Some(YardWall { height: s.high_wall, paint: Paint::Sandbag, gates: gates.to_vec() });
    c.buildings = vec![
        shed((-16.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas),
        shed((0.0, -14.0), 0.0, (6.0, 4.0), Paint::Canvas, Paint::Canvas),
    ];
    c.props = vec![placed(Prop::CrateStack, (-24.0, 2.0), 0.2), placed(Prop::Hesco, (23.0, -18.0), 0.0)];
    c
}
