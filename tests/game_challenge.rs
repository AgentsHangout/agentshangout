//! GM8 (#876): challenge another bot to a game. Written from docs/games.md.

mod support;

use agenthangout::{auth::hash_secret, db::Db, games, items, now, AppState, Config};
use serde_json::{json, Value};

const CODE: &str = "letmein";

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

async fn post(base: &str, key: &str, path: &str, body: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.text().await.unwrap());
    response.json().await.unwrap()
}

// GM8: a challenge posts one line and records a play for each; refusals record nothing.
#[tokio::test]
async fn a_challenge_posts_the_result_and_refusals_record_nothing() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: Some(CODE.to_string()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: Vec::new(),
            require_rules: false,
        },
    );
    let served = state.clone();
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(served)).await });

    let made = post(
        &base,
        "",
        "/api/hangouts",
        json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }),
    )
    .await;
    let (key, ann) = (text(&made["owner_key"]), text(&made["person"]["id"]));
    let arcade = text(
        &post(&base, &key, "/api/hangouts", json!({ "name": "Arcade", "create_code": CODE })).await
            ["hangout"]["id"],
    );
    let elsewhere = text(
        &post(&base, &key, "/api/hangouts", json!({ "name": "Den", "create_code": CODE })).await
            ["hangout"]["id"],
    );
    let (scout, token) = support::legacy_bot(&state, &ann, &arcade, "Scout");
    let (rex, _) = support::legacy_bot(&state, &ann, &arcade, "Rex");
    support::legacy_bot(&state, &ann, &elsewhere, "Faraway");
    let caller = state.authenticate(&hash_secret(&token)).unwrap().unwrap();
    let at = now();
    state
        .db
        .place_item::<()>(
            &agenthangout::db::NewFloorItem {
                id: "cabinet-1",
                hangout_id: &arcade,
                kind: "arcade_cabinet",
                x: 10,
                y: 6,
                facing: 0,
                placed_by: &ann,
                now: at,
            },
            |_, _, _| Ok(()),
        )
        .unwrap()
        .unwrap();
    let week = games::week_of(at);
    let free = |agent: &str, wait: i64| {
        state
            .db
            .next_play_at((&ann, Some(agent)), &arcade, "arcade_cabinet", at + wait, games::CAPS)
            .unwrap()
            .is_none()
    };
    let lines = || state.db.messages(&arcade, 0, 50).unwrap();

    // Refused before anything is rolled: a bot that isn't here, and oneself.
    for name in ["Faraway", "Nobody", "Scout"] {
        assert!(items::use_item_against(&state, &caller, "cabinet-1", Some(name)).is_err());
    }
    assert!(free(&scout.id, 0) && lines().is_empty());

    // A challenge: one line, a play each.
    let won = items::use_item_against(&state, &caller, "cabinet-1", Some("rex")).unwrap();
    let (a, b) =
        (won["score"].as_i64().unwrap(), won["opponent"]["result"]["score"].as_i64().unwrap());
    assert_eq!(won["remaining"], 5, "{won}");
    assert_eq!(won["opponent"]["result"]["remaining"], 5, "{won}");
    let said = lines();
    assert_eq!(said.len(), 1);
    let expected = match a.cmp(&b) {
        std::cmp::Ordering::Greater => format!("Scout beat Rex at Arcade cabinet, {a} to {b}."),
        std::cmp::Ordering::Less => format!("Rex beat Scout at Arcade cabinet, {b} to {a}."),
        std::cmp::Ordering::Equal => format!("Scout and Rex tied at Arcade cabinet, {a} each."),
    };
    assert_eq!(said[0].body, expected);
    assert!(!free(&scout.id, 0) && !free(&rex.id, 0), "both plays are recorded");
    assert!(free(&scout.id, 61) && free(&rex.id, 61));

    // Quo is out of plays: Pip's challenge is refused and Pip's count stays where it was.
    let (pip, pip_token) = support::legacy_bot(&state, &ann, &arcade, "Pip");
    let (quo, _) = support::legacy_bot(&state, &ann, &arcade, "Quo");
    for n in 0..6 {
        let play = agenthangout::db::NewPlay {
            hangout_id: &arcade,
            item_id: "cabinet-1",
            kind: "arcade_cabinet",
            agent_id: Some(&quo.id),
            person_id: &ann,
            score: n,
            week,
            now: at - 3000 + n * 100,
        };
        state.db.record_play(&play, games::CAPS).unwrap();
    }
    assert!(!free(&quo.id, 0));
    let pip_caller = state.authenticate(&hash_secret(&pip_token)).unwrap().unwrap();
    assert!(items::use_item_against(&state, &pip_caller, "cabinet-1", Some("Quo")).is_err());
    assert!(free(&pip.id, 0));
    assert_eq!(lines().len(), 1);
}
