//! A Huey as low-poly meshes in the airframe's own frame: -Z forward, Y up, the centre of mass at the
//! origin. Colour is in the vertices, so the airframe is one draw whatever it is made of. The doors are
//! real openings and the windscreen a separate see-through mesh, because people sit inside this.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

const OLIVE: Color3 = [0.20, 0.24, 0.13];
const OLIVE_DARK: Color3 = [0.14, 0.17, 0.09];
const FLOOR: Color3 = [0.11, 0.11, 0.10];
const INTERIOR: Color3 = [0.24, 0.25, 0.22];
const SEAT: Color3 = [0.28, 0.24, 0.16];
const METAL: Color3 = [0.18, 0.18, 0.19];
const PANEL: Color3 = [0.05, 0.05, 0.06];
const DIAL: Color3 = [0.015, 0.015, 0.018];
const TICK: Color3 = [0.75, 0.75, 0.70];
const NEEDLE: Color3 = [0.95, 0.92, 0.75];
const BLADE: Color3 = [0.06, 0.06, 0.06];
const BLADE_TIP: Color3 = [0.8, 0.7, 0.15];

type Color3 = [f32; 3];

/// Where each dial sits on the panel, in the airframe's frame, in the order `prs_helicopter` reads them.
pub const DIALS: [Vec3; 4] = [
    Vec3::new(0.32, 0.07, -3.13),
    Vec3::new(0.54, 0.07, -3.13),
    Vec3::new(0.76, 0.07, -3.13),
    Vec3::new(0.98, 0.07, -3.13),
];
const DIAL_RADIUS: f32 = 0.085;

/// Triangles with flat normals and colour per corner.
#[derive(Default)]
struct Build {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Build {
    /// Corners in order round the face. Their winding says which way it faces.
    fn face(&mut self, corners: &[Vec3], colour: Color3) {
        let mut normal = Vec3::ZERO;
        for i in 1..corners.len() - 1 {
            normal += (corners[i] - corners[0]).cross(corners[i + 1] - corners[0]);
        }
        let normal = normal.normalize_or(Vec3::Y);
        let base = self.positions.len() as u32;
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colours.push([colour[0], colour[1], colour[2], 1.0]);
        }
        for i in 1..corners.len() as u32 - 1 {
            self.indices.extend([base, base + i, base + i + 1]);
        }
    }

    fn cuboid(&mut self, center: Vec3, half: Vec3, colour: Color3) {
        self.tilted_box(center, half, Quat::IDENTITY, colour);
    }

    fn tilted_box(&mut self, center: Vec3, half: Vec3, rotation: Quat, colour: Color3) {
        let at = |x: f32, y: f32, z: f32| center + rotation * (Vec3::new(x, y, z) * half);
        self.face(&[at(-1., -1., 1.), at(1., -1., 1.), at(1., 1., 1.), at(-1., 1., 1.)], colour);
        self.face(&[at(1., -1., -1.), at(-1., -1., -1.), at(-1., 1., -1.), at(1., 1., -1.)], colour);
        self.face(&[at(1., -1., 1.), at(1., -1., -1.), at(1., 1., -1.), at(1., 1., 1.)], colour);
        self.face(&[at(-1., -1., -1.), at(-1., -1., 1.), at(-1., 1., 1.), at(-1., 1., -1.)], colour);
        self.face(&[at(-1., 1., 1.), at(1., 1., 1.), at(1., 1., -1.), at(-1., 1., -1.)], colour);
        self.face(&[at(-1., -1., -1.), at(1., -1., -1.), at(1., -1., 1.), at(-1., -1., 1.)], colour);
    }

    /// A bar between two points, square in section.
    fn bar(&mut self, from: Vec3, to: Vec3, radius: f32, colour: Color3) {
        let along = to - from;
        let rotation = Quat::from_rotation_arc(Vec3::Y, along.normalize_or(Vec3::Y));
        self.tilted_box((from + to) * 0.5, Vec3::new(radius, along.length() * 0.5, radius), rotation, colour);
    }

    /// A skin between sections going aft, seen from outside.
    fn loft(&mut self, sections: &[Section], colour: Color3) {
        self.skin(sections, colour, 0, false);
    }

    /// The other side of a skin, for something you can see into. Faces are one-sided, so the inside of a
    /// shell has to be there in its own right.
    fn lining(&mut self, sections: &[Section], colour: Color3) {
        self.skin(sections, colour, 0, true);
    }

    /// `leave_out` is how many faces of each section to skip, from the top round.
    fn skin(&mut self, sections: &[Section], colour: Color3, leave_out: usize, inward: bool) {
        for pair in sections.windows(2) {
            let (a, b) = (pair[0].ring(), pair[1].ring());
            for i in leave_out..a.len() {
                let j = (i + 1) % a.len();
                if inward {
                    self.face(&[a[i], a[j], b[j], b[i]], colour);
                } else {
                    self.face(&[b[i], b[j], a[j], a[i]], colour);
                }
            }
        }
    }

    /// Flat ends for a loft.
    fn cap(&mut self, section: &Section, colour: Color3, facing_forward: bool) {
        let mut ring = section.ring().to_vec();
        if !facing_forward {
            ring.reverse();
        }
        self.face(&ring, colour);
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colours)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// A slice of fuselage: from `low` to `high` and `half` wide, with its corners taken off.
#[derive(Clone, Copy)]
struct Section {
    z: f32,
    low: f32,
    high: f32,
    half: f32,
}

const fn section(z: f32, low: f32, high: f32, half: f32) -> Section {
    Section { z, low, high, half }
}

impl Section {
    /// Clockwise looking towards -Z from behind, which is anticlockwise from the front.
    fn ring(&self) -> [Vec3; 8] {
        let (w, narrow) = (self.half, self.half * 0.72);
        let height = self.high - self.low;
        let (a, b) = (self.low + height * 0.2, self.low + height * 0.78);
        let z = self.z;
        [
            Vec3::new(-narrow, self.high, z),
            Vec3::new(narrow, self.high, z),
            Vec3::new(w, b, z),
            Vec3::new(w, a, z),
            Vec3::new(narrow, self.low, z),
            Vec3::new(-narrow, self.low, z),
            Vec3::new(-w, a, z),
            Vec3::new(-w, b, z),
        ]
    }
}

fn airframe() -> Mesh {
    let mut m = Build::default();

    // The nose below the windscreen, closed over in front of the panel and open at the top round the
    // seats, then what is behind the cabin, then the boom.
    let nose = [section(-4.05, -0.62, -0.32, 0.45), section(-3.7, -0.9, 0.02, 0.95), section(-3.15, -0.95, 0.14, 1.2)];
    m.loft(&nose, OLIVE);
    m.lining(&nose, INTERIOR);
    m.cap(&nose[0], OLIVE_DARK, true);
    let tub = [section(-3.15, -0.95, -0.18, 1.2), section(-2.5, -0.95, -0.18, 1.28), section(-1.5, -0.95, -0.18, 1.3)];
    m.skin(&tub, OLIVE, 1, false);
    m.skin(&tub, INTERIOR, 1, true);
    let rear = [
        section(1.2, -0.95, 1.1, 1.3),
        section(2.4, -0.7, 1.08, 1.2),
        section(3.6, 0.1, 1.04, 0.5),
        section(6.0, 0.42, 1.0, 0.3),
        section(8.5, 0.62, 0.98, 0.16),
    ];
    m.loft(&rear, OLIVE);
    m.cap(&rear[0], INTERIOR, true);
    m.cap(&rear[4], OLIVE_DARK, false);

    // Floor, roof and the sides under the doors. The cabin itself is open on both sides.
    m.cuboid(Vec3::new(0.0, -0.96, -0.15), Vec3::new(1.3, 0.05, 1.35), FLOOR);
    m.cuboid(Vec3::new(0.0, -1.03, -0.15), Vec3::new(1.15, 0.05, 1.35), OLIVE_DARK);
    m.cuboid(Vec3::new(0.0, 1.05, -0.35), Vec3::new(1.2, 0.05, 1.6), OLIVE);
    m.cuboid(Vec3::new(0.0, 0.99, -0.35), Vec3::new(1.15, 0.015, 1.55), INTERIOR);
    m.cuboid(Vec3::new(0.0, -0.9, -2.4), Vec3::new(1.2, 0.02, 0.85), FLOOR);
    // Door posts holding the roof up.
    for side in [-1.0f32, 1.0] {
        m.cuboid(Vec3::new(side * 1.27, 0.42, -1.5), Vec3::new(0.05, 0.62, 0.07), OLIVE);
        m.cuboid(Vec3::new(side * 1.27, 0.08, 1.15), Vec3::new(0.05, 1.05, 0.07), OLIVE);
        m.cuboid(Vec3::new(side * 1.25, 0.99, -0.15), Vec3::new(0.06, 0.07, 1.38), OLIVE_DARK);
    }

    // The frame round the windscreen.
    let roof_front = |x: f32| Vec3::new(x, 1.0, -1.95);
    let sill = |x: f32| Vec3::new(x, 0.1, -3.22);
    for x in [-1.12, 0.0, 1.12] {
        m.bar(sill(x), roof_front(x), 0.018, OLIVE_DARK);
    }
    for side in [-1.0f32, 1.0] {
        m.bar(roof_front(side * 1.12), Vec3::new(side * 1.25, 1.0, -1.5), 0.02, OLIVE_DARK);
        m.bar(sill(side * 1.12), Vec3::new(side * 1.28, -0.18, -2.5), 0.02, OLIVE_DARK);
        m.bar(Vec3::new(side * 1.28, -0.18, -1.5), Vec3::new(side * 1.28, -0.18, -2.5), 0.02, OLIVE_DARK);
    }
    m.face(&[roof_front(-1.12), roof_front(1.12), Vec3::new(1.2, 1.0, -1.5), Vec3::new(-1.2, 1.0, -1.5)], INTERIOR);

    // Inside: seats, the panel and the bulkhead behind them.
    for x in [-0.65f32, 0.65] {
        m.cuboid(Vec3::new(x, -0.58, -2.32), Vec3::new(0.25, 0.05, 0.25), SEAT);
        m.cuboid(Vec3::new(x, -0.2, -2.05), Vec3::new(0.25, 0.42, 0.05), SEAT);
        m.cuboid(Vec3::new(x, -0.76, -2.32), Vec3::new(0.2, 0.14, 0.2), METAL);
    }
    m.cuboid(Vec3::new(0.0, -0.45, -1.7), Vec3::new(1.15, 0.45, 0.04), INTERIOR);
    // The bench along the back wall of the cabin.
    m.cuboid(Vec3::new(0.0, -0.55, 0.95), Vec3::new(1.1, 0.05, 0.22), SEAT);
    m.cuboid(Vec3::new(0.0, -0.1, 1.1), Vec3::new(1.1, 0.42, 0.04), SEAT);
    panel(&mut m);
    // Cyclic sticks and collectives are on the floor in front of each seat.
    for x in [-0.65f32, 0.65] {
        m.bar(Vec3::new(x, -0.88, -2.72), Vec3::new(x, -0.35, -2.68), 0.015, METAL);
        m.bar(Vec3::new(x - 0.3, -0.72, -2.35), Vec3::new(x - 0.33, -0.48, -2.68), 0.018, METAL);
    }

    // The engine, the mast and the tail.
    let cowl = [section(-0.4, 1.08, 1.2, 0.45), section(0.2, 1.08, 1.68, 0.62), section(2.4, 1.05, 1.62, 0.58), section(3.4, 1.0, 1.15, 0.3)];
    m.loft(&cowl, OLIVE);
    m.cap(&cowl[0], OLIVE_DARK, true);
    m.cap(&cowl[3], METAL, false);
    m.cuboid(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.11, 0.36, 0.11), METAL);
    let fin = [Vec3::new(0.0, 0.7, 7.9), Vec3::new(0.0, 0.62, 9.0), Vec3::new(0.0, 2.25, 9.65), Vec3::new(0.0, 2.25, 9.15)];
    m.face(&fin, OLIVE_DARK);
    m.face(&[fin[3], fin[2], fin[1], fin[0]], OLIVE_DARK);
    m.tilted_box(Vec3::new(0.0, 0.72, 6.1), Vec3::new(1.45, 0.025, 0.28), Quat::IDENTITY, OLIVE_DARK);

    // The skids.
    for side in [-1.0f32, 1.0] {
        let x = side * 1.25;
        m.bar(Vec3::new(x, -1.68, -2.45), Vec3::new(x, -1.68, 2.05), 0.05, METAL);
        m.bar(Vec3::new(x, -1.68, 2.05), Vec3::new(x, -1.52, 2.45), 0.045, METAL);
        m.bar(Vec3::new(x, -1.68, -2.45), Vec3::new(x, -1.48, -2.85), 0.045, METAL);
        m.bar(Vec3::new(x, -1.68, -1.3), Vec3::new(side * 0.75, -1.05, -1.3), 0.05, METAL);
        m.bar(Vec3::new(x, -1.68, 0.9), Vec3::new(side * 0.75, -1.05, 0.9), 0.05, METAL);
    }
    m.mesh()
}

/// The panel and its dial faces, tipped back to face the pilots.
fn panel(m: &mut Build) {
    m.cuboid(Vec3::new(0.0, -0.03, -3.17), Vec3::new(1.15, 0.2, 0.03), PANEL);
    m.cuboid(Vec3::new(0.0, 0.18, -3.15), Vec3::new(1.17, 0.012, 0.05), PANEL);
    m.cuboid(Vec3::new(0.0, -0.5, -3.0), Vec3::new(0.18, 0.28, 0.2), PANEL);
    for &at in &DIALS {
        let center = at + Vec3::Z * 0.011;
        let ring: Vec<Vec3> = (0..12)
            .map(|i| {
                let a = i as f32 / 12.0 * std::f32::consts::TAU;
                center + Vec3::new(a.cos(), a.sin(), 0.0) * DIAL_RADIUS
            })
            .collect();
        m.face(&ring, DIAL);
        for tick in 0..10 {
            let a = tick as f32 / 10.0 * std::f32::consts::TAU;
            let out = Vec3::new(a.sin(), a.cos(), 0.0);
            let from = center + out * DIAL_RADIUS * 0.8 + Vec3::Z * 0.002;
            let wide = Vec3::new(out.y, -out.x, 0.0) * 0.004;
            let to = from + out * DIAL_RADIUS * 0.17;
            m.face(&[from - wide, from + wide, to + wide, to - wide], TICK);
        }
    }
}

fn needle() -> Mesh {
    let mut m = Build::default();
    let (w, long) = (0.0055, DIAL_RADIUS * 0.82);
    m.face(&[Vec3::new(-w, -0.012, 0.0), Vec3::new(w, -0.012, 0.0), Vec3::new(w * 0.5, long, 0.0), Vec3::new(-w * 0.5, long, 0.0)], NEEDLE);
    m.mesh()
}

fn glass() -> Mesh {
    let mut m = Build::default();
    let colour = [0.55, 0.65, 0.68];
    let roof = |x: f32| Vec3::new(x, 1.0, -1.95);
    let sill = |x: f32| Vec3::new(x, 0.1, -3.22);
    m.face(&[sill(-1.12), sill(0.0), roof(0.0), roof(-1.12)], colour);
    m.face(&[sill(0.0), sill(1.12), roof(1.12), roof(0.0)], colour);
    for side in [-1.0f32, 1.0] {
        let x = side * 1.28;
        let window = [Vec3::new(x, -0.18, -1.5), Vec3::new(x, -0.18, -2.5), sill(side * 1.12), roof(side * 1.12), Vec3::new(side * 1.25, 1.0, -1.5)];
        if side > 0.0 {
            m.face(&window, colour);
        } else {
            let mut window = window;
            window.reverse();
            m.face(&window, colour);
        }
    }
    m.mesh()
}

/// Two blades along X, spinning about Y at the hub.
fn main_blades(radius: f32, chord: f32) -> Mesh {
    let mut m = Build::default();
    let half = chord * 0.5;
    m.cuboid(Vec3::ZERO, Vec3::new(radius, 0.025, half), BLADE);
    for side in [-1.0f32, 1.0] {
        m.cuboid(Vec3::new(side * (radius - 0.25), 0.0, 0.0), Vec3::new(0.25, 0.028, half + 0.002), BLADE_TIP);
    }
    m.cuboid(Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.3, 0.09, 0.12), METAL);
    m.bar(Vec3::new(0.0, 0.1, -0.9), Vec3::new(0.0, 0.1, 0.9), 0.02, METAL);
    m.mesh()
}

/// How much of a blurred rotor there is at this share of its radius: a blade covers the inside of the disc
/// for longer, but it is the tips you see, and the very edge fades away to nothing.
fn blur_alpha(share: f32) -> f32 {
    let edge = if share >= 0.999 { 0.0 } else { 1.0 };
    (0.35 + 0.65 * share * share) * edge
}

/// The disc of a fast rotor, as faint rings that say where the blades are without being any of them.
fn rotor_disc(radius: f32, rings: usize) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colours = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let sides = 32;
    for ring in 0..rings {
        let (inner, outer) = (ring as f32 / rings as f32 * radius, (ring + 1) as f32 / rings as f32 * radius);
        // The tips are what you see, and the inside of a disc mostly shows what is behind it.
        let share = (ring + 1) as f32 / rings as f32;
        let inner_share = ring as f32 / rings as f32;
        let (inner_alpha, alpha) = (blur_alpha(inner_share), blur_alpha(share));
        let base = positions.len() as u32;
        for i in 0..=sides {
            let a = i as f32 / sides as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            positions.push([c * inner, 0.0, s * inner]);
            positions.push([c * outer, 0.0, s * outer]);
            normals.push([0.0, 1.0, 0.0]);
            normals.push([0.0, 1.0, 0.0]);
            colours.push([0.05, 0.05, 0.05, inner_alpha]);
            colours.push([0.05, 0.05, 0.05, alpha]);
        }
        for i in 0..sides as u32 {
            let (a, b) = (base + i * 2, base + i * 2 + 1);
            indices.extend([a, a + 2, b, b, a + 2, b + 2]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}

/// Two blades along Y, spinning about X.
fn tail_blades(radius: f32) -> Mesh {
    let mut m = Build::default();
    m.cuboid(Vec3::ZERO, Vec3::new(0.015, radius, 0.09), BLADE);
    for side in [-1.0f32, 1.0] {
        m.cuboid(Vec3::new(0.0, side * (radius - 0.1), 0.0), Vec3::new(0.018, 0.1, 0.092), BLADE_TIP);
    }
    m.cuboid(Vec3::ZERO, Vec3::new(0.08, 0.08, 0.08), METAL);
    m.mesh()
}

/// The meshes every helicopter shares. Sized from the rotor it is told about.
#[derive(Resource, Clone)]
pub struct HeliModel {
    pub airframe: Handle<Mesh>,
    pub glass: Handle<Mesh>,
    pub needle: Handle<Mesh>,
    pub blades: Handle<Mesh>,
    pub disc: Handle<Mesh>,
    pub tail_blades: Handle<Mesh>,
    pub tail_disc: Handle<Mesh>,
    pub paint: Handle<StandardMaterial>,
    pub glass_paint: Handle<StandardMaterial>,
    pub blur_paint: Handle<StandardMaterial>,
}

/// One rotor's blades, and how many radians of `rotor_angle` it does per radian of the main rotor.
pub fn build_heli_model(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    radius: f32,
    chord: f32,
    tail_radius: f32,
) -> HeliModel {
    let paint = materials.add(StandardMaterial { perceptual_roughness: 0.75, ..default() });
    let glass_paint = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.16),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.15,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let blur_paint = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.9,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    HeliModel {
        airframe: meshes.add(airframe()),
        glass: meshes.add(glass()),
        needle: meshes.add(needle()),
        blades: meshes.add(main_blades(radius, chord)),
        disc: meshes.add(rotor_disc(radius, 5)),
        tail_blades: meshes.add(tail_blades(tail_radius)),
        tail_disc: meshes.add(rotor_disc(tail_radius, 2)),
        paint,
        glass_paint,
        blur_paint,
    }
}

/// Sits on the entity whose transform is the airframe. Everything else hangs off this.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliBody(pub usize);

/// Turns about Y with the rotor, and is tilted with the disc.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliRotor {
    pub heli: usize,
}

/// Shown while the rotor is slow enough to see.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliBlades {
    pub heli: usize,
}

/// Shown once it is not, and more solid the faster it goes.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliBlur {
    pub heli: usize,
}

/// Turns with the tail rotor, and goes when it does.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliTail {
    pub heli: usize,
}

/// A pointer on a dial, by which dial it is. Turned about Z.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeliNeedle {
    pub heli: usize,
    pub dial: usize,
}

/// Where the main rotor hub is and where the tail rotor is, in the airframe's frame.
#[derive(Clone, Copy, Debug)]
pub struct HeliLayout {
    pub hub: Vec3,
    pub tail: Vec3,
}

/// Puts one helicopter in the world and returns its root.
pub fn spawn_heli(commands: &mut Commands, model: &HeliModel, heli: usize, layout: HeliLayout) -> Entity {
    let m = model;
    commands
        .spawn((HeliBody(heli), Transform::default(), Visibility::default(), Mesh3d(m.airframe.clone()), MeshMaterial3d(m.paint.clone())))
        .with_children(|parent| {
            parent.spawn((NotShadowCaster, Transform::default(), Visibility::default(), Mesh3d(m.glass.clone()), MeshMaterial3d(m.glass_paint.clone())));
            parent
                .spawn((HeliRotor { heli }, Transform::from_translation(layout.hub), Visibility::default()))
                .with_children(|rotor| {
                    rotor.spawn((HeliBlades { heli }, Transform::default(), Visibility::default(), Mesh3d(m.blades.clone()), MeshMaterial3d(m.paint.clone())));
                    rotor.spawn((HeliBlur { heli }, NotShadowCaster, Transform::default(), Visibility::Hidden, Mesh3d(m.disc.clone()), MeshMaterial3d(m.blur_paint.clone())));
                });
            parent
                .spawn((HeliTail { heli }, Transform::from_translation(layout.tail), Visibility::default()))
                .with_children(|tail| {
                    tail.spawn((Transform::default(), Visibility::default(), Mesh3d(m.tail_blades.clone()), MeshMaterial3d(m.paint.clone())));
                });
            for (dial, &at) in DIALS.iter().enumerate() {
                parent.spawn((
                    HeliNeedle { heli, dial },
                    Transform::from_translation(at + Vec3::Z * 0.016),
                    Visibility::default(),
                    Mesh3d(m.needle.clone()),
                    MeshMaterial3d(m.paint.clone()),
                ));
            }
        })
        .id()
}
