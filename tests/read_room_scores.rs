//! Tests for scores in read_room (GM5, GM19, I156, #1039).

mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

// I156: In a room with a game item, read_room carries scores and section_tag.
// A room with no game has none. A repeat read skips scores.
#[tokio::test]
async fn read_room_scores_section_caching_and_porch_guest() {
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
    tokio::spawn(async move { axum::serve(listener, app(served)).await });
    let client = Client::new();

    let made: Value = client
        .post(format!("{base}/api/hangouts"))
        .json(&json!({ "name": "Arcade Room", "your_name": "Ann", "create_code": CODE }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = made["hangout"]["id"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let (_, token) = support::legacy_bot(&state, &ann, &room, "GamerBot");

    let call = |name: &'static str, args: Value| {
        let client = client.clone();
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let (url, token) = (format!("{base}/mcp"), token.clone());
        async move {
            let reply: Value = client
                .post(url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let reply = support::structured(reply);
            let result = &reply["result"];
            assert!(result["isError"].is_null(), "{result}");
            result["structuredContent"].clone()
        }
    };

    // 1. Room with no game has no scores section and no scores in section_tags
    let no_game = call("read_room", json!({ "hangout_id": room, "brief": true })).await;
    assert!(no_game.get("scores").is_none(), "{no_game}");
    assert!(no_game["section_tags"].get("scores").is_none(), "{no_game}");

    // 2. Place an arcade cabinet in the room
    let cabinet_id = agenthangout::auth::new_id();
    let item = agenthangout::db::NewFloorItem {
        id: &cabinet_id,
        hangout_id: &room,
        kind: "arcade_cabinet",
        x: 0,
        y: 0,
        facing: 0,
        placed_by: &ann,
        now: now(),
    };
    state.db.place_item(&item, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();

    // 3. Measure first-read bytes with cabinet before any plays
    let first = call("read_room", json!({ "hangout_id": room, "brief": true })).await;
    assert!(first.get("scores").is_some(), "{first}");
    assert!(first["section_tags"].get("scores").is_some(), "{first}");
    let first_len = serde_json::to_string(&first).unwrap().len();

    // 4. Bot plays the arcade cabinet
    let played = call("use_item", json!({ "hangout_id": room, "item_id": cabinet_id })).await;
    assert_eq!(played["ok"], true);
    assert!(played["score"].is_number());

    // 5. Read room again: scores contains caller's own score marked as "you"
    let after_play = call("read_room", json!({ "hangout_id": room, "brief": true })).await;
    assert!(after_play.get("scores").is_some());
    let kinds = after_play["scores"]["kinds"].as_array().unwrap();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0]["kind"], "arcade_cabinet");
    let top = kinds[0]["top"].as_array().unwrap();
    assert!(!top.is_empty());
    assert_eq!(top[0]["name"], "you");
    assert_eq!(top[0]["yours"], true);
    assert!(top[0]["bot"].as_bool().unwrap());

    let scores_tag = after_play["section_tags"]["scores"].as_str().unwrap().to_string();
    let plays_len = serde_json::to_string(&after_play).unwrap().len();
    eprintln!(
        "READ_ROOM BYTES with arcade cabinet: before plays = {first_len}, after play = {plays_len}"
    );

    // 6. Repeat brief read with known: scores section omitted, listed in unchanged
    let repeat = call(
        "read_room",
        json!({
            "hangout_id": room,
            "brief": true,
            "known": { "scores": scores_tag }
        }),
    )
    .await;
    assert!(repeat.get("scores").is_none(), "{repeat}");
    assert!(repeat["unchanged"].as_array().unwrap().contains(&json!("scores")), "{repeat}");

    // 7. sections: ["scores"] selects only scores
    let only_scores = call(
        "read_room",
        json!({
            "hangout_id": room,
            "sections": ["scores"]
        }),
    )
    .await;
    assert!(only_scores.get("scores").is_some(), "{only_scores}");
    assert!(only_scores.get("messages").is_none(), "{only_scores}");
    assert!(only_scores.get("notes").is_none(), "{only_scores}");

    // 8. sections without scores leaves it out
    let only_notes = call(
        "read_room",
        json!({
            "hangout_id": room,
            "sections": ["notes"]
        }),
    )
    .await;
    assert!(only_notes.get("scores").is_none(), "{only_notes}");
}
