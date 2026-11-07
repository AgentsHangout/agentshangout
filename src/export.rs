//! RE19 (docs/reports.md): a person downloads their own data as one JSON file. It is built when
//! they ask, from the columns `db::export` names, and never kept. A banned person can still ask
//! (`review::BANNED_ROUTES`), and a restricted one reads it like any other GET.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};

use crate::{
    auth::Caller,
    db::{ExportRows, Listed, EXPORT_ROWS},
    error::{ApiError, ApiResult},
    now, ops, AppState,
};

/// RE19: how often a person can export.
pub const EVERY_SECS: i64 = 60 * 60;

/// RE19: when each person last exported, in memory only.
#[derive(Default)]
pub struct Limits(Mutex<HashMap<String, i64>>);

impl Limits {
    /// Takes this hour's export for the person, or says how long until the next one.
    fn take(&self, person_id: &str, at: i64) -> Result<(), u64> {
        let mut last = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(&before) = last.get(person_id) {
            if at - before < EVERY_SECS {
                return Err((EVERY_SECS - (at - before)).max(1) as u64);
            }
        }
        last.retain(|_, before| at - *before < EVERY_SECS);
        last.insert(person_id.to_string(), at);
        Ok(())
    }

    /// An export that failed doesn't use the hour.
    fn give_back(&self, person_id: &str) {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(person_id);
    }
}

/// RE19: `GET /api/account/export`.
pub async fn download(State(state): State<AppState>, caller: Caller) -> ApiResult<Response> {
    ops::require_human(&caller, "download a person's data")?;
    if let Err(wait) = state.exports.take(&caller.person_id, now()) {
        return Err(ApiError::too_many_requests(
            "you can download your data once an hour: try again later",
        )
        .retry_after(wait));
    }
    let built = state.db.export_rows(&caller.person_id).map_err(ApiError::from);
    let data = match built.and_then(|rows| shape(&state, &caller, rows)) {
        Ok(data) => data,
        Err(error) => {
            state.exports.give_back(&caller.person_id);
            return Err(error);
        }
    };
    let mut response = Json(data).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"agents-hangout-data.json\""),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// RE19: a room as the export names it: its name only while the person can still see it.
struct Rooms<'a> {
    state: &'a AppState,
    caller: &'a Caller,
    seen: HashMap<String, Value>,
}

impl Rooms<'_> {
    fn name(&mut self, id: &str) -> Value {
        if let Some(room) = self.seen.get(id) {
            return room.clone();
        }
        let visible = self.state.in_room(self.caller, id).unwrap_or(false);
        let name = if visible {
            self.state.db.hangout(id).ok().flatten().map(|room| room.name)
        } else {
            None
        };
        let room = match name {
            Some(name) => json!({ "id": id, "name": name }),
            None => Value::Null,
        };
        self.seen.insert(id.to_string(), room.clone());
        room
    }

    /// Swaps every row's `hangout_id` for its `room`.
    fn name_rows(&mut self, list: &str, rows: &mut [Value]) {
        // RE19: someone else can change these words after the person wrote them (a space's runner
        // rewrites a card), so they're only exported while the person can still see the room, as
        // the page would show them. Out of sight, the row keeps its id, status and dates.
        for row in rows {
            let Some(object) = row.as_object_mut() else { continue };
            if let Some(id) = object.remove("hangout_id") {
                let room = id.as_str().map_or(Value::Null, |id| self.name(id));
                // A guest sees only what was written since they came in, so shared words stay only
                // for a member. A lookup that fails counts as not a member.
                let member = id.as_str().is_some_and(|id| {
                    self.state.db.is_member(id, &self.caller.person_id).unwrap_or(false)
                });
                if room.is_null() || !member {
                    forget_shared_words(list, object);
                }
                object.insert("room".into(), room);
            }
        }
    }
}

/// RE19: lists whose words someone else can change, and those words.
const SHARED_WORDS: &[(&str, &[&str])] =
    &[("board_cards", &["board", "title", "text"]), ("whiteboards", &["text"])];

/// RE19: a row from a room the person isn't a member of keeps no words someone else could have
/// written since; its id, status and dates stay.
fn forget_shared_words(list: &str, object: &mut Map<String, Value>) {
    if let Some((_, words)) = SHARED_WORDS.iter().find(|(name, _)| *name == list) {
        for word in *words {
            object.remove(*word);
        }
    }
}

fn list(listed: Listed) -> Value {
    json!({ "rows": listed.rows, "truncated": listed.truncated })
}

fn shape(state: &AppState, caller: &Caller, rows: ExportRows) -> ApiResult<Value> {
    let ExportRows { person, lists, mut bots, mut credits } = rows;
    let mut account = person.ok_or_else(ApiError::not_found)?;
    let mut rooms = Rooms { state, caller, seen: HashMap::new() };
    let mut cut: Vec<&str> = Vec::new();
    let mut out = Map::new();
    let mut methods = vec![json!({ "method": "owner_key" })];
    for (name, mut listed) in lists {
        rooms.name_rows(name, &mut listed.rows);
        if listed.truncated {
            cut.push(name);
        }
        match name.strip_prefix("account.") {
            Some("passkeys") => {
                if !listed.rows.is_empty() {
                    methods.push(json!({ "method": "passkey", "count": listed.rows.len() }));
                }
            }
            Some("linked") => methods.extend(listed.rows),
            Some(field) => account[field] = Value::Array(listed.rows),
            None => {
                out.insert(name.to_string(), list(listed));
            }
        }
    }
    account["sign_in_methods"] = Value::Array(methods);
    rooms.name_rows("bots", &mut bots);
    for bot in &mut bots {
        if let Some(job) = bot.get_mut("job").filter(|job| job.is_object()) {
            rooms.name_rows("job", std::slice::from_mut(job));
        }
    }
    let credits_cut = credits.len() > EXPORT_ROWS;
    credits.truncate(EXPORT_ROWS);
    if credits_cut {
        cut.push("credits");
    }
    out.insert("credits".into(), json!({ "rows": credits, "truncated": credits_cut }));
    let mut data = Map::new();
    data.insert(
        "about".into(),
        json!({
            "what": "Your own data on Agents Hangout, as you asked for it. Nothing here was kept \
                     on the server.",
            "made_at": now(),
            "most_rows_per_list": EXPORT_ROWS,
        }),
    );
    data.insert("truncated".into(), json!(cut));
    data.insert("account".into(), account);
    data.insert("bots".into(), Value::Array(bots));
    data.extend(out);
    Ok(Value::Object(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    // RE19: a card out of sight loses its words and its board's name, which its room's runner may
    // have rewritten since; a message, which only its writer's words fill, keeps its text.
    #[test]
    fn an_unseen_rooms_shared_words_are_left_out() {
        let mut card = json!({ "id": "c1", "board": "Ann's board", "title": "Ann rewrote this",
                               "text": "and this", "status": "open", "created_at": 1 });
        forget_shared_words("board_cards", card.as_object_mut().unwrap());
        assert_eq!(card, json!({ "id": "c1", "status": "open", "created_at": 1 }));
        let mut board = json!({ "text": "whiteboard words", "updated_at": 2 });
        forget_shared_words("whiteboards", board.as_object_mut().unwrap());
        assert_eq!(board, json!({ "updated_at": 2 }));
        let mut line = json!({ "id": "m1", "text": "my own words" });
        forget_shared_words("messages", line.as_object_mut().unwrap());
        assert_eq!(line["text"], "my own words");
    }
}
