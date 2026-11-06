//! The whole game simulation for one tick: commands in, new state and events out.
//!
//! This is the crate a dedicated server will run as is. It wires movement, weapons, health and what a
//! soldier carries together so they can stay ignorant of each other, and it never touches Bevy.

mod care;
pub mod copters;
mod crowd;
mod grenades;
mod hits;
mod lift;
mod push;
mod past;
mod rest;
mod rides;

pub use care::{Care, Dressed, Treatment};
pub use hits::zone_of;
pub use copters::{Helicopter, PILOT_SEAT, heli_carrier, heli_of};
pub use rest::GunRest;
pub use rides::{OnDeck, SeatEvent, UseTarget};
pub use prs_seats::TransitKind;
use past::PastBodies;

use glam::{Quat, Vec2, Vec3};
pub use prs_ballistics::Bullet;
use prs_ballistics::zero_elevation;
use prs_body::{AnimInput, BodyAnim, BodyConfig, BodyInput, Grips, GroundHit, GunHold, LedgeHold, Pose};
use prs_collision::Capsule;
use prs_health::{Health, HealthConfig, HealthEvent, Zone};
use prs_suppression::{Suppression, SuppressionConfig};
use prs_throwables::{Cloud, ThrowConfig, Thrown, ThrownKind};

pub use grenades::ThrowHold;
use prs_inventory::{Inventory, InventoryConfig, Loadout, Slot, Stash, encumbrance, gun_weight};
use prs_movement::{ClimbKind, LeanInput, MoveConfig, MoveEvent, MoveInput, MoveState, Stance, abort_climb, yaw_basis};
use prs_protocol::{Buttons, PlayerCmd, TICK_DT, TICK_HZ};
use prs_seats::{SeatedAt, Transit};
use prs_vehicles::{VehicleEvent, VehicleState, VehicleTable, VehicleZone};
use prs_weapons::{
    BodyState, GunPose, GunPoseInput, Posture, WeaponConfig, WeaponEvent, WeaponId, WeaponInput, WeaponState,
    WeaponTable, gun_pose, zeroed,
};

pub use prs_body as body;
pub use prs_health as health;
pub use prs_mods;
pub use prs_suppression as suppression;
pub use prs_throwables as throwables;
pub use prs_inventory as inventory;
pub use prs_level::{CapturePoint, Level, Paint, Solid, SpawnPoint, TRUNK_HALF_HEIGHT, TRUNK_HALF_WIDTH, Team, VehicleSpawn};
pub use prs_collision::{CollisionWorld, Surface};
pub use prs_movement as movement;
pub use prs_protocol as protocol;
pub use prs_helicopter as helicopter;
pub use prs_seats as seats;
pub use prs_vehicles as vehicles;
pub use prs_weapons as weapons;

const TARGET_BODY: Capsule = Capsule { height: 1.8, radius: 0.28 };
const GROUND_PROBE_ABOVE: f32 = 0.5;
const GROUND_PROBE_BELOW: f32 = 0.5;
/// How far above the ground someone getting out is put, and how far to look for it if it comes to it.
const EXIT_CLEARANCE: f32 = 0.02;
const EXIT_SEARCH: f32 = 30.0;
/// How close to a hull's top feet count as standing on it rather than being inside it.
const DECK_TOLERANCE: f32 = 0.2;

pub type SoldierId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimEvent {
    ShotFired { soldier: SoldierId, origin: Vec3, dir: Vec3, weapon: WeaponId },
    Impact { point: Vec3, normal: Vec3, surface: Surface },
    TargetHit { target: usize, point: Vec3, headshot: bool },
    /// A bullet found a soldier. `damage` is after the body part it landed on.
    SoldierHit { soldier: SoldierId, shooter: SoldierId, zone: Zone, point: Vec3, damage: f32 },
    /// A bullet just missed a soldier. `point` is where it came closest to them, or where it stopped
    /// if that was nearest. `amount` is what it added to their suppression.
    /// Something thrown left the hand. Everyone flies their own copy from this, as they do a bullet.
    ThrownReleased { soldier: SoldierId, id: u32, kind: ThrownKind, origin: Vec3, vel: Vec3, fuse: f32 },
    /// It hit something and went on.
    ThrownBounced { point: Vec3, speed: f32 },
    /// It went off. Only a server decides that, and what it did.
    ThrownBurst { id: u32, kind: ThrownKind, point: Vec3 },
    BulletPassed { soldier: SoldierId, shooter: SoldierId, point: Vec3, weapon: WeaponId, distance: f32, cracked: bool, amount: f32 },
    /// A change in how a soldier is doing: wounded, bleeding, down, dead, revived.
    Health { soldier: SoldierId, event: HealthEvent },
    Respawned { soldier: SoldierId },
    Move { soldier: SoldierId, event: MoveEvent },
    Weapon { soldier: SoldierId, event: WeaponEvent },
    Carry { soldier: SoldierId, event: CarryEvent },
    KitDropped { kit: u32, pos: Vec3 },
    KitGone { kit: u32 },
    /// Someone got into, moved around or got out of a vehicle.
    Seat { soldier: SoldierId, event: SeatEvent },
    Vehicle { vehicle: usize, event: VehicleEvent },
    /// A bullet found part of a vehicle.
    VehicleHit { vehicle: usize, shooter: SoldierId, zone: VehicleZone, point: Vec3, normal: Vec3 },
    Heli { heli: usize, event: prs_helicopter::HeliEvent },
    /// A bullet found part of a helicopter.
    HeliHit { heli: usize, shooter: SoldierId, zone: prs_helicopter::HeliZone, point: Vec3 },
}

/// What a soldier did with what they carry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarryEvent {
    Selected(Slot),
    /// The gun in the hands is now this one, still on its way up.
    GunChanged(WeaponId),
    KitTaken { kit: u32 },
    TreatmentStarted { care: Care, patient: SoldierId },
    TreatmentFinished { care: Care, patient: SoldierId },
    TreatmentInterrupted { care: Care, patient: SoldierId },
    /// The gun went down on something, or came up off it.
    Rested,
    RestLifted,
}

/// A soldier's kit left on the ground, for anyone to go through.
#[derive(Clone, Debug, PartialEq)]
pub struct DroppedKit {
    pub id: u32,
    pub pos: Vec3,
    pub stash: Stash,
    pub age: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Soldier {
    pub body: MoveState,
    pub weapon: WeaponState,
    pub yaw: f32,
    pub pitch: f32,
    /// Metres an obstacle has pushed the gun back from its normal hold.
    pub gun_retract: f32,
    /// The gun set down on something, if it is.
    pub rest: GunRest,
    /// What the body remembers between ticks. Derived from the rest of this and feeds nothing back.
    pub anim: BodyAnim,
    /// Which side they are on. Nothing but treating each other cares yet: bullets hit everyone.
    pub team: u8,
    pub health: Health,
    /// How much they are being shot at. The server's word, as nobody predicts other people's bullets.
    pub suppression: Suppression,
    /// What they are doing with something to throw.
    pub throw: ThrowHold,
    pub inventory: Inventory,
    pub treatment: Option<Treatment>,
    /// The vehicle they have their hands on, to right it.
    pub lifting: Option<u32>,
    /// The vehicle they are leaning into, and which of its spots.
    pub pushing: Option<(u32, u8)>,
    /// Seconds left of the prompt saying something is in the way of that.
    pub lift_refused: f32,
    /// Where a dressing has been put on them, which stays until they respawn or the wound is gone.
    pub dressed: Dressed,
    /// Who hit them last.
    pub attacker: Option<SoldierId>,
    /// Where they come back after dying, and what with.
    pub home: Vec3,
    pub home_yaw: f32,
    pub loadout: Loadout,
    /// Their weapon's random numbers come from this and how many lives they have had.
    pub seed: u32,
    pub lives: u32,
    /// The trigger has to come up after treating someone before the gun will fire.
    pub hold_fire: bool,
    pub last_buttons: Buttons,
    /// Whether this slot is in the game. A soldier who has not started or has left takes no time
    /// and no bullets. Ids stay put, so a slot waits here for the next player.
    pub present: bool,
    /// The seat they are in, if they are riding. Everything above stays in world space.
    pub seat: Option<SeatedAt>,
    /// On the way through a door to or from that seat, or across to it from another.
    pub transit: Option<Transit>,
    /// Stood on a vehicle's hull, on foot.
    pub deck: Option<OnDeck>,
}

impl Soldier {
    /// Where this soldier sees from, leaning included.
    pub fn eye(&self) -> Vec3 {
        self.body.eye() + self.body.lean.offset(self.yaw) + self.rest.eye
    }

    /// Where this soldier's gun is right now, as anyone watching should draw it.
    pub fn gun_pose(&self, weapons: &WeaponTable) -> GunPose {
        self.gun_pose_at(self.held_config(weapons), self.weapon.aim_offset())
    }

    pub fn held_config<'a>(&self, weapons: &'a WeaponTable) -> &'a WeaponConfig {
        &weapons[self.weapon.weapon]
    }

    /// Kilograms of everything they carry.
    pub fn load(&self, cfg: &InventoryConfig, weapons: &WeaponTable) -> f32 {
        let in_hand = gun_weight(self.weapon.weapon, self.weapon.rounds(), weapons);
        self.inventory.stash.weight(cfg, weapons) + in_hand
    }

    fn gun_pose_at(&self, cfg: &WeaponConfig, aim_offset: Vec2) -> GunPose {
        gun_pose(
            cfg,
            &GunPoseInput {
                eye: self.eye(),
                yaw: self.yaw,
                pitch: self.pitch,
                lean: self.body.lean.amount,
                aim_offset,
                ads: self.weapon.ads,
                ready: self.weapon.ready,
                retract: self.gun_retract,
                fallen: self.anim.collapse,
                slung: self.anim.slung,
                on_back: self.anim.heave,
            },
        )
    }
}

/// How far a gun goes onto its sling for this climb, 0 to 1. A quick hop keeps a hand on it, low, and it
/// takes both hands to get up anything higher.
pub fn sling_for(kind: ClimbKind) -> f32 {
    match kind {
        ClimbKind::None => 0.0,
        ClimbKind::Vault => 0.5,
        ClimbKind::Mantle | ClimbKind::PullUp | ClimbKind::Stool => 1.0,
    }
}

/// How far a gun should be on its sling. Both hands are needed for treating someone, so it goes there
/// for that too, and comes back to the hand when they are.
pub fn sling_goal(climb: ClimbKind, treating: bool) -> f32 {
    if treating { 1.0 } else { sling_for(climb) }
}

/// How far down a soldier's eye has gone, as the body's stance blends: (crouched, lying).
pub fn stance_blend(cfg: &MoveConfig, eye_height: f32) -> (f32, f32) {
    let crouch = (cfg.stand_eye - eye_height) / (cfg.stand_eye - cfg.crouch_eye).max(1e-3);
    let prone = (cfg.crouch_eye - eye_height) / (cfg.crouch_eye - cfg.prone_eye).max(1e-3);
    (crouch.clamp(0.0, 1.0), prone.clamp(0.0, 1.0))
}

/// How far the gun would stick into whatever is in front of it, measured with the gun held ready
/// so that lowering it does not change the answer and make it flicker up and down.
fn barrel_overlap(world: &CollisionWorld, barrel: &ReadyBarrel) -> f32 {
    match world.raycast(barrel.start, barrel.forward, barrel.along) {
        Some(hit) => barrel.along - hit.distance,
        None => 0.0,
    }
}

/// The barrel with the gun held ready, sighted from level with the eye, so a gun cannot be poked
/// through a wall from up close.
struct ReadyBarrel {
    start: Vec3,
    forward: Vec3,
    along: f32,
}

impl ReadyBarrel {
    /// Only the part of it beyond a point it rests on.
    fn ahead_of(&self, pivot: Vec3) -> Self {
        let muzzle = self.start + self.forward * self.along;
        let along = (muzzle - pivot).dot(self.forward).max(0.0);
        Self { start: muzzle - self.forward * along, forward: self.forward, along }
    }
}

fn ready_barrel(cfg: &WeaponConfig, eye: Vec3, yaw: f32, pitch: f32, lean: f32, ads: f32) -> ReadyBarrel {
    let ready = gun_pose(
        cfg,
        &GunPoseInput { eye, yaw, pitch, lean, aim_offset: Vec2::ZERO, ads, ready: 1.0, retract: 0.0, fallen: 0.0, slung: 0.0, on_back: 0.0 },
    );
    let along = (ready.muzzle - eye).dot(ready.forward);
    ReadyBarrel { start: ready.muzzle - ready.forward * along, forward: ready.forward, along }
}

/// How much of a full lean a gun leaves, from the upright eye out to where the barrel would begin.
/// From there on `barrel_overlap` has it, with the gun pulling back like any other time.
fn gun_lean_fit(world: &CollisionWorld, cfg: &WeaponConfig, eye: Vec3, yaw: f32, pitch: f32, ads: f32, full: Vec3, sign: f32) -> f32 {
    let barrel = ready_barrel(cfg, eye + full, yaw, pitch, sign, ads);
    let path = barrel.start - eye;
    let reach = path.length();
    if reach < 1e-4 {
        return 1.0;
    }
    world.raycast(eye, path / reach, reach).map_or(1.0, |hit| hit.distance / reach)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pos: Vec3,
    /// Seconds until it stands back up. 0 means standing.
    pub down_for: f32,
    pub hits: u32,
}

impl Target {
    pub fn is_up(&self) -> bool {
        self.down_for == 0.0
    }
}

pub struct Sim {
    pub tick: u32,
    pub level: Level,
    pub world: CollisionWorld,
    pub move_cfg: MoveConfig,
    pub weapons: WeaponTable,
    pub body_cfg: BodyConfig,
    pub health_cfg: HealthConfig,
    pub suppression_cfg: SuppressionConfig,
    pub throw_cfg: ThrowConfig,
    /// What has been thrown and has not gone off, and smoke that is still hanging about.
    pub thrown: Vec<Thrown>,
    pub clouds: Vec<Cloud>,
    pub world_cfg: WorldConfig,
    pub inventory_cfg: InventoryConfig,
    pub soldiers: Vec<Soldier>,
    pub targets: Vec<Target>,
    pub bullets: Vec<FlyingBullet>,
    pub kits: Vec<DroppedKit>,
    pub vehicle_cfgs: VehicleTable,
    pub vehicles: Vec<VehicleState>,
    /// How far open each way in of each vehicle is, 0 to 1. Apart from the vehicle's own state because
    /// nobody predicts it: it follows from who is going through.
    pub vehicle_doors: Vec<Vec<f32>>,
    pub heli_cfg: prs_helicopter::HeliConfig,
    /// What a person sitting in a helicopter gets in and out by, and what riding does to a gun.
    pub heli_ride: prs_vehicles::RideConfig,
    pub helicopters: Vec<Helicopter>,
    /// The crowns of the trees, which nothing but a rotor can hit.
    canopies: CollisionWorld,
    heli_events: Vec<prs_helicopter::HeliEvent>,
    /// Filled during `tick`, cleared at the start of the next one.
    pub events: Vec<SimEvent>,
    /// Whether the dead wait to be brought back with [`Sim::respawn_at`], rather than coming back on their
    /// own when their clock runs out. A game mode decides when and where, if there is one.
    pub hold_respawns: bool,
    /// Elevation per weapon that puts the bullet on the sights at its zero range.
    zeros: Vec<f32>,
    next_kit: u32,
    move_events: Vec<MoveEvent>,
    vehicle_events: Vec<VehicleEvent>,
    vehicle_pair_events: Vec<(usize, VehicleEvent)>,
    weapon_events: Vec<WeaponEvent>,
    health_events: Vec<HealthEvent>,
    /// Bones worked out this tick, only for soldiers a bullet came close to.
    hit_poses: Vec<Option<Pose>>,
    /// Where everyone was on recent ticks, for hitting them where the shooter saw them.
    past: PastBodies,
    /// How many ticks in the past each soldier is seeing everyone else, as of their last command.
    view_lag: Vec<u32>,
    /// Where everyone was at the start of this tick, for bumping into each other. Worked out afresh
    /// from the soldiers each tick, so it is nothing to export.
    crowd: crowd::Crowd,
    /// The tick everyone else is being shown at, on a client, where that is in the past. Someone walking
    /// is then looked for where they will have got to by now, to bump into them in about the right place.
    pub others_seen_at: Option<f32>,
}

/// A bullet in the air and whose it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyingBullet {
    pub bullet: Bullet,
    pub shooter: SoldierId,
    pub weapon: WeaponId,
    /// How many ticks in the past the shooter saw everyone else when firing. The bullet flies through
    /// that world, so what was on their sights is what it hits. 0 for a sim with one set of eyes.
    pub rewind: u32,
}

/// The few rules of the world that belong to nothing smaller.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorldConfig {
    /// Furthest you can look up or down, in radians.
    pub pitch_limit: f32,
    /// Seconds a range target stays down, and how high on it counts as the head, in metres.
    pub target_down_time: f32,
    pub target_head_height: f32,
    /// How far past a vehicle's panel a bullet that went through picks up again, in metres.
    pub panel_through: f32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self { pitch_limit: 1.54, target_down_time: 4.0, target_head_height: 1.5, panel_through: 0.08 }
    }
}

/// Everything that changes from one tick to the next. Export this on one sim and import it on another
/// with the same level and config, and the two step identically from then on.
#[derive(Clone, Debug, PartialEq)]
pub struct SimState {
    pub tick: u32,
    pub soldiers: Vec<Soldier>,
    pub targets: Vec<Target>,
    pub bullets: Vec<FlyingBullet>,
    pub kits: Vec<DroppedKit>,
    pub next_kit: u32,
    pub vehicles: Vec<VehicleState>,
    pub vehicle_doors: Vec<Vec<f32>>,
    pub helicopters: Vec<Helicopter>,
    pub thrown: Vec<Thrown>,
    pub clouds: Vec<Cloud>,
}

fn zeros_for(weapons: &WeaponTable) -> Vec<f32> {
    weapons
        .weapons
        .iter()
        .map(|w| zero_elevation(&w.ballistics, w.zero_range, w.hold.sight_height))
        .collect()
}

impl Sim {
    /// A sim with whatever the active mod says. A bad mod stops the program with the message saying why.
    pub fn new(level: Level) -> Self {
        Self::with_mod(level, prs_mods::active()).unwrap_or_else(|error| prs_mods::stop(&error))
    }

    /// A sim with this mod laid over the rules.
    pub fn with_mod(level: Level, active: &prs_mods::Mod) -> Result<Self, prs_mods::ModError> {
        let world = level.collision();
        let weapons = active.over("weapons", WeaponTable::default())?;
        let vehicle_cfgs = active.over("vehicles", VehicleTable::default())?;
        let vehicles = Self::park_vehicles(&level, &vehicle_cfgs);
        let vehicle_doors = Self::shut_doors(&vehicles, &vehicle_cfgs);
        let heli_cfg = active.over("helicopter", prs_helicopter::HeliConfig::default())?;
        let helicopters = Self::park_helicopters(&level, &heli_cfg);
        let canopies = level.canopies();
        let targets = level
            .targets
            .iter()
            .map(|&pos| Target { pos, down_for: 0.0, hits: 0 })
            .collect();
        Ok(Self {
            tick: 0,
            zeros: zeros_for(&weapons),
            world,
            level,
            move_cfg: active.over("movement", MoveConfig::default())?,
            weapons,
            body_cfg: active.over("body", BodyConfig::default())?,
            health_cfg: active.over("health", HealthConfig::default())?,
            suppression_cfg: active.over("suppression", SuppressionConfig::default())?,
            throw_cfg: active.over("throwables", ThrowConfig::default())?,
            thrown: Vec::new(),
            clouds: Vec::new(),
            world_cfg: active.over("world", WorldConfig::default())?,
            inventory_cfg: active.over("inventory", InventoryConfig::default())?,
            soldiers: Vec::new(),
            targets,
            bullets: Vec::new(),
            kits: Vec::new(),
            vehicle_cfgs,
            vehicles,
            vehicle_doors,
            heli_cfg,
            heli_ride: active.over("vehicle_ride", copters::default_ride())?,
            helicopters,
            canopies,
            heli_events: Vec::new(),
            vehicle_events: Vec::new(),
            vehicle_pair_events: Vec::new(),
            events: Vec::new(),
            hold_respawns: false,
            next_kit: 0,
            move_events: Vec::new(),
            weapon_events: Vec::new(),
            health_events: Vec::new(),
            hit_poses: Vec::new(),
            past: PastBodies::default(),
            view_lag: Vec::new(),
            crowd: crowd::Crowd::default(),
            others_seen_at: None,
        })
    }

    pub fn export_state(&self) -> SimState {
        SimState {
            tick: self.tick,
            soldiers: self.soldiers.clone(),
            targets: self.targets.clone(),
            bullets: self.bullets.clone(),
            kits: self.kits.clone(),
            next_kit: self.next_kit,
            vehicles: self.vehicles.clone(),
            vehicle_doors: self.vehicle_doors.clone(),
            helicopters: self.helicopters.clone(),
            thrown: self.thrown.clone(),
            clouds: self.clouds.clone(),
        }
    }

    pub fn import_state(&mut self, state: &SimState) {
        self.tick = state.tick;
        self.soldiers.clone_from(&state.soldiers);
        self.targets.clone_from(&state.targets);
        self.bullets.clone_from(&state.bullets);
        self.kits.clone_from(&state.kits);
        self.next_kit = state.next_kit;
        self.vehicles.clone_from(&state.vehicles);
        self.vehicle_doors.clone_from(&state.vehicle_doors);
        self.helicopters.clone_from(&state.helicopters);
        self.thrown.clone_from(&state.thrown);
        self.clouds.clone_from(&state.clouds);
    }

    /// Changes the guns in the game, as a mod would. Do this before anyone spawns.
    pub fn set_weapons(&mut self, weapons: WeaponTable) {
        self.zeros = zeros_for(&weapons);
        self.weapons = weapons;
    }

    /// A soldier at the level's spawn with the standard kit.
    pub fn spawn_soldier(&mut self, seed: u32) -> SoldierId {
        let loadout = self.inventory_cfg.loadout;
        self.spawn_soldier_with(seed, 0, loadout)
    }

    pub fn spawn_soldier_with(&mut self, seed: u32, team: u8, loadout: Loadout) -> SoldierId {
        let (home, home_yaw) = (self.level.spawn, self.level.spawn_yaw);
        let (inventory, gun) = Inventory::issue(&loadout, &self.weapons);
        let weapon = WeaponState::holding(gun, &self.weapons[loadout.primary], seed);
        let soldier = Soldier {
            body: MoveState::new(home, &self.move_cfg),
            weapon,
            yaw: home_yaw,
            pitch: 0.0,
            gun_retract: 0.0,
            rest: GunRest::default(),
            anim: BodyAnim::new(home_yaw),
            team,
            health: Health::new(&self.health_cfg),
            suppression: Suppression::default(),
            throw: ThrowHold::default(),
            inventory,
            treatment: None,
            lifting: None,
            pushing: None,
            lift_refused: 0.0,
            dressed: Dressed::default(),
            attacker: None,
            home,
            home_yaw,
            loadout,
            seed,
            lives: 0,
            hold_fire: false,
            last_buttons: Buttons::default(),
            present: true,
            seat: None,
            transit: None,
            deck: None,
        };
        self.soldiers.push(soldier);
        self.soldiers.len() - 1
    }

    /// Moves a soldier and where they come back to.
    pub fn set_home(&mut self, id: SoldierId, pos: Vec3, yaw: f32) {
        let soldier = &mut self.soldiers[id];
        soldier.home = pos;
        soldier.home_yaw = yaw;
        soldier.body.pos = pos;
        soldier.yaw = yaw;
    }

    /// Brings a soldier in somewhere, with this kit, whatever state they were in.
    pub fn respawn_at(&mut self, id: SoldierId, pos: Vec3, yaw: f32, loadout: Loadout) {
        let soldier = &mut self.soldiers[id];
        soldier.home = pos;
        soldier.home_yaw = yaw;
        soldier.loadout = loadout;
        soldier.present = true;
        self.respawn(id);
    }

    /// Back at their spawn with a new kit, as if for the first time.
    pub fn respawn(&mut self, id: SoldierId) {
        let soldier = &self.soldiers[id];
        let (home, home_yaw, loadout) = (soldier.home, soldier.home_yaw, soldier.loadout);
        let lives = soldier.lives + 1;
        let seed = soldier.seed.wrapping_add(lives.wrapping_mul(0x9E37_79B1));
        let (inventory, gun) = Inventory::issue(&loadout, &self.weapons);
        let weapon = WeaponState::holding(gun, &self.weapons[loadout.primary], seed);
        let soldier = &mut self.soldiers[id];
        soldier.body = MoveState::new(home, &self.move_cfg);
        soldier.weapon = weapon;
        soldier.yaw = home_yaw;
        soldier.pitch = 0.0;
        soldier.gun_retract = 0.0;
        soldier.rest = GunRest::default();
        soldier.anim = BodyAnim::new(home_yaw);
        soldier.health = Health::new(&self.health_cfg);
        soldier.suppression = Suppression::default();
        soldier.inventory = inventory;
        soldier.treatment = None;
        soldier.lifting = None;
        soldier.pushing = None;
        soldier.lift_refused = 0.0;
        soldier.dressed = Dressed::default();
        soldier.attacker = None;
        soldier.lives = lives;
        soldier.hold_fire = false;
        soldier.seat = None;
        soldier.transit = None;
        soldier.deck = None;
        self.events.push(SimEvent::Respawned { soldier: id });
    }

    /// A soldier's state as the body needs it, with the gun exactly where `gun_pose` says.
    pub fn body_input(&self, id: SoldierId) -> BodyInput {
        let soldier = &self.soldiers[id];
        let body = &soldier.body;
        let cfg = soldier.held_config(&self.weapons);
        let gun = soldier.gun_pose(&self.weapons);
        let g = &cfg.hold.grips;
        let (crouch, prone) = stance_blend(&self.move_cfg, body.eye_height);
        BodyInput {
            feet: body.pos,
            eye: soldier.eye(),
            yaw: soldier.yaw,
            pitch: soldier.pitch,
            head_turn: Vec2::ZERO,
            lean: body.lean.amount,
            throw: (soldier.throw.held / self.throw_cfg.windup.max(1e-3)).min(1.0),
            hand_at: None,
            velocity: body.vel,
            grounded: body.grounded,
            stride: body.stride,
            stride_length: self.move_cfg.stride_length(body.stance, body.sprinting),
            crouch,
            prone,
            ads: soldier.weapon.ads,
            ready: soldier.weapon.ready,
            reload: soldier.weapon.reload_progress(cfg),
            limp: soldier.health.leg_wound,
            gun: GunHold {
                origin: gun.origin,
                rotation: gun.rotation,
                grips: Grips {
                    trigger: g.trigger,
                    trigger_wrist: g.trigger_wrist,
                    support: g.support,
                    support_wrist: g.support_wrist,
                    magazine: g.magazine,
                },
            },
            anim: soldier.anim,
            sit: soldier.seat.map(|at| self.sit_pose(at, id, soldier.yaw)),
            ledge: body.climb.active().then(|| LedgeHold {
                ledge: body.climb.hands,
                yaw: body.climb.yaw,
                grip: body.climb.grip(&self.move_cfg.climb),
                both: !matches!(body.climb.kind, ClimbKind::Vault),
            }),
            perched: body.perch.on.is_some(),
            first_person: false,
            push: match (soldier.lifting, soldier.pushing) {
                (_, Some((v, spot))) => self.push_hands(v as usize, spot),
                (Some(v), _) if (v as usize) < self.vehicles.len() => Some(self.lift_hands(id, v as usize)),
                _ => None,
            },
        }
    }

    /// The bones for a body input on this level's ground. Pure, so anyone gets the same answer.
    pub fn body_pose(&self, input: &BodyInput) -> Pose {
        let ground = |at: Vec3| {
            // From just above the soldier's own reach down past it, so standing next to a wall the
            // top of the wall does not count as ground, and starting inside something gives nothing.
            let top = Vec3::new(at.x, input.feet.y + GROUND_PROBE_ABOVE, at.z);
            self.world
                .raycast(top, Vec3::NEG_Y, GROUND_PROBE_ABOVE + GROUND_PROBE_BELOW)
                .filter(|hit| hit.distance > 0.0)
                .map(|hit| GroundHit { height: top.y - hit.distance, normal: hit.normal })
        };
        prs_body::solve(&self.body_cfg, input, &ground)
    }

    /// Advances one tick. `cmds` holds one command per soldier that acted this tick. Anyone else
    /// stands where they are with their hands off everything, and still bleeds.
    pub fn step(&mut self, cmds: &[(SoldierId, PlayerCmd)]) {
        self.events.clear();
        self.tick += 1;
        self.step_vehicles(cmds);
        self.take_down_crowd(self.tick);
        for id in 0..self.soldiers.len() {
            let soldier = &self.soldiers[id];
            if !soldier.present {
                continue;
            }
            // Told nothing, whoever is in a seat goes on looking the same way out of it.
            let carried = self.ride_for(id).map_or(0.0, |ride| ride.carried_yaw());
            let soldier = &self.soldiers[id];
            let idle = PlayerCmd { tick: self.tick, yaw: soldier.yaw + carried, pitch: soldier.pitch, ..Default::default() };
            let cmd = cmds.iter().find(|(who, _)| *who == id).map_or(idle, |(_, cmd)| *cmd);
            self.step_soldier(id, &cmd);
        }
        self.step_bullets();
        self.step_thrown();
        self.step_vitals();
        self.step_kits();
        for target in &mut self.targets {
            target.down_for = (target.down_for - TICK_DT).max(0.0);
        }
        self.remember_bodies();
    }

    /// One tick of a single soldier, for a client predicting its own. Nothing else in the world moves,
    /// and bullets are left for [`Self::fly_tracers`], because only a server decides what they hit.
    pub fn step_predicted(&mut self, id: SoldierId, cmd: &PlayerCmd) {
        self.events.clear();
        self.tick = cmd.tick;
        // Not in the game yet, so on the server nothing is happening to them either.
        if !self.soldiers[id].present {
            return;
        }
        self.step_predicted_vehicle(id, cmd);
        self.sync_movers();
        self.take_down_crowd(cmd.tick);
        self.step_soldier(id, cmd);
        self.step_vitals_of(id);
    }

    /// A bullet a client has been told about, to draw.
    pub fn add_tracer(&mut self, shooter: SoldierId, origin: Vec3, dir: Vec3, weapon: WeaponId) {
        let bullet = Bullet::new(origin, dir, &self.weapons[weapon].ballistics);
        self.bullets.push(FlyingBullet { bullet, shooter, weapon, rewind: 0 });
    }

    /// Bullets as a client draws them: they stop at walls and hit nothing and nobody.
    pub fn fly_tracers(&mut self) {
        let world = &self.world;
        let (vehicles, vehicle_doors, vehicle_cfgs) = (&self.vehicles, &self.vehicle_doors, &self.vehicle_cfgs);
        self.bullets.retain_mut(|flying| {
            let (from, to) = flying.bullet.advance(TICK_DT);
            let span = to - from;
            let length = span.length();
            let wall = length > 0.0 && world.raycast(from, span / length, length).is_some();
            let armour = length > 0.0 && vehicles.iter().zip(vehicle_doors).any(|(vehicle, doors)| {
                let found = prs_vehicles::strike(vehicle, &vehicle_cfgs[vehicle.kind], doors, &vehicle.frame(), from, span / length, length);
                found.is_some_and(|strike| strike.pass <= 0.0)
            });
            !wall && !armour && !flying.bullet.expired()
        });
    }

    fn step_soldier(&mut self, id: SoldierId, cmd: &PlayerCmd) {
        let soldier = &mut self.soldiers[id];
        let up = soldier.health.is_up();
        // Someone who is down can look around but asks nothing of their body.
        let buttons = if up { cmd.buttons } else { Buttons::default() };
        let wish = if up { cmd.wish_move } else { Vec2::ZERO };
        let last = soldier.last_buttons;
        let pressed = |b: Buttons| buttons.contains(b) && !last.contains(b);
        // F does what you are at and facing, before anything else this tick: a door or a kit, or the way out.
        let mut take_kit = false;
        if pressed(Buttons::USE) && !soldier.body.climb.active() {
            if soldier.seat.is_some() {
                self.try_leave(id);
            } else {
                match self.use_target_looking(id, view_dir(cmd.yaw, cmd.pitch)) {
                    Some(UseTarget::Door { carrier, entry, seat }) => self.start_entering(id, carrier, entry, seat),
                    Some(UseTarget::Kit(_)) => take_kit = true,
                    Some(UseTarget::Lift { vehicle }) => self.start_lifting(id, vehicle),
                    Some(UseTarget::Push { vehicle, spot }) => self.start_pushing(id, vehicle, spot),
                    None => {}
                }
            }
        }
        self.step_lift(id, buttons);
        self.step_push(id, buttons);
        // Hands on a vehicle, they can still shuffle about, slowly.
        let hands_on = self.soldiers[id].lifting.and_then(|v| self.vehicles.get(v as usize)).map(|v| self.vehicle_cfgs[v.kind].lift.walk_share)
            .or(self.soldiers[id].pushing.and_then(|(v, _)| self.vehicles.get(v as usize)).map(|v| self.vehicle_cfgs[v.kind].push.walk_share))
            .unwrap_or(1.0);
        self.step_transit(id);
        // In a seat the number keys are the seats, and nobody changes kit.
        if self.soldiers[id].seat.is_some() {
            let keys = [Buttons::SLOT_1, Buttons::SLOT_2, Buttons::SLOT_3, Buttons::SLOT_4];
            if let Some(seat) = keys.iter().position(|&key| pressed(key)) {
                self.swap_seat(id, seat);
            }
        }
        self.check_mount(id);
        self.check_boosted(id);
        let ride = self.ride_for(id);
        // Before they move, so a hull that has driven into them finds them there.
        if ride.is_none() && self.soldiers[id].pushing.is_none() {
            let was = self.soldiers[id].body.pos;
            self.keep_off_vehicles(id);
            // Something drove into the way of a climb, and whatever pushed them off it has made them let go.
            if self.soldiers[id].body.climb.active() && self.soldiers[id].body.pos != was {
                abort_climb(&mut self.soldiers[id].body, &self.move_cfg, &self.world);
            }
        }
        let crowding = if ride.is_none() { self.crowding_of(id) } else { Default::default() };
        let stool = pressed(Buttons::JUMP).then(|| self.stool_ahead(id, cmd.yaw)).flatten();
        let carrying = self.is_carrying(id);
        let hurt = self.climb_hurt(&self.soldiers[id]);
        let soldier = &mut self.soldiers[id];
        soldier.last_buttons = cmd.buttons;

        // A body on the ground keeps the gun as it fell, and the view is on its own.
        let (yaw, pitch) = if up {
            let (yaw, pitch) = (cmd.yaw, cmd.pitch.clamp(-self.world_cfg.pitch_limit, self.world_cfg.pitch_limit));
            ride.map_or((yaw, pitch), |ride| ride.look(yaw, pitch))
        } else {
            (soldier.yaw, soldier.pitch * (1.0 - TICK_DT))
        };
        let turn_rate = if soldier.anim.collapse > 0.0 {
            0.0
        } else {
            let carried = ride.map_or(0.0, |ride| ride.carried_yaw());
            let own = Vec2::new(yaw - soldier.yaw - carried, pitch - soldier.pitch).length().to_degrees() / TICK_DT;
            own + ride.map_or(0.0, |ride| ride.jolt_rate())
        };
        soldier.yaw = yaw;
        soldier.pitch = pitch;

        for (button, slot) in [
            (Buttons::SLOT_1, Slot::Primary),
            (Buttons::SLOT_2, Slot::Sidearm),
            (Buttons::SLOT_3, Slot::Bandage),
            (Buttons::SLOT_4, Slot::Medkit),
        ] {
            if ride.is_none() && pressed(button) && soldier.inventory.select(slot) {
                self.events.push(SimEvent::Carry { soldier: id, event: CarryEvent::Selected(slot) });
            }
        }

        // The gun has to be all the way down before it can be changed for another.
        if let Some(slot) = soldier.inventory.gun_wanted()
            && soldier.weapon.ready <= 0.0
        {
            let current = soldier.weapon.gun();
            if let Some(next) = soldier.inventory.exchange_held(slot, current) {
                let weapon = next.weapon;
                soldier.weapon.put_in_hands(next);
                self.events.push(SimEvent::Carry { soldier: id, event: CarryEvent::GunChanged(weapon) });
            }
        }
        if ride.is_none()
            && pressed(Buttons::GRENADE)
            && let Some(slot) = soldier.inventory.next_throwable()
            && soldier.inventory.select(slot)
        {
            self.events.push(SimEvent::Carry { soldier: id, event: CarryEvent::Selected(slot) });
        }
        let using_item = !soldier.inventory.selected.is_gun();
        let lifting = soldier.lifting.is_some() || soldier.pushing.is_some();
        let hands_full = lifting || ride.is_some_and(|ride| ride.hands_full() || ride.busy());
        let climbing = soldier.body.climb.active();
        // B needs a free hand and something to do with it. With nothing to bandage it does nothing at all.
        let bandaging = up
            && buttons.contains(Buttons::BANDAGE)
            && soldier.health.wants_bandage()
            && soldier.inventory.stash.bandages > 0
            && soldier.transit.is_none();
        let stowing = !up || using_item || soldier.inventory.gun_wanted().is_some() || hands_full || climbing || bandaging;
        if !buttons.contains(Buttons::FIRE) {
            soldier.hold_fire = false;
        }
        let trigger = buttons.contains(Buttons::FIRE) && !using_item && !soldier.hold_fire && !hands_full && !climbing && !bandaging;

        self.move_events.clear();
        // A bandage on yourself goes at the pace of aiming, in whatever stance you are in.
        let bandaging_self = self.health_cfg.bandage_on_the_move
            && (bandaging || soldier.treatment.is_some_and(|t| t.care == Care::Bandage && t.patient == id));
        let aiming = (buttons.contains(Buttons::ADS) && soldier.weapon.ads > 0.0) || bandaging_self;
        let load = soldier.load(&self.inventory_cfg, &self.weapons);
        let (burdened, extra_drain) = encumbrance(load, &self.inventory_cfg);
        let health_cfg = &self.health_cfg;
        let move_input = MoveInput {
            wish,
            yaw,
            jump_pressed: pressed(Buttons::JUMP),
            sprint_held: buttons.contains(Buttons::SPRINT) && hands_on >= 1.0,
            crouch_pressed: pressed(Buttons::CROUCH),
            prone_pressed: pressed(Buttons::PRONE),
            aiming,
            slowed: (1.0 - (1.0 - burdened) * (1.0 - soldier.health.slowed(health_cfg))) * hands_on,
            extra_drain,
            no_sprint: !soldier.health.can_sprint(health_cfg),
            collapsing: !up,
            hurt,
            burden: burdened,
            busy_hands: soldier.weapon.reload_timer > 0.0 || soldier.treatment.is_some(),
            stool,
            carrying,
            crowd: crowding,
        };
        let cfg = &self.move_cfg;
        match ride {
            Some(ride) => ride.place(&mut soldier.body, cfg),
            None => {
                prs_movement::step(&mut soldier.body, cfg, &move_input, &self.world, TICK_DT, &mut self.move_events);
            }
        }
        self.hold_pusher(id);
        self.hold_lifter(id);
        let soldier = &mut self.soldiers[id];
        let cfg = &self.move_cfg;

        let weapon_cfg = &self.weapons[soldier.weapon.weapon];
        let world = &self.world;
        let (upright, ads) = (soldier.body.eye(), soldier.weapon.ads);
        let gun_fit = |full: Vec3, sign: f32| gun_lean_fit(world, weapon_cfg, upright, yaw, pitch, ads, full, sign);
        let lean_input = LeanInput {
            left_pressed: pressed(Buttons::LEAN_LEFT) && ride.is_none(),
            right_pressed: pressed(Buttons::LEAN_RIGHT) && ride.is_none(),
            yaw,
            up: up && !soldier.body.climb.active(),
            gun_fit: &gun_fit,
        };
        if ride.is_none() {
            prs_movement::step_lean(&mut soldier.body, cfg, &lean_input, world, TICK_DT);
        }

        let can_hold = up && ride.is_none() && !using_item && !hands_full && !bandaging && soldier.inventory.gun_wanted().is_none() && !soldier.body.climb.active() && soldier.transit.is_none();
        self.step_rest(id, pressed(Buttons::REST), yaw, pitch, can_hold);
        let soldier = &mut self.soldiers[id];
        let weapon_cfg = &self.weapons[soldier.weapon.weapon];
        let world = &self.world;
        let cfg = &self.move_cfg;
        let health_cfg = &self.health_cfg;

        let eye = soldier.eye();
        let lean = soldier.body.lean.amount;
        let mut barrel = ready_barrel(weapon_cfg, eye, yaw, pitch, lean, ads);
        // Set down, the part behind the contact has only to be clear of what stands up beside it, and
        // what is ahead of it has to be clear altogether.
        let mut frame_in_way = false;
        if soldier.rest.on && soldier.rest.amount >= 1.0 {
            let ahead = barrel.ahead_of(soldier.rest.pivot);
            let lift = Vec3::Y * weapon_cfg.rest.frame_lift;
            let behind = ahead.start - barrel.start;
            frame_in_way = behind.length() > 1e-3 && world.walk_ray(barrel.start + lift, behind.normalize(), behind.length()).is_some();
            barrel = ahead;
        }
        let on_vehicle = rides::barrel_on_vehicles(&self.vehicles, &self.vehicle_doors, &self.vehicle_cfgs, barrel.start, barrel.forward, barrel.along);
        let wanted_retract = barrel_overlap(world, &barrel).max(on_vehicle);
        let hold = &weapon_cfg.hold;
        let reach = hold.retract_speed * TICK_DT;
        let retract_target =
            if wanted_retract > hold.max_retract { hold.blocked_retract } else { wanted_retract };
        soldier.gun_retract += (retract_target - soldier.gun_retract).clamp(-reach, reach);
        // Blocked if pulling back is not enough, or has not caught up yet: the muzzle must never
        // be inside a wall when the trigger breaks.
        let muzzle_blocked = frame_in_way
            || wanted_retract > hold.max_retract
            || wanted_retract > soldier.gun_retract + 0.03
            || ride.is_some_and(|ride| !ride.gun_bears(yaw, pitch));

        let body = &soldier.body;
        let climbing = climbing || body.climb.active();
        let weapon_input = WeaponInput {
            fire_held: trigger,
            ads_held: buttons.contains(Buttons::ADS) && !using_item && !climbing,
            reload_pressed: pressed(Buttons::RELOAD) && !using_item && !climbing,
            // Only with a gun you can use where you are: at the controls there is nothing to switch.
            mode_pressed: pressed(Buttons::FIRE_MODE) && !hands_full,
            breath_held: buttons.contains(Buttons::SPRINT),
            turn_rate,
            muzzle_blocked,
            stowing,
            body: BodyState {
                posture: match body.stance {
                    Stance::Stand => Posture::Stand,
                    Stance::Crouch => Posture::Crouch,
                    Stance::Prone => Posture::Prone,
                },
                speed_fraction: ride.map_or(body.horizontal_speed() / cfg.run_speed, |ride| ride.speed_fraction()),
                airborne: ride.map_or(!body.grounded, |ride| ride.airborne),
                sprinting: body.sprinting,
                changing_stance: body.stance_timer > 0.0,
                lean_rate: body.lean.rate.abs(),
                stamina: body.stamina,
                arm_wound: soldier.health.arm_wound,
                blood_loss: soldier.health.blood_loss(health_cfg),
                rest: soldier.rest.settle * soldier.rest.settle * (3.0 - 2.0 * soldier.rest.settle),
                suppression: soldier.suppression.level,
            },
        };
        self.weapon_events.clear();
        prs_weapons::step(&mut soldier.weapon, weapon_cfg, &weapon_input, TICK_DT, &mut self.weapon_events);

        let (crouch, prone) = stance_blend(cfg, soldier.body.eye_height);
        let anim_input = AnimInput {
            yaw: soldier.yaw,
            velocity: if ride.is_some() { Vec3::ZERO } else { soldier.body.vel },
            grounded: soldier.body.grounded,
            crouch,
            prone,
            down: !up,
        };
        soldier.anim.step(&self.body_cfg, &anim_input, TICK_DT);
        soldier.anim.step_brace(&self.body_cfg, carrying, TICK_DT);
        soldier.anim.step_heave(&self.body_cfg, lifting && soldier.weapon.ready <= 0.0, TICK_DT);
        let slung = sling_goal(soldier.body.climb.kind, (soldier.treatment.is_some() || lifting) && soldier.weapon.ready <= 0.0);
        soldier.anim.step_sling(&self.body_cfg, slung, TICK_DT);
        if let Some(ride) = ride {
            soldier.anim.hip_yaw = if ride.seat.swivel { soldier.yaw } else { ride.frame.yaw() + ride.seat.facing };
        }

        for &event in &self.move_events {
            self.events.push(SimEvent::Move { soldier: id, event });
        }
        let (yaw, pitch) = (soldier.yaw, soldier.pitch);
        let weapon = soldier.weapon.weapon;
        for &event in &self.weapon_events {
            if let WeaponEvent::Fired { gun_offset, direction_offset } = event {
                let rest = &mut self.soldiers[id].rest;
                rest.settle = (rest.settle - weapon_cfg.rest.shot_unsettles).max(0.0);
                let shooter = &self.soldiers[id];
                let origin = shooter.gun_pose_at(weapon_cfg, gun_offset).muzzle;
                let zero = zeroed(weapon_cfg, shooter.body.lean.amount, self.zeros[weapon.0 as usize]);
                let dir = view_dir(yaw + direction_offset.x + zero.x, pitch + direction_offset.y + zero.y);
                let rewind = self.view_lag.get(id).copied().unwrap_or(0);
                let origin = self.shot_origin(id, origin, rewind);
                let bullet = Bullet::new(origin, dir, &weapon_cfg.ballistics);
                self.bullets.push(FlyingBullet { bullet, shooter: id, weapon, rewind });
                self.events.push(SimEvent::ShotFired { soldier: id, origin, dir, weapon });
            }
            self.events.push(SimEvent::Weapon { soldier: id, event });
        }

        self.note_deck(id);
        if ride.is_none() && take_kit {
            self.take_nearby_kit(id);
        }
        self.step_care(id, buttons.contains(Buttons::FIRE), buttons.contains(Buttons::BANDAGE));
        let treating = self.care_pose(id);
        self.soldiers[id].anim.step_care(&self.body_cfg, treating, TICK_DT);
        self.step_throw(id, buttons, up);
    }

    /// The nearest kit a soldier can reach from where they stand.
    pub fn kit_in_reach(&self, id: SoldierId) -> Option<usize> {
        let pos = self.soldiers[id].body.pos;
        self.kits
            .iter()
            .enumerate()
            .map(|(i, kit)| (i, kit.pos.distance(pos)))
            .filter(|(_, distance)| *distance <= self.inventory_cfg.pickup_reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    fn take_nearby_kit(&mut self, id: SoldierId) {
        let Some(index) = self.kit_in_reach(id) else {
            return;
        };
        let soldier = &mut self.soldiers[id];
        let kit = &mut self.kits[index];
        let kit_id = kit.id;
        let before = soldier.weapon.weapon;
        let gun = soldier.inventory.take_kit(&mut kit.stash, soldier.weapon.gun(), &self.inventory_cfg);
        if gun.weapon == before {
            soldier.weapon.ammo = gun.ammo;
            soldier.weapon.magazines = gun.magazines;
        } else {
            let weapon = gun.weapon;
            soldier.weapon.put_in_hands(gun);
            self.events.push(SimEvent::Carry { soldier: id, event: CarryEvent::GunChanged(weapon) });
        }
        self.events.push(SimEvent::Carry { soldier: id, event: CarryEvent::KitTaken { kit: kit_id } });
        if self.kits[index].stash.is_empty() {
            self.kits.swap_remove(index);
            self.events.push(SimEvent::KitGone { kit: kit_id });
        }
    }

    /// Everyone's wounds and clocks, for everyone, whether they acted this tick or not.
    fn step_vitals(&mut self) {
        for id in 0..self.soldiers.len() {
            if self.soldiers[id].present {
                self.step_vitals_of(id);
            }
        }
    }

    fn step_vitals_of(&mut self, id: SoldierId) {
        self.health_events.clear();
        let held = self.health_cfg.bandage_holds_bleeding
            && self.soldiers[id].health.bleeding()
            && self.soldiers.iter().any(|s| s.present && s.treatment.is_some_and(|t| t.care == Care::Bandage && t.patient == id));
        let due = self.soldiers[id].health.step_held(&self.health_cfg, TICK_DT, held, &mut self.health_events);
        self.soldiers[id].suppression.step(&self.suppression_cfg, TICK_DT);
        self.settle_health(id);
        if due && !self.hold_respawns {
            self.respawn(id);
        }
    }

    /// Hurts a soldier directly, as a bullet landing on this part of them would. For scripted scenes
    /// and anything else that hurts without a bullet.
    pub fn hurt(&mut self, id: SoldierId, zone: Zone, raw: f32) {
        self.health_events.clear();
        self.soldiers[id].health.hit(&self.health_cfg, zone, raw, &mut self.health_events);
        self.interrupt_hands(id);
        self.interrupt_transit(id);
        self.settle_health(id);
    }

    /// A knock with nothing sharp in it: a crash, a fall, being run down, a fire.
    pub fn knock(&mut self, id: SoldierId, raw: f32) {
        self.health_events.clear();
        self.soldiers[id].health.knock(&self.health_cfg, raw, &mut self.health_events);
        self.interrupt_hands(id);
        self.interrupt_transit(id);
        self.settle_health(id);
    }

    /// Passes on what just happened to a soldier's health, and deals with what follows from it.
    fn settle_health(&mut self, id: SoldierId) {
        for i in 0..self.health_events.len() {
            let event = self.health_events[i];
            self.events.push(SimEvent::Health { soldier: id, event });
            match event {
                HealthEvent::Downed => {
                    self.interrupt_hands(id);
                    self.eject(id);
                }
                HealthEvent::Died => {
                    self.interrupt_hands(id);
                    self.eject(id);
                    self.drop_kit(id);
                }
                _ => {}
            }
        }
        self.health_events.clear();
    }

    /// Everything they had goes on the ground where they fell.
    fn drop_kit(&mut self, id: SoldierId) {
        let soldier = &mut self.soldiers[id];
        let stash = soldier.inventory.empty_out(soldier.weapon.gun());
        soldier.weapon.ammo = 0;
        soldier.weapon.magazines.clear();
        let feet = soldier.body.pos;
        let above = feet + Vec3::Y * GROUND_PROBE_ABOVE;
        let pos = self
            .world
            .raycast(above, Vec3::NEG_Y, self.inventory_cfg.pickup_reach + GROUND_PROBE_ABOVE)
            .map_or(feet, |hit| above + Vec3::NEG_Y * hit.distance);
        // Beside the body, so as not to lie under it.
        let pos = pos + yaw_basis(soldier.anim.fall_yaw).1 * self.inventory_cfg.kit_beside_body;
        let kit = self.next_kit;
        self.next_kit += 1;
        self.kits.push(DroppedKit { id: kit, pos, stash, age: 0.0 });
        self.events.push(SimEvent::KitDropped { kit, pos });
    }

    fn step_kits(&mut self) {
        let lifetime = self.inventory_cfg.kit_lifetime;
        let events = &mut self.events;
        self.kits.retain_mut(|kit| {
            kit.age += TICK_DT;
            let keep = kit.age < lifetime;
            if !keep {
                events.push(SimEvent::KitGone { kit: kit.id });
            }
            keep
        });
    }
}

/// Unit vector for a yaw and pitch, matching the camera: yaw 0 pitch 0 looks down -Z.
pub fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    let (forward, _) = yaw_basis(yaw);
    forward * pitch.cos() + Vec3::Y * pitch.sin()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod crowd_tests;
#[cfg(test)]
mod rest_tests;
#[cfg(test)]
mod copters_tests;
#[cfg(test)]
mod rides_tests;
