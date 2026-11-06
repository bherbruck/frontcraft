//! Runs the client's own player ahead of the server with the same game, and puts it right.
//!
//! Each predicted tick keeps the command and the player as it stood afterwards. When the server's
//! word on a tick arrives it is compared with what was predicted for that tick. If they agree
//! nothing happens at all. If not, the player goes back to that tick, takes the server's values,
//! and replays every command since. The jump that leaves is moved into a visual offset that decays,
//! so the state is right at once and the picture catches up without a pop.

use glam::{Quat, Vec3};
use prs_protocol::PlayerCmd;
use std::collections::VecDeque;

use crate::game::{PredictedGame, Schema};

/// How a correction is shown, which depends on what is being corrected: a soldier can be teleported
/// and hardly notice, and the same thing in a cockpit makes people ill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CorrectionStyle {
    /// How fast the visual offset dies away, per second.
    pub smoothing_rate: f32,
    /// An error bigger than this is a teleport, and is shown as one.
    pub snap_distance: f32,
    /// Fastest it may die away, m/s and rad/s, however large it is. A big one then shows as a gentle
    /// slide instead of a lurch.
    pub max_speed: f32,
    pub max_turn_rate: f32,
}

#[derive(Clone, Debug)]
pub struct PredictConfig {
    /// Below this a difference from the server is ignored. Floating point results differ between
    /// builds of the same game, so this must not be zero even though on one build they are identical.
    pub epsilon: f32,
    /// Most ticks of unconfirmed commands to keep.
    pub history_ticks: usize,
    /// How corrections show on a soldier on foot, unless the game says otherwise for what they are doing.
    pub correction: CorrectionStyle,
}

impl Default for PredictConfig {
    fn default() -> Self {
        Self { epsilon: 1e-3, history_ticks: 240, correction: CorrectionStyle { smoothing_rate: 12.0, snap_distance: 3.0, max_speed: f32::INFINITY, max_turn_rate: f32::INFINITY } }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PredictStats {
    /// Server states compared with what was predicted.
    pub compared: u32,
    /// Of those, how many disagreed and caused a replay.
    pub reconciles: u32,
    /// Biggest position change a replay caused, metres.
    pub max_correction: f32,
    /// The biggest difference from the server on any comparison, metres.
    pub max_error: f32,
}

struct Predicted<G: PredictedGame> {
    tick: u32,
    cmd: PlayerCmd,
    saved: G::Saved,
}

pub struct Predictor<G: PredictedGame> {
    pub game: G,
    tick: u32,
    history: VecDeque<Predicted<G>>,
    visual_offset: Vec3,
    /// The same for which way what you are riding faces. Drawn as this times the real attitude.
    visual_turn: Quat,
    cfg: PredictConfig,
    stats: PredictStats,
}

impl<G: PredictedGame> Predictor<G> {
    pub fn new(game: G, tick: u32, cfg: PredictConfig) -> Self {
        Self {
            game,
            tick,
            history: VecDeque::new(),
            visual_offset: Vec3::ZERO,
            visual_turn: Quat::IDENTITY,
            cfg,
            stats: PredictStats::default(),
        }
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn stats(&self) -> PredictStats {
        self.stats
    }

    /// Where to draw the local player: the predicted state plus what is left of any correction.
    pub fn view_position(&self) -> Vec3 {
        self.game.position() + self.visual_offset
    }

    pub fn visual_offset(&self) -> Vec3 {
        self.visual_offset
    }

    pub fn visual_turn(&self) -> Quat {
        self.visual_turn
    }

    fn style(&self) -> CorrectionStyle {
        self.game.correction_style(&self.cfg.correction)
    }

    /// Runs one new command, adding whatever it caused to `events`.
    pub fn step(&mut self, cmd: PlayerCmd, events: &mut Vec<G::Event>) {
        self.game.step(&cmd, Some(events));
        self.tick = cmd.tick;
        self.history.push_back(Predicted { tick: cmd.tick, cmd, saved: self.game.save() });
        while self.history.len() > self.cfg.history_ticks.max(1) {
            self.history.pop_front();
        }
    }

    /// The clock has gone back, so what was predicted past it never happened as far as the server knows.
    pub fn restart_at(&mut self, tick: u32) {
        self.tick = tick;
        self.history.clear();
    }

    /// Takes in several ticks that ran at once, without showing a jump.
    pub fn absorb_jump(&mut self, before: Vec3, attitude_before: Quat) {
        self.add_offset(before - self.game.position(), attitude_before);
    }

    fn add_offset(&mut self, offset: Vec3, attitude_before: Quat) {
        self.visual_offset += offset;
        self.visual_turn = (attitude_before * self.game.attitude().inverse() * self.visual_turn).normalize();
        if self.visual_offset.length() > self.style().snap_distance {
            self.clear_visual();
        }
    }

    /// The next thing shown is where the player really is, however far that is from the last.
    pub fn clear_visual(&mut self) {
        self.visual_offset = Vec3::ZERO;
        self.visual_turn = Quat::IDENTITY;
    }

    pub fn decay_visual(&mut self, dt: f32) {
        let style = self.style();
        let share = 1.0 - (-style.smoothing_rate * dt).exp();
        let length = self.visual_offset.length();
        if length > 0.0 {
            let fade = (length * share).min(style.max_speed * dt);
            self.visual_offset *= 1.0 - fade / length;
        }
        let angle = self.visual_turn.angle_between(Quat::IDENTITY);
        if angle > 0.0 {
            let fade = (angle * share).min(style.max_turn_rate * dt);
            self.visual_turn = self.visual_turn.slerp(Quat::IDENTITY, fade / angle).normalize();
        }
    }

    /// The server says that after `tick` the player was `server`.
    pub fn reconcile(&mut self, tick: u32, server: &<G::S as Schema>::Own) {
        while self.history.front().is_some_and(|p| p.tick < tick) {
            self.history.pop_front();
        }
        let Some(front) = self.history.front_mut() else {
            // The server is ahead of anything predicted. Take its word; the clock is about to jump.
            let mut now = self.game.save();
            G::apply_own(&mut now, server);
            self.game.restore(&now);
            return;
        };
        if front.tick != tick {
            return;
        }
        self.stats.compared += 1;
        let predicted = G::own_of(&front.saved);
        let error = G::S::own_position(&predicted).distance(G::S::own_position(server));
        self.stats.max_error = self.stats.max_error.max(error);
        if G::S::own_matches(&predicted, server, self.cfg.epsilon) {
            self.history.pop_front();
            return;
        }

        self.stats.reconciles += 1;
        let (before, attitude_before) = (self.game.position(), self.game.attitude());
        G::apply_own(&mut front.saved, server);
        self.game.restore(&front.saved);
        self.history.pop_front();
        for entry in &mut self.history {
            self.game.step(&entry.cmd, None);
            entry.saved = self.game.save();
        }
        let correction = before - self.game.position();
        self.stats.max_correction = self.stats.max_correction.max(correction.length());
        self.add_offset(correction, attitude_before);
    }
}
