//! Draws the level and what happens in it: terrain, cover, targets, soldiers, tracers, and impacts.

mod bake;
mod climb_hint;
mod flags;
mod soldiers;
mod statics;
mod throwables;
mod helicopters;
mod vehicles;

use bevy::prelude::*;
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::prs_sim::vehicles::{VehicleEvent, VehicleZone};
use prs_runtime::{DrawnGun, GunDrawn, LocalSoldier, LevelReady, SimMessage, SimResource};

const MAX_DEBRIS: usize = 220;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (helicopters::spawn_new_helicopters, helicopters::update_helicopters, helicopters::blow_dust).chain())
            .insert_resource(prs_runtime::prs_sim::prs_mods::tuned("view.smoke_jet", throwables::SmokeJet::default()))
            .insert_resource(prs_runtime::prs_sim::prs_mods::tuned("view.climb_hint", climb_hint::ClimbHintLook::default()))
            .init_resource::<climb_hint::ClimbHint>()
            .add_systems(Update, climb_hint::draw_climb_hint)
            .insert_resource(vehicles::FireLook::default())
            .insert_resource(vehicles::VehicleModels::default())
            .add_systems(
                Update,
                (vehicles::spawn_new_vehicles, vehicles::update_vehicles, vehicles::scorch_wrecks, vehicles::emit_puffs, vehicles::update_puffs).chain(),
            )
            .add_systems(Update, (throwables::update_throwables, throwables::update_smoke_jets, throwables::update_bursts, throwables::update_in_hand.after(GunDrawn)))
            .add_systems(Update, (statics::spawn_level, spawn_targets, flags::spawn_flags).run_if(resource_added::<LevelReady>))
            .add_systems(
                Update,
                (flags::update_flags, flags::update_rallies, soldiers::spawn_new_soldiers, animate_targets, soldiers::update_soldiers.after(GunDrawn), soldiers::update_care_items, soldiers::update_kits, soldiers::flash_other_rifles, spawn_impacts, update_debris, draw_tracers).chain(),
            );
    }
}

#[derive(Component)]
struct TargetVisual {
    id: usize,
    fall: f32,
}

fn spawn_targets(
    mut commands: Commands,
    sim: Res<SimResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cloth = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.2, 0.15),
        perceptual_roughness: 1.0,
        ..default()
    });
    let vest = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.18),
        perceptual_roughness: 1.0,
        ..default()
    });
    let skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.56, 0.42),
        perceptual_roughness: 1.0,
        ..default()
    });
    let legs = meshes.add(Cuboid::new(0.34, 0.86, 0.2));
    let torso = meshes.add(Cuboid::new(0.46, 0.6, 0.24));
    let head = meshes.add(Cuboid::new(0.2, 0.22, 0.2));

    for (id, target) in sim.0.targets.iter().enumerate() {
        commands
            .spawn((
                TargetVisual { id, fall: 0.0 },
                Transform::from_translation(target.pos),
                Visibility::default(),
            ))
            .with_children(|root| {
                root.spawn((Mesh3d(legs.clone()), MeshMaterial3d(cloth.clone()), Transform::from_xyz(0.0, 0.43, 0.0)));
                root.spawn((Mesh3d(torso.clone()), MeshMaterial3d(vest.clone()), Transform::from_xyz(0.0, 1.16, 0.0)));
                root.spawn((Mesh3d(head.clone()), MeshMaterial3d(skin.clone()), Transform::from_xyz(0.0, 1.6, 0.0)));
            });
    }
}

fn animate_targets(
    time: Res<Time>,
    sim: Res<SimResource>,
    mut visuals: Query<(&mut TargetVisual, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut visual, mut transform) in &mut visuals {
        let goal = if sim.0.targets[visual.id].is_up() { 0.0 } else { 1.0 };
        let rate = if goal > visual.fall { 5.5 } else { 2.2 };
        visual.fall = (visual.fall + (goal - visual.fall).clamp(-rate * dt, rate * dt)).clamp(0.0, 1.0);
        // Accelerates as it topples, so a hit reads as weight rather than an animation.
        let angle = visual.fall * visual.fall * std::f32::consts::FRAC_PI_2 * 0.98;
        transform.rotation = Quat::from_rotation_x(-angle);
    }
}

#[derive(Component)]
struct Debris {
    velocity: Vec3,
    life: f32,
    max_life: f32,
    size: f32,
}

#[derive(Default)]
struct DebrisAssets {
    mesh: Option<Handle<Mesh>>,
    dust: Option<Handle<StandardMaterial>>,
    spark: Option<Handle<StandardMaterial>>,
    blood: Option<Handle<StandardMaterial>>,
}

fn spawn_impacts(
    mut commands: Commands,
    mut messages: MessageReader<SimMessage>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<DebrisAssets>,
    existing: Query<Entity, With<Debris>>,
    mut seed: Local<u32>,
) {
    let mut count = existing.iter().count();
    let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0))).clone();
    let dust = assets
        .dust
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.58, 0.5, 0.38),
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .clone();
    let spark = assets
        .spark
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.8, 0.5),
                emissive: LinearRgba::rgb(6.0, 3.5, 1.0),
                unlit: true,
                ..default()
            })
        })
        .clone();
    let blood = assets
        .blood
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.42, 0.07, 0.05),
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .clone();

    let mut random = move || {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };

    for SimMessage(event) in messages.read() {
        let (point, normal, material, n, speed, size) = match *event {
            SimEvent::Impact { point, normal, surface } => {
                use prs_runtime::prs_sim::Surface;
                match surface {
                    Surface::Metal => (point, normal, &spark, 6, 6.0, 0.018),
                    Surface::Concrete => (point, normal, &dust, 7, 3.2, 0.03),
                    _ => (point, normal, &dust, 8, 2.8, 0.05),
                }
            }
            SimEvent::TargetHit { point, .. } | SimEvent::SoldierHit { point, .. } => (point, Vec3::Y, &blood, 7, 2.2, 0.035),
            SimEvent::VehicleHit { point, normal, zone: VehicleZone::Tyre(_), .. } => (point, normal, &dust, 5, 2.0, 0.03),
            SimEvent::VehicleHit { point, normal, .. } => (point, normal, &spark, 7, 6.5, 0.018),
            SimEvent::Vehicle { event: VehicleEvent::Crashed { point, .. }, .. } => (point, Vec3::Y, &spark, 10, 5.0, 0.02),
            _ => continue,
        };
        for _ in 0..n {
            if count >= MAX_DEBRIS {
                break;
            }
            count += 1;
            let spread = Vec3::new(random() - 0.5, random() - 0.5, random() - 0.5) * 2.0;
            let velocity = (normal + spread * 0.75).normalize_or(Vec3::Y) * speed * (0.4 + random());
            let life = 0.35 + random() * 0.45;
            commands.spawn((
                Debris { velocity, life, max_life: life, size: size * (0.6 + random()) },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(point + normal * 0.03).with_scale(Vec3::splat(size)),
            ));
        }
    }
}

fn update_debris(
    mut commands: Commands,
    time: Res<Time>,
    mut debris: Query<(Entity, &mut Debris, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut d, mut transform) in &mut debris {
        d.life -= dt;
        if d.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        d.velocity.y -= 6.0 * dt;
        d.velocity *= (-2.0 * dt).exp();
        let step = d.velocity * dt;
        transform.translation += step;
        transform.rotate_x(dt * 7.0);
        // Dust grows as it thins out, then vanishes.
        let t = d.life / d.max_life;
        transform.scale = Vec3::splat(d.size * (2.2 - 1.2 * t) * t.powf(0.35));
    }
}

/// A tracer's length once it is well away.
const TRACER_LENGTH: f32 = 38.0;
/// How far from the muzzle a tracer has given up the drawn gun for the bullet's real path, in metres.
const TRACER_MEETS_PATH: f32 = 25.0;

/// The two ends of a tracer that has flown `flown` metres along `dir` to `head`. It never reaches back
/// past where the bullet started. `drawn_off` is how far the gun as drawn is from the gun the sim fired:
/// the near end of the tracer starts from the drawn one, and the bullet itself is not touched.
fn tracer_span(head: Vec3, dir: Vec3, flown: f32, drawn_off: Vec3) -> (Vec3, Vec3) {
    let length = flown.min(TRACER_LENGTH);
    let tail = head - dir * length;
    let lean = |from_muzzle: f32| drawn_off * (1.0 - from_muzzle / TRACER_MEETS_PATH).max(0.0);
    (tail + lean(flown - length), head + lean(flown))
}

fn draw_tracers(
    sim: Res<SimResource>,
    time: Res<Time<Fixed>>,
    local: Res<LocalSoldier>,
    drawn: Res<DrawnGun>,
    mut gizmos: Gizmos,
) {
    let overstep = time.overstep_fraction() * time.timestep().as_secs_f32();
    let sim = &sim.0;
    // Your gun on screen has this frame's look and the view's springs on it. The sim's does not.
    let drawn_off = sim.soldiers.get(local.0).map_or(Vec3::ZERO, |soldier| {
        let muzzle = drawn.origin + drawn.rotation * Vec3::NEG_Z * soldier.held_config(&sim.weapons).hold.muzzle_forward;
        muzzle - soldier.gun_pose(&sim.weapons).muzzle
    });
    for flying in &sim.bullets {
        let bullet = &flying.bullet;
        let head = bullet.pos + bullet.vel * overstep;
        let dir = bullet.vel.normalize_or_zero();
        let flown = (bullet.age + overstep) * bullet.vel.length();
        let off = if flying.shooter == local.0 { drawn_off } else { Vec3::ZERO };
        let (tail, head) = tracer_span(head, dir, flown, off);
        gizmos.line(tail, head, Color::srgb(1.0, 0.82, 0.5));
    }
}

#[cfg(test)]
mod tracer_tests {
    use super::*;
    use prs_runtime::prs_sim::weapons::WeaponTable;

    /// A pistol round is slower than a tracer used to grow, so its tail reached up to nine metres back
    /// past the muzzle, behind whoever fired it. No gun's tail may ever reach behind its muzzle.
    #[test]
    fn no_tracer_ever_reaches_behind_the_muzzle_however_slow_the_bullet() {
        for weapon in WeaponTable::default().weapons {
            let muzzle = Vec3::new(2.0, 1.5, -1.0);
            let mut bullet = prs_runtime::prs_sim::Bullet::new(muzzle, Vec3::NEG_Z, &weapon.ballistics);
            for _ in 0..180 {
                bullet.advance(1.0 / 60.0);
                let flown = bullet.age * bullet.vel.length();
                let (tail, _) = tracer_span(bullet.pos, bullet.vel.normalize(), flown, Vec3::ZERO);
                let ahead = (tail - muzzle).dot(Vec3::NEG_Z);
                assert!(ahead >= -1e-3, "{}: tail {ahead} m from the muzzle at age {}", weapon.name, bullet.age);
            }
        }
    }

    #[test]
    fn a_new_tracer_starts_at_the_gun_you_see_and_soon_joins_the_bullets_path() {
        let off = Vec3::new(0.02, -0.01, 0.0);
        let muzzle = Vec3::new(1.0, 1.5, 0.0);
        // Just out of the barrel, it reaches right back to the muzzle as drawn.
        let (tail, _) = tracer_span(muzzle + Vec3::NEG_Z * 14.0, Vec3::NEG_Z, 14.0, off);
        assert!(tail.distance(muzzle + off) < 1e-5, "{tail}");
        // Further on, neither end has anything to do with the gun any more.
        let far = muzzle + Vec3::NEG_Z * 100.0;
        let (tail, head) = tracer_span(far, Vec3::NEG_Z, 100.0, off);
        assert_eq!(head, far);
        assert!(tail.distance(far + Vec3::Z * TRACER_LENGTH) < 1e-4);
    }
}
