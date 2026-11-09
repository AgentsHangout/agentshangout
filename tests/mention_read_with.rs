//! E47 in docs/events.md: a mention carries the brief read_room to make, and following it returns
//! the mention with unchanged sections left out on a repeat (I116).

mod support;

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
                require_rules: false,
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self { base, client: Client::new(), state }
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Value {
        let mut request = self.client.post(format!("{}{path}", self.base)).json(&body);
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        let text = request.send().await.unwrap().text().await.unwrap();
        serde_json::from_str(&text).unwrap()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let reply = self
            .post(
                token,
                "/mcp",
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                        "params": { "name": name, "arguments": arguments } }),
            )
            .await;
        reply["result"]["structuredContent"].clone()
    }
}

#[tokio::test]
async fn a_mention_carries_the_brief_read_that_returns_it() {
    let server = Server::start().await;
    let created =
        server.post("", "/api/hangouts", json!({ "name": "Crew", "your_name": "Justin" })).await;
    let justin = created["owner_key"].as_str().unwrap().to_string();
    let room = created["hangout"]["id"].as_str().unwrap().to_string();
    let person =
        server.state.authenticate(&agenthangout::auth::hash_secret(&justin)).unwrap().unwrap();
    let (_, tree) = support::legacy_bot(&server.state, &person.person_id, &room, "Grokliver Tree");
    let (_, other) = support::legacy_bot(&server.state, &person.person_id, &room, "Other");
    let (tree, other) = (tree, other);
    let say = |token: String, text: &'static str| {
        let (server, room) = (&server, room.clone());
        async move {
            server.tool(&token, "say", json!({ "hangout_id": room, "text": text })).await;
        }
    };
    say(other.clone(), "zeroth line").await;
    say(other.clone(), "earlier line").await;
    say(other.clone(), "first line").await;
    say(other.clone(), "second line").await;
    say(other.clone(), "@Grokliver Tree what do you think?").await;

    let events = server.tool(&tree, "events_since", json!({ "since": "0" })).await;
    let mention = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "mention")
        .unwrap_or_else(|| panic!("no mention in {events}"))
        .clone();
    let with = mention["data"]["read_with"].clone();
    assert_eq!(with["tool"], "read_room", "{mention}");
    assert_eq!(with["hangout_id"], room.as_str(), "{mention}");
    assert_eq!(with["brief"], true, "{mention}");
    let mention_id = mention["data"]["message_id"].as_i64().unwrap();
    assert!(with["after_message_id"].as_i64().unwrap() < mention_id, "{mention}");
    assert!(mention["data"]["what_to_do"].as_str().unwrap().contains("read_with"));

    // Calling it exactly as given returns the mention.
    let args = json!({
        "hangout_id": with["hangout_id"],
        "brief": with["brief"],
        "after_message_id": with["after_message_id"],
    });
    let read = server.tool(&tree, "read_room", args.clone()).await;
    let ids: Vec<i64> =
        read["messages"].as_array().unwrap().iter().filter_map(|m| m["id"].as_i64()).collect();
    assert!(ids.contains(&mention_id), "{read}");

    // The server remembered the tags (I116), so a repeat leaves unchanged sections out.
    let again = server.tool(&tree, "read_room", args).await;
    let unchanged = again["unchanged"].as_array().unwrap_or_else(|| panic!("{again}"));
    assert!(!unchanged.is_empty(), "{again}");
    assert!(again.get("whiteboard").is_none() && again.get("pins").is_none(), "{again}");
}

// Tadpole's test (msg 10473): a read that clears a mention must not drop the line. A bot that
// crashes after catching up and before replying can read the same line again with read_with.
#[tokio::test]
async fn a_read_that_clears_the_mention_still_returns_the_line() {
    let server = Server::start().await;
    let created =
        server.post("", "/api/hangouts", json!({ "name": "Crew", "your_name": "Justin" })).await;
    let justin = created["owner_key"].as_str().unwrap().to_string();
    let room = created["hangout"]["id"].as_str().unwrap().to_string();
    let person =
        server.state.authenticate(&agenthangout::auth::hash_secret(&justin)).unwrap().unwrap();
    let (tree_agent, tree) =
        support::legacy_bot(&server.state, &person.person_id, &room, "Grokliver Tree");
    let (_, other) = support::legacy_bot(&server.state, &person.person_id, &room, "Other");
    server.tool(&other, "say", json!({ "hangout_id": room, "text": "earlier line" })).await;
    server
        .tool(&other, "say", json!({ "hangout_id": room, "text": "@Grokliver Tree you there?" }))
        .await;

    let events = server.tool(&tree, "events_since", json!({ "since": "0" })).await;
    let mention = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "mention")
        .unwrap_or_else(|| panic!("no mention in {events}"))
        .clone();
    let with = mention["data"]["read_with"].clone();
    let mention_id = mention["data"]["message_id"].as_i64().unwrap();
    let args = json!({
        "hangout_id": with["hangout_id"],
        "brief": with["brief"],
        "after_message_id": with["after_message_id"],
    });

    // The first read clears the mention.
    server.tool(&tree, "read_room", args.clone()).await;
    let unread = server.state.db.unread_mentions(&tree_agent.id).unwrap();
    assert!(unread.iter().all(|m| m.message_id != mention_id), "mention still unread");

    // A repeat read with the same after_message_id still returns the line.
    let again = server.tool(&tree, "read_room", args).await;
    let ids: Vec<i64> =
        again["messages"].as_array().unwrap().iter().filter_map(|m| m["id"].as_i64()).collect();
    assert!(ids.contains(&mention_id), "{again}");
}
