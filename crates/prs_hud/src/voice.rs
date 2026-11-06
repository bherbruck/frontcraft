//! Who is talking, a line when voice can't do what was asked, and the rows in the menu for muting.

use bevy::prelude::*;
use prs_runtime::prs_sim::SoldierId;
use prs_runtime::{GameMode, LocalSoldier, PlayerNames, VoiceChannel, VoiceLink};
use prs_voice::{VoiceTuning, Voices};

use crate::{INK, Intro, Setting, menu_row, player_name};

pub(crate) struct VoiceHudPlugin;

impl Plugin for VoiceHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_talkers).add_systems(Update, (show_talkers, list_for_muting));
    }
}

#[derive(Component)]
struct TalkerList;

/// Holds a row for each player who can be muted. Inside the menu, under its settings.
#[derive(Component)]
pub(crate) struct MuteList;

fn spawn_talkers(mut commands: Commands) {
    commands.spawn((
        TalkerList,
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(INK),
        Node { position_type: PositionType::Absolute, left: px(16), top: percent(30), ..default() },
    ));
}

fn channel_name(channel: VoiceChannel) -> &'static str {
    match channel {
        VoiceChannel::Local => "near you",
        VoiceChannel::Squad => "squad radio",
    }
}

/// What the list of who is talking says. Everyone by the number they have on the scoreboard.
fn talker_lines(voices: &Voices, names: &PlayerNames) -> String {
    let mut text = String::new();
    if let Some(channel) = voices.talking {
        text.push_str(&format!("You are talking: {}\n", channel_name(channel)));
    }
    for talker in voices.talking_now() {
        let name = player_name(names, talker.player as usize, false);
        text.push_str(&format!("{name}: {}\n", channel_name(talker.channel)));
    }
    if let Some(notice) = voices.notice() {
        text.push_str(notice);
    }
    text
}

fn show_talkers(voices: Res<Voices>, names: Res<PlayerNames>, mut list: Query<&mut Text, With<TalkerList>>) {
    let lines = talker_lines(&voices, &names);
    for mut text in &mut list {
        if text.0 != lines {
            text.0 = lines.clone();
        }
    }
}

/// Everyone else with a person behind them, whoever was heard last at the top, then in order.
fn who_to_list(players: impl Iterator<Item = u16>, voices: &Voices, most: usize) -> Vec<u16> {
    let mut who: Vec<u16> = players.collect();
    who.sort_by(|a, b| {
        let (heard_a, heard_b) = (voices.last_heard(*a), voices.last_heard(*b));
        heard_b.partial_cmp(&heard_a).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b))
    });
    who.truncate(most);
    who
}

/// Rows only change as the menu opens or someone comes or goes, so that nothing moves under the mouse.
#[allow(clippy::too_many_arguments)]
fn list_for_muting(
    mut commands: Commands,
    tuning: Res<VoiceTuning>,
    voices: Res<Voices>,
    game: Res<GameMode>,
    local: Res<LocalSoldier>,
    names: Res<PlayerNames>,
    link: Option<Res<VoiceLink>>,
    intro: Query<Ref<Visibility>, With<Intro>>,
    list: Query<Entity, With<MuteList>>,
    mut listed: Local<Vec<u16>>,
) {
    let Ok(visibility) = intro.single() else { return };
    let Ok(list) = list.single() else { return };
    if *visibility == Visibility::Hidden {
        return;
    }
    let allowed = link.is_some_and(|link| link.allowed);
    let humans = game.0.state.players.iter().filter(|p| allowed && !p.bot && p.soldier != local.0 as SoldierId);
    let who = who_to_list(humans.map(|p| p.soldier as u16), &voices, tuning.mute_rows);
    let mut same_people = who.clone();
    same_people.sort_unstable();
    let mut before = listed.clone();
    before.sort_unstable();
    if same_people == before && !visibility.is_changed() {
        return;
    }
    // The same row again, if only a name has changed.
    if same_people == before && !visibility.is_changed() && !names.is_changed() {
        return;
    }
    *listed = who.clone();
    commands.entity(list).despawn_children().with_children(|rows| {
        for player in who {
            menu_row(rows, &format!("Mute {}", player_name(&names, player as usize, false)), Setting::Mute(player));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_talking_and_nothing_to_say_shows_nothing() {
        assert_eq!(talker_lines(&Voices::default(), &PlayerNames::default()), "");
    }

    #[test]
    fn talking_says_which_way() {
        let mut voices = Voices::default();
        voices.talking = Some(VoiceChannel::Squad);
        assert_eq!(talker_lines(&voices, &PlayerNames::default()), "You are talking: squad radio\n");
    }

    #[test]
    fn the_list_of_who_to_mute_stops_at_its_limit() {
        assert_eq!(who_to_list([9, 3, 5].into_iter(), &Voices::default(), 2), [3, 5]);
    }
}
