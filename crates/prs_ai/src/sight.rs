//! What a soldier can make out of another: which parts of them show, through what, and how far. Walls and
//! smoke stop a look outright, and leaves let some of it through, by how much of a canopy it crosses. Seeing
//! anyone takes a while, faster the more of them shows, and a sliver of someone at range hardly ever.

use glam::{Vec2, Vec3};
use prs_sim::movement::{Stance, yaw_basis};
use prs_sim::{Level, Sim, Soldier, SoldierId};

/// How sight works for everyone, whatever their skill. Lengths are metres and times seconds.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SightConfig {
    /// Points looked at on one soldier in one look. All of them get looked at over a few looks.
    pub rays_per_look: u32,
    /// How much a metre of leaves dims a look, as in `e^(-density * metres)`, and how much has to get through
    /// for a point to count as seen.
    pub leaf_density: f32,
    pub see_through: f32,
    /// How far a fir's lowest branches are lifted, as the scene draws them.
    pub fir_clearance: f32,
    /// Side of a square of the map that trees are filed under, for finding the ones a look passes.
    pub foliage_cell: f32,
    /// Someone indoors, looked at from outside, is this much harder to make out, unless they are within
    /// `window_light` of a window, where the light from it falls on them.
    pub room_shade: f32,
    pub window_light: f32,
    /// How long after a shot the shooter stays obvious.
    pub fired_memory: f32,
    /// How many people at once a soldier keeps track of how aware of they are.
    pub most_watched: usize,
    /// Longest gap between two looks at someone that awareness builds or fades over, so that a soldier
    /// with many to look at does not take forever about it.
    pub longest_gap: f32,
    /// Where on a standing body the points are, as shares of the way from feet to eyes, and how far the
    /// shoulders are out to the side.
    pub chest: f32,
    pub hips: f32,
    pub knees: f32,
    pub shoulder_width: f32,
    /// For someone lying down: how far back from the eyes the chest, hips and knees are.
    pub prone_chest: f32,
    pub prone_hips: f32,
    pub prone_knees: f32,
}

impl Default for SightConfig {
    fn default() -> Self {
        Self {
            rays_per_look: 3,
            leaf_density: 0.55,
            see_through: 0.5,
            fir_clearance: 2.4,
            foliage_cell: 16.0,
            room_shade: 0.2,
            window_light: 1.0,
            fired_memory: 1.5,
            most_watched: 6,
            longest_gap: 1.0,
            chest: 0.75,
            hips: 0.5,
            knees: 0.22,
            shoulder_width: 0.22,
            prone_chest: 0.35,
            prone_hips: 0.8,
            prone_knees: 1.3,
        }
    }
}

/// The points on a soldier a look can find, in the order to aim at them, the gun last: it gives someone away
/// but is no place to shoot.
pub const BODY_POINTS: usize = 7;
pub const GUN_POINT: usize = BODY_POINTS - 1;

pub fn body_points(sim: &Sim, cfg: &SightConfig, them: &Soldier) -> [Vec3; BODY_POINTS] {
    let feet = them.body.pos;
    let eye = them.eye();
    let (forward, right) = yaw_basis(them.yaw);
    let gun = them.gun_pose(&sim.weapons).muzzle;
    let shoulder = right * cfg.shoulder_width;
    if them.body.stance == Stance::Prone {
        let low = |back: f32| Vec3::new(eye.x, (eye.y + feet.y) * 0.5, eye.z) - forward * back;
        let chest = low(cfg.prone_chest);
        return [chest, low(cfg.prone_hips), eye, chest + shoulder, chest - shoulder, low(cfg.prone_knees), gun];
    }
    let up = eye - feet;
    let chest = feet + up * cfg.chest;
    [chest, feet + up * cfg.hips, eye, chest + shoulder, chest - shoulder, feet + up * cfg.knees, gun]
}

/// The trees, filed by square, to find the ones a look goes past without trying all of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Foliage {
    cell: f32,
    half: f32,
    across: usize,
    /// Middle and half extents of each canopy.
    canopies: Vec<(Vec3, Vec3)>,
    cells: Vec<Vec<u32>>,
}

impl Foliage {
    pub fn new(level: &Level, cfg: &SightConfig) -> Self {
        let cell = cfg.foliage_cell.max(1.0);
        let half = level.terrain.size * 0.5;
        let across = (level.terrain.size / cell).ceil().max(1.0) as usize;
        let canopies: Vec<_> = level.trees.iter().map(|t| t.canopy(cfg.fir_clearance)).collect();
        let mut cells = vec![Vec::new(); across * across];
        for (index, (center, size)) in canopies.iter().enumerate() {
            let lo = Self::cell_of(cell, half, across, Vec2::new(center.x - size.x, center.z - size.z));
            let hi = Self::cell_of(cell, half, across, Vec2::new(center.x + size.x, center.z + size.z));
            for j in lo.1..=hi.1 {
                for i in lo.0..=hi.0 {
                    cells[j * across + i].push(index as u32);
                }
            }
        }
        Self { cell, half, across, canopies, cells }
    }

    fn cell_of(cell: f32, half: f32, across: usize, at: Vec2) -> (usize, usize) {
        let i = ((at.x + half) / cell).floor().clamp(0.0, (across - 1) as f32) as usize;
        let j = ((at.y + half) / cell).floor().clamp(0.0, (across - 1) as f32) as usize;
        (i, j)
    }

    /// Metres of leaves between two points.
    pub fn leaves_between(&self, from: Vec3, to: Vec3) -> f32 {
        let path = to - from;
        let flat = Vec2::new(path.x, path.z).length();
        let steps = (flat / (self.cell * 0.5)).ceil().max(1.0) as usize;
        let mut seen: Vec<u32> = Vec::new();
        for step in 0..=steps {
            let at = from + path * (step as f32 / steps as f32);
            let (i, j) = Self::cell_of(self.cell, self.half, self.across, Vec2::new(at.x, at.z));
            for &tree in &self.cells[j * self.across + i] {
                if !seen.contains(&tree) {
                    seen.push(tree);
                }
            }
        }
        seen.iter().map(|&tree| chord(from, to, self.canopies[tree as usize])).sum()
    }
}

/// How long a straight line from `a` to `b` is inside an upright ellipsoid.
fn chord(a: Vec3, b: Vec3, (center, half): (Vec3, Vec3)) -> f32 {
    let scale = half.max(Vec3::splat(1e-3)).recip();
    let (p, d) = ((a - center) * scale, (b - a) * scale);
    let (qa, qb, qc) = (d.dot(d), 2.0 * p.dot(d), p.dot(p) - 1.0);
    let root = qb * qb - 4.0 * qa * qc;
    if qa < 1e-9 || root <= 0.0 {
        return 0.0;
    }
    let root = root.sqrt();
    let enter = ((-qb - root) / (2.0 * qa)).clamp(0.0, 1.0);
    let leave = ((-qb + root) / (2.0 * qa)).clamp(0.0, 1.0);
    (leave - enter) * (b - a).length()
}

/// How aware one soldier is of another, and which of their points showed last time each was looked at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Watch {
    pub soldier: SoldierId,
    /// From 0 to 1. At 1 they have been seen for what they are.
    pub awareness: f32,
    /// Bit `n` set: point `n` showed when last looked at. `looked` says which have been looked at at all.
    pub shown: u8,
    pub looked: u8,
    /// Which point to look at next, and the tick of the last look.
    pub next: u8,
    pub last_tick: u32,
}

impl Watch {
    pub fn new(soldier: SoldierId, tick: u32) -> Self {
        Self { soldier, awareness: 0.0, shown: 0, looked: 0, next: 0, last_tick: tick }
    }

    /// Share of the body that showed, gun left out, going on what has been looked at.
    pub fn body_share(&self) -> f32 {
        let body = (1u8 << GUN_POINT) - 1;
        let looked = (self.looked & body).count_ones();
        if looked == 0 { 0.0 } else { (self.shown & body).count_ones() as f32 / looked as f32 }
    }

    pub fn any_shown(&self) -> bool {
        self.shown != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_through_the_middle_of_a_canopy_is_as_long_as_it_is_wide() {
        let canopy = (Vec3::new(0.0, 5.0, 0.0), Vec3::new(3.0, 2.0, 3.0));
        let through = chord(Vec3::new(-10.0, 5.0, 0.0), Vec3::new(10.0, 5.0, 0.0), canopy);
        assert!((through - 6.0).abs() < 1e-3, "{through}");
        assert_eq!(chord(Vec3::new(-10.0, 1.0, 0.0), Vec3::new(10.0, 1.0, 0.0), canopy), 0.0, "under it");
        assert_eq!(chord(Vec3::new(-10.0, 5.0, 0.0), Vec3::new(-5.0, 5.0, 0.0), canopy), 0.0, "short of it");
    }
}
