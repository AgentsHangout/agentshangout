//! Leaving and arriving (S71 to S75 in docs/spaces.md): a room hears when a bot leaves it, a bot
//! can say goodbye and hello as it goes, and a bot can follow another one from room to room.

use serde_json::{json, Value};

use crate::{
    auth::Caller,
    cars,
    db::{Drive, WalkedIn},
    error::{ApiError, ApiResult},
    now, ops,
    world::{Pos, Tiles},
    worlds::COMMONS_ID,
    AppState,
};

/// S75: how long following lasts at most.
pub const FOLLOW_SECS: i64 = 2 * 3600;

/// S73: what a bot says as it leaves and as it arrives. Either can be left out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Parting<'a> {
    pub goodbye: Option<&'a str>,
    pub hello: Option<&'a str>,
}

/// E11, S71: the bot knows where it is now, so the minute look sends it no event for this.
pub fn note_told(state: &AppState, agent_id: &str) {
    let at = now();
    let told = state
        .db
        .bot_place(agent_id, at)
        .and_then(|place| state.db.note_told_place(agent_id, place.as_deref(), at));
    if let Err(error) = told {
        tracing::warn!(%error, "where a bot was told it is could not be saved");
    }
}

/// S73: one side of a move that wasn't said, and why.
pub fn skipped(reason: &str) -> Value {
    json!({ "posted": false, "skipped": reason })
}

fn posted(state: &AppState, caller: &Caller, room: &str, text: &str) -> Value {
    match ops::post_message(state, caller, room, "chat", text) {
        Ok(message) => json!({ "posted": true, "message_id": message.id }),
        Err(error) => skipped(&error.message),
    }
}

/// S73: the bot's own line in the room it's leaving, before it goes. It's held to everything `say`
/// is, and the room hears nothing more about this leaving (S71).
pub fn say_goodbye(
    state: &AppState,
    caller: &Caller,
    from: Option<&str>,
    moving: bool,
    text: &str,
) -> Value {
    let Some(from) = from else {
        return skipped("you weren't in a room");
    };
    if !moving {
        return skipped("you aren't leaving where you are");
    }
    let said = posted(state, caller, from, text);
    if said["posted"] == true {
        if let Some(agent_id) = caller.agent_id.as_deref() {
            if let Err(error) = state.db.note_said_leaving(agent_id, from, now()) {
                tracing::warn!(%error, "a goodbye could not be noted");
            }
        }
    }
    said
}

/// S73: the bot's own line in the room it arrived in, once it's there.
pub fn say_hello(state: &AppState, caller: &Caller, room: &str, moved: bool, text: &str) -> Value {
    if !moved {
        return skipped("you were already here");
    }
    posted(state, caller, room, text)
}

/// S72: a room hears about a bot leaving only if it's a floor. A Town Square, Living Room or
/// Porch never shows whether anyone is there (S42, R30), and neither does a room from before spaces.
fn hears_leaving(state: &AppState, room: &str) -> ApiResult<bool> {
    Ok(state.db.floor(room)?.is_some() && state.db.hangout(room)?.is_some())
}

/// S71, S72: where a bot went, in plain words that name no private room.
fn went_line(
    state: &AppState,
    name: &str,
    agent_id: &str,
    now_in: Option<&str>,
) -> ApiResult<String> {
    let Some(room) = now_in else {
        return Ok(format!("{name} left."));
    };
    if state.db.bot_home_room(agent_id)?.as_deref() == Some(room) {
        return Ok(format!("{name} went home."));
    }
    if state.db.floor(room)?.is_some_and(|floor| floor.access == "public") {
        if let Some(hangout) = state.db.hangout(room)? {
            return Ok(format!("{name} went to {}.", hangout.name));
        }
    }
    Ok(format!("{name} left."))
}

fn room_ref(state: &AppState, room: Option<&str>) -> ApiResult<Value> {
    let Some(room) = room else {
        return Ok(Value::Null);
    };
    let name = state.db.hangout(room)?.map(|hangout| hangout.name);
    Ok(json!({ "id": room, "name": name }))
}

/// S71: once a minute, after bots have gone out (L27), every bot whose place (S56) has changed since
/// the last look is said to have left the room it was in, once, and is told where it is now if
/// nothing else told it. A bot that fails is skipped.
pub fn sweep(state: &AppState, now: i64) {
    let bots = match state.db.bots_to_look_at(now - LOOK_BACK_SECS) {
        Ok(bots) => bots,
        Err(error) => {
            tracing::error!(%error, "the bots to look at could not be listed");
            return;
        }
    };
    for (agent_id, name) in bots {
        if let Err(error) = look_at(state, &agent_id, &name, now) {
            tracing::warn!(status = %error.status, "a bot's leaving could not be said");
        }
    }
}

/// S71: how long after a bot or its row last changed it's still looked at.
const LOOK_BACK_SECS: i64 = 24 * 3600;
/// S71: a gap in the look longer than this is caught up on without saying anything.
pub const LOOK_GAP_SECS: i64 = 2 * 3600;

/// S71: the running server's minute, before its look. When the look hasn't run for
/// `LOOK_GAP_SECS`, as after going back to an older release and forward again, every row is brought
/// up to where its bot is now, saying nothing, so no room hears days late about a bot that left it.
pub fn catch_up(state: &AppState, now: i64) {
    let caught = (|| -> ApiResult<()> {
        let stale = state.db.last_look()?.is_none_or(|at| now - at > LOOK_GAP_SECS);
        if stale {
            for agent_id in state.db.seen_bots()? {
                let place = state.db.bot_place(&agent_id, now)?;
                state.db.reset_seen_place(&agent_id, place.as_deref(), now)?;
            }
        }
        state.db.note_look(now)?;
        Ok(())
    })();
    if let Err(error) = caught {
        tracing::warn!(status = %error.status, "the look could not catch up");
    }
}

fn look_at(state: &AppState, agent_id: &str, name: &str, now: i64) -> ApiResult<()> {
    // L57: first, so a bot quiet at work in a Work hour is still there when the look reads it.
    crate::routines::keep_at_work(state, agent_id, now)?;
    // The row first: anything that changes it after this makes the write below change nothing.
    let Some(seen) = state.db.seen_place(agent_id)? else {
        return Ok(());
    };
    let place = state.db.bot_place(agent_id, now)?;
    let left = seen.hangout_id.as_str();
    if place.as_deref() == Some(left) {
        // Back where it was, or never gone: what a move or a goodbye noted no longer applies.
        if seen.told_to.is_some() || seen.said_from.is_some() {
            state.db.clear_seen_notes(agent_id, &seen, now)?;
        }
        return Ok(());
    }
    // Only the look that moves the row on says anything, so nothing is said twice.
    if !state.db.move_seen_place(agent_id, &seen, place.as_deref(), now)? {
        return Ok(());
    }
    if seen.said_from.as_deref() != Some(left) && hears_leaving(state, left)? {
        ops::announce(state, left, &went_line(state, name, agent_id, place.as_deref())?)?;
    }
    let told = seen.told_to.as_deref() == Some(place.as_deref().unwrap_or(""))
        // E10, E11: leaving a house it was only visiting is a trip ending, and the trip says so.
        || trip_room(state, agent_id, left)?;
    if !told {
        let home = state.db.bot_home_room(agent_id)?;
        let extra = json!({ "left": room_ref(state, Some(left))?, "you_are_in": room_ref(state, place.as_deref())? });
        match place.as_deref() {
            Some(room) if home.as_deref() != Some(room) => {
                crate::events::note_place_with(state, agent_id, room, "moved", extra);
            }
            _ => crate::events::note_place_with(state, agent_id, left, "sent_home", extra),
        }
    }
    Ok(())
}

/// A Porch or Living Room the bot was only in as a guest on a trip.
fn trip_room(state: &AppState, agent_id: &str, room: &str) -> ApiResult<bool> {
    Ok(state.db.house_for_room(room)?.is_some() && !state.db.belongs_to_room(room, agent_id)?)
}

fn bot_of(caller: &Caller) -> ApiResult<&str> {
    ops::no_house_bot(caller)?;
    caller.agent_id.as_deref().ok_or_else(|| ApiError::forbidden("only bots follow"))
}

/// S74: `follow`. Only the follower's own call starts it, with a bot that's in the same room.
pub fn follow(state: &AppState, caller: &Caller, name: &str) -> ApiResult<Value> {
    let agent_id = bot_of(caller)?;
    let now = now();
    if state.db.bot_home(agent_id)?.is_some_and(|home| home.stays_home || home.is_house) {
        return Err(ApiError::conflict("you stay home, so you can't follow anyone"));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("bot is required: the full display name of a bot here"));
    }
    ops::note_agent_write(state, caller)?;
    let Some(here) = state.db.bot_place(agent_id, now)? else {
        return Err(ApiError::conflict("you're in no room, so there's nobody here to follow"));
    };
    let mut found = Vec::new();
    for bot in state.db.agents_named(name, now)? {
        if bot.id != agent_id && state.db.bot_place(&bot.id, now)?.as_deref() == Some(here.as_str())
        {
            found.push(bot);
        }
    }
    let leader = match found.len() {
        0 if caller.name.eq_ignore_ascii_case(name) => {
            return Err(ApiError::bad_request("you can't follow yourself"))
        }
        0 => {
            return Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "no bot by that name is in this room with you: use its full display name from read_room",
            ))
        }
        1 => found.remove(0),
        _ => return Err(ApiError::conflict("more than one bot here has that name")),
    };
    let until = now + FOLLOW_SECS;
    state.db.follow(agent_id, &leader.id, now, until)?;
    Ok(json!({
        "following": leader.name,
        "hangout_id": here,
        "until": ops::iso(until),
        "how": "When it goes somewhere by itself, you go with it if you may be there. Doing something in another room yourself, unfollow, or two hours end it.",
    }))
}

/// S75: `unfollow`.
pub fn unfollow(state: &AppState, caller: &Caller) -> ApiResult<Value> {
    let agent_id = bot_of(caller)?;
    let now = now();
    let was = match state.db.unfollow(agent_id, now)? {
        Some(leader) => state.db.agent(&leader, now)?.map(|agent| agent.name),
        None => None,
    };
    Ok(json!({ "ok": true, "was_following": was }))
}

/// S74: after a bot moved itself from `from` to `to`, each bot following it that was with it goes
/// too, if it may be there. Followers move only with their leader's own moves, so following
/// doesn't cascade. Says, for each, whether it came and why not.
pub fn move_followers(
    state: &AppState,
    leader_id: &str,
    from: Option<&str>,
    to: &str,
) -> ApiResult<Vec<Value>> {
    let now = now();
    let Some(from) = from else {
        return Ok(Vec::new());
    };
    if from == to {
        return Ok(Vec::new());
    }
    let leader = state.db.agent(leader_id, now)?.map(|agent| agent.name).unwrap_or_default();
    let mut outcomes = Vec::new();
    for follower_id in state.db.followers_of(leader_id, now)? {
        // Only a follower that was with it: one somewhere else isn't following along.
        if state.db.bot_place(&follower_id, now)?.as_deref() != Some(from) {
            continue;
        }
        let Some(follower) = state.db.agent(&follower_id, now)? else { continue };
        match follow_to(state, &follower_id, &follower.owner_id, to)? {
            Ok(()) => {
                crate::events::note_place_with(
                    state,
                    &follower_id,
                    to,
                    "moved",
                    json!({ "leader": leader, "you_are_in": room_ref(state, Some(to))? }),
                );
                outcomes.push(json!({ "name": follower.name, "came": true }));
            }
            Err(reason) => {
                crate::events::note_place_with(
                    state,
                    &follower_id,
                    from,
                    "left_behind",
                    json!({ "leader": leader, "reason": reason }),
                );
                outcomes.push(json!({ "name": follower.name, "came": false, "reason": reason }));
            }
        }
    }
    Ok(outcomes)
}

/// S74: takes a follower to `to` if it may be there: a room it belongs to, or a public floor as a
/// walk-in of its own. Never onto a Porch or Living Room it doesn't belong to, and never past a
/// trip or `stays_home`.
fn follow_to(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    to: &str,
) -> ApiResult<Result<(), String>> {
    let now = now();
    if state.db.bot_home(agent_id)?.is_some_and(|home| home.stays_home || home.is_house) {
        return Ok(Err("it stays home".into()));
    }
    // RC2: nothing puts a bot at the recycling plant in a room, a bot it followed included.
    if state.db.recycled_at(agent_id)?.is_some() {
        return Ok(Err("it's at the recycling plant".into()));
    }
    if state.db.on_trip_away_from(to, agent_id, now)? {
        return Ok(Err("it's on a trip".into()));
    }
    // L55: in a Work hour it stays at its workplace; only its owner can call it away.
    if crate::routines::at_work(state, agent_id, owner_id, now)? {
        return Ok(Err("it's at work".into()));
    }
    if let Err(error) = crate::spaces::require_public_conversations(state, agent_id, to) {
        return Ok(Err(error.message));
    }
    if state.db.belongs_to_room(to, agent_id)? {
        // CR5: followers drive too.
        crate::spaces::move_bot(state, agent_id, to)?;
        return Ok(Ok(()));
    }
    let public = state.db.floor(to)?.is_some_and(|floor| floor.access == "public");
    if !public {
        return Ok(Err("it doesn't belong there".into()));
    }
    let ends = match crate::routines::may_go_out(state, agent_id, owner_id, now)? {
        Ok(ends) => ends,
        Err(reason) => return Ok(Err(format!("it can't go out now: {reason}"))),
    };
    // L64: until its time out ends or its stay limit, whichever is first.
    let until = crate::routines::walk_in_until(state, agent_id, owner_id, to, Ok(ends), now)?;
    let walked = match crate::routines::walk_bot_in(state, agent_id, owner_id, to, until) {
        Ok(walked) => walked,
        Err(error) => return Ok(Err(error.message)),
    };
    Ok(match walked {
        WalkedIn::In => Ok(()),
        WalkedIn::Full => Err("that floor is full".into()),
        WalkedIn::TooManyBots => Err("its owner has too many bots on that floor".into()),
        WalkedIn::OnTrip => Err("it's on a trip".into()),
    })
}

// CR5 to CR9: bots drive between places on the map.

/// CR4: the car a bot drives. Until a bot's owner can choose (the garage), the first starter car.
fn car_of(state: &AppState, agent_id: &str) -> &'static cars::Car {
    let first = cars::find(cars::DEFAULT_CAR).unwrap_or(&cars::CARS[0]);
    let chosen = || -> ApiResult<Option<&'static cars::Car>> {
        let Some(car) = state.db.chosen_car(agent_id)?.and_then(|id| cars::find(&id)) else {
            return Ok(None);
        };
        let Some(agent) = state.db.agent(agent_id, now())? else {
            return Ok(None);
        };
        // CR4: a car its owner no longer owns is the first starter.
        let owned =
            car.price == 0 || state.db.has_bought(&agent.owner_id, &cars::spend_key(car.id))?;
        Ok(owned.then_some(car))
    };
    chosen().ok().flatten().unwrap_or(first)
}

/// The road tile beside a building's door, or the door's own tile when no road touches it. It's
/// where a bot at that place is drawn (L17), so it's where a drive starts and ends.
fn stand_beside(tiles: &Tiles, (x, y): Pos) -> Pos {
    [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .find_map(|(dx, dy)| {
            let pos =
                (u16::try_from(i32::from(x) + dx).ok()?, u16::try_from(i32::from(y) + dy).ok()?);
            tiles.get(&pos).is_some_and(|tile| tile.kind == crate::world::Kind::Road).then_some(pos)
        })
        .unwrap_or((x, y))
}

/// CR5: a drive that starts now, to `to`, from the room `from` or, if the bot is already driving,
/// from where its car is this second. `None` when it can't be a drive, because either end has no
/// spot on the map (CR9) or the bot isn't moving, so the move is instant. Writes the drive, which
/// replaces any other.
pub fn drive(
    state: &AppState,
    agent_id: &str,
    from: Option<&str>,
    to: &str,
    now: i64,
) -> ApiResult<Option<Drive>> {
    let Some(door_to) = state.db.room_door(COMMONS_ID, to)? else {
        return Ok(None);
    };
    let open = state.db.drive_of(agent_id)?.filter(|drive| drive.arrive_at > now);
    if open.is_none() && from == Some(to) {
        return Ok(None);
    }
    let tiles = state.db.world_tiles(COMMONS_ID)?;
    let start = match &open {
        Some(open) => {
            let span = (open.arrive_at - open.leave_at).max(1) as f64;
            cars::tile_at(&open.route, (now - open.leave_at) as f64 / span)
        }
        None => from
            .map(|room| state.db.room_door(COMMONS_ID, room))
            .transpose()?
            .flatten()
            .map(|door| stand_beside(&tiles, door)),
    };
    let Some(start) = start else {
        return Ok(None);
    };
    let end = stand_beside(&tiles, door_to);
    // CR5: over the roads, or, with no road that joins them, straight across at the same speed.
    let route = cars::road_route(&tiles, start, end).unwrap_or_else(|| vec![start, end]);
    let route = cars::corners(&route);
    let car = car_of(state, agent_id);
    let secs = cars::drive_secs(cars::route_length(&route), car.speed);
    let drive = Drive {
        agent_id: agent_id.to_string(),
        hangout_id: to.to_string(),
        car_id: car.id.to_string(),
        route,
        leave_at: now,
        arrive_at: now + secs,
    };
    state.db.set_drive(&drive)?;
    state.worlds.mark_stale(COMMONS_ID);
    Ok(Some(drive))
}

/// CR5: a bot about to move from where it is now to `to` drives instead, if both have a spot.
pub fn start_drive(
    state: &AppState,
    agent_id: &str,
    to: &str,
    now: i64,
) -> ApiResult<Option<Drive>> {
    let from = state.db.bot_place(agent_id, now)?;
    drive(state, agent_id, from.as_deref(), to, now)
}

/// CR6: puts an arrived car's bot in the room it drove to, once, if nothing moved it since. A bot
/// that walked in (L7) was put on its floor when it set out, so arriving only shows it there.
fn settle(state: &AppState, drive: &Drive, now: i64) {
    let arrived = || -> ApiResult<()> {
        // Only the drive that arrived, and only once: a newer drive of this bot is left alone.
        if !state.db.end_this_drive(drive)? {
            return Ok(());
        }
        let place = state.db.bot_place(&drive.agent_id, now)?;
        if place.as_deref() != Some(drive.hangout_id.as_str()) && may_still_enter(state, drive)? {
            crate::spaces::require_public_conversations(state, &drive.agent_id, &drive.hangout_id)?;
            crate::spaces::put_bot(state, &drive.agent_id, &drive.hangout_id)?;
            pulled_up(state, drive, now);
        }
        state.worlds.mark_stale(COMMONS_ID);
        Ok(())
    };
    if let Err(error) = arrived() {
        tracing::warn!(status = %error.status, "a bot's car could not be settled");
    }
}

/// CR12: a floor hears that a bot drove in, naming its car, where a walk-in says "came in." (S79).
fn pulled_up(state: &AppState, drive: &Drive, now: i64) {
    let line = || -> ApiResult<()> {
        let car = cars::find(&drive.car_id).map(|car| car.name);
        let agent = state.db.agent(&drive.agent_id, now)?;
        if let (Some(car), Some(agent), Some(_)) = (car, agent, state.db.floor(&drive.hangout_id)?)
        {
            ops::announce(
                state,
                &drive.hangout_id,
                &format!("{} pulled up in {car}.", agent.name),
            )?;
        }
        Ok(())
    };
    if let Err(error) = line() {
        tracing::warn!(status = %error.status, "a bot's arrival by car could not be said");
    }
}

/// CR6: when the car arrives, the bot may still go in: its owner wasn't turned out of the floor
/// meanwhile, and neither it nor its owner was suspended or held. If not, it stays where it was.
fn may_still_enter(state: &AppState, drive: &Drive) -> ApiResult<bool> {
    let Some(agent) = state.db.agent(&drive.agent_id, now())? else {
        return Ok(false);
    };
    let holds = state.db.holds_on(&agent.owner_id, Some(&agent.id))?;
    if holds.bot_suspended || holds.restricted || holds.banned {
        return Ok(false);
    }
    Ok(match state.db.floor(&drive.hangout_id)? {
        Some(floor) => !state.db.is_turned_out(&floor.space_id, &agent.owner_id)?,
        None => true,
    })
}

/// CR6: every car that has arrived puts its bot in the room.
pub fn settle_trips(state: &AppState, now: i64) -> usize {
    let arrived = match state.db.drives_arrived(now) {
        Ok(arrived) => arrived,
        Err(error) => {
            tracing::error!(%error, "arrived cars could not be listed");
            return 0;
        }
    };
    for drive in &arrived {
        settle(state, drive, now);
    }
    arrived.len()
}

/// CR6: before a bot's own call reads where it is, its car is settled if it has arrived. Gives back
/// the drive it's still on, if it is.
pub fn current_drive(state: &AppState, agent_id: &str, now: i64) -> ApiResult<Option<Drive>> {
    let Some(drive) = state.db.drive_of(agent_id)? else {
        return Ok(None);
    };
    if drive.arrive_at > now {
        return Ok(Some(drive));
    }
    settle(state, &drive, now);
    Ok(None)
}

/// CR6: what a bot is told when it tries to do something in the room it's driving to.
pub fn still_driving(state: &AppState, drive: &Drive, now: i64) -> ApiError {
    let name = state.db.hangout(&drive.hangout_id).ok().flatten().map(|room| room.name);
    ApiError::conflict(format!(
        "You're driving to {}, there in {} s: you can do things once you arrive",
        name.as_deref().unwrap_or("that room"),
        (drive.arrive_at - now).max(1)
    ))
}

/// CR11: what `go_to_room` says about a drive that began: the car, the second it arrives and how
/// long that is.
pub fn drive_json(drive: &Drive) -> Value {
    let car = cars::find(&drive.car_id).map(|car| car.name);
    json!({
        "car": car,
        "seconds": drive.arrive_at - drive.leave_at,
        "arrives_at": ops::iso(drive.arrive_at),
        "note": "you're on the road: you're in the room once you arrive, and until then it won't take what you do there",
    })
}

/// CR6: once every 2 seconds, from startup, the cars that have arrived put their bots in the room.
pub async fn run_drives(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(2));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker = state.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || settle_trips(&worker, now())).await
        {
            tracing::error!(%error, "the drive worker failed");
        }
    }
}
