//! Bots in a match. They fight, and when there is nobody to fight they go where the mode says their squad
//! should be, so they head for the objective, stand in it and take it. All they hear from the mode is
//! [`Match::order_for`].

use prs_mode::Match;
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::weapons::WeaponTable;
use prs_sim::{Sim, SoldierId};

use glam::Vec2;

use crate::convoy::{Convoy, ConvoyConfig};
use crate::flow::{FlowGrid, FlowMap, FlowRules};
use crate::platoon::Platoon;
use crate::skill::FightConfig;
use crate::walker::WalkArea;
use crate::{WalkerConfig, doors_near};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TroopsConfig {
    pub walk: WalkerConfig,
    pub fight: FightConfig,
    /// Ticks between asking the mode where to go. Orders do not change much faster than points do.
    pub order_ticks: u32,
    /// Where to go with no orders, as a share of the map from its middle.
    pub no_orders_radius: f32,
    /// Cell size of the way across the map. Has to be finer than the gaps in the walls it has to find.
    pub flow_cell: f32,
    pub flow_max_slope: f32,
    /// Room to leave around what is parked at the start, which a soldier gets pushed off if they walk into it.
    pub flow_vehicle_room: f32,
    pub flow_helicopter_room: f32,
    /// How far outside the other side's main base a bot turns back, in metres.
    pub base_margin: f32,
    pub convoy: ConvoyConfig,
}

impl Default for TroopsConfig {
    fn default() -> Self {
        Self {
            // On the way somewhere, not out for a walk.
            walk: WalkerConfig { jog_chance: 0.75, stop_chance: 0.06, crouch_chance: 0.05, mood_time: Vec2::new(5.0, 20.0), stop_time: Vec2::new(0.5, 2.5), ..Default::default() },
            fight: FightConfig::default(),
            order_ticks: 60,
            no_orders_radius: 0.08,
            flow_cell: 2.5,
            flow_max_slope: 1.0,
            flow_vehicle_room: 5.0,
            flow_helicopter_room: 10.0,
            base_margin: 15.0,
            convoy: ConvoyConfig::default(),
        }
    }
}

/// Plain data, like [`Platoon`], and like it not part of anyone's state: only their commands count.
#[derive(Clone, Debug, PartialEq)]
pub struct Troops {
    pub cfg: TroopsConfig,
    pub platoon: Platoon,
    pub areas: Vec<WalkArea>,
    /// The way to every capture point. Worked out once, and read for nothing after that.
    pub flow: FlowMap,
    /// The way to every capture point for a truck.
    pub road: FlowMap,
    /// Squads riding to where they are going.
    pub convoy: Convoy,
    cmds: Vec<(SoldierId, PlayerCmd)>,
}

impl Troops {
    /// An area for each capture point, in the level's order, and one last for having no orders.
    pub fn new(sim: &Sim, cfg: TroopsConfig) -> Self {
        let mut areas: Vec<WalkArea> = sim
            .level
            .capture_points
            .iter()
            .map(|p| {
                let center = Vec2::new(p.center.x, p.center.z);
                WalkArea { center, radius: p.radius, doors: doors_near(sim, center, p.radius) }
            })
            .collect();
        let radius = sim.level.terrain.size * cfg.no_orders_radius;
        areas.push(WalkArea { center: Vec2::ZERO, radius, doors: Vec::new() });
        let movement = &sim.move_cfg;
        let rules = FlowRules {
            cell: cfg.flow_cell,
            margin: movement.radius + 0.1,
            step: movement.step_height,
            headroom: movement.stand_height,
            max_rise: cfg.flow_cell * cfg.flow_max_slope,
            max_dip: f32::INFINITY,
        };
        let mut grid = FlowGrid::new(&sim.level, rules);
        for vehicle in &sim.vehicles {
            grid.mark_round(Vec2::new(vehicle.pos.x, vehicle.pos.z), cfg.flow_vehicle_room);
        }
        for helicopter in &sim.helicopters {
            grid.mark_round(Vec2::new(helicopter.home.x, helicopter.home.z), cfg.flow_helicopter_room);
        }
        // One for each area, so that having no orders still has a way to the middle of the map through the gates.
        let fields = areas.iter().map(|area| grid.field_to(area.center, area.radius)).collect();
        // The same again for what a truck can get through, which leaves out gaps it would not fit and banks it would not climb.
        let road = FlowGrid::new(
            &sim.level,
            FlowRules { margin: cfg.convoy.drive_margin, max_rise: cfg.flow_cell * cfg.convoy.drive_slope, max_dip: cfg.convoy.drive_dip, ..rules },
        );
        let road_fields = sim.level.capture_points.iter().map(|p| road.field_to(Vec2::new(p.center.x, p.center.z), p.radius)).collect();
        let platoon = Platoon::new(sim, cfg.fight.clone(), cfg.walk.clone());
        let convoy = Convoy::new(cfg.convoy.clone());
        Self { cfg, platoon, areas, flow: FlowMap { grid, fields }, road: FlowMap { grid: road, fields: road_fields }, convoy, cmds: Vec::new() }
    }

    /// What every bot wants to do on the coming tick.
    pub fn think(&mut self, sim: &Sim, game: &Match) -> &[(SoldierId, PlayerCmd)] {
        self.muster(sim, game);
        if sim.tick % self.cfg.order_ticks.max(1) == 0 {
            self.follow_orders(sim, game);
        }
        self.cmds.clear();
        self.cmds.extend_from_slice(self.platoon.think(sim, &self.areas, Some(&self.flow)));
        if sim.tick % self.convoy.cfg.check_ticks.max(1) == 0 {
            self.convoy.find_rides(sim, game, &self.platoon.fighters, &self.flow, &self.road);
        }
        self.convoy.steer(sim, &mut self.platoon.fighters, &self.road, &mut self.cmds);
        self.keep_out_of_bases(sim, game);
        &self.cmds
    }

    /// Whatever else a bot had in mind, near the other side's main base it turns round and leaves, in good
    /// time. Bots die by the same rule as anyone, so this is them obeying it and not being let off.
    fn keep_out_of_bases(&mut self, sim: &Sim, game: &Match) {
        for (id, cmd) in &mut self.cmds {
            if sim.soldiers[*id].seat.is_some() {
                continue;
            }
            let Some(player) = game.state.player(*id) else { continue };
            let pos = sim.soldiers[*id].body.pos;
            let Some(base) = game.enemy_base_at(sim, pos, player.team, self.cfg.base_margin) else { continue };
            let at = Vec2::new(pos.x, pos.z);
            // Inside there may be walls, so by the way out that goes on to wherever it is meant to be.
            let way_out = game.enemy_base_at(sim, pos, player.team, 0.0).and_then(|_| {
                let field = self.flow.fields.get(game.order_for(sim, *id)?.point)?;
                self.flow.grid.toward(field, at)
            });
            let heading = way_out.map_or(at - Vec2::new(base.x, base.z), |next| next - at);
            cmd.yaw = (-heading.x).atan2(-heading.y);
            cmd.pitch = 0.0;
            cmd.wish_move = Vec2::Y;
            cmd.buttons = Buttons(Buttons::SPRINT.0);
        }
    }

    /// One fighter for each bot in the match, no more and no fewer.
    fn muster(&mut self, sim: &Sim, game: &Match) {
        self.platoon.fighters.retain(|f| game.state.player(f.soldier).is_some_and(|p| p.bot));
        for player in game.state.players.iter().filter(|p| p.bot) {
            if self.platoon.fighters.iter().any(|f| f.soldier == player.soldier) {
                continue;
            }
            let area = self.areas.len() - 1;
            // The squad's own board, kept apart by team because a squad number means nothing across them.
            let squad = player.squad.map_or(u32::MAX - u32::from(player.team), |s| s);
            let gunner = sim.soldiers[player.soldier].loadout.primary == WeaponTable::MACHINE_GUN;
            let holds = gunner || player.soldier % 2 == 0;
            let place = game.state.squads.iter().filter(|s| s.team == player.team).position(|s| s.id == squad).unwrap_or(0);
            self.platoon.enlist(sim, player.soldier, squad, place, holds, area);
        }
    }

    fn follow_orders(&mut self, sim: &Sim, game: &Match) {
        let no_orders = self.areas.len() - 1;
        for fighter in &mut self.platoon.fighters {
            let area = game.order_for(sim, fighter.soldier).map_or(no_orders, |order| order.point);
            if fighter.nav.area != area {
                fighter.nav.area = area;
                fighter.nav.goal_left = 0.0;
                fighter.nav.route.clear();
            }
        }
    }
}
