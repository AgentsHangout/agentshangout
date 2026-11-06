//! GB1 and GB4: bridge keys and the turns they have played (docs/bridge.md).
use super::*;

/// GB1: how long an owned bot's bridge key works. It sits in addresses, so it doesn't last.
pub const BRIDGE_KEY_SECS: i64 = 24 * 60 * 60;
/// GB4: how long a played turn is remembered: as long as the key works.
pub const BRIDGE_TURN_SECS: i64 = BRIDGE_KEY_SECS;

impl Db {
    /// GB1: a bot's new bridge key, which ends the one before it and the turns that one played.
    pub fn set_bridge_key(&self, agent_id: &str, key_hash: &str, now: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM bridge_keys WHERE agent_id = ?", [agent_id])?;
        tx.execute(
            "INSERT INTO bridge_keys (agent_id, key_hash, created_at) VALUES (?, ?, ?)",
            params![agent_id, key_hash, now],
        )?;
        tx.commit()
    }

    /// GB1: ends a bot's bridge key. Says whether it had one.
    pub fn end_bridge_key(&self, agent_id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("DELETE FROM bridge_keys WHERE agent_id = ?", [agent_id])? > 0)
    }

    /// GB1, GB2: the bot a bridge key belongs to, while it hasn't run out.
    pub fn bridge_agent(&self, key_hash: &str, now: i64) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT agent_id FROM bridge_keys WHERE key_hash = ? AND created_at > ?",
                params![key_hash, now - BRIDGE_KEY_SECS],
                |row| row.get(0),
            )
            .optional()
    }

    /// Moves a bridge key back in time, for tests of it running out (GB1).
    pub fn age_bridge_key(&self, agent_id: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE bridge_keys SET created_at = created_at - ?2 WHERE agent_id = ?1",
            params![agent_id, by],
        )?;
        Ok(())
    }

    /// GB4: counts a turn as played. False when this key already played it within a day.
    pub fn take_bridge_turn(&self, agent_id: &str, turn: &str, now: i64) -> rusqlite::Result<bool> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM bridge_turns WHERE agent_id = ? AND at < ?",
            params![agent_id, now - BRIDGE_TURN_SECS],
        )?;
        let added = conn.execute(
            "INSERT OR IGNORE INTO bridge_turns (agent_id, turn, at) VALUES (?, ?, ?)",
            params![agent_id, turn, now],
        )?;
        Ok(added == 1)
    }
}
