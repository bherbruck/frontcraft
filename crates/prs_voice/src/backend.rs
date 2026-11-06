//! The part of voice that has to be in a browser, or on a desktop has nothing to do yet.

/// How the microphone is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mic {
    Idle,
    Asking,
    Live,
    Refused,
    /// No microphone, or a page the browser won't give one to, which is anything but https.
    Missing,
    /// No Opus encoder here.
    NoCodec,
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Mic;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/src/voice.js")]
    extern "C" {
        #[allow(clippy::too_many_arguments)]
        fn voice_configure(bitrate: u32, frame_ms: u32, jitter_ms: f32, most_behind_ms: f32, radio_low: f32, radio_high: f32, radio_gain: f32, idle_ms: f32);
        fn voice_can_hear() -> bool;
        fn voice_mic_state() -> u8;
        fn voice_ptt(down: bool);
        fn voice_close_mic();
        fn voice_take_frame() -> Option<js_sys::Uint8Array>;
        fn voice_play(id: u16, radio: bool, seq: u8, bytes: &[u8]);
        fn voice_shape(id: u16, gain: f32, cutoff: f32, pan: f32);
        fn voice_stop(id: u16);
        fn voice_volume(volume: f32);
        fn voice_caps_lock_can_be_held() -> bool;
    }

    pub const SUPPORTED: bool = true;

    pub fn caps_lock_can_be_held() -> bool {
        voice_caps_lock_can_be_held()
    }

    pub fn configure(cfg: &crate::VoiceTuning) {
        voice_configure(cfg.bitrate, cfg.frame_ms, cfg.jitter_ms, cfg.most_behind_ms, cfg.radio_band_hz.0, cfg.radio_band_hz.1, cfg.radio_gain, cfg.forget_stream_after * 1000.0);
    }

    pub fn can_hear() -> bool {
        voice_can_hear()
    }

    pub fn mic() -> Mic {
        match voice_mic_state() {
            1 => Mic::Asking,
            2 => Mic::Live,
            3 => Mic::Refused,
            4 => Mic::Missing,
            5 => Mic::NoCodec,
            _ => Mic::Idle,
        }
    }

    pub fn push_to_talk(down: bool) {
        voice_ptt(down);
    }

    pub fn close_mic() {
        voice_close_mic();
    }

    pub fn take_frame() -> Option<Vec<u8>> {
        voice_take_frame().map(|bytes| bytes.to_vec())
    }

    pub fn play(id: u16, radio: bool, seq: u8, bytes: &[u8]) {
        voice_play(id, radio, seq, bytes);
    }

    pub fn shape(id: u16, gain: f32, cutoff: f32, pan: f32) {
        voice_shape(id, gain, cutoff, pan);
    }

    pub fn stop(id: u16) {
        voice_stop(id);
    }

    pub fn set_volume(volume: f32) {
        voice_volume(volume);
    }
}

/// On a desktop there is no Opus or microphone yet, so voice is as if nobody had asked for it.
#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::Mic;

    pub const SUPPORTED: bool = false;

    pub fn caps_lock_can_be_held() -> bool {
        false
    }

    pub fn configure(_cfg: &crate::VoiceTuning) {}
    pub fn can_hear() -> bool {
        false
    }
    pub fn mic() -> Mic {
        Mic::NoCodec
    }
    pub fn push_to_talk(_down: bool) {}
    pub fn close_mic() {}
    pub fn take_frame() -> Option<Vec<u8>> {
        None
    }
    pub fn play(_id: u16, _radio: bool, _seq: u8, _bytes: &[u8]) {}
    pub fn shape(_id: u16, _gain: f32, _cutoff: f32, _pan: f32) {}
    pub fn stop(_id: u16) {}
    pub fn set_volume(_volume: f32) {}
}

pub use imp::*;
