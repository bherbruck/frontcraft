//! The wire format. Hand written because it is small, has to be identical on wasm and native, and
//! has to survive anything an attacker sends: every decode returns an error rather than panicking.
//!
//! Every message is one transport frame, so a WebSocket message or a datagram carries it unchanged.
//! All integers are little endian or varints; world state is quantised and sent as deltas against
//! a baseline the client is known to hold, so an idle player costs a few bytes.

use glam::{IVec3, Vec2, Vec3};
use prs_protocol::{Buttons, PlayerCmd};

use crate::entity::{MAX_FIELDS, NetEntity, read_count, read_update, write_update};
use crate::game::Schema;

/// Bumped on any change to this file. A game's own format is covered by its rules hash.
pub const PROTOCOL_VERSION: u16 = 7;

/// What the host's frames must not exceed. Set well above a full 64 player snapshot.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_CMDS_PER_PACKET: usize = 64;
const MAX_REQUESTS_PER_PACKET: usize = 32;
const MAX_REQUEST_BYTES: usize = 256;
/// The most any voice frame could ever be. A server's own limit is well under this.
const MAX_VOICE_BYTES: usize = 4096;
/// How many players one client can say it has muted.
const MAX_VOICE_MUTED: usize = 1024;
const MAX_EVENTS_PER_SNAPSHOT: usize = 4096;
pub(crate) const MAX_ENTITIES: usize = 1024;

const TAG_HELLO: u8 = 1;
const TAG_CMDS: u8 = 2;
const TAG_JOIN: u8 = 3;
const TAG_VOICE_UP: u8 = 4;
const TAG_VOICE_SET: u8 = 5;
const TAG_WELCOME: u8 = 16;
const TAG_REJECT: u8 = 17;
const TAG_SNAPSHOT: u8 = 18;
const TAG_INFO: u8 = 19;
const TAG_VOICE: u8 = 20;
const MAX_NAME: usize = 256;

/// Whether a frame has to arrive, rather than being one of a stream where the next makes up for a lost one.
/// A transport that can lose things sends these the slow, sure way.
pub fn must_arrive(frame: &[u8]) -> bool {
    matches!(frame.first(), Some(&(TAG_HELLO | TAG_JOIN | TAG_VOICE_SET | TAG_INFO | TAG_WELCOME | TAG_REJECT)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    BadTag,
    TooMany,
    BadVarint,
    /// A delta against a snapshot this client no longer has, or never had.
    MissingBaseline,
}

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }

    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }

    pub fn varu(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf.push(v as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }

    pub fn vari(&mut self, v: i64) {
        self.varu(((v << 1) ^ (v >> 63)) as u64);
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let (head, rest) = self.buf.split_first_chunk::<N>().ok_or(WireError::Truncated)?;
        self.buf = rest;
        Ok(*head)
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take::<1>()?[0])
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let (head, rest) = self.buf.split_at_checked(len).ok_or(WireError::Truncated)?;
        self.buf = rest;
        Ok(head)
    }

    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.take()?))
    }

    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_le_bytes(self.take()?))
    }

    pub fn vec2(&mut self) -> Result<Vec2, WireError> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }

    pub fn vec3(&mut self) -> Result<Vec3, WireError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    pub fn varu(&mut self) -> Result<u64, WireError> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(WireError::BadVarint)
    }

    pub fn vari(&mut self) -> Result<i64, WireError> {
        let v = self.varu()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }

    /// A tick, or anything else that has to fit 32 bits.
    pub fn varu32(&mut self) -> Result<u32, WireError> {
        u32::try_from(self.varu()?).map_err(|_| WireError::BadVarint)
    }

    /// A count of things to read, refused above `max` so a short frame cannot ask for a huge allocation.
    pub fn count(&mut self, max: usize) -> Result<usize, WireError> {
        let n = self.varu()? as usize;
        if n > max { Err(WireError::TooMany) } else { Ok(n) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientMsg {
    /// The first thing a client says, to find out what is being played.
    Hello { protocol: u16 },
    /// Now set up for what is being played, asks for a player. `rules` must match the server's.
    /// `name` is what they would like to be called, which the server cleans up as it likes.
    Join { rules: u64, name: String },
    Cmds(CmdPacket),
    /// A moment of someone talking. Whoever hears it is for the server to say.
    VoiceUp(VoiceUp),
    /// Whether to send this client voice at all, and whose never to send it. The whole list each time.
    VoiceSet { listening: bool, muted: Vec<u16> },
}

/// Who a voice frame is for. There is deliberately nothing here to name a listener with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceChannel {
    /// Out loud, for whoever is close enough, on either team.
    Local,
    /// Over the radio, for the talker's squad wherever they are.
    Squad,
}

impl VoiceChannel {
    fn from_u8(value: u8) -> Result<Self, WireError> {
        match value {
            0 => Ok(Self::Local),
            1 => Ok(Self::Squad),
            _ => Err(WireError::BadTag),
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Local => 0,
            Self::Squad => 1,
        }
    }
}

/// One compressed moment of voice on its way to the server. The bytes mean nothing to anyone but a
/// listener's decoder, so the server only ever looks at how many there are.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceUp {
    pub channel: VoiceChannel,
    /// Counts up and wraps, so that a listener can tell when some went missing.
    pub seq: u8,
    pub bytes: Vec<u8>,
}

/// One moment of voice on its way to a listener, with who said it filled in by the server.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceFrame {
    pub speaker: u16,
    pub channel: VoiceChannel,
    pub seq: u8,
    /// Where it was said, in quarter metres. Nothing over the radio, which comes from nowhere.
    pub at: Option<[i32; 3]>,
    pub bytes: Vec<u8>,
}

/// Quarter metres, near enough for an ear and no use for finding someone more exactly than hearing them.
const VOICE_AT_PER_METRE: f32 = 4.0;

impl VoiceFrame {
    pub fn place(position: Vec3) -> [i32; 3] {
        (position * VOICE_AT_PER_METRE).round().as_ivec3().to_array()
    }

    pub fn position(&self) -> Option<Vec3> {
        self.at.map(|at| IVec3::from_array(at).as_vec3() / VOICE_AT_PER_METRE)
    }
}

pub fn encode_voice(frame: &VoiceFrame) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(TAG_VOICE);
    w.u16(frame.speaker);
    w.u8(frame.channel.to_u8());
    w.u8(frame.seq);
    if let (VoiceChannel::Local, Some([x, y, z])) = (frame.channel, frame.at) {
        w.vari(x as i64);
        w.vari(y as i64);
        w.vari(z as i64);
    }
    w.varu(frame.bytes.len() as u64);
    w.buf.extend_from_slice(&frame.bytes);
    w.buf
}

fn read_voice(r: &mut Reader) -> Result<VoiceFrame, WireError> {
    let speaker = r.u16()?;
    let channel = VoiceChannel::from_u8(r.u8()?)?;
    let seq = r.u8()?;
    let at = if channel == VoiceChannel::Local {
        Some([r.vari()? as i32, r.vari()? as i32, r.vari()? as i32])
    } else {
        None
    };
    let len = r.count(MAX_VOICE_BYTES)?;
    Ok(VoiceFrame { speaker, channel, seq, at, bytes: r.bytes(len)?.to_vec() })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CmdPacket {
    /// The newest snapshot this client has, so the server can delta against it. 0 means none.
    pub ack_snapshot: u32,
    /// Which of the snapshots before that it has as well: bit `n` is the one `n + 1` snapshots back.
    /// Everything is not in every snapshot, so a client having a later one doesn't say it has an earlier.
    pub ack_bits: u32,
    /// The client's clock when this was sent, echoed back to measure round trip time.
    pub sent_ms: u16,
    /// The server tick this client is drawing everyone else at, so that its shots can be judged
    /// against what it saw. 0 before it has seen anything.
    pub view_tick: u32,
    /// Oldest first. More than one only when the transport can lose packets.
    pub cmds: Vec<PlayerCmd>,
    /// Requests of the game beyond moving a soldier, in its own format. Resent until acknowledged.
    pub requests: Vec<(u32, Vec<u8>)>,
}

impl ClientMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Self::Hello { protocol } => {
                w.u8(TAG_HELLO);
                w.u16(*protocol);
            }
            Self::Join { rules, name } => {
                w.u8(TAG_JOIN);
                w.u64(*rules);
                write_text(name, &mut w);
            }
            Self::Cmds(packet) => {
                w.u8(TAG_CMDS);
                w.varu(packet.ack_snapshot as u64);
                w.varu(packet.ack_bits as u64);
                w.u16(packet.sent_ms);
                w.varu(packet.view_tick as u64);
                write_cmds(&mut w, &packet.cmds);
                w.varu(packet.requests.len() as u64);
                for (seq, bytes) in &packet.requests {
                    w.varu(*seq as u64);
                    w.varu(bytes.len() as u64);
                    w.buf.extend_from_slice(bytes);
                }
            }
            Self::VoiceUp(up) => {
                w.u8(TAG_VOICE_UP);
                w.u8(up.channel.to_u8());
                w.u8(up.seq);
                w.varu(up.bytes.len() as u64);
                w.buf.extend_from_slice(&up.bytes);
            }
            Self::VoiceSet { listening, muted } => {
                w.u8(TAG_VOICE_SET);
                w.u8(u8::from(*listening));
                w.varu(muted.len() as u64);
                for player in muted {
                    w.u16(*player);
                }
            }
        }
        w.buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(bytes);
        match r.u8()? {
            TAG_HELLO => Ok(Self::Hello { protocol: r.u16()? }),
            TAG_JOIN => Ok(Self::Join { rules: r.u64()?, name: read_text(&mut r)? }),
            TAG_CMDS => Ok(Self::Cmds(CmdPacket {
                ack_snapshot: r.varu32()?,
                ack_bits: r.varu32()?,
                sent_ms: r.u16()?,
                view_tick: r.varu32()?,
                cmds: read_cmds(&mut r)?,
                requests: read_requests(&mut r)?,
            })),
            TAG_VOICE_UP => {
                let channel = VoiceChannel::from_u8(r.u8()?)?;
                let seq = r.u8()?;
                let len = r.count(MAX_VOICE_BYTES)?;
                Ok(Self::VoiceUp(VoiceUp { channel, seq, bytes: r.bytes(len)?.to_vec() }))
            }
            TAG_VOICE_SET => {
                let listening = r.u8()? != 0;
                let n = r.count(MAX_VOICE_MUTED)?;
                let mut muted = Vec::with_capacity(n);
                for _ in 0..n {
                    muted.push(r.u16()?);
                }
                Ok(Self::VoiceSet { listening, muted })
            }
            _ => Err(WireError::BadTag),
        }
    }
}

fn read_requests(r: &mut Reader) -> Result<Vec<(u32, Vec<u8>)>, WireError> {
    let n = r.count(MAX_REQUESTS_PER_PACKET)?;
    let mut requests = Vec::with_capacity(n);
    for _ in 0..n {
        let seq = r.varu32()?;
        let len = r.count(MAX_REQUEST_BYTES)?;
        requests.push((seq, r.bytes(len)?.to_vec()));
    }
    Ok(requests)
}

const CMD_WISH: u8 = 1 << 0;
const CMD_YAW: u8 = 1 << 1;
const CMD_PITCH: u8 = 1 << 2;
const CMD_BUTTONS: u8 = 1 << 3;
const CMD_STICK: u8 = 1 << 4;

/// Commands are sent at full precision: the server has to get exactly what the client predicted
/// with, or every shot of mouse movement would be a misprediction. Only unchanged fields are left out.
fn write_cmds(w: &mut Writer, cmds: &[PlayerCmd]) {
    w.varu(cmds.len() as u64);
    let Some(first) = cmds.first() else { return };
    w.varu(first.tick as u64);
    let mut prev = PlayerCmd { tick: first.tick, ..Default::default() };
    for cmd in cmds {
        w.varu(cmd.tick.wrapping_sub(prev.tick) as u64);
        let mut mask = 0;
        if cmd.wish_move.x.to_bits() != prev.wish_move.x.to_bits()
            || cmd.wish_move.y.to_bits() != prev.wish_move.y.to_bits()
        {
            mask |= CMD_WISH;
        }
        if cmd.yaw.to_bits() != prev.yaw.to_bits() {
            mask |= CMD_YAW;
        }
        if cmd.pitch.to_bits() != prev.pitch.to_bits() {
            mask |= CMD_PITCH;
        }
        if cmd.buttons != prev.buttons {
            mask |= CMD_BUTTONS;
        }
        if cmd.stick != prev.stick {
            mask |= CMD_STICK;
        }
        w.u8(mask);
        if mask & CMD_WISH != 0 {
            w.vec2(cmd.wish_move);
        }
        if mask & CMD_YAW != 0 {
            w.f32(cmd.yaw);
        }
        if mask & CMD_PITCH != 0 {
            w.f32(cmd.pitch);
        }
        if mask & CMD_BUTTONS != 0 {
            w.varu(cmd.buttons.0 as u64);
        }
        if mask & CMD_STICK != 0 {
            w.u8(cmd.stick[0] as u8);
            w.u8(cmd.stick[1] as u8);
        }
        prev = *cmd;
    }
}

fn read_cmds(r: &mut Reader) -> Result<Vec<PlayerCmd>, WireError> {
    let n = r.count(MAX_CMDS_PER_PACKET)?;
    let mut cmds = Vec::with_capacity(n);
    if n == 0 {
        return Ok(cmds);
    }
    let mut prev = PlayerCmd { tick: r.varu32()?, ..Default::default() };
    for _ in 0..n {
        let tick = prev.tick.wrapping_add(r.varu32()?);
        let mask = r.u8()?;
        let mut cmd = PlayerCmd { tick, ..prev };
        if mask & CMD_WISH != 0 {
            cmd.wish_move = r.vec2()?;
        }
        if mask & CMD_YAW != 0 {
            cmd.yaw = r.f32()?;
        }
        if mask & CMD_PITCH != 0 {
            cmd.pitch = r.f32()?;
        }
        if mask & CMD_BUTTONS != 0 {
            cmd.buttons = Buttons(r.varu32()?);
        }
        if mask & CMD_STICK != 0 {
            cmd.stick = [r.u8()? as i8, r.u8()? as i8];
        }
        cmds.push(cmd);
        prev = cmd;
    }
    Ok(cmds)
}

/// Why a server turned a client away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    Protocol = 0,
    Rules = 1,
    Full = 2,
    Kicked = 3,
    /// Never sent: a client gives itself this if it does not have the map being played.
    UnknownMap = 4,
}

impl RejectReason {
    fn from_u8(v: u8) -> Result<Self, WireError> {
        Ok(match v {
            0 => Self::Protocol,
            1 => Self::Rules,
            2 => Self::Full,
            3 => Self::Kicked,
            4 => Self::UnknownMap,
            _ => return Err(WireError::BadTag),
        })
    }
}

pub struct Welcome<S: Schema> {
    pub soldier: u16,
    pub tick: u32,
    /// How the player stands right now, whole. It is held so until the client's first command, so the
    /// client starts exactly where the server does, whatever the game did to it on the way in.
    pub own: S::Own,
    pub snapshot_every: u32,
    pub baseline_window: u32,
}

pub enum ServerMsg<S: Schema> {
    /// What is being played, so a client can set up the same before asking to join, and whether this
    /// server carries voice. The map comes with the hash and length of its file, so a client can tell
    /// whether the copy it has is the same one, or fetch it from the server.
    Info { map: String, mode: String, voice: bool, map_hash: u64, map_bytes: u32 },
    Welcome(Welcome<S>),
    /// Why, and for different rules a note from the server saying what it is running.
    Reject(RejectReason, String),
    Snapshot(Snapshot<S>),
    Voice(VoiceFrame),
}

pub struct Snapshot<S: Schema> {
    pub tick: u32,
    /// How many ticks early the client's commands have been arriving. Negative means too late.
    /// `None` if nothing has arrived since the last snapshot.
    pub cmd_slack: Option<i32>,
    /// The client's `sent_ms` and how many ticks the server sat on it, for round trip time.
    pub echo: Option<(u16, u8)>,
    /// The newest of the client's requests the server has taken in.
    pub request_ack: u32,
    /// The client's own player, or `None` until the server has started running it.
    pub own: Option<S::Own>,
    /// Whatever has changed that there was room for, whole, whatever the delta on the wire was. What
    /// is not here is as it was, not gone: there is no time for everything in every snapshot.
    pub entities: Vec<NetEntity>,
    /// What this client should forget: it has left the game, or gone out of range.
    pub gone: Vec<u32>,
    /// World state that changes rarely, in the game's own format. Only sent when it has changed.
    pub shared: Vec<u8>,
    pub events: Vec<(u32, S::Event)>,
}

/// What a client is known to hold from an earlier snapshot, for sending only what is different.
pub struct BaseRef<'a, S: Schema> {
    pub own: Option<&'a S::Own>,
    pub shared: &'a [u8],
}

/// Lets a decoder find what a delta was made against.
pub trait Baselines<S: Schema> {
    fn base_at(&self, tick: u32) -> Option<BaseRef<'_, S>>;
    fn entity_at(&self, key: u32, tick: u32) -> Option<[i32; MAX_FIELDS]>;
}

/// One entity on its way out, with what it is a delta against: how many ticks back, and the fields.
pub struct EntityOut<'a> {
    pub entity: &'a NetEntity,
    pub base: Option<(u32, &'a [i32; MAX_FIELDS])>,
}

const HAS_SLACK: u8 = 1 << 0;
const HAS_ECHO: u8 = 1 << 1;
const HAS_OWN: u8 = 1 << 2;
const HAS_SHARED: u8 = 1 << 3;

/// Everything in a snapshot but the entities, up to where they go. Then come [`write_entities`] and
/// [`write_snapshot_end`], so that a server can see how much room is left as it goes.
pub fn write_snapshot_start<S: Schema>(snapshot: &Snapshot<S>, base_tick: u32, base: Option<&BaseRef<S>>) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(TAG_SNAPSHOT);
    w.varu(snapshot.tick as u64);
    w.varu(if base.is_some() { snapshot.tick.wrapping_sub(base_tick) as u64 } else { 0 });
    let send_shared = base.is_none_or(|b| b.shared != snapshot.shared);
    let flags = if snapshot.cmd_slack.is_some() { HAS_SLACK } else { 0 }
        | if snapshot.echo.is_some() { HAS_ECHO } else { 0 }
        | if snapshot.own.is_some() { HAS_OWN } else { 0 }
        | if send_shared { HAS_SHARED } else { 0 };
    w.u8(flags);
    w.varu(snapshot.request_ack as u64);
    if let Some(slack) = snapshot.cmd_slack {
        w.vari(slack as i64);
    }
    if let Some((ms, held)) = snapshot.echo {
        w.u16(ms);
        w.u8(held);
    }
    if let Some(own) = &snapshot.own {
        S::write_own(own, base.and_then(|b| b.own), &mut w);
    }
    if send_shared {
        w.varu(snapshot.shared.len() as u64);
        w.buf.extend_from_slice(&snapshot.shared);
    }
    w.varu(snapshot.events.len() as u64);
    let mut last = snapshot.tick;
    for (tick, event) in &snapshot.events {
        w.vari(last as i64 - *tick as i64);
        last = *tick;
        S::write_event(event, &mut w);
    }
    w.varu(snapshot.gone.len() as u64);
    for key in &snapshot.gone {
        w.varu(*key as u64);
    }
    w.buf
}

/// How many bytes one entity takes as a delta, for deciding whether there is room for it.
pub fn encode_entity<S: Schema>(out: &EntityOut) -> Vec<u8> {
    let mut w = Writer::default();
    let groups = S::groups(out.entity.kind()).expect("the game's own kind of entity");
    write_update(&mut w, groups, out.entity, out.base);
    w.buf
}

/// Adds the entities, already encoded, to finish a snapshot.
pub fn finish_snapshot(mut start: Vec<u8>, entities: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::default();
    w.varu(entities.len() as u64);
    start.extend_from_slice(&w.buf);
    for entity in entities {
        start.extend_from_slice(entity);
    }
    start
}

pub fn write_text(text: &str, w: &mut Writer) {
    w.varu(text.len() as u64);
    w.buf.extend_from_slice(text.as_bytes());
}

/// Any bytes that aren't UTF-8 become replacement characters, so it is always a string.
pub fn read_text(r: &mut Reader) -> Result<String, WireError> {
    let len = r.count(MAX_NAME)?;
    Ok(String::from_utf8_lossy(r.bytes(len)?).into_owned())
}

pub fn encode_info(map: &str, mode: &str, voice: bool, map_hash: u64, map_bytes: u32) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(TAG_INFO);
    write_text(map, &mut w);
    write_text(mode, &mut w);
    w.u8(u8::from(voice));
    w.u64(map_hash);
    w.varu(map_bytes as u64);
    w.buf
}

pub fn encode_welcome<S: Schema>(welcome: &Welcome<S>) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(TAG_WELCOME);
    w.u16(welcome.soldier);
    w.varu(welcome.tick as u64);
    w.varu(welcome.snapshot_every as u64);
    w.varu(welcome.baseline_window as u64);
    S::write_own(&welcome.own, None, &mut w);
    w.buf
}

pub fn encode_reject(reason: RejectReason, note: &str) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(TAG_REJECT);
    w.u8(reason as u8);
    write_text(note, &mut w);
    w.buf
}

impl<S: Schema> ServerMsg<S> {
    pub fn decode(bytes: &[u8], baselines: &dyn Baselines<S>) -> Result<Self, WireError> {
        let mut r = Reader::new(bytes);
        match r.u8()? {
            TAG_INFO => Ok(Self::Info {
                map: read_text(&mut r)?,
                mode: read_text(&mut r)?,
                voice: r.u8()? != 0,
                map_hash: r.u64()?,
                map_bytes: r.varu32()?,
            }),
            TAG_VOICE => Ok(Self::Voice(read_voice(&mut r)?)),
            TAG_WELCOME => Ok(Self::Welcome(Welcome {
                soldier: r.u16()?,
                tick: r.varu32()?,
                snapshot_every: r.varu32()?,
                baseline_window: r.varu32()?,
                own: S::read_own(None, &mut r)?,
            })),
            TAG_REJECT => Ok(Self::Reject(RejectReason::from_u8(r.u8()?)?, read_text(&mut r)?)),
            TAG_SNAPSHOT => Ok(Self::Snapshot(read_snapshot::<S>(&mut r, baselines)?)),
            _ => Err(WireError::BadTag),
        }
    }
}

fn read_snapshot<S: Schema>(r: &mut Reader, baselines: &dyn Baselines<S>) -> Result<Snapshot<S>, WireError> {
    let tick = r.varu32()?;
    let back = r.varu32()?;
    let flags = r.u8()?;
    let request_ack = r.varu32()?;
    let cmd_slack = if flags & HAS_SLACK != 0 { Some(r.vari()? as i32) } else { None };
    let echo = if flags & HAS_ECHO != 0 { Some((r.u16()?, r.u8()?)) } else { None };
    let base = if back == 0 {
        None
    } else {
        Some(baselines.base_at(tick.wrapping_sub(back)).ok_or(WireError::MissingBaseline)?)
    };
    let own = if flags & HAS_OWN != 0 { Some(S::read_own(base.as_ref().and_then(|b| b.own), r)?) } else { None };
    let shared = if flags & HAS_SHARED != 0 {
        let len = r.count(MAX_FRAME_BYTES)?;
        r.bytes(len)?.to_vec()
    } else {
        base.as_ref().map_or(Vec::new(), |b| b.shared.to_vec())
    };
    let n = r.count(MAX_EVENTS_PER_SNAPSHOT)?;
    let mut events = Vec::with_capacity(n.min(64));
    let mut last = tick;
    for _ in 0..n {
        last = (last as i64 - r.vari()?) as u32;
        events.push((last, S::read_event(r)?));
    }
    let n = read_count(r)?;
    let mut gone = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        gone.push(r.varu32()?);
    }
    let n = read_count(r)?;
    let mut entities = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let base_of = |key: u32, back: u32| baselines.entity_at(key, tick.wrapping_sub(back));
        entities.push(read_update(r, S::groups, &base_of)?);
    }
    Ok(Snapshot { tick, cmd_slack, echo, request_ack, own, entities, gone, shared, events })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoBaselines;
    impl Baselines<Plain> for NoBaselines {
        fn base_at(&self, _: u32) -> Option<BaseRef<'_, Plain>> {
            None
        }
        fn entity_at(&self, _: u32, _: u32) -> Option<[i32; MAX_FIELDS]> {
            None
        }
    }

    /// A schema with a little of everything, for feeding garbage to.
    struct Plain;
    impl Schema for Plain {
        type Own = f32;
        type Event = u8;
        type Pose = Vec3;
        fn groups(kind: u8) -> Option<&'static [(usize, usize)]> {
            (kind == 0).then_some(&[(0, 3)][..])
        }
        fn write_own(own: &f32, _: Option<&f32>, w: &mut Writer) {
            w.f32(*own);
        }
        fn read_own(_: Option<&f32>, r: &mut Reader) -> Result<f32, WireError> {
            r.f32()
        }
        fn urgent(_: &u8, _: u16) -> bool {
            false
        }
        fn own_matches(a: &f32, b: &f32, epsilon: f32) -> bool {
            (a - b).abs() <= epsilon
        }
        fn own_position(_: &f32) -> Vec3 {
            Vec3::ZERO
        }
        fn write_event(event: &u8, w: &mut Writer) {
            w.u8(*event);
        }
        fn read_event(r: &mut Reader) -> Result<u8, WireError> {
            r.u8()
        }
        fn pose(_: &NetEntity) -> Vec3 {
            Vec3::ZERO
        }
        fn lerp_pose(a: &Vec3, _: &Vec3, _: f32) -> Vec3 {
            *a
        }
        fn pose_position(pose: &Vec3) -> Vec3 {
            *pose
        }
        fn extrapolate_pose(pose: &Vec3, _: f32) -> Vec3 {
            *pose
        }
    }

    #[test]
    fn varints_round_trip() {
        for v in [0i64, 1, -1, 63, -64, 64, 1 << 40, -(1 << 40), i64::MAX, i64::MIN] {
            let mut w = Writer::default();
            w.vari(v);
            assert_eq!(Reader::new(&w.buf).vari(), Ok(v));
        }
    }

    #[test]
    fn commands_keep_every_bit_of_mouse_movement() {
        let packet = CmdPacket {
            ack_snapshot: 77,
            ack_bits: 5,
            sent_ms: 1234,
            view_tick: 990,
            requests: vec![(4, vec![1, 2, 3])],
            cmds: (0..5)
                .map(|i| PlayerCmd {
                    tick: 1000 + i,
                    wish_move: Vec2::new(0.1, i as f32 * 0.2),
                    yaw: 1.0 / (i + 3) as f32,
                    pitch: -0.25,
                    buttons: Buttons(i as u32 % 2),
                    stick: [i as i8, -(i as i8)],
                })
                .collect(),
        };
        let bytes = ClientMsg::Cmds(packet.clone()).encode();
        assert_eq!(ClientMsg::decode(&bytes), Ok(ClientMsg::Cmds(packet)));
    }

    /// Garbage from anyone must come back as an error, never a panic or a huge allocation.
    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let mut seed = 0x1234_5678_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..20_000 {
            let len = next() as usize % 80;
            let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = [TAG_HELLO, TAG_JOIN, TAG_CMDS, TAG_INFO, TAG_WELCOME, TAG_REJECT, TAG_SNAPSHOT][next() as usize % 7];
            }
            let _ = ClientMsg::decode(&bytes);
            let _ = ServerMsg::<Plain>::decode(&bytes, &NoBaselines);
        }
    }

    #[test]
    fn truncating_a_message_anywhere_is_an_error() {
        let bytes = ClientMsg::Cmds(CmdPacket {
            ack_snapshot: 9,
            ack_bits: 0,
            sent_ms: 1,
            view_tick: 3,
            requests: Vec::new(),
            cmds: vec![PlayerCmd { tick: 5, yaw: 1.0, ..Default::default() }],
        })
        .encode();
        for cut in 0..bytes.len() {
            assert!(ClientMsg::decode(&bytes[..cut]).is_err(), "cut at {cut}");
        }
    }
}
