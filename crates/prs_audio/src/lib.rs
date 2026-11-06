//! Plays the sounds of the simulation. Sound carries most of a gun's weight, so every shot is
//! rendered for the moment it is heard: which way the barrel pointed, how far away it was, and what
//! stands between it and you. Other people's sounds also arrive late and from the side they came from.

mod engine;
mod hearing;
mod reload;
mod rotor;
mod smoke;
mod synth;

use bevy::audio::Volume;
use bevy::prelude::*;
use prs_runtime::prs_sim::movement::Stance;
use prs_runtime::prs_sim::throwables::ThrownKind;
use prs_runtime::prs_sim::health::HealthEvent;
use prs_runtime::prs_sim::helicopter::HeliEvent;
use prs_runtime::prs_sim::movement::MoveEvent;
use prs_runtime::prs_sim::weapons::{WeaponEvent, WeaponId};
use prs_runtime::prs_sim::vehicles::{VehicleEvent, VehicleZone};
use prs_runtime::prs_sim::{Care, CarryEvent, SeatEvent, SimEvent, Surface};
use prs_synth::{ContactSound, Moved};
use prs_runtime::{BleedShown, LocalSoldier, LocalView, PendingInput, SimMessage, SimResource};
use std::sync::Arc;

/// Distance is handled in `hearing`, so emitters sit on a small sphere around motionless ears and
/// only say which direction a sound came from. Inside a metre, rodio applies no distance fade.
const EMITTER_RADIUS: f32 = 0.5;
const EAR_GAP: f32 = 0.3;

pub use hearing::{Acoustics, Carry};
pub use prs_synth::SoundSet;
pub use rotor::RotorLoop;
pub use smoke::SmokeVoice;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        let acoustics = prs_mods::tuned("audio", Acoustics::default());
        let levels = AudioLevels { unless_chosen: acoustics.sound_set, ..default() };
        app.insert_resource(acoustics)
            .insert_resource(levels)
            .add_systems(Update, apply_levels)
            .add_systems(Startup, (build_bank, spawn_ears, engine::build_engine_loops, rotor::build_rotor_bank))
            .add_systems(Update, (engine::run_engines, heartbeat, gun_on_wall))
            .add_systems(Update, smoke::update_smoke_hiss)
            .add_systems(Update, (turn_ears, play_events, reload::reload_sounds, play_delayed, rotor::update_rotor_sounds).chain());
    }
}

/// How loud a player wants things, which a server never hears of. Effects are the one-off sounds of the
/// world, and master is everything including what runs on and on, such as engines.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct AudioLevels {
    master: f32,
    effects: f32,
    /// The set a player has chosen, if they have, and what plays until they do.
    chosen: Option<SoundSet>,
    unless_chosen: SoundSet,
}

impl Default for AudioLevels {
    fn default() -> Self {
        Self { master: 1.0, effects: 1.0, chosen: None, unless_chosen: SoundSet::default() }
    }
}

impl AudioLevels {
    pub fn master(&self) -> f32 {
        self.master
    }

    pub fn effects(&self) -> f32 {
        self.effects
    }

    /// The set playing now.
    pub fn sound_set(&self) -> SoundSet {
        self.chosen.unwrap_or(self.unless_chosen)
    }

    /// What a player has picked, which is all that gets saved, so a mod can still pick for everyone else.
    pub fn chosen_sound_set(&self) -> Option<SoundSet> {
        self.chosen
    }

    pub fn choose_sound_set(&mut self, set: SoundSet) {
        self.chosen = Some(set);
    }

    pub fn set_master(&mut self, volume: f32) {
        self.master = volume.clamp(0.0, 1.0);
    }

    pub fn set_effects(&mut self, volume: f32) {
        self.effects = volume.clamp(0.0, 1.0);
    }
}

/// Every new sound is multiplied by the engine's global volume, so one-off sounds need nothing else.
fn apply_levels(levels: Res<AudioLevels>, mut global: ResMut<GlobalVolume>) {
    if levels.is_changed() {
        global.volume = Volume::Linear(levels.master * levels.effects);
    }
}

#[derive(Component)]
struct Ears;

/// Swapped on purpose. Rodio 0.22 makes the ear further from the sound the louder one, so the ear
/// it treats as left goes on the right. `left_and_right_come_out_of_the_right_sides` checks this.
fn ears() -> SpatialListener {
    let SpatialListener { left_ear_offset, right_ear_offset } = SpatialListener::new(EAR_GAP);
    SpatialListener { left_ear_offset: right_ear_offset, right_ear_offset: left_ear_offset }
}

fn spawn_ears(mut commands: Commands) {
    commands.spawn((Ears, ears(), Transform::default()));
}

fn turn_ears(input: Res<PendingInput>, mut ears: Query<&mut Transform, With<Ears>>) {
    for mut transform in &mut ears {
        transform.rotation = Quat::from_rotation_y(input.yaw + input.free_look.x);
    }
}

#[derive(Resource)]
struct Bank {
    dry: Handle<AudioSource>,
    selector: Handle<AudioSource>,
    tear: Handle<AudioSource>,
    zip: Handle<AudioSource>,
    heartbeat: Handle<AudioSource>,
}

fn build_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |bytes: Vec<u8>| sources.add(AudioSource { bytes: Arc::from(bytes) });
    let bank = Bank {
        dry: add(synth::dry_click()),
        selector: add(synth::clack(0x53, 3100.0, 0.35)),
        tear: add(synth::tear()),
        zip: add(synth::clack(0x56, 2200.0, 0.3)),
        heartbeat: add(synth::heartbeat()),
    };
    commands.insert_resource(bank);
}

#[derive(Clone, Copy)]
enum FarSound {
    Shot { barrel: Vec3, weapon: WeaponId },
    /// Goes past you, at this nearness from 0 to 1.
    Passing { closeness: f32, cracked: bool },
    Impact { surface: Surface },
    BodyHit,
    Metal,
    HullGroan,
    Crash { speed: f32 },
    Blast,
    Grenade,
    Clink { speed: f32 },
    Step { surface: Surface, intensity: f32 },
    Crawl { surface: Surface },
}

/// A sound still on its way through the air. It is rendered on arrival, for where you are by then.
#[derive(Component)]
struct Delayed {
    left: f32,
    source: Vec3,
    sound: FarSound,
    seed: u32,
    speed: f32,
}

pub fn ear_position(view: &LocalView) -> Vec3 {
    let lean = view.blended.lean_eye;
    let right = Vec3::new(view.blended.gun_yaw.cos(), 0.0, -view.blended.gun_yaw.sin());
    view.blended.feet + Vec3::Y * (view.blended.eye_height + lean.y) + right * lean.x
}

fn play(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(speed),
    ));
}

/// One slide, loud as it is set to be and `louder` on top, for when the same sound does for several things.
fn slide(commands: &mut Commands, sources: &mut Assets<AudioSource>, seed: u32, sound: &prs_synth::SlideSound, louder: f32, speed: f32) {
    let handle = sources.add(rendered(&prs_synth::slide(seed, sound)));
    play(commands, &handle, sound.volume * louder, speed);
}

/// A body or its kit moving, from whichever set is playing.
fn moved(commands: &mut Commands, sources: &mut Assets<AudioSource>, acoustics: &Acoustics, set: SoundSet, seed: u32, what: Moved, louder: f32, speed: f32) {
    match set {
        SoundSet::Current => slide(commands, sources, seed, acoustics.slides.of(what), louder, speed),
        SoundSet::New => touched(commands, sources, seed, acoustics.contacts.of(what), louder, speed),
    }
}

/// One contact, loud as it is set to be and `louder` on top.
fn touched(commands: &mut Commands, sources: &mut Assets<AudioSource>, seed: u32, sound: &ContactSound, louder: f32, speed: f32) {
    let handle = sources.add(rendered(&synth::contact(seed, sound)));
    play(commands, &handle, sound.volume * louder, speed);
}

/// A boot coming down on this, from whichever set is playing.
fn step_sound(acoustics: &Acoustics, set: SoundSet, seed: u32, surface: Surface) -> Vec<f32> {
    match set {
        SoundSet::Current => synth::footstep(seed, acoustics.step_on(surface)),
        SoundSet::New => synth::varied_footstep(seed, acoustics.step_on(surface), &acoustics.step_vary),
    }
}

/// The gun drawing back along a wall it has come up against. Nothing in the rules cares about this, so it is
/// worked out here, from how fast your gun is being pulled back.
fn gun_on_wall(
    mut commands: Commands,
    levels: Res<AudioLevels>,
    time: Res<Time>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    acoustics: Res<Acoustics>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut last: Local<f32>,
    mut wait: Local<f32>,
    mut seed: Local<u32>,
) {
    let Some(soldier) = sim.0.soldiers.get(local.0) else {
        return;
    };
    let dt = time.delta_secs();
    *wait = (*wait - dt).max(0.0);
    let speed = (soldier.gun_retract - *last) / dt.max(1e-3);
    *last = soldier.gun_retract;
    if speed > acoustics.gun_on_wall_speed && *wait == 0.0 {
        *wait = acoustics.gun_on_wall_wait;
        *seed = seed.wrapping_add(0x9E37_79B9);
        moved(&mut commands, &mut sources, &acoustics, levels.sound_set(), *seed, Moved::GunOnWall, 1.0, 1.0);
    }
}

fn play_events(
    levels: Res<AudioLevels>,
    mut commands: Commands,
    mut messages: MessageReader<SimMessage>,
    bank: Option<Res<Bank>>,
    view: Res<LocalView>,
    local: Res<LocalSoldier>,
    acoustics: Res<Acoustics>,
    sim: Res<SimResource>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut counter: Local<u32>,
    mut underfoot: Local<Surface>,
) {
    let Some(bank) = bank else {
        return;
    };
    let set = levels.sound_set();
    let underfoot_now = sim.0.soldiers.get(local.0).map_or(Surface::Dirt, |soldier| soldier.body.ground_surface);
    for SimMessage(event) in messages.read() {
        *counter = counter.wrapping_mul(1664525).wrapping_add(1013904223);
        let seed = *counter;
        let wobble = 1.0 + ((*counter >> 8) & 0xff) as f32 / 255.0 * 0.06 - 0.03;
        let ear = ear_position(&view);
        let mut far = |source: Vec3, sound: FarSound| {
            commands.spawn(Delayed { left: ear.distance(source) / acoustics.speed_of_sound, source, sound, seed, speed: wobble });
        };

        match *event {
            SimEvent::ShotFired { soldier, origin, dir, weapon } if soldier != local.0 => {
                far(origin, FarSound::Shot { barrel: dir, weapon });
            }
            SimEvent::ShotFired { origin, dir, weapon, .. } => {
                let brightness = acoustics.shot_brightness(origin, dir, ear);
                let mut samples = synth::gun_shot(seed, brightness, &sim.0.weapons[weapon].report);
                let mut volume = acoustics.own_shot_volume;
                if let Some((cutoff, gain)) = acoustics.suppressed.at(view.blended.suppression) {
                    samples = synth::muffled(&samples, cutoff);
                    volume *= gain;
                }
                let shot = sources.add(rendered(&samples));
                play(&mut commands, &shot, volume, wobble);
            }
            SimEvent::BulletPassed { soldier, point, distance, cracked, .. } if soldier == local.0 => {
                let closeness = (1.0 - distance / acoustics.bullet_passing_reach).max(0.0);
                far(point, FarSound::Passing { closeness, cracked });
            }
            SimEvent::BulletPassed { .. } => {}
            SimEvent::ThrownReleased { soldier, .. } if soldier == local.0 => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.2, 1.1 * wobble),
            SimEvent::ThrownReleased { .. } => {}
            SimEvent::ThrownBounced { point, speed } => far(point, FarSound::Clink { speed }),
            SimEvent::ThrownBurst { kind: ThrownKind::Frag, point, .. } => far(point, FarSound::Grenade),
            // It goes on for as long as the cloud does, so `smoke` looks after it from there.
            SimEvent::ThrownBurst { kind: ThrownKind::Smoke, .. } => {}
            SimEvent::Impact { point, surface, .. } => far(point, FarSound::Impact { surface }),
            SimEvent::TargetHit { point, .. } | SimEvent::SoldierHit { point, .. } => far(point, FarSound::BodyHit),
            SimEvent::Move { soldier, event: MoveEvent::Footstep { intensity, surface } } if soldier != local.0 => {
                let feet = sim.0.soldiers[soldier].body.pos + Vec3::Y * acoustics.source_clearance;
                if feet.distance(ear) < acoustics.other_steps_within {
                    let sound = if sim.0.soldiers[soldier].body.stance == Stance::Prone { FarSound::Crawl { surface } } else { FarSound::Step { surface, intensity } };
                    far(feet, sound);
                }
            }
            SimEvent::Weapon { soldier, .. } | SimEvent::Move { soldier, .. } | SimEvent::Carry { soldier, .. } | SimEvent::Health { soldier, .. }
                if soldier != local.0 => {}
            SimEvent::Respawned { .. } | SimEvent::KitDropped { .. } | SimEvent::KitGone { .. } => {}
            SimEvent::HeliHit { point, .. } => far(point, FarSound::Metal),
            SimEvent::Heli { event: HeliEvent::Crunch { point, speed }, .. } => far(point, FarSound::Crash { speed }),
            SimEvent::Heli { event: HeliEvent::RotorStrike { point, canopy }, .. } => {
                far(point, if canopy { FarSound::Impact { surface: Surface::Concrete } } else { FarSound::Metal });
            }
            SimEvent::Heli { .. } => {}
            SimEvent::VehicleHit { point, zone: VehicleZone::Tyre(_), .. } => far(point, FarSound::Impact { surface: Surface::Dirt }),
            SimEvent::VehicleHit { point, .. } => far(point, FarSound::Metal),
            SimEvent::Vehicle { event: VehicleEvent::Crashed { point, speed }, .. } => far(point, FarSound::Crash { speed }),
            SimEvent::Vehicle { event: VehicleEvent::Exploded { point }, .. } => far(point, FarSound::Blast),
            SimEvent::Vehicle { event: VehicleEvent::Heave { point }, .. } => far(point, FarSound::HullGroan),
            SimEvent::Vehicle { event: VehicleEvent::Landed { point, .. }, .. } => far(point, FarSound::Crash { speed: acoustics.landed_speed }),
            SimEvent::Vehicle { event: VehicleEvent::TyreBurst { .. }, vehicle } => far(sim.0.vehicles[vehicle].pos, FarSound::Impact { surface: Surface::Dirt }),
            SimEvent::Vehicle { .. } => {}
            SimEvent::Seat { soldier, .. } if soldier != local.0 => {}
            SimEvent::Seat { event, .. } => match event {
                SeatEvent::Entering { .. } | SeatEvent::Leaving { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::GunChange, 1.2, 0.8 * wobble),
                SeatEvent::Entered { .. } | SeatEvent::Left { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.4, wobble),
                SeatEvent::Swapped { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.6, wobble),
                SeatEvent::ExitBlocked { .. } => {}
            },
            SimEvent::Health { event, .. } => match event {
                // Going down is heard through the body. Nothing else about a wound has a sound of its own.
                HealthEvent::Downed => {
                    let fall = sources.add(rendered(&step_sound(&acoustics, set, seed, underfoot_now)));
                    play(&mut commands, &fall, acoustics.landing_volume(acoustics.land_full_speed), 0.85);
                }
                _ => {}
            },
            SimEvent::Carry { event, .. } => match event {
                CarryEvent::Selected(_) => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 0.8, 1.3 * wobble),
                CarryEvent::GunChanged(_) => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::GunChange, 1.0, wobble),
                CarryEvent::KitTaken { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.4, 0.85 * wobble),
                CarryEvent::TreatmentStarted { care: Care::Bandage, .. } => match set {
                    SoundSet::Current => play(&mut commands, &bank.tear, prs_synth::CURRENT_TEAR_VOLUME, wobble),
                    SoundSet::New => touched(&mut commands, &mut sources, seed, &acoustics.contacts.tear, 1.0, wobble),
                },
                CarryEvent::TreatmentStarted { care: Care::Medkit, .. } => match set {
                    SoundSet::Current => play(&mut commands, &bank.zip, prs_synth::CURRENT_ZIP_VOLUME, wobble),
                    SoundSet::New => touched(&mut commands, &mut sources, seed, &acoustics.contacts.zip, 1.0, wobble),
                },
                CarryEvent::TreatmentFinished { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.2, wobble),
                CarryEvent::TreatmentInterrupted { .. } => {}
                CarryEvent::Rested => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 1.4, 0.8 * wobble),
                CarryEvent::RestLifted => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Handling, 0.8, wobble),
            },
            SimEvent::Weapon { event, .. } => match event {
                WeaponEvent::Fired { .. } => {}
                WeaponEvent::DryFire => match set {
                    SoundSet::Current => play(&mut commands, &bank.dry, prs_synth::CURRENT_DRY_VOLUME, wobble),
                    SoundSet::New => touched(&mut commands, &mut sources, seed, &acoustics.contacts.dry, 1.0, wobble),
                },
                // Heard in step with the hands instead, in `reload`.
                WeaponEvent::ReloadStarted | WeaponEvent::ReloadFinished => {}
                WeaponEvent::ModeChanged(_) => match set {
                    SoundSet::Current => play(&mut commands, &bank.selector, prs_synth::CURRENT_SELECTOR_VOLUME, wobble),
                    SoundSet::New => touched(&mut commands, &mut sources, seed, &acoustics.contacts.selector, 1.0, wobble),
                },
                WeaponEvent::AdsRaised => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::SightsUp, 1.0, wobble),
                WeaponEvent::AdsLowered => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::SightsDown, 1.0, wobble),
            },
            SimEvent::Move { event, .. } => match event {
                MoveEvent::Footstep { intensity, surface } => {
                    *underfoot = surface;
                    if sim.0.soldiers.get(local.0).is_some_and(|soldier| soldier.body.stance == Stance::Prone) {
                        match set {
                            SoundSet::Current => slide(&mut commands, &mut sources, seed, &acoustics.crawl_on(surface), 1.0, wobble),
                            SoundSet::New => touched(&mut commands, &mut sources, seed, &acoustics.contact_crawl_on(surface), 1.0, wobble),
                        }
                    } else {
                        let step = sources.add(rendered(&step_sound(&acoustics, set, seed, surface)));
                        play(&mut commands, &step, acoustics.step_volume.0 + intensity * acoustics.step_volume.1, wobble);
                        moved(&mut commands, &mut sources, &acoustics, set, seed.rotate_left(13), Moved::Gear, intensity, wobble);
                    }
                }
                MoveEvent::Landed { speed } => {
                    let step = sources.add(rendered(&step_sound(&acoustics, set, seed, underfoot_now)));
                    play(&mut commands, &step, acoustics.landing_volume(speed), wobble);
                }
                MoveEvent::Jumped => {
                    let push = sources.add(rendered(&step_sound(&acoustics, set, seed, *underfoot)));
                    play(&mut commands, &push, acoustics.step_volume.0 + acoustics.step_volume.1 * 0.4, wobble);
                }
                MoveEvent::StanceChanged { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Stance, 1.0, wobble),
                MoveEvent::Climbed { .. } => moved(&mut commands, &mut sources, &acoustics, set, seed, Moved::Climb, 1.0, wobble),
                MoveEvent::Stepped { .. } => {}
            },
        }
    }
}

/// Your own heart, in time with the red at the edge of the screen and as loud as the bleed is bad.
fn heartbeat(mut commands: Commands, shown: Res<BleedShown>, bank: Option<Res<Bank>>, mut heard: Local<u32>) {
    if shown.beats == *heard {
        return;
    }
    *heard = shown.beats;
    if let (Some(bank), Some(band)) = (bank, shown.look.band(shown.severity)) {
        play(&mut commands, &bank.heartbeat, band.heartbeat_volume * shown.relieved(), 1.0);
    }
}

fn rendered(samples: &[f32]) -> AudioSource {
    AudioSource { bytes: Arc::from(synth::encode(samples)) }
}

fn play_delayed(
    levels: Res<AudioLevels>,
    mut commands: Commands,
    time: Res<Time>,
    view: Res<LocalView>,
    sim: Res<SimResource>,
    acoustics: Res<Acoustics>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut delayed: Query<(Entity, &mut Delayed)>,
) {
    let set = levels.sound_set();
    let ear = ear_position(&view);
    let world = &sim.0.world;
    let blocked = |from: Vec3, to: Vec3| {
        let path = to - from;
        world.raycast(from, path.normalize_or(Vec3::Y), path.length()).is_some()
    };
    for (entity, mut sound) in &mut delayed {
        sound.left -= time.delta_secs();
        if sound.left > 0.0 {
            continue;
        }
        commands.entity(entity).despawn();

        let distance = ear.distance(sound.source);
        let (mut samples, mut volume) = match sound.sound {
            FarSound::Shot { barrel, weapon } => {
                let brightness = acoustics.shot_brightness(sound.source, barrel, ear);
                (
                    synth::gun_shot(sound.seed, brightness, &sim.0.weapons[weapon].report),
                    acoustics.shot.volume(distance) * acoustics.shot_loudness(brightness),
                )
            }
            FarSound::Passing { closeness, cracked } => {
                let (far, near) = acoustics.bullet_passing_volume;
                (synth::bullet_passing(sound.seed, closeness, cracked), far + (near - far) * closeness)
            }
            FarSound::Impact { surface } => {
                let carry = acoustics.impact.volume(distance);
                match set {
                    SoundSet::Current => (synth::impact(sound.seed), carry),
                    SoundSet::New => {
                        let hit = acoustics.impact_on(surface);
                        (synth::contact(sound.seed, hit), carry * hit.volume)
                    }
                }
            }
            FarSound::BodyHit => (synth::body_hit(sound.seed), acoustics.body_hit.volume(distance)),
            FarSound::Metal => {
                let carry = acoustics.metal_hit.volume(distance);
                match set {
                    SoundSet::Current => (synth::metal_hit(sound.seed), carry),
                    SoundSet::New => (synth::contact(sound.seed, &acoustics.contacts.metal_hit), carry * acoustics.contacts.metal_hit.volume),
                }
            }
            FarSound::HullGroan => (synth::slide(sound.seed, &acoustics.slides.hull_groan), acoustics.crash.volume(distance) * acoustics.slides.hull_groan.volume),
            FarSound::Crash { speed } => (
                match set {
                    SoundSet::Current => synth::crash(sound.seed),
                    SoundSet::New => synth::contact_crash(sound.seed, &acoustics.contacts.crash_metal),
                },
                acoustics.crash.volume(distance) * (speed / acoustics.crash_full_speed).min(1.0),
            ),
            FarSound::Blast => (synth::blast(sound.seed), acoustics.blast.volume(distance)),
            FarSound::Grenade => (synth::blast(sound.seed), acoustics.grenade.volume(distance)),
            FarSound::Clink { speed } => {
                let carry = acoustics.clink.volume(distance) * (speed / acoustics.clink_full_speed.max(0.1)).min(1.0);
                match set {
                    SoundSet::Current => (synth::metal_hit(sound.seed), carry),
                    SoundSet::New => (synth::contact(sound.seed, &acoustics.contacts.clink), carry * acoustics.contacts.clink.volume),
                }
            }
            FarSound::Crawl { surface } => {
                let carry = acoustics.other_steps.volume(distance);
                match set {
                    SoundSet::Current => {
                        let crawl = acoustics.crawl_on(surface);
                        (prs_synth::slide(sound.seed, &crawl), carry * crawl.volume)
                    }
                    SoundSet::New => {
                        let crawl = acoustics.contact_crawl_on(surface);
                        (synth::contact(sound.seed, &crawl), carry * crawl.volume)
                    }
                }
            }
            FarSound::Step { surface, intensity } => (
                step_sound(&acoustics, set, sound.seed, surface),
                acoustics.other_steps.volume(distance) * (acoustics.step_volume.0 + intensity * acoustics.step_volume.1),
            ),
        };
        if let Some(muffle) = acoustics.muffling(sound.source, ear, blocked) {
            samples = synth::muffled(&samples, muffle.cutoff_hz);
            volume *= muffle.gain;
        }
        // The bullet itself cuts through it. It is everything else that goes dull.
        if !matches!(sound.sound, FarSound::Passing { .. })
            && let Some((cutoff, gain)) = acoustics.suppressed.at(view.blended.suppression)
        {
            samples = synth::muffled(&samples, cutoff);
            volume *= gain;
        }
        commands.spawn((
            AudioPlayer(sources.add(rendered(&samples))),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(sound.speed)
                .with_spatial(true),
            Transform::from_translation((sound.source - ear).normalize_or_zero() * EMITTER_RADIUS),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    use rodio::buffer::SamplesBuffer;
    use rodio::source::Spatial;
    use std::num::NonZero;

    /// Runs a sound through the real stereo code and returns its left and right volume.
    fn left_and_right(direction: Vec3) -> (f32, f32) {
        let ears = ears();
        let mono = SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(44_100).unwrap(), vec![1.0; 4]);
        let mut out = Spatial::new(
            mono,
            (direction * EMITTER_RADIUS).into(),
            ears.left_ear_offset.into(),
            ears.right_ear_offset.into(),
        );
        assert_eq!(out.channels().get(), 2);
        (out.next().unwrap() as f32, out.next().unwrap() as f32)
    }

    #[test]
    fn a_players_choice_of_sound_set_beats_the_config_and_only_a_choice_is_saved() {
        let mut levels = AudioLevels { unless_chosen: SoundSet::Current, ..default() };
        assert_eq!(levels.sound_set(), SoundSet::Current);
        assert_eq!(levels.chosen_sound_set(), None);
        levels.choose_sound_set(SoundSet::New);
        assert_eq!(levels.sound_set(), SoundSet::New);
        assert_eq!(levels.chosen_sound_set(), Some(SoundSet::New));
        assert_eq!(AudioLevels::default().sound_set(), SoundSet::New);
    }

    #[test]
    fn a_step_follows_the_switch_and_stays_as_tuned_under_current() {
        let a = Acoustics::default();
        assert_eq!(step_sound(&a, SoundSet::Current, 9, Surface::Dirt), synth::footstep(9, a.step_on(Surface::Dirt)));
        assert_ne!(step_sound(&a, SoundSet::New, 9, Surface::Dirt), synth::footstep(9, a.step_on(Surface::Dirt)));
    }

    #[test]
    fn left_and_right_come_out_of_the_right_sides() {
        let (left, right) = left_and_right(Vec3::X);
        assert!(right > left * 1.5, "from the right: {left} {right}");
        let (left, right) = left_and_right(Vec3::NEG_X);
        assert!(left > right * 1.5, "from the left: {left} {right}");
    }

    #[test]
    fn straight_ahead_and_all_around_is_full_volume_whatever_the_distance_was() {
        let (left, right) = left_and_right(Vec3::NEG_Z);
        assert!((left - right).abs() < 1e-5);
        assert!(left > 0.7, "the stereo code must not fade it: {left}");
    }
}
