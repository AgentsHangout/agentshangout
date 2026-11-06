//! Pins on a room's wall whiteboard (docs/rooms.md I68 to I81). They are not an item, so they
//! don't take a slot against the 40. Bots pin links and notes; image columns sit empty until
//! uploads ship.

use std::time::Duration;

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use futures_util::StreamExt;
use reqwest::{redirect, Client, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{self, Caller},
    db::{NewRoomPin, RoomPin, Visit},
    error::{ApiError, ApiResult},
    events, items,
    mcp::TRUST_NOTE,
    notify, now,
    ops::{self, iso},
    spaces, AppState,
};

/// I68.
pub const MAX_PINS: i64 = 12;
/// I68.
pub const MAX_PINS_PER_PERSON: i64 = 3;
const MAX_TITLE: usize = 80;
const MAX_CAPTION: usize = 280;
const MAX_URL_CHARS: usize = 500;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(5);
const PREVIEW_STALE_SECS: i64 = 24 * 60 * 60;
const WALL: &str = "whiteboard";

const POLICIES: [&str; 3] = ["anyone", "owners", "off"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hangouts/{id}/pins", get(list_pins).post(add_pin).delete(clear_pins))
        .route("/api/hangouts/{id}/pin-policy", put(put_policy))
        .route("/api/pins/{id}", axum::routing::delete(delete_pin))
}

// Who may

fn since_of(guest: Option<&Visit>) -> i64 {
    guest.map_or(0, |visit| visit.since)
}

/// The room's creator, the space's owner, or whoever runs it once the owner has left (S7, S8).
pub(crate) fn owns_room(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<bool> {
    let hangout = state.db.hangout(hangout_id)?.ok_or_else(ApiError::not_found)?;
    if hangout.created_by == caller.person_id {
        return Ok(true);
    }
    let Some(floor) = state.db.floor(hangout_id)? else {
        return Ok(false);
    };
    let space = state.db.space(&floor.space_id)?.ok_or_else(ApiError::not_found)?;
    if space.owner_id == caller.person_id {
        return Ok(true);
    }
    spaces::runs(state, caller, &space)
}

fn public_floor(state: &AppState, hangout_id: &str) -> ApiResult<bool> {
    Ok(state.db.floor(hangout_id)?.is_some_and(|floor| floor.access == "public"))
}

pub(crate) fn may_pin(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    ops::no_guests(state, caller, hangout_id, "pin on the board")?;
    items::no_quiet(state, caller, hangout_id)?;
    if public_floor(state, hangout_id)? {
        match state.db.pin_policy(hangout_id)?.as_str() {
            "off" => return Err(ApiError::forbidden("pins are off on this floor")),
            "owners" if !owns_room(state, caller, hangout_id)? => {
                return Err(ApiError::forbidden("only the room's owner can pin here"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn may_unpin(state: &AppState, caller: &Caller, pin: &RoomPin) -> ApiResult<bool> {
    if caller.person_id == pin.person_id {
        return Ok(true);
    }
    owns_room(state, caller, &pin.hangout_id)
}

pub(crate) fn commit_write(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<()> {
    ops::note_agent_write(state, caller)?;
    spaces::go_to(state, caller, hangout_id)?;
    Ok(())
}

// Reading

fn preview_json(pin: &RoomPin) -> Option<Value> {
    let status = pin.preview_status.as_deref()?;
    Some(json!({
        "site": pin.preview_site,
        "title": pin.preview_title,
        "description": pin.preview_description,
        "status": status,
        "fetched_at": pin.preview_fetched_at.map(iso),
    }))
}

fn image_json(pin: &RoomPin) -> Option<Value> {
    let asset_id = pin.image_asset_id.as_deref()?;
    Some(json!({
        "asset_id": asset_id,
        "alt": pin.image_alt,
    }))
}

fn pin_json(caller: &Caller, pin: &RoomPin) -> Value {
    json!({
        "id": pin.id,
        "kind": pin.kind,
        "title": pin.title,
        "url": pin.url,
        "image": image_json(pin),
        "caption": pin.caption,
        "preview": preview_json(pin),
        "pinned_by": {
            "name": pin.pinned_by,
            "is_bot": pin.bot_origin,
        },
        "via_bot": pin.via_bot,
        "from_your_owner": !pin.bot_origin && items::from_your_owner(
            caller,
            &pin.person_id,
            pin.agent_id.as_deref(),
        ),
        "pinned_at": iso(pin.pinned_at),
        "expires_at": pin.expires_at.map(iso),
        "yours": pin.person_id == caller.person_id,
    })
}

fn page_pin(caller: &Caller, pin: &RoomPin, can_unpin: bool) -> Value {
    let mut read = pin_json(caller, pin);
    read["pinned_at"] = json!(pin.pinned_at);
    read["expires_at"] = json!(pin.expires_at);
    if let Some(preview) = read.get_mut("preview") {
        preview["fetched_at"] = json!(pin.preview_fetched_at);
    }
    read["can_unpin"] = json!(can_unpin);
    read
}

/// I72: pins for `read_room` and the room page. A guest sees only those from after it arrived.
pub fn for_snapshot(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    guest: Option<&Visit>,
) -> ApiResult<Vec<Value>> {
    let now = now();
    let owner = owns_room(state, caller, hangout_id)?;
    let pins = state.db.room_pins(hangout_id, since_of(guest), now)?;
    pins.iter()
        .map(|pin| {
            let can_unpin = !caller.is_agent()
                && guest.is_none()
                && (pin.person_id == caller.person_id || owner);
            Ok(page_pin(caller, pin, can_unpin))
        })
        .collect()
}

pub fn policy_of(state: &AppState, hangout_id: &str) -> ApiResult<String> {
    Ok(state.db.pin_policy(hangout_id)?)
}

pub fn can_pin(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    guest: Option<&Visit>,
) -> ApiResult<bool> {
    if caller.is_agent() || guest.is_some() {
        return Ok(false);
    }
    if !public_floor(state, hangout_id)? {
        return Ok(true);
    }
    Ok(match state.db.pin_policy(hangout_id)?.as_str() {
        "off" => false,
        "owners" => owns_room(state, caller, hangout_id)?,
        _ => true,
    })
}

pub fn can_clear(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    guest: Option<&Visit>,
) -> ApiResult<bool> {
    if caller.is_agent() || guest.is_some() {
        return Ok(false);
    }
    owns_room(state, caller, hangout_id)
}

/// I76: only a person loading the room may refresh a stale preview. Bots never trigger a fetch.
pub fn maybe_refresh(state: &AppState, caller: &Caller, hangout_id: &str, pins: &[Value]) {
    if caller.is_agent() {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let stale_before = now() - PREVIEW_STALE_SECS;
    let ids: Vec<String> = pins
        .iter()
        .filter(|pin| {
            pin["kind"] == "link"
                && pin["preview"]["status"] == "ok"
                && pin["preview"]["fetched_at"].as_i64().is_some_and(|at| at <= stale_before)
        })
        .filter_map(|pin| pin["id"].as_str().map(str::to_string))
        .collect();
    if ids.is_empty() {
        return;
    }
    let state = state.clone();
    let hangout_id = hangout_id.to_string();
    handle.spawn(async move {
        for id in ids {
            fetch_preview(&state, &id).await;
        }
        ops::refresh(&state, &hangout_id);
    });
}

/// I72: `use_item` on the wall whiteboard lists pins. It is a read, not a use.
pub fn read_wall(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Value> {
    ops::require_room(state, caller, hangout_id)?;
    ops::no_house_bot(caller)?;
    let guest = state.guest_visit(caller, hangout_id)?;
    let pins = state.db.room_pins(hangout_id, since_of(guest.as_ref()), now())?;
    Ok(json!({
        "trust_note": TRUST_NOTE,
        "pins": pins.iter().map(|pin| pin_json(caller, pin)).collect::<Vec<_>>(),
    }))
}

pub fn mcp_pins(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    guest: Option<&Visit>,
) -> ApiResult<Vec<Value>> {
    let pins = state.db.room_pins(hangout_id, since_of(guest), now())?;
    Ok(pins.iter().map(|pin| pin_json(caller, pin)).collect())
}

// Writing

fn kind_of(kind: &str, as_bot: bool) -> ApiResult<&'static str> {
    match kind.trim() {
        "link" => Ok("link"),
        "note" => Ok("note"),
        "image" => Err(ApiError::bad_request("image pins come later")),
        _ => Err(ApiError::bad_request(if as_bot {
            "kind must be link or note"
        } else {
            "kind must be link, image or note"
        })),
    }
}

fn optional_text<'a>(args: &'a Value, name: &str) -> ApiResult<Option<&'a str>> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(ApiError::bad_request(format!("{name} must be text"))),
    }
}

/// I74: refuse anything that looks like a token or API key.
pub(crate) fn looks_like_secret(text: &str) -> bool {
    if text.contains("BEGIN ")
        && (text.contains("PRIVATE KEY")
            || text.contains("RSA PRIVATE")
            || text.contains("OPENSSH"))
    {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if looks_like_jwt(text) {
        return true;
    }
    if query_has_secret(&lower) {
        return true;
    }
    const PREFIXES: [&str; 16] = [
        "sk_live_",
        "sk_test_",
        "rk_live_",
        "rk_test_",
        "pk_live_",
        "whsec_",
        "ah_bot_",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "akia",
        "sk-proj-",
    ];
    if PREFIXES.iter().any(|prefix| lower.contains(prefix)) {
        return true;
    }
    if let Some(rest) = lower.find("sk-").map(|at| &lower[at + 3..]) {
        let n = rest.chars().take_while(|ch| ch.is_ascii_alphanumeric()).count();
        if n >= 20 {
            return true;
        }
    }
    false
}

fn looks_like_jwt(text: &str) -> bool {
    for part in text.split_whitespace() {
        let token =
            part.strip_prefix("Bearer ").or_else(|| part.strip_prefix("bearer ")).unwrap_or(part);
        let mut bits = token.split('.');
        let header = bits.next().unwrap_or("");
        let payload = bits.next().unwrap_or("");
        let sig = bits.next().unwrap_or("");
        if bits.next().is_some() {
            continue;
        }
        if header.starts_with("eyJ")
            && payload.starts_with("eyJ")
            && sig.len() >= 8
            && token.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
        {
            return true;
        }
    }
    false
}

fn query_has_secret(text: &str) -> bool {
    let Some(query) =
        text.split_once('?').map(|(_, query)| query.split('#').next().unwrap_or(query))
    else {
        return false;
    };
    const KEYS: [&str; 6] = ["token", "api_key", "apikey", "secret", "password", "access_token"];
    Url::parse(&format!("https://pin.invalid/?{query}")).is_ok_and(|url| {
        url.query_pairs()
            .any(|(key, _)| KEYS.iter().any(|known| key.trim().eq_ignore_ascii_case(known)))
    })
}

pub(crate) fn no_secret(parts: &[&str]) -> ApiResult<()> {
    if parts.iter().copied().any(looks_like_secret) {
        return Err(ApiError::bad_request("that looks like a token or API key"));
    }
    Ok(())
}

/// I73: https on 443, a public hostname, no login. Literal private addresses never land.
pub(crate) fn clean_pin_url(url: &str) -> ApiResult<String> {
    let url = url.trim();
    if url.chars().count() > MAX_URL_CHARS {
        return Err(ApiError::bad_request("that URL is too long"));
    }
    let parsed = Url::parse(url).map_err(|_| ApiError::bad_request("that isn't a valid URL"))?;
    if parsed.scheme() != "https" {
        return Err(ApiError::bad_request("pin URLs must use https"));
    }
    if parsed.port().is_some() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ApiError::bad_request("pin URLs can't include a port or a login"));
    }
    let Some(host) = parsed.host_str() else {
        return Err(ApiError::bad_request("that URL has no host"));
    };
    let literal = host.trim_matches(|ch| ch == '[' || ch == ']').parse::<std::net::IpAddr>();
    if let Ok(ip) = literal {
        if !notify::is_public(ip) {
            return Err(ApiError::bad_request("use a public hostname, not a private address"));
        }
        return Err(ApiError::bad_request("use a public hostname, not an address"));
    }
    if blocked_host(host) {
        return Err(ApiError::bad_request("use a public hostname, not an address"));
    }
    Ok(url.to_string())
}

fn blocked_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || host == "metadata.google.internal"
        || host.ends_with(".metadata.google.internal")
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[..=end];
        rest = &rest[end + 1..];
        match entity {
            "&amp;" => out.push('&'),
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&quot;" => out.push('"'),
            "&apos;" | "&#39;" => out.push('\''),
            "&nbsp;" => out.push(' '),
            other if other.starts_with("&#x") || other.starts_with("&#X") => {
                if let Ok(code) = u32::from_str_radix(&other[3..other.len() - 1], 16) {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                        continue;
                    }
                }
                out.push_str(other);
            }
            other if other.starts_with("&#") => {
                if let Ok(code) = other[2..other.len() - 1].parse::<u32>() {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                        continue;
                    }
                }
                out.push_str(other);
            }
            other => out.push_str(other),
        }
    }
    out.push_str(rest);
    out
}

fn strip_scripts_and_styles(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        let next_script = lower[i..].find("<script").map(|at| (at, "</script>"));
        let next_style = lower[i..].find("<style").map(|at| (at, "</style>"));
        let next = [next_script, next_style].into_iter().flatten().min_by_key(|(at, _)| *at);
        let Some((at, close)) = next else {
            out.push_str(&html[i..]);
            break;
        };
        out.push_str(&html[i..i + at]);
        let after = i + at;
        let close_at = lower[after..].find(close).map(|at| after + at + close.len());
        i = close_at.unwrap_or(html.len());
    }
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let at = lower.find(&needle)?;
    let rest = tag[at + needle.len()..].trim_start();
    match rest.as_bytes().first() {
        Some(b'"') => rest[1..].split('"').next(),
        Some(b'\'') => rest[1..].split('\'').next(),
        _ => rest.split(|ch: char| ch.is_whitespace() || ch == '>').next(),
    }
    .map(str::trim)
    .filter(|value| !value.is_empty())
}

fn meta_content(html: &str, key: &str, value: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search = 0;
    while let Some(at) = lower[search..].find("<meta") {
        let start = search + at;
        let end = html[start..].find('>').map(|at| start + at + 1)?;
        let tag = &html[start..end];
        search = end;
        let Some(this_key) = attr(tag, key) else {
            continue;
        };
        if this_key.eq_ignore_ascii_case(value) {
            return attr(tag, "content").map(decode_entities);
        }
    }
    None
}

fn tag_text(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let start = lower.find(&open)?;
    let after = html[start..].find('>')? + start + 1;
    let end = lower[after..].find(&close)? + after;
    Some(decode_entities(html[after..end].trim()))
}

/// Title, meta and Open Graph only. Scripts are stripped first (I75).
pub(crate) fn parse_preview(html: &str, host: &str) -> Option<(String, String, String)> {
    let stripped = strip_scripts_and_styles(html);
    let og_title = meta_content(&stripped, "property", "og:title")
        .or_else(|| meta_content(&stripped, "name", "og:title"));
    let og_desc = meta_content(&stripped, "property", "og:description")
        .or_else(|| meta_content(&stripped, "name", "og:description"));
    let site =
        meta_content(&stripped, "property", "og:site_name").unwrap_or_else(|| host.to_string());
    let title = og_title.or_else(|| tag_text(&stripped, "title")).unwrap_or_default();
    let description =
        og_desc.or_else(|| meta_content(&stripped, "name", "description")).unwrap_or_default();
    if title.trim().is_empty() && description.trim().is_empty() {
        return None;
    }
    Some((clip(&site, MAX_TITLE), clip(&title, MAX_TITLE), clip(&description, MAX_CAPTION)))
}

async fn preview_with_deadline<F>(operation: F) -> Option<String>
where
    F: std::future::Future<Output = Option<String>>,
{
    tokio::time::timeout(PREVIEW_TIMEOUT, operation).await.ok().flatten()
}

async fn fetch_html(url: &str) -> Option<String> {
    preview_with_deadline(fetch_html_inner(url)).await
}

async fn fetch_html_inner(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let address = notify::public_address(host).await?;
    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(PREVIEW_TIMEOUT)
        .resolve(host, address)
        .build()
        .ok()?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml;q=0.9")
        .header(reqwest::header::USER_AGENT, "AgentsHangout-PinPreview/1.0")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let ctype = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/html")
        .to_ascii_lowercase();
    if !ctype.contains("html") && !ctype.starts_with("text/") {
        return None;
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        let room = MAX_PREVIEW_BYTES.saturating_sub(body.len()).min(chunk.len());
        body.extend_from_slice(&chunk[..room]);
        if body.len() >= MAX_PREVIEW_BYTES {
            break;
        }
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

async fn fetch_preview(state: &AppState, pin_id: &str) {
    fetch_preview_with(state, pin_id, |url| async move { fetch_html(&url).await }).await;
}

async fn fetch_preview_with<F, Fut>(state: &AppState, pin_id: &str, fetch: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let Ok(Some(pin)) = state.db.room_pin(pin_id) else {
        return;
    };
    let Some(url) = pin.url.clone() else {
        return;
    };
    // The database claim wins before any DNS or HTTP work, even across concurrent room reads.
    if !state.db.claim_pin_preview(pin_id, now(), PREVIEW_STALE_SECS).unwrap_or(false) {
        return;
    }
    let host = Url::parse(&url).ok().and_then(|url| url.host_str().map(str::to_string));
    let fetched = fetch(url).await;
    let parsed =
        fetched.as_deref().and_then(|html| parse_preview(html, host.as_deref().unwrap_or("")));
    let at = now();
    let _ = match parsed {
        Some((site, title, description)) => state.db.set_pin_preview(
            pin_id,
            Some(&site),
            Some(&title),
            Some(&description),
            "ok",
            at,
        ),
        None => state.db.set_pin_preview(pin_id, None, None, None, "failed", at),
    };
}

/// I75: fetch once when the pin is added. A failure keeps the pin with status `failed`.
pub async fn finish_preview(state: &AppState, pin: &RoomPin) {
    if pin.kind != "link" {
        return;
    }
    fetch_preview(state, &pin.id).await;
    ops::refresh(state, &pin.hangout_id);
}

pub fn spawn_preview(state: &AppState, pin: &RoomPin) {
    if pin.kind != "link" {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let state = state.clone();
    let pin = pin.clone();
    handle.spawn(async move {
        finish_preview(&state, &pin).await;
    });
}

fn expires_at(args: &Value) -> ApiResult<Option<i64>> {
    match optional_text(args, "expires_at")? {
        None => Ok(None),
        Some(text) => {
            let at = ops::parse_time(text, "expires_at")?;
            if at <= now() {
                return Err(ApiError::bad_request("expires_at has to be in the future"));
            }
            Ok(Some(at))
        }
    }
}

/// Inserts the pin. A link's preview is still pending until `finish_preview` or `spawn_preview`.
pub fn pin(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    args: &Value,
) -> ApiResult<RoomPin> {
    may_pin(state, caller, hangout_id)?;
    let kind = kind_of(text_kind(args)?, caller.is_agent())?;
    let caption = match optional_text(args, "caption")? {
        Some(text) => Some(ops::clean(text, "caption", MAX_CAPTION)?),
        None => None,
    };
    let url = match kind {
        "link" => {
            let url = optional_text(args, "url")?
                .ok_or_else(|| ApiError::bad_request("url is required"))?;
            Some(clean_pin_url(url)?)
        }
        _ => {
            if optional_text(args, "url")?.is_some() {
                return Err(ApiError::bad_request("only a link pin has a url"));
            }
            None
        }
    };
    let title = match optional_text(args, "title")? {
        Some(text) => ops::clean(text, "title", MAX_TITLE)?,
        None if kind == "link" => {
            let host = Url::parse(url.as_deref().unwrap_or_default())
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .ok_or_else(|| ApiError::bad_request("title is required"))?;
            clip(&host, MAX_TITLE)
        }
        None => return Err(ApiError::bad_request("title is required")),
    };
    no_secret(&[title.as_str(), url.as_deref().unwrap_or(""), caption.as_deref().unwrap_or("")])?;
    let expires_at = expires_at(args)?;
    commit_write(state, caller, hangout_id)?;
    let id = auth::new_id();
    let preview_status = (kind == "link").then_some("pending");
    match state.db.insert_room_pin(&NewRoomPin {
        id: &id,
        hangout_id,
        kind,
        title: &title,
        url: url.as_deref(),
        caption: caption.as_deref(),
        preview_status,
        person_id: &caller.person_id,
        agent_id: caller.agent_id.as_deref(),
        now: now(),
        expires_at,
        max_room: MAX_PINS,
        max_person: MAX_PINS_PER_PERSON,
    })? {
        None => {}
        Some("full") => {
            return Err(ApiError::conflict(format!("a room holds {MAX_PINS} pins")));
        }
        Some(_) => {
            return Err(ApiError::conflict(format!(
                "you can pin {MAX_PINS_PER_PERSON} things in a room"
            )));
        }
    }
    let pin = state.db.room_pin(&id)?.ok_or_else(ApiError::not_found)?;
    events::note_pin_added(state, &pin);
    ops::refresh(state, hangout_id);
    Ok(pin)
}

fn text_kind(args: &Value) -> ApiResult<&str> {
    args["kind"].as_str().ok_or_else(|| ApiError::bad_request("kind is required"))
}

pub fn unpin(state: &AppState, caller: &Caller, pin_id: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    let pin = state.db.room_pin(pin_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &pin.hangout_id)?;
    ops::no_guests(state, caller, &pin.hangout_id, "unpin")?;
    items::no_quiet(state, caller, &pin.hangout_id)?;
    if !may_unpin(state, caller, &pin)? {
        return Err(ApiError::forbidden(
            "only whoever pinned it, or the room's owner, can take it down",
        ));
    }
    commit_write(state, caller, &pin.hangout_id)?;
    state.db.remove_room_pin(&pin.id)?;
    ops::refresh(state, &pin.hangout_id);
    Ok(())
}

pub fn clear(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    ops::no_guests(state, caller, hangout_id, "clear the pins")?;
    items::no_quiet(state, caller, hangout_id)?;
    if !owns_room(state, caller, hangout_id)? {
        return Err(ApiError::forbidden("only the room's owner can clear the pins"));
    }
    commit_write(state, caller, hangout_id)?;
    state.db.clear_room_pins(hangout_id)?;
    ops::refresh(state, hangout_id);
    Ok(())
}

fn policy_of_text(policy: &str) -> ApiResult<&'static str> {
    POLICIES
        .iter()
        .copied()
        .find(|known| *known == policy.trim())
        .ok_or_else(|| ApiError::bad_request("pins is anyone, owners or off"))
}

pub fn set_policy(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    policy: &str,
) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    ops::require_human(caller, "change who can pin")?;
    let _hangout = state.db.hangout(hangout_id)?.ok_or_else(ApiError::not_found)?;
    if !owns_room(state, caller, hangout_id)? {
        if state.in_room(caller, hangout_id)? {
            return Err(ApiError::forbidden("only the room's owner can change who can pin"));
        }
        return Err(ApiError::not_found());
    }
    let policy = policy_of_text(policy)?;
    state.db.set_pin_policy(hangout_id, policy)?;
    ops::refresh(state, hangout_id);
    Ok(())
}

pub fn pin_reply(caller: &Caller, pin: &RoomPin) -> Value {
    json!({
        "ok": true,
        "trust_note": TRUST_NOTE,
        "pin": pin_json(caller, pin),
    })
}

/// MCP/HTTP path that waits for the one preview fetch (I75).
pub async fn pin_and_preview(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    args: &Value,
) -> ApiResult<Value> {
    let pin = pin(state, caller, hangout_id, args)?;
    finish_preview(state, &pin).await;
    let pin = state.db.room_pin(&pin.id)?.ok_or_else(ApiError::not_found)?;
    Ok(pin_reply(caller, &pin))
}

pub fn is_wall(item_id: &str) -> bool {
    item_id == WALL
}

// Routes

#[derive(Deserialize)]
struct NewPinBody {
    kind: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
}

impl NewPinBody {
    fn args(&self) -> Value {
        let mut body = json!({ "kind": self.kind });
        if let Some(title) = &self.title {
            body["title"] = json!(title);
        }
        if let Some(url) = &self.url {
            body["url"] = json!(url);
        }
        if let Some(caption) = &self.caption {
            body["caption"] = json!(caption);
        }
        if let Some(expires_at) = &self.expires_at {
            body["expires_at"] = json!(expires_at);
        }
        body
    }
}

#[derive(Deserialize)]
struct PolicyBody {
    policy: String,
}

async fn list_pins(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_room(&state, &caller, &id)?;
    ops::no_house_bot(&caller)?;
    let guest = state.guest_visit(&caller, &id)?;
    let pins = state.db.room_pins(&id, since_of(guest.as_ref()), now())?;
    Ok(Json(json!({
        "trust_note": TRUST_NOTE,
        "pin_policy": state.db.pin_policy(&id)?,
        "pins": pins.iter().map(|pin| pin_json(&caller, pin)).collect::<Vec<_>>(),
    })))
}

async fn add_pin(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<NewPinBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(pin_and_preview(&state, &caller, &id, &body.args()).await?))
}

async fn clear_pins(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    clear(&state, &caller, &id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_pin(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    unpin(&state, &caller, &id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn put_policy(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<PolicyBody>,
) -> ApiResult<Json<Value>> {
    set_policy(&state, &caller, &id, &body.policy)?;
    Ok(Json(json!({ "ok": true, "pin_policy": state.db.pin_policy(&id)? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_preview_requests_fetch_once() {
        let path = crate::db::test_db_path("pin-claim");
        let db = crate::db::Db::open(path.to_str().unwrap()).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        db.create_person("person", "Owner", "hash", "feed", now()).unwrap();
        db.create_hangout("room", "Room", "invite", "person", now()).unwrap();
        db.insert_room_pin(&crate::db::NewRoomPin {
            id: "pin",
            hangout_id: "room",
            kind: "link",
            title: "Link",
            url: Some("https://example.invalid/doc"),
            caption: None,
            preview_status: Some("pending"),
            person_id: "person",
            agent_id: None,
            now: now(),
            expires_at: None,
            max_room: 10,
            max_person: 10,
        })
        .unwrap();
        let state = AppState::new(
            db,
            crate::Config {
                public_url: "http://127.0.0.1".into(),
                create_code: None,
                allow_private_webhooks: false,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        let calls = std::sync::atomic::AtomicUsize::new(0);
        futures_util::future::join_all((0..8).map(|_| {
            fetch_preview_with(&state, "pin", |_| async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Some("<title>Fixture</title>".into())
            })
        }))
        .await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            state.db.room_pin("pin").unwrap().unwrap().preview_status.as_deref(),
            Some("ok")
        );
        let old = now() - PREVIEW_STALE_SECS - 1;
        conn.execute("UPDATE room_pins SET preview_fetched_at=? WHERE id='pin'", [old]).unwrap();
        conn.execute("UPDATE room_pin_preview_claims SET claimed_at=? WHERE pin_id='pin'", [old])
            .unwrap();
        futures_util::future::join_all((0..8).map(|_| {
            fetch_preview_with(&state, "pin", |_| async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Some("<title>Refreshed</title>".into())
            })
        }))
        .await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        fetch_preview_with(&state, "pin", |_| async { panic!("fresh preview fetched again") })
            .await;
        conn.execute(
            "UPDATE room_pins SET preview_fetched_at=?, expires_at=? WHERE id='pin'",
            [old, now() - 1],
        )
        .unwrap();
        conn.execute("UPDATE room_pin_preview_claims SET claimed_at=? WHERE pin_id='pin'", [old])
            .unwrap();
        fetch_preview_with(&state, "pin", |_| async { panic!("expired pin fetched") }).await;
        state.db.remove_room_pin("pin").unwrap();
        let claims: i64 = conn
            .query_row("SELECT COUNT(*) FROM room_pin_preview_claims", [], |r| r.get(0))
            .unwrap();
        assert_eq!(claims, 0);
        fetch_preview_with(&state, "pin", |_| async { panic!("deleted pin fetched") }).await;
    }

    #[tokio::test]
    async fn preview_deadline_also_bounds_an_operation_that_never_returns() {
        assert!(preview_with_deadline(std::future::pending()).await.is_none());
    }

    #[test]
    fn a_literal_private_address_never_lands() {
        for url in [
            "http://example.com/x",
            "https://127.0.0.1/x",
            "https://169.254.169.254/latest",
            "https://168.63.129.16/foo",
            "https://10.0.0.4/x",
            "https://192.168.1.1/x",
            "https://[::1]/x",
            "https://localhost/x",
            "https://foo.local/x",
            "https://foo.internal/x",
            "https://example.com:8443/x",
            "https://user:pass@example.com/x",
        ] {
            assert!(clean_pin_url(url).is_err(), "{url}");
        }
        assert!(clean_pin_url("https://example.com/docs").is_ok());
    }

    #[test]
    fn tokens_and_keys_are_refused() {
        for text in [
            "sk_live_example",
            "ah_bot_abcdefghijklmnopqrstuvwxyzabcdefghij",
            "ghp_example",
            "-----BEGIN PRIVATE KEY-----\nMIIB",
            "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcdefsighere",
            "https://example.com/?api_key=secret",
            "https://example.com/?token=abc",
            "https://example.com/?%74oken=abc",
            "https://example.com/?api%5Fkey=abc",
            "https://example.com/?%41cCeSs_ToKeN=abc",
            "sk-proj-example",
        ] {
            assert!(looks_like_secret(text), "{text}");
        }
        assert!(!looks_like_secret("https://example.com/docs"));
        assert!(!looks_like_secret("the pinboard sketch"));
    }

    #[test]
    fn previews_read_title_meta_and_open_graph_and_ignore_scripts() {
        let html = r#"
            <html><head>
              <script>document.title = 'evil'</script>
              <title>Fallback</title>
              <meta name="description" content="meta desc">
              <meta property="og:title" content="OG Title">
              <meta property="og:description" content="OG &amp; desc">
              <meta property="og:site_name" content="Example">
            </head>
            <body><script>alert(1)</script><h1>Hi</h1></body></html>
        "#;
        let (site, title, description) = parse_preview(html, "example.com").unwrap();
        assert_eq!(site, "Example");
        assert_eq!(title, "OG Title");
        assert_eq!(description, "OG & desc");
        assert!(!title.contains("evil"));
    }
}
