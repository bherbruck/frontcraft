//! First-person presentation: the camera rig and the weapon in your hands.
//!
//! Nothing here affects gameplay. The sim decides where a bullet goes; this crate makes that
//! decision feel physical. Gun movement mostly moves the rifle rather than the screen, which is
//! far less jarring. The rifle is turned about the eye by whatever the camera did not take, so the
//! front sight on screen always marks where the bullet is going.

mod distance;
mod spring;

use bevy::prelude::*;
pub use distance::ViewDistance;
use bevy::render::view::ColorGrading;
use prs_atmosphere::Atmosphere;
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::prs_sim::movement::{MoveEvent, Stance};
use prs_models::GunRoot;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::prs_sim::weapons::{GunPoseInput, WeaponEvent, gun_pose};
use prs_runtime::{
    HeliFrames, VehicleFrames, DrawnGun, GunDrawn, LocalSoldier, LocalView, LookScale, PendingInput, SimMessage, SimResource,
    ThirdPerson,
};
use spring::Spring;

/// How far up to the shoulder the gun is before its stock is behind your eye and no longer drawn.
const STOCK_HIDDEN_FROM_ADS: f32 = 0.55;

/// Vertical field of view limits, in degrees.
pub const FOV_RANGE: (f32, f32) = (50.0, 85.0);
pub const ZOOM_FOV_MIN: f32 = 20.0;

#[derive(Resource, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewSettings {
    fov: f32,
    zoom_fov: f32,
    /// How quickly the view eases towards the zoom being asked for, per second.
    pub zoom_rate: f32,
    /// Where the camera sits watching yourself from outside, from your eye in the view's frame.
    pub third_person_offset: Vec3,
    /// Share of recoil shown by turning the whole screen. The rifle shows the rest, along with all sway
    /// and wander: a screen that drifts on its own is what makes people feel sick.
    pub camera_recoil_share: f32,
    /// What a shot does to the whole screen, as a push on its pitch, yaw and roll, plus a little more
    /// roll, and how much of that going to the sights takes out. A touch of this sells a shot.
    pub shot_punch: Vec3,
    pub shot_tilt: f32,
    pub shot_steady_at_sights: f32,
    /// What a shot does to the gun in your hands: back along itself and up, and its twist.
    pub shot_kick_back: f32,
    pub shot_kick_up: f32,
    pub shot_twist: Vec3,
    /// How far a landing drops the view, and how hard it nods.
    pub landing_drop: f32,
    pub landing_nod: f32,
    /// In a seat the eye's height follows the chassis this quickly, per second, and no further behind
    /// than this: the head takes the small bumps out as a neck does, so the road does not shake the screen.
    pub seat_settle_rate: f32,
    pub seat_settle_limit: f32,
    /// How much of a vehicle's roll reaches the view. The horizon staying nearly level is what lets
    /// people ride without feeling sick.
    pub seat_roll_share: f32,
    /// How fast the view of someone at a helicopter's controls follows the airframe, per second, and how
    /// much of its roll it follows. It leads nothing and adds nothing, so it stays gentle in any weather.
    pub pilot_follow_rate: f32,
    pub pilot_roll_share: f32,
    /// How far down a pilot's head rests, so the gauges are in view with the horizon.
    pub pilot_head_down: f32,
    /// How far the view rolls at full lean, radians. Far less than the gun and body do: a rolling
    /// horizon is the classic way to make people sick, so it stays small and can go to nothing.
    pub lean_roll: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self { fov: 62.0, zoom_fov: 40.0, zoom_rate: 12.0, third_person_offset: Vec3::new(1.5, 0.35, 3.4), camera_recoil_share: 0.15,
            shot_punch: Vec3::new(0.45, 0.2, 0.45),
            shot_tilt: 0.07,
            shot_steady_at_sights: 0.45,
            shot_kick_back: 5.0,
            shot_kick_up: 0.5,
            shot_twist: Vec3::new(14.0, 3.5, 6.0),
            landing_drop: 1.9,
            landing_nod: 0.35, lean_roll: 0.05, seat_settle_rate: 9.0, seat_settle_limit: 0.06, seat_roll_share: 0.2, pilot_follow_rate: 14.0, pilot_roll_share: 0.8, pilot_head_down: 0.06 }
    }
}

impl ViewSettings {
    pub fn fov(&self) -> f32 {
        self.fov
    }

    pub fn zoom_fov(&self) -> f32 {
        self.zoom_fov
    }

    /// Stays within bounds, and pulls the zoom FOV down if it would end up wider than this.
    pub fn set_fov(&mut self, degrees: f32) {
        self.fov = degrees.clamp(FOV_RANGE.0, FOV_RANGE.1);
        self.zoom_fov = self.zoom_fov.min(self.fov);
    }

    /// Stays within bounds. Never wider than the regular FOV, since zooming should not zoom out.
    pub fn set_zoom_fov(&mut self, degrees: f32) {
        self.zoom_fov = degrees.clamp(ZOOM_FOV_MIN, self.fov);
    }

    fn world_fov(&self, zoom: f32) -> f32 {
        self.fov + (self.zoom_fov - self.fov) * zoom
    }
}

/// How far from screen centre the true aim point sits, as yaw and pitch radians.
pub fn sight_offset(aim_offset: Vec2, recoil: Vec2, camera_recoil_share: f32) -> Vec2 {
    aim_offset - recoil * camera_recoil_share
}

fn half_tan(fov_degrees: f32) -> f32 {
    (fov_degrees.to_radians() * 0.5).tan()
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Rig::default())
            .insert_resource(prs_mods::tuned("view.camera", ViewSettings::default()))
            .insert_resource(prs_mods::tuned("view.distance", ViewDistance::default()))
            .add_systems(PreStartup, distance::reach_from_distance)
            .add_systems(PostUpdate, distance::apply_far_plane)
            .insert_resource(prs_mods::tuned("view.suppression", SuppressionLook::default()))
            .add_systems(Update, update_suppression_look)
            .add_systems(Startup, (spawn_camera, spawn_gun))
            .add_systems(
                Update,
                (toggle_third_person, read_events, update_camera, update_weapon.in_set(GunDrawn)).chain(),
            );
    }
}

/// How being shot at looks. All calm: nothing here moves the view, and it eases in and out so that a
/// near miss darkens the edges rather than flashing them.
#[derive(Resource, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuppressionLook {
    /// Most colour that goes at full suppression, from 0 to 1.
    pub desaturate: f32,
    /// How darkly the edges close in at full suppression, from 0 to 1.
    pub darkness: f32,
    /// How big the dark ring is drawn against the screen, at no suppression and at full. Smaller
    /// closes further in.
    pub ring_open: f32,
    pub ring_closed: f32,
    /// How fast it follows the level up and down, per second.
    pub rise_rate: f32,
    pub fall_rate: f32,
    /// The level as shown this frame.
    pub shown: f32,
}

impl Default for SuppressionLook {
    fn default() -> Self {
        Self { desaturate: 0.5, darkness: 0.72, ring_open: 1.7, ring_closed: 1.0, rise_rate: 5.0, fall_rate: 2.0, shown: 0.0 }
    }
}

fn update_suppression_look(
    time: Res<Time>,
    view: Res<LocalView>,
    atmosphere: Res<Atmosphere>,
    mut look: ResMut<SuppressionLook>,
    mut camera: Query<&mut ColorGrading, With<WorldCamera>>,
) {
    let level = view.blended.suppression;
    let rate = if level > look.shown { look.rise_rate } else { look.fall_rate };
    look.shown += (level - look.shown) * (1.0 - (-rate * time.delta_secs()).exp());
    let saturation = atmosphere.saturation * (1.0 - look.desaturate * look.shown);
    for mut grading in &mut camera {
        if grading.global.post_saturation != saturation {
            grading.global.post_saturation = saturation;
        }
    }
}

/// Screen-space size of the world camera, for HUD elements that show an angle.
#[derive(Component)]
pub struct WorldCamera;

#[derive(Component)]
struct WeaponRoot;

/// All visual-only motion state.
#[derive(Resource)]
struct Rig {
    /// Landing dip and jump lift. Heavy and slow to return, sells the soldier's weight.
    body_drop: Spring,
    /// Vertical pop from stairs, absorbed quickly so steps are felt rather than seen.
    step_offset: f32,
    /// Roll into strafes and shot shake.
    tilt: Spring,
    punch: Spring,
    /// Weapon position and rotation offsets.
    weapon_kick: Spring,
    weapon_twist: Spring,
    weapon_lag: Spring,
    weapon_drop: Spring,
    last_look: Vec2,
    /// How high the eye has settled to in a seat, while riding.
    seat_height: Option<f32>,
    /// Where the view from a helicopter's controls has followed the airframe to.
    pilot_rot: Option<Quat>,
    /// How far zoomed in, from 0 to 1.
    zoom: f32,
    /// Where the camera's eye is this frame, including bob.
    eye: Vec3,
    flash_time: f32,
    shot_side: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Self {
            body_drop: Spring::new(60.0, 11.0),
            step_offset: 0.0,
            tilt: Spring::new(45.0, 11.0),
            punch: Spring::new(320.0, 24.0),
            weapon_kick: Spring::new(420.0, 26.0),
            weapon_twist: Spring::new(260.0, 20.0),
            weapon_lag: Spring::new(45.0, 12.0),
            weapon_drop: Spring::new(70.0, 13.0),
            last_look: Vec2::ZERO,
            seat_height: None,
            pilot_rot: None,
            zoom: 0.0,
            eye: Vec3::ZERO,
            flash_time: 0.0,
            shot_side: 1.0,
        }
    }
}

fn spawn_camera(mut commands: Commands, atmosphere: Res<Atmosphere>, settings: Res<ViewSettings>, reach: Res<prs_atmosphere::ViewReach>) {
    commands.spawn((
        WorldCamera,
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: settings.fov().to_radians(),
            near: 0.05,
            far: reach.0,
            ..default()
        }),
        atmosphere.camera(reach.0),
        Transform::default(),
    ));
}

fn spawn_gun(mut commands: Commands) {
    commands.spawn((WeaponRoot, GunRoot::default(), Transform::default(), Visibility::default()));
}

fn toggle_third_person(keys: Res<ButtonInput<KeyCode>>, mut third: ResMut<ThirdPerson>) {
    if keys.just_pressed(KeyCode::F4) {
        third.0 = !third.0;
    }
}

fn read_events(
    mut messages: MessageReader<SimMessage>,
    local: Res<LocalSoldier>,
    mut rig: ResMut<Rig>,
    view: Res<LocalView>,
    sim: Res<SimResource>,
    settings: Res<ViewSettings>,
) {
    for SimMessage(event) in messages.read() {
        match *event {
            SimEvent::Move { soldier, event } if soldier == local.0 => match event {
                MoveEvent::Landed { speed } => {
                    let hit = (speed / 7.0).clamp(0.15, 1.3);
                    rig.body_drop.velocity.y -= settings.landing_drop * hit;
                    rig.tilt.velocity.x -= settings.landing_nod * hit;
                    rig.weapon_drop.velocity.y -= 1.3 * hit;
                }
                MoveEvent::Jumped => {
                    rig.body_drop.velocity.y += 0.25;
                    rig.weapon_drop.velocity.y -= 0.6;
                }
                MoveEvent::Stepped { height } => rig.step_offset -= height,
                MoveEvent::StanceChanged { stance } => {
                    let lower = matches!(stance, Stance::Prone | Stance::Crouch);
                    rig.weapon_drop.velocity.y -= if lower { 0.5 } else { 0.8 };
                    rig.tilt.velocity.z += if lower { 0.06 } else { -0.06 };
                }
                _ => {}
            },
            SimEvent::Weapon { soldier, event: WeaponEvent::Fired { .. } } if soldier == local.0 => {
                let ads = view.blended.ads;
                let steady = 1.0 - settings.shot_steady_at_sights * ads;
                rig.shot_side = -rig.shot_side;
                let side = rig.shot_side;
                // A touch of screen shake sells the shot. The rifle below does the heavy lifting.
                rig.punch.velocity += settings.shot_punch * Vec3::new(1.0, side, side) * steady;
                rig.tilt.velocity.z += settings.shot_tilt * side * steady;
                rig.weapon_kick.velocity.z += settings.shot_kick_back * (1.0 - 0.3 * ads);
                rig.weapon_kick.velocity.y += settings.shot_kick_up * (1.0 - 0.75 * ads);
                rig.weapon_twist.velocity += settings.shot_twist * Vec3::new(1.0, side, side) * (1.0 - 0.8 * ads);
                rig.flash_time = sim.0.weapons[view.blended.weapon].flash.time;
            }
            // Coming up on the shoulder, and the sound does the rest: no camera shake for wounds.
            SimEvent::Respawned { soldier } if soldier == local.0 => {
                *rig = Rig::default();
            }
            SimEvent::Weapon { soldier, event: WeaponEvent::ReloadFinished } if soldier == local.0 => {
                rig.weapon_twist.velocity.x -= 4.0;
                rig.weapon_drop.velocity.y += 0.4;
            }
            _ => {}
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bob driven by distance walked, so footsteps and camera always agree. Returns (side, up).
fn bob(stride: f32) -> Vec2 {
    let phase = stride * std::f32::consts::PI;
    Vec2::new(phase.cos(), -(phase * 2.0).cos())
}

/// The view from the controls of a helicopter: the airframe's attitude a moment behind and with less of its
/// roll, and the head turned on top of that. Nothing is added to it, so the machine is all there is to see.
fn pilot_view(airframe: Quat, head: Vec2, followed: &mut Option<Quat>, settings: &ViewSettings, dt: f32) -> Quat {
    let (yaw, pitch, roll) = airframe.to_euler(EulerRot::YXZ);
    let target = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll * settings.pilot_roll_share);
    let share = 1.0 - (-settings.pilot_follow_rate * dt).exp();
    let now = followed.map_or(target, |before| before.slerp(target, share));
    *followed = Some(now);
    now * Quat::from_euler(EulerRot::YXZ, head.x, head.y - settings.pilot_head_down, 0.0)
}

/// The eye in a seat, with its height following the seat's a moment behind and never far from it.
fn settled_eye(eye: Vec3, height: &mut Option<f32>, settings: &ViewSettings, dt: f32) -> Vec3 {
    let limit = settings.seat_settle_limit;
    let settled = height.unwrap_or(eye.y);
    let next = settled + (eye.y - settled) * (1.0 - (-settings.seat_settle_rate * dt).exp());
    let next = next.clamp(eye.y - limit, eye.y + limit);
    *height = Some(next);
    Vec3::new(eye.x, next, eye.z)
}

fn update_camera(
    time: Res<Time>,
    view: Res<LocalView>,
    input: Res<PendingInput>,
    settings: Res<ViewSettings>,
    mut look_scale: ResMut<LookScale>,
    mut rig: ResMut<Rig>,
    third: Res<ThirdPerson>,
    frames: Res<VehicleFrames>,
    helis: Res<HeliFrames>,
    mut camera: Query<(&mut Transform, &mut Projection), With<WorldCamera>>,
) {
    let dt = time.delta_secs().min(0.05);
    let s = &view.blended;
    let ads = smooth(s.ads);

    rig.step_offset *= (-22.0 * dt).exp();
    rig.body_drop.update(Vec3::ZERO, dt);
    rig.punch.update(Vec3::ZERO, dt);

    let speed = Vec2::new(s.velocity.x, s.velocity.z).length();
    let basis = Quat::from_rotation_y(input.yaw);
    let local_vel = basis.inverse() * s.velocity;
    let riding = s.seat.is_some();
    let lean = if riding { 0.0 } else { -local_vel.x * 0.001 * (1.0 - 0.7 * ads) };
    rig.tilt.update(Vec3::new(0.0, 0.0, lean), dt);

    let bob_amount = if riding { 0.0 } else { (speed / 6.0).min(1.0) * if s.grounded { 1.0 } else { 0.0 } * (1.0 - 0.8 * ads) };
    let b = bob(s.stride) * bob_amount;

    let eye = s.feet
        + Vec3::Y * (s.eye_height + rig.body_drop.value.y + rig.step_offset + b.y * 0.007)
        + basis * Vec3::X * (b.x * 0.003 + s.lean_eye.x)
        + Vec3::Y * s.lean_eye.y
        + s.rest_eye;

    let camera_aim = s.recoil * settings.camera_recoil_share;
    let pitch = input.pitch + input.free_look.y + camera_aim.y + rig.punch.value.x.to_radians() + rig.tilt.value.x.to_radians() * 2.0;
    let yaw = input.yaw + input.free_look.x + camera_aim.x + rig.punch.value.y.to_radians();
    let carrier_roll = s.seat.and_then(|at| frames.blended.get(at.carrier as usize)).map_or(0.0, |frame| (frame.rot * Vec3::X).y.clamp(-1.0, 1.0).asin());
    let roll = rig.tilt.value.z + rig.punch.value.z.to_radians() - s.lean * settings.lean_roll + carrier_roll * settings.seat_roll_share;
    let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    let pilot_frame = s.seat.filter(|_| input.at_controls).and_then(|at| prs_runtime::prs_sim::heli_of(at.carrier)).and_then(|h| helis.blended.get(h));
    let rotation = match pilot_frame {
        Some(frame) => pilot_view(frame.rot, input.free_look, &mut rig.pilot_rot, &settings, dt),
        None => {
            rig.pilot_rot = None;
            rotation
        }
    };

    let zoom_target = if input.zoom { 1.0 } else { 0.0 };
    rig.zoom += (zoom_target - rig.zoom) * (1.0 - (-settings.zoom_rate * dt).exp());
    let world_fov = settings.world_fov(smooth(rig.zoom));
    // Keeps tracking speed on screen the same however far you are zoomed in.
    look_scale.0 = half_tan(world_fov) / half_tan(settings.fov());
    let eye = if riding && pilot_frame.is_none() { settled_eye(eye, &mut rig.seat_height, &settings, dt) } else { eye };
    if !riding {
        rig.seat_height = None;
    }
    rig.eye = eye;
    for (mut transform, mut projection) in &mut camera {
        transform.rotation = rotation;
        transform.translation = if third.0 { eye + rotation * settings.third_person_offset } else { eye };
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = world_fov.to_radians();
        }
    }
}

/// Puts the rifle where the sim says it is, using this frame's look so it does not lag your hand.
/// The springs add a little life for the person holding it, and fade out at the shoulder.
fn update_weapon(
    time: Res<Time>,
    view: Res<LocalView>,
    input: Res<PendingInput>,
    sim: Res<SimResource>,
    mut rig: ResMut<Rig>,
    mut drawn: ResMut<DrawnGun>,
    third: Res<ThirdPerson>,
    mut root: Query<(&mut Transform, &mut GunRoot), With<WeaponRoot>>,
) {
    let dt = time.delta_secs().clamp(0.0001, 0.05);
    let Ok((mut transform, mut gun)) = root.single_mut() else {
        return;
    };
    let s = &view.blended;
    // A dead soldier's gun is in their kit on the ground, not in their hands.
    gun.weapon = (s.life != Life::Dead && !s.gun_put_away).then_some(s.weapon);
    gun.stock_hidden = !third.0 && s.ads > STOCK_HIDDEN_FROM_ADS;
    // Someone who is down is no longer holding it up: it stays where it fell and the view looks away.
    let up = s.life == Life::Up;
    let (gun_yaw, gun_pitch) = if up { (input.yaw, input.pitch) } else { (s.gun_yaw, s.gun_pitch) };
    let ads = smooth(s.ads);
    let reload = (s.reload_fraction * std::f32::consts::PI).sin().powf(0.4);

    rig.weapon_kick.update(Vec3::ZERO, dt);
    rig.weapon_twist.update(Vec3::ZERO, dt);
    rig.weapon_drop.update(Vec3::ZERO, dt);

    // The gun trails behind fast mouse movement.
    let look = Vec2::new(input.yaw, input.pitch);
    let look_rate = ((look - rig.last_look) / dt).clamp(Vec2::splat(-4.0), Vec2::splat(4.0));
    rig.last_look = look;
    let lag_scale = 1.0 - 0.8 * ads;
    rig.weapon_lag
        .update(Vec3::new(look_rate.x, look_rate.y, 0.0) * 0.004 * lag_scale, dt);

    let speed = Vec2::new(s.velocity.x, s.velocity.z).length();
    let b = bob(s.stride) * (speed / 6.0).min(1.0) * if s.grounded && s.seat.is_none() { 1.0 } else { 0.0 } * (1.0 - 0.9 * ads);

    let mut offset = Vec3::new(rig.weapon_lag.value.x, rig.weapon_lag.value.y, 0.0);
    offset += Vec3::new(b.x * 0.011, b.y * 0.007, 0.0);
    offset.y += rig.weapon_drop.value.y * 0.12;
    offset += rig.weapon_kick.value * 0.018;
    // Brought up and in with the magazine turned towards you, to see the off hand working on it.
    offset += Vec3::new(-0.09, 0.055, 0.03) * reload;
    // Changing guns or using an item takes it out of the way, further than a run lowers it, so a new
    // one comes up from below rather than appearing in the air.
    let put_away = if s.changing { smooth(1.0 - s.ready) } else { 0.0 };
    offset += Vec3::new(0.0, -0.2, 0.05) * put_away;

    let twist = rig.weapon_twist.value;
    let turn = Quat::from_euler(
        EulerRot::YXZ,
        twist.y.to_radians() - rig.weapon_lag.value.x,
        twist.x.to_radians() + rig.weapon_lag.value.y + 0.32 * reload - 0.5 * put_away,
        twist.z.to_radians() + rig.weapon_lag.value.x * 0.6 - 0.6 * reload,
    );

    let pose = gun_pose(
        &sim.0.weapons[s.weapon],
        &GunPoseInput {
            eye: rig.eye,
            yaw: gun_yaw,
            pitch: gun_pitch,
            lean: s.lean,
            aim_offset: s.aim_offset,
            ads: s.ads,
            ready: s.ready,
            retract: s.gun_retract,
            fallen: s.collapse,
            slung: s.slung,
            on_back: s.on_back,
        },
    );
    transform.translation = pose.origin + pose.rotation * offset;
    transform.rotation = pose.rotation * turn;
    *drawn = DrawnGun { eye: rig.eye, origin: transform.translation, rotation: transform.rotation };

    rig.flash_time -= dt;
    if rig.flash_time > 0.0 {
        gun.flash = rig.flash_time;
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_mod_can_change_the_camera_and_the_look_of_being_shot_at() {
        let text = "view.camera.camera_recoil_share = 0.0\nview.suppression.darkness = 0.1";
        let active = prs_mods::Mod::parse("test", text).unwrap();
        assert_eq!(active.over("view.camera", ViewSettings::default()).unwrap().camera_recoil_share, 0.0);
        assert_eq!(active.over("view.suppression", SuppressionLook::default()).unwrap().darkness, 0.1);
        let arcade = prs_mods::Mod::parse("arcade", include_str!("../../../mods/arcade.toml")).unwrap();
        assert_eq!(arcade.over("view.camera", ViewSettings::default()).unwrap().camera_recoil_share, 0.0);
    }

    use super::*;

    #[test]
    fn a_seat_takes_the_shake_out_of_the_view_and_stays_with_the_truck() {
        let s = ViewSettings::default();
        let mut height = None;
        let dt = 1.0 / 60.0;
        // A washboard road at 8 Hz, two centimetres up and down, moves the view far less.
        let mut most = 0.0_f32;
        for frame in 0..600 {
            let t = frame as f32 * dt;
            let road = (t * 8.0 * std::f32::consts::TAU).sin() * 0.02;
            let eye = settled_eye(Vec3::Y * (1.5 + road), &mut height, &s, dt);
            if frame > 120 {
                most = most.max((eye.y - 1.5).abs());
            }
        }
        assert!(most < 0.008, "{most}");
        // Going up a long slope it never falls more than a little behind.
        height = None;
        for frame in 0..600 {
            let climb = frame as f32 * dt * 8.0;
            let eye = settled_eye(Vec3::Y * climb, &mut height, &s, dt);
            assert!((eye.y - climb).abs() <= s.seat_settle_limit + 1e-3);
        }
    }

    #[test]
    fn fov_settings_stay_within_bounds() {
        let mut s = ViewSettings::default();
        s.set_fov(500.0);
        assert_eq!(s.fov(), FOV_RANGE.1);
        s.set_fov(1.0);
        assert_eq!(s.fov(), FOV_RANGE.0);
        s.set_zoom_fov(1.0);
        assert_eq!(s.zoom_fov(), ZOOM_FOV_MIN);
    }

    #[test]
    fn zoom_fov_can_never_be_wider_than_regular_fov() {
        let mut s = ViewSettings::default();
        s.set_zoom_fov(84.0);
        assert_eq!(s.zoom_fov(), s.fov());
        s.set_fov(70.0);
        s.set_zoom_fov(65.0);
        s.set_fov(55.0);
        assert_eq!(s.zoom_fov(), 55.0);
    }

    #[test]
    fn only_zooming_narrows_the_view() {
        let s = ViewSettings::default();
        assert_eq!(s.world_fov(0.0), s.fov());
        assert_eq!(s.world_fov(1.0), s.zoom_fov());
    }
}
