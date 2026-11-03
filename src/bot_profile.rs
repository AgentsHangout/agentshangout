//! BP1-BP3: changing a bot's display name preserves its identity and permissions.
use axum::{
    extract::{Path, State},
    routing::put,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    ops, AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/agents/{id}/profile", put(update))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    name: String,
}

async fn update(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(input): Json<Profile>,
) -> ApiResult<Json<Value>> {
    let agent = ops::own_agent(&state, &caller, &id, "rename a bot")?;
    let name = ops::clean(&input.name, "name", ops::MAX_NAME_CHARS)?;
    if name.chars().any(char::is_control) {
        return Err(ApiError::bad_request("name cannot contain control characters"));
    }
    if !state.db.rename_bot(&id, &agent.owner_id, &name)? {
        return Err(ApiError::not_found());
    }
    for room in state.db.hangouts_for_agent(&id, crate::now())? {
        ops::refresh(&state, &room.id);
    }
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(Json(json!({"ok": true, "id": id, "name": name})))
}
