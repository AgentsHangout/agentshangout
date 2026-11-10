//! I153: `inbox` with `seen_up_to` marks the caller's mentions read up to a message id, in every room.

mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

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

    async fn mention(&self, pip: &str, room: &str, words: &str) -> i64 {
        let args = json!({ "hangout_id": room, "text": format!("@Scout {words}") });
        let said = self.tool(pip, "say", args).await;
        said["structuredContent"]["message_id"].as_i64().unwrap_or_else(|| panic!("{said}"))
    }
}

#[tokio::test]
async fn seen_up_to_clears_mentions_in_every_room_up_to_an_id() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: Some("letmein".into()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: Vec::new(),
            require_rules: false,
        },
    );
    let served = state.clone();
    tokio::spawn(async move { axum::serve(listener, app(served)).await });
    let server = Server { base: base.clone(), client: Client::new() };
    let made = server
        .client
        .post(format!("{base}/api/hangouts"))
        .json(&json!({ "name": "A", "your_name": "Ann", "create_code": "letmein" }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let room = made["hangout"]["id"].as_str().unwrap().to_string();
    let owner = made["person"]["id"].as_str().unwrap().to_string();
    let (scout, scout_token) = support::legacy_bot(&state, &owner, &room, "Scout");
    let (pip, pip_token) = support::legacy_bot(&state, &owner, &room, "Pip");
    state.db.create_hangout("second", "Second", "second-code", &owner, now()).unwrap();
    state.db.add_agent_to_room("second", &scout.id).unwrap();
    state.db.add_agent_to_room("second", &pip.id).unwrap();

    let first = server.mention(&pip_token, &room, "one").await;
    state.db.set_bot_place(&pip.id, "second", now()).unwrap();
    let middle = server.mention(&pip_token, "second", "two").await;
    let last = server.mention(&pip_token, "second", "three").await;
    assert!(first < middle && middle < last);
    let unread = || async {
        let inbox =
            server.tool(&scout_token, "inbox", json!({})).await["structuredContent"].clone();
        inbox["mentions_count"].as_i64().unwrap_or(0)
    };
    assert_eq!(unread().await, 3);

    // A bad value is refused and clears nothing.
    for bad in [json!(-1), json!("7"), json!(1.5)] {
        let refused = server.tool(&scout_token, "inbox", json!({ "seen_up_to": bad })).await;
        assert_eq!(refused["isError"], true, "{refused}");
    }
    assert_eq!(unread().await, 3);

    // Up to the middle id: the first (other room) and the middle one clear, the newest stays.
    let inbox = server.tool(&scout_token, "inbox", json!({ "seen_up_to": middle })).await
        ["structuredContent"]
        .clone();
    assert_eq!(inbox["mentions_count"], 1, "{inbox}");
    assert_eq!(inbox["mentions"][0]["message_id"], last, "{inbox}");
    assert_eq!(unread().await, 1);

    // Clearing the last one still says how many are left: 0, rather than leaving the count out.
    let inbox = server.tool(&scout_token, "inbox", json!({ "seen_up_to": last })).await
        ["structuredContent"]
        .clone();
    assert_eq!(inbox["mentions_count"], 0, "{inbox}");
    assert_eq!(unread().await, 0);
}
