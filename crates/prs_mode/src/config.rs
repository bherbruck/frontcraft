//! What a mode is, as plain data. Lengths are metres and times are seconds. Change any of it, or build
//! a new [`Mode`] out of it, and nothing else needs to know.

use prs_sim::inventory::Loadout;
use prs_sim::weapons::WeaponTable;

use crate::objective::{AdvanceAndSecure, FightOnly, Objective};

/// How a soldier gets back into the fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Deployment {
    /// They wait on the spawn screen and say where and with what.
    Choose,
    /// They come back where they started with what they started with.
    Automatic,
}

/// One kit on the spawn screen.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Kit {
    pub name: String,
    pub loadout: Loadout,
    /// How many in one squad may carry it.
    pub per_squad: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TicketRules {
    /// What each team starts with. Run out and the round is lost.
    pub start: i32,
    /// Going down or dying costs this once, however long it takes.
    pub per_death: i32,
    /// Handed back when a medic gets someone up again.
    pub revive_refund: i32,
    /// More on top when it was your own side who did it.
    pub teamkill_penalty: i32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SquadRules {
    pub max_members: usize,
    pub max_per_team: usize,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RallyRules {
    /// Tickets a placement costs the team.
    pub ticket_cost: i32,
    /// Seconds before the squad can place another.
    pub cooldown: f32,
    /// How many squadmates have to be close by, and how close.
    pub needs_nearby: usize,
    pub nearby_radius: f32,
    /// No placing one with an enemy this close.
    pub enemy_radius: f32,
    /// No placing one this close to your own main base.
    pub min_from_base: f32,
    /// No placing one inside a capture zone.
    pub clear_of_zones: bool,
    /// How many soldiers can spawn on it before it is used up.
    pub spawns: u32,
    /// Seconds it lasts.
    pub lifetime: f32,
    /// Enemies outnumbering friends this close for this long destroy it.
    pub overrun_radius: f32,
    pub overrun_seconds: f32,
    /// Seconds on top of the usual wait to spawn on it.
    pub extra_wait: f32,
    /// How far around it soldiers come in, how many places to try, and how far to look for ground.
    pub spawn_spread: f32,
    pub spawn_tries: u32,
    pub ground_probe: f32,
    pub spawn_lift: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRules {
    /// Seconds for one soldier to take a neutral point all the way, or to take a held one down to neutral.
    pub capture_seconds: f32,
    /// How much faster for each soldier of advantage after the first, and the most that can add up to.
    pub per_extra_soldier: f32,
    pub max_speedup: f32,
    /// How many of a side have to be in the zone for it to count.
    pub min_soldiers: u32,
    /// How far above and below its middle the zone reaches.
    pub zone_height: f32,
    /// Whether someone counts up where it took another soldier's back to get to. A roof nobody can walk up
    /// to is a good place to shoot from, but one enemy lying up there could otherwise hold a point for
    /// ever from where nobody on foot can get at them.
    pub boosted_soldiers_count: bool,
    /// Whether anyone sitting in a helicopter counts for a zone they are in, even hovering over it.
    pub aircraft_count: bool,
    /// Share of the way per second that a point left alone drifts back to whoever holds it, or
    /// back to nobody if nobody does.
    pub drift_per_second: f32,
    /// How many points each team holds from the start, from its own end of the chain.
    pub start_held: usize,
    /// Tickets a minute the side holding fewer points loses, per point it is behind by.
    pub bleed_per_point_per_minute: f32,
    /// The order of the chain, as indices into the level's points. Without one they are ordered by
    /// how far along the way from one base to the other they lie.
    pub chain: Option<Vec<usize>>,
}

impl Default for CaptureRules {
    fn default() -> Self {
        Self {
            capture_seconds: 45.0,
            per_extra_soldier: 0.25,
            max_speedup: 2.0,
            min_soldiers: 1,
            zone_height: 25.0,
            boosted_soldiers_count: false,
            aircraft_count: false,
            drift_per_second: 0.01,
            start_held: 1,
            bleed_per_point_per_minute: 5.0,
            chain: None,
        }
    }
}

/// Keeping each side out of the other's main base, so nobody can be shot as they spawn. Nobody is made
/// safe: whoever goes in gets a warning and a clock, and dies if they are still there when it runs out.
/// How big a base is comes with the map (`Team::base_radius`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BaseRules {
    /// Seconds from going in to being killed for it.
    pub warning_seconds: f32,
    /// How far above the middle of a base it stops counting, so that aircraft can pass over.
    pub ceiling: f32,
}

/// How you tell a friend from anyone else, on a map where bodies are all low-poly.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IdentifyRules {
    /// Degrees off the middle of your view a friend is marked at, and squadmates.
    pub cone_degrees: f32,
    pub squad_cone_degrees: f32,
    /// How far away a friend can be made out at all.
    pub range: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModeConfig {
    pub name: String,
    pub deployment: Deployment,
    /// Seconds between dying and being able to come back.
    pub respawn_seconds: f32,
    /// Seconds down before a soldier may stop waiting for a medic.
    pub give_up_after: f32,
    pub kits: Vec<Kit>,
    /// No tickets means nobody can lose them, and that a round only ends on `time_limit`.
    pub tickets: Option<TicketRules>,
    pub squads: SquadRules,
    pub rally: Option<RallyRules>,
    /// Seconds a round lasts, after which the side with more tickets wins.
    pub time_limit: Option<f32>,
    /// Seconds on the round end screen before the next round.
    pub restart_seconds: f32,
    /// Bots fill each side up to this many soldiers, and players push them out again. Both sides are kept
    /// the same size, so the side with fewer players gets more bots. None leaves bots to whoever put them there.
    pub bots_per_team: Option<usize>,
    /// The most bots there are in all, whatever `bots_per_team` would make it.
    pub bots_most: Option<usize>,
    /// From this many players in all there are no bots.
    pub no_bots_from_players: Option<usize>,
    /// A bot on its feet only makes way for a player when no player is this close to it, in metres, so
    /// nobody sees one vanish. A bot off the field or dead goes at once.
    pub bot_unseen_distance: f32,
    /// How far ahead one team's players can get of the other's before no more may switch to it.
    pub team_balance_margin: usize,
    /// Below this many players in all, anyone can go to either side. Only players count, never bots.
    pub balance_from_players: usize,
    /// None means no markers at all.
    pub identify: Option<IdentifyRules>,
    /// None means anyone can go anywhere.
    pub base_protection: Option<BaseRules>,
}

/// A mode: its data, and the part that makes it a game rather than just a fight.
#[derive(Debug)]
pub struct Mode {
    pub cfg: ModeConfig,
    pub objective: Box<dyn Objective>,
}

impl Mode {
    pub const NAMES: [&'static str; 3] = ["aas", "tdm", "practice"];

    /// The mode with this short name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        let mut mode = match name {
            "aas" => Self::advance_and_secure(),
            "tdm" => Self::team_deathmatch(),
            "practice" => Self::free_practice(),
            _ => return None,
        };
        mode.cfg = prs_mods::tuned(&format!("mode.{name}"), mode.cfg);
        Some(mode)
    }

    /// Project Reality's main mode: a chain of points to take in order, and tickets to run the other
    /// side out of.
    pub fn advance_and_secure() -> Self {
        Self {
            cfg: ModeConfig {
                name: "Advance and Secure".into(),
                deployment: Deployment::Choose,
                respawn_seconds: 5.0,
                give_up_after: 5.0,
                kits: standard_kits(),
                tickets: Some(TicketRules { start: 200, per_death: 1, revive_refund: 1, teamkill_penalty: 2 }),
                squads: SquadRules { max_members: 6, max_per_team: 6 },
                rally: Some(RallyRules {
                    ticket_cost: 2,
                    cooldown: 90.0,
                    needs_nearby: 1,
                    nearby_radius: 25.0,
                    enemy_radius: 50.0,
                    min_from_base: 100.0,
                    clear_of_zones: true,
                    spawns: 6,
                    lifetime: 300.0,
                    overrun_radius: 12.0,
                    overrun_seconds: 6.0,
                    extra_wait: 5.0,
                    spawn_spread: 4.0,
                    spawn_tries: 8,
                    ground_probe: 2.0,
                    spawn_lift: 0.1,
                }),
                time_limit: Some(2700.0),
                restart_seconds: 20.0,
                bots_per_team: Some(8),
                team_balance_margin: 1,
                balance_from_players: 6,
                bots_most: None,
                no_bots_from_players: None,
                bot_unseen_distance: 150.0,
                identify: Some(standard_identify()),
                base_protection: Some(standard_base_rules()),
            },
            objective: Box::new(AdvanceAndSecure(CaptureRules::default())),
        }
    }

    /// The same harness with nothing to hold: first side to run out of tickets loses.
    pub fn team_deathmatch() -> Self {
        Self {
            cfg: ModeConfig {
                name: "Team Deathmatch".into(),
                deployment: Deployment::Choose,
                respawn_seconds: 3.0,
                give_up_after: 3.0,
                kits: standard_kits(),
                tickets: Some(TicketRules { start: 50, per_death: 1, revive_refund: 1, teamkill_penalty: 1 }),
                squads: SquadRules { max_members: 6, max_per_team: 6 },
                rally: None,
                time_limit: Some(1200.0),
                restart_seconds: 15.0,
                bots_per_team: Some(8),
                team_balance_margin: 1,
                balance_from_players: 6,
                bots_most: None,
                no_bots_from_players: None,
                bot_unseen_distance: 150.0,
                identify: Some(standard_identify()),
                base_protection: Some(standard_base_rules()),
            },
            objective: Box::new(FightOnly),
        }
    }

    /// Just the sim: no sides that matter, nothing to win, and back where you started after a wait.
    pub fn free_practice() -> Self {
        Self {
            cfg: ModeConfig {
                name: "Free Practice".into(),
                deployment: Deployment::Automatic,
                respawn_seconds: 3.0,
                give_up_after: 3.0,
                kits: Vec::new(),
                tickets: None,
                squads: SquadRules { max_members: 6, max_per_team: 6 },
                rally: None,
                time_limit: None,
                restart_seconds: 0.0,
                bots_per_team: None,
                team_balance_margin: 1,
                balance_from_players: 6,
                bots_most: None,
                no_bots_from_players: None,
                bot_unseen_distance: 150.0,
                identify: None,
                base_protection: None,
            },
            objective: Box::new(FightOnly),
        }
    }
}

/// Only medics carry a medkit, so getting someone up again takes one nearby. Limits per squad keep a
/// squad a squad rather than six machine guns.
pub fn standard_kits() -> Vec<Kit> {
    let kit = |name: &str, primary, bandages, medkits, frags, smokes, per_squad| Kit {
        name: name.into(),
        loadout: Loadout { primary, sidearm: Some(WeaponTable::PISTOL), bandages, medkits, frags, smokes },
        per_squad,
    };
    vec![
        kit("Rifleman", WeaponTable::ASSAULT_RIFLE, 2, 0, 2, 2, None),
        kit("Medic", WeaponTable::ASSAULT_RIFLE, 4, 2, 0, 2, Some(2)),
        kit("Machine gunner", WeaponTable::MACHINE_GUN, 2, 0, 1, 0, Some(1)),
        kit("Marksman", WeaponTable::MARKSMAN_RIFLE, 2, 0, 1, 1, Some(1)),
    ]
}

fn standard_base_rules() -> BaseRules {
    BaseRules { warning_seconds: 10.0, ceiling: 60.0 }
}

fn standard_identify() -> IdentifyRules {
    IdentifyRules { cone_degrees: 4.0, squad_cone_degrees: 8.0, range: 300.0 }
}
