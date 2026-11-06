//! The map on M, laid out like the spawn screen: the map as big as it will go, and beside it each point
//! along the chain with who holds it and how it stands, and your squad. Nothing about the other side but
//! what the points say.

use bevy::prelude::*;
use bevy::text::LineBreak;
use prs_models::TeamLooks;
use prs_runtime::{GameMode, LocalSoldier, PlayerNames, SimResource};

use crate::deploy::{MapScreen, MapSlot, Screens, ScreensDecided};
use crate::map::point_status;
use crate::theme::*;
use crate::{player_name, team_name};

/// Rows kept ready for points. A mod with more would want a different screen.
const POINT_ROWS: usize = 16;

pub(crate) struct OverviewPlugin;

impl Plugin for OverviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_overview).add_systems(Update, update_overview.after(ScreensDecided));
    }
}

#[derive(Component)]
struct OverviewRoot;
/// The nth point along the chain.
#[derive(Component)]
struct ChainRow(usize);
#[derive(Component)]
struct RowName;
#[derive(Component)]
struct RowOwner;
#[derive(Component)]
struct RowState;
#[derive(Component)]
struct SquadWords;

fn words(size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(LineBreak::NoWrap))
}

fn heading(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((Text::new(title), TextFont { font_size: FontSize::Px(TYPE_LABEL), ..default() }, TextColor(INK_DIM), Node { flex_shrink: 0.0, ..default() }));
}

fn spawn_overview(mut commands: Commands) {
    commands
        .spawn((
            OverviewRoot,
            GlobalZIndex(4),
            Visibility::Hidden,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, ..default() },
            BackgroundColor(SHADE),
        ))
        .with_children(|root| {
            // Room for the bar across the top, which the spawn screen and this share.
            root.spawn(Node { height: px(BAR_HEIGHT), flex_shrink: 0.0, ..default() });
            root.spawn(Node { flex_grow: 1.0, min_height: px(0), padding: UiRect::all(px(PAD)), column_gap: px(PAD), ..default() }).with_children(|body| {
                body.spawn((MapSlot(MapScreen::Overview), Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() }));
                body.spawn((
                    Node {
                        width: px(PANEL_WIDTH),
                        min_width: px(PANEL_MIN_WIDTH),
                        max_width: percent(PANEL_MAX_SHARE),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(PAD)),
                        row_gap: px(GAP),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                ))
                .with_children(|panel| {
                    heading(panel, "OBJECTIVES");
                    for nth in 0..POINT_ROWS {
                        panel
                            .spawn((
                                ChainRow(nth),
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    flex_shrink: 0.0,
                                    min_height: px(ROW_HEIGHT),
                                    justify_content: JustifyContent::Center,
                                    padding: UiRect::axes(px(GAP * 2.0), px(GAP)),
                                    border: UiRect::left(px(ACCENT_WIDTH)),
                                    display: Display::None,
                                    ..default()
                                },
                                BackgroundColor(ROW),
                                BorderColor::all(Color::NONE),
                            ))
                            .with_children(|row| {
                                row.spawn(Node { column_gap: px(GAP * 2.0), align_items: AlignItems::Baseline, ..default() }).with_children(|line| {
                                    line.spawn((RowName, words(TYPE_BODY, INK), Node { flex_grow: 1.0, ..default() }));
                                    line.spawn((RowOwner, words(TYPE_SMALL, INK_DIM)));
                                });
                                row.spawn((RowState, words(TYPE_SMALL, WARN), Node { display: Display::None, ..default() }));
                            });
                    }
                    panel.spawn((Node { height: px(1), flex_shrink: 0.0, margin: UiRect::vertical(px(SECTION * 0.5)), ..default() }, BackgroundColor(RULE)));
                    heading(panel, "YOUR SQUAD");
                    panel.spawn((SquadWords, Text::new(""), TextFont { font_size: FontSize::Px(TYPE_SMALL), ..default() }, TextColor(INK)));
                    panel.spawn(Node { flex_grow: 1.0, ..default() });
                    panel.spawn((Text::new("M close   TAB scoreboard"), TextFont { font_size: FontSize::Px(TYPE_LABEL), ..default() }, TextColor(INK_FAINT), Node { align_self: AlignSelf::Center, ..default() }));
                });
            });
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_overview(
    screens: Res<Screens>,
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    looks: Res<TeamLooks>,
    names: Res<PlayerNames>,
    mut root: Query<&mut Visibility, With<OverviewRoot>>,
    mut rows: Query<(&ChainRow, &mut Node, &mut BorderColor, &Children)>,
    children: Query<&Children>,
    mut texts: Query<(&mut Text, &mut TextColor, &mut Node, Has<RowName>, Has<RowOwner>, Has<RowState>), (Without<ChainRow>, Without<SquadWords>)>,
    mut squad_words: Query<&mut Text, (With<SquadWords>, Without<RowName>, Without<RowOwner>, Without<RowState>)>,
) {
    for mut visibility in &mut root {
        visibility.set_if_neq(if screens.map { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !screens.map {
        return;
    }
    let (game, sim) = (&game.0, &sim.0);
    let state = &game.state;
    let team = state.player(local.0).map_or(sim.soldiers[local.0].team, |p| p.team);
    for (row, mut node, mut border, kids) in &mut rows {
        let point = state.chain.get(row.0).and_then(|&index| Some((index, state.points.get(index)?, sim.level.capture_points.get(index)?)));
        let display = if point.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let Some((index, point, zone)) = point else { continue };
        let accent = point.owner.map_or(NEUTRAL.with_alpha(0.5), |owner| looks.of(owner).banner);
        if border.left != accent {
            *border = BorderColor::all(accent);
        }
        let owner = point.owner.map_or("nobody".to_string(), |owner| team_name(&sim.level, owner));
        let status = point_status(state, index, point, team);
        for entity in kids.iter().flat_map(|kid| std::iter::once(kid).chain(children.get(kid).into_iter().flatten().copied())) {
            let Ok((mut text, mut color, mut node, is_name, is_owner, is_state)) = texts.get_mut(entity) else { continue };
            let line = match (is_name, is_owner, is_state) {
                (true, ..) => format!("{}  {}", (b'A' + row.0 as u8) as char, zone.name),
                (_, true, _) => {
                    color.0 = point.owner.map_or(INK_DIM, |owner| looks.of(owner).banner);
                    owner.clone()
                }
                (.., true) => {
                    let display = if status.is_some() { Display::Flex } else { Display::None };
                    if node.display != display {
                        node.display = display;
                    }
                    color.0 = if status == Some("NEXT") { INK } else { WARN };
                    status.unwrap_or("").to_string()
                }
                _ => continue,
            };
            if text.0 != line {
                text.0 = line;
            }
        }
    }
    let squad = state.squad_of(local.0);
    let line = squad.map_or("Not in a squad".to_string(), |squad| {
        let lines: Vec<String> = squad
            .members
            .iter()
            .map(|&member| {
                let who = if member == local.0 { "You".to_string() } else { state.player(member).map_or_else(String::new, |p| player_name(&names, p.soldier, p.bot)) };
                let lead = if member == squad.leader { "SL " } else { "" };
                let soldier = &sim.soldiers[member];
                let how = if !soldier.present { "  waiting" } else if soldier.health.is_dead() { "  dead" } else if !soldier.health.is_up() { "  down" } else { "" };
                format!("{lead}{who}{how}")
            })
            .collect();
        format!("Squad {}\n{}", squad.id + 1, lines.join("\n"))
    });
    for mut text in &mut squad_words {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}
