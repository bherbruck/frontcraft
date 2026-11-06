//! Treating the wounded: holding fire with a bandage or medkit selected, on an ally in front of you if
//! there is one who needs it and on yourself if not. It takes seconds of standing still with your gun
//! down, and moving, letting go or being hit starts it over.

use crate::*;
use prs_body::{CareKit, CarePose, CareSite};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Care {
    Bandage,
    Medkit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Treatment {
    pub care: Care,
    pub patient: SoldierId,
    /// Seconds still to go.
    pub left: f32,
    /// Seconds it takes in all, for showing how far along it is.
    pub total: f32,
}

/// Where a dressing has gone on, for drawing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dressed {
    pub arm: bool,
    pub leg: bool,
}

/// The worse of a limb wound if there is one, and the trunk if not.
fn wound_site(health: &Health) -> CareSite {
    if health.arm_wound > 0.0 && health.arm_wound >= health.leg_wound {
        CareSite::Arm
    } else if health.leg_wound > 0.0 {
        CareSite::Leg
    } else {
        CareSite::Chest
    }
}

impl Sim {
    /// Where the hands go for a treatment.
    fn care_site(&self, id: SoldierId, patient: SoldierId) -> CareSite {
        if patient != id {
            return CareSite::Ally;
        }
        wound_site(&self.soldiers[id].health)
    }

    /// The treatment under way as a body needs it, the same for whoever works it out.
    pub fn care_pose(&self, id: SoldierId) -> Option<CarePose> {
        let soldier = &self.soldiers[id];
        soldier.treatment.filter(|_| soldier.weapon.ready <= 0.0).map(|t| CarePose {
            site: self.care_site(id, t.patient),
            kit: match t.care {
                Care::Bandage => CareKit::Bandage,
                Care::Medkit => CareKit::Medkit,
            },
            progress: (1.0 - t.left / t.total.max(1e-3)).clamp(0.0, 1.0),
        })
    }

    /// Whether this soldier could usefully be given this care.
    fn wants(&self, patient: SoldierId, care: Care) -> bool {
        let health = &self.soldiers[patient].health;
        match care {
            Care::Bandage => health.wants_bandage(),
            Care::Medkit => health.wants_medkit(&self.health_cfg),
        }
    }

    /// Who you would be treating right now, if anyone.
    fn choose_patient(&self, id: SoldierId, care: Care) -> Option<SoldierId> {
        let me = &self.soldiers[id];
        let (forward, _) = yaw_basis(me.yaw);
        let cfg = &self.health_cfg;
        let ally = self
            .soldiers
            .iter()
            .enumerate()
            .filter(|(other, s)| *other != id && s.present && s.team == me.team && self.wants(*other, care))
            .filter_map(|(other, s)| {
                let to = s.body.pos - me.body.pos;
                let flat = Vec3::new(to.x, 0.0, to.z);
                let close = to.length() <= cfg.care_reach;
                let facing = flat.normalize_or(forward).dot(forward) >= cfg.care_facing;
                // The downed come first, then whoever is closest.
                (close && facing).then_some((other, (!s.health.is_up(), -to.length())))
            })
            .max_by(|a, b| a.1.0.cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)))
            .map(|(other, _)| other);
        ally.or_else(|| self.wants(id, care).then_some(id))
    }

    fn care_time(&self, patient: SoldierId, care: Care) -> f32 {
        let cfg = &self.health_cfg;
        match care {
            Care::Bandage => cfg.bandage_time,
            Care::Medkit if self.soldiers[patient].health.is_up() => cfg.medkit_time,
            Care::Medkit => cfg.revive_time,
        }
    }

    /// Stops a treatment under way, if there is one.
    /// Whatever their hands are doing that something can break into.
    pub(crate) fn interrupt_hands(&mut self, id: SoldierId) {
        self.interrupt_care(id);
        self.soldiers[id].lifting = None;
        self.soldiers[id].pushing = None;
    }

    pub(crate) fn interrupt_care(&mut self, id: SoldierId) {
        if let Some(t) = self.soldiers[id].treatment.take() {
            let event = CarryEvent::TreatmentInterrupted { care: t.care, patient: t.patient };
            self.events.push(SimEvent::Carry { soldier: id, event });
        }
    }

    /// `trigger` is fire held, which uses whatever is selected. `own_bandage` is B held, which is a
    /// bandage on yourself whatever is selected, and works walking about or in a seat. Going through a
    /// door or across to another seat stops either.
    pub(crate) fn step_care(&mut self, id: SoldierId, trigger: bool, own_bandage: bool) {
        // A wound that has gone needs no dressing.
        let soldier = &mut self.soldiers[id];
        soldier.dressed.arm &= soldier.health.arm_wound > 0.0;
        soldier.dressed.leg &= soldier.health.leg_wound > 0.0;
        let soldier = &self.soldiers[id];
        let own_bandage = own_bandage && soldier.inventory.stash.bandages > 0 && soldier.health.wants_bandage();
        let care = match soldier.inventory.selected {
            _ if own_bandage => Care::Bandage,
            Slot::Bandage => Care::Bandage,
            Slot::Medkit => Care::Medkit,
            _ => {
                self.interrupt_care(id);
                return;
            }
        };
        if own_bandage && soldier.treatment.is_some_and(|t| t.care != Care::Bandage || t.patient != id) {
            self.interrupt_care(id);
        }
        let soldier = &self.soldiers[id];
        let own_dressing = own_bandage || soldier.treatment.is_some_and(|t| t.care == Care::Bandage && t.patient == id);
        let walks_on = own_dressing && self.health_cfg.bandage_on_the_move;
        let moving = soldier.seat.is_none() && soldier.body.horizontal_speed() > self.health_cfg.care_max_speed;
        if !(trigger || own_bandage) || (moving && !walks_on) || !soldier.health.is_up() || soldier.transit.is_some() {
            self.interrupt_care(id);
            return;
        }
        let Some(mut treatment) = soldier.treatment else {
            let patient = if own_bandage { Some(id) } else { self.choose_patient(id, care) };
            if let Some(patient) = patient {
                let total = self.care_time(patient, care);
                self.soldiers[id].treatment = Some(Treatment { care, patient, left: total, total });
                let event = CarryEvent::TreatmentStarted { care, patient };
                self.events.push(SimEvent::Carry { soldier: id, event });
            }
            return;
        };

        let patient = treatment.patient;
        let to = self.soldiers[patient].body.pos - soldier.body.pos;
        if !self.wants(patient, care) || to.length() > self.health_cfg.care_reach {
            self.interrupt_care(id);
            return;
        }
        // The dressing is on its way from the moment you go for it, but the work takes both hands, and those
        // are not free until the gun is out of the way.
        if soldier.weapon.ready > 0.0 {
            return;
        }
        treatment.left -= TICK_DT;
        if treatment.left > 0.0 {
            self.soldiers[id].treatment = Some(treatment);
            return;
        }

        self.health_events.clear();
        let cfg = &self.health_cfg;
        let health = &mut self.soldiers[patient].health;
        match care {
            Care::Bandage => health.bandage(&mut self.health_events),
            Care::Medkit => health.medkit(cfg, &mut self.health_events),
        }
        self.settle_health(patient);
        let site = wound_site(&self.soldiers[patient].health);
        let dressed = &mut self.soldiers[patient].dressed;
        dressed.arm |= site == CareSite::Arm;
        dressed.leg |= site == CareSite::Leg;

        let soldier = &mut self.soldiers[id];
        match care {
            Care::Bandage => soldier.inventory.stash.bandages -= 1,
            Care::Medkit => soldier.inventory.stash.medkits -= 1,
        }
        soldier.treatment = None;
        // Back to the gun, which will not fire until the finger comes off. B took nothing out, so whatever
        // was in hand is still what is selected.
        if !own_bandage {
            soldier.inventory.selected = soldier.inventory.held;
            soldier.hold_fire = true;
        }
        let event = CarryEvent::TreatmentFinished { care, patient };
        self.events.push(SimEvent::Carry { soldier: id, event });
    }
}
