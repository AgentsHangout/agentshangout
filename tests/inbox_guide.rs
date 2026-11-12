//! Test for the inbox `guide` tag, written from #1129 in docs/rooms.md.

mod support;

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

// #1129: a repeat inbox call that passes back the guide tag leaves the fixed notes out.
#[tokio::test]
async fn a_repeat_inbox_skips_the_fixed_notes_until_they_change() {
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

    let call = |args: Value| {
        let client = client.clone();
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "inbox", "arguments": args } });
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

    // The first call sends the clock's note and a guide tag.
    let first = call(json!({})).await;
    assert!(first["clock"]["note"].is_string(), "{first}");
    let guide = first["section_tags"]["inbox_guide"].as_str().unwrap().to_string();
    assert_eq!(guide.len(), 16, "{first}");
    assert!(first["trust_note"].is_string(), "{first}");

    // Passing the tag back: the notes are gone and say so, and the clock's times stay.
    let repeat = call(json!({ "known": { "inbox_guide": guide } })).await;
    assert!(repeat["clock"]["note"].is_string(), "{repeat}");
    assert!(repeat["clock"]["owner_now"].is_string() || repeat["clock"]["now"].is_string());
    assert!(repeat["trust_note"].is_string(), "{repeat}");
    assert!(repeat["unchanged"].as_array().unwrap().contains(&json!("inbox_guide")), "{repeat}");
    assert_eq!(repeat["section_tags"]["inbox_guide"], first["section_tags"]["inbox_guide"]);
    // #1135 follow-up: the capacity note and block_how are fixed text too, so they go with the rest.
    assert!(first["project_capacity"]["note"].is_string(), "{first}");
    assert!(first["routine"]["block_how"].is_string(), "{first}");
    assert!(repeat["project_capacity"].get("note").is_none(), "{repeat}");
    assert!(repeat["routine"].get("block_how").is_none(), "{repeat}");

    // full sends everything again, whatever was passed.
    let full = call(json!({ "full": true, "known": { "inbox_guide": guide } })).await;
    assert!(full["clock"]["note"].is_string(), "{full}");
    assert!(full.get("unchanged").is_none(), "{full}");

    // A tag that doesn't match sends the notes.
    let other = call(json!({ "known": { "inbox_guide": "0123456789abcdef" } })).await;
    assert!(other["clock"]["note"].is_string(), "{other}");

    // read_room's `guide` tag is its own key. inbox's tag used to be `guide` too, so merging the two
    // known maps overwrote read_room's tag and read_room resent its whole guide (live on v0.1.720).
    assert!(first["section_tags"].get("guide").is_none(), "{first}");
    let clash = call(json!({ "known": { "guide": guide } })).await;
    assert!(clash.get("unchanged").is_none(), "{clash}");

    // The clock's note is not in the tag: a clock change keeps the tag, and the note still comes.
    state.db.set_person_clock(&ann, "America/Chicago", -360, agenthangout::now()).unwrap();
    let clocked = call(json!({ "known": { "inbox_guide": guide } })).await;
    assert!(clocked["clock"]["note"].is_string(), "{clocked}");
    assert_eq!(clocked["section_tags"]["inbox_guide"], first["section_tags"]["inbox_guide"]);
    assert!(clocked["unchanged"].as_array().unwrap().contains(&json!("inbox_guide")), "{clocked}");
}
