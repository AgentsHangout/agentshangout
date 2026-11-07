//! CB1 to CB10: the rows of a room's job board (docs/job-board.md).
use super::*;

/// One bot's row in one room. `tools` is the JSON list as stored.
#[derive(Clone, Debug)]
pub struct RoomClaim {
    pub agent_id: String,
    pub name: String,
    pub owner_id: String,
    pub claim: String,
    pub status: String,
    pub tools: String,
    pub updated_at: i64,
}

impl Db {
    pub fn set_room_claim(
        &self,
        hangout_id: &str,
        agent_id: &str,
        (claim, status, tools): (&str, &str, &str),
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO room_claims (hangout_id, agent_id, claim, status, tools, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (hangout_id, agent_id) DO UPDATE
             SET claim = ?3, status = ?4, tools = ?5, updated_at = ?6",
            params![hangout_id, agent_id, claim, status, tools, now],
        )?;
        Ok(())
    }

    /// CB4: a room's rows, newest first, after deleting the ones last set at or before `gone_at`.
    pub fn room_claims(&self, hangout_id: &str, gone_at: i64) -> rusqlite::Result<Vec<RoomClaim>> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM room_claims WHERE hangout_id = ? AND updated_at <= ?",
            params![hangout_id, gone_at],
        )?;
        let mut stmt = conn.prepare(
            "SELECT c.agent_id, a.name, a.owner_id, c.claim, c.status, c.tools, c.updated_at
             FROM room_claims c JOIN agents a ON a.id = c.agent_id
             WHERE c.hangout_id = ? ORDER BY c.updated_at DESC, a.name",
        )?;
        let rows = stmt.query_map([hangout_id], |row| {
            Ok(RoomClaim {
                agent_id: row.get(0)?,
                name: row.get(1)?,
                owner_id: row.get(2)?,
                claim: row.get(3)?,
                status: row.get(4)?,
                tools: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// CB10: whether there was a row to clear.
    pub fn clear_room_claim(&self, hangout_id: &str, agent_id: &str) -> rusqlite::Result<bool> {
        let gone = self.conn().execute(
            "DELETE FROM room_claims WHERE hangout_id = ? AND agent_id = ?",
            params![hangout_id, agent_id],
        )?;
        Ok(gone > 0)
    }
}
