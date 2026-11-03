//! RP1 to RP9: plans that repeat, and what each start did for each bot (docs/repeating-plans.md).
use super::*;

/// RP1: how a plan repeats, in phase 1: one time a day, on chosen weekdays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub event_id: String,
    pub owner_id: String,
    /// Bits from Monday (1) to Sunday (64). Every day is 127.
    pub days: i64,
    /// Minutes after the owner's midnight.
    pub time_min: i64,
    /// The owner's saved offset from UTC when it was set (B2).
    pub offset_min: i64,
    /// The last local day, counted from 1 January 1970, that can have a start. Inclusive.
    pub until_day: Option<i64>,
    pub paused: bool,
    /// No start before this is handled: when it was set or last changed.
    pub since: i64,
}

fn cadence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Cadence> {
    Ok(Cadence {
        event_id: row.get(0)?,
        owner_id: row.get(1)?,
        days: row.get(2)?,
        time_min: row.get(3)?,
        offset_min: row.get(4)?,
        until_day: row.get(5)?,
        paused: row.get(6)?,
        since: row.get(7)?,
    })
}

/// RP5: one of a bot's starts: the plan's id, title and room, when it started, and what it did.
pub type BotStart = (String, String, String, i64, String);

const CADENCE_COLS: &str =
    "event_id, owner_id, days, time_min, offset_min, until_day, paused, since";

impl Db {
    /// RP1: the plan's cadence, if it repeats.
    pub fn cadence(&self, event_id: &str) -> rusqlite::Result<Option<Cadence>> {
        self.conn()
            .query_row(
                &format!("SELECT {CADENCE_COLS} FROM plan_cadences WHERE event_id = ?"),
                [event_id],
                cadence_from_row,
            )
            .optional()
    }

    /// RP1: every cadence that isn't paused, for the loop.
    pub fn running_cadences(&self) -> rusqlite::Result<Vec<Cadence>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {CADENCE_COLS} FROM plan_cadences WHERE paused = 0 ORDER BY event_id"
        ))?;
        let rows = stmt.query_map([], cadence_from_row)?;
        rows.collect()
    }

    /// RP1: how many repeating plans a person has.
    pub fn cadences_of(&self, owner_id: &str) -> rusqlite::Result<i64> {
        self.conn().query_row(
            "SELECT COUNT(*) FROM plan_cadences WHERE owner_id = ?",
            [owner_id],
            |row| row.get(0),
        )
    }

    /// RP1: sets or replaces a plan's cadence.
    pub fn set_cadence(&self, cadence: &Cadence) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO plan_cadences
                 (event_id, owner_id, days, time_min, offset_min, until_day, paused, since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (event_id) DO UPDATE SET days = excluded.days,
                 time_min = excluded.time_min, offset_min = excluded.offset_min,
                 until_day = excluded.until_day, paused = excluded.paused, since = excluded.since",
            params![
                cadence.event_id,
                cadence.owner_id,
                cadence.days,
                cadence.time_min,
                cadence.offset_min,
                cadence.until_day,
                cadence.paused,
                cadence.since
            ],
        )?;
        Ok(())
    }

    /// RP9: the plan stops repeating. Past starts stay on the record.
    pub fn end_cadence(&self, event_id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("DELETE FROM plan_cadences WHERE event_id = ?", [event_id])? > 0)
    }

    /// RP4, RP6: takes a start and writes each bot's outcome, once, and gives back the outcomes as
    /// written. None if another tick or server had already taken that start, and then nothing is
    /// written. A `cued` past `daily_cap` cues that UTC day becomes `over_cap`, counted in the same
    /// immediate transaction, so two servers on two plans can't both pass the cap.
    pub fn handle_start(
        &self,
        event_id: &str,
        starts_at: i64,
        outcomes: &[(String, &'static str)],
        daily_cap: i64,
        now: i64,
    ) -> rusqlite::Result<Option<Vec<(String, &'static str)>>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let taken = tx.execute(
            "INSERT OR IGNORE INTO plan_starts (event_id, starts_at, handled_at) VALUES (?, ?, ?)",
            params![event_id, starts_at, now],
        )?;
        if taken == 0 {
            return Ok(None);
        }
        let day_start = starts_at.div_euclid(86_400) * 86_400;
        let mut written = Vec::with_capacity(outcomes.len());
        for (agent_id, outcome) in outcomes {
            let mut outcome = *outcome;
            if outcome == "cued" {
                let cued: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM plan_start_bots
                     WHERE agent_id = ? AND outcome = 'cued' AND starts_at >= ? AND starts_at < ?",
                    params![agent_id, day_start, day_start + 86_400],
                    |row| row.get(0),
                )?;
                if cued >= daily_cap {
                    outcome = "over_cap";
                }
            }
            tx.execute(
                "INSERT INTO plan_start_bots (event_id, starts_at, agent_id, outcome)
                 VALUES (?, ?, ?, ?)",
                params![event_id, starts_at, agent_id, outcome],
            )?;
            written.push((agent_id.clone(), outcome));
        }
        tx.commit()?;
        Ok(Some(written))
    }

    /// RP4: whether this start of this plan has been handled.
    pub fn start_handled(&self, event_id: &str, starts_at: i64) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM plan_starts WHERE event_id = ? AND starts_at = ?",
            params![event_id, starts_at],
        )
    }

    /// RP3: a cue rang the bot. Two plans starting together ring it once (E22's three minutes).
    pub fn note_cue_rung(
        &self,
        event_id: &str,
        starts_at: i64,
        agent_id: &str,
        at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE plan_start_bots SET rung_at = ? WHERE event_id = ? AND starts_at = ? AND agent_id = ?",
            params![at, event_id, starts_at, agent_id],
        )?;
        Ok(())
    }

    /// RP3: whether a cue rang this bot since `since`.
    pub fn cue_rung_since(&self, agent_id: &str, since: i64) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM plan_start_bots WHERE agent_id = ? AND rung_at > ?",
            params![agent_id, since],
        )
    }

    /// RP6: how many times a bot was cued in the UTC day that holds `day_start`.
    pub fn cues_on_day(&self, agent_id: &str, day_start: i64) -> rusqlite::Result<i64> {
        self.conn().query_row(
            "SELECT COUNT(*) FROM plan_start_bots
             WHERE agent_id = ? AND outcome = 'cued' AND starts_at >= ? AND starts_at < ?",
            params![agent_id, day_start, day_start + 86_400],
            |row| row.get(0),
        )
    }

    /// RP5: this bot's starts since `since`, newest first, with each plan's title and room.
    pub fn starts_of_bot(&self, agent_id: &str, since: i64) -> rusqlite::Result<Vec<BotStart>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT s.event_id, e.title, e.hangout_id, s.starts_at, s.outcome
               FROM plan_start_bots s JOIN events e ON e.id = s.event_id
              WHERE s.agent_id = ? AND s.starts_at >= ?
              ORDER BY s.starts_at DESC, s.event_id LIMIT 20",
        )?;
        let rows = stmt.query_map(params![agent_id, since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect()
    }
}
