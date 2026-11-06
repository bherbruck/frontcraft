//! A WebSocket in a browser. The browser delivers frames through callbacks, which go in a queue
//! until the session asks for them.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, MessageEvent, WebSocket};

use crate::transport::{Transport, TransportState};

pub struct WsClient {
    socket: Option<WebSocket>,
    inbox: Rc<RefCell<VecDeque<Vec<u8>>>>,
    _on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
}

impl WsClient {
    /// Starts connecting and returns straight away.
    pub fn connect(url: &str) -> Self {
        let inbox: Rc<RefCell<VecDeque<Vec<u8>>>> = Rc::default();
        let Ok(socket) = WebSocket::new(url) else {
            return Self { socket: None, inbox, _on_message: None };
        };
        socket.set_binary_type(BinaryType::Arraybuffer);
        let queue = inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<ArrayBuffer>() {
                queue.borrow_mut().push_back(Uint8Array::new(&buffer).to_vec());
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Self { socket: Some(socket), inbox, _on_message: Some(on_message) }
    }
}

/// Bytes the browser can be holding for the socket before voice stops being handed to it. A moment
/// of commands and little more, so what is said never gets in front of what is done.
const CONGESTED_BYTES: u32 = 2048;

impl Transport for WsClient {
    fn congested(&self) -> bool {
        self.socket.as_ref().is_some_and(|socket| socket.buffered_amount() > CONGESTED_BYTES)
    }

    fn state(&mut self) -> TransportState {
        match self.socket.as_ref().map(WebSocket::ready_state) {
            Some(WebSocket::CONNECTING) => TransportState::Connecting,
            Some(WebSocket::OPEN) => TransportState::Open,
            _ => TransportState::Closed,
        }
    }

    fn send(&mut self, frame: &[u8]) {
        if let Some(socket) = &self.socket {
            // Fails only if the socket has closed, which `state` will say.
            let _ = socket.send_with_u8_array(frame);
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.inbox.borrow_mut().pop_front()
    }

    fn is_reliable(&self) -> bool {
        true
    }
}

impl Drop for WsClient {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            socket.set_onmessage(None);
            let _ = socket.close();
        }
    }
}
