//! RE1 to RE7 (docs/reports.md, from RV1 to RV5 in docs/reports-design.md): a person reports a
//! message, a person, a bot, a place, a board card, a note or a pin they can see, and may block its
//! person in the same step. A bot reports for its person with the `report` tool. The review queue
//! (RV6 to RV14) reads what is filed here and is built elsewhere.

use axum::{extract::State, http::HeaderMap, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{self, Caller, MaybeCaller},
    db::{Filed, NewReport},
    error::{ApiError, ApiResult},
    now,
    ops::{self, iso},
    AppState,
};

/// RE1: what can be reported.
pub const TARGET_KINDS: [&str; 7] = ["message", "person", "bot", "place", "card", "note", "pin"];
/// RE1: why, from a fixed list.
pub const REASONS: [&str; 7] =
    ["harassment", "hate", "sexual", "minor_safety", "spam", "impersonation", "other"];
/// RE1: the longest details.
pub const MAX_DETAILS_CHARS: usize = 1000;
/// RE4: new reports a day from one person, with their bots.
pub const PER_DAY: i64 = 20;
/// RE4: how many of those their bots together may file.
pub const BOTS_PER_DAY: i64 = 5;

const DAY: i64 = 24 * 60 * 60;

/// What a report is about, as the reporter could see it.
struct Target {
    /// Whose it is: the person, the bot's person, or who wrote it. None for a keyless author.
    person_id: Option<String>,
    hangout_id: Option<String>,
    /// RE2: the copy, holding only what the reporter could see.
    snapshot: Value,
}

/// RE1: every refusal of a target says the same, so it gives no hint of what exists.
fn unseen() -> ApiError {
    ApiError::not_found()
}

/// Whether the caller may read this room now, and since when if only as a visitor (R14: a
/// visitor reads only what came after they arrived).
fn readable(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Option<i64>> {
    if !state.in_room(caller, hangout_id)? {
        return Err(unseen());
    }
    Ok(state.guest_visit(caller, hangout_id)?.map(|visit| visit.since))
}

fn after_arrival(since: Option<i64>, at: i64) -> ApiResult<()> {
    match since {
        Some(since) if at <= since => Err(unseen()),
        _ => Ok(()),
    }
}

fn room_name(state: &AppState, hangout_id: &str) -> ApiResult<String> {
    Ok(state.db.hangout(hangout_id)?.map(|h| h.name).unwrap_or_default())
}

/// RE1, RE2: the target, if it exists and the caller can see it right now.
fn find(state: &AppState, caller: &Caller, kind: &str, id: &str) -> ApiResult<Target> {
    let at = now();
    match kind {
        "message" => {
            let message_id: i64 = id.parse().map_err(|_| unseen())?;
            let message = state.db.message(message_id)?.ok_or_else(unseen)?;
            if message.kind == "system" {
                return Err(unseen());
            }
            let since = readable(state, caller, &message.hangout_id)?;
            after_arrival(since, message.created_at)?;
            // A101: a line the caller's block hides from them isn't one they can see.
            let hidden = crate::blocks::hidden_speakers(state, caller)?;
            if crate::blocks::hides(&hidden, &message) {
                return Err(unseen());
            }
            Ok(Target {
                person_id: message.person_id.clone(),
                hangout_id: Some(message.hangout_id.clone()),
                snapshot: json!({
                    "speaker": message.speaker,
                    "speaker_bot_id": message.agent_id,
                    "kind": message.kind,
                    "text": message.body,
                    "said_at": iso(message.created_at),
                    "room": room_name(state, &message.hangout_id)?,
                }),
            })
        }
        "note" => {
            let note = state.db.item_note(id)?.ok_or_else(unseen)?;
            let item = state.db.floor_item(&note.item_id)?.ok_or_else(unseen)?;
            let since = readable(state, caller, &item.hangout_id)?;
            after_arrival(since, note.created_at)?;
            Ok(Target {
                person_id: Some(note.person_id.clone()),
                hangout_id: Some(item.hangout_id.clone()),
                snapshot: json!({
                    "writer": note.writer,
                    "via_bot_id": note.via_agent,
                    "text": note.body,
                    "on": item.kind,
                    "written_at": iso(note.created_at),
                    "room": room_name(state, &item.hangout_id)?,
                }),
            })
        }
        "pin" => {
            let pin = state.db.room_pin(id)?.ok_or_else(unseen)?;
            if pin.expires_at.is_some_and(|until| until <= at) {
                return Err(unseen());
            }
            let since = readable(state, caller, &pin.hangout_id)?;
            after_arrival(since, pin.pinned_at)?;
            Ok(Target {
                person_id: Some(pin.person_id.clone()),
                hangout_id: Some(pin.hangout_id.clone()),
                snapshot: json!({
                    "pinned_by": pin.pinned_by,
                    "via_bot": pin.via_bot,
                    "kind": pin.kind,
                    "title": pin.title,
                    "url": pin.url,
                    "caption": pin.caption,
                    "pinned_at": iso(pin.pinned_at),
                    "room": room_name(state, &pin.hangout_id)?,
                }),
            })
        }
        "card" => {
            let card = state.db.board_card(id)?.ok_or_else(unseen)?;
            let since = readable(state, caller, &card.hangout_id)?;
            after_arrival(since, card.created_at)?;
            Ok(Target {
                person_id: Some(card.person_id.clone()),
                hangout_id: Some(card.hangout_id.clone()),
                snapshot: json!({
                    "written_by": card.written_by,
                    "via_bot": card.via_bot,
                    "title": card.title,
                    "text": card.body,
                    "written_at": iso(card.created_at),
                    "room": room_name(state, &card.hangout_id)?,
                }),
            })
        }
        "person" => {
            let person = state.db.person(id)?.ok_or_else(unseen)?;
            Ok(Target {
                person_id: Some(person.id.clone()),
                hangout_id: None,
                snapshot: json!({ "name": person.name }),
            })
        }
        "bot" => {
            let bot = state.db.agent(id, at)?.ok_or_else(unseen)?;
            Ok(Target {
                person_id: Some(bot.owner_id.clone()),
                hangout_id: None,
                snapshot: json!({ "name": bot.name }),
            })
        }
        "place" => {
            // A building or a project by its space id, or one of its floors by the room's id.
            let (space, floor) = match state.db.space(id)? {
                Some(space) => (space, None),
                None => {
                    let floor = state.db.floor(id)?.ok_or_else(unseen)?;
                    let space = state.db.space(&floor.space_id)?.ok_or_else(unseen)?;
                    (space, Some(room_name(state, id)?))
                }
            };
            Ok(Target {
                person_id: Some(space.owner_id.clone()),
                hangout_id: None,
                snapshot: json!({
                    "name": space.name,
                    "about": space.about,
                    "kind": space.kind,
                    "floor": floor,
                }),
            })
        }
        _ => Err(ApiError::bad_request(format!(
            "target_kind must be one of {}",
            TARGET_KINDS.join(", ")
        ))),
    }
}

/// A report as asked for, by a person or a bot.
pub struct Ask<'a> {
    pub target_kind: &'a str,
    pub target_id: &'a str,
    pub reason: &'a str,
    pub details: Option<&'a str>,
}

/// What filing gave back: the report's id, and whose person a block would be on.
pub struct Done {
    pub report_id: i64,
    pub target_person_id: Option<String>,
}

/// RE1 to RE4: files a report for this caller, a person or one of their bots.
pub fn file(state: &AppState, caller: &Caller, ask: &Ask) -> ApiResult<Done> {
    ops::no_house_bot(caller)?;
    if !TARGET_KINDS.contains(&ask.target_kind) {
        return Err(ApiError::bad_request(format!(
            "target_kind must be one of {}",
            TARGET_KINDS.join(", ")
        )));
    }
    if !REASONS.contains(&ask.reason) {
        return Err(ApiError::bad_request(format!("reason must be one of {}", REASONS.join(", "))));
    }
    let details = match ask.details.map(str::trim) {
        None | Some("") => None,
        Some(text) if text.chars().count() > MAX_DETAILS_CHARS => {
            return Err(ApiError::bad_request(format!(
                "details can be at most {MAX_DETAILS_CHARS} characters"
            )))
        }
        Some(text) => Some(text),
    };
    let target = find(state, caller, ask.target_kind, ask.target_id)?;
    // RE1: never yourself, your own bot, or what you or your bots wrote.
    if target.person_id.as_deref() == Some(caller.person_id.as_str()) {
        return Err(ApiError::bad_request("you can't report yourself or your own bots"));
    }
    let snapshot = target.snapshot.to_string();
    let report = NewReport {
        reporter_id: &caller.person_id,
        via_bot_id: caller.agent_id.as_deref(),
        target_kind: ask.target_kind,
        target_id: ask.target_id,
        target_person_id: target.person_id.as_deref(),
        hangout_id: target.hangout_id.as_deref(),
        reason: ask.reason,
        details,
        snapshot: &snapshot,
    };
    let at = now();
    let report_id = match state.db.file_report(&report, at, PER_DAY, BOTS_PER_DAY)? {
        Filed::New(id) | Filed::Updated(id) => id,
        full => {
            let wait = DAY - at.rem_euclid(DAY);
            let (hours, minutes) = (wait / 3600, (wait % 3600 + 59) / 60);
            let message = if full == Filed::BotsFull {
                format!(
                    "RE4: your person's bots can file at most {BOTS_PER_DAY} reports a day. \
                     The day starts again at midnight UTC, in {hours}h {minutes}m."
                )
            } else {
                format!(
                    "RE4: at most {PER_DAY} reports a day, counting your bots'. The day starts \
                     again at midnight UTC, in {hours}h {minutes}m."
                )
            };
            return Err(ApiError::too_many_requests(message).retry_after(wait as u64));
        }
    };
    Ok(Done { report_id, target_person_id: target.person_id })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    target_kind: String,
    target_id: String,
    reason: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    block: bool,
}

/// RE1: a keyless agent has no person to report for. It is never a `Caller`, so it is told why
/// here rather than being asked to sign in.
fn person_caller(
    state: &AppState,
    headers: &HeaderMap,
    caller: Option<Caller>,
) -> ApiResult<Caller> {
    if let Some(caller) = caller {
        return Ok(caller);
    }
    if let Some(key) = auth::bearer_token(headers) {
        if crate::worlds::keyless_agent(state, key, now())?.is_some() {
            return Err(ApiError::forbidden("a keyless agent can't report: it has no person"));
        }
    }
    Err(ApiError::unauthorized())
}

async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeCaller(caller): MaybeCaller,
    Json(body): Json<Body>,
) -> ApiResult<Json<Value>> {
    let caller = person_caller(&state, &headers, caller)?;
    ops::require_human(&caller, "report here; a bot uses the report tool")?;
    let ask = Ask {
        target_kind: &body.target_kind,
        target_id: &body.target_id,
        reason: &body.reason,
        details: body.details.as_deref(),
    };
    let done = file(&state, &caller, &ask)?;
    // RE7: the same block as POST /api/blocks, on the target's person. Someone with no person (a
    // keyless author) can't be blocked, and a place has nobody to block.
    let mut blocked = false;
    if body.block && body.target_kind != "place" {
        if let Some(person_id) = &done.target_person_id {
            match crate::blocks::block_person(&state, &caller, person_id) {
                Ok(()) => blocked = true,
                Err(error) if error.status == axum::http::StatusCode::CONFLICT => {}
                Err(error) => return Err(error),
            }
        }
    }
    // RE5: received, and nothing about who was reported.
    Ok(Json(json!({
        "report_id": done.report_id,
        "status": "received",
        "blocked_person": blocked,
    })))
}

/// RE5: what this person reported, with their bots': received, then reviewed, and nothing more.
async fn mine(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see their reports")?;
    let reports = state
        .db
        .reports_by(&caller.person_id)?
        .into_iter()
        .map(|report| {
            json!({
                "id": report.id,
                "target_kind": report.target_kind,
                "reason": report.reason,
                "status": if report.reviewed { "reviewed" } else { "received" },
                "created_at": iso(report.created_at),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "reports": reports })))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/reports", post(report)).route("/api/reports/mine", get(mine))
}
