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

    /// A106: the trips between `a` and `b`, either way, asked or under way, so each can be ended the
    /// ordinary way (`city::end_trip`), which leaves no sign of why.
    pub fn trips_between(&self, a: &str, b: &str, now: i64) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT t.id FROM trips t JOIN agents g ON g.id = t.agent_id
             WHERE t.status IN ('asking', 'here') AND (t.ends_at IS NULL OR t.ends_at > ?3)
               AND ((g.owner_id = ?2 AND EXISTS (SELECT 1 FROM residents r
                                                 WHERE r.house_id = t.house_id AND r.person_id = ?1))
                 OR (g.owner_id = ?1 AND EXISTS (SELECT 1 FROM residents r
                                                 WHERE r.house_id = t.house_id AND r.person_id = ?2)))",
        )?;
        let rows = stmt.query_map(params![a, b, now], |row| row.get(0))?;
        rows.collect()
    }

    /// A106: when `a` blocks `b`, what was between them ends, in one transaction: friendships (theirs
    /// and their bots'), house keys either way, invites to a place either way, and apprentice
    /// proposals still open between them, declined as an owner would. Trips are ended first, the
    /// ordinary way (`trips_between`). Safe to run again: a block made twice ends them again.
    pub fn end_ties(&self, a: &str, b: &str, now: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
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
        tx.execute(
            "DELETE FROM passes WHERE
                (EXISTS (SELECT 1 FROM residents r WHERE r.house_id = passes.house_id AND r.person_id = ?1)
                 AND agent_id IN (SELECT id FROM agents WHERE owner_id = ?2))
             OR (EXISTS (SELECT 1 FROM residents r WHERE r.house_id = passes.house_id AND r.person_id = ?2)
                 AND agent_id IN (SELECT id FROM agents WHERE owner_id = ?1))",
            params![a, b],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO friend_invite_ends (invite_id)
             SELECT id FROM friend_invites
             WHERE (from_person = ?1 AND to_person = ?2) OR (from_person = ?2 AND to_person = ?1)",
            params![a, b],
        )?;
        tx.execute(
            "UPDATE apprentice_proposals SET status = 'declined', declined_by = 'owner', decided_at = ?3
             WHERE status IN ('asking_bot', 'asking_owners')
               AND ((proposer_owner = ?1 AND partner_owner = ?2)
                 OR (proposer_owner = ?2 AND partner_owner = ?1))",
            params![a, b, now],
        )?;
        tx.commit()
    }
}
