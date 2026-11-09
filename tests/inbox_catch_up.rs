//! I158 (docs/rooms.md), #878: `inbox` with `catch_up` pages the bot's mentions and events with one cursor, reads only
//! the mentions it returns, and a capped batch resumes with nothing skipped.

mod support;

use agenthangout::{app, db::Db, db::NewMessage, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
}

impl Server {
    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        });
        let reply = self.client.post(format!("{}/mcp", self.base)).bearer_auth(token).json(&call);
        reply.send().await.unwrap().json::<Value>().await.unwrap()["result"].clone()
    }
}

#[tokio::test]
async fn catch_up_pages_mentions_and_events_with_nothing_skipped() {
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
    let server = Server { base: base.clone(), client: Client::new() };

    let made: Value = server
        .client
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

    // 120 mentions of Fern, and one line that doesn't name it.
    let mut named = Vec::new();
    for n in 1..=120 {
        let line = state
            .db
            .insert_message(&NewMessage {
                hangout_id: &room,
                person_id: Some(&ann),
                agent_id: None,
                speaker: "Ann",
                kind: "chat",
                body: &format!("Fern, ask {n}"),
                now: now(),
            })
            .unwrap();
        state.db.add_mention(&fern.id, &room, line.id, now()).unwrap();
        named.push(line.id);
    }

    // Pages of 50, 50, 20. Each page's ids run on from the last, and the cursor is passed back.
    let mut cursor: Option<String> = None;
    let mut pages = Vec::new();
    let mut seen_ids = Vec::new();
    for _ in 0..10 {
        let mut args = json!({ "catch_up": true });
        if let Some(cursor) = &cursor {
            args["cursor"] = json!(cursor);
        }
        let reply = server.tool(&token, "inbox", args).await["structuredContent"].clone();
        assert_eq!(reply["catch_up"], json!(true), "{reply}");
        let ids: Vec<i64> = reply["mentions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["message_id"].as_i64().unwrap())
            .collect();
        pages.push(ids.len());
        seen_ids.extend(ids);
        cursor = Some(reply["cursor"].as_str().unwrap().to_string());
        if reply["has_more"] == json!(false) {
            break;
        }
        // Mid-catch-up, only the lines returned so far are read.
        let read_so_far = seen_ids.len() as i64;
        let plain = server.tool(&token, "inbox", json!({})).await["structuredContent"].clone();
        assert_eq!(plain["mentions_count"].as_i64().unwrap(), 120 - read_so_far, "{plain}");
    }

    assert_eq!(pages, vec![50, 50, 20]);
    assert_eq!(seen_ids, named, "every mention once, in order, nothing skipped");

    // Everything it returned is read; a further catch-up has nothing and says so.
    let plain = server.tool(&token, "inbox", json!({})).await["structuredContent"].clone();
    assert_eq!(plain["mentions_count"].as_i64().unwrap_or(0), 0, "{plain}");
    let done = server
        .tool(&token, "inbox", json!({ "catch_up": true, "cursor": cursor.unwrap() }))
        .await["structuredContent"]
        .clone();
    assert_eq!(done["mentions"], json!([]), "{done}");
    assert_eq!(done["has_more"], json!(false), "{done}");

    // A cursor it didn't hand out is refused.
    let refused =
        server.tool(&token, "inbox", json!({ "catch_up": true, "cursor": "not-a-cursor" })).await;
    assert_eq!(refused["isError"], json!(true), "{refused}");
}
