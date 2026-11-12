//! Section tag on `people` (the names here) and `people_here`, written from #748 in docs/rooms.md.

mod support;

use agenthangout::{
    app,
    db::{Db, NewMessage},
    now, AppState, Config,
};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

#[tokio::test]
async fn a_repeat_brief_read_skips_people_until_someone_joins() {
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
        .json(&json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = made["hangout"]["id"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let (_, token) = support::legacy_bot(&state, &ann, &room, "Fern");
    let invite = state.db.hangout(&room).unwrap().unwrap().invite_code;

    let call = |args: Value| {
        let client = client.clone();
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "read_room", "arguments": args } });
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

    // A first read sends the names and their tag.
    let first = call(json!({ "hangout_id": room, "brief": true })).await;
    assert!(first["people"].is_array(), "{first}");
    let tag = first["section_tags"]["people"].as_str().unwrap().to_string();
    let last = first["last_message_id"].as_i64().unwrap_or(0);

    // A chat line is new, but nobody joined: people and people_here are left out.
    state
        .db
        .insert_message(&NewMessage {
            hangout_id: &room,
            person_id: Some(&ann),
            agent_id: None,
            speaker: "Ann",
            kind: "chat",
            body: "Hello",
            now: now(),
        })
        .unwrap();
    let again = call(json!({ "hangout_id": room, "brief": true, "after_message_id": last,
        "known": { "people": tag } }))
    .await;
    assert!(again.get("people").is_none(), "{again}");
    assert!(again.get("people_here").is_none(), "{again}");
    assert!(again["unchanged"].as_array().unwrap().contains(&json!("people")), "{again}");
    assert!(again["section_tags"].get("people").is_none(), "{again}");

    // Someone joins: the names come back under a new tag.
    let joined = client
        .post(format!("{base}/api/invites/{invite}/join"))
        .json(&json!({ "your_name": "Bo" }))
        .send()
        .await
        .unwrap();
    assert!(joined.status().is_success(), "{}", joined.status());
    let changed = call(json!({ "hangout_id": room, "brief": true, "after_message_id": last,
        "known": { "people": tag } }))
    .await;
    assert!(changed["people"].as_array().unwrap().contains(&json!("Bo")), "{changed}");
    assert_ne!(changed["section_tags"]["people"], json!(tag), "{changed}");

    // A full read sends it whatever is known.
    let full = call(json!({ "hangout_id": room, "full": true, "known": { "people": tag } })).await;
    assert!(full.get("people").is_some(), "{full}");
}
