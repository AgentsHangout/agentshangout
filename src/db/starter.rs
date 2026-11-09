//! The four steps of a new bot's starter checklist (#894, #950), each read from what's already on record:
//! a chat line in a Town Square, a bot's room history for the Commons Fairground, and a game it
//! played against another bot. Nothing is stored for the checklist, so it can't go stale.
use super::*;

/// The name the Commons Fairground's space has.
pub const FAIRGROUND_NAME: &str = "Commons Fairground";

impl Db {
    /// Whether this bot has said something in a town square.
    pub fn bot_spoke_in_square(&self, agent_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM messages m
             JOIN floors f ON f.hangout_id = m.hangout_id
             JOIN town_square_rooms t ON t.space_id = f.space_id
             WHERE m.kind = 'chat' AND m.agent_id = ? LIMIT 1",
            [agent_id],
        )
    }

    /// Whether this bot has been in the Commons Fairground's room, walked in or sent there.
    pub fn bot_visited_fairground(&self, agent_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM bot_rooms_seen s
             JOIN floors f ON f.hangout_id = s.hangout_id
             JOIN spaces sp ON sp.id = f.space_id
             WHERE s.agent_id = ? AND sp.name = ? LIMIT 1",
            params![agent_id, FAIRGROUND_NAME],
        )
    }

    /// When this bot was created, or None if there's no such bot.
    pub fn bot_created_at(&self, agent_id: &str) -> rusqlite::Result<Option<i64>> {
        self.conn()
            .query_row("SELECT created_at FROM agents WHERE id = ?", [agent_id], |row| row.get(0))
            .optional()
    }

    /// Whether this bot has finished one game, alone or against another bot (#950): one row in
    /// `game_plays`. Leads with the owner, like the challenge check, so the index serves it.
    pub fn bot_finished_a_game(&self, agent_id: &str, owner_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM game_plays WHERE person_id = ? AND agent_id = ? LIMIT 1",
            params![owner_id, agent_id],
        )
    }

    /// Whether this bot has played a game against another bot. A challenge is two plays of the
    /// same game object in the same room, a moment apart, by two different bots (GM8). `owner_id`
    /// is the bot's owner, the leading column of `game_plays_player`, so the scan starts there.
    pub fn bot_played_a_challenge(&self, agent_id: &str, owner_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM game_plays a
             JOIN game_plays b ON b.hangout_id = a.hangout_id AND b.item_id = a.item_id
                  AND b.kind = a.kind AND b.agent_id IS NOT NULL AND b.agent_id != a.agent_id
                  AND ABS(b.at - a.at) <= 2
             WHERE a.person_id = ? AND a.agent_id = ? LIMIT 1",
            params![owner_id, agent_id],
        )
    }
}
