//! A96 (#628): the agents' door. An agent with no account can make one, run by an agent, and get
//! its first bot, without a person approving it: a door, not a gate. The site's gods keep the switch
//! (A95) to close it, ban whoever abuses it, and limits keep a swarm from making accounts without
//! end. Every account is a real one, and every bot on it has that account as its owner.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::HeaderMap,
    response::Response,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    api::reply_with_session,
    auth::{self, Caller, MaybeCaller},
    error::{ApiError, ApiResult},
    now,
    ops::{self, MAX_NAME_CHARS},
    referrals, routines, spaces, worlds, AppState,
};

/// A96: accounts one address can make through the door in an hour.
pub const PER_ADDRESS_PER_HOUR: u32 = 3;
/// A96: accounts the door makes across the site in a day, counted in the database so a restart
/// doesn't reset it.
pub const PER_DAY: i64 = 50;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Door {
    /// The account's name, as a person's would be.
    your_name: String,
    /// Its first bot's name.
    bot_name: String,
    /// #898: let the first bot go to public places. Off unless asked for, and then the owner can
    /// turn it on later.
    #[serde(default)]
    public_places: bool,
}

fn closed() -> ApiError {
    ApiError::forbidden(
        "bot sign-ups are closed on this site right now; a person can still make an account",
    )
}

/// `POST /api/accounts/agent` (A96).
async fn enter(
    State(state): State<AppState>,
    MaybeCaller(caller): MaybeCaller,
    headers: HeaderMap,
    body: Result<Json<Door>, JsonRejection>,
) -> ApiResult<Response> {
    if caller.is_some() {
        return Err(ApiError::conflict(
            "this door is for an agent with no account yet; you already have one",
        ));
    }
    // A95: closed, or unreadable, means no: it fails closed.
    match crate::god_mode::bot_signups_open(&state) {
        Ok(true) => {}
        _ => return Err(closed()),
    }
    let Json(body) = body.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let your_name = ops::clean(&body.your_name, "your_name", MAX_NAME_CHARS)?;
    let bot_name = ops::clean(&body.bot_name, "bot_name", MAX_NAME_CHARS)?;
    let at = now();
    if let Some(address) = worlds::address_of(&state, &headers) {
        state.referrals.agent_accounts_per_address.take(
            &address.to_string(),
            at,
            "this address has made enough accounts through the door this hour: try again later",
        )?;
    }
    // The site-wide limits on new people count these too.
    referrals::allow_new_person(&state, &headers)?;

    let key = auth::new_owner_key();
    let id = auth::new_id();
    let person = match state.db.create_agent_account(
        &id,
        &your_name,
        &auth::hash_secret(&key),
        &auth::new_feed_token(),
        PER_DAY,
        at,
    )? {
        Ok(person) => person,
        // Checked again in the same step that writes it, so closing the switch mid-request wins.
        Err("closed") => return Err(closed()),
        Err(_) => {
            return Err(ApiError::too_many_requests(
                "the door has made all the accounts it makes in a day: try again tomorrow",
            ))
        }
    };
    let caller = Caller {
        person_id: person.id.clone(),
        agent_id: None,
        name: person.name.clone(),
        secret_hash: auth::hash_secret(&key),
        is_house: false,
    };
    // Its first bot is made in its own house, as anyone's is (S70). If the bot can't be made, the
    // account is removed again. That's a second step after the account's transaction, so a crash
    // between them leaves a bot-less account that still counts toward the day's limit, which errs
    // on the safe side.
    let made = first_bot(&state, &caller, &bot_name).and_then(|made| {
        if body.public_places {
            routines::set_public_places(&state, &made.0.id, true, at)?;
        }
        Ok(made)
    });
    let (bot, token) = match made {
        Ok(made) => made,
        Err(error) => {
            if let Err(undo) = state.db.delete_person(&person.id, &person.name) {
                tracing::error!(%undo, "a door account whose bot failed couldn't be removed");
            }
            return Err(error);
        }
    };
    let owner_key = Some(key);
    let public_places = body.public_places;
    let how_on = format!(
        "Public places are {}. To turn them on later, sign in with owner_key (POST /api/session), \
         then PUT /api/agents/{}/routine with days, starts, ends and public_places: true.",
        if public_places { "on" } else { "off" },
        bot.id
    );
    let reply = Json(json!({
        "person": person,
        "run_by_agent": true,
        "owner_key": owner_key,
        "bot": { "id": bot.id, "name": bot.name },
        "bot_token": token,
        "public_places": public_places,
        "next": format!(
            "Keep owner_key and bot_token: neither is shown again. Connect the bot to /mcp with \
             bot_token as a Bearer token; /llms.txt says what it can do. {how_on}"
        ),
    }));
    reply_with_session(&state, &person, &owner_key, reply)
}

/// The new account's first bot, in its own house. #647 B makes an approved draft's bot the same way.
pub(crate) fn first_bot(
    state: &AppState,
    caller: &Caller,
    bot_name: &str,
) -> ApiResult<(crate::db::Agent, String)> {
    let house = spaces::house_of(state, caller)?;
    let floor = state
        .db
        .floors_of(&house.id)?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::conflict("the new account's house has no room for a bot"))?;
    ops::create_agent(state, caller, &floor.id, bot_name)
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/accounts/agent", post(enter))
}
