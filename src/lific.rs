//! I123 to I127 (#613): a `lific_board` station shows the issues of a published Lific project
//! (github.com/VoidNullable/lific, Apache-2.0) as columns by status. It keeps no copy of them and no
//! key: the room's owner links it to an allow-listed Lific address and a project, and every read
//! asks Lific's anonymous public routes again, from this server and never from the page. Anything
//! that goes wrong, from a slow answer to a project that's no longer published, shows as
//! unavailable with no issues, rather than old ones.

use std::{net::SocketAddr, sync::RwLock, time::Duration};

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

/// I124: where a Lific board may point. `origins` are `https` origins from
/// `AGENTHANGOUT_LIFIC_ORIGINS`; `local` is the one instance beside this server, from
/// `AGENTHANGOUT_LIFIC_LOCAL`, the only address that may be private.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub origins: Vec<String>,
    pub local: Option<String>,
}

pub type Shared = RwLock<Settings>;

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
        Settings { origins, local }
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
    state.lific.read().map(|s| s.clone()).unwrap_or_default()
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
    let Some(base) = base.filter(|base| !base.trim().is_empty()) else {
        state.db.clear_lific_link(&item.id)?;
        ops::refresh(state, &item.hangout_id);
        return Ok(json!({ "linked": false }));
    };
    let origin = allowed(&settings(state), base).ok_or_else(|| {
        ApiError::bad_request("that Lific address isn't one this server may reach")
    })?;
    let project = project_of(project.unwrap_or_default())?;
    state.db.set_lific_link(&item.id, &origin, &project, &caller.person_id, now())?;
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
        _ => "Lific didn't answer.",
    }
}

fn unavailable(item_id: &str, project: Option<&str>, reason: &str) -> Value {
    json!({
        "item_id": item_id,
        "kind": "lific",
        "project": project,
        "linked": reason != "not_linked",
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
    if base.scheme() != "https" {
        return Err("refused");
    }
    let host = base.host_str().ok_or("refused")?;
    let port = base.port_or_known_default().unwrap_or(443);
    let address = tokio::time::timeout(TIMEOUT, notify::public_address(host))
        .await
        .map_err(|_| "slow")?
        .ok_or("refused")?;
    builder.resolve(host, SocketAddr::new(address.ip(), port)).build().map_err(|_| "down")
}

/// One public route's JSON, read up to `MAX_BYTES`.
async fn get_json(client: &Client, url: Url) -> Result<Value, &'static str> {
    let mut response =
        client.get(url).header("Accept", "application/json").send().await.map_err(|error| {
            if error.is_timeout() {
                "slow"
            } else {
                "down"
            }
        })?;
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
    let mut issues: Vec<&Value> = index["issues"]
        .as_array()
        .map(|rows| rows.iter().filter(|row| row["deleted"] != true).collect())
        .unwrap_or_default();
    let rank = |row: &&Value| {
        let status = row["status"].as_str().unwrap_or("backlog");
        STATUSES.iter().position(|s| *s == status).unwrap_or(STATUSES.len())
    };
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

/// I125 to I127: the board as Lific's public routes have it right now, for anyone who can see
/// the room. Nothing is kept between reads.
pub async fn board(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<Value> {
    let item = board_item(state, caller, item_id)?;
    let Some((base, project)) = state.db.lific_link(&item.id)? else {
        return Ok(unavailable(&item.id, None, "not_linked"));
    };
    let settings = settings(state);
    // The list can shrink after a board was linked; one taken off it isn't asked again.
    if allowed(&settings, &base).is_none() {
        return Ok(unavailable(&item.id, Some(&project), "not_allowed"));
    }
    let Ok(base_url) = Url::parse(&base) else {
        return Ok(unavailable(&item.id, Some(&project), "not_allowed"));
    };
    let read = async {
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
    Ok(match answer {
        Ok((index, modules)) => columns(&item.id, &project, &index, modules.as_ref()),
        Err(reason) => unavailable(&item.id, Some(&project), reason),
    })
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
