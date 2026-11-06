//! Plays by itself, for proving multiplayer works where nobody can hold a mouse, such as a headless
//! browser, which cannot lock the pointer. Walks off for a few seconds so that the fight has some
//! distance in it, then aims at the nearest other soldier and fires. `?netbot=1` on the web, or
//! `PRS_NETBOT=1` on a desktop. `?netbot=look` aims and holds fire, for a picture of everyone alive. In a
//! browser the page title reports how it is going.

use bevy::prelude::*;
use prs_runtime::prs_sim::SimEvent;
use prs_runtime::prs_sim::protocol::Buttons;
use prs_input::Captured;
use prs_runtime::prs_mode::ModeCmd;
use prs_runtime::{LocalSoldier, NetStatus, PendingInput, PendingModeCmds, SimMessage, SimResource};

const WALK_OFF_SECONDS: f32 = 2.0;
const ASK_TO_DEPLOY_EVERY_SECONDS: f32 = 3.0;
const SHOT_EVERY_SECONDS: f32 = 0.5;
const TRIGGER_HELD_SECONDS: f32 = 0.05;
const CHEST_HEIGHT: f32 = 1.2;

pub struct NetBotPlugin;

impl Plugin for NetBotPlugin {
    fn build(&self, app: &mut App) {
        if requested() {
            app.insert_resource(Tally { firing: !holds_fire(), ..default() }).add_systems(
                RunFixedMainLoop,
                (count_hits, drive).chain().in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop).after(prs_input::ReadInput),
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn requested() -> bool {
    std::env::var("PRS_NETBOT").is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn holds_fire() -> bool {
    std::env::var("PRS_NETBOT").is_ok_and(|value| value == "look")
}

#[cfg(target_arch = "wasm32")]
fn requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|search| search.contains("netbot="))
}

#[cfg(target_arch = "wasm32")]
fn holds_fire() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|search| search.contains("netbot=look"))
}

#[derive(Resource, Default)]
struct Tally {
    shots: u32,
    hits_given: u32,
    hits_taken: u32,
    joined_for: f32,
    firing: bool,
}

fn count_hits(mut messages: MessageReader<SimMessage>, local: Res<LocalSoldier>, mut tally: ResMut<Tally>) {
    for SimMessage(event) in messages.read() {
        match *event {
            SimEvent::ShotFired { soldier, .. } if soldier == local.0 => tally.shots += 1,
            SimEvent::SoldierHit { soldier, shooter, .. } => {
                tally.hits_given += (shooter == local.0) as u32;
                tally.hits_taken += (soldier == local.0) as u32;
            }
            _ => {}
        }
    }
}

fn drive(
    time: Res<Time<Real>>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    status: Res<NetStatus>,
    mut input: ResMut<PendingInput>,
    mut tally: ResMut<Tally>,
    mut captured: ResMut<Captured>,
    mut requests: ResMut<PendingModeCmds>,
) {
    // Nobody here to click, and the page saying to click would cover what there is to see.
    captured.0 = true;
    let NetStatus::Playing { players, rtt_ms, reconciles, over, .. } = *status else {
        report_problem(status.problem());
        return;
    };
    // Real seconds, because on a software renderer a frame can take several.
    tally.joined_for += time.delta_secs();
    let sim = &sim.0;
    let me = &sim.soldiers[local.0];
    // Straight through a spawn screen, if there is one.
    let before = tally.joined_for - time.delta_secs();
    if (!me.present || me.health.is_dead()) && before % ASK_TO_DEPLOY_EVERY_SECONDS > tally.joined_for % ASK_TO_DEPLOY_EVERY_SECONDS {
        requests.0.push(ModeCmd::Deploy);
    }
    report(&tally, me.health.health, players, rtt_ms, reconciles, over);

    if tally.joined_for < WALK_OFF_SECONDS {
        input.wish_move = Vec2::new(0.0, 1.0);
        input.yaw = local.0 as f32 * std::f32::consts::PI;
        return;
    }
    input.wish_move = Vec2::ZERO;
    let muzzle = me.gun_pose(&sim.weapons).muzzle;
    let nearest = sim
        .soldiers
        .iter()
        .enumerate()
        .filter(|(id, s)| *id != local.0 && s.present && s.health.is_up())
        .map(|(_, s)| s.body.pos)
        .min_by(|a, b| a.distance(muzzle).total_cmp(&b.distance(muzzle)));
    let Some(them) = nearest else {
        input.held.set(Buttons::FIRE, false);
        return;
    };
    let to = them + Vec3::Y * CHEST_HEIGHT - muzzle;
    let gun = me.weapon.aim_offset();
    input.yaw = (-to.x).atan2(-to.z) - gun.x;
    input.pitch = (to.y / to.length()).asin() - gun.y;
    input.held.set(Buttons::FIRE, tally.firing && tally.joined_for % SHOT_EVERY_SECONDS < TRIGGER_HELD_SECONDS);
}

#[cfg(target_arch = "wasm32")]
fn report(tally: &Tally, health: f32, players: usize, rtt_ms: f32, reconciles: u32, over: &str) {
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        document.set_title(&format!(
            "netbot players={players} shots={} given={} taken={} health={health:.0} rtt={rtt_ms:.1} reconciles={reconciles} over={over}",
            tally.shots, tally.hits_given, tally.hits_taken,
        ));
    }
}

#[cfg(target_arch = "wasm32")]
fn report_problem(problem: Option<String>) {
    if let (Some(problem), Some(document)) = (problem, web_sys::window().and_then(|window| window.document())) {
        document.set_title(&format!("netbot {problem}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn report(_: &Tally, _: f32, _: usize, _: f32, _: u32, _: &str) {}

#[cfg(not(target_arch = "wasm32"))]
fn report_problem(_: Option<String>) {}
