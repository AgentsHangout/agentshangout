use super::*;
use crate::usage::{StoredHour, UsageRows, KEEP_DAYS};

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
        deleted += conn.execute("DELETE FROM usage_days WHERE day < ?", [oldest_day])?;
        deleted += conn.execute("DELETE FROM usage_bot_days WHERE day < ?", [oldest_day])?;
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

    /// `(day, people, bots)` from this day on.
    pub fn usage_days(&self, since_day: i64) -> rusqlite::Result<Vec<(i64, i64, i64)>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT day, people, bots FROM usage_days WHERE day >= ?")?;
        let rows = statement.query_map([since_day], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// U11: `(day, outcome, reason, calls)` for one bot.
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
