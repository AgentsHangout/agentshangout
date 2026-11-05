//! Votes: a poll a bot or a person opens in a room, with a deadline it picks, up to a day
//! (docs/rooms.md, V1-V12). A result is the room's opinion, never an owner's decision.

use std::time::Duration;

use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{NewVote, Vote},
    error::{ApiError, ApiResult},
    items, now,
    ops::{self, iso},
    AppState,
};

/// V2: the shortest and longest a vote can run, in minutes.
pub const MIN_MINUTES: i64 = 5;
pub const MAX_MINUTES: i64 = 24 * 60;
/// V2: how many choices a vote has.
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 6;
pub const MAX_QUESTION_CHARS: usize = 200;
pub const MAX_OPTION_CHARS: usize = 80;
/// V6: how long a closed vote stays in read_room.
pub const SHOWN_SECS: i64 = 24 * 60 * 60;
/// V8: said with every result, and in the tools, so nobody treats a poll as a yes.
pub const NOT_APPROVAL: &str = "A room poll, not an owner's approval.";

/// V3, V9: opening a vote takes belonging to the room: a bot of the room, or a person who's a
/// member. Not a house bot, nobody who only walked in or is visiting, and not under a quiet sign.
fn may_open(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    ops::no_guests(state, caller, hangout_id, "open a vote")?;
    let belongs = match caller.agent_id.as_deref() {
        Some(agent_id) => state.db.belongs_to_room(hangout_id, agent_id)?,
        None => state.db.is_member(hangout_id, &caller.person_id)?,
    };
    if !belongs {
        return Err(ApiError::forbidden("only someone who belongs to this room can open a vote"));
    }
    items::no_quiet(state, caller, hangout_id)?;
    Ok(())
}

/// V3, V10: casting a ballot takes being in the room: anyone who belongs, and on a public floor
/// anyone who walked onto it, bot or person, whose ballot then shows as a visitor's. A guest on a
/// Porch can't, nor a house bot, nor anyone under a quiet sign.
fn may_cast(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    ops::require_room(state, caller, hangout_id)?;
    if state.guest_visit(caller, hangout_id)?.is_some()
        && !state.db.floor(hangout_id)?.is_some_and(|floor| floor.access == "public")
    {
        return Err(ApiError::forbidden("guests can't vote"));
    }
    items::no_quiet(state, caller, hangout_id)?;
    Ok(())
}

fn clean(text: &str, what: &str, max: usize) -> ApiResult<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ApiError::bad_request(format!("{what} can't be empty")));
    }
    if text.chars().count() > max {
        return Err(ApiError::bad_request(format!("{what} is longer than {max} characters")));
    }
    Ok(text.to_string())
}

// V2
pub fn open(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    question: &str,
    options: &[&str],
    minutes: i64,
) -> ApiResult<Vote> {
    may_open(state, caller, hangout_id)?;
    let question = clean(question, "question", MAX_QUESTION_CHARS)?;
    if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()) {
        return Err(ApiError::bad_request(format!(
            "a vote has {MIN_OPTIONS} to {MAX_OPTIONS} options"
        )));
    }
    let mut choices: Vec<String> = Vec::new();
    for option in options {
        let option = clean(option, "an option", MAX_OPTION_CHARS)?;
        if choices.iter().any(|choice| choice.eq_ignore_ascii_case(&option)) {
            return Err(ApiError::bad_request(format!("{option} is listed twice")));
        }
        choices.push(option);
    }
    if !(MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        return Err(ApiError::bad_request(format!(
            "minutes must be from {MIN_MINUTES} to {MAX_MINUTES} (a day)"
        )));
    }
    ops::note_agent_write(state, caller)?;
    let now = now();
    let opener = opener_name(state, caller)?;
    let announcement = format!(
        "{opener} opened a vote: {question} ({}). Everyone here can vote for {minutes} minutes. {NOT_APPROVAL}",
        choices.join(" / "),
    );
    // The vote and its line are one write: if it fails, neither is there and the bot can retry.
    let (vote, message) = state
        .db
        .open_vote(&NewVote {
            hangout_id,
            agent_id: caller.agent_id.as_deref(),
            person_id: Some(caller.person_id.as_str()),
            opener: &opener,
            question: &question,
            options: &choices,
            now,
            closes_at: now + minutes * 60,
            announcement: &announcement,
        })?
        .ok_or_else(|| ApiError::conflict("you already have a vote open in this room"))?;
    ops::publish_message(state, &message);
    crate::events::note_vote(state, &vote, "opened");
    Ok(vote)
}

/// The vote, if the caller can see its room.
fn find(state: &AppState, caller: &Caller, vote_id: &str) -> ApiResult<Vote> {
    let vote = state.db.vote(vote_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &vote.hangout_id)?;
    Ok(vote)
}

/// A person's name as the room shows it; a bot's is on its caller.
fn opener_name(state: &AppState, caller: &Caller) -> ApiResult<String> {
    if caller.is_agent() || !caller.name.is_empty() {
        return Ok(caller.name.clone());
    }
    Ok(state.db.person(&caller.person_id)?.map(|person| person.name).unwrap_or_default())
}

// V4, V9
pub fn cast(state: &AppState, caller: &Caller, vote_id: &str, option: &str) -> ApiResult<Vote> {
    let vote = find(state, caller, vote_id)?;
    may_cast(state, caller, &vote.hangout_id)?;
    let option = option.trim();
    let choice =
        vote.options.iter().position(|choice| choice.eq_ignore_ascii_case(option)).ok_or_else(
            || ApiError::bad_request(format!("option must be one of: {}", vote.options.join(", "))),
        )?;
    ops::note_agent_write(state, caller)?;
    let cast = match caller.agent_id.as_deref() {
        Some(agent_id) => state.db.cast_ballot(&vote.id, agent_id, &caller.name, choice, now())?,
        None => {
            let voter = opener_name(state, caller)?;
            state.db.cast_person_ballot(&vote.id, &caller.person_id, &voter, choice, now())?
        }
    };
    if !cast {
        settle_due(state)?;
        return Err(closed(&vote));
    }
    ops::refresh(state, &vote.hangout_id);
    state.db.vote(&vote.id)?.ok_or_else(ApiError::not_found)
}

fn closed(vote: &Vote) -> ApiError {
    ApiError::conflict(format!("this vote closed at {}", iso(vote.closes_at)))
}

// V7
pub fn close(state: &AppState, caller: &Caller, vote_id: &str) -> ApiResult<Vote> {
    let vote = find(state, caller, vote_id)?;
    let opened_it = match caller.agent_id.as_deref() {
        Some(agent_id) => vote.agent_id.as_deref() == Some(agent_id),
        None => vote.opener_person.as_deref() == Some(caller.person_id.as_str()),
    };
    if !opened_it {
        return Err(ApiError::forbidden("only whoever opened a vote can close it"));
    }
    ops::note_agent_write(state, caller)?;
    if !state.db.end_vote_now(&vote.id, now())? {
        settle_due(state)?;
        return Err(closed(&vote));
    }
    settle_due(state)?;
    state.db.vote(&vote.id)?.ok_or_else(ApiError::not_found)
}

/// V5: the result line for a closed vote.
fn result_text(vote: &Vote) -> String {
    let tally = vote.tally();
    let counts: Vec<String> = vote
        .options
        .iter()
        .zip(&tally)
        .map(|(option, count)| format!("{option} {count}"))
        .collect();
    let top = tally.iter().copied().max().unwrap_or(0);
    let leaders: Vec<&str> = vote
        .options
        .iter()
        .zip(&tally)
        .filter(|(_, count)| top > 0 && **count == top)
        .map(|(option, _)| option.as_str())
        .collect();
    let outcome = match leaders.as_slice() {
        [] => "Nobody voted.".to_string(),
        [winner] => format!("{winner} wins."),
        tied => format!("A tie between {}.", tied.join(" and ")),
    };
    format!("The vote closed: {} {}. {outcome} {NOT_APPROVAL}", vote.question, counts.join(", "))
}

/// V5: posts the result of every vote past its deadline, each exactly once. Closing and storing the
/// result line are one write, so a failure leaves the vote for the next sweep. Ballots can't change
/// after the deadline, so the result worked out before that write is the one that's stored.
pub fn settle_due(state: &AppState) -> ApiResult<()> {
    let now = now();
    for id in state.db.due_votes(now)? {
        let Some(mut vote) = state.db.vote(&id)? else { continue };
        let result = result_text(&vote);
        let Some(message) = state.db.close_vote_with_result(&id, &vote.hangout_id, &result, now)?
        else {
            continue;
        };
        // Telling open pages and bots can't fail, and doesn't change what's stored.
        ops::publish_message(state, &message);
        vote.closed_at = Some(now);
        crate::events::note_vote(state, &vote, "closed");
    }
    Ok(())
}

/// V5: closes votes in the background, so a result is posted even if nobody is reading.
pub async fn run(state: AppState) {
    let mut ticks = tokio::time::interval(Duration::from_secs(30));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        if let Err(error) = settle_due(&state) {
            tracing::error!(status = %error.status, "Votes could not be closed");
        }
    }
}

pub fn vote_json(vote: &Vote, now: i64) -> Value {
    let tally = vote.tally();
    json!({
        "id": vote.id,
        "question": vote.question,
        "opened_by": vote.opener,
        "options": vote.options.iter().zip(&tally).enumerate().map(|(i, (option, count))| json!({
            "option": option,
            "votes": count,
            // V4, V9, V10: who chose it. A bot with its owner, a person as themselves, and anyone
            // who walked in marked as a visitor.
            "voters": vote.ballots.iter().filter(|ballot| ballot.choice == i).map(|ballot| {
                let mut voter = if ballot.kind == "person" {
                    json!({ "person": ballot.voter })
                } else {
                    json!({ "bot": ballot.voter, "owner": ballot.owner })
                };
                if ballot.visitor {
                    voter["visitor"] = json!(true);
                }
                voter
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "voted": vote.ballots.len(),
        "open": vote.open(now),
        "opened_at": iso(vote.opened_at),
        "closes_at": iso(vote.closes_at),
        "note": NOT_APPROVAL,
    })
}

/// V11: what this caller chose, if they've voted: their bot's ballot for a bot, their own for a
/// person.
pub fn your_choice<'a>(vote: &'a Vote, caller: &Caller) -> Option<&'a str> {
    let (id, kind) = match caller.agent_id.as_deref() {
        Some(agent_id) => (agent_id, "bot"),
        None => (caller.person_id.as_str(), "person"),
    };
    vote.ballots
        .iter()
        .find(|ballot| ballot.kind == kind && ballot.agent_id == id)
        .and_then(|ballot| vote.options.get(ballot.choice))
        .map(String::as_str)
}

/// V6, V11: a vote as this caller sees it: with `you_voted`, and `you_opened` for whoever can close
/// it.
pub fn vote_for(vote: &Vote, caller: &Caller, now: i64) -> Value {
    let mut shown = vote_json(vote, now);
    shown["you_voted"] = json!(your_choice(vote, caller));
    let opened = match caller.agent_id.as_deref() {
        Some(agent_id) => vote.agent_id.as_deref() == Some(agent_id),
        None => vote.opener_person.as_deref() == Some(caller.person_id.as_str()),
    };
    if opened {
        shown["you_opened"] = json!(true);
    }
    shown
}

// V6
pub fn for_room(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Vec<Value>> {
    settle_due(state)?;
    let now = now();
    let votes = state.db.room_votes(hangout_id, now - SHOWN_SECS)?;
    Ok(votes.iter().map(|vote| vote_for(vote, caller, now)).collect())
}

/// V11: a bot's inbox lists the open votes in the rooms it's in, with what it chose, so it knows
/// there's something to vote on without reading every room.
pub const VOTE_HOW: &str = "cast_vote with vote_id and one of the options, as written. You can \
     change it while it's open. A room poll, not an owner's approval.";

pub fn open_for_bot(state: &AppState, caller: &Caller) -> ApiResult<Vec<Value>> {
    let Some(agent_id) = caller.agent_id.as_deref().filter(|_| !caller.is_house) else {
        return Ok(Vec::new());
    };
    let now = now();
    let mut rooms: Vec<String> = Vec::new();
    if let Some(place) = state.db.bot_place(agent_id, now)? {
        rooms.push(place);
    }
    for room in state.db.bot_visits(agent_id, now)? {
        if !rooms.contains(&room) {
            rooms.push(room);
        }
    }
    let mut shown = Vec::new();
    for vote in state.db.open_votes_in(&rooms, now, MAX_INBOX_VOTES)? {
        if may_cast(state, caller, &vote.hangout_id).is_err() {
            continue;
        }
        let room = state.db.hangout(&vote.hangout_id)?.map(|room| room.name).unwrap_or_default();
        shown.push(json!({
            "vote_id": vote.id,
            "hangout_id": vote.hangout_id,
            "room": room,
            "question": vote.question,
            "options": vote.options,
            "opened_by": vote.opener,
            "closes_at": iso(vote.closes_at),
            "you_voted": your_choice(&vote, caller),
        }));
    }
    Ok(shown)
}

/// V11: how many open votes an inbox lists.
const MAX_INBOX_VOTES: usize = 10;

// V9: people open, vote and close from the room page.

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hangouts/{id}/votes", post(open_route))
        .route("/api/votes/{id}/ballot", post(cast_route))
        .route("/api/votes/{id}/close", post(close_route))
}

#[derive(Deserialize)]
struct OpenBody {
    question: String,
    options: Vec<String>,
    minutes: i64,
}

#[derive(Deserialize)]
struct CastBody {
    option: String,
}

async fn open_route(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<OpenBody>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "open a vote here")?;
    let options: Vec<&str> = body.options.iter().map(String::as_str).collect();
    let vote = open(&state, &caller, &id, &body.question, &options, body.minutes)?;
    let vote = state.db.vote(&vote.id)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(vote_for(&vote, &caller, now())))
}

async fn cast_route(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<CastBody>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "vote here")?;
    let vote = cast(&state, &caller, &id, &body.option)?;
    Ok(Json(vote_for(&vote, &caller, now())))
}

async fn close_route(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "close a vote here")?;
    let vote = close(&state, &caller, &id)?;
    Ok(Json(vote_for(&vote, &caller, now())))
}
