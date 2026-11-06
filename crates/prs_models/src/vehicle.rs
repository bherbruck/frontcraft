//! A vehicle as a low-poly model built from the same panels bullets hit, so what you can see is what
//! can stop a round, and from the seats and wheels the simulation uses. All one mesh with its colour
//! in the vertices, plus a mesh for each wheel, so a vehicle costs a handful of draws.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use prs_runtime::prs_sim::seats::DoorPanel;
use prs_runtime::prs_sim::vehicles::{PanelLook, VehicleConfig, WheelConfig};

/// How a kind of vehicle's parts are coloured and finished. Plain data, so a mod can repaint one.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct VehiclePaint {
    pub paint: Vec3,
    pub dark: Vec3,
    pub canvas: Vec3,
    pub trim: Vec3,
    pub seat: Vec3,
    pub tyre: Vec3,
    pub hub: Vec3,
    pub lamp: Vec3,
    /// What a wreck's colours are multiplied by.
    pub burnt: Vec3,
}

impl Default for VehiclePaint {
    fn default() -> Self {
        Self {
            paint: Vec3::new(0.33, 0.36, 0.24),
            dark: Vec3::new(0.13, 0.14, 0.11),
            canvas: Vec3::new(0.42, 0.4, 0.29),
            trim: Vec3::new(0.2, 0.21, 0.17),
            seat: Vec3::new(0.16, 0.14, 0.11),
            tyre: Vec3::new(0.05, 0.05, 0.05),
            hub: Vec3::new(0.3, 0.32, 0.24),
            lamp: Vec3::new(0.78, 0.76, 0.6),
            burnt: Vec3::new(0.14, 0.13, 0.12),
        }
    }
}

impl VehiclePaint {
    fn of(&self, look: PanelLook) -> Vec3 {
        match look {
            PanelLook::Paint => self.paint,
            PanelLook::Dark => self.dark,
            PanelLook::Canvas => self.canvas,
            PanelLook::Trim => self.trim,
        }
    }
}

#[derive(Default)]
struct Builder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Builder {
    fn face(&mut self, points: &[Vec3], color: Vec3) {
        let base = self.positions.len() as u32;
        let normal = (points[1] - points[0]).cross(points[2] - points[0]).normalize_or(Vec3::Y);
        let linear = LinearRgba::from(Color::srgb(color.x, color.y, color.z)).to_f32_array();
        for p in points {
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear);
        }
        for i in 1..points.len() as u32 - 1 {
            self.indices.extend([base, base + i, base + i + 1]);
        }
    }

    /// A turned box.
    fn add_box(&mut self, center: Vec3, half: Vec3, rotation: Quat, color: Vec3) {
        let at = |x: f32, y: f32, z: f32| center + rotation * (half * Vec3::new(x, y, z));
        for corners in [
            [at(1.0, -1.0, -1.0), at(1.0, 1.0, -1.0), at(1.0, 1.0, 1.0), at(1.0, -1.0, 1.0)],
            [at(-1.0, -1.0, 1.0), at(-1.0, 1.0, 1.0), at(-1.0, 1.0, -1.0), at(-1.0, -1.0, -1.0)],
            [at(-1.0, 1.0, -1.0), at(-1.0, 1.0, 1.0), at(1.0, 1.0, 1.0), at(1.0, 1.0, -1.0)],
            [at(-1.0, -1.0, 1.0), at(-1.0, -1.0, -1.0), at(1.0, -1.0, -1.0), at(1.0, -1.0, 1.0)],
            [at(1.0, -1.0, 1.0), at(1.0, 1.0, 1.0), at(-1.0, 1.0, 1.0), at(-1.0, -1.0, 1.0)],
            [at(-1.0, -1.0, -1.0), at(-1.0, 1.0, -1.0), at(1.0, 1.0, -1.0), at(1.0, -1.0, -1.0)],
        ] {
            self.face(&corners, color);
        }
    }

    /// A round thing with its axis along X.
    fn add_drum(&mut self, radius: f32, half_width: f32, x: f32, sides: usize, color: Vec3) {
        let rim = |i: usize, side: f32| {
            let a = i as f32 / sides as f32 * std::f32::consts::TAU;
            Vec3::new(x + side * half_width, a.cos() * radius, a.sin() * radius)
        };
        for i in 0..sides {
            let j = (i + 1) % sides;
            self.face(&[rim(j, -1.0), rim(j, 1.0), rim(i, 1.0), rim(i, -1.0)], color);
        }
        let outer: Vec<Vec3> = (0..sides).map(|i| rim(i, 1.0)).collect();
        self.face(&outer, color);
        let inner: Vec<Vec3> = (0..sides).rev().map(|i| rim(i, -1.0)).collect();
        self.face(&inner, color);
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// The body: everything bullets can hit, then seats, the wheel and the lamps, which they pass through.
fn body_mesh(cfg: &VehicleConfig, paint: &VehiclePaint) -> Mesh {
    let mut b = Builder::default();
    for (i, panel) in cfg.panels.iter().enumerate() {
        if cfg.door_of_panel(i).is_none() {
            b.add_box(panel.center, panel.half, panel.rotation, paint.of(panel.look));
        }
    }
    for seat in &cfg.seats {
        let facing = Quat::from_rotation_y(seat.facing);
        b.add_box(seat.hips + facing * Vec3::new(0.0, -0.13, -0.08), Vec3::new(0.23, 0.05, 0.25), facing, paint.seat);
        b.add_box(seat.hips + facing * Vec3::new(0.0, 0.26, 0.21), Vec3::new(0.23, 0.31, 0.05), facing * Quat::from_rotation_x(-0.12), paint.seat);
    }
    // The column stands still in the body, with the wheel turning on top of it as a mesh of its own.
    if let (Some((middle, _)), Some(seat)) = (cfg.steering_wheel(), cfg.seats.iter().find(|seat| seat.hands.is_some())) {
        let floor = Vec3::new(middle.x, seat.feet.y, seat.feet.z - 0.2);
        let span = middle - floor;
        let column = Quat::from_rotation_arc(Vec3::Y, span.normalize_or(Vec3::Y));
        b.add_box((middle + floor) * 0.5, Vec3::new(0.02, span.length() * 0.5, 0.02), column, paint.dark);
    }
    if let Some(front) = cfg.hull.first() {
        let nose = front.center.z - front.half.z;
        let tail = front.center.z + front.half.z;
        let width = front.half.x;
        let low = front.center.y - front.half.y;
        b.add_box(Vec3::new(0.0, low + 0.08, nose - 0.03), Vec3::new(width + 0.03, 0.07, 0.05), Quat::IDENTITY, paint.dark);
        b.add_box(Vec3::new(0.0, low + 0.08, tail + 0.03), Vec3::new(width + 0.03, 0.07, 0.05), Quat::IDENTITY, paint.dark);
        for side in [-1.0, 1.0] {
            let lamp = Vec3::new(side * width * 0.62, front.center.y + front.half.y * 0.52, nose + 0.0);
            b.add_box(lamp, Vec3::new(0.09, 0.09, 0.03), Quat::IDENTITY, paint.lamp);
        }
    }
    // Arches over each wheel, clear of its full travel.
    for wheel in &cfg.wheels {
        let side = wheel.mount.x.signum();
        let top = wheel.mount.y + wheel.radius + 0.03;
        let center = Vec3::new(wheel.mount.x + side * 0.02, top, wheel.mount.z);
        b.add_box(center, Vec3::new(wheel.width * 0.5 + 0.05, 0.025, wheel.radius + 0.08), Quat::IDENTITY, paint.paint);
    }
    b.mesh()
}

/// A door, relative to its hinge.
fn door_mesh(cfg: &VehicleConfig, door: &DoorPanel, paint: &VehiclePaint) -> Mesh {
    let panel = &cfg.panels[door.panel as usize];
    let mut b = Builder::default();
    b.add_box(panel.center - door.hinge, panel.half, panel.rotation, paint.of(panel.look));
    b.mesh()
}

/// A steering wheel flat on its own axis: a ring and three spokes, so it can be seen to turn.
fn steering_wheel_mesh(radius: f32, paint: &VehiclePaint) -> Mesh {
    const SIDES: usize = 10;
    let mut b = Builder::default();
    for i in 0..SIDES {
        let a = (i as f32 + 0.5) / SIDES as f32 * std::f32::consts::TAU;
        let along = std::f32::consts::PI * radius / SIDES as f32 * 1.05;
        let at = Vec3::new(a.cos(), 0.0, a.sin()) * radius;
        b.add_box(at, Vec3::new(0.012, 0.012, along), Quat::from_rotation_y(-a), paint.dark);
    }
    for a in [std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2 + 2.09, std::f32::consts::FRAC_PI_2 - 2.09] {
        let out = Vec3::new(a.cos(), 0.0, a.sin()) * radius * 0.5;
        b.add_box(out, Vec3::new(radius * 0.5, 0.008, 0.012), Quat::from_rotation_y(-a), paint.hub);
    }
    b.mesh()
}

fn wheel_mesh(wheel: &WheelConfig, paint: &VehiclePaint) -> Mesh {
    const SIDES: usize = 10;
    let mut b = Builder::default();
    let half = wheel.width * 0.5;
    b.add_drum(wheel.radius, half, 0.0, SIDES, paint.tyre);
    b.add_drum(wheel.radius * 0.56, 0.02, half, SIDES, paint.hub);
    b.add_drum(wheel.radius * 0.56, 0.02, -half, SIDES, paint.hub);
    // Bolts, so that a turning wheel looks like it is turning.
    for i in 0..5 {
        let a = i as f32 / 5.0 * std::f32::consts::TAU;
        let at = Vec3::new(0.0, a.cos(), a.sin()) * wheel.radius * 0.36;
        for side in [-1.0, 1.0] {
            b.add_box(at + Vec3::X * side * (half + 0.025), Vec3::splat(0.02), Quat::IDENTITY, paint.dark);
        }
    }
    b.mesh()
}

/// One vehicle's root. Its transform is the chassis, between ticks as the runtime blends it.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleRoot {
    pub vehicle: usize,
}

/// A wheel, under the root. Its axle moves with the suspension.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleWheel {
    pub vehicle: usize,
    pub wheel: usize,
}

/// A strut from the chassis down to a wheel. Stretched as the suspension moves.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleStrut {
    pub vehicle: usize,
    pub wheel: usize,
}

/// A door, under the root, turned about its hinge as far as it is open.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleDoor {
    pub vehicle: usize,
    pub entry: usize,
}

/// The steering wheel, under the root. Turned by the same steering as the road wheels.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleSteeringWheel {
    pub vehicle: usize,
}

/// The drawn body, so that its material can change when it burns out.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleBody {
    pub vehicle: usize,
}

/// Everything one kind of vehicle is made of. Build once and share it.
#[derive(Clone)]
pub struct VehicleModel {
    body: Handle<Mesh>,
    wheels: Vec<Handle<Mesh>>,
    strut: Handle<Mesh>,
    steering_wheel: Option<(Handle<Mesh>, Vec3)>,
    /// Each door, by the entry it goes with, and where its hinge is.
    doors: Vec<(usize, Handle<Mesh>, Vec3)>,
    pub painted: Handle<StandardMaterial>,
    pub burnt: Handle<StandardMaterial>,
}

impl VehicleModel {
    pub fn new(cfg: &VehicleConfig, paint: &VehiclePaint, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let painted = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, reflectance: 0.25, ..default() });
        let scorch = paint.burnt;
        let burnt = materials.add(StandardMaterial { base_color: Color::srgb(scorch.x, scorch.y, scorch.z), perceptual_roughness: 1.0, reflectance: 0.05, ..default() });
        let mut strut = Builder::default();
        strut.add_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.035, 0.5, 0.035), Quat::IDENTITY, paint.dark);
        Self {
            body: meshes.add(body_mesh(cfg, paint)),
            wheels: cfg.wheels.iter().map(|wheel| meshes.add(wheel_mesh(wheel, paint))).collect(),
            strut: meshes.add(strut.mesh()),
            doors: cfg
                .entries
                .iter()
                .enumerate()
                .filter_map(|(e, entry)| entry.door.map(|door| (e, meshes.add(door_mesh(cfg, &door, paint)), door.hinge)))
                .collect(),
            steering_wheel: cfg.steering_wheel().map(|(middle, radius)| (meshes.add(steering_wheel_mesh(radius, paint)), middle)),
            painted,
            burnt,
        }
    }
}

/// Draws a vehicle. The scene moves the root and the wheels.
pub fn spawn_vehicle(commands: &mut Commands, model: &VehicleModel, vehicle: usize) {
    commands
        .spawn((VehicleRoot { vehicle }, Transform::default(), Visibility::default()))
        .with_children(|root| {
            root.spawn((VehicleBody { vehicle }, Mesh3d(model.body.clone()), MeshMaterial3d(model.painted.clone()), Transform::default()));
            for (entry, mesh, hinge) in &model.doors {
                root.spawn((VehicleDoor { vehicle, entry: *entry }, Mesh3d(mesh.clone()), MeshMaterial3d(model.painted.clone()), Transform::from_translation(*hinge)));
            }
            if let Some((mesh, middle)) = &model.steering_wheel {
                root.spawn((VehicleSteeringWheel { vehicle }, Mesh3d(mesh.clone()), MeshMaterial3d(model.painted.clone()), Transform::from_translation(*middle)));
            }
            for (wheel, mesh) in model.wheels.iter().enumerate() {
                root.spawn((VehicleWheel { vehicle, wheel }, Mesh3d(mesh.clone()), MeshMaterial3d(model.painted.clone()), Transform::default()));
                root.spawn((VehicleStrut { vehicle, wheel }, Mesh3d(model.strut.clone()), MeshMaterial3d(model.painted.clone()), Transform::default()));
            }
        });
}
