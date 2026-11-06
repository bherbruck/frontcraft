//! Bullets and what is left afterwards. A vehicle stops working in stages: tyres go, the engine fades
//! and dies, the hull is holed, and then it burns and goes up. Nobody aims at a health bar.

use glam::Vec3;
use prs_collision::{ray_vs_bone, ray_vs_box};
use prs_seats::Frame;

use crate::{DamageConfig, PanelZone, VehicleConfig, VehicleEvent, VehicleState};

/// What's left of a vehicle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damage {
    pub hull: f32,
    pub engine: f32,
    /// Seconds it has been on fire, or `None` if it is not.
    pub burning: Option<f32>,
    pub wrecked: bool,
    /// Seconds since it went up.
    pub wreck_age: f32,
}

impl Damage {
    pub fn new(cfg: &DamageConfig) -> Self {
        Self { hull: cfg.hull, engine: cfg.engine, burning: None, wrecked: false, wreck_age: 0.0 }
    }

    /// How much of its power it has left, from 1 down to none.
    pub fn power_share(&self, cfg: &DamageConfig) -> f32 {
        if self.wrecked {
            return 0.0;
        }
        (self.engine / cfg.engine.max(1.0)).clamp(0.0, 1.0).powf(cfg.power_curve)
    }

    /// Whether anyone looking should see smoke.
    pub fn smoking(&self, cfg: &DamageConfig) -> bool {
        !self.wrecked && (self.engine < cfg.engine * cfg.smoke_below || self.hull < cfg.hull * cfg.smoke_below)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleZone {
    Body,
    Engine,
    Tyre(u8),
}

/// A bullet finding a part of a vehicle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub distance: f32,
    pub normal: Vec3,
    pub zone: VehicleZone,
    /// Share of its speed the bullet keeps going with. Zero stops it.
    pub pass: f32,
}

/// The first part of a vehicle a stretch of bullet flight touches, with the vehicle at `frame`. Only
/// panels that are there get hit, doors where they have swung to: where there is no panel the bullet goes on to whoever is inside.
pub fn strike(state: &VehicleState, cfg: &VehicleConfig, doors: &[f32], frame: &Frame, from: Vec3, dir: Vec3, length: f32) -> Option<Strike> {
    // Inside this it cannot possibly touch anything, which is true of nearly every bullet.
    let reach = cfg.reach();
    let along = (frame.pos - from).dot(dir).clamp(0.0, length);
    if (from + dir * along).distance_squared(frame.pos) > reach * reach {
        return None;
    }
    let mut nearest: Option<Strike> = None;
    for (i, panel) in cfg.panels.iter().enumerate() {
        let (center, rotation) = cfg.panel_at(i, doors);
        if let Some((distance, normal)) = ray_vs_box(from, dir, length, frame.point(center), panel.half, frame.rot * rotation)
            && nearest.is_none_or(|n| distance < n.distance)
        {
            let zone = match panel.zone {
                PanelZone::Body => VehicleZone::Body,
                PanelZone::Engine => VehicleZone::Engine,
            };
            nearest = Some(Strike { distance, normal, zone, pass: panel.pass });
        }
    }
    let side = frame.rot * Vec3::X;
    for (i, wheel) in cfg.wheels.iter().enumerate() {
        let axle = frame.point(state.axle(cfg, i));
        let half = side * (wheel.width * 0.5);
        if let Some(distance) = ray_vs_bone(from, dir, length, axle - half, axle + half, wheel.radius)
            && nearest.is_none_or(|n| distance < n.distance)
        {
            let at = from + dir * distance;
            nearest = Some(Strike { distance, normal: (at - axle).normalize_or(Vec3::Y), zone: VehicleZone::Tyre(i as u8), pass: 0.0 });
        }
    }
    nearest
}

/// A bullet's damage landing on a part of the vehicle.
pub fn hurt(state: &mut VehicleState, cfg: &DamageConfig, zone: VehicleZone, raw: f32, events: &mut Vec<VehicleEvent>) {
    let d = &mut state.damage;
    if d.wrecked {
        return;
    }
    match zone {
        VehicleZone::Body => d.hull -= raw * cfg.hull_scale,
        VehicleZone::Engine => {
            let was = d.engine;
            d.engine = (d.engine - raw * cfg.engine_scale).max(0.0);
            if was > 0.0 && d.engine <= 0.0 {
                events.push(VehicleEvent::EngineDead);
            } else if was <= 0.0 {
                // A dead engine still has fuel in it.
                d.hull -= raw * cfg.engine_scale;
                ignite(d, events);
            }
        }
        VehicleZone::Tyre(wheel) => {
            let w = &mut state.wheels[wheel as usize];
            let was = w.tyre;
            w.tyre = (w.tyre - raw * cfg.tyre_scale).max(0.0);
            if was > 0.0 && w.tyre <= 0.0 {
                events.push(VehicleEvent::TyreBurst { wheel });
                crate::wake(state);
            }
        }
    }
    if state.damage.hull <= 0.0 {
        ignite(&mut state.damage, events);
    }
}

/// What a hard knock takes off the hull.
pub fn crash(state: &mut VehicleState, cfg: &DamageConfig, speed: f32, events: &mut Vec<VehicleEvent>) {
    state.damage.hull -= (speed - cfg.crash_from).max(0.0) * cfg.crash_hull;
    if state.damage.hull <= 0.0 {
        ignite(&mut state.damage, events);
    }
}

fn ignite(d: &mut Damage, events: &mut Vec<VehicleEvent>) {
    if d.burning.is_none() && !d.wrecked {
        d.burning = Some(0.0);
        events.push(VehicleEvent::OnFire);
    }
}

/// Fire and the clock that follows it. Says when a new vehicle is due.
pub fn step_damage(state: &mut VehicleState, cfg: &DamageConfig, dt: f32, events: &mut Vec<VehicleEvent>) -> bool {
    let point = state.pos;
    // One left on its roof goes the way of a wreck, unless someone comes and rights it first.
    let over = (state.rot * Vec3::Y).y < cfg.righted_above && state.occupants.is_empty();
    state.abandoned_for = if over { state.abandoned_for + dt } else { 0.0 };
    if state.abandoned_for >= cfg.respawn_after {
        return true;
    }
    let d = &mut state.damage;
    if d.wrecked {
        d.wreck_age += dt;
        return d.wreck_age >= cfg.respawn_after;
    }
    if let Some(burned) = &mut d.burning {
        *burned += dt;
        d.hull -= cfg.burn_rate * dt;
        d.engine = 0.0;
        if *burned >= cfg.burn_time {
            d.burning = None;
            d.wrecked = true;
            d.wreck_age = 0.0;
            events.push(VehicleEvent::Exploded { point });
            crate::wake(state);
        }
    }
    false
}
