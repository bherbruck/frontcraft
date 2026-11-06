//! The game's footstep synth for the tuner page at `/tune/`, so its sliders play what the game would.

use prs_movement::{MoveConfig, Stance};
use prs_synth::{ContactSound, ContactSounds, STEP_VOLUME, SlideSound, SlideSounds, SmokeSound, StepSound, StepSounds, StepVary, footstep};
use wasm_bindgen::prelude::*;

/// The settings the game ships with, as JSON: `{ "steps": { "dirt": {..}, .. }, "step_volume": [quiet, sprint] }`,
/// and how fast and how hard a soldier steps in each gait, as the game works them out.
#[wasm_bindgen]
pub fn defaults() -> String {
    let cfg = MoveConfig::default();
    let gait = |name: &str, speed: f32, stance: Stance, sprinting: bool| {
        let per_second = speed / cfg.stride_length(stance, sprinting) * cfg.footsteps_per_stride;
        serde_json::json!({ "name": name, "per_second": per_second, "intensity": (speed / cfg.sprint_speed).clamp(0.15, 1.0) })
    };
    serde_json::json!({
        "steps": StepSounds::default(),
        "slides": SlideSounds::default(),
        "smoke": SmokeSound::default(),
        "contacts": ContactSounds::default(),
        "step_vary": StepVary::default(),
        "current_volumes": {
            "selector": prs_synth::CURRENT_SELECTOR_VOLUME,
            "dry": prs_synth::CURRENT_DRY_VOLUME,
            "zip": prs_synth::CURRENT_ZIP_VOLUME,
            "tear": prs_synth::CURRENT_TEAR_VOLUME,
        },
        "up_close": { "impact": prs_synth::IMPACT_LOUDNESS, "metal_hit": prs_synth::METAL_HIT_LOUDNESS, "clink": prs_synth::CLINK_LOUDNESS, "crash": prs_synth::CRASH_LOUDNESS },
        "step_volume": STEP_VOLUME,
        "gaits": [
            gait("Crouch", cfg.crouch_speed, Stance::Crouch, false),
            gait("Aiming", cfg.ads_speed, Stance::Stand, false),
            gait("Run", cfg.run_speed, Stance::Stand, false),
            gait("Sprint", cfg.sprint_speed, Stance::Stand, true),
        ],
    })
    .to_string()
}

/// One step on the ground described by `sound`, which is one surface's JSON from [`defaults`].
#[wasm_bindgen]
pub fn step(seed: u32, sound: &str) -> Result<Vec<f32>, String> {
    let sound: StepSound = serde_json::from_str(sound).map_err(|error| error.to_string())?;
    Ok(footstep(seed, &sound))
}

/// One slide of the kind described by `sound`, which is one of the `slides` JSON from [`defaults`].
#[wasm_bindgen]
pub fn slide(seed: u32, sound: &str) -> Result<Vec<f32>, String> {
    let sound: SlideSound = serde_json::from_str(sound).map_err(|error| error.to_string())?;
    Ok(prs_synth::slide(seed, &sound))
}

/// A few seconds of a smoke grenade pouring, from its first fizz, described by the `smoke` JSON from [`defaults`].
#[wasm_bindgen]
pub fn smoke(seed: u32, sound: &str, seconds: f32) -> Result<Vec<f32>, String> {
    let sound: SmokeSound = serde_json::from_str(sound).map_err(|error| error.to_string())?;
    Ok(prs_synth::smoke_run(seed, &sound, seconds))
}

/// A step on the New set, with its grit varied by `vary`, which is the `step_vary` JSON from [`defaults`].
#[wasm_bindgen]
pub fn varied_step(seed: u32, sound: &str, vary: &str) -> Result<Vec<f32>, String> {
    let sound: StepSound = serde_json::from_str(sound).map_err(|error| error.to_string())?;
    let vary: StepVary = serde_json::from_str(vary).map_err(|error| error.to_string())?;
    Ok(prs_synth::varied_footstep(seed, &sound, &vary))
}

/// One contact, described by one of the `contacts` JSON from [`defaults`].
#[wasm_bindgen]
pub fn contact(seed: u32, sound: &str) -> Result<Vec<f32>, String> {
    let sound: ContactSound = serde_json::from_str(sound).map_err(|error| error.to_string())?;
    Ok(prs_synth::contact(seed, &sound))
}

/// A vehicle crash on the New set, whose metal is the `crash_metal` JSON from [`defaults`].
#[wasm_bindgen]
pub fn contact_crash(seed: u32, metal: &str) -> Result<Vec<f32>, String> {
    let metal: ContactSound = serde_json::from_str(metal).map_err(|error| error.to_string())?;
    Ok(prs_synth::contact_crash(seed, &metal))
}

/// What one of the sounds with no settings plays as on the Current set: `selector`, `dry`, `zip`, `tear`,
/// `metal_hit`, `crash` or `impact`.
#[wasm_bindgen]
pub fn current_sound(seed: u32, name: &str) -> Result<Vec<f32>, String> {
    match name {
        "selector" => Ok(prs_synth::current_selector()),
        "dry" => Ok(prs_synth::dry_click()),
        "zip" => Ok(prs_synth::current_zip()),
        "tear" => Ok(prs_synth::tear()),
        "metal_hit" => Ok(prs_synth::metal_hit(seed)),
        "crash" => Ok(prs_synth::crash(seed)),
        "impact" => Ok(prs_synth::impact(seed)),
        _ => Err(format!("no sound called {name}")),
    }
}
