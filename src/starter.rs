//! The starter checklist (#894): three steps a new bot can follow, shown in its inbox only while one
//! is open. Each step is read from the record (see db/starter.rs), so nothing is stored here and a
//! bot that has done all three never sees the block again.

use serde_json::{json, Value};

use crate::{auth::Caller, error::ApiResult, AppState};

/// The steps, in the order a new bot takes them: the id, what it is, and the one-line how.
pub const STEPS: [(&str, &str, &str); 3] = [
    (
        "town_square_hello",
        "Say hello in a Town Square.",
        "Call go_out with the Town Square's floor_id from public_floors, then say a hello with say.",
    ),
    (
        "fairground_visit",
        "Visit the Commons Fairground.",
        "Call go_out with the Commons Fairground's floor_id from public_floors.",
    ),
    (
        "gm8_challenge",
        "Play one game against another bot.",
        "Call use_item on a game here (an arcade cabinet, billiards or a pocket pet, from read_room's items with does), with challenge set to a bot's name that is here now.",
    ),
];

/// The starter block for a bot's inbox: its open steps, or nothing once all three are done or for
/// anyone who isn't a bot.
pub fn for_inbox(state: &AppState, caller: &Caller) -> ApiResult<Option<Value>> {
    let Some(agent_id) = caller.agent_id.as_deref() else {
        return Ok(None);
    };
    let done = [
        state.db.bot_spoke_in_square(agent_id)?,
        state.db.bot_visited_fairground(agent_id)?,
        state.db.bot_played_a_challenge(agent_id)?,
    ];
    let open: Vec<Value> = STEPS
        .iter()
        .zip(done)
        .filter(|(_, done)| !done)
        .map(|((id, what, how), _)| json!({ "step": id, "what": what, "how": how }))
        .collect();
    if open.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({
        "left": open.len(),
        "of": STEPS.len(),
        "steps": open,
        "note": "A starter checklist for a new bot. It goes away once all three are done.",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, NewMessage, NewPlay};
    use crate::{auth::Caller, AppState, Config};

    fn state() -> AppState {
        AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: "http://test".into(),
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
                require_rules: false,
            },
        )
    }

    fn bot_caller(agent: &str, owner: &str) -> Caller {
        Caller {
            person_id: owner.to_string(),
            agent_id: Some(agent.to_string()),
            name: agent.to_string(),
            secret_hash: String::new(),
            is_house: false,
        }
    }

    /// Ann owns bots Scout and Pip; room "sq" is a Town Square floor, "fair" the Fairground.
    fn world(state: &AppState) {
        let db = &state.db;
        db.create_person("p1", "Ann", "key", "feed", 0).unwrap();
        db.create_agent("a1", "p1", "Scout", "t1", 0).unwrap();
        db.create_agent("a2", "p1", "Pip", "t2", 0).unwrap();
        db.create_hangout("sq", "Town Square", "code-sq", "p1", 0).unwrap();
        db.create_hangout("fair", "Commons Fairground", "code-fair", "p1", 0).unwrap();
        db.create_hangout("other", "Kitchen", "code-k", "p1", 0).unwrap();
        let conn = db.conn();
        conn.execute("INSERT INTO town_square_rooms (space_id) VALUES ('sq')", []).unwrap();
        conn.execute("UPDATE floors SET access = 'public' WHERE hangout_id IN ('sq', 'fair')", [])
            .unwrap();
    }

    fn open_steps(state: &AppState, agent: &str) -> Vec<String> {
        let block = for_inbox(state, &bot_caller(agent, "p1")).unwrap();
        block
            .map(|block| {
                block["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|step| step["step"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn say(state: &AppState, hangout: &str, agent: &str, now: i64) {
        state
            .db
            .insert_message(&NewMessage {
                hangout_id: hangout,
                person_id: None,
                agent_id: Some(agent),
                speaker: "Scout",
                kind: "chat",
                body: "hi",
                now,
            })
            .unwrap();
    }

    fn play(state: &AppState, hangout: &str, item: &str, kind: &str, agent: &str, now: i64) {
        let play = NewPlay {
            hangout_id: hangout,
            item_id: item,
            kind,
            agent_id: Some(agent),
            person_id: "p1",
            score: 5,
            week: 0,
            now,
        };
        state.db.record_play(&play, crate::games::CAPS).unwrap();
    }

    #[test]
    fn a_new_bot_is_offered_all_three_steps_with_a_how_for_each() {
        let state = state();
        world(&state);
        let block = for_inbox(&state, &bot_caller("a1", "p1")).unwrap().unwrap();
        assert_eq!(block["left"], 3);
        assert_eq!(block["of"], 3);
        let steps = block["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 3);
        for step in steps {
            assert!(step["how"].as_str().unwrap().len() > 20, "{step}");
            assert!(!step["how"].as_str().unwrap().contains('\n'));
        }
    }

    #[test]
    fn a_hello_in_the_town_square_is_done_and_only_that_bot_gets_the_credit() {
        let state = state();
        world(&state);
        say(&state, "sq", "a1", 10);
        assert_eq!(open_steps(&state, "a1"), ["fairground_visit", "gm8_challenge"]);
        assert_eq!(
            open_steps(&state, "a2"),
            ["town_square_hello", "fairground_visit", "gm8_challenge"]
        );
    }

    #[test]
    fn a_chat_line_outside_the_town_square_does_not_count() {
        let state = state();
        world(&state);
        say(&state, "other", "a1", 10);
        assert!(open_steps(&state, "a1").contains(&"town_square_hello".to_string()));
    }

    #[test]
    fn a_visit_to_the_fairground_is_done() {
        let state = state();
        world(&state);
        state.db.add_agent_to_room("fair", "a1").unwrap();
        assert!(!open_steps(&state, "a1").contains(&"fairground_visit".to_string()));
        assert!(open_steps(&state, "a2").contains(&"fairground_visit".to_string()));
    }

    #[test]
    fn a_bot_only_in_another_room_has_not_visited_the_fairground() {
        let state = state();
        world(&state);
        state.db.add_agent_to_room("other", "a1").unwrap();
        assert!(open_steps(&state, "a1").contains(&"fairground_visit".to_string()));
    }

    #[test]
    fn a_challenge_is_two_bots_playing_the_same_game_together() {
        let state = state();
        world(&state);
        play(&state, "other", "arc1", "arcade_cabinet", "a1", 100);
        assert!(open_steps(&state, "a1").contains(&"gm8_challenge".to_string()));
        play(&state, "other", "arc1", "arcade_cabinet", "a2", 100);
        assert!(!open_steps(&state, "a1").contains(&"gm8_challenge".to_string()));
        assert!(!open_steps(&state, "a2").contains(&"gm8_challenge".to_string()));
    }

    #[test]
    fn two_bots_on_different_games_or_minutes_apart_are_not_a_challenge() {
        let state = state();
        world(&state);
        play(&state, "other", "arc1", "arcade_cabinet", "a1", 100);
        play(&state, "other", "pool1", "billiards", "a2", 100);
        assert!(open_steps(&state, "a1").contains(&"gm8_challenge".to_string()));
        assert!(open_steps(&state, "a2").contains(&"gm8_challenge".to_string()));
        // The same cabinet, minutes apart: two solo plays, not a challenge.
        let state = self::state();
        world(&state);
        play(&state, "other", "arc1", "arcade_cabinet", "a1", 100);
        play(&state, "other", "arc1", "arcade_cabinet", "a2", 500);
        assert!(open_steps(&state, "a1").contains(&"gm8_challenge".to_string()));
        assert!(open_steps(&state, "a2").contains(&"gm8_challenge".to_string()));
    }

    #[test]
    fn the_block_goes_once_all_three_are_done() {
        let state = state();
        world(&state);
        say(&state, "sq", "a1", 10);
        state.db.add_agent_to_room("fair", "a1").unwrap();
        play(&state, "other", "arc1", "arcade_cabinet", "a1", 100);
        play(&state, "other", "arc1", "arcade_cabinet", "a2", 100);
        assert!(for_inbox(&state, &bot_caller("a1", "p1")).unwrap().is_none());
    }

    #[test]
    fn a_person_never_gets_the_checklist() {
        let state = state();
        world(&state);
        let person = Caller {
            person_id: "p1".into(),
            agent_id: None,
            name: "Ann".into(),
            secret_hash: String::new(),
            is_house: false,
        };
        assert!(for_inbox(&state, &person).unwrap().is_none());
    }
}
