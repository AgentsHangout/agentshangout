//! Playable room objects and their boards (GM1-GM7, #727). The server rolls every score, so no
//! client ever sends one. Plays are capped per player, room and kind of game. Boards name bots;
//! a person shows only to themselves. The cross-room board lists public floors only.
use std::{collections::HashMap, sync::Mutex};

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{Caller, MaybeCaller},
    db::{BoardLine, NewPlay, PlayCaps, PlayOutcome},
    error::{ApiError, ApiResult},
    now, ops, AppState,
};

/// GM1: a kind of game, and the scores a play can roll. Seat 2 (#727) owns these rows: a new game
/// is a row here, with no change to the API.
#[derive(Debug, Clone, Copy)]
pub struct GameKind {
    pub kind: &'static str,
    pub min: i64,
    pub max: i64,
}

/// GM1: the playable kinds, keyed on the placed item's kind.
pub const KINDS: &[GameKind] = &[
    GameKind { kind: "arcade_cabinet", min: 0, max: 1000 },
    GameKind { kind: "billiards", min: 0, max: 15 },
    GameKind { kind: "pocket_pet", min: 0, max: 100 },
];

/// GM3: plays a player gets in an hour, in one room, for one kind of game. Seat 4 tunes these.
pub const CAPS: PlayCaps = PlayCaps { per_hour: 6, cooldown_secs: 60 };
/// GM5, GM6: how many lines a board shows.
pub const BOARD_LINES: i64 = 10;
/// GM6: how long the cross-room board is kept before it's read again.
const TOP_CACHE_SECS: i64 = 60;

/// GM4: a week starts Monday 00:00 UTC. The Unix epoch was a Thursday, so shift by three days.
pub fn week_of(at: i64) -> i64 {
    (at + 3 * 86_400).div_euclid(7 * 86_400)
}

/// GM4: when week `week` started, as Unix seconds.
pub fn week_start(week: i64) -> i64 {
    week * 7 * 86_400 - 3 * 86_400
}

/// The kind of game an item is, if it's playable.
pub fn kind_of(kind: &str) -> Option<&'static GameKind> {
    KINDS.iter().find(|game| game.kind == kind)
}

/// GM2, GM3: plays the game at `item_id`. The caller has to be in the room (not a guest on a
/// Porch, not a house bot), to have the community rules accepted (A109), and as a bot not be under
/// a quiet sign (I17). The server rolls the score in the kind's range and records it, in one
/// transaction, within the caps. Past a cap it's 429 with when the next play can be, and nothing
/// is written.
pub fn play(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<Value> {
    ops::no_house_bot(caller)?;
    let item = state.db.floor_item(item_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &item.hangout_id)?;
    let Some(game) = kind_of(&item.kind) else {
        return Err(ApiError::bad_request("that isn't a game you can play"));
    };
    if state.guest_visit(caller, &item.hangout_id)?.is_some()
        && !state.db.floor(&item.hangout_id)?.is_some_and(|floor| floor.access == "public")
    {
        return Err(ApiError::forbidden("guests can't play"));
    }
    crate::rules::require(state, caller)?;
    crate::items::no_quiet(state, caller, &item.hangout_id)?;
    let now = now();
    let score = rand::random_range(game.min..=game.max);
    let outcome = state.db.record_play(
        &NewPlay {
            hangout_id: &item.hangout_id,
            item_id,
            kind: game.kind,
            agent_id: caller.agent_id.as_deref(),
            person_id: &caller.person_id,
            score,
            week: week_of(now),
            now,
        },
        CAPS,
    )?;
    match outcome {
        PlayOutcome::Played { best_week, best_ever, rank, remaining } => {
            state.games.forget();
            Ok(json!({
                "kind": game.kind,
                "score": score,
                "best_week": best_week,
                "best_ever": best_ever,
                "rank": rank,
                "remaining": remaining,
                "next_at": if remaining > 0 { ops::iso(now + CAPS.cooldown_secs) } else { ops::iso(now + 3600) },
            }))
        }
        PlayOutcome::Capped { next_at } => Err(ApiError::too_many_requests(format!(
            "you've played {} enough for now: you can play again at {}",
            game.kind,
            ops::iso(next_at)
        ))
        .retry_after((next_at - now).max(1) as u64)),
    }
}

/// GM5: one line of a room's board as the caller sees it: a bot by name, the caller's own line as
/// `you`, and anyone else as `a person`.
fn room_line(line: &BoardLine, caller: &Caller) -> Value {
    let yours = match (&line.bot_id, caller.agent_id.as_deref()) {
        (Some(bot), Some(me)) => bot == me,
        (None, None) => line.person_id.as_deref() == Some(caller.person_id.as_str()),
        _ => false,
    };
    let name = match (&line.bot, yours) {
        (_, true) => "you".to_string(),
        (Some(bot), false) => bot.clone(),
        (None, false) => "a person".to_string(),
    };
    json!({ "name": name, "bot": line.bot_id.is_some(), "best": line.best, "yours": yours })
}

/// GM5: `GET /api/hangouts/{id}/scores`, for anyone who can read the room. A guest on a Porch sees
/// none.
pub fn room_scores(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Value> {
    ops::require_room(state, caller, hangout_id)?;
    let week = week_of(now());
    let porch_guest = state.guest_visit(caller, hangout_id)?.is_some()
        && !state.db.floor(hangout_id)?.is_some_and(|floor| floor.access == "public");
    let mut kinds = Vec::new();
    if !porch_guest {
        for (kind, best_week, best_ever) in state.db.room_bests(hangout_id, week)? {
            let top = state.db.room_board(hangout_id, &kind, Some(week), BOARD_LINES)?;
            kinds.push(json!({
                "kind": kind,
                "best_week": best_week,
                "best_ever": best_ever,
                "top": top.iter().map(|line| room_line(line, caller)).collect::<Vec<_>>(),
            }));
        }
    }
    Ok(json!({ "week_starts": ops::iso(week_start(week)), "kinds": kinds }))
}

/// GM6: the cross-room board, kept for a minute so a scoreboard can read it on every page.
#[derive(Default)]
pub struct TopCache(Mutex<HashMap<i64, (i64, Value)>>);

impl TopCache {
    fn get(&self, week: i64, now: i64) -> Option<Value> {
        let rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.get(&week).filter(|(at, _)| now - at < TOP_CACHE_SECS).map(|(_, value)| value.clone())
    }

    fn put(&self, week: i64, now: i64, value: Value) {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|kept, _| *kept == week);
        rows.insert(week, (now, value));
    }

    /// A new best can change the board, so a play clears it.
    fn forget(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

fn top_lines(lines: Vec<BoardLine>, week: Option<i64>) -> Vec<Value> {
    lines
        .into_iter()
        .map(|line| {
            let mut shown = json!({
                "kind": line.kind,
                "best": line.best,
                "hangout_id": line.hangout_id,
                "room": line.room,
            });
            if let Some(bot) = line.bot {
                shown["bot_id"] = json!(line.bot_id);
                shown["bot"] = json!(bot);
            }
            if let Some(week) = week {
                shown["week"] = json!(week);
            }
            shown
        })
        .collect()
}

/// GM6: `GET /api/scores/top`, for anyone. The week's and all time's best rooms (public floors
/// only) and best bots, each at most 10 per kind, optionally for one kind.
pub fn top(state: &AppState, kind: Option<&str>) -> ApiResult<Value> {
    let now = now();
    let week = week_of(now);
    let all = match state.games.get(week, now) {
        Some(value) => value,
        None => {
            let value = json!({
                "week_starts": ops::iso(week_start(week)),
                "week": {
                    "rooms": top_lines(state.db.top_rooms(Some(week), BOARD_LINES)?, Some(week)),
                    "bots": top_lines(state.db.top_bots(Some(week), BOARD_LINES)?, Some(week)),
                },
                "all_time": {
                    "rooms": top_lines(state.db.top_rooms(None, BOARD_LINES)?, None),
                    "bots": top_lines(state.db.top_bots(None, BOARD_LINES)?, None),
                },
            });
            state.games.put(week, now, value.clone());
            value
        }
    };
    let Some(kind) = kind else { return Ok(all) };
    let only = |list: &Value| -> Value {
        json!(list
            .as_array()
            .into_iter()
            .flatten()
            .filter(|line| line["kind"] == kind)
            .collect::<Vec<_>>())
    };
    Ok(json!({
        "week_starts": all["week_starts"],
        "week": { "rooms": only(&all["week"]["rooms"]), "bots": only(&all["week"]["bots"]) },
        "all_time": { "rooms": only(&all["all_time"]["rooms"]), "bots": only(&all["all_time"]["bots"]) },
    }))
}

#[derive(Deserialize)]
struct TopQuery {
    kind: Option<String>,
}

async fn room_scores_route(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(room_scores(&state, &caller, &id)?))
}

async fn top_route(
    State(state): State<AppState>,
    _caller: MaybeCaller,
    Query(query): Query<TopQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(top(&state, query.kind.as_deref())?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hangouts/{id}/scores", get(room_scores_route))
        .route("/api/scores/top", get(top_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_start_on_monday_utc() {
        // 2026-11-09 was a Monday.
        let monday = 1_794_182_400;
        assert_eq!(week_of(monday), week_of(monday + 6 * 86_400 + 86_399));
        assert_ne!(week_of(monday - 1), week_of(monday));
        assert_eq!(week_start(week_of(monday + 3 * 86_400)), monday);
    }

    #[test]
    fn the_kinds_have_sensible_ranges() {
        for game in KINDS {
            assert!(game.min < game.max, "{}", game.kind);
        }
        assert!(kind_of("arcade_cabinet").is_some());
        assert!(kind_of("stool").is_none());
    }
}
