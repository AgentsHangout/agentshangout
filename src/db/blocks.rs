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

    /// A101: the people this person has blocked, whose lines they and their bots don't see.
    pub fn blocked_ids(
        &self,
        person_id: &str,
    ) -> rusqlite::Result<std::collections::HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT blocked_id FROM person_blocks WHERE blocker_id = ?")?;
        let rows = stmt.query_map([person_id], |row| row.get(0))?;
        rows.collect()
    }

    /// A106: when `a` blocks `b`, what was between them ends: friendships (theirs and their bots'),
    /// house keys either way and the trips they let in, other trips under way or asked either way,
    /// and apprentice proposals still open between them. Taking the block back restores none.
    pub fn end_ties(&self, a: &str, b: &str, now: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        // Friendships: the two people, and any between their bots.
        tx.execute(
            "DELETE FROM explicit_friendships WHERE kind = 'person'
               AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))",
            params![a, b],
        )?;
        tx.execute(
            "DELETE FROM explicit_friendships WHERE kind = 'bot'
               AND ((from_id IN (SELECT id FROM agents WHERE owner_id = ?1)
                     AND to_id IN (SELECT id FROM agents WHERE owner_id = ?2))
                 OR (from_id IN (SELECT id FROM agents WHERE owner_id = ?2)
                     AND to_id IN (SELECT id FROM agents WHERE owner_id = ?1)))",
            params![a, b],
        )?;
        // Keys either way: a pass from a house one of them lives in to a bot of the other's.
        let either_way = "(EXISTS (SELECT 1 FROM residents r WHERE r.house_id = {house} AND r.person_id = ?1)
                           AND {agent} IN (SELECT id FROM agents WHERE owner_id = ?2))
                       OR (EXISTS (SELECT 1 FROM residents r WHERE r.house_id = {house} AND r.person_id = ?2)
                           AND {agent} IN (SELECT id FROM agents WHERE owner_id = ?1))";
        let passes = either_way.replace("{house}", "p.house_id").replace("{agent}", "p.agent_id");
        tx.execute(
            &format!(
                "UPDATE trips SET status = 'ended', ended_by = 'block'
                 WHERE status = 'here' AND pass_id IN (SELECT p.id FROM passes p WHERE {passes})"
            ),
            params![a, b],
        )?;
        tx.execute(&format!("DELETE FROM passes AS p WHERE {passes}"), params![a, b])?;
        // Other trips, under way or asked, either way.
        let trips =
            either_way.replace("{house}", "trips.house_id").replace("{agent}", "trips.agent_id");
        tx.execute(
            &format!(
                "UPDATE trips SET status = CASE status WHEN 'asking' THEN 'declined' ELSE 'ended' END,
                     ended_by = 'block'
                 WHERE status IN ('asking', 'here') AND ({trips})"
            ),
            params![a, b],
        )?;
        // Apprentice proposals still open between them.
        tx.execute(
            "UPDATE apprentice_proposals SET status = 'declined', declined_by = 'block', decided_at = ?3
             WHERE status IN ('asking_bot', 'asking_owners')
               AND ((proposer_owner = ?1 AND partner_owner = ?2)
                 OR (proposer_owner = ?2 AND partner_owner = ?1))",
            params![a, b, now],
        )?;
        tx.commit()
    }
}
