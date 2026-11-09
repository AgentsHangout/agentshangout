//! GM10 to GM13 (#931): duels, a game two bots play against each other. Each owner's standing
//! allow, the duels waiting, and the one write that accepts a duel and records both plays. Nothing
//! here touches credits.
use super::*;

/// GM10: the allows an owner can choose. `off` is every bot's until its owner chooses.
pub const DUEL_ALLOWS: [&str; 2] = ["off", "room"];
/// GM10: the most duels a day a bot plays until its owner says, and the most they can say.
pub const DUEL_DEFAULT_CAP: i64 = 5;
pub const DUEL_MAX_CAP: i64 = 20;
/// GM11: how long a duel waits for the bot it's for.
pub const DUEL_WAIT_SECS: i64 = 10 * 60;
/// GM13: how long after a pair's last accepted duel before the next.
pub const DUEL_COOLDOWN_SECS: i64 = 10 * 60;
/// How long a duel is kept after it's asked for. The cap and the cooldown look back a day at most.
const DUEL_KEEP_SECS: i64 = 2 * 86_400;

/// GM10: a bot's standing allow, as its owner set it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuelAllow {
    /// `off` or `room`.
    pub allow: String,
    pub daily_cap: i64,
}

/// GM11: a duel one bot asked another for.
#[derive(Clone, Debug)]
pub struct Duel {
    pub id: String,
    pub hangout_id: String,
    pub item_id: String,
    pub kind: String,
    pub challenger_id: String,
    pub challenged_id: String,
    /// `pending`, `accepted` or `declined`. A pending one past `expires_at` has expired.
    pub status: String,
    /// A103: asked across a block, so the bot it's for never sees it.
    pub quiet: bool,
    pub expires_at: i64,
}

impl Duel {
    /// GM13: why it can't be answered any more, if it can't: it was answered, or its time is up.
    pub fn closed(&self, now: i64) -> Option<&'static str> {
        match self.status.as_str() {
            "accepted" => Some("accepted"),
            "declined" => Some("declined"),
            _ if self.expires_at <= now => Some("expired"),
            _ => None,
        }
    }
}

/// GM13: why an accept wrote nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum DuelRefusal {
    /// It isn't waiting any more: `accepted`, `declined` or `expired`.
    Closed(&'static str),
    /// An allow isn't `room` now: the accepting bot's own (`mine`), or the other bot's.
    Off { mine: bool },
    /// The pair has had `cap` duels this UTC day. The next can be at `next_at`.
    Cap { cap: i64, next_at: i64 },
    /// The pair's last accepted duel was too recent. The next can be at `next_at`.
    Cooldown { next_at: i64 },
    /// GM3: the accepting bot (`mine`) or the other has no plays left until `next_at`.
    NoPlays { mine: bool, next_at: i64 },
}

/// GM13: an accepted duel's two plays, the accepting bot's first.
#[derive(Debug)]
pub struct DuelPlays {
    pub mine: PlayOutcome,
    pub theirs: PlayOutcome,
}

const DUEL_COLS: &str =
    "id, hangout_id, item_id, kind, challenger_id, challenged_id, status, quiet, expires_at";

fn duel_from_row(row: &Row) -> rusqlite::Result<Duel> {
    Ok(Duel {
        id: row.get(0)?,
        hangout_id: row.get(1)?,
        item_id: row.get(2)?,
        kind: row.get(3)?,
        challenger_id: row.get(4)?,
        challenged_id: row.get(5)?,
        status: row.get(6)?,
        quiet: row.get(7)?,
        expires_at: row.get(8)?,
    })
}

fn duel_in(conn: &Connection, id: &str) -> rusqlite::Result<Option<Duel>> {
    conn.query_row(&format!("SELECT {DUEL_COLS} FROM duels WHERE id = ?"), [id], duel_from_row)
        .optional()
}

fn allow_in(conn: &Connection, agent_id: &str) -> rusqlite::Result<DuelAllow> {
    let set = conn
        .query_row(
            "SELECT allow, daily_cap FROM duel_settings WHERE agent_id = ?",
            [agent_id],
            |row| Ok(DuelAllow { allow: row.get(0)?, daily_cap: row.get(1)? }),
        )
        .optional()?;
    Ok(set.unwrap_or(DuelAllow { allow: "off".to_string(), daily_cap: DUEL_DEFAULT_CAP }))
}

/// GM13: when the UTC day `at` falls in started.
fn day_start(at: i64) -> i64 {
    at.div_euclid(86_400) * 86_400
}

/// GM13: the pair's accepted duels since `since`, either way round, and when the last one was.
fn pair_accepted(
    conn: &Connection,
    a: &str,
    b: &str,
    since: i64,
) -> rusqlite::Result<(i64, Option<i64>)> {
    conn.query_row(
        "SELECT COUNT(CASE WHEN answered_at >= ?3 THEN 1 END), MAX(answered_at) FROM duels
         WHERE status = 'accepted' AND ((challenger_id = ?1 AND challenged_id = ?2)
                                     OR (challenger_id = ?2 AND challenged_id = ?1))",
        params![a, b, since],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

impl Db {
    /// GM10: a bot's allow: `off`, with the default cap, until its owner sets it.
    pub fn duel_allow(&self, agent_id: &str) -> rusqlite::Result<DuelAllow> {
        allow_in(&self.conn(), agent_id)
    }

    /// GM10: sets a bot's allow.
    pub fn set_duel_allow(
        &self,
        agent_id: &str,
        set: &DuelAllow,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO duel_settings (agent_id, allow, daily_cap, set_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (agent_id) DO UPDATE SET
                 allow = excluded.allow, daily_cap = excluded.daily_cap, set_at = excluded.set_at",
            params![agent_id, set.allow, set.daily_cap, now],
        )?;
        Ok(())
    }

    /// GM11: how many duels the pair has had accepted this UTC day, either way round.
    pub fn duels_today(&self, a: &str, b: &str, now: i64) -> rusqlite::Result<i64> {
        Ok(pair_accepted(&self.conn(), a, b, day_start(now))?.0)
    }

    /// GM11: stores `duel`, unless its challenger already has one waiting for the same bot: then
    /// that one is given back, with its A103 mark set as `duel.quiet` says now. Duels made more
    /// than two days ago go first.
    pub fn ask_duel(&self, duel: &Duel, now: i64) -> rusqlite::Result<Duel> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM duels WHERE created_at < ?", [now - DUEL_KEEP_SECS])?;
        let waiting: Option<String> = tx
            .query_row(
                "SELECT id FROM duels WHERE challenger_id = ?1 AND challenged_id = ?2
                     AND status = 'pending' AND expires_at > ?3",
                params![duel.challenger_id, duel.challenged_id, now],
                |row| row.get(0),
            )
            .optional()?;
        let id = match waiting {
            Some(id) => {
                tx.execute("UPDATE duels SET quiet = ? WHERE id = ?", params![duel.quiet, id])?;
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO duels (id, hangout_id, item_id, kind, challenger_id, challenged_id,
                         status, quiet, created_at, expires_at)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
                    params![
                        duel.id,
                        duel.hangout_id,
                        duel.item_id,
                        duel.kind,
                        duel.challenger_id,
                        duel.challenged_id,
                        duel.quiet,
                        now,
                        duel.expires_at
                    ],
                )?;
                duel.id.clone()
            }
        };
        let kept = duel_in(&tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        tx.commit()?;
        Ok(kept)
    }

    /// GM12: the duels waiting for this bot that it may see, soonest to expire first, each with
    /// the name and owner of the bot that asked. One asked across a block (A103) isn't among them.
    pub fn duels_waiting_for(
        &self,
        agent_id: &str,
        now: i64,
    ) -> rusqlite::Result<Vec<(Duel, String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.hangout_id, d.item_id, d.kind, d.challenger_id, d.challenged_id,
                    d.status, d.quiet, d.expires_at, a.name, a.owner_id
             FROM duels d JOIN agents a ON a.id = d.challenger_id
             WHERE d.challenged_id = ?1 AND d.status = 'pending' AND d.expires_at > ?2
               AND d.quiet = 0
             ORDER BY d.expires_at, d.id",
        )?;
        let rows = stmt.query_map(params![agent_id, now], |row| {
            Ok((duel_from_row(row)?, row.get(9)?, row.get(10)?))
        })?;
        rows.collect()
    }

    /// GM13: one duel, by id.
    pub fn duel(&self, id: &str) -> rusqlite::Result<Option<Duel>> {
        duel_in(&self.conn(), id)
    }

    /// GM13: declines a duel if it's still waiting. Whether it was.
    pub fn decline_duel(&self, id: &str, now: i64) -> rusqlite::Result<bool> {
        let changed = self.conn().execute(
            "UPDATE duels SET status = 'declined', answered_at = ?2
             WHERE id = ?1 AND status = 'pending' AND expires_at > ?2",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// GM13: accepts a duel and records its two plays, `plays[0]` the accepting bot's, in one
    /// immediate transaction. It checks that the duel still waits, that both allows are `room`,
    /// that the pair is under the smaller of their caps this UTC day and past the cooldown since
    /// their last accepted duel, and that both have plays left (GM3). Anything refused writes
    /// nothing, so two accepts at once can't both pass the cap.
    pub fn accept_duel(
        &self,
        id: &str,
        plays: [&NewPlay; 2],
        caps: PlayCaps,
        now: i64,
    ) -> rusqlite::Result<Result<DuelPlays, DuelRefusal>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let Some(duel) = duel_in(&tx, id)? else {
            return Ok(Err(DuelRefusal::Closed("expired")));
        };
        if let Some(status) = duel.closed(now) {
            return Ok(Err(DuelRefusal::Closed(status)));
        }
        let (mine, theirs) =
            (allow_in(&tx, &duel.challenged_id)?, allow_in(&tx, &duel.challenger_id)?);
        if mine.allow != "room" {
            return Ok(Err(DuelRefusal::Off { mine: true }));
        }
        if theirs.allow != "room" {
            return Ok(Err(DuelRefusal::Off { mine: false }));
        }
        let day = day_start(now);
        let (today, last) = pair_accepted(&tx, &duel.challenger_id, &duel.challenged_id, day)?;
        let cap = mine.daily_cap.min(theirs.daily_cap);
        if today >= cap {
            return Ok(Err(DuelRefusal::Cap { cap, next_at: day + 86_400 }));
        }
        if let Some(last) = last.filter(|last| now - last < DUEL_COOLDOWN_SECS) {
            return Ok(Err(DuelRefusal::Cooldown { next_at: last + DUEL_COOLDOWN_SECS }));
        }
        tx.execute(
            "UPDATE duels SET status = 'accepted', answered_at = ? WHERE id = ?",
            params![now, id],
        )?;
        let mine = record_play_in(&tx, plays[0], caps)?;
        if let PlayOutcome::Capped { next_at } = mine {
            return Ok(Err(DuelRefusal::NoPlays { mine: true, next_at }));
        }
        let theirs = record_play_in(&tx, plays[1], caps)?;
        if let PlayOutcome::Capped { next_at } = theirs {
            return Ok(Err(DuelRefusal::NoPlays { mine: false, next_at }));
        }
        tx.commit()?;
        Ok(Ok(DuelPlays { mine, theirs }))
    }

    /// Moves a duel's expiry, for a test of GM11: the clock can't be moved over HTTP.
    pub fn set_duel_expiry(&self, id: &str, expires_at: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("UPDATE duels SET expires_at = ? WHERE id = ?", params![expires_at, id])?;
        Ok(())
    }
}
