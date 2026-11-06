//! The client side of multiplayer inside Bevy. Everything else in the game keeps reading
//! [`SimResource`], [`LocalView`] and [`SimMessage`], and cannot tell where they came from.
//!
//! A session owns its game and Bevy's resources own the sim presentation reads, and those are the
//! same sim. So the session's game holds a tiny spare, and the two are swapped for the moments the
//! session is working. Swapping moves a handful of pointers.

use bevy::prelude::*;
use std::time::Duration;
use bevy::platform::time::Instant;
use prs_net::{WsClient, WtClient};
use prs_net::client::{ClientConfig, ClientSession, ClientState};
use prs_net::transport::{FirstThen, Transport, TransportState, pump};
use prs_net::wire::{RejectReason, VoiceFrame, VoiceUp};
use prs_mode::{Match, Mode};
use prs_net_sim::mode::encode_cmd;
use prs_net_sim::{SimClient, WireEvent};
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::{Level, MapData, Sim};
use std::sync::{Arc, Mutex};

use crate::map_fetch::{self, Fetch};

use crate::{
    GameMode, LevelReady, LocalSoldier, LocalState, LocalView, ModeMessage, PendingInput, PendingModeCmds, SimMessage, SimResource,
};

/// How a multiplayer game is going, for whoever shows it.
#[derive(Resource, Debug, Clone, Default)]
pub enum NetStatus {
    /// Single player: there is no server to talk to.
    #[default]
    Offline,
    Connecting {
        server: String,
    },
    /// Fetching the map the server plays, which this game does not have.
    LoadingMap {
        name: String,
        received: usize,
        total: usize,
    },
    Playing {
        rtt_ms: f32,
        /// How many ticks ahead of the server the client is running.
        lead_ticks: i64,
        reconciles: u32,
        players: usize,
        /// Which transport, for whoever is looking into how it plays.
        over: &'static str,
    },
    Refused {
        reason: String,
    },
    Lost,
}

impl NetStatus {
    /// A line for the player when something is keeping them from playing.
    pub fn problem(&self) -> Option<String> {
        match self {
            Self::Offline | Self::Playing { .. } => None,
            Self::Connecting { server } => Some(format!("Connecting to {server}")),
            Self::LoadingMap { name, received, total } => {
                Some(format!("Loading the map {name}: {} of {} KB", received.div_ceil(1000), total.div_ceil(1000)))
            }
            Self::Refused { reason } => Some(format!("The server refused: {reason}")),
            Self::Lost => Some("Connection lost. Reload the page to try again.".into()),
        }
    }
}

/// What everyone on the server is called, by soldier, as the server last said. Empty when playing alone.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct PlayerNames(pub Vec<(u16, String)>);

impl PlayerNames {
    pub fn of(&self, soldier: usize) -> Option<&str> {
        self.0.iter().find(|(id, _)| *id as usize == soldier).map(|(_, name)| name.as_str())
    }
}

/// Voice on its way through the connection, which is the only thing here that knows there is one.
/// None of it goes anywhere near the sim. There is none of this at all when playing alone.
#[derive(Resource, Default)]
pub struct VoiceLink {
    /// Whether we are in a game on a server that carries voice.
    pub allowed: bool,
    pub up: Vec<VoiceUp>,
    pub down: Vec<VoiceFrame>,
    /// Whether to be sent voice, and who not to be sent it from, when that has changed.
    pub set: Option<(bool, Vec<u16>)>,
    /// How many frames of our own talking can wait to be sent.
    pub buffer_frames: usize,
}

const MAX_FRAME_TIME: Duration = Duration::from_secs(2);
/// How long WebTransport gets to open before the WebSocket is tried instead. A blocked UDP port
/// shows up as nothing at all happening, so this is all the time a player behind one waits. A busy
/// page answers slowly while it loads: a software-rendered one took nearly 3 s to open a session.
const WEBTRANSPORT_PATIENCE: Duration = Duration::from_secs(5);

struct NetLink {
    session: ClientSession<SimClient>,
    socket: FirstThen,
    server: String,
    /// Whether what is in [`SimResource`] is the real level yet.
    joined: bool,
    /// The map once it is here, for the game to be built on.
    map: Arc<Mutex<Option<MapData>>>,
    fetching: Option<Fetch>,
    gave_up: bool,
}

/// A sim with nothing in it and next to no cost, to stand in until the server has said which map.
pub(crate) fn placeholder_sim() -> Sim {
    let mut sim = Sim::new(Level::flat(2.0));
    sim.spawn_soldier(0);
    sim
}

/// The client's copy of the game once the server has said what it is playing and its map is here. Its sim
/// moves into [`SimResource`] on joining, and the game holds the placeholder in return.
fn make_game(map: Arc<Mutex<Option<MapData>>>) -> impl FnOnce(&str, &str) -> Option<SimClient> {
    move |_, mode| {
        let level = map.lock().ok()?.take()?.level().ok()?;
        Some(SimClient::with_sim(Sim::new(level), Mode::named(mode)?))
    }
}

/// Finds the map the server is playing, here or by fetching it, and lets the session go on once it has it.
fn get_map(link: &mut NetLink) {
    let Some(offer) = link.session.map_wanted().cloned() else { return };
    let max_bytes = prs_net_sim::limits().max_map_bytes;
    if link.fetching.is_none()
        && let Some(why) = map_fetch::refuse(&offer, max_bytes)
    {
        link.session.map_unavailable(why);
        return;
    }
    let found = match &link.fetching {
        None => match map_fetch::find_local(&offer) {
            Some(data) => Some(Ok(data)),
            None => {
                link.fetching = Some(Fetch::start(&link.server, &offer, max_bytes));
                None
            }
        },
        Some(fetch) => fetch.take(),
    };
    match found {
        Some(Ok(data)) => {
            *link.map.lock().unwrap() = Some(data);
            link.fetching = None;
            link.session.map_ready();
        }
        Some(Err(why)) => {
            link.fetching = None;
            link.session.map_unavailable(why);
        }
        None => {}
    }
}

pub(crate) fn build(app: &mut App, server: String, name: String) {
    // Anything longer than Bevy will catch up on is an absence.
    let cfg = ClientConfig { name, away_after_s: MAX_FRAME_TIME.as_secs_f32(), ..default() };
    let map = Arc::new(Mutex::new(None));
    let mut session = ClientSession::new(Box::new(make_game(map.clone())), cfg);
    session.wait_for_map();
    let socket = connect(&server);
    let mut virtual_time = Time::<Virtual>::default();
    // The server does not wait, so a slow machine has to run every tick it can't draw rather than fall
    // behind. Bevy's default stops time for anything over a quarter of a second.
    virtual_time.set_max_delta(MAX_FRAME_TIME);
    app.insert_resource(virtual_time)
        .insert_resource(NetStatus::Connecting { server: server.clone() })
        .init_resource::<VoiceLink>()
        .insert_non_send(NetLink { session, socket, server, joined: false, map, fetching: None, gave_up: false })
        .add_systems(FixedUpdate, tick_online)
        .add_systems(RunFixedMainLoop, start_frame.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop))
        .add_systems(
            RunFixedMainLoop,
            draw_online
                .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop)
                .before(crate::blend_view),
        );
}

/// WebTransport if it can be had, and the WebSocket otherwise.
fn connect(server: &str) -> FirstThen {
    let fallback = server.to_owned();
    let then: Box<dyn FnOnce() -> Box<dyn Transport>> = Box::new(move || Box::new(WsClient::connect(&fallback)));
    if websocket_only() {
        // Never opens, so the WebSocket is tried at once.
        return FirstThen::new(Box::new(Refused), then, 0.0, Box::new(|| 0.0));
    }
    let start = Instant::now();
    FirstThen::new(
        Box::new(WtClient::connect(server)),
        then,
        WEBTRANSPORT_PATIENCE.as_secs_f64() * 1000.0,
        Box::new(move || start.elapsed().as_secs_f64() * 1000.0),
    )
}

struct Refused;

impl Transport for Refused {
    fn state(&mut self) -> TransportState {
        TransportState::Closed
    }
    fn send(&mut self, _: &[u8]) {}
    fn recv(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn is_reliable(&self) -> bool {
        true
    }
}

/// `PRS_TRANSPORT=ws` on a desktop, for a network that lets UDP through but badly.
#[cfg(not(target_arch = "wasm32"))]
fn websocket_only() -> bool {
    std::env::var("PRS_TRANSPORT").is_ok_and(|value| value == "ws")
}

/// `?transport=ws`, the same in a browser.
#[cfg(target_arch = "wasm32")]
fn websocket_only() -> bool {
    let search = web_sys::window().and_then(|window| window.location().search().ok()).unwrap_or_default();
    search.trim_start_matches('?').split('&').any(|pair| pair == "transport=ws")
}

impl NetLink {
    /// Whether the session has a sim that should be what everyone is looking at.
    fn is_playing(&self) -> bool {
        self.session.state() == ClientState::Playing
    }

    fn swap_sim(&mut self, shown: &mut Sim) {
        if let Some(game) = self.session.game_mut() {
            std::mem::swap(&mut game.sim, shown);
        }
    }
}

/// Tells the session how long this frame really took, before the fixed steps that catch up on it. A
/// hidden browser tab or a minimised window gets no frames at all, and comes back with a long one.
fn start_frame(mut link: NonSendMut<NetLink>, real: Res<Time<Real>>) {
    link.session.frame_started(real.delta_secs());
}

fn tick_online(
    mut commands: Commands,
    mut link: NonSendMut<NetLink>,
    mut shown: ResMut<SimResource>,
    mut local: ResMut<LocalSoldier>,
    mut input: ResMut<PendingInput>,
    mut view: ResMut<LocalView>,
    mut status: ResMut<NetStatus>,
    mut messages: MessageWriter<SimMessage>,
    mut mode_messages: MessageWriter<ModeMessage>,
    mut game_mode: ResMut<GameMode>,
    mut requests: ResMut<PendingModeCmds>,
    mut voice: ResMut<VoiceLink>,
    mut names: ResMut<PlayerNames>,
) {
    let link = &mut *link;
    for request in requests.0.drain(..) {
        link.session.send_request(encode_cmd(&request));
    }
    link.session.set_voice_buffer(voice.buffer_frames);
    for up in voice.up.drain(..) {
        link.session.send_voice(up);
    }
    if let Some((listening, muted)) = voice.set.take() {
        link.session.set_voice(listening, muted);
    }
    let was_playing = link.is_playing();
    if was_playing {
        link.swap_sim(&mut shown.0);
    }
    pump(&mut link.session, &mut link.socket);
    get_map(link);
    let intent = &mut *input;
    link.session.update(TICK_DT, |tick| {
        let cmd = PlayerCmd {
            tick,
            wish_move: intent.wish_move,
            yaw: intent.yaw,
            pitch: intent.pitch,
            buttons: intent.held.union(intent.tapped),
            stick: intent.stick.quantised(),
            ..default()
        };
        intent.tapped = Buttons::default();
        cmd
    });
    pump(&mut link.session, &mut link.socket);
    let frames = link.session.drain_voice_in();
    voice.down.extend(frames);
    voice.allowed = link.is_playing() && link.session.voice_allowed();
    // Asks the game, so it has to come before the sim goes back.
    let view_feet = link.session.view_position();
    if link.is_playing() {
        link.swap_sim(&mut shown.0);
    }

    if link.is_playing() && !link.joined {
        link.joined = true;
        // The sim everything looks at is now the real one, on the map the server named.
        if let Some(mode) = link.session.mode_name().and_then(Mode::named) {
            game_mode.0 = Match::new(mode, &mut shown.0);
        }
        commands.insert_resource(LevelReady);
    }
    if let Some(state) = link.session.game_mut().and_then(|game| game.take_match_update()) {
        game_mode.0.import_state(state);
    }
    if let Some(game) = link.session.game().filter(|game| game.names != names.0) {
        names.0 = game.names.clone();
    }
    let id = link.session.game().map_or(local.0, |game| game.local);
    if link.is_playing() && local.0 != id {
        local.0 = id;
    }

    for event in link.session.drain_predicted_events() {
        messages.write(SimMessage(event));
    }
    send_server_events(link, &mut messages, &mut mode_messages);

    *status = status_of(link, &shown.0);
    if !link.joined && !link.gave_up && matches!(*status, NetStatus::Refused { .. } | NetStatus::Lost) {
        link.gave_up = true;
        give_up_on_server(&status.problem().unwrap_or_default());
    }
    if link.is_playing() {
        view.previous = view.current;
        view.current = LocalState::capture(&shown.0, local.0);
        if let Some(feet) = view_feet {
            view.current.feet = feet;
        }
    }
}

fn draw_online(
    mut link: NonSendMut<NetLink>,
    mut shown: ResMut<SimResource>,
    time: Res<Time>,
    mut helis: ResMut<crate::HeliFrames>,
    mut messages: MessageWriter<SimMessage>,
    mut mode_messages: MessageWriter<ModeMessage>,
) {
    let link = &mut *link;
    if !link.is_playing() {
        return;
    }
    link.swap_sim(&mut shown.0);
    link.session.draw(time.delta_secs());
    link.swap_sim(&mut shown.0);
    helis.own_offset = link.session.predictor().map_or(Vec3::ZERO, |p| p.visual_offset());
    helis.own_turn = link.session.predictor().map_or(Quat::IDENTITY, |p| p.visual_turn());
    send_server_events(link, &mut messages, &mut mode_messages);
}

fn send_server_events(
    link: &mut NetLink,
    messages: &mut MessageWriter<SimMessage>,
    mode_messages: &mut MessageWriter<ModeMessage>,
) {
    for event in link.session.drain_server_events() {
        match event {
            WireEvent::Sim(event) => {
                messages.write(SimMessage(event));
            }
            WireEvent::Mode(event) => {
                mode_messages.write(ModeMessage(event));
            }
        }
    }
}

/// Never got in: back to the page's start, which says why and offers playing alone instead of leaving
/// them looking at nothing. Once playing, the last of the game stays on screen with a message.
#[cfg(target_arch = "wasm32")]
fn give_up_on_server(why: &str) {
    if let Some(window) = web_sys::window() {
        let why: String = why.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '+' }).collect();
        let _ = window.location().set_search(&format!("?unreachable={why}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn give_up_on_server(_: &str) {}

fn status_of(link: &mut NetLink, sim: &Sim) -> NetStatus {
    match link.session.state() {
        ClientState::Rejected(RejectReason::Rules) => {
            let you = prs_sim::prs_mods::describe(&prs_sim::prs_mods::active().name);
            let note = link.session.reject_note();
            let server = if note.is_empty() { String::new() } else { format!(" The server runs {note}.") };
            NetStatus::Refused { reason: format!("{}. You run {you}.{server}", refused(RejectReason::Rules)) }
        }
        ClientState::Rejected(RejectReason::UnknownMap) if !link.session.reject_note().is_empty() => {
            NetStatus::Refused { reason: link.session.reject_note().to_owned() }
        }
        ClientState::Rejected(reason) => NetStatus::Refused { reason: refused(reason).into() },
        _ if link.socket.state() == TransportState::Closed => NetStatus::Lost,
        ClientState::Joining => match (link.session.map_wanted(), &link.fetching) {
            (Some(offer), Some(fetch)) => NetStatus::LoadingMap { name: offer.name.clone(), received: fetch.received(), total: offer.bytes as usize },
            _ => NetStatus::Connecting { server: link.server.clone() },
        },
        ClientState::Playing => NetStatus::Playing {
            rtt_ms: link.session.clock().map_or(0.0, |c| c.rtt_ms()),
            lead_ticks: link
                .session
                .predictor()
                .zip(link.session.interp())
                .map_or(0, |(p, i)| p.tick() as i64 - i.play_tick() as i64),
            reconciles: link.session.predictor().map_or(0, |p| p.stats().reconciles),
            players: sim.soldiers.iter().filter(|s| s.present).count(),
            over: if link.socket.is_reliable() { "WebSocket" } else { "WebTransport" },
        },
    }
}

fn refused(reason: RejectReason) -> &'static str {
    match reason {
        RejectReason::Protocol => "it runs a different version",
        RejectReason::Rules => "it plays by different rules or a different map",
        RejectReason::Full => "it is full",
        RejectReason::Kicked => "you were kicked",
        RejectReason::UnknownMap => "you do not have the map it is playing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common_setup;
    use prs_net::server::{GameServer, ServerConfig};
    use prs_net::ws_host::{HostConfig, WsHost};
    use prs_net::wt_host::WtConfig;
    use prs_net_sim::{SimServer, SimServerConfig};
    use std::net::SocketAddr;
    use std::time::Instant;

    fn client(url: &str) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        common_setup(&mut app);
        let mut sim = placeholder_sim();
        app.insert_resource(GameMode(Match::new(Mode::free_practice(), &mut sim)));
        crate::insert_sim(&mut app, sim, 0);
        build(&mut app, url.to_owned(), String::new());
        app
    }

    /// The real thing but for drawing it: Bevy apps with this plugin, over real sockets, to a real server.
    #[test]
    fn two_clients_join_a_real_server_and_see_each_other_on_its_map() {
        let cfg = HostConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            status_every: None,
            run_for: Some(Duration::from_secs(8)),
            webtransport: Some(WtConfig { bind: SocketAddr::from(([127, 0, 0, 1], 0)), ..Default::default() }),
            ..Default::default()
        };
        let host = WsHost::bind(cfg).unwrap();
        let url = format!("ws://{}", host.local_addr().unwrap());
        std::thread::spawn(move || {
            let game = SimServer::new("range", "practice", SimServerConfig::default()).unwrap();
            host.run(GameServer::new(game, ServerConfig::default()));
        });

        let mut apps = [client(&url), client(&url)];
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            for app in &mut apps {
                app.world_mut().resource_mut::<PendingInput>().wish_move = Vec2::new(0.0, 1.0);
                app.update();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut locals = Vec::new();
        for app in &mut apps {
            let status = app.world().resource::<NetStatus>().clone();
            assert!(matches!(status, NetStatus::Playing { players: 2, over: "WebTransport", .. }), "{status:?}");
            assert!(app.world().get_resource::<LevelReady>().is_some());
            let sim = &app.world().resource::<SimResource>().0;
            assert_eq!(sim.level.targets.len(), Level::named("range").unwrap().targets.len(), "the real map");
            locals.push(app.world().resource::<LocalSoldier>().0);
        }
        assert_ne!(locals[0], locals[1], "each is their own soldier");
    }

    /// A server playing a map no client was built with: the first fetches it from the server, checks it and
    /// keeps it, and the second finds it kept.
    #[test]
    fn a_client_fetches_a_map_it_has_never_seen_from_the_server_and_keeps_it() {
        let cache = std::env::temp_dir().join(format!("prs-map-cache-{}", std::process::id()));
        // SAFETY: no other test here reads this, and it is set before anything is fetched.
        unsafe { std::env::set_var("PRS_CACHE_DIR", &cache) };
        let range = MapData::built_in("range").unwrap();
        let text = range.text.replacen("\ntargets = [\n", "\ntargets = [\n  [3.0, 0.5, -40.0],\n", 1);
        let outpost = MapData::new("outpost", text);
        let served = outpost.text.clone();
        let cfg = HostConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            status_every: None,
            run_for: Some(Duration::from_secs(10)),
            plain_http: Some(prs_net::ws_host::PlainHttp(std::sync::Arc::new(move |mut stream, request| {
                use std::io::Write;
                let answer = if request.starts_with("GET /map/outpost.toml") {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{served}", served.len())
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
                };
                let _ = stream.write_all(answer.as_bytes());
            }))),
            ..Default::default()
        };
        let host = WsHost::bind(cfg).unwrap();
        let url = format!("ws://{}", host.local_addr().unwrap());
        let map = outpost.clone();
        std::thread::spawn(move || {
            let game = SimServer::with_map(&map, "practice", SimServerConfig::default()).unwrap();
            host.run(GameServer::new(game, ServerConfig::default()));
        });

        for which in ["fetched", "kept"] {
            let mut app = client(&url);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(4) && app.world().get_resource::<LevelReady>().is_none() {
                app.update();
                std::thread::sleep(Duration::from_millis(5));
            }
            let status = app.world().resource::<NetStatus>().clone();
            assert!(matches!(status, NetStatus::Playing { .. }), "{which}: {status:?}");
            let sim = &app.world().resource::<SimResource>().0;
            assert_eq!(sim.level.source, outpost.hash, "{which}: on the server's map");
            assert_eq!(sim.level.targets.len(), range.level().unwrap().targets.len() + 1, "{which}");
        }
        let kept = std::fs::read_dir(cache.join("prs").join("maps")).unwrap().count();
        let _ = std::fs::remove_dir_all(&cache);
        assert_eq!(kept, 1);
    }
}
