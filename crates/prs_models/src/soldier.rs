//! A soldier as low-poly rigid segments, one per bone, sized from the body config. Each bone moves
//! as a single transform, so a soldier costs nineteen draws at worst and shares its meshes with every
//! other soldier, and nothing needs skinning support from WebGL2. Segments overlap at the joints
//! rather than bending, which is how the faceted style hides them anyway.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use prs_body::{BodyConfig, Bone};

/// Seen by lights and not by the camera: casts a shadow without being drawn. Your own head lives
/// here in first person, so your shadow still has one.
pub const SHADOW_ONLY_LAYER: usize = 1;

/// Sits on the entity a bone drives. Its transform is the bone's, in world space.
#[derive(Component, Clone, Copy, Debug)]
pub struct SoldierBone {
    pub soldier: usize,
    pub bone: Bone,
}

/// A magazine in a soldier's off hand, shown for the middle of a reload.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeldMagazine {
    pub soldier: usize,
}

/// What a soldier has in their hands while treating someone. The roll is in the working hand and a
/// medkit's pouch in the other.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeldCare {
    pub soldier: usize,
    pub item: CareItem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CareItem {
    Roll,
    Pouch,
}

/// A dressing on a limb, grown along the limb as it goes on.
#[derive(Component, Clone, Copy, Debug)]
pub struct LimbWrap {
    pub soldier: usize,
    pub limb: Bone,
    /// Metres up the bone from its start.
    pub at: f32,
}

/// One drawn piece of a bone that should vanish for the person inside it.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeadPart {
    pub soldier: usize,
}

/// A cross-section of a segment along its bone: y up the bone, half width, half depth, and how far
/// forward (towards -Z) it is pushed.
#[derive(Clone, Copy)]
struct Ring {
    y: f32,
    width: f32,
    depth: f32,
    forward: f32,
}

const fn ring(y: f32, width: f32, depth: f32, forward: f32) -> Ring {
    Ring { y, width, depth, forward }
}

/// A band round the upper arm, a little proud of the sleeve under it all the way round. Six sides, as
/// the sleeve has, so the flats line up.
fn armband_rings(length: f32, size: Vec2) -> [Ring; 4] {
    let (from, to) = (length * 0.16, length * 0.36);
    // The sleeve widens from 0.85 at its root to full at a third of the way down.
    let proud = 1.07;
    [
        ring(from, size.x * 0.97 * proud, size.y * 0.97 * proud, 0.0),
        ring(from + 0.006, size.x * proud, size.y * proud, 0.0),
        ring(to - 0.006, size.x * proud, size.y * proud, 0.0),
        ring(to, size.x * 0.97 * proud, size.y * 0.97 * proud, 0.0),
    ]
}

fn armband_mesh(length: f32, size: Vec2, add: &mut dyn FnMut(&[Ring], usize) -> Handle<Mesh>) -> Handle<Mesh> {
    add(&armband_rings(length, size), 6)
}

/// The same round the helmet, between its rim and its crown, with the helmet's eight sides.
fn helmet_band_rings(head: f32, size: Vec2) -> [Ring; 4] {
    let (from, to) = (head * 0.5, head * 0.66);
    [
        ring(from, size.x * 1.15, size.y * 1.17, 0.0),
        ring(from + 0.004, size.x * 1.165, size.y * 1.185, 0.0),
        ring(to - 0.004, size.x * 1.16, size.y * 1.18, 0.0),
        ring(to, size.x * 1.145, size.y * 1.165, 0.0),
    ]
}

fn helmet_band(head: f32, size: Vec2, add: &mut dyn FnMut(&[Ring], usize) -> Handle<Mesh>) -> Handle<Mesh> {
    add(&helmet_band_rings(head, size), 8)
}

/// Lofts rings into a closed faceted solid with its own flat normals, by the same recipe as the
/// terrain. `sides` is how many facets round: 4 reads as a box, 6 or 8 as a limb.
fn loft(rings: &[Ring], sides: usize) -> Mesh {
    let corner = |r: &Ring, i: usize| {
        // Rotated half a step so the flats, not the points, face front and back.
        let a = (i as f32 + 0.5) / sides as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        // Scaled so width and depth are out to the flats on a four-sided section too.
        let k = 1.0 / (std::f32::consts::PI / sides as f32).cos();
        Vec3::new(c * r.width * k, r.y, s * r.depth * k - r.forward)
    };
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut face = |points: &[Vec3]| {
        let mut normal = Vec3::ZERO;
        for i in 1..points.len() - 1 {
            normal += (points[i] - points[0]).cross(points[i + 1] - points[0]);
        }
        let normal = normal.normalize_or(Vec3::Y);
        let base = positions.len() as u32;
        for p in points {
            positions.push(p.to_array());
            normals.push(normal.to_array());
        }
        for i in 1..points.len() as u32 - 1 {
            indices.extend([base, base + i, base + i + 1]);
        }
    };
    for pair in rings.windows(2) {
        for i in 0..sides {
            let j = (i + 1) % sides;
            face(&[corner(&pair[0], i), corner(&pair[1], i), corner(&pair[1], j), corner(&pair[0], j)]);
        }
    }
    let first: Vec<Vec3> = (0..sides).map(|i| corner(&rings[0], i)).collect();
    face(&first);
    let last: Vec<Vec3> = (0..sides).rev().map(|i| corner(&rings[rings.len() - 1], i)).collect();
    face(&last);

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

type Part = (Handle<Mesh>, Handle<StandardMaterial>);

/// Everything one soldier is made of. Build once and share it, so soldiers batch together.
#[derive(Clone)]
pub struct SoldierModel {
    parts: Vec<(Bone, Vec<Part>)>,
    magazine: Part,
    roll: Part,
    medkit: Part,
    forearm_wrap: Part,
    thigh_wrap: Part,
    arm_at: f32,
    thigh_at: f32,
}

/// What a side wears, and the colour its things are shown in. This is how you can tell a body on the
/// other side from a friend's before you are close enough for anything else. A mod changes them under
/// `team_looks`, by side, as in `team_looks.1.armband = [1.0, 0.5, 0.0]`.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TeamLook {
    #[serde(with = "srgb")]
    pub uniform: Color,
    #[serde(with = "srgb")]
    pub trousers: Color,
    #[serde(with = "srgb")]
    pub vest: Color,
    #[serde(with = "srgb")]
    pub pouch: Color,
    #[serde(with = "srgb")]
    pub helmet: Color,
    /// For flags and for anything on the screen.
    #[serde(with = "srgb")]
    pub banner: Color,
    /// Bands round both upper arms and the helmet, so a side can be told up close from the body alone.
    /// Matte cloth like the rest, only brighter and more saturated than anything else a soldier wears.
    #[serde(with = "srgb")]
    pub armband: Color,
}

/// Colours in a mod are three numbers, red, green and blue, as you would give them to `Color::srgb`.
mod srgb {
    use bevy::prelude::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(color: &Color, s: S) -> Result<S::Ok, S::Error> {
        let c = color.to_srgba();
        [c.red, c.green, c.blue].serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let [r, g, b] = <[f32; 3]>::deserialize(d)?;
        Ok(Color::srgb(r, g, b))
    }
}

#[derive(Resource, Clone, Debug)]
pub struct TeamLooks(pub Vec<TeamLook>);

/// Keyed by side, so a mod can change one thing about one side without giving every colour of both.
impl serde::Serialize for TeamLooks {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().enumerate().map(|(side, look)| (side.to_string(), look)))
    }
}

impl<'de> serde::Deserialize<'de> for TeamLooks {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let by_side = std::collections::BTreeMap::<String, TeamLook>::deserialize(d)?;
        let mut sides: Vec<(usize, TeamLook)> = Vec::with_capacity(by_side.len());
        for (side, look) in by_side {
            let side = side.parse().map_err(|_| serde::de::Error::custom(format!("a side is a number, not `{side}`")))?;
            sides.push((side, look));
        }
        sides.sort_by_key(|(side, _)| *side);
        if sides.is_empty() || sides.iter().enumerate().any(|(n, (side, _))| n != *side) {
            return Err(serde::de::Error::custom("sides go 0, 1 and on, with none missing"));
        }
        Ok(Self(sides.into_iter().map(|(_, look)| look).collect()))
    }
}

impl TeamLooks {
    /// The defaults with the active mod over them.
    pub fn tuned() -> Self {
        prs_mods::tuned("team_looks", Self::default())
    }

    /// A side past the end of the list looks like the last one.
    pub fn of(&self, team: u8) -> &TeamLook {
        &self.0[(team as usize).min(self.0.len() - 1)]
    }
}

impl Default for TeamLooks {
    /// Woodland against desert: far enough apart in lightness and hue that the difference survives
    /// fog and low sun, but drab enough that nobody is easy to spot for wearing it.
    fn default() -> Self {
        Self(vec![
            TeamLook {
                uniform: Color::srgb(0.3, 0.34, 0.22),
                trousers: Color::srgb(0.27, 0.3, 0.2),
                vest: Color::srgb(0.2, 0.22, 0.15),
                pouch: Color::srgb(0.17, 0.19, 0.13),
                helmet: Color::srgb(0.24, 0.27, 0.18),
                banner: Color::srgb(0.32, 0.56, 0.95),
                armband: Color::srgb(0.2, 0.42, 0.9),
            },
            TeamLook {
                uniform: Color::srgb(0.6, 0.5, 0.36),
                trousers: Color::srgb(0.55, 0.46, 0.33),
                vest: Color::srgb(0.42, 0.34, 0.24),
                pouch: Color::srgb(0.36, 0.29, 0.2),
                helmet: Color::srgb(0.5, 0.42, 0.3),
                banner: Color::srgb(0.92, 0.33, 0.28),
                armband: Color::srgb(0.92, 0.14, 0.1),
            },
        ])
    }
}

impl SoldierModel {
    pub fn new(cfg: &BodyConfig, look: &TeamLook, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let mut cloth = |color: Color| {
            materials.add(StandardMaterial { base_color: color, perceptual_roughness: 1.0, ..default() })
        };
        let uniform = cloth(look.uniform);
        let trousers = cloth(look.trousers);
        let vest = cloth(look.vest);
        let pouch = cloth(look.pouch);
        let skin = cloth(Color::srgb(0.72, 0.56, 0.42));
        let helmet = cloth(look.helmet);
        let glove = cloth(Color::srgb(0.13, 0.14, 0.11));
        let boot = cloth(Color::srgb(0.12, 0.1, 0.08));
        let armband = cloth(look.armband);

        let b = &cfg.bones;
        let mut add = |rings: &[Ring], sides: usize| meshes.add(loft(rings, sides));

        let p = b.pelvis_size;
        let pelvis = vec![(
            add(&[ring(-0.09, p.x * 0.8, p.y * 0.8, 0.0), ring(-0.02, p.x, p.y, 0.0), ring(b.pelvis + 0.02, p.x * 0.92, p.y * 0.92, 0.0)], 8),
            trousers.clone(),
        )];
        let belt = (add(&[ring(b.pelvis - 0.035, p.x * 1.03, p.y * 1.05, 0.0), ring(b.pelvis + 0.015, p.x * 1.03, p.y * 1.05, 0.0)], 8), pouch.clone());

        let s = b.spine_size;
        let spine = vec![(
            add(&[ring(-0.03, s.x * 1.0, s.y * 1.0, 0.0), ring(b.spine * 0.5, s.x * 0.96, s.y * 0.96, 0.0), ring(b.spine + 0.03, s.x * 1.05, s.y * 1.05, 0.0)], 8),
            uniform.clone(),
        )];

        let c = b.chest_size;
        let l = b.chest;
        let chest = vec![
            (
                add(&[ring(-0.02, c.x * 0.88, c.y * 0.86, 0.0), ring(l * 0.65, c.x, c.y * 0.92, 0.0), ring(l - 0.015, c.x * 0.84, c.y * 0.74, 0.01), ring(l + 0.012, c.x * 0.62, c.y * 0.6, 0.01)], 8),
                uniform.clone(),
            ),
            // Plate carrier, and the magazine pouches on its front.
            (
                add(&[ring(0.01, c.x * 0.95, c.y * 1.02, 0.0), ring(l * 0.72, c.x * 1.03, c.y * 1.06, 0.0), ring(l - 0.05, c.x * 0.62, c.y * 0.86, 0.008)], 8),
                vest.clone(),
            ),
            (
                add(&[ring(0.0, c.x * 0.66, 0.03, c.y + 0.015), ring(0.13, c.x * 0.66, 0.03, c.y + 0.02)], 4),
                pouch.clone(),
            ),
        ];

        let n = b.neck_size;
        let neck = vec![
            (add(&[ring(b.neck * 0.25, n.x * 1.05, n.y * 1.02, 0.0), ring(b.neck + 0.03, n.x, n.y, 0.0)], 6), skin.clone()),
            // Collar.
            (add(&[ring(-0.02, n.x * 1.5, n.y * 1.5, 0.0), ring(b.neck * 0.42, n.x * 1.18, n.y * 1.16, 0.0)], 6), uniform.clone()),
        ];

        let h = b.head_size;
        let hl = b.head;
        let head = vec![
            (
                add(&[ring(-0.015, h.x * 0.68, h.y * 0.7, 0.015), ring(hl * 0.42, h.x * 0.94, h.y * 0.94, 0.012), ring(hl * 0.85, h.x * 0.88, h.y * 0.9, 0.0)], 6),
                skin.clone(),
            ),
            // Helmet.
            (
                add(&[ring(hl * 0.46, h.x * 1.14, h.y * 1.16, 0.0), ring(hl * 0.74, h.x * 1.13, h.y * 1.14, 0.0), ring(hl * 1.02, h.x * 0.74, h.y * 0.78, -0.002), ring(hl * 1.12, h.x * 0.3, h.y * 0.34, -0.002)], 8),
                helmet.clone(),
            ),
            // The side's band round the helmet, just proud of it.
            (helmet_band(hl, h, &mut add), armband.clone()),
        ];

        let sh = b.shoulder_size;
        let shoulder = vec![(
            add(&[ring(-0.015, sh.x * 0.72, sh.y * 0.8, 0.0), ring(b.shoulder * 0.7, sh.x, sh.y, 0.0), ring(b.shoulder + 0.03, sh.x, sh.y, 0.0)], 6),
            uniform.clone(),
        )];

        let ua = b.upper_arm_size;
        let upper_arm = vec![
            (
                add(&[ring(-0.035, ua.x * 0.85, ua.y * 0.85, 0.0), ring(b.upper_arm * 0.35, ua.x, ua.y, 0.0), ring(b.upper_arm + 0.015, ua.x * 0.76, ua.y * 0.76, 0.0)], 6),
                uniform.clone(),
            ),
            // The side's band, high on the arm where the sleeve is widest. It rides on the bone, so it goes
            // wherever the arm does.
            (armband_mesh(b.upper_arm, ua, &mut add), armband.clone()),
        ];
        let fa = b.forearm_size;
        let forearm = vec![(
            add(&[ring(-0.03, fa.x * 0.98, fa.y * 0.98, 0.0), ring(b.forearm * 0.3, fa.x * 1.06, fa.y * 1.06, 0.0), ring(b.forearm + 0.012, fa.x * 0.66, fa.y * 0.72, 0.0)], 6),
            uniform.clone(),
        )];
        let hs = b.hand_size;
        let hand = vec![(
            add(&[ring(-0.012, hs.x * 0.76, hs.y * 0.68, 0.0), ring(b.hand * 0.55, hs.x, hs.y, 0.0), ring(b.hand + 0.022, hs.x * 0.9, hs.y * 0.8, 0.0)], 4),
            glove.clone(),
        )];

        let t = b.thigh_size;
        let thigh = vec![(
            add(&[ring(-0.05, t.x * 0.9, t.y * 0.92, 0.0), ring(b.thigh * 0.28, t.x, t.y, 0.0), ring(b.thigh + 0.015, t.x * 0.72, t.y * 0.78, 0.0)], 6),
            trousers.clone(),
        )];
        let sn = b.shin_size;
        let shin = vec![
            (
                add(&[ring(-0.03, sn.x * 0.98, sn.y * 1.02, 0.008), ring(b.shin * 0.3, sn.x, sn.y, 0.0), ring(b.shin * 0.72, sn.x * 0.72, sn.y * 0.74, 0.0)], 6),
                trousers.clone(),
            ),
            // Boot tops.
            (
                add(&[ring(b.shin * 0.68, sn.x * 0.84, sn.y * 0.86, 0.0), ring(b.shin + 0.025, sn.x * 0.8, sn.y * 0.82, 0.0)], 6),
                boot.clone(),
            ),
        ];

        // The foot's bone runs heel to toe, with the sole on its +Z side.
        let f = b.foot_size;
        let sole = b.ankle_height;
        let top = -0.035;
        let middle = (sole + top) * 0.5;
        let half = (sole - top) * 0.5;
        let foot = vec![(
            add(&[ring(-0.055, f.x * 0.8, half * 0.92, -middle), ring(b.foot * 0.45, f.x, half * 0.72, -(middle + half * 0.28)), ring(b.foot, f.x * 0.86, half * 0.36, -(sole - half * 0.36))], 6),
            boot.clone(),
        )];

        let mut parts = vec![
            (Bone::Pelvis, [pelvis.clone(), vec![belt]].concat()),
            (Bone::Spine, spine),
            (Bone::Chest, chest),
            (Bone::Neck, neck),
            (Bone::Head, head),
        ];
        for (bones, set) in [
            ([Bone::ShoulderL, Bone::ShoulderR], &shoulder),
            ([Bone::UpperArmL, Bone::UpperArmR], &upper_arm),
            ([Bone::ForearmL, Bone::ForearmR], &forearm),
            ([Bone::HandL, Bone::HandR], &hand),
            ([Bone::ThighL, Bone::ThighR], &thigh),
            ([Bone::ShinL, Bone::ShinR], &shin),
            ([Bone::FootL, Bone::FootR], &foot),
        ] {
            for bone in bones {
                parts.push((bone, set.clone()));
            }
        }
        let magazine = (add(&[ring(0.0, 0.013, 0.027, 0.0), ring(0.13, 0.013, 0.027, 0.0)], 4), pouch.clone());
        // Gauze, because a dressing that matched the uniform would not read as one. The roll stands on its
        // axis across the hand, and its end tails off as it unwinds.
        let gauze = cloth(Color::srgb(0.82, 0.8, 0.72));
        let roll = (add(&[ring(-0.042, 0.03, 0.03, 0.0), ring(-0.036, 0.034, 0.034, 0.0), ring(0.036, 0.034, 0.034, 0.0), ring(0.042, 0.03, 0.03, 0.0)], 8), gauze.clone());
        let medkit = (add(&[ring(-0.05, 0.03, 0.06, 0.0), ring(0.05, 0.03, 0.06, 0.0)], 4), cloth(Color::srgb(0.48, 0.14, 0.11)));
        let wrap = |size: Vec2, taper: f32, add: &mut dyn FnMut(&[Ring], usize) -> Handle<Mesh>| {
            let (w, d) = (size.x * taper, size.y * taper);
            add(&[ring(-0.05, w * 0.98, d * 0.98, 0.0), ring(-0.04, w * 1.04, d * 1.04, 0.0), ring(0.04, w * 1.04, d * 1.04, 0.0), ring(0.05, w * 0.98, d * 0.98, 0.0)], 6)
        };
        let forearm_wrap = (wrap(b.forearm_size, 1.0, &mut add), gauze.clone());
        let thigh_wrap = (wrap(b.thigh_size, 1.02, &mut add), gauze);
        Self { parts, magazine, roll, medkit, forearm_wrap, thigh_wrap, arm_at: b.forearm * cfg.care.arm_at, thigh_at: b.thigh * cfg.care.thigh_at }
    }
}

/// One entity per bone at the top of the world, each moved directly: bones are worked out in world
/// space, so nesting them would only make every one of them undo its parent.
pub fn spawn_soldier(commands: &mut Commands, model: &SoldierModel, soldier: usize) {
    for (bone, parts) in &model.parts {
        commands
            .spawn((SoldierBone { soldier, bone: *bone }, Transform::default(), Visibility::default()))
            .with_children(|root| {
                for (mesh, material) in parts {
                    let mut part = root.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), RenderLayers::layer(0)));
                    if matches!(bone, Bone::Head | Bone::Neck) {
                        part.insert(HeadPart { soldier });
                    }
                }
                if *bone == Bone::HandL {
                    let (mesh, material) = &model.magazine;
                    root.spawn((
                        HeldMagazine { soldier },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, 0.055, 0.0),
                        Visibility::Hidden,
                    ));
                    let (mesh, material) = &model.medkit;
                    root.spawn((
                        HeldCare { soldier, item: CareItem::Pouch },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, 0.09, 0.0),
                        Visibility::Hidden,
                    ));
                }
                if *bone == Bone::HandR {
                    let (mesh, material) = &model.roll;
                    root.spawn((
                        HeldCare { soldier, item: CareItem::Roll },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, 0.09, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                        Visibility::Hidden,
                    ));
                }
                let wrap = match bone {
                    Bone::ForearmL => Some((&model.forearm_wrap, model.arm_at)),
                    Bone::ThighL => Some((&model.thigh_wrap, model.thigh_at)),
                    _ => None,
                };
                if let Some(((mesh, material), at)) = wrap {
                    root.spawn((
                        LimbWrap { soldier, limb: *bone, at },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, at, 0.0),
                        Visibility::Hidden,
                    ));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The team bands are small and sit over other surfaces, so one turned inside out would be easy to
    /// miss and would show the sleeve through it.
    #[test]
    fn team_bands_face_outwards_and_sit_proud_of_what_they_wrap() {
        let cfg = BodyConfig::default();
        let b = &cfg.bones;
        let bands = [loft(&armband_rings(b.upper_arm, b.upper_arm_size), 6), loft(&helmet_band_rings(b.head, b.head_size), 8)];
        for band in &bands {
            let report = crate::mesh_check::look_all_round(band, 24, 7);
            assert_eq!(report.normals_disagree, 0, "{report:?}");
            assert!(report.share_inside_out() < 0.02, "{report:?}");
            assert!(report.triangles <= 64, "a band should cost next to nothing: {report:?}");
        }
        // Wider than the sleeve at its widest, and than the helmet where it goes round it.
        for r in armband_rings(b.upper_arm, b.upper_arm_size) {
            assert!(r.width > b.upper_arm_size.x && r.depth > b.upper_arm_size.y);
        }
        for r in helmet_band_rings(b.head, b.head_size) {
            assert!(r.width > b.head_size.x * 1.14 && r.depth > b.head_size.y * 1.16);
        }
    }

    #[test]
    fn a_mod_can_change_one_sides_armband_and_leaves_everything_else_as_it_was() {
        let active = prs_mods::Mod::parse("test", "team_looks.1.armband = [1.0, 0.5, 0.0]").unwrap();
        let (plain, modded) = (TeamLooks::default(), active.over("team_looks", TeamLooks::default()).unwrap());
        assert_eq!(modded.of(1).armband, Color::srgb(1.0, 0.5, 0.0));
        assert_eq!(modded.0.len(), plain.0.len());
        assert_eq!(modded.of(1).uniform, plain.of(1).uniform);
        assert_eq!(modded.of(0).armband, plain.of(0).armband);
    }

    #[test]
    fn every_side_has_a_band_brighter_than_anything_else_it_wears() {
        let saturation = |c: Color| {
            let c = c.to_srgba();
            let (hi, lo) = (c.red.max(c.green).max(c.blue), c.red.min(c.green).min(c.blue));
            if hi > 0.0 { (hi - lo) / hi } else { 0.0 }
        };
        for look in &TeamLooks::default().0 {
            for worn in [look.uniform, look.trousers, look.vest, look.pouch, look.helmet] {
                assert!(saturation(look.armband) > saturation(worn) + 0.3, "{look:?}");
            }
        }
    }
}
