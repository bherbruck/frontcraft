//! What the mode shows you while you are playing: how the zone you are in stands, a line when something
//! happens that concerns you. Also who you are looking at, if they are a friend.
//!
//! You cannot tell friend from foe by a low-poly body at a distance, and you should not be able to
//! read the whole battle from your HUD either. So a friend gets a small team-coloured mark only when
//! you are looking almost straight at them, nearer for squadmates, with a clear line to them. It
//! answers "is that one of ours?" at the moment you are about to shoot, and says nothing about anyone
//! you are not already looking at. Enemies never get one, so no mark means nothing either way.

use bevy::prelude::*;
use prs_models::TeamLooks;
use prs_runtime::prs_mode::{ModeEvent, RallyLoss, Refusal, ZoneStatus};
use prs_runtime::prs_sim::Sim;
use prs_runtime::prs_sim::protocol::TICK_DT;
use prs_runtime::prs_sim::health::Life;
use prs_runtime::{GameMode, LocalSoldier, LocalView, ModeMessage, SimResource};
use prs_view::WorldCamera;

use crate::deploy::Screens;
use crate::{INK, team_name};

const NOTICE_SECONDS: f32 = 5.0;
const WARNING: Color = Color::srgb(1.0, 0.45, 0.3);
const SHADOW: TextShadow = TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) };
const BAR_WIDTH: f32 = 260.0;
const MARK_SIZE: f32 = 7.0;
/// Metres above the feet that a mark sits, for someone on their feet.
const MARK_ABOVE_EYE: f32 = 0.5;
/// Share of the cone over which a mark fades in.
const MARK_FADE: f32 = 0.3;
/// How wide a soldier counts as for being looked at, in metres. Up close they fill the view, and
/// aiming anywhere on them counts, not just near the mark over their head.
const BODY_RADIUS: f32 = 0.35;
/// Points along a soldier, feet to mark, tried for the one nearest where you are looking.
const BODY_SAMPLES: usize = 8;

pub(crate) struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_field).add_systems(Update, (update_zone, update_base_warning, update_notice, update_marks).after(crate::deploy::ScreensDecided));
    }
}

#[derive(Component)]
struct ZoneText;
#[derive(Component)]
struct ZoneBar;
#[derive(Component)]
struct ZoneFill;
#[derive(Component)]
struct Notice;
#[derive(Component)]
struct BaseWarning;
#[derive(Component)]
struct FriendMark;
#[derive(Component)]
struct FriendDown;

fn spawn_field(mut commands: Commands) {
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: px(22),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(5),
            ..default()
        })
        .with_children(|top| {
            top.spawn((ZoneText, Text::new(""), font(17.0), TextColor(INK), SHADOW));
            top.spawn((
                ZoneBar,
                Visibility::Hidden,
                Node { width: px(BAR_WIDTH), height: px(6), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ))
            .with_child((ZoneFill, Node { height: percent(100), width: percent(0), ..default() }, BackgroundColor(INK)));
            top.spawn((Notice, Text::new(""), font(15.0), TextColor(INK), SHADOW));
        });
    commands.spawn((
        BaseWarning,
        Text::new(""),
        font(24.0),
        TextColor(WARNING),
        SHADOW,
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, width: percent(100), top: percent(30), ..default() },
    ));
}

fn update_zone(
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    screens: Res<Screens>,
    looks: Res<TeamLooks>,
    mut text: Query<(&mut Text, &mut TextColor), With<ZoneText>>,
    mut bar: Query<&mut Visibility, With<ZoneBar>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), With<ZoneFill>>,
) {
    let game = &game.0;
    let player = game.state.player(local.0);
    let inside = player.filter(|_| view.blended.life == Life::Up && !screens.deploy && !game.state.ended()).and_then(|p| {
        game.mode.objective.zone_status(&sim.0.level, &game.state, &sim.0.soldiers[p.soldier])
    });
    let (Ok((mut text, mut color)), Ok(mut bar), Ok((mut fill, mut fill_color))) = (text.single_mut(), bar.single_mut(), fill.single_mut()) else {
        return;
    };
    bar.set_if_neq(if inside.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    let Some((index, status)) = inside else {
        if !text.0.is_empty() {
            text.0.clear();
        }
        return;
    };
    let point = &game.state.points[index];
    let doing = match status {
        ZoneStatus::Held => "Held",
        ZoneStatus::Taking => "Taking",
        ZoneStatus::Neutralising => "Bringing it down",
        ZoneStatus::Losing => "Losing it",
        ZoneStatus::Contested => "Contested",
        ZoneStatus::NotNext => "Not the next point",
        ZoneStatus::TooFew => "Not enough of you",
        ZoneStatus::NotCounted => "You do not count from here",
    };
    let line = format!("{}: {doing}", sim.0.level.capture_points[index].name);
    if text.0 != line {
        text.0 = line;
    }
    let tint = point.claimant.map_or(INK, |team| looks.of(team).banner);
    color.0 = INK;
    fill_color.0 = tint;
    fill.width = percent(point.progress_at(sim.0.tick) * 100.0);
}

/// The server has seen you go into the other side's main base, and when the clock runs out it will kill you.
fn update_base_warning(game: Res<GameMode>, sim: Res<SimResource>, local: Res<LocalSoldier>, mut text: Query<&mut Text, With<BaseWarning>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let game = &game.0;
    let since = game.state.player(local.0).and_then(|p| p.trespass_since);
    let line = match (since, game.mode.cfg.base_protection.as_ref()) {
        (Some(since), Some(rules)) => {
            let spent = sim.0.tick.saturating_sub(since) as f32 * TICK_DT;
            format!("ENEMY MAIN BASE\nGet out: {:.0}", (rules.warning_seconds - spent).max(0.0))
        }
        _ => String::new(),
    };
    if text.0 != line {
        text.0 = line;
    }
}

/// What it says when the mode says no.
fn refusal_line(why: Refusal) -> &'static str {
    match why {
        Refusal::RoundOver => "The round is over",
        Refusal::NoSuchKit => "No such kit",
        Refusal::KitTaken => "Your squad already has enough of that kit",
        Refusal::NoRally => "Your squad has no rally point",
        Refusal::NoRallyRules => "No rally points in this mode",
        Refusal::SquadFull => "That squad is full",
        Refusal::NoSuchSquad => "That squad has gone",
        Refusal::TooManySquads => "Your side has all the squads it can have",
        Refusal::OtherTeam => "That squad is on the other side",
        Refusal::InTheField => "Not while you are in the field",
        Refusal::TeamsUneven => "That would make the sides too uneven",
        Refusal::NotLeader => "Only a squad leader can place a rally point",
        Refusal::NotUp => "Not in the state you are in",
        Refusal::NotOnGround => "Get on the ground first",
        Refusal::RallyCooling => "Too soon to place another rally point",
        Refusal::NoSquadmateNear => "A rally point needs a squadmate with you",
        Refusal::EnemyNear => "Too near the enemy for a rally point",
        Refusal::TooCloseToBase => "Too near your base for a rally point",
        Refusal::InAZone => "Not in a capture zone",
        Refusal::NoTickets => "Your side cannot afford a rally point",
        Refusal::NotDownLongEnough => "Hold on a little longer",
    }
}

/// What this event has to say to this soldier, if anything.
fn notice_for(event: &ModeEvent, sim: &Sim, you: usize, your_squad: Option<u32>) -> Option<String> {
    match *event {
        ModeEvent::Refused { soldier, why } if soldier == you => Some(refusal_line(why).to_string()),
        ModeEvent::PointCaptured { point, team } => {
            let zone = sim.level.capture_points.get(point)?;
            Some(format!("{} has taken {}", team_name(&sim.level, team), zone.name))
        }
        ModeEvent::PointNeutralised { point, by } => {
            let zone = sim.level.capture_points.get(point)?;
            Some(format!("{} has brought down {}", team_name(&sim.level, by), zone.name))
        }
        ModeEvent::TeamKill { killer, victim } if killer == you => Some(format!("You hit a friend, soldier {victim}")),
        ModeEvent::TeamKill { killer, victim } if victim == you => Some(format!("Hit by a friend, soldier {killer}")),
        ModeEvent::RallyPlaced { squad } if your_squad == Some(squad) => Some("Rally point placed".into()),
        ModeEvent::RallyLost { squad, why } if your_squad == Some(squad) => match why {
            RallyLoss::Overrun => Some("Your rally point has been overrun".into()),
            RallyLoss::Expired => Some("Your rally point has gone stale".into()),
            RallyLoss::UsedUp => Some("Your rally point is used up".into()),
            RallyLoss::Replaced | RallyLoss::SquadGone => None,
        },
        _ => None,
    }
}

fn update_notice(
    mut messages: MessageReader<ModeMessage>,
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    time: Res<Time>,
    mut left: Local<f32>,
    mut text: Query<&mut Text, With<Notice>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let squad = game.0.state.player(local.0).and_then(|p| p.squad);
    for ModeMessage(event) in messages.read() {
        if let Some(line) = notice_for(event, &sim.0, local.0, squad) {
            text.0 = line;
            *left = NOTICE_SECONDS;
        }
    }
    *left -= time.delta_secs();
    if *left <= 0.0 && !text.0.is_empty() {
        text.0.clear();
    }
}

/// See the top of this file for why it is done this way.
#[allow(clippy::too_many_arguments)]
fn update_marks(
    mut commands: Commands,
    game: Res<GameMode>,
    sim: Res<SimResource>,
    local: Res<LocalSoldier>,
    view: Res<LocalView>,
    screens: Res<Screens>,
    looks: Res<TeamLooks>,
    scale: Res<UiScale>,
    cameras: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    mut marks: Query<(&mut Node, &mut BackgroundColor, &mut Visibility, &Children), With<FriendMark>>,
    mut downs: Query<&mut Visibility, (With<FriendDown>, Without<FriendMark>)>,
) {
    let game = &game.0;
    let sim = &sim.0;
    let (Ok((camera, transform)), Some(rules), Some(me)) = (cameras.single(), game.mode.cfg.identify.as_ref(), game.state.player(local.0)) else {
        return;
    };
    let eye = transform.translation();
    let forward = transform.forward().as_vec3();
    let your_squad = me.squad;
    let looking = view.blended.life == Life::Up && !screens.deploy && !screens.map && !screens.board;
    let mut wanted = Vec::new();
    if looking {
        for player in game.state.players.iter().filter(|p| p.team == me.team && p.soldier != local.0) {
            let soldier = &sim.soldiers[player.soldier];
            if !soldier.present || soldier.health.is_dead() {
                continue;
            }
            let down = soldier.health.life == Life::Down;
            let top = soldier.body.pos + Vec3::Y * if down { MARK_ABOVE_EYE } else { soldier.body.eye_height + MARK_ABOVE_EYE };
            let distance = (top - eye).length();
            if distance > rules.range || distance < 0.5 {
                continue;
            }
            let cone = if player.squad.is_some() && player.squad == your_squad { rules.squad_cone_degrees } else { rules.cone_degrees };
            // Whichever part of them is nearest the middle of the screen, less how wide they look from here.
            let (angle, seen) = (0..=BODY_SAMPLES)
                .map(|i| soldier.body.pos.lerp(top, i as f32 / BODY_SAMPLES as f32))
                .map(|point| {
                    let to = point - eye;
                    let far = to.length().max(1e-3);
                    let wide = BODY_RADIUS.atan2(far).to_degrees();
                    ((forward.angle_between(to / far).to_degrees() - wide).max(0.0), point)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .expect("at least one point");
            if angle > cone {
                continue;
            }
            // Only someone you could actually see.
            let to = seen - eye;
            if sim.world.raycast(eye, to.normalize_or_zero(), (to.length() - 0.3).max(0.0)).is_some() {
                continue;
            }
            let Ok(at) = camera.world_to_viewport(transform, top) else { continue };
            let at = crate::scale::to_hud(at, &scale);
            let fade = ((cone - angle) / (cone * MARK_FADE).max(1e-3)).clamp(0.0, 1.0);
            wanted.push((at, fade, down, looks.of(player.team).banner));
        }
    }
    let have = marks.iter().count();
    for _ in have..wanted.len() {
        commands
            .spawn((
                FriendMark,
                Visibility::Hidden,
                UiTransform::from_rotation(Rot2::FRAC_PI_4),
                Node { position_type: PositionType::Absolute, width: px(MARK_SIZE), height: px(MARK_SIZE), margin: UiRect::all(px(-MARK_SIZE * 0.5)), ..default() },
                BackgroundColor(INK),
            ))
            .with_child((
                FriendDown,
                Visibility::Hidden,
                UiTransform::from_rotation(Rot2::radians(-std::f32::consts::FRAC_PI_4)),
                Text::new("+"),
                TextFont { font_size: FontSize::Px(14.0), ..default() },
                TextColor(Color::WHITE),
                Node { position_type: PositionType::Absolute, top: px(-14), left: px(-1), ..default() },
            ));
    }
    let mut wanted = wanted.into_iter();
    for (mut node, mut background, mut visibility, children) in &mut marks {
        let Some((at, fade, down, color)) = wanted.next() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        node.left = px(at.x);
        node.top = px(at.y);
        background.0 = color.with_alpha(0.85 * fade);
        for &child in children {
            if let Ok(mut plus) = downs.get_mut(child) {
                plus.set_if_neq(if down { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
}
