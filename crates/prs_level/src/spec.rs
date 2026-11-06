//! A map as data: the land, what stands on it, and where the teams fight. [`MapSpec::build`] turns
//! this into a [`Level`](crate::Level). A mod adds a map by filling in one of these.

use crate::Paint;
use glam::{Vec2, Vec3};
use prs_collision::Surface;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MapSpec {
    /// Where the scatter of trees and rubble starts from.
    pub seed: u32,
    pub terrain: TerrainSpec,
    pub style: BuildStyle,
    pub roads: Vec<Road>,
    pub ditches: Vec<Ditch>,
    /// Painted in order, so later zones lie on top of earlier ones.
    pub zones: Vec<Zone>,
    pub compounds: Vec<Compound>,
    pub walls: Vec<FieldWall>,
    pub rubble: Vec<Rubble>,
    pub props: Vec<Placed>,
    pub woods: Vec<Woodland>,
    pub teams: Vec<TeamSpec>,
    pub capture_points: Vec<CapturePointSpec>,
    /// Where a stand-in soldier can stand and fire, and which way.
    pub sentry: SpawnSpec,
    /// Where a team's helicopter starts, with room for its rotor. In the order of `teams`.
    pub helipads: Vec<SpawnSpec>,
    /// Where vehicles start: a row of the game's table of vehicles, and where.
    pub vehicles: Vec<(u8, SpawnSpec)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridges: Vec<Bridge>,
    /// Flights of steps laid down a slope, such as into a canal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Steps>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landmarks: Vec<Sunburst>,
}

/// A deck across a gap, from one end on the ground to the other, at the height of the ground at its ends,
/// with a parapet down each side and piers down to the ground below every so often.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bridge {
    pub from: Vec2,
    pub to: Vec2,
    pub width: f32,
    pub thickness: f32,
    /// How high the parapets stand above the deck. Zero leaves the sides open.
    pub parapet: f32,
    /// Longest stretch of deck between piers.
    pub span: f32,
    pub paint: Paint,
}

/// Steps from `from`, at the height of the ground there, down or up to `to`, each no higher than the
/// style's stair rise, standing on whatever the ground does below them.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Steps {
    pub from: Vec2,
    pub to: Vec2,
    pub width: f32,
    pub paint: Paint,
}

/// A landmark to steer by: a column, and rays fanned round a hub at its top in the plane facing `yaw`.
/// Every `short_every`-th ray is shorter, so the burst does not read as a wheel.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Sunburst {
    pub at: Vec2,
    pub yaw: f32,
    pub plinth: Vec3,
    pub column: Vec3,
    pub hub: f32,
    pub rays: u32,
    pub ray_length: f32,
    pub ray_width: f32,
    pub ray_depth: f32,
    pub short_every: u32,
    pub short_share: f32,
    pub paint: Paint,
    pub stone: Paint,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TerrainSpec {
    pub size: f32,
    /// Coarsest grid across the map. Cells split where the ground needs it.
    pub base_cells: u32,
    /// How many times a cell may split where the height calls for it.
    pub max_splits: u32,
    /// How many times a cell may split just to follow a change of ground colour.
    pub paint_splits: u32,
    /// Largest height error the mesh may leave, in metres.
    pub tolerance: f32,
    /// Somewhere other than the places in `detail`, the mesh may leave this much instead, so open country
    /// on a big map costs fewer triangles than a town does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_tolerance: Option<f32>,
    /// The same for following changes of ground colour: how many splits it may take out there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_paint_splits: Option<u32>,
    /// Circles, (middle, radius), held to `tolerance` when `open_tolerance` is set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<(Vec2, f32)>,
    pub waves: Vec<Wave>,
    pub hills: Vec<Hill>,
    pub calm: Vec<Calm>,
    pub rim: Rim,
}

/// A long sine swell across the land.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Wave {
    pub frequency: Vec2,
    pub phase: f32,
    pub height: f32,
}

/// A smooth elliptical rise, or a dip if `height` is negative.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Hill {
    pub at: Vec2,
    pub radius: Vec2,
    pub yaw: f32,
    pub height: f32,
}

/// Quiets waves and hills within `radius`, fading back to full by `fade`, so a town can sit on
/// gentle ground. `keep` is how much of them is left at the middle.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Calm {
    pub at: Vec2,
    pub radius: f32,
    pub fade: f32,
    pub keep: f32,
}

/// High ground around the edge. Its outer face is too steep to climb, which is what ends the map.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rim {
    /// Distance from the map edge where the ground starts to rise.
    pub inset: f32,
    pub height: f32,
    /// Width of the unclimbable face at the very edge.
    pub face: f32,
    pub face_height: f32,
    /// How far the foot of the rim wanders, so it does not run ruler straight.
    pub wander: f32,
}

/// How buildings are dimensioned. Fitted to `MoveConfig`: a test holds the cover heights to its eye
/// heights, and others walk the stairs and doorways.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct BuildStyle {
    pub wall: f32,
    pub partition: f32,
    pub storey: f32,
    pub slab: f32,
    /// Height of a ground floor above the yard: one easy step up.
    pub plinth: f32,
    /// How far walls and floors carry on below ground, so uneven ground never shows a gap.
    pub foot: f32,
    pub door_width: f32,
    pub door_head: f32,
    pub wide_door_width: f32,
    pub wide_door_head: f32,
    pub window_width: f32,
    pub window_sill: f32,
    pub window_head: f32,
    pub parapet: f32,
    pub rail: f32,
    pub stair_width: f32,
    pub stair_rise: f32,
    pub stair_run: f32,
    /// Clear height left above a tread where a flight passes under the floor above.
    pub headroom: f32,
    /// Hides a prone soldier. A crouching one can fire over it.
    pub prone_cover: f32,
    /// Hides a crouching soldier. A standing one can fire over it.
    pub crouch_cover: f32,
    /// Hides a standing soldier, with no way to see over.
    pub high_wall: f32,
    pub yard_wall: f32,
    pub yard_margin: f32,
    pub pad_fade: f32,
    pub roof_overhang: f32,
    pub roof_thickness: f32,
    pub field_wall: f32,
    pub wall_segment: f32,
    pub hedge_thickness: f32,
    pub hedge_segment: f32,
    pub awning_height: f32,
    pub awning_depth: f32,
}

impl Default for BuildStyle {
    fn default() -> Self {
        Self {
            wall: 0.3,
            partition: 0.14,
            storey: 3.0,
            slab: 0.2,
            plinth: 0.16,
            foot: 0.7,
            door_width: 1.2,
            door_head: 2.2,
            wide_door_width: 2.8,
            wide_door_head: 2.6,
            window_width: 1.1,
            window_sill: 0.95,
            window_head: 2.05,
            parapet: 0.95,
            rail: 0.95,
            stair_width: 1.2,
            stair_rise: 0.2,
            stair_run: 0.3,
            headroom: 2.15,
            prone_cover: 0.75,
            crouch_cover: 1.25,
            high_wall: 2.3,
            yard_wall: 0.35,
            yard_margin: 1.5,
            pad_fade: 7.0,
            roof_overhang: 0.35,
            roof_thickness: 0.09,
            field_wall: 0.5,
            wall_segment: 3.6,
            hedge_thickness: 1.3,
            hedge_segment: 2.2,
            awning_height: 2.75,
            awning_depth: 1.3,
        }
    }
}

/// What the ground is, which only sets its colour. The land is one surface whatever it looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Ground {
    /// Coloured by height and slope, as on the training range.
    #[default]
    Natural,
    Meadow,
    Pasture,
    Wheat,
    Stubble,
    Plough,
    Orchard,
    Road,
    Track,
    Dirt,
    Yard,
    Paving,
    DitchBed,
    Scrub,
    Shingle,
    Water,
}

impl Ground {
    /// What walking on it or shooting it sounds like.
    pub fn surface(self) -> Surface {
        match self {
            Ground::Natural | Ground::Meadow | Ground::Pasture | Ground::Wheat | Ground::Stubble | Ground::Orchard => Surface::Grass,
            Ground::Road | Ground::Shingle => Surface::Gravel,
            Ground::Paving => Surface::Concrete,
            Ground::Plough | Ground::Track | Ground::Dirt | Ground::Yard | Ground::DitchBed | Ground::Scrub | Ground::Water => Surface::Dirt,
        }
    }

    pub(crate) fn is_built(self) -> bool {
        matches!(self, Ground::Road | Ground::Track | Ground::Yard | Ground::Paving)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Road {
    pub points: Vec<Vec2>,
    pub width: f32,
    /// The ground eases back to natural over this distance beyond the edge.
    pub shoulder: f32,
    pub ground: Ground,
}

/// A trench cut into the land.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Ditch {
    pub points: Vec<Vec2>,
    /// Across the top, bank to bank.
    pub width: f32,
    pub bed_width: f32,
    pub depth: f32,
    pub ground: Ground,
    /// Cut through any road that crosses it, as a canal is, rather than be filled in under the road.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cuts_roads: bool,
    /// Stone filling each bank, so the water meets a straight wall however coarse the ground beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quay: Option<Quay>,
}

/// A canal's banks built up in blocks from its bed to the ground beside it, each filling the bank from the edge
/// of the bed to a little past its top. Left open wherever a bridge or a flight of steps meets the bank.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Quay {
    /// How far past the top of the bank each block reaches, and how far out over the edge of the bed.
    pub beyond: f32,
    pub over_bed: f32,
    /// How long each block is, at most.
    pub length: f32,
    pub paint: Paint,
}

/// A rotated rectangle of ground colour.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Zone {
    pub at: Vec2,
    pub half: Vec2,
    pub yaw: f32,
    pub ground: Ground,
}

/// Walls and buildings on one level pad of ground. Everything inside is in its local frame, with
/// x to the right and z towards the viewer at yaw zero. The side that faces +z is [`Side::S`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Compound {
    pub at: Vec2,
    pub yaw: f32,
    pub half: Vec2,
    /// Raises the pad above the natural ground, for terracing.
    pub lift: f32,
    pub ground: Ground,
    pub wall: Option<YardWall>,
    pub buildings: Vec<Building>,
    pub props: Vec<Placed>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct YardWall {
    pub height: f32,
    pub paint: Paint,
    pub gates: Vec<Gate>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Gate {
    pub side: Side,
    /// Along the wall from its middle.
    pub along: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    /// Faces -z.
    N,
    /// Faces +z.
    S,
    /// Faces +x.
    E,
    /// Faces -x.
    W,
}

impl Side {
    pub fn normal(self) -> Vec2 {
        match self {
            Side::N => Vec2::NEG_Y,
            Side::S => Vec2::Y,
            Side::E => Vec2::X,
            Side::W => Vec2::NEG_X,
        }
    }

    pub fn opposite(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::S => Side::N,
            Side::E => Side::W,
            Side::W => Side::E,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Building {
    pub at: Vec2,
    pub yaw: f32,
    /// Outer half size.
    pub half: Vec2,
    pub storeys: u32,
    /// Floor to floor. `None` takes the style's.
    pub storey_height: Option<f32>,
    pub walls: Paint,
    pub floors: Paint,
    pub stairs: Paint,
    pub roof: Roof,
    pub openings: Vec<Opening>,
    pub partitions: Vec<Partition>,
    pub flights: Vec<Flight>,
    /// Boxes fixed to it, like awnings and shutters, in its own frame with y up from the yard.
    pub fixtures: Vec<Part>,
    /// Targets standing in its rooms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetSpec>,
}

/// A target on a floor of a building, in the building's frame, turned to face `facing` (yaw zero faces +z).
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TargetSpec {
    pub at: Vec2,
    pub storey: u32,
    pub facing: f32,
    pub kind: crate::TargetKind,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum Roof {
    /// Walkable behind a parapet.
    Flat { paint: Paint },
    /// A ridge along x from gable to gable, with this pitch in radians.
    Gable { paint: Paint, pitch: f32 },
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Opening {
    pub side: Side,
    pub storey: u32,
    /// Along the wall from its middle.
    pub along: f32,
    pub kind: OpeningKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OpeningKind {
    Door,
    WideDoor,
    Window,
}

/// An inside wall along one axis, with doors at distances along it from `from`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Partition {
    pub storey: u32,
    pub from: Vec2,
    pub to: Vec2,
    pub doors: Vec<f32>,
}

/// A straight run of stairs.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Flight {
    /// Middle of the front edge of the first tread, in the building's frame.
    pub foot: Vec2,
    /// The way it climbs.
    pub ascent: Side,
    /// The storey it starts from. `None` starts on the ground outside.
    pub from: Option<u32>,
    /// The floor it reaches. The building's storey count means the roof.
    pub to: u32,
    pub finish: FlightFinish,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum FlightFinish {
    /// Comes up through a hole in the floor it reaches, with a rail on its open sides. `open_right`
    /// and `open_left` are as seen climbing.
    Through { open_right: bool, open_left: bool },
    /// Ends on a landing outside the wall, stepping through a gap in the parapet towards `exit`.
    Landing { exit: Side },
}

/// A wall across open land that follows the ground, in short straight lengths.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldWall {
    pub points: Vec<Vec2>,
    pub height: f32,
    /// Each length is up to this much taller or shorter, for a broken top.
    pub unevenness: f32,
    pub thickness: f32,
    /// How long each straight length is.
    pub segment: f32,
    /// Each length strays this far sideways, as a hedge does.
    pub wander: f32,
    pub paint: Paint,
    /// Stretches left open, as (from, to) in metres along the wall.
    pub gaps: Vec<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rubble {
    pub at: Vec2,
    pub radius: f32,
    pub count: u32,
    pub size: (f32, f32),
    pub paint: Paint,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Placed {
    pub prop: Prop,
    pub at: Vec2,
    pub yaw: f32,
}

/// Small solid things. [`Prop::parts`] says what each is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Prop {
    Crate,
    CrateStack,
    Drums,
    HayBale,
    Sandbags,
    Barrier,
    Wreck,
    Truck,
    Well,
    Stall,
    Hesco,
    Tank,
    Tomb,
    Boxcar,
    Kiln,
    WaterTower,
    Canopy,
    Mast,
}

/// One box of a prop: offset from the prop's origin on the ground, half size, yaw, paint.
pub type Part = ([f32; 3], [f32; 3], f32, Paint);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Woodland {
    pub kind: crate::TreeKind,
    pub shape: WoodShape,
    pub height: (f32, f32),
    pub radius: (f32, f32),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum WoodShape {
    /// Trees along a line, about `spacing` apart.
    Line { points: Vec<Vec2>, spacing: f32, jitter: f32 },
    /// A random clump.
    Clump { at: Vec2, half: Vec2, yaw: f32, count: u32 },
    /// Regular rows, as an orchard is planted.
    Rows { at: Vec2, half: Vec2, yaw: f32, spacing: f32 },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TeamSpec {
    pub name: String,
    pub spawns: Vec<SpawnSpec>,
    /// How far from the middle of its spawns the base reaches. See [`crate::Team::base_radius`].
    pub base_radius: f32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpawnSpec {
    pub at: Vec2,
    pub yaw: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CapturePointSpec {
    pub name: String,
    pub at: Vec2,
    pub radius: f32,
}

impl Prop {
    pub fn parts(self) -> &'static [Part] {
        use Paint::{Brick, Canvas, Cement, Hay, Metal, Rock, Rust, Sandbag, Stone, Terracotta, Tin, Wood};
        match self {
            Prop::Crate => &[([0.0, 0.5, 0.0], [0.6, 0.5, 0.6], 0.0, Wood)],
            Prop::CrateStack => &[
                ([0.0, 0.55, 0.0], [0.7, 0.55, 0.7], 0.0, Wood),
                ([1.35, 0.45, 0.25], [0.55, 0.45, 0.55], 0.4, Wood),
                ([0.1, 1.45, 0.05], [0.5, 0.35, 0.5], 0.5, Wood),
            ],
            // A pair of boxes turned 45 degrees apart reads as round from any distance.
            Prop::Drums => &[
                ([0.0, 0.45, 0.0], [0.27, 0.45, 0.27], 0.0, Rust),
                ([0.0, 0.45, 0.0], [0.27, 0.45, 0.27], 0.785, Rust),
                ([0.68, 0.45, 0.12], [0.27, 0.45, 0.27], 0.3, Metal),
                ([0.68, 0.45, 0.12], [0.27, 0.45, 0.27], 1.085, Metal),
            ],
            Prop::HayBale => &[
                ([0.0, 0.6, 0.0], [1.2, 0.6, 0.6], 0.0, Hay),
                ([0.15, 0.55, 1.3], [1.1, 0.55, 0.6], 0.09, Hay),
            ],
            Prop::Sandbags => &[
                ([0.0, 0.3, 0.0], [1.5, 0.3, 0.42], 0.0, Sandbag),
                ([0.05, 0.72, 0.0], [1.42, 0.2, 0.36], 0.0, Sandbag),
                ([-0.04, 1.08, 0.0], [1.34, 0.17, 0.32], 0.0, Sandbag),
            ],
            Prop::Barrier => &[
                ([0.0, 0.2, 0.0], [1.5, 0.2, 0.34], 0.0, Stone),
                ([0.0, 0.7, 0.0], [1.5, 0.45, 0.14], 0.0, Stone),
            ],
            Prop::Wreck => &[
                ([0.0, 0.42, 0.0], [0.92, 0.32, 2.1], 0.0, Rust),
                ([0.0, 0.95, -0.2], [0.8, 0.27, 1.05], 0.0, Rust),
                ([0.0, 1.24, -0.2], [0.72, 0.03, 0.9], 0.0, Metal),
            ],
            Prop::Truck => &[
                ([0.0, 0.5, 0.0], [1.1, 0.4, 3.3], 0.0, Metal),
                ([0.0, 1.45, -2.25], [1.1, 0.75, 1.0], 0.0, Metal),
                ([0.0, 1.5, 0.95], [1.12, 0.65, 2.3], 0.0, Canvas),
            ],
            Prop::Well => &[
                ([0.0, 0.42, -0.85], [1.0, 0.42, 0.18], 0.0, Stone),
                ([0.0, 0.42, 0.85], [1.0, 0.42, 0.18], 0.0, Stone),
                ([-0.85, 0.42, 0.0], [0.18, 0.42, 0.68], 0.0, Stone),
                ([0.85, 0.42, 0.0], [0.18, 0.42, 0.68], 0.0, Stone),
            ],
            Prop::Stall => &[
                ([-1.5, 1.2, -0.9], [0.05, 1.2, 0.05], 0.0, Wood),
                ([1.5, 1.2, -0.9], [0.05, 1.2, 0.05], 0.0, Wood),
                ([-1.5, 1.2, 0.9], [0.05, 1.2, 0.05], 0.0, Wood),
                ([1.5, 1.2, 0.9], [0.05, 1.2, 0.05], 0.0, Wood),
                ([0.0, 2.45, 0.0], [1.75, 0.04, 1.2], 0.0, Canvas),
                ([0.0, 0.45, -0.3], [1.4, 0.45, 0.45], 0.0, Wood),
            ],
            Prop::Hesco => &[
                ([-1.05, 0.95, 0.0], [0.5, 0.95, 0.5], 0.0, Sandbag),
                ([0.0, 0.95, 0.0], [0.5, 0.95, 0.5], 0.0, Sandbag),
                ([1.05, 0.95, 0.0], [0.5, 0.95, 0.5], 0.0, Sandbag),
            ],
            Prop::Tank => &[
                ([-0.8, 1.4, -0.8], [0.07, 1.4, 0.07], 0.0, Tin),
                ([0.8, 1.4, -0.8], [0.07, 1.4, 0.07], 0.0, Tin),
                ([-0.8, 1.4, 0.8], [0.07, 1.4, 0.07], 0.0, Tin),
                ([0.8, 1.4, 0.8], [0.07, 1.4, 0.07], 0.0, Tin),
                ([0.0, 3.5, 0.0], [1.0, 0.75, 1.0], 0.0, Tin),
                ([0.0, 3.5, 0.0], [1.0, 0.75, 1.0], 0.785, Tin),
            ],
            Prop::Tomb => &[
                ([0.0, 0.25, 0.0], [0.4, 0.25, 0.9], 0.0, Rock),
                ([0.0, 0.75, -0.85], [0.36, 0.45, 0.07], 0.0, Stone),
            ],
            // A goods wagon on its wheels: hard cover a squad can work along, too high to see over.
            Prop::Boxcar => &[
                ([0.0, 0.45, 0.0], [1.2, 0.45, 6.4], 0.0, Metal),
                ([0.0, 2.1, 0.0], [1.45, 1.2, 7.0], 0.0, Rust),
                ([0.0, 3.34, 0.0], [1.3, 0.04, 6.9], 0.0, Tin),
            ],
            // A brick kiln with its chimney, the tallest thing for a long way.
            Prop::Kiln => &[
                ([0.0, 1.6, 0.0], [3.0, 1.6, 2.2], 0.0, Brick),
                ([0.0, 3.4, 0.0], [2.6, 0.2, 1.8], 0.0, Brick),
                ([2.2, 7.0, 1.2], [0.5, 7.0, 0.5], 0.0, Brick),
            ],
            Prop::WaterTower => &[
                ([-1.4, 4.0, -1.4], [0.12, 4.0, 0.12], 0.0, Metal),
                ([1.4, 4.0, -1.4], [0.12, 4.0, 0.12], 0.0, Metal),
                ([-1.4, 4.0, 1.4], [0.12, 4.0, 0.12], 0.0, Metal),
                ([1.4, 4.0, 1.4], [0.12, 4.0, 0.12], 0.0, Metal),
                ([0.0, 9.4, 0.0], [1.9, 1.5, 1.9], 0.0, Terracotta),
                ([0.0, 9.4, 0.0], [1.9, 1.5, 1.9], 0.785, Terracotta),
                ([0.0, 11.0, 0.0], [1.4, 0.1, 1.4], 0.0, Terracotta),
            ],
            // A radio mast: a lattice tower, seen from across the map.
            Prop::Mast => &[
                ([-0.5, 12.0, -0.5], [0.08, 12.0, 0.08], 0.0, Metal),
                ([0.5, 12.0, -0.5], [0.08, 12.0, 0.08], 0.0, Metal),
                ([-0.5, 12.0, 0.5], [0.08, 12.0, 0.08], 0.0, Metal),
                ([0.5, 12.0, 0.5], [0.08, 12.0, 0.08], 0.0, Metal),
                ([0.0, 6.0, 0.0], [0.6, 0.06, 0.6], 0.0, Metal),
                ([0.0, 12.0, 0.0], [0.6, 0.06, 0.6], 0.0, Metal),
                ([0.0, 18.0, 0.0], [0.6, 0.06, 0.6], 0.0, Metal),
                ([0.0, 25.0, 0.0], [0.06, 1.0, 0.06], 0.0, Terracotta),
                ([0.0, 1.0, -1.6], [1.2, 1.0, 0.9], 0.0, Cement),
            ],
            // A canvas roof on posts, over a market alley, high enough to walk under.
            Prop::Canopy => &[
                ([-2.2, 1.4, -3.0], [0.06, 1.4, 0.06], 0.0, Wood),
                ([2.2, 1.4, -3.0], [0.06, 1.4, 0.06], 0.0, Wood),
                ([-2.2, 1.4, 3.0], [0.06, 1.4, 0.06], 0.0, Wood),
                ([2.2, 1.4, 3.0], [0.06, 1.4, 0.06], 0.0, Wood),
                ([0.0, 2.84, 0.0], [2.4, 0.04, 3.2], 0.0, Canvas),
            ],
        }
    }
}
