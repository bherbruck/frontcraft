//! What a player has set up for themselves stays set: FOV, zoom FOV, sensitivity, volume, voice and how big
//! the HUD is, in the
//! browser's local storage or a small file. It is a few `name = value` lines read one at a time, so a
//! damaged store loses only what is damaged and everything else comes up as usual. These belong to the
//! player and not to the rules, so a server never hears of them.

use bevy::prelude::*;
use prs_audio::{AudioLevels, SoundSet};
use prs_input::InputSettings;
use prs_view::ViewSettings;
use prs_voice::VoiceSettings;

use crate::scale::{HudScaleRules, HudSize};

/// How long after the last change it writes, so holding a button does not write every step.
const WRITE_AFTER_SECONDS: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub fov: f32,
    pub zoom_fov: f32,
    pub sensitivity: f32,
    pub master_volume: f32,
    pub effects_volume: f32,
    pub rest_stops_at_arc: bool,
    /// The sound set they have picked. Until they pick one, what plays is the game's or a mod's choice.
    pub sound_set: Option<SoundSet>,
    /// Whether the microphone may be opened at all, and how loud other people are.
    pub mic_on: bool,
    pub voice_volume: f32,
    /// Their own setting for how big the HUD is, over what the window chooses.
    pub hud_size: f32,
}

impl Saved {
    /// Reads what it can over `base`. Unknown lines, bad numbers and anything not finite are skipped.
    pub fn parse(text: &str, base: Saved) -> Saved {
        let mut out = base;
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.trim().parse::<f32>() else {
                continue;
            };
            if !value.is_finite() {
                continue;
            }
            match name.trim() {
                "fov" => out.fov = value,
                "zoom_fov" => out.zoom_fov = value,
                "sensitivity" => out.sensitivity = value,
                "master_volume" => out.master_volume = value,
                "effects_volume" => out.effects_volume = value,
                "rest_stops_at_arc" => out.rest_stops_at_arc = value != 0.0,
                "mic_on" => out.mic_on = value != 0.0,
                "voice_volume" => out.voice_volume = value,
                "hud_size" => out.hud_size = value,
                "sound_set" if value == 0.0 => out.sound_set = Some(SoundSet::New),
                "sound_set" if value == 1.0 => out.sound_set = Some(SoundSet::Current),
                _ => {}
            }
        }
        out
    }

    pub fn text(&self) -> String {
        let sound_set = match self.sound_set {
            Some(SoundSet::New) => "sound_set = 0\n",
            Some(SoundSet::Current) => "sound_set = 1\n",
            None => "",
        };
        format!(
            "fov = {}\nzoom_fov = {}\nsensitivity = {}\nmaster_volume = {}\neffects_volume = {}\nrest_stops_at_arc = {}\nmic_on = {}\nvoice_volume = {}\nhud_size = {}\n{sound_set}",
            self.fov,
            self.zoom_fov,
            self.sensitivity,
            self.master_volume,
            self.effects_volume,
            u8::from(self.rest_stops_at_arc),
            u8::from(self.mic_on),
            self.voice_volume,
            self.hud_size
        )
    }

    fn current(view: &ViewSettings, input: &InputSettings, audio: &AudioLevels, voice: &VoiceSettings, size: &HudSize) -> Self {
        Self {
            fov: view.fov(),
            zoom_fov: view.zoom_fov(),
            sensitivity: input.sensitivity,
            master_volume: audio.master(),
            effects_volume: audio.effects(),
            rest_stops_at_arc: input.rest_stops_at_arc,
            sound_set: audio.chosen_sound_set(),
            mic_on: voice.mic_on(),
            voice_volume: voice.volume(),
            hud_size: size.0,
        }
    }

    /// Puts these into play through the setters, which hold every value to its limits.
    #[allow(clippy::too_many_arguments)]
    fn apply(&self, view: &mut ViewSettings, input: &mut InputSettings, audio: &mut AudioLevels, voice: &mut VoiceSettings, size: &mut HudSize, rules: &HudScaleRules) {
        size.0 = rules.held(self.hud_size);
        voice.set_mic_on(self.mic_on);
        voice.set_volume(self.voice_volume);
        view.set_fov(self.fov);
        view.set_zoom_fov(self.zoom_fov);
        input.sensitivity = 1.0;
        input.adjust_sensitivity(self.sensitivity);
        audio.set_master(self.master_volume);
        audio.set_effects(self.effects_volume);
        input.rest_stops_at_arc = self.rest_stops_at_arc;
        if let Some(set) = self.sound_set {
            audio.choose_sound_set(set);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod store {
    use std::path::PathBuf;

    /// Where the file goes, if anywhere. `PRS_SETTINGS_FILE` chooses, and empty means none. A scripted
    /// run has none either, so a screenshot or a soak neither reads nor changes what a player has set.
    fn path() -> Option<PathBuf> {
        if let Ok(file) = std::env::var("PRS_SETTINGS_FILE") {
            return (!file.is_empty()).then(|| file.into());
        }
        if std::env::var("PRS_SHOT").is_ok() || std::env::var("PRS_SOAK").is_ok() {
            return None;
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("prs").join("settings.txt"))
    }

    pub fn read() -> Option<String> {
        std::fs::read_to_string(path()?).ok()
    }

    pub fn write(text: &str) {
        let Some(path) = path() else {
            return;
        };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Err(error) = std::fs::write(&path, text) {
            bevy::log::warn!("could not save settings to {}: {error}", path.display());
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod store {
    const KEY: &str = "prs.settings";

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn read() -> Option<String> {
        storage()?.get_item(KEY).ok()?
    }

    /// Private windows and full stores refuse, and the settings then last as long as the page.
    pub fn write(text: &str) {
        if let Some(storage) = storage() {
            let _ = storage.set_item(KEY, text);
        }
    }
}

pub(crate) fn load_settings(
    mut view: ResMut<ViewSettings>,
    mut input: ResMut<InputSettings>,
    mut audio: ResMut<AudioLevels>,
    mut voice: ResMut<VoiceSettings>,
    mut size: ResMut<HudSize>,
    rules: Res<HudScaleRules>,
) {
    let Some(text) = store::read() else {
        return;
    };
    let saved = Saved::parse(&text, Saved::current(&view, &input, &audio, &voice, &size));
    saved.apply(&mut view, &mut input, &mut audio, &mut voice, &mut size, &rules);
}

/// Writes a little after things stop changing.
pub(crate) fn save_settings(
    time: Res<Time<Real>>,
    view: Res<ViewSettings>,
    input: Res<InputSettings>,
    audio: Res<AudioLevels>,
    voice: Res<VoiceSettings>,
    size: Res<HudSize>,
    mut written: Local<Option<Saved>>,
    mut wait: Local<Option<f32>>,
) {
    let now = Saved::current(&view, &input, &audio, &voice, &size);
    let Some(last) = *written else {
        // The first look is what was just loaded, which is nothing new to write.
        *written = Some(now);
        return;
    };
    if now == last {
        *wait = None;
        return;
    }
    let left = wait.unwrap_or(WRITE_AFTER_SECONDS) - time.delta_secs();
    if left > 0.0 {
        *wait = Some(left);
        return;
    }
    store::write(&now.text());
    *written = Some(now);
    *wait = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Saved {
        Saved { fov: 62.0, zoom_fov: 40.0, sensitivity: 1.0, master_volume: 1.0, effects_volume: 1.0, rest_stops_at_arc: true, sound_set: None, mic_on: false, voice_volume: 1.0, hud_size: 1.0 }
    }

    #[test]
    fn what_was_written_reads_back_the_same() {
        let saved = Saved { fov: 71.0, zoom_fov: 33.5, sensitivity: 0.37, master_volume: 0.4, effects_volume: 0.85, rest_stops_at_arc: false, sound_set: None, mic_on: true, voice_volume: 0.6, hud_size: 1.3 };
        assert_eq!(Saved::parse(&saved.text(), base()), saved);
    }

    #[test]
    fn a_damaged_store_loses_only_what_is_damaged() {
        let text = "fov = banana\nzoom_fov = NaN\nsensitivity = inf\n\u{0}\u{fffd}garbage\n= 3\nmaster_volume = 0.5\nnot a line at all\nfuture_setting = 9";
        let read = Saved::parse(text, base());
        assert_eq!(read, Saved { master_volume: 0.5, ..base() });
        assert_eq!(Saved::parse("", base()), base());
        assert_eq!(Saved::parse("%%%%%\n\n\n", base()), base());
    }

    #[test]
    fn the_sound_set_is_saved_only_once_chosen_and_nonsense_is_ignored() {
        assert!(!base().text().contains("sound_set"));
        for set in [SoundSet::New, SoundSet::Current] {
            let chosen = Saved { sound_set: Some(set), ..base() };
            assert_eq!(Saved::parse(&chosen.text(), base()), chosen);
        }
        assert_eq!(Saved::parse("sound_set = 7", base()), base());
    }

    #[test]
    fn values_out_of_range_are_held_to_their_limits_when_applied() {
        let (mut view, mut audio) = (ViewSettings::default(), AudioLevels::default());
        let mut input = InputSettings { sensitivity: 1.0, zoom_hold_seconds: 0.18, free_look_yaw_limit: 2.15, free_look_return_rate: 12.0, rest_stops_at_arc: true };
        let wild = Saved { fov: 9999.0, zoom_fov: -5.0, sensitivity: 1e9, master_volume: 40.0, effects_volume: -3.0, rest_stops_at_arc: false, sound_set: Some(SoundSet::Current), mic_on: false, voice_volume: 7.0, hud_size: 40.0 };
        let mut voice = VoiceSettings::default();
        let (mut size, rules) = (HudSize(1.0), HudScaleRules::default());
        wild.apply(&mut view, &mut input, &mut audio, &mut voice, &mut size, &rules);
        assert_eq!(size.0, rules.player_most);
        assert_eq!(voice.volume(), 1.0);
        assert_eq!(view.fov(), prs_view::FOV_RANGE.1);
        assert_eq!(view.zoom_fov(), prs_view::ZOOM_FOV_MIN);
        assert_eq!(input.sensitivity, 8.0);
        assert_eq!((audio.master(), audio.effects()), (1.0, 0.0));
        assert_eq!(audio.sound_set(), SoundSet::Current);
    }
}
