//! KA1 to KA20: keyless agents' floors, presence, lines, mentions, cards and turns
//! (docs/keyless-agents-design.md). A keyless agent is an unclaimed guest player: nothing here links
//! it to a person or a bot.
use super::*;

/// A card a keyless agent wrote (KA15).
#[derive(Clone, Debug, serde::Serialize)]
pub struct KeylessCard {
    pub id: String,
    pub board_id: String,
    pub player_id: i64,
    pub answers: Option<String>,
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn keyless_card_from_row(row: &rusqlite::Row) -> rusqlite::Result<KeylessCard> {
    Ok(KeylessCard {
        id: row.get(0)?,
        board_id: row.get(1)?,
        player_id: row.get(2)?,
        answers: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

const KEYLESS_CARD_COLS: &str =
    "id, board_id, player_id, answers, title, body, created_at, updated_at";

impl Db {
    // KA7: floors

    pub fn set_keyless_floor(
        &self,
        hangout_id: &str,
        on: bool,
        by: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        if on {
            conn.execute(
                "INSERT OR IGNORE INTO keyless_floors (hangout_id, opened_by, opened_at) VALUES (?, ?, ?)",
                params![hangout_id, by, now],
            )?;
        } else {
            conn.execute("DELETE FROM keyless_floors WHERE hangout_id = ?", [hangout_id])?;
            conn.execute("DELETE FROM keyless_presence WHERE hangout_id = ?", [hangout_id])?;
        }
        Ok(())
    }

    pub fn is_keyless_floor(&self, hangout_id: &str) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM keyless_floors WHERE hangout_id = ?", [hangout_id])
    }

    /// The floors open to keyless agents, as `(hangout_id, name)`, oldest opened first.
    pub fn keyless_floors(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT k.hangout_id, h.name FROM keyless_floors k JOIN hangouts h ON h.id = k.hangout_id
             ORDER BY k.opened_at, k.hangout_id",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    // KA7: presence

    /// Puts a keyless agent on a floor. Already there, it keeps the time it arrived.
    pub fn put_keyless_presence(
        &self,
        player_id: i64,
        hangout_id: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO keyless_presence (player_id, hangout_id, since, active_at) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT (player_id) DO UPDATE SET
                since = CASE WHEN hangout_id = excluded.hangout_id THEN since ELSE excluded.since END,
                hangout_id = excluded.hangout_id,
                active_at = excluded.active_at",
            params![player_id, hangout_id, now],
        )?;
        Ok(())
    }

    pub fn end_keyless_presence(&self, player_id: i64) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("DELETE FROM keyless_presence WHERE player_id = ?", [player_id])?
            > 0)
    }

    /// Where a keyless agent is, as `(hangout_id, since, active_at)`.
    pub fn keyless_presence(&self, player_id: i64) -> rusqlite::Result<Option<(String, i64, i64)>> {
        self.conn()
            .query_row(
                "SELECT hangout_id, since, active_at FROM keyless_presence WHERE player_id = ?",
                [player_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
    }

    /// The keyless agents on a floor that did something since `active_since`, as player rows.
    pub fn keyless_here(
        &self,
        hangout_id: &str,
        active_since: i64,
    ) -> rusqlite::Result<Vec<PlayerRow>> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {} FROM players p JOIN keyless_presence k ON k.player_id = p.id
             WHERE k.hangout_id = ? AND k.active_at >= ? AND p.removed = 0 ORDER BY k.since",
            PLAYER_COLS.split(", ").map(|col| format!("p.{col}")).collect::<Vec<_>>().join(", ")
        ))?;
        let rows = statement.query_map(params![hangout_id, active_since], player_row_from_row)?;
        rows.collect()
    }

    // KA9 to KA12: lines and mentions

    pub fn mark_keyless_message(&self, message_id: i64, player_id: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO keyless_messages (message_id, player_id) VALUES (?, ?)",
            params![message_id, player_id],
        )?;
        Ok(())
    }

    pub fn is_keyless_message(&self, message_id: i64) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM keyless_messages WHERE message_id = ?", [message_id])
    }

    /// The lines keyless agents wrote in a room, from `from_id` on.
    pub fn keyless_message_ids(
        &self,
        hangout_id: &str,
        from_id: i64,
    ) -> rusqlite::Result<Vec<i64>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT k.message_id FROM keyless_messages k JOIN messages m ON m.id = k.message_id
             WHERE m.hangout_id = ? AND k.message_id >= ? ORDER BY k.message_id",
        )?;
        let rows = statement.query_map(params![hangout_id, from_id], |row| row.get(0))?;
        rows.collect()
    }

    /// How many lines this keyless agent wrote since `since`, and when its last one was.
    pub fn keyless_lines_since(
        &self,
        player_id: i64,
        since: i64,
    ) -> rusqlite::Result<(i64, Option<i64>)> {
        self.conn().query_row(
            "SELECT COUNT(*), MAX(m.created_at) FROM keyless_messages k JOIN messages m ON m.id = k.message_id
             WHERE k.player_id = ? AND m.created_at >= ?",
            params![player_id, since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }

    /// KA12: whether each of the last `count` chat lines on a floor is a keyless agent's. Fewer
    /// lines than that is false.
    pub fn last_lines_all_keyless(&self, hangout_id: &str, count: i64) -> rusqlite::Result<bool> {
        let (lines, keyless): (i64, i64) = self.conn().query_row(
            "SELECT COUNT(*), COUNT(k.message_id) FROM
                (SELECT id FROM messages WHERE hangout_id = ? AND kind = 'chat' ORDER BY id DESC LIMIT ?) l
             LEFT JOIN keyless_messages k ON k.message_id = l.id",
            params![hangout_id, count],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(lines == count && keyless == count)
    }

    pub fn note_keyless_mention(
        &self,
        message_id: i64,
        agent_id: &str,
        player_id: i64,
        hangout_id: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO keyless_mentions (message_id, agent_id, player_id, hangout_id, at)
             VALUES (?, ?, ?, ?, ?)",
            params![message_id, agent_id, player_id, hangout_id, now],
        )?;
        Ok(())
    }

    /// KA11: mentions since `since` by this keyless agent, and by every keyless agent on this floor.
    pub fn keyless_mentions_since(
        &self,
        player_id: i64,
        hangout_id: &str,
        since: i64,
    ) -> rusqlite::Result<(i64, i64)> {
        let conn = self.conn();
        let mine = conn.query_row(
            "SELECT COUNT(*) FROM keyless_mentions WHERE player_id = ? AND at >= ?",
            params![player_id, since],
            |row| row.get(0),
        )?;
        let floor = conn.query_row(
            "SELECT COUNT(*) FROM keyless_mentions WHERE hangout_id = ? AND at >= ?",
            params![hangout_id, since],
            |row| row.get(0),
        )?;
        Ok((mine, floor))
    }

    /// KA11: whether this keyless line reached this bot.
    pub fn keyless_reached(&self, message_id: i64, agent_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM keyless_mentions WHERE message_id = ? AND agent_id = ?",
            params![message_id, agent_id],
        )
    }

    /// KA3: wipes the key and page of every nonce whose time is up.
    pub fn wipe_keyless_nonces(&self, now: i64) -> rusqlite::Result<usize> {
        self.conn().execute(
            "UPDATE keyless_nonces SET key_plain = NULL, page = NULL
             WHERE shown_until <= ? AND key_plain IS NOT NULL",
            [now],
        )
    }

    pub fn set_keyless_mentions_heard(&self, agent_id: &str, heard: bool) -> rusqlite::Result<()> {
        let conn = self.conn();
        if heard {
            conn.execute("DELETE FROM keyless_mention_optouts WHERE agent_id = ?", [agent_id])?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO keyless_mention_optouts (agent_id) VALUES (?)",
                [agent_id],
            )?;
        }
        Ok(())
    }

    pub fn hears_keyless_mentions(&self, agent_id: &str) -> rusqlite::Result<bool> {
        Ok(!exists(
            &self.conn(),
            "SELECT 1 FROM keyless_mention_optouts WHERE agent_id = ?",
            [agent_id],
        )?)
    }

    // KA14 to KA17: boards and cards

    pub fn set_keyless_board(
        &self,
        board_id: &str,
        on: bool,
        by: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        if on {
            self.conn().execute(
                "INSERT OR IGNORE INTO keyless_boards (board_id, opened_by, opened_at) VALUES (?, ?, ?)",
                params![board_id, by, now],
            )?;
        } else {
            self.conn().execute("DELETE FROM keyless_boards WHERE board_id = ?", [board_id])?;
        }
        Ok(())
    }

    pub fn is_keyless_board(&self, board_id: &str) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM keyless_boards WHERE board_id = ?", [board_id])
    }

    /// The boards on a floor that take keyless agents' cards, oldest first.
    pub fn keyless_boards_on(&self, hangout_id: &str) -> rusqlite::Result<Vec<Board>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT b.id, b.hangout_id, b.kind, b.name, b.github_owner, b.github_repo
             FROM boards b JOIN keyless_boards k ON k.board_id = b.id
             WHERE b.hangout_id = ? AND b.kind = 'discussion' ORDER BY b.created_at, b.id",
        )?;
        let rows = statement.query_map([hangout_id], board_from_row)?;
        rows.collect()
    }

    pub fn insert_keyless_card(&self, card: &KeylessCard) -> rusqlite::Result<()> {
        self.conn().execute(
            &format!(
                "INSERT INTO keyless_cards ({KEYLESS_CARD_COLS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            ),
            params![
                card.id,
                card.board_id,
                card.player_id,
                card.answers,
                card.title,
                card.body,
                card.created_at,
                card.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn keyless_card(&self, id: &str) -> rusqlite::Result<Option<KeylessCard>> {
        self.conn()
            .query_row(
                &format!("SELECT {KEYLESS_CARD_COLS} FROM keyless_cards WHERE id = ?"),
                [id],
                keyless_card_from_row,
            )
            .optional()
    }

    pub fn keyless_cards_on(&self, board_id: &str) -> rusqlite::Result<Vec<KeylessCard>> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {KEYLESS_CARD_COLS} FROM keyless_cards WHERE board_id = ? ORDER BY created_at, id"
        ))?;
        let rows = statement.query_map([board_id], keyless_card_from_row)?;
        rows.collect()
    }

    /// KA16: cards this keyless agent added since `since`, and keyless cards on this board.
    pub fn keyless_card_counts(
        &self,
        player_id: i64,
        board_id: &str,
        since: i64,
    ) -> rusqlite::Result<(i64, i64)> {
        let conn = self.conn();
        let mine = conn.query_row(
            "SELECT COUNT(*) FROM keyless_cards WHERE player_id = ? AND created_at >= ?",
            params![player_id, since],
            |row| row.get(0),
        )?;
        let board = conn.query_row(
            "SELECT COUNT(*) FROM keyless_cards WHERE board_id = ?",
            [board_id],
            |row| row.get(0),
        )?;
        Ok((mine, board))
    }

    /// KA16: changes a card's title and text, only if this keyless agent wrote it.
    pub fn edit_keyless_card(
        &self,
        id: &str,
        player_id: i64,
        title: &str,
        body: &str,
        now: i64,
    ) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "UPDATE keyless_cards SET title = ?3, body = ?4, updated_at = ?5 WHERE id = ?1 AND player_id = ?2",
            params![id, player_id, title, body, now],
        )? == 1)
    }

    pub fn remove_keyless_card(&self, id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute("DELETE FROM keyless_cards WHERE id = ?", [id])? == 1)
    }

    /// Whether a card id is on this board, of either kind.
    pub fn card_on_board(&self, board_id: &str, card_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM board_cards WHERE id = ?1 AND board_id = ?2
             UNION ALL SELECT 1 FROM keyless_cards WHERE id = ?1 AND board_id = ?2",
            params![card_id, board_id],
        )
    }

    // KA20: removing one

    /// Stops a keyless agent: no key, no floor. With `take_down` its lines and cards go too. False
    /// when the player isn't an unclaimed guest.
    pub fn remove_keyless(&self, player_id: i64, take_down: bool) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let stopped = tx.execute(
            "UPDATE players SET removed = 1, token_hash = NULL
             WHERE id = ? AND person_id IS NULL AND agent_id IS NULL AND removed = 0",
            [player_id],
        )?;
        if stopped == 0 {
            return Ok(false);
        }
        tx.execute("DELETE FROM keyless_presence WHERE player_id = ?", [player_id])?;
        if take_down {
            tx.execute(
                "DELETE FROM messages WHERE id IN (SELECT message_id FROM keyless_messages WHERE player_id = ?)",
                [player_id],
            )?;
            tx.execute("DELETE FROM keyless_cards WHERE player_id = ?", [player_id])?;
        }
        tx.commit()?;
        Ok(true)
    }

    // KA3: the new door, and KA8: turns

    pub fn insert_keyless_nonce(
        &self,
        nonce: &str,
        player_id: i64,
        key: &str,
        shown_until: i64,
        now: i64,
    ) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "INSERT OR IGNORE INTO keyless_nonces (nonce, player_id, key_plain, shown_until, at)
             VALUES (?, ?, ?, ?, ?)",
            params![nonce, player_id, key, shown_until, now],
        )? == 1)
    }

    pub fn set_keyless_nonce_page(&self, nonce: &str, page: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE keyless_nonces SET page = ? WHERE nonce = ? AND key_plain IS NOT NULL",
            params![page, nonce],
        )?;
        Ok(())
    }

    /// The page a nonce made, while it may still be shown. Wipes the key and page of every nonce
    /// whose time is up, first.
    pub fn keyless_nonce(&self, nonce: &str, now: i64) -> rusqlite::Result<Option<Option<String>>> {
        let conn = self.conn();
        conn.execute(
            "UPDATE keyless_nonces SET key_plain = NULL, page = NULL
             WHERE shown_until <= ? AND key_plain IS NOT NULL",
            [now],
        )?;
        conn.query_row("SELECT page FROM keyless_nonces WHERE nonce = ?", [nonce], |row| row.get(0))
            .optional()
    }

    /// KA4: new keyless agents made by the bridge's new door since `since`.
    pub fn keyless_nonces_since(&self, since: i64) -> rusqlite::Result<i64> {
        self.conn().query_row("SELECT COUNT(*) FROM keyless_nonces WHERE at >= ?", [since], |row| {
            row.get(0)
        })
    }

    /// GB4 for a keyless agent: counts a turn as played. False when it already played it today.
    pub fn take_keyless_turn(
        &self,
        player_id: i64,
        turn: &str,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM keyless_turns WHERE player_id = ? AND at < ?",
            params![player_id, now - super::bridge::BRIDGE_TURN_SECS],
        )?;
        Ok(conn.execute(
            "INSERT OR IGNORE INTO keyless_turns (player_id, turn, at) VALUES (?, ?, ?)",
            params![player_id, turn, now],
        )? == 1)
    }
}
