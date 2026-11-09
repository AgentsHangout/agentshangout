//! GM9 (#905): a solo game play hints that a bot here can be challenged.

mod support;

use agenthangout::{
    auth::hash_secret,
    db::{Db, DuelAllow},
    items, now, AppState, Config,
};
use serde_json::{json, Value};

const CODE: &str = "letmein";

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

#[tokio::test]
async fn a_solo_play_names_one_bot_here_and_only_then() {
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
    let key = made["owner_key"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let room = async |name: &str| {
        post(&base, &key, "/api/hangouts", json!({ "name": name, "create_code": CODE })).await
            ["hangout"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let arcade = room("Arcade").await;
    let den = room("Den").await;
    support::legacy_bot(&state, &ann, &den, "Faraway");
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
                now: now(),
            },
            |_, _, _| Ok(()),
        )
        .unwrap()
        .unwrap();
    let play = |name: &str, challenge: Option<&str>| {
        let (_, token) = support::legacy_bot(&state, &ann, &arcade, name);
        let caller = state.authenticate(&hash_secret(&token)).unwrap().unwrap();
        items::use_item_against(&state, &caller, "cabinet-1", challenge).unwrap()
    };

    // Alone (Faraway is in another room): no hint.
    assert!(play("Scout", None).get("challenge_hint").is_none());
    // Scout is here now: the hint names it, and only it.
    let hint = play("Rex", None)["challenge_hint"].as_str().unwrap().to_string();
    assert_eq!(hint, "Add challenge: \"Scout\" to play someone here; the room hears who won.");
    // First by name, never the caller or the bot elsewhere.
    let hint = play("Cleo", None)["challenge_hint"].as_str().unwrap().to_string();
    assert!(hint.contains("\"Rex\"") && !hint.contains("Faraway"), "{hint}");
    // A challenge gets no hint (GM11: both owners allow duels here, so it asks).
    let (fresh, _) = support::legacy_bot(&state, &ann, &arcade, "Fresh");
    let (zed, token) = support::legacy_bot(&state, &ann, &arcade, "Zed");
    for bot in [&fresh.id, &zed.id] {
        let room = DuelAllow { allow: "room".to_string(), daily_cap: 5 };
        state.db.set_duel_allow(bot, &room, now()).unwrap();
    }
    let zed = state.authenticate(&hash_secret(&token)).unwrap().unwrap();
    let asked = items::use_item_against(&state, &zed, "cabinet-1", Some("Fresh")).unwrap();
    assert!(asked.get("challenge_hint").is_none() && asked["status"] == "pending", "{asked}");
}
