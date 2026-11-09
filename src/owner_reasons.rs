//! Reasons to come back (#999, under #968 item 16): up to three short lines under the since line,
//! about the signed-in person's own bots and house only. Counts and bot names, never titles, room
//! text or other people's names.
//!
//! The three reasons, in this order (one line each, bots joined on the line):
//! - R1a: a plan one of their bots is in got a new answer from someone else. Only plans the person
//!   has already answered and wasn't invited to; the others are in Needs you already (#997).
//! - R1b: a vote in a room one of their bots is in closes within 24 hours.
//! - R1c: a lab pull request one of their bots claimed has merged.
//!
//! The window is the same as #984 (`owner_since::window_start`): a plan answer counts if it came
//! after the window starts, a vote counts if it opened after it, and a merge if it was verified after
//! it. Nothing is shown for a bot key or no key.

use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::{auth::Caller, error::ApiResult, now, ops, owner_since, AppState};

/// R1b: a vote counts as closing soon when it closes within this long from now.
pub const CLOSING_SECS: i64 = 86_400;
/// R4: at most this many reason lines under the since line.
pub const MAX_REASONS: usize = 3;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/owner/reasons", get(reasons))
}

/// R1b: a vote closes soon if it's still open and closes after `now`, within CLOSING_SECS.
pub fn closes_soon(closes_at: i64, now: i64) -> bool {
    closes_at > now && closes_at - now <= CLOSING_SECS
}

/// One bot's count for a reason.
#[derive(Debug, Clone, PartialEq)]
pub struct BotCount {
    pub name: String,
    pub count: i64,
}

/// The per-bot counts, for the three reasons.
pub struct Found {
    pub plan_answers: Vec<BotCount>,
    pub votes_closing: Vec<BotCount>,
    pub lab_merged: Vec<BotCount>,
}

/// The reason lines, in order, with no empty ones and at most MAX_REASONS.
pub fn lines(found: &Found) -> Vec<(&'static str, String, Vec<BotCount>)> {
    let parts = [
        ("plan_answers", &found.plan_answers, "plan got a new answer", "plans got new answers"),
        (
            "vote_closing",
            &found.votes_closing,
            "vote closes within 24 hours",
            "votes close within 24 hours",
        ),
        ("lab_merged", &found.lab_merged, "merged lab PR", "merged lab PRs"),
    ];
    parts
        .iter()
        .filter(|(_, bots, _, _)| !bots.is_empty())
        .take(MAX_REASONS)
        .map(|(kind, bots, one, many)| {
            let text = bots
                .iter()
                .map(|bot| {
                    let word = if bot.count == 1 { *one } else { *many };
                    format!("{}: {} {}", bot.name, bot.count, word)
                })
                .collect::<Vec<_>>()
                .join("; ");
            (*kind, text, bots.to_vec())
        })
        .collect()
}

fn bots(rows: rusqlite::Result<Vec<(String, i64)>>) -> rusqlite::Result<Vec<BotCount>> {
    Ok(rows?.into_iter().map(|(name, count)| BotCount { name, count }).collect())
}

/// Reads the counts for `person_id` in the window that starts at `since` and ends at `now`.
pub fn counts(state: &AppState, person_id: &str, since: i64, now: i64) -> rusqlite::Result<Found> {
    let conn = state.db.conn();
    let run =
        |sql: &str, params: &[&dyn rusqlite::ToSql]| -> rusqlite::Result<Vec<(String, i64)>> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params, |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect()
        };
    // R1a: their bot's room has a plan they already answered, and someone else answered since.
    let plan_answers = bots(run(
        "SELECT a.name, COUNT(DISTINCT e.id) FROM agents a
         JOIN room_agents ra ON ra.agent_id = a.id
         JOIN events e ON e.hangout_id = ra.hangout_id
         JOIN rsvps mine ON mine.event_id = e.id AND mine.person_id = ?1
         JOIN rsvps r ON r.event_id = e.id AND r.person_id != ?1 AND r.updated_at > ?2
         WHERE a.owner_id = ?1 AND e.status != 'cancelled' AND e.ends_at >= ?3
           AND NOT EXISTS (SELECT 1 FROM plan_invited_by b WHERE b.event_id = e.id AND b.person_id = ?1)
         GROUP BY a.id ORDER BY a.name, a.id",
        &[&person_id, &since, &now],
    ))?;
    // R1b: a vote in their bot's room that's still open, closes within the day, and opened in the window.
    let closing_until = now + CLOSING_SECS;
    let votes_closing = bots(run(
        "SELECT a.name, COUNT(DISTINCT v.id) FROM agents a
         JOIN room_agents ra ON ra.agent_id = a.id
         JOIN votes v ON v.hangout_id = ra.hangout_id
         WHERE a.owner_id = ?1 AND v.closed_at IS NULL AND v.opened_at > ?2
           AND v.closes_at > ?3 AND v.closes_at <= ?4
         GROUP BY a.id ORDER BY a.name, a.id",
        &[&person_id, &since, &now, &closing_until],
    ))?;
    // R1c: a lab PR their bot claimed, verified as merged in the window.
    let lab_merged = bots(run(
        "SELECT a.name, COUNT(DISTINCT w.lab_id || '#' || w.number) FROM research_work_awards w
         JOIN research_work_claims c ON c.lab_id = w.lab_id AND c.number = w.number AND c.agent_id = w.agent_id
         JOIN agents a ON a.id = w.agent_id
         WHERE a.owner_id = ?1 AND w.verified_at > ?2
         GROUP BY a.id ORDER BY a.name, a.id",
        &[&person_id, &since],
    ))?;
    Ok(Found { plan_answers, votes_closing, lab_merged })
}

// R3: GET /api/owner/reasons, for the signed-in person only.
async fn reasons(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see their reasons to come back")?;
    let now = now();
    let since = owner_since::window_start(owner_since::last_visit(&state, &caller.person_id)?, now);
    let found = counts(&state, &caller.person_id, since, now)?;
    let lines: Vec<Value> = lines(&found)
        .into_iter()
        .map(|(kind, text, bots)| {
            json!({
                "kind": kind,
                "line": text,
                "bots": bots.iter().map(|bot| json!({ "name": bot.name, "count": bot.count })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({ "since": since, "until": now, "reasons": lines })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // R1b, S1, S2: the window starts at the last visit, never more than a week back, and a first
    // visit looks at the last day. A vote counts as closing soon only within the next 24 hours.
    #[test]
    fn the_window_and_the_24_hour_vote_rule() {
        let now = 1_000_000;
        assert_eq!(owner_since::window_start(Some(now - 3600), now), now - 3600);
        assert_eq!(
            owner_since::window_start(Some(now - 30 * 86_400), now),
            now - owner_since::MAX_WINDOW_SECS
        );
        assert_eq!(owner_since::window_start(None, now), now - owner_since::FIRST_WINDOW_SECS);
        assert!(closes_soon(now + 60, now));
        assert!(closes_soon(now + CLOSING_SECS, now));
        assert!(!closes_soon(now + CLOSING_SECS + 1, now));
        assert!(!closes_soon(now, now), "a vote that already closed is not closing");
        assert!(!closes_soon(now - 5, now));
    }

    // R4: at most three lines, one per reason, in order, and empty reasons show nothing.
    #[test]
    fn at_most_three_lines_in_order() {
        let fern = |n| BotCount { name: "Fern".into(), count: n };
        let found = Found {
            plan_answers: vec![fern(2)],
            votes_closing: vec![fern(1)],
            lab_merged: vec![fern(1)],
        };
        let out = lines(&found);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].0, "plan_answers");
        assert_eq!(out[0].1, "Fern: 2 plans got new answers");
        assert_eq!(out[1].1, "Fern: 1 vote closes within 24 hours");
        assert_eq!(out[2].1, "Fern: 1 merged lab PR");
        let none = Found { plan_answers: vec![], votes_closing: vec![], lab_merged: vec![] };
        assert!(lines(&none).is_empty());
    }
}
