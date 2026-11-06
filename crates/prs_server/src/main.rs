//! `prs_server [--bind ADDR] [--map NAME] [--mode NAME] [--bots N] [--web DIR] [--max-players N] [--snapshot-every TICKS] [--seconds S] [--quiet] [--no-voice] [--wt-port PORT] [--wt-cert PEM --wt-key PEM] [--no-webtransport]`
//!
//! A host sets these through the environment instead: `PORT`, `PRS_MAP`, `PRS_MODE`, `PRS_MOD`, `PRS_BOTS`,
//! `PRS_MAX_PLAYERS`, `PRS_ALLOWED_ORIGINS` (comma separated), `PRS_WEB_DIR`, `PRS_VOICE=off`, `PRS_WT_PORT`
//! (`off` for none), `PRS_WT_CERT` and `PRS_WT_KEY`. Arguments win. WebSockets, the page out of the web
//! folder, `GET /health`, `GET /webtransport` and the map at `GET /map/<name>.toml` all come in on one TCP
//! port. `--map` is a name, from `PRS_MAPS_DIR` or `maps` here or else built in, or a path to a `.toml`.
//!
//! WebTransport is on a UDP port of its own, 4433 unless told otherwise. Without a certificate it makes
//! one for itself, which browsers accept only from a page that names it, as this server's pages do: fine
//! on localhost or a LAN. A public server needs a real certificate for its name, such as the web server's.

use prs_net::names::NameRules;
use prs_net::server::{GameServer, ServerConfig, VoiceConfig};
use prs_net::ws_host::{HostConfig, PlainHttp, WsHost};
use prs_net::wt_host::{WtCertificate, WtConfig};
use prs_net_sim::{SimServer, SimServerConfig};
use prs_sim::MapData;
use prs_server::web::{MapFile, WebConfig, WebRoot, answer_status};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    prs_mods::activate_from_env();
    let mut server = ServerConfig { rules_note: prs_mods::describe(&prs_mods::active().name), ..Default::default() };
    let voice = prs_mods::tuned("server_voice", VoiceLimits::default());
    server.voice = voice.config();
    server.names = prs_mods::tuned("server_names", Names::default()).rules();
    let mut host = HostConfig::default();
    let mut map = env("PRS_MAP").unwrap_or_else(|| "range".to_owned());
    let mut mode = env("PRS_MODE");
    let mut bots = env("PRS_BOTS").map(|value| value.parse().unwrap_or_else(|_| usage("PRS_BOTS")));
    if let Some(port) = env("PORT") {
        let port: u16 = port.parse().unwrap_or_else(|_| usage("PORT"));
        // Both IPv4 and IPv6, since a host's proxy can come in on either.
        host.bind = SocketAddr::from(([0u16; 8], port));
    }
    if let Some(players) = env("PRS_MAX_PLAYERS") {
        server.max_players = players.parse().unwrap_or_else(|_| usage("PRS_MAX_PLAYERS"));
    }
    if env("PRS_VOICE").is_some_and(|value| value == "off") {
        server.voice.enabled = false;
    }
    if let Some(origins) = env("PRS_ALLOWED_ORIGINS") {
        host.allowed_origins = origins.split(',').map(|origin| origin.trim().to_owned()).collect();
    }
    let mut web_dir = env("PRS_WEB_DIR");
    let mut wt_port = env("PRS_WT_PORT");
    let (mut wt_cert, mut wt_key) = (env("PRS_WT_CERT"), env("PRS_WT_KEY"));
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage(&arg));
        match arg.as_str() {
            "--bind" => host.bind = value().parse().unwrap_or_else(|_| usage("--bind")),
            "--map" => map = value(),
            "--mode" => mode = Some(value()),
            "--web" => web_dir = Some(value()),
            "--bots" => bots = Some(value().parse().unwrap_or_else(|_| usage("--bots"))),
            "--max-players" => server.max_players = value().parse().unwrap_or_else(|_| usage("--max-players")),
            "--snapshot-every" => server.snapshot_every = value().parse().unwrap_or_else(|_| usage("--snapshot-every")),
            "--seconds" => {
                host.run_for = Some(Duration::from_secs_f64(value().parse().unwrap_or_else(|_| usage("--seconds"))));
            }
            "--quiet" => host.status_every = None,
            "--no-voice" => server.voice.enabled = false,
            "--wt-port" => wt_port = Some(value()),
            "--wt-cert" => wt_cert = Some(value()),
            "--wt-key" => wt_key = Some(value()),
            "--no-webtransport" => wt_port = Some("off".to_owned()),
            _ => usage(&arg),
        }
    }
    // A name from the maps folder or built in, or a path to a file.
    let data = MapData::find(&map).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let map = data.name.clone();
    // A range is for practising. Anywhere else there is a match to play, unless told otherwise.
    let mode = mode.unwrap_or_else(|| if map == "range" { "practice" } else { "aas" }.to_owned());
    let bots_per_team = bots.map(|total: usize| total.div_ceil(2));
    let game = SimServer::with_map(&data, &mode, sim_config(bots_per_team, voice.heard_metres)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let site = web_dir.map(|folder| WebRoot::new(folder.as_ref(), WebConfig::default())).transpose()?;
    let map_file = MapFile::new(&data.name, &data.text);
    host.plain_http = Some(PlainHttp(Arc::new(move |mut stream, request| {
        // Whoever asked has gone or asked for nonsense, and either way the game carries on.
        if map_file.respond(&mut stream, request).is_some() {
            return;
        }
        let _ = match &site {
            Some(site) => site.respond(stream, request),
            None => answer_status(&mut stream, "404 Not Found"),
        };
    })));
    host.webtransport = webtransport(host.bind, wt_port, wt_cert, wt_key);
    let listening = WsHost::bind(host)?;
    println!("prs_server playing {mode} on {map} at ws://{}", listening.local_addr()?);
    if let Some(at) = listening.webtransport_addr() {
        println!("webtransport on udp {at}");
    }
    let (_, stats) = listening.run(GameServer::new(game, server));
    println!(
        "ran {} ticks, busy {:.1}%, worst lag {} ticks, {}",
        stats.ticks,
        stats.busy_fraction * 100.0,
        stats.ticks_behind_max,
        stats.times,
    );
    Ok(())
}

/// What a mod can change about voice on a server. Whether it runs at all is the host's to say.
#[derive(serde::Serialize, serde::Deserialize)]
struct VoiceLimits {
    max_frame_bytes: usize,
    talker_frames_per_second: u32,
    talker_bytes_per_second: u32,
    listener_bytes_per_second: u32,
    queue_bytes: usize,
    heard_metres: f32,
}

impl Default for VoiceLimits {
    fn default() -> Self {
        let cfg = VoiceConfig::default();
        Self {
            max_frame_bytes: cfg.max_frame_bytes,
            talker_frames_per_second: cfg.talker_frames_per_second,
            talker_bytes_per_second: cfg.talker_bytes_per_second,
            listener_bytes_per_second: cfg.listener_bytes_per_second,
            queue_bytes: cfg.queue_bytes,
            heard_metres: SimServerConfig::default().reach.voice_heard,
        }
    }
}

impl VoiceLimits {
    fn config(&self) -> VoiceConfig {
        VoiceConfig {
            max_frame_bytes: self.max_frame_bytes,
            talker_frames_per_second: self.talker_frames_per_second,
            talker_bytes_per_second: self.talker_bytes_per_second,
            listener_bytes_per_second: self.listener_bytes_per_second,
            queue_bytes: self.queue_bytes,
            ..Default::default()
        }
    }
}

/// What a mod can change about the names players give themselves.
#[derive(serde::Serialize, serde::Deserialize)]
struct Names {
    max_chars: usize,
    ascii_only: bool,
    punctuation: String,
    reserved: Vec<String>,
    fallback: String,
}

impl Default for Names {
    fn default() -> Self {
        let NameRules { max_chars, ascii_only, punctuation, reserved, fallback } = NameRules::default();
        Self { max_chars, ascii_only, punctuation, reserved, fallback }
    }
}

impl Names {
    fn rules(self) -> NameRules {
        let Self { max_chars, ascii_only, punctuation, reserved, fallback } = self;
        // Compared in lower case, however a mod writes them.
        let reserved = reserved.iter().map(|word| word.to_lowercase()).collect();
        NameRules { max_chars, ascii_only, punctuation, reserved, fallback }
    }
}

fn sim_config(bots_per_team: Option<usize>, voice_heard: f32) -> SimServerConfig {
    let mut cfg = SimServerConfig { bots_per_team, ..Default::default() };
    cfg.reach.voice_heard = voice_heard;
    cfg
}

/// WebTransport beside the WebSocket, on the same address, unless the port is `off`.
fn webtransport(beside: SocketAddr, port: Option<String>, cert: Option<String>, key: Option<String>) -> Option<WtConfig> {
    let defaults = WtConfig::default();
    let port = match port.as_deref() {
        Some("off") => return None,
        Some(port) => port.parse().unwrap_or_else(|_| usage("--wt-port")),
        None => defaults.bind.port(),
    };
    let certificate = match (cert, key) {
        (Some(chain), Some(key)) => WtCertificate::Files { chain: chain.into(), key: key.into() },
        (None, None) => defaults.certificate.clone(),
        _ => usage("--wt-cert and --wt-key go together"),
    };
    Some(WtConfig { bind: SocketAddr::new(beside.ip(), port), certificate, ..defaults })
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn usage(bad: &str) -> ! {
    eprintln!("bad argument near {bad}");
    eprintln!("prs_server [--bind ADDR] [--map NAME] [--mode NAME] [--bots N] [--web DIR] [--max-players N] [--snapshot-every TICKS] [--seconds S] [--quiet] [--no-voice] [--wt-port PORT] [--wt-cert PEM --wt-key PEM] [--no-webtransport]");
    std::process::exit(2);
}
