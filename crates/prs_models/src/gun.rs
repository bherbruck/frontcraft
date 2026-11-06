//! Guns as low-poly parts. Each shape takes its measurements from the weapon's own hold, so the sights
//! on the model are where the simulation says they are. Parts are built once per weapon and shared.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use prs_runtime::SimResource;
use prs_runtime::prs_sim::weapons::{GunShape, HoldConfig, WeaponConfig, WeaponId, WeaponTable};

/// Root of a gun model. Its origin is the receiver, and it points down its local -Z. Set `weapon` and
/// the parts follow.
#[derive(Component, Default)]
pub struct GunRoot {
    /// What should be drawn here, if anything.
    pub weapon: Option<WeaponId>,
    shown: Option<WeaponId>,
    /// Seconds of muzzle flash left.
    pub flash: f32,
    /// The magazine has come off for a reload.
    pub magazine_out: bool,
    /// Lying on the ground, so it will never fire and needs no flash or light.
    pub inert: bool,
    /// At the shoulder of whoever is looking through it, the stock is behind their eye. Drawn, it
    /// would only be a big dark shape in the way of the sights.
    pub stock_hidden: bool,
}

impl GunRoot {
    pub fn holding(weapon: WeaponId) -> Self {
        Self { weapon: Some(weapon), ..default() }
    }

    pub fn lying(weapon: WeaponId) -> Self {
        Self { weapon: Some(weapon), inert: true, ..default() }
    }
}

#[derive(Component)]
pub(crate) struct MuzzleFlash;

#[derive(Component)]
pub(crate) struct GunMagazine;

#[derive(Component)]
pub(crate) struct MuzzleLight;

#[derive(Component)]
pub(crate) struct GunStock;

#[derive(Clone, Copy)]
enum Paint {
    Black,
    Dark,
    Tan,
    Olive,
    Wood,
}

type Spec = (Vec3, Vec3, f32, Paint);
type Part = (Handle<Mesh>, Handle<StandardMaterial>, Transform);

struct GunModel {
    parts: Vec<Part>,
    /// Behind the receiver, and behind your eye once it is at your shoulder.
    stock: Vec<Part>,
    magazine: Part,
    flash: Transform,
    light: f32,
    light_range: f32,
    light_at: Vec3,
}

#[derive(Resource)]
pub struct GunModels {
    models: Vec<GunModel>,
    flash_mesh: Handle<Mesh>,
    flash_material: Handle<StandardMaterial>,
}

/// What a piece of a sight is made of.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Form {
    Block(Vec3),
    /// A round opening looking along the barrel: radii and thickness along it.
    Ring { inner: f32, outer: f32, depth: f32 },
}

#[derive(Clone, Copy)]
struct Piece {
    form: Form,
    at: Vec3,
    /// Leaning sideways, in radians about the barrel.
    roll: f32,
    paint: Paint,
}

fn block(size: Vec3, at: Vec3, paint: Paint) -> Piece {
    Piece { form: Form::Block(size), at, roll: 0.0, paint }
}

/// A block between two points in the plane across the barrel, for anything that leans.
fn strut(from: Vec2, to: Vec2, thick: f32, depth: f32, z: f32, paint: Paint) -> Piece {
    let along = to - from;
    let middle = (from + to) * 0.5;
    Piece {
        form: Form::Block(Vec3::new(thick, along.length(), depth)),
        at: Vec3::new(middle.x, middle.y, z),
        roll: -along.x.atan2(along.y),
        paint,
    }
}

/// A front post with its top exactly on the line of sight, reaching down to `base`.
fn front_post(h: &HoldConfig, base: f32, width: f32, depth: f32, paint: Paint) -> Piece {
    let length = h.sight_height - base;
    block(Vec3::new(width, length, depth), Vec3::new(0.0, h.sight_height - length * 0.5, -h.front_sight), paint)
}

/// Each gun's own irons. All of them are built from the line of sight: the top of the front post and
/// the middle of the rear opening are exactly at `sight_height`, whatever else is around them.
fn sights(cfg: &WeaponConfig) -> Vec<Piece> {
    let h = &cfg.hold;
    let (sight, front, rear) = (h.sight_height, -h.front_sight, -h.rear_sight);
    let on_sight = |z: f32| Vec3::new(0.0, sight, z);
    match cfg.shape {
        // A peep you look through, on a thin stem off the rail, and a post in an A-frame of wings.
        GunShape::Carbine => {
            let (rail, front_top) = (0.017, 0.03);
            vec![
                Piece { form: Form::Ring { inner: 0.0078, outer: 0.0112, depth: 0.005 }, at: on_sight(rear), roll: 0.0, paint: Paint::Black },
                block(Vec3::new(0.0035, sight - 0.0112 - rail, 0.006), Vec3::new(0.0, (sight - 0.0112 + rail) * 0.5, rear), Paint::Black),
                block(Vec3::new(0.018, 0.005, 0.03), Vec3::new(0.0, rail + 0.0025, rear), Paint::Dark),
                front_post(h, front_top, 0.0028, 0.005, Paint::Black),
                strut(Vec2::new(-0.016, front_top), Vec2::new(-0.0065, sight - 0.003), 0.003, 0.008, front, Paint::Black),
                strut(Vec2::new(0.016, front_top), Vec2::new(0.0065, sight - 0.003), 0.003, 0.008, front, Paint::Black),
                block(Vec3::new(0.034, 0.012, 0.024), Vec3::new(0.0, front_top - 0.005, front), Paint::Dark),
            ]
        }
        // An open ladder of a rear leaf with a notch in its bar, and a hooded post.
        GunShape::MachineGun => {
            let cover = 0.011;
            let bar_top = sight - 0.0035;
            let hood = 0.0125;
            let block_top = 0.042;
            vec![
                block(Vec3::new(0.002, bar_top - cover, 0.004), Vec3::new(-0.0135, (bar_top + cover) * 0.5, rear), Paint::Black),
                block(Vec3::new(0.002, bar_top - cover, 0.004), Vec3::new(0.0135, (bar_top + cover) * 0.5, rear), Paint::Black),
                block(Vec3::new(0.03, 0.005, 0.004), Vec3::new(0.0, bar_top - 0.0025, rear), Paint::Black),
                block(Vec3::new(0.0085, 0.008, 0.004), Vec3::new(-0.0105, bar_top + 0.004, rear), Paint::Black),
                block(Vec3::new(0.0085, 0.008, 0.004), Vec3::new(0.0105, bar_top + 0.004, rear), Paint::Black),
                front_post(h, block_top, 0.0035, 0.006, Paint::Black),
                Piece { form: Form::Ring { inner: hood, outer: hood + 0.003, depth: 0.012 }, at: on_sight(front), roll: 0.0, paint: Paint::Black },
                block(Vec3::new(0.026, block_top - 0.008, 0.03), Vec3::new(0.0, (block_top + 0.008) * 0.5, front), Paint::Dark),
            ]
        }
        // A smaller peep between two protective ears, and a post with upright wings.
        GunShape::MarksmanRifle => {
            let (receiver, front_top) = (0.02, 0.034);
            let ear_top = sight + 0.008;
            vec![
                Piece { form: Form::Ring { inner: 0.0062, outer: 0.0096, depth: 0.005 }, at: on_sight(rear), roll: 0.0, paint: Paint::Black },
                block(Vec3::new(0.0035, sight - 0.0096 - receiver, 0.006), Vec3::new(0.0, (sight - 0.0096 + receiver) * 0.5, rear), Paint::Black),
                block(Vec3::new(0.004, ear_top - receiver, 0.012), Vec3::new(-0.017, (ear_top + receiver) * 0.5, rear), Paint::Dark),
                block(Vec3::new(0.004, ear_top - receiver, 0.012), Vec3::new(0.017, (ear_top + receiver) * 0.5, rear), Paint::Dark),
                block(Vec3::new(0.038, 0.005, 0.016), Vec3::new(0.0, receiver + 0.0025, rear), Paint::Dark),
                front_post(h, front_top, 0.0028, 0.005, Paint::Black),
                block(Vec3::new(0.003, sight - front_top, 0.008), Vec3::new(-0.0105, (sight + front_top) * 0.5, front), Paint::Black),
                block(Vec3::new(0.003, sight - front_top, 0.008), Vec3::new(0.0105, (sight + front_top) * 0.5, front), Paint::Black),
                block(Vec3::new(0.024, front_top - 0.006, 0.022), Vec3::new(0.0, (front_top + 0.006) * 0.5, front), Paint::Dark),
            ]
        }
        // A notch cut in a block on the slide and a plain blade. Nothing else.
        GunShape::Pistol => {
            let slide = 0.021;
            let notch_floor = sight - 0.0022;
            vec![
                block(Vec3::new(0.026, notch_floor - slide, 0.012), Vec3::new(0.0, (notch_floor + slide) * 0.5, rear), Paint::Black),
                block(Vec3::new(0.0085, 0.006, 0.012), Vec3::new(-0.00875, notch_floor + 0.003, rear), Paint::Black),
                block(Vec3::new(0.0085, 0.006, 0.012), Vec3::new(0.00875, notch_floor + 0.003, rear), Paint::Black),
                front_post(h, slide, 0.0042, 0.012, Paint::Black),
            ]
        }
    }
}

/// A ring with its opening along the barrel, facing front and back.
fn ring_mesh(inner: f32, outer: f32, depth: f32) -> Mesh {
    const SIDES: usize = 10;
    let (mut positions, mut indices) = (Vec::new(), Vec::new());
    let mut quad = |points: [Vec3; 4]| {
        let base = positions.len() as u32;
        positions.extend(points.map(|p| p.to_array()));
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let half = depth * 0.5;
    let corner = |i: usize, r: f32, z: f32| {
        let (s, c) = (i as f32 / SIDES as f32 * std::f32::consts::TAU).sin_cos();
        Vec3::new(c * r, s * r, z)
    };
    for i in 0..SIDES {
        let j = i + 1;
        quad([corner(i, inner, half), corner(i, outer, half), corner(j, outer, half), corner(j, inner, half)]);
        quad([corner(j, inner, -half), corner(j, outer, -half), corner(i, outer, -half), corner(i, inner, -half)]);
        quad([corner(i, outer, -half), corner(j, outer, -half), corner(j, outer, half), corner(i, outer, half)]);
        quad([corner(i, inner, half), corner(j, inner, half), corner(j, inner, -half), corner(i, inner, -half)]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
        .with_duplicated_vertices()
        .with_computed_flat_normals()
}

/// Where the corners of a flash are, for a unit length and width. It starts narrow at z = 0, swells
/// quickly into a spiky star, and comes to a point at z = -1.
fn flash_rings() -> [(f32, f32, f32); 3] {
    // How far along, and radius on the points and between them.
    [(0.0, 0.1, 0.1), (-0.3, 0.5, 0.2), (-1.0, 0.02, 0.02)]
}

fn flash_mesh() -> Mesh {
    const POINTS: usize = 5;
    let corner = |ring: (f32, f32, f32), i: usize| {
        let (s, c) = (i as f32 / (POINTS * 2) as f32 * std::f32::consts::TAU).sin_cos();
        let radius = if i % 2 == 0 { ring.1 } else { ring.2 };
        [c * radius, s * radius, ring.0]
    };
    let (mut positions, mut indices) = (Vec::new(), Vec::new());
    let rings = flash_rings();
    for ring in rings {
        for i in 0..POINTS * 2 {
            positions.push(corner(ring, i));
        }
    }
    let count = (POINTS * 2) as u32;
    for r in 0..rings.len() as u32 - 1 {
        for i in 0..count {
            let j = (i + 1) % count;
            let (a, b, c, d) = (r * count + i, r * count + j, (r + 1) * count + j, (r + 1) * count + i);
            indices.extend([a, c, b, a, d, c]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
        .with_duplicated_vertices()
        .with_computed_flat_normals()
}

/// Puts a flash with its narrow end exactly on the muzzle, as long and wide as this gun's is.
fn flash_transform(cfg: &WeaponConfig) -> Transform {
    let f = &cfg.flash;
    Transform::from_xyz(0.0, 0.0, -cfg.hold.muzzle_forward).with_scale(Vec3::new(f.width, f.width, f.length))
}

/// The parts of a gun, then its stock, then the magazine that comes off it. Barrels are on y = 0, which
/// is the bore the simulation fires along.
fn shape(cfg: &WeaponConfig) -> (Vec<Spec>, Vec<Spec>, Spec) {
    let h = &cfg.hold;
    let muzzle = h.muzzle_forward;
    match cfg.shape {
        GunShape::Carbine => {
            let parts = vec![
                // Upper and lower receiver.
                (Vec3::new(0.034, 0.034, 0.21), Vec3::new(0.0, 0.0, -0.02), 0.0, Paint::Black),
                (Vec3::new(0.03, 0.03, 0.14), Vec3::new(0.0, -0.03, 0.0), 0.0, Paint::Dark),
                // Handguard and barrel.
                (Vec3::new(0.038, 0.04, 0.2), Vec3::new(0.0, -0.002, -0.22), 0.0, Paint::Tan),
                (Vec3::new(0.013, 0.013, 0.17), Vec3::new(0.0, 0.0, -0.4), 0.0, Paint::Black),
                (Vec3::new(0.021, 0.021, 0.05), Vec3::new(0.0, 0.0, -0.49), 0.0, Paint::Dark),
                // Grip.
                (Vec3::new(0.026, 0.085, 0.033), Vec3::new(0.0, -0.075, 0.07), -0.35, Paint::Black),
            ];
            let stock = vec![
                // Buffer tube and stock.
                (Vec3::new(0.022, 0.022, 0.15), Vec3::new(0.0, -0.002, 0.15), 0.0, Paint::Black),
                (Vec3::new(0.03, 0.075, 0.1), Vec3::new(0.0, -0.016, 0.24), 0.0, Paint::Tan),
            ];
            (parts, stock, (Vec3::new(0.022, 0.13, 0.05), Vec3::new(0.0, -0.1, -0.055), 0.2, Paint::Dark))
        }
        GunShape::MachineGun => {
            let parts = vec![
                // A big square receiver with the feed cover on top, kept low behind the rear sight
                // or at the shoulder it fills the view.
                (Vec3::new(0.05, 0.062, 0.28), Vec3::new(0.0, -0.02, -0.03), 0.0, Paint::Black),
                (Vec3::new(0.044, 0.02, 0.14), Vec3::new(0.0, 0.02, -0.1), 0.0, Paint::Dark),
                // Handguard, a barrel with its gas tube underneath, and the flash hider.
                (Vec3::new(0.052, 0.05, 0.2), Vec3::new(0.0, -0.012, -0.26), 0.0, Paint::Dark),
                (Vec3::new(0.019, 0.019, 0.44), Vec3::new(0.0, 0.0, -0.52), 0.0, Paint::Black),
                (Vec3::new(0.012, 0.012, 0.3), Vec3::new(0.0, -0.022, -0.5), 0.0, Paint::Dark),
                (Vec3::new(0.03, 0.03, 0.06), Vec3::new(0.0, 0.0, -(muzzle - 0.03)), 0.0, Paint::Dark),
                // Folded bipod legs against the barrel.
                (Vec3::new(0.01, 0.012, 0.2), Vec3::new(-0.022, -0.03, -0.55), 0.0, Paint::Black),
                (Vec3::new(0.01, 0.012, 0.2), Vec3::new(0.022, -0.03, -0.55), 0.0, Paint::Black),
                (Vec3::new(0.03, 0.095, 0.036), Vec3::new(0.0, -0.09, 0.09), -0.35, Paint::Black),
            ];
            // A heavy skeleton stock.
            let stock = vec![
                (Vec3::new(0.024, 0.024, 0.16), Vec3::new(0.0, -0.034, 0.18), 0.0, Paint::Black),
                (Vec3::new(0.03, 0.09, 0.07), Vec3::new(0.0, -0.058, 0.28), 0.0, Paint::Olive),
            ];
            // The belt's soft box.
            (parts, stock, (Vec3::new(0.085, 0.13, 0.13), Vec3::new(-0.018, -0.105, -0.08), 0.0, Paint::Olive))
        }
        GunShape::MarksmanRifle => {
            let parts = vec![
                (Vec3::new(0.036, 0.04, 0.3), Vec3::new(0.0, 0.0, -0.03), 0.0, Paint::Black),
                // A long wooden-looking fore end and stock.
                (Vec3::new(0.042, 0.042, 0.32), Vec3::new(0.0, -0.006, -0.32), 0.0, Paint::Wood),
                (Vec3::new(0.014, 0.014, 0.29), Vec3::new(0.0, 0.0, -0.625), 0.0, Paint::Black),
                (Vec3::new(0.024, 0.024, 0.06), Vec3::new(0.0, 0.0, -(muzzle - 0.03)), 0.0, Paint::Dark),
                (Vec3::new(0.028, 0.085, 0.034), Vec3::new(0.0, -0.075, 0.07), -0.35, Paint::Black),
            ];
            let stock = vec![
                (Vec3::new(0.028, 0.055, 0.2), Vec3::new(0.0, -0.03, 0.19), 0.0, Paint::Wood),
                (Vec3::new(0.03, 0.09, 0.05), Vec3::new(0.0, -0.045, 0.31), 0.0, Paint::Black),
            ];
            (parts, stock, (Vec3::new(0.024, 0.1, 0.07), Vec3::new(0.0, -0.075, -0.06), 0.05, Paint::Dark))
        }
        GunShape::Pistol => {
            let parts = vec![
                // Slide, frame, grip and trigger guard.
                (Vec3::new(0.028, 0.03, 0.2), Vec3::new(0.0, 0.006, -(muzzle - 0.1)), 0.0, Paint::Black),
                (Vec3::new(0.026, 0.02, 0.15), Vec3::new(0.0, -0.018, -0.05), 0.0, Paint::Dark),
                (Vec3::new(0.028, 0.092, 0.04), Vec3::new(0.0, -0.062, 0.028), -0.22, Paint::Dark),
                (Vec3::new(0.012, 0.022, 0.05), Vec3::new(0.0, -0.038, -0.03), 0.0, Paint::Dark),
            ];
            (parts, Vec::new(), (Vec3::new(0.022, 0.03, 0.034), Vec3::new(0.0, -0.115, 0.042), -0.22, Paint::Black))
        }
    }
}

impl GunModels {
    pub fn new(table: &WeaponTable, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let mut paint = |color: Color, rough: f32| {
            materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: rough,
                metallic: 0.15,
                ..default()
            })
        };
        let black = paint(Color::srgb(0.055, 0.058, 0.062), 0.55);
        let dark = paint(Color::srgb(0.1, 0.105, 0.11), 0.7);
        let tan = paint(Color::srgb(0.36, 0.32, 0.24), 0.85);
        let olive = paint(Color::srgb(0.2, 0.23, 0.14), 0.95);
        let wood = paint(Color::srgb(0.2, 0.135, 0.08), 0.8);
        let material = |p: Paint| match p {
            Paint::Black => black.clone(),
            Paint::Dark => dark.clone(),
            Paint::Tan => tan.clone(),
            Paint::Olive => olive.clone(),
            Paint::Wood => wood.clone(),
        };
        let mut built = |piece: Piece, tilt: f32| -> Part {
            let mesh = match piece.form {
                Form::Block(size) => meshes.add(Cuboid::from_size(size)),
                Form::Ring { inner, outer, depth } => meshes.add(ring_mesh(inner, outer, depth)),
            };
            let rotation = Quat::from_rotation_x(tilt) * Quat::from_rotation_z(piece.roll);
            (mesh, material(piece.paint), Transform::from_translation(piece.at).with_rotation(rotation))
        };
        let models = table
            .weapons
            .iter()
            .map(|cfg| {
                let (body, stock, magazine) = shape(cfg);
                let mut solid = |spec: Spec| built(block(spec.0, spec.1, spec.3), spec.2);
                let mut parts: Vec<Part> = body.into_iter().map(&mut solid).collect();
                let stock = stock.into_iter().map(&mut solid).collect();
                let magazine = solid(magazine);
                parts.extend(sights(cfg).into_iter().map(|piece| built(piece, 0.0)));
                GunModel {
                    parts,
                    stock,
                    magazine,
                    flash: flash_transform(cfg),
                    light: cfg.flash.light,
                    light_range: cfg.flash.light_range,
                    light_at: Vec3::new(0.0, 0.0, -cfg.hold.muzzle_forward - cfg.flash.length * 0.5),
                }
            })
            .collect();

        let flash_material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.8, 0.45),
            emissive: LinearRgba::rgb(14.0, 9.0, 3.5),
            unlit: true,
            cull_mode: None,
            ..default()
        });
        Self { models, flash_mesh: meshes.add(flash_mesh()), flash_material }
    }

    fn spawn(&self, root: &mut ChildSpawnerCommands, weapon: WeaponId, inert: bool) {
        let model = &self.models[weapon.0 as usize];
        let (mesh, material, transform) = &model.magazine;
        // Comes off during a reload, so it is not with the rest.
        root.spawn((GunMagazine, Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *transform, NotShadowCaster));
        for (mesh, material, transform) in &model.parts {
            root.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *transform, NotShadowCaster));
        }
        for (mesh, material, transform) in &model.stock {
            root.spawn((GunStock, Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *transform, NotShadowCaster));
        }
        if inert {
            return;
        }
        root.spawn((
            MuzzleFlash,
            Mesh3d(self.flash_mesh.clone()),
            MeshMaterial3d(self.flash_material.clone()),
            model.flash,
            Visibility::Hidden,
            NotShadowCaster,
        ));
        root.spawn((
            MuzzleLight,
            PointLight {
                color: Color::srgb(1.0, 0.72, 0.4),
                intensity: 0.0,
                range: model.light_range,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(model.light_at),
        ));
    }
}

pub(crate) fn build_gun_models(
    mut commands: Commands,
    sim: Res<SimResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GunModels::new(&sim.0.weapons, &mut meshes, &mut materials));
}

/// Swaps the parts under any gun whose weapon has changed.
pub(crate) fn show_guns(mut commands: Commands, models: Res<GunModels>, mut guns: Query<(Entity, &mut GunRoot)>) {
    for (entity, mut gun) in &mut guns {
        if gun.shown == gun.weapon {
            continue;
        }
        gun.shown = gun.weapon;
        let mut root = commands.entity(entity);
        root.despawn_related::<Children>();
        if let Some(weapon) = gun.weapon {
            let inert = gun.inert;
            root.with_children(|parts| models.spawn(parts, weapon, inert));
        }
    }
}

pub(crate) fn update_flashes(
    time: Res<Time>,
    mut guns: Query<&mut GunRoot>,
    mut flashes: Query<(&ChildOf, &mut Visibility), With<MuzzleFlash>>,
    mut lights: Query<(&ChildOf, &mut PointLight), With<MuzzleLight>>,
    mut magazines: Query<(&ChildOf, &mut Visibility), (With<GunMagazine>, Without<MuzzleFlash>)>,
    mut stocks: Query<(&ChildOf, &mut Visibility), (With<GunStock>, Without<MuzzleFlash>, Without<GunMagazine>)>,
    models: Res<GunModels>,
) {
    for (parent, mut visibility) in &mut stocks {
        let hidden = guns.get(parent.parent()).is_ok_and(|g| g.stock_hidden);
        visibility.set_if_neq(if hidden { Visibility::Hidden } else { Visibility::Inherited });
    }
    for (parent, mut visibility) in &mut magazines {
        let out = guns.get(parent.parent()).is_ok_and(|g| g.magazine_out);
        visibility.set_if_neq(if out { Visibility::Hidden } else { Visibility::Inherited });
    }
    for (parent, mut visibility) in &mut flashes {
        let on = guns.get(parent.parent()).is_ok_and(|g| g.flash > 0.0);
        visibility.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (parent, mut light) in &mut lights {
        let gun = guns.get(parent.parent()).ok().filter(|g| g.flash > 0.0);
        let shown = gun.and_then(|g| g.shown);
        light.intensity = shown.map_or(0.0, |weapon| models.models[weapon.0 as usize].light);
    }
    for mut gun in &mut guns {
        gun.flash -= time.delta_secs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guns() -> Vec<WeaponConfig> {
        WeaponTable::default().weapons
    }

    #[test]
    fn a_flash_starts_at_the_muzzle_and_only_goes_forward() {
        for cfg in guns() {
            let t = flash_transform(&cfg);
            assert_eq!(t.translation.z, -cfg.hold.muzzle_forward, "{}", cfg.name);
            assert_eq!(t.translation.truncate(), Vec2::ZERO, "{}: off the bore", cfg.name);
            for (along, ..) in flash_rings() {
                let z = t.translation.z + along * t.scale.z;
                assert!(z <= t.translation.z, "{}: flash reaches behind the muzzle", cfg.name);
                assert!(z >= -cfg.hold.muzzle_forward - cfg.flash.length - 1e-5);
            }
            // Narrow where it leaves the barrel, so it does not wrap round the end of it.
            let (_, at_muzzle, _) = flash_rings()[0];
            assert!(at_muzzle * t.scale.x < 0.015, "{}", cfg.name);
        }
    }

    /// The flash, the light and the bullet all come from this point, so the model has to end there.
    #[test]
    fn every_model_ends_at_the_muzzle_the_simulation_fires_from() {
        for cfg in guns() {
            let (body, _, _) = shape(&cfg);
            let front = body.iter().map(|(size, at, ..)| at.z - size.z * 0.5).fold(0.0, f32::min);
            assert!((front + cfg.hold.muzzle_forward).abs() < 1e-4, "{}: model ends at {front}", cfg.name);
        }
    }

    #[test]
    fn the_flash_light_is_a_flicker_and_the_pistol_has_the_least() {
        let table = WeaponTable::default();
        let pistol = table[WeaponTable::PISTOL].flash;
        for cfg in guns() {
            // Lux on a surface a metre away. The sun on a clear day here is several times this.
            let at_a_metre = cfg.flash.light / (4.0 * std::f32::consts::PI);
            assert!(at_a_metre < 3500.0, "{} throws {at_a_metre} lux at 1 m", cfg.name);
            if cfg.shape != GunShape::Pistol {
                assert!(cfg.flash.light > pistol.light && cfg.flash.light_range > pistol.light_range, "{}", cfg.name);
            }
        }
    }

    #[test]
    fn a_pistol_flash_is_small_and_a_machine_gun_flash_is_big() {
        let t = WeaponTable::default();
        let (pistol, rifle, lmg) = (t[WeaponTable::PISTOL].flash, t[WeaponTable::ASSAULT_RIFLE].flash, t[WeaponTable::MACHINE_GUN].flash);
        assert!(pistol.length < rifle.length && rifle.length < lmg.length);
        assert!(pistol.width < rifle.width && rifle.width < lmg.width);
    }

    /// The top of the front post is the point on screen the bullet goes to, so it has to be exact.
    #[test]
    fn every_front_post_tops_out_exactly_on_the_line_of_sight() {
        for cfg in guns() {
            let h = &cfg.hold;
            let post = sights(&cfg)
                .into_iter()
                .filter(|p| p.at.z == -h.front_sight && p.at.x == 0.0 && p.roll == 0.0)
                .find_map(|p| match p.form {
                    Form::Block(size) if size.x < 0.005 => Some((p.at, size)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{} has no front post", cfg.name));
            assert!((post.0.y + post.1.y * 0.5 - h.sight_height).abs() < 1e-5, "{}", cfg.name);
        }
    }

    /// Nothing solid on the gun may sit on the line from the rear sight to the front post.
    #[test]
    fn the_line_of_sight_is_clear_all_the_way_along_the_gun() {
        for cfg in guns() {
            let h = &cfg.hold;
            let (body, _, magazine) = shape(&cfg);
            let mut boxes: Vec<(Vec3, Vec3)> = body.iter().map(|s| (s.0, s.1)).collect();
            boxes.push((magazine.0, magazine.1));
            for piece in sights(&cfg) {
                if let Form::Block(size) = piece.form
                    && piece.roll == 0.0
                {
                    boxes.push((size, piece.at));
                }
            }
            // Just over the post, from behind the rear sight to in front of the muzzle.
            let eye_line = h.sight_height + 0.0005;
            for step in 0..200 {
                let z = -h.rear_sight + 0.02 - (h.muzzle_forward + 0.1) * step as f32 / 200.0;
                let point = Vec3::new(0.0, eye_line, z);
                for (size, at) in &boxes {
                    let inside = (point - *at).abs().cmple(*size * 0.5).all();
                    assert!(!inside, "{}: {size} at {at} blocks the sights at z {z}", cfg.name);
                }
            }
        }
    }

    #[test]
    fn apertures_are_centred_on_the_line_of_sight_and_wide_enough_to_see_the_front_sight_through() {
        for cfg in guns() {
            let h = &cfg.hold;
            for piece in sights(&cfg) {
                if let Form::Ring { inner, .. } = piece.form {
                    assert_eq!(piece.at.truncate(), Vec2::new(0.0, h.sight_height), "{}", cfg.name);
                    assert!(inner > 0.005, "{}", cfg.name);
                }
            }
        }
    }

    #[test]
    fn no_two_guns_have_the_same_sights() {
        let forms: Vec<Vec<Form>> = guns().iter().map(|cfg| sights(cfg).iter().map(|p| p.form).collect()).collect();
        for (i, a) in forms.iter().enumerate() {
            for b in &forms[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn a_pistol_has_a_notch_and_a_blade_and_nothing_else() {
        let cfg = WeaponConfig::pistol();
        let pieces = sights(&cfg);
        assert!(pieces.iter().all(|p| matches!(p.form, Form::Block(_))), "no rings on a pistol");
        let front = pieces.iter().filter(|p| p.at.z == -cfg.hold.front_sight).count();
        assert_eq!(front, 1, "a plain blade: no wings and no base");
    }
}
