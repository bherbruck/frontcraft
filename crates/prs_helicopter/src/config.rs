//! Every number in the flight model. The defaults are a loaded UH-1H: a two-bladed teetering rotor that
//! has almost no hub moment, so the machine turns only by leaning its thrust line away from the centre
//! of mass, which is much of why it feels the way it does.

use glam::{Vec2, Vec3};
use prs_seats::{Arc, Entry, SeatConfig, SeatLink, SeatRole};
use std::f32::consts::FRAC_PI_2;

use crate::damage::HeliZone;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HeliConfig {
    /// Kilograms: airframe, crew and about half the fuel.
    pub mass: f32,
    /// Moments of inertia about the body's x (pitch), y (yaw) and z (roll), kg m^2.
    pub inertia: Vec3,
    pub gravity: f32,
    /// Physics steps per tick. Skids are stiff springs and want more than one.
    pub substeps: u32,
    pub air: AirConfig,
    pub rotor: RotorConfig,
    pub tail: TailConfig,
    pub engine: EngineConfig,
    pub airframe: AirframeConfig,
    pub controls: ControlsConfig,
    pub gear: GearConfig,
    pub damage: DamageConfig,
    /// The first is the pilot's.
    pub seats: Vec<SeatConfig>,
    /// The open doors and how they connect. Each is reached from the ground, by the skid.
    pub entries: Vec<Entry>,
    pub links: Vec<SeatLink>,
    pub stick: StickConfig,
    pub dials: crate::DialConfig,
    pub wash: WashConfig,
    pub net_view: crate::NetView,
}

/// How the downwash looks kicking dust up. Nothing in here moves anything.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WashConfig {
    /// Hub height over ground above which there is nothing to see.
    pub reach: f32,
    /// Puffs a second at the most.
    pub rate: f32,
    /// The ring they start in, as a share of rotor radius.
    pub ring: f32,
    /// How fast they blow outwards and rise, m/s.
    pub out_speed: f32,
    pub rise_speed: f32,
    pub life: f32,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AirConfig {
    /// kg/m^3 at sea level.
    pub density: f32,
    /// Metres of height over which the air thins by a factor of e.
    pub scale_height: f32,
    /// Above this height the air thins quickly, which is what ends the map upwards.
    pub ceiling: f32,
    /// How far above the ceiling the rotor has run out of air to work with.
    pub ceiling_fade: f32,
    /// A wind from the map's edge starts this far inside it and blows you back in.
    pub edge_margin: f32,
    /// Acceleration from that wind, m/s^2 per metre past where it starts.
    pub edge_push: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RotorConfig {
    pub radius: f32,
    pub blades: f32,
    pub chord: f32,
    /// Blade lift curve slope, per radian.
    pub lift_slope: f32,
    pub profile_drag: f32,
    /// Real induced power over ideal: tip losses and an uneven wake.
    pub induced_factor: f32,
    /// Rad/s at 100% rotor speed.
    pub nominal_speed: f32,
    /// Rotor speed the blades cannot be driven past, as a fraction of nominal.
    pub max_speed: f32,
    /// Blades and hub, kg m^2. This is the energy an autorotation lands on.
    pub inertia: f32,
    /// Blade pitch at three quarters span with the collective down and up, radians.
    pub min_pitch: f32,
    pub max_pitch: f32,
    /// Blade loading (thrust coefficient over solidity) at which the blades stall.
    pub stall_loading: f32,
    /// How much loading is lost for each unit asked for past the stall.
    pub stall_falloff: f32,
    /// Extra profile power per unit of relative loading past the stall.
    pub stall_power: f32,
    /// Seconds the wake takes to catch up with a change in thrust.
    pub inflow_lag: f32,
    /// Keeps induced flow finite where momentum theory breaks down, in units of hover induced speed.
    pub wake_floor: f32,
    /// The hub, from the centre of mass.
    pub hub: Vec3,
    /// How far full stick tilts the disc: x sideways, y fore and aft, radians.
    pub max_tilt: Vec2,
    /// Seconds for the disc to follow the stick.
    pub disc_lag: f32,
    /// Seconds by which the disc lags the mast when the airframe rotates, with the stabiliser bar's
    /// help. As tilt this is what damps pitch and roll, so it is worth as much as the thrust is.
    pub rate_lag: f32,
    /// Disc tilt away from the wind, radians for each unit of speed over tip speed.
    pub flapback: f32,
    /// What little moment the hub itself can pass on, N m per radian of tilt at nominal speed.
    pub hub_moment: f32,
    /// Vortex ring: descent over hover induced speed where it starts, is at its worst, and is over.
    pub vrs_start: f32,
    pub vrs_full: f32,
    pub vrs_end: f32,
    /// Horizontal speed over hover induced speed where the wake starts to be left behind, and has been.
    pub vrs_clear_from: f32,
    pub vrs_clear_at: f32,
    /// Share of thrust lost at its worst.
    pub vrs_loss: f32,
    /// Seconds to get into it, and to get out once the air allows it.
    pub vrs_onset: f32,
    pub vrs_recovery: f32,
    /// Peak moment of the buffet at its worst, N m, and how fast it wanders, rad/s.
    pub vrs_buffet: f32,
    pub vrs_buffet_rate: f32,
    /// Horizontal speed over hover induced speed at which the ground no longer cushions anything.
    pub ground_effect_fade: f32,
    /// How much of a cushion on one side alone is felt as a moment, in rotor radii of arm.
    pub ground_effect_arm: f32,
}

impl HeliConfig {
    /// Where the tail rotor's hub is drawn and turns, as against `TailConfig::at` where it pushes from.
    pub fn tail_rotor_hub(&self) -> Vec3 {
        self.tail.at + self.tail.drawn_offset
    }
}

impl RotorConfig {
    pub fn area(&self) -> f32 {
        std::f32::consts::PI * self.radius * self.radius
    }

    pub fn solidity(&self) -> f32 {
        self.blades * self.chord / (std::f32::consts::PI * self.radius)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TailConfig {
    /// The tail rotor from the centre of mass. Level with it, so that the pedals do not roll the machine.
    pub at: Vec3,
    pub radius: f32,
    /// Newtons at full pedal and nominal rotor speed.
    pub max_thrust: f32,
    /// Newtons lost per m/s of air coming through it. Most of the yaw damping in the hover.
    pub inflow_loss: f32,
    /// Share of main rotor torque countered with no pedal, as a linkage from the collective would.
    /// The rest shows up as yaw whenever the power changes.
    pub torque_mix: f32,
    /// From where it pushes to where it really is: off to the left of the fin and higher up.
    pub drawn_offset: Vec3,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineConfig {
    /// Shaft watts the transmission will take.
    pub max_power: f32,
    /// Newton metres on the shaft, which is all that matters while the rotor is barely turning.
    pub max_torque: f32,
    /// Seconds from pressing start to the engine at flight speed.
    pub start_time: f32,
    /// Seconds for it to wind down once off or dead.
    pub stop_time: f32,
    /// Available power goes as spool to this power, so that the rotor comes up late in the start.
    pub spool_exponent: f32,
    /// Fractions of full power per second it can come up and go down.
    pub power_up_rate: f32,
    pub power_down_rate: f32,
    /// How hard the governor reacts to rotor speed being off, in power per unit of error.
    pub governor_gain: f32,
    /// Watts always lost in the gearboxes and accessories at nominal rotor speed.
    pub drive_loss: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AirframeConfig {
    /// Drag area side on, from above and head on, m^2.
    pub drag_area: Vec3,
    /// Fin: area, lift slope, arm behind the centre of mass, and the slip angle it stalls at.
    pub fin_area: f32,
    pub fin_slope: f32,
    pub fin_arm: f32,
    pub fin_stall: f32,
    /// Horizontal stabiliser, as the fin, plus how much it is set nose down.
    pub stabiliser_area: f32,
    pub stabiliser_slope: f32,
    pub stabiliser_arm: f32,
    pub stabiliser_stall: f32,
    pub stabiliser_incidence: f32,
    /// N m per rad/s about each body axis, from the airframe moving through air.
    pub damping: Vec3,
    /// Points on the airframe that must not go through anything, and what is hurt when they do.
    pub probes: [BodyProbe; 7],
    /// How much a collision bounces.
    pub restitution: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BodyProbe {
    pub at: Vec3,
    pub zone: HeliZone,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ControlsConfig {
    /// Full travel per second with W or S held.
    pub collective_rate: f32,
    /// Pedal position a held key goes to from the trim, how fast it gets there, and how fast holding it
    /// walks the trim. Keys are all or nothing, so a tap gives a nudge and a hold moves where it rests.
    pub pedal_kick: f32,
    pub pedal_rate: f32,
    pub pedal_trim_rate: f32,
    /// Full travel per second through the cyclic linkage.
    pub cyclic_rate: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GearConfig {
    /// The ends of the skids from the centre of mass.
    pub points: [Vec3; 4],
    /// N/m per point, and N s/m per point.
    pub stiffness: f32,
    pub damping: f32,
    pub friction: f32,
    /// How fast friction can stop a skid sliding, N s/m per point.
    pub friction_rate: f32,
    /// How far above and below a skid to look for the ground, m.
    pub probe_up: f32,
    pub probe_down: f32,
    /// Below this share of its weight in thrust and these speeds it stays where it stands.
    pub park_thrust: f32,
    pub park_speed: f32,
    pub park_spin: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DamageConfig {
    pub engine_hp: f32,
    pub tail_hp: f32,
    pub rotor_hp: f32,
    pub cockpit_hp: f32,
    pub structure_hp: f32,
    /// What fraction of what any zone takes also goes to the structure.
    pub structure_share: f32,
    /// Share of power left just before the engine stops, and of thrust just before the blades go.
    pub engine_floor: f32,
    pub rotor_floor: f32,
    /// Share of cyclic left with the cockpit shot out.
    pub cockpit_control: f32,
    /// Descent onto the skids that does nothing, and that kills the machine, m/s.
    pub landing_safe: f32,
    pub landing_fatal: f32,
    /// The same for the airframe hitting something.
    pub collision_safe: f32,
    pub collision_fatal: f32,
    /// Occupants are hurt by a crash faster than this, this much per m/s over it.
    pub hurt_from: f32,
    pub hurt_per_speed: f32,
    /// What being in it when it is written off does to you.
    pub wreck_hurt: f32,
    /// The tail rotor against someone on foot: how far from its hub it reaches them, what it does each
    /// second they stay there at nominal rotor speed, and the rotor speed share below which it is harmless.
    pub tail_reach: f32,
    pub tail_hurt: f32,
    pub tail_harmless: f32,
    /// How low and slow it must be to get in or out, m and m/s, from its skids to the ground.
    pub door_height: f32,
    /// Jumping out: the drop and speed that do nothing, and what each metre and m/s beyond them costs.
    pub jump_safe_height: f32,
    pub jump_safe_speed: f32,
    pub jump_hurt_per_metre: f32,
    pub jump_hurt_per_speed: f32,
    /// Seconds a wreck lies there before a new one is on the pad.
    pub respawn_after: f32,
    /// Share of a bullet's speed left after going through the skin of each part.
    pub pass_cockpit: f32,
    pub pass_structure: f32,
    /// Rotor hit points a blade strike costs each tick at nominal speed.
    pub strike_solid: f32,
    pub strike_canopy: f32,
    /// Share of rotor speed each strike takes, and the yaw moment it gives, N m.
    pub strike_brake: f32,
    pub strike_kick: f32,
    /// How many rays in the plane of the disc look for something to hit.
    pub strike_rays: u32,
    /// Per second, how fast a wreck's rotor stops, and how fast scraping along stops the rest of it.
    pub wreck_rotor_stop: f32,
    pub wreck_settle: f32,
    /// Speed and spin under which a wreck touching something has come to rest.
    pub wreck_park_speed: f32,
    /// The parts that bullets can hit, as boxes in the airframe's frame.
    pub zones: [ZoneBox; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ZoneBox {
    pub center: Vec3,
    pub half: Vec3,
    pub zone: HeliZone,
}

/// The virtual stick the mouse pushes around. Only ever a position in the end: see [`crate::VirtualStick`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StickConfig {
    /// Stick travel per radian of mouse look, so that it follows mouse sensitivity.
    pub per_radian: f32,
    /// Seconds for the stick to come halfway home on its own.
    pub centre_half_life: f32,
}

impl Default for HeliConfig {
    fn default() -> Self {
        let rotor_hub = Vec3::new(0.0, 2.4, 0.0);
        Self {
            mass: 3400.0,
            inertia: Vec3::new(16_000.0, 14_000.0, 6_000.0),
            gravity: 9.81,
            substeps: 4,
            air: AirConfig {
                density: 1.225,
                scale_height: 8500.0,
                ceiling: 700.0,
                ceiling_fade: 300.0,
                edge_margin: 60.0,
                edge_push: 0.6,
            },
            rotor: RotorConfig {
                radius: 7.32,
                blades: 2.0,
                chord: 0.53,
                lift_slope: 5.7,
                profile_drag: 0.011,
                induced_factor: 1.25,
                nominal_speed: 33.9,
                max_speed: 1.2,
                inertia: 3200.0,
                min_pitch: 0.0,
                max_pitch: 14.0f32.to_radians(),
                stall_loading: 0.14,
                stall_falloff: 0.2,
                stall_power: 5.0,
                inflow_lag: 0.12,
                wake_floor: 0.6,
                hub: rotor_hub,
                max_tilt: Vec2::new(8.0f32.to_radians(), 10.0f32.to_radians()),
                disc_lag: 0.12,
                rate_lag: 0.25,
                flapback: 0.15,
                hub_moment: 6000.0,
                vrs_start: 0.3,
                vrs_full: 0.7,
                vrs_end: 1.9,
                vrs_clear_from: 0.3,
                vrs_clear_at: 1.0,
                vrs_loss: 0.4,
                vrs_onset: 1.2,
                vrs_recovery: 1.2,
                vrs_buffet: 4000.0,
                vrs_buffet_rate: 5.0,
                ground_effect_fade: 1.5,
                ground_effect_arm: 0.05,
            },
            tail: TailConfig {
                at: Vec3::new(0.0, 0.0, 8.7),
                radius: 1.3,
                max_thrust: 5000.0,
                inflow_loss: 200.0,
                torque_mix: 0.7,
                drawn_offset: Vec3::new(-0.2, 0.92, 0.45),
            },
            engine: EngineConfig {
                max_power: 850_000.0,
                max_torque: 50_000.0,
                start_time: 20.0,
                stop_time: 8.0,
                spool_exponent: 3.0,
                power_up_rate: 0.6,
                power_down_rate: 1.0,
                governor_gain: 8.0,
                drive_loss: 25_000.0,
            },
            airframe: AirframeConfig {
                drag_area: Vec3::new(16.0, 20.0, 3.0),
                fin_area: 1.2,
                fin_slope: 4.0,
                fin_arm: 8.0,
                fin_stall: 0.4,
                stabiliser_area: 1.5,
                stabiliser_slope: 3.5,
                stabiliser_arm: 6.0,
                stabiliser_stall: 0.35,
                stabiliser_incidence: 0.135,
                damping: Vec3::new(1500.0, 1500.0, 800.0),
                probes: [
                    BodyProbe { at: Vec3::new(0.0, -0.6, -3.6), zone: HeliZone::Cockpit },
                    BodyProbe { at: Vec3::new(1.3, -0.4, 0.0), zone: HeliZone::Structure },
                    BodyProbe { at: Vec3::new(-1.3, -0.4, 0.0), zone: HeliZone::Structure },
                    BodyProbe { at: Vec3::new(0.0, 0.7, 5.0), zone: HeliZone::Structure },
                    BodyProbe { at: Vec3::new(0.0, 0.2, 9.0), zone: HeliZone::TailRotor },
                    BodyProbe { at: Vec3::new(0.0, 1.4, 9.2), zone: HeliZone::TailRotor },
                    BodyProbe { at: rotor_hub, zone: HeliZone::Engine },
                ],
                restitution: 0.15,
            },
            controls: ControlsConfig {
                collective_rate: 0.15,
                pedal_kick: 0.32,
                pedal_rate: 2.5,
                pedal_trim_rate: 0.08,
                cyclic_rate: 6.0,
            },
            gear: GearConfig {
                points: [
                    Vec3::new(-1.25, -1.7, -2.0),
                    Vec3::new(1.25, -1.7, -2.0),
                    Vec3::new(-1.25, -1.7, 1.7),
                    Vec3::new(1.25, -1.7, 1.7),
                ],
                stiffness: 160_000.0,
                damping: 14_000.0,
                friction: 0.7,
                friction_rate: 60_000.0,
                probe_up: 1.5,
                probe_down: 1.5,
                park_thrust: 0.6,
                park_speed: 0.05,
                park_spin: 0.03,
            },
            damage: DamageConfig {
                engine_hp: 300.0,
                tail_hp: 150.0,
                rotor_hp: 300.0,
                cockpit_hp: 250.0,
                structure_hp: 1200.0,
                structure_share: 0.5,
                engine_floor: 0.45,
                rotor_floor: 0.7,
                cockpit_control: 0.6,
                landing_safe: 3.0,
                landing_fatal: 9.0,
                collision_safe: 3.0,
                collision_fatal: 18.0,
                hurt_from: 4.0,
                hurt_per_speed: 7.0,
                wreck_hurt: 90.0,
                tail_reach: 1.6,
                tail_hurt: 400.0,
                tail_harmless: 0.15,
                door_height: 2.5,
                jump_safe_height: 0.8,
                jump_safe_speed: 1.5,
                jump_hurt_per_metre: 22.0,
                jump_hurt_per_speed: 9.0,
                respawn_after: 60.0,
                pass_cockpit: 0.8,
                pass_structure: 0.75,
                strike_solid: 110.0,
                strike_canopy: 25.0,
                strike_brake: 0.03,
                strike_kick: 15_000.0,
                strike_rays: 8,
                wreck_rotor_stop: 0.5,
                wreck_settle: 3.0,
                wreck_park_speed: 0.4,
                zones: [
                    ZoneBox { center: Vec3::new(0.0, 0.1, -2.6), half: Vec3::new(1.2, 1.0, 1.0), zone: HeliZone::Cockpit },
                    ZoneBox { center: Vec3::new(0.0, 1.2, 1.3), half: Vec3::new(0.7, 0.55, 1.4), zone: HeliZone::Engine },
                    ZoneBox { center: Vec3::new(0.0, 0.8, 9.0), half: Vec3::new(0.25, 1.35, 1.35), zone: HeliZone::TailRotor },
                    ZoneBox { center: Vec3::new(0.0, 0.0, 0.0), half: Vec3::new(1.3, 1.25, 1.8), zone: HeliZone::Structure },
                    ZoneBox { center: Vec3::new(0.0, 0.5, 5.0), half: Vec3::new(0.35, 0.45, 3.2), zone: HeliZone::Structure },
                ],
            },
            seats: seats(),
            entries: entries(),
            links: vec![
                SeatLink { a: 0, b: 1, time: 1.6 },
                SeatLink { a: 2, b: 3, time: 1.6 },
                // Over the back of the seats between the cockpit and the cabin, which is all one space.
                SeatLink { a: 0, b: 3, time: 2.2 },
                SeatLink { a: 1, b: 2, time: 2.2 },
            ],
            stick: StickConfig { per_radian: 6.0, centre_half_life: 0.45 },
            dials: crate::DialConfig::default(),
            net_view: crate::NetView::default(),
            wash: WashConfig { reach: 12.0, rate: 70.0, ring: 0.55, out_speed: 7.0, rise_speed: 0.9, life: 1.0, size: 0.4 },
        }
    }
}

fn cockpit_seat(x: f32) -> SeatConfig {
    SeatConfig {
        role: SeatRole::Passenger,
        eye: Vec3::new(x, 0.46, -2.32),
        facing: 0.0,
        swivel: false,
        hips: Vec3::new(x, -0.42, -2.3),
        feet: Vec3::new(x, -0.82, -2.92),
        hands: None,
        look: Arc { yaw: Vec2::new(-2.3, 2.3), pitch: Vec2::new(-1.1, 1.0) },
        fire: None,
    }
}

/// Sits in the open door with their feet out on the skid, looking and firing out of it.
fn door_seat(side: f32) -> SeatConfig {
    SeatConfig {
        role: SeatRole::Passenger,
        eye: Vec3::new(side * 1.0, 0.1, 0.1),
        facing: -side * FRAC_PI_2,
        swivel: false,
        hips: Vec3::new(side * 1.0, -0.65, 0.1),
        feet: Vec3::new(side * 1.55, -1.42, 0.1),
        hands: None,
        look: Arc { yaw: Vec2::new(-1.7, 1.7), pitch: Vec2::new(-1.3, 0.9) },
        fire: Some(Arc { yaw: Vec2::new(-1.25, 1.25), pitch: Vec2::new(-1.2, 0.5) }),
    }
}

fn entries() -> Vec<Entry> {
    let door = |name: &'static str, seat: u8, side: f32, z: f32| Entry {
        name,
        stand: Vec3::new(side * 1.9, -1.6, z),
        on_ground: true,
        hand: Vec3::new(side * 1.2, -0.3, z),
        via: Vec3::new(side * 1.25, -0.5, z),
        seats: vec![seat],
        time: 1.3,
        door: None,
    };
    vec![door("pilot's door", 0, 1.0, -2.3), door("cockpit door", 1, -1.0, -2.3), door("cabin door", 2, -1.0, 0.1), door("cabin door", 3, 1.0, 0.1)]
}

fn seats() -> Vec<SeatConfig> {
    vec![
        SeatConfig { role: SeatRole::Driver, hands: Some([Vec3::new(0.32, -0.45, -2.62), Vec3::new(0.65, -0.32, -2.68)]), ..cockpit_seat(0.65) },
        cockpit_seat(-0.65),
        door_seat(-1.0),
        door_seat(1.0),
    ]
}
