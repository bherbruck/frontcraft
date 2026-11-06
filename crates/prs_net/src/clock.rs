//! Keeps a client's commands arriving at the server just before they are needed.
//!
//! The client runs ahead of the server, stamping each command with the server tick it should run
//! on. How far ahead is measured rather than calculated: the server reports how many ticks early
//! commands have been arriving, which already includes latency, jitter and any clock drift. The
//! worst of the last second is compared with the margin wanted, and the client's clock is sped up
//! or slowed down slightly until they agree. If commands are arriving late it jumps at once.

use prs_protocol::TICK_HZ;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct ClockConfig {
    /// How many ticks early the latest-arriving command should be.
    pub target_slack: f32,
    /// Leave the clock alone within this many ticks of the target.
    pub deadband: f32,
    /// Most the clock is sped up or slowed down by, as a fraction.
    pub max_rate_adjust: f32,
    /// Fraction of rate change per tick of error.
    pub rate_gain: f32,
    /// How many server reports make up "recently".
    pub slack_window: usize,
    /// Extra lead to start with, trimmed once real measurements come in.
    pub startup_extra: f32,
    /// Weight of a new round trip measurement in the average.
    pub rtt_smoothing: f32,
    /// This many ticks early and there is no slowing down to it: go straight back. A long stall while
    /// joining makes the first measurement of the round trip far too big, and the server refuses commands
    /// from too far ahead, so otherwise the player would never get going at all.
    pub too_far_ahead: i32,
    /// After a jump, reports are only believed once the server has heard a packet sent this long after
    /// it. Until then they are about commands stamped by the old clock.
    pub jump_settle_ms: f32,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            target_slack: 2.0,
            deadband: 1.0,
            max_rate_adjust: 0.05,
            rate_gain: 0.03,
            slack_window: 30,
            startup_extra: 4.0,
            rtt_smoothing: 0.1,
            too_far_ahead: 60,
            // Two snapshots' worth: a report covers every packet since the last one.
            jump_settle_ms: 70.0,
        }
    }
}

pub struct ClockSync {
    cfg: ClockConfig,
    /// The last tick handed out.
    tick: u32,
    pending: f64,
    rate: f64,
    slack: VecDeque<i32>,
    rtt_ms: f32,
    jumps: u32,
    /// The client's time of the last jump, while reports may still be about before it.
    jumped_at_ms: Option<u16>,
}

impl ClockSync {
    /// `server_tick` was the server's when it wrote the message that took `rtt_ms` to come back.
    /// A command sent now gets there another half round trip after the server's tick is read here.
    pub fn new(cfg: ClockConfig, server_tick: u32, rtt_ms: f32) -> Self {
        let lead = rtt_ms as f64 * TICK_HZ / 1000.0 + (cfg.target_slack + cfg.startup_extra) as f64;
        Self {
            tick: server_tick + lead.ceil() as u32,
            pending: 0.0,
            rate: 1.0,
            slack: VecDeque::new(),
            rtt_ms,
            jumps: 0,
            jumped_at_ms: None,
            cfg,
        }
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn rtt_ms(&self) -> f32 {
        self.rtt_ms
    }

    pub fn jumps(&self) -> u32 {
        self.jumps
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// How many ticks to run for `dt` seconds of real time.
    pub fn advance(&mut self, dt: f32) -> u32 {
        self.pending += dt as f64 * TICK_HZ * self.rate;
        let whole = self.pending.floor();
        self.pending -= whole;
        self.tick += whole as u32;
        whole as u32
    }

    /// Takes a server's report: how early commands arrived, if it says, and when the newest packet it
    /// had heard was sent, by the client's clock in milliseconds. Returns how many ticks to jump ahead.
    ///
    /// Reports already on their way when the clock jumps describe the clock from before, and each of
    /// them would make it jump again. Believing them made the lead swing by thousands of ticks, a round
    /// trip apart, for minutes after a tab came back from being out of sight.
    pub fn on_report(&mut self, slack: Option<i32>, heard_sent_ms: Option<u16>, now_ms: u16) -> u32 {
        if let Some(at) = self.jumped_at_ms {
            let after = heard_sent_ms.map(|sent| sent.wrapping_sub(at) as i16 as f32);
            if !after.is_some_and(|ms| ms >= self.cfg.jump_settle_ms) {
                return 0;
            }
            self.jumped_at_ms = None;
        }
        let Some(slack) = slack else { return 0 };
        let jumps = self.jumps;
        let jump = self.on_slack(slack);
        if self.jumps != jumps {
            self.jumped_at_ms = Some(now_ms);
        }
        jump
    }

    /// Takes the server's report of how early commands arrived. Returns how many ticks to jump ahead.
    pub fn on_slack(&mut self, slack: i32) -> u32 {
        if slack > self.cfg.too_far_ahead {
            self.tick = self.tick.saturating_sub((slack as f32 - self.cfg.target_slack) as u32);
            self.jumps += 1;
            self.slack.clear();
            self.rate = 1.0;
            return 0;
        }
        self.slack.push_back(slack);
        while self.slack.len() > self.cfg.slack_window.max(1) {
            self.slack.pop_front();
        }
        if slack < 0 {
            let jump = (self.cfg.target_slack - slack as f32).ceil() as u32;
            self.tick += jump;
            self.jumps += 1;
            // Those reports are about the old lead, and it would otherwise be corrected twice.
            self.slack.clear();
            self.rate = 1.0;
            return jump;
        }
        let worst = self.slack.iter().copied().min().unwrap_or(slack) as f32;
        let error = worst - self.cfg.target_slack;
        self.rate = if error.abs() <= self.cfg.deadband {
            1.0
        } else {
            let max = self.cfg.max_rate_adjust;
            1.0 - (error * self.cfg.rate_gain).clamp(-max, max) as f64
        };
        0
    }

    pub fn on_rtt(&mut self, rtt_ms: f32) {
        self.rtt_ms += (rtt_ms - self.rtt_ms) * self.cfg.rtt_smoothing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_from_before_a_jump_do_not_jump_it_again() {
        let mut clock = ClockSync::new(ClockConfig::default(), 100, 50.0);
        let before = clock.tick();
        assert_eq!(clock.on_report(Some(-300), Some(1000), 5000), 302);
        // The same lateness, about packets sent before the jump.
        assert_eq!(clock.on_report(Some(-299), Some(4990), 5010), 0);
        assert_eq!(clock.on_report(Some(-298), None, 5020), 0);
        assert_eq!(clock.tick(), before + 302);
        // Once the server has heard from after it, reports count again.
        assert_eq!(clock.on_report(Some(-3), Some(5100), 5200), 5);
    }

    #[test]
    fn late_commands_jump_ahead_at_once() {
        let mut clock = ClockSync::new(ClockConfig::default(), 100, 50.0);
        let before = clock.tick();
        let jump = clock.on_slack(-3);
        assert_eq!(jump, 5);
        assert_eq!(clock.tick(), before + 5);
    }

    #[test]
    fn far_too_much_lead_goes_straight_back() {
        let mut clock = ClockSync::new(ClockConfig::default(), 100, 30_000.0);
        let before = clock.tick();
        clock.on_slack(1800);
        assert_eq!(clock.tick(), before - 1798);
    }

    #[test]
    fn too_much_lead_slows_the_clock_and_too_little_speeds_it() {
        let mut clock = ClockSync::new(ClockConfig::default(), 100, 50.0);
        clock.on_slack(20);
        assert!(clock.rate() < 1.0);
        let mut clock = ClockSync::new(ClockConfig::default(), 100, 50.0);
        clock.on_slack(0);
        assert!(clock.rate() > 1.0);
    }
}
