//! A100, A104 (#685): blocks and mention mutes between people.
use super::*;

/// Whether a row went in, or the person is at `most` already.
pub enum Added {
    Added,
    Already,
    Full,
}

fn listed(
    conn: &Connection,
    sql: &str,
    person_id: &str,
) -> rusqlite::Result<Vec<(String, String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([person_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

impl Db {
    /// A100, A104: adds a block (`table` person_blocks) or a mute (person_mutes), up to `most`.
    fn add_tie(
        &self,
        table: &str,
        (from, to): (&str, &str),
        from_id: &str,
        to_id: &str,
        most: i64,
        at: i64,
    ) -> rusqlite::Result<Added> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let already: bool = tx
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE {from} = ?1 AND {to} = ?2"),
                params![from_id, to_id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if already {
            return Ok(Added::Already);
        }
        let count: i64 = tx.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {from} = ?"),
            [from_id],
            |row| row.get(0),
        )?;
        if count >= most {
            return Ok(Added::Full);
        }
        tx.execute(
            &format!("INSERT INTO {table} ({from}, {to}, created_at) VALUES (?1, ?2, ?3)"),
            params![from_id, to_id, at],
        )?;
        tx.commit()?;
        Ok(Added::Added)
    }

    pub fn block_person(
        &self,
        blocker: &str,
        blocked: &str,
        most: i64,
        at: i64,
    ) -> rusqlite::Result<Added> {
        self.add_tie("person_blocks", ("blocker_id", "blocked_id"), blocker, blocked, most, at)
    }

    pub fn unblock_person(&self, blocker: &str, blocked: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "DELETE FROM person_blocks WHERE blocker_id = ?1 AND blocked_id = ?2",
            params![blocker, blocked],
        )? > 0)
    }

    pub fn mute_person(
        &self,
        muter: &str,
        muted: &str,
        most: i64,
        at: i64,
    ) -> rusqlite::Result<Added> {
        self.add_tie("person_mutes", ("muter_id", "muted_id"), muter, muted, most, at)
    }

    pub fn unmute_person(&self, muter: &str, muted: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "DELETE FROM person_mutes WHERE muter_id = ?1 AND muted_id = ?2",
            params![muter, muted],
        )? > 0)
    }

    /// A100: this person's blocks, newest first: id, name, when.
    pub fn blocks_of(&self, person_id: &str) -> rusqlite::Result<Vec<(String, String, i64)>> {
        listed(
            &self.conn(),
            "SELECT b.blocked_id, p.name, b.created_at FROM person_blocks b
             JOIN people p ON p.id = b.blocked_id WHERE b.blocker_id = ? ORDER BY b.created_at DESC",
            person_id,
        )
    }

    /// A104: whose mentions this person has muted, newest first: id, name, when.
    pub fn mutes_of(&self, person_id: &str) -> rusqlite::Result<Vec<(String, String, i64)>> {
        listed(
            &self.conn(),
            "SELECT m.muted_id, p.name, m.created_at FROM person_mutes m
             JOIN people p ON p.id = m.muted_id WHERE m.muter_id = ? ORDER BY m.created_at DESC",
            person_id,
        )
    }

    /// A102, A104: whether `speaker`'s lines, or their bots', may not mention `listener` or the
    /// listener's bots: a block either way, or the listener muted the speaker.
    pub fn contact_cut(&self, listener: &str, speaker: &str) -> rusqlite::Result<bool> {
        self.conn().query_row(
            "SELECT EXISTS (SELECT 1 FROM person_blocks
                            WHERE (blocker_id = ?1 AND blocked_id = ?2)
                               OR (blocker_id = ?2 AND blocked_id = ?1))
                 OR EXISTS (SELECT 1 FROM person_mutes WHERE muter_id = ?1 AND muted_id = ?2)",
            params![listener, speaker],
            |row| row.get(0),
        )
    }
}
