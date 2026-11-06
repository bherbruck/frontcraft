//! What has been thrown and the smoke it leaves. A cloud is a few layers of the same low-poly ball: an opaque
//! heart, which is what really hides things, a thin shell turning slowly over it, and wisps low down so that it has
//! no hard edge on the ground. It is the same mesh throughout and no shadows, so on WebGL2 it stays a handful of
//! draws. They draw both sides, so someone inside one sees nothing but smoke. Under it the grenade itself pours out
//! a stream of the same puffs that feeds it, from where it is falling or where it has come to rest.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use prs_runtime::prs_sim::{SimEvent, Soldier};
use prs_runtime::prs_sim::body::Bone;
use prs_runtime::prs_sim::inventory::Slot;
use prs_runtime::prs_sim::throwables::{Cloud, ThrownKind, fragment_dir};
use prs_runtime::{DrawnGun, LocalSoldier, PendingInput, SimMessage, SimResource, ThirdPerson};

/// Puffs to a layer of a cloud: as few as still looks like a cloud rather than a ball.
const HEART: u32 = 4;
const SHELL: u32 = 9;
const WISPS: u32 = 7;
const HAZE: u32 = 6;
/// How fast a shell puff goes round, and how much it breathes in and out.
const DRIFT: f32 = 0.05;
const BREATHE: f32 = 0.06;

/// In your own hand, in first person: where it sits ready and where winding up takes it, from the eye.
const HAND_READY: Vec3 = Vec3::new(0.2, -0.26, -0.6);
const HAND_BACK: Vec3 = Vec3::new(0.24, -0.18, -0.52);

/// How the grenade grows into a cloud. None of it is in the rules, and all of it comes from where the grenade is.
#[derive(Resource, Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SmokeJet {
    /// Puffs in the stream at once, for each grenade, and how many seconds each lasts.
    pub puffs: u32,
    pub life: f32,
    /// How far up one goes in its life, in metres, and how far it wanders off to the side.
    pub rise: f32,
    pub spread: f32,
    /// How big one is as it leaves the grenade and at its biggest, in metres.
    pub start_size: f32,
    pub end_size: f32,
    /// The share of its life it is thick for, and thin after that, and the share it spends going away at the end.
    pub thick_until: f32,
    pub thin_from: f32,
    pub vanish_for: f32,
    /// How much lower than a flying grenade's middle a resting one lies, in metres, to be on the ground.
    pub rest_sink: f32,
}

impl Default for SmokeJet {
    fn default() -> Self {
        Self { puffs: 18, life: 1.5, rise: 2.8, spread: 0.45, start_size: 0.06, end_size: 0.6, thick_until: 0.35, thin_from: 0.7, vanish_for: 0.2, rest_sink: 0.015 }
    }
}

impl SmokeJet {
    /// Where a puff is and how big, `age` seconds after leaving `from`. Quick out of the can and slowing as it spreads.
    fn puff_at(&self, from: Vec3, age: f32, key: u32) -> (Vec3, f32) {
        let t = (age / self.life.max(1e-3)).clamp(0.0, 1.0);
        let angle = key.wrapping_mul(2654435761) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
        let out = 1.0 - (1.0 - t) * (1.0 - t);
        let at = from + Vec3::Y * self.rise * out + Vec3::new(angle.cos(), 0.0, angle.sin()) * self.spread * t * t;
        let vanish = ((1.0 - t) / self.vanish_for.max(1e-3)).min(1.0);
        (at, (self.start_size + (self.end_size - self.start_size) * out) * vanish)
    }
}

/// One puff of the stream. Each leaves the grenade from wherever it is by then and stays where the air has it, so
/// the ones from a falling grenade are left behind it as a trail. When one has gone it comes round again.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetPuff {
    cloud: u32,
    index: u32,
    /// Below zero, it has not left yet.
    age: f32,
    from: Vec3,
    /// How hard the grenade was pouring as it left.
    pouring: f32,
}

impl JetPuff {
    fn new(cloud: u32, index: u32, jet: &SmokeJet) -> Self {
        Self { cloud, index, age: -(index as f32) * jet.life / jet.puffs.max(1) as f32, from: Vec3::ZERO, pouring: 0.0 }
    }

    fn step(&mut self, jet: &SmokeJet, source: Vec3, pouring: f32, dt: f32) {
        let life = jet.life.max(1e-3);
        let was = self.age;
        self.age += dt;
        if self.age >= life {
            self.age = (self.age - life).min(life);
        } else if was >= 0.0 || self.age < 0.0 {
            return;
        }
        self.from = source;
        self.pouring = pouring;
    }

    /// Where it is and how big, if it is there at all.
    fn shown(&self, jet: &SmokeJet) -> Option<(Vec3, f32)> {
        if self.age < 0.0 || self.pouring <= 0.0 {
            return None;
        }
        let (at, size) = jet.puff_at(self.from, self.age, self.cloud ^ self.index.wrapping_mul(31));
        Some((at, size * self.pouring))
    }

    /// 0 is thick, 1 middling and 2 thin.
    fn thickness(&self, jet: &SmokeJet) -> u8 {
        let t = self.age / jet.life.max(1e-3);
        if t < jet.thick_until {
            0
        } else if t < jet.thin_from {
            1
        } else {
            2
        }
    }
}

#[derive(Component)]
pub(crate) struct ThrownVisual(u32);

#[derive(Component, Clone, Copy)]
pub(crate) struct Puff {
    cloud: u32,
    offset: Vec3,
    size: f32,
    /// Radians a second round the middle, either way.
    drift: f32,
    phase: f32,
}

/// One in someone's hand, about to go.
#[derive(Component)]
pub(crate) struct InHand {
    soldier: usize,
    kind: ThrownKind,
}

/// A frag going off: a dirty ball, gone in a moment.
#[derive(Component)]
pub(crate) struct BurstPuff {
    age: f32,
}

#[derive(Default)]
pub(crate) struct Looks {
    ready: Option<LookHandles>,
}

#[derive(Clone)]
struct LookHandles {
    grenade: Handle<Mesh>,
    ball: Handle<Mesh>,
    frag: Handle<StandardMaterial>,
    smoke_can: Handle<StandardMaterial>,
    heart: Handle<StandardMaterial>,
    shell: Handle<StandardMaterial>,
    wisp: Handle<StandardMaterial>,
    haze: Handle<StandardMaterial>,
    burst: Handle<StandardMaterial>,
}

fn looks(state: &mut Looks, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> LookHandles {
    state
        .ready
        .get_or_insert_with(|| {
            let flat = |color: Color| StandardMaterial { base_color: color, perceptual_roughness: 1.0, ..default() };
            LookHandles {
                grenade: meshes.add(Capsule3d::new(0.035, 0.05).mesh().rings(1).latitudes(4).longitudes(6)),
                ball: {
                    // Faceted like everything else, so that turning slowly is something you can see.
                    let mut ball = Sphere::new(1.0).mesh().ico(1).expect("a small icosphere");
                    ball.duplicate_vertices();
                    ball.compute_flat_normals();
                    meshes.add(ball)
                },
                frag: materials.add(flat(Color::srgb(0.2, 0.26, 0.15))),
                smoke_can: materials.add(flat(Color::srgb(0.5, 0.52, 0.5))),
                // Lit mostly from within, because a cloud has no hard shadow side.
                heart: materials.add(smoke_look(1.0)),
                shell: materials.add(smoke_look(0.55)),
                wisp: materials.add(smoke_look(0.28)),
                haze: materials.add(smoke_look(0.16)),
                burst: materials.add(StandardMaterial { cull_mode: None, ..flat(Color::srgb(0.2, 0.17, 0.14)) }),
            }
        })
        .clone()
}

fn smoke_look(alpha: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(0.4, 0.4, 0.4, alpha),
        emissive: LinearRgba::rgb(0.42, 0.42, 0.41),
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        cull_mode: None,
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    }
}

/// Spread out evenly, each its own size, the same for everyone who sees this cloud.
fn puffs_of(cloud: u32) -> Vec<(Puff, u8)> {
    let mix = |v: u32| {
        let mut x = v.wrapping_mul(2654435761).wrapping_add(0x9E37_79B1);
        x ^= x >> 15;
        x = x.wrapping_mul(2246822519);
        x ^= x >> 13;
        (x >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut puffs = Vec::new();
    let mut add = |layer: u8, count: u32, out: f32, size: f32, squash: f32, fall: f32| {
        for index in 0..count {
            let key = cloud ^ (index << 4) ^ ((layer as u32) << 20);
            let dir = fragment_dir(key, index, count);
            let offset = Vec3::new(dir.x, dir.y * squash + fall, dir.z) * (out * (0.85 + 0.3 * mix(key)));
            let drift = DRIFT * if mix(key ^ 7) < 0.5 { -1.0 } else { 1.0 } * (0.6 + mix(key ^ 11));
            let puff = Puff { cloud, offset, size: size * (0.8 + 0.4 * mix(key ^ 3)), drift, phase: mix(key ^ 5) * std::f32::consts::TAU };
            puffs.push((puff, layer));
        }
    };
    add(0, HEART, 0.26, 0.42, 0.55, 0.0);
    add(1, SHELL, 0.6, 0.42, 0.6, 0.05);
    // Thin and large, for an edge you cannot quite point to.
    add(3, HAZE, 0.72, 0.5, 0.6, 0.05);
    // Low and wide, right down on the ground.
    add(2, WISPS, 0.95, 0.26, 0.08, -0.33);
    puffs
}

pub(crate) fn update_throwables(
    mut commands: Commands,
    sim: Res<SimResource>,
    time: Res<Time<Fixed>>,
    jet: Res<SmokeJet>,
    mut state: Local<Looks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut thrown: Query<(Entity, &ThrownVisual, &mut Transform), Without<Puff>>,
    mut puffs: Query<(Entity, &Puff, &mut Transform), Without<ThrownVisual>>,
) {
    let sim = &sim.0;
    let look = looks(&mut state, &mut meshes, &mut materials);
    // Between ticks it goes on as it was going, so it does not judder through the air.
    let ahead = time.overstep_fraction() * time.timestep().as_secs_f32();
    for (entity, visual, mut transform) in &mut thrown {
        if let Some(t) = sim.thrown.iter().find(|t| t.id == visual.0) {
            transform.translation = t.pos + t.vel * ahead;
        } else if let Some(cloud) = sim.clouds.iter().find(|c| c.id == visual.0) {
            // Lying where it came to rest, on its side, until the smoke runs out.
            transform.translation = cloud.grenade_at(&sim.throw_cfg.smoke) - Vec3::Y * jet.rest_sink;
            transform.rotation = Quat::from_rotation_y(visual.0 as f32) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        } else {
            commands.entity(entity).despawn();
        }
    }
    // Clouds as well as what is flying: someone who has only just arrived still sees the can.
    for cloud in &sim.clouds {
        if !sim.thrown.iter().any(|t| t.id == cloud.id) && !thrown.iter().any(|(_, visual, _)| visual.0 == cloud.id) {
            commands.spawn((ThrownVisual(cloud.id), Mesh3d(look.grenade.clone()), MeshMaterial3d(look.smoke_can.clone()), Transform::from_translation(cloud.grenade_at(&sim.throw_cfg.smoke))));
        }
    }
    for t in &sim.thrown {
        if !thrown.iter().any(|(_, visual, _)| visual.0 == t.id) {
            let material = if t.kind == ThrownKind::Frag { look.frag.clone() } else { look.smoke_can.clone() };
            commands.spawn((ThrownVisual(t.id), Mesh3d(look.grenade.clone()), MeshMaterial3d(material), Transform::from_translation(t.pos)));
        }
    }

    let cfg = &sim.throw_cfg.smoke;
    let place = |cloud: &Cloud, puff: &Puff, transform: &mut Transform| {
        let radius = cloud.radius(cfg);
        let breath = 1.0 + BREATHE * (cloud.age * 0.5 + puff.phase).sin();
        let round = Quat::from_rotation_y(cloud.age * puff.drift);
        transform.translation = cloud.pos + round * puff.offset * radius * breath;
        transform.scale = Vec3::splat(radius * puff.size * breath);
        transform.rotation = Quat::from_axis_angle(Vec3::new(0.3, 1.0, -0.2).normalize(), cloud.age * puff.drift * 3.0 + puff.phase);
    };
    for (entity, puff, mut transform) in &mut puffs {
        match sim.clouds.iter().find(|c| c.id == puff.cloud) {
            Some(cloud) => place(cloud, puff, &mut transform),
            None => commands.entity(entity).despawn(),
        }
    }
    for cloud in &sim.clouds {
        if puffs.iter().any(|(_, puff, _)| puff.cloud == cloud.id) {
            continue;
        }
        for (puff, layer) in puffs_of(cloud.id) {
            let mut transform = Transform::default();
            place(cloud, &puff, &mut transform);
            let material = match layer {
                0 => look.heart.clone(),
                1 => look.shell.clone(),
                2 => look.wisp.clone(),
                _ => look.haze.clone(),
            };
            commands.spawn((puff, Mesh3d(look.ball.clone()), MeshMaterial3d(material), transform, NotShadowCaster));
        }
    }
}

/// The stream of puffs out of each smoke grenade.
pub(crate) fn update_smoke_jets(
    mut commands: Commands,
    sim: Res<SimResource>,
    time: Res<Time>,
    jet: Res<SmokeJet>,
    mut state: Local<Looks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(Entity, &mut JetPuff, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let sim = &sim.0;
    let look = looks(&mut state, &mut meshes, &mut materials);
    let cfg = &sim.throw_cfg.smoke;
    for (entity, mut puff, mut transform, mut visibility, mut material) in &mut puffs {
        let Some(cloud) = sim.clouds.iter().find(|c| c.id == puff.cloud) else {
            commands.entity(entity).despawn();
            continue;
        };
        puff.step(&jet, sim.smoke_source(cloud), cloud.pouring(cfg), time.delta_secs());
        let Some((at, size)) = puff.shown(&jet) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        transform.translation = at;
        transform.scale = Vec3::splat(size);
        let wanted = match puff.thickness(&jet) {
            0 => &look.shell,
            1 => &look.wisp,
            _ => &look.haze,
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
    for cloud in &sim.clouds {
        if puffs.iter().any(|(_, puff, ..)| puff.cloud == cloud.id) {
            continue;
        }
        for index in 0..jet.puffs {
            commands.spawn((JetPuff::new(cloud.id, index, &jet), Mesh3d(look.ball.clone()), MeshMaterial3d(look.shell.clone()), Transform::default(), Visibility::Hidden, NotShadowCaster));
        }
    }
}

/// Where your own grenade sits in first person, if you have one out. Your hand closes on the same spot.
pub(crate) fn first_person_grenade(soldier: &Soldier, windup: f32, eye: Vec3, yaw: f32, pitch: f32) -> Option<Vec3> {
    let out = matches!(soldier.inventory.selected, Slot::Frag | Slot::Smoke) && soldier.weapon.ready <= 0.0;
    (out || soldier.throw.held > 0.0).then(|| {
        let wound = (soldier.throw.held / windup.max(1e-3)).min(1.0);
        eye + Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * HAND_READY.lerp(HAND_BACK, wound)
    })
}

/// Anyone winding up has it in their hand, and you see yours in front of you for as long as it is out.
pub(crate) fn update_in_hand(
    mut commands: Commands,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    drawn: Res<DrawnGun>,
    input: Res<PendingInput>,
    third: Res<ThirdPerson>,
    mut state: Local<Looks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut held: Query<(Entity, &InHand, &mut Transform)>,
) {
    let sim = &sim.0;
    let look = looks(&mut state, &mut meshes, &mut materials);
    let holding = |id: usize| -> Option<ThrownKind> {
        let soldier = &sim.soldiers[id];
        if !soldier.present || !soldier.health.is_up() {
            return None;
        }
        let kind = if soldier.inventory.selected == Slot::Smoke { ThrownKind::Smoke } else { ThrownKind::Frag };
        let out = matches!(soldier.inventory.selected, Slot::Frag | Slot::Smoke) && soldier.weapon.ready <= 0.0;
        (soldier.throw.held > 0.0 || (id == local.0 && out)).then_some(kind)
    };
    let place = |id: usize| -> Vec3 {
        let soldier = &sim.soldiers[id];
        if id == local.0 && !third.0 {
            let spot = first_person_grenade(soldier, sim.throw_cfg.windup, drawn.eye, input.yaw, input.pitch);
            return spot.unwrap_or(drawn.eye);
        }
        sim.body_pose(&sim.body_input(id))[Bone::HandR].end()
    };
    for (entity, hand, mut transform) in &mut held {
        if hand.soldier < sim.soldiers.len() && holding(hand.soldier) == Some(hand.kind) {
            transform.translation = place(hand.soldier);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for id in 0..sim.soldiers.len() {
        let Some(kind) = holding(id) else { continue };
        if held.iter().any(|(_, hand, _)| hand.soldier == id && hand.kind == kind) {
            continue;
        }
        let material = if kind == ThrownKind::Frag { look.frag.clone() } else { look.smoke_can.clone() };
        commands.spawn((InHand { soldier: id, kind }, Mesh3d(look.grenade.clone()), MeshMaterial3d(material), Transform::from_translation(place(id))));
    }
}

pub(crate) fn update_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut messages: MessageReader<SimMessage>,
    mut state: Local<Looks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: Query<(Entity, &mut BurstPuff, &mut Transform)>,
) {
    const LIFE: f32 = 0.45;
    const SIZE: f32 = 3.0;
    let look = looks(&mut state, &mut meshes, &mut materials);
    for SimMessage(event) in messages.read() {
        if let SimEvent::ThrownBurst { kind: ThrownKind::Frag, point, .. } = *event {
            commands.spawn((BurstPuff { age: 0.0 }, Mesh3d(look.ball.clone()), MeshMaterial3d(look.burst.clone()), Transform::from_translation(point).with_scale(Vec3::splat(0.1)), NotShadowCaster));
        }
    }
    for (entity, mut burst, mut transform) in &mut bursts {
        burst.age += time.delta_secs();
        if burst.age >= LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let t = burst.age / LIFE;
        // Out fast and gone small: nothing needs blending for that to read as a burst.
        transform.scale = Vec3::splat(SIZE * (1.0 - (1.0 - t) * (1.0 - t)) * (1.0 - t * t * t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn stream(jet: &SmokeJet) -> Vec<JetPuff> {
        (0..jet.puffs).map(|index| JetPuff::new(7, index, jet)).collect()
    }

    #[test]
    fn every_puff_starts_small_at_the_can_itself() {
        let jet = SmokeJet::default();
        let can = Vec3::new(3.0, 1.0, -2.0);
        let mut puffs = stream(&jet);
        let mut left = vec![false; puffs.len()];
        for _ in 0..(jet.life / DT) as usize + 2 {
            for (puff, left) in puffs.iter_mut().zip(&mut left) {
                let was = puff.age;
                puff.step(&jet, can, 1.0, DT);
                if was < 0.0 && puff.age >= 0.0 {
                    *left = true;
                    let (at, size) = puff.shown(&jet).unwrap();
                    assert!(at.distance(can) < 0.1, "{at} is not at the can");
                    assert!(size < jet.start_size * 2.0, "{size}");
                }
            }
        }
        assert!(left.iter().skip(1).all(|&l| l), "the whole stream got going within one life");
    }

    #[test]
    fn it_is_a_stream_that_rises_and_spreads_and_not_a_ball_around_the_can() {
        let jet = SmokeJet::default();
        let mut puffs = stream(&jet);
        for _ in 0..(jet.life * 3.0 / DT) as usize {
            puffs.iter_mut().for_each(|p| p.step(&jet, Vec3::ZERO, 1.0, DT));
        }
        let shown: Vec<(Vec3, f32)> = puffs.iter().filter_map(|p| p.shown(&jet)).collect();
        assert_eq!(shown.len(), puffs.len());
        let top = shown.iter().fold(0.0_f32, |top, (at, _)| top.max(at.y));
        assert!(top > jet.rise * 0.8, "{top}");
        // The lowest are thin and the highest are wide.
        let low = shown.iter().filter(|(at, _)| at.y < jet.rise * 0.3).fold(0.0_f32, |big, (_, size)| big.max(*size));
        let high = shown.iter().filter(|(at, _)| at.y > jet.rise * 0.5).fold(0.0_f32, |big, (_, size)| big.max(*size));
        assert!(low > 0.0 && high > low * 1.3, "{low} {high}");
    }

    #[test]
    fn what_a_falling_can_pours_stays_behind_it_as_a_trail() {
        let jet = SmokeJet { rise: 0.0, spread: 0.0, ..SmokeJet::default() };
        let mut puffs = stream(&jet);
        let mut can = Vec3::new(0.0, 100.0, 0.0);
        for _ in 0..(jet.life * 2.0 / DT) as usize {
            can.y -= 10.0 * DT;
            puffs.iter_mut().for_each(|p| p.step(&jet, can, 1.0, DT));
        }
        let heights: Vec<f32> = puffs.iter().filter_map(|p| p.shown(&jet)).map(|(at, _)| at.y).collect();
        let (lowest, highest) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
        assert!(lowest < can.y + 2.0, "new ones are by the can at {}: {lowest}", can.y);
        assert!(highest > can.y + 10.0, "old ones are far behind it at {}: {highest}", can.y);
        // Older is thinner, so it thins out the further it gets from the can.
        let oldest = puffs.iter().max_by(|a, b| a.age.total_cmp(&b.age)).unwrap();
        assert_eq!(oldest.thickness(&jet), 2);
    }

    #[test]
    fn as_the_smoke_runs_out_the_stream_dwindles_away_to_nothing() {
        let jet = SmokeJet::default();
        let mut puffs = stream(&jet);
        let mut biggest = |pouring: f32| {
            for _ in 0..(jet.life * 2.0 / DT) as usize {
                puffs.iter_mut().for_each(|p| p.step(&jet, Vec3::ZERO, pouring, DT));
            }
            puffs.iter().filter_map(|p| p.shown(&jet)).fold(0.0_f32, |big, (_, size)| big.max(size))
        };
        let full = biggest(1.0);
        let half = biggest(0.5);
        let none = biggest(0.0);
        assert!(half < full * 0.6, "{half} {full}");
        assert_eq!(none, 0.0);
    }

    #[test]
    fn a_mod_can_turn_the_stream_right_down_without_anything_breaking() {
        let jet = SmokeJet { puffs: 0, life: 0.0, vanish_for: 0.0, ..SmokeJet::default() };
        let mut puff = JetPuff::new(1, 0, &jet);
        puff.step(&jet, Vec3::ZERO, 1.0, DT);
        assert!(puff.shown(&jet).is_none_or(|(at, size)| at.is_finite() && size.is_finite()));
    }
}
