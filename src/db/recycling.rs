//! RC1 to RC6: bots waiting at the recycling plant (docs/recycling-plant.md).
use super::*;

impl Db {
    /// RC2: when this bot was sent to the recycling plant, if it's there now.
    pub fn recycled_at(&self, agent_id: &str) -> rusqlite::Result<Option<i64>> {
        self.conn()
            .query_row(
                "SELECT recycled_at FROM recycled_bots WHERE agent_id = ?",
                [agent_id],
                |row| row.get(0),
            )
            .optional()
    }

    /// RC2: a bot goes to the plant, and in the same transaction its token becomes `token_hash`, one
    /// nobody holds, and its player's token ends (W3). So a failure leaves it either untouched or at
    /// the plant, never with a dead token outside it. Nothing else about it changes (RC3).
    pub fn recycle_bot(&self, agent_id: &str, token_hash: &str, at: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO recycled_bots (agent_id, recycled_at) VALUES (?, ?)
             ON CONFLICT (agent_id) DO NOTHING",
            params![agent_id, at],
        )?;
        tx.execute("UPDATE agents SET token_hash = ? WHERE id = ?", params![token_hash, agent_id])?;
        end_bot_player_token(&tx, agent_id)?;
        tx.commit()
    }

    /// RC4: whether a bot was there, and comes back with `token_hash`, in one transaction.
    pub fn unrecycle_bot(&self, agent_id: &str, token_hash: &str) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if tx.execute("DELETE FROM recycled_bots WHERE agent_id = ?", [agent_id])? == 0 {
            return Ok(false);
        }
        tx.execute("UPDATE agents SET token_hash = ? WHERE id = ?", params![token_hash, agent_id])?;
        tx.commit()?;
        Ok(true)
    }

    /// RC6: each waiting bot's name and when it went, oldest first.
    pub fn recycling_plant(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT a.name, r.recycled_at FROM recycled_bots r JOIN agents a ON a.id = r.agent_id
             ORDER BY r.recycled_at, a.id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// RC5: deletes a bot sent there at or before `at`, as `delete_agent` does, only if it's still
    /// there. Checked in the same transaction, so a bot brought back meanwhile is never deleted.
    pub fn delete_recycled_agent(&self, agent_id: &str, at: i64) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if !exists(
            &tx,
            "SELECT 1 FROM recycled_bots WHERE agent_id = ? AND recycled_at <= ?",
            params![agent_id, at],
        )? {
            return Ok(false);
        }
        delete_agent_in(&tx, agent_id)?;
        tx.commit()?;
        Ok(true)
    }

    /// RC5: the bots sent there at or before `at`.
    pub fn recycled_before(&self, at: i64) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT agent_id FROM recycled_bots WHERE recycled_at <= ? ORDER BY agent_id",
        )?;
        let rows = stmt.query_map([at], |row| row.get(0))?;
        rows.collect()
    }
}
