//! HC59 to HC64 (#1056): a floor pools credits for one pack or one coin piece.
//! The rules are in docs/economy.md. The web card and the MCP tool are a later slice.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{self, Caller},
    db::{FundView, NewFund, NewPut, PutIn, FUND_DAYS},
    economy::{self, owner_said_yes_to_fund},
    error::{ApiError, ApiResult},
    items::{comes_with, pack_of, paid_pack_with, piece_price, PIECE},
    now, ops, AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/floors/{id}/funds", get(list_funds).post(start_fund))
        .route("/api/funds/{id}/put", post(put_in))
        .route("/api/funds/{id}/cancel", post(cancel_fund))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartBody {
    item: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PutBody {
    amount: i64,
    #[serde(default)]
    asked: Option<String>,
}

/// HC63: the fund as anyone who can see the floor may read it. `yours` is only this caller's
/// put-in. The total and the target are the rest of what the fund shows.
fn fund_json(fund: &FundView) -> Value {
    json!({
        "id": fund.id,
        "item": fund.item,
        "target": fund.target,
        "total": fund.total,
        "yours": fund.yours,
        "started_by_you": fund.started_by_you,
        "status": fund.status,
        "ends_at": fund.ends_at,
        "created_at": fund.created_at,
    })
}

/// A pack or a `piece:<kind>` a fund can buy, and its price. A car, a person, a free pack and a
/// pack whose season is over are refused.
fn price_of(item: &str) -> ApiResult<i64> {
    let item = item.trim();
    if item.is_empty() || item.len() > 64 || item.starts_with("car:") || item.starts_with("person")
    {
        return Err(ApiError::bad_request(
            "a fund buys a pack or a piece for the floor, never a car or a person",
        ));
    }
    if let Some(kind) = item.strip_prefix(PIECE) {
        let cost = piece_price(kind).ok_or_else(|| {
            ApiError::bad_request("that isn't a piece a fund can buy: see credits")
        })?;
        let pack = paid_pack_with(kind).ok_or_else(ApiError::not_found)?;
        if pack.over {
            return Err(ApiError::conflict("that piece's pack is over, so it can't be bought"));
        }
        return Ok(cost);
    }
    if item.contains(':') {
        return Err(ApiError::bad_request(
            "a fund buys a pack or a piece for the floor, never a car or a person",
        ));
    }
    let pack = pack_of(item)
        .filter(|pack| pack.cost > 0)
        .ok_or_else(|| ApiError::bad_request("that isn't a pack a fund can buy: see credits"))?;
    if pack.over {
        return Err(ApiError::conflict("that pack's season is over, so it can't be bought"));
    }
    Ok(pack.cost)
}

/// Packs that already place this piece, so a floor that unlocked the pack has the piece too.
/// A piece the pack only sells (`SOLD_ALONE`) is not covered.
fn covered_by(item: &str) -> Vec<&str> {
    let Some(kind) = item.strip_prefix(PIECE) else {
        return Vec::new();
    };
    match paid_pack_with(kind) {
        Some(pack) if comes_with(pack, kind) => vec![pack.pack],
        _ => Vec::new(),
    }
}

/// Anyone who can see this floor: someone in it, or a person looking at a public floor they
/// haven't been turned out of. A private floor they aren't on is the same as a missing one.
fn see_floor(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<crate::db::Floor> {
    if caller.is_house {
        return Err(ApiError::not_found());
    }
    let floor = state.db.floor(hangout_id)?.ok_or_else(ApiError::not_found)?;
    let space = state.db.space(&floor.space_id)?.ok_or_else(ApiError::not_found)?;
    if state.db.is_turned_out(&space.id, &caller.person_id)? {
        return Err(ApiError::not_found());
    }
    if state.in_room(caller, hangout_id)? {
        return Ok(floor);
    }
    if floor.access == "public" && !caller.is_agent() {
        return Ok(floor);
    }
    Err(ApiError::not_found())
}

/// `GET /api/floors/{id}/funds` (HC63).
async fn list_funds(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(funds_here(&state, &caller, &id)?))
}

/// HC63: a floor's funds as this caller may see them, and whether they may start one. The `funds`
/// tool gives a bot the same.
pub(crate) fn funds_here(state: &AppState, caller: &Caller, id: &str) -> ApiResult<Value> {
    see_floor(state, caller, id)?;
    let funds = state.db.funds_on(id, &caller.person_id)?;
    // HC59: whether this caller may start one here, so a page offers it only to them.
    let can_start = !caller.is_agent() && state.db.is_member(id, &caller.person_id)?;
    Ok(json!({
        "funds": funds.iter().map(fund_json).collect::<Vec<_>>(),
        "can_start": can_start,
    }))
}

/// `POST /api/floors/{id}/funds` (HC59). A member starts a fund for one pack or piece.
async fn start_fund(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<StartBody>,
) -> ApiResult<Json<Value>> {
    see_floor(&state, &caller, &id)?;
    ops::require_human(&caller, "start a fund")?;
    if !state.db.is_member(&id, &caller.person_id)? {
        return Err(ApiError::forbidden("only a member of this floor can start a fund"));
    }
    let item = body.item.trim();
    let target = price_of(item)?;
    let covered = covered_by(item);
    let at = now();
    // What's paid but not written down yet counts toward the three-open check's person, and a
    // later put reads the same balance.
    economy::credits(&state.db, &caller.person_id, at)?;
    let started = state.db.start_fund(&NewFund {
        id: &auth::new_id(),
        hangout_id: &id,
        starter_id: &caller.person_id,
        item,
        target,
        covered_by: &covered,
        now: at,
    })?;
    match started {
        Ok(fund) => {
            debug_assert_eq!(fund.ends_at - fund.created_at, FUND_DAYS * crate::ops::DAY_SECS);
            Ok(Json(fund_json(&fund)))
        }
        Err(reason) => Err(ApiError::conflict(reason)),
    }
}

/// `POST /api/funds/{id}/put` (HC60, HC61).
async fn put_in(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<PutBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(put_into(&state, &caller, &id, body.amount, body.asked.as_deref())?))
}

/// HC60 and HC61: puts `amount` of the caller's credits (their owner's, for a bot) into a fund.
/// The `funds` tool does the same, so a bot's put needs the same cited yes.
pub(crate) fn put_into(
    state: &AppState,
    caller: &Caller,
    id: &str,
    amount: i64,
    asked: Option<&str>,
) -> ApiResult<Value> {
    let body = PutBody { amount, asked: asked.map(str::to_string) };
    if caller.is_house {
        return Err(ApiError::not_found());
    }
    if body.amount < 1 {
        return Err(ApiError::bad_request("put in at least 1 credit"));
    }
    let Some(head) = state.db.fund_row(id)? else {
        return Err(ApiError::not_found());
    };
    let hangout = head.hangout_id;
    let item = head.item;
    see_floor(state, caller, &hangout)?;
    let at = now();
    if let Some(agent_id) = &caller.agent_id {
        // HC60: a yes that names this fund, not a yes to buying the item. A room message is not a yes.
        owner_said_yes_to_fund(&state.db, caller, agent_id, &item, body.asked.as_deref(), at)?;
    }
    economy::credits(&state.db, &caller.person_id, at)?;
    // A person's own put cites no question. Only a bot's yes is stored, and only once.
    let asked = caller.agent_id.as_deref().and(body.asked.as_deref());
    let put = state.db.put_in_fund(&NewPut {
        id: &auth::new_id(),
        fund_id: id,
        person_id: &caller.person_id,
        agent_id: caller.agent_id.as_deref(),
        amount: body.amount,
        asked,
        now: at,
    })?;
    match put {
        PutIn::Taken { taken, fund } => Ok(json!({ "put": taken, "fund": fund_json(&fund) })),
        PutIn::Missing => Err(ApiError::not_found()),
        PutIn::NotOpen => Err(ApiError::conflict("this fund is no longer open")),
        PutIn::Ended => Err(ApiError::conflict("this fund has ended")),
        PutIn::NothingLeft => Err(ApiError::conflict("this fund is already full")),
        PutIn::TooFew { balance } => {
            Err(ApiError::conflict(format!("that is more credits than the balance of {balance}")))
        }
        PutIn::OverBotLimit { left_today } => Err(ApiError::forbidden(format!(
            "that's more than this bot may spend today ({left_today} left). Its owner sets the \
             limit on their Your credits card"
        ))),
        PutIn::YesUsed => Err(ApiError::conflict(
            "that yes was already used for a put-in. Ask again, and pass the new question's id \
             as asked",
        )),
    }
}

/// `POST /api/funds/{id}/cancel` (HC62). The starter gets every put-in back. Complete funds stay
/// spent.
async fn cancel_fund(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if caller.is_house {
        return Err(ApiError::not_found());
    }
    ops::require_human(&caller, "cancel a fund")?;
    let Some(head) = state.db.fund_row(&id)? else {
        return Err(ApiError::not_found());
    };
    let hangout = head.hangout_id;
    let starter = head.starter_id;
    let sees = see_floor(&state, &caller, &hangout).is_ok();
    if starter.as_deref() != Some(caller.person_id.as_str()) {
        if !sees {
            return Err(ApiError::not_found());
        }
        return Err(ApiError::forbidden("only the person who started this fund can cancel it"));
    }
    match state.db.cancel_fund(&id, &caller.person_id, now())? {
        Ok(fund) => Ok(Json(fund_json(&fund))),
        Err("no such fund") => Err(ApiError::not_found()),
        Err("you didn't start this fund") => {
            Err(ApiError::forbidden("only the person who started this fund can cancel it"))
        }
        Err(reason) => Err(ApiError::conflict(reason)),
    }
}
