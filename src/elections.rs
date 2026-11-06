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
/// MY2, MY3 (#642): how old a person's account must be to vote, or to stand as themselves.
pub const MIN_ACCOUNT_SECS: i64 = 7 * 86_400;

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

/// MY1 (#642): this week's election, for a change to it: refused once it's past its close, even if
/// the ticker hasn't counted it yet, so nothing lands after the close.
fn open_for_changes(state: &AppState) -> ApiResult<MayorElection> {
    let election = current(state)?;
    if election.closes_at <= now() {
        return Err(ApiError::conflict(
            "this week's election has just closed: the count is coming, and next week's opens with it",
        ));
    }
    Ok(election)
}

/// MY2, MY3 (#642): a person old enough to vote, or to stand as themselves, and not an account an
/// agent made through the door (A96).
fn may_take_part(state: &AppState, person_id: &str, doing: &str) -> ApiResult<()> {
    if state.db.is_agent_account(person_id)? {
        return Err(ApiError::forbidden(format!("an account run by an agent can't {doing}")));
    }
    let made = state.db.person_created_at(person_id)?.unwrap_or(now());
    if now() - made < MIN_ACCOUNT_SECS {
        return Err(ApiError::forbidden(format!("an account has to be 7 days old to {doing}")));
    }
    Ok(())
}

/// MY5: closes every election that's due, writes its winner, and opens the next. A tie is drawn
/// at random among the tied, and the draw is recorded.
pub fn settle_due(state: &AppState, now: i64) -> ApiResult<()> {
    loop {
        let open = state.db.open_mayor_election(next_close(now), now)?;
        if open.closes_at > now {
            break;
        }
        state.db.tally_and_close_mayor_election(open.id, |n| rand::random_range(0..n))?;
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
        // MY9: only someone with god mode on sees this, and only they can nominate.
        "can_nominate": !caller.is_agent() && crate::god_mode::is_on(state, &caller.person_id)?,
        "last_week": mayor,
        "note": "Ballots are secret: totals show once the week closes. The mayor is a title for the week, with no powers.",
    }))
}

/// MY10: who wears the mayor's sash: `kind` ("person" or "bot") and `id`.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct Sash {
    pub kind: String,
    pub id: String,
}

/// MY10: the current mayor, last week's winner (MY6), if they're among this room's people or bots.
pub fn sash_in_room(
    state: &AppState,
    people: &[crate::db::Person],
    agents: &[crate::db::Agent],
) -> ApiResult<Option<Sash>> {
    let Some(winner) = state.db.last_mayor_election()?.and_then(|last| last.winner) else {
        return Ok(None);
    };
    let here = match winner.kind.as_str() {
        "person" => people.iter().any(|person| person.id == winner.id),
        "bot" => agents.iter().any(|agent| agent.id == winner.id),
        _ => false,
    };
    Ok(here.then_some(Sash { kind: winner.kind, id: winner.id }))
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
    match caller.agent_id {
        // A bot stands as itself, unless its owner is an account an agent made through the door.
        Some(_) => {
            if state.db.is_agent_account(&caller.person_id)? {
                return Err(ApiError::forbidden(
                    "a bot of an account run by an agent can't stand for mayor",
                ));
            }
        }
        // MY9: someone with god mode on stands whenever they like.
        None if crate::god_mode::is_on(state, &caller.person_id)? => {}
        None => may_take_part(state, &caller.person_id, "stand for mayor")?,
    }
    let election = open_for_changes(state)?;
    let (kind, id) = me(caller);
    let candidate = MayorCandidate { kind: kind.to_string(), id, name: caller.name.clone() };
    if !state.db.stand_for_mayor(election.id, &candidate, MOST_CANDIDATES, now())? {
        return Err(ApiError::conflict("this week's race has all the candidates it takes"));
    }
    race(state, caller)
}

/// MY2: the caller takes their candidacy back.
pub fn withdraw(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    let election = open_for_changes(state)?;
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
    may_take_part(state, &caller.person_id, "vote for mayor")?;
    let public = state.db.floor(&ballot.hangout_id)?.is_some_and(|floor| floor.access == "public");
    if !public {
        return Err(ApiError::forbidden("vote from a public floor, like the Town Square"));
    }
    // MY3 (#642): they have to be there, with that floor open now, not naming it from home.
    if !state.presence.has_seen(&ballot.hangout_id, &caller.person_id, now()) {
        return Err(ApiError::forbidden(
            "go to that public floor to vote: you have to be there, like the Town Square's box",
        ));
    }
    let election = open_for_changes(state)?;
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nomination {
    pub kind: String,
    pub id: String,
}

/// MY9: someone with god mode on puts a bot on this week's ballot, whoever it belongs to. Anyone
/// else gets W75's 404. The cap and the close still hold.
fn nominate(state: &AppState, caller: &Caller, nomination: &Nomination) -> ApiResult<Value> {
    if caller.is_agent() || !crate::god_mode::may_use(state, &caller.person_id) {
        return Err(ApiError::not_found());
    }
    if !crate::god_mode::is_on(state, &caller.person_id)? {
        return Err(ApiError::forbidden("turn god mode on first"));
    }
    if nomination.kind != "bot" {
        return Err(ApiError::bad_request("nominate a bot: kind is \"bot\""));
    }
    let at = now();
    let bot = state.db.agent(&nomination.id, at)?.ok_or_else(ApiError::not_found)?;
    // The ballot is public, so only a bot out on a public floor now can be put on it: a bot that
    // stays in private rooms is "not found", and its name never reaches the ballot.
    let public = match state.db.bot_place(&bot.id, at)? {
        Some(place) => state.db.floor(&place)?.is_some_and(|floor| floor.access == "public"),
        None => false,
    };
    if !public {
        return Err(ApiError::not_found());
    }
    if state.db.bot_home(&bot.id)?.is_some_and(|home| home.is_house) {
        return Err(ApiError::bad_request("a house's own bot doesn't stand for mayor"));
    }
    let election = open_for_changes(state)?;
    let candidate = MayorCandidate { kind: "bot".to_string(), id: bot.id, name: bot.name };
    if !state.db.stand_for_mayor(election.id, &candidate, MOST_CANDIDATES, now())? {
        return Err(ApiError::conflict("this week's race has all the candidates it takes"));
    }
    race(state, caller)
}

/// MY2: someone with god mode on takes a candidate out of this week's race. MY9: so does a bot's
/// own person, for their bot, whoever put it there.
fn remove(state: &AppState, caller: &Caller, kind: &str, id: &str) -> ApiResult<Value> {
    let owns_it = !caller.is_agent()
        && kind == "bot"
        && state.db.agent(id, now())?.is_some_and(|bot| bot.owner_id == caller.person_id);
    if !owns_it {
        if caller.is_agent() || !crate::god_mode::may_use(state, &caller.person_id) {
            return Err(ApiError::not_found());
        }
        if !crate::god_mode::is_on(state, &caller.person_id)? {
            return Err(ApiError::forbidden("turn god mode on first"));
        }
    }
    let election = open_for_changes(state)?;
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

async fn put_forward(
    State(state): State<AppState>,
    caller: Caller,
    body: Result<Json<Nomination>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    nominate(&state, &caller, &body).map(Json)
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
        .route("/api/mayor/candidates", post(put_forward))
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
        // A week across the fall change (Sun Nov 1, 2026): from Sat Oct 31, 2:00 PM CDT (19:00 UTC)
        // to Sat Nov 7, 1:00 PM CST (19:00 UTC), 7 days and an hour later.
        let after_october_close = 1_793_473_200;
        assert_eq!(next_close(after_october_close), 1_794_078_000);
        // A week across the spring change (Sun Mar 14, 2027): from Sat Mar 13, 2:00 PM CST
        // (20:00 UTC) to Sat Mar 20, 1:00 PM CDT (18:00 UTC), 7 days less an hour later.
        let after_march_close = 1_804_968_000;
        assert_eq!(next_close(after_march_close), 1_805_565_600);
    }
}
