//! #1194: a read_room that leaves out the bots section gives the same messages and items as before.

mod support;

use agenthangout::{
    app,
    db::{Db, NewFloorItem, NewMessage},
    now, AppState, Config,
};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

/// A room with 60 bots of one owner and 100 lines, and the bot that reads it.
async fn busy_room() -> (String, Client, AppState, String) {
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
        .json(&json!({ "name": "Busy room", "your_name": "Ann", "create_code": CODE }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = made["hangout"]["id"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let mut bots = Vec::new();
    let mut token = String::new();
    for i in 0..60 {
        let (bot, bot_token) = support::legacy_bot(&state, &ann, &room, &format!("Bot{i}"));
        if i == 0 {
            token = bot_token;
        }
        bots.push(bot);
    }
    let t = now();
    for i in 0..100 {
        let b = &bots[i % bots.len()];
        state
            .db
            .insert_message(&NewMessage {
                hangout_id: &room,
                person_id: None,
                agent_id: Some(&b.id),
                speaker: &b.name,
                kind: "chat",
                body: &format!("line {i}"),
                now: t - 1000 + i as i64,
            })
            .unwrap();
    }
    (room, client, state, token)
}

/// Calls read_room over HTTP and returns the reply.
async fn read(client: &Client, base: &str, token: &str, args: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_room", "arguments": args}});
    client
        .post(format!("{base}/mcp"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// The JSON a read_room reply carries as its text.
fn body(reply: &Value) -> Value {
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn sections_items_names_the_bot_using_a_stool() {
    let (room, client, state, token) = busy_room().await;
    let base = state.config.public_url.clone();
    let bots = state.db.agents_in(&room, now()).unwrap();
    let user = bots.iter().find(|bot| bot.name == "Bot1").unwrap().clone();
    let stool = NewFloorItem {
        id: "stool-1",
        hangout_id: &room,
        kind: "stool",
        x: 3,
        y: 3,
        facing: 0,
        placed_by: &user.owner_id,
        now: now(),
    };
    state.db.place_item(&stool, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();
    state
        .db
        .use_item(&user.id, "stool-1", now() + 600, now(), |_| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    // Bot0 (the caller) reads only the items. Bot1 is another bot, and it sits on the stool.
    let reply =
        read(&client, &base, &token, json!({ "hangout_id": room, "sections": ["items"] })).await;
    assert!(reply["result"]["isError"].is_null(), "{reply}");
    let got = body(&reply);
    assert!(got.get("bots").is_none(), "{got}");
    let items = got["items"].as_array().unwrap();
    let stool =
        items.iter().find(|item| item["id"] == "stool-1").unwrap_or_else(|| panic!("{got}"));
    assert_eq!(stool["used_by"], json!([user.name]), "{got}");
}

// The messages a messages-only read sends are the same as a full read's, and it still says
// whether the caller's own bot is here.
#[tokio::test]
async fn sections_messages_sends_the_same_lines_and_no_bots() {
    let (room, client, state, token) = busy_room().await;
    let base = state.config.public_url.clone();
    let full = read(&client, &base, &token, json!({ "hangout_id": room })).await;
    let only =
        read(&client, &base, &token, json!({ "hangout_id": room, "sections": ["messages"] })).await;
    let full = &body(&full);
    let only = &body(&only);
    assert_eq!(only["messages"], full["messages"]);
    assert_eq!(only["earlier_messages"], full["earlier_messages"]);
    assert_eq!(only["you_are_here"], json!(true));
    assert!(only.get("bots").is_none(), "{only}");
}
