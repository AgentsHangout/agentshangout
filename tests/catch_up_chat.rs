//! #878 (the chat slice of the catch-up inbox): `inbox` with `catch_up` also pages the chat in the
//! rooms a bot is in, oldest first, skipping its own lines and any room it isn't in, and a `cu1`
//! cursor still works, starting its chat at the newest line.

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
        support::structured(
            reply.send().await.unwrap().json::<Value>().await.unwrap()["result"].clone(),
        )
    }
}

fn say(state: &AppState, room: &str, person: &str, body: &str) -> i64 {
    state
        .db
        .insert_message(&NewMessage {
            hangout_id: room,
            person_id: Some(person),
            agent_id: None,
            speaker: "Ann",
            kind: "chat",
            body,
            now: now(),
        })
        .unwrap()
        .id
}

#[tokio::test]
async fn catch_up_chat_pages_the_rooms_a_bot_is_in() {
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

    let mut made = Vec::new();
    for name in ["Ann's room", "Other room"] {
        let reply: Value = server
            .client
            .post(format!("{base}/api/hangouts"))
            .json(&json!({ "name": name, "your_name": "Ann", "create_code": CODE }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        made.push(reply["hangout"]["id"].as_str().unwrap().to_string());
    }
    let (room, other) = (made[0].clone(), made[1].clone());
    let ann = {
        let made: Value = server
            .client
            .post(format!("{base}/api/hangouts"))
            .json(&json!({ "name": "Ann's third", "your_name": "Ann", "create_code": CODE }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        made["person"]["id"].as_str().unwrap().to_string()
    };
    let (fern, token) = support::legacy_bot(&state, &ann, &room, "Fern");

    // The first call starts the chat at the newest line, so nothing before it is replayed.
    let first = server.tool(&token, "inbox", json!({ "catch_up": true })).await
        ["structuredContent"]
        .clone();
    let cursor0 = first["cursor"].as_str().unwrap().to_string();
    assert!(cursor0.starts_with("cu2-"), "{first}");
    assert_eq!(first["chat"], json!([]), "{first}");

    // 60 lines in the room, one of them a mention of Fern; 3 in the other room, which Fern isn't in;
    // 2 from Fern itself at the end.
    let mut ann_lines = Vec::new();
    for n in 1..=60 {
        ann_lines.push(say(&state, &room, &ann, &format!("line {n}")));
    }
    let mention_line = ann_lines[10];
    state.db.add_mention(&fern.id, &room, mention_line, now()).unwrap();
    for n in 1..=3 {
        say(&state, &other, &ann, &format!("elsewhere {n}"));
    }
    for n in 1..=2 {
        state
            .db
            .insert_message(&NewMessage {
                hangout_id: &room,
                person_id: None,
                agent_id: Some(&fern.id),
                speaker: "Fern",
                kind: "chat",
                body: &format!("Fern says {n}"),
                now: now(),
            })
            .unwrap();
    }

    // Page one: the first 50 lines of the room, no Fern, nothing from the other room.
    let page1 = server.tool(&token, "inbox", json!({ "catch_up": true, "cursor": cursor0 })).await
        ["structuredContent"]
        .clone();
    let chat1 = page1["chat"].as_array().unwrap();
    assert_eq!(chat1.len(), 50, "{page1}");
    assert_eq!(page1["has_more"], json!(true), "{page1}");
    assert!(chat1.iter().all(|line| line["hangout_id"] == json!(room)), "{page1}");
    assert!(chat1.iter().all(|line| line["from"] != json!("Fern")), "{page1}");
    let ids1: Vec<i64> = chat1.iter().map(|line| line["message_id"].as_i64().unwrap()).collect();
    assert_eq!(ids1, ann_lines[..50].to_vec(), "{page1}");
    // The mention came back in the mentions part, and was read as it came back.
    assert_eq!(page1["mentions"][0]["message_id"], json!(mention_line), "{page1}");

    // Page two: the rest of the room's lines, and Fern's two are skipped.
    let page2 = server
        .tool(&token, "inbox", json!({ "catch_up": true, "cursor": page1["cursor"] }))
        .await["structuredContent"]
        .clone();
    let ids2: Vec<i64> = page2["chat"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["message_id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids2, ann_lines[50..].to_vec(), "{page2}");
    assert_eq!(page2["has_more"], json!(false), "{page2}");

    // A read_room after the mention's line still returns it, though the catch-up read it.
    let room_read = server
        .tool(
            &token,
            "read_room",
            json!({ "hangout_id": room, "after_message_id": mention_line - 1, "oldest_first": true, "limit": 2 }),
        )
        .await["structuredContent"]
        .clone();
    assert!(room_read.to_string().contains(&format!("\"id\":{mention_line}")), "{room_read}");

    // A cu1 cursor from before the chat part still works: its chat starts at the newest line.
    say(&state, &room, &ann, "after the cu1 cursor");
    let cu1 = format!("cu1-0-{}", page2["cursor"].as_str().unwrap().split('-').nth(2).unwrap());
    let old = server.tool(&token, "inbox", json!({ "catch_up": true, "cursor": cu1 })).await
        ["structuredContent"]
        .clone();
    assert_eq!(old["chat"], json!([]), "{old}");
    assert!(old["cursor"].as_str().unwrap().starts_with("cu2-"), "{old}");

    // A cursor it didn't hand out is still refused.
    let refused =
        server.tool(&token, "inbox", json!({ "catch_up": true, "cursor": "cu2-1-2" })).await;
    assert_eq!(refused["isError"], json!(true), "{refused}");
}
