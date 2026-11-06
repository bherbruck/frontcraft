//! Mods: a small file that changes numbers. `weapons.m4.rounds_per_minute = 900` alone is a whole mod.
//!
//! Every tuning type stays a plain struct with Rust defaults, and the defaults go on being what the game
//! is. A mod is laid over them: the defaults are turned into a tree, the file's values replace what they
//! name, and the tree is turned back into the type. So a mod says only what it changes, a setting that
//! does not exist is an error naming its path, and this crate knows nothing about guns.
//!
//! A `"*"` key means every entry beside it, so `[weapons."*".recoil]` is all guns. Named entries still
//! win over it. Lists are replaced whole.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fmt;
use std::sync::OnceLock;

/// The sections a mod can have, so that a misspelt one is caught whichever program reads the file. A
/// server never looks at `audio`, and a client must not think `wepons` was fine.
pub const SECTIONS: &[&str] = &[
    "weapons", "movement", "health", "suppression", "inventory", "body", "vehicles", "helicopter", "vehicle_ride",
    "world", "mode", "ai", "atmosphere", "audio", "view", "team_looks",
];

/// Everything that can be wrong with a mod, always saying where.
#[derive(Clone, Debug, PartialEq)]
pub enum ModError {
    NotFound { name: String, looked_in: String },
    /// Not valid TOML.
    Syntax { name: String, message: String },
    /// A setting that does not exist.
    Unknown { name: String, path: String, closest: Option<String> },
    /// The right setting with the wrong kind of value.
    Kind { name: String, path: String, wanted: &'static str, found: &'static str },
    /// The value did not fit, such as a fire mode that does not exist.
    Invalid { name: String, path: String, message: String },
}

impl fmt::Display for ModError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotFound { name, looked_in } => write!(f, "mod '{name}': no file {looked_in}"),
            Self::Syntax { name, message } => write!(f, "mod '{name}': {message}"),
            Self::Unknown { name, path, closest: Some(closest) } => {
                write!(f, "mod '{name}': there is no setting {path}. Did you mean {closest}?")
            }
            Self::Unknown { name, path, closest: None } => write!(f, "mod '{name}': there is no setting {path}"),
            Self::Kind { name, path, wanted, found } => write!(f, "mod '{name}': {path} should be {wanted}, not {found}"),
            Self::Invalid { name, path, message } => write!(f, "mod '{name}': {path}: {message}"),
        }
    }
}

impl std::error::Error for ModError {}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "nothing",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "a table",
    }
}

fn from_toml(value: toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => Value::from(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(from_toml).collect()),
        toml::Value::Table(table) => Value::Object(table.into_iter().map(|(k, v)| (k, from_toml(v))).collect()),
    }
}

/// How many single-letter edits apart two names are, for suggesting what someone meant.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = (ca != *cb) as usize;
            let next = (row[j + 1] + 1).min(row[j] + 1).min(diagonal + cost);
            diagonal = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

fn closest<'a>(wanted: &str, known: impl Iterator<Item = &'a String>) -> Option<String> {
    known
        .map(|name| (edits(wanted, name), name))
        .filter(|(distance, _)| *distance <= wanted.len().max(3) / 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name.clone())
}

/// One mod, as read from its file. The empty mod changes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mod {
    pub name: String,
    root: Map<String, Value>,
}

impl Mod {
    pub fn parse(name: &str, text: &str) -> Result<Self, ModError> {
        let table: toml::Table =
            text.parse().map_err(|e: toml::de::Error| ModError::Syntax { name: name.into(), message: e.to_string() })?;
        let Value::Object(root) = from_toml(toml::Value::Table(table)) else { unreachable!("a table comes out a table") };
        for key in root.keys() {
            if !SECTIONS.contains(&key.as_str()) {
                let known: Vec<String> = SECTIONS.iter().map(|s| s.to_string()).collect();
                return Err(ModError::Unknown { name: name.into(), path: key.clone(), closest: closest(key, known.iter()) });
            }
        }
        Ok(Self { name: name.into(), root })
    }

    /// Reads `mods/<name>.toml` from the folder given, as a path relative to where the program runs.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(name: &str, folder: &str) -> Result<Self, ModError> {
        let path = format!("{folder}/{name}.toml");
        let text = std::fs::read_to_string(&path).map_err(|_| ModError::NotFound { name: name.into(), looked_in: path })?;
        Self::parse(name, &text)
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// What this mod has to say at a dotted path such as `mode.aas`.
    fn at(&self, path: &str) -> Option<&Value> {
        let mut parts = path.split('.');
        let mut here = self.root.get(parts.next()?)?;
        for part in parts {
            here = here.as_object()?.get(part)?;
        }
        Some(here)
    }

    /// `default`, with whatever this mod says at `path` laid over it. With nothing to say it comes
    /// straight back untouched, so playing without a mod costs nothing and cannot change a number.
    pub fn over<T: Serialize + DeserializeOwned>(&self, path: &str, default: T) -> Result<T, ModError> {
        let Some(over) = self.at(path) else {
            return Ok(default);
        };
        let name = || self.name.clone();
        let invalid = |at: String, message: String| ModError::Invalid { name: name(), path: at, message };
        let mut tree = serde_json::to_value(&default).map_err(|e| invalid(path.into(), e.to_string()))?;
        lay_over(&mut tree, over, path, &self.name)?;
        serde_path_to_error::deserialize(tree).map_err(|e: serde_path_to_error::Error<serde_json::Error>| {
            let at = e.path().to_string();
            invalid(if at == "." { path.into() } else { format!("{path}.{at}") }, e.inner().to_string())
        })
    }
}

/// Replaces in `tree` whatever `over` names. Every key has to exist already, and a value has to be the
/// same kind as what it replaces, except where the default has nothing at all.
fn lay_over(tree: &mut Value, over: &Value, path: &str, name: &str) -> Result<(), ModError> {
    match (&mut *tree, over) {
        (Value::Object(table), Value::Object(changes)) => {
            // Everything first, so that a named entry beats it whichever comes first in the file.
            if let Some(all) = changes.get("*") {
                for (key, entry) in table.iter_mut() {
                    lay_over(entry, all, &format!("{path}.{key}"), name)?;
                }
            }
            for (key, change) in changes {
                if key == "*" {
                    continue;
                }
                let at = format!("{path}.{key}");
                let Some(entry) = table.get_mut(key) else {
                    return Err(ModError::Unknown { name: name.into(), path: at, closest: closest(key, table.keys()) });
                };
                lay_over(entry, change, &at, name)?;
            }
            Ok(())
        }
        (Value::Null, change) => {
            *tree = change.clone();
            Ok(())
        }
        (old, change) if kind_of(old) == kind_of(change) => {
            *old = change.clone();
            Ok(())
        }
        (old, change) => {
            Err(ModError::Kind { name: name.into(), path: path.into(), wanted: kind_of(old), found: kind_of(change) })
        }
    }
}

static ACTIVE: OnceLock<Mod> = OnceLock::new();
static EMPTY: OnceLock<Mod> = OnceLock::new();

/// Makes this the mod for the whole program. Done once, first thing. Without it there is no mod.
pub fn activate(active: Mod) {
    let _ = ACTIVE.set(active);
}

pub fn active() -> &'static Mod {
    ACTIVE.get().unwrap_or_else(|| EMPTY.get_or_init(Mod::default))
}

/// `default` with the active mod laid over it. A bad mod stops the program here with the message saying
/// what is wrong, because playing with half of someone's mod would be worse.
pub fn tuned<T: Serialize + DeserializeOwned>(path: &str, default: T) -> T {
    active().over(path, default).unwrap_or_else(|error| stop(&error))
}

/// Ends the program with what is wrong with its mod.
pub fn stop(error: &ModError) -> ! {
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("{error}");
        std::process::exit(2);
    }
    #[cfg(target_arch = "wasm32")]
    panic!("{error}");
}

/// Activates the mod named by `PRS_MOD`, from `PRS_MODS_DIR` or a `mods` folder where the program runs.
#[cfg(not(target_arch = "wasm32"))]
pub fn activate_from_env() {
    let Ok(name) = std::env::var("PRS_MOD") else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let folder = std::env::var("PRS_MODS_DIR").unwrap_or_else(|_| "mods".into());
    match Mod::read(&name, &folder) {
        Ok(found) => activate(found),
        Err(error) => stop(&error),
    }
}

/// Says in words what a program is running, for messages to the player.
pub fn describe(name: &str) -> String {
    if name.is_empty() { "no mod".into() } else { format!("mod '{name}'") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    enum Mode {
        Semi,
        Auto,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Recoil {
        kick: f32,
        spread: f32,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Gun {
        rpm: f32,
        magazine: u32,
        mode: Mode,
        modes: Vec<Mode>,
        aim: [f32; 2],
        sidearm: Option<u16>,
        recoil: Recoil,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Table {
        m4: Gun,
        m9: Gun,
    }

    fn gun(rpm: f32) -> Gun {
        Gun { rpm, magazine: 30, mode: Mode::Semi, modes: vec![Mode::Semi], aim: [1.0, 2.0], sidearm: None, recoil: Recoil { kick: 2.0, spread: 0.1 } }
    }

    fn table() -> Table {
        Table { m4: gun(750.0), m9: gun(400.0) }
    }

    fn over(text: &str) -> Result<Table, ModError> {
        Mod::parse("test", text)?.over("weapons", table())
    }

    #[test]
    fn one_line_is_a_whole_mod_and_everything_else_stays() {
        let t = over("weapons.m4.rpm = 900").unwrap();
        assert_eq!(t.m4.rpm, 900.0, "a whole number is fine for a decimal setting");
        assert_eq!(t.m4.recoil, table().m4.recoil);
        assert_eq!(t.m9, table().m9);
    }

    #[test]
    fn nothing_to_say_changes_nothing_down_to_the_bit() {
        let tricky = Table { m4: gun(1.0 / 3.0), m9: gun(0.1 + 0.2) };
        let empty = Mod::parse("test", "").unwrap();
        assert_eq!(empty.over("weapons", tricky.clone()).unwrap(), tricky);
        // And all the way through the tree and back.
        let touched = Mod::parse("test", "weapons.m9.magazine = 30").unwrap();
        let back = touched.over("weapons", tricky.clone()).unwrap();
        assert_eq!(back.m4.rpm.to_bits(), tricky.m4.rpm.to_bits());
        assert_eq!(back.m9.rpm.to_bits(), tricky.m9.rpm.to_bits());
    }

    #[test]
    fn a_setting_that_does_not_exist_says_its_path_and_what_was_meant() {
        let error = over("weapons.m4.recoil.kik = 3").unwrap_err();
        assert_eq!(error, ModError::Unknown { name: "test".into(), path: "weapons.m4.recoil.kik".into(), closest: Some("kick".into()) });
        assert_eq!(error.to_string(), "mod 'test': there is no setting weapons.m4.recoil.kik. Did you mean kick?");
        assert!(matches!(over("weapons.ak.rpm = 3").unwrap_err(), ModError::Unknown { path, .. } if path == "weapons.ak"));
    }

    #[test]
    fn a_misspelt_section_is_caught_whoever_reads_it() {
        let error = Mod::parse("test", "wepons.m4.rpm = 3").unwrap_err();
        assert_eq!(error, ModError::Unknown { name: "test".into(), path: "wepons".into(), closest: Some("weapons".into()) });
    }

    #[test]
    fn the_wrong_kind_of_value_says_what_was_wanted() {
        let error = over("weapons.m4.rpm = \"fast\"").unwrap_err();
        assert_eq!(error.to_string(), "mod 'test': weapons.m4.rpm should be a number, not text");
        let error = over("weapons.m4.recoil = 3").unwrap_err();
        assert_eq!(error.to_string(), "mod 'test': weapons.m4.recoil should be a table, not a number");
    }

    #[test]
    fn a_value_that_does_not_fit_says_where() {
        let error = over("weapons.m4.mode = \"Burst\"").unwrap_err();
        let ModError::Invalid { path, message, .. } = error else { panic!("{error:?}") };
        assert_eq!(path, "weapons.m4.mode");
        assert!(message.contains("Burst"), "{message}");
        let error = over("weapons.m4.magazine = 1.5").unwrap_err();
        assert!(matches!(error, ModError::Invalid { path, .. } if path == "weapons.m4.magazine"));
    }

    #[test]
    fn broken_toml_is_an_error_with_its_place() {
        let error = Mod::parse("test", "weapons.m4.rpm = = 3").unwrap_err();
        assert!(matches!(&error, ModError::Syntax { message, .. } if message.contains("line 1") || message.contains('1')), "{error}");
    }

    #[test]
    fn a_star_means_every_entry_and_a_named_one_still_wins() {
        let t = over("[weapons.\"*\".recoil]\nkick = 0.5\n[weapons.m9.recoil]\nkick = 9.0").unwrap();
        assert_eq!(t.m4.recoil.kick, 0.5);
        assert_eq!(t.m9.recoil.kick, 9.0);
        assert_eq!(t.m4.recoil.spread, 0.1);
        assert!(over("weapons.\"*\".nope = 1").is_err());
    }

    #[test]
    fn lists_are_replaced_whole_and_unset_options_can_be_given() {
        let t = over("[weapons.m4]\nmodes = [\"Auto\", \"Semi\"]\naim = [3, 4]\nsidearm = 2").unwrap();
        assert_eq!(t.m4.modes, vec![Mode::Auto, Mode::Semi]);
        assert_eq!(t.m4.aim, [3.0, 4.0]);
        assert_eq!(t.m4.sidearm, Some(2));
    }

    #[test]
    fn a_section_can_be_a_path_and_other_sections_are_left_alone() {
        let m = Mod::parse("test", "mode.aas.rpm = 5\nhealth = { }").unwrap();
        assert_eq!(m.over("mode.aas", gun(1.0)).unwrap().rpm, 5.0);
        assert_eq!(m.over("mode.tdm", gun(1.0)).unwrap().rpm, 1.0);
    }
}
