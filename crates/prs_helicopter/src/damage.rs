//! Parts that can be shot or broken, and what losing them does.

use glam::Vec3;

use crate::config::{DamageConfig, HeliConfig};
use crate::{Engine, HeliEvent, HeliState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HeliZone {
    Engine,
    TailRotor,
    MainRotor,
    Cockpit,
    Structure,
}

/// Hit points left in each part. Nothing here comes back on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneHealth {
    pub engine: f32,
    pub tail: f32,
    pub rotor: f32,
    pub cockpit: f32,
    pub structure: f32,
}

impl ZoneHealth {
    pub fn new(cfg: &DamageConfig) -> Self {
        Self {
            engine: cfg.engine_hp,
            tail: cfg.tail_hp,
            rotor: cfg.rotor_hp,
            cockpit: cfg.cockpit_hp,
            structure: cfg.structure_hp,
        }
    }
}

/// Share of full power the engine can still give.
pub fn engine_share(state: &HeliState, cfg: &HeliConfig) -> f32 {
    let d = &cfg.damage;
    if state.health.engine <= 0.0 {
        return 0.0;
    }
    d.engine_floor + (1.0 - d.engine_floor) * (state.health.engine / d.engine_hp)
}

/// Share of thrust left in the blades.
pub fn rotor_share(state: &HeliState, cfg: &HeliConfig) -> f32 {
    let d = &cfg.damage;
    if state.health.rotor <= 0.0 || state.wrecked {
        return 0.0;
    }
    d.rotor_floor + (1.0 - d.rotor_floor) * (state.health.rotor / d.rotor_hp)
}

/// Share of cyclic that still gets through the cockpit's shot up linkages.
pub fn cyclic_share(state: &HeliState, cfg: &HeliConfig) -> f32 {
    let d = &cfg.damage;
    d.cockpit_control + (1.0 - d.cockpit_control) * (state.health.cockpit / d.cockpit_hp).max(0.0)
}

/// Takes damage on one part, some of which goes to the airframe holding it all together.
pub fn apply(state: &mut HeliState, cfg: &HeliConfig, zone: HeliZone, amount: f32) {
    let h = &mut state.health;
    match zone {
        HeliZone::Engine => h.engine -= amount,
        HeliZone::TailRotor => h.tail -= amount,
        HeliZone::MainRotor => h.rotor -= amount,
        HeliZone::Cockpit => h.cockpit -= amount,
        HeliZone::Structure => {}
    }
    let share = if zone == HeliZone::Structure { 1.0 } else { cfg.damage.structure_share };
    h.structure -= amount * share;
    state.parked = false;
}

/// Turns hit points that have gone with this tick into their effects.
pub fn consequences(state: &mut HeliState, _cfg: &HeliConfig, events: &mut Vec<HeliEvent>) {
    let h = &mut state.health;
    if h.engine <= 0.0 && h.engine > f32::NEG_INFINITY {
        h.engine = f32::NEG_INFINITY;
        if state.engine != Engine::Dead {
            state.engine = Engine::Dead;
            events.push(HeliEvent::EngineDied);
        }
    }
    if h.tail <= 0.0 && h.tail > f32::NEG_INFINITY {
        h.tail = f32::NEG_INFINITY;
        events.push(HeliEvent::TailRotorLost);
    }
    if h.rotor <= 0.0 && h.rotor > f32::NEG_INFINITY {
        h.rotor = f32::NEG_INFINITY;
        events.push(HeliEvent::RotorLost);
    }
    if h.structure <= 0.0 && !state.wrecked {
        state.wrecked = true;
        state.parked = false;
        if state.engine != Engine::Dead {
            state.engine = Engine::Dead;
        }
        events.push(HeliEvent::Wrecked);
    }
}

/// Share of a bullet's speed left after going through this part, or none if it stops there.
pub fn passes(cfg: &DamageConfig, zone: HeliZone) -> Option<f32> {
    match zone {
        HeliZone::Cockpit => Some(cfg.pass_cockpit),
        HeliZone::Structure => Some(cfg.pass_structure),
        HeliZone::Engine | HeliZone::TailRotor | HeliZone::MainRotor => None,
    }
}

/// What a bullet on this line hits first, and how far along. The blades are thin, and only a line
/// that crosses the disc where a blade is right now finds them. A line starting inside a part is on
/// its way out of it.
pub fn hit_zone(state: &HeliState, cfg: &HeliConfig, origin: Vec3, dir: Vec3, length: f32) -> Option<(HeliZone, f32)> {
    let inverse = state.rot.inverse();
    let local_origin = inverse * (origin - state.pos);
    let local_dir = inverse * dir;
    let mut best: Option<(HeliZone, f32)> = None;
    for zone in &cfg.damage.zones {
        if let Some(t) = ray_box(local_origin, local_dir, zone.center, zone.half)
            && t <= length
            && best.is_none_or(|(_, b)| t < b)
        {
            best = Some((zone.zone, t));
        }
    }
    if let Some(t) = ray_blade(state, cfg, local_origin, local_dir)
        && t <= length
        && best.is_none_or(|(_, b)| t < b)
    {
        best = Some((HeliZone::MainRotor, t));
    }
    best
}

fn ray_box(origin: Vec3, dir: Vec3, center: Vec3, half: Vec3) -> Option<f32> {
    let inverse = dir.recip();
    let a = (center - half - origin) * inverse;
    let b = (center + half - origin) * inverse;
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    (far >= near && near >= 0.0).then_some(near)
}

fn ray_blade(state: &HeliState, cfg: &HeliConfig, origin: Vec3, dir: Vec3) -> Option<f32> {
    let r = &cfg.rotor;
    let normal = state.disc_normal();
    let across = dir.dot(normal);
    if across.abs() < 1e-4 {
        return None;
    }
    let t = (r.hub - origin).dot(normal) / across;
    if t < 0.0 {
        return None;
    }
    let point = origin + dir * t - r.hub;
    let radius = point.length();
    if radius > r.radius {
        return None;
    }
    // A teetering rotor's two blades lie along one line, so the angle only matters modulo half a turn.
    let blade = Vec3::new(state.rotor_angle.cos(), 0.0, -state.rotor_angle.sin());
    let along = point.dot(blade);
    let off = (point - blade * along).length();
    (off <= r.chord * 0.5).then_some(t)
}
