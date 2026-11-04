//! Ephemeral human composer hints (I108). No draft text, database rows, bot wakes or history.
use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    ops, AppState,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};

pub const TTL_SECS: i64 = 6;
const MAX_ENTRIES: usize = 4096;
#[derive(Default)]
pub struct Runtime(Mutex<HashMap<(String, String), i64>>);

impl Runtime {
    pub fn set(&self, room: &str, person: &str, on: bool, now: i64) -> bool {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|_, until| *until > now);
        let key = (room.to_string(), person.to_string());
        if !on {
            return rows.remove(&key).is_some();
        }
        if rows.len() >= MAX_ENTRIES && !rows.contains_key(&key) {
            if let Some(oldest) =
                rows.iter().min_by_key(|(_, until)| *until).map(|(key, _)| key.clone())
            {
                rows.remove(&oldest);
            }
        }
        rows.insert(key, now + TTL_SECS);
        true
    }

    fn in_room(&self, room: &str, now: i64) -> Vec<(String, i64)> {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|_, until| *until > now);
        rows.iter()
            .filter(|((id, _), _)| id == room)
            .map(|((_, person), until)| (person.clone(), *until))
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct Indicator {
    pub person_id: String,
    pub name: String,
    pub until: i64,
}

pub fn visible(state: &AppState, caller: &Caller, room: &str) -> ApiResult<Vec<Indicator>> {
    if caller.is_agent() {
        return Ok(Vec::new());
    }
    let people = state.db.people_in(room)?;
    let mut list = Vec::new();
    for (id, until) in state.typing.in_room(room, crate::now()) {
        if let Some(person) = people.iter().find(|p| p.id == id) {
            list.push(Indicator { person_id: id, name: person.name.clone(), until });
        }
    }
    list.sort_by(|a, b| a.person_id.cmp(&b.person_id));
    Ok(list)
}

pub fn set(state: &AppState, caller: &Caller, room: &str, on: bool) -> ApiResult<()> {
    ops::require_human(caller, "show typing")?;
    ops::no_guests(state, caller, room, "show typing")?;
    if on {
        crate::items::no_quiet(state, caller, room)?;
    }
    if on && !state.allow_writes_sliding(&format!("typing:{}", caller.person_id), 60) {
        return Err(ApiError::too_many_requests("typing updates are too frequent"));
    }
    if state.typing.set(room, &caller.person_id, on, crate::now()) {
        publish(state, caller, room)?;
    }
    Ok(())
}

pub fn clear(state: &AppState, caller: &Caller, room: &str) -> ApiResult<()> {
    if state.typing.set(room, &caller.person_id, false, crate::now()) {
        publish(state, caller, room)?;
    }
    Ok(())
}

fn publish(state: &AppState, caller: &Caller, room: &str) -> ApiResult<()> {
    ops::publish(
        state,
        room,
        "typing",
        serde_json::json!(visible(state, caller, room)?).to_string(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expires_without_reads_renewing_and_is_bounded() {
        let state = Runtime::default();
        state.set("room", "person", true, 100);
        assert_eq!(state.in_room("room", 105).len(), 1);
        assert!(state.in_room("other", 105).is_empty());
        assert!(state.in_room("room", 106).is_empty());
        for n in 0..MAX_ENTRIES + 10 {
            state.set("room", &n.to_string(), true, 200);
        }
        assert_eq!(state.in_room("room", 200).len(), MAX_ENTRIES);
        state.set("room", "last", true, 201);
        state.set("room", "last", false, 201);
        assert!(!state.in_room("room", 201).iter().any(|(p, _)| p == "last"));
    }
}
