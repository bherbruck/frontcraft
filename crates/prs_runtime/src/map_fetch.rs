//! Getting the map a server plays before joining it. The copy built in, or one in the maps folder, does if
//! its hash is the server's. Otherwise it comes from the server itself, at `/map/<name>.toml` under the
//! address that was joined, is checked against the hash, and on a desktop is kept in a cache folder for
//! next time. A browser keeps it for itself: the address has the hash in it, so it is kept for good.

use prs_net::client::MapOffer;
use prs_sim::MapData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Larger than any map should be, so a server cannot have a client fill its memory.
const MAX_MAP_BYTES: usize = 32 << 20;

/// Where the server keeps its map, given where its WebSocket is. Under the same path, so a proxy that sends
/// `/range` to one server sends this to the same one.
pub(crate) fn map_url(server: &str, name: &str, hash: u64) -> Option<String> {
    let (scheme, rest) = server.split_once("://")?;
    let scheme = match scheme {
        "ws" => "http",
        "wss" => "https",
        _ => return None,
    };
    let address = rest.split(['?', '#']).next()?.trim_end_matches('/');
    Some(format!("{scheme}://{address}/map/{name}.toml?v={hash:016x}"))
}

/// Whether what came is the map that was offered.
pub(crate) fn check(offer: &MapOffer, bytes: Vec<u8>) -> Result<MapData, String> {
    if offer.bytes != 0 && bytes.len() != offer.bytes as usize {
        return Err(format!("the map came as {} bytes, not {}", bytes.len(), offer.bytes));
    }
    let text = String::from_utf8(bytes).map_err(|_| "the map is not text".to_owned())?;
    let data = MapData::new(&offer.name, text);
    if offer.hash != 0 && data.hash != offer.hash {
        return Err("the map that came is not the one the server plays".into());
    }
    data.level().map_err(|error| error.to_string())?;
    Ok(data)
}

/// A copy already here that is the one offered: built in, in the maps folder, or fetched before.
pub(crate) fn find_local(offer: &MapOffer) -> Option<MapData> {
    let same = |data: &MapData| offer.hash == 0 || data.hash == offer.hash;
    if let Some(data) = MapData::find(&offer.name).ok().filter(same) {
        return Some(data);
    }
    let text = cache::read(offer)?;
    check(offer, text.into_bytes()).ok()
}

/// A download under way. What has come so far is counted as it comes.
pub(crate) struct Fetch {
    pub received: Arc<AtomicUsize>,
    result: Arc<Mutex<Option<Result<MapData, String>>>>,
}

impl Fetch {
    pub(crate) fn start(server: &str, offer: &MapOffer) -> Self {
        let fetch = Self { received: Arc::default(), result: Arc::default() };
        match map_url(server, &offer.name, offer.hash) {
            Some(url) => download(url, offer.clone(), fetch.received.clone(), fetch.result.clone()),
            None => *fetch.result.lock().unwrap() = Some(Err(format!("no way to fetch a map from {server}"))),
        }
        fetch
    }

    pub(crate) fn received(&self) -> usize {
        self.received.load(Ordering::Relaxed)
    }

    /// The map, or why not, once it is over.
    pub(crate) fn take(&self) -> Option<Result<MapData, String>> {
        self.result.lock().unwrap().take()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download(url: String, offer: MapOffer, received: Arc<AtomicUsize>, result: Arc<Mutex<Option<Result<MapData, String>>>>) {
    std::thread::spawn(move || {
        let fetched = match prs_net::http_native::fetch(&url, MAX_MAP_BYTES, &received) {
            Ok((200, bytes)) => check(&offer, bytes),
            Ok((status, _)) => Err(format!("the server would not give its map ({status})")),
            Err(error) => Err(format!("the map could not be fetched: {error}")),
        };
        if let Ok(data) = &fetched {
            cache::write(data);
        }
        *result.lock().unwrap() = Some(fetched);
    });
}

#[cfg(target_arch = "wasm32")]
fn download(url: String, offer: MapOffer, received: Arc<AtomicUsize>, result: Arc<Mutex<Option<Result<MapData, String>>>>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    wasm_bindgen_futures::spawn_local(async move {
        let fetched = async {
            let window = web_sys::window().ok_or("no window")?;
            let response: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
                .await
                .map_err(|_| "the map could not be fetched")?
                .dyn_into()
                .map_err(|_| "no answer")?;
            if !response.ok() {
                return Err(format!("the server would not give its map ({})", response.status()));
            }
            let buffer = JsFuture::from(response.array_buffer().map_err(|_| "no body")?).await.map_err(|_| "the map was cut short")?;
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            if bytes.len() > MAX_MAP_BYTES {
                return Err("the map is too big".into());
            }
            received.store(bytes.len(), Ordering::Relaxed);
            check(&offer, bytes)
        }
        .await;
        *result.lock().unwrap() = Some(fetched.map_err(|error: String| error));
    });
}

/// Maps fetched before, in the user's cache folder, named by their hash so an old one is never taken for a
/// new one.
#[cfg(not(target_arch = "wasm32"))]
mod cache {
    use super::*;
    use std::path::PathBuf;

    fn folder() -> Option<PathBuf> {
        let base = std::env::var_os("PRS_CACHE_DIR")
            .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
        Some(base.join("prs").join("maps"))
    }

    fn file(name: &str, hash: u64) -> Option<PathBuf> {
        prs_sim::valid_map_name(name).then_some(())?;
        Some(folder()?.join(format!("{name}-{hash:016x}.toml")))
    }

    pub(super) fn read(offer: &MapOffer) -> Option<String> {
        std::fs::read_to_string(file(&offer.name, offer.hash)?).ok()
    }

    /// Best effort: without a cache it is fetched again next time.
    pub(super) fn write(data: &MapData) {
        let Some(path) = file(&data.name, data.hash) else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &data.text);
    }
}

#[cfg(target_arch = "wasm32")]
mod cache {
    use super::*;

    pub(super) fn read(_: &MapOffer) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_is_fetched_from_under_the_address_that_was_joined() {
        let hash = 0xab;
        assert_eq!(map_url("ws://127.0.0.1:7777", "town", hash).as_deref(), Some("http://127.0.0.1:7777/map/town.toml?v=00000000000000ab"));
        assert_eq!(map_url("wss://play.frontcraft.online", "town", hash).as_deref(), Some("https://play.frontcraft.online/map/town.toml?v=00000000000000ab"));
        assert_eq!(
            map_url("wss://play.frontcraft.online/range/", "range", hash).as_deref(),
            Some("https://play.frontcraft.online/range/map/range.toml?v=00000000000000ab")
        );
        assert_eq!(map_url("wss://host/range?x=1", "range", hash).as_deref(), Some("https://host/range/map/range.toml?v=00000000000000ab"));
        assert_eq!(map_url("ftp://host", "town", hash), None);
    }

    #[test]
    fn only_the_map_that_was_offered_is_taken() {
        let town = MapData::built_in("town").unwrap();
        let offer = MapOffer { name: "town".into(), hash: town.hash, bytes: town.text.len() as u32 };
        assert_eq!(check(&offer, town.text.clone().into_bytes()).unwrap().hash, town.hash);
        assert!(check(&offer, format!("{} ", town.text).into_bytes()).is_err(), "a byte more is another file");
        let wrong = MapOffer { hash: town.hash ^ 1, ..offer.clone() };
        assert!(check(&wrong, town.text.clone().into_bytes()).is_err());
        assert!(check(&MapOffer { bytes: 3, ..offer.clone() }, b"abc".to_vec()).is_err(), "not a map");
    }

    #[test]
    fn a_copy_built_in_does_when_it_is_the_same_and_not_when_it_is_not() {
        let town = MapData::built_in("town").unwrap();
        let offer = MapOffer { name: "town".into(), hash: town.hash, bytes: town.text.len() as u32 };
        assert_eq!(find_local(&offer).map(|data| data.hash), Some(town.hash));
        assert!(find_local(&MapOffer { hash: 12345, ..offer }).is_none());
        assert!(find_local(&MapOffer { name: "nowhere".into(), hash: 1, bytes: 1 }).is_none());
    }
}
