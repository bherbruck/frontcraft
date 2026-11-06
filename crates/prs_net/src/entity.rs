//! A player as everyone else sees it: whole numbers, sent as deltas, so an idle one costs little.

use crate::wire::{MAX_ENTITIES, Reader, WireError, Writer};

/// More than a game should need. Unused fields cost nothing on the wire.
pub const MAX_FIELDS: usize = 64;

const KIND_SHIFT: u32 = 16;

/// Which kind of thing and which one of them, as one number that sorts by kind.
pub fn entity_key(kind: u8, id: u32) -> u32 {
    (kind as u32) << KIND_SHIFT | id
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetEntity {
    pub key: u32,
    /// Quantised state. The game's [`crate::game::Schema`] says what each one means.
    pub fields: [i32; MAX_FIELDS],
}

impl NetEntity {
    pub fn new(kind: u8, id: u32) -> Self {
        Self { key: entity_key(kind, id), fields: [0; MAX_FIELDS] }
    }

    pub fn kind(&self) -> u8 {
        (self.key >> KIND_SHIFT) as u8
    }

    pub fn id(&self) -> u32 {
        self.key & ((1 << KIND_SHIFT) - 1)
    }
}

/// How a kind of entity's fields are grouped for deltas: (first field, how many). `None` if there is
/// no such kind, which a decoder takes as garbage.
pub type GroupsOf = fn(u8) -> Option<&'static [(usize, usize)]>;

/// One entity's delta. `base` is what it is against and how many ticks back that was, or `None` to
/// send it whole, for a client who has nothing to go on.
pub(crate) fn write_update(
    w: &mut Writer,
    groups: &[(usize, usize)],
    entity: &NetEntity,
    base: Option<(u32, &[i32; MAX_FIELDS])>,
) {
    let zero = [0; MAX_FIELDS];
    let was = base.map_or(&zero, |(_, fields)| fields);
    let mut mask = 0u64;
    for (bit, &(first, count)) in groups.iter().enumerate() {
        if entity.fields[first..first + count] != was[first..first + count] {
            mask |= 1 << bit;
        }
    }
    w.varu(entity.key as u64);
    w.varu(base.map_or(0, |(back, _)| back as u64));
    w.varu(mask);
    for (bit, &(first, count)) in groups.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            for i in first..first + count {
                w.vari(entity.fields[i] as i64 - was[i] as i64);
            }
        }
    }
}

/// Reads one entity. `base_of` finds what an entity was `back` ticks ago, for a client that has it.
pub(crate) fn read_update(
    r: &mut Reader,
    groups_of: GroupsOf,
    base_of: &dyn Fn(u32, u32) -> Option<[i32; MAX_FIELDS]>,
) -> Result<NetEntity, WireError> {
    let key = r.varu32()?;
    let back = r.varu32()?;
    let mask = r.varu()?;
    let groups = groups_of((key >> KIND_SHIFT) as u8).ok_or(WireError::BadTag)?;
    let mut fields = if back == 0 { [0; MAX_FIELDS] } else { base_of(key, back).ok_or(WireError::MissingBaseline)? };
    for (bit, &(first, count)) in groups.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            for field in &mut fields[first..first + count] {
                *field = (*field as i64).wrapping_add(r.vari()?) as i32;
            }
        }
    }
    Ok(NetEntity { key, fields })
}

pub(crate) fn read_count(r: &mut Reader) -> Result<usize, WireError> {
    r.count(MAX_ENTITIES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(kind: u8) -> Option<&'static [(usize, usize)]> {
        (kind == 0).then_some(&[(0, 3), (3, 1), (4, 2)][..])
    }

    fn entity(id: u16, values: [i32; 6]) -> NetEntity {
        let mut e = NetEntity::new(0, id as u32);
        e.fields[..6].copy_from_slice(&values);
        e
    }

    fn round_trip(now: &NetEntity, base: Option<&NetEntity>) -> (NetEntity, usize) {
        let mut w = Writer::default();
        let g = groups(0).unwrap();
        write_update(&mut w, g, now, base.map(|b| (2, &b.fields)));
        let fields = base.map(|b| b.fields);
        let got = read_update(&mut Reader::new(&w.buf), groups, &|_, _| fields).unwrap();
        (got, w.buf.len())
    }

    #[test]
    fn deltas_cost_little_and_rebuild_exactly() {
        let base = entity(1, [100_000, 2, -300_000, 4, 5, 6]);
        let now = entity(1, [100_001, 2, -300_000, 4, 5, 6]);
        let (whole, whole_len) = round_trip(&now, None);
        let (delta, delta_len) = round_trip(&now, Some(&base));
        assert_eq!(whole, now);
        assert_eq!(delta, now);
        assert!(delta_len * 2 < whole_len, "{delta_len} vs {whole_len}");
    }

    #[test]
    fn a_delta_against_something_the_client_lacks_says_so() {
        let mut w = Writer::default();
        write_update(&mut w, groups(0).unwrap(), &entity(1, [1; 6]), Some((4, &[0; MAX_FIELDS])));
        let got = read_update(&mut Reader::new(&w.buf), groups, &|_, _| None);
        assert_eq!(got, Err(WireError::MissingBaseline));
    }

    #[test]
    fn an_unknown_kind_is_garbage() {
        let mut w = Writer::default();
        w.varu(entity_key(9, 0) as u64);
        w.varu(0);
        w.varu(0);
        let got = read_update(&mut Reader::new(&w.buf), groups, &|_, _| None);
        assert_eq!(got, Err(WireError::BadTag));
    }

    #[test]
    fn extreme_values_survive_the_delta() {
        let (got, _) = round_trip(&entity(1, [i32::MAX; 6]), Some(&entity(1, [i32::MIN; 6])));
        assert_eq!(got, entity(1, [i32::MAX; 6]));
    }
}
