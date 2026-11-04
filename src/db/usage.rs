use super::*;
use crate::usage::{StoredHour, StoredLatency, UsageRows, BUCKETS, KEEP_DAYS};

impl Db {
    /// U4: adds counted calls to their hour, bot-day and day.
    pub fn write_usage(&self, rows: &UsageRows) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for row in &rows.hours {
            tx.execute(
                "INSERT INTO usage_hours (hour, surface, name, caller, outcome, reason, version,
                     calls, millis, timed, slow)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT (hour, surface, name, caller, outcome, reason, version) DO UPDATE SET
                     calls = calls + excluded.calls, millis = millis + excluded.millis,
                     timed = timed + excluded.timed, slow = slow + excluded.slow",
                params![
                    row.hour,
                    row.surface,
                    row.name,
                    row.caller,
                    row.outcome,
                    row.reason,
                    rows.version,
                    row.calls,
                    row.millis,
                    row.timed,
                    row.slow
                ],
            )?;
        }
        for (agent, day, outcome, reason, calls) in &rows.bots {
            // A bot that has gone has no totals left to add to (U8).
            tx.execute(
                "INSERT INTO usage_bot_days (agent_id, day, outcome, reason, calls)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM agents WHERE id = ?1)
                 ON CONFLICT (agent_id, day, outcome, reason) DO UPDATE SET
                     calls = calls + excluded.calls",
                params![agent, day, outcome, reason, calls],
            )?;
        }
        for (agent, day, events) in &rows.bot_events {
            // U22: as for its calls, a bot that has gone has nothing left to add to (U8).
            tx.execute(
                "INSERT INTO usage_bot_events (agent_id, day, events)
                 SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM agents WHERE id = ?1)
                 ON CONFLICT (agent_id, day) DO UPDATE SET events = events + excluded.events",
                params![agent, day, events],
            )?;
        }
        for (hour, people, bots) in rows.active_hours {
            tx.execute(
                "INSERT INTO usage_active_hours (hour, people, bots) VALUES (?1, ?2, ?3)
                 ON CONFLICT (hour) DO UPDATE SET people = MAX(people, excluded.people),
                     bots = MAX(bots, excluded.bots)",
                params![hour, people, bots],
            )?;
        }
        for (hour, line, count) in &rows.lines {
            tx.execute(
                "INSERT INTO usage_lines (hour, line, count) VALUES (?1, ?2, ?3)
                 ON CONFLICT (hour, line) DO UPDATE SET count = count + excluded.count",
                params![hour, line, count],
            )?;
        }
        for (hour, surface, name, hist) in &rows.latency {
            let mut values: Vec<rusqlite::types::Value> = vec![
                (*hour).into(),
                (*surface).to_string().into(),
                (*name).to_string().into(),
                rows.version.to_string().into(),
            ];
            values.extend(hist.iter().map(|n| rusqlite::types::Value::from(*n)));
            let columns: String = (0..BUCKETS).map(|i| format!(", b{i}")).collect();
            let marks: String = (0..BUCKETS).map(|i| format!(", ?{}", i + 5)).collect();
            let adds: Vec<String> =
                (0..BUCKETS).map(|i| format!("b{i} = b{i} + excluded.b{i}")).collect();
            tx.execute(
                &format!(
                    "INSERT INTO usage_latency (hour, surface, name, version{columns})
                     VALUES (?1, ?2, ?3, ?4{marks})
                     ON CONFLICT (hour, surface, name, version) DO UPDATE SET {}",
                    adds.join(", ")
                ),
                rusqlite::params_from_iter(values),
            )?;
        }
        for (day, people, bots) in rows.days {
            // U5: after a restart the day carries on from the larger number.
            tx.execute(
                "INSERT INTO usage_days (day, people, bots) VALUES (?1, ?2, ?3)
                 ON CONFLICT (day) DO UPDATE SET people = MAX(people, excluded.people),
                     bots = MAX(bots, excluded.bots)",
                params![day, people, bots],
            )?;
        }
        tx.commit()
    }

    /// U8: deletes counts older than the days that are kept.
    pub fn sweep_usage(&self, now: i64) -> rusqlite::Result<usize> {
        let oldest_day = now.div_euclid(86_400) - KEEP_DAYS + 1;
        let conn = self.conn();
        let mut deleted =
            conn.execute("DELETE FROM usage_hours WHERE hour < ?", [oldest_day * 24])?;
        deleted +=
            conn.execute("DELETE FROM usage_active_hours WHERE hour < ?", [oldest_day * 24])?;
        deleted += conn.execute("DELETE FROM usage_latency WHERE hour < ?", [oldest_day * 24])?;
        deleted += conn.execute("DELETE FROM usage_lines WHERE hour < ?", [oldest_day * 24])?;
        deleted += conn.execute("DELETE FROM usage_days WHERE day < ?", [oldest_day])?;
        deleted += conn.execute("DELETE FROM usage_bot_days WHERE day < ?", [oldest_day])?;
        deleted += conn.execute("DELETE FROM usage_bot_events WHERE day < ?", [oldest_day])?;
        deleted += conn.execute("DELETE FROM usage_bot_cost WHERE day < ?", [oldest_day])?;
        Ok(deleted)
    }

    /// U10: the hours from this day on, added up by day.
    pub fn usage_hours(&self, since_day: i64) -> rusqlite::Result<Vec<StoredHour>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT hour / 24, surface, name, version, outcome, reason,
                    SUM(calls), SUM(millis), SUM(timed), SUM(slow)
             FROM usage_hours WHERE hour >= ?
             GROUP BY hour / 24, surface, name, version, outcome, reason",
        )?;
        let rows = statement.query_map([since_day * 24], |r| {
            Ok(StoredHour {
                day: r.get(0)?,
                surface: r.get(1)?,
                name: r.get(2)?,
                version: r.get(3)?,
                outcome: r.get(4)?,
                reason: r.get(5)?,
                calls: r.get(6)?,
                millis: r.get(7)?,
                timed: r.get(8)?,
                slow: r.get(9)?,
            })
        })?;
        rows.collect()
    }

    /// U15: `(hour, outcome, calls)` from this hour on, for the per hour view.
    pub fn usage_hour_calls(&self, since_hour: i64) -> rusqlite::Result<Vec<(i64, String, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT hour, outcome, SUM(calls) FROM usage_hours WHERE hour >= ?
             GROUP BY hour, outcome",
        )?;
        let rows = statement.query_map([since_hour], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// U15: `(hour, people, bots)` from this hour on.
    pub fn usage_active_hours(&self, since_hour: i64) -> rusqlite::Result<Vec<(i64, i64, i64)>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT hour, people, bots FROM usage_active_hours WHERE hour >= ?")?;
        let rows = statement.query_map([since_hour], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// U17: `(hour, line, count)` from this hour on.
    pub fn usage_lines(&self, since_hour: i64) -> rusqlite::Result<Vec<(i64, String, i64)>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT hour, line, count FROM usage_lines WHERE hour >= ?")?;
        let rows = statement.query_map([since_hour], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// U16: the response-time buckets from this hour on.
    pub fn usage_latency(&self, since_hour: i64) -> rusqlite::Result<Vec<StoredLatency>> {
        let conn = self.conn();
        let columns: String = (0..BUCKETS).map(|i| format!(", b{i}")).collect();
        let mut statement = conn.prepare(&format!(
            "SELECT hour, surface, name, version{columns} FROM usage_latency WHERE hour >= ?"
        ))?;
        let rows = statement.query_map([since_hour], |r| {
            let mut hist = [0i64; BUCKETS];
            for (i, slot) in hist.iter_mut().enumerate() {
                *slot = r.get(4 + i)?;
            }
            Ok(StoredLatency {
                hour: r.get(0)?,
                surface: r.get(1)?,
                name: r.get(2)?,
                version: r.get(3)?,
                hist,
            })
        })?;
        rows.collect()
    }

    /// `(day, people, bots)` from this day on.
    pub fn usage_days(&self, since_day: i64) -> rusqlite::Result<Vec<(i64, i64, i64)>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT day, people, bots FROM usage_days WHERE day >= ?")?;
        let rows = statement.query_map([since_day], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// U23: a bot's own report of its cost and tokens for a day. It replaces any earlier one for
    /// that day, so the table holds one row per bot and day.
    pub fn set_usage_bot_cost(
        &self,
        agent_id: &str,
        day: i64,
        cost: &crate::usage::ReportedCost,
        at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO usage_bot_cost (agent_id, day, cost_cents, input_tokens, output_tokens, reported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (agent_id, day) DO UPDATE SET
             cost_cents = excluded.cost_cents, input_tokens = excluded.input_tokens,
             output_tokens = excluded.output_tokens, reported_at = excluded.reported_at",
            params![agent_id, day, cost.cost_cents, cost.input_tokens, cost.output_tokens, at],
        )?;
        Ok(())
    }

    /// U24: what a bot reported for each day, from this day on.
    pub fn usage_bot_cost(
        &self,
        agent_id: &str,
        since_day: i64,
    ) -> rusqlite::Result<Vec<(i64, crate::usage::ReportedCost)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT day, cost_cents, input_tokens, output_tokens FROM usage_bot_cost
             WHERE agent_id = ? AND day >= ? ORDER BY day",
        )?;
        let rows = statement.query_map(params![agent_id, since_day], |r| {
            Ok((
                r.get(0)?,
                crate::usage::ReportedCost {
                    cost_cents: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                },
            ))
        })?;
        rows.collect()
    }

    /// U11: `(day, outcome, reason, calls)` for one bot.
    /// U22: a bot's events by day, from this day on.
    pub fn usage_bot_events(
        &self,
        agent_id: &str,
        since_day: i64,
    ) -> rusqlite::Result<Vec<(i64, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT day, events FROM usage_bot_events WHERE agent_id = ? AND day >= ? ORDER BY day",
        )?;
        let rows =
            statement.query_map(params![agent_id, since_day], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    pub fn usage_bot_days(
        &self,
        agent_id: &str,
        since_day: i64,
    ) -> rusqlite::Result<Vec<(i64, String, String, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT day, outcome, reason, calls FROM usage_bot_days
             WHERE agent_id = ? AND day >= ?",
        )?;
        let rows = statement.query_map(params![agent_id, since_day], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.collect()
    }
}
