//! What a vehicle is, as data. The numbers here are what make one feel the way it does, and the tests
//! in `tests.rs` hold the utility truck to what it is meant to do.

use glam::{Quat, Vec2, Vec3};
use prs_collision::Surface;
use prs_seats::{Arc, DoorPanel, Entry, SeatConfig, SeatLink, SeatRole};

use crate::VehicleKind;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WheelConfig {
    /// Where the axle sits with the suspension fully compressed, in the chassis's frame. The chassis
    /// frame's origin is its centre of mass.
    pub mount: Vec3,
    pub radius: f32,
    pub width: f32,
    /// How far down from `mount` the axle can hang.
    pub travel: f32,
    /// Whether this wheel turns with the steering.
    pub steers: bool,
    pub driven: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Suspension {
    /// Newtons per metre, per wheel.
    pub stiffness: f32,
    /// Newtons per metre a second, per wheel.
    pub damping: f32,
    /// Newtons per metre of difference across an axle, holding the body level through a corner.
    pub anti_roll: f32,
    /// Which wheels share an axle, for that.
    pub axles: [(u8, u8); 2],
    /// Share of a bottomed-out wheel's depth corrected each substep.
    pub bump_correction: f32,
}

/// How much grip and drag one kind of ground has.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroundGrip {
    pub grip: f32,
    /// Rolling resistance, as a share of the load.
    pub rolling: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tyres {
    pub concrete: GroundGrip,
    pub gravel: GroundGrip,
    pub dirt: GroundGrip,
    pub grass: GroundGrip,
    /// Anything else: wood, metal.
    pub other: GroundGrip,
    /// Below this speed a tyre simply holds, as a parked vehicle's does.
    pub static_speed: f32,
    /// Shape of grip against slip angle, the usual B and C of the magic formula.
    pub slip_stiffness: f32,
    pub slip_shape: f32,
    /// However much is going into driving or braking, a tyre keeps this share of its grip for corners.
    pub lateral_reserve: f32,
    /// Spinning up past grip: metres a second of slip per unit of force over the limit.
    pub spin_gain: f32,
    /// A flat tyre: its share of grip, its extra rolling drag, and how much lower the axle rides.
    pub flat_grip: f32,
    pub flat_rolling: f32,
    pub flat_drop: f32,
}

impl Tyres {
    pub fn on(&self, surface: Surface) -> GroundGrip {
        match surface {
            Surface::Concrete => self.concrete,
            Surface::Gravel => self.gravel,
            Surface::Dirt => self.dirt,
            Surface::Grass => self.grass,
            Surface::Wood | Surface::Metal | Surface::Flesh => self.other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Engine {
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    /// How high the engine runs on the clutch from a standing start, at full throttle.
    pub launch_rpm: f32,
    /// Share of peak torque at each rpm. Read between points, and nothing past the last.
    pub curve: Vec<Vec2>,
    pub peak_torque: f32,
    pub gears: Vec<f32>,
    pub reverse: f32,
    pub final_drive: f32,
    pub shift_up_rpm: f32,
    pub shift_down_rpm: f32,
    pub shift_time: f32,
    /// Having changed gear by hand: if set, back to automatic on stopping, and otherwise for as long as
    /// anyone is at the wheel.
    pub manual_until_stop: bool,
    /// How quickly the engine's speed follows the wheels, per second.
    pub rpm_rate: f32,
    /// Newtons of drag at the wheels with the throttle shut.
    pub engine_brake: f32,
    /// How quickly the throttle pedal follows the driver, per second.
    pub throttle_rate: f32,
    /// A freely spinning wheel off the ground: its moment of inertia.
    pub wheel_inertia: f32,
    pub wheel_air_drag: f32,
}

impl Engine {
    pub fn torque_at(&self, rpm: f32) -> f32 {
        let first = self.curve[0];
        if rpm <= first.x {
            return first.y * self.peak_torque;
        }
        for pair in self.curve.windows(2) {
            if rpm <= pair[1].x {
                let t = (rpm - pair[0].x) / (pair[1].x - pair[0].x).max(1.0);
                return (pair[0].y + (pair[1].y - pair[0].y) * t) * self.peak_torque;
            }
        }
        0.0
    }

    /// The gearing between engine and wheel in a gear.
    pub fn ratio(&self, gear: i8) -> f32 {
        let box_ratio = if gear < 0 { self.reverse } else { self.gears[(gear.max(1) - 1) as usize] };
        box_ratio * self.final_drive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Brakes {
    /// Newtons at each wheel with the pedal down.
    pub per_wheel: f32,
    /// The share of that at the wheels that do not steer, which carry less under braking, so that the front locks
    /// first as on any real vehicle and it goes straight on rather than swapping ends.
    pub rear_share: f32,
    /// Newtons at each wheel holding it parked.
    pub parked: f32,
    /// Newtons of handbrake at each wheel, steered and not, the ones behind enough to lock them.
    pub handbrake_front: f32,
    pub handbrake_rear: f32,
    /// Slower than this, the throttle the other way stops it first and then goes.
    pub reverse_below: f32,
    /// With no input and slower than this, it holds itself still.
    pub hold_below: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Steering {
    /// The furthest the wheels turn, at any speed, in radians.
    pub max_angle: f32,
    /// Radians per second.
    pub rate: f32,
    pub return_rate: f32,
    /// How far the steering wheel turns for each radian of the road wheels, and how far back from flat it
    /// leans towards the driver. Drawn, and the hands on it go with it as far as `hands_follow`.
    pub wheel_ratio: f32,
    pub wheel_tilt: f32,
    pub hands_follow: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Aero {
    /// Newtons per square metre per second squared.
    pub drag: f32,
    /// Share of spin lost per second, so a body in the air does not tumble forever.
    pub angular_drag: f32,
}

/// One solid box of the chassis that hits the world, in the chassis's frame.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HullBox {
    pub center: Vec3,
    pub half: Vec3,
}

/// How a chassis meets things it was not meant to.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HullContact {
    pub friction: f32,
    pub bounce: f32,
    /// Share of depth corrected each substep, and how much depth is let be.
    pub correction: f32,
    pub slop: f32,
}

/// What a bullet does when it finds a part of the vehicle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanelZone {
    Body,
    Engine,
}

/// How a panel is drawn. The box itself is what stops bullets, so what you see is what you can hide behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanelLook {
    Paint,
    Dark,
    Canvas,
    Trim,
}

/// A flat piece of the vehicle as bullets and eyes meet it. Where there is no panel there is open air,
/// and whoever sits behind it is exposed.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Panel {
    pub center: Vec3,
    pub half: Vec3,
    pub rotation: Quat,
    pub zone: PanelZone,
    pub look: PanelLook,
    /// Lets through this share of a bullet's speed, or stops it at zero.
    pub pass: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DamageConfig {
    pub hull: f32,
    pub engine: f32,
    pub tyre: f32,
    /// A bullet's damage is scaled by this on each part.
    pub hull_scale: f32,
    pub engine_scale: f32,
    pub tyre_scale: f32,
    /// Power left is the engine's health share to this power, so it fades and then stops.
    pub power_curve: f32,
    /// Below this share of hull or engine it smokes, for the people drawing it.
    pub smoke_below: f32,
    /// Hull lost a second once on fire, how long it burns before going up, and what it does to those inside.
    pub burn_rate: f32,
    pub burn_time: f32,
    pub burn_hurt: f32,
    /// Going up: hurt at the middle, and the metres it reaches.
    pub blast_hurt: f32,
    pub blast_radius: f32,
    /// Seconds a wreck sits before a new one comes, and one left tipped over with nobody in it. It
    /// counts as tipped over with its up axis this far from straight down, as a dot product.
    pub respawn_after: f32,
    pub righted_above: f32,
    /// Hitting something: below this change in speed nothing happens, and past it the hull loses this per m/s.
    pub crash_from: f32,
    pub crash_hull: f32,
    /// Past this change in speed, those inside are hurt by this per m/s over.
    pub hurt_from: f32,
    pub hurt_per_speed: f32,
    /// Running someone down: from this closing speed, hurt per m/s.
    pub ram_from: f32,
    pub ram_hurt: f32,
}

/// Getting in, getting out and what riding does to a gun.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RideConfig {
    /// How far from where you stand to use a door you can be, across and up or down, in metres.
    pub board_reach: f32,
    pub board_climb: f32,
    /// You have to be looking this close to what you put a hand to, as a dot product.
    pub board_facing: f32,
    /// Faster than this nobody can get in.
    pub board_speed: f32,
    /// Getting out faster than this is jumping out, and costs this much per m/s over it.
    pub exit_speed: f32,
    pub jump_hurt: f32,
    /// The furthest to look for ground below a door.
    pub exit_drop: f32,
    /// How quickly a look inside follows the seat tilting, per second. Its turning is followed exactly, and this
    /// way a slope arrives and a bump does not.
    pub tilt_rate: f32,
    /// Radians a second a look comes back to what the seat allows, having got there looking somewhere else.
    pub look_settle: f32,
    /// What a chassis's speed and spin do to a gun held inside it.
    pub speed_shake: f32,
    pub turn_shake: f32,
    /// How high a standing soldier's hips are, for the start and end of going through a door.
    pub stand_hips: f32,
}

/// How a driver sees a correction to their own vehicle. They are looking out of it at the world, so it
/// fades gently rather than jumping: the screen must not lurch because a packet was late.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetView {
    /// Per second, how quickly what is left of it dies away.
    pub fade_rate: f32,
    /// Fastest it may do that, in m/s and rad/s, however large it is.
    pub fade_speed: f32,
    pub fade_turn: f32,
    /// A correction bigger than this is a teleport and is shown as one.
    pub snap_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SleepConfig {
    pub speed: f32,
    pub spin: f32,
    pub after: f32,
}

/// Pushing it by hand, on foot, holding the use key at one end or a corner of it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PushConfig {
    /// Newtons each of them puts into it, along the ground, and this share of that again upwards, as a shove at a
    /// bumper lifts as well.
    pub force: f32,
    pub lift_share: f32,
    /// From the eye to the spot, to start and to keep going.
    pub reach: f32,
    pub hold_reach: f32,
    /// How squarely they have to face the spot to start, as a cosine.
    pub facing: f32,
    /// Stamina a second each, of a full 1.
    pub stamina_drain: f32,
    /// Slower than this, a vehicle being pushed has not got its tyres into the ground yet: rolling resistance
    /// builds up to all of it by this speed, m/s, so however few are pushing it goes, if only at a crawl.
    pub roll_from: f32,
    /// How much of their walking pace they have, leaning into it.
    pub walk_share: f32,
    /// How fast they are drawn back to the furthest their hands reach, if the vehicle has got further, m/s.
    pub pull_rate: f32,
    /// Coming at them faster than this where their hands are, they get out of its way, and going away faster than
    /// this it has got away from them, metres a second.
    pub stop_speed: f32,
    pub keep_up: f32,
    /// Where hands go to push, in the chassis's frame.
    pub spots: Vec<Vec3>,
}

/// Both ends and the four corners of a box, at its middle height: where a bumper is.
pub fn push_spots(hull: &HullBox) -> Vec<Vec3> {
    let (c, h) = (hull.center, hull.half);
    let mut spots = Vec::new();
    for end in [-1.0, 1.0] {
        for across in [-0.8, 0.0, 0.8] {
            spots.push(Vec3::new(c.x + h.x * across, c.y, c.z + h.z * end));
        }
    }
    spots
}

/// Righting it by hand, on foot, holding the use key.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LiftConfig {
    /// How many it takes before it moves at all.
    pub crew: u32,
    /// Radians per second it rolls upright for each of them.
    pub rate_per_soldier: f32,
    /// How far a body-up points at the sky, 0 to 1, below which it can be lifted.
    pub start_below: f32,
    /// And above which nobody needs to any more. Past the point of tipping, so it goes on over by itself.
    pub done_above: f32,
    /// From the eye to the nearest of the hull, to start. Then their feet go no further from it than `hold_within`,
    /// as a gun set down does not come off at the end of its arc, and `hold_reach` from the eye is it having got
    /// away from them.
    pub reach: f32,
    pub hold_within: f32,
    pub hold_reach: f32,
    /// How much of their walking pace they have, and how fast they are drawn back within reach, m/s.
    pub walk_share: f32,
    pub pull_rate: f32,
    /// How squarely they have to face it to start, as a cosine.
    pub facing: f32,
    /// Stamina a second each, of a full 1.
    pub stamina_drain: f32,
    /// Lying this flat, there is no way it wants to go, so the side nearest the crew comes up first.
    pub flat_below: f32,
    /// If it moves at less than this share of what they are putting in for this long, something is in the way.
    pub stall_share: f32,
    pub stall_after: f32,
    /// How long the prompt says so.
    pub refused_for: f32,
    /// A groan of metal this often while it is coming up.
    pub groan_every: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VehicleConfig {
    /// What a mod calls it.
    #[serde(skip)]
    pub key: prs_seats::StaticStr,
    #[serde(deserialize_with = "prs_seats::leaked_str")]
    pub name: prs_seats::StaticStr,
    pub mass: f32,
    /// About the chassis's own right, up and back axes, in kg m^2.
    pub inertia: Vec3,
    pub gravity: f32,
    /// Steps of physics per tick. More is better for bumpers meeting thin walls.
    pub substeps: u32,
    /// Passes over the tyres each substep.
    pub iterations: u32,
    pub wheels: Vec<WheelConfig>,
    pub suspension: Suspension,
    pub tyres: Tyres,
    pub engine: Engine,
    pub brakes: Brakes,
    pub steering: Steering,
    pub aero: Aero,
    pub hull: Vec<HullBox>,
    pub lift: LiftConfig,
    pub push: PushConfig,
    pub contact: HullContact,
    pub panels: Vec<Panel>,
    pub seats: Vec<SeatConfig>,
    /// The ways in and out, and which seats connect inside.
    pub entries: Vec<Entry>,
    pub links: Vec<SeatLink>,
    pub damage: DamageConfig,
    pub ride: RideConfig,
    pub net_view: NetView,
    pub sleep: SleepConfig,
}

fn panel(center: Vec3, half: Vec3, zone: PanelZone, look: PanelLook, pass: f32) -> Panel {
    Panel { center, half, rotation: Quat::IDENTITY, zone, look, pass }
}

fn side_window_fire(side: f32) -> Arc {
    // Out of its own side, ahead and behind: on the left, positive yaw.
    let (lo, hi) = if side < 0.0 { (0.35, 2.3) } else { (-2.3, -0.35) };
    Arc { yaw: Vec2::new(lo, hi), pitch: Vec2::new(-0.35, 0.55) }
}

fn truck_seat(role: SeatRole, side: f32, z: f32) -> SeatConfig {
    let x = 0.42 * side;
    let (front, hands) = match role {
        SeatRole::Driver => (-0.5, Some([Vec3::new(x - 0.16, 0.41, z - 0.46), Vec3::new(x + 0.16, 0.41, z - 0.46)])),
        SeatRole::Passenger => (-0.48, None),
    };
    SeatConfig {
        role,
        eye: Vec3::new(x, 0.74, z - 0.05),
        facing: 0.0,
        swivel: false,
        hips: Vec3::new(x, 0.0, z + 0.08),
        feet: Vec3::new(x, -0.36, z + front),
        hands,
        look: Arc { yaw: Vec2::new(-2.2, 2.2), pitch: Vec2::new(-0.8, 0.8) },
        fire: (role == SeatRole::Passenger).then(|| side_window_fire(side)),
    }
}

fn truck_door(name: &'static str, seat: u8, side: f32, z: f32, panel: u8, hinge_z: f32) -> Entry {
    Entry {
        name,
        stand: Vec3::new(side * 1.5, -0.85, z),
        on_ground: true,
        hand: Vec3::new(side * 0.93, 0.3, z - 0.3),
        via: Vec3::new(side * 0.78, 0.2, z + 0.05),
        seats: vec![seat],
        time: 1.1,
        door: Some(DoorPanel { panel, hinge: Vec3::new(side * 0.9, -0.05, hinge_z), open: side * 1.15, time: 0.3 }),
    }
}

impl VehicleConfig {
    /// How far above the ground its centre of mass sits at rest.
    pub fn ride_height(&self) -> f32 {
        let wheel = &self.wheels[0];
        let sag = self.mass * self.gravity / (self.wheels.len() as f32 * self.suspension.stiffness);
        wheel.radius + (wheel.travel - sag) - wheel.mount.y
    }

    /// A box around the chassis that everything about it fits in: a radius from its centre of mass.
    pub fn reach(&self) -> f32 {
        self.hull.iter().map(|h| h.center.length() + h.half.length()).fold(0.0, f32::max)
    }

    /// Where a panel is and how it is turned, with doors open this far. `doors` has one share for each
    /// entry, 0 shut and 1 open. An empty list has them all shut.
    pub fn panel_at(&self, index: usize, doors: &[f32]) -> (Vec3, Quat) {
        let panel = &self.panels[index];
        let swung = self.entries.iter().zip(doors).find_map(|(entry, share)| {
            entry.door.filter(|door| door.panel as usize == index && *share > 0.0).map(|door| (door, *share))
        });
        match swung {
            Some((door, share)) => {
                let turn = Quat::from_rotation_y(door.open * share);
                (door.hinge + turn * (panel.center - door.hinge), turn * panel.rotation)
            }
            None => (panel.center, panel.rotation),
        }
    }

    /// Whether this panel is a door, which is drawn on its own to swing.
    pub fn door_of_panel(&self, index: usize) -> Option<usize> {
        self.entries.iter().position(|entry| entry.door.is_some_and(|door| door.panel as usize == index))
    }

    /// Where the steering wheel is, which the hands of whoever has the controls say: its middle and size.
    /// The hands sit at ten to two, above the middle.
    pub fn steering_wheel(&self) -> Option<(Vec3, f32)> {
        let [left, right] = self.seats.iter().find_map(|seat| seat.hands)?;
        let radius = left.distance(right) * 0.62;
        Some(((left + right) * 0.5 - Vec3::Y * radius * 0.55, radius))
    }

    /// How the steering wheel is turned right now, in the chassis's frame.
    pub fn steering_wheel_turn(&self, steer: f32) -> Quat {
        let tilt = Quat::from_rotation_x(self.steering.wheel_tilt);
        // Turning right takes the top of the wheel to the right, which is backwards about its axis.
        tilt * Quat::from_rotation_y(-steer * self.steering.wheel_ratio)
    }

    /// A four-seat open-sided utility truck with a canvas top: no armour, and everyone inside can be
    /// seen and shot, and can shoot back out of their side.
    pub fn utility_truck() -> Self {
        let wheel = |x: f32, z: f32, steers: bool| WheelConfig {
            mount: Vec3::new(x, -0.22, z),
            radius: 0.42,
            width: 0.26,
            travel: 0.3,
            steers,
            driven: true,
        };
        let (body, engine) = (PanelZone::Body, PanelZone::Engine);
        let (paint, dark, canvas, trim) = (PanelLook::Paint, PanelLook::Dark, PanelLook::Canvas, PanelLook::Trim);
        // Thin steel lets most of a rifle round through: a door is concealment and not cover.
        let steel = 0.6;
        let mut panels = vec![
            // Bonnet and engine bay.
            panel(Vec3::new(0.0, 0.06, -1.55), Vec3::new(0.7, 0.24, 0.62), engine, paint, 0.0),
            // Grille, wings and floor.
            panel(Vec3::new(0.0, -0.12, -2.2), Vec3::new(0.84, 0.3, 0.04), body, dark, steel),
            panel(Vec3::new(0.0, -0.42, 0.1), Vec3::new(0.92, 0.05, 2.1), body, dark, 0.0),
            // Bulkhead between engine and cabin, and the tailgate.
            panel(Vec3::new(0.0, -0.05, -0.92), Vec3::new(0.86, 0.36, 0.03), body, paint, steel),
            panel(Vec3::new(0.0, -0.05, 2.15), Vec3::new(0.92, 0.36, 0.04), body, paint, steel),
            // Windscreen frame: two posts and a header, no glass.
            panel(Vec3::new(-0.88, 0.72, -0.9), Vec3::new(0.018, 0.42, 0.018), body, trim, 0.0),
            panel(Vec3::new(0.88, 0.72, -0.9), Vec3::new(0.018, 0.42, 0.018), body, trim, 0.0),
            panel(Vec3::new(0.0, 1.13, -0.9), Vec3::new(0.9, 0.02, 0.02), body, trim, 0.0),
            // Canvas top on a roll bar.
            panel(Vec3::new(0.0, 1.17, 0.1), Vec3::new(0.9, 0.02, 1.02), body, canvas, 0.95),
            panel(Vec3::new(-0.88, 0.72, 1.1), Vec3::new(0.022, 0.42, 0.022), body, trim, 0.0),
            panel(Vec3::new(0.88, 0.72, 1.1), Vec3::new(0.022, 0.42, 0.022), body, trim, 0.0),
        ];
        // Doors up to the sill, with the seats' whole upper bodies open above them, then rear wings.
        // Two doors a side, then over the back wheel and low behind it. The doors' numbers go in `entries`.
        let doors_from = panels.len() as u8;
        for side in [-1.0, 1.0] {
            panels.push(panel(Vec3::new(0.9 * side, -0.05, -0.36), Vec3::new(0.03, 0.36, 0.56), body, paint, steel));
            panels.push(panel(Vec3::new(0.9 * side, -0.05, 0.525), Vec3::new(0.03, 0.36, 0.325), body, paint, steel));
            panels.push(panel(Vec3::new(0.9 * side, 0.18, 1.495), Vec3::new(0.03, 0.13, 0.645), body, paint, steel));
            panels.push(panel(Vec3::new(0.9 * side, -0.1, 1.92), Vec3::new(0.03, 0.15, 0.22), body, paint, steel));
        }
        Self {
            key: "truck",
            name: "utility truck",
            mass: 2300.0,
            inertia: Vec3::new(3900.0, 4300.0, 1150.0),
            gravity: 9.81,
            substeps: 2,
            iterations: 3,
            wheels: vec![wheel(-0.82, -1.35, true), wheel(0.82, -1.35, true), wheel(-0.82, 1.25, false), wheel(0.82, 1.25, false)],
            suspension: Suspension {
                stiffness: 62000.0,
                damping: 5400.0,
                anti_roll: 18000.0,
                axles: [(0, 1), (2, 3)],
                bump_correction: 0.25,
            },
            tyres: Tyres {
                concrete: GroundGrip { grip: 0.85, rolling: 0.015 },
                gravel: GroundGrip { grip: 0.66, rolling: 0.035 },
                dirt: GroundGrip { grip: 0.76, rolling: 0.03 },
                grass: GroundGrip { grip: 0.62, rolling: 0.05 },
                other: GroundGrip { grip: 0.7, rolling: 0.02 },
                static_speed: 0.4,
                slip_stiffness: 20.0,
                slip_shape: 1.25,
                lateral_reserve: 0.25,
                spin_gain: 0.004,
                flat_grip: 0.45,
                flat_rolling: 0.25,
                flat_drop: 0.14,
            },
            engine: Engine {
                idle_rpm: 800.0,
                redline_rpm: 4600.0,
                launch_rpm: 2100.0,
                curve: vec![Vec2::new(800.0, 0.72), Vec2::new(1800.0, 1.0), Vec2::new(3200.0, 0.93), Vec2::new(4300.0, 0.68), Vec2::new(4600.0, 0.0)],
                peak_torque: 270.0,
                gears: vec![3.9, 2.3, 1.45, 1.0],
                reverse: 3.6,
                final_drive: 4.1,
                shift_up_rpm: 4100.0,
                shift_down_rpm: 1500.0,
                shift_time: 0.28,
                manual_until_stop: false,
                rpm_rate: 7.0,
                engine_brake: 1100.0,
                throttle_rate: 5.0,
                wheel_inertia: 3.0,
                wheel_air_drag: 0.4,
            },
            brakes: Brakes { per_wheel: 7000.0, rear_share: 0.45, parked: 7000.0, handbrake_front: 0.0, handbrake_rear: 9000.0, reverse_below: 0.8, hold_below: 0.5 },
            steering: Steering {
                max_angle: 0.6,
                rate: 1.5,
                return_rate: 3.0,
                wheel_ratio: 4.0,
                wheel_tilt: 1.05,
                hands_follow: 0.7,
            },
            aero: Aero { drag: 1.9, angular_drag: 0.15 },
            hull: vec![
                HullBox { center: Vec3::new(0.0, -0.1, 0.0), half: Vec3::new(0.92, 0.38, 2.22) },
                HullBox { center: Vec3::new(0.0, 0.76, 0.1), half: Vec3::new(0.86, 0.43, 1.0) },
            ],
            lift: LiftConfig { crew: 1, rate_per_soldier: 0.16, start_below: 0.5, done_above: 0.7, reach: 1.5, hold_within: 1.1, hold_reach: 2.6, walk_share: 0.5, pull_rate: 1.5, facing: 0.2, stamina_drain: 0.035, flat_below: 0.05, stall_share: 0.2, stall_after: 1.0, refused_for: 2.5, groan_every: 1.1 },
            push: PushConfig { force: 850.0, lift_share: 0.35, reach: 1.6, hold_reach: 2.2, facing: 0.3, stamina_drain: 0.04, roll_from: 1.0, walk_share: 0.5, pull_rate: 1.5, stop_speed: 0.6, keep_up: 2.2, spots: push_spots(&HullBox { center: Vec3::new(0.0, -0.1, 0.0), half: Vec3::new(0.92, 0.38, 2.22) }) },
            contact: HullContact { friction: 0.55, bounce: 0.1, correction: 0.2, slop: 0.005 },
            panels,
            seats: vec![
                truck_seat(SeatRole::Driver, -1.0, -0.25),
                truck_seat(SeatRole::Passenger, 1.0, -0.25),
                truck_seat(SeatRole::Passenger, -1.0, 0.72),
                truck_seat(SeatRole::Passenger, 1.0, 0.72),
            ],
            entries: vec![
                truck_door("driver's door", 0, -1.0, -0.25, doors_from, -0.92),
                truck_door("passenger door", 1, 1.0, -0.25, doors_from + 4, -0.92),
                truck_door("rear door", 2, -1.0, 0.72, doors_from + 1, 0.2),
                truck_door("rear door", 3, 1.0, 0.72, doors_from + 5, 0.2),
            ],
            // Across the cab and across the back, with no way from one to the other without getting out.
            links: vec![SeatLink { a: 0, b: 1, time: 1.4 }, SeatLink { a: 2, b: 3, time: 1.4 }],
            damage: DamageConfig {
                hull: 900.0,
                engine: 220.0,
                tyre: 45.0,
                hull_scale: 1.0,
                engine_scale: 1.0,
                tyre_scale: 1.0,
                power_curve: 0.5,
                smoke_below: 0.45,
                burn_rate: 25.0,
                burn_time: 14.0,
                burn_hurt: 18.0,
                blast_hurt: 320.0,
                blast_radius: 9.0,
                respawn_after: 45.0,
                righted_above: 0.3,
                crash_from: 5.0,
                crash_hull: 28.0,
                hurt_from: 11.0,
                hurt_per_speed: 5.0,
                ram_from: 3.0,
                ram_hurt: 14.0,
            },
            ride: RideConfig { board_reach: 0.95, board_climb: 0.7, board_facing: 0.2, board_speed: 1.5, exit_speed: 4.0, jump_hurt: 12.0, exit_drop: 3.0, tilt_rate: 3.0, look_settle: 2.5, speed_shake: 0.09, turn_shake: 0.5, stand_hips: 0.92 },
            net_view: NetView { fade_rate: 4.0, fade_speed: 2.5, fade_turn: 0.6, snap_distance: 8.0 },
            sleep: SleepConfig { speed: 0.04, spin: 0.04, after: 0.6 },
        }
    }
}

impl VehicleConfig {
    /// Everything about where its parts are, stretched across and along, the way a smaller or bigger one of
    /// the same build would be. Heights stay as they are, since the people in it do not change.
    fn stretched(mut self, across: f32, along: f32) -> Self {
        let s = Vec3::new(across, 1.0, along);
        for wheel in &mut self.wheels {
            wheel.mount *= s;
        }
        for panel in &mut self.panels {
            panel.center *= s;
            panel.half *= s;
        }
        for hull in &mut self.hull {
            hull.center *= s;
            hull.half *= s;
        }
        for seat in &mut self.seats {
            seat.eye *= s;
            seat.hips *= s;
            seat.feet *= s;
            if let Some(hands) = &mut seat.hands {
                *hands = hands.map(|hand| hand * s);
            }
        }
        for spot in &mut self.push.spots {
            *spot *= s;
        }
        for entry in &mut self.entries {
            entry.stand *= s;
            entry.hand *= s;
            entry.via *= s;
            if let Some(door) = &mut entry.door {
                door.hinge *= s;
            }
        }
        self
    }

    /// An open light jeep: four seats, no doors and no top, quick, and soon broken. The utility truck's build,
    /// smaller and lighter, with everything above the sills left off.
    pub fn light_jeep() -> Self {
        let mut cfg = Self::utility_truck().stretched(0.92, 0.84);
        cfg.key = "jeep";
        cfg.name = "light jeep";
        // Nothing to open: in over the side. The doors and the canvas go, last first so the rest keep their places.
        let mut gone: Vec<usize> = cfg.entries.iter_mut().filter_map(|entry| entry.door.take().map(|door| door.panel as usize)).collect();
        gone.extend(cfg.panels.iter().enumerate().filter(|(_, panel)| panel.look == PanelLook::Canvas).map(|(i, _)| i));
        gone.sort_unstable();
        for i in gone.into_iter().rev() {
            cfg.panels.remove(i);
        }
        for entry in &mut cfg.entries {
            entry.name = "side";
            entry.time = 0.8;
        }
        cfg.mass = 1450.0;
        cfg.inertia = Vec3::new(2000.0, 2200.0, 640.0);
        for wheel in &mut cfg.wheels {
            wheel.radius = 0.38;
            wheel.width = 0.22;
        }
        cfg.suspension.stiffness = 40000.0;
        cfg.suspension.damping = 4100.0;
        cfg.suspension.anti_roll = 11000.0;
        cfg.engine.peak_torque = 255.0;
        cfg.engine.final_drive = 3.7;
        cfg.engine.engine_brake = 800.0;
        cfg.brakes.per_wheel = 4800.0;
        cfg.brakes.parked = 4800.0;
        cfg.brakes.handbrake_rear = 6500.0;
        cfg.aero.drag = 1.35;
        cfg.lift.rate_per_soldier = 0.22;
        cfg.damage.hull = 450.0;
        cfg.damage.engine = 140.0;
        cfg.damage.tyre = 35.0;
        cfg.damage.crash_hull = 40.0;
        cfg.damage.respawn_after = 40.0;
        cfg.damage.blast_hurt = 220.0;
        cfg.damage.blast_radius = 7.0;
        cfg.ride.speed_shake = 0.11;
        cfg
    }
}

fn armoured_seat(role: SeatRole, x: f32, z: f32) -> SeatConfig {
    let hands = (role == SeatRole::Driver).then_some([Vec3::new(x - 0.16, 0.44, z - 0.46), Vec3::new(x + 0.16, 0.44, z - 0.46)]);
    SeatConfig {
        role,
        eye: Vec3::new(x, 0.78, z - 0.05),
        facing: 0.0,
        swivel: false,
        hips: Vec3::new(x, 0.04, z + 0.08),
        feet: Vec3::new(x, -0.32, z - 0.5),
        hands,
        look: Arc { yaw: Vec2::new(-1.8, 1.8), pitch: Vec2::new(-0.5, 0.6) },
        fire: None,
    }
}

impl VehicleConfig {
    /// A light armoured car: rifle rounds only find the slits and the tyres, those inside are out of it
    /// and see out through the slits, and whoever stands in the roof hatch can shoot all round and be shot.
    pub fn armoured_car() -> Self {
        let wheel = |x: f32, z: f32, steers: bool| WheelConfig {
            mount: Vec3::new(x, -0.277, z),
            radius: 0.55,
            width: 0.34,
            travel: 0.35,
            steers,
            driven: true,
        };
        let (body, engine) = (PanelZone::Body, PanelZone::Engine);
        let (paint, dark, trim) = (PanelLook::Paint, PanelLook::Dark, PanelLook::Trim);
        let plate = |center: Vec3, half: Vec3| panel(center, half, body, paint, 0.0);
        let mut panels = vec![
            // The nose and engine, then the floor.
            panel(Vec3::new(0.0, 0.1, -2.0), Vec3::new(0.96, 0.62, 0.72), engine, paint, 0.0),
            panel(Vec3::new(0.0, -0.45, 0.45), Vec3::new(1.02, 0.08, 2.2), body, dark, 0.0),
            // Across the front of the crew, below the slit and above it.
            plate(Vec3::new(0.0, 0.12, -1.26), Vec3::new(1.0, 0.58, 0.04)),
            plate(Vec3::new(0.0, 0.98, -1.26), Vec3::new(1.0, 0.12, 0.04)),
            // Across the back.
            plate(Vec3::new(0.0, 0.3, 2.65), Vec3::new(1.05, 0.8, 0.05)),
            // A roof all round the hatch.
            plate(Vec3::new(0.0, 1.12, -0.5), Vec3::new(1.05, 0.04, 0.8)),
            plate(Vec3::new(0.0, 1.12, 1.75), Vec3::new(1.05, 0.04, 0.9)),
            plate(Vec3::new(-0.75, 1.12, 0.55), Vec3::new(0.3, 0.04, 0.3)),
            plate(Vec3::new(0.75, 1.12, 0.55), Vec3::new(0.3, 0.04, 0.3)),
            plate(Vec3::new(0.0, -0.3, 3.2), Vec3::new(0.8, 0.32, 0.48)),
            // A rim around the hatch, for the eye and for a little cover.
            panel(Vec3::new(0.0, 1.22, 0.2), Vec3::new(0.5, 0.06, 0.03), body, trim, 0.0),
        ];
        for side in [-1.0, 1.0] {
            panels.push(plate(Vec3::new(side * 1.05, 0.98, 0.7), Vec3::new(0.04, 0.12, 1.95)));
            panels.push(plate(Vec3::new(side * 1.05, 0.12, -1.175), Vec3::new(0.04, 0.58, 0.075)));
        }
        // Below the slit, a plate each side between its doors.
        panels.push(plate(Vec3::new(-1.05, 0.12, 1.275), Vec3::new(0.04, 0.58, 1.375)));
        panels.push(plate(Vec3::new(1.05, 0.12, 0.8), Vec3::new(0.04, 0.58, 0.9)));
        panels.push(plate(Vec3::new(1.05, 0.12, 2.575), Vec3::new(0.04, 0.58, 0.075)));
        // Then the doors, whose numbers go in `entries`.
        let doors_from = panels.len() as u8;
        panels.push(plate(Vec3::new(-1.05, 0.12, -0.6), Vec3::new(0.04, 0.58, 0.5)));
        panels.push(plate(Vec3::new(1.05, 0.12, -0.6), Vec3::new(0.04, 0.58, 0.5)));
        panels.push(plate(Vec3::new(1.05, 0.12, 2.1), Vec3::new(0.04, 0.58, 0.4)));
        Self {
            key: "armoured_car",
            name: "armoured car",
            mass: 7500.0,
            inertia: Vec3::new(21000.0, 23000.0, 6500.0),
            gravity: 9.81,
            substeps: 2,
            iterations: 3,
            wheels: vec![wheel(-0.98, -1.75, true), wheel(0.98, -1.75, true), wheel(-0.98, 1.65, false), wheel(0.98, 1.65, false)],
            suspension: Suspension {
                stiffness: 145000.0,
                damping: 15000.0,
                anti_roll: 50000.0,
                axles: [(0, 1), (2, 3)],
                bump_correction: 0.25,
            },
            tyres: Tyres {
                concrete: GroundGrip { grip: 0.85, rolling: 0.02 },
                gravel: GroundGrip { grip: 0.68, rolling: 0.035 },
                dirt: GroundGrip { grip: 0.76, rolling: 0.03 },
                grass: GroundGrip { grip: 0.64, rolling: 0.05 },
                other: GroundGrip { grip: 0.7, rolling: 0.025 },
                static_speed: 0.4,
                slip_stiffness: 18.0,
                slip_shape: 1.25,
                lateral_reserve: 0.25,
                spin_gain: 0.0012,
                flat_grip: 0.8,
                flat_rolling: 0.05,
                flat_drop: 0.08,
            },
            engine: Engine {
                idle_rpm: 700.0,
                redline_rpm: 3200.0,
                launch_rpm: 1500.0,
                curve: vec![Vec2::new(700.0, 0.75), Vec2::new(1400.0, 1.0), Vec2::new(2400.0, 0.9), Vec2::new(3000.0, 0.6), Vec2::new(3200.0, 0.0)],
                peak_torque: 800.0,
                gears: vec![5.2, 2.9, 1.7, 1.0],
                reverse: 4.6,
                final_drive: 3.8,
                shift_up_rpm: 2800.0,
                shift_down_rpm: 1100.0,
                shift_time: 0.42,
                manual_until_stop: false,
                rpm_rate: 5.0,
                engine_brake: 2600.0,
                throttle_rate: 3.5,
                wheel_inertia: 12.0,
                wheel_air_drag: 0.5,
            },
            brakes: Brakes { per_wheel: 20000.0, rear_share: 0.45, parked: 22000.0, handbrake_front: 0.0, handbrake_rear: 26000.0, reverse_below: 0.7, hold_below: 0.4 },
            steering: Steering {
                max_angle: 0.5,
                rate: 1.1,
                return_rate: 2.2,
                wheel_ratio: 5.0,
                wheel_tilt: 1.15,
                hands_follow: 0.7,
            },
            aero: Aero { drag: 6.0, angular_drag: 0.15 },
            hull: vec![
                HullBox { center: Vec3::new(0.0, 0.2, 0.0), half: Vec3::new(1.08, 0.8, 2.72) },
                // A step up at the back, low enough to climb and from there the deck.
                HullBox { center: Vec3::new(0.0, -0.3, 3.2), half: Vec3::new(0.8, 0.32, 0.48) },
            ],
            lift: LiftConfig { crew: 2, rate_per_soldier: 0.05, start_below: 0.5, done_above: 0.7, reach: 1.5, hold_within: 1.1, hold_reach: 2.6, walk_share: 0.5, pull_rate: 1.5, facing: 0.2, stamina_drain: 0.03, flat_below: 0.05, stall_share: 0.2, stall_after: 1.0, refused_for: 2.5, groan_every: 1.5 },
            push: PushConfig { force: 850.0, lift_share: 0.35, reach: 1.6, hold_reach: 2.2, facing: 0.3, stamina_drain: 0.04, roll_from: 1.0, walk_share: 0.5, pull_rate: 1.5, stop_speed: 0.6, keep_up: 2.2, spots: vec![
                // The nose, and the box on the back that sticks out furthest behind.
                Vec3::new(-0.86, -0.15, -2.72),
                Vec3::new(0.0, -0.15, -2.72),
                Vec3::new(0.86, -0.15, -2.72),
                Vec3::new(-0.6, -0.3, 3.68),
                Vec3::new(0.0, -0.3, 3.68),
                Vec3::new(0.6, -0.3, 3.68),
            ] },
            contact: HullContact { friction: 0.5, bounce: 0.08, correction: 0.2, slop: 0.005 },
            panels,
            seats: vec![
                armoured_seat(SeatRole::Driver, -0.48, -0.55),
                SeatConfig {
                    role: SeatRole::Passenger,
                    eye: Vec3::new(0.0, 1.62, 0.55),
                    facing: 0.0,
                    swivel: true,
                    hips: Vec3::new(0.0, 0.86, 0.72),
                    feet: Vec3::new(0.0, 0.0, 0.3),
                    hands: None,
                    look: Arc { yaw: Vec2::new(-std::f32::consts::PI, std::f32::consts::PI), pitch: Vec2::new(-0.35, 0.7) },
                    fire: Some(Arc { yaw: Vec2::new(-std::f32::consts::PI, std::f32::consts::PI), pitch: Vec2::new(-0.25, 0.6) }),
                },
                armoured_seat(SeatRole::Passenger, 0.48, -0.55),
                armoured_seat(SeatRole::Passenger, 0.48, 1.6),
            ],
            entries: vec![
                Entry { name: "driver's door", stand: Vec3::new(-1.8, -1.05, -0.55), on_ground: true, hand: Vec3::new(-1.08, 0.3, -0.75), via: Vec3::new(-0.9, 0.15, -0.5), seats: vec![0], time: 1.7, door: Some(DoorPanel { panel: doors_from, hinge: Vec3::new(-1.05, 0.12, -1.1), open: -1.2, time: 0.4 }) },
                Entry { name: "side door", stand: Vec3::new(1.8, -1.05, -0.55), on_ground: true, hand: Vec3::new(1.08, 0.3, -0.75), via: Vec3::new(0.9, 0.15, -0.5), seats: vec![2], time: 1.7, door: Some(DoorPanel { panel: doors_from + 1, hinge: Vec3::new(1.05, 0.12, -1.1), open: 1.2, time: 0.4 }) },
                Entry { name: "side hatch", stand: Vec3::new(1.5, -1.05, 2.3), on_ground: true, hand: Vec3::new(1.08, 0.3, 2.1), via: Vec3::new(0.95, 0.1, 2.0), seats: vec![3], time: 2.0, door: Some(DoorPanel { panel: doors_from + 2, hinge: Vec3::new(1.05, 0.12, 1.7), open: 1.2, time: 0.4 }) },
                // On the deck, having climbed up from the step at the back.
                Entry { name: "hatch", stand: Vec3::new(0.0, 1.0, 1.5), on_ground: false, hand: Vec3::new(0.0, 1.05, 0.55), via: Vec3::new(0.0, 1.25, 0.72), seats: vec![1], time: 1.5, door: None },
            ],
            // Round the inside and up to the hatch from the back.
            links: vec![SeatLink { a: 0, b: 2, time: 1.6 }, SeatLink { a: 2, b: 3, time: 2.0 }, SeatLink { a: 3, b: 1, time: 1.6 }],
            damage: DamageConfig {
                hull: 2600.0,
                engine: 900.0,
                tyre: 220.0,
                hull_scale: 0.06,
                engine_scale: 0.08,
                tyre_scale: 0.6,
                power_curve: 0.5,
                smoke_below: 0.35,
                burn_rate: 40.0,
                burn_time: 12.0,
                burn_hurt: 18.0,
                blast_hurt: 400.0,
                blast_radius: 12.0,
                respawn_after: 90.0,
                righted_above: 0.3,
                crash_from: 7.0,
                crash_hull: 40.0,
                hurt_from: 12.0,
                hurt_per_speed: 5.0,
                ram_from: 2.5,
                ram_hurt: 22.0,
            },
            ride: RideConfig { board_reach: 1.0, board_climb: 0.8, board_facing: 0.2, board_speed: 1.0, exit_speed: 3.0, jump_hurt: 14.0, exit_drop: 3.0, tilt_rate: 3.0, look_settle: 2.5, speed_shake: 0.08, turn_shake: 0.5, stand_hips: 0.92 },
            net_view: NetView { fade_rate: 4.0, fade_speed: 2.5, fade_turn: 0.6, snap_distance: 8.0 },
            sleep: SleepConfig { speed: 0.04, spin: 0.04, after: 0.6 },
        }
    }
}

/// The vehicles a game has, looked up by [`VehicleKind`].
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleTable {
    pub vehicles: Vec<VehicleConfig>,
}

/// In a mod a vehicle goes by its `key`, as `vehicles.truck`. Its place in the list is its [`VehicleKind`],
/// which is why a table keeps the order it was written in.
impl serde::Serialize for VehicleTable {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.vehicles.iter().map(|v| (v.key, v)))
    }
}

impl<'de> serde::Deserialize<'de> for VehicleTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Vehicles;
        impl<'de> serde::de::Visitor<'de> for Vehicles {
            type Value = VehicleTable;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a table of vehicles by name")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<VehicleTable, A::Error> {
                let mut vehicles = Vec::new();
                while let Some((key, mut vehicle)) = map.next_entry::<String, VehicleConfig>()? {
                    vehicle.key = Box::leak(key.into_boxed_str());
                    vehicles.push(vehicle);
                }
                Ok(VehicleTable { vehicles })
            }
        }
        d.deserialize_map(Vehicles)
    }
}

impl VehicleTable {
    pub const UTILITY_TRUCK: VehicleKind = VehicleKind(0);
    pub const ARMOURED_CAR: VehicleKind = VehicleKind(1);
    pub const LIGHT_JEEP: VehicleKind = VehicleKind(2);
}

impl Default for VehicleTable {
    fn default() -> Self {
        Self { vehicles: vec![VehicleConfig::utility_truck(), VehicleConfig::armoured_car(), VehicleConfig::light_jeep()] }
    }
}

impl std::ops::Index<VehicleKind> for VehicleTable {
    type Output = VehicleConfig;

    fn index(&self, kind: VehicleKind) -> &VehicleConfig {
        &self.vehicles[kind.0 as usize]
    }
}
