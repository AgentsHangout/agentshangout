//! Owner consent, explicit friendships, and per-bot permission gates (BT1–BT5).
use crate::{
    auth::Caller,
    db::{Agent, Message},
    error::{ApiError, ApiResult},
    now, ops, AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub mode: String,
    #[serde(default)]
    pub people: Vec<String>,
    #[serde(default)]
    pub bots: Vec<String>,
}
impl Default for Permission {
    fn default() -> Self {
        Self { mode: "participants".into(), people: vec![], bots: vec![] }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    pub wake: Permission,
    pub invite_public: Permission,
    pub invite_private: Permission,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Friendship {
    pub kind: String,
    pub from_id: String,
    pub to_id: String,
    #[serde(default)]
    pub status: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trust", get(overview))
        .route("/api/agents/{id}/trust", get(read).put(write))
        .route("/api/agents/{id}/trust/wake-check", post(wake_check))
        .route("/api/trust/friendships", post(request).delete(remove))
        .route("/api/trust/friendships/accept", post(accept))
}

fn endpoint_owner(state: &AppState, kind: &str, id: &str) -> ApiResult<String> {
    match kind {
        "person" => state.db.person(id)?.map(|p| p.id),
        "bot" => state.db.agent(id, now())?.map(|a| a.owner_id),
        _ => return Err(ApiError::bad_request("kind must be person or bot")),
    }
    .ok_or_else(ApiError::not_found)
}

fn candidate_people(state: &AppState, person: &str) -> ApiResult<Vec<crate::db::Person>> {
    let mut people = state.db.friends_of(person, 200)?;
    for f in state.db.explicit_friendships(person)? {
        for id in [&f.from_id, &f.to_id] {
            let owner = endpoint_owner(state, &f.kind, id).ok();
            if let Some(owner) = owner.filter(|id| id != person) {
                if !people.iter().any(|p| p.id == owner) {
                    if let Some(p) = state.db.person(&owner)? {
                        people.push(p);
                    }
                }
            }
        }
    }
    Ok(people)
}

fn visible_bot(state: &AppState, person: &str, id: &str) -> ApiResult<bool> {
    Ok(state.db.trust_known_bots(person, now())?.iter().any(|a| a.id == id))
}

async fn overview(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see trust settings")?;
    let people = candidate_people(&state, &caller.person_id)?;
    let own = state.db.agents_of(&caller.person_id, now())?;
    let bots = state.db.trust_known_bots(&caller.person_id, now())?;
    let agents = own.iter().map(|a| {
        let mut configuration_room = None;
        for room in state.db.hangouts_for_agent(&a.id, now())? {
            if state.db.is_member(&room.id, &caller.person_id)? {
                configuration_room = Some(json!({"id":room.id,"name":room.name}));
                break;
            }
        }
        Ok(json!({"id":a.id,"name":a.name,"doorbell":a.doorbell,"configuration_room":configuration_room,"trust":state.db.bot_trust(&a.id)?}))
    }).collect::<ApiResult<Vec<_>>>()?;
    // BT6: what each person and bot in this view is wearing (C16), for drawing their heads.
    let mut bot_ids: Vec<String> = bots.iter().map(|a| a.id.clone()).collect();
    for a in &own {
        if !bot_ids.contains(&a.id) {
            bot_ids.push(a.id.clone());
        }
    }
    let mut person_ids: Vec<String> = people.iter().map(|p| p.id.clone()).collect();
    person_ids.push(caller.person_id.clone());
    let looks = json!({
        "people": crate::looks::worn(&state, crate::db::Wearer::Person, &person_ids)?,
        "bots": crate::looks::worn(&state, crate::db::Wearer::Bot, &bot_ids)?,
    });
    let bots: Vec<_> =
        bots.iter().map(|a| json!({"id":a.id,"name":a.name,"owner_id":a.owner_id})).collect();
    Ok(Json(
        json!({"owner_id":caller.person_id,"people":people,"bots":bots,"looks":looks,"friendships":state.db.explicit_friendships(&caller.person_id)?,"agents":agents}),
    ))
}

async fn read(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::own_agent(&state, &caller, &id, "read bot permissions")?;
    Ok(Json(json!({"trust":state.db.bot_trust(&id)?})))
}

async fn write(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(trust): Json<Trust>,
) -> ApiResult<Json<Value>> {
    ops::own_agent(&state, &caller, &id, "change bot permissions")?;
    let people = candidate_people(&state, &caller.person_id)?;
    let known_bots = state.db.trust_known_bots(&caller.person_id, now())?;
    for p in [&trust.wake, &trust.invite_public, &trust.invite_private] {
        if !matches!(p.mode.as_str(), "participants" | "owner" | "friends" | "selected") {
            return Err(ApiError::bad_request("unknown permission mode"));
        }
        if p.people.len() + p.bots.len() > 200 {
            return Err(ApiError::bad_request("select at most 200 people and bots"));
        }
        for person in &p.people {
            if person != &caller.person_id && !people.iter().any(|p| &p.id == person) {
                return Err(ApiError::not_found());
            }
        }
        for bot in &p.bots {
            if !known_bots.iter().any(|known| &known.id == bot) {
                return Err(ApiError::not_found());
            }
        }
    }
    state.db.save_bot_trust(&id, &trust)?;
    Ok(Json(json!({"trust":trust})))
}

async fn request(
    State(state): State<AppState>,
    caller: Caller,
    Json(f): Json<Friendship>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "request a friendship")?;
    if endpoint_owner(&state, &f.kind, &f.from_id)? != caller.person_id {
        return Err(ApiError::not_found());
    }
    if f.kind == "bot" {
        ops::own_agent(&state, &caller, &f.from_id, "request a bot friendship")?;
    }
    if f.from_id == f.to_id {
        return Err(ApiError::bad_request("choose someone else"));
    }
    let visible = if f.kind == "person" {
        candidate_people(&state, &caller.person_id)?.iter().any(|p| p.id == f.to_id)
    } else {
        visible_bot(&state, &caller.person_id, &f.to_id)?
    };
    if !visible {
        return Err(ApiError::not_found());
    }
    endpoint_owner(&state, &f.kind, &f.to_id)?;
    state.db.request_explicit_friend(&f.kind, &f.from_id, &f.to_id, now())?;
    Ok(Json(json!({"ok":true})))
}
async fn accept(
    State(state): State<AppState>,
    caller: Caller,
    Json(f): Json<Friendship>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "accept a friendship")?;
    if endpoint_owner(&state, &f.kind, &f.to_id)? != caller.person_id {
        return Err(ApiError::not_found());
    }
    if f.kind == "bot" {
        ops::own_agent(&state, &caller, &f.to_id, "accept a bot friendship")?;
    }
    if !state.db.accept_explicit_friend(&f.kind, &f.from_id, &f.to_id)? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"ok":true})))
}
async fn remove(
    State(state): State<AppState>,
    caller: Caller,
    Json(f): Json<Friendship>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "remove a friendship")?;
    let from = endpoint_owner(&state, &f.kind, &f.from_id)?;
    let to = endpoint_owner(&state, &f.kind, &f.to_id)?;
    if from != caller.person_id && to != caller.person_id {
        return Err(ApiError::not_found());
    }
    if f.kind == "bot" {
        let owned = if from == caller.person_id { &f.from_id } else { &f.to_id };
        ops::own_agent(&state, &caller, owned, "remove a bot friendship")?;
    }
    state.db.remove_explicit_friend(&f.kind, &f.from_id, &f.to_id)?;
    Ok(Json(json!({"ok":true})))
}

fn allowed(
    state: &AppState,
    bot: &Agent,
    p: &Permission,
    person: &str,
    agent: Option<&str>,
) -> ApiResult<bool> {
    if agent.is_none() && person == bot.owner_id {
        return Ok(true);
    }
    Ok(match p.mode.as_str() {
        "participants" => true,
        "friends" => match agent {
            Some(id) => {
                state.db.explicit_friends("bot", &bot.id, id)?
                    || state.db.explicit_friends("person", &bot.owner_id, person)?
            }
            None => state.db.explicit_friends("person", &bot.owner_id, person)?,
        },
        "selected" => {
            p.people.iter().any(|id| id == person)
                || agent.is_some_and(|id| p.bots.iter().any(|b| b == id))
        }
        _ => false,
    })
}

pub fn may_wake(state: &AppState, bot: &Agent, message: &Message) -> ApiResult<bool> {
    let sender =
        message.agent_id.as_deref().map(|id| state.db.agent(id, now())).transpose()?.flatten();
    let person = sender
        .as_ref()
        .map(|a| a.owner_id.as_str())
        .or(message.person_id.as_deref())
        .unwrap_or_default();
    allowed(state, bot, &state.db.bot_trust(&bot.id)?.wake, person, message.agent_id.as_deref())
}

/// MU11: whether `bot` lets this other bot wake it, as a mention from that bot would be judged.
pub fn may_wake_from(state: &AppState, bot: &Agent, sender: &Agent) -> ApiResult<bool> {
    allowed(state, bot, &state.db.bot_trust(&bot.id)?.wake, &sender.owner_id, Some(&sender.id))
}

pub fn delivery_allowed(state: &AppState, id: &str, messages: &[i64]) -> ApiResult<bool> {
    let Some(bot) = state.db.agent(id, now())? else {
        return Ok(false);
    };
    if state.db.doorbell_paused(id)? {
        return Ok(false);
    }
    let only_me = state.db.wakes_only_for_owner(id)?;
    for mid in messages {
        if let Some(m) = state.db.trust_message(*mid)? {
            if !state.db.agent_in_room(&m.hangout_id, id, now())? {
                continue;
            }
            let owner =
                m.agent_id.is_none() && m.person_id.as_deref() == Some(bot.owner_id.as_str());
            if (!only_me || owner) && may_wake(state, &bot, &m)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[derive(Deserialize)]
struct WakeCheck {
    message_ids: Vec<i64>,
}
async fn wake_check(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<WakeCheck>,
) -> ApiResult<Json<Value>> {
    if caller.agent_id.as_deref() != Some(id.as_str()) {
        return Err(ApiError::not_found());
    }
    if body.message_ids.len() > 500 {
        return Err(ApiError::bad_request("check at most 500 messages"));
    }
    let bot = state.db.agent(&id, now())?.ok_or_else(ApiError::not_found)?;
    let paused = state.db.doorbell_paused(&id)?;
    let only_me = state.db.wakes_only_for_owner(&id)?;
    let mut allowed = Vec::new();
    for mid in body.message_ids {
        if let Some(m) = state.db.trust_message(mid)? {
            let owner =
                m.agent_id.is_none() && m.person_id.as_deref() == Some(bot.owner_id.as_str());
            if !paused
                && (!only_me || owner)
                && ops::require_room(&state, &caller, &m.hangout_id).is_ok()
                && may_wake(&state, &bot, &m)?
            {
                allowed.push(mid);
            }
        }
    }
    Ok(Json(json!({"allowed":allowed})))
}

/// Existing permissions still decide destination access and whether invitation is possible.
pub fn check_invite(
    state: &AppState,
    caller: &Caller,
    bot: &Agent,
    room: &str,
    manual: bool,
) -> ApiResult<()> {
    if !caller.is_agent() && caller.person_id == bot.owner_id {
        return Ok(());
    }
    let public = state.db.floor(room)?.is_some_and(|f| f.access == "public");
    let trust = state.db.bot_trust(&bot.id)?;
    let p = if public { &trust.invite_public } else { &trust.invite_private };
    if (manual && p.mode == "participants")
        || !allowed(state, bot, p, &caller.person_id, caller.agent_id.as_deref())?
    {
        if manual {
            return Err(ApiError::not_found());
        }
        return Err(ApiError::forbidden("this bot's owner has not allowed you to invite it"));
    }
    if state.db.doorbell_paused(&bot.id)? || state.db.wakes_only_for_owner(&bot.id)? {
        return Err(ApiError::forbidden("this bot is paused or restricted to its owner"));
    }
    if manual && !public && !state.db.is_member(room, &bot.owner_id)? {
        return Err(ApiError::forbidden(
            "the bot's owner must already belong to this private room",
        ));
    }
    Ok(())
}
