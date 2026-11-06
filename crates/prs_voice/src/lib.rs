//! Talking to other players: out loud for whoever is near, and over the radio to your squad. The
//! server says who gets what and this plays what comes, so all of it stays out of the sim, which never
//! hears of any of it. See `prs_net::server::VoiceConfig` for what a server lets through.

mod backend;

use bevy::prelude::*;
use prs_audio::{Acoustics, AudioLevels, Carry, ear_position};
use prs_runtime::{GameMode, LocalView, SimResource, VoiceChannel, VoiceLink, VoiceUp};

pub use backend::Mic;

/// What talks on what, as long as it is held, and all of it in one place. Not sim buttons, as nothing
/// here is any of the sim's business. Caps Lock has rules of its own: see [`CapsLockHold`].
pub const TALK_KEYS: [(KeyCode, VoiceChannel); 3] = [
    (KeyCode::CapsLock, VoiceChannel::Local),
    (KeyCode::KeyH, VoiceChannel::Local),
    (KeyCode::KeyY, VoiceChannel::Squad),
];
/// Mouse 4 and 5. `web/index.html` stops them being the browser's back and forward.
pub const TALK_BUTTONS: [(MouseButton, VoiceChannel); 2] = [
    (MouseButton::Back, VoiceChannel::Local),
    (MouseButton::Forward, VoiceChannel::Squad),
];
/// With more than one held, the first of these that is goes.
const CHANNELS_FIRST_TO_LAST: [VoiceChannel; 2] = [VoiceChannel::Squad, VoiceChannel::Local];

/// The channel being held down, if any. `caps_lock` is whether Caps Lock counts just now.
fn held_channel(keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>, caps_lock: bool) -> Option<VoiceChannel> {
    let key_held = |key: KeyCode| keys.pressed(key) && (key != KeyCode::CapsLock || caps_lock);
    CHANNELS_FIRST_TO_LAST.into_iter().find(|channel| {
        TALK_KEYS.iter().any(|(key, on)| on == channel && key_held(*key)) || TALK_BUTTONS.iter().any(|(button, on)| on == channel && buttons.pressed(*button))
    })
}

/// Caps Lock is the one key that can look held when nobody is holding it. On a Mac it goes down at one
/// press and comes up at the next, and elsewhere its release has been known to go missing. A microphone
/// must not stay open on that, so holding it only counts for so long, and then only a new press will do.
#[derive(Default)]
struct CapsLockHold {
    since: Option<f32>,
}

impl CapsLockHold {
    /// Whether it counts as held now.
    fn update(&mut self, down: bool, just_pressed: bool, now: f32, longest: f32) -> bool {
        if just_pressed {
            self.since = Some(now);
        }
        if !down {
            self.since = None;
        }
        self.since.is_some_and(|since| now - since < longest)
    }
}

#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoiceTuning {
    /// Opus, in bits a second, and how much speech goes in each frame. Longer frames cost fewer bytes
    /// on the wire and add their length to the delay. Opus takes 10, 20, 40 and 60.
    pub bitrate: u32,
    pub frame_ms: u32,
    /// How far ahead a voice is played, to ride out frames arriving unevenly, and how far behind it can
    /// fall before frames are thrown away to catch up.
    pub jitter_ms: f32,
    pub most_behind_ms: f32,
    /// How much of our own talking can wait to be sent, in milliseconds. A machine drawing few frames a
    /// second sends rarely, and anything more than this waiting is thrown away, oldest first.
    pub send_buffer_ms: f32,
    /// How a voice fades with distance, and in metres where it fades to nothing. A server sends none
    /// past its own `voice_heard`, so this is best kept inside that.
    pub carry: Carry,
    pub fades_out: (f32, f32),
    /// What gets through the radio, in hertz, and how loud it is.
    pub radio_band_hz: (f32, f32),
    pub radio_gain: f32,
    /// Seconds between working out what walls do to each voice, which means casting rays.
    pub shape_every: f32,
    /// How long a name stays up after the last of them is heard, and a message from here.
    pub shown_for: f32,
    pub notice_for: f32,
    /// A voice with nothing new after this many seconds gives up its decoder.
    pub forget_stream_after: f32,
    /// Rows of players to mute in the menu, the last ones heard first.
    pub mute_rows: usize,
    /// How much each press of the volume buttons changes it.
    pub volume_step: f32,
    /// The longest Caps Lock can hold the microphone open before it has to be pressed again, in seconds.
    pub caps_lock_longest_hold: f32,
}

impl Default for VoiceTuning {
    fn default() -> Self {
        Self {
            bitrate: 16_000,
            frame_ms: 40,
            jitter_ms: 60.0,
            most_behind_ms: 300.0,
            send_buffer_ms: 500.0,
            carry: Carry { loudness: 1.0, reach: 12.0 },
            fades_out: (55.0, 75.0),
            radio_band_hz: (300.0, 3400.0),
            radio_gain: 0.9,
            shape_every: 0.1,
            shown_for: 0.6,
            notice_for: 4.0,
            forget_stream_after: 5.0,
            mute_rows: 6,
            volume_step: 0.1,
            caps_lock_longest_hold: 15.0,
        }
    }
}

/// The volume of a voice at this distance, before what is in the way.
pub fn voice_volume(cfg: &VoiceTuning, distance: f32) -> f32 {
    let (from, to) = cfg.fades_out;
    cfg.carry.volume(distance) * ((to - distance) / (to - from).max(1e-3)).clamp(0.0, 1.0)
}

/// What a player has chosen, kept with their other settings. The microphone starts on, but nothing is
/// captured until they press to talk, and the browser asks them before the first time.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct VoiceSettings {
    mic_on: bool,
    volume: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self { mic_on: true, volume: 1.0 }
    }
}

impl VoiceSettings {
    pub fn mic_on(&self) -> bool {
        self.mic_on
    }

    pub fn set_mic_on(&mut self, on: bool) {
        self.mic_on = on;
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }
}

/// Someone heard lately.
#[derive(Clone, Copy, Debug)]
pub struct Talker {
    pub player: u16,
    pub channel: VoiceChannel,
    /// Where they were when last heard, out loud.
    at: Option<Vec3>,
    /// When they were last heard.
    pub heard_at: f32,
    next_shape: f32,
}

/// Who is talking, for showing. Mutes are here too, and last as long as the connection does: a player
/// number means nothing after that, there being no accounts.
#[derive(Resource, Default)]
pub struct Voices {
    talkers: Vec<Talker>,
    muted: Vec<u16>,
    /// Which way we are talking.
    pub talking: Option<VoiceChannel>,
    notice: Option<(&'static str, f32)>,
    /// When it was last looked at, so that those showing need no clock of their own.
    now: f32,
    shown_for: f32,
}

impl Voices {
    pub fn is_muted(&self, player: u16) -> bool {
        self.muted.contains(&player)
    }

    pub fn set_muted(&mut self, player: u16, muted: bool) {
        self.muted.retain(|&p| p != player);
        if muted {
            self.muted.push(player);
        }
    }

    /// Those talking right now.
    pub fn talking_now(&self) -> impl Iterator<Item = &Talker> {
        self.talkers.iter().filter(|t| self.now - t.heard_at < self.shown_for)
    }

    /// When someone was last heard, if lately.
    pub fn last_heard(&self, player: u16) -> Option<f32> {
        self.talkers.iter().filter(|t| t.player == player).map(|t| t.heard_at).reduce(f32::max)
    }

    /// Something the player needs to be told, while it lasts.
    pub fn notice(&self) -> Option<&'static str> {
        self.notice.filter(|(_, until)| self.now < *until).map(|(text, _)| text)
    }
}

pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        let tuning = prs_mods::tuned("voice", VoiceTuning::default());
        backend::configure(&tuning);
        app.insert_resource(Voices { shown_for: tuning.shown_for, ..default() })
            .insert_resource(tuning)
            .init_resource::<VoiceSettings>()
            .add_systems(Update, (push_to_talk, send_and_receive, shape_voices).chain());
    }
}

/// What a press of push to talk means right now, if anything is in the way of it.
fn why_not_talking(settings: &VoiceSettings, allowed: Option<bool>) -> Option<&'static str> {
    if !backend::SUPPORTED {
        return Some("Voice only works in the browser for now");
    }
    match allowed {
        None => return Some("Voice needs a server"),
        Some(false) => return Some("This server has voice turned off"),
        Some(true) => {}
    }
    (!settings.mic_on).then_some("The microphone is off. Turn it on in the ESC menu")
}

fn mic_notice(mic: Mic) -> Option<&'static str> {
    match mic {
        Mic::Asking => Some("Waiting for the browser to allow the microphone"),
        Mic::Refused => Some("The browser refused the microphone"),
        Mic::Missing => Some("No microphone: it takes one, and a page on https"),
        Mic::NoCodec => Some("This browser can't encode voice, though you can still hear it"),
        Mic::Idle | Mic::Live => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_to_talk(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    settings: Res<VoiceSettings>,
    tuning: Res<VoiceTuning>,
    link: Option<Res<VoiceLink>>,
    mut voices: ResMut<Voices>,
    mut was_on: Local<bool>,
    mut last_mic: Local<Option<Mic>>,
    mut caps_lock: Local<CapsLockHold>,
) {
    let now = time.elapsed_secs();
    voices.now = now;
    if *was_on && !settings.mic_on {
        backend::close_mic();
        voices.talking = None;
    }
    *was_on = settings.mic_on;

    let caps_down = keys.pressed(KeyCode::CapsLock);
    let caps_pressed = keys.just_pressed(KeyCode::CapsLock);
    let caps_was_held = caps_lock.since.is_some_and(|since| now - since < tuning.caps_lock_longest_hold);
    let caps_held = caps_lock.update(caps_down, caps_pressed, now, tuning.caps_lock_longest_hold);
    let caps_counts = caps_held && backend::caps_lock_can_be_held();
    let wanted = held_channel(&keys, &buttons, caps_counts);
    let in_the_way = why_not_talking(&settings, link.as_ref().map(|link| link.allowed));
    let pressed = keys.get_just_pressed().any(|key| TALK_KEYS.iter().any(|(talk, _)| talk == key))
        || buttons.get_just_pressed().any(|button| TALK_BUTTONS.iter().any(|(talk, _)| talk == button));
    if pressed {
        if let Some(reason) = in_the_way {
            voices.notice = Some((reason, now + tuning.notice_for));
        } else if caps_pressed && !backend::caps_lock_can_be_held() {
            voices.notice = Some(("Caps Lock can't be held on this computer. Hold H or Mouse 4", now + tuning.notice_for));
        }
    }
    if caps_was_held && caps_down && !caps_held {
        voices.notice = Some(("Caps Lock has been down a long time, so the microphone closed. Press it again", now + tuning.notice_for));
    }
    let talking = wanted.filter(|_| in_the_way.is_none());
    if talking.is_some() != voices.talking.is_some() {
        backend::push_to_talk(talking.is_some());
    }
    voices.talking = talking;

    let mic = backend::mic();
    if *last_mic != Some(mic) {
        *last_mic = Some(mic);
        if let Some(text) = mic_notice(mic).filter(|_| backend::SUPPORTED && settings.mic_on) {
            voices.notice = Some((text, now + tuning.notice_for));
        }
    }
}

/// Moves voice between the browser and the connection, and tells the server what this player wants.
#[allow(clippy::too_many_arguments)]
fn send_and_receive(
    time: Res<Time<Real>>,
    tuning: Res<VoiceTuning>,
    settings: Res<VoiceSettings>,
    levels: Res<AudioLevels>,
    game: Res<GameMode>,
    link: Option<ResMut<VoiceLink>>,
    mut voices: ResMut<Voices>,
    mut told: Local<Option<(bool, Vec<u16>)>>,
    mut volume: Local<Option<f32>>,
    mut seq: Local<u8>,
    mut last_channel: Local<Option<VoiceChannel>>,
) {
    let Some(mut link) = link else { return };
    let now = time.elapsed_secs();
    link.buffer_frames = send_buffer_frames(&tuning);

    // Once a player is gone, so is their number and any mute on it.
    let players = &game.0.state.players;
    if !players.is_empty() {
        voices.muted.retain(|&muted| players.iter().any(|p| p.soldier == muted as usize));
    }

    let want = (link.allowed && backend::can_hear() && settings.volume() > 0.0, voices.muted.clone());
    if link.allowed && told.as_ref() != Some(&want) {
        if let Some((_, before)) = &*told {
            for player in want.1.iter().filter(|p| !before.contains(p)) {
                backend::stop(*player);
            }
        } else {
            want.1.iter().for_each(|player| backend::stop(*player));
        }
        link.set = Some(want.clone());
        *told = Some(want);
    }
    if !link.allowed {
        *told = None;
    }

    let level = settings.volume() * levels.master();
    if *volume != Some(level) {
        *volume = Some(level);
        backend::set_volume(level);
    }

    // What the encoder finishes after the key comes up still goes the way it was going.
    if voices.talking.is_some() {
        *last_channel = voices.talking;
    }
    while let Some(bytes) = backend::take_frame() {
        let Some(channel) = *last_channel else { continue };
        if link.allowed {
            link.up.push(VoiceUp { channel, seq: *seq, bytes });
            *seq = seq.wrapping_add(1);
        }
    }

    let frames = std::mem::take(&mut link.down);
    for frame in frames {
        if voices.is_muted(frame.speaker) {
            continue;
        }
        backend::play(frame.speaker, frame.channel == VoiceChannel::Squad, frame.seq, &frame.bytes);
        let at = frame.position();
        match voices.talkers.iter_mut().find(|t| t.player == frame.speaker && t.channel == frame.channel) {
            Some(talker) => {
                talker.at = at;
                talker.heard_at = now;
            }
            None => voices.talkers.push(Talker { player: frame.speaker, channel: frame.channel, at, heard_at: now, next_shape: now }),
        }
    }
    // Kept a while so that the player can still find them to mute, once they have stopped.
    voices.talkers.retain(|t| now - t.heard_at < KEPT_FOR_MUTING);
}

/// Frames enough to hold `send_buffer_ms` of talking.
fn send_buffer_frames(cfg: &VoiceTuning) -> usize {
    (cfg.send_buffer_ms / cfg.frame_ms.max(1) as f32).ceil().max(1.0) as usize
}

/// How long someone stays near the top of the list of who to mute, in seconds.
const KEPT_FOR_MUTING: f32 = 300.0;

/// What the world does to everyone talking out loud: how far, which way, and through what.
fn shape_voices(
    time: Res<Time<Real>>,
    tuning: Res<VoiceTuning>,
    acoustics: Res<Acoustics>,
    view: Res<LocalView>,
    sim: Res<SimResource>,
    mut voices: ResMut<Voices>,
) {
    let now = time.elapsed_secs();
    let ear = ear_position(&view);
    let right = Vec3::new(view.blended.gun_yaw.cos(), 0.0, -view.blended.gun_yaw.sin());
    let world = &sim.0.world;
    let blocked = |from: Vec3, to: Vec3| {
        let path = to - from;
        world.raycast(from, path.normalize_or(Vec3::Y), path.length()).is_some()
    };
    for talker in &mut voices.talkers {
        let Some(at) = talker.at else { continue };
        if now < talker.next_shape || now - talker.heard_at > tuning.forget_stream_after {
            continue;
        }
        talker.next_shape = now + tuning.shape_every;
        let (mut gain, mut cutoff) = (voice_volume(&tuning, ear.distance(at)), NOTHING_CUT_HZ);
        if let Some(muffle) = acoustics.muffling(at, ear, blocked) {
            gain *= muffle.gain;
            cutoff = muffle.cutoff_hz;
        }
        backend::shape(talker.player, gain, cutoff, (at - ear).normalize_or_zero().dot(right));
    }
}

/// Past what anyone can hear, so nothing gets taken off.
const NOTHING_CUT_HZ: f32 = 20_000.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_that_never_comes_up_cannot_hold_the_microphone_open() {
        let mut hold = CapsLockHold::default();
        assert!(hold.update(true, true, 0.0, 15.0));
        assert!(hold.update(true, false, 14.0, 15.0));
        assert!(!hold.update(true, false, 15.0, 15.0), "it should have closed by now");
        assert!(!hold.update(true, false, 500.0, 15.0));
        assert!(!hold.update(false, false, 501.0, 15.0));
        assert!(hold.update(true, true, 502.0, 15.0), "and a new press opens it again");
    }

    #[test]
    fn letting_go_of_caps_lock_lets_go() {
        let mut hold = CapsLockHold::default();
        assert!(hold.update(true, true, 0.0, 15.0));
        assert!(!hold.update(false, false, 1.0, 15.0));
    }

    #[test]
    fn each_binding_talks_on_its_channel_and_the_radio_wins() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut buttons = ButtonInput::<MouseButton>::default();
        assert_eq!(held_channel(&keys, &buttons, true), None);
        for (key, channel) in TALK_KEYS {
            keys.press(key);
            assert_eq!(held_channel(&keys, &buttons, true), Some(channel), "{key:?}");
            keys.release(key);
        }
        for (button, channel) in TALK_BUTTONS {
            buttons.press(button);
            assert_eq!(held_channel(&keys, &buttons, true), Some(channel), "{button:?}");
            buttons.release(button);
        }
        keys.press(KeyCode::KeyH);
        keys.press(KeyCode::KeyY);
        assert_eq!(held_channel(&keys, &buttons, true), Some(VoiceChannel::Squad));
    }

    #[test]
    fn caps_lock_does_nothing_when_it_does_not_count() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::CapsLock);
        assert_eq!(held_channel(&keys, &ButtonInput::default(), false), None);
    }

    #[test]
    fn a_voice_fades_to_nothing_before_a_server_stops_sending_it() {
        let cfg = VoiceTuning::default();
        assert!(voice_volume(&cfg, 1.0) > 0.8);
        assert!(voice_volume(&cfg, 30.0) < voice_volume(&cfg, 10.0));
        assert_eq!(voice_volume(&cfg, cfg.fades_out.1), 0.0);
        assert!(cfg.fades_out.1 < 80.0, "the default server's reach");
    }

    #[test]
    fn the_microphone_starts_on_and_the_volume_stays_in_range() {
        let mut settings = VoiceSettings::default();
        assert!(settings.mic_on());
        settings.set_volume(5.0);
        assert_eq!(settings.volume(), 1.0);
        settings.set_volume(-1.0);
        assert_eq!(settings.volume(), 0.0);
    }

    #[test]
    fn the_send_buffer_holds_half_a_second_at_any_frame_length() {
        let mut cfg = VoiceTuning::default();
        assert_eq!(send_buffer_frames(&cfg), 13);
        cfg.frame_ms = 20;
        assert_eq!(send_buffer_frames(&cfg), 25);
        cfg.send_buffer_ms = 0.0;
        assert_eq!(send_buffer_frames(&cfg), 1);
    }

    #[test]
    fn a_mute_can_be_taken_back() {
        let mut voices = Voices::default();
        voices.set_muted(4, true);
        voices.set_muted(4, true);
        assert!(voices.is_muted(4));
        voices.set_muted(4, false);
        assert!(!voices.is_muted(4));
    }
}
