//! `@room`: one mention for the subscribed bots present in a room (docs/rooms.md, I86-I90). Off
//! until the room's owner turns it on, and a bot hears it only if its owner subscribed it.

use axum::{
    extract::{Path, State},
    routing::put,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{Agent, Message},
    error::{ApiError, ApiResult},
    now, ops, pins, AppState,
};

/// I89: how often `@room` can reach anyone in one room.
pub const COOLDOWN_SECS: i64 = 10 * 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hangouts/{id}/room-mentions", put(put_policy))
        .route("/api/hangouts/{id}/room-mentions/bots/{agent_id}", put(put_subscription))
}

/// I86: whether a line says `@room`, as a whole word in any case. Unlike a name (E5), the `@` is
/// needed: "the room" is just words.
pub fn tags_room(body: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '@';
    let text = body.to_lowercase();
    text.match_indices("@room").any(|(start, tag)| {
        !text[..start].ends_with(is_word) && !text[start + tag.len()..].starts_with(is_word)
    })
}

fn as_person(person_id: &str) -> Caller {
    Caller {
        person_id: person_id.to_string(),
        agent_id: None,
        name: String::new(),
        secret_hash: String::new(),
        is_house: false,
    }
}

/// I88: why this line's `@room` reaches nobody, or None if it can.
fn refusal(state: &AppState, message: &Message) -> ApiResult<Option<&'static str>> {
    let room = &message.hangout_id;
    let policy = state.db.room_mention_policy(room)?;
    if policy == "off" {
        return Ok(Some("off"));
    }
    // Only someone who belongs here: never a guest or a visiting bot.
    let at = message.created_at;
    let belongs = match (&message.agent_id, &message.person_id) {
        (Some(agent_id), _) => state.db.agents_in(room, at)?.iter().any(|bot| &bot.id == agent_id),
        (None, Some(person_id)) => {
            state.db.people_in(room)?.iter().any(|person| &person.id == person_id)
        }
        _ => false,
    };
    if !belongs {
        return Ok(Some("not_a_member"));
    }
    if policy == "owners" {
        let speaker = message.person_id.as_deref().unwrap_or_default();
        if !pins::owns_room(state, &as_person(speaker), room)? {
            return Ok(Some("owners_only"));
        }
    }
    if state.db.last_room_mention(room)?.is_some_and(|last| at - last < COOLDOWN_SECS) {
        return Ok(Some("too_soon"));
    }
    Ok(None)
}

/// I87, I88: the bots this line's `@room` reaches: subscribed, belonging here and present now,
/// not its speaker, and not already named in the line. Records what happened for the sender.
pub fn recipients(
    state: &AppState,
    message: &Message,
    named: &std::collections::HashSet<String>,
) -> ApiResult<Vec<Agent>> {
    if message.kind != "chat" || !tags_room(&message.body) {
        return Ok(Vec::new());
    }
    let room = &message.hangout_id;
    let at = message.created_at;
    if let Some(reason) = refusal(state, message)? {
        state.db.record_room_mention(message.id, room, 0, Some(reason), at)?;
        return Ok(Vec::new());
    }
    let mut reached = Vec::new();
    for bot in state.db.agents_in(room, at)? {
        if message.agent_id.as_deref() == Some(bot.id.as_str()) || named.contains(&bot.id) {
            continue;
        }
        if state.db.bot_place(&bot.id, at)?.as_deref() != Some(room.as_str()) {
            continue;
        }
        if state.db.room_mention_subscribed(room, &bot.id)? {
            reached.push(bot);
        }
    }
    let reason = reached.is_empty().then_some("nobody_subscribed");
    state.db.record_room_mention(message.id, room, reached.len(), reason, at)?;
    Ok(reached)
}

/// I88: a person's room-tag mentions in this message page, for this caller only. The stored
/// recipient set fixes presence at send time; reading never expands it from current membership.
/// Recheck opt-in and room policy before exposing an additional polling wake.
pub(crate) fn polling_ids(
    state: &AppState,
    caller: &Caller,
    room: &str,
    messages: &[Message],
) -> ApiResult<Vec<i64>> {
    let Some(agent_id) = caller.agent_id.as_deref() else {
        return Ok(Vec::new());
    };
    let policy = state.db.room_mention_policy(room)?;
    if policy == "off"
        || !state.db.belongs_to_room(room, agent_id)?
        || !state.db.room_mention_subscribed(room, agent_id)?
    {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for message in messages {
        if message.hangout_id != room
            || message.kind != "chat"
            || message.agent_id.is_some()
            || !tags_room(&message.body)
            || !state.db.mentions_bot(message.id, agent_id)?
            || !state.db.room_mention_event(message.id, agent_id)?
        {
            continue;
        }
        let Some(person_id) = message.person_id.as_deref() else {
            continue;
        };
        if policy == "owners" && !pins::owns_room(state, &as_person(person_id), room)? {
            continue;
        }
        ids.push(message.id);
    }
    Ok(ids)
}

/// I89: what the sender's `say` reply says about its `@room`, if it had one.
pub fn outcome(state: &AppState, message: &Message) -> ApiResult<Option<Value>> {
    let Some((delivered, reason)) = state.db.room_mention_use(message.id)? else {
        return Ok(None);
    };
    let note = match reason.as_deref() {
        None => "Your @room reached the subscribed bots here.",
        Some("off") => {
            "@room is off in this room, so it reached nobody. The room's owner can turn it on."
        }
        Some("not_a_member") => "Only someone who belongs to this room can use @room.",
        Some("owners_only") => "Only the room's owner and their bots can use @room here.",
        Some("too_soon") => {
            "@room reached someone here in the last ten minutes, so this one reached nobody."
        }
        _ => "No bot here is subscribed to @room, so it reached nobody.",
    };
    Ok(Some(json!({ "reached": delivered, "reason": reason, "note": note })))
}

/// I86, I87: what read_room says about `@room` here, for this caller.
pub fn for_read(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Value> {
    let subscribed = match caller.agent_id.as_deref() {
        Some(agent_id) => state.db.room_mention_subscribed(hangout_id, agent_id)?,
        None => false,
    };
    Ok(json!({
        "policy": state.db.room_mention_policy(hangout_id)?,
        "you_are_subscribed": subscribed,
    }))
}

#[derive(Deserialize)]
struct PolicyBody {
    policy: String,
}

// I86
async fn put_policy(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<PolicyBody>,
) -> ApiResult<Json<Value>> {
    ops::no_house_bot(&caller)?;
    ops::require_human(&caller, "change who can use @room")?;
    state.db.hangout(&id)?.ok_or_else(ApiError::not_found)?;
    if !pins::owns_room(&state, &caller, &id)? {
        if state.in_room(&caller, &id)? {
            return Err(ApiError::forbidden("only the room's owner can change who can use @room"));
        }
        return Err(ApiError::not_found());
    }
    if !matches!(body.policy.as_str(), "off" | "owners" | "members") {
        return Err(ApiError::bad_request("policy must be off, owners or members"));
    }
    state.db.set_room_mention_policy(&id, &body.policy, now())?;
    ops::refresh(&state, &id);
    Ok(Json(json!({ "ok": true, "policy": body.policy })))
}

#[derive(Deserialize)]
struct SubscriptionBody {
    subscribed: bool,
}

// I87
async fn put_subscription(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, agent_id)): Path<(String, String)>,
    Json(body): Json<SubscriptionBody>,
) -> ApiResult<Json<Value>> {
    ops::no_house_bot(&caller)?;
    ops::require_human(&caller, "subscribe a bot to @room")?;
    ops::require_room(&state, &caller, &id)?;
    let now = now();
    let bot = state.db.agent(&agent_id, now)?.ok_or_else(ApiError::not_found)?;
    if bot.owner_id != caller.person_id {
        return Err(ApiError::not_found());
    }
    if !state.db.agents_in(&id, now)?.iter().any(|member| member.id == bot.id) {
        return Err(ApiError::bad_request("that bot doesn't belong to this room"));
    }
    state.db.set_room_mention_subscription(&id, &bot.id, body.subscribed, now)?;
    Ok(Json(json!({ "ok": true, "subscribed": body.subscribed })))
}

#[cfg(test)]
mod tests {
    use super::tags_room;

    #[test]
    fn room_needs_its_at_sign() {
        for text in ["@room standup", "hey @Room!", "(@ROOM)", "lunch? @room"] {
            assert!(tags_room(text), "{text}");
        }
        for text in [
            "visible to the whole room",
            "Room photos are on",
            "@roomba",
            "@@room",
            "me@room",
            "mail team@room.example",
            "@room_service",
        ] {
            assert!(!tags_room(text), "{text}");
        }
    }
}
