//! World state that changes rarely and that nobody predicts: what is on the ground, which targets are
//! standing, who is sitting in what and what everyone is called. Sent whole, and only when it has changed.

use prs_net::wire::{Reader, WireError, Writer, read_text, write_text};
use prs_sim::throwables::Cloud;
use prs_sim::{DroppedKit, Sim};

use prs_mode::MatchState;

use crate::mode::{read_state, write_state};
use crate::own::{read_stash, write_stash};
use crate::rides::{read_occupants, write_occupants};

const MAX_ITEMS: usize = 4096;
/// A client only needs to know that a target is down. The server says when it stands back up.
const DOWN: f32 = 1.0;

/// What the players are called, by number. Only ever shown, never decides anything.
pub type Names = Vec<(u16, String)>;

pub(crate) fn write(sim: &Sim, state: &MatchState, names: &Names) -> Vec<u8> {
    let mut w = Writer::default();
    write_state(state, &mut w);
    w.varu(sim.targets.len() as u64);
    for target in &sim.targets {
        w.varu(target.hits as u64);
        w.u8(target.is_up() as u8);
    }
    w.varu(sim.kits.len() as u64);
    for kit in &sim.kits {
        w.varu(kit.id as u64);
        w.vec3(kit.pos);
        write_stash(&kit.stash, &mut w);
    }
    // For anyone joining with smoke already hanging about. Anyone here has heard it go off.
    w.varu(sim.clouds.len() as u64);
    for cloud in &sim.clouds {
        w.varu(cloud.id as u64);
        w.vec3(cloud.pos);
        w.varu(cloud.age as u64);
    }
    w.varu(sim.vehicles.len() as u64);
    for vehicle in &sim.vehicles {
        write_occupants(&vehicle.occupants, &mut w);
    }
    w.varu(sim.helicopters.len() as u64);
    for heli in &sim.helicopters {
        write_occupants(&heli.occupants, &mut w);
    }
    w.varu(names.len() as u64);
    for (id, name) in names {
        w.u16(*id);
        write_text(name, &mut w);
    }
    w.buf
}

pub(crate) fn apply(sim: &mut Sim, bytes: &[u8]) -> Result<(MatchState, Names), WireError> {
    let mut r = Reader::new(bytes);
    let state = read_state(&mut r)?;
    for i in 0..r.count(MAX_ITEMS)? {
        let hits = r.varu32()?;
        let up = r.u8()? != 0;
        if let Some(target) = sim.targets.get_mut(i) {
            target.hits = hits;
            target.down_for = if up { 0.0 } else { DOWN };
        }
    }
    sim.kits.clear();
    for _ in 0..r.count(MAX_ITEMS)? {
        // Age is left at zero: a client doesn't expire kits, it hears when they are gone.
        let (id, pos, stash) = (r.varu32()?, r.vec3()?, read_stash(&mut r)?);
        sim.kits.push(DroppedKit { id, pos, stash, age: 0.0 });
    }
    let mut listed = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        let (id, pos, age) = (r.varu32()?, r.vec3()?, r.varu32()? as f32);
        listed.push(id);
        match sim.clouds.iter_mut().find(|c| c.id == id) {
            // Its own copy of the grenade is showing it falling. Once that is done, this says where it ended up.
            Some(cloud) => {
                if !sim.thrown.iter().any(|t| t.id == id) {
                    cloud.pos = pos;
                }
            }
            None => sim.clouds.push(Cloud { id, pos, age }),
        }
    }
    sim.clouds.retain(|c| listed.contains(&c.id));
    for i in 0..r.count(MAX_ITEMS)? {
        let occupants = read_occupants(&mut r)?;
        if let Some(vehicle) = sim.vehicles.get_mut(i) {
            vehicle.occupants = occupants;
        }
    }
    for i in 0..r.count(MAX_ITEMS)? {
        let occupants = read_occupants(&mut r)?;
        if let Some(heli) = sim.helicopters.get_mut(i) {
            heli.occupants = occupants;
        }
    }
    let mut names = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        names.push((r.u16()?, read_text(&mut r)?));
    }
    Ok((state, names))
}
