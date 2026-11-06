//! Scripted runs for looking at what a game mode puts on the screen and in the world.
//!
//! `PRS_MODE=aas PRS_SCENE=mode:<what>` with `PRS_SHOT` takes these, on any map with points:
//! * `deploy` the spawn screen, `map` the map on M, `board` the scoreboard on TAB, `end` a round just over
//! * `zone` standing in the next point with the capture under way, `contested` the same with an enemy
//! * `flag` looking at a flag part way up, `rally` looking at a rally point just placed
//! * `friend` a friend ahead to be marked, `sides` one of each side next to each other, `down` a friend down
//! * `base` standing in the other side's main base with the warning up
//! * `deploy:many` the spawn screen with twelve squads and twelve kits, `deploy:one` with one of each, `deploy:wait`
//!   ready and counting down
//! * `click` chooses a kit by number key and by its pointer, clicks the base on the map and deploys, and fails
//!   if any of it did nothing. At any window size, and online

use bevy::prelude::*;
use prs_input::{Captured, UiCursor};
use prs_runtime::prs_sim::health::Zone;
use prs_runtime::prs_sim::{Sim, SoldierId};
use prs_hud::{DeployAction, DeployButton};
use prs_runtime::prs_mode::{Match, ModeCmd, PointState, SpawnAt};
use prs_runtime::{GameMode, LocalSoldier, PendingInput, PendingModeCmds, SimResource};

#[derive(Resource)]
pub struct ModeScene(pub String);

fn look_at(input: &mut PendingInput, sim: &Sim, local: SoldierId, at: Vec3) {
    let from = sim.soldiers[local].body.eye();
    let d = at - from;
    input.yaw = (-d.x).atan2(-d.z);
    input.pitch = (d.y / d.length()).asin();
}

/// The nth bot on this team.
fn bot(game: &Match, team: u8, nth: usize) -> Option<SoldierId> {
    game.state.players.iter().filter(|p| p.bot && p.team == team).nth(nth).map(|p| p.soldier)
}

/// Keeps someone standing here, whatever they think they are doing.
fn hold(sim: &mut Sim, id: SoldierId, at: Vec3) {
    let ground = sim.world.raycast(at + Vec3::Y * 30.0, Vec3::NEG_Y, 80.0).map_or(at.y, |hit| at.y + 30.0 - hit.distance);
    sim.soldiers[id].body.pos = Vec3::new(at.x, ground + 0.05, at.z);
    sim.soldiers[id].body.vel = Vec3::ZERO;
}

#[allow(clippy::too_many_arguments)]
pub fn drive_mode(
    scene: Res<ModeScene>,
    time: Res<Time>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut sim: ResMut<SimResource>,
    mut game: ResMut<GameMode>,
    local: Res<LocalSoldier>,
    mut input: ResMut<PendingInput>,
    mut cmds: ResMut<PendingModeCmds>,
    mut cursor: ResMut<UiCursor>,
    captured: Res<Captured>,
    buttons: Query<(&DeployButton, &ComputedNode, &UiGlobalTransform)>,
    window: Query<&Window>,
    mut sent: Local<u32>,
) {
    let t = time.elapsed_secs();
    let sim = &mut sim.0;
    let game = &mut game.0;
    let id = local.0;
    let once = |sent: &mut u32, step: u32| {
        let due = *sent == step;
        if due {
            *sent += 1;
        }
        due
    };
    if scene.0 == "deploy" {
        return;
    }
    // The most rows the screen has room for, and the fewest.
    if let Some(rows) = scene.0.strip_prefix("deploy:") {
        let team = game.state.player(id).map_or(0, |p| p.team);
        if rows == "wait" {
            // Dead a moment ago and already asked to go, with a few seconds still to wait.
            let tick = sim.tick;
            if let Some(player) = game.state.players.iter_mut().find(|p| p.soldier == id)
                && once(&mut sent, 0)
            {
                player.back_at = tick + 30 * 60;
                player.ready = true;
            }
        } else if rows == "one" {
            game.mode.cfg.kits.truncate(1);
            let mine = game.state.player(id).and_then(|p| p.squad);
            game.state.squads.retain(|s| s.team != team || Some(s.id) == mine);
        } else if once(&mut sent, 0) {
            let template = game.mode.cfg.kits[0].clone();
            for n in game.mode.cfg.kits.len()..12 {
                game.mode.cfg.kits.push(prs_runtime::prs_mode::Kit { name: format!("Specialist {}", n + 1), ..template.clone() });
            }
            let leader = id;
            for n in 0..12 {
                let squad_id = 100 + n;
                if game.state.squads.iter().filter(|s| s.team == team).count() < 12 {
                    game.state.squads.push(prs_runtime::prs_mode::Squad { id: squad_id, team, leader, members: vec![leader], rally: None, rally_ready_at: 0 });
                }
            }
        }
        return;
    }
    if scene.0 == "click" {
        // Wherever the buttons have been laid out, in window coordinates.
        let scale = window.single().map_or(1.0, |window| window.scale_factor());
        let centre = |want: DeployAction| {
            let found = buttons.iter().find(|(button, ..)| button.0 == want);
            found.map(|(.., at)| at.translation / scale).expect("the spawn screen has that button")
        };
        // By frames, since frames can be slow enough here to step over anything timed in seconds.
        if t < 2.0 {
            return;
        }
        *sent += 1;
        match *sent {
            4 => keys.press(KeyCode::Digit3),
            8 => {
                assert_eq!(game.state.player(id).expect("in the match").kit, 2, "the third number key should choose the third kit");
                cursor.position = centre(DeployAction::Kit(1));
                cursor.clicked = true;
            }
            14 => {
                assert_eq!(game.state.player(id).expect("in the match").kit, 1, "clicking Medic should choose that kit");
                cursor.position = centre(DeployAction::Base);
                cursor.clicked = true;
            }
            18 => {
                assert_eq!(game.state.player(id).expect("in the match").spawn, SpawnAt::Base, "clicking the base on the map chooses it");
                cursor.position = centre(DeployAction::Deploy);
                cursor.clicked = true;
            }
            46 => {
                assert!(captured.0, "and the mouse should never have been let go");
                assert!(game.state.player(id).is_some_and(|p| p.deployed), "the match should say you are in, after {t} seconds");
                assert!(sim.soldiers[id].present, "clicking DEPLOY should have put you in");
                assert_eq!(sim.soldiers[id].inventory.stash.medkits, 2);
            }
            _ => {}
        }
        return;
    }
    if t > 0.6 && once(&mut sent, 0) {
        cmds.0.push(ModeCmd::Deploy);
    }
    if t < 1.0 {
        return;
    }
    let team = game.state.player(id).map_or(0, |p| p.team);
    let next = game.state.attack_target(team).unwrap_or(0);
    let zone = sim.level.capture_points.get(next).map_or(Vec3::ZERO, |z| z.center);
    match scene.0.as_str() {
        "base" => {
            let center = sim.level.teams[1 - team as usize].base_center();
            hold(sim, id, center + Vec3::new(8.0, 0.0, 8.0));
        }
        "map" => {
            if once(&mut sent, 1) {
                keys.press(KeyCode::KeyM);
            }
        }
        "board" => keys.press(KeyCode::Tab),
        "end" => {
            if once(&mut sent, 1) {
                game.state.teams[1 - team as usize].tickets = 1;
                if let Some(enemy) = bot(game, 1 - team, 0) {
                    sim.hurt(enemy, Zone::Chest, 1000.0);
                }
            }
            if game.state.teams[1 - team as usize].tickets > 0 && t > 1.5 {
                game.state.teams[1 - team as usize].tickets = 0;
            }
        }
        "zone" | "contested" => {
            hold(sim, id, zone);
            if scene.0 == "contested"
                && let Some(enemy) = bot(game, 1 - team, 0)
            {
                hold(sim, enemy, zone + Vec3::X * 2.0);
            }
        }
        "flag" => {
            hold(sim, id, zone + Vec3::new(-40.0, 0.0, 10.0));
            if let Some(point) = game.state.points.get_mut(next) {
                *point = PointState { claimant: Some(team), progress: 0.6, rate: 0.0, since: sim.tick, ..*point };
            }
            look_at(&mut input, sim, id, zone + Vec3::Y * 4.0);
        }
        "rally" => {
            let spot = zone + Vec3::new(sim.level.capture_points[next].radius + 40.0, 0.0, 0.0);
            hold(sim, id, spot);
            let mate = game.state.squad_of(id).and_then(|s| s.members.iter().copied().find(|&m| m != id));
            if let Some(mate) = mate {
                hold(sim, mate, spot + Vec3::X * 4.0);
            }
            if t > 1.5 && once(&mut sent, 1) {
                cmds.0.push(ModeCmd::PlaceRally);
            }
            if t > 2.0 {
                hold(sim, id, spot + Vec3::new(0.0, 0.0, 4.0));
                let rally = game.state.squad_of(id).and_then(|s| s.rally).map_or(spot, |r| r.pos);
                look_at(&mut input, sim, id, rally);
            }
        }
        "friend" | "sides" | "down" => {
            // Out in the open, on the far side of the first point and looking on along the chain.
            let start = sim.level.capture_points[game.state.chain[0]].center;
            let (here, spot) = (start + Vec3::new(0.0, 0.0, 60.0), start + Vec3::new(0.0, 0.0, 30.0));
            hold(sim, id, here);
            let spot_eye = sim.world.raycast(spot + Vec3::Y * 30.0, Vec3::NEG_Y, 80.0).map_or(spot, |hit| spot + Vec3::Y * (30.0 - hit.distance));
            look_at(&mut input, sim, id, spot_eye + Vec3::Y * 1.0);
            if let Some(friend) = bot(game, team, 0) {
                hold(sim, friend, spot);
                if scene.0 == "down" && t > 1.5 && once(&mut sent, 1) {
                    sim.soldiers[friend].health.health = 1.0;
                    sim.hurt(friend, Zone::Chest, 5.0);
                }
            }
            if scene.0 == "sides"
                && let Some(enemy) = bot(game, 1 - team, 0)
            {
                hold(sim, enemy, spot + Vec3::new(3.0, 0.0, 0.0));
            }
        }
        _ => {}
    }
}
