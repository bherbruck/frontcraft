//! A network in a test: frames are delayed, reordered and lost on purpose, on a virtual clock.
//!
//! `LinkMode::Datagram` behaves like UDP. `LinkMode::Reliable` behaves like TCP under a WebSocket:
//! nothing is lost, but a lost packet is resent later and everything behind it has to wait, which is
//! the real cost of TCP for a game. `LinkMode::Quic` is WebTransport as the hosts use it: frames that
//! must arrive go on one reliable stream, and the rest as datagrams that never wait for it.

use std::cell::{Cell, RefCell};
use std::collections::BinaryHeap;
use std::rc::Rc;

use crate::transport::{Transport, TransportState};
use crate::wire::must_arrive;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinkMode {
    Datagram,
    Reliable {
        /// How long a lost packet takes to be resent. Doubles each time the resend is lost too.
        retransmit_ms: f64,
    },
    Quic {
        /// For the stream, as for `Reliable`.
        retransmit_ms: f64,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct LinkConfig {
    /// One way.
    pub latency_ms: f64,
    /// Each frame's delay varies by up to this much either side of `latency_ms`.
    pub jitter_ms: f64,
    /// Chance of each frame being lost, 0 to 1.
    pub loss: f64,
    pub mode: LinkMode,
}

impl LinkConfig {
    pub fn perfect(latency_ms: f64) -> Self {
        Self { latency_ms, jitter_ms: 0.0, loss: 0.0, mode: LinkMode::Datagram }
    }
}

#[derive(Eq, PartialEq)]
struct InFlight {
    at_us: u64,
    seq: u64,
    bytes: Vec<u8>,
}

impl Ord for InFlight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (other.at_us, other.seq).cmp(&(self.at_us, self.seq))
    }
}

impl PartialOrd for InFlight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinkStats {
    pub frames: u64,
    pub bytes: u64,
    /// Lost for good on a datagram link, resent on a reliable one.
    pub lost: u64,
}

/// One direction of a connection.
pub struct SimLink {
    cfg: LinkConfig,
    rng: u64,
    queue: BinaryHeap<InFlight>,
    seq: u64,
    last_us: u64,
    stats: LinkStats,
}

impl SimLink {
    pub fn new(cfg: LinkConfig, seed: u64) -> Self {
        Self {
            cfg,
            rng: seed | 1,
            queue: BinaryHeap::new(),
            seq: 0,
            last_us: 0,
            stats: LinkStats::default(),
        }
    }

    pub fn stats(&self) -> LinkStats {
        self.stats
    }

    pub fn set_config(&mut self, cfg: LinkConfig) {
        self.cfg = cfg;
    }

    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn send(&mut self, now_ms: f64, bytes: Vec<u8>) {
        self.stats.frames += 1;
        self.stats.bytes += bytes.len() as u64;
        let swing = (self.random() * 2.0 - 1.0) * self.cfg.jitter_ms;
        let mut delay = (self.cfg.latency_ms + swing).max(0.0);
        let in_order = match self.cfg.mode {
            LinkMode::Datagram => false,
            LinkMode::Reliable { .. } => true,
            LinkMode::Quic { .. } => must_arrive(&bytes),
        };
        match self.cfg.mode {
            LinkMode::Reliable { retransmit_ms } | LinkMode::Quic { retransmit_ms } if in_order => {
                let mut wait = retransmit_ms;
                while self.random() < self.cfg.loss {
                    self.stats.lost += 1;
                    delay += wait;
                    wait *= 2.0;
                }
            }
            _ => {
                if self.random() < self.cfg.loss {
                    self.stats.lost += 1;
                    return;
                }
            }
        }
        let mut at_us = ((now_ms + delay) * 1000.0) as u64;
        if in_order {
            at_us = at_us.max(self.last_us);
            self.last_us = at_us;
        }
        self.seq += 1;
        self.queue.push(InFlight { at_us, seq: self.seq, bytes });
    }

    pub fn recv(&mut self, now_ms: f64) -> Option<Vec<u8>> {
        let now_us = (now_ms * 1000.0) as u64;
        if self.queue.peek()?.at_us > now_us {
            return None;
        }
        self.queue.pop().map(|f| f.bytes)
    }
}

/// One end of a simulated connection, usable wherever a real transport is.
pub struct SimEndpoint {
    pub clock_ms: Rc<Cell<f64>>,
    pub tx: Rc<RefCell<SimLink>>,
    pub rx: Rc<RefCell<SimLink>>,
    reliable: bool,
}

/// A client end and a server end, sharing `clock_ms`.
pub fn sim_connection(
    clock_ms: Rc<Cell<f64>>,
    up: LinkConfig,
    down: LinkConfig,
    seed: u64,
) -> (SimEndpoint, SimEndpoint) {
    let reliable = matches!(up.mode, LinkMode::Reliable { .. });
    let up = Rc::new(RefCell::new(SimLink::new(up, seed.wrapping_mul(2).wrapping_add(0x9E37))));
    let down = Rc::new(RefCell::new(SimLink::new(down, seed.wrapping_mul(3).wrapping_add(0x79B9))));
    let client = SimEndpoint { clock_ms: clock_ms.clone(), tx: up.clone(), rx: down.clone(), reliable };
    let server = SimEndpoint { clock_ms, tx: down, rx: up, reliable };
    (client, server)
}

impl Transport for SimEndpoint {
    fn state(&mut self) -> TransportState {
        TransportState::Open
    }

    fn send(&mut self, frame: &[u8]) {
        self.tx.borrow_mut().send(self.clock_ms.get(), frame.to_vec());
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.borrow_mut().recv(self.clock_ms.get())
    }

    fn is_reliable(&self) -> bool {
        self.reliable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(link: &mut SimLink, until_ms: f64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(f) = link.recv(until_ms) {
            out.push(f);
        }
        out
    }

    #[test]
    fn nothing_arrives_before_its_latency() {
        let mut link = SimLink::new(LinkConfig::perfect(50.0), 1);
        link.send(0.0, vec![1]);
        assert!(link.recv(49.0).is_none());
        assert_eq!(link.recv(50.0), Some(vec![1]));
    }

    #[test]
    fn a_datagram_link_loses_about_what_it_was_told() {
        let cfg = LinkConfig { latency_ms: 0.0, jitter_ms: 0.0, loss: 0.2, mode: LinkMode::Datagram };
        let mut link = SimLink::new(cfg, 7);
        for _ in 0..10_000 {
            link.send(0.0, vec![0]);
        }
        let got = drain(&mut link, 1.0).len();
        assert!((7500..8500).contains(&got), "{got}");
    }

    #[test]
    fn a_reliable_link_loses_nothing_and_stalls_what_follows_a_loss() {
        let cfg = LinkConfig {
            latency_ms: 10.0,
            jitter_ms: 5.0,
            loss: 0.05,
            mode: LinkMode::Reliable { retransmit_ms: 200.0 },
        };
        let mut link = SimLink::new(cfg, 3);
        for i in 0..2000u32 {
            link.send(i as f64, i.to_le_bytes().to_vec());
        }
        let got = drain(&mut link, 1e9);
        assert_eq!(got.len(), 2000);
        for (i, frame) in got.iter().enumerate() {
            assert_eq!(frame[..], (i as u32).to_le_bytes(), "out of order at {i}");
        }
        assert!(link.stats().lost > 0);
    }
}
