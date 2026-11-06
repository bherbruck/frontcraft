//! Squads with a long way to go taking a truck. They walk to its doors, get in by the same button anyone
//! does, one of them drives along the same way across the map they would have walked, and near the
//! objective they get out and carry on as before. All of it comes out as commands: nothing here touches
//! the sim. Whoever has a truck is still a fighter, so a passenger shoots, and the gun goes where it goes.

use glam::{Vec2, Vec3};
use prs_mode::Match;
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::seats::{SeatRole, SeatedAt};
use prs_sim::vehicles::VehicleState;
use prs_sim::{Sim, SoldierId};

use crate::fighter::{Fighter, Plan};
use crate::flow::FlowMap;
use crate::walker::{wrap, yaw_of};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConvoyConfig {
    /// Ticks between squads looking for something to ride in.
    pub check_ticks: u32,
    /// A squad this many metres of walking from where it is going looks for a ride.
    pub ride_from: f32,
    /// And takes one this far from where it stands, in metres.
    pub truck_reach: f32,
    /// Not worth it for fewer than this many.
    pub min_riders: usize,
    /// Seconds for everyone to get to the doors and in, after which it goes without them.
    pub load_time: f32,
    pub cruise_speed: f32,
    /// Pointing this far off where the way goes, in radians, slow right down to `turn_speed` to make the turn.
    pub turn_off: f32,
    pub turn_speed: f32,
    /// This far over the speed it should be doing, it brakes rather than just easing off.
    pub brake_over: f32,
    /// How hard the wheel is put over for how far off the way it is pointing.
    pub steer_gain: f32,
    /// How fast someone riding brings their look round to the way their seat faces, radians a second.
    pub look_rate: f32,
    /// Slows to this inside `slow_within` metres of a spot on the way close to the end.
    pub arrive_speed: f32,
    pub slow_within: f32,
    /// Everyone out this many metres along the way from where they are going.
    pub dismount_at: f32,
    /// Everyone out if someone is seen this close to the driver.
    pub dismount_contact: f32,
    /// Slower than this and asking to go counts as being stuck, after this many seconds.
    pub stuck_speed: f32,
    pub stuck_time: f32,
    /// Going this few metres in this many seconds is being stuck too, however busily the wheels are turning.
    pub progress_metres: f32,
    pub progress_time: f32,
    /// How long to back off before trying again, and how many tries before getting out and walking.
    pub back_time: f32,
    pub stuck_tries: u32,
    /// Seconds a whole drive may take before they get out and walk, whatever else is going on.
    pub drive_time: f32,
    /// Seconds to get out before trying the seat next door or moving the truck.
    pub exit_time: f32,
    /// Ticks between pressing a button that needs letting go of.
    pub press_ticks: u32,
    /// Having been on a truck, or failed to get on, seconds before looking for another.
    pub rest: f32,
    /// Only worth going this many times as far by road as on foot.
    pub longest_road: f32,
    /// How far past the end of a truck to walk to get round it.
    pub round_room: f32,
    /// What counts as in the way of a truck on the way across the map: how much room either side of the
    /// middle of it, how much the ground may rise per metre, and how far below its surroundings it may lie,
    /// because ditches do not get driven through on purpose.
    pub drive_margin: f32,
    pub drive_slope: f32,
    pub drive_dip: f32,
}

impl Default for ConvoyConfig {
    fn default() -> Self {
        Self {
            check_ticks: 120,
            ride_from: 150.0,
            truck_reach: 45.0,
            min_riders: 2,
            load_time: 20.0,
            cruise_speed: 13.0,
            turn_off: 0.5,
            turn_speed: 6.0,
            brake_over: 1.5,
            steer_gain: 2.0,
            look_rate: 2.0,
            arrive_speed: 6.0,
            slow_within: 30.0,
            dismount_at: 45.0,
            dismount_contact: 90.0,
            stuck_speed: 0.6,
            stuck_time: 2.0,
            progress_metres: 3.0,
            progress_time: 6.0,
            back_time: 1.5,
            stuck_tries: 3,
            drive_time: 150.0,
            exit_time: 5.0,
            press_ticks: 20,
            rest: 45.0,
            longest_road: 2.5,
            round_room: 1.0,
            drive_margin: 1.15,
            drive_slope: 0.4,
            drive_dip: 0.3,
        }
    }
}

/// How far a trip has got.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leg {
    Loading { left: f32 },
    Driving { left: f32, stuck_for: f32, backing: f32, tries: u32, mark: Vec2, marked: f32 },
    Unloading { left: f32 },
}

/// One of a truck's riders, and the door they are for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rider {
    pub soldier: SoldierId,
    pub entry: u8,
}

/// A squad and a truck.
#[derive(Clone, Debug, PartialEq)]
pub struct Trip {
    pub vehicle: usize,
    pub leg: Leg,
    pub riders: Vec<Rider>,
}

/// Plain data, like the rest of the bots' thinking, and nobody's state but theirs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Convoy {
    pub cfg: ConvoyConfig,
    pub trips: Vec<Trip>,
    /// Who cannot look for a ride until when, by tick.
    pub resting: Vec<(SoldierId, u32)>,
    /// What has happened so far, for tests and for tuning.
    pub started: u32,
    pub arrived: u32,
    pub gave_up: u32,
}

fn flat(at: Vec3) -> Vec2 {
    Vec2::new(at.x, at.z)
}

impl Convoy {
    pub fn new(cfg: ConvoyConfig) -> Self {
        Self { cfg, ..Self::default() }
    }

    /// Whether anyone could take this one away now.
    fn free(&self, sim: &Sim, v: usize) -> bool {
        let vehicle = &sim.vehicles[v];
        vehicle.occupants.is_empty()
            && !vehicle.damage.wrecked
            && vehicle.damage.burning.is_none()
            && vehicle.damage.engine > 0.0
            && vehicle.vel.length() < sim.vehicle_cfgs[vehicle.kind].ride.board_speed
            && (vehicle.rot * Vec3::Y).y > 0.7
            && vehicle.wheels.iter().all(|wheel| wheel.tyre > 0.0)
            && !self.trips.iter().any(|trip| trip.vehicle == v)
    }

    fn busy(&self, sim: &Sim, soldier: SoldierId) -> bool {
        sim.soldiers[soldier].seat.is_some() || self.trips.iter().any(|trip| trip.riders.iter().any(|rider| rider.soldier == soldier)) || self.resting.iter().any(|&(who, _)| who == soldier)
    }

    /// Squads with a long way to go look round for a truck.
    pub(crate) fn find_rides(&mut self, sim: &Sim, game: &Match, fighters: &[Fighter], walk: &FlowMap, flow: &FlowMap) {
        self.resting.retain(|&(_, until)| until > sim.tick);
        let mut squads: Vec<(u32, u8)> = Vec::new();
        for fighter in fighters {
            let Some(player) = game.state.player(fighter.soldier) else { continue };
            if !squads.contains(&(fighter.squad, player.team)) {
                squads.push((fighter.squad, player.team));
            }
        }
        for (squad, team) in squads {
            let mut members: Vec<&Fighter> = fighters
                .iter()
                .filter(|f| f.squad == squad && game.state.player(f.soldier).is_some_and(|p| p.team == team))
                .filter(|f| matches!(f.plan, Plan::Move) && sim.soldiers[f.soldier].present && sim.soldiers[f.soldier].health.is_up() && !self.busy(sim, f.soldier))
                .collect();
            if members.len() < self.cfg.min_riders {
                continue;
            }
            let lead = flat(sim.soldiers[members[0].soldier].body.pos);
            let Some(field) = walk.fields.get(members[0].nav.area) else { continue };
            let Some(walking) = walk.grid.metres_to(field, lead).filter(|far| *far >= self.cfg.ride_from) else {
                continue;
            };
            let Some(road) = flow.fields.get(members[0].nav.area) else { continue };
            let nearest = (0..sim.vehicles.len())
                // A road that goes the long way round is no use if it is longer than walking is slower.
                .filter(|&v| self.free(sim, v) && flow.grid.metres_to(road, flat(sim.vehicles[v].pos)).is_some_and(|far| far < walking * self.cfg.longest_road))
                .map(|v| (v, flat(sim.vehicles[v].pos).distance(lead)))
                .filter(|&(_, far)| far < self.cfg.truck_reach)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((v, _)) = nearest else { continue };
            // Not in the other side's base, which nobody stays in long enough to drive out of.
            if game.enemy_base_at(sim, sim.soldiers[members[0].soldier].body.pos, team, 0.0).is_some() || game.enemy_base_at(sim, sim.vehicles[v].pos, team, 0.0).is_some() {
                continue;
            }
            let truck = flat(sim.vehicles[v].pos);
            members.sort_by(|a, b| flat(sim.soldiers[a.soldier].body.pos).distance(truck).total_cmp(&flat(sim.soldiers[b.soldier].body.pos).distance(truck)));
            let cfg = &sim.vehicle_cfgs[sim.vehicles[v].kind];
            // Doors from the ground only: the driver's first, for whoever is closest, then the rest.
            let mut doors: Vec<u8> = (0..cfg.entries.len() as u8).filter(|&e| cfg.entries[e as usize].on_ground).collect();
            doors.sort_by_key(|&e| cfg.entries[e as usize].seats.first().is_none_or(|&seat| cfg.seats[seat as usize].role != SeatRole::Driver));
            if !doors.first().is_some_and(|&e| cfg.entries[e as usize].seats.first().is_some_and(|&seat| cfg.seats[seat as usize].role == SeatRole::Driver)) {
                continue;
            }
            let riders: Vec<Rider> = members.iter().zip(&doors).map(|(fighter, &entry)| Rider { soldier: fighter.soldier, entry }).collect();
            if riders.len() >= self.cfg.min_riders {
                self.trips.push(Trip { vehicle: v, leg: Leg::Loading { left: self.cfg.load_time }, riders });
                self.started += 1;
            }
        }
    }

    fn rest(&mut self, sim: &Sim, soldier: SoldierId) {
        self.resting.push((soldier, sim.tick + (self.cfg.rest / TICK_DT) as u32));
    }

    /// Every trip for a tick: what anyone on one does instead of what they would have done.
    pub(crate) fn steer(&mut self, sim: &Sim, fighters: &mut [Fighter], flow: &FlowMap, cmds: &mut [(SoldierId, PlayerCmd)]) {
        let mut trips = std::mem::take(&mut self.trips);
        trips.retain_mut(|trip| self.step_trip(sim, fighters, flow, cmds, trip));
        self.trips = trips;
    }

    /// Says whether there is any more to this trip.
    fn step_trip(&mut self, sim: &Sim, fighters: &mut [Fighter], flow: &FlowMap, cmds: &mut [(SoldierId, PlayerCmd)], trip: &mut Trip) -> bool {
        let v = trip.vehicle;
        let here = |soldier: SoldierId| sim.soldiers[soldier].seat.filter(|at| at.carrier == v as u32 && prs_sim::heli_of(at.carrier).is_none());
        // Anyone gone down, gone off to fight on foot or put somewhere else is no longer on this trip.
        let mut left_behind = Vec::new();
        trip.riders.retain(|rider| {
            let soldier = &sim.soldiers[rider.soldier];
            let fighting = fighters.iter().find(|f| f.soldier == rider.soldier).is_none_or(|f| !matches!(f.plan, Plan::Move));
            let on_foot_fighting = soldier.seat.is_none() && fighting && matches!(trip.leg, Leg::Loading { .. });
            let stay = soldier.present && soldier.health.is_up() && !on_foot_fighting && (soldier.seat.is_none() || here(rider.soldier).is_some());
            if !stay {
                left_behind.push(rider.soldier);
            }
            stay
        });
        for soldier in left_behind {
            self.rest(sim, soldier);
        }
        if trip.riders.is_empty() {
            return false;
        }
        let cfg = sim.vehicle_cfgs[sim.vehicles[v].kind].clone();
        let driver = trip.riders.iter().copied().find(|rider| here(rider.soldier).is_some_and(|at| cfg.seats[at.seat as usize].role == SeatRole::Driver));
        let driver_for = trip.riders.first().copied();
        let dt = TICK_DT;
        trip.leg = match trip.leg {
            Leg::Loading { left } => {
                let left = left - dt;
                let all_in = trip.riders.iter().all(|rider| here(rider.soldier).is_some() && sim.soldiers[rider.soldier].transit.is_none());
                let driver_in = driver.is_some_and(|rider| sim.soldiers[rider.soldier].transit.is_none());
                // Whoever was to drive has been lost, or time is up with nobody at the wheel: that is the end of it.
                let driver_gone = driver.is_none() && driver_for.is_none_or(|rider| cfg.entries[rider.entry as usize].seats.first().is_none_or(|&seat| cfg.seats[seat as usize].role != SeatRole::Driver));
                if driver_gone || (left <= 0.0 && !driver_in) {
                    self.gave_up += 1;
                    Leg::Unloading { left: cfg.ride.exit_speed.max(self.cfg.exit_time) }
                } else if driver_in && (all_in || left <= 0.0) {
                    // Those who did not make it are on foot, and stay that way.
                    let late: Vec<SoldierId> = trip.riders.iter().filter(|rider| here(rider.soldier).is_none()).map(|rider| rider.soldier).collect();
                    trip.riders.retain(|rider| here(rider.soldier).is_some());
                    for soldier in late {
                        self.rest(sim, soldier);
                    }
                    Leg::Driving { left: self.cfg.drive_time, stuck_for: 0.0, backing: 0.0, tries: 0, mark: flat(sim.vehicles[v].pos), marked: 0.0 }
                } else {
                    Leg::Loading { left }
                }
            }
            Leg::Driving { left, stuck_for, backing, tries, mark, marked } => {
                let vehicle = &sim.vehicles[v];
                let arrived = driver.is_some_and(|rider| self.arrived_at(fighters, flow, rider.soldier, vehicle));
                if driver.is_none() {
                    self.gave_up += 1;
                    Leg::Unloading { left: self.cfg.exit_time }
                } else if arrived {
                    self.arrived += 1;
                    Leg::Unloading { left: self.cfg.exit_time }
                } else if left <= 0.0 || tries > self.cfg.stuck_tries || vehicle.damage.engine <= 0.0 || vehicle.damage.wrecked || vehicle.damage.burning.is_some() {
                    self.gave_up += 1;
                    Leg::Unloading { left: self.cfg.exit_time }
                } else if backing > 0.0 {
                    Leg::Driving { left: left - dt, stuck_for: 0.0, backing: backing - dt, tries, mark: flat(vehicle.pos), marked: 0.0 }
                } else {
                    let stuck = if vehicle.vel.length() < self.cfg.stuck_speed { stuck_for + dt } else { 0.0 };
                    let moved = flat(vehicle.pos).distance(mark) >= self.cfg.progress_metres;
                    let (mark, marked) = if moved { (flat(vehicle.pos), 0.0) } else { (mark, marked + dt) };
                    if stuck >= self.cfg.stuck_time || marked >= self.cfg.progress_time {
                        Leg::Driving { left: left - dt, stuck_for: 0.0, backing: self.cfg.back_time, tries: tries + 1, mark, marked: 0.0 }
                    } else {
                        Leg::Driving { left: left - dt, stuck_for: stuck, backing: 0.0, tries, mark, marked }
                    }
                }
            }
            // Nobody has got out yet with the doors tried for this long: something is against them, so move it.
            Leg::Unloading { left } if left <= 0.0 && driver.is_some() => Leg::Driving { left: self.cfg.back_time * 2.0, stuck_for: 0.0, backing: self.cfg.back_time, tries: 0, mark: flat(sim.vehicles[v].pos), marked: 0.0 },
            Leg::Unloading { left } => Leg::Unloading { left: left - dt },
        };
        // Over on its side or its roof there is nowhere to drive it, and nobody sits there waiting.
        if (sim.vehicles[v].rot * glam::Vec3::Y).y < cfg.damage.righted_above && !matches!(trip.leg, Leg::Unloading { .. }) {
            self.gave_up += 1;
            trip.leg = Leg::Unloading { left: self.cfg.exit_time };
        }
        for rider in trip.riders.iter().copied() {
            let Some((_, cmd)) = cmds.iter_mut().find(|(who, _)| *who == rider.soldier) else { continue };
            match here(rider.soldier) {
                None => self.walk_to_door(sim, &cfg, sim.vehicles.get(v).expect("on the trip"), rider, cmd),
                Some(at) => self.aboard(sim, flow, fighters, trip.leg, v, rider, at, cmd),
            }
        }
        if matches!(trip.leg, Leg::Unloading { .. }) {
            // Out and on foot, they go back to walking from where they got down.
            let (gone, staying): (Vec<Rider>, Vec<Rider>) = trip.riders.iter().copied().partition(|rider| sim.soldiers[rider.soldier].seat.is_none());
            for rider in gone {
                self.rest(sim, rider.soldier);
                if let Some(fighter) = fighters.iter_mut().find(|f| f.soldier == rider.soldier) {
                    fighter.nav.route.clear();
                    fighter.nav.goal_left = 0.0;
                }
            }
            trip.riders = staying;
        }
        !trip.riders.is_empty()
    }

    /// Near enough to where they are going, or close to a fight.
    fn arrived_at(&self, fighters: &[Fighter], flow: &FlowMap, driver: SoldierId, vehicle: &VehicleState) -> bool {
        let Some(fighter) = fighters.iter().find(|f| f.soldier == driver) else { return true };
        let near_fight = fighter.contact.is_some_and(|contact| contact.visible && contact.at.distance(vehicle.pos) < self.cfg.dismount_contact);
        let there = flow.fields.get(fighter.nav.area).and_then(|field| flow.grid.metres_to(field, flat(vehicle.pos))).is_none_or(|far| far < self.cfg.dismount_at);
        near_fight || there
    }

    /// On foot, going to a door: round the end of the truck if it is on the other side, and in.
    fn walk_to_door(&self, sim: &Sim, cfg: &prs_sim::vehicles::VehicleConfig, vehicle: &VehicleState, rider: Rider, cmd: &mut PlayerCmd) {
        let soldier = &sim.soldiers[rider.soldier];
        let frame = vehicle.frame();
        let entry = &cfg.entries[rider.entry as usize];
        let (stand, hand) = (frame.point(entry.stand), frame.point(entry.hand));
        let feet = soldier.body.pos;
        let local = frame.to_local(feet);
        // On the far side of it they go along it past the end first, however close they are to the door.
        let mut goal = flat(stand);
        let mut rounding = false;
        if let Some(hull) = cfg.hull.first() {
            let round = cfg.hull.iter().map(|h| h.center.z.abs() + h.half.z).fold(hull.half.z, f32::max) + self.cfg.round_room;
            if local.x * entry.stand.x < 0.0 && local.z.abs() < round {
                goal = flat(frame.point(Vec3::new(local.x, 0.0, (round + self.cfg.round_room) * local.z.signum())));
                rounding = true;
            }
        }
        let to = goal - flat(feet);
        let far = to.length();
        cmd.buttons = Buttons::default();
        if rounding || far > cfg.ride.board_reach * 0.7 {
            cmd.yaw = yaw_of(to);
            cmd.pitch = 0.0;
            cmd.wish_move = Vec2::Y;
            cmd.buttons.set(Buttons::SPRINT, far > 8.0);
        } else {
            let look = hand - soldier.eye();
            cmd.yaw = yaw_of(flat(look));
            cmd.pitch = (look.y / look.length().max(1e-3)).asin();
            cmd.wish_move = Vec2::ZERO;
            cmd.buttons.set(Buttons::USE, sim.tick % self.cfg.press_ticks.max(1) == 0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn aboard(&self, sim: &Sim, flow: &FlowMap, fighters: &[Fighter], leg: Leg, v: usize, rider: Rider, at: SeatedAt, cmd: &mut PlayerCmd) {
        let vehicle = &sim.vehicles[v];
        let cfg = &sim.vehicle_cfgs[vehicle.kind];
        let driving = cfg.seats[at.seat as usize].role == SeatRole::Driver;
        // Whatever they would have done with their legs and their kit has no place in a seat.
        cmd.wish_move = Vec2::ZERO;
        let keep = Buttons::FIRE.0 | Buttons::ADS.0 | Buttons::RELOAD.0 | Buttons::BANDAGE.0;
        cmd.buttons = Buttons(cmd.buttons.0 & keep);
        if driving {
            cmd.yaw = vehicle.frame().yaw();
            cmd.pitch = 0.0;
            // A driver can still see to their own bleeding.
            cmd.buttons = Buttons(cmd.buttons.0 & Buttons::BANDAGE.0);
        } else if !cmd.buttons.contains(Buttons::FIRE) && !cmd.buttons.contains(Buttons::ADS) {
            // With nothing to shoot at they look out the way the seat faces, and so turn with the truck.
            let soldier = &sim.soldiers[rider.soldier];
            let facing = vehicle.frame().yaw() + cfg.seats[at.seat as usize].facing;
            let most = self.cfg.look_rate * TICK_DT;
            cmd.yaw = soldier.yaw + wrap(facing - soldier.yaw).clamp(-most, most);
            cmd.pitch = soldier.pitch + (-soldier.pitch).clamp(-most, most);
        }
        match leg {
            Leg::Loading { .. } => {
                if driving {
                    cmd.buttons.set(Buttons::JUMP, true);
                }
            }
            Leg::Driving { backing, .. } => {
                if driving {
                    let Some(field) = fighters.iter().find(|f| f.soldier == rider.soldier).and_then(|f| flow.fields.get(f.nav.area)) else { return };
                    let at = flat(vehicle.pos);
                    let next = flow.grid.toward(field, at).unwrap_or(at);
                    let off = wrap(yaw_of(next - at) - vehicle.frame().yaw());
                    let steer = (-off * self.cfg.steer_gain).clamp(-1.0, 1.0);
                    let speed = vehicle.vel.length();
                    let far = flow.grid.metres_to(field, at).unwrap_or(0.0) - self.cfg.dismount_at;
                    let limit = if far < self.cfg.slow_within { self.cfg.arrive_speed } else { self.cfg.cruise_speed };
                    // The further off the way it is pointing, the slower it should be doing that.
                    let turning = (1.0 - off.abs() / self.cfg.turn_off).max(0.0);
                    let limit = limit.min(self.cfg.turn_speed + (self.cfg.cruise_speed - self.cfg.turn_speed) * turning);
                    let throttle = if speed > limit + self.cfg.brake_over {
                        -0.5
                    } else if speed > limit {
                        0.0
                    } else {
                        1.0
                    };
                    cmd.wish_move = if backing > 0.0 { Vec2::new(-steer, -1.0) } else { Vec2::new(steer, throttle) };
                }
            }
            Leg::Unloading { .. } => {
                let press = sim.tick % self.cfg.press_ticks.max(1) == 0;
                if driving && vehicle.vel.length() > cfg.ride.exit_speed * 0.5 {
                    // The foot brake to stop it, and the handbrake so that it does not then go the other way.
                    cmd.wish_move = Vec2::new(0.0, -vehicle.forward_speed().signum());
                    cmd.buttons.set(Buttons::JUMP, true);
                } else if sim.soldiers[rider.soldier].transit.is_none() && vehicle.vel.length() <= cfg.ride.exit_speed {
                    // Their own door, and every other try the seat next to it for the door on that side.
                    let turn = sim.tick / self.cfg.press_ticks.max(1);
                    if turn % 2 == 0 {
                        cmd.buttons.set(Buttons::USE, press);
                    } else if let Some(key) = other_seat_key(sim, at) {
                        cmd.buttons.set(key, press);
                    }
                }
            }
        }
    }
}

/// The number key for a free seat connected to this one, if there is one.
fn other_seat_key(sim: &Sim, at: SeatedAt) -> Option<Buttons> {
    let keys = [Buttons::SLOT_1, Buttons::SLOT_2, Buttons::SLOT_3, Buttons::SLOT_4];
    keys.iter().enumerate().find(|&(seat, _)| sim.seat_connects(at, seat)).map(|(_, key)| *key)
}
