//! I86-I90: `@room`, a mention of the subscribed bots present in a room (docs/rooms.md).
use super::*;

impl Db {
    /// I86: `off`, `owners` or `members`. A room nobody set is `off`.
    pub fn room_mention_policy(&self, hangout_id: &str) -> rusqlite::Result<String> {
        let policy = self
            .conn()
            .query_row(
                "SELECT policy FROM room_mention_settings WHERE hangout_id = ?",
                [hangout_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(policy.unwrap_or_else(|| "off".to_string()))
    }

    pub fn set_room_mention_policy(
        &self,
        hangout_id: &str,
        policy: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO room_mention_settings (hangout_id, policy, set_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (hangout_id) DO UPDATE SET policy = excluded.policy, set_at = excluded.set_at",
            params![hangout_id, policy, now],
        )?;
        Ok(())
    }

    /// I120: which of this person's bots hear `@room` in this room, by id.
    pub fn room_mention_bots_of(
        &self,
        hangout_id: &str,
        owner_id: &str,
    ) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT s.agent_id FROM room_mention_subscriptions s JOIN agents a ON a.id = s.agent_id
             WHERE s.hangout_id = ? AND a.owner_id = ? ORDER BY s.agent_id",
        )?;
        let rows = statement.query_map(params![hangout_id, owner_id], |row| row.get(0))?;
        rows.collect()
    }

    /// I87: whether this bot's owner subscribed it to `@room` in this room.
    pub fn room_mention_subscribed(
        &self,
        hangout_id: &str,
        agent_id: &str,
    ) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM room_mention_subscriptions WHERE hangout_id = ? AND agent_id = ?",
            params![hangout_id, agent_id],
        )
    }

    pub fn set_room_mention_subscription(
        &self,
        hangout_id: &str,
        agent_id: &str,
        subscribed: bool,
        now: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        if subscribed {
            conn.execute(
                "INSERT OR IGNORE INTO room_mention_subscriptions (hangout_id, agent_id, set_at)
                 VALUES (?, ?, ?)",
                params![hangout_id, agent_id, now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM room_mention_subscriptions WHERE hangout_id = ? AND agent_id = ?",
                params![hangout_id, agent_id],
            )?;
        }
        Ok(())
    }

    /// I88: this bot's event recorded a room-tag mention rather than a direct name. Older
    /// events without provenance cannot grant an extra polling wake.
    pub(crate) fn room_mention_event(
        &self,
        message_id: i64,
        agent_id: &str,
    ) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM bot_events WHERE agent_id = ?1 AND ref_key = ?2 AND name = 'mention'
               AND json_extract(data, '$.via_room_tag') = 1",
            params![agent_id, format!("mention:{message_id}")],
        )
    }

    /// I89: when `@room` last reached anyone in this room.
    pub fn last_room_mention(&self, hangout_id: &str) -> rusqlite::Result<Option<i64>> {
        self.conn().query_row(
            "SELECT MAX(at) FROM room_mention_uses WHERE hangout_id = ? AND delivered > 0",
            [hangout_id],
            |row| row.get(0),
        )
    }

    /// I89: what an `@room` in this message did: how many bots it reached, or why none.
    pub fn record_room_mention(
        &self,
        message_id: i64,
        hangout_id: &str,
        delivered: usize,
        reason: Option<&str>,
        at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO room_mention_uses (message_id, hangout_id, delivered, reason, at)
             VALUES (?, ?, ?, ?, ?)",
            params![message_id, hangout_id, delivered as i64, reason, at],
        )?;
        Ok(())
    }

    pub fn room_mention_use(
        &self,
        message_id: i64,
    ) -> rusqlite::Result<Option<(i64, Option<String>)>> {
        self.conn()
            .query_row(
                "SELECT delivered, reason FROM room_mention_uses WHERE message_id = ?",
                [message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    /// Moves a room's last `@room` back, for tests of how often it can be used (I89).
    pub fn age_room_mentions(&self, hangout_id: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE room_mention_uses SET at = at - ?2 WHERE hangout_id = ?1",
            params![hangout_id, by],
        )?;
        Ok(())
    }
}
