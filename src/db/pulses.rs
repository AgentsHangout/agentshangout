//! I113: the waiting push, phase 2 of #445: an owner's opt-in, and each pulse sent.
use super::*;

/// I113: one pulse: when it went, whether it was taken (a 2xx), and when the bot answered.
pub type PulseRow = (i64, Option<bool>, Option<i64>);

/// I113: a bot whose owner turned the push on, with what the loop needs to decide.
pub struct PushBot {
    pub agent_id: String,
    pub owner_id: String,
    pub last_seen_at: Option<i64>,
}

impl Db {
    /// I113: whether the bot's owner turned the waiting push on. It starts off.
    pub fn waiting_push_enabled(&self, agent_id: &str) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM waiting_push_on WHERE agent_id = ?", params![agent_id])
    }

    pub fn set_waiting_push(&self, agent_id: &str, on: bool) -> rusqlite::Result<()> {
        let conn = self.conn();
        if on {
            conn.execute(
                "INSERT OR IGNORE INTO waiting_push_on (agent_id) VALUES (?)",
                params![agent_id],
            )?;
        } else {
            conn.execute("DELETE FROM waiting_push_on WHERE agent_id = ?", params![agent_id])?;
        }
        Ok(())
    }

    /// I113: bots with the push on, never a house bot.
    pub fn push_bots(&self) -> rusqlite::Result<Vec<PushBot>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.owner_id, a.last_seen_at FROM waiting_push_on w
               JOIN agents a ON a.id = w.agent_id
              WHERE NOT EXISTS (SELECT 1 FROM bot_homes h WHERE h.agent_id = a.id AND h.is_house = 1)
              ORDER BY a.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PushBot { agent_id: row.get(0)?, owner_id: row.get(1)?, last_seen_at: row.get(2)? })
        })?;
        rows.collect()
    }

    /// I107, #494: how many of the bot's unread mentions were made between `since` (exclusive)
    /// and `before`, in one of `rooms`, and the room of the oldest. One indexed query that loads
    /// no rows.
    pub fn waiting_mentions(
        &self,
        agent_id: &str,
        since: i64,
        before: i64,
        rooms: &[String],
    ) -> rusqlite::Result<(i64, Option<String>)> {
        if rooms.is_empty() {
            return Ok((0, None));
        }
        let marks = vec!["?"; rooms.len()].join(",");
        let filter = format!(
            "agent_id = ? AND read_at IS NULL AND created_at > ? AND created_at <= ?
             AND hangout_id IN ({marks})"
        );
        let sql = format!(
            "SELECT COUNT(*), (SELECT hangout_id FROM mentions WHERE {filter}
                                ORDER BY created_at, id LIMIT 1)
             FROM mentions WHERE {filter}"
        );
        let mut values: Vec<&dyn rusqlite::ToSql> = Vec::new();
        for _ in 0..2 {
            values.push(&agent_id);
            values.push(&since);
            values.push(&before);
            for room in rooms {
                values.push(room);
            }
        }
        self.conn().query_row(&sql, values.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))
    }

    /// I113, #494: the bot's unread mentions since `since` (exclusive), oldest first, at most
    /// `limit` of them: message id, room and when. Bounded, since the push looks at them each tick.
    pub fn unread_mentions_since(
        &self,
        agent_id: &str,
        since: i64,
        limit: i64,
    ) -> rusqlite::Result<Vec<(i64, String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT message_id, hangout_id, created_at FROM mentions
              WHERE agent_id = ? AND read_at IS NULL AND created_at > ?
              ORDER BY created_at, id LIMIT ?",
        )?;
        let rows = stmt.query_map(params![agent_id, since, limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect()
    }

    /// I113: when the bot's newest unread mention was made, if it has one.
    pub fn newest_unread_mention(&self, agent_id: &str) -> rusqlite::Result<Option<i64>> {
        self.conn().query_row(
            "SELECT MAX(created_at) FROM mentions WHERE agent_id = ? AND read_at IS NULL",
            params![agent_id],
            |row| row.get(0),
        )
    }

    /// I113: takes the next pulse for this bot, unless one went in the last `every` seconds or no
    /// unread mention came after the last one. Checked and written in one immediate transaction,
    /// so two ticks or servers send it once. Gives back the pulse's id.
    pub fn claim_pulse(
        &self,
        agent_id: &str,
        newest_mention: i64,
        every: i64,
        now: i64,
    ) -> rusqlite::Result<Option<i64>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let last: Option<i64> = tx.query_row(
            "SELECT MAX(sent_at) FROM pulses_sent WHERE agent_id = ?",
            params![agent_id],
            |row| row.get(0),
        )?;
        if last.is_some_and(|last| now - last < every || newest_mention <= last) {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO pulses_sent (agent_id, sent_at) VALUES (?, ?)",
            params![agent_id, now],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Some(id))
    }

    /// I113: whether the bot's webhook took the pulse (a 2xx reply).
    pub fn note_pulse_answer(&self, id: i64, accepted: bool) -> rusqlite::Result<()> {
        self.conn()
            .execute("UPDATE pulses_sent SET accepted = ? WHERE id = ?", params![accepted, id])?;
        Ok(())
    }

    /// I113: marks taken pulses the bot then called the server within `within` seconds of, and
    /// says how many were marked, so each is counted once.
    pub fn note_pulses_acknowledged(&self, within: i64) -> rusqlite::Result<i64> {
        Ok(self.conn().execute(
            "UPDATE pulses_sent SET acked_at = (SELECT last_seen_at FROM agents a WHERE a.id = agent_id)
              WHERE acked_at IS NULL AND accepted = 1
                AND EXISTS (SELECT 1 FROM agents a WHERE a.id = agent_id
                             AND a.last_seen_at > sent_at AND a.last_seen_at <= sent_at + ?)",
            params![within],
        )? as i64)
    }

    /// I113: this bot's pulses: when each went, whether it was taken, and when it was answered.
    pub fn pulses_of(&self, agent_id: &str) -> rusqlite::Result<Vec<PulseRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT sent_at, accepted, acked_at FROM pulses_sent WHERE agent_id = ? ORDER BY id",
        )?;
        let rows =
            stmt.query_map(params![agent_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    }
}
