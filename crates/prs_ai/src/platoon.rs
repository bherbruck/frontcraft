//! Everyone fighting in one sim. Plain data, and not part of anyone's state: only commands come out of it,
//! so it belongs on whatever is the authority, and a client sees these as soldiers like any others.

use glam::Vec2;
use prs_ballistics::zero_elevation;
use prs_sim::protocol::PlayerCmd;
use prs_sim::weapons::{WeaponId, WeaponTable};
use prs_sim::{Sim, SimEvent, SoldierId};

use crate::fighter::{Battle, Fighter, SquadBoard};
use crate::flow::FlowMap;
use crate::sight::Foliage;
use crate::skill::FightConfig;
use crate::walker::{Dice, WalkArea, Walker};
use crate::WalkerConfig;

/// Elevation by range for each gun, worked out once, because flying a test bullet is not cheap.
#[derive(Clone, Debug, PartialEq)]
pub struct Holdover {
    step: f32,
    /// Radians above the gun's zero, by range, for each row of the table.
    table: Vec<Vec<f32>>,
}

impl Holdover {
    pub fn new(weapons: &WeaponTable) -> Self {
        const STEP: f32 = 50.0;
        const STEPS: usize = 17;
        let table = weapons
            .weapons
            .iter()
            .map(|w| {
                let zero = zero_elevation(&w.ballistics, w.zero_range, w.hold.sight_height);
                (0..STEPS).map(|i| zero_elevation(&w.ballistics, (i as f32 * STEP).max(10.0), w.hold.sight_height) - zero).collect()
            })
            .collect();
        Self { step: STEP, table }
    }

    /// How much higher than the sights this range wants the view.
    pub fn above_zero(&self, weapon: WeaponId, range: f32) -> f32 {
        let row = &self.table[weapon.0 as usize];
        let at = (range / self.step).clamp(0.0, (row.len() - 1) as f32 - 1e-3);
        let (i, t) = (at as usize, at.fract());
        row[i] + (row[i + 1] - row[i]) * t
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Platoon {
    pub cfg: FightConfig,
    pub walk: WalkerConfig,
    pub fighters: Vec<Fighter>,
    pub boards: Vec<SquadBoard>,
    pub holdover: Holdover,
    pub foliage: Foliage,
    pub seed: u32,
    /// The tick each soldier last fired on, for anyone looking their way.
    fired: Vec<u32>,
    feet: Vec<Vec2>,
    cmds: Vec<(SoldierId, PlayerCmd)>,
}

impl Platoon {
    pub fn new(sim: &Sim, cfg: FightConfig, walk: WalkerConfig) -> Self {
        let foliage = Foliage::new(&sim.level, &cfg.sight);
        Self {
            seed: walk.seed,
            cfg,
            walk,
            fighters: Vec::new(),
            boards: Vec::new(),
            holdover: Holdover::new(&sim.weapons),
            foliage,
            fired: Vec::new(),
            feet: Vec::new(),
            cmds: Vec::new(),
        }
    }

    /// One more soldier. Those who `hold` keep firing at where someone was while the rest go round.
    /// `place` is which of its team's squads this is, because squad numbers run on across both teams.
    pub fn enlist(&mut self, sim: &Sim, soldier: SoldierId, squad: u32, place: usize, holds: bool, area: usize) {
        let s = &sim.soldiers[soldier];
        let seed = self.seed.wrapping_add((soldier as u32 + 1).wrapping_mul(0x9E37_79B1));
        let at = Vec2::new(s.body.pos.x, s.body.pos.z);
        let nav = Walker::new(soldier, area, Dice::new(seed), soldier as u32, at, s.yaw);
        self.fighters.push(Fighter::new(soldier, squad, holds, nav, Dice::new(seed ^ 0xF16), s));
        if !self.boards.iter().any(|b| b.squad == squad) {
            let flank_side = if place % 2 == 0 { 1.0 } else { -1.0 };
            self.boards.push(SquadBoard { squad, contact: None, flank_side, center: None, last_throw: 0 });
        }
    }

    /// Where each squad is, as the first of them up and whoever is with them.
    fn find_centers(&mut self, sim: &Sim) {
        let group = self.cfg.squad_group;
        let mut spots = Vec::new();
        for board in &mut self.boards {
            spots.clear();
            let members = self.fighters.iter().filter(|f| f.squad == board.squad).map(|f| &sim.soldiers[f.soldier]);
            spots.extend(members.filter(|s| s.present && s.health.is_up()).map(|s| Vec2::new(s.body.pos.x, s.body.pos.z)));
            // The one with the most of them nearby, so that someone just back at the base does not lead.
            let near = |a: Vec2, b: &&Vec2| b.distance(a) < group;
            let leader = spots.iter().copied().max_by_key(|&a| spots.iter().filter(|b| near(a, b)).count());
            board.center = leader.map(|a| {
                let close = spots.iter().filter(|b| near(a, b));
                close.clone().sum::<Vec2>() / close.count() as f32
            });
        }
    }

    /// What everyone wants to do on the coming tick, given where each is meant to be.
    pub fn think(&mut self, sim: &Sim, areas: &[WalkArea], flow: Option<&FlowMap>) -> &[(SoldierId, PlayerCmd)] {
        self.feet.clear();
        self.feet.extend(sim.soldiers.iter().filter(|s| s.present && s.health.is_up()).map(|s| Vec2::new(s.body.pos.x, s.body.pos.z)));
        self.cmds.clear();
        self.fired.resize(sim.soldiers.len(), 0);
        for event in &sim.events {
            if let SimEvent::ShotFired { soldier, .. } = *event {
                self.fired[soldier] = sim.tick;
            }
        }
        if sim.tick % self.cfg.think_ticks.max(1) == 0 {
            self.find_centers(sim);
        }
        for fighter in &mut self.fighters {
            if !sim.soldiers[fighter.soldier].present {
                continue;
            }
            let way = flow.and_then(|map| map.fields.get(fighter.nav.area).map(|field| (&map.grid, field)));
            let battle = Battle {
                sim,
                cfg: &self.cfg,
                walk: &self.walk,
                holdover: &self.holdover,
                foliage: &self.foliage,
                fired: &self.fired,
                area: &areas[fighter.nav.area],
                feet: &self.feet,
                way,
            };
            fighter.listen(&battle);
            let board = self.boards.iter_mut().find(|b| b.squad == fighter.squad).expect("made on enlisting");
            let cmd = fighter.step(&battle, board);
            self.cmds.push((fighter.soldier, cmd));
        }
        &self.cmds
    }
}
