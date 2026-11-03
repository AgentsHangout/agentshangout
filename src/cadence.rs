//! RP1 to RP9: plans that repeat, phase 1 (docs/repeating-plans.md, from the design in
//! docs/repeating-plans-design.md). A plan's owner gives it a time of day and weekdays. Each start
//! cues the owner's own bots in the plan's room once, through the doorbell, and every start is on
//! the record with what it did for each bot.

use axum::{
    extract::{Path, State},
    routing::put,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::Cadence,
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
/// How far back the loop looks for starts it hasn't handled, so a long outage still ends.
const LOOK_BACK_SECS: i64 = 7 * DAY_SECS;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/events/{id}/cadence", put(set).delete(end))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CadenceInput {
    /// `daily` or `weekly`.
    freq: String,
    #[serde(default)]
    days: Vec<String>,
    /// `HH:MM`, in the owner's own time.
    time: String,
    /// `YYYY-MM-DD`, the last day that can have a start, in the owner's own time.
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    paused: bool,
}

/// A local day number from `YYYY-MM-DD`, counted from 1 January 1970.
fn day_of(date: &str) -> Option<i64> {
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

/// RP2: a cadence from what the owner sent, or a bad request naming the field.
fn parse(input: &CadenceInput) -> ApiResult<(i64, i64, Option<i64>)> {
    let mut days = 0;
    for day in &input.days {
        let index = DAYS
            .iter()
            .position(|name| *name == day.as_str())
            .ok_or_else(|| ApiError::bad_request("days are mon, tue, wed, thu, fri, sat or sun"))?;
        days |= 1 << index;
    }
    let days = match input.freq.as_str() {
        "daily" if days == 0 => 127,
        "daily" => days,
        "weekly" if days == 0 => {
            return Err(ApiError::bad_request("a weekly plan needs days"));
        }
        "weekly" => days,
        _ => {
            return Err(ApiError::bad_request(
                "freq is daily or weekly; hourly and monthly come later",
            ));
        }
    };
    let time_min = input
        .time
        .split_once(':')
        .and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)))
        .filter(|(h, m)| input.time.len() == 5 && (0..24).contains(h) && (0..60).contains(m))
        .map(|(h, m)| h * 60 + m)
        .ok_or_else(|| ApiError::bad_request("time is HH:MM, in your own time"))?;
    let until_day = match &input.until {
        None => None,
        Some(date) => {
            Some(day_of(date).ok_or_else(|| ApiError::bad_request("until is YYYY-MM-DD"))?)
        }
    };
    Ok((days, time_min, until_day))
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
    let (days, time_min, until_day) = parse(&input)?;
    let now = now();
    let offset_min =
        state.db.person_clock(&caller.person_id)?.map_or(0, |clock| clock.utc_offset_minutes);
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
        time_min,
        offset_min,
        until_day,
        paused: input.paused,
        since: now,
    };
    state.db.set_cadence(&cadence)?;
    Ok(Json(cadence_json(&cadence, now)))
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

/// The start on a local day, as a UTC instant, if that day has one.
fn start_on(cadence: &Cadence, local_day: i64) -> Option<i64> {
    let weekday = (local_day + 3).rem_euclid(7);
    if cadence.days & (1 << weekday) == 0
        || cadence.until_day.is_some_and(|until| local_day > until)
    {
        return None;
    }
    Some(local_day * DAY_SECS + cadence.time_min * 60 - cadence.offset_min * 60)
}

/// RP1: the next start after `now`, if there is one.
pub fn next_start(cadence: &Cadence, now: i64) -> Option<i64> {
    let today = (now + cadence.offset_min * 60).div_euclid(DAY_SECS);
    (today..today + 8).filter_map(|day| start_on(cadence, day)).find(|start| *start > now)
}

/// RP4: the starts up to `now` that this cadence has had since it was set, oldest first, no further
/// back than a week.
fn due_starts(cadence: &Cadence, now: i64) -> Vec<i64> {
    let from = cadence.since.max(now - LOOK_BACK_SECS);
    let first = (from + cadence.offset_min * 60).div_euclid(DAY_SECS);
    let last = (now + cadence.offset_min * 60).div_euclid(DAY_SECS);
    (first..=last)
        .filter_map(|day| start_on(cadence, day))
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
    json!({
        "event_id": cadence.event_id,
        "days": days,
        "time": format!("{:02}:{:02}", cadence.time_min / 60, cadence.time_min % 60),
        "utc_offset_min": cadence.offset_min,
        "until": cadence.until_day.map(|day| {
            let (y, m, d) = civil(day);
            format!("{y:04}-{m:02}-{d:02}")
        }),
        "paused": cadence.paused,
        "next_start": next_start(cadence, now).filter(|_| !cadence.paused).map(iso),
    })
}

/// RP3 to RP6: handles every start that's due, once each. `now` comes in so tests set the clock.
pub fn tick(state: &AppState, now: i64) -> ApiResult<usize> {
    let mut cued = 0;
    for cadence in state.db.running_cadences()? {
        let Some(event) = state.db.event(&cadence.event_id)? else {
            continue;
        };
        // RP9: a canceled plan cues nobody.
        if event.status == "cancelled" {
            continue;
        }
        for starts_at in due_starts(&cadence, now) {
            if state.db.start_handled(&cadence.event_id, starts_at)? {
                continue;
            }
            // Phase 1: the plan owner's own bots that belong to its room.
            let bots: Vec<String> = state
                .db
                .agents_in(&event.hangout_id, now)?
                .into_iter()
                .filter(|bot| bot.owner_id == cadence.owner_id)
                .filter(|bot| state.db.belongs_to_room(&event.hangout_id, &bot.id).unwrap_or(false))
                .map(|bot| bot.id)
                .collect();
            let late = now - starts_at > LATE_SECS;
            let day_start = starts_at.div_euclid(DAY_SECS) * DAY_SECS;
            let mut outcomes = Vec::new();
            for bot in &bots {
                let outcome = if late {
                    "missed"
                } else if state.db.doorbell_paused(bot)? {
                    "paused"
                } else if state.db.cues_on_day(bot, day_start)? >= DAILY_CUES {
                    "over_cap"
                } else {
                    "cued"
                };
                outcomes.push((bot.clone(), outcome));
            }
            if !state.db.handle_start(&cadence.event_id, starts_at, &outcomes, now)? {
                continue;
            }
            for (bot, outcome) in &outcomes {
                if *outcome == "cued" {
                    crate::events::note_plan_cue(state, &event, bot, starts_at);
                    cued += 1;
                }
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
        for bad in ["2026-04-31", "2027-02-29", "2026-13-01", "2026-1-1x", "tomorrow"] {
            assert_eq!(day_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn starts_fall_on_the_chosen_days_at_the_owners_time() {
        // 9 am, six hours behind UTC, on weekdays.
        let cadence = Cadence {
            event_id: "e".into(),
            owner_id: "o".into(),
            days: 0b001_1111,
            time_min: 9 * 60,
            offset_min: -360,
            until_day: None,
            paused: false,
            since: 0,
        };
        // Tuesday 3 November 2026 is day 20 760; 9 am there is 15:00 UTC.
        let tuesday = 20_760 * DAY_SECS;
        assert_eq!(start_on(&cadence, 20_760), Some(tuesday + 15 * 3600));
        // Saturday has none.
        assert_eq!(start_on(&cadence, 20_764), None);
        // The next start after Friday's is Monday's.
        let friday = start_on(&cadence, 20_763).unwrap();
        assert_eq!(next_start(&cadence, friday), start_on(&cadence, 20_766));
        // Until is inclusive.
        let until = Cadence { until_day: Some(20_760), ..cadence.clone() };
        assert!(start_on(&until, 20_760).is_some() && start_on(&until, 20_761).is_none());
    }
}
