//! RP1 to RP9: plans that repeat, and what each start did for each bot (docs/repeating-plans.md).
use super::*;

/// RP1: how a plan repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub event_id: String,
    pub owner_id: String,
    /// Bits from Monday (1) to Sunday (64). Every day is 127. A monthly plan has none.
    pub days: i64,
    pub shape: Shape,
    /// The owner's saved offset from UTC when it was set (B2).
    pub offset_min: i64,
    /// The last local day, counted from 1 January 1970, that can have a start. Inclusive.
    pub until_day: Option<i64>,
    pub paused: bool,
    /// No start before this is handled: when it was set or last changed.
    pub since: i64,
}

/// RP1, RP10, RP11: when in a day a plan starts. Times are minutes after the owner's midnight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// On `days`, at each of 1 to 4 times, earliest first. `weekly` only says how it was asked for.
    Days { weekly: bool, times: Vec<i64> },
    /// On this day of the month, 1 to 28, at each of 1 to 4 times, earliest first.
    Monthly { month_day: i64, times: Vec<i64> },
    /// On `days`, every `every_hours` hours from `from`, while it's no later than `to`.
    Hourly { every_hours: i64, from: i64, to: i64 },
}

impl Shape {
    pub fn freq(&self) -> &'static str {
        match self {
            Shape::Days { weekly: false, .. } => "daily",
            Shape::Days { weekly: true, .. } => "weekly",
            Shape::Monthly { .. } => "monthly",
            Shape::Hourly { .. } => "hourly",
        }
    }

    /// Its starts in one day, earliest first.
    pub fn times(&self) -> Vec<i64> {
        match self {
            Shape::Days { times, .. } | Shape::Monthly { times, .. } => times.clone(),
            Shape::Hourly { every_hours, from, to } => {
                // RP11 keeps it 1 or more; max(1) only stops a bad row from panicking the loop.
                (*from..=*to).step_by((*every_hours).max(1) as usize * 60).collect()
            }
        }
    }
}

type ShapeRow = (String, i64, String, Option<i64>, Option<i64>, Option<i64>, Option<i64>, i64);

fn cadence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Cadence> {
    let mut days: i64 = row.get(2)?;
    let time_min: i64 = row.get(3)?;
    let since: i64 = row.get(7)?;
    let stored: Option<ShapeRow> = match row.get::<_, Option<String>>(8)? {
        Some(freq) => Some((
            freq,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
            row.get(14)?,
            row.get(15)?,
        )),
        None => None,
    };
    let shape = match stored {
        // A shape set by this binary, and not since changed by an older one.
        Some((freq, shape_days, times, month_day, every_hours, from, to, shape_since))
            if shape_since == since =>
        {
            days = shape_days;
            let times: Vec<i64> = times.split(',').filter_map(|t| t.parse().ok()).collect();
            match (freq.as_str(), month_day, every_hours, from, to) {
                ("monthly", Some(month_day), ..) => Shape::Monthly { month_day, times },
                ("hourly", _, Some(every_hours), Some(from), Some(to)) => {
                    Shape::Hourly { every_hours, from, to }
                }
                (freq, ..) => Shape::Days { weekly: freq == "weekly", times },
            }
        }
        // Phase 1's one time a day.
        _ => Shape::Days { weekly: days != 127, times: vec![time_min] },
    };
    Ok(Cadence {
        event_id: row.get(0)?,
        owner_id: row.get(1)?,
        days,
        shape,
        offset_min: row.get(4)?,
        until_day: row.get(5)?,
        paused: row.get(6)?,
        since,
    })
}

/// RP5: one of a bot's starts: the plan's id, title and room, when it started, and what it did.
pub type BotStart = (String, String, String, i64, String);

const CADENCE_COLS: &str = "c.event_id, c.owner_id, c.days, c.time_min, c.offset_min, c.until_day,
     c.paused, c.since, s.freq, s.days, s.times, s.month_day, s.every_hours, s.window_from,
     s.window_to, s.since";
const CADENCE_FROM: &str =
    "plan_cadences c LEFT JOIN plan_cadence_shapes s ON s.event_id = c.event_id";

impl Db {
    /// RP1: the plan's cadence, if it repeats.
    pub fn cadence(&self, event_id: &str) -> rusqlite::Result<Option<Cadence>> {
        self.conn()
            .query_row(
                &format!("SELECT {CADENCE_COLS} FROM {CADENCE_FROM} WHERE c.event_id = ?"),
                [event_id],
                cadence_from_row,
            )
            .optional()
    }

    /// RP1: every cadence that isn't paused, for the loop.
    pub fn running_cadences(&self) -> rusqlite::Result<Vec<Cadence>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {CADENCE_COLS} FROM {CADENCE_FROM} WHERE c.paused = 0 ORDER BY c.event_id"
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

    /// RP1: sets or replaces a plan's cadence, and its shape in the same transaction.
    pub fn set_cadence(&self, cadence: &Cadence) -> rusqlite::Result<()> {
        let times = cadence.shape.times();
        // What a binary from before RP10 reads: the first time, and no days for a shape it can't
        // follow, so it cues nothing rather than the wrong thing.
        let legacy_days = match cadence.shape {
            Shape::Days { .. } => cadence.days,
            Shape::Monthly { .. } | Shape::Hourly { .. } => 0,
        };
        let (month_day, every_hours, from, to) = match cadence.shape {
            Shape::Days { .. } => (None, None, None, None),
            Shape::Monthly { month_day, .. } => (Some(month_day), None, None, None),
            Shape::Hourly { every_hours, from, to } => {
                (None, Some(every_hours), Some(from), Some(to))
            }
        };
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO plan_cadences
                 (event_id, owner_id, days, time_min, offset_min, until_day, paused, since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (event_id) DO UPDATE SET days = excluded.days,
                 time_min = excluded.time_min, offset_min = excluded.offset_min,
                 until_day = excluded.until_day, paused = excluded.paused, since = excluded.since",
            params![
                cadence.event_id,
                cadence.owner_id,
                legacy_days,
                times.first().copied().unwrap_or(0),
                cadence.offset_min,
                cadence.until_day,
                cadence.paused,
                cadence.since
            ],
        )?;
        tx.execute(
            "INSERT INTO plan_cadence_shapes (event_id, freq, days, times, month_day, every_hours,
                 window_from, window_to, since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (event_id) DO UPDATE SET freq = excluded.freq, days = excluded.days,
                 times = excluded.times, month_day = excluded.month_day,
                 every_hours = excluded.every_hours, window_from = excluded.window_from,
                 window_to = excluded.window_to, since = excluded.since",
            params![
                cadence.event_id,
                cadence.shape.freq(),
                cadence.days,
                times.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
                month_day,
                every_hours,
                from,
                to,
                cadence.since
            ],
        )?;
        tx.commit()
    }

    /// RP7: replaces the bots `owner_id` said yes for on this plan. An empty list is a no.
    pub fn set_cue_bots(
        &self,
        event_id: &str,
        owner_id: &str,
        agent_ids: &[String],
        now: i64,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM plan_cue_bots WHERE event_id = ? AND owner_id = ?",
            params![event_id, owner_id],
        )?;
        for agent_id in agent_ids {
            tx.execute(
                "INSERT OR IGNORE INTO plan_cue_bots (event_id, agent_id, owner_id, said_at)
                 VALUES (?, ?, ?, ?)",
                params![event_id, agent_id, owner_id, now],
            )?;
        }
        tx.commit()
    }

    /// RP7: the bots `owner_id` said yes for on this plan.
    pub fn cue_bots_of(&self, event_id: &str, owner_id: &str) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT agent_id FROM plan_cue_bots WHERE event_id = ? AND owner_id = ?
             ORDER BY agent_id",
        )?;
        let rows = stmt.query_map(params![event_id, owner_id], |row| row.get(0))?;
        rows.collect()
    }

    /// RP7: whether this bot's owner, as it is now, said yes for it on this plan.
    pub fn cue_yes(&self, event_id: &str, agent_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM plan_cue_bots c JOIN agents a ON a.id = c.agent_id
              WHERE c.event_id = ? AND c.agent_id = ? AND a.owner_id = c.owner_id",
            params![event_id, agent_id],
        )
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

    /// RP3: claims this start's ring for the bot, unless a cue rang it since `since`. Checked and
    /// written in one immediate transaction, so two ticks can't both ring it.
    pub fn claim_cue_ring(
        &self,
        event_id: &str,
        starts_at: i64,
        agent_id: &str,
        since: i64,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if exists(
            &tx,
            "SELECT 1 FROM plan_start_bots WHERE agent_id = ? AND rung_at > ?",
            params![agent_id, since],
        )? {
            return Ok(false);
        }
        tx.execute(
            "UPDATE plan_start_bots SET rung_at = ? WHERE event_id = ? AND starts_at = ? AND agent_id = ?",
            params![at, event_id, starts_at, agent_id],
        )?;
        tx.commit()?;
        Ok(true)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn monthly(since: i64) -> Cadence {
        Cadence {
            event_id: "e".into(),
            owner_id: "o".into(),
            days: 0,
            shape: Shape::Monthly { month_day: 1, times: vec![600, 900] },
            offset_min: 0,
            until_day: None,
            paused: false,
            since,
        }
    }

    // RP10: a binary from before the shapes reads no days for a monthly plan, so it cues nothing;
    // and once it changes the cadence, its own one time a day counts, not the stale shape.
    #[test]
    fn an_older_binary_reads_a_safe_cadence_and_its_edits_win() {
        let db = Db::open(":memory:").unwrap();
        db.conn().pragma_update(None, "foreign_keys", false).unwrap();
        db.set_cadence(&monthly(100)).unwrap();
        assert_eq!(db.cadence("e").unwrap(), Some(monthly(100)));
        let (days, time_min): (i64, i64) = db
            .conn()
            .query_row("SELECT days, time_min FROM plan_cadences", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((days, time_min), (0, 600));

        // What the older binary's set_cadence writes: its own columns only.
        db.conn()
            .execute("UPDATE plan_cadences SET days = 127, time_min = 540, since = 200", [])
            .unwrap();
        let read = db.cadence("e").unwrap().unwrap();
        assert_eq!(read.days, 127);
        assert_eq!(read.shape, Shape::Days { weekly: false, times: vec![540] });
    }
}
