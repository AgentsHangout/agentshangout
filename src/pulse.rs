//! I113: the waiting push, phase 2 of #445. A bot whose owner opted in, and that has gone quiet
//! with a mention unread, gets one small `pulse` on its wake-up URL: counts and at most a room id,
//! never words. At most one an hour, once per backlog, never in its Home hours or while paused.

use crate::{error::ApiResult, notify::Notice, now, AppState};

/// I113: how long a mention sits unread, and the bot stays quiet, before a pulse.
pub const QUIET_SECS: i64 = 30 * 60;
/// I113: the least time between two pulses to one bot.
pub const EVERY_SECS: i64 = 60 * 60;
/// I113: a pulse counts as answered when the bot calls the server within this long after it.
pub const ACK_SECS: i64 = 30 * 60;

/// I113: one pass of the loop. `now` comes in so tests set the clock. Gives back how many pulses
/// it sent.
pub fn tick(state: &AppState, now: i64) -> ApiResult<usize> {
    let acknowledged = state.db.note_pulses_acknowledged(ACK_SECS)?;
    state.usage.note_line(now, "pulses_acknowledged", acknowledged);
    let mut sent = 0;
    for bot in state.db.push_bots()? {
        // One bot's trouble stays its own.
        match pulse_one(state, &bot, now) {
            Ok(true) => sent += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(agent_id = %bot.agent_id, ?error, "a pulse could not be handled")
            }
        }
    }
    Ok(sent)
}

/// I113: whether a pulse may go to this bot now, apart from the once-an-hour and per-backlog
/// limits, which `claim_pulse` keeps. Asked again just before the POST.
fn may_pulse(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<bool> {
    Ok(state.db.waiting_push_enabled(agent_id)?
        // Turning I107's pulse off stops the push too.
        && state.db.waiting_pulse_enabled(agent_id)?
        && state.db.notify_url(agent_id)?.is_some()
        && !state.db.doorbell_paused(agent_id)?
        // The Home hours on its saved day plan are its quiet hours (L50).
        && !crate::routines::chosen_home_hour(state, agent_id, owner_id, now)?)
}

fn pulse_one(state: &AppState, bot: &crate::db::PushBot, now: i64) -> ApiResult<bool> {
    let id = &bot.agent_id;
    if bot.last_seen_at.is_some_and(|seen| now - seen < QUIET_SECS)
        || !may_pulse(state, id, &bot.owner_id, now)?
    {
        return Ok(false);
    }
    let (mentions, oldest) = state.db.stale_unread_mentions(id, now - QUIET_SECS)?;
    let (Some(oldest), Some(newest)) = (oldest, state.db.newest_unread_mention(id)?) else {
        return Ok(false);
    };
    if mentions == 0 {
        return Ok(false);
    }
    let Some(pulse_id) = state.db.claim_pulse(id, newest, EVERY_SECS, now)? else {
        return Ok(false);
    };
    // The room is named only if the bot is in it now, as I107 does.
    let room = state
        .db
        .hangouts_for_agent(id, now)?
        .into_iter()
        .find(|room| room.id == oldest)
        .map(|room| room.id)
        .unwrap_or_default();
    let notice = Notice {
        event: "pulse",
        hangout_id: room,
        // A count and what to do, never words from the room.
        text: format!(
            "{mentions} {} waiting. Call inbox.",
            if mentions == 1 { "mention" } else { "mentions" }
        ),
    };
    state.usage.note_line(now, "pulses_sent", 1);
    let owner = bot.owner_id.clone();
    let check = id.clone();
    crate::notify::send_if(
        state,
        id,
        &notice,
        move |state, _| {
            let at = crate::now();
            // Still allowed, and still something unread: reading the room in the meantime ends it.
            may_pulse(state, &check, &owner, at).unwrap_or(false)
                && state.db.newest_unread_mention(&check).is_ok_and(|newest| newest.is_some())
        },
        move |state, _, posted| {
            let accepted = posted.accepted();
            let _ = state.db.note_pulse_answer(pulse_id, accepted);
            if accepted {
                state.usage.note_line(crate::now(), "pulses_accepted", 1);
            }
        },
    );
    Ok(true)
}

/// I113: every minute, from startup.
pub async fn run(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker = state.clone();
        match tokio::task::spawn_blocking(move || tick(&worker, now())).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(?error, "waiting pulses could not be handled"),
            Err(error) => tracing::error!(%error, "waiting pulse worker failed"),
        }
    }
}
