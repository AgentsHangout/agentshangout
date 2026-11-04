//! I106: a closing clock's time comes, and the visitors it names are sent out. Run at startup and
//! every minute, as `recycling::run_sweep` is.

use crate::{now, AppState};

/// Sends out whoever walked onto a floor whose closing clock is due, then takes the time off the
/// clock so it rings once. Returns how many it sent. Each leaves with a reason (S62), and the
/// minute look says the goodbyes (S71).
pub fn sweep(state: &AppState, at: i64) -> rusqlite::Result<usize> {
    let mut sent = 0;
    for clock in state.db.due_closing_clocks(at)? {
        for (who, bot) in state.db.floor_walkers(&clock.hangout_id, at)? {
            let named =
                clock.everyone || bot.as_ref().is_some_and(|bot| clock.bot_ids.contains(bot));
            if named
                && state.db.end_visit_as(&clock.hangout_id, &who, at, "the closing clock rang")?
            {
                sent += 1;
            }
        }
        state.db.clear_closing_clock(&clock.item_id)?;
        crate::ops::refresh(state, &clock.hangout_id);
    }
    Ok(sent)
}

pub async fn run_sweep(state: AppState) {
    let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let worker_state = state.clone();
        match tokio::task::spawn_blocking(move || sweep(&worker_state, now())).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::error!(%error, "closing clocks could not be rung"),
            Err(error) => tracing::error!(%error, "closing clock worker failed"),
        }
    }
}
