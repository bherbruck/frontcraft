//! The spawn screen, and which of the full-screen things are up. It comes up by itself when you are off
//! the field, and on ENTER otherwise to sort out your squad and kit for next time.
//!
//! The map is most of it, with where you can come in marked on it to click. Beside it are the squads, the
//! kits and the way in, with how long until you can go on the button itself. Across the top are the
//! tickets and the clock. All of it is laid out by flex, so it fits whatever the window, and lists that
//! will not fit scroll. How it looks is in [`crate::theme`].

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::text::LineBreak;
use bevy::window::PrimaryWindow;
use prs_input::{Captured, UiCursor};
use prs_models::TeamLooks;
use prs_runtime::prs_mode::{Deployment, Match, MatchState, ModeCmd, Phase, PlayerState, SpawnAt, TEAMS};
use prs_runtime::prs_sim::Sim;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::prs_sim::protocol::TICK_DT;
use prs_runtime::{GameMode, LocalSoldier, LocalView, PendingModeCmds, PlayerNames, SimResource};

use crate::scale::to_hud;
use crate::theme::*;
use crate::{player_name, team_name};

/// Rows kept ready. More squads or kits than this in a mod would want a different screen.
const KIT_ROWS: usize = 12;
const SQUAD_ROWS: usize = 12;
const CURSOR_SIZE: f32 = 9.0;
/// Number keys choose a kit, as well as the pointer.
const KIT_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// Which screens are up. What the mouse does follows from `deploy`.
#[derive(Resource, Debug, Default)]
pub struct Screens {
    pub deploy: bool,
    /// Asked for with ENTER, rather than being there because you are off the field.
    pub deploy_asked: bool,
    pub map: bool,
    pub board: bool,
}

/// Runs once [`Screens`] is settled for the frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ScreensDecided;

/// Runs once what the pointer is over has been worked out for the frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PointerTracked;

pub(crate) struct DeployPlugin;

impl Plugin for DeployPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Screens::default()).add_systems(Startup, spawn_screen).add_systems(
            Update,
            (
                update_screens.in_set(ScreensDecided),
                track_pointer.in_set(PointerTracked),
                scroll_lists,
                update_top_bar,
                update_rows,
                update_way_in,
                update_pointer,
            )
                .chain(),
        );
    }
}

/// Something on the spawn screen to click, and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeployAction {
    Team(u8),
    Kit(usize),
    /// The nth squad on your side.
    Squad(usize),
    NewSquad,
    LeaveSquad,
    Base,
    Rally,
    /// Go, or stop waiting to.
    Deploy,
    Close,
}

/// On anything that does a [`DeployAction`] when clicked, the map's spawn marks included. Public so that a
/// script can find where a button is.
#[derive(Component, Clone, Copy, Debug)]
pub struct DeployButton(pub DeployAction);

/// Whether the pointer is over it this frame, and held down on it.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Pointed {
    pub hovered: bool,
    pub pressed: bool,
}

/// The screens the map is shown on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapScreen {
    Deploy,
    Overview,
}

/// Where the map goes on a screen. There is one map, placed over whichever screen is up.
#[derive(Component)]
pub(crate) struct MapSlot(pub MapScreen);

/// A list that scrolls under the wheel when it is too long, and hides what is scrolled away from clicks.
#[derive(Component)]
struct ScrollList;

#[derive(Component)]
struct DeployRoot;
/// The bar across the top, shared by the spawn screen and the map.
#[derive(Component)]
struct TopBar;
#[derive(Component)]
struct ModeTitle;
#[derive(Component)]
struct TeamTitle;
#[derive(Component)]
struct Tickets(u8);
#[derive(Component)]
struct RoundClock;
#[derive(Component)]
struct SpawnLine;
#[derive(Component)]
struct Hints;
/// The parts of a row's words: the main line, what sits at the right of it, a line under it, and a key.
#[derive(Component)]
struct RowMain;
#[derive(Component)]
struct RowAside;
#[derive(Component)]
struct RowSub;
#[derive(Component)]
struct RowKey;
#[derive(Component)]
struct PointerMark;

/// ENTER, number keys, M, TAB and T, and what being off the field does to the screens. The first thing a
/// player sees is the intro with the controls on it, and the first click gets here with the mouse
/// captured. It stays captured from then on, whatever happens to them, and a screen that comes up has a
/// pointer of its own. So a browser, which wants a click to capture the mouse, only ever gets asked once.
#[allow(clippy::too_many_arguments)]
fn update_screens(
    keys: Res<ButtonInput<KeyCode>>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    captured: Res<Captured>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<UiCursor>,
    mut screens: ResMut<Screens>,
    mut cmds: ResMut<PendingModeCmds>,
) {
    let game = &game.0;
    let player = game.state.player(local.0);
    let choosing = player.is_some() && game.mode.cfg.deployment == Deployment::Choose;
    let life = view.blended.life;
    let off_field = choosing && (player.is_some_and(|p| !p.deployed) || life == Life::Dead);
    let ended = game.state.ended();

    if captured.0 && keys.just_pressed(KeyCode::Enter) {
        if life == Life::Down {
            cmds.0.push(ModeCmd::GiveUp);
        } else if off_field {
            cmds.0.push(if player.is_some_and(|p| p.ready) { ModeCmd::CancelDeploy } else { ModeCmd::Deploy });
        } else if choosing {
            screens.deploy_asked = !screens.deploy_asked;
        }
    }
    if captured.0 && keys.just_pressed(KeyCode::KeyT) {
        cmds.0.push(ModeCmd::PlaceRally);
    }
    if !captured.0 {
        screens.deploy_asked = false;
    }
    screens.deploy = captured.0 && (off_field || screens.deploy_asked) && !ended;
    if screens.deploy {
        for (kit, key) in KIT_KEYS.into_iter().enumerate() {
            if keys.just_pressed(key) {
                cmds.0.push(ModeCmd::SetKit(kit));
            }
        }
    }
    if screens.deploy && !cursor.active {
        cursor.position = Vec2::new(window.width(), window.height()) * 0.5;
    }
    cursor.active = screens.deploy;
    if captured.0 && keys.just_pressed(KeyCode::KeyM) && !screens.deploy {
        screens.map = !screens.map;
    }
    screens.map &= !screens.deploy;
    screens.board = keys.pressed(KeyCode::Tab) || ended;
}

/// What a button asks for, if anything.
fn command_for(action: DeployAction, state: &MatchState, soldier: usize) -> Option<ModeCmd> {
    let player = state.player(soldier);
    let team = player.map_or(0, |p| p.team);
    Some(match action {
        DeployAction::Team(team) => ModeCmd::JoinTeam(team),
        DeployAction::Kit(kit) => ModeCmd::SetKit(kit),
        DeployAction::Squad(nth) => ModeCmd::JoinSquad(state.squads.iter().filter(|s| s.team == team).nth(nth)?.id),
        DeployAction::NewSquad => ModeCmd::CreateSquad,
        DeployAction::LeaveSquad => ModeCmd::LeaveSquad,
        DeployAction::Base => ModeCmd::SetSpawn(SpawnAt::Base),
        DeployAction::Rally => ModeCmd::SetSpawn(SpawnAt::Rally),
        DeployAction::Deploy => {
            if player.is_some_and(|p| p.ready) { ModeCmd::CancelDeploy } else { ModeCmd::Deploy }
        }
        DeployAction::Close => return None,
    })
}

/// Whether a point in physical pixels is on this node.
fn covers(node: &ComputedNode, transform: &UiGlobalTransform, point: Vec2) -> bool {
    node.size().x > 0.0 && node.contains_point(*transform, point)
}

/// What the pointer is over, and what a click there asks for. A row scrolled out of its list is not there
/// to be clicked, and a hidden mark is not either.
#[allow(clippy::type_complexity)]
fn track_pointer(
    mut screens: ResMut<Screens>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    cursor: Res<UiCursor>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cmds: ResMut<PendingModeCmds>,
    mut buttons: Query<(&DeployButton, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, Option<&ChildOf>, &mut Pointed)>,
    lists: Query<(&ComputedNode, &UiGlobalTransform), With<ScrollList>>,
) {
    // Layout is in physical pixels and the pointer in the window's logical ones.
    let point = cursor.position * window.scale_factor();
    for (button, node, transform, visible, parent, mut pointed) in &mut buttons {
        let in_view = parent.and_then(|p| lists.get(p.parent()).ok()).is_none_or(|(list, at)| covers(list, at, point));
        let hovered = screens.deploy && visible.get() && in_view && covers(node, transform, point);
        let now = Pointed { hovered, pressed: hovered && cursor.held };
        if pointed.hovered != now.hovered || pointed.pressed != now.pressed {
            *pointed = now;
        }
        if hovered && cursor.clicked {
            match command_for(button.0, &game.0.state, local.0) {
                Some(cmd) => cmds.0.push(cmd),
                None => screens.deploy_asked = false,
            }
        }
    }
}

/// The wheel moves whichever list the pointer is over.
fn scroll_lists(
    screens: Res<Screens>,
    cursor: Res<UiCursor>,
    window: Single<&Window, With<PrimaryWindow>>,
    wheel: Res<AccumulatedMouseScroll>,
    mut lists: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<ScrollList>>,
) {
    if !screens.deploy || wheel.delta.y == 0.0 {
        return;
    }
    let notches = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y,
        MouseScrollUnit::Pixel => wheel.delta.y / SCROLL_STEP,
    };
    for (node, transform, mut scroll) in &mut lists {
        if covers(node, transform, cursor.position * window.scale_factor()) {
            // Layout keeps it within what there is to scroll.
            scroll.0.y = (scroll.0.y - notches * SCROLL_STEP).max(0.0);
        }
    }
}

fn update_pointer(cursor: Res<UiCursor>, scale: Res<UiScale>, mut mark: Query<(&mut Node, &mut Visibility), With<PointerMark>>) {
    let at = to_hud(cursor.position, &scale);
    for (mut node, mut visibility) in &mut mark {
        visibility.set_if_neq(if cursor.active { Visibility::Inherited } else { Visibility::Hidden });
        node.left = px(at.x);
        node.top = px(at.y);
    }
}

fn text(size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(LineBreak::NoWrap))
}

/// Words that may go onto another line rather than be cut off.
fn wrapping_text(size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(LineBreak::WordBoundary))
}

fn heading(parent: &mut ChildSpawnerCommands, words: &str) {
    parent.spawn((Text::new(words), TextFont { font_size: FontSize::Px(TYPE_LABEL), ..default() }, TextColor(INK_DIM)));
}

fn spawn_screen(mut commands: Commands) {
    commands.spawn((
        PointerMark,
        GlobalZIndex(20),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            width: px(CURSOR_SIZE),
            height: px(CURSOR_SIZE),
            margin: UiRect::all(px(-CURSOR_SIZE * 0.5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::WHITE),
        BorderColor::all(Color::BLACK),
    ));
    commands
        .spawn((
            DeployRoot,
            GlobalZIndex(4),
            Visibility::Hidden,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, ..default() },
            BackgroundColor(SHADE),
        ))
        .with_children(|root| {
            root.spawn(Node { height: px(BAR_HEIGHT), flex_shrink: 0.0, ..default() });
            root.spawn(Node { flex_grow: 1.0, min_height: px(0), padding: UiRect::all(px(PAD)), column_gap: px(PAD), ..default() }).with_children(|body| {
                body.spawn((MapSlot(MapScreen::Deploy), Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() }));
                side_panel(body);
            });
        });
    top_bar(&mut commands);
}

/// Who you are, the tickets and the clock.
fn top_bar(commands: &mut Commands) {
    commands.spawn((
        TopBar,
        GlobalZIndex(6),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: px(BAR_HEIGHT),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(px(PAD)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(SECTION),
            ..default()
        },
        BackgroundColor(PANEL),
    ))
    .with_children(|bar| {
        bar.spawn(Node { flex_basis: px(0), flex_grow: 1.0, column_gap: px(GAP * 2.0), align_items: AlignItems::Baseline, overflow: Overflow::clip(), ..default() }).with_children(|left| {
            left.spawn((ModeTitle, text(TYPE_SMALL, INK_DIM)));
            left.spawn((TeamTitle, text(TYPE_TITLE, INK)));
        });
        bar.spawn(Node { column_gap: px(SECTION), align_items: AlignItems::Baseline, ..default() }).with_children(|middle| {
            for team in 0..TEAMS as u8 {
                if team > 0 {
                    middle.spawn((Text::new("TICKETS"), TextFont { font_size: FontSize::Px(TYPE_LABEL), ..default() }, TextColor(INK_FAINT)));
                }
                middle.spawn((Tickets(team), text(TYPE_TITLE, INK)));
            }
        });
        bar.spawn(Node { flex_basis: px(0), flex_grow: 1.0, justify_content: JustifyContent::FlexEnd, overflow: Overflow::clip(), ..default() }).with_child((RoundClock, text(TYPE_BODY, INK_DIM)));
    });
}

/// Squads, kits and the way in.
fn side_panel(body: &mut ChildSpawnerCommands) {
    body.spawn((
        Node {
            width: px(PANEL_WIDTH),
            min_width: px(PANEL_MIN_WIDTH),
            max_width: percent(PANEL_MAX_SHARE),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(PAD)),
            row_gap: px(GAP),
            min_height: px(0),
            ..default()
        },
        BackgroundColor(PANEL),
    ))
    .with_children(|panel| {
        panel.spawn(Node { column_gap: px(GAP), flex_shrink: 0.0, ..default() }).with_children(|row| {
            for team in 0..TEAMS as u8 {
                row_button(row, DeployAction::Team(team), true);
            }
        });
        rule(panel);
        panel.spawn(Node { align_items: AlignItems::Center, column_gap: px(GAP), flex_shrink: 0.0, ..default() }).with_children(|row| {
            row.spawn(Node { flex_grow: 1.0, ..default() }).with_children(|title| heading(title, "SQUADS"));
            small_button(row, DeployAction::NewSquad);
            small_button(row, DeployAction::LeaveSquad);
        });
        list(panel, (0..SQUAD_ROWS).map(DeployAction::Squad));
        rule(panel);
        panel.spawn(Node { flex_shrink: 0.0, ..default() }).with_children(|title| heading(title, "KIT"));
        list(panel, (0..KIT_ROWS).map(DeployAction::Kit));
        rule(panel);
        // Whatever height is left goes here, so the way in is always at the bottom.
        panel.spawn(Node { flex_grow: 1.0, ..default() });
        panel.spawn((SpawnLine, wrapping_text(TYPE_SMALL, INK_DIM), Node { flex_shrink: 0.0, ..default() }));
        action_button(panel, DeployAction::Deploy);
        action_button(panel, DeployAction::Close);
        panel.spawn((Hints, text(TYPE_LABEL, INK_FAINT), Node { flex_shrink: 0.0, align_self: AlignSelf::Center, ..default() }));
    });
}

fn rule(panel: &mut ChildSpawnerCommands) {
    panel.spawn((Node { height: px(1), flex_shrink: 0.0, margin: UiRect::vertical(px(SECTION * 0.5)), ..default() }, BackgroundColor(RULE)));
}

/// Rows in a box that takes what height there is and scrolls the rest.
fn list(panel: &mut ChildSpawnerCommands, actions: impl Iterator<Item = DeployAction>) {
    panel
        .spawn((
            ScrollList,
            ScrollPosition::default(),
            Node { flex_direction: FlexDirection::Column, row_gap: px(GAP), flex_shrink: 1.0, min_height: px(ROW_HEIGHT), overflow: Overflow::scroll_y(), ..default() },
        ))
        .with_children(|list| {
            for action in actions {
                row_button(list, action, false);
            }
        });
}

/// A row: an optional key on the left, words, something at the right, and a line under it.
fn row_button(parent: &mut ChildSpawnerCommands, action: DeployAction, grow: bool) {
    parent
        .spawn((
            DeployButton(action),
            Pointed::default(),
            Node {
                flex_grow: if grow { 1.0 } else { 0.0 },
                flex_basis: if grow { px(0) } else { Val::Auto },
                flex_shrink: 0.0,
                min_height: px(ROW_HEIGHT),
                padding: UiRect::axes(px(GAP * 2.0), px(GAP)),
                border: UiRect::left(px(ACCENT_WIDTH)),
                column_gap: px(GAP * 2.0),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(ROW),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((RowKey, text(TYPE_SMALL, INK_DIM), Node { min_width: px(TYPE_SMALL), display: Display::None, ..default() }));
            row.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: px(0), overflow: Overflow::clip(), ..default() }).with_children(|words| {
                words.spawn((RowMain, text(TYPE_BODY, INK)));
                // Under the name, and the one line that may wrap, so it all shows.
                words.spawn((RowSub, wrapping_text(TYPE_SMALL, INK_DIM), Node { display: Display::None, ..default() }));
            });
            row.spawn((RowAside, text(TYPE_SMALL, INK_DIM)));
        });
}

fn small_button(parent: &mut ChildSpawnerCommands, action: DeployAction) {
    parent
        .spawn((
            DeployButton(action),
            Pointed::default(),
            Node { padding: UiRect::axes(px(GAP * 2.0), px(GAP)), border: UiRect::left(px(ACCENT_WIDTH)), ..default() },
            BackgroundColor(ROW),
            BorderColor::all(Color::NONE),
        ))
        .with_child((RowMain, text(TYPE_SMALL, INK)));
}

fn action_button(parent: &mut ChildSpawnerCommands, action: DeployAction) {
    parent
        .spawn((
            DeployButton(action),
            Pointed::default(),
            Node {
                min_height: px(ACTION_HEIGHT),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(WAITING),
        ))
        .with_children(|button| {
            button.spawn((RowMain, text(TYPE_ACTION, INK)));
            button.spawn((RowSub, text(TYPE_SMALL, INK_DIM), Node { display: Display::None, ..default() }));
        });
}

/// Whole seconds until a spawn on this choice, counting the extra a rally point costs.
fn seconds_to_go(game: &Match, player: &PlayerState, tick: u32) -> u32 {
    let extra = if player.spawn == SpawnAt::Rally { game.mode.cfg.rally.as_ref().map_or(0.0, |r| r.extra_wait) } else { 0.0 };
    (player.wait_seconds(tick) + extra).max(0.0).ceil() as u32
}

/// Minutes and seconds.
fn clock(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[allow(clippy::type_complexity)]
fn update_top_bar(
    screens: Res<Screens>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    sim: Res<SimResource>,
    looks: Res<TeamLooks>,
    mut root: Query<&mut Visibility, (With<DeployRoot>, Without<TopBar>)>,
    mut bar: Query<&mut Visibility, (With<TopBar>, Without<DeployRoot>)>,
    mut words: ParamSet<(
        Query<&mut Text, With<ModeTitle>>,
        Query<(&mut Text, &mut TextColor), With<TeamTitle>>,
        Query<(&Tickets, &mut Text, &mut TextColor)>,
        Query<&mut Text, With<RoundClock>>,
    )>,
) {
    for mut visibility in &mut root {
        visibility.set_if_neq(if screens.deploy { Visibility::Inherited } else { Visibility::Hidden });
    }
    let shown = screens.deploy || screens.map;
    for mut visibility in &mut bar {
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !shown {
        return;
    }
    let (game, sim) = (&game.0, &sim.0);
    let Some(player) = game.state.player(local.0) else {
        return;
    };
    for mut text in &mut words.p0() {
        set(&mut text, game.mode.cfg.name.to_uppercase());
    }
    for (mut text, mut color) in &mut words.p1() {
        set(&mut text, team_name(&sim.level, player.team));
        color.0 = looks.of(player.team).banner;
    }
    let tickets = game.mode.cfg.tickets.is_some();
    for (side, mut text, mut color) in &mut words.p2() {
        let left = game.state.teams.get(side.0 as usize).map_or(0, |t| t.tickets.max(0));
        // Each side's name on its own side of its number, so the two numbers sit together.
        let name = team_name(&sim.level, side.0);
        let line = if side.0 == 0 { format!("{name}  {left}") } else { format!("{left}  {name}") };
        set(&mut text, if tickets { line } else { String::new() });
        color.0 = looks.of(side.0).banner;
    }
    let elapsed = sim.tick.saturating_sub(game.state.started_at) as f32 * TICK_DT;
    let line = match (game.state.phase, game.mode.cfg.time_limit) {
        (Phase::Ended { .. }, _) => format!("Round {}  over", game.state.round),
        (Phase::Playing, Some(limit)) => format!("Round {}  {} left", game.state.round, clock(limit - elapsed)),
        (Phase::Playing, None) => format!("Round {}  {}", game.state.round, clock(elapsed)),
    };
    for mut text in &mut words.p3() {
        set(&mut text, line.clone());
    }
}

fn set(text: &mut Text, line: String) {
    if text.0 != line {
        text.0 = line;
    }
}

/// What one row says and how it stands.
struct RowLook {
    key: Option<String>,
    main: String,
    aside: String,
    sub: Option<String>,
    chosen: bool,
    /// Can be clicked for something now.
    open: bool,
    shown: bool,
}

impl RowLook {
    fn hidden() -> Self {
        Self { key: None, main: String::new(), aside: String::new(), sub: None, chosen: false, open: false, shown: false }
    }
}

/// What a kit carries, in a line.
fn carries(sim: &Sim, loadout: &prs_runtime::prs_sim::inventory::Loadout) -> String {
    let mut parts = vec![sim.weapons[loadout.primary].name.to_string()];
    if let Some(gun) = loadout.sidearm {
        parts.push(sim.weapons[gun].name.to_string());
    }
    for (count, one, many) in [(loadout.bandages, "bandage", "bandages"), (loadout.medkits, "medkit", "medkits"), (loadout.frags, "frag", "frags"), (loadout.smokes, "smoke", "smokes")] {
        if count > 0 {
            parts.push(format!("{count} {}", if count == 1 { one } else { many }));
        }
    }
    parts.join(", ")
}

/// Every row and small button: what it says, and how it looks.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_rows(
    screens: Res<Screens>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    sim: Res<SimResource>,
    names: Res<PlayerNames>,
    looks: Res<TeamLooks>,
    mut rows: Query<(&DeployButton, &Pointed, &mut BackgroundColor, &mut BorderColor, &mut Node, &Children), Without<RowKey>>,
    children: Query<&Children>,
    mut parts: Query<(&mut Text, &mut TextColor, &mut Node, Has<RowMain>, Has<RowAside>, Has<RowSub>, Has<RowKey>), Without<DeployButton>>,
) {
    if !screens.deploy {
        return;
    }
    let (game, sim) = (&game.0, &sim.0);
    let state = &game.state;
    let Some(player) = state.player(local.0) else {
        return;
    };
    let squads: Vec<_> = state.squads.iter().filter(|s| s.team == player.team).collect();
    let max_members = game.mode.cfg.squads.max_members;
    let mine = state.squad_of(local.0);
    let name_of = |soldier: usize| match state.player(soldier) {
        Some(_) if soldier == local.0 => "You".to_string(),
        Some(p) => player_name(&names, p.soldier, p.bot),
        None => String::new(),
    };
    for (button, pointed, mut background, mut border, mut node, kids) in &mut rows {
        let look = match button.0 {
            DeployAction::Team(team) => RowLook {
                key: None,
                main: team_name(&sim.level, team),
                aside: format!("{} players", state.team_size(team)),
                sub: None,
                chosen: player.team == team,
                open: player.team != team,
                shown: true,
            },
            DeployAction::Kit(kit) => match game.mode.cfg.kits.get(kit) {
                Some(def) => {
                    let taken = mine.map_or(0, |squad| squad.members.iter().filter(|&&m| m != local.0 && state.player(m).is_some_and(|p| p.kit == kit)).count());
                    let full = def.per_squad.is_some_and(|limit| taken as u32 >= limit);
                    let aside = match def.per_squad {
                        Some(_) if full => "FULL".to_string(),
                        Some(limit) => format!("{taken}/{limit}"),
                        None => String::new(),
                    };
                    let key = (kit < KIT_KEYS.len()).then(|| format!("{}", kit + 1));
                    RowLook { key, main: def.name.clone(), aside, sub: Some(carries(sim, &def.loadout)), chosen: player.kit == kit, open: !full, shown: true }
                }
                None => RowLook::hidden(),
            },
            DeployAction::Squad(nth) => match squads.get(nth) {
                Some(squad) => {
                    let yours = player.squad == Some(squad.id);
                    let leader = format!("SL {}", name_of(squad.leader));
                    let sub = if yours {
                        let others: Vec<String> = squad.members.iter().filter(|&&m| m != squad.leader).map(|&m| name_of(m)).collect();
                        if others.is_empty() { leader } else { format!("{leader}, {}", others.join(", ")) }
                    } else {
                        leader
                    };
                    let room = squad.members.len() < max_members;
                    RowLook {
                        key: None,
                        main: format!("Squad {}", squad.id + 1),
                        aside: format!("{}/{max_members}", squad.members.len()),
                        sub: Some(sub),
                        chosen: yours,
                        open: !yours && room,
                        shown: true,
                    }
                }
                None => RowLook::hidden(),
            },
            DeployAction::NewSquad => RowLook { main: "New".into(), open: true, shown: true, ..RowLook::hidden() },
            DeployAction::LeaveSquad => RowLook { main: "Leave".into(), open: true, shown: player.squad.is_some(), ..RowLook::hidden() },
            // The map's marks and the way in are looked after elsewhere.
            _ => continue,
        };
        let display = if look.shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let color = match (look.open || look.chosen, pointed.pressed && look.open, pointed.hovered && look.open, look.chosen) {
            (false, ..) => ROW_OFF,
            (_, true, ..) => ROW_PRESSED,
            (_, _, true, _) => ROW_HOVER,
            (.., true) => ROW_CHOSEN,
            _ => ROW,
        };
        background.set_if_neq(BackgroundColor(color));
        let accent = if look.chosen { looks.of(player.team).banner } else { Color::NONE };
        if border.left != accent {
            *border = BorderColor::all(accent);
        }
        let main_color = if look.open || look.chosen { INK } else { INK_FAINT };
        for entity in kids.iter().flat_map(|kid| std::iter::once(kid).chain(children.get(kid).into_iter().flatten().copied())) {
            let Ok((mut text, mut color, mut node, is_main, is_aside, is_sub, is_key)) = parts.get_mut(entity) else { continue };
            let (line, shown) = match (is_main, is_aside, is_sub, is_key) {
                (true, ..) => (look.main.clone(), true),
                (_, true, ..) => (look.aside.clone(), true),
                (_, _, true, _) => (look.sub.clone().unwrap_or_default(), look.sub.is_some()),
                (.., true) => (look.key.clone().unwrap_or_default(), look.key.is_some()),
                _ => continue,
            };
            set(&mut text, line);
            let display = if shown { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
            if is_main && color.0 != main_color {
                color.0 = main_color;
            }
        }
    }
}

/// The button that gets you in, the way back to the fight, and where you will come in.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_way_in(
    screens: Res<Screens>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    sim: Res<SimResource>,
    mut buttons: Query<(&DeployButton, &Pointed, &mut BackgroundColor, &mut Node, &Children)>,
    mut parts: Query<(&mut Text, &mut Node, Has<RowMain>), (Without<DeployButton>, Or<(With<RowMain>, With<RowSub>)>)>,
    mut lines: ParamSet<(Query<&mut Text, (With<SpawnLine>, Without<RowMain>, Without<RowSub>)>, Query<&mut Text, (With<Hints>, Without<RowMain>, Without<RowSub>)>)>,
) {
    if !screens.deploy {
        return;
    }
    let (game, sim) = (&game.0, &sim.0);
    let Some(player) = game.state.player(local.0) else {
        return;
    };
    let on_field = player.deployed && !sim.soldiers[local.0].health.is_dead();
    let left = seconds_to_go(game, player, sim.tick);
    for (button, pointed, mut background, mut node, kids) in &mut buttons {
        let (main, sub, shown, color) = match button.0 {
            DeployAction::Deploy => {
                let (main, sub, rest, hover) = match (player.ready, left) {
                    (true, 0) => ("DEPLOYING".to_string(), "", READY, READY),
                    (true, left) => (format!("READY  {left}"), "click to cancel", READY, WAITING_HOVER),
                    (false, 0) => ("DEPLOY".to_string(), "", GO, GO_HOVER),
                    (false, left) => (format!("DEPLOY IN {left}"), "click to go when it is time", WAITING, WAITING_HOVER),
                };
                let color = if pointed.pressed { READY } else if pointed.hovered { hover } else { rest };
                (main, sub.to_string(), !on_field, color)
            }
            DeployAction::Close => {
                let color = if pointed.hovered { WAITING_HOVER } else { WAITING };
                ("BACK TO THE FIGHT".to_string(), "kit and squad count from your next spawn".to_string(), screens.deploy_asked, color)
            }
            _ => continue,
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        background.set_if_neq(BackgroundColor(color));
        for &kid in kids {
            let Ok((mut text, mut node, is_main)) = parts.get_mut(kid) else { continue };
            if is_main {
                set(&mut text, main.clone());
            } else {
                set(&mut text, sub.clone());
                let display = if sub.is_empty() { Display::None } else { Display::Flex };
                if node.display != display {
                    node.display = display;
                }
            }
        }
    }
    let rally = game.state.squad_of(local.0).and_then(|s| s.rally);
    let at = match (player.spawn, rally) {
        (SpawnAt::Rally, Some(rally)) => format!("Spawning at your rally point, {} left", rally.spawns_left),
        _ => "Spawning at the main base".to_string(),
    };
    let choose = if rally.is_some() { ". Click the map to change." } else { "" };
    for mut text in &mut lines.p0() {
        set(&mut text, format!("{at}{choose}"));
    }
    let kits = game.mode.cfg.kits.len().min(KIT_KEYS.len());
    let keys = if kits > 1 { format!("1-{kits} kit   ") } else { String::new() };
    for mut text in &mut lines.p1() {
        set(&mut text, format!("ENTER deploy   {keys}WHEEL scroll   ESC menu"));
    }
}
