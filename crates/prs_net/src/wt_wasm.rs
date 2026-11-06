//! WebTransport in a browser. Asks the server beside the WebSocket where it is, then opens a session
//! with one stream for what must arrive and datagrams for the rest. Everything the browser hands back
//! goes in a queue until the session asks for it.
//!
//! web-sys only has WebTransport behind an unstable flag, so it is reached by name instead. A browser
//! without it fails straight away, and the client falls back on the WebSocket.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{ReadableStreamDefaultReader, Response, WritableStreamDefaultWriter};

use crate::quic::{StreamFrames, WtInfo, goes_on_stream, info_url, session_url, stream_frame};
use crate::transport::{Transport, TransportState};

#[derive(Default)]
struct Shared {
    state: Cell<Option<TransportState>>,
    inbox: RefCell<VecDeque<Vec<u8>>>,
    open: RefCell<Option<Open>>,
}

struct Open {
    session: JsValue,
    stream: WritableStreamDefaultWriter,
    datagrams: WritableStreamDefaultWriter,
    max_datagram: Option<usize>,
    /// Writes that fail do so because the session has closed, which `closed` reports. Their promises
    /// still need somewhere to land.
    ignore: Closure<dyn FnMut(JsValue)>,
}

pub struct WtClient {
    shared: Rc<Shared>,
}

impl WtClient {
    /// Starts connecting, given the address of the server's WebSocket, and returns straight away.
    pub fn connect(websocket: &str) -> Self {
        let shared = Rc::new(Shared::default());
        shared.state.set(Some(TransportState::Connecting));
        let (websocket, inside) = (websocket.to_owned(), shared.clone());
        spawn_local(async move {
            if open(&websocket, &inside).await.is_err() {
                inside.state.set(Some(TransportState::Closed));
            }
        });
        Self { shared }
    }
}

fn get(from: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    Reflect::get(from, &JsValue::from_str(name))
}

fn call(on: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: js_sys::Function = get(on, name)?.dyn_into()?;
    function.apply(on, &args.iter().collect::<Array>())
}

async fn wait(promise: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(promise.dyn_into::<Promise>()?).await
}

async fn ask(websocket: &str) -> Result<WtInfo, JsValue> {
    let url = info_url(websocket).ok_or("not a websocket address")?;
    let window = web_sys::window().ok_or("no window")?;
    let response: Response = wait(window.fetch_with_str(&url).into()).await?.dyn_into()?;
    if !response.ok() {
        return Err("no webtransport here".into());
    }
    let text = wait(response.text()?.into()).await?.as_string().unwrap_or_default();
    WtInfo::from_json(&text).ok_or_else(|| "unreadable".into())
}

async fn open(websocket: &str, shared: &Rc<Shared>) -> Result<(), JsValue> {
    let info = ask(websocket).await?;
    let url = session_url(websocket, &info).ok_or("not a websocket address")?;
    let options = Object::new();
    if let Some(hash) = info.certificate_hash {
        let named = Object::new();
        Reflect::set(&named, &"algorithm".into(), &"sha-256".into())?;
        Reflect::set(&named, &"value".into(), &Uint8Array::from(&hash[..]))?;
        Reflect::set(&options, &"serverCertificateHashes".into(), &Array::of1(&named))?;
    }
    let constructor: js_sys::Function = get(&js_sys::global(), "WebTransport")?.dyn_into()?;
    let session = Reflect::construct(&constructor, &Array::of2(&url.into(), &options))?;
    wait(get(&session, "ready")?).await?;

    let stream = wait(call(&session, "createBidirectionalStream", &[])?).await?;
    let datagrams = get(&session, "datagrams")?;
    let writer = |of: &JsValue| -> Result<WritableStreamDefaultWriter, JsValue> {
        call(&get(of, "writable")?, "getWriter", &[])?.dyn_into()
    };
    let reader = |of: &JsValue| -> Result<ReadableStreamDefaultReader, JsValue> {
        call(&get(of, "readable")?, "getReader", &[])?.dyn_into()
    };
    let max_datagram = get(&datagrams, "maxDatagramSize")?.as_f64().map(|most| most as usize);
    *shared.open.borrow_mut() = Some(Open {
        session: session.clone(),
        stream: writer(&stream)?,
        datagrams: writer(&datagrams)?,
        max_datagram,
        ignore: Closure::new(|_| {}),
    });
    let (stream_reader, datagram_reader) = (reader(&stream)?, reader(&datagrams)?);
    shared.state.set(Some(TransportState::Open));

    let inside = shared.clone();
    spawn_local(async move {
        let mut frames = StreamFrames::default();
        while let Some(bytes) = next_chunk(&stream_reader).await {
            frames.push(&bytes);
            loop {
                match frames.next_frame() {
                    Ok(Some(frame)) => inside.inbox.borrow_mut().push_back(frame),
                    Ok(None) => break,
                    Err(_) => return close(&inside),
                }
            }
        }
        close(&inside);
    });
    let inside = shared.clone();
    spawn_local(async move {
        while let Some(datagram) = next_chunk(&datagram_reader).await {
            inside.inbox.borrow_mut().push_back(datagram);
        }
    });
    let inside = shared.clone();
    spawn_local(async move {
        let _ = wait(get(&session, "closed").unwrap_or(JsValue::UNDEFINED)).await;
        close(&inside);
    });
    Ok(())
}

/// The next piece read, or `None` once the stream has ended or failed.
async fn next_chunk(reader: &ReadableStreamDefaultReader) -> Option<Vec<u8>> {
    let result = JsFuture::from(reader.read()).await.ok()?;
    if get(&result, "done").ok()?.is_truthy() {
        return None;
    }
    Some(get(&result, "value").ok()?.dyn_into::<Uint8Array>().ok()?.to_vec())
}

fn close(shared: &Shared) {
    shared.state.set(Some(TransportState::Closed));
    if let Some(open) = shared.open.borrow_mut().take() {
        let _ = call(&open.session, "close", &[]);
    }
}

impl Transport for WtClient {
    fn state(&mut self) -> TransportState {
        self.shared.state.get().unwrap_or(TransportState::Closed)
    }

    fn send(&mut self, frame: &[u8]) {
        let open = self.shared.open.borrow();
        let Some(open) = open.as_ref() else { return };
        let (writer, bytes) = if goes_on_stream(frame, open.max_datagram) {
            (&open.stream, stream_frame(frame))
        } else {
            (&open.datagrams, frame.to_vec())
        };
        let _ = writer.write_with_chunk(&Uint8Array::from(&bytes[..])).catch(&open.ignore);
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.shared.inbox.borrow_mut().pop_front()
    }

    fn is_reliable(&self) -> bool {
        false
    }

    /// A browser hands one datagram on per turn of its event loop, and a page drawing slowly has few
    /// turns. Writing every packet regardless queued them up for good: commands reached the server
    /// seconds late, and later and later.
    fn congested(&self) -> bool {
        let open = self.shared.open.borrow();
        open.as_ref().is_some_and(|open| open.datagrams.desired_size().ok().flatten().is_some_and(|room| room <= 0.0))
    }
}

impl Drop for WtClient {
    fn drop(&mut self) {
        close(&self.shared);
    }
}
