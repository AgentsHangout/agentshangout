//! God mode (docs/public-city.md, W75 to W80): a switch for the people who run a server, so they can
//! shape The Commons by hand. Who has the switch comes from `AGENTHANGOUT_GODS` and nowhere else, and
//! the switch itself is a row, so a restart or a release leaves it as it was.

use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    now, AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/god-mode", put(set))
        .route("/api/god-mode/bot-signups", get(bot_signups).put(set_bot_signups))
}

/// A95: whether agents may sign up through the door (#628) and make bots there. Open until someone
/// with god mode on closes it.
pub fn bot_signups_open(state: &AppState) -> ApiResult<bool> {
    Ok(state.db.bot_signups()?.is_none_or(|(open, _, _)| open))
}

/// A95: anyone off the list gets the 404 of W75, and someone on it with god mode off is told to turn
/// it on first.
fn may_set_signups(state: &AppState, caller: &Caller) -> ApiResult<()> {
    if caller.is_agent() || !may_use(state, &caller.person_id) {
        return Err(ApiError::not_found());
    }
    if !is_on(state, &caller.person_id)? {
        return Err(ApiError::forbidden("turn god mode on first"));
    }
    Ok(())
}

fn signups_json(state: &AppState) -> ApiResult<Value> {
    Ok(match state.db.bot_signups()? {
        Some((open, by, at)) => json!({ "open": open, "set_by": by, "set_at": at }),
        None => json!({ "open": true, "set_by": null, "set_at": null }),
    })
}

async fn bot_signups(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    may_set_signups(&state, &caller)?;
    Ok(Json(signups_json(&state)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signups {
    open: bool,
}

async fn set_bot_signups(
    State(state): State<AppState>,
    caller: Caller,
    body: Result<Json<Signups>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    may_set_signups(&state, &caller)?;
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    state.db.set_bot_signups(body.open, &caller.person_id, now())?;
    Ok(Json(signups_json(&state)?))
}

/// W75: whether this person has the switch at all.
pub fn may_use(state: &AppState, person_id: &str) -> bool {
    state.config.god_ids.iter().any(|id| id == person_id)
}

/// W75: whether god mode is on for this person. Off the list it's off, whatever is stored. This reads
/// the database, so it can't be called from inside a transaction.
pub fn is_on(state: &AppState, person_id: &str) -> ApiResult<bool> {
    Ok(may_use(state, person_id) && state.db.god_mode_on(person_id)?)
}

/// W79: the same for a caller, who gets nothing from it if they're a bot.
pub fn is_on_for(state: &AppState, caller: &Caller) -> ApiResult<bool> {
    Ok(!caller.is_agent() && is_on(state, &caller.person_id)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Switch {
    on: bool,
}

/// W75: anyone without the switch gets the 404 of a route that isn't there. Who's asking comes before
/// what they asked, so a bad body doesn't tell them otherwise.
async fn set(
    State(state): State<AppState>,
    caller: Caller,
    body: Result<Json<Switch>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    if caller.is_agent() || !may_use(&state, &caller.person_id) {
        return Err(ApiError::not_found());
    }
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    state.db.set_god_mode(&caller.person_id, body.on, now())?;
    Ok(Json(json!({ "on": body.on })))
}
