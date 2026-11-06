//! What a server will take from a client. A client can only ever ask for input a real player could
//! have given, and the sim does the rest, so this is where cheating by malformed input stops.
//!
//! Limits are generous on purpose: they stop nonsense rather than police play, and a mod with
//! other rules can set them to whatever it needs.

use glam::Vec2;
use prs_protocol::PlayerCmd;

#[derive(Clone, Debug)]
pub struct CmdLimits {
    /// How far past the server's tick a command may be stamped. Further than this is a client whose
    /// clock is wrong or lying. Also bounds how many commands one client can have waiting.
    pub max_future_ticks: u32,
    /// Longest `wish_move` is allowed to be. Anything longer is shortened to this. A little over 1,
    /// because a normalised vector can come out a rounding error long, and changing an honest
    /// command at all would put the server out of step with what the client predicted.
    pub max_wish: f32,
    /// Most the view can turn in one tick, radians. A real flick is nowhere near. It is clamped
    /// rather than refused, so the client's next command is still good.
    pub max_turn_per_tick: f32,
    /// Most command packets accepted per second. A real client sends one per tick at most.
    pub max_packets_per_second: u32,
    /// Refused commands in a second that get a client disconnected. 0 means never.
    pub kick_after_rejects_per_second: u32,
}

impl Default for CmdLimits {
    fn default() -> Self {
        Self {
            max_future_ticks: 120,
            max_wish: 1.001,
            max_turn_per_tick: std::f32::consts::TAU,
            max_packets_per_second: 240,
            kick_after_rejects_per_second: 600,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// NaN or infinity anywhere.
    NotFinite,
    /// Stamped too far ahead of the server.
    FromTheFuture,
    /// Stamped for a tick already run.
    Late,
}

/// Checks a command as it arrives. `next_tick` is the next tick the server will run.
pub fn check(cmd: &PlayerCmd, next_tick: u32, limits: &CmdLimits) -> Result<PlayerCmd, Rejected> {
    if !(cmd.wish_move.is_finite() && cmd.yaw.is_finite() && cmd.pitch.is_finite()) {
        return Err(Rejected::NotFinite);
    }
    if cmd.tick < next_tick {
        return Err(Rejected::Late);
    }
    if cmd.tick - next_tick > limits.max_future_ticks {
        return Err(Rejected::FromTheFuture);
    }
    Ok(PlayerCmd { wish_move: cmd.wish_move.clamp_length_max(limits.max_wish), ..*cmd })
}

/// Clamps the turn a command asks for against where the soldier is looking now. Done as a command
/// is run rather than as it arrives, because only then is the starting point known.
pub fn clamp_turn(cmd: &PlayerCmd, current_yaw: f32, current_pitch: f32, limits: &CmdLimits) -> PlayerCmd {
    let max = limits.max_turn_per_tick;
    let turn = Vec2::new(cmd.yaw - current_yaw, cmd.pitch - current_pitch);
    if turn.x.abs() <= max && turn.y.abs() <= max {
        return *cmd;
    }
    PlayerCmd {
        yaw: current_yaw + turn.x.clamp(-max, max),
        pitch: current_pitch + turn.y.clamp(-max, max),
        ..*cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(tick: u32) -> PlayerCmd {
        PlayerCmd { tick, ..Default::default() }
    }

    #[test]
    fn a_long_move_is_shortened() {
        let huge = PlayerCmd { wish_move: Vec2::new(1000.0, 1000.0), ..cmd(10) };
        let out = check(&huge, 10, &CmdLimits::default()).unwrap();
        assert!((out.wish_move.length() - 1.001).abs() < 1e-5);
    }

    #[test]
    fn a_normalised_move_is_left_exactly_as_sent() {
        for i in 0..1000 {
            let angle = i as f32 * 0.37;
            let wish = Vec2::new(angle.sin(), angle.cos()).normalize();
            let sent = PlayerCmd { wish_move: wish, ..cmd(10) };
            assert_eq!(check(&sent, 10, &CmdLimits::default()), Ok(sent));
        }
    }

    #[test]
    fn nan_and_infinity_are_refused() {
        let limits = CmdLimits::default();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(check(&PlayerCmd { yaw: bad, ..cmd(10) }, 10, &limits), Err(Rejected::NotFinite));
            assert_eq!(check(&PlayerCmd { pitch: bad, ..cmd(10) }, 10, &limits), Err(Rejected::NotFinite));
            let wish = Vec2::new(bad, 0.0);
            assert_eq!(check(&PlayerCmd { wish_move: wish, ..cmd(10) }, 10, &limits), Err(Rejected::NotFinite));
        }
    }

    #[test]
    fn ticks_outside_the_window_are_refused() {
        let limits = CmdLimits { max_future_ticks: 5, ..Default::default() };
        assert_eq!(check(&cmd(9), 10, &limits), Err(Rejected::Late));
        assert!(check(&cmd(10), 10, &limits).is_ok());
        assert!(check(&cmd(15), 10, &limits).is_ok());
        assert_eq!(check(&cmd(16), 10, &limits), Err(Rejected::FromTheFuture));
    }

    #[test]
    fn an_extreme_turn_is_clamped_to_the_limit() {
        let limits = CmdLimits::default();
        let out = clamp_turn(&PlayerCmd { yaw: 1e30, pitch: -1e30, ..cmd(1) }, 2.0, 0.0, &limits);
        assert_eq!(out.yaw, 2.0 + limits.max_turn_per_tick);
        assert_eq!(out.pitch, -limits.max_turn_per_tick);
    }

    #[test]
    fn an_ordinary_turn_is_left_exactly_as_sent() {
        let sent = PlayerCmd { yaw: 2.123_456_7, pitch: 0.1, ..cmd(1) };
        assert_eq!(clamp_turn(&sent, 2.0, 0.0, &CmdLimits::default()), sent);
    }
}
