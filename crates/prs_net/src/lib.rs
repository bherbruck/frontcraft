//! Netcode with no game in it: an authoritative server, clients that predict themselves and
//! interpolate everyone else, and everything in between.
//!
//! Which game it is comes in through [`game`], so this crate knows nothing of soldiers or weapons
//! and can be tested with a toy. `prs_net_sim` plugs in the real simulation.
//!
//! Pure logic and sockets are kept apart: [`client::ClientSession`] and [`server::GameServer`] take
//! bytes in and give bytes out, so the same code runs over a real WebSocket, a browser's WebSocket
//! or the in-process [`simnet`]. Nothing here knows Bevy exists.

pub mod client;
pub mod clock;
pub mod entity;
pub mod game;
pub mod interp;
pub mod names;
pub mod predict;
pub mod quic;
pub mod server;
pub mod simnet;
pub mod testkit;
pub mod transport;
pub mod validate;
pub mod wire;

#[cfg(not(target_arch = "wasm32"))]
pub mod ws_host;
#[cfg(not(target_arch = "wasm32"))]
pub mod wt_host;
#[cfg(not(target_arch = "wasm32"))]
mod ws_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod http_native;
#[cfg(not(target_arch = "wasm32"))]
mod wt_native;
#[cfg(target_arch = "wasm32")]
mod ws_wasm;
#[cfg(target_arch = "wasm32")]
mod wt_wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use ws_native::WsClient;
#[cfg(not(target_arch = "wasm32"))]
pub use wt_native::WtClient;
#[cfg(target_arch = "wasm32")]
pub use ws_wasm::WsClient;
#[cfg(target_arch = "wasm32")]
pub use wt_wasm::WtClient;

/// A hash of some text, for comparing rules across machines. The same on every platform.
pub fn hash_text(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}
