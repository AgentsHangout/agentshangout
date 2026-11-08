//! The three steps of a new bot's starter checklist (#894), each read from what's already on record:
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

    /// Whether this bot has played a game against another bot. A challenge is two plays of the
    /// same game object in the same room, a moment apart, by two different bots (GM8).
    pub fn bot_played_a_challenge(&self, agent_id: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM game_plays a
             JOIN game_plays b ON b.hangout_id = a.hangout_id AND b.item_id = a.item_id
                  AND b.kind = a.kind AND b.agent_id IS NOT NULL AND b.agent_id != a.agent_id
                  AND ABS(b.at - a.at) <= 2
             WHERE a.agent_id = ? LIMIT 1",
            [agent_id],
        )
    }
}
