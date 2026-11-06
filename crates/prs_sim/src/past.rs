//! Where everyone was on recent ticks. A server keeps this so that a bullet can be tested against
//! the world its shooter was looking at, which is a little in the past. Only the inputs to a body are
//! kept, because bones are costly and nearly all of these will never be looked at.

use crate::*;
use std::collections::VecDeque;

#[derive(Default)]
pub(crate) struct PastBodies {
    keep: u32,
    /// Oldest first, one for each tick, each with one body per soldier slot, then vehicles and helicopters.
    ticks: VecDeque<(u32, Vec<Option<BodyInput>>, Vec<prs_seats::Frame>, Vec<prs_seats::Frame>)>,
}

impl PastBodies {
    /// How this soldier stood when the shooter saw them, `rewind` ticks before `now`. `None` if that
    /// is the present, or further back than is on record, or they were not there then.
    pub(crate) fn seen(&self, id: SoldierId, now: u32, rewind: u32) -> Option<&BodyInput> {
        if rewind == 0 {
            return None;
        }
        let tick = now.checked_sub(rewind)?;
        let oldest = self.ticks.front()?.0;
        let index = tick.checked_sub(oldest)? as usize;
        self.ticks.get(index)?.1.get(id)?.as_ref()
    }

    /// Where this vehicle was when the shooter saw it, on the same terms as [`Self::seen`].
    pub(crate) fn vehicle_seen(&self, vehicle: usize, now: u32, rewind: u32) -> Option<prs_seats::Frame> {
        if rewind == 0 {
            return None;
        }
        let tick = now.checked_sub(rewind)?;
        let oldest = self.ticks.front()?.0;
        let index = tick.checked_sub(oldest)? as usize;
        self.ticks.get(index)?.2.get(vehicle).copied()
    }

    /// The same for a helicopter.
    pub(crate) fn helicopter_seen(&self, heli: usize, now: u32, rewind: u32) -> Option<prs_seats::Frame> {
        if rewind == 0 {
            return None;
        }
        let tick = now.checked_sub(rewind)?;
        let oldest = self.ticks.front()?.0;
        let index = tick.checked_sub(oldest)? as usize;
        self.ticks.get(index)?.3.get(heli).copied()
    }
}

impl Sim {
    /// Starts keeping this many ticks of where everyone was. A sim with no remote shooters keeps none.
    pub fn remember_past_ticks(&mut self, ticks: u32) {
        self.past.keep = ticks;
        self.past.ticks.clear();
    }

    /// Tells the sim how far in the past this soldier is seeing everyone else, for the shots they fire.
    /// Anything beyond what is on record just uses the present.
    pub fn set_view_lag(&mut self, id: SoldierId, ticks: u32) {
        if self.view_lag.len() <= id {
            self.view_lag.resize(id + 1, 0);
        }
        self.view_lag[id] = ticks;
    }

    pub(crate) fn remember_bodies(&mut self) {
        if self.past.keep == 0 {
            return;
        }
        let bodies = (0..self.soldiers.len())
            .map(|id| {
                let soldier = &self.soldiers[id];
                (soldier.present && !soldier.health.is_dead()).then(|| self.body_input(id))
            })
            .collect();
        let vehicles = self.vehicles.iter().map(VehicleState::frame).collect();
        let helicopters = self.helicopters.iter().map(Helicopter::frame).collect();
        self.past.ticks.push_back((self.tick, bodies, vehicles, helicopters));
        while self.past.ticks.len() > self.past.keep as usize {
            self.past.ticks.pop_front();
        }
    }
}
