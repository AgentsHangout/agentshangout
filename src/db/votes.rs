//! V1-V8: polls bots open in a room (docs/rooms.md, Votes).
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct Ballot {
    /// The bot's id, or the person's for a person's ballot (V9).
    pub agent_id: String,
    pub voter: String,
    /// A bot's owner. Empty for a person, who is their own.
    pub owner: String,
    pub choice: usize,
    /// V9: `bot` or `person`.
    pub kind: String,
    /// V10: cast by someone who doesn't belong to the room, who walked onto its public floor.
    pub visitor: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Vote {
    pub id: String,
    pub hangout_id: String,
    pub agent_id: Option<String>,
    pub opener: String,
    pub question: String,
    pub options: Vec<String>,
    pub opened_at: i64,
    pub closes_at: i64,
    /// When its result was posted. Set once.
    pub closed_at: Option<i64>,
    pub ballots: Vec<Ballot>,
    /// V9: the person who opened it, when a person did.
    pub opener_person: Option<String>,
}

impl Vote {
    /// V5: open until its deadline, or until its result is posted if that was early.
    pub fn open(&self, now: i64) -> bool {
        self.closed_at.is_none() && self.closes_at > now
    }

    /// How many ballots each option has, in the options' order.
    pub fn tally(&self) -> Vec<usize> {
        let mut counts = vec![0; self.options.len()];
        for ballot in &self.ballots {
            if let Some(count) = counts.get_mut(ballot.choice) {
                *count += 1;
            }
        }
        counts
    }
}

pub struct NewVote<'a> {
    pub hangout_id: &'a str,
    /// The bot that opens it, or None when a person does (V9).
    pub agent_id: Option<&'a str>,
    pub person_id: Option<&'a str>,
    pub opener: &'a str,
    pub question: &'a str,
    pub options: &'a [String],
    pub now: i64,
    pub closes_at: i64,
    /// V2: the line the room is told, stored with the vote so neither is there without the other.
    pub announcement: &'a str,
}

const VOTE_COLS: &str =
    "id, hangout_id, agent_id, opener, question, options, opened_at, closes_at, closed_at";

fn vote_from_row(row: &Row) -> rusqlite::Result<Vote> {
    let options: String = row.get(5)?;
    Ok(Vote {
        id: row.get(0)?,
        hangout_id: row.get(1)?,
        agent_id: row.get(2)?,
        opener: row.get(3)?,
        question: row.get(4)?,
        options: serde_json::from_str(&options).unwrap_or_default(),
        opened_at: row.get(6)?,
        closes_at: row.get(7)?,
        closed_at: row.get(8)?,
        ballots: Vec::new(),
        opener_person: None,
    })
}

fn with_ballots(conn: &Connection, mut vote: Vote) -> rusqlite::Result<Vote> {
    // Bots' ballots and people's (V9), in the order they were cast. A visitor is one who doesn't
    // belong to the room now (V10).
    let mut stmt = conn.prepare(
        "SELECT b.agent_id, b.voter, COALESCE(p.name, ''), b.choice, 'bot',
                NOT EXISTS (SELECT 1 FROM room_agents r WHERE r.hangout_id = ?2 AND r.agent_id = b.agent_id),
                b.cast_at
         FROM vote_ballots b
         LEFT JOIN agents a ON a.id = b.agent_id
         LEFT JOIN people p ON p.id = a.owner_id
         WHERE b.vote_id = ?1
         UNION ALL
         SELECT pb.person_id, pb.voter, '', pb.choice, 'person',
                NOT EXISTS (SELECT 1 FROM members m WHERE m.hangout_id = ?2 AND m.person_id = pb.person_id),
                pb.cast_at
         FROM vote_person_ballots pb WHERE pb.vote_id = ?1
         ORDER BY 7, 1",
    )?;
    let rows = stmt.query_map(params![vote.id, vote.hangout_id], |row| {
        Ok(Ballot {
            agent_id: row.get(0)?,
            voter: row.get(1)?,
            owner: row.get(2)?,
            choice: row.get::<_, i64>(3)? as usize,
            kind: row.get(4)?,
            visitor: row.get(5)?,
        })
    })?;
    vote.ballots = rows.collect::<rusqlite::Result<_>>()?;
    vote.opener_person = conn
        .query_row("SELECT person_id FROM person_votes WHERE vote_id = ?", [&vote.id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(vote)
}

impl Db {
    /// V2: opens a vote and writes its line to the room in one step. None, and nothing written, if
    /// this bot, or this person (V9), already has one open in the room.
    pub fn open_vote(&self, vote: &NewVote) -> rusqlite::Result<Option<(Vote, Message)>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let busy = match (vote.agent_id, vote.person_id) {
            (Some(agent_id), _) => exists(
                &tx,
                "SELECT 1 FROM votes WHERE hangout_id = ? AND agent_id = ?
                   AND closed_at IS NULL AND closes_at > ?",
                params![vote.hangout_id, agent_id, vote.now],
            )?,
            (None, Some(person_id)) => exists(
                &tx,
                "SELECT 1 FROM votes v JOIN person_votes pv ON pv.vote_id = v.id
                 WHERE v.hangout_id = ? AND pv.person_id = ?
                   AND v.closed_at IS NULL AND v.closes_at > ?",
                params![vote.hangout_id, person_id, vote.now],
            )?,
            (None, None) => true,
        };
        if busy {
            return Ok(None);
        }
        let id = crate::auth::new_id();
        let options = serde_json::to_string(vote.options).unwrap_or_else(|_| "[]".into());
        tx.execute(
            "INSERT INTO votes (id, hangout_id, agent_id, opener, question, options, opened_at,
                                closes_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                vote.hangout_id,
                vote.agent_id,
                vote.opener,
                vote.question,
                options,
                vote.now,
                vote.closes_at
            ],
        )?;
        if let (None, Some(person_id)) = (vote.agent_id, vote.person_id) {
            tx.execute(
                "INSERT INTO person_votes (vote_id, person_id) VALUES (?, ?)",
                params![id, person_id],
            )?;
        }
        let message = insert_message_row(
            &tx,
            &NewMessage {
                hangout_id: vote.hangout_id,
                person_id: None,
                agent_id: None,
                speaker: "Hangout",
                kind: "system",
                body: vote.announcement,
                now: vote.now,
            },
        )?;
        tx.commit()?;
        let opened = Vote {
            id,
            hangout_id: vote.hangout_id.to_string(),
            agent_id: vote.agent_id.map(str::to_string),
            opener: vote.opener.to_string(),
            question: vote.question.to_string(),
            options: vote.options.to_vec(),
            opened_at: vote.now,
            closes_at: vote.closes_at,
            closed_at: None,
            ballots: Vec::new(),
            opener_person: vote
                .agent_id
                .is_none()
                .then(|| vote.person_id.map(str::to_string))
                .flatten(),
        };
        Ok(Some((opened, message)))
    }

    pub fn vote(&self, id: &str) -> rusqlite::Result<Option<Vote>> {
        let conn = self.conn();
        let vote = conn
            .query_row(&format!("SELECT {VOTE_COLS} FROM votes WHERE id = ?"), [id], vote_from_row)
            .optional()?;
        vote.map(|vote| with_ballots(&conn, vote)).transpose()
    }

    /// V6: a room's votes that are open, or whose result was posted since then, newest first.
    pub fn room_votes(&self, hangout_id: &str, since: i64) -> rusqlite::Result<Vec<Vote>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {VOTE_COLS} FROM votes
             WHERE hangout_id = ? AND (closed_at IS NULL OR closed_at >= ?)
             ORDER BY opened_at DESC, id LIMIT 20"
        ))?;
        let votes = stmt.query_map(params![hangout_id, since], vote_from_row)?;
        let votes = votes.collect::<rusqlite::Result<Vec<_>>>()?;
        votes.into_iter().map(|vote| with_ballots(&conn, vote)).collect()
    }

    /// V4: records or changes a bot's ballot, only while the vote is open. False if it isn't.
    pub fn cast_ballot(
        &self,
        vote_id: &str,
        agent_id: &str,
        voter: &str,
        choice: usize,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "INSERT INTO vote_ballots (vote_id, agent_id, voter, choice, cast_at)
             SELECT id, ?2, ?3, ?4, ?5 FROM votes
             WHERE id = ?1 AND closed_at IS NULL AND closes_at > ?5
             ON CONFLICT (vote_id, agent_id) DO UPDATE SET choice = excluded.choice,
                 voter = excluded.voter, cast_at = excluded.cast_at",
            params![vote_id, agent_id, voter, choice as i64, now],
        )?;
        Ok(changed == 1)
    }

    /// V9: records or changes a person's ballot, only while the vote is open. False if it isn't.
    pub fn cast_person_ballot(
        &self,
        vote_id: &str,
        person_id: &str,
        voter: &str,
        choice: usize,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "INSERT INTO vote_person_ballots (vote_id, person_id, voter, choice, cast_at)
             SELECT id, ?2, ?3, ?4, ?5 FROM votes
             WHERE id = ?1 AND closed_at IS NULL AND closes_at > ?5
             ON CONFLICT (vote_id, person_id) DO UPDATE SET choice = excluded.choice,
                 voter = excluded.voter, cast_at = excluded.cast_at",
            params![vote_id, person_id, voter, choice as i64, now],
        )?;
        Ok(changed == 1)
    }

    /// V11: the open votes in these rooms, newest first, at most `limit`.
    pub fn open_votes_in(
        &self,
        rooms: &[String],
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<Vote>> {
        let conn = self.conn();
        let mut votes = Vec::new();
        let mut stmt = conn.prepare(&format!(
            "SELECT {VOTE_COLS} FROM votes WHERE hangout_id = ? AND closed_at IS NULL
               AND closes_at > ? ORDER BY opened_at DESC, id"
        ))?;
        for room in rooms {
            let found = stmt.query_map(params![room, now], vote_from_row)?;
            for vote in found {
                votes.push(with_ballots(&conn, vote?)?);
            }
        }
        votes.sort_by(|a, b| b.opened_at.cmp(&a.opened_at).then_with(|| a.id.cmp(&b.id)));
        votes.truncate(limit);
        Ok(votes)
    }

    /// V7: brings an open vote's deadline to now. False if it was already over.
    pub fn end_vote_now(&self, id: &str, now: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "UPDATE votes SET closes_at = ?2 WHERE id = ?1 AND closed_at IS NULL AND closes_at > ?2",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// V5: votes past their deadline whose result isn't posted yet.
    pub fn due_votes(&self, now: i64) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM votes WHERE closed_at IS NULL AND closes_at <= ? ORDER BY closes_at",
        )?;
        let rows = stmt.query_map([now], |row| row.get(0))?;
        rows.collect()
    }

    /// V5: closes a vote past its deadline and writes its result line to the room in one step, so
    /// the result is stored exactly once: if writing it fails, the vote stays unclosed and the next
    /// sweep tries again. None if it was already closed, or isn't due.
    pub fn close_vote_with_result(
        &self,
        id: &str,
        hangout_id: &str,
        result: &str,
        now: i64,
    ) -> rusqlite::Result<Option<Message>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let claimed = tx.execute(
            "UPDATE votes SET closed_at = ?2 WHERE id = ?1 AND closed_at IS NULL AND closes_at <= ?2",
            params![id, now],
        )?;
        if claimed != 1 {
            return Ok(None);
        }
        let message = insert_message_row(
            &tx,
            &NewMessage {
                hangout_id,
                person_id: None,
                agent_id: None,
                speaker: "Hangout",
                kind: "system",
                body: result,
                now,
            },
        )?;
        tx.commit()?;
        Ok(Some(message))
    }

    /// Moves a vote's clock back, for tests of how it closes (V5).
    pub fn age_vote(&self, id: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE votes SET opened_at = opened_at - ?2, closes_at = closes_at - ?2,
                              closed_at = closed_at - ?2 WHERE id = ?1",
            params![id, by],
        )?;
        Ok(())
    }
}
