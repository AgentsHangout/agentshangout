//! Test for paging `read_room` forward from a cursor, written from I154 in docs/rooms.md.

mod support;

use agenthangout::{
    app,
    db::{Db, NewMessage},
    now, AppState, Config,
};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

fn ids(reply: &Value) -> Vec<i64> {
    reply["messages"].as_array().unwrap().iter().map(|m| m["id"].as_i64().unwrap()).collect()
}

// I154
#[tokio::test]
async fn read_room_oldest_first_pages_forward_without_skipping_or_clearing_unread_mentions() {
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
    let (fern, token) = support::legacy_bot(&state, &ann, &room, "Fern");

    let mut first = 0;
    let mut mention_id = 0;
    for n in 1..=250 {
        let body = format!("Line {n}");
        let line = state
            .db
            .insert_message(&NewMessage {
                hangout_id: &room,
                person_id: Some(&ann),
                agent_id: None,
                speaker: "Ann",
                kind: "chat",
                body: &body,
                now: now(),
            })
            .unwrap();
        if n == 1 {
            first = line.id;
        }
        if n == 220 {
            mention_id = line.id;
            state.db.add_mention(&fern.id, &room, line.id, now()).unwrap();
        }
    }
    let unread = || {
        state
            .db
            .unread_mentions(&fern.id)
            .unwrap()
            .iter()
            .filter(|m| m.message_id == mention_id)
            .count()
    };
    let read = |args: Value| {
        let client = client.clone();
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "read_room", "arguments": args } });
        let (url, token) = (format!("{base}/mcp"), token.clone());
        async move {
            let reply: Value = client
                .post(url)
                .bearer_auth(token)
                .json(&call)
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

    // The first page is the oldest 200, in order, with more to come. The mention is further on.
    let page1 = read(json!({
        "hangout_id": room, "oldest_first": true, "after_message_id": first - 1, "limit": 200
    }))
    .await;
    let one = ids(&page1);
    assert_eq!(one, (first..first + 200).collect::<Vec<_>>());
    assert_eq!(page1["has_more"], true, "{page1}");
    assert_eq!(page1["last_message_id"], first + 199);
    assert!(page1.get("earlier_messages").is_none(), "{page1}");
    assert_eq!(unread(), 1, "a mention past the page stays unread");

    // The second page starts at the last id and holds the other 50.
    let page2 = read(json!({
        "hangout_id": room, "oldest_first": true, "after_message_id": first + 199, "limit": 200
    }))
    .await;
    let two = ids(&page2);
    assert_eq!(two, (first + 200..first + 250).collect::<Vec<_>>());
    assert_eq!(page2["has_more"], false, "{page2}");
    assert_eq!(unread(), 0, "the page that reaches the mention reads it");

    // Without a cursor the flag starts at the room's first line.
    let start = read(json!({ "hangout_id": room, "oldest_first": true, "limit": 5 })).await;
    assert_eq!(ids(&start), (first..first + 5).collect::<Vec<_>>());
    assert_eq!(start["has_more"], true);

    // Without the flag nothing changes: the newest lines, no has_more, just the newest.
    let plain = read(json!({ "hangout_id": room, "after_message_id": 0, "limit": 20 })).await;
    let newest = ids(&plain);
    assert_eq!(newest.len(), 20);
    assert_eq!(newest.last().copied(), Some(first + 249));
    assert!(plain.get("has_more").is_none(), "{plain}");
    assert!(plain["earlier_messages"].as_i64().unwrap() > 0, "{plain}");
}
