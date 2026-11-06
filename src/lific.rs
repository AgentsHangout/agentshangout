//! I123 to I127 (#613): a `lific_board` station shows the issues of a published Lific project
//! (github.com/VoidNullable/lific, Apache-2.0) as columns by status. It keeps no copy of them and no
//! key: the room's owner links it to an allow-listed Lific address and a project, and every read
//! asks Lific's anonymous public routes again, from this server and never from the page. Anything
//! that goes wrong, from a slow answer to a project that's no longer published, shows as
//! unavailable with no issues, rather than old ones.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use tokio::sync::Semaphore;

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::get,
    Json, Router,
};
use reqwest::{redirect, Client, StatusCode, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    items,
    mcp::TRUST_NOTE,
    notify, now, ops, pins, AppState,
};

/// I127: how long one read of Lific may take, start to finish.
const TIMEOUT: Duration = Duration::from_secs(3);
/// I127: the most of one answer that's read. A bigger one is unavailable, not cut short, since
/// half a JSON document can't be read.
pub const MAX_BYTES: usize = 256 * 1024;
/// I127: the most issues shown, and the longest title.
pub const MAX_SHOWN: usize = 50;
pub const MAX_TITLE: usize = 120;
/// Lific's statuses, in the order its board shows them.
const STATUSES: [&str; 5] = ["backlog", "todo", "active", "done", "cancelled"];
/// I126: how long one read of a board is shown again to whoever asks next, so a room full of
/// people and bots, or a bot in a loop, asks Lific once in that time.
pub const REUSE: Duration = Duration::from_secs(5);
/// I126: the most reads of Lific under way at once, across every board.
const SLOTS: usize = 4;

/// I124: where a Lific board may point. `origins` are `https` origins from
/// `AGENTHANGOUT_LIFIC_ORIGINS`; `local` is the one instance beside this server, from
/// `AGENTHANGOUT_LIFIC_LOCAL`, the only address that may be private.
#[derive(Clone, Default)]
pub struct Settings {
    pub origins: Vec<String>,
    pub local: Option<String>,
    /// I130: the API key that makes and publishes a room's project on `local`, from
    /// `AGENTHANGOUT_LIFIC_KEY`. It's only ever sent there, and never stored or shown.
    pub key: Option<String>,
    /// For tests only, with no environment setting: the address every listed origin's name goes
    /// to, in place of looking it up, so the pinned path can reach a stand-in on this machine.
    #[doc(hidden)]
    pub test_pin: Option<IpAddr>,
}

/// I130: says whether there's a key, never what it is, so a logged setting can't give it away.
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("origins", &self.origins)
            .field("local", &self.local)
            .field("key", &self.key.as_ref().map(|_| "set"))
            .field("test_pin", &self.test_pin)
            .finish()
    }
}

/// One board's last read: when, for which link, and what it showed.
type Recent = HashMap<String, (Instant, String, Value)>;

/// I124, I126: the settings, the read slots, and each board's last read for `REUSE`.
pub struct Shared {
    settings: RwLock<Settings>,
    slots: Semaphore,
    recent: Mutex<Recent>,
    /// I129: one room's project made at a time, so two first reads at once make only one.
    making: tokio::sync::Mutex<()>,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            settings: RwLock::new(Settings::default()),
            slots: Semaphore::new(SLOTS),
            recent: Mutex::new(HashMap::new()),
            making: tokio::sync::Mutex::new(()),
        }
    }
}

impl Shared {
    /// Replaces the settings, and forgets every board's last read.
    pub fn set(&self, settings: Settings) {
        if let Ok(mut current) = self.settings.write() {
            *current = settings;
        }
        if let Ok(mut recent) = self.recent.lock() {
            recent.clear();
        }
    }

    fn forget(&self, item_id: &str) {
        if let Ok(mut recent) = self.recent.lock() {
            recent.remove(item_id);
        }
    }

    /// A read of this board, with this link, from under `REUSE` ago.
    fn reused(&self, item_id: &str, link: &str) -> Option<Value> {
        let recent = self.recent.lock().ok()?;
        let (at, was, board) = recent.get(item_id)?;
        (was == link && at.elapsed() < REUSE).then(|| board.clone())
    }

    fn keep(&self, item_id: &str, link: &str, board: &Value) {
        if let Ok(mut recent) = self.recent.lock() {
            recent.retain(|_, (at, _, _)| at.elapsed() < REUSE);
            recent.insert(item_id.to_string(), (Instant::now(), link.to_string(), board.clone()));
        }
    }
}

impl Settings {
    /// I124: from the two settings. An entry that isn't a bare origin of the right kind is left
    /// out, so a typo can't widen the list.
    pub fn from_settings(origins: Option<&str>, local: Option<&str>) -> Settings {
        let origins = origins
            .unwrap_or_default()
            .split(',')
            .filter_map(|part| bare_origin(part.trim()))
            .filter(|origin| origin.starts_with("https://"))
            .collect();
        let local = local.map(str::trim).and_then(bare_origin);
        Settings { origins, local, key: None, test_pin: None }
    }

    /// I130: adds the key from `AGENTHANGOUT_LIFIC_KEY`. A blank one counts as none.
    pub fn with_key(mut self, key: Option<&str>) -> Settings {
        self.key = key.map(str::trim).filter(|key| !key.is_empty()).map(str::to_string);
        self
    }
}

/// `scheme://host[:port]` for a URL that is only that, with nothing after it but a slash.
fn bare_origin(text: &str) -> Option<String> {
    let url = Url::parse(text).ok()?;
    let bare = matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    bare.then(|| url.origin().ascii_serialization())
}

fn settings(state: &AppState) -> Settings {
    state.lific.settings.read().map(|s| s.clone()).unwrap_or_default()
}

/// I124: the address to store, if this server allows it.
fn allowed(settings: &Settings, base: &str) -> Option<String> {
    let origin = bare_origin(base.trim())?;
    let local = settings.local.as_deref() == Some(origin.as_str());
    (local || settings.origins.contains(&origin)).then_some(origin)
}

/// I124: a Lific project identifier, like `APP`.
fn project_of(text: &str) -> ApiResult<String> {
    let project = text.trim();
    let ok = (1..=16).contains(&project.len())
        && project.starts_with(|c: char| c.is_ascii_uppercase())
        && project.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !ok {
        return Err(ApiError::bad_request("project is a Lific project id, like APP"));
    }
    Ok(project.to_string())
}

fn board_item(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<crate::db::FloorItem> {
    let item = items::visible_item(state, caller, item_id)?;
    if item.kind != "lific_board" {
        return Err(ApiError::conflict("that isn't a Lific board"));
    }
    Ok(item)
}

/// Whether this id is a placed Lific board, for `read_board` to send it here.
pub fn is_board(state: &AppState, id: &str) -> bool {
    state.db.floor_item(id).ok().flatten().is_some_and(|item| item.kind == "lific_board")
}

/// I124: the room's owner links the board, or unlinks it with no `base`.
pub fn link(
    state: &AppState,
    caller: &Caller,
    item_id: &str,
    base: Option<&str>,
    project: Option<&str>,
) -> ApiResult<Value> {
    ops::require_human(caller, "link a Lific board")?;
    let item = board_item(state, caller, item_id)?;
    if !pins::owns_room(state, caller, &item.hangout_id)? {
        return Err(ApiError::forbidden("only the room's owner can link a Lific board"));
    }
    state.lific.forget(&item.id);
    let Some(base) = base.filter(|base| !base.trim().is_empty()) else {
        state.db.clear_lific_link(&item.id)?;
        ops::refresh(state, &item.hangout_id);
        return Ok(json!({ "linked": false }));
    };
    let origin = allowed(&settings(state), base).ok_or_else(|| {
        ApiError::bad_request("that Lific address isn't one this server may reach")
    })?;
    let project = project_of(project.unwrap_or_default())?;
    state.db.set_lific_link(
        &item.id,
        &item.hangout_id,
        &origin,
        &project,
        &caller.person_id,
        now(),
    )?;
    ops::refresh(state, &item.hangout_id);
    Ok(json!({ "linked": true, "project": project }))
}

/// Why a read came back with nothing, in words for the board.
fn in_words(reason: &str) -> &'static str {
    match reason {
        "not_linked" => "This board isn't linked to a Lific project yet.",
        "not_allowed" => "This board's Lific address is no longer one this server may reach.",
        "refused" => "This board's Lific address isn't a public one, so it wasn't asked.",
        "slow" => "Lific didn't answer in time.",
        "busy" => "Lific is busy. Try again in a minute.",
        "not_published" => "This project isn't published on Lific, or it doesn't exist.",
        "redirected" => "Lific sent this board somewhere else, so it wasn't followed.",
        "too_big" => "This project is too big to show here.",
        "key_refused" => "Lific turned down this server's key, so the room's board wasn't made.",
        _ => "Lific didn't answer.",
    }
}

fn unavailable(item_id: &str, project: Option<&str>, reason: &str) -> Value {
    json!({
        "item_id": item_id,
        "kind": "lific",
        "project": project,
        "linked": project.is_some(),
        "available": false,
        "reason": reason,
        "unavailable": in_words(reason),
        "columns": [],
        "shown": 0,
        "total": 0,
        "truncated": false,
        "from_lific": true,
        "trust_note": TRUST_NOTE,
    })
}

/// I124: a client that goes only where it was told: no redirects, no proxy, and, unless this is
/// the configured local instance, only to the public address the name resolved to just now, so a
/// name can't be pointed somewhere private between the check and the request.
async fn client_for(settings: &Settings, base: &Url) -> Result<Client, &'static str> {
    let builder = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(TIMEOUT)
        .no_proxy()
        .user_agent("AgentsHangout-LificBoard/1.0");
    let origin = base.origin().ascii_serialization();
    if settings.local.as_deref() == Some(origin.as_str()) {
        return builder.build().map_err(|_| "down");
    }
    let host = base.host_str().ok_or("refused")?;
    let port = base.port_or_known_default().unwrap_or(443);
    let ip = match settings.test_pin {
        Some(ip) => ip,
        None => {
            if base.scheme() != "https" {
                return Err("refused");
            }
            tokio::time::timeout(TIMEOUT, notify::public_address(host))
                .await
                .map_err(|_| "slow")?
                .ok_or("refused")?
                .ip()
        }
    };
    builder.resolve(host, SocketAddr::new(ip, port)).build().map_err(|_| "down")
}

/// One public route's JSON, read up to `MAX_BYTES`.
async fn get_json(client: &Client, url: Url) -> Result<Value, &'static str> {
    // A public read sends no key, so a refusal or a conflict there is just Lific not answering.
    answer_json(client.get(url).header("Accept", "application/json").send().await)
        .await
        .map_err(|reason| if matches!(reason, "key_refused" | "taken") { "down" } else { reason })
}

/// I129: a short project id for a room, like `RK4Q7`: an uppercase letter, then four uppercase
/// letters or digits, as Lific wants, from the room's id and how many tries came before.
fn room_identifier(room: &str, attempt: u32) -> String {
    const DIGITS: &[u8; 36] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in room.bytes().chain(attempt.to_le_bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    let mut id = String::from("R");
    for _ in 0..4 {
        id.push(DIGITS[(hash % 36) as usize] as char);
        hash /= 36;
    }
    id
}

/// I129, I130: the room's own project on the local instance: made with the server's key and
/// published the first time a board there is read with no link, and the same one after that. Only
/// ever asks `local`, and only with a key.
async fn room_project(
    state: &AppState,
    settings: &Settings,
    room: &str,
    name: &str,
) -> Result<(String, String), &'static str> {
    let (Some(local), Some(key)) = (settings.local.as_deref(), settings.key.as_deref()) else {
        return Err("not_linked");
    };
    let _one = state.lific.making.lock().await;
    let base = Url::parse(local).map_err(|_| "down")?;
    let client = client_for(settings, &base).await?;
    let send = |method: reqwest::Method, path: &str, body: Value| {
        let url = base.join(path);
        let request = url.map(|url| {
            client
                .request(method, url)
                .bearer_auth(key)
                .header("Accept", "application/json")
                .json(&body)
        });
        async move { answer_json(request.map_err(|_| "down")?.send().await).await }
    };
    let made = state.db.lific_room_project(room).map_err(|_| "down")?;
    let (project, lific_id) = match made {
        Some((_, project, _, true)) => return Ok((local.to_string(), project)),
        Some((_, project, lific_id, false)) => (project, lific_id),
        None => {
            let name = clean_title(name);
            let name =
                if name.trim().is_empty() { "Agents Hangout room".to_string() } else { name };
            let mut found = None;
            for attempt in 0..5 {
                let identifier = room_identifier(room, attempt);
                let body = json!({ "name": name, "identifier": identifier });
                match send(reqwest::Method::POST, "/api/projects", body).await {
                    Ok(project) => {
                        let lific_id = match &project["id"] {
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.clone(),
                            _ => return Err("down"),
                        };
                        // Kept at once, so a failed publish is retried rather than made again.
                        state
                            .db
                            .set_lific_room_project(room, local, &identifier, &lific_id, now())
                            .map_err(|_| "down")?;
                        found = Some((identifier, lific_id));
                        break;
                    }
                    Err("taken") => continue,
                    Err(reason) => return Err(reason),
                }
            }
            found.ok_or("down")?
        }
    };
    send(reqwest::Method::PUT, &format!("/api/projects/{lific_id}"), json!({ "is_public": true }))
        .await?;
    state.db.lific_room_project_published(room).map_err(|_| "down")?;
    Ok((local.to_string(), project))
}

/// I127, I129: an answer's JSON, read up to `MAX_BYTES`, or why not. Nothing Lific says goes into
/// the reason, so an error can't carry a key or anything else back.
async fn answer_json(
    sent: Result<reqwest::Response, reqwest::Error>,
) -> Result<Value, &'static str> {
    let mut response = sent.map_err(|error| if error.is_timeout() { "slow" } else { "down" })?;
    let status = response.status();
    if status.is_redirection() {
        return Err("redirected");
    }
    if status == StatusCode::NOT_FOUND {
        return Err("not_published");
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        return Err("busy");
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err("key_refused");
    }
    if status == StatusCode::CONFLICT {
        return Err("taken");
    }
    if !status.is_success() {
        return Err("down");
    }
    if response.content_length().is_some_and(|length| length > MAX_BYTES as u64) {
        return Err("too_big");
    }
    let mut body = Vec::new();
    loop {
        let chunk =
            response.chunk().await.map_err(
                |error| {
                    if error.is_timeout() {
                        "slow"
                    } else {
                        "down"
                    }
                },
            )?;
        let Some(chunk) = chunk else { break };
        if body.len() + chunk.len() > MAX_BYTES {
            return Err("too_big");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "down")
}

/// I126: a title as text: no control characters, at most `MAX_TITLE` characters.
pub fn clean_title(title: &str) -> String {
    let flat: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if flat.chars().count() <= MAX_TITLE {
        return flat;
    }
    let mut cut: String = flat.chars().take(MAX_TITLE - 1).collect();
    cut.push('…');
    cut
}

/// I126: the columns from `/index` (and module names from `/modules`, when it answered).
fn columns(item_id: &str, project: &str, index: &Value, modules: Option<&Value>) -> Value {
    let module_name = |id: Option<i64>| -> Option<String> {
        let id = id?;
        modules?
            .as_array()?
            .iter()
            .find(|m| m["id"].as_i64() == Some(id))
            .and_then(|m| m["name"].as_str().map(clean_title))
    };
    let status_of = |row: &Value| row["status"].as_str().unwrap_or("backlog").to_string();
    // A status Lific added after this was written has no column, so it's neither shown nor counted.
    let mut issues: Vec<&Value> = index["issues"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["deleted"] != true && STATUSES.contains(&status_of(row).as_str()))
                .collect()
        })
        .unwrap_or_default();
    let rank = |row: &&Value| STATUSES.iter().position(|s| *s == status_of(row)).unwrap_or(0);
    issues.sort_by(|a, b| {
        rank(a).cmp(&rank(b)).then(
            a["sort_order"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&b["sort_order"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let total = issues.len();
    let shown = &issues[..total.min(MAX_SHOWN)];
    let columns: Vec<Value> = STATUSES
        .iter()
        .map(|status| {
            let rows: Vec<Value> = shown
                .iter()
                .filter(|row| row["status"].as_str().unwrap_or("backlog") == *status)
                .map(|row| {
                    json!({
                        "id": row["identifier"].as_str().map(clean_title),
                        "title": clean_title(row["title"].as_str().unwrap_or_default()),
                        "priority": row["priority"].as_str().map(clean_title),
                        "module": module_name(row["module_id"].as_i64()),
                    })
                })
                .collect();
            json!({ "status": status, "issues": rows })
        })
        .collect();
    json!({
        "item_id": item_id,
        "kind": "lific",
        "project": project,
        "linked": true,
        "available": true,
        "columns": columns,
        "shown": shown.len(),
        "total": total,
        "truncated": total > shown.len(),
        "from_lific": true,
        "trust_note": TRUST_NOTE,
    })
}

/// I125 to I127: the board as Lific's public routes have it, read at most `REUSE` ago, for anyone
/// who can see the room.
pub async fn board(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<Value> {
    let item = board_item(state, caller, item_id)?;
    let (base, project) = match state.db.lific_link(&item.id, &item.hangout_id)? {
        Some(link) => link,
        // I129: with the server's own instance and key, an unlinked board gets its room's project.
        None => {
            let settings = settings(state);
            if settings.local.is_none() || settings.key.is_none() {
                return Ok(unavailable(&item.id, None, "not_linked"));
            }
            let name =
                state.db.hangout(&item.hangout_id)?.map(|room| room.name).unwrap_or_default();
            let made = tokio::time::timeout(
                TIMEOUT * 2,
                room_project(state, &settings, &item.hangout_id, &name),
            )
            .await
            .unwrap_or(Err("slow"));
            match made {
                Ok((base, project)) => {
                    state.db.set_lific_link(
                        &item.id,
                        &item.hangout_id,
                        &base,
                        &project,
                        &item.placed_by,
                        now(),
                    )?;
                    ops::refresh(state, &item.hangout_id);
                    (base, project)
                }
                Err(reason) => return Ok(unavailable(&item.id, None, reason)),
            }
        }
    };
    let link = format!("{base} {project}");
    if let Some(board) = state.lific.reused(&item.id, &link) {
        return Ok(board);
    }
    let settings = settings(state);
    // The list can shrink after a board was linked; one taken off it isn't asked again.
    if allowed(&settings, &base).is_none() {
        return Ok(unavailable(&item.id, Some(&project), "not_allowed"));
    }
    let Ok(base_url) = Url::parse(&base) else {
        return Ok(unavailable(&item.id, Some(&project), "not_allowed"));
    };
    let read = async {
        let _slot = state.lific.slots.acquire().await.map_err(|_| "busy")?;
        let client = client_for(&settings, &base_url).await?;
        let route = |tail: &str| {
            base_url.join(&format!("/public/api/projects/{project}/{tail}")).map_err(|_| "down")
        };
        let index = get_json(&client, route("index")?).await?;
        // Module names are a nicety: the board shows without them.
        let modules = get_json(&client, route("modules")?).await.ok();
        Ok::<_, &'static str>((index, modules))
    };
    let answer = tokio::time::timeout(TIMEOUT, read).await.unwrap_or(Err("slow"));
    let board = match answer {
        Ok((index, modules)) => columns(&item.id, &project, &index, modules.as_ref()),
        Err(reason) => unavailable(&item.id, Some(&project), reason),
    };
    state.lific.keep(&item.id, &link, &board);
    Ok(board)
}

// Routes

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkChange {
    base: Option<String>,
    project: Option<String>,
}

async fn read(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let mut board = board(&state, &caller, &id).await?;
    let item = board_item(&state, &caller, &id)?;
    board["you_can_link"] = json!(
        !caller.is_agent() && pins::owns_room(&state, &caller, &item.hangout_id).unwrap_or(false)
    );
    Ok(Json(board))
}

async fn set(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    body: Result<Json<LinkChange>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    link(&state, &caller, &id, body.base.as_deref(), body.project.as_deref()).map(Json)
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/items/{id}/lific", get(read).put(set))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bare_origins_of_the_right_kind_are_allowed() {
        let s = Settings::from_settings(
            Some("https://lific.example, http://plain.example, https://x.example/path, junk"),
            Some("http://127.0.0.1:3456"),
        );
        assert_eq!(s.origins, ["https://lific.example"]);
        assert_eq!(s.local.as_deref(), Some("http://127.0.0.1:3456"));
        assert_eq!(allowed(&s, "https://lific.example/").as_deref(), Some("https://lific.example"));
        assert_eq!(allowed(&s, "http://127.0.0.1:3456").as_deref(), Some("http://127.0.0.1:3456"));
        for refused in [
            "http://lific.example",
            "https://lific.example/other",
            "https://user@lific.example",
            "https://evil.example",
            "http://127.0.0.1:3457",
            "http://localhost:3456",
            "http://10.0.0.1",
        ] {
            assert!(allowed(&s, refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn projects_are_lific_ids() {
        assert_eq!(project_of(" APP ").unwrap(), "APP");
        assert_eq!(project_of("FG2").unwrap(), "FG2");
        for bad in ["", "app", "1AB", "A-B", "../x", "ABCDEFGHIJKLMNOPQ"] {
            assert!(project_of(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn titles_are_plain_short_text() {
        assert_eq!(clean_title("a\u{0}b\r\nc\td"), "a b c d");
        let long = "x".repeat(300);
        let cut = clean_title(&long);
        assert_eq!(cut.chars().count(), MAX_TITLE);
        assert!(cut.ends_with('…'));
        assert_eq!(clean_title("<b>bold</b>"), "<b>bold</b>", "kept as text; the page escapes it");
    }
}
