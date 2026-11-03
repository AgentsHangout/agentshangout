//! S70: registered bots originate in their owner's house Crew, never a map guest or another room.
use agenthangout::{auth::Caller, db::Db, now, ops, AppState, Config};
use axum::http::StatusCode;

fn person(state: &AppState, id: &str) -> Caller {
    state.db.create_person(id, id, id, &format!("feed-{id}"), now()).unwrap();
    Caller {
        person_id: id.into(),
        agent_id: None,
        name: id.into(),
        secret_hash: id.into(),
        is_house: false,
    }
}

#[tokio::test]
async fn only_own_house_registers_bots_and_existing_bots_can_join_other_rooms() {
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: "http://localhost:8787".into(),
            create_code: Some("test".into()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    let ann = person(&state, "ann");
    let bo = person(&state, "bo");
    let house =
        state.db.ensure_house("ann-house", "Ann House", "ann-invite", "ann", now()).unwrap();
    let other = state.db.ensure_house("bo-house", "Bo House", "bo-invite", "bo", now()).unwrap();
    state.db.add_member(&other.id, "ann", now()).unwrap();
    let studio =
        state.db.create_hangout("studio", "Studio", "studio-invite", "ann", now()).unwrap();
    for room in [&other.id, &studio.id] {
        let error = ops::create_agent(&state, &ann, room, "Scout").unwrap_err();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert!(error.message.contains("Crew"));
        assert!(state.db.agents_of("ann", now()).unwrap().is_empty(), "refusal created a bot");
    }
    let (agent, token) = ops::create_agent(&state, &ann, &house.id, "Scout").unwrap();
    assert!(token.starts_with("ah_bot_"));
    assert_eq!(agent.owner_id, "ann");
    ops::bring_agent(&state, &ann, &studio.id, &agent.id).unwrap();
    assert!(state.db.agents_of("bo", now()).unwrap().is_empty());
    let (bo_bot, _) = ops::create_agent(&state, &bo, &other.id, "Wren").unwrap();
    assert_eq!(bo_bot.owner_id, "bo");
}
