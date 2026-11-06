//! Whether a mesh faces the way it should. The renderer draws only the faces whose corners wind round
//! anticlockwise, and it lights them by the normals stored with them, so a mesh wound or stored the wrong
//! way looks inside out, or lit from behind, in a way that is easy to miss on anything small.

use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;

/// Two surfaces closer than this, metres along a line, count as the same surface.
const SAME_PLACE: f32 = 1e-3;

/// What looking at a mesh from all round it found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceReport {
    pub triangles: usize,
    /// Triangles whose stored normals point away from the way they are wound.
    pub normals_disagree: usize,
    /// Lines from outside that reached the mesh at all, and how many of those met a back first. That is
    /// a face that culling would throw away, with whatever is behind it showing through.
    pub rays: usize,
    pub rays_met_a_back: usize,
}

impl FaceReport {
    pub fn share_inside_out(&self) -> f32 {
        self.rays_met_a_back as f32 / self.rays.max(1) as f32
    }
}

fn triangles_of(mesh: &Mesh) -> (Vec<[Vec3; 3]>, Vec<[Vec3; 3]>) {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return (Vec::new(), Vec::new());
    };
    let empty = Vec::new();
    let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(normals)) => normals,
        _ => &empty,
    };
    let indices: Vec<usize> = match mesh.indices() {
        Some(Indices::U32(list)) => list.iter().map(|i| *i as usize).collect(),
        Some(Indices::U16(list)) => list.iter().map(|i| *i as usize).collect(),
        None => (0..positions.len()).collect(),
    };
    let at = |list: &[[f32; 3]], t: &[usize]| [Vec3::from(list[t[0]]), Vec3::from(list[t[1]]), Vec3::from(list[t[2]])];
    let corners = indices.chunks_exact(3).map(|t| at(positions, t)).collect();
    let stored = if normals.is_empty() { Vec::new() } else { indices.chunks_exact(3).map(|t| at(normals, t)).collect() };
    (corners, stored)
}

/// Where a line first meets a triangle, either side, and whether from behind.
fn first_hit(triangles: &[[Vec3; 3]], from: Vec3, dir: Vec3) -> Option<bool> {
    let mut nearest: Option<(f32, bool)> = None;
    for [a, b, c] in triangles {
        let (ab, ac) = (*b - *a, *c - *a);
        let p = dir.cross(ac);
        let det = ab.dot(p);
        if det.abs() < 1e-9 {
            continue;
        }
        let to = from - *a;
        let u = to.dot(p) / det;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = to.cross(ab);
        let v = dir.dot(q) / det;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = ac.dot(q) / det;
        // Anticlockwise as seen by the line comes out as a positive determinant.
        // A lining and the skin it lines are in the same place, so culling picks whichever faces you.
        let facing = det > 0.0;
        if t > 0.0 && nearest.is_none_or(|n| t < n.0 - SAME_PLACE || (facing && t < n.0 + SAME_PLACE)) {
            nearest = Some((t, facing));
        }
    }
    nearest.map(|n| n.1)
}

/// Looks at a mesh along `views` directions from all round it, along a square of `across` lines each.
pub fn look_all_round(mesh: &Mesh, views: usize, across: usize) -> FaceReport {
    let (triangles, stored) = triangles_of(mesh);
    let mut report = FaceReport { triangles: triangles.len(), ..default() };
    for (corners, normals) in triangles.iter().zip(&stored) {
        let wound = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if normals.iter().any(|n| n.dot(wound) < 0.0) {
            report.normals_disagree += 1;
        }
    }
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in triangles.iter().flatten() {
        low = low.min(*p);
        high = high.max(*p);
    }
    let (middle, reach) = ((low + high) * 0.5, (high - low).length().max(1e-3));
    for view in 0..views {
        // Spread evenly over a sphere, on a spiral.
        let y = 1.0 - 2.0 * (view as f32 + 0.5) / views as f32;
        let around = view as f32 * 2.399_963;
        let dir = Vec3::new((1.0 - y * y).sqrt() * around.cos(), y, (1.0 - y * y).sqrt() * around.sin());
        let (side, up) = dir.any_orthonormal_pair();
        for i in 0..across {
            for j in 0..across {
                let share = |n: usize| (n as f32 + 0.5) / across as f32 - 0.5;
                let start = middle - dir * reach * 2.0 + (side * share(i) + up * share(j)) * reach;
                if let Some(from_front) = first_hit(&triangles, start, dir) {
                    report.rays += 1;
                    if !from_front {
                        report.rays_met_a_back += 1;
                    }
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TeamLooks, VehicleModel, VehiclePaint, SoldierModel, GunModels, build_heli_model};
    use prs_body::BodyConfig;
    use prs_runtime::prs_sim::helicopter::HeliConfig;
    use prs_runtime::prs_sim::vehicles::VehicleTable;
    use prs_runtime::prs_sim::weapons::WeaponTable;

    /// Everything that culling applies to has to face outwards, near enough. A little can meet a back,
    /// where a line slips in at a seam between two parts that only overlap.
    const MOST_INSIDE_OUT: f32 = 0.02;

    fn check(name: &str, mesh: &Mesh, failures: &mut Vec<String>) {
        let report = look_all_round(mesh, 60, 9);
        if report.normals_disagree > 0 || report.share_inside_out() > MOST_INSIDE_OUT {
            failures.push(format!("{name}: {report:?}"));
        }
    }

    #[test]
    fn every_model_faces_outwards() {
        let mut failures = Vec::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();

        let cfg = HeliConfig::default();
        let heli = build_heli_model(&mut meshes, &mut materials, cfg.rotor.radius, cfg.rotor.chord, cfg.tail.radius);
        for (name, handle) in [("helicopter airframe", &heli.airframe), ("helicopter blades", &heli.blades), ("tail blades", &heli.tail_blades)] {
            check(name, meshes.get(handle).unwrap(), &mut failures);
        }

        let mut others = Assets::<Mesh>::default();
        let paint = VehiclePaint::default();
        for vehicle in &VehicleTable::default().vehicles {
            VehicleModel::new(vehicle, &paint, &mut others, &mut materials);
        }
        SoldierModel::new(&BodyConfig::default(), TeamLooks::default().of(0), &mut others, &mut materials);
        GunModels::new(&WeaponTable::default(), &mut others, &mut materials);
        let all: Vec<_> = others.iter().map(|(_, mesh)| mesh.clone()).collect();
        assert!(all.len() > 20, "only {} meshes", all.len());
        for (at, mesh) in all.iter().enumerate() {
            check(&format!("mesh {at}"), mesh, &mut failures);
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

