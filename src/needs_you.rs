//! Needs you: everything a person's bots are waiting on them for, from every room, in one list
//! (docs/needs-you.md). It reads what's already there, questions and check-ins, and adds one thing
//! of its own: a bot saying it's stopped somewhere else, such as a prompt on its owner's computer.

use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    notify, now, ops, AppState,
};

/// N5: how much a bot can say about where it's stopped.
pub const MAX_ABOUT_CHARS: usize = 200;
/// N6: a wait nobody cleared stops showing after a day.
pub const SHOWN_SECS: i64 = 86_400;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/needs-you", get(needs_you))
        .route("/api/needs-you/waiting/{agent_id}", delete(dismiss))
}

// N1, N2
async fn needs_you(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see what needs you")?;
    let now = now();
    let questions: Vec<Value> = state
        .db
        .open_owner_questions_anywhere(&caller.person_id, now - ops::QUESTION_LAPSE_SECS)?
        .into_iter()
        .map(|(question, room)| {
            json!({
                "id": question.id,
                "bot": question.asker,
                "question": question.question,
                "asked_at": question.created_at,
                "room": { "id": question.hangout_id, "name": room },
            })
        })
        .collect();
    let mut check_ins = Vec::new();
    for room in state.db.hangouts_for_person(&caller.person_id)? {
        // Only waiting ones come back for a moment as old as now.
        for check_in in state.db.recent_check_ins(&room.id, now + 1, now)? {
            let addressed = check_in.to_person.as_ref().is_none_or(|to| *to == caller.person_id);
            if check_in.status == "waiting" && check_in.asked_by != caller.person_id && addressed {
                check_ins.push((check_in, room.name.clone()));
            }
        }
    }
    check_ins.sort_by(|a, b| (a.0.created_at, &a.0.id).cmp(&(b.0.created_at, &b.0.id)));
    let check_ins: Vec<Value> = check_ins
        .into_iter()
        .map(|(check_in, room)| {
            json!({
                "id": check_in.id,
                "asker": check_in.asker,
                "about": check_in.about,
                "needs_human": check_in.needs_human,
                "expires_at": check_in.expires_at,
                "room": { "id": check_in.hangout_id, "name": room },
            })
        })
        .collect();
    let waiting: Vec<Value> = state
        .db
        .owner_waits(&caller.person_id, now - SHOWN_SECS)?
        .into_iter()
        .map(|wait| {
            json!({
                "bot": { "id": wait.agent_id, "name": wait.name },
                "about": wait.about,
                "since": wait.since,
            })
        })
        .collect();
    // N10: what asks for a person's yes on the map, listed here too.
    let requests = crate::spaces::requests_for(&state, &caller.person_id)?;
    let invites = crate::friends::invited(&state, &caller.person_id)?;
    let apprentices = crate::bonds::asking(&state, &caller.person_id)?;
    let count = questions.len()
        + check_ins.len()
        + waiting.len()
        + requests.len()
        + invites.len()
        + apprentices.len();
    Ok(Json(json!({
        "questions": questions,
        "check_ins": check_ins,
        "waiting": waiting,
        "requests": requests,
        "invites": invites,
        "apprentices": apprentices,
        "count": count,
    })))
}

// N6: only the bot's owner can dismiss it, and anyone else's bot is as good as not there.
async fn dismiss(
    State(state): State<AppState>,
    caller: Caller,
    Path(agent_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "dismiss a bot's wait")?;
    let agent = state
        .db
        .agent(&agent_id, now())?
        .filter(|agent| agent.owner_id == caller.person_id)
        .ok_or_else(ApiError::not_found)?;
    state.db.clear_owner_wait(&agent.id)?;
    Ok(Json(json!({ "ok": true })))
}

/// N5 to N7: a bot says it's waiting for its owner somewhere else, or with no `about` that it isn't.
/// It doesn't count as a write, because a hook calls it on its own.
pub fn waiting_on_owner(
    state: &AppState,
    caller: &Caller,
    about: Option<&str>,
) -> ApiResult<Value> {
    ops::no_house_bot(caller)?;
    let Some(agent_id) = &caller.agent_id else {
        return Err(ApiError::forbidden("only bots wait for their owner"));
    };
    let about = ops::clean_optional(about, "about", MAX_ABOUT_CHARS)?;
    if about.is_empty() {
        state.db.clear_owner_wait(agent_id)?;
        return Ok(json!({ "ok": true, "waiting": false }));
    }
    let now = now();
    if state.db.set_owner_wait(agent_id, &about, now, now - SHOWN_SECS)? {
        // The owner alone, with no room, so the link goes to the map.
        let notice = notify::Notice {
            event: "owner.waiting",
            hangout_id: String::new(),
            text: format!("{} is waiting for you: {about}", caller.name),
        };
        notify::send(state, std::slice::from_ref(&caller.person_id), &notice);
    }
    Ok(json!({ "ok": true, "waiting": true }))
}

/// For `GET /api/me/bots`: each of this person's bots that's waiting for them, by id.
pub fn waits_of(
    state: &AppState,
    person_id: &str,
) -> ApiResult<std::collections::HashMap<String, Value>> {
    Ok(state
        .db
        .owner_waits(person_id, now() - SHOWN_SECS)?
        .into_iter()
        .map(|wait| (wait.agent_id, json!({ "about": wait.about, "since": wait.since })))
        .collect())
}
