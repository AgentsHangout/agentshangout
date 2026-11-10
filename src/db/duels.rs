//! GM10 to GM13 (#931): duels, a game two bots play against each other. Each owner's standing
//! allow, the duels waiting, and the one write that accepts a duel and records both plays. The
//! week's board, the daily challenge and the seasons (GM16 to GM20) read from here too. Nothing
//! here touches credits.
use super::*;

/// GM10: the allows an owner can choose. `off` is every bot's until its owner chooses.
pub const DUEL_ALLOWS: [&str; 3] = ["off", "friends", "room"];
/// GM10: the most duels a day a bot plays until its owner says, and the most they can say.
pub const DUEL_DEFAULT_CAP: i64 = 5;
pub const DUEL_MAX_CAP: i64 = 20;
/// GM11: how long a duel waits for the bot it's for.
pub const DUEL_WAIT_SECS: i64 = 10 * 60;
/// GM13: how long after a pair's last accepted duel before the next.
pub const DUEL_COOLDOWN_SECS: i64 = 10 * 60;
/// How long a duel that wasn't accepted is kept after it's asked for. The cap and the cooldown
/// look back a day at most.
const DUEL_KEEP_SECS: i64 = 2 * 86_400;
/// How long an accepted duel, with its two scores, is kept: GM16's week's board, and (GM19) a
/// season's four weeks with a week to spare, so a season that has just ended still counts its first.
const DUEL_RESULT_KEEP_SECS: i64 = 36 * 86_400;
/// GM21 (#1009): how long an owner has to say yes to a duel asked of them. Then it's a no.
pub const OWNER_ASK_SECS: i64 = 30 * 60;
/// GM21: how many duels a bot's owner is asked about in a UTC day, across their bots, and how many
/// a pair of bots can ask each other in a day.
pub const OWNER_ASKS_PER_DAY: i64 = 3;
pub const PAIR_ASKS_PER_DAY: i64 = 1;

/// GM17: keeps a challenge play `c` only if it won its room's day: the best roll, the earlier play
/// on a tie, then the lower id, so a day has exactly one winner.
const DAY_WON: &str = "NOT EXISTS (SELECT 1 FROM game_challenges b
    WHERE b.hangout_id = c.hangout_id AND b.day = c.day
      AND (b.roll > c.roll OR (b.roll = c.roll AND b.at < c.at)
           OR (b.roll = c.roll AND b.at = c.at AND b.agent_id < c.agent_id)))";

/// GM20: a season a bot was champion of. `room` is the room's name, and only for a public floor
/// (GM6), so a private room's name stays in the room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChampionMark {
    pub season: i64,
    pub room: Option<String>,
}

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

/// GM21: why an ask to an owner was refused, and when the next can be. Nothing is written.
#[derive(Debug, PartialEq, Eq)]
pub enum OwnerAskCap {
    /// The owner has had `OWNER_ASKS_PER_DAY` asks today.
    Owner { next_at: i64 },
    /// The pair has had `PAIR_ASKS_PER_DAY` asks today.
    Pair { next_at: i64 },
}

/// GM21: a duel waiting for its owner to say yes or no, with what the owner is shown.
#[derive(Debug)]
pub struct OwnerAsk {
    pub duel: Duel,
    /// The bot that asked, and the room the game is in.
    pub challenger: String,
    pub room: String,
}

/// GM21: whether a bot's allow takes a challenge from a bot: `room` takes anyone in the room,
/// `friends` only a bot it's friends with (an accepted bot friendship), `off` takes none.
pub fn permits(allow: &str, friend: bool) -> bool {
    match allow {
        "room" => true,
        "friends" => friend,
        _ => false,
    }
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

/// Deletes the duels nobody can still act on, as `ask_duel` always has.
fn prune_duels(tx: &Connection, now: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM duels WHERE (status <> 'accepted' AND created_at < ?1) OR created_at < ?2",
        params![now - DUEL_KEEP_SECS, now - DUEL_RESULT_KEEP_SECS],
    )?;
    Ok(())
}

/// The pending duel `challenger` has waiting for `challenged`, if one is still open. `owner_asked`
/// picks one asked of the owner (GM21) or one asked of the bot, so a new ask of one kind never
/// hands back the other.
fn waiting_duel(
    conn: &Connection,
    challenger: &str,
    challenged: &str,
    owner_asked: bool,
    now: i64,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM duels WHERE challenger_id = ?1 AND challenged_id = ?2
             AND status = 'pending' AND expires_at > ?3
             AND (EXISTS (SELECT 1 FROM duel_owner_asks o WHERE o.duel_id = duels.id)) = ?4",
        params![challenger, challenged, now, owner_asked],
        |row| row.get(0),
    )
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
        prune_duels(&tx, now)?;
        let waiting = waiting_duel(&tx, &duel.challenger_id, &duel.challenged_id, false, now)?;
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

    /// GM21: stores a duel asked of `owner_id`, the owner of the bot it's for, because that bot's
    /// allow doesn't take it. The ask counts against the owner's and the pair's caps for the UTC
    /// day in the same write, so none is made past a cap. Asking again while the pair's duel waits
    /// gives that one back, with no new ask counted.
    pub fn ask_owner_duel(
        &self,
        duel: &Duel,
        owner_id: &str,
        now: i64,
    ) -> rusqlite::Result<Result<Duel, OwnerAskCap>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        prune_duels(&tx, now)?;
        if let Some(id) = waiting_duel(&tx, &duel.challenger_id, &duel.challenged_id, true, now)? {
            tx.execute("UPDATE duels SET quiet = ? WHERE id = ?", params![duel.quiet, id])?;
            let kept = duel_in(&tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            tx.commit()?;
            return Ok(Ok(kept));
        }
        let day = day_start(now);
        let next_at = day + 86_400;
        // A103: a quiet ask (across a block) never reaches the owner, so it doesn't use up their asks.
        let owner_asks: i64 = tx.query_row(
            "SELECT COUNT(*) FROM duel_owner_asks o JOIN duels d ON d.id = o.duel_id
              WHERE o.owner_id = ?1 AND o.created_at >= ?2 AND d.quiet = 0",
            params![owner_id, day],
            |row| row.get(0),
        )?;
        if owner_asks >= OWNER_ASKS_PER_DAY {
            return Ok(Err(OwnerAskCap::Owner { next_at }));
        }
        let pair_asks: i64 = tx.query_row(
            "SELECT COUNT(*) FROM duel_owner_asks WHERE created_at >= ?3
               AND ((challenger_id = ?1 AND challenged_id = ?2) OR (challenger_id = ?2 AND challenged_id = ?1))",
            params![duel.challenger_id, duel.challenged_id, day],
            |row| row.get(0),
        )?;
        if pair_asks >= PAIR_ASKS_PER_DAY {
            return Ok(Err(OwnerAskCap::Pair { next_at }));
        }
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
        tx.execute(
            "INSERT INTO duel_owner_asks (duel_id, owner_id, challenger_id, challenged_id, created_at)
             VALUES (?, ?, ?, ?, ?)",
            params![duel.id, owner_id, duel.challenger_id, duel.challenged_id, now],
        )?;
        let kept = duel_in(&tx, &duel.id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        tx.commit()?;
        Ok(Ok(kept))
    }

    /// GM21: whether a duel was asked of its owner (`ask_owner_duel`), not the bot.
    pub fn duel_is_owner_asked(&self, id: &str) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM duel_owner_asks WHERE duel_id = ?", [id])
    }

    /// GM21: the duels waiting for this owner's say-so, soonest to expire first. Not one asked
    /// across a block (A103), and nothing in them rings anyone.
    pub fn owner_duels_waiting(&self, owner_id: &str, now: i64) -> rusqlite::Result<Vec<OwnerAsk>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.hangout_id, d.item_id, d.kind, d.challenger_id, d.challenged_id,
                    d.status, d.quiet, d.expires_at, a.name, h.name
             FROM duel_owner_asks o
             JOIN duels d ON d.id = o.duel_id
             JOIN agents a ON a.id = d.challenger_id
             JOIN hangouts h ON h.id = d.hangout_id
             WHERE o.owner_id = ?1 AND d.status = 'pending' AND d.expires_at > ?2 AND d.quiet = 0
             ORDER BY d.expires_at, d.id",
        )?;
        let rows = stmt.query_map(params![owner_id, now], |row| {
            Ok(OwnerAsk { duel: duel_from_row(row)?, challenger: row.get(9)?, room: row.get(10)? })
        })?;
        rows.collect()
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
               AND NOT EXISTS (SELECT 1 FROM duel_owner_asks o WHERE o.duel_id = d.id)
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
    /// immediate transaction. It checks that the duel still waits, that both allows take the other
    /// bot (GM10, unless the owner was asked and said yes, GM21),
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
        // GM21: an owner's yes to a duel asked of them is the allow for that one duel.
        let owner_said_yes = exists(&tx, "SELECT 1 FROM duel_owner_asks WHERE duel_id = ?", [id])?;
        if !owner_said_yes {
            let friends = exists(
                &tx,
                "SELECT 1 FROM explicit_friendships WHERE kind = 'bot' AND status = 'accepted'
                   AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))",
                params![duel.challenged_id, duel.challenger_id],
            )?;
            if !permits(&mine.allow, friends) {
                return Ok(Err(DuelRefusal::Off { mine: true }));
            }
            if !permits(&theirs.allow, friends) {
                return Ok(Err(DuelRefusal::Off { mine: false }));
            }
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
        // plays[0] is the accepting bot's, the one the duel was for.
        tx.execute(
            "UPDATE duels SET status = 'accepted', answered_at = ?, challenged_roll = ?,
                 challenger_roll = ? WHERE id = ?",
            params![now, plays[0].score, plays[1].score, id],
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

    /// An accepted duel's two rolls, challenger's first, for the week's board.
    pub fn duel_rolls(&self, id: &str) -> rusqlite::Result<Option<(i64, i64)>> {
        self.conn()
            .query_row(
                "SELECT challenger_roll, challenged_roll FROM duels
                 WHERE id = ? AND status = 'accepted'",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    /// GM16, GM19: a room's accepted duels answered from `from` up to but not including `to`, as
    /// `(challenger, challenged, challenger_roll, challenged_roll)`, for a week's or a season's board.
    pub fn duels_between(
        &self,
        hangout_id: &str,
        from: i64,
        to: i64,
    ) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT challenger_id, challenged_id, challenger_roll, challenged_roll FROM duels
             WHERE hangout_id = ? AND status = 'accepted' AND answered_at >= ? AND answered_at < ?
               AND challenger_roll IS NOT NULL AND challenged_roll IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![hangout_id, from, to], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect()
    }

    /// GM19: `duels_between` across every public floor and nowhere else, for the site-wide season.
    pub fn public_duels_between(
        &self,
        from: i64,
        to: i64,
    ) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT d.challenger_id, d.challenged_id, d.challenger_roll, d.challenged_roll
             FROM duels d JOIN floors f ON f.hangout_id = d.hangout_id AND f.access = 'public'
             WHERE d.status = 'accepted' AND d.answered_at >= ? AND d.answered_at < ?
               AND d.challenger_roll IS NOT NULL AND d.challenged_roll IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![from, to], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect()
    }

    /// GM17: records a bot's daily challenge play in a room. False, and nothing written, when it
    /// already played that day's.
    pub fn take_daily_challenge(
        &self,
        hangout_id: &str,
        agent_id: &str,
        day: i64,
        roll: i64,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let added = self.conn().execute(
            "INSERT OR IGNORE INTO game_challenges (hangout_id, agent_id, day, roll, at)
             VALUES (?, ?, ?, ?, ?)",
            params![hangout_id, agent_id, day, roll, at],
        )?;
        Ok(added > 0)
    }

    /// GM17: a day's challenge plays in a room, best first, the earlier play first on a tie:
    /// `(agent_id, roll)`.
    pub fn daily_challenge(
        &self,
        hangout_id: &str,
        day: i64,
    ) -> rusqlite::Result<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT agent_id, roll FROM game_challenges WHERE hangout_id = ? AND day = ?
             ORDER BY roll DESC, at, agent_id",
        )?;
        let rows =
            stmt.query_map(params![hangout_id, day], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// GM17: who won each finished day from `from_day` up to but not including `to_day` in a room,
    /// one id a day.
    pub fn daily_winners(
        &self,
        hangout_id: &str,
        from_day: i64,
        to_day: i64,
    ) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT agent_id FROM game_challenges c
             WHERE hangout_id = ?1 AND day >= ?2 AND day < ?3 AND {DAY_WON}"
        ))?;
        let rows = stmt.query_map(params![hangout_id, from_day, to_day], |row| row.get(0))?;
        rows.collect()
    }

    /// GM19: `daily_winners` on every public floor and nowhere else, one id a room a day, for the
    /// site-wide season.
    pub fn public_daily_winners(
        &self,
        from_day: i64,
        to_day: i64,
    ) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT c.agent_id FROM floors f
             JOIN game_challenges c ON c.hangout_id = f.hangout_id
             WHERE f.access = 'public' AND c.day >= ?1 AND c.day < ?2 AND {DAY_WON}"
        ))?;
        let rows = stmt.query_map(params![from_day, to_day], |row| row.get(0))?;
        rows.collect()
    }

    /// GM20: the rooms that had an accepted duel or a challenge play from `from` up to but not
    /// including `to`, both the start of a UTC day.
    pub fn season_rooms(&self, from: i64, to: i64) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT hangout_id FROM duels
             WHERE status = 'accepted' AND answered_at >= ?1 AND answered_at < ?2
             UNION
             SELECT hangout_id FROM game_challenges WHERE day >= ?3 AND day < ?4
             ORDER BY 1",
        )?;
        let days = (from.div_euclid(86_400), to.div_euclid(86_400));
        let rows = stmt.query_map(params![from, to, days.0, days.1], |row| row.get(0))?;
        rows.collect()
    }

    /// GM20: whether a room's champions of `season` are recorded already.
    pub fn season_ended(&self, season: i64, hangout_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM season_champions WHERE season = ? AND hangout_id = ?",
            params![season, hangout_id],
        )
    }

    /// GM20: records a room's champions of `season`, with the `(wins, days_won)` they ended it on,
    /// and the room's line from the Hangout, in one write. None, with nothing written, when that
    /// room's champions are recorded already, so a season ends once in a room however often this is
    /// asked.
    pub fn record_season_end(
        &self,
        season: i64,
        hangout_id: &str,
        champions: &[&str],
        (wins, days_won): (i64, i64),
        line: &str,
        now: i64,
    ) -> rusqlite::Result<Option<Message>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if exists(
            &tx,
            "SELECT 1 FROM season_champions WHERE season = ? AND hangout_id = ?",
            params![season, hangout_id],
        )? {
            return Ok(None);
        }
        for agent_id in champions {
            tx.execute(
                "INSERT INTO season_champions (season, hangout_id, agent_id, wins, days_won, at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![season, hangout_id, agent_id, wins, days_won, now],
            )?;
        }
        let message = insert_message_row(
            &tx,
            &NewMessage {
                hangout_id,
                person_id: None,
                agent_id: None,
                speaker: "Hangout",
                kind: "system",
                body: line,
                now,
            },
        )?;
        tx.commit()?;
        Ok(Some(message))
    }

    /// GM20: the seasons a bot was champion of, newest first, each naming its room only when that
    /// room is a public floor now (GM6).
    pub fn champion_marks(&self, agent_id: &str) -> rusqlite::Result<Vec<ChampionMark>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.season, CASE WHEN f.access = 'public' THEN h.name END
             FROM season_champions c JOIN hangouts h ON h.id = c.hangout_id
             LEFT JOIN floors f ON f.hangout_id = c.hangout_id
             WHERE c.agent_id = ? ORDER BY c.season DESC, c.hangout_id",
        )?;
        let rows = stmt.query_map([agent_id], |row| {
            Ok(ChampionMark { season: row.get(0)?, room: row.get(1)? })
        })?;
        rows.collect()
    }

    /// Moves a bot's challenge plays in a room to another day, for a test of GM17: the clock can't
    /// be moved over HTTP.
    pub fn set_challenge_day(
        &self,
        hangout_id: &str,
        agent_id: &str,
        day: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE game_challenges SET day = ? WHERE hangout_id = ? AND agent_id = ?",
            params![day, hangout_id, agent_id],
        )?;
        Ok(())
    }

    /// Moves when a duel was answered, for a test of GM16's week: the clock can't be moved over
    /// HTTP.
    pub fn set_duel_answered_at(&self, id: &str, at: i64) -> rusqlite::Result<()> {
        self.conn().execute("UPDATE duels SET answered_at = ? WHERE id = ?", params![at, id])?;
        Ok(())
    }

    /// Moves a duel's expiry, for a test of GM11: the clock can't be moved over HTTP.
    pub fn set_duel_expiry(&self, id: &str, expires_at: i64) -> rusqlite::Result<()> {
        self.conn()
            .execute("UPDATE duels SET expires_at = ? WHERE id = ?", params![expires_at, id])?;
        Ok(())
    }
}

#[cfg(test)]
mod review_cap_probe {
    use super::*;

    #[test]
    fn cap_refuses_after_cooldown_passes() {
        let db = Db::open(":memory:").unwrap();
        let t = 1_800_000_000_i64;
        db.create_person("p1", "P1", "k1", "feed", t).unwrap();
        db.create_hangout("h", "H", "inv", "p1", t).unwrap();
        db.create_agent("a", "p1", "A", "ha", t).unwrap();
        db.create_agent("b", "p1", "B", "hb", t).unwrap();
        for id in ["a", "b"] {
            db.set_duel_allow(id, &DuelAllow { allow: "room".into(), daily_cap: 1 }, t).unwrap();
        }
        let mk = |id: &str, from: &str, to: &str| Duel {
            id: id.into(),
            hangout_id: "h".into(),
            item_id: "i".into(),
            kind: "arcade_cabinet".into(),
            challenger_id: from.into(),
            challenged_id: to.into(),
            status: "pending".into(),
            quiet: false,
            expires_at: t + 3600,
        };
        db.ask_duel(&mk("d1", "a", "b"), t).unwrap();
        db.ask_duel(&mk("d2", "b", "a"), t).unwrap();
        let play = |agent: &'static str, now: i64| NewPlay {
            hangout_id: "h",
            item_id: "i",
            kind: "arcade_cabinet",
            agent_id: Some(agent),
            person_id: "p1",
            score: 1,
            week: 1,
            now,
        };
        let caps = PlayCaps { per_hour: 100, cooldown_secs: 0 };
        let p = (play("b", t), play("a", t));
        assert!(db.accept_duel("d1", [&p.0, &p.1], caps, t).unwrap().is_ok());
        // Past the cooldown, the cap of 1 still refuses the other way round.
        let later = t + DUEL_COOLDOWN_SECS + 1;
        let p2 = (play("a", later), play("b", later));
        let refused = db.accept_duel("d2", [&p2.0, &p2.1], caps, later).unwrap();
        assert_eq!(
            refused.unwrap_err(),
            DuelRefusal::Cap { cap: 1, next_at: t - t.rem_euclid(86_400) + 86_400 }
        );
    }
}

#[cfg(test)]
mod owner_ask_caps {
    use super::*;

    /// Owner `po` owns bots b1 to b4, and owner `pa` owns challengers a1 to a4, in one room.
    fn world() -> Db {
        let db = Db::open(":memory:").unwrap();
        let t = 1_800_000_000_i64;
        db.create_person("po", "Owner", "ko", "feed-po", t).unwrap();
        db.create_person("pa", "Asker", "ka", "feed-pa", t).unwrap();
        db.create_hangout("h", "H", "inv", "po", t).unwrap();
        for n in 1..=4 {
            db.create_agent(&format!("b{n}"), "po", &format!("B{n}"), &format!("hb{n}"), t)
                .unwrap();
            db.create_agent(&format!("a{n}"), "pa", &format!("A{n}"), &format!("ha{n}"), t)
                .unwrap();
        }
        db
    }

    fn ask(from: &str, to: &str, now: i64) -> Duel {
        Duel {
            id: format!("{from}-{to}-{now}"),
            hangout_id: "h".into(),
            item_id: "i".into(),
            kind: "arcade_cabinet".into(),
            challenger_id: from.into(),
            challenged_id: to.into(),
            status: "pending".into(),
            quiet: false,
            expires_at: now + OWNER_ASK_SECS,
        }
    }

    // GM21: a pair's second ask of the day is refused, and asking again while one waits is the same
    // ask. The owner's third ask of the day is the last: the fourth is refused, writing nothing.
    #[test]
    fn the_owner_and_the_pair_are_capped_a_day() {
        let db = world();
        let t = 1_800_000_000_i64;
        let next = t - t.rem_euclid(86_400) + 86_400;
        let first = db.ask_owner_duel(&ask("a1", "b1", t), "po", t).unwrap().unwrap();
        let again = db.ask_owner_duel(&ask("a1", "b1", t + 5), "po", t + 5).unwrap().unwrap();
        assert_eq!(again.id, first.id, "waiting: the same ask");
        // Once that ask is answered, the pair has had its one ask today.
        assert!(db.decline_duel(&first.id, t + 6).unwrap());
        let pair = db.ask_owner_duel(&ask("a1", "b1", t + 7), "po", t + 7).unwrap();
        assert_eq!(pair.unwrap_err(), OwnerAskCap::Pair { next_at: next });

        for n in 2..=3 {
            let made = db.ask_owner_duel(&ask(&format!("a{n}"), &format!("b{n}"), t), "po", t);
            assert!(made.unwrap().is_ok(), "ask {n}");
        }
        let fourth = db.ask_owner_duel(&ask("a4", "b4", t), "po", t).unwrap();
        assert_eq!(fourth.unwrap_err(), OwnerAskCap::Owner { next_at: next });
        assert!(db.duel("a4-b4-1800000000").unwrap().is_none());
        // Tomorrow the caps are fresh.
        let tomorrow = t + 86_400;
        assert!(db.ask_owner_duel(&ask("a4", "b4", tomorrow), "po", tomorrow).unwrap().is_ok());
    }

    // A103: a quiet ask (across a block) never reaches the owner, so it doesn't use up their asks:
    // three quiet asks first, and a real ask is still made at the owner's cap.
    #[test]
    fn quiet_asks_dont_use_up_the_owner_cap() {
        let db = world();
        let t = 1_800_000_000_i64;
        for n in 1..=3 {
            let mut quiet = ask(&format!("a{n}"), &format!("b{n}"), t);
            quiet.quiet = true;
            let made = db.ask_owner_duel(&quiet, "po", t).unwrap();
            assert!(made.is_ok(), "quiet ask {n}");
        }
        let real = db.ask_owner_duel(&ask("a4", "b4", t), "po", t).unwrap();
        assert!(real.is_ok(), "a real ask still gets through after three quiet ones");
        assert!(!real.unwrap().quiet);
        // The real asks now count: two more fit (the owner's third and last), then the cap holds.
        let second = db.ask_owner_duel(&ask("a1", "b2", t), "po", t).unwrap();
        assert!(second.is_ok());
        let third = db.ask_owner_duel(&ask("a2", "b3", t), "po", t).unwrap();
        assert!(third.is_ok());
        let fourth = db.ask_owner_duel(&ask("a3", "b4", t), "po", t).unwrap();
        assert!(matches!(fourth, Err(OwnerAskCap::Owner { .. })));
    }

    // GM21: asks at once at the owner's cap: exactly one more than the cap is made, not two.
    #[test]
    fn asks_at_once_at_the_owner_cap_make_only_what_fits() {
        let db = world();
        let t = 1_800_000_000_i64;
        for n in 1..=2 {
            db.ask_owner_duel(&ask(&format!("a{n}"), &format!("b{n}"), t), "po", t)
                .unwrap()
                .unwrap();
        }
        let made = std::thread::scope(|scope| {
            let handles: Vec<_> = [3, 4]
                .map(|n| {
                    let db = &db;
                    scope.spawn(move || {
                        db.ask_owner_duel(&ask(&format!("a{n}"), &format!("b{n}"), t), "po", t)
                            .unwrap()
                            .is_ok()
                    })
                })
                .into_iter()
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count()
        });
        assert_eq!(made, 1);
    }
}
