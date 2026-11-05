//! Explicit consent and bot permissions; deliberately separate from inferred acquaintances.
use super::*;
use crate::bot_trust::{Friendship, Trust};

impl Db {
    pub fn trust_known_bots(&self, person: &str, at: i64) -> rusqlite::Result<Vec<Agent>> {
        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare("SELECT DISTINCT a.id FROM agents a WHERE a.owner_id=?1 OR EXISTS(SELECT 1 FROM room_agents ra JOIN members m ON m.hangout_id=ra.hangout_id WHERE ra.agent_id=a.id AND m.person_id=?1) OR EXISTS(SELECT 1 FROM trips t JOIN residents r ON r.house_id=t.house_id WHERE t.agent_id=a.id AND r.person_id=?1) OR EXISTS(SELECT 1 FROM explicit_friendships f WHERE f.kind='bot' AND (f.from_id=a.id OR f.to_id=a.id) AND (f.from_id IN (SELECT id FROM agents WHERE owner_id=?1) OR f.to_id IN (SELECT id FROM agents WHERE owner_id=?1))) ORDER BY a.name,a.id")?;
            let rows = stmt.query_map([person], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut bots: Vec<Agent> = ids
            .iter()
            .map(|id| self.agent(id, at))
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        let mut rooms =
            self.hangouts_for_person(person)?.into_iter().map(|h| h.id).collect::<Vec<_>>();
        rooms.extend(self.walked_onto(person, at)?);
        for room in rooms {
            for bot in self.agents_in(&room, at)?.into_iter().chain(self.visitors_in(&room, at)?) {
                if !bots.iter().any(|b| b.id == bot.id) {
                    bots.push(bot);
                }
            }
        }
        Ok(bots)
    }
    pub fn bot_trust(&self, agent_id: &str) -> rusqlite::Result<Trust> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT settings FROM bot_trust_settings WHERE agent_id=?1",
                [agent_id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()
        .map(|v| v.unwrap_or_default())
    }

    /// BT10: the bots whose wake setting names who may reach them (friends or selected people and
    /// bots), the only ones a tag from a room they aren't in can reach.
    pub fn bots_reachable_elsewhere(&self) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT agent_id FROM bot_trust_settings
              WHERE json_extract(settings, '$.wake.mode') IN ('friends', 'selected')",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    /// BT10: notes a tag from a room the bot isn't in, and whether it may ring: not when this
    /// tagger rang it in the last 3 minutes, or when 20 rang it in the last hour (E22, E31).
    /// Returns the tag's id and whether it rings.
    pub fn add_absent_tag(
        &self,
        agent_id: &str,
        tagger: &str,
        tagger_id: &str,
        room: &str,
        at: i64,
    ) -> rusqlite::Result<(i64, bool)> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let held: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM absent_tags
              WHERE agent_id = ?1 AND tagger_id = ?2 AND rung = 1 AND at > ?3)",
            params![agent_id, tagger_id, at - 180],
            |r| r.get(0),
        )?;
        let hour: i64 = tx.query_row(
            "SELECT COUNT(*) FROM absent_tags WHERE agent_id = ?1 AND rung = 1 AND at > ?2",
            params![agent_id, at - 3600],
            |r| r.get(0),
        )?;
        let ring = !held && hour < 20;
        tx.execute(
            "INSERT INTO absent_tags (agent_id, tagger, tagger_id, room, at, rung)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![agent_id, tagger, tagger_id, room, at, ring],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok((id, ring))
    }

    /// BT10: a bot's tags from rooms it isn't in since `since`, newest first: who and which room.
    pub fn absent_tags_since(
        &self,
        agent_id: &str,
        since: i64,
        limit: i64,
    ) -> rusqlite::Result<Vec<(String, String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tagger, room, at FROM absent_tags WHERE agent_id = ?1 AND at >= ?2
              ORDER BY at DESC, id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![agent_id, since, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect()
    }

    pub fn save_bot_trust(&self, agent_id: &str, trust: &Trust) -> rusqlite::Result<()> {
        self.conn().execute("INSERT INTO bot_trust_settings(agent_id,settings) VALUES(?1,?2) ON CONFLICT(agent_id) DO UPDATE SET settings=excluded.settings", params![agent_id, serde_json::to_string(trust).expect("serializable trust")])?;
        Ok(())
    }

    pub fn explicit_friendships(&self, person_id: &str) -> rusqlite::Result<Vec<Friendship>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT kind,from_id,to_id,status FROM explicit_friendships WHERE (kind='person' AND (from_id=?1 OR to_id=?1)) OR (kind='bot' AND (from_id IN (SELECT id FROM agents WHERE owner_id=?1) OR to_id IN (SELECT id FROM agents WHERE owner_id=?1))) ORDER BY created_at,from_id,to_id")?;
        let rows = stmt.query_map([person_id], |r| {
            Ok(Friendship {
                kind: r.get(0)?,
                from_id: r.get(1)?,
                to_id: r.get(2)?,
                status: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn explicit_friends(
        &self,
        kind: &str,
        first: &str,
        second: &str,
    ) -> rusqlite::Result<bool> {
        self.conn().query_row("SELECT EXISTS(SELECT 1 FROM explicit_friendships WHERE kind=?1 AND status='accepted' AND ((from_id=?2 AND to_id=?3) OR (from_id=?3 AND to_id=?2)))", params![kind,first,second], |r| r.get(0))
    }

    pub fn request_explicit_friend(
        &self,
        kind: &str,
        from: &str,
        to: &str,
        at: i64,
    ) -> rusqlite::Result<()> {
        // Crossed requests stay pending: acceptance is always a separate explicit action.
        self.conn().execute("INSERT INTO explicit_friendships(kind,from_id,to_id,status,created_at) SELECT ?1,?2,?3,'pending',?4 WHERE NOT EXISTS(SELECT 1 FROM explicit_friendships WHERE kind=?1 AND ((from_id=?2 AND to_id=?3) OR (from_id=?3 AND to_id=?2)))", params![kind,from,to,at])?;
        Ok(())
    }

    pub fn accept_explicit_friend(
        &self,
        kind: &str,
        from: &str,
        to: &str,
    ) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("UPDATE explicit_friendships SET status='accepted' WHERE kind=?1 AND from_id=?2 AND to_id=?3 AND status='pending'", params![kind,from,to])? > 0)
    }

    pub fn remove_explicit_friend(&self, kind: &str, from: &str, to: &str) -> rusqlite::Result<()> {
        self.conn().execute("DELETE FROM explicit_friendships WHERE kind=?1 AND ((from_id=?2 AND to_id=?3) OR (from_id=?3 AND to_id=?2))", params![kind,from,to])?;
        Ok(())
    }

    pub fn trust_message(&self, id: i64) -> rusqlite::Result<Option<Message>> {
        self.conn()
            .query_row(
                &format!("SELECT {MESSAGE_COLS} FROM messages WHERE id=?1"),
                [id],
                message_from_row,
            )
            .optional()
    }
}
