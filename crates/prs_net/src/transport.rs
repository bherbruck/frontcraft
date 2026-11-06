//! How frames get between a client and a server. A session only sees whole frames.
//!
//! WebSocket is first because it works in every browser and through everything else. It runs over
//! TCP, so one lost packet holds up everything behind it until it is resent: the client's clock
//! then has to lead by the length of those stalls, and remote soldiers are drawn late or on
//! extrapolation while one lasts. `tests/loop.rs` measures how much that costs.
//! WebTransport datagrams remove it by implementing this same trait, with `is_reliable` false: the
//! session then acks baselines and sends commands redundantly. A client tries that first and falls
//! back on the WebSocket, through [`FirstThen`].

use crate::client::ClientSession;
use crate::game::PredictedGame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportState {
    Connecting,
    Open,
    Closed,
}

pub trait Transport {
    /// Takes `&mut self` because finding out may mean checking on a connection in progress.
    fn state(&mut self) -> TransportState;
    /// Sends one frame. Never blocks. Does nothing if the connection is not open.
    fn send(&mut self, frame: &[u8]);
    /// The next frame received, if any. Never blocks.
    fn recv(&mut self) -> Option<Vec<u8>>;
    /// Whether every frame arrives, in order.
    fn is_reliable(&self) -> bool;
    /// Whether frames are piling up on their way out, so that anything that can be lost should be, and
    /// commands should wait to go together.
    fn congested(&self) -> bool {
        false
    }
}

/// Moves everything waiting in either direction between a session and its transport.
pub fn pump<G: PredictedGame>(session: &mut ClientSession<G>, transport: &mut dyn Transport) {
    if transport.state() == TransportState::Open {
        session.set_lossy(!transport.is_reliable());
        session.set_held_back(transport.congested());
        for frame in session.drain_outgoing() {
            transport.send(&frame);
        }
        // Always after the game's own, and never allowed to wait in front of what comes next.
        for frame in session.drain_voice_up() {
            if !transport.congested() {
                transport.send(&frame);
            }
        }
    }
    // Even once closed: the last thing to arrive may be why.
    while let Some(frame) = transport.recv() {
        session.receive(&frame);
    }
}

/// One transport, and another if the first cannot open in time. Once one is open it is kept: a game
/// that loses its connection has lost it, rather than starting again over something else.
pub struct FirstThen {
    current: Box<dyn Transport>,
    then: Option<Box<dyn FnOnce() -> Box<dyn Transport>>>,
    give_up_at_ms: f64,
    now_ms: Box<dyn Fn() -> f64>,
    fell_back: bool,
}

impl FirstThen {
    /// `now_ms` is any clock in milliseconds. `then` is only made if it is needed.
    pub fn new(
        first: Box<dyn Transport>,
        then: Box<dyn FnOnce() -> Box<dyn Transport>>,
        patience_ms: f64,
        now_ms: Box<dyn Fn() -> f64>,
    ) -> Self {
        let give_up_at_ms = now_ms() + patience_ms;
        Self { current: first, then: Some(then), give_up_at_ms, now_ms, fell_back: false }
    }

    /// Whether it had to fall back.
    pub fn fell_back(&self) -> bool {
        self.fell_back
    }
}

impl Transport for FirstThen {
    fn state(&mut self) -> TransportState {
        let state = self.current.state();
        if self.then.is_some() {
            if state == TransportState::Open {
                self.then = None;
            } else if state == TransportState::Closed || (self.now_ms)() >= self.give_up_at_ms {
                let then = self.then.take().expect("just checked");
                self.current = then();
                self.fell_back = true;
                return self.current.state();
            }
        }
        state
    }

    fn send(&mut self, frame: &[u8]) {
        self.current.send(frame);
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.current.recv()
    }

    fn is_reliable(&self) -> bool {
        self.current.is_reliable()
    }

    fn congested(&self) -> bool {
        self.current.congested()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Fixed(TransportState, bool);

    impl Transport for Fixed {
        fn state(&mut self) -> TransportState {
            self.0
        }
        fn send(&mut self, _: &[u8]) {}
        fn recv(&mut self) -> Option<Vec<u8>> {
            None
        }
        fn is_reliable(&self) -> bool {
            self.1
        }
    }

    fn first_then(first: TransportState, clock: &Rc<Cell<f64>>) -> FirstThen {
        let clock = clock.clone();
        FirstThen::new(
            Box::new(Fixed(first, false)),
            Box::new(|| Box::new(Fixed(TransportState::Open, true))),
            1000.0,
            Box::new(move || clock.get()),
        )
    }

    #[test]
    fn the_second_is_only_used_if_the_first_fails_or_takes_too_long() {
        let clock = Rc::new(Cell::new(0.0));
        let mut opened = first_then(TransportState::Open, &clock);
        clock.set(5000.0);
        assert_eq!((opened.state(), opened.is_reliable(), opened.fell_back()), (TransportState::Open, false, false));

        let mut refused = first_then(TransportState::Closed, &clock);
        assert_eq!((refused.state(), refused.is_reliable(), refused.fell_back()), (TransportState::Open, true, true));

        let mut slow = first_then(TransportState::Connecting, &clock);
        clock.set(5999.0);
        assert_eq!(slow.state(), TransportState::Connecting);
        clock.set(6000.0);
        assert_eq!((slow.state(), slow.is_reliable()), (TransportState::Open, true));
    }

    #[test]
    fn one_that_opened_and_then_closed_stays_closed() {
        let clock = Rc::new(Cell::new(0.0));
        let mut link = first_then(TransportState::Open, &clock);
        link.state();
        link.current = Box::new(Fixed(TransportState::Closed, false));
        assert_eq!(link.state(), TransportState::Closed);
    }
}
