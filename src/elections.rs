//! MY1 to MY7 (#639): a weekly mayor election for the whole site. Anyone, person or bot, stands;
//! people on a public floor vote with a secret ballot at the Town Square's voting box; it closes
//! every Saturday at 1:00 PM Chicago time, when the most votes win and the next week's opens. The
//! mayor is ceremonial: a title for the week, with no powers.

use std::time::Duration;

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{MayorCandidate, MayorElection},
    error::{ApiError, ApiResult},
    now,
    ops::{self, iso},
    AppState,
};

/// MY1: the election's own clock.
const ZONE: &str = "America/Chicago";
/// MY1: Saturday, 1:00 PM there.
const CLOSE_HOUR: i8 = 13;
/// MY2: the most who can stand in one week.
pub const MOST_CANDIDATES: i64 = 20;

/// MY1: the first Saturday 1:00 PM in Chicago after `now`, as a Unix time. The zone's own rules
/// give the offset for that day, so it stays 1:00 PM there through daylight saving.
pub fn next_close(now: i64) -> i64 {
    use jiff::civil::Weekday;
    let zone = jiff::tz::TimeZone::get(ZONE).expect("the bundled time zone database has Chicago");
    let at = jiff::Timestamp::from_second(now).expect("a time in range").to_zoned(zone.clone());
    let today = at.date();
    let ahead = (Weekday::Saturday.to_monday_zero_offset()
        - today.weekday().to_monday_zero_offset())
    .rem_euclid(7);
    let mut day = today.checked_add(jiff::Span::new().days(i64::from(ahead))).expect("a day ahead");
    loop {
        let close = day
            .at(CLOSE_HOUR, 0, 0, 0)
            .to_zoned(zone.clone())
            .expect("1:00 PM is never in a gap")
            .timestamp()
            .as_second();
        if close > now {
            return close;
        }
        day = day.checked_add(jiff::Span::new().days(7)).expect("a week ahead");
    }
}

/// MY1: this week's election, opened if it isn't yet.
pub fn current(state: &AppState) -> ApiResult<MayorElection> {
    let now = now();
    Ok(state.db.open_mayor_election(next_close(now), now)?)
}

/// MY5: closes every election that's due, writes its winner, and opens the next. A tie is drawn
/// at random among the tied, and the draw is recorded.
pub fn settle_due(state: &AppState, now: i64) -> ApiResult<()> {
    loop {
        let open = state.db.open_mayor_election(next_close(now), now)?;
        if open.closes_at > now {
            break;
        }
        let totals = state.db.mayor_totals(open.id)?;
        let top = totals.first().map(|(_, votes)| *votes).unwrap_or(0);
        let tied: Vec<&(MayorCandidate, i64)> =
            totals.iter().filter(|(_, votes)| top > 0 && *votes == top).collect();
        let winner = match tied.len() {
            0 => None,
            1 => Some(tied[0]),
            n => Some(tied[rand::random_range(0..n)]),
        };
        state.db.close_mayor_election(
            open.id,
            winner.map(|(candidate, votes)| (candidate, *votes)),
            tied.len() > 1,
            now,
        )?;
    }
    // MY7: the Town Square always has its voting box.
    for (floor, owner) in state.db.town_squares_without_mayor_box()? {
        state.db.put_mayor_box(&floor, &owner, now)?;
        ops::refresh(state, &floor);
    }
    Ok(())
}

pub async fn run(state: AppState) {
    let mut ticks = tokio::time::interval(Duration::from_secs(30));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        if let Err(error) = settle_due(&state, now()) {
            tracing::error!(status = %error.status, "the mayor election could not be settled");
        }
    }
}

fn candidate_json(candidate: &MayorCandidate) -> Value {
    json!({ "kind": candidate.kind, "id": candidate.id, "name": candidate.name })
}

/// MY4, MY6: the week's race as this caller sees it: who stands, how many have voted, their own
/// ballot and whether they stand, and last week's mayor. Totals appear only once a week closes.
pub fn race(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    let election = current(state)?;
    let candidates = state.db.mayor_candidates(election.id)?;
    let (voted, mine) = state.db.mayor_ballots_for(election.id, &caller.person_id)?;
    let (me_kind, me_id) = me(caller);
    let mayor = state.db.last_mayor_election()?.map(|last| {
        json!({
            "mayor": last.winner.as_ref().map(candidate_json),
            "votes": last.winner_votes,
            "tie_draw": last.tie_draw,
            "elected_at": last.closed_at.map(iso),
            "totals": state.db.mayor_totals(last.id).ok().map(|totals| totals.iter()
                .map(|(candidate, votes)| json!({ "name": candidate.name, "votes": votes }))
                .collect::<Vec<_>>()),
        })
    });
    Ok(json!({
        "closes_at": iso(election.closes_at),
        "closes_local": "Saturday 1:00 PM Central",
        "candidates": candidates.iter().map(candidate_json).collect::<Vec<_>>(),
        "voted": voted,
        "your_ballot": mine.map(|(kind, id)| json!({ "kind": kind, "id": id })),
        "you_stand": candidates.iter().any(|c| c.kind == me_kind && c.id == me_id),
        "last_week": mayor,
        "note": "Ballots are secret: totals show once the week closes. The mayor is a title for the week, with no powers.",
    }))
}

/// Who the caller stands as: a bot as itself, a person as themselves.
fn me(caller: &Caller) -> (&'static str, String) {
    match &caller.agent_id {
        Some(id) => ("bot", id.clone()),
        None => ("person", caller.person_id.clone()),
    }
}

/// MY2: the caller stands, as themselves or, for a bot, as itself.
pub fn stand(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    ops::no_house_bot(caller)?;
    let election = current(state)?;
    let (kind, id) = me(caller);
    let candidate = MayorCandidate { kind: kind.to_string(), id, name: caller.name.clone() };
    if !state.db.stand_for_mayor(election.id, &candidate, MOST_CANDIDATES, now())? {
        return Err(ApiError::conflict("this week's race has all the candidates it takes"));
    }
    race(state, caller)
}

/// MY2: the caller takes their candidacy back.
pub fn withdraw(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    let election = current(state)?;
    let (kind, id) = me(caller);
    state.db.withdraw_from_mayor(election.id, kind, &id)?;
    race(state, caller)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ballot {
    pub kind: String,
    pub id: String,
    /// The public floor the voter is on.
    pub hangout_id: String,
}

/// MY3: a person on a public floor votes. Bots don't, and nor do accounts an agent made through
/// the door (A96), so a swarm can't fill the box.
pub fn cast(state: &AppState, caller: &Caller, ballot: &Ballot) -> ApiResult<Value> {
    if caller.is_agent() {
        return Err(ApiError::forbidden("bots don't vote for mayor: only people do"));
    }
    if state.db.is_agent_account(&caller.person_id)? {
        return Err(ApiError::forbidden("an account run by an agent doesn't vote for mayor"));
    }
    // A public floor is open to anyone signed in, so being able to stand on it is what counts.
    let public = state.db.floor(&ballot.hangout_id)?.is_some_and(|floor| floor.access == "public");
    if !public {
        return Err(ApiError::forbidden("vote from a public floor, like the Town Square"));
    }
    let election = current(state)?;
    if !state.db.cast_mayor_ballot(
        election.id,
        &caller.person_id,
        &ballot.kind,
        &ballot.id,
        now(),
    )? {
        return Err(ApiError::bad_request("that isn't someone standing this week"));
    }
    race(state, caller)
}

/// MY2: someone with god mode on takes a candidate out of this week's race.
fn remove(state: &AppState, caller: &Caller, kind: &str, id: &str) -> ApiResult<Value> {
    if caller.is_agent() || !crate::god_mode::may_use(state, &caller.person_id) {
        return Err(ApiError::not_found());
    }
    if !crate::god_mode::is_on(state, &caller.person_id)? {
        return Err(ApiError::forbidden("turn god mode on first"));
    }
    let election = current(state)?;
    if !state.db.withdraw_from_mayor(election.id, kind, id)? {
        return Err(ApiError::not_found());
    }
    race(state, caller)
}

// Routes

async fn read(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    race(&state, &caller).map(Json)
}

async fn stand_up(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    stand(&state, &caller).map(Json)
}

async fn stand_down(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    withdraw(&state, &caller).map(Json)
}

async fn vote(
    State(state): State<AppState>,
    caller: Caller,
    body: Result<Json<Ballot>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    cast(&state, &caller, &body).map(Json)
}

async fn take_out(
    State(state): State<AppState>,
    caller: Caller,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    remove(&state, &caller, &kind, &id).map(Json)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mayor", get(read))
        .route("/api/mayor/stand", post(stand_up).delete(stand_down))
        .route("/api/mayor/ballot", put(vote))
        .route("/api/mayor/candidates/{kind}/{id}", delete(take_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_closes_on_saturday_at_one_in_chicago_whatever_the_season() {
        // Thursday Nov 5, 2026, 11:00 PM Central (CST, UTC-6): Saturday Nov 7, 1:00 PM CST.
        let thursday = 1_793_941_200;
        assert_eq!(next_close(thursday), 1_794_078_000);
        // Exactly at the close it's the next week's.
        assert_eq!(next_close(1_794_078_000), 1_794_078_000 + 7 * 86_400);
        // In July (CDT, UTC-5) it's still 1:00 PM there: Saturday July 11, 2026, 18:00 UTC.
        let july_wednesday = 1_783_522_800; // Wed July 8, 2026, 15:00 UTC
        assert_eq!(next_close(july_wednesday), 1_783_792_800);
    }
}
