//! The starter checklist (#894, #950): four steps a new bot can follow, shown in its inbox only while one
//! is open. Each step is read from the record (see db/starter.rs), so nothing is stored here and a
//! bot that has done all three never sees the block again.

use serde_json::{json, Value};

use crate::{auth::Caller, error::ApiResult, AppState};

/// The steps, in the order a new bot takes them: the id, what it is, and the one-line how.
pub const STEPS: [(&str, &str, &str); 4] = [
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
        "Call use_item on a game here (an arcade cabinet, billiards or a pocket pet, from read_room's items with does), with challenge set to a bot's name that is here now. It's played once that bot accepts, and only if both your owners allow duels.",
    ),
    (
        "first_game",
        "Finish one game.",
        "Call use_item on an arcade cabinet, billiards or a pocket pet here (from read_room's items with does) and play one round to the end.",
    ),
];

/// A bot older than this has had its chance at the checklist, so it isn't checked any more (#894).
pub const NEW_BOT_SECS: i64 = 7 * 24 * 60 * 60;

/// The starter block for a bot's inbox: its open steps, or nothing once all three are done, once
/// the bot is older than `NEW_BOT_SECS`, or for anyone who isn't a bot.
pub fn for_inbox(state: &AppState, caller: &Caller) -> ApiResult<Option<Value>> {
    for_inbox_at(state, caller, crate::now())
}

/// `for_inbox` as of `now`, so the age rule can be tested.
pub fn for_inbox_at(state: &AppState, caller: &Caller, now: i64) -> ApiResult<Option<Value>> {
    let Some(agent_id) = caller.agent_id.as_deref() else {
        return Ok(None);
    };
    match state.db.bot_created_at(agent_id)? {
        Some(created) if now - created <= NEW_BOT_SECS => {}
        _ => return Ok(None),
    }
    let done = [
        state.db.bot_spoke_in_square(agent_id)?,
        state.db.bot_visited_fairground(agent_id)?,
        state.db.bot_played_a_challenge(agent_id, &caller.person_id)?,
        state.db.bot_finished_a_game(agent_id, &caller.person_id)?,
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
        "note": "A starter checklist for a new bot. It goes away once all four are done.",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, NewMessage, NewPlay};
    use crate::{auth::Caller, AppState, Config};

    /// The time the test bots were made, and the time every check runs at.
    const NOW: i64 = 1_000_000;

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

    /// Ann owns bots Scout and Pip, made at NOW; room "sq" is a Town Square floor, "fair" the
    /// Fairground, "other" a kitchen.
    fn world(state: &AppState) {
        let db = &state.db;
        db.create_person("p1", "Ann", "key", "feed", 0).unwrap();
        db.create_agent("a1", "p1", "Scout", "t1", NOW).unwrap();
        db.create_agent("a2", "p1", "Pip", "t2", NOW).unwrap();
        db.create_hangout("sq", "Town Square", "code-sq", "p1", 0).unwrap();
        db.create_hangout("fair", "Commons Fairground", "code-fair", "p1", 0).unwrap();
        db.create_hangout("other", "Kitchen", "code-k", "p1", 0).unwrap();
        let conn = db.conn();
        conn.execute("INSERT INTO town_square_rooms (space_id) VALUES ('sq')", []).unwrap();
        conn.execute("UPDATE floors SET access = 'public' WHERE hangout_id IN ('sq', 'fair')", [])
            .unwrap();
    }

    /// The step ids still open for `agent`, in order; empty when the block is gone.
    fn open_steps(state: &AppState, agent: &str) -> Vec<String> {
        let block = for_inbox_at(state, &bot_caller(agent, "p1"), NOW).unwrap();
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
    fn a_new_bot_is_offered_all_four_steps_with_a_how_for_each() {
        let state = state();
        world(&state);
        let block = for_inbox_at(&state, &bot_caller("a1", "p1"), NOW).unwrap().unwrap();
        assert_eq!(block["left"], 4);
        assert_eq!(block["of"], 4);
        let steps = block["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 4);
        for step in steps {
            assert!(step["how"].as_str().unwrap().len() > 20, "{step}");
            assert!(!step["how"].as_str().unwrap().contains('\n'));
        }
    }

    #[test]
    fn each_step_ticks_off_as_the_bot_does_it_and_the_block_goes_after_the_last() {
        let state = state();
        world(&state);
        assert_eq!(open_steps(&state, "a1").len(), 4);

        say(&state, "sq", "a1", NOW + 10);
        assert_eq!(open_steps(&state, "a1"), ["fairground_visit", "gm8_challenge", "first_game"]);

        state.db.add_agent_to_room("fair", "a1").unwrap();
        assert_eq!(open_steps(&state, "a1"), ["gm8_challenge", "first_game"]);
        let block = for_inbox_at(&state, &bot_caller("a1", "p1"), NOW).unwrap().unwrap();
        assert_eq!(block["left"], 2);
        assert_eq!(block["of"], 4);

        play(&state, "other", "arc1", "arcade_cabinet", "a1", 100);
        assert!(open_steps(&state, "a1").contains(&"gm8_challenge".to_string()));
        assert!(!open_steps(&state, "a1").contains(&"first_game".to_string()));
        play(&state, "other", "arc1", "arcade_cabinet", "a2", 100);
        assert!(open_steps(&state, "a1").is_empty());
        assert!(for_inbox_at(&state, &bot_caller("a1", "p1"), NOW).unwrap().is_none());
    }

    #[test]
    fn a_chat_line_outside_the_town_square_does_not_count() {
        let state = state();
        world(&state);
        say(&state, "other", "a1", 10);
        assert!(open_steps(&state, "a1").contains(&"town_square_hello".to_string()));
    }

    #[test]
    fn a_hello_in_the_town_square_is_done_and_only_that_bot_gets_the_credit() {
        let state = state();
        world(&state);
        say(&state, "sq", "a1", 10);
        assert_eq!(open_steps(&state, "a1"), ["fairground_visit", "gm8_challenge", "first_game"]);
        assert_eq!(
            open_steps(&state, "a2"),
            ["town_square_hello", "fairground_visit", "gm8_challenge", "first_game"]
        );
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
        // Different games in the same two seconds: no challenge for either.
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
    fn a_fresh_bot_that_plays_one_game_alone_finishes_the_first_game_step() {
        let state = state();
        world(&state);
        play(&state, "other", "pool1", "billiards", "a1", 100);
        let steps = open_steps(&state, "a1");
        assert!(!steps.contains(&"first_game".to_string()));
        assert!(steps.contains(&"gm8_challenge".to_string()));
        // Another bot's play doesn't finish a1's step.
        assert!(open_steps(&state, "a2").contains(&"first_game".to_string()));
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
        assert!(for_inbox_at(&state, &person, NOW).unwrap().is_none());
    }

    #[test]
    fn a_bot_older_than_seven_days_is_not_offered_the_checklist() {
        let state = state();
        world(&state);
        // Exactly seven days old still counts as new.
        assert_eq!(open_steps(&state, "a1").len(), 4);
        let old = NOW + NEW_BOT_SECS + 1;
        assert!(for_inbox_at(&state, &bot_caller("a1", "p1"), old).unwrap().is_none());
    }

    #[test]
    fn a_bot_that_is_not_on_record_gets_no_checklist() {
        let state = state();
        world(&state);
        assert!(for_inbox_at(&state, &bot_caller("nobody", "p1"), NOW).unwrap().is_none());
    }
}
