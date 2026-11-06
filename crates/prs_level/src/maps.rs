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

use crate::{CapturePoint, Doorway, Level, MapSpec, Room, Solid, SpawnPoint, Staircase, Team, Terrain, Tree, VehicleSpawn, Window};

/// What a file says it is written for. A file for a later one is refused, not misread.
pub const FORMAT: u32 = 1;

/// The maps the game is built with, as their files.
pub const BUILT_IN: &[(&str, &str)] = &[("range", include_str!("../../../maps/range.toml")), ("town", include_str!("../../../maps/town.toml"))];

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

    pub fn level(&self) -> Result<Level, MapError> {
        let file: MapFile = toml::from_str(&self.text).map_err(|error| MapError::Syntax { name: self.name.clone(), message: error.to_string() })?;
        let mut level = file.into_level(&self.name)?;
        level.source = self.hash;
        Ok(level)
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
                let expected = ((grid.cells + 1) * (grid.cells + 1)) as usize;
                if grid.heights.len() != expected {
                    return Err(MapError::Heights { name: name.to_owned(), expected, found: grid.heights.len() });
                }
                let spawn = self.spawn.ok_or_else(|| incomplete("a spawn"))?;
                Level {
                    terrain: Terrain::from_heights(&grid),
                    solids: Vec::new(),
                    trees: Vec::new(),
                    targets: Vec::new(),
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
