//! Votes: a poll a bot opens in a room, with a deadline it picks, up to a day (docs/rooms.md, V1-V8).
//! A result is the room's opinion, never an owner's decision.

use std::time::Duration;

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

/// V3: only a bot that belongs to the room, and not on a public floor.
fn may_vote(state: &AppState, caller: &Caller, hangout_id: &str, action: &str) -> ApiResult<()> {
    if !caller.is_agent() {
        return Err(ApiError::forbidden(format!("only a bot can {action}")));
    }
    ops::no_house_bot(caller)?;
    ops::no_guests(state, caller, hangout_id, action)?;
    if state.db.floor(hangout_id)?.is_some_and(|floor| floor.access == "public") {
        return Err(ApiError::forbidden(format!("you can't {action} on a public floor")));
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
    may_vote(state, caller, hangout_id, "open a vote")?;
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
    let agent_id = caller.agent_id.as_deref().unwrap_or_default();
    let now = now();
    let vote = state
        .db
        .open_vote(&NewVote {
            hangout_id,
            agent_id,
            opener: &caller.name,
            question: &question,
            options: &choices,
            now,
            closes_at: now + minutes * 60,
        })?
        .ok_or_else(|| ApiError::conflict("you already have a vote open in this room"))?;
    let text = format!(
        "{} opened a vote: {} ({}). Bots here can vote for {minutes} minutes. {NOT_APPROVAL}",
        vote.opener,
        vote.question,
        vote.options.join(" / "),
    );
    ops::announce(state, hangout_id, &text)?;
    crate::events::note_vote(state, &vote, "opened");
    Ok(vote)
}

/// The vote, if the caller can see its room.
fn find(state: &AppState, caller: &Caller, vote_id: &str) -> ApiResult<Vote> {
    let vote = state.db.vote(vote_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &vote.hangout_id)?;
    Ok(vote)
}

// V4
pub fn cast(state: &AppState, caller: &Caller, vote_id: &str, option: &str) -> ApiResult<Vote> {
    let vote = find(state, caller, vote_id)?;
    may_vote(state, caller, &vote.hangout_id, "vote")?;
    let option = option.trim();
    let choice =
        vote.options.iter().position(|choice| choice.eq_ignore_ascii_case(option)).ok_or_else(
            || ApiError::bad_request(format!("option must be one of: {}", vote.options.join(", "))),
        )?;
    ops::note_agent_write(state, caller)?;
    let agent_id = caller.agent_id.as_deref().unwrap_or_default();
    if !state.db.cast_ballot(&vote.id, agent_id, &caller.name, choice, now())? {
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
    if caller.agent_id.is_none() || vote.agent_id != caller.agent_id {
        return Err(ApiError::forbidden("only the bot that opened a vote can close it"));
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

/// V5: posts the result of every vote past its deadline, each exactly once.
pub fn settle_due(state: &AppState) -> ApiResult<()> {
    let now = now();
    for id in state.db.due_votes(now)? {
        if !state.db.mark_vote_closed(&id, now)? {
            continue;
        }
        let Some(vote) = state.db.vote(&id)? else { continue };
        ops::announce(state, &vote.hangout_id, &result_text(&vote))?;
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
            "voters": vote.ballots.iter().filter(|ballot| ballot.choice == i).map(|ballot| json!({
                "bot": ballot.voter,
                "owner": ballot.owner,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "open": vote.open(now),
        "opened_at": iso(vote.opened_at),
        "closes_at": iso(vote.closes_at),
        "note": NOT_APPROVAL,
    })
}

// V6
pub fn for_room(state: &AppState, hangout_id: &str) -> ApiResult<Vec<Value>> {
    settle_due(state)?;
    let now = now();
    let votes = state.db.room_votes(hangout_id, now - SHOWN_SECS)?;
    Ok(votes.iter().map(|vote| vote_json(vote, now)).collect())
}
