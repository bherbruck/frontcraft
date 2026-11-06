//! What someone at a helicopter's controls gets on the screen, on top of the gauges in front of them: a
//! small mark for where the stick is, because it is otherwise only in your hand, and the readings in
//! plain numbers.

use bevy::prelude::*;
use prs_runtime::prs_sim::helicopter::{Engine, Reading};
use prs_runtime::prs_sim::{Helicopter, PILOT_SEAT, heli_of};
use prs_runtime::{LocalView, PendingInput, SimResource};

const RADAR_REACH: f32 = 500.0;
const STICK_BOX: f32 = 52.0;
const STICK_DOT: f32 = 6.0;
const FAINT: Color = Color::srgba(0.85, 0.88, 0.82, 0.28);
const INK: Color = Color::srgba(0.9, 0.93, 0.86, 0.72);
const WARN: Color = Color::srgba(0.95, 0.72, 0.3, 0.95);
/// Rotor speed below which it is sagging and you should hear about it.
const ROTOR_LOW: f32 = 0.9;
const ROTOR_HIGH: f32 = 1.08;

pub struct CockpitPlugin;

impl Plugin for CockpitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel).add_systems(Update, update_panel);
    }
}

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct StickDot;
#[derive(Component)]
struct Numbers;
#[derive(Component)]
struct Warning;

fn spawn_panel(mut commands: Commands) {
    let font = |size: f32| TextFont { font_size: FontSize::Px(size), ..default() };
    commands
        .spawn((
            Panel,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(52),
                left: px(28),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(6),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn((Warning, Text::new(""), font(16.0), TextColor(WARN)));
            panel
                .spawn((
                    Node { width: px(STICK_BOX), height: px(STICK_BOX), border: UiRect::all(px(1)), ..default() },
                    BorderColor::all(FAINT),
                ))
                .with_children(|square| {
                    square.spawn((
                        StickDot,
                        Node {
                            position_type: PositionType::Absolute,
                            width: px(STICK_DOT),
                            height: px(STICK_DOT),
                            left: px((STICK_BOX - STICK_DOT) * 0.5),
                            top: px((STICK_BOX - STICK_DOT) * 0.5),
                            ..default()
                        },
                        BackgroundColor(INK),
                    ));
                });
            panel.spawn((Numbers, Text::new(""), font(14.0), TextColor(INK)));
        });
}

/// What needs saying to the pilot, the worst first.
fn warning(heli: &Helicopter, reading: &Reading) -> &'static str {
    let state = &heli.state;
    if state.engine == Engine::Dead {
        "ENGINE OUT - COLLECTIVE DOWN"
    } else if state.engine == Engine::Off {
        "SPACE TO START ENGINE"
    } else if state.engine == Engine::Starting {
        "ENGINE STARTING"
    } else if state.health.tail <= 0.0 {
        "TAIL ROTOR GONE"
    } else if reading.rotor < ROTOR_LOW {
        "ROTOR LOW"
    } else if reading.rotor > ROTOR_HIGH {
        "ROTOR HIGH"
    } else if state.vortex > 0.3 {
        "SINKING IN OWN WAKE - FLY OUT"
    } else {
        ""
    }
}

#[allow(clippy::type_complexity)]
fn update_panel(
    view: Res<LocalView>,
    sim: Res<SimResource>,
    input: Res<PendingInput>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut dot: Query<&mut Node, With<StickDot>>,
    mut numbers: Query<&mut Text, (With<Numbers>, Without<Warning>)>,
    mut warnings: Query<&mut Text, (With<Warning>, Without<Numbers>)>,
) {
    let sim = &sim.0;
    let heli = view.blended.seat.filter(|at| at.seat as usize == PILOT_SEAT).and_then(|at| heli_of(at.carrier)).and_then(|h| sim.helicopters.get(h));
    let wanted = if heli.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    for mut visible in &mut panel {
        visible.set_if_neq(wanted);
    }
    let Some(heli) = heli else {
        return;
    };
    let state = &heli.state;
    let cfg = &sim.heli_cfg;
    let below = sim.world.raycast(state.pos, Vec3::NEG_Y, RADAR_REACH).map_or(RADAR_REACH, |hit| hit.distance);
    let reading = Reading::of(state, cfg, (below + cfg.gear.points[0].y).max(0.0));
    for mut node in &mut dot {
        let room = (STICK_BOX - STICK_DOT) * 0.5;
        node.left = px(room + input.stick.pos.x * room);
        node.top = px(room - input.stick.pos.y * room);
    }
    let dead = state.health.cockpit <= 0.0;
    for mut text in &mut numbers {
        text.0 = if dead {
            "INSTRUMENTS OUT".into()
        } else {
            format!(
                "SPD {:.0}  ALT {:.0}  VS {:+.1}\nROTOR {:.0}%  TQ {:.0}%",
                reading.airspeed,
                reading.height,
                reading.climb,
                reading.rotor * 100.0,
                reading.power * 100.0
            )
        };
    }
    for mut text in &mut warnings {
        text.0 = warning(heli, &reading).into();
    }
}
