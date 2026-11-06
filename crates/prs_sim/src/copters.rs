//! Helicopters in the game. The flying is all in `prs_helicopter`. This says who is flying, what a crash
//! does to the people inside, and keeps them on the same seats as anything else that carries people.

use crate::*;
use prs_helicopter::{HeliConfig, HeliEvent, HeliInput, HeliState, HeliWorld, HeliZone};
use prs_seats::{Frame, Occupants, SeatedAt};

/// Carrier ids from here up are helicopters, and below it they are vehicles.
pub const HELI_CARRIERS: u32 = 1 << 16;

/// The pilot's seat. Everyone else is along for the ride.
pub const PILOT_SEAT: usize = 0;

/// One helicopter in the game. Plain data, so copying this is all there is to saving it.
#[derive(Clone, Debug, PartialEq)]
pub struct Helicopter {
    pub state: HeliState,
    pub occupants: Occupants,
    /// The pad it comes back to, on the ground, and which way it faces there.
    pub home: Vec3,
    pub home_yaw: f32,
    /// Seconds it has been a wreck.
    pub wrecked_for: f32,
}

impl Helicopter {
    pub(crate) fn new(cfg: &HeliConfig, home: Vec3, home_yaw: f32) -> Self {
        Self {
            state: HeliState::parked_at(cfg, home, home_yaw),
            occupants: Occupants::empty(cfg.seats.len()),
            home,
            home_yaw,
            wrecked_for: 0.0,
        }
    }

    pub fn frame(&self) -> Frame {
        Frame { pos: self.state.pos, rot: self.state.rot }
    }

    pub fn pilot(&self) -> Option<SoldierId> {
        self.occupants.0.get(PILOT_SEAT).copied().flatten().map(|who| who as usize)
    }
}

/// The carrier id of a helicopter.
pub fn heli_carrier(heli: usize) -> u32 {
    HELI_CARRIERS + heli as u32
}

/// The helicopter a carrier id refers to, if it is one.
pub fn heli_of(carrier: u32) -> Option<usize> {
    carrier.checked_sub(HELI_CARRIERS).map(|i| i as usize)
}

/// What a command means to someone at the controls.
fn controls(cmd: &PlayerCmd, last: Buttons) -> HeliInput {
    let [x, y] = cmd.stick;
    HeliInput {
        collective: cmd.wish_move.y,
        pedals: cmd.wish_move.x,
        stick: Vec2::new(x as f32, y as f32) / 127.0,
        engine_switch: cmd.buttons.contains(Buttons::JUMP) && !last.contains(Buttons::JUMP),
    }
}

/// Getting in and out of a helicopter, and what riding in one does to a gun held in the door.
pub(crate) fn default_ride() -> prs_vehicles::RideConfig {
    prs_vehicles::RideConfig { board_reach: 1.1, board_climb: 0.8, board_facing: 0.2, board_speed: 1.5, exit_speed: 5.0, jump_hurt: 12.0, exit_drop: 3.0, tilt_rate: 5.0, look_settle: 2.5, speed_shake: 0.04, turn_shake: 0.5, stand_hips: 0.92 }
}

impl Sim {
    /// A helicopter on every helipad the level has.
    pub(crate) fn park_helicopters(level: &Level, cfg: &HeliConfig) -> Vec<Helicopter> {
        level.helipads.iter().map(|pad| Helicopter::new(cfg, pad.pos, pad.yaw)).collect()
    }

    /// Changes how helicopters fly, as a mod would. Do this before anyone spawns.
    pub fn set_helicopter(&mut self, cfg: HeliConfig) {
        self.helicopters = Self::park_helicopters(&self.level, &cfg);
        self.heli_cfg = cfg;
    }

    /// The rest of a soldier's commands for this tick, for a pilot.
    fn heli_input(&self, heli: &Helicopter, cmds: &[(SoldierId, PlayerCmd)]) -> HeliInput {
        let Some(pilot) = heli.pilot() else {
            return HeliInput::default();
        };
        let soldier = &self.soldiers[pilot];
        match cmds.iter().find(|(who, _)| *who == pilot) {
            Some((_, cmd)) if soldier.health.is_up() && soldier.transit.is_none() => controls(cmd, soldier.last_buttons),
            _ => HeliInput::default(),
        }
    }

    pub(crate) fn step_helicopters(&mut self, cmds: &[(SoldierId, PlayerCmd)]) {
        for h in 0..self.helicopters.len() {
            let input = self.heli_input(&self.helicopters[h], cmds);
            self.step_one_helicopter(h, &input);
            self.tail_rotor_hazard(h);
        }
    }

    /// Whoever walks into a turning tail rotor regrets it. It is the one thing about a helicopter on the
    /// ground that can kill you without it moving.
    fn tail_rotor_hazard(&mut self, h: usize) {
        let d = &self.heli_cfg.damage;
        let state = &self.helicopters[h].state;
        let share = state.rotor_fraction(&self.heli_cfg);
        if share < d.tail_harmless || state.health.tail <= 0.0 {
            return;
        }
        let (hub, reach) = (self.helicopters[h].frame().point(self.heli_cfg.tail_rotor_hub()), d.tail_reach);
        let hurt = d.tail_hurt * share * TICK_DT;
        for id in 0..self.soldiers.len() {
            let soldier = &self.soldiers[id];
            if !soldier.present || soldier.seat.is_some() || !soldier.health.is_up() {
                continue;
            }
            // Nearest point on the upright line of their body.
            let feet = soldier.body.pos;
            let height = (hub.y - feet.y).clamp(0.0, soldier.body.eye_height);
            if (feet + Vec3::Y * height).distance(hub) <= reach {
                self.hurt(id, Zone::Chest, hurt);
            }
        }
    }

    /// Whether someone at the door could get in or out of this one now: low, slow and in one piece.
    pub(crate) fn heli_at_door_height(&self, h: usize) -> bool {
        let state = &self.helicopters[h].state;
        let d = &self.heli_cfg.damage;
        let reach = d.door_height - self.heli_cfg.gear.points[0].y;
        state.vel.length() <= self.heli_ride.exit_speed && self.world.raycast(state.pos, Vec3::NEG_Y, reach).is_some()
    }

    /// Jumping down from it: what the drop and the speed do to them.
    pub(crate) fn jump_out_hurts(&mut self, id: SoldierId, h: usize, feet: Vec3) {
        let state = &self.helicopters[h].state;
        if state.on_ground {
            return;
        }
        let d = &self.heli_cfg.damage;
        let drop = (state.pos.y + self.heli_cfg.gear.points[0].y - feet.y - d.jump_safe_height).max(0.0);
        let speed = (state.vel.length() - d.jump_safe_speed).max(0.0);
        let hurt = drop * d.jump_hurt_per_metre + speed * d.jump_hurt_per_speed;
        if hurt > 0.0 {
            self.hurt(id, Zone::Leg, hurt);
        }
    }

    fn step_one_helicopter(&mut self, h: usize, input: &HeliInput) {
        let mut happened = std::mem::take(&mut self.heli_events);
        happened.clear();
        let world = HeliWorld { solid: &self.world, canopy: &self.canopies, half_size: self.level.terrain.size * 0.5 };
        prs_helicopter::step(&mut self.helicopters[h].state, &self.heli_cfg, input, &world, TICK_DT, &mut happened);
        self.heli_happened(h, &happened);
        self.heli_events = happened;
        if self.helicopters[h].state.wrecked {
            let heli = &mut self.helicopters[h];
            heli.wrecked_for += TICK_DT;
            if heli.wrecked_for >= self.heli_cfg.damage.respawn_after {
                self.eject_all_from(HELI_CARRIERS + h as u32);
                let heli = &self.helicopters[h];
                self.helicopters[h] = Helicopter::new(&self.heli_cfg, heli.home, heli.home_yaw);
                self.events.push(SimEvent::Heli { heli: h, event: HeliEvent::Respawned });
            }
        }
    }

    /// A pilot running ahead of the server on their own machine. Nothing else moves.
    pub(crate) fn step_predicted_helicopter(&mut self, id: SoldierId, h: usize, cmd: &PlayerCmd) {
        if self.helicopters[h].pilot() == Some(id) && self.soldiers[id].transit.is_none() {
            let input = controls(cmd, self.soldiers[id].last_buttons);
            let world = HeliWorld { solid: &self.world, canopy: &self.canopies, half_size: self.level.terrain.size * 0.5 };
            let mut happened = Vec::new();
            prs_helicopter::step(&mut self.helicopters[h].state, &self.heli_cfg, &input, &world, TICK_DT, &mut happened);
        }
    }

    /// Whatever follows from what has happened to a helicopter.
    fn heli_happened(&mut self, h: usize, happened: &[HeliEvent]) {
        for &event in happened {
            self.events.push(SimEvent::Heli { heli: h, event });
            let d = &self.heli_cfg.damage;
            let (hurt_from, per_speed, wreck_hurt) = (d.hurt_from, d.hurt_per_speed, d.wreck_hurt);
            match event {
                HeliEvent::Crunch { speed, .. } if speed > hurt_from => {
                    for who in self.heli_occupants(h) {
                        self.hurt(who, Zone::Chest, (speed - hurt_from) * per_speed);
                    }
                }
                HeliEvent::Wrecked => {
                    for who in self.heli_occupants(h) {
                        self.hurt(who, Zone::Chest, wreck_hurt);
                    }
                    self.eject_all_from(HELI_CARRIERS + h as u32);
                }
                _ => {}
            }
        }
    }

    fn heli_occupants(&self, h: usize) -> Vec<SoldierId> {
        self.helicopters[h].occupants.0.iter().flatten().map(|&who| who as usize).collect()
    }

    /// Someone on foot walks round a helicopter rather than through it, and is pushed clear if it lands on them.
    pub(crate) fn keep_off_helicopters(&mut self, id: SoldierId) {
        let capsule = self.move_cfg.capsule(self.soldiers[id].body.stance);
        let half_height = capsule.height * 0.5;
        let middle = self.soldiers[id].body.pos + Vec3::Y * half_height;
        for heli in &self.helicopters {
            let local = heli.frame().to_local(middle);
            for zone in &self.heli_cfg.damage.zones {
                let from = local - zone.center;
                let room = zone.half + Vec3::new(capsule.radius, half_height, capsule.radius);
                let over = room - from.abs();
                if over.min_element() <= 0.0 {
                    continue;
                }
                let out = if over.x < over.z { Vec3::X * over.x * from.x.signum() } else { Vec3::Z * over.z * from.z.signum() };
                let push = heli.state.rot * out;
                self.soldiers[id].body.pos += Vec3::new(push.x, 0.0, push.z);
                return;
            }
        }
    }

    /// A bullet into a helicopter, at the part it found.
    pub fn bullet_into_helicopter(&mut self, h: usize, zone: HeliZone, raw: f32) {
        prs_helicopter::damage::apply(&mut self.helicopters[h].state, &self.heli_cfg, zone, raw);
        let mut happened = Vec::new();
        prs_helicopter::damage::consequences(&mut self.helicopters[h].state, &self.heli_cfg, &mut happened);
        self.heli_happened(h, &happened);
    }

    /// The first part of any helicopter this stretch of flight touches. A gunner's own helicopter is
    /// not in the way of their own bullets.
    pub(crate) fn strike_helicopter(&self, from: Vec3, dir: Vec3, length: f32, flying: &FlyingBullet) -> Option<(usize, HeliZone, f32)> {
        let own = self.soldiers[flying.shooter].seat.and_then(|SeatedAt { carrier, .. }| heli_of(carrier));
        let mut nearest: Option<(usize, HeliZone, f32)> = None;
        for (h, heli) in self.helicopters.iter().enumerate() {
            if own == Some(h) {
                continue;
            }
            let mut state = heli.state;
            if let Some(frame) = self.past.helicopter_seen(h, self.tick, flying.rewind) {
                state.pos = frame.pos;
                state.rot = frame.rot;
            }
            if let Some((zone, distance)) = prs_helicopter::hit_zone(&state, &self.heli_cfg, from, dir, length)
                && nearest.is_none_or(|n| distance < n.2)
            {
                nearest = Some((h, zone, distance));
            }
        }
        nearest
    }
}
