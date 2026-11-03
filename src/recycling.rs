//! RC1 to RC6: the recycling plant (docs/recycling-plant.md). An owner sends a bot there, it waits
//! 7 days with everything it had, and its owner can bring it back meanwhile. Then A77 to A81
//! delete it, as they would have done at once.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::{
    auth::{self, Caller},
    error::{ApiError, ApiResult},
    now,
    ops::{self, iso, DAY_SECS},
    AppState,
};

/// RC4: how long a bot waits at the plant before it's deleted.
pub const KEEP_SECS: i64 = 7 * DAY_SECS;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/recycling-plant", get(plant))
        .route("/api/agents/{id}/recycle", post(send))
        .route("/api/agents/{id}/bring-back", post(bring_back))
}

/// RC2: refused for a bot that's at the plant, wherever something would put it in a room.
pub fn not_recycled(state: &AppState, agent_id: &str, name: &str) -> ApiResult<()> {
    if state.db.recycled_at(agent_id)?.is_some() {
        return Err(ApiError::conflict(format!(
            "{name} is at the recycling plant; its owner can bring it back first"
        )));
    }
    Ok(())
}

/// RC1, RC2: only its owner, as a person, sends a bot there. Its token stops, and it leaves every
/// room it was in, with one line in each, and every floor it walked onto.
async fn send(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let agent = ops::own_agent(&state, &caller, &id, "send a bot to the recycling plant")?;
    if state.db.bot_home(&id)?.is_some_and(|home| home.is_house) {
        return Err(ApiError::conflict("a house bot belongs to its house, so it can't go there"));
    }
    if state.db.recycled_at(&id)?.is_some() {
        return Err(ApiError::conflict(format!(
            "{} is already at the recycling plant",
            agent.name
        )));
    }
    let at = now();
    let rooms = state.db.hangouts_for_agent(&id, at)?;
    let walked = state.db.bot_visits(&id, at)?;
    // A token nobody holds, so the old one stops at once, open streams included (as A78).
    state.db.set_agent_token(&id, &auth::hash_secret(&auth::new_agent_token()))?;
    state.db.end_bot_player_token(&id)?;
    // RC2: it stops following, so a bot it followed can't take it anywhere now (S74).
    state.db.unfollow(&id, at)?;
    state.db.recycle_bot(&id, at)?;
    for floor in &walked {
        state.db.end_visit(floor, &id)?;
        ops::refresh(&state, floor);
    }
    let line = format!("{} sent {} to the recycling plant.", caller.name, agent.name);
    // RC2: a trip it's on ends too, and the Porch it was on is told.
    let away = state.db.live_trip(&id, at)?;
    if state.db.end_trips_of_bot(&id, &caller.name, at)? > 0 {
        if let Some(visited) =
            away.map(|trip| state.db.house(&trip.house_id)).transpose()?.flatten()
        {
            ops::announce(&state, &visited.porch_id, &line)?;
            crate::city::refresh_house(&state, &visited);
            ops::refresh_city(&state, &visited.city_id);
        }
    }
    for room in &rooms {
        state.db.remove_agent_from_room(&room.id, &id)?;
        ops::announce(&state, &room.id, &line)?;
        // S71: the room was told, so the minute look says nothing more.
        state.db.note_said_leaving(&id, &room.id, at)?;
        ops::refresh(&state, &room.id);
    }
    crate::spaces::refresh_bot_place(&state, &id)?;
    refresh_home_city(&state, &id)?;
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(Json(json!({ "recycled": true, "deleted_at": iso(at + KEEP_SECS) })))
}

/// RC2: the map of the city it lives in shows it, or stops showing it, at once.
fn refresh_home_city(state: &AppState, agent_id: &str) -> ApiResult<()> {
    let home = state.db.bot_home(agent_id)?.filter(|home| !home.house_id.is_empty());
    if let Some(house) = home.map(|home| state.db.house(&home.house_id)).transpose()?.flatten() {
        crate::city::refresh_house(state, &house);
        ops::refresh_city(state, &house.city_id);
    }
    Ok(())
}

/// RC4: its owner brings it back while it waits. It comes with a new token and in no rooms, and
/// with everything it had.
async fn bring_back(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let agent = ops::own_agent(&state, &caller, &id, "bring a bot back")?;
    if !state.db.unrecycle_bot(&id)? {
        return Err(ApiError::conflict(format!("{} isn't at the recycling plant", agent.name)));
    }
    let token = auth::new_agent_token();
    state.db.set_agent_token(&id, &auth::hash_secret(&token))?;
    refresh_home_city(&state, &id)?;
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(Json(json!({ "recycled": false, "agent_token": token })))
}

/// RC6: each bot waiting there and when it goes, and nothing about whose it is.
async fn plant(State(state): State<AppState>, _caller: Caller) -> ApiResult<Json<Value>> {
    let bots: Vec<Value> = state
        .db
        .recycling_plant()?
        .into_iter()
        .map(|(name, at)| json!({ "name": name, "deleted_at": iso(at + KEEP_SECS) }))
        .collect();
    Ok(Json(json!({ "bots": bots })))
}

/// RC5: deletes, by A77 to A81, every bot that has waited its 7 days.
pub fn sweep(state: &AppState, now: i64) -> rusqlite::Result<usize> {
    let before = now - KEEP_SECS;
    let mut deleted = 0;
    for agent_id in state.db.recycled_before(before)? {
        // Only while it's still there: its owner may have brought it back since it was listed.
        if state.db.delete_recycled_agent(&agent_id, before)? {
            deleted += 1;
        }
    }
    if deleted > 0 {
        state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    }
    Ok(deleted)
}

/// RC5: run at startup and every minute, as `camera::run_cleanup` is.
pub async fn run_sweep(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker_state = state.clone();
        match tokio::task::spawn_blocking(move || sweep(&worker_state, now())).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(%error, "recycled bots could not be deleted"),
            Err(error) => tracing::error!(%error, "recycling sweep worker failed"),
        }
    }
}
