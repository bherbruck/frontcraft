//! Draws other players slightly in the past, between snapshots that have already arrived, so that
//! their motion is smooth however unevenly the snapshots come in.
//!
//! The playback clock runs at real time and is nudged, never jumped, to stay `delay_ticks` behind
//! the newest snapshot. Running out of snapshots extrapolates a little on velocity and then holds.
//!
//! Not everything is in every snapshot: what hasn't changed isn't sent, and far things are sent less
//! often. So each entity has its own track, and is drawn further in the past by however much longer
//! it has been waiting between updates than the snapshots do.

use prs_protocol::TICK_HZ;
use std::collections::{BTreeMap, VecDeque};
use std::marker::PhantomData;

use crate::entity::{MAX_FIELDS, NetEntity};
use crate::game::Schema;

#[derive(Clone, Debug)]
pub struct InterpConfig {
    /// How far behind the newest snapshot to draw. Must cover the gap between snapshots and jitter.
    pub delay_ticks: f32,
    /// Most the playback clock is sped up or slowed down by, as a fraction.
    pub max_rate_adjust: f32,
    /// Fraction of rate change per tick of error.
    pub rate_gain: f32,
    /// Leave the clock alone within this many ticks of where it should be.
    pub deadband: f32,
    /// This far out, stop being gentle and jump.
    pub snap_ticks: f32,
    /// How far past the newest snapshot to keep moving on velocity before holding.
    pub max_extrapolate_ticks: f32,
    /// A player moving further than this between snapshots has teleported and is not blended.
    pub teleport_distance: f32,
    /// How many ticks of an entity to keep, for drawing and for the server to delta against.
    pub keep_ticks: u32,
    /// Weight of a new gap between updates in an entity's average.
    pub gap_smoothing: f32,
}

impl Default for InterpConfig {
    fn default() -> Self {
        Self {
            delay_ticks: 6.0,
            max_rate_adjust: 0.05,
            rate_gain: 0.02,
            deadband: 0.5,
            snap_ticks: 18.0,
            max_extrapolate_ticks: 6.0,
            teleport_distance: 5.0,
            keep_ticks: 180,
            gap_smoothing: 0.2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InterpStats {
    /// Frames drawn past the newest snapshot.
    pub starved_frames: u32,
    /// Times the playback clock had to jump.
    pub snaps: u32,
}

/// How many times its usual gap something has to go quiet for to have been standing still.
const STOOD_STILL: f32 = 4.0;
/// How many times the snapshot gap an entity's own gap has to be to count as sent less often.
const SENT_LESS_OFTEN: f32 = 1.5;

struct Track {
    /// Oldest first.
    updates: VecDeque<(u32, [i32; MAX_FIELDS])>,
    /// About how many ticks there are between its updates while it is changing.
    gap: f32,
}

pub struct Interpolator<S: Schema> {
    cfg: InterpConfig,
    tracks: BTreeMap<u32, Track>,
    /// The tick of the newest snapshot, whatever was in it.
    newest: Option<u32>,
    /// How many ticks apart snapshots are.
    snapshot_every: f32,
    /// Fractional tick being drawn.
    play: f64,
    stats: InterpStats,
    schema: PhantomData<S>,
}

impl<S: Schema> Interpolator<S> {
    pub fn new(cfg: InterpConfig, snapshot_every: u32) -> Self {
        Self {
            cfg,
            tracks: BTreeMap::new(),
            newest: None,
            snapshot_every: snapshot_every.max(1) as f32,
            play: 0.0,
            stats: InterpStats::default(),
            schema: PhantomData,
        }
    }

    pub fn stats(&self) -> InterpStats {
        self.stats
    }

    /// The fractional server tick being drawn right now.
    pub fn play_tick(&self) -> f64 {
        self.play
    }

    /// What this entity was on this tick, if it was in a snapshot then and is still kept.
    pub fn entity_at(&self, key: u32, tick: u32) -> Option<[i32; MAX_FIELDS]> {
        let track = self.tracks.get(&key)?;
        track.updates.iter().find(|(t, _)| *t == tick).map(|(_, fields)| *fields)
    }

    /// Snapshots must be pushed in order. The session drops any that arrive out of it.
    pub fn push(&mut self, tick: u32, entities: &[NetEntity], gone: &[u32]) {
        if self.newest.is_none() {
            self.play = tick as f64 - self.cfg.delay_ticks as f64;
        }
        self.newest = Some(tick);
        for key in gone {
            self.tracks.remove(key);
        }
        for entity in entities {
            let (expected, smoothing) = (self.snapshot_every, self.cfg.gap_smoothing);
            let track = self.tracks.entry(entity.key).or_insert_with(|| Track { updates: VecDeque::new(), gap: expected });
            if let Some(&(last, fields)) = track.updates.back() {
                let gap = (tick - last) as f32;
                // A long wait for something that had not been changing: it stood still until just now,
                // rather than creeping there the whole time. Not for a few lost snapshots.
                if gap > (track.gap * STOOD_STILL).max(self.cfg.delay_ticks * 2.0) {
                    let moved_at = tick.saturating_sub(track.gap.ceil() as u32).max(last + 1);
                    if moved_at > last && moved_at < tick {
                        track.updates.push_back((moved_at, fields));
                    }
                }
                track.gap += (gap.min(track.gap * 4.0) - track.gap) * smoothing;
            }
            track.updates.push_back((tick, entity.fields));
            // The newest one from before the window stays too: the server may still be building on it
            // until it hears this arrived, however long that thing went unchanged.
            while track.updates.get(1).is_some_and(|(t, _)| t + self.cfg.keep_ticks < tick) {
                track.updates.pop_front();
            }
        }
    }

    pub fn advance(&mut self, dt: f32) {
        let Some(newest) = self.newest else { return };
        let error = (newest as f64 - self.cfg.delay_ticks as f64 - self.play) as f32;
        if error.abs() > self.cfg.snap_ticks {
            self.play = newest as f64 - self.cfg.delay_ticks as f64;
            self.stats.snaps += 1;
            return;
        }
        let rate = if error.abs() <= self.cfg.deadband {
            1.0
        } else {
            let max = self.cfg.max_rate_adjust;
            1.0 + (error * self.cfg.rate_gain).clamp(-max, max)
        };
        let step = dt as f64 * TICK_HZ * rate as f64;
        // A frame this long has already jumped on screen, so go straight back to where drawing should
        // be. Left to the gentle clock it would run out of snapshots for seconds.
        if step > self.cfg.max_extrapolate_ticks as f64 {
            self.play = newest as f64 - self.cfg.delay_ticks as f64;
            self.stats.snaps += 1;
            return;
        }
        // With no new snapshots coming in, go no further than drawing can use.
        let furthest = newest as f64 + self.cfg.max_extrapolate_ticks as f64;
        self.play = (self.play + step).min(furthest);
    }

    /// Everyone as they should be drawn now, sorted by key.
    pub fn sample(&mut self) -> Vec<(u32, S::Pose)> {
        let Some(newest) = self.newest else { return Vec::new() };
        let mut starved = false;
        let mut out = Vec::with_capacity(self.tracks.len());
        for (&key, track) in &self.tracks {
            // Something sent a lot less often than the snapshots is drawn that much further back. A
            // snapshot that got lost now and then is not worth moving anything for.
            let behind = if track.gap > self.snapshot_every * SENT_LESS_OFTEN { track.gap - self.snapshot_every } else { 0.0 };
            let play = self.play - behind as f64;
            // Only what comes with every snapshot says anything about snapshots running out.
            let mut ran_out = false;
            let Some(pose) = sample_track::<S>(key, track, play, newest, &self.cfg, &mut ran_out) else { continue };
            starved |= ran_out && behind == 0.0;
            out.push((key, pose));
        }
        self.stats.starved_frames += starved as u32;
        out
    }
}

fn pose_of<S: Schema>(key: u32, fields: &[i32; MAX_FIELDS]) -> S::Pose {
    S::pose(&NetEntity { key, fields: *fields })
}

fn sample_track<S: Schema>(
    key: u32,
    track: &Track,
    play: f64,
    newest: u32,
    cfg: &InterpConfig,
    starved: &mut bool,
) -> Option<S::Pose> {
    let &(last_tick, last) = track.updates.back()?;
    if play >= last_tick as f64 {
        // It wasn't sent because it wasn't changing, so there's nothing to reach for.
        let moving = (newest - last_tick) as f32 <= track.gap * 1.5;
        if !moving {
            return Some(pose_of::<S>(key, &last));
        }
        // Past the newest snapshot of all, rather than past this one going quiet for a moment.
        *starved |= last_tick == newest && play > last_tick as f64;
        let ahead = (play - last_tick as f64) as f32 / TICK_HZ as f32;
        let limit = cfg.max_extrapolate_ticks / TICK_HZ as f32;
        return Some(S::extrapolate_pose(&pose_of::<S>(key, &last), ahead.min(limit)));
    }
    let Some(at) = track.updates.iter().rposition(|(tick, _)| (*tick as f64) <= play) else {
        return Some(pose_of::<S>(key, &track.updates[0].1));
    };
    let (from_tick, from) = &track.updates[at];
    let (to_tick, to) = &track.updates[at + 1];
    let t = ((play - *from_tick as f64) / (*to_tick - *from_tick) as f64) as f32;
    let (start, end) = (pose_of::<S>(key, from), pose_of::<S>(key, to));
    let moved = S::pose_position(&start).distance(S::pose_position(&end));
    Some(if moved > cfg.teleport_distance { end } else { S::lerp_pose(&start, &end, t) })
}
