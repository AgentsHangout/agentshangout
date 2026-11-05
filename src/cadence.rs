//! RP1 to RP11: plans that repeat (docs/repeating-plans.md, from the design in
//! docs/repeating-plans-design.md). A plan's owner gives it weekdays and times of day, a day of the
//! month, or an hourly window. Each start cues the owner's own bots in the plan's room once, through
//! the doorbell, and any other member's bots there they said yes for (RP7). Every start is on the
//! record with what it did for each bot.

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{Cadence, Shape},
    error::{ApiError, ApiResult},
    now,
    ops::{self, iso, DAY_SECS},
    AppState,
};

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
/// RP5: a start found later than this is missed, not cued.
pub const LATE_SECS: i64 = 15 * 60;
/// RP6: the most a bot is cued in one UTC day, across every plan.
pub const DAILY_CUES: i64 = 24;
/// RP1: the most repeating plans one person has.
pub const MAX_CADENCES: i64 = 20;
/// RP3: after a cue rings a bot, the next cue in this long doesn't ring it again (as E22's hold).
const CUE_RING_SECS: i64 = 3 * 60;
/// RP10: the most times a day a plan can start at.
const MAX_TIMES: usize = 4;
/// RP10: times closer than this would start a plan twice in an hour.
const MIN_GAP_MIN: i64 = 60;
/// How far back the loop looks for starts it hasn't handled, so a long outage still ends.
const LOOK_BACK_SECS: i64 = 7 * DAY_SECS;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/events/{id}/cadence", get(read).put(set).delete(end))
        .route("/api/events/{id}/cue-bots", put(set_cue_bots))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CadenceInput {
    /// `daily`, `weekly`, `monthly` or `hourly`.
    freq: String,
    #[serde(default)]
    days: Vec<String>,
    /// `HH:MM`, in the owner's own time. One time; `times` gives up to four.
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    times: Vec<String>,
    /// RP10: a monthly plan's day, 1 to 28.
    #[serde(default)]
    month_day: Option<i64>,
    /// RP11: an hourly plan's step, in hours.
    #[serde(default)]
    interval: Option<i64>,
    /// RP11: an hourly plan's first and last possible start.
    #[serde(default)]
    window: Option<WindowInput>,
    /// `YYYY-MM-DD`, the last day that can have a start, in the owner's own time.
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    paused: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CueBotsInput {
    bots: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowInput {
    from: String,
    to: String,
}

/// A local day number from `YYYY-MM-DD`, counted from 1 January 1970.
fn day_of(date: &str) -> Option<i64> {
    // Exactly YYYY-MM-DD, so 2026-1-1 isn't read as a date.
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let mut parts = date.split('-');
    let (y, m, d) = (
        parts.next()?.parse::<i64>().ok()?,
        parts.next()?.parse::<i64>().ok()?,
        parts.next()?.parse::<i64>().ok()?,
    );
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let day = era * 146_097 + doe - 719_468;
    // A date like 31 April rolls into May; refuse it rather than move it.
    (civil(day) == (if m <= 2 { y + 1 } else { y }, m, d)).then_some(day)
}

/// The civil date of a day number.
fn civil(day: i64) -> (i64, i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Minutes after midnight from `HH:MM` on a 24-hour clock.
fn minute_of(time: &str) -> Option<i64> {
    time.split_once(':')
        .and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)))
        .filter(|(h, m)| time.len() == 5 && (0..24).contains(h) && (0..60).contains(m))
        .map(|(h, m)| h * 60 + m)
}

/// RP10: one to four times, earliest first, at least an hour apart. When a start day can follow
/// another, the last time of one day and the first of the next must be an hour apart too.
fn parse_times(input: &CadenceInput, next_day_too: bool) -> ApiResult<Vec<i64>> {
    let given: Vec<&String> = input.time.iter().chain(&input.times).collect();
    if input.time.is_some() && !input.times.is_empty() {
        return Err(ApiError::bad_request("send time or times, not both"));
    }
    if given.is_empty() {
        return Err(ApiError::bad_request("time is HH:MM, in your own time"));
    }
    if given.len() > MAX_TIMES {
        return Err(ApiError::bad_request(format!("at most {MAX_TIMES} times a day")));
    }
    let mut times = given
        .into_iter()
        .map(|time| minute_of(time))
        .collect::<Option<Vec<i64>>>()
        .ok_or_else(|| ApiError::bad_request("time is HH:MM, in your own time"))?;
    times.sort_unstable();
    let too_close = times.windows(2).any(|pair| pair[1] - pair[0] < MIN_GAP_MIN)
        || (next_day_too
            && times.len() > 1
            && times[0] + DAY_SECS / 60 - times[times.len() - 1] < MIN_GAP_MIN);
    if too_close {
        return Err(ApiError::bad_request("times are at least an hour apart"));
    }
    Ok(times)
}

/// RP2, RP10, RP11: a cadence from what the owner sent, or a bad request naming the field.
fn parse(input: &CadenceInput) -> ApiResult<(i64, Shape, Option<i64>)> {
    let mut days = 0;
    for day in &input.days {
        let index = DAYS
            .iter()
            .position(|name| *name == day.as_str())
            .ok_or_else(|| ApiError::bad_request("days are mon, tue, wed, thu, fri, sat or sun"))?;
        days |= 1 << index;
    }
    let hourly = input.freq == "hourly";
    if !hourly && input.interval.is_some() {
        return Err(ApiError::bad_request("interval is for hourly plans"));
    }
    if !hourly && input.window.is_some() {
        return Err(ApiError::bad_request("window is for hourly plans"));
    }
    if input.freq != "monthly" && input.month_day.is_some() {
        return Err(ApiError::bad_request("month_day is for monthly plans"));
    }
    // Whether some start day is followed by another, Sunday then Monday included.
    let next_day_too = |days: i64| days & (((days << 1) | (days >> 6)) & 127) != 0;
    let (days, shape) = match input.freq.as_str() {
        "daily" | "weekly" => {
            let weekly = input.freq == "weekly";
            let days = match days {
                0 if weekly => return Err(ApiError::bad_request("a weekly plan needs days")),
                0 => 127,
                days => days,
            };
            (days, Shape::Days { weekly, times: parse_times(input, next_day_too(days))? })
        }
        "monthly" => {
            if days != 0 {
                return Err(ApiError::bad_request("a monthly plan has a month_day, not days"));
            }
            let month_day =
                input.month_day.filter(|day| (1..=28).contains(day)).ok_or_else(|| {
                    ApiError::bad_request("month_day is 1 to 28, so every month has it")
                })?;
            (0, Shape::Monthly { month_day, times: parse_times(input, false)? })
        }
        "hourly" => {
            if input.time.is_some() || !input.times.is_empty() {
                return Err(ApiError::bad_request(
                    "an hourly plan has an interval and a window, not times",
                ));
            }
            let every_hours = input
                .interval
                .filter(|hours| (1..24).contains(hours))
                .ok_or_else(|| ApiError::bad_request("interval is 1 to 23 hours"))?;
            let window = input
                .window
                .as_ref()
                .ok_or_else(|| ApiError::bad_request("an hourly plan needs a window"))?;
            let (from, to) = minute_of(&window.from)
                .zip(minute_of(&window.to))
                .filter(|(from, to)| from < to)
                .ok_or_else(|| {
                    ApiError::bad_request("window is from and to, HH:MM, with to after from")
                })?;
            (if days == 0 { 127 } else { days }, Shape::Hourly { every_hours, from, to })
        }
        _ => return Err(ApiError::bad_request("freq is daily, weekly, monthly or hourly")),
    };
    let until_day = match &input.until {
        None => None,
        Some(date) => {
            Some(day_of(date).ok_or_else(|| ApiError::bad_request("until is YYYY-MM-DD"))?)
        }
    };
    Ok((days, shape, until_day))
}

/// RP1: only the plan's owner, as a person, in its room. The owner's saved offset is copied in, so
/// moving or traveling never shifts it until they set it again.
async fn set(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(input): Json<CadenceInput>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "make a plan repeat")?;
    let event = state.db.event(&id)?.ok_or_else(ApiError::not_found)?;
    ops::no_guests(&state, &caller, &event.hangout_id, "make a plan repeat")?;
    if event.proposed_by != caller.person_id {
        return Err(ApiError::forbidden("only whoever made the plan can make it repeat"));
    }
    if event.status == "cancelled" {
        return Err(ApiError::conflict("this plan was canceled"));
    }
    let (days, shape, until_day) = parse(&input)?;
    let now = now();
    let clock = state.db.person_clock(&caller.person_id)?;
    let offset_min = clock.as_ref().map_or(0, |clock| clock.utc_offset_minutes);
    // RP8: the zone is kept only if the bundled database knows it; otherwise the offset holds.
    let time_zone =
        clock.map(|clock| clock.time_zone).filter(|zone| jiff::tz::TimeZone::get(zone).is_ok());
    if until_day.is_some_and(|until| until < (now + offset_min * 60).div_euclid(DAY_SECS)) {
        return Err(ApiError::bad_request("until is already past"));
    }
    if state.db.cadence(&id)?.is_none() && state.db.cadences_of(&caller.person_id)? >= MAX_CADENCES
    {
        return Err(ApiError::conflict(format!("at most {MAX_CADENCES} repeating plans each")));
    }
    let cadence = Cadence {
        event_id: id.clone(),
        owner_id: caller.person_id.clone(),
        days,
        shape,
        offset_min,
        time_zone,
        until_day,
        paused: input.paused,
        // A start in the last tick's 30 seconds that nothing has handled yet still counts: the key
        // keeps it from being handled twice.
        since: now - 30,
    };
    state.db.set_cadence(&cadence)?;
    let mut reply = cadence_json(&cadence, now);
    if state.db.person_clock(&caller.person_id)?.is_none() {
        reply["clock_note"] = json!("You haven't saved a clock, so this time is in UTC. Open the site once to save yours, then set this again.");
    }
    Ok(Json(reply))
}

/// RP13: what the plan page shows. A member, as a person, reads whether the plan repeats and
/// which of their own bots they said yes for.
async fn read(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "read a plan's repeat")?;
    let event = state.db.event(&id)?.ok_or_else(ApiError::not_found)?;
    ops::no_guests(&state, &caller, &event.hangout_id, "read a plan's repeat")?;
    let cadence = state.db.cadence(&id)?.map(|cadence| cadence_json(&cadence, now()));
    Ok(Json(json!({
        "event_id": id,
        "cadence": cadence,
        "your_bots": state.db.cue_bots_of(&id, &caller.person_id)?,
    })))
}

/// RP9: it stops repeating, and every start so far stays on the record.
async fn end(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "stop a plan repeating")?;
    let event = state.db.event(&id)?.ok_or_else(ApiError::not_found)?;
    if event.proposed_by != caller.person_id {
        return Err(ApiError::forbidden("only whoever made the plan can stop it repeating"));
    }
    if !state.db.end_cadence(&id)? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

/// RP7: another member says yes for their own bots in the plan's room to be cued by it. The list
/// replaces what they said before, and an empty one is a no. It lets no bot into the room.
async fn set_cue_bots(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(input): Json<CueBotsInput>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "say yes for a bot")?;
    let event = state.db.event(&id)?.ok_or_else(ApiError::not_found)?;
    ops::no_guests(&state, &caller, &event.hangout_id, "say yes for a bot")?;
    if event.status == "cancelled" {
        return Err(ApiError::conflict("this plan was canceled"));
    }
    if event.proposed_by == caller.person_id {
        return Err(ApiError::bad_request("your own bots in the room are cued already"));
    }
    let now = now();
    let mut bots = input.bots;
    bots.sort();
    bots.dedup();
    for bot in &bots {
        let agent = state.db.agent(bot, now)?.ok_or_else(ApiError::not_found)?;
        if agent.owner_id != caller.person_id {
            return Err(ApiError::forbidden("you can only say yes for your own bots"));
        }
        if !state.db.belongs_to_room(&event.hangout_id, bot)? {
            return Err(ApiError::bad_request(
                "that bot isn't in this plan's room, and a yes doesn't bring it in",
            ));
        }
    }
    state.db.set_cue_bots(&id, &caller.person_id, &bots, now)?;
    Ok(Json(json!({ "event_id": id, "bots": state.db.cue_bots_of(&id, &caller.person_id)? })))
}

/// RP3, RP7: whether a bot is cued by a plan: Some(true) for its owner's own plan, Some(false) for
/// another member's that its owner said yes for, None if it isn't. Either way it must belong to the
/// room; a yes stands only while its owner is still a member there.
pub(crate) fn cue_kind(
    state: &AppState,
    event_id: &str,
    hangout_id: &str,
    plan_owner: &str,
    bot: &crate::db::Agent,
) -> ApiResult<Option<bool>> {
    if !state.db.belongs_to_room(hangout_id, &bot.id)? {
        return Ok(None);
    }
    if bot.owner_id == plan_owner {
        return Ok(Some(true));
    }
    Ok((state.db.cue_yes(event_id, &bot.id)? && state.db.is_member(hangout_id, &bot.owner_id)?)
        .then_some(false))
}

/// RP7: another owner's plan rings their bot only if its owner hasn't set only_me (E25) and it
/// isn't at its workplace in a Work hour (L55, its owner's call alone).
pub(crate) fn others_cue_may_ring(
    state: &AppState,
    bot: &crate::db::Agent,
    now: i64,
) -> ApiResult<bool> {
    Ok(!state.db.wakes_only_for_owner(&bot.id)?
        && !crate::routines::at_work(state, &bot.id, &bot.owner_id, now)?)
}

/// The starts on a local day, as UTC instants, earliest first.
fn starts_on(cadence: &Cadence, local_day: i64) -> Vec<i64> {
    let on = match cadence.shape {
        Shape::Monthly { month_day, .. } => civil(local_day).2 == month_day,
        Shape::Days { .. } | Shape::Hourly { .. } => {
            cadence.days & (1 << (local_day + 3).rem_euclid(7)) != 0
        }
    };
    if !on || cadence.until_day.is_some_and(|until| local_day > until) {
        return Vec::new();
    }
    let times = cadence.shape.times().into_iter();
    match zone(cadence) {
        Some(zone) => {
            let mut starts: Vec<i64> =
                times.filter_map(|time| zoned_start(&zone, local_day, time)).collect();
            // Two times in a spring-forward gap both start when it ends: once.
            starts.sort_unstable();
            starts.dedup();
            starts
        }
        None => {
            let midnight = local_day * DAY_SECS - cadence.offset_min * 60;
            times.map(|time| midnight + time * 60).collect()
        }
    }
}

/// RP8: the cadence's time zone, if it has one the bundled database knows.
fn zone(cadence: &Cadence) -> Option<jiff::tz::TimeZone> {
    cadence.time_zone.as_deref().and_then(|name| jiff::tz::TimeZone::get(name).ok())
}

/// RP8: when `minute` after midnight on `local_day` happens in `zone`. A time in a spring-forward
/// gap starts at the first instant after it; a time that happens twice in the fall-back hour starts
/// at the earlier of the two.
fn zoned_start(zone: &jiff::tz::TimeZone, local_day: i64, minute: i64) -> Option<i64> {
    use jiff::tz::AmbiguousOffset;
    let (y, m, d) = civil(local_day);
    let at = jiff::civil::DateTime::new(
        i16::try_from(y).ok()?,
        i8::try_from(m).ok()?,
        i8::try_from(d).ok()?,
        i8::try_from(minute / 60).ok()?,
        i8::try_from(minute % 60).ok()?,
        0,
        0,
    )
    .ok()?;
    let instant = match zone.to_ambiguous_timestamp(at).offset() {
        AmbiguousOffset::Unambiguous { offset } | AmbiguousOffset::Fold { before: offset, .. } => {
            offset.to_timestamp(at).ok()?
        }
        // Read with the offset after the gap, the time falls just before it, so the next
        // transition is the gap's end.
        AmbiguousOffset::Gap { after, .. } => {
            zone.following(after.to_timestamp(at).ok()?).next()?.timestamp()
        }
    };
    Some(instant.as_second())
}

/// The cadence's offset from UTC at `at`, in seconds: its zone's then, or the saved one.
fn offset_at(cadence: &Cadence, at: i64) -> i64 {
    zone(cadence)
        .zip(jiff::Timestamp::from_second(at).ok())
        .map_or(cadence.offset_min * 60, |(zone, at)| i64::from(zone.to_offset(at).seconds()))
}

/// RP1: the next start after `now`, if there is one. A monthly plan's is at most a month away.
pub fn next_start(cadence: &Cadence, now: i64) -> Option<i64> {
    let today = (now + offset_at(cadence, now)).div_euclid(DAY_SECS);
    (today..today + 32).flat_map(|day| starts_on(cadence, day)).find(|start| *start > now)
}

/// RP4: the starts up to `now` that this cadence has had since it was set, oldest first, no further
/// back than a week.
fn due_starts(cadence: &Cadence, now: i64) -> Vec<i64> {
    let from = cadence.since.max(now - LOOK_BACK_SECS);
    let first = (from + offset_at(cadence, from)).div_euclid(DAY_SECS);
    let last = (now + offset_at(cadence, now)).div_euclid(DAY_SECS);
    (first..=last)
        .flat_map(|day| starts_on(cadence, day))
        .filter(|start| *start >= from && *start <= now)
        .collect()
}

fn cadence_json(cadence: &Cadence, now: i64) -> Value {
    let days: Vec<&str> = DAYS
        .iter()
        .enumerate()
        .filter(|(i, _)| cadence.days & (1 << i) != 0)
        .map(|(_, name)| *name)
        .collect();
    let clock = |min: i64| format!("{:02}:{:02}", min / 60, min % 60);
    let times: Vec<String> = cadence.shape.times().into_iter().map(clock).collect();
    let mut reply = json!({
        "event_id": cadence.event_id,
        "freq": cadence.shape.freq(),
        "days": days,
        "time": times.first(),
        "times": times,
        "utc_offset_min": cadence.offset_min,
        "time_zone": cadence.time_zone,
        "until": cadence.until_day.map(|day| {
            let (y, m, d) = civil(day);
            format!("{y:04}-{m:02}-{d:02}")
        }),
        "paused": cadence.paused,
        "next_start": next_start(cadence, now).filter(|_| !cadence.paused).map(iso),
    });
    match cadence.shape {
        Shape::Monthly { month_day, .. } => reply["month_day"] = json!(month_day),
        Shape::Hourly { every_hours, from, to } => {
            reply["interval"] = json!(every_hours);
            reply["window"] = json!({ "from": clock(from), "to": clock(to) });
        }
        Shape::Days { .. } => {}
    }
    reply
}

/// RP3 to RP6: handles every start that's due, once each. `now` comes in so tests set the clock.
pub fn tick(state: &AppState, now: i64) -> ApiResult<usize> {
    let mut cued = 0;
    for cadence in state.db.running_cadences()? {
        // One plan's trouble stays its own: it's logged, and the plans after it still run.
        match tick_one(state, &cadence, now) {
            Ok(count) => cued += count,
            Err(error) => {
                tracing::warn!(event_id = %cadence.event_id, ?error, "a repeating plan could not be handled")
            }
        }
    }
    Ok(cued)
}

/// RP3 to RP6 for one plan: its due starts, each handled once.
fn tick_one(state: &AppState, cadence: &Cadence, now: i64) -> ApiResult<usize> {
    let mut cued = 0;
    let Some(event) = state.db.event(&cadence.event_id)? else {
        return Ok(0);
    };
    // RP9: a canceled plan cues nobody.
    if event.status == "cancelled" {
        return Ok(0);
    }
    for starts_at in due_starts(cadence, now) {
        if state.db.start_handled(&cadence.event_id, starts_at)? {
            continue;
        }
        // RP3, RP7: the plan owner's own bots in its room, and others' bots there with a yes.
        let mut bots = Vec::new();
        for bot in state.db.agents_in(&event.hangout_id, now)? {
            if let Some(own) =
                cue_kind(state, &cadence.event_id, &event.hangout_id, &cadence.owner_id, &bot)?
            {
                bots.push((bot, own));
            }
        }
        let late = now - starts_at > LATE_SECS;
        let mut outcomes = Vec::new();
        for (bot, _) in &bots {
            let outcome = if late {
                "missed"
            } else if state.db.doorbell_paused(&bot.id)? {
                "paused"
            } else {
                "cued"
            };
            outcomes.push((bot.id.clone(), outcome));
        }
        // RP6: the cap is counted again inside the start's own transaction.
        let Some(outcomes) =
            state.db.handle_start(&cadence.event_id, starts_at, &outcomes, DAILY_CUES, now)?
        else {
            continue;
        };
        for (bot, outcome) in &outcomes {
            if *outcome == "cued" {
                let Some((agent, own)) = bots.iter().find(|(agent, _)| agent.id == *bot) else {
                    continue;
                };
                // RP3: the ring follows the owner-mention safeguards: none without a wake-up
                // URL (the event still reaches inbox and the doorbell), and none while an earlier
                // ring in that room is unread (E22). A paused bot was already `paused` above.
                let ring = state.db.notify_url(bot)?.is_some()
                    && (*own || others_cue_may_ring(state, agent, now)?)
                    && !ops::owner_ring_held(state, bot, &event.hangout_id, now)?
                    && state.db.claim_cue_ring(
                        &cadence.event_id,
                        starts_at,
                        bot,
                        now - CUE_RING_SECS,
                        now,
                    )?;
                crate::events::note_plan_cue(state, &event, bot, starts_at, ring, *own);
                cued += 1;
            }
        }
    }
    Ok(cued)
}

/// RP4: every 30 seconds, from startup.
pub async fn run(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(30));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker = state.clone();
        match tokio::task::spawn_blocking(move || tick(&worker, now())).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(?error, "repeating plans could not be handled"),
            Err(error) => tracing::error!(%error, "repeating plans worker failed"),
        }
    }
}

/// RP5: this bot's starts in the last day, for `inbox`, so it can say it missed one.
pub fn recent_for_bot(state: &AppState, agent_id: &str, now: i64) -> ApiResult<Vec<Value>> {
    Ok(state
        .db
        .starts_of_bot(agent_id, now - DAY_SECS)?
        .into_iter()
        .map(|(event_id, title, hangout_id, starts_at, outcome)| {
            json!({ "event_id": event_id, "title": title, "hangout_id": hangout_id,
                    "starts_at": iso(starts_at), "outcome": outcome })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_and_impossible_ones_are_refused() {
        assert_eq!(day_of("1970-01-01"), Some(0));
        assert_eq!(day_of("2026-11-03"), Some(20_760));
        assert_eq!(civil(20_760), (2026, 11, 3));
        assert_eq!(day_of("2028-02-29").map(civil), Some((2028, 2, 29)));
        for bad in ["2026-04-31", "2027-02-29", "2026-13-01", "2026-1-1x", "2026-1-01", "tomorrow"]
        {
            assert_eq!(day_of(bad), None, "{bad}");
        }
    }

    fn weekdays_at_nine() -> Cadence {
        // 9 am, six hours behind UTC, on weekdays.
        Cadence {
            event_id: "e".into(),
            owner_id: "o".into(),
            days: 0b001_1111,
            shape: Shape::Days { weekly: true, times: vec![9 * 60] },
            offset_min: -360,
            time_zone: None,
            until_day: None,
            paused: false,
            since: 0,
        }
    }

    /// Every day at `times`, in Chicago's zone (its saved offset says CST).
    fn chicago_daily(times: Vec<i64>) -> Cadence {
        Cadence {
            days: 127,
            shape: Shape::Days { weekly: false, times },
            time_zone: Some("America/Chicago".into()),
            ..weekdays_at_nine()
        }
    }

    fn utc(text: &str) -> i64 {
        text.parse::<jiff::Timestamp>().unwrap().as_second()
    }

    // RP8: 9 am stays 9 am on both sides of each daylight saving change, on fixed dates.
    #[test]
    fn a_start_keeps_its_local_time_across_daylight_saving() {
        let nine = chicago_daily(vec![9 * 60]);
        let day = |date| day_of(date).unwrap();
        // Spring forward on Sunday 8 March 2026: CST (-6) before, CDT (-5) after.
        assert_eq!(starts_on(&nine, day("2026-03-07")), [utc("2026-03-07T15:00:00Z")]);
        assert_eq!(starts_on(&nine, day("2026-03-09")), [utc("2026-03-09T14:00:00Z")]);
        // Fall back on Sunday 1 November 2026.
        assert_eq!(starts_on(&nine, day("2026-10-31")), [utc("2026-10-31T14:00:00Z")]);
        assert_eq!(starts_on(&nine, day("2026-11-02")), [utc("2026-11-02T15:00:00Z")]);
        // Asked across the change, the next start is the local 9 am after it.
        assert_eq!(
            next_start(&nine, utc("2026-03-08T15:30:00Z")),
            Some(utc("2026-03-09T14:00:00Z"))
        );
        // The same plan with only the saved offset drifts to 8 am, as before RP8.
        let fixed = Cadence { time_zone: None, ..nine.clone() };
        assert_eq!(starts_on(&fixed, day("2026-03-09")), [utc("2026-03-09T15:00:00Z")]);
        // A zone the database doesn't know falls back to the saved offset too.
        let unknown = Cadence { time_zone: Some("Mars/Olympus".into()), ..nine };
        assert_eq!(starts_on(&unknown, day("2026-03-09")), [utc("2026-03-09T15:00:00Z")]);
    }

    // RP8: 2:30 am doesn't exist on 8 March 2026 in Chicago, so it starts at 3:00 am CDT.
    #[test]
    fn a_time_in_the_spring_forward_gap_starts_when_the_gap_ends() {
        let gap = day_of("2026-03-08").unwrap();
        let end = utc("2026-03-08T08:00:00Z");
        assert_eq!(starts_on(&chicago_daily(vec![2 * 60 + 30]), gap), [end]);
        // Two times in the gap start once, not twice, beside a time after it.
        let both = chicago_daily(vec![2 * 60, 2 * 60 + 30, 4 * 60]);
        assert_eq!(starts_on(&both, gap), [end, utc("2026-03-08T09:00:00Z")]);
    }

    // RP8: 1:30 am happens twice on 1 November 2026 in Chicago; it starts once, at the first.
    #[test]
    fn a_time_in_the_fall_back_hour_starts_once_at_the_earlier() {
        let fold = day_of("2026-11-01").unwrap();
        let half_past_one = chicago_daily(vec![60 + 30]);
        assert_eq!(starts_on(&half_past_one, fold), [utc("2026-11-01T06:30:00Z")]);
        // The loop over that whole day finds it once.
        let day_start = utc("2026-11-01T05:00:00Z");
        let cadence = Cadence { since: day_start, ..half_past_one };
        assert_eq!(
            due_starts(&cadence, utc("2026-11-02T05:59:00Z")),
            [utc("2026-11-01T06:30:00Z")]
        );
    }

    #[test]
    fn starts_fall_on_the_chosen_days_at_the_owners_time() {
        let cadence = weekdays_at_nine();
        // Tuesday 3 November 2026 is day 20 760; 9 am there is 15:00 UTC.
        let tuesday = 20_760 * DAY_SECS;
        assert_eq!(starts_on(&cadence, 20_760), vec![tuesday + 15 * 3600]);
        // Saturday has none.
        assert!(starts_on(&cadence, 20_764).is_empty());
        // The next start after Friday's is Monday's.
        let friday = starts_on(&cadence, 20_763)[0];
        assert_eq!(next_start(&cadence, friday), Some(starts_on(&cadence, 20_766)[0]));
        // Until is inclusive.
        let until = Cadence { until_day: Some(20_760), ..cadence.clone() };
        assert!(!starts_on(&until, 20_760).is_empty() && starts_on(&until, 20_761).is_empty());
    }

    // RP10: several times a day, and a day of the month.
    #[test]
    fn several_times_a_day_and_monthly_days_start_at_each_time() {
        let twice = Cadence {
            shape: Shape::Days { weekly: false, times: vec![9 * 60, 13 * 60 + 30] },
            days: 127,
            ..weekdays_at_nine()
        };
        let tuesday = 20_760 * DAY_SECS;
        assert_eq!(
            starts_on(&twice, 20_760),
            vec![tuesday + 15 * 3600, tuesday + 19 * 3600 + 30 * 60]
        );
        // The next start after the 9 am one is the 1:30 pm one, the same day.
        assert_eq!(next_start(&twice, tuesday + 15 * 3600), Some(tuesday + 19 * 3600 + 1800));

        let monthly = Cadence {
            days: 0,
            shape: Shape::Monthly { month_day: 15, times: vec![10 * 60] },
            ..weekdays_at_nine()
        };
        // 15 November 2026 is a Sunday, and a monthly plan doesn't care.
        let fifteenth = day_of("2026-11-15").unwrap();
        assert_eq!(starts_on(&monthly, fifteenth), vec![fifteenth * DAY_SECS + 16 * 3600]);
        assert!(starts_on(&monthly, fifteenth + 1).is_empty());
        // From the 16th, the next is 15 December, 30 days on.
        let december = day_of("2026-12-15").unwrap();
        assert_eq!(
            next_start(&monthly, (fifteenth + 1) * DAY_SECS),
            Some(december * DAY_SECS + 16 * 3600)
        );
    }

    // RP11: anchored at the window's start and stepped by the interval, the end included.
    #[test]
    fn hourly_starts_step_from_the_window_start() {
        let lab = Cadence {
            shape: Shape::Hourly { every_hours: 3, from: 9 * 60, to: 17 * 60 },
            ..weekdays_at_nine()
        };
        let tuesday = 20_760 * DAY_SECS + 6 * 3600;
        assert_eq!(
            starts_on(&lab, 20_760),
            vec![tuesday + 9 * 3600, tuesday + 12 * 3600, tuesday + 15 * 3600]
        );
        let four = Cadence {
            shape: Shape::Hourly { every_hours: 4, from: 9 * 60, to: 17 * 60 },
            ..lab.clone()
        };
        assert_eq!(starts_on(&four, 20_760).last(), Some(&(tuesday + 17 * 3600)));
        assert!(starts_on(&lab, 20_764).is_empty());
    }
}
