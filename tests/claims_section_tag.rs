//! Test for section tags on `claims`, written from I155 in docs/rooms.md.

mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

// I155
#[tokio::test]
async fn a_repeat_brief_read_skips_claims_until_a_claim_changes() {
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
    // A job board put straight into the room, as tests/rooms.rs does for a Lific board.
    let item = agenthangout::db::NewFloorItem {
        id: &agenthangout::auth::new_id(),
        hangout_id: &room,
        kind: "job_board",
        x: 0,
        y: 0,
        facing: 0,
        placed_by: &ann,
        now: now(),
    };
    state.db.place_item(&item, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();

    let call = |name: &'static str, args: Value| {
        let client = client.clone();
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args } });
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
            let result = &reply["result"];
            assert!(result["isError"].is_null(), "{result}");
            result["structuredContent"].clone()
        }
    };
    let claim = |text: &'static str| {
        call("set_claim", json!({ "hangout_id": room, "claim": text, "status": "coding" }))
    };

    claim("coding #1").await;
    let first = call("read_room", json!({ "hangout_id": room, "brief": true })).await;
    assert!(first["claims"].is_array() || first["claims"].is_string(), "{first}");
    let tag = first["section_tags"]["claims"].as_str().unwrap().to_string();
    let last = first["last_message_id"].as_i64().unwrap_or(0);

    // Nothing changed: the second brief read leaves claims out and says so.
    let again = call(
        "read_room",
        json!({ "hangout_id": room, "brief": true, "after_message_id": last,
                "known": { "claims": tag } }),
    )
    .await;
    assert!(again.get("claims").is_none(), "{again}");
    assert!(again["unchanged"].as_array().unwrap().contains(&json!("claims")), "{again}");

    // A claim changed: it comes back with a new tag.
    claim("coding #2").await;
    let changed = call(
        "read_room",
        json!({ "hangout_id": room, "brief": true, "after_message_id": last,
                "known": { "claims": tag } }),
    )
    .await;
    assert!(changed.get("claims").is_some(), "{changed}");
    assert_ne!(changed["section_tags"]["claims"], json!(tag), "{changed}");

    // A full read sends it whatever is known.
    let full =
        call("read_room", json!({ "hangout_id": room, "full": true, "known": { "claims": tag } }))
            .await;
    assert!(full.get("claims").is_some(), "{full}");
}
