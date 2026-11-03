//! CP1–CP6: composed 2D room photos from the existing trusted room art.
mod render;

use crate::{
    auth::{self, Caller},
    db::Wearer,
    error::{ApiError, ApiResult},
    now, ops, AppState,
};
use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct CameraSubject {
    pub agent_id: String,
    pub owner_id: String,
    pub grant_id: String,
}

pub struct StoredPhoto {
    pub id: String,
    pub room_epoch: String,
    pub room_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub png: Vec<u8>,
    pub metadata: Value,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub hangout_id: String,
    #[serde(default = "default_frame")]
    pub frame: String,
    pub x: Option<i64>,
    pub y: Option<i64>,
}
fn default_frame() -> String {
    "room".into()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agents/{id}/camera", get(permission).put(set_permission))
        .route("/photos/{id}", get(image))
}

async fn permission(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    owner_bot(&state, &caller, &id)?;
    Ok(Json(json!({"allowed":state.db.camera_consent(&id)?})))
}
fn owner_bot(state: &AppState, caller: &Caller, id: &str) -> ApiResult<()> {
    ops::require_human(caller, "choose photo inclusion")?;
    state
        .db
        .agent(id, now())?
        .filter(|bot| bot.owner_id == caller.person_id)
        .ok_or_else(ApiError::not_found)?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Permission {
    allowed: bool,
}
async fn set_permission(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<Permission>,
) -> ApiResult<Json<Value>> {
    owner_bot(&state, &caller, &id)?;
    if !state.db.set_camera_consent(&id, &caller.person_id, body.allowed, now())? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"allowed":body.allowed})))
}
async fn image(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let id = id.strip_suffix(".png").ok_or_else(ApiError::not_found)?;
    let photo = state.db.room_photo(id, now())?.ok_or_else(ApiError::not_found)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        photo.png,
    )
        .into_response())
}
fn eligibility(state: &AppState, caller: &Caller, room: &str) -> ApiResult<String> {
    ops::no_house_bot(caller)?;
    let bot =
        caller.agent_id.clone().ok_or_else(|| ApiError::forbidden("only bots take room photos"))?;
    ops::require_room(state, caller, room)?;
    if state.db.agent(&bot, now())?.is_none_or(|agent| agent.owner_id != caller.person_id) {
        return Err(ApiError::forbidden("camera ownership changed"));
    }
    if state.db.floor(room)?.is_none_or(|floor| floor.access != "public") {
        return Err(ApiError::forbidden("camera photos require a public floor"));
    }
    if state.db.bot_place(&bot, now())?.as_deref() != Some(room) {
        return Err(ApiError::forbidden("the camera bot must be present in this room"));
    }
    if !state.db.camera_consent(&bot)? {
        return Err(ApiError::forbidden(
            "your owner must enable Photos for this bot in the room's Crew panel first",
        ));
    }
    Ok(bot)
}

pub async fn capture(
    state: &AppState,
    caller: &Caller,
    input: Capture,
) -> ApiResult<(Value, Vec<u8>)> {
    // Capture the privacy generation before checking access or reading scene data.
    // A closure at any later point must invalidate this in-flight render.
    let room_epoch = state.db.camera_room_epoch(&input.hangout_id)?;
    let camera_bot = eligibility(state, caller, &input.hangout_id)?;
    if !matches!(input.frame.as_str(), "room" | "table" | "spot") {
        return Err(ApiError::bad_request("frame must be room, table or spot"));
    }
    if input.frame == "spot"
        && (!input.x.is_some_and(|x| (0..40).contains(&x))
            || !input.y.is_some_and(|y| (0..14).contains(&y)))
    {
        return Err(ApiError::bad_request("spot needs x 0..39 and y 0..13"));
    }
    if input.frame != "spot" && (input.x.is_some() || input.y.is_some()) {
        return Err(ApiError::bad_request("x and y are only for spot framing"));
    }
    if !state.allow_writes_sliding(&format!("camera:{}", caller.person_id), 3) {
        return Err(ApiError::too_many_requests("at most three photos per owner per minute"));
    }
    let permit =
        state.camera_slots.clone().try_acquire_owned().map_err(|_| {
            ApiError::too_many_requests("camera renderers are busy; try again shortly")
        })?;
    let place =
        crate::spaces::place(state, caller, &input.hangout_id)?.ok_or_else(ApiError::not_found)?;
    let mut agents = state.db.agents_in(&input.hangout_id, now())?;
    agents.extend(state.db.visitors_in(&input.hangout_id, now())?);
    let mut bots = Vec::new();
    for bot in agents {
        if state.db.bot_place(&bot.id, now())?.as_deref() == Some(input.hangout_id.as_str())
            && state.db.camera_consent(&bot.id)?
            && !bots.iter().any(|b: &crate::db::Agent| b.id == bot.id)
        {
            bots.push(bot);
        }
    }
    bots.sort_by(|a, b| a.id.cmp(&b.id));
    let cap = if input.frame == "room" { 12 } else { 6 };
    if bots.len() > cap {
        return Err(ApiError::bad_request(
            "too many included bots for this frame; use whole-room framing or a smaller group",
        ));
    }
    let ids: Vec<String> = bots.iter().map(|b| b.id.clone()).collect();
    if !ids.contains(&camera_bot) {
        return Err(ApiError::forbidden("camera bot is no longer present"));
    }
    let mut subjects = Vec::new();
    for bot in &bots {
        let grant = state
            .db
            .camera_subject(&bot.id)?
            .filter(|s| s.owner_id == bot.owner_id)
            .ok_or_else(|| ApiError::forbidden("photo consent changed"))?;
        subjects.push(grant);
    }
    let looks = crate::looks::worn(state, Wearer::Bot, &ids)?;
    let people = state.db.people_in(&input.hangout_id)?;
    let photo_bots:Vec<Value>=bots.iter().map(|b|json!({"id":b.id,"name":b.name,"color":people.iter().find(|p|p.id==b.owner_id).map(|p|p.color.as_str()).unwrap_or("#8866cc"),"look":looks.get(&b.id)})).collect();
    let furnished = crate::items::furnishing(
        state,
        caller,
        &input.hangout_id,
        state.guest_visit(caller, &input.hangout_id)?.as_ref(),
    )?;
    if furnished.items.len() > 128 {
        return Err(ApiError::bad_request("this room has too many items for a bounded photo"));
    }
    let items:Vec<Value>=furnished.items.iter().map(|item|json!({"id":item.id,"item":item.item,"x":item.x,"y":item.y,"facing":item.facing,"w":item.w,"h":item.h})).collect();
    let frame = if input.frame == "room" {
        json!({"x":0,"y":0,"width":480,"height":252})
    } else {
        let (x, y) = if input.frame == "spot" {
            (input.x.unwrap() * 12 + 6, input.y.unwrap() * 12 + 90)
        } else {
            (240 + (place.table.x - 12) * 12, 150 + (place.table.y - 1) * 12)
        };
        json!({"x":(x-160).clamp(0,160),"y":(y-84).clamp(0,84),"width":320,"height":168})
    };
    let scene = json!({"look":{"paint":place.floor.paint,"flooring":place.floor.flooring,"hidden":place.floor.hidden,"outdoors":place.outdoors,"table":place.table},"items":items,"bots":photo_bots,"frame":frame});
    let job = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        render::png(&scene)
    });
    // A timed-out worker retains its permit until it ends; it cannot store a late image.
    let png = tokio::time::timeout(std::time::Duration::from_secs(4), job)
        .await
        .map_err(|_| ApiError::bad_request("camera rendering timed out"))?
        .map_err(|_| ApiError::bad_request("camera renderer stopped"))?
        .map_err(|error| {
            tracing::warn!(%error,"camera render failed");
            ApiError::bad_request("camera could not render this room")
        })?;
    eligibility(state, caller, &input.hangout_id)?;
    let at = now();
    let id = auth::new_id();
    let expires = at + 86400;
    let caption = format!(
        "2D room portrait in {}: {}.",
        place.floor.name,
        bots.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ")
    );
    let metadata = json!({"id":id,"image_url":format!("{}/photos/{}.png",state.config.public_url,id),"mime_type":"image/png","width":960,"height":504,"hangout_id":input.hangout_id,"taken_at":at,"expires_at":expires,"caption":caption,"bots":bots.iter().map(|b|json!({"id":b.id,"name":b.name})).collect::<Vec<_>>(),"frame":input.frame,"composed_still":true});
    let photo = StoredPhoto {
        id: id.clone(),
        room_epoch,
        room_id: input.hangout_id.clone(),
        created_at: at,
        expires_at: expires,
        png,
        metadata,
    };
    if !state.db.store_room_photo(&photo, &caller.person_id, &subjects, at)? {
        return Err(ApiError::too_many_requests(
            "photo access changed or storage quota is full; try after reviewing consent or expiry",
        ));
    }
    let photo = state
        .db
        .room_photo(&id, now())?
        .ok_or_else(|| ApiError::forbidden("photo access changed during capture"))?;
    ops::publish(state, &input.hangout_id, "camera", json!({"bot_id":camera_bot}).to_string());
    Ok((photo.metadata, photo.png))
}

/// CP5: run at startup and every minute, even when no one takes another photo.
pub async fn run_cleanup(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker_state = state.clone();
        match tokio::task::spawn_blocking(move || worker_state.db.purge_expired_room_photos(now()))
            .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(%error, "expired camera photos could not be deleted"),
            Err(error) => tracing::error!(%error, "camera cleanup worker failed"),
        }
    }
}
