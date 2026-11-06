//! Room emotes (I143-I146, #638): a person or a bot shows a short emote over its avatar, from a
//! fixed set, for everyone in the room. Like typing hints (I108) it lives only in memory: no chat
//! line, no database row, no wake and no history.
use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    ops, AppState,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};

/// I143: the emotes there are, in the order the menu offers them.
pub const EMOTES: [&str; 8] =
    ["wave", "laugh", "clap", "cheer", "think", "shrug", "heart", "dance"];
/// I144: how long one shows.
pub const SHOWN_SECS: i64 = 10;
/// I145: the gap a person or bot leaves between two emotes.
pub const GAP_SECS: i64 = 5;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone)]
struct Shown {
    emote: &'static str,
    at: i64,
}

/// Keyed by (room, who): a person's id or a bot's id.
#[derive(Default)]
pub struct Runtime(Mutex<HashMap<(String, String), Shown>>);

impl Runtime {
    /// Shows `emote` for `who` in `room`, unless they emoted there less than GAP_SECS ago.
    fn show(&self, room: &str, who: &str, emote: &'static str, now: i64) -> bool {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|_, shown| shown.at + SHOWN_SECS > now);
        let key = (room.to_string(), who.to_string());
        if rows.get(&key).is_some_and(|shown| now - shown.at < GAP_SECS) {
            return false;
        }
        if rows.len() >= MAX_ENTRIES && !rows.contains_key(&key) {
            if let Some(oldest) =
                rows.iter().min_by_key(|(_, shown)| shown.at).map(|(key, _)| key.clone())
            {
                rows.remove(&oldest);
            }
        }
        rows.insert(key, Shown { emote, at: now });
        true
    }

    fn in_room(&self, room: &str, now: i64) -> Vec<(String, Shown)> {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|_, shown| shown.at + SHOWN_SECS > now);
        rows.iter()
            .filter(|((id, _), _)| id == room)
            .map(|((_, who), shown)| (who.clone(), shown.clone()))
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct Emote {
    /// The person's or the bot's id, as the room's people and agents list them.
    pub id: String,
    /// `person` or `bot`.
    pub kind: &'static str,
    pub name: String,
    pub emote: &'static str,
    /// Unix seconds when it stops showing.
    pub until: i64,
}

/// I146: the emotes showing in this room now, for anyone who can see the room. A guest on a Porch
/// sees none, as with votes (V10).
pub fn visible(state: &AppState, caller: &Caller, room: &str) -> ApiResult<Vec<Emote>> {
    if porch_guest(state, caller, room)? {
        return Ok(Vec::new());
    }
    let now = crate::now();
    let shown = state.emotes.in_room(room, now);
    if shown.is_empty() {
        return Ok(Vec::new());
    }
    let people = state.db.people_in(room)?;
    let mut list = Vec::new();
    for (who, emote) in shown {
        let until = emote.at + SHOWN_SECS;
        if let Some(person) = people.iter().find(|p| p.id == who) {
            list.push(Emote {
                id: who,
                kind: "person",
                name: person.name.clone(),
                emote: emote.emote,
                until,
            });
        } else if let Some(bot) = state.db.agent(&who, now)? {
            if state.db.bot_place(&who, now)?.as_deref() == Some(room) {
                list.push(Emote {
                    id: who,
                    kind: "bot",
                    name: bot.name,
                    emote: emote.emote,
                    until,
                });
            }
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

fn porch_guest(state: &AppState, caller: &Caller, room: &str) -> ApiResult<bool> {
    Ok(state.guest_visit(caller, room)?.is_some()
        && !state.db.floor(room)?.is_some_and(|floor| floor.access == "public"))
}

/// I143-I145: `emote` must be one of EMOTES. A person emotes in a room they can open; a bot in the
/// room it's in. A guest on a Porch can't, nor a house bot, nor a bot under a quiet sign (I17), and
/// one emote per GAP_SECS.
pub fn emote(state: &AppState, caller: &Caller, room: &str, emote: &str) -> ApiResult<()> {
    ops::no_house_bot(caller)?;
    let word = emote.trim().to_ascii_lowercase();
    let Some(emote) = EMOTES.iter().copied().find(|e| *e == word) else {
        return Err(ApiError::bad_request(format!("emote must be one of: {}", EMOTES.join(", "))));
    };
    ops::require_room(state, caller, room)?;
    if porch_guest(state, caller, room)? {
        return Err(ApiError::forbidden("guests can't emote"));
    }
    crate::items::no_quiet(state, caller, room)?;
    let who = caller.agent_id.clone().unwrap_or_else(|| caller.person_id.clone());
    if !state.emotes.show(room, &who, emote, crate::now()) {
        return Err(ApiError::too_many_requests(format!("one emote every {GAP_SECS} seconds")));
    }
    publish(state, caller, room)
}

/// I143: a bot's emote goes to the room it's in.
pub fn bot_emote(state: &AppState, caller: &Caller, emote: &str) -> ApiResult<()> {
    let Some(agent_id) = caller.agent_id.as_deref() else {
        return Err(ApiError::forbidden("only bots emote this way"));
    };
    let Some(room) = state.db.bot_place(agent_id, crate::now())? else {
        return Err(ApiError::conflict("you're not in a room to emote in"));
    };
    self::emote(state, caller, &room, emote)
}

fn publish(state: &AppState, caller: &Caller, room: &str) -> ApiResult<()> {
    ops::publish(
        state,
        room,
        "emotes",
        serde_json::json!(visible(state, caller, room)?).to_string(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shows_for_ten_seconds_with_a_gap_and_is_bounded() {
        let state = Runtime::default();
        assert!(state.show("room", "ann", "wave", 100));
        assert!(!state.show("room", "ann", "clap", 104), "five seconds between emotes");
        assert!(state.show("other", "ann", "clap", 104), "the gap is per room");
        assert_eq!(state.in_room("room", 109).len(), 1);
        assert!(state.in_room("room", 110).is_empty(), "gone after ten seconds");
        assert!(state.show("room", "ann", "heart", 105));
        assert_eq!(state.in_room("room", 105)[0].1.emote, "heart", "a new one replaces the last");
        for n in 0..MAX_ENTRIES + 10 {
            state.show("room", &n.to_string(), "wave", 200);
        }
        assert!(state.in_room("room", 200).len() <= MAX_ENTRIES);
    }
}
