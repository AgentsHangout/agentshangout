//! Since you were last here: a count of what happened for a person while they were away, for one
//! line at the top of the site (#984, under #968 item 16). Counts only: no room text, no names of
//! other people, and nothing for anyone but the signed-in person.
//!
//! The window starts at the person's last visit, and never goes back more than a week. A person
//! who has never recorded a visit gets the last day, the same as Needs you.

use axum::{extract::State, routing::get, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::{auth::Caller, error::ApiResult, now, ops, AppState};

/// S1: the longest look back. Older than this, the count isn't a reason to come back, it's a list.
pub const MAX_WINDOW_SECS: i64 = 7 * 86_400;
/// S2: with no recorded visit yet, look at the last day (as Needs you does).
pub const FIRST_WINDOW_SECS: i64 = 86_400;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/owner/since", get(since))
        .route("/api/owner/since/seen", post(mark_seen))
}

/// The start of the window for this person at `now`: their last visit, capped at MAX_WINDOW_SECS.
pub fn window_start(last_visit: Option<i64>, now: i64) -> i64 {
    let floor = now - MAX_WINDOW_SECS;
    match last_visit {
        Some(seen) => seen.max(floor),
        None => now - FIRST_WINDOW_SECS,
    }
}

/// S3: the counts since `since`, for one person. Everything here is a count.
pub struct Counts {
    pub mentions: i64,
    pub plans_invited: i64,
    pub people_joined_rooms: i64,
}

impl Counts {
    pub fn total(&self) -> i64 {
        self.mentions + self.plans_invited + self.people_joined_rooms
    }
}

/// Reads the counts for `person_id` since `since`. Only rows that belong to this person count.
pub fn counts(state: &AppState, person_id: &str, since: i64) -> rusqlite::Result<Counts> {
    let conn = state.db.conn();
    let mentions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM person_mentions WHERE person_id = ?1 AND at > ?2",
        rusqlite::params![person_id, since],
        |row| row.get(0),
    )?;
    let plans_invited: i64 = conn.query_row(
        "SELECT COUNT(*) FROM plan_invited_by WHERE person_id = ?1 AND invited_at > ?2",
        rusqlite::params![person_id, since],
        |row| row.get(0),
    )?;
    // Joins to a room this person is in, by people other than the person themself, counted once per
    // room. Only joins on or after this person's own join to that room count: someone who joined
    // before this person was in the room was already there when they arrived, so it isn't news.
    let people_joined_rooms: i64 = conn.query_row(
        "SELECT COUNT(*) FROM members m
         JOIN members me ON me.hangout_id = m.hangout_id AND me.person_id = ?1
         WHERE m.joined_at > ?2 AND m.person_id != ?1
           AND m.joined_at >= me.joined_at",
        rusqlite::params![person_id, since],
        |row| row.get(0),
    )?;
    // The window is `at > since` / `joined_at > since`, so an event in the same second as the
    // previous visit is not counted. Harmless at one-second resolution, and it keeps the reset exact.
    Ok(Counts { mentions, plans_invited, people_joined_rooms })
}

pub fn last_visit(state: &AppState, person_id: &str) -> rusqlite::Result<Option<i64>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare("SELECT seen_at FROM owner_visits WHERE person_id = ?1")?;
    let mut rows = stmt.query([person_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

// S4: GET /api/owner/since, for the signed-in person only.
async fn since(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see what happened since their last visit")?;
    let now = now();
    let start = window_start(last_visit(&state, &caller.person_id)?, now);
    let found = counts(&state, &caller.person_id, start)?;
    Ok(Json(json!({
        "since": start,
        "until": now,
        "total": found.total(),
        "counts": {
            "mentions": found.mentions,
            "plans_invited": found.plans_invited,
            "people_joined_rooms": found.people_joined_rooms,
        },
    })))
}

// S5: POST /api/owner/since/seen: this person has looked, so the next window starts now.
async fn mark_seen(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "mark their visit")?;
    state.db.conn().execute(
        "INSERT INTO owner_visits (person_id, seen_at) VALUES (?1, ?2)
         ON CONFLICT(person_id) DO UPDATE SET seen_at = excluded.seen_at",
        rusqlite::params![caller.person_id, now()],
    )?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // S1, S2: the window starts at the last visit, never more than a week back, and a first visit
    // looks at the last day.
    #[test]
    fn the_window_is_the_last_visit_capped_at_a_week() {
        let now = 1_000_000;
        assert_eq!(window_start(Some(now - 3600), now), now - 3600);
        assert_eq!(window_start(Some(now - 30 * 86_400), now), now - MAX_WINDOW_SECS);
        assert_eq!(window_start(None, now), now - FIRST_WINDOW_SECS);
    }
}
