//! The game client for desktop and web. All behaviour lives in the crates; this only wires them up.

mod bench;
mod dev;
#[cfg(not(target_arch = "wasm32"))]
mod dev_climb;
#[cfg(not(target_arch = "wasm32"))]
mod dev_heli;
#[cfg(not(target_arch = "wasm32"))]
mod dev_rest;
#[cfg(not(target_arch = "wasm32"))]
mod dev_mode;
mod netbot;
#[cfg(not(target_arch = "wasm32"))]
mod soak;

use bevy::prelude::*;
use bevy::window::PresentMode;

/// `PRS_MOD=name` reads `mods/name.toml`, before anything has looked at a default.
#[cfg(not(target_arch = "wasm32"))]
fn activate_mod() {
    prs_mods::activate_from_env();
}

/// `?mod=name` fetches `mods/name.toml` from beside the page. It waits for the answer on purpose: nothing
/// else can start until the rules are known, and the file is a few hundred bytes from the same place.
#[cfg(target_arch = "wasm32")]
fn activate_mod() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let search = window.location().search().unwrap_or_default();
    let Some(name) = web_sys::UrlSearchParams::new_with_str(&search).ok().and_then(|params| params.get("mod")) else {
        return;
    };
    let path = format!("mods/{name}.toml");
    let request = web_sys::XmlHttpRequest::new().expect("a browser has this");
    let fetched = request.open_with_async("GET", &path, false).and_then(|()| request.send());
    let found = fetched.is_ok() && request.status().is_ok_and(|status| status == 200);
    let text = if found { request.response_text().ok().flatten() } else { None };
    let parsed = match text {
        Some(text) => prs_mods::Mod::parse(&name, &text),
        None => Err(prs_mods::ModError::NotFound { name: name.clone(), looked_in: path }),
    };
    match parsed {
        Ok(found) => prs_mods::activate(found),
        Err(error) => prs_mods::stop(&error),
    }
}

/// `PRS_WINDOW=1920x1080` opens the window that size, for screenshots of how things lay out.
#[cfg(not(target_arch = "wasm32"))]
fn requested_size() -> bevy::window::WindowResolution {
    let size = std::env::var("PRS_WINDOW").ok().and_then(|spec| {
        let (w, h) = spec.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    size.map_or_else(default, |(w, h)| bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0))
}

#[cfg(target_arch = "wasm32")]
fn requested_size() -> bevy::window::WindowResolution {
    default()
}

fn main() {
    bench::mark_start();
    activate_mod();
    let window = Window {
        title: "FrontCraft".into(),
        // Latency beats tearing for a shooter. Browsers ignore this and sync on their own.
        present_mode: PresentMode::AutoNoVsync,
        fit_canvas_to_parent: true,
        canvas: Some("#game".into()),
        prevent_default_event_handling: true,
        resolution: requested_size(),
        ..default()
    };

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(spec) = soak::requested().filter(|_| std::env::var("PRS_SOAK_WINDOW").is_err()) {
        let mut app = App::new();
        add_game(soak::engine(&mut app));
        return soak::run(app, spec);
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    }));
    add_game(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(spec) = soak::requested_with_window() {
        return soak::watch(app, spec);
    }
    app.run();
}

/// Everything the game is, whatever it is running on.
fn add_game(app: &mut App) {
    app.add_plugins((
            prs_runtime::RuntimePlugin,
            prs_models::ModelsPlugin,
            prs_atmosphere::AtmospherePlugin,
            prs_input::InputPlugin,
            prs_view::ViewPlugin,
            prs_scene::ScenePlugin,
            prs_audio::AudioPlugin,
            prs_voice::VoicePlugin,
            prs_hud::HudPlugin,
            dev::DevPlugin,
            bench::BenchPlugin,
            netbot::NetBotPlugin,
        ));
}
