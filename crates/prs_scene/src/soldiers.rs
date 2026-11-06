//! Soldiers' bodies and the rifles of everyone but you. Every body is the same skeleton from the same
//! solver, yours included, in first person and third. All that changes for you is that the head goes
//! away when you are inside it, and that your body fits the gun the view actually drew.

use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use prs_models::{CareItem, GunRoot, HeadPart, HeldCare, HeldMagazine, LimbWrap, SHADOW_ONLY_LAYER, SoldierBone, SoldierModel, TeamLooks, spawn_soldier};
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::prs_sim::DroppedKit;
use prs_runtime::prs_sim::body::{Bone, CareKit, CareSite, Pose};
use prs_runtime::prs_sim::weapons::WeaponId;
use prs_runtime::prs_sim::Sim;
use prs_runtime::prs_sim::seats::Frame;
use prs_runtime::prs_sim::heli_of;
use prs_runtime::{HeliFrames, VehicleFrames, DrawnGun, LocalSoldier, LocalView, PendingInput, SimMessage, SimResource, ThirdPerson};

/// The rifle of someone other than you. Yours is drawn by the first-person view.
#[derive(Component)]
pub(crate) struct OtherRifle(usize);

/// Online, slots appear as players join, so anyone not drawn yet gets drawn. Yours gets a rifle like
/// anyone's, which stays empty while it is yours. A slot that changes sides is dressed again.
pub(crate) fn spawn_new_soldiers(
    mut commands: Commands,
    sim: Res<SimResource>,
    looks: Res<TeamLooks>,
    mut drawn: Local<Vec<u8>>,
    mut models: Local<Vec<(u8, SoldierModel)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bones: Query<(Entity, &SoldierBone)>,
) {
    for (id, soldier) in sim.0.soldiers.iter().enumerate() {
        let team = soldier.team;
        if drawn.get(id) == Some(&team) {
            continue;
        }
        if models.iter().all(|(t, _)| *t != team) {
            let model = SoldierModel::new(&sim.0.body_cfg, looks.of(team), &mut meshes, &mut materials);
            models.push((team, model));
        }
        let (_, model) = models.iter().find(|(t, _)| *t == team).expect("just made");
        if id < drawn.len() {
            for (entity, bone) in &bones {
                if bone.soldier == id {
                    commands.entity(entity).despawn();
                }
            }
            drawn[id] = team;
        } else {
            drawn.push(team);
            commands.spawn((OtherRifle(id), GunRoot::default(), Transform::default(), Visibility::default()));
        }
        spawn_soldier(&mut commands, model, id);
    }
}

/// The tick's frame and the drawn frame of whatever this soldier is riding in.
fn carried(sim: &Sim, frames: &VehicleFrames, helis: &HeliFrames, id: usize) -> Option<(Frame, Frame)> {
    let at = sim.soldiers[id].seat?;
    let drawn = drawn_carrier(at.carrier, frames, helis)?;
    Some((sim.carrier_frame(at.carrier), drawn))
}

/// Where whatever is carrying someone is being drawn this frame.
fn drawn_carrier(carrier: u32, frames: &VehicleFrames, helis: &HeliFrames) -> Option<Frame> {
    match heli_of(carrier) {
        Some(h) => helis.blended.get(h).copied(),
        None => frames.blended.get(carrier as usize).copied(),
    }
}

/// Runs after the view has placed the gun, so your hands are on this frame's gun and not the last.
pub(crate) fn update_soldiers(
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    drawn: Res<DrawnGun>,
    input: Res<PendingInput>,
    third: Res<ThirdPerson>,
    frames: Res<VehicleFrames>,
    helis: Res<HeliFrames>,
    mut poses: Local<Vec<Pose>>,
    mut reloads: Local<Vec<f32>>,
    mut bones: Query<(&SoldierBone, &mut Transform), Without<OtherRifle>>,
    mut rifles: Query<(&OtherRifle, &mut Transform, &mut GunRoot), Without<SoldierBone>>,
    mut own_rifle: Query<&mut GunRoot, (Without<OtherRifle>, Without<LyingKit>)>,
    mut held: Query<(&HeldMagazine, &mut Visibility)>,
    mut head_parts: Query<(&HeadPart, &mut RenderLayers)>,
) {
    let sim = &sim.0;
    let reload_cfg = &sim.body_cfg.reload;
    poses.clear();
    reloads.clear();
    for id in 0..sim.soldiers.len() {
        let mut body = sim.body_input(id);
        if id == local.0 && view.blended.life == Life::Up {
            // You see the blended moment between ticks and this frame's look, so your body does too.
            let s = &view.blended;
            let (crouch, prone) = prs_runtime::prs_sim::stance_blend(&sim.move_cfg, s.eye_height);
            body.feet = s.feet;
            body.eye = drawn.eye;
            body.yaw = input.yaw;
            body.pitch = input.pitch;
            body.head_turn = input.free_look;
            body.lean = s.lean;
            body.first_person = !third.0;
            if !third.0 {
                body.hand_at = crate::throwables::first_person_grenade(&sim.soldiers[id], sim.throw_cfg.windup, drawn.eye, input.yaw, input.pitch);
            }
            body.velocity = s.velocity;
            body.grounded = s.grounded;
            body.stride = s.stride;
            body.crouch = crouch;
            body.prone = prone;
            body.ads = s.ads;
            body.ready = s.ready;
            body.reload = s.reload_progress;
            body.gun.origin = drawn.origin;
            body.gun.rotation = drawn.rotation;
            body.anim = s.anim;
            if let (Some(at), Some(frame)) = (sim.soldiers[id].seat, s.seat.and_then(|at| drawn_carrier(at.carrier, &frames, &helis))) {
                body.sit = Some(sim.sit_pose_at(id, at, &frame, input.yaw));
            }
        }
        let mut pose = sim.body_pose(&body);
        if id != local.0 {
            // Worked out at the tick, and then carried to where the vehicle is drawn.
            if let Some((now, drawn)) = carried(sim, &frames, &helis, id) {
                for bone in &mut pose.bones {
                    bone.start = drawn.point(now.to_local(bone.start));
                    bone.rotation = drawn.rot * now.rot.conjugate() * bone.rotation;
                }
            }
        }
        poses.push(pose);
        reloads.push(body.reload);
    }

    for (part, mut transform) in &mut bones {
        let Some(pose) = poses.get(part.soldier) else { continue };
        let bone = pose[part.bone];
        transform.translation = bone.start;
        transform.rotation = bone.rotation;
        // A slot nobody is in shows nothing.
        transform.scale = if sim.soldiers[part.soldier].present { Vec3::ONE } else { Vec3::ZERO };
    }
    for (rifle, mut transform, mut model) in &mut rifles {
        let soldier = &sim.soldiers[rifle.0];
        let pose = soldier.gun_pose(&sim.weapons);
        // A dead soldier's gun is lying with the rest of their kit, and yours is the view's to draw.
        let shown = soldier.present && !soldier.health.is_dead() && rifle.0 != local.0 && !sim.gun_put_away(rifle.0);
        model.weapon = shown.then_some(soldier.weapon.weapon);
        let (origin, rotation) = match carried(sim, &frames, &helis, rifle.0) {
            Some((now, drawn)) => (drawn.point(now.to_local(pose.origin)), drawn.rot * now.rot.conjugate() * pose.rotation),
            None => (pose.origin, pose.rotation),
        };
        transform.translation = origin;
        transform.rotation = rotation;
        model.magazine_out = reload_cfg.magazine_in_hand(reloads[rifle.0]);
    }
    for mut model in &mut own_rifle {
        model.magazine_out = reload_cfg.magazine_in_hand(reloads[local.0]);
    }
    for (magazine, mut visibility) in &mut held {
        let holding = sim.soldiers[magazine.soldier].present && reload_cfg.magazine_in_hand(reloads[magazine.soldier]);
        visibility.set_if_neq(if holding { Visibility::Inherited } else { Visibility::Hidden });
    }
    // Your head stays in the world for the sun to see, so your shadow keeps it.
    let inside_layer = RenderLayers::layer(SHADOW_ONLY_LAYER);
    let seen_layer = RenderLayers::layer(0);
    for (part, mut layers) in &mut head_parts {
        let inside = part.soldier == local.0 && !third.0;
        layers.set_if_neq(if inside { inside_layer.clone() } else { seen_layer.clone() });
    }
}

/// The roll and pouch in a soldier's hands, and the dressing going on or staying on a limb. Hands put
/// the roll wherever it goes, so all there is to do here is how much of each to show.
pub(crate) fn update_care_items(
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    third: Res<ThirdPerson>,
    mut held: Query<(&HeldCare, &mut Visibility, &mut Transform), Without<LimbWrap>>,
    mut wraps: Query<(&LimbWrap, &mut Visibility, &mut Transform), Without<HeldCare>>,
) {
    let sim = &sim.0;
    let anim_of = |id: usize| if id == local.0 { view.blended.anim } else { sim.soldiers[id].anim };
    for (item, mut visibility, mut transform) in &mut held {
        let anim = anim_of(item.soldier);
        let out = sim.soldiers[item.soldier].present && anim.care > 0.15;
        let shown = match item.item {
            CareItem::Roll => {
                let left = 1.0 + (sim.body_cfg.care.roll_left - 1.0) * anim.care_progress;
                // The roll stands along the hand's width, so its radius is the other two of its axes.
                transform.scale = Vec3::new(left, 1.0, left);
                out
            }
            CareItem::Pouch => out && anim.care_kit == CareKit::Medkit,
        };
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (wrap, mut visibility, mut transform) in &mut wraps {
        let soldier = &sim.soldiers[wrap.soldier];
        let anim = anim_of(wrap.soldier);
        let (site, dressed) = match wrap.limb {
            Bone::ForearmL => (CareSite::Arm, soldier.dressed.arm),
            _ => (CareSite::Leg, soldier.dressed.leg),
        };
        // You cannot see your own leg, so what you watch being wrapped is the arm you are holding up.
        let in_view = wrap.soldier == local.0 && !third.0 && wrap.limb == Bone::ForearmL && !matches!(anim.care_site, CareSite::Ally);
        let going_on = if anim.care_site == site || in_view { anim.care_progress * anim.care } else { 0.0 };
        let amount = if dressed && soldier.present { 1.0 } else { going_on };
        transform.translation.y = wrap.at;
        transform.scale = Vec3::new(1.0, amount.max(0.01), 1.0);
        let show = soldier.present && amount > 0.02;
        visibility.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// A kit on the ground, drawn as the gun on top of it and a pouch beside it.
#[derive(Component)]
pub(crate) struct LyingKit {
    id: u32,
    gun: Option<WeaponId>,
}

/// Keeps what is drawn on the ground the same as what the sim says is on the ground.
pub(crate) fn update_kits(
    mut commands: Commands,
    sim: Res<SimResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pouch: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    drawn: Query<(Entity, &LyingKit)>,
) {
    let (mesh, material) = pouch
        .get_or_insert_with(|| {
            let material = StandardMaterial {
                base_color: Color::srgb(0.2, 0.22, 0.15),
                perceptual_roughness: 1.0,
                ..default()
            };
            (meshes.add(Cuboid::new(0.3, 0.12, 0.2)), materials.add(material))
        })
        .clone();
    let top_gun = |kit: &DroppedKit| kit.stash.guns.iter().flatten().next().map(|gun| gun.weapon);
    for (entity, lying) in &drawn {
        let current = sim.0.kits.iter().find(|kit| kit.id == lying.id);
        if current.is_none_or(|kit| top_gun(kit) != lying.gun) {
            commands.entity(entity).despawn();
        }
    }
    for kit in &sim.0.kits {
        let gun = top_gun(kit);
        if drawn.iter().any(|(_, lying)| lying.id == kit.id && lying.gun == gun) {
            continue;
        }
        let turned = kit.id as f32 * 2.399;
        let flat = Quat::from_rotation_y(turned) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut root = commands.spawn((
            LyingKit { id: kit.id, gun },
            Transform::from_translation(kit.pos + Vec3::Y * 0.04).with_rotation(flat),
            Visibility::default(),
        ));
        if let Some(weapon) = gun {
            root.insert(GunRoot::lying(weapon));
        }
        root.with_children(|parts| {
            parts.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(-0.05, 0.0, 0.3).with_rotation(flat.inverse()),
            ));
        });
    }
}

pub(crate) fn flash_other_rifles(
    sim: Res<SimResource>,
    mut messages: MessageReader<SimMessage>,
    mut rifles: Query<(&OtherRifle, &mut GunRoot)>,
) {
    for SimMessage(event) in messages.read() {
        if let SimEvent::ShotFired { soldier, weapon, .. } = *event {
            for (other, mut rifle) in &mut rifles {
                if other.0 == soldier {
                    rifle.flash = sim.0.weapons[weapon].flash.time;
                }
            }
        }
    }
}
