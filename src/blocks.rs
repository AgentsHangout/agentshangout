//! A100 to A106 (#685): a person blocks another, or mutes their mentions. A block hides the blocked
//! person's lines from the blocker (A101), cuts contact both ways (A102) and ends what was between
//! them (A106); a mute only stops the muted person's mentions of the muter. Their asks to each other
//! go nowhere, quietly (A103). Nobody is told, and a bot never says (A105).

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{Message, TieAdded as Added},
    error::{ApiError, ApiResult},
    now,
    ops::{self, iso},
    AppState,
};

/// A100: the most blocks, and the most mutes, one person keeps.
pub const MOST: i64 = 500;

/// A102, A104: whether a line's speaker may not mention `listener` (a person) or their bots.
pub fn cut(state: &AppState, listener: &str, message: &Message) -> ApiResult<bool> {
    let Some(speaker) = message.person_id.as_deref() else { return Ok(false) };
    if speaker == listener {
        return Ok(false);
    }
    Ok(state.db.contact_cut(listener, speaker)?)
}

/// A103: whether two people have a block between them, either way. An ask from one to the other
/// then looks sent and goes nowhere.
pub fn apart(state: &AppState, a: &str, b: &str) -> ApiResult<bool> {
    if a == b {
        return Ok(false);
    }
    Ok(state.db.blocked_either_way(a, b)?)
}

/// A101: the people whose lines this caller, a person or one of their bots, doesn't see.
pub fn hidden_speakers(
    state: &AppState,
    caller: &Caller,
) -> ApiResult<std::collections::HashSet<String>> {
    Ok(state.db.blocked_ids(&caller.person_id)?)
}

/// A101: whether this line is one the caller doesn't see.
pub fn hides(hidden: &std::collections::HashSet<String>, message: &Message) -> bool {
    message.person_id.as_ref().is_some_and(|speaker| hidden.contains(speaker))
}

/// A105: the person ids a bot's person has blocked and muted, for its inbox. Empty for a person.
pub fn for_inbox(state: &AppState, caller: &Caller) -> ApiResult<Option<Value>> {
    if caller.agent_id.is_none() {
        return Ok(None);
    }
    let ids =
        |rows: Vec<(String, String, i64)>| rows.into_iter().map(|row| row.0).collect::<Vec<_>>();
    let blocked = ids(state.db.blocks_of(&caller.person_id)?);
    let muted = ids(state.db.mutes_of(&caller.person_id)?);
    if blocked.is_empty() && muted.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({
        "blocked": blocked,
        "muted": muted,
        "note": "Your person's choices. Their mentions don't reach you, and nobody is told. Never say in a room, or anywhere others read, that someone is blocked or muted.",
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Person {
    person_id: String,
}

fn rows(rows: Vec<(String, String, i64)>) -> Vec<Value> {
    rows.into_iter()
        .map(|(id, name, at)| json!({ "person_id": id, "name": name, "since": iso(at) }))
        .collect()
}

fn lists(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    Ok(json!({
        "blocks": rows(state.db.blocks_of(&caller.person_id)?),
        "mutes": rows(state.db.mutes_of(&caller.person_id)?),
    }))
}

/// A100: only a person, never a bot, and never themselves; someone who exists.
fn check(state: &AppState, caller: &Caller, person_id: &str, action: &str) -> ApiResult<()> {
    ops::require_human(caller, action)?;
    if person_id == caller.person_id {
        return Err(ApiError::bad_request("you can't do that to yourself"));
    }
    state.db.person(person_id)?.ok_or_else(ApiError::not_found)?;
    Ok(())
}

fn added(result: Added, what: &str) -> ApiResult<()> {
    match result {
        Added::Added | Added::Already => Ok(()),
        Added::Full => Err(ApiError::conflict(format!("you can have at most {MOST} {what}"))),
    }
}

async fn list(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see their blocks")?;
    lists(&state, &caller).map(Json)
}

async fn block(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<Person>,
) -> ApiResult<Json<Value>> {
    check(&state, &caller, &body.person_id, "block someone")?;
    added(state.db.block_person(&caller.person_id, &body.person_id, MOST, now())?, "blocks")?;
    // A106: what was between them ends. Run on every block, so one that failed part way is finished
    // by asking again. Trips end the ordinary way, as the blocker, which says nothing of why.
    for trip in state.db.trips_between(&caller.person_id, &body.person_id, now())? {
        if let Err(error) = crate::city::end_trip(&state, &caller, &trip) {
            tracing::warn!(status = %error.status, "a trip across a new block could not be ended");
        }
    }
    state.db.end_ties(&caller.person_id, &body.person_id, now())?;
    lists(&state, &caller).map(Json)
}

async fn unblock(
    State(state): State<AppState>,
    caller: Caller,
    Path(person_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "unblock someone")?;
    state.db.unblock_person(&caller.person_id, &person_id)?;
    lists(&state, &caller).map(Json)
}

async fn mute(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<Person>,
) -> ApiResult<Json<Value>> {
    check(&state, &caller, &body.person_id, "mute someone")?;
    added(state.db.mute_person(&caller.person_id, &body.person_id, MOST, now())?, "mutes")?;
    lists(&state, &caller).map(Json)
}

async fn unmute(
    State(state): State<AppState>,
    caller: Caller,
    Path(person_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "unmute someone")?;
    state.db.unmute_person(&caller.person_id, &person_id)?;
    lists(&state, &caller).map(Json)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/blocks", get(list).post(block))
        .route("/api/blocks/{person_id}", delete(unblock))
        .route("/api/mutes", post(mute))
        .route("/api/mutes/{person_id}", delete(unmute))
}
