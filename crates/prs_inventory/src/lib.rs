//! What a soldier carries, as plain data. The gun in their hands lives in the weapon state, because
//! that is what fires it. This holds everything else, and the rules for moving it around.

use prs_weapons::{Gun, WeaponId, WeaponTable};

/// Something a number key selects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slot {
    #[default]
    Primary,
    Sidearm,
    Bandage,
    Medkit,
    Frag,
    Smoke,
}

impl Slot {
    pub const ALL: [Slot; 6] = [Slot::Primary, Slot::Sidearm, Slot::Bandage, Slot::Medkit, Slot::Frag, Slot::Smoke];

    pub fn is_gun(self) -> bool {
        matches!(self, Slot::Primary | Slot::Sidearm)
    }

    fn gun_index(self) -> usize {
        match self {
            Slot::Sidearm => 1,
            _ => 0,
        }
    }
}

/// Which guns and how much of everything a soldier is issued.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Loadout {
    pub primary: WeaponId,
    pub sidearm: Option<WeaponId>,
    pub bandages: u32,
    pub medkits: u32,
    pub frags: u32,
    pub smokes: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventoryConfig {
    pub loadout: Loadout,
    /// Most a soldier can carry of each.
    pub max_bandages: u32,
    pub max_medkits: u32,
    pub max_frags: u32,
    pub max_smokes: u32,
    pub bandage_weight: f32,
    pub medkit_weight: f32,
    pub frag_weight: f32,
    pub smoke_weight: f32,
    /// Kilograms a soldier carries with no effect. A standard rifleman's kit sits just under this.
    pub free_load: f32,
    /// Share of speed lost per kilogram over `free_load`, and the most it can ever take.
    pub slow_per_kg: f32,
    pub max_slow: f32,
    /// Share of extra stamina drain per kilogram over `free_load`.
    pub drain_per_kg: f32,
    /// How close a kit has to be to reach it, in metres.
    pub pickup_reach: f32,
    /// Seconds a kit stays on the ground.
    pub kit_lifetime: f32,
    /// How far to one side of a body its kit lands, in metres.
    pub kit_beside_body: f32,
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self {
            loadout: Loadout {
                primary: WeaponTable::ASSAULT_RIFLE,
                sidearm: Some(WeaponTable::PISTOL),
                bandages: 2,
                medkits: 1,
                frags: 2,
                smokes: 2,
            },
            max_bandages: 4,
            max_medkits: 2,
            max_frags: 4,
            max_smokes: 4,
            bandage_weight: 0.05,
            medkit_weight: 0.6,
            frag_weight: 0.4,
            smoke_weight: 0.4,
            free_load: 10.1,
            slow_per_kg: 0.012,
            max_slow: 0.2,
            drain_per_kg: 0.05,
            pickup_reach: 2.2,
            kit_lifetime: 180.0,
            kit_beside_body: 0.5,
        }
    }
}

/// What goes in pouches or on the ground. A soldier's and a dropped kit are the same thing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stash {
    /// Primary and sidearm.
    pub guns: [Option<Gun>; 2],
    pub bandages: u32,
    pub medkits: u32,
    pub frags: u32,
    pub smokes: u32,
}

impl Stash {
    pub fn is_empty(&self) -> bool {
        self.guns.iter().all(Option::is_none) && self.bandages == 0 && self.medkits == 0 && self.frags == 0 && self.smokes == 0
    }

    /// Kilograms of it.
    pub fn weight(&self, cfg: &InventoryConfig, table: &WeaponTable) -> f32 {
        let guns: f32 = self.guns.iter().flatten().map(|gun| gun_weight(gun.weapon, gun.rounds(), table)).sum();
        guns + self.bandages as f32 * cfg.bandage_weight
            + self.medkits as f32 * cfg.medkit_weight
            + self.frags as f32 * cfg.frag_weight
            + self.smokes as f32 * cfg.smoke_weight
    }
}

/// Kilograms of a gun and however many rounds are carried for it.
pub fn gun_weight(weapon: WeaponId, rounds: u32, table: &WeaponTable) -> f32 {
    let weapon = &table[weapon];
    weapon.weight + rounds as f32 * weapon.round_weight
}

/// A soldier's share of speed lost to weight, and extra stamina drain as a share.
pub fn encumbrance(load: f32, cfg: &InventoryConfig) -> (f32, f32) {
    let over = (load - cfg.free_load).max(0.0);
    ((over * cfg.slow_per_kg).min(cfg.max_slow), over * cfg.drain_per_kg)
}

/// Everything a soldier has but the gun in their hands. The slot of the gun being held stays empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inventory {
    pub stash: Stash,
    /// Which gun slot the gun in the hands belongs in.
    pub held: Slot,
    /// What the soldier has asked to have ready.
    pub selected: Slot,
}

impl Inventory {
    /// Issues a loadout. Returns the gun to start out holding as well.
    pub fn issue(loadout: &Loadout, table: &WeaponTable) -> (Self, Gun) {
        let stash = Stash {
            guns: [None, loadout.sidearm.map(|id| Gun::issued(id, &table[id]))],
            bandages: loadout.bandages,
            medkits: loadout.medkits,
            frags: loadout.frags,
            smokes: loadout.smokes,
        };
        let inventory = Self { stash, held: Slot::Primary, selected: Slot::Primary };
        (inventory, Gun::issued(loadout.primary, &table[loadout.primary]))
    }

    pub fn count(&self, slot: Slot) -> u32 {
        match slot {
            Slot::Primary | Slot::Sidearm if slot == self.held => 1,
            Slot::Primary | Slot::Sidearm => self.stash.guns[slot.gun_index()].is_some() as u32,
            Slot::Bandage => self.stash.bandages,
            Slot::Medkit => self.stash.medkits,
            Slot::Frag => self.stash.frags,
            Slot::Smoke => self.stash.smokes,
        }
    }

    /// The one key for anything thrown goes round what there is, starting from a frag.
    pub fn next_throwable(&self) -> Option<Slot> {
        let (frags, smokes) = (self.stash.frags > 0, self.stash.smokes > 0);
        match self.selected {
            Slot::Frag if smokes => Some(Slot::Smoke),
            Slot::Frag => None,
            Slot::Smoke if frags => Some(Slot::Frag),
            Slot::Smoke => None,
            _ if frags => Some(Slot::Frag),
            _ if smokes => Some(Slot::Smoke),
            _ => None,
        }
    }

    /// Asks for a slot. Returns whether anything changed: an empty slot does nothing.
    pub fn select(&mut self, slot: Slot) -> bool {
        if slot == self.selected || self.count(slot) == 0 {
            return false;
        }
        self.selected = slot;
        true
    }

    /// The gun slot being asked for, if the soldier wants a different gun from the one in hand.
    pub fn gun_wanted(&self) -> Option<Slot> {
        (self.selected.is_gun() && self.selected != self.held).then_some(self.selected)
    }

    /// Takes the gun in `slot` from the pouches and puts `current` in the slot of the gun it replaces.
    pub fn exchange_held(&mut self, slot: Slot, current: Gun) -> Option<Gun> {
        let next = self.stash.guns[slot.gun_index()].take()?;
        self.stash.guns[self.held.gun_index()] = Some(current);
        self.held = slot;
        Some(next)
    }

    /// Takes everything out, for dropping, with the gun from the hands back in its slot.
    pub fn empty_out(&mut self, in_hand: Gun) -> Stash {
        let mut stash = std::mem::take(&mut self.stash);
        stash.guns[self.held.gun_index()] = Some(in_hand);
        stash
    }

    /// Goes through a kit on the ground, with `in_hand` being the gun in the hands. The primary is
    /// swapped for the kit's, or if it is the same kind of gun then its ammunition is taken. A sidearm
    /// goes the same way, and only if there is one to replace or none yet. Bandages and medkits go in
    /// the pouches as far as they fit. Returns the gun that should now be in the hands.
    pub fn take_kit(&mut self, kit: &mut Stash, in_hand: Gun, cfg: &InventoryConfig) -> Gun {
        self.stash.guns[self.held.gun_index()] = Some(in_hand);
        for (mine, theirs) in self.stash.guns.iter_mut().zip(kit.guns.iter_mut()) {
            match (mine.as_mut(), theirs.take()) {
                (Some(mine), Some(theirs)) if mine.weapon == theirs.weapon => {
                    mine.magazines.extend(theirs.magazines);
                    if theirs.ammo > 0 {
                        mine.magazines.push(theirs.ammo);
                    }
                }
                (_, Some(next)) => *theirs = mine.replace(next),
                (_, None) => {}
            }
        }
        let bandages = kit.bandages.min(cfg.max_bandages.saturating_sub(self.stash.bandages));
        self.stash.bandages += bandages;
        kit.bandages -= bandages;
        let medkits = kit.medkits.min(cfg.max_medkits.saturating_sub(self.stash.medkits));
        self.stash.medkits += medkits;
        kit.medkits -= medkits;
        let frags = kit.frags.min(cfg.max_frags.saturating_sub(self.stash.frags));
        self.stash.frags += frags;
        kit.frags -= frags;
        let smokes = kit.smokes.min(cfg.max_smokes.saturating_sub(self.stash.smokes));
        self.stash.smokes += smokes;
        kit.smokes -= smokes;
        self.stash.guns[self.held.gun_index()].take().expect("the held slot was just filled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prs_weapons::FireMode;

    fn table() -> WeaponTable {
        WeaponTable::default()
    }

    fn issued() -> (Inventory, Gun) {
        Inventory::issue(&InventoryConfig::default().loadout, &table())
    }

    #[test]
    fn a_soldier_is_issued_a_loaded_gun_with_every_magazine_tracked() {
        let (inventory, gun) = issued();
        let cfg = &table()[gun.weapon];
        assert_eq!(gun.ammo, cfg.magazine);
        assert_eq!(gun.magazines.len(), cfg.spare_magazines as usize);
        assert!(gun.magazines.iter().all(|&m| m == cfg.magazine));
        assert!(inventory.stash.guns[1].is_some(), "and a sidearm in its holster");
        assert!(inventory.stash.guns[0].is_none(), "the gun in the hands is not also in the pouches");
    }

    #[test]
    fn selecting_an_empty_slot_does_nothing() {
        let (mut inventory, _) = issued();
        inventory.stash.medkits = 0;
        assert!(!inventory.select(Slot::Medkit));
        assert_eq!(inventory.selected, Slot::Primary);
        assert!(inventory.select(Slot::Bandage));
        assert_eq!(inventory.gun_wanted(), None, "a bandage does not change which gun is in the hands");
        assert!(inventory.select(Slot::Sidearm));
        assert_eq!(inventory.gun_wanted(), Some(Slot::Sidearm));
    }

    #[test]
    fn changing_guns_keeps_each_gun_as_it_was() {
        let (mut inventory, mut gun) = issued();
        gun.ammo = 7;
        gun.mode = FireMode::Auto;
        let pistol = inventory.exchange_held(Slot::Sidearm, gun.clone()).unwrap();
        assert_eq!(pistol.weapon, WeaponTable::PISTOL);
        assert_eq!(inventory.held, Slot::Sidearm);
        let back = inventory.exchange_held(Slot::Primary, pistol.clone()).unwrap();
        assert_eq!(back, gun);
        assert_eq!(inventory.stash.guns[1].as_ref(), Some(&pistol));
    }

    #[test]
    fn a_kit_is_a_swap_and_nothing_is_lost() {
        let t = table();
        let cfg = InventoryConfig::default();
        let (mut inventory, gun) = issued();
        let lmg = Gun::issued(WeaponTable::MACHINE_GUN, &t[WeaponTable::MACHINE_GUN]);
        let mut kit = Stash { guns: [Some(lmg.clone()), None], bandages: 9, medkits: 0, frags: 0, smokes: 0 };
        let in_hands = inventory.take_kit(&mut kit, gun.clone(), &cfg);
        assert_eq!(in_hands, lmg);
        assert_eq!(kit.guns[0].as_ref(), Some(&gun), "your old rifle stays behind for someone else");
        assert!(inventory.stash.guns[1].is_some(), "your pistol is still yours");
        assert_eq!(inventory.stash.bandages, cfg.max_bandages);
        assert_eq!(kit.bandages, 9 - (cfg.max_bandages - 2));
    }

    #[test]
    fn a_kit_with_the_same_gun_is_resupply() {
        let t = table();
        let cfg = InventoryConfig::default();
        let (mut inventory, mut gun) = issued();
        gun.magazines = vec![30];
        let mut theirs = Gun::issued(WeaponTable::ASSAULT_RIFLE, &t[WeaponTable::ASSAULT_RIFLE]);
        theirs.ammo = 12;
        theirs.magazines = vec![30, 4];
        let mut kit = Stash { guns: [Some(theirs), None], ..Default::default() };
        let in_hands = inventory.take_kit(&mut kit, gun, &cfg);
        assert_eq!(in_hands.magazines, vec![30, 30, 4, 12]);
        assert!(kit.is_empty());
    }

    #[test]
    fn a_kit_taken_while_holding_the_sidearm_leaves_the_sidearm_in_the_hands() {
        let t = table();
        let cfg = InventoryConfig::default();
        let (mut inventory, rifle) = issued();
        let pistol = inventory.exchange_held(Slot::Sidearm, rifle.clone()).unwrap();
        let lmg = Gun::issued(WeaponTable::MACHINE_GUN, &t[WeaponTable::MACHINE_GUN]);
        let mut kit = Stash { guns: [Some(lmg.clone()), None], ..Default::default() };
        let in_hands = inventory.take_kit(&mut kit, pistol.clone(), &cfg);
        assert_eq!(in_hands, pistol);
        assert_eq!(inventory.stash.guns[0], Some(lmg));
        assert_eq!(kit.guns[0], Some(rifle));
    }

    #[test]
    fn dropping_everything_includes_the_gun_in_the_hands() {
        let (mut inventory, rifle) = issued();
        let stash = inventory.empty_out(rifle.clone());
        assert_eq!(stash.guns[0], Some(rifle));
        assert!(stash.guns[1].is_some());
        assert_eq!(stash.bandages, 2);
        assert!(inventory.stash.is_empty());
    }

    #[test]
    fn a_standard_rifleman_carries_no_penalty_and_a_machine_gunner_a_light_one() {
        let t = table();
        let cfg = InventoryConfig::default();
        let (inventory, rifle) = issued();
        let rifleman = inventory.stash.weight(&cfg, &t) + gun_weight(rifle.weapon, rifle.rounds(), &t);
        assert_eq!(encumbrance(rifleman, &cfg), (0.0, 0.0), "default kit weighs {rifleman}");
        let lmg = Gun::issued(WeaponTable::MACHINE_GUN, &t[WeaponTable::MACHINE_GUN]);
        let gunner = inventory.stash.weight(&cfg, &t) + gun_weight(lmg.weapon, lmg.rounds(), &t);
        let (slow, drain) = encumbrance(gunner, &cfg);
        assert!(slow > 0.05 && slow < 0.2, "{gunner} kg slows by {slow}");
        assert!(drain > 0.1, "{drain}");
    }
}
