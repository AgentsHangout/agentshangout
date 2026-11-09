//! A118, A119 (#983): OAuth Client ID Metadata Documents. An `https://` `client_id` names a JSON
//! document that this authorization server fetches and treats as the client's registration.
//!
//! The URL comes from whoever starts an authorization, so the fetch is bounded the way wake-up
//! URLs are (`notify`): https on the standard port only, no redirects, every address the host
//! resolves to must be public, and the connection is pinned to the address that was checked, so
//! a second lookup can't move it. It also has a short timeout and reads at most a few KB, and
//! what it reads is kept in memory for a while, in a cache bounded by size and by time.

use std::{collections::HashMap, net::SocketAddr, sync::Mutex, time::Duration};

use reqwest::{
    header::{HeaderMap, ACCEPT, CACHE_CONTROL},
    redirect, Client, StatusCode, Url,
};
use serde_json::Value;

use crate::AppState;
#[cfg(debug_assertions)]
use std::sync::{Arc, OnceLock};

/// A118: the longest `client_id` that can be a document's URL. Every other id keeps A65's 128.
pub const MAX_URL_CHARS: usize = 512;
/// A119: how long the whole fetch may take: the lookup, the connection and the body.
const TIMEOUT: Duration = Duration::from_secs(3);
/// A119: the most bytes of a document read, counted as they arrive.
const MAX_BYTES: usize = 5 * 1024;
/// A119: the most documents kept at once.
const MAX_ENTRIES: usize = 256;
/// A119: the longest a document is kept, whatever its max-age says.
const MAX_SECS: i64 = 3600;
/// A119: how long a document that says nothing about caching is kept.
const DEFAULT_SECS: i64 = 300;

/// A client's registration, as read from its document.
#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub name: String,
    pub redirects: Vec<String>,
    /// The URL's host, which the consent page names as where the details came from.
    pub host: String,
}

/// A test's stand-in for the network, in a debug build only (`serve_from_memory`).
#[cfg(debug_assertions)]
type Fetcher = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// A119: documents read lately, each until its time is up.
#[derive(Default)]
pub struct Documents {
    cache: Mutex<HashMap<String, (Document, i64)>>,
    #[cfg(debug_assertions)]
    fetcher: OnceLock<Fetcher>,
}

impl Documents {
    /// Tests only: serve documents from memory in place of the network, so a test never has to
    /// loosen A119 to reach a local server. It exists only in a debug build, which is what
    /// `cargo test` makes; the release build the server runs has no such seam.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn serve_from_memory(
        &self,
        fetch: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) {
        let _ = self.fetcher.set(Arc::new(fetch));
    }

    /// A test's document in place of a fetch: only in a debug build, and only once a test set it.
    fn served(&self, client_id: &str) -> Option<Option<(String, i64)>> {
        #[cfg(debug_assertions)]
        if let Some(fetch) = self.fetcher.get() {
            return Some(fetch(client_id).map(|body| (body, DEFAULT_SECS)));
        }
        let _ = client_id;
        None
    }

    fn cached(&self, client_id: &str, now: i64) -> Option<Document> {
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(client_id).filter(|(_, until)| *until > now).map(|(document, _)| document.clone())
    }

    fn keep(&self, client_id: &str, document: Document, until: i64, now: i64) {
        if until <= now {
            return;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.len() >= MAX_ENTRIES && !cache.contains_key(client_id) {
            cache.retain(|_, (_, kept_until)| *kept_until > now);
            let soonest = cache.iter().min_by_key(|(_, (_, kept_until))| *kept_until);
            if let Some(id) =
                soonest.filter(|_| cache.len() >= MAX_ENTRIES).map(|(id, _)| id.clone())
            {
                cache.remove(&id);
            }
        }
        cache.insert(client_id.to_string(), (document, until));
    }
}

/// A118: whether this `client_id` is a document's URL rather than a registered client's id.
pub(crate) fn is_url(client_id: &str) -> bool {
    client_id.starts_with("https://")
}

/// A118: the URL, when it's one this server fetches: https in its canonical form, with a path,
/// and no login, port or fragment.
fn valid_url(raw: &str) -> Option<Url> {
    if raw.len() > MAX_URL_CHARS {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    let fine = url.scheme() == "https"
        && url.as_str() == raw
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && url.path() != "/";
    fine.then_some(url)
}

/// A118: the client this URL names, from what was read lately or fetched now. `may_fetch` is asked
/// before a fetch, so fetching a document counts against the same limit as registering a client.
pub(crate) async fn client(
    state: &AppState,
    client_id: &str,
    may_fetch: impl FnOnce() -> bool,
) -> Option<Document> {
    let url = valid_url(client_id)?;
    let now = crate::now();
    if let Some(document) = state.cimd.cached(client_id, now) {
        return Some(document);
    }
    if !may_fetch() {
        return None;
    }
    let (body, keep_secs) = match state.cimd.served(client_id) {
        Some(served) => served?,
        None => fetch(&url).await?,
    };
    let document = read(client_id, &url, &body)?;
    state.cimd.keep(client_id, document.clone(), now + keep_secs, now);
    Some(document)
}

/// A118: the document as this client's registration: JSON whose `client_id` is this URL exactly,
/// within the limits on what a client may register (DCR's, `oauth::valid_metadata`).
fn read(client_id: &str, url: &Url, body: &str) -> Option<Document> {
    let value: Value = serde_json::from_str(body).ok()?;
    if value.get("client_id").and_then(Value::as_str) != Some(client_id) {
        return None;
    }
    let registration: crate::oauth::Registration = serde_json::from_value(value).ok()?;
    if !crate::oauth::valid_metadata(&registration) {
        return None;
    }
    Some(Document {
        name: crate::oauth::client_name(&registration.client_name).to_string(),
        redirects: registration.redirect_uris,
        host: url.host_str()?.to_string(),
    })
}

/// A119: the document's body, and how long to keep it, or nothing if any bound wasn't met.
async fn fetch(url: &Url) -> Option<(String, i64)> {
    let fetched = async {
        let host = url.host_str()?;
        let found: Vec<SocketAddr> = tokio::net::lookup_host((host, 443)).await.ok()?.collect();
        let address = checked(found)?;
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .no_proxy()
            .resolve(host, address)
            .build()
            .ok()?;
        let mut response =
            client.get(url.clone()).header(ACCEPT, "application/json").send().await.ok()?;
        if response.status() != StatusCode::OK {
            return None;
        }
        let keep_secs = max_age(response.headers());
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if !within_cap(&mut body, &chunk) {
                return None;
            }
        }
        Some((String::from_utf8(body).ok()?, keep_secs))
    };
    tokio::time::timeout(TIMEOUT, fetched).await.ok()?
}

/// A119: adds a chunk to a document's body, or refuses once the body would pass `MAX_BYTES`.
fn within_cap(body: &mut Vec<u8>, chunk: &[u8]) -> bool {
    if body.len() + chunk.len() > MAX_BYTES {
        return false;
    }
    body.extend_from_slice(chunk);
    true
}

/// A119: the address to connect to, only when every address the host resolved to is public (no
/// loopback, private, link-local, unspecified or multicast, IPv4-mapped included). IPv4 first, as
/// for wake-up URLs, since this server has no IPv6 route.
fn checked(mut addresses: Vec<SocketAddr>) -> Option<SocketAddr> {
    if addresses.is_empty()
        || addresses.iter().any(|address| !crate::notify::is_public(address.ip()))
    {
        return None;
    }
    addresses.sort_by_key(SocketAddr::is_ipv6);
    addresses.first().copied()
}

/// A119: how long to keep a document: its max-age, at most an hour, a few minutes when it says
/// nothing, and not at all with no-store or no-cache.
fn max_age(headers: &HeaderMap) -> i64 {
    let said = headers.get(CACHE_CONTROL).and_then(|value| value.to_str().ok()).unwrap_or("");
    let mut keep = DEFAULT_SECS;
    for part in said.split(',').map(|part| part.trim().to_ascii_lowercase()) {
        if part == "no-store" || part == "no-cache" {
            return 0;
        }
        if let Some(secs) = part.strip_prefix("max-age=") {
            keep = secs.parse().unwrap_or(0);
        }
    }
    keep.clamp(0, MAX_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(address: &str) -> SocketAddr {
        format!("{address}:443").parse().unwrap()
    }

    #[test]
    fn only_a_host_whose_every_address_is_public_is_fetched() {
        for refused in [
            "127.0.0.1",
            "[::1]",
            "10.0.0.7",
            "192.168.1.20",
            "172.16.0.1",
            "169.254.169.254",
            "[fe80::1]",
            "[::ffff:127.0.0.1]",
            "[::ffff:10.0.0.7]",
            "0.0.0.0",
            "[::]",
            "224.0.0.1",
            "[ff02::1]",
        ] {
            assert_eq!(checked(vec![at(refused)]), None, "{refused}");
        }
        assert_eq!(checked(vec![]), None, "nothing resolved");
        // One private answer among public ones refuses the whole host.
        assert_eq!(checked(vec![at("93.184.215.14"), at("10.0.0.7")]), None);
        let public = vec![at("[2606:2800:21f:cb07:6820:80da:af6b:8b2c]"), at("93.184.215.14")];
        assert_eq!(checked(public), Some(at("93.184.215.14")), "IPv4 first");
    }

    #[test]
    fn a_document_is_kept_for_its_max_age_up_to_an_hour() {
        let kept = |value: &str| {
            let mut headers = HeaderMap::new();
            if !value.is_empty() {
                headers.insert(CACHE_CONTROL, value.parse().unwrap());
            }
            max_age(&headers)
        };
        assert_eq!(kept(""), DEFAULT_SECS, "no Cache-Control");
        assert_eq!(kept("max-age=60"), 60);
        assert_eq!(kept("public, max-age=120"), 120);
        assert_eq!(kept("max-age=99999"), MAX_SECS, "capped at an hour");
        assert_eq!(kept("max-age=soon"), 0, "unreadable");
        assert_eq!(kept("no-store"), 0);
        assert_eq!(kept("max-age=60, no-cache"), 0);
    }

    #[test]
    fn a_document_is_read_to_5_kb_and_no_further() {
        let mut body = Vec::new();
        assert!(within_cap(&mut body, &vec![b'a'; MAX_BYTES - 1]));
        assert!(within_cap(&mut body, b"b"), "exactly 5 KB is fine");
        assert!(!within_cap(&mut body, b"c"), "one byte more is refused");
        assert_eq!(body.len(), MAX_BYTES);
    }

    #[test]
    fn a_document_url_is_canonical_https_with_a_path() {
        assert!(valid_url("https://client.example/oauth/client.json").is_some());
        for refused in [
            "http://client.example/client.json",
            "https://client.example/",
            "https://client.example",
            "https://user@client.example/client.json",
            "https://client.example:8443/client.json",
            "https://client.example/client.json#part",
            "https://Client.example/client.json",
            "https://client.example/a/../client.json",
        ] {
            assert!(valid_url(refused).is_none(), "{refused}");
        }
        assert!(valid_url(&format!("https://client.example/{}", "a".repeat(600))).is_none());
    }
}
