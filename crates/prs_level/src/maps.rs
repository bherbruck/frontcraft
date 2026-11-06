//! Maps as files, so a server can play one that no client was built with. A file is TOML, like a mod: a
//! `[spec]` that builds the map from a seed and a description, as the town does, or a `[terrain]` of heights
//! with everything on it listed, as the range does, and either way pieces placed by hand on top. One piece
//! is one line, so a change to a map reads as a change to those lines.
//!
//! The game carries `maps/*.toml` inside it, for playing alone and for tests. A folder of maps beside the
//! program comes first, so a server can be given a new one without a new build.

use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::fmt;

use crate::spec::WoodShape;
use crate::{CapturePoint, Doorway, Level, MapSpec, Room, Solid, SpawnPoint, Staircase, TargetMark, Team, Terrain, Tree, VehicleSpawn, Window};

/// What a file says it is written for. A file for a later one is refused, not misread.
pub const FORMAT: u32 = 1;

/// The maps the game is built with, as their files.
pub const BUILT_IN: &[(&str, &str)] = &[("range", include_str!("../../../maps/range.toml")), ("town", include_str!("../../../maps/town.toml"))];

/// How big a map may make itself. A map can come from any server, so it is held to these before anything is
/// built from it: nothing in it may ask for more memory or time than a real map would.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapLimits {
    /// Metres across.
    pub max_size: f32,
    /// The finest grid across the terrain: a height grid's cells, or a spec's coarsest cells split all the way.
    pub max_cells: u32,
    pub max_splits: u32,
    /// Entries in all the lists in the file together.
    pub max_items: usize,
    /// Trees a spec's woods would plant, and pieces of rubble it would scatter.
    pub max_trees: u64,
    pub max_rubble: u64,
    /// Lengths of field wall and steps of stairs it would put up, together.
    pub max_pieces: u64,
    pub max_storeys: u32,
}

impl Default for MapLimits {
    /// Several times the town in each, which is itself a kilometre across.
    fn default() -> Self {
        Self { max_size: 4096.0, max_cells: 2048, max_splits: 6, max_items: 200_000, max_trees: 50_000, max_rubble: 20_000, max_pieces: 100_000, max_storeys: 16 }
    }
}

/// A grid of ground heights, row by row from the north-west corner, `cells + 1` to a row.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightGrid {
    pub size: f32,
    pub cells: u32,
    pub heights: Vec<f32>,
}

/// A map file as it is written.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapFile {
    pub format: u32,
    /// Where a player alone starts. Needed when there is no spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn: Option<SpawnPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentry: Option<SpawnPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<Solid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trees: Vec<Tree>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<Vec3>,
    /// How each target stands, by the same index as `targets`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_marks: Vec<TargetMark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rooms: Vec<Room>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doorways: Vec<Doorway>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<Window>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staircases: Vec<Staircase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helipads: Vec<SpawnPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vehicles: Vec<VehicleSpawn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<Team>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capture_points: Vec<CapturePoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain: Option<HeightGrid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<MapSpec>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MapError {
    NotFound(String),
    BadName(String),
    Syntax { name: String, message: String },
    Format { name: String, format: u32 },
    Incomplete { name: String, missing: &'static str },
    Heights { name: String, expected: usize, found: usize },
    TooBig { name: String, what: String },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MapError::NotFound(name) => write!(f, "no map called {name}"),
            MapError::BadName(name) => write!(f, "{name:?} is not a map name: letters, digits, - and _ only"),
            MapError::Syntax { name, message } => write!(f, "map {name}: {message}"),
            MapError::Format { name, format } => write!(f, "map {name} is format {format}, and this game reads up to {FORMAT}"),
            MapError::Incomplete { name, missing } => write!(f, "map {name} has no spec, so it needs {missing}"),
            MapError::Heights { name, expected, found } => write!(f, "map {name} needs {expected} heights for its grid and has {found}"),
            MapError::TooBig { name, what } => write!(f, "map {name} is past what this game builds: {what}"),
        }
    }
}

/// FNV-1a over the bytes of the file. Both sides hash what was sent, so it does not matter how the numbers
/// in it come out once built.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Whether this can be a map's name: 1 to 32 of lower case letters, digits, `-` and `_`. A name comes from
/// whatever server was joined, and goes into a file name and an address, so nothing else ever passes.
pub fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// One map's file, as read or as downloaded, and its hash.
#[derive(Clone, Debug, PartialEq)]
pub struct MapData {
    pub name: String,
    pub text: String,
    pub hash: u64,
}

impl MapData {
    pub fn new(name: &str, text: String) -> Self {
        Self { name: name.to_owned(), hash: hash_bytes(text.as_bytes()), text }
    }

    /// The map the game was built with, by this name.
    pub fn built_in(name: &str) -> Option<Self> {
        BUILT_IN.iter().find(|(n, _)| *n == name).map(|(n, text)| Self::new(n, (*text).to_owned()))
    }

    /// A map file at a path, named after the file. Only for what whoever runs the program asks for, such as a
    /// server's `--map`: never for a name that came from anywhere else.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(path: &std::path::Path) -> Result<Self, MapError> {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if !valid_name(stem) {
            return Err(MapError::BadName(stem.to_owned()));
        }
        std::fs::read_to_string(path).map(|text| Self::new(stem, text)).map_err(|_| MapError::NotFound(path.display().to_string()))
    }

    /// A map by name from the maps folder, `PRS_MAPS_DIR` or `maps` where the program runs, or else one built
    /// in. Only ever a plain name, so whatever a server says, nothing outside that folder is read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn find(name: &str) -> Result<Self, MapError> {
        if !valid_name(name) {
            return Err(MapError::BadName(name.to_owned()));
        }
        let folder = std::env::var("PRS_MAPS_DIR").unwrap_or_else(|_| "maps".into());
        match std::fs::read_to_string(std::path::Path::new(&folder).join(format!("{name}.toml"))) {
            Ok(text) => Ok(Self::new(name, text)),
            Err(_) => Self::built_in(name).ok_or_else(|| MapError::NotFound(name.to_owned())),
        }
    }

    /// A browser has no folder of its own, so only what is built in, and whatever a server sends.
    #[cfg(target_arch = "wasm32")]
    pub fn find(name: &str) -> Result<Self, MapError> {
        if !valid_name(name) {
            return Err(MapError::BadName(name.to_owned()));
        }
        Self::built_in(name).ok_or_else(|| MapError::NotFound(name.to_owned()))
    }

    /// The level, held to the usual limits.
    pub fn level(&self) -> Result<Level, MapError> {
        self.level_within(&MapLimits::default())
    }

    pub fn level_within(&self, limits: &MapLimits) -> Result<Level, MapError> {
        let syntax = |error: toml::de::Error| MapError::Syntax { name: self.name.clone(), message: error.to_string() };
        let value: toml::Value = toml::from_str(&self.text).map_err(syntax)?;
        let too_big = |what: String| MapError::TooBig { name: self.name.clone(), what };
        let mut items = 0;
        plain(&value, limits, &mut items).map_err(too_big)?;
        let file = MapFile::deserialize(value).map_err(syntax)?;
        file.within(limits).map_err(too_big)?;
        let mut level = file.into_level(&self.name)?;
        level.source = self.hash;
        Ok(level)
    }
}

/// Every number is a number no further out than the map is wide, and there are not more entries than a map
/// has. Every length, spacing and coordinate in a map is one of these, so nothing worked out from them can
/// come to infinity or not a number.
fn plain(value: &toml::Value, limits: &MapLimits, items: &mut usize) -> Result<(), String> {
    let past = |n: f64| !(n.abs() <= limits.max_size as f64);
    match value {
        toml::Value::Float(f) if past(*f) => Err(format!("a number past the size of a map: {f}")),
        // A whole number goes into a length as readily as any other.
        toml::Value::Integer(i) if past(*i as f64) => Err(format!("a number past the size of a map: {i}")),
        toml::Value::Array(list) => {
            *items += list.len();
            if *items > limits.max_items {
                return Err(format!("more than {} entries", limits.max_items));
            }
            list.iter().try_for_each(|v| plain(v, limits, items))
        }
        // Only the seed is a number that is no size of anything.
        toml::Value::Table(table) => table.iter().filter(|(key, _)| key.as_str() != "seed").try_for_each(|(_, v)| plain(v, limits, items)),
        _ => Ok(()),
    }
}

impl MapFile {
    /// Whether building this would stay inside the limits, worked out from the description alone, for every
    /// loop the builder runs that the file can make longer. Each comparison is written so that a value that is
    /// not a number fails it.
    fn within(&self, limits: &MapLimits) -> Result<(), String> {
        let size = |size: f32| if size > 0.0 && size <= limits.max_size { Ok(()) } else { Err(format!("{size} m across")) };
        let positive = |what: &str, v: f32| if v > 0.0 { Ok(()) } else { Err(format!("a {what} of {v}")) };
        if let Some(grid) = &self.terrain {
            size(grid.size)?;
            if grid.cells == 0 || grid.cells > limits.max_cells {
                return Err(format!("a grid of {} cells", grid.cells));
            }
        }
        let Some(spec) = &self.spec else { return Ok(()) };
        let terrain = &spec.terrain;
        size(terrain.size)?;
        let splits = terrain.max_splits.max(terrain.paint_splits);
        let finest = (terrain.base_cells as u64).checked_shl(splits.min(63)).unwrap_or(u64::MAX);
        if terrain.base_cells == 0 || splits > limits.max_splits || finest > limits.max_cells as u64 {
            return Err(format!("{} cells split {splits} times", terrain.base_cells));
        }
        let length = |points: &[glam::Vec2]| points.windows(2).map(|p| p[0].distance(p[1]) as f64).sum::<f64>();
        let mut trees = 0.0_f64;
        for wood in &spec.woods {
            trees += match &wood.shape {
                WoodShape::Line { points, spacing, .. } => {
                    positive("tree spacing", *spacing)?;
                    length(points) / *spacing as f64 + points.len() as f64
                }
                WoodShape::Clump { count, .. } => *count as f64,
                WoodShape::Rows { half, spacing, .. } => {
                    positive("tree spacing", *spacing)?;
                    let spacing = *spacing as f64;
                    (half.x.abs() as f64 * 2.0 / spacing + 1.0) * (half.y.abs() as f64 * 2.0 / spacing + 1.0)
                }
            };
        }
        if !(trees <= limits.max_trees as f64) {
            return Err(format!("about {trees:.0} trees"));
        }
        let rubble: u64 = spec.rubble.iter().map(|r| r.count as u64).sum();
        if rubble > limits.max_rubble {
            return Err(format!("{rubble} pieces of rubble"));
        }
        let mut pieces = 0.0_f64;
        for wall in &spec.walls {
            positive("wall segment", wall.segment)?;
            pieces += length(&wall.points) / wall.segment as f64 + wall.points.len() as f64;
        }
        let style = &spec.style;
        positive("stair rise", style.stair_rise)?;
        if !(style.plinth >= 0.0) {
            return Err(format!("a plinth of {}", style.plinth));
        }
        for building in spec.compounds.iter().flat_map(|c| &c.buildings) {
            if building.storeys > limits.max_storeys {
                return Err(format!("a building of {} storeys", building.storeys));
            }
            let storey = building.storey_height.unwrap_or(style.storey);
            positive("storey height", storey)?;
            for flight in &building.flights {
                if flight.to > building.storeys || flight.from.is_some_and(|from| from > flight.to) {
                    return Err(format!("stairs to storey {} of a building of {}", flight.to, building.storeys));
                }
                // As the builder climbs it: from the floor it starts on, or the ground, to the floor it reaches.
                let floor = |level: u32| style.plinth as f64 + storey as f64 * level as f64;
                let climb = floor(flight.to) - flight.from.map_or(0.0, floor);
                pieces += (climb / style.stair_rise as f64).ceil().max(0.0);
            }
        }
        if !(pieces <= limits.max_pieces as f64) {
            return Err(format!("about {pieces:.0} lengths of wall and steps of stairs"));
        }
        Ok(())
    }
}

impl Terrain {
    /// The same grid [`Terrain::generate`] makes, with its heights given rather than worked out.
    pub fn from_heights(grid: &HeightGrid) -> Self {
        let next = Cell::new(0);
        Terrain::generate(grid.size, grid.cells, |_| {
            let at = next.get();
            next.set(at + 1);
            grid.heights[at]
        })
    }
}

impl MapFile {
    fn into_level(self, name: &str) -> Result<Level, MapError> {
        if self.format > FORMAT {
            return Err(MapError::Format { name: name.to_owned(), format: self.format });
        }
        let incomplete = |missing| MapError::Incomplete { name: name.to_owned(), missing };
        let mut level = match (self.spec, self.terrain) {
            (Some(spec), _) => spec.build(),
            (None, Some(grid)) => {
                let row = grid.cells as usize + 1;
                let expected = row.checked_mul(row).ok_or_else(|| incomplete("a grid it can hold"))?;
                if grid.heights.len() != expected {
                    return Err(MapError::Heights { name: name.to_owned(), expected, found: grid.heights.len() });
                }
                let spawn = self.spawn.ok_or_else(|| incomplete("a spawn"))?;
                Level {
                    terrain: Terrain::from_heights(&grid),
                    solids: Vec::new(),
                    trees: Vec::new(),
                    targets: Vec::new(),
                    target_marks: Vec::new(),
                    spawn: spawn.pos,
                    spawn_yaw: spawn.yaw,
                    teams: Vec::new(),
                    capture_points: Vec::new(),
                    rooms: Vec::new(),
                    doorways: Vec::new(),
                    windows: Vec::new(),
                    staircases: Vec::new(),
                    sentry: self.sentry.ok_or_else(|| incomplete("a sentry"))?,
                    helipads: Vec::new(),
                    vehicles: Vec::new(),
                    source: 0,
                }
            }
            (None, None) => return Err(incomplete("a terrain")),
        };
        if let Some(spawn) = self.spawn {
            level.spawn = spawn.pos;
            level.spawn_yaw = spawn.yaw;
        }
        if let Some(sentry) = self.sentry {
            level.sentry = sentry;
        }
        level.solids.extend(self.solids);
        level.trees.extend(self.trees);
        level.targets.extend(self.targets);
        level.target_marks.extend(self.target_marks);
        level.rooms.extend(self.rooms);
        level.doorways.extend(self.doorways);
        level.windows.extend(self.windows);
        level.staircases.extend(self.staircases);
        level.helipads.extend(self.helipads);
        level.vehicles.extend(self.vehicles);
        level.teams.extend(self.teams);
        level.capture_points.extend(self.capture_points);
        Ok(level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value with every number as short as it can be and still read back as the same `f32`.
    fn short(value: impl Serialize) -> toml::Value {
        fn tidy(value: &mut toml::Value) {
            match value {
                toml::Value::Float(f) => *f = format!("{}", *f as f32).parse().unwrap(),
                toml::Value::Array(items) => items.iter_mut().for_each(tidy),
                toml::Value::Table(table) => table.iter_mut().for_each(|(_, v)| tidy(v)),
                _ => {}
            }
        }
        let mut value = toml::Value::try_from(value).expect("a map is plain data");
        tidy(&mut value);
        value
    }

    /// Writes a map with each piece on a line of its own, the heights a row to a line, and the spec last.
    fn write(file: &MapFile) -> String {
        let mut out = format!("# Written from code by `PRS_WRITE_MAPS=1 cargo test -p prs_level maps`.\nformat = {}\n", file.format);
        if let Some(spawn) = &file.spawn {
            out += &format!("spawn = {}\n", short(spawn));
        }
        if let Some(sentry) = &file.sentry {
            out += &format!("sentry = {}\n", short(sentry));
        }
        fn list<T: Serialize>(out: &mut String, key: &str, items: &[T]) {
            if items.is_empty() {
                return;
            }
            *out += &format!("{key} = [\n");
            for item in items {
                *out += &format!("  {},\n", short(item));
            }
            *out += "]\n";
        }
        list(&mut out, "solids", &file.solids);
        list(&mut out, "trees", &file.trees);
        list(&mut out, "targets", &file.targets);
        list(&mut out, "target_marks", &file.target_marks);
        list(&mut out, "rooms", &file.rooms);
        list(&mut out, "doorways", &file.doorways);
        list(&mut out, "windows", &file.windows);
        list(&mut out, "staircases", &file.staircases);
        list(&mut out, "helipads", &file.helipads);
        list(&mut out, "vehicles", &file.vehicles);
        list(&mut out, "teams", &file.teams);
        list(&mut out, "capture_points", &file.capture_points);
        if let Some(grid) = &file.terrain {
            out += &format!("\n[terrain]\nsize = {}\ncells = {}\nheights = [\n", short(grid.size), grid.cells);
            for row in grid.heights.chunks(grid.cells as usize + 1) {
                let row: Vec<String> = row.iter().map(|h| short(h).to_string()).collect();
                out += &format!("  {},\n", row.join(", "));
            }
            out += "]\n";
        }
        if let Some(spec) = &file.spec {
            #[derive(Serialize)]
            struct Only<'a> {
                spec: &'a MapSpec,
            }
            out += "\n";
            out += &toml::to_string(&short(Only { spec })).expect("a spec is plain data");
        }
        out
    }

    fn range_file() -> MapFile {
        let level = Level::training_range();
        let terrain = &level.terrain;
        let row = terrain.cells as usize + 1;
        assert_eq!(terrain.vertices.len(), row * row, "the range is a plain grid");
        MapFile {
            format: FORMAT,
            spawn: Some(SpawnPoint { pos: level.spawn, yaw: level.spawn_yaw }),
            sentry: Some(level.sentry),
            solids: level.solids,
            trees: level.trees,
            targets: level.targets,
            target_marks: level.target_marks,
            rooms: level.rooms,
            doorways: level.doorways,
            windows: level.windows,
            staircases: level.staircases,
            helipads: level.helipads,
            vehicles: level.vehicles,
            teams: level.teams,
            capture_points: level.capture_points,
            terrain: Some(HeightGrid { size: terrain.size, cells: terrain.cells, heights: terrain.vertices.iter().map(|v| v.y).collect() }),
            spec: None,
        }
    }

    fn town_file() -> MapFile {
        MapFile { format: FORMAT, spec: Some(MapSpec::town()), ..MapFile::default() }
    }

    fn same(a: &Level, b: &Level) -> bool {
        format!("{a:?}") == format!("{b:?}")
    }

    /// The files are what the code makes, to the bit. `PRS_WRITE_MAPS=1` writes them again from the code.
    #[test]
    fn the_map_files_are_exactly_the_maps_the_code_builds() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../maps");
        let maps = [("range", range_file(), Level::training_range()), ("town", town_file(), MapSpec::town().build())];
        if std::env::var("PRS_WRITE_MAPS").is_ok() {
            for (name, file, _) in &maps {
                std::fs::write(folder.join(format!("{name}.toml")), write(file)).unwrap();
            }
        }
        for (name, file, built) in maps {
            let text = write(&file);
            let written = MapData::new(name, text).level().unwrap();
            assert!(same(&written, &Level { source: written.source, ..built.clone() }), "{name} does not come back the same from what is written");
            if std::env::var("PRS_WRITE_MAPS").is_ok() {
                // What is built in is the file from before it was just written.
                continue;
            }
            let shipped = MapData::built_in(name).unwrap();
            let from_file = shipped.level().unwrap();
            assert_eq!(from_file.source, shipped.hash);
            assert!(
                same(&from_file, &Level { source: shipped.hash, ..built }),
                "maps/{name}.toml is not what the code builds. Run PRS_WRITE_MAPS=1 cargo test -p prs_level maps"
            );
        }
    }

    #[test]
    fn a_map_by_name_is_the_one_built_in_and_carries_its_hash() {
        let town = Level::named("town").unwrap();
        assert_eq!(town.source, MapData::built_in("town").unwrap().hash);
        assert_ne!(town.source, Level::named("range").unwrap().source);
        assert!(Level::named("nowhere").is_none());
    }

    #[test]
    fn pieces_placed_by_hand_go_on_top_of_a_spec() {
        let base = MapData::built_in("town").unwrap();
        let text = format!("{}\n", base.text.replacen(
            "format = 1\n",
            "format = 1\nsolids = [\n  { center = [1.0, 2.0, 3.0], half = [0.5, 0.5, 0.5], rotation = [0.0, 0.0, 0.0, 1.0], paint = \"Concrete\" },\n]\ntargets = [[4.0, 5.0, 6.0]]\n",
            1,
        ));
        let extra = MapData::new("town2", text).level().unwrap();
        let plain = base.level().unwrap();
        assert_eq!(extra.solids.len(), plain.solids.len() + 1);
        assert_eq!(extra.solids.last().unwrap().center, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(extra.targets.last(), Some(&Vec3::new(4.0, 5.0, 6.0)));
        assert_ne!(extra.source, plain.source, "a different file is a different map");
    }

    #[test]
    fn a_bad_map_says_what_is_wrong_with_it() {
        let level = |text: &str| MapData::new("bad", text.to_owned()).level().map(|_| ());
        assert_eq!(level("format = 1\n"), Err(MapError::Incomplete { name: "bad".into(), missing: "a terrain" }));
        assert_eq!(level("format = 9\n"), Err(MapError::Format { name: "bad".into(), format: 9 }));
        assert!(matches!(level("format = 1\nsolidz = []\n"), Err(MapError::Syntax { .. })));
        assert_eq!(
            level("format = 1\nspawn = { pos = [0.0, 0.0, 0.0], yaw = 0.0 }\nsentry = { pos = [0.0, 0.0, 0.0], yaw = 0.0 }\n[terrain]\nsize = 10.0\ncells = 2\nheights = [0.0]\n"),
            Err(MapError::Heights { name: "bad".into(), expected: 9, found: 1 })
        );
        let flat = level("format = 1\nspawn = { pos = [0.0, 0.0, 0.0], yaw = 0.0 }\nsentry = { pos = [0.0, 0.0, 0.0], yaw = 0.0 }\n[terrain]\nsize = 10.0\ncells = 1\nheights = [0.0, 0.0, 0.0, 0.0]\n");
        assert_eq!(flat, Ok(()));
    }

    /// A map from a hostile server, asking for more than any map needs, is refused before it is built.
    #[test]
    fn a_map_that_asks_for_too_much_is_refused_before_anything_is_built() {
        let level = |text: &str| MapData::new("big", text.to_owned()).level();
        let grid = |cells: u64| format!("format = 1\nspawn = {{ pos = [0.0, 0.0, 0.0], yaw = 0.0 }}\nsentry = {{ pos = [0.0, 0.0, 0.0], yaw = 0.0 }}\n[terrain]\nsize = 10.0\ncells = {cells}\nheights = [0.0]\n");
        assert!(matches!(level(&grid(4_294_967_295)), Err(MapError::TooBig { .. })), "cells that overflow");
        assert!(matches!(level(&grid(100_000)), Err(MapError::TooBig { .. })));
        assert!(matches!(level("format = 1\nspawn = { pos = [nan, 0.0, 0.0], yaw = 0.0 }\n"), Err(MapError::TooBig { .. })), "not a number");
        let town = MapData::built_in("town").unwrap().text;
        let swap = |from: &str, to: &str| {
            assert!(town.contains(from), "{from}");
            level(&town.replacen(from, to, 1))
        };
        assert!(matches!(swap("base_cells = 64", "base_cells = 4000000"), Err(MapError::TooBig { .. })));
        assert!(matches!(swap("max_splits = 3", "max_splits = 40"), Err(MapError::TooBig { .. })));
        assert!(matches!(swap("size = 1024.0", "size = 1e30"), Err(MapError::TooBig { .. })));
        // A key and its value, which may be inside an inline table.
        let line_of = |key: &str| {
            let at = town.find(key).unwrap();
            let end = at + key.len() + town[at + key.len()..].find([',', '\n', '}']).unwrap();
            town[at..end].to_owned()
        };
        let spacing = line_of("spacing = ");
        assert!(matches!(swap(&spacing, "spacing = 0.000001"), Err(MapError::TooBig { .. })), "trees planted a micron apart");
        assert!(matches!(swap(&spacing, "spacing = 0.0"), Err(MapError::TooBig { .. })), "or no distance apart");
        assert!(matches!(swap(&spacing, "spacing = -1.0"), Err(MapError::TooBig { .. })));
        assert!(matches!(swap(&spacing, "spacing = nan"), Err(MapError::TooBig { .. })), "a wood with a spacing that is not a number");
        assert!(matches!(swap(&line_of("\nsegment = "), "\nsegment = 0.00001"), Err(MapError::TooBig { .. })), "a wall in tiny lengths");
        assert!(matches!(swap(&line_of("\nstair_rise = "), "\nstair_rise = 0.00001"), Err(MapError::TooBig { .. })), "stairs with tiny steps");
        assert!(matches!(swap(&line_of("\nto = "), "\nto = 1000000000"), Err(MapError::TooBig { .. })), "stairs to storey 1e9");
        assert!(matches!(swap(&line_of("\nstoreys = "), "\nstoreys = 1000000"), Err(MapError::TooBig { .. })));
        assert!(matches!(swap("seed = 1307", "seed = 1307\nignored = 1e300"), Err(MapError::TooBig { .. })), "a number past any map");
        assert!(matches!(swap(&line_of("\nplinth = "), "\nplinth = -1.0"), Err(MapError::TooBig { .. })), "a plinth below ground");
        let style = town.find("[spec.style]").unwrap();
        let storey = style + town[style..].find("\nstorey = ").unwrap();
        let end = storey + 1 + town[storey + 1..].find('\n').unwrap();
        let flat = format!("{}\nstorey = 0.0{}", &town[..storey], &town[end..]);
        assert!(matches!(level(&flat), Err(MapError::TooBig { .. })), "storeys of no height");
        let half = town.find("half = [").unwrap();
        let first = half + "half = [".len();
        let comma = first + town[first..].find(',').unwrap();
        let whole = format!("{}99999999999{}", &town[..first], &town[comma..]);
        assert!(matches!(level(&whole), Err(MapError::TooBig { .. })), "a whole number past any map");
        assert!(swap("seed = 1307", "seed = 4000000000").is_ok(), "a seed can be any u32");
        let many = format!("format = 1\ntargets = [{}]\n", vec!["[0.0, 0.0, 0.0]"; 200_001].join(", "));
        assert!(matches!(level(&many), Err(MapError::TooBig { .. })));
        assert!(MapData::built_in("town").unwrap().level().is_ok() && MapData::built_in("range").unwrap().level().is_ok(), "the real maps fit");
    }

    #[test]
    fn a_name_is_only_ever_a_plain_word() {
        assert!(valid_name("range") && valid_name("kill-house_2"));
        let long = "a".repeat(33);
        for bad in ["", "../town", "a/b", "town.toml", "a b", "é", "Town", "/etc/passwd", "..", "C:\\x", long.as_str()] {
            assert!(!valid_name(bad), "{bad}");
        }
        for path in ["../etc/passwd", "../../secret.toml", "/etc/hosts.toml", "maps/../town.toml", "C:\\Windows\\x.toml"] {
            assert!(matches!(MapData::find(path), Err(MapError::BadName(_))), "{path}");
        }
    }
}
