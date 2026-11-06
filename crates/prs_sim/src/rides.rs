//! Soldiers and what carries them. A seated soldier keeps their place in the world: the seat puts their
//! eye where it says, and `gun_pose` and everything downstream carry on from that eye as ever, so a bullet
//! still leaves the real muzzle. All a seat changes is what the soldier's own commands mean.

use crate::copters::{HELI_CARRIERS, heli_of};
use crate::hits::BODY_MIDDLE;
use crate::*;
use prs_body::SitPose;
use prs_collision::Capsule;
use prs_seats::{Entry, Frame, Occupants, Placed, SeatConfig, SeatLink, SeatRole, SeatedAt, Standing, Transit, TransitKind, can_fire, limit_look, link_between};
use prs_vehicles::{DriveInput, RideConfig, VehicleEvent, VehicleKind, VehicleState, VehicleZone};

/// Something that happened with a seat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeatEvent {
    /// Has started through a door or hatch towards this seat.
    Entering { carrier: u32, seat: u8 },
    Entered { carrier: u32, seat: u8 },
    /// Has started moving across to this seat.
    Swapped { carrier: u32, seat: u8 },
    /// Has started out through a door or hatch.
    Leaving { carrier: u32 },
    Left { carrier: u32 },
    /// Wanted out, but the way out is blocked.
    ExitBlocked { carrier: u32 },
}

/// Stood on a vehicle's hull, and where on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnDeck {
    pub vehicle: u32,
    pub local: Vec3,
}

/// What F would do right now on foot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UseTarget {
    /// A vehicle on its side or its roof, to get your hands to.
    Lift { vehicle: usize },
    /// One end or a corner of a vehicle on its wheels, to lean into.
    Push { vehicle: usize, spot: u8 },
    /// Go through this door to this seat.
    Door { carrier: u32, entry: u8, seat: u8 },
    /// Go through the kit at this index.
    Kit(usize),
}

/// What one tick of a soldier needs to know about the seat they are in.
#[derive(Clone, Copy)]
pub(crate) struct Ride {
    pub frame: Frame,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub airborne: bool,
    pub seat: SeatConfig,
    pub cfg: RideConfig,
    /// Where the body is in the carrier's frame: in the seat, or on the way.
    pub placed: Placed,
    pub transit: Option<Transit>,
}

impl Ride {
    /// The look nearest to what was asked that the seat allows.
    pub fn look(&self, yaw: f32, pitch: f32) -> (f32, f32) {
        if self.busy() {
            return (yaw, pitch);
        }
        limit_look(&self.frame, &self.seat, yaw, pitch)
    }

    /// On the way through a door or across to another seat, with the gun down.
    pub fn busy(&self) -> bool {
        self.transit.is_some()
    }

    /// Controls in the hands, so no gun.
    pub fn hands_full(&self) -> bool {
        !self.busy() && (self.seat.role == SeatRole::Driver || self.seat.fire.is_none())
    }

    pub fn gun_bears(&self, yaw: f32, pitch: f32) -> bool {
        !self.busy() && can_fire(&self.frame, &self.seat, yaw, pitch)
    }

    /// How much riding unsettles a gun, as if running at this share of running speed.
    pub fn speed_fraction(&self) -> f32 {
        self.vel.length() * self.cfg.speed_shake
    }

    /// How fast the gun is being thrown about by the vehicle under it, degrees a second.
    pub fn jolt_rate(&self) -> f32 {
        self.ang_vel.length().to_degrees() * self.cfg.turn_shake
    }

    /// How far round the vehicle has carried a look this tick, which is not the soldier turning.
    pub fn carried_yaw(&self) -> f32 {
        self.ang_vel.y * TICK_DT
    }

    /// Soldier state for sitting here, set so that `Soldier::eye` is the seat's eye.
    pub fn place(&self, body: &mut MoveState, move_cfg: &MoveConfig) {
        body.stamina = (body.stamina + move_cfg.stamina_regen * TICK_DT).min(1.0);
        let eye = self.frame.point(self.placed.eye);
        body.stance = Stance::Crouch;
        body.eye_height = move_cfg.crouch_eye;
        body.pos = eye - Vec3::Y * body.eye_height;
        body.vel = self.vel;
        body.grounded = true;
        body.ground_normal = Vec3::Y;
        body.sprinting = false;
        body.stance_timer = 0.0;
        body.stride = 0.0;
        body.eye_rate = 0.0;
        body.lean = Default::default();
    }
}

/// What a command asks of a vehicle. Keys pressed together come as a shorter diagonal, which here is full
/// throttle and full lock at once and not a bit of each.
fn drive_from(cmd: &PlayerCmd, last: Buttons) -> DriveInput {
    let most = cmd.wish_move.abs().max_element();
    let wish = if most > 0.0 { cmd.wish_move * (cmd.wish_move.length().min(1.0) / most) } else { Vec2::ZERO };
    // Q and E, which lean on foot, change down and up at the wheel: one press, one gear.
    let pressed = |b: Buttons| cmd.buttons.contains(b) && !last.contains(b);
    let shift = pressed(Buttons::LEAN_RIGHT) as i8 - pressed(Buttons::LEAN_LEFT) as i8;
    DriveInput { throttle: wish.y, steer: wish.x, manned: true, handbrake: cmd.buttons.contains(Buttons::JUMP), shift, ..DriveInput::default() }
}

/// How far a gun's barrel, held ready, would stick into any vehicle along this line.
pub(crate) fn barrel_on_vehicles(vehicles: &[VehicleState], doors: &[Vec<f32>], cfgs: &VehicleTable, start: Vec3, forward: Vec3, along: f32) -> f32 {
    vehicles
        .iter()
        .zip(doors)
        .filter_map(|(vehicle, doors)| prs_vehicles::strike(vehicle, &cfgs[vehicle.kind], doors, &vehicle.frame(), start, forward, along))
        .map(|strike| along - strike.distance)
        .fold(0.0, f32::max)
}

/// Which way a soldier's hips go in a seat.
fn seat_yaw(frame: &Frame, seat: &SeatConfig) -> f32 {
    frame.yaw() + seat.facing
}

/// How many rays to check a door's swing with.
const DOOR_ARC_RAYS: u32 = 3;
/// A face with this much of its normal upwards counts as ground rather than wall.
const GROUND_NORMAL: f32 = 0.5;
/// Its up axis this far from straight up, as a dot product, and a vehicle counts as tipped over.
const TIPPED_OVER: f32 = 0.5;
/// How far from the middle of something tipped over someone crawls out to, in metres.
const TIPPED_REACH: f32 = 3.5;
/// How far above the ground under a door to try to fit someone getting out, in metres.
const EXIT_LIFTS: [f32; 4] = [0.0, 0.2, 0.4, 0.6];
/// Feet this far past the edge of a hull still count as on it, as a body's radius holds it up there.
const DECK_MARGIN: f32 = 0.3;

impl Sim {
    /// The vehicles a level starts with, parked where it says.
    pub(crate) fn park_vehicles(level: &Level, table: &VehicleTable) -> Vec<VehicleState> {
        level
            .vehicles
            .iter()
            .filter_map(|spawn| {
                let kind = VehicleKind(spawn.kind);
                let cfg = table.vehicles.get(spawn.kind as usize)?;
                let pos = spawn.pos + Vec3::Y * cfg.ride_height();
                Some(VehicleState::new(kind, cfg, Frame { pos, rot: Quat::from_rotation_y(spawn.yaw) }))
            })
            .collect()
    }

    /// A shut door for every way in of every vehicle.
    pub(crate) fn shut_doors(vehicles: &[VehicleState], table: &VehicleTable) -> Vec<Vec<f32>> {
        vehicles.iter().map(|vehicle| vec![0.0; table[vehicle.kind].entries.len()]).collect()
    }

    /// Doors swing open while someone is going through them, and shut after. Who is going through is the
    /// server's to know: a driver predicting their vehicle does not predict its doors.
    fn step_doors(&mut self) {
        for v in 0..self.vehicles.len() {
            let cfg = &self.vehicle_cfgs[self.vehicles[v].kind];
            for (e, entry) in cfg.entries.iter().enumerate() {
                let Some(door) = entry.door else { continue };
                let using = self.vehicles[v].occupants.0.iter().flatten().any(|&who| {
                    matches!(
                        self.soldiers.get(who as usize).and_then(|soldier| soldier.transit).map(|transit| transit.kind),
                        Some(TransitKind::In { entry } | TransitKind::Out { entry }) if entry as usize == e
                    )
                });
                let reach = TICK_DT / door.time.max(1e-3);
                let share = &mut self.vehicle_doors[v][e];
                *share = (*share + if using { reach } else { -reach }).clamp(0.0, 1.0);
            }
        }
    }

    /// Changes the vehicles in the game, as a mod would. Do this before anyone spawns.
    pub fn set_vehicles(&mut self, table: VehicleTable) {
        self.vehicles = Self::park_vehicles(&self.level, &table);
        self.vehicle_doors = Self::shut_doors(&self.vehicles, &table);
        self.vehicle_cfgs = table;
    }

    /// Where a carrier is. The one place that looks one up, so anything that carries people can join in.
    pub fn carrier_frame(&self, carrier: u32) -> Frame {
        match heli_of(carrier) {
            Some(h) => self.helicopters[h].frame(),
            None => self.vehicles[carrier as usize].frame(),
        }
    }

    /// The seats a carrier has.
    pub fn carrier_seats(&self, carrier: u32) -> &[SeatConfig] {
        match heli_of(carrier) {
            Some(_) => &self.heli_cfg.seats,
            None => &self.vehicle_cfgs[self.vehicles[carrier as usize].kind].seats,
        }
    }

    /// The ways in and out of a carrier.
    pub fn carrier_entries(&self, carrier: u32) -> &[Entry] {
        match heli_of(carrier) {
            Some(_) => &self.heli_cfg.entries,
            None => &self.vehicle_cfgs[self.vehicles[carrier as usize].kind].entries,
        }
    }

    fn carrier_links(&self, carrier: u32) -> &[SeatLink] {
        match heli_of(carrier) {
            Some(_) => &self.heli_cfg.links,
            None => &self.vehicle_cfgs[self.vehicles[carrier as usize].kind].links,
        }
    }

    fn carrier_occupants(&self, carrier: u32) -> &Occupants {
        match heli_of(carrier) {
            Some(h) => &self.helicopters[h].occupants,
            None => &self.vehicles[carrier as usize].occupants,
        }
    }

    fn carrier_speed(&self, carrier: u32) -> f32 {
        match heli_of(carrier) {
            Some(h) => self.helicopters[h].state.vel.length(),
            None => self.vehicles[carrier as usize].vel.length(),
        }
    }

    /// Whether anyone can get into it at all.
    fn carrier_open(&self, carrier: u32) -> bool {
        match heli_of(carrier) {
            Some(h) => !self.helicopters[h].state.wrecked,
            None => {
                let damage = &self.vehicles[carrier as usize].damage;
                !damage.wrecked && damage.burning.is_none()
            }
        }
    }

    fn carrier_burning(&self, carrier: u32) -> bool {
        heli_of(carrier).is_none() && self.vehicles[carrier as usize].damage.burning.is_some()
    }

    fn carrier_occupants_mut(&mut self, carrier: u32) -> &mut prs_seats::Occupants {
        match heli_of(carrier) {
            Some(h) => &mut self.helicopters[h].occupants,
            None => &mut self.vehicles[carrier as usize].occupants,
        }
    }

    pub fn carrier_ride_cfg(&self, carrier: u32) -> RideConfig {
        match heli_of(carrier) {
            Some(_) => self.heli_ride,
            None => self.vehicle_cfgs[self.vehicles[carrier as usize].kind].ride,
        }
    }

    pub(crate) fn ride_of(&self, at: SeatedAt) -> Ride {
        if let Some(h) = heli_of(at.carrier) {
            let heli = &self.helicopters[h].state;
            let seat = self.heli_cfg.seats[at.seat as usize];
            let frame = self.helicopters[h].frame();
            // The sim's spin is in the airframe's frame, and everything out here is in the world's.
            let ang_vel = frame.rot * heli.spin;
            return Ride {
                frame,
                vel: heli.vel + ang_vel.cross(frame.rot * seat.eye),
                ang_vel,
                airborne: !heli.on_ground,
                seat,
                cfg: self.heli_ride,
                placed: Placed::in_seat(&seat),
                transit: None,
            };
        }
        let vehicle = &self.vehicles[at.carrier as usize];
        let cfg = &self.vehicle_cfgs[vehicle.kind];
        let seat = cfg.seats[at.seat as usize];
        let frame = vehicle.frame();
        let r = frame.rot * seat.eye;
        Ride {
            frame,
            vel: vehicle.vel + vehicle.ang_vel.cross(r),
            ang_vel: vehicle.ang_vel,
            airborne: !vehicle.wheels.iter().any(|w| w.on_ground),
            seat,
            cfg: cfg.ride,
            placed: Placed::in_seat(&seat),
            transit: None,
        }
    }

    /// What one tick of this soldier needs to know about the seat they hold, if they do.
    pub(crate) fn ride_for(&self, id: SoldierId) -> Option<Ride> {
        let soldier = &self.soldiers[id];
        let mut ride = self.ride_of(soldier.seat?);
        if let (Some(at), Some(transit)) = (soldier.seat, soldier.transit) {
            ride.transit = Some(transit);
            let standing = Standing { eye: self.move_cfg.stand_eye, hips: ride.cfg.stand_hips };
            ride.placed = transit.placed(self.carrier_seats(at.carrier), self.carrier_entries(at.carrier), &ride.seat, standing);
        }
        Some(ride)
    }

    /// How a body sits in the seat it is in, in the world.
    /// Puts a seated soldier where their seat is now, as a client does for anyone it is drawing.
    pub fn place_seated(&mut self, id: SoldierId) {
        let Some(ride) = self.ride_for(id) else { return };
        ride.place(&mut self.soldiers[id].body, &self.move_cfg);
    }

    /// Whether this soldier is the one flying or driving what they sit in.
    pub fn is_at_controls(&self, id: SoldierId) -> bool {
        let soldier = &self.soldiers[id];
        let Some(at) = soldier.seat else { return false };
        let role = self.carrier_seats(at.carrier).get(at.seat as usize).map(|seat| seat.role);
        role == Some(SeatRole::Driver) && soldier.transit.is_none()
    }

    /// Where a shot really starts from. A passenger rides what they sit in as they see it, `rewind`
    /// ticks in the past, so that is where their seat is for the shot too. Whoever is at the controls
    /// predicts it and is in the present, and so is anyone on foot.
    pub fn shot_origin(&self, id: SoldierId, origin: Vec3, rewind: u32) -> Vec3 {
        let Some(at) = self.soldiers[id].seat else { return origin };
        if self.is_at_controls(id) {
            return origin;
        }
        let seen = self.past_carrier_frame(at.carrier, rewind);
        seen.map_or(origin, |frame| frame.point(self.carrier_frame(at.carrier).to_local(origin)))
    }

    /// Where a vehicle or helicopter was `rewind` ticks ago, if that is on record.
    pub fn past_carrier_frame(&self, carrier: u32, rewind: u32) -> Option<Frame> {
        match heli_of(carrier) {
            Some(h) => self.past.helicopter_seen(h, self.tick, rewind),
            None => self.past.vehicle_seen(carrier as usize, self.tick, rewind),
        }
    }

    pub(crate) fn sit_pose(&self, at: SeatedAt, id: SoldierId, look_yaw: f32) -> SitPose {
        self.sit_pose_at(id, at, &self.carrier_frame(at.carrier), look_yaw)
    }

    /// How a body sits in a seat with the carrier at this frame, for drawing one between ticks.
    pub fn sit_pose_at(&self, id: SoldierId, at: SeatedAt, frame: &Frame, look_yaw: f32) -> SitPose {
        let seat = &self.carrier_seats(at.carrier)[at.seat as usize];
        let Some(ride) = self.ride_for(id).filter(|ride| ride.busy()) else {
            let (mut feet, mut yaw) = (frame.point(seat.feet), seat_yaw(frame, seat));
            if seat.swivel {
                // Stood on a ring, the whole body goes round with the look.
                let hips = frame.point(seat.hips);
                let round = Quat::from_rotation_y(look_yaw - yaw);
                feet = hips + round * (feet - hips);
                yaw = look_yaw;
            }
            let hands = seat.hands.map(|hands| self.hands_on_wheel(at.carrier, hands)).map(|[l, r]| [frame.point(l), frame.point(r)]);
            return SitPose { hips: frame.point(seat.hips), feet, yaw, hands };
        };
        // On the way through a door: placed by the path, hands on the lowered gun.
        SitPose { hips: frame.point(ride.placed.hips), feet: frame.point(ride.placed.feet), yaw: seat_yaw(frame, seat), hands: None }
    }

    /// Hands on a steering wheel go round with it, in the carrier's frame, as far as arms do without letting go.
    fn hands_on_wheel(&self, carrier: u32, hands: [Vec3; 2]) -> [Vec3; 2] {
        if heli_of(carrier).is_some() {
            return hands;
        }
        let vehicle = &self.vehicles[carrier as usize];
        let cfg = &self.vehicle_cfgs[vehicle.kind];
        let Some((middle, _)) = cfg.steering_wheel() else { return hands };
        let s = &cfg.steering;
        let turned = (vehicle.steer * s.wheel_ratio).clamp(-s.hands_follow, s.hands_follow) / s.wheel_ratio;
        let round = cfg.steering_wheel_turn(turned) * cfg.steering_wheel_turn(0.0).conjugate();
        hands.map(|hand| middle + round * (hand - middle))
    }

    /// Where this soldier's eye is with their carrier at this frame, for drawing between ticks.
    pub fn seated_eye_at(&self, id: SoldierId, frame: &Frame) -> Option<Vec3> {
        self.ride_for(id).map(|ride| frame.point(ride.placed.eye))
    }

    /// Whether this soldier's gun is put away, with their hands on the controls, and so not drawn.
    pub fn gun_put_away(&self, id: SoldierId) -> bool {
        self.ride_for(id).is_some_and(|ride| ride.hands_full())
    }

    fn drive_input(&self, vehicle: &VehicleState, cmds: &[(SoldierId, PlayerCmd)]) -> DriveInput {
        let cfg = &self.vehicle_cfgs[vehicle.kind];
        let Some(driver) = vehicle.driver(cfg) else {
            return DriveInput::default();
        };
        // Not yet: they are still on their way into the seat, or on their way out of it.
        if self.soldiers[driver as usize].transit.is_some() {
            return DriveInput::default();
        }
        let cmd = cmds.iter().find(|(who, _)| *who == driver as usize).map(|(_, cmd)| cmd);
        let last = self.soldiers[driver as usize].last_buttons;
        cmd.map_or(DriveInput { manned: true, ..DriveInput::default() }, |cmd| drive_from(cmd, last))
    }

    /// Seats whose soldier is no longer there, and soldiers whose seat no longer knows them.
    fn free_lost_seats(&mut self) {
        for (h, heli) in self.helicopters.iter_mut().enumerate() {
            for (s, slot) in heli.occupants.0.iter_mut().enumerate() {
                let here = SeatedAt { carrier: HELI_CARRIERS + h as u32, seat: s as u8 };
                if let Some(who) = *slot
                    && self.soldiers.get(who as usize).is_none_or(|soldier| !soldier.present || soldier.seat != Some(here))
                {
                    *slot = None;
                }
            }
        }
        for (v, vehicle) in self.vehicles.iter_mut().enumerate() {
            for (s, slot) in vehicle.occupants.0.iter_mut().enumerate() {
                let here = SeatedAt { carrier: v as u32, seat: s as u8 };
                if let Some(who) = *slot
                    && self.soldiers.get(who as usize).is_none_or(|soldier| !soldier.present || soldier.seat != Some(here))
                {
                    *slot = None;
                }
            }
        }
    }

    /// All the vehicles for a tick, before anyone in them moves, so that a seat is where it is by the
    /// time its soldier fires.
    pub(crate) fn step_vehicles(&mut self, cmds: &[(SoldierId, PlayerCmd)]) {
        self.free_lost_seats();
        for v in 0..self.vehicles.len() {
            let mut input = self.drive_input(&self.vehicles[v], cmds);
            let lifted = self.add_lift(v, &mut input);
            let pushed = self.add_push(v, &mut input);
            self.before_lift(v, lifted || pushed);
            self.step_one_vehicle(v, &input);
        }
        self.collide_vehicles();
        self.burn_occupants();
        self.step_doors();
        self.step_helicopters(cmds);
        self.sync_movers();
    }

    /// Hulls for bodies to walk into and stand on, wherever vehicles have got to. Made fresh each tick
    /// from nothing but the vehicles, so there is nothing here to save or get out of step.
    pub(crate) fn sync_movers(&mut self) {
        self.world.clear_movers();
        for vehicle in &self.vehicles {
            for hull in &self.vehicle_cfgs[vehicle.kind].hull {
                self.world.add_mover(vehicle.pos + vehicle.rot * hull.center, hull.half, vehicle.rot);
            }
        }
    }

    /// The vehicle this soldier is driving, for a client predicting them. Nothing else moves.
    pub(crate) fn step_predicted_vehicle(&mut self, id: SoldierId, cmd: &PlayerCmd) {
        let Some(at) = self.soldiers[id].seat else {
            return;
        };
        if let Some(h) = heli_of(at.carrier) {
            self.step_predicted_helicopter(id, h, cmd);
            return;
        }
        let vehicle = &self.vehicles[at.carrier as usize];
        if vehicle.driver(&self.vehicle_cfgs[vehicle.kind]) == Some(id as u32) && self.soldiers[id].transit.is_none() {
            let mut input = drive_from(cmd, self.soldiers[id].last_buttons);
            self.add_lift(at.carrier as usize, &mut input);
            self.add_push(at.carrier as usize, &mut input);
            let cfg = &self.vehicle_cfgs[vehicle.kind];
            let mut happened = Vec::new();
            let state = &mut self.vehicles[at.carrier as usize];
            prs_vehicles::step(state, cfg, &input, &self.world, TICK_DT, &mut happened);
            // What it does to the vehicle itself is part of how it drives from here on, so that
            // follows too. What it does to anyone, and a new vehicle coming, are the server's to say.
            prs_vehicles::step_damage(state, &cfg.damage, TICK_DT, &mut happened);
            let mut i = 0;
            while i < happened.len() {
                if let VehicleEvent::Crashed { speed, .. } = happened[i] {
                    prs_vehicles::crash(state, &cfg.damage, speed, &mut happened);
                }
                i += 1;
            }
        }
    }

    /// Every vehicle and helicopter back where the level parks it, new, with nobody in it. For a new round.
    pub fn reset_rides(&mut self) {
        for v in 0..self.vehicles.len() {
            let (kind, home) = (self.vehicles[v].kind, self.vehicles[v].home);
            self.eject_all(v);
            self.vehicles[v] = VehicleState::new(kind, &self.vehicle_cfgs[kind], home);
            self.vehicle_doors[v].fill(0.0);
            self.events.push(SimEvent::Vehicle { vehicle: v, event: VehicleEvent::Respawned });
        }
        for h in 0..self.helicopters.len() {
            self.eject_all_from(HELI_CARRIERS + h as u32);
            let heli = &self.helicopters[h];
            self.helicopters[h] = crate::copters::Helicopter::new(&self.heli_cfg, heli.home, heli.home_yaw);
            self.events.push(SimEvent::Heli { heli: h, event: prs_helicopter::HeliEvent::Respawned });
        }
    }

    fn step_one_vehicle(&mut self, v: usize, input: &DriveInput) {
        let mut happened = std::mem::take(&mut self.vehicle_events);
        happened.clear();
        let kind = self.vehicles[v].kind;
        let cfg = &self.vehicle_cfgs[kind];
        prs_vehicles::step(&mut self.vehicles[v], cfg, input, &self.world, TICK_DT, &mut happened);
        let due = prs_vehicles::step_damage(&mut self.vehicles[v], &cfg.damage, TICK_DT, &mut happened);
        if due {
            let home = self.vehicles[v].home;
            self.eject_all(v);
            self.vehicles[v] = VehicleState::new(kind, &self.vehicle_cfgs[kind], home);
            self.vehicle_doors[v].fill(0.0);
            happened.push(VehicleEvent::Respawned);
        }
        self.vehicle_happened(v, &mut happened);
        self.vehicle_events = happened;
    }

    /// Whatever follows from what has happened to a vehicle.
    fn vehicle_happened(&mut self, v: usize, happened: &mut Vec<VehicleEvent>) {
        let mut i = 0;
        while i < happened.len() {
            let event = happened[i];
            i += 1;
            self.events.push(SimEvent::Vehicle { vehicle: v, event });
            match event {
                VehicleEvent::LiftBlocked => self.lift_blocked(v),
                VehicleEvent::Crashed { speed, .. } => {
                    let cfg = &self.vehicle_cfgs[self.vehicles[v].kind].damage;
                    let over = speed - cfg.hurt_from;
                    let hurt = over * cfg.hurt_per_speed;
                    prs_vehicles::crash(&mut self.vehicles[v], cfg, speed, happened);
                    if over > 0.0 {
                        for who in self.occupants_of(v) {
                            self.knock(who, hurt);
                        }
                    }
                }
                VehicleEvent::Exploded { point } => {
                    self.eject_all(v);
                    self.blast(v, point);
                }
                _ => {}
            }
        }
    }

    fn occupants_of(&self, v: usize) -> Vec<SoldierId> {
        self.vehicles[v].occupants.0.iter().flatten().map(|&who| who as usize).collect()
    }

    /// Everyone near a vehicle going up who is not behind something.
    fn blast(&mut self, v: usize, point: Vec3) {
        let (reach, hurt) = {
            let cfg = &self.vehicle_cfgs[self.vehicles[v].kind].damage;
            (cfg.blast_radius, cfg.blast_hurt)
        };
        for id in 0..self.soldiers.len() {
            let soldier = &self.soldiers[id];
            if !soldier.present || soldier.health.is_dead() {
                continue;
            }
            let chest = soldier.body.pos + Vec3::Y * BODY_MIDDLE;
            let path = chest - point;
            let distance = path.length();
            if distance >= reach {
                continue;
            }
            // From above the wreck, which is itself in the way of nothing.
            let from = point + Vec3::Y;
            let to_chest = chest - from;
            let shielded = self.world.raycast(from, to_chest.normalize_or(Vec3::Y), to_chest.length()).is_some();
            if !shielded {
                self.hurt(id, Zone::Chest, hurt * (1.0 - distance / reach));
            }
        }
    }

    /// Fire reaches whoever stays in it, once a second.
    fn burn_occupants(&mut self) {
        if self.tick % TICK_HZ as u32 != 0 {
            return;
        }
        for v in 0..self.vehicles.len() {
            if self.vehicles[v].damage.burning.is_some() {
                let hurt = self.vehicle_cfgs[self.vehicles[v].kind].damage.burn_hurt;
                for who in self.occupants_of(v) {
                    self.knock(who, hurt);
                }
            }
        }
    }

    fn collide_vehicles(&mut self) {
        let mut pair = Vec::new();
        for i in 1..self.vehicles.len() {
            let (before, rest) = self.vehicles.split_at_mut(i);
            let b = &mut rest[0];
            for (j, a) in before.iter_mut().enumerate() {
                if a.asleep && b.asleep {
                    continue;
                }
                prs_vehicles::collide(a, &self.vehicle_cfgs[a.kind], b, &self.vehicle_cfgs[b.kind], &mut pair);
                for (second, event) in pair.drain(..) {
                    let vehicle = if second { i } else { j };
                    self.vehicle_pair_events.push((vehicle, event));
                }
            }
        }
        for (v, event) in std::mem::take(&mut self.vehicle_pair_events) {
            let mut happened = vec![event];
            self.vehicle_happened(v, &mut happened);
        }
    }

    /// A number key in a seat. It takes time, and only goes to a free seat connected to this one.
    pub(crate) fn swap_seat(&mut self, id: SoldierId, seat: usize) {
        let soldier = &self.soldiers[id];
        let (Some(at), None) = (soldier.seat, soldier.transit) else {
            return;
        };
        if at.seat as usize == seat
            || !self.carrier_occupants(at.carrier).is_free(seat)
            || link_between(self.carrier_links(at.carrier), at.seat, seat as u8).is_none()
        {
            return;
        }
        let occupants = self.carrier_occupants_mut(at.carrier);
        occupants.leave(id as u32);
        occupants.sit(seat, id as u32);
        let seat = seat as u8;
        let soldier = &mut self.soldiers[id];
        soldier.seat = Some(SeatedAt { carrier: at.carrier, seat });
        soldier.transit = Some(Transit::new(TransitKind::Across { from: at.seat }));
        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Swapped { carrier: at.carrier, seat } });
    }

    /// Whether someone in this seat could move to that one now.
    pub fn seat_connects(&self, at: SeatedAt, seat: usize) -> bool {
        at.seat as usize != seat && self.carrier_occupants(at.carrier).is_free(seat) && link_between(self.carrier_links(at.carrier), at.seat, seat as u8).is_some()
    }

    /// Every carrier there is, by id.
    fn carrier_ids(&self) -> impl Iterator<Item = u32> {
        (0..self.vehicles.len() as u32).chain((0..self.helicopters.len() as u32).map(|h| HELI_CARRIERS + h))
    }

    /// The door this soldier is at and facing, the seat it would take them to, and how squarely they
    /// face it. Nothing is offered from further than an arm's length, through a wall, or to a door that
    /// cannot open.
    fn door_in_reach(&self, id: SoldierId, look: Vec3) -> Option<(u32, u8, u8, f32)> {
        let soldier = &self.soldiers[id];
        if soldier.seat.is_some() || !soldier.health.is_up() || soldier.body.climb.active() {
            return None;
        }
        let (eye, feet) = (soldier.eye(), soldier.body.pos);
        let mut best = None;
        let mut facing_best = f32::MIN;
        for carrier in self.carrier_ids() {
            let cfg = self.carrier_ride_cfg(carrier);
            if !self.carrier_open(carrier) || self.carrier_speed(carrier) > cfg.board_speed || heli_of(carrier).is_some_and(|h| !self.heli_at_door_height(h)) {
                continue;
            }
            let frame = self.carrier_frame(carrier);
            for (e, entry) in self.carrier_entries(carrier).iter().enumerate() {
                let stand = frame.point(entry.stand);
                if Vec2::new(stand.x - feet.x, stand.z - feet.z).length() > cfg.board_reach + cfg.board_climb {
                    continue;
                }
                let occupants = self.carrier_occupants(carrier);
                let Some(&seat) = entry.seats.iter().find(|&&seat| occupants.is_free(seat as usize)) else {
                    continue;
                };
                let hand = frame.point(entry.hand);
                let to_hand = hand - eye;
                let facing = to_hand.normalize_or_zero().dot(look);
                if facing < cfg.board_facing || facing <= facing_best {
                    continue;
                }
                let seat_eye = frame.point(self.carrier_seats(carrier)[seat as usize].eye);
                let Some(spot) = self.door_spot(seat_eye, entry, &frame, cfg.exit_drop) else {
                    continue;
                };
                let (across, up) = (Vec2::new(spot.x - feet.x, spot.z - feet.z).length(), (spot.y - feet.y).abs());
                if across > cfg.board_reach || up > cfg.board_climb {
                    continue;
                }
                // Stop at the near face of the door: the vehicle itself is not in the way of its handle.
                let clear = self.world.raycast(eye, to_hand.normalize_or(Vec3::Y), to_hand.length()).is_none();
                if clear {
                    best = Some((carrier, e as u8, seat, facing));
                    facing_best = facing;
                }
            }
        }
        best
    }

    /// What F would do right now for someone on foot: whichever of a door and a kit they face more squarely.
    pub fn use_target(&self, id: SoldierId) -> Option<UseTarget> {
        let soldier = &self.soldiers[id];
        self.use_target_looking(id, view_dir(soldier.yaw, soldier.pitch))
    }

    /// The same, looking this way, as a command that presses F says.
    pub(crate) fn use_target_looking(&self, id: SoldierId, look: Vec3) -> Option<UseTarget> {
        let door = self.door_in_reach(id, look);
        let kit = self.kit_in_reach(id);
        match (door, kit) {
            _ if self.lift_in_reach(id, look).is_some_and(|(_, facing)| {
                kit.is_none_or(|kit| (self.kits[kit].pos - self.soldiers[id].eye()).normalize_or_zero().dot(look) <= facing)
            }) =>
            {
                self.lift_in_reach(id, look).map(|(vehicle, _)| UseTarget::Lift { vehicle })
            }
            _ if self.push_in_reach(id, look).is_some_and(|(_, _, facing)| {
                door.is_none_or(|door| door.3 < facing)
                    && kit.is_none_or(|kit| (self.kits[kit].pos - self.soldiers[id].eye()).normalize_or_zero().dot(look) <= facing)
            }) =>
            {
                self.push_in_reach(id, look).map(|(vehicle, spot, _)| UseTarget::Push { vehicle, spot })
            }
            (Some((carrier, entry, seat, facing)), Some(kit)) => {
                let to_kit = (self.kits[kit].pos - self.soldiers[id].eye()).normalize_or_zero();
                if to_kit.dot(look) > facing {
                    Some(UseTarget::Kit(kit))
                } else {
                    Some(UseTarget::Door { carrier, entry, seat })
                }
            }
            (Some((carrier, entry, seat, _)), None) => Some(UseTarget::Door { carrier, entry, seat }),
            (None, Some(kit)) => Some(UseTarget::Kit(kit)),
            (None, None) => None,
        }
    }

    /// F on foot at a door: they hold the seat from now on, and are on their way to it in the open.
    pub(crate) fn start_entering(&mut self, id: SoldierId, carrier: u32, entry: u8, seat: u8) {
        self.sit_quietly(id, SeatedAt { carrier, seat });
        self.soldiers[id].transit = Some(Transit::new(TransitKind::In { entry }));
        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Entering { carrier, seat } });
    }

    /// Puts someone in a free seat straight away, wherever they are. For scripts and scenes: a soldier
    /// gets in by [`Self::use_target`] and the door.
    pub fn sit_in(&mut self, id: SoldierId, at: SeatedAt) {
        self.sit_quietly(id, at);
        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Entered { carrier: at.carrier, seat: at.seat } });
    }

    fn sit_quietly(&mut self, id: SoldierId, at: SeatedAt) {
        self.carrier_occupants_mut(at.carrier).sit(at.seat as usize, id as u32);
        if heli_of(at.carrier).is_none() {
            prs_vehicles::wake(&mut self.vehicles[at.carrier as usize]);
        }
        self.interrupt_hands(id);
        let soldier = &mut self.soldiers[id];
        // Whatever was in the hands to treat someone goes away for the gun.
        if !soldier.inventory.selected.is_gun() {
            soldier.inventory.selected = soldier.inventory.held;
        }
        soldier.seat = Some(at);
    }

    /// F in a seat. Out of whichever of its doors they are looking towards and can open, or not at all.
    pub(crate) fn try_leave(&mut self, id: SoldierId) -> bool {
        let soldier = &self.soldiers[id];
        let (Some(at), None) = (soldier.seat, soldier.transit) else {
            return false;
        };
        let look = view_dir(soldier.yaw, soldier.pitch);
        let Some(entry) = self.way_out(at, look) else {
            self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::ExitBlocked { carrier: at.carrier } });
            return false;
        };
        self.soldiers[id].transit = Some(Transit::new(TransitKind::Out { entry }));
        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Leaving { carrier: at.carrier } });
        true
    }

    /// One tick of going through a door or across to another seat.
    pub(crate) fn step_transit(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        let (Some(at), Some(mut transit)) = (soldier.seat, soldier.transit) else {
            return;
        };
        let time = match transit.kind {
            TransitKind::In { entry } | TransitKind::Out { entry } => self.carrier_entries(at.carrier).get(entry as usize).map_or(1.0, |entry| entry.time),
            TransitKind::Across { from } => link_between(self.carrier_links(at.carrier), from, at.seat).unwrap_or(1.0),
        };
        // It has driven off with them half in: they lose their grip and come back out.
        let cfg = self.carrier_ride_cfg(at.carrier);
        if matches!(transit.kind, TransitKind::In { .. }) && self.carrier_speed(at.carrier) > cfg.board_speed {
            transit.back = true;
        }
        let ended = transit.advance(time, TICK_DT);
        self.soldiers[id].transit = Some(transit);
        if !ended {
            return;
        }
        match (transit.kind, transit.back) {
            (TransitKind::In { .. }, false) => {
                self.soldiers[id].transit = None;
                self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Entered { carrier: at.carrier, seat: at.seat } });
            }
            (TransitKind::Out { entry }, false) => {
                // It may have moved or something may have come along since the door was tried.
                let spot = self.door_spot_of(at, entry);
                match spot {
                    Some(feet) => self.stand_up(id, at, feet, true),
                    None => {
                        self.soldiers[id].transit = Some(Transit { back: true, ..transit });
                        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::ExitBlocked { carrier: at.carrier } });
                    }
                }
            }
            (TransitKind::In { entry }, true) => {
                let feet = self.door_spot_of(at, entry).unwrap_or_else(|| self.ground_beside(at, entry));
                self.stand_up(id, at, feet, false);
            }
            (TransitKind::Out { .. }, true) | (TransitKind::Across { .. }, _) => self.soldiers[id].transit = None,
        }
    }

    /// Being hit in a doorway stops you going through it: you go back the way you came. Not back into a fire.
    pub(crate) fn interrupt_transit(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        let Some(at) = soldier.seat else { return };
        let burning = self.carrier_burning(at.carrier);
        if let Some(transit) = self.soldiers[id].transit.as_mut()
            && !matches!(transit.kind, TransitKind::Across { .. })
            && !burning
        {
            transit.back = true;
        }
    }

    /// Takes someone out of their seat whatever the way out is like: they are down, dead, or it blew up.
    /// They come out at a door of that seat, the one they were at if they were at one.
    pub(crate) fn eject(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        let Some(at) = soldier.seat else {
            return;
        };
        let look = view_dir(soldier.yaw, soldier.pitch);
        let entry = match soldier.transit.map(|transit| transit.kind) {
            Some(TransitKind::In { entry }) | Some(TransitKind::Out { entry }) => Some(entry),
            _ => self.way_out(at, look),
        };
        let entry = entry.or_else(|| self.entry_of_seat(at));
        let feet = match entry {
            Some(entry) => self.door_spot_of(at, entry).unwrap_or_else(|| self.ground_beside(at, entry)),
            None => self.soldiers[id].body.pos,
        };
        self.stand_up(id, at, feet, false);
    }

    fn eject_all(&mut self, v: usize) {
        for who in self.occupants_of(v) {
            self.eject(who);
        }
    }

    /// Everyone out of a carrier, whatever it is.
    pub(crate) fn eject_all_from(&mut self, carrier: u32) {
        let occupants = self.carrier_occupants_mut(carrier).0.clone();
        for who in occupants.into_iter().flatten() {
            self.eject(who as usize);
        }
    }

    /// On their feet outside. Going fast enough, that is a jump, and it hurts.
    fn stand_up(&mut self, id: SoldierId, at: SeatedAt, feet: Vec3, jumped: bool) {
        let vel = self.ride_of(at).vel;
        let ride = self.carrier_ride_cfg(at.carrier);
        self.carrier_occupants_mut(at.carrier).leave(id as u32);
        let cfg = &self.move_cfg;
        let soldier = &mut self.soldiers[id];
        soldier.seat = None;
        soldier.transit = None;
        let body = &mut soldier.body;
        body.pos = feet;
        body.vel = vel;
        body.grounded = false;
        body.stance = Stance::Stand;
        body.eye_height = cfg.stand_eye;
        soldier.anim.hip_yaw = soldier.yaw;
        self.events.push(SimEvent::Seat { soldier: id, event: SeatEvent::Left { carrier: at.carrier } });
        let over = vel.length() - ride.exit_speed;
        if jumped {
            match heli_of(at.carrier) {
                Some(h) => self.jump_out_hurts(id, h, feet),
                None if over > 0.0 => self.knock(id, over * ride.jump_hurt),
                None => {}
            }
        }
    }

    /// The door of this seat they would use, looking this way: the one most that way that can open.
    fn way_out(&self, at: SeatedAt, look: Vec3) -> Option<u8> {
        let frame = self.carrier_frame(at.carrier);
        let seat_eye = frame.point(self.carrier_seats(at.carrier)[at.seat as usize].eye);
        let drop = self.carrier_ride_cfg(at.carrier).exit_drop;
        let mut doors: Vec<(u8, f32)> = self
            .carrier_entries(at.carrier)
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.seats.contains(&at.seat))
            .map(|(e, entry)| (e as u8, (frame.point(entry.stand) - seat_eye).normalize_or_zero().dot(look)))
            .collect();
        doors.sort_by(|a, b| b.1.total_cmp(&a.1));
        doors.into_iter().map(|(e, _)| e).find(|&e| self.door_spot(seat_eye, &self.carrier_entries(at.carrier)[e as usize], &frame, drop).is_some())
    }

    /// Any door of this seat, open or not.
    fn entry_of_seat(&self, at: SeatedAt) -> Option<u8> {
        self.carrier_entries(at.carrier).iter().position(|entry| entry.seats.contains(&at.seat)).map(|e| e as u8)
    }

    fn door_spot_of(&self, at: SeatedAt, entry: u8) -> Option<Vec3> {
        let frame = self.carrier_frame(at.carrier);
        let seat_eye = frame.point(self.carrier_seats(at.carrier)[at.seat as usize].eye);
        let drop = self.carrier_ride_cfg(at.carrier).exit_drop;
        self.door_spot(seat_eye, self.carrier_entries(at.carrier).get(entry as usize)?, &frame, drop)
    }

    /// Where someone stands to use this door, if it can open: something under the feet, room to stand,
    /// and nothing solid between the seat and there.
    fn door_spot(&self, eye: Vec3, entry: &Entry, frame: &Frame, drop: f32) -> Option<Vec3> {
        let capsule = self.move_cfg.capsule(Stance::Stand);
        let stand = frame.point(entry.stand);
        // On its side or its roof the doors are not where they were, and anyone can crawl out of the open
        // side of it: from the middle out past the hull, in whichever direction there is room to stand.
        if (frame.rot * Vec3::Y).y < TIPPED_OVER {
            let out = Vec3::new(stand.x - frame.pos.x, 0.0, stand.z - frame.pos.z).normalize_or(Vec3::X);
            let across = Vec3::new(-out.z, 0.0, out.x);
            return [out, -out, across, -across].iter().find_map(|way| {
                let at = frame.pos + *way * TIPPED_REACH;
                let above = at + Vec3::Y * drop;
                let hit = self.world.raycast(above, Vec3::NEG_Y, drop * 2.0).filter(|hit| hit.distance > 0.0)?;
                let ground = above + Vec3::NEG_Y * (hit.distance - EXIT_CLEARANCE);
                EXIT_LIFTS.iter().map(|lift| ground + Vec3::Y * *lift).find(|feet| self.world.capsule_is_clear(*feet, capsule))
            });
        }
        let feet = if entry.on_ground {
            let above = Vec3::new(stand.x, stand.y + drop, stand.z);
            let hit = self.world.raycast(above, Vec3::NEG_Y, drop * 2.0).filter(|hit| hit.distance > 0.0)?;
            above + Vec3::NEG_Y * (hit.distance - EXIT_CLEARANCE)
        } else {
            stand + Vec3::Y * EXIT_CLEARANCE
        };
        // On a slope a body fits a little above the ground under its middle, and drops the rest.
        let feet = EXIT_LIFTS.iter().map(|lift| feet + Vec3::Y * *lift).find(|feet| self.world.capsule_is_clear(*feet, capsule))?;
        let chest = feet + Vec3::Y * BODY_MIDDLE;
        let path = chest - eye;
        // A wall is in the way of that. The lip of the ditch it has gone into is not.
        let clear = self.world.raycast(eye, path.normalize_or(Vec3::Y), path.length()).is_none_or(|hit| hit.normal.y > GROUND_NORMAL);
        (clear && self.door_can_swing(entry, frame)).then_some(feet)
    }

    /// Whether the door of this way in has room to swing, its far edge in the arc it sweeps.
    fn door_can_swing(&self, entry: &Entry, frame: &Frame) -> bool {
        let Some(door) = entry.door else { return true };
        let hinge = frame.point(door.hinge);
        let Some(panel) = self.vehicle_cfgs.vehicles.iter().find_map(|cfg| {
            cfg.entries.iter().any(|other| std::ptr::eq(other, entry)).then(|| cfg.panels[door.panel as usize].center)
        }) else {
            return true;
        };
        let reach = (panel - door.hinge) * 2.0;
        // A few rays round the arc, as the door has to get there as well as be there.
        (1..=DOOR_ARC_RAYS).all(|i| {
            let turned = Quat::from_rotation_y(door.open * i as f32 / DOOR_ARC_RAYS as f32);
            let edge = frame.rot * (turned * reach);
            let length = edge.length();
            // A wall stops a door. A bit of bank it is parked against gets scraped over.
            self.world.raycast(hinge, edge / length.max(1e-3), length).is_none_or(|hit| hit.normal.y > GROUND_NORMAL)
        })
    }

    /// The ground at a door, for someone being taken out when it would not open.
    fn ground_beside(&self, at: SeatedAt, entry: u8) -> Vec3 {
        let frame = self.carrier_frame(at.carrier);
        let stand = self.carrier_entries(at.carrier).get(entry as usize).map_or(frame.pos, |entry| frame.point(entry.stand));
        let above = stand + Vec3::Y * EXIT_SEARCH;
        self.world
            .raycast(above, Vec3::NEG_Y, EXIT_SEARCH * 2.0)
            .map_or(stand, |hit| above + Vec3::NEG_Y * (hit.distance - EXIT_CLEARANCE))
    }

    /// Whether a soldier standing here is inside any vehicle, and the way out and how far.
    fn hull_overlap(&self, feet: Vec3, capsule: Capsule) -> Option<(usize, Vec3)> {
        let half_height = capsule.height * 0.5;
        let middle = feet + Vec3::Y * half_height;
        for (v, vehicle) in self.vehicles.iter().enumerate() {
            let cfg = &self.vehicle_cfgs[vehicle.kind];
            if vehicle.pos.distance_squared(middle) > (cfg.reach() + capsule.height).powi(2) {
                continue;
            }
            let frame = vehicle.frame();
            let local = frame.to_local(middle);
            for hull in &cfg.hull {
                let d = local - hull.center;
                let room = hull.half + Vec3::new(capsule.radius, half_height, capsule.radius);
                let over = room - d.abs();
                // Standing on top of it is not being inside it.
                if over.x <= 0.0 || over.z <= 0.0 || over.y <= DECK_TOLERANCE {
                    continue;
                }
                let out = if over.x < over.z { Vec3::X * over.x * d.x.signum() } else { Vec3::Z * over.z * d.z.signum() };
                let world = frame.rot * out;
                return Some((v, Vec3::new(world.x, 0.0, world.z)));
            }
        }
        None
    }

    /// Someone on foot and the vehicles around them: pushed clear of each, and hurt if one came at them
    /// fast enough.
    pub(crate) fn keep_off_vehicles(&mut self, id: SoldierId) {
        self.keep_off_helicopters(id);
        if self.vehicles.is_empty() {
            return;
        }
        self.ride_on_deck(id);
        // A climb has already checked its path, up the side of a hull if that is what it is climbing.
        let crouched = self.move_cfg.capsule(Stance::Crouch);
        if self.soldiers[id].body.climb.active() && self.world.capsule_is_clear(self.soldiers[id].body.pos, crouched) {
            return;
        }
        let capsule = self.move_cfg.capsule(self.soldiers[id].body.stance);
        let Some((v, out)) = self.hull_overlap(self.soldiers[id].body.pos, capsule) else {
            return;
        };
        let vehicle = &self.vehicles[v];
        let normal = out.normalize_or(Vec3::X);
        let r = self.soldiers[id].body.pos - vehicle.pos;
        let coming = (vehicle.vel + vehicle.ang_vel.cross(r)).dot(normal);
        let moving = self.soldiers[id].body.vel.dot(normal);
        let cfg = &self.vehicle_cfgs[vehicle.kind].damage;
        // Walking into one is your own doing. It takes the vehicle coming at you to hurt.
        let hurt = (coming - moving.max(0.0) > cfg.ram_from).then(|| (coming - moving.max(0.0) - cfg.ram_from) * cfg.ram_hurt);
        let body = &mut self.soldiers[id].body;
        body.pos += out;
        if coming > moving {
            body.vel += normal * (coming - moving);
        }
        if let Some(raw) = hurt {
            self.knock(id, raw);
        }
    }

    /// Someone stood on a hull goes with it: where they are is where the hull is, plus where on it they
    /// were. That is what lets a client predict them on a hull it only sees in the past, as a seat does.
    fn ride_on_deck(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        if soldier.body.climb.active() {
            return;
        }
        if let Some(deck) = soldier.deck
            && let Some(vehicle) = self.vehicles.get(deck.vehicle as usize)
        {
            let at = vehicle.frame().point(deck.local);
            self.soldiers[id].body.pos = at;
        }
    }

    /// After they have moved, whether they are stood on a hull, and where on it.
    pub(crate) fn note_deck(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        let feet = soldier.body.pos;
        let deck = if soldier.seat.is_some() || !soldier.body.grounded {
            None
        } else {
            self.vehicles.iter().enumerate().find_map(|(v, vehicle)| {
                let cfg = &self.vehicle_cfgs[vehicle.kind];
                if vehicle.pos.distance_squared(feet) > cfg.reach().powi(2) {
                    return None;
                }
                let local = vehicle.frame().to_local(feet);
                cfg.hull
                    .iter()
                    .any(|hull| {
                        let d = (local - hull.center).abs();
                        d.x < hull.half.x + DECK_MARGIN && d.z < hull.half.z + DECK_MARGIN && (local.y - hull.center.y - hull.half.y).abs() < DECK_TOLERANCE
                    })
                    .then_some(OnDeck { vehicle: v as u32, local })
            })
        };
        self.soldiers[id].deck = deck;
    }

    /// Damage to a vehicle at this part, as a bullet landing there would do.
    pub fn bullet_into_vehicle(&mut self, v: usize, zone: VehicleZone, raw: f32) {
        let mut happened = Vec::new();
        let kind = self.vehicles[v].kind;
        prs_vehicles::hurt(&mut self.vehicles[v], &self.vehicle_cfgs[kind].damage, zone, raw, &mut happened);
        self.vehicle_happened(v, &mut happened);
    }
}
