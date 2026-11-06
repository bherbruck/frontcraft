//! The scoreboard on TAB, and the same with who won on it when a round ends.

use bevy::prelude::*;
use prs_models::TeamLooks;
use prs_runtime::prs_mode::{Match, Phase, TEAMS};
use prs_runtime::prs_sim::Sim;
use prs_runtime::prs_sim::protocol::TICK_DT;
use prs_runtime::{GameMode, LocalSoldier, PlayerNames, SimResource};
use prs_voice::Voices;

use crate::deploy::Screens;
use crate::{INK, player_name, team_name};

pub(crate) struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_board).add_systems(Update, update_board.after(crate::deploy::ScreensDecided));
    }
}

#[derive(Component)]
struct Board;
#[derive(Component)]
struct Banner;
#[derive(Component)]
struct TeamColumn(u8);

fn spawn_board(mut commands: Commands) {
    commands
        .spawn((
            Board,
            GlobalZIndex(6),
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::top(percent(5)),
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.78)),
        ))
        .with_children(|root| {
            root.spawn((Banner, Text::new(""), TextFont { font_size: FontSize::Px(30.0), ..default() }, TextColor(INK)));
            root.spawn(Node { column_gap: px(36), ..default() }).with_children(|row| {
                for team in 0..TEAMS as u8 {
                    row.spawn((TeamColumn(team), Text::new(""), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(INK)));
                }
            });
        });
}

/// One team in columns, by squad and then by how they have done. The font is monospaced, so spaces
/// are all it takes.
fn team_lines(sim: &Sim, game: &Match, team: u8, you: usize, voices: &Voices, names: &PlayerNames) -> String {
    let state = &game.state;
    let side = &state.teams[team as usize];
    let points = if state.points.is_empty() { String::new() } else { format!("   {} of {} points", state.points_held(team), state.points.len()) };
    let tickets = if game.mode.cfg.tickets.is_some() { format!("   {} tickets", side.tickets.max(0)) } else { String::new() };
    let mut text = format!("{}{tickets}{points}\n\n   {:<11}{:>6}{:>7}{:>6}{:>5}{:>9}\n", team_name(&sim.level, team), "", "kills", "deaths", "saves", "caps", "friendly");
    let mut players: Vec<_> = state.players.iter().filter(|p| p.team == team).collect();
    players.sort_by_key(|p| (p.squad, std::cmp::Reverse(p.score.kills), p.soldier));
    let mut squad = None;
    for player in players {
        if player.squad != squad {
            squad = player.squad;
            if let Some(squad) = squad {
                text.push_str(&format!("Squad {}\n", squad + 1));
            }
        }
        let lead = state.squad_of(player.soldier).is_some_and(|s| s.leader == player.soldier);
        let name = if player.soldier == you {
            "You".to_string()
        } else {
            player_name(names, player.soldier, player.bot)
        };
        let score = player.score;
        text.push_str(&format!(
            " {} {name:<11}{:>6}{:>7}{:>6}{:>5}{:>9}\n",
            if lead { "*" } else { " " },
            score.kills,
            score.deaths,
            score.revives,
            score.captures,
            score.team_kills,
        ));
        if voices.is_muted(player.soldier as u16) {
            text.truncate(text.trim_end().len());
            text.push_str("  muted\n");
        }
    }
    text
}

fn seconds_until(tick: u32, sim: &Sim) -> f32 {
    tick.saturating_sub(sim.tick) as f32 * TICK_DT
}

#[allow(clippy::type_complexity)]
fn update_board(
    screens: Res<Screens>,
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    looks: Res<TeamLooks>,
    voices: Res<Voices>,
    names: Res<PlayerNames>,
    mut board: Query<&mut Visibility, With<Board>>,
    mut banner: Query<(&mut Text, &mut TextColor), (With<Banner>, Without<TeamColumn>)>,
    mut columns: Query<(&TeamColumn, &mut Text, &mut TextColor), Without<Banner>>,
) {
    let game = &game.0;
    let has_teams = !game.state.players.is_empty() && game.mode.cfg.tickets.is_some();
    let shown = screens.board && has_teams;
    for mut visibility in &mut board {
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !shown {
        return;
    }
    let sim = &sim.0;
    for (mut text, mut color) in &mut banner {
        let (line, tint) = match game.state.phase {
            Phase::Ended { winner: Some(team), restart_at } => {
                (format!("{} WINS\nnext round in {:.0}", team_name(&sim.level, team).to_uppercase(), seconds_until(restart_at, sim)), looks.of(team).banner)
            }
            Phase::Ended { winner: None, restart_at } => (format!("A DRAW\nnext round in {:.0}", seconds_until(restart_at, sim)), INK),
            Phase::Playing => (game.mode.cfg.name.clone(), INK),
        };
        if text.0 != line {
            text.0 = line;
        }
        color.0 = tint;
    }
    for (column, mut text, mut color) in &mut columns {
        let line = team_lines(sim, game, column.0, local.0, &voices, &names);
        if text.0 != line {
            text.0 = line;
        }
        color.0 = looks.of(column.0).banner.mix(&Color::WHITE, 0.45);
    }
}
