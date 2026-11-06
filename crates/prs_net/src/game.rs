//! All this crate needs to know about a game. `prs_net_sim` implements these for the real
//! simulation, and the tests for a toy.

use glam::{Quat, Vec3};
use prs_protocol::PlayerCmd;
use std::fmt::Debug;

use crate::entity::NetEntity;
use crate::predict::CorrectionStyle;
use crate::wire::{Reader, VoiceChannel, Welcome, WireError, Writer};

/// What decides how often something is sent to a client when there isn't room for everything.
#[derive(Clone, Copy, Debug)]
pub struct Relevance {
    pub position: Vec3,
    /// Multiplies its share of attention. A soldier is 1, and something big seen from afar is more.
    pub importance: f32,
    /// Beyond this a client does not know of it at all, in metres.
    pub max_range: f32,
}

/// Where a client is looking from, for deciding what matters to it.
#[derive(Clone, Copy, Debug)]
pub struct Viewer {
    pub position: Vec3,
    pub forward: Vec3,
}

/// What of a game goes on the wire.
pub trait Schema: 'static {
    /// The state a client predicts for itself and gets told at full precision, to compare exactly.
    /// Must hold everything that decides later ticks, timers and random generators too. Whatever is
    /// left out stays wrong after a misprediction, and every tick from then on disagrees.
    type Own: Clone + Debug + PartialEq;
    /// Things that happen, once, that a client could not work out for itself.
    type Event: Clone + Debug + PartialEq;
    /// Someone else in world units, as drawn at one moment.
    type Pose: Clone;

    /// How each kind of [`NetEntity`]'s fields are grouped for deltas: (first field, how many). Fields
    /// that change together should share a group, as each group costs a bit in the mask.
    fn groups(kind: u8) -> Option<&'static [(usize, usize)]>;

    /// `base` is what the client is known to hold already, if anything, for sending less.
    fn write_own(own: &Self::Own, base: Option<&Self::Own>, w: &mut Writer);
    fn read_own(base: Option<&Self::Own>, r: &mut Reader) -> Result<Self::Own, WireError>;
    /// Whether the two agree, to `epsilon` on anything continuous and exactly on anything discrete.
    fn own_matches(a: &Self::Own, b: &Self::Own, epsilon: f32) -> bool;
    fn own_position(own: &Self::Own) -> Vec3;

    fn write_event(event: &Self::Event, w: &mut Writer);
    fn read_event(r: &mut Reader) -> Result<Self::Event, WireError>;
    /// Whether this should reach player `me` at once. Anything else waits until the moment it
    /// happened comes round on screen, so that a shot lines up with the soldier firing it.
    fn urgent(event: &Self::Event, me: u16) -> bool;

    fn pose(entity: &NetEntity) -> Self::Pose;
    fn lerp_pose(a: &Self::Pose, b: &Self::Pose, t: f32) -> Self::Pose;
    fn pose_position(pose: &Self::Pose) -> Vec3;
    /// The pose moved on `seconds` by its own velocity.
    fn extrapolate_pose(pose: &Self::Pose, seconds: f32) -> Self::Pose;
}

/// The game as a client runs it, to predict its own player.
pub trait PredictedGame {
    type S: Schema;
    /// Everything needed to go back to this moment, private state included.
    type Saved: Clone;
    /// What a predicted tick has to show or play straight away.
    type Event;

    /// Must match the server's [`ServerGame::rules_hash`], or prediction would be wrong every tick.
    fn rules_hash(&self) -> u64;
    /// Takes in world state that changes rarely, as given by [`ServerGame::shared`].
    fn apply_shared(&mut self, shared: &[u8]);
    /// Something the server says happened, as it comes to be shown.
    /// Says whether the player has just been put somewhere new, so that it is shown as a jump.
    fn on_server_event(&mut self, event: &<Self::S as Schema>::Event) -> bool;
    /// The server has given us a player, standing as `welcome.own` says.
    fn join(&mut self, welcome: &Welcome<Self::S>);
    /// Runs one command for the local player. `events` is `None` on a replay, as those events have
    /// already been shown.
    fn step(&mut self, cmd: &PlayerCmd, events: Option<&mut Vec<Self::Event>>);
    fn save(&self) -> Self::Saved;
    fn restore(&mut self, saved: &Self::Saved);
    fn own_of(saved: &Self::Saved) -> <Self::S as Schema>::Own;
    fn apply_own(saved: &mut Self::Saved, own: &<Self::S as Schema>::Own);
    fn position(&self) -> Vec3;
    /// Which way whatever the player is at the controls of faces. Corrections to this are blended too.
    fn attitude(&self) -> Quat {
        Quat::IDENTITY
    }
    /// How a correction should show on the player as they are now.
    fn correction_style(&self, usual: &CorrectionStyle) -> CorrectionStyle {
        *usual
    }
    /// The tick everybody else is about to be shown at, ahead of [`Self::show_remotes`].
    fn remotes_from(&mut self, _tick: f64) {}
    /// Everybody else, at the poses to draw them this frame.
    fn show_remotes(&mut self, remotes: &[(u32, <Self::S as Schema>::Pose)], dt: f32);
}

/// The game as the server runs it, the only copy that counts.
pub trait ServerGame {
    type S: Schema;

    fn rules_hash(&self) -> u64;
    /// What is being played, for a client to set up the same.
    fn map_name(&self) -> String;
    /// The hash and length of the map's file, for a client to check its copy against or fetch it by.
    fn map_file(&self) -> (u64, u32) {
        (0, 0)
    }
    fn mode_name(&self) -> String;
    fn tick(&self) -> u32;
    /// A new player, held exactly as they are until [`Self::start`].
    fn spawn(&mut self, seed: u32) -> u16;
    /// A player's first command has come, so from now on they are in the game.
    fn start(&mut self, id: u16);
    fn remove(&mut self, id: u16);
    /// How many ticks in the past this player sees everyone else, as of their latest command.
    fn set_view_lag(&mut self, id: u16, ticks: u32);
    /// Where a player is looking now: (yaw, pitch).
    fn look(&self, id: u16) -> (f32, f32);
    /// Advances one tick. `cmds` holds one command for each player that acted.
    fn step(&mut self, cmds: &[(u16, PlayerCmd)], events: &mut Vec<<Self::S as Schema>::Event>);
    /// Everything anyone needs to see, with players and whatever else moves, sorted by key.
    fn capture_world(&self) -> Vec<(NetEntity, Relevance)>;
    fn viewer(&self, id: u16) -> Viewer;
    /// Where this happened and how far off anyone cares. `None` if everyone does, wherever they are.
    fn event_reach(&self, event: &<Self::S as Schema>::Event) -> Option<(Vec3, f32)>;
    /// A request from a player that is not about moving their soldier. Untrusted, like everything.
    fn request(&mut self, id: u16, bytes: &[u8]);
    fn own(&self, id: u16) -> <Self::S as Schema>::Own;
    /// World state that changes rarely. Sent whenever these bytes differ from what a client has.
    fn shared(&self) -> Vec<u8>;
    /// Whether this player predicted this event itself, so it should not hear it again.
    fn predicted_by(event: &<Self::S as Schema>::Event, id: u16) -> bool;
    /// What a player is called, already cleaned up. Never part of the rules: it changes nothing.
    fn set_name(&mut self, _id: u16, _name: &str) {}
    /// Where a voice on this channel comes from. `None` if this player can't be heard on it just now.
    /// The radio comes from nowhere, so any place will do for it.
    fn voice_from(&self, _speaker: u16, _channel: VoiceChannel) -> Option<Vec3> {
        None
    }
    /// Whether one player can hear another on this channel. Decides everything about who gets a voice.
    fn voice_reaches(&self, _speaker: u16, _listener: u16, _channel: VoiceChannel) -> bool {
        false
    }
}
