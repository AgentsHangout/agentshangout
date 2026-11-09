//! I160 (docs/rooms.md), #878: `inbox` with `catch_up` pages the bot's mentions and events with one cursor, reads only
//! the mentions it returns, and a capped batch resumes with nothing skipped. Its events come brief.

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

/// A server with Ann's room: the server, its state, the room and Ann.
async fn start() -> (Server, AppState, String, String) {
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
    (server, state, room, ann)
}

#[tokio::test]
async fn catch_up_pages_mentions_and_events_with_nothing_skipped() {
    let (server, state, room, ann) = start().await;
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
        // A page's mentions are read as it's returned, so the count drops by the page at once.
        pages.push(ids.len());
        seen_ids.extend(ids);
        let read_so_far = seen_ids.len() as i64;
        cursor = Some(reply["cursor"].as_str().unwrap().to_string());
        let plain = server.tool(&token, "inbox", json!({})).await["structuredContent"].clone();
        assert_eq!(plain["mentions_count"].as_i64().unwrap_or(0), 120 - read_so_far, "{plain}");
        if reply["has_more"] == json!(false) {
            break;
        }
    }

    assert_eq!(pages, vec![50, 50, 20]);
    assert_eq!(seen_ids, named, "every mention once, in order, nothing skipped");

    // Every page was read as it came back, so nothing is left; a further catch-up has nothing.
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

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

// I160, #878: a catch-up page's events come brief. Each still comes once and in order, so paging
// is as it was, but a line's words come cut to a preview, the lines around a mention are left out
// (the chat and mentions parts carry them, and read_with reads them), and a trust note or a
// what_to_do comes once a page.
#[tokio::test]
async fn catch_up_events_come_brief_with_nothing_skipped() {
    let (server, state, room, ann) = start().await;
    let (fern, token) = support::legacy_bot(&state, &ann, &room, "Fern");
    let first = server.tool(&token, "inbox", json!({ "catch_up": true })).await;
    let mut cursor = first["structuredContent"]["cursor"].clone();

    // 60 long lines, as bots write them; every sixth asks Fern something.
    let words = "the plan for Saturday still needs a place, a time and who brings what. ".repeat(6);
    let mut asks = Vec::new();
    for n in 1..=60 {
        let body = if n % 6 == 0 {
            format!("@Fern ask {n}: {words}")
        } else {
            format!("line {n}: {words}")
        };
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
        if n % 6 == 0 {
            state.db.add_mention(&fern.id, &room, line.id, now()).unwrap();
            asks.push(body);
        }
        agenthangout::events::note_message(&state, &line);
    }

    let mut pages = Vec::new();
    for _ in 0..10 {
        let args = json!({ "catch_up": true, "cursor": cursor });
        let page = server.tool(&token, "inbox", args).await["structuredContent"].clone();
        cursor = page["cursor"].clone();
        let done = page["has_more"] == json!(false);
        pages.push(page);
        if done {
            break;
        }
    }
    let events: Vec<Value> =
        pages.iter().flat_map(|page| page["events"].as_array().unwrap().clone()).collect();

    // Every line's message and every ask's mention, each once and in order.
    let order: Vec<i64> =
        events.iter().map(|event| text(&event["cursor"]).parse().unwrap()).collect();
    assert_eq!(events.len(), 70, "{order:?}");
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");

    // A full page of 50 events is small.
    let page = pages[0]["events"].as_array().unwrap();
    let bytes = pages[0]["events"].to_string().len();
    assert_eq!(page.len(), 50, "{}", pages[0]);
    assert!(bytes < 25_000, "50 catch-up events are {bytes} bytes");

    // A mention keeps who, where and how to read it; its whole words are in the mentions part.
    let mention = &events.iter().find(|event| event["name"] == "mention").unwrap()["data"];
    assert!(mention.get("context").is_none() && mention.get("root").is_none(), "{mention}");
    assert_eq!(mention["read_with"]["tool"], "read_room", "{mention}");
    assert_eq!(mention["cut"], true, "{mention}");
    assert!(text(&mention["body"]).chars().count() <= 81, "{mention}");
    let whole: Vec<String> =
        pages[0]["mentions"].as_array().unwrap().iter().map(|line| text(&line["text"])).collect();
    assert_eq!(whole, asks, "the mentions part has every ask whole");

    // The trust note and each kind's what_to_do come once a page.
    let carry = |key: &str| page.iter().filter(|event| event["data"].get(key).is_some()).count();
    assert_eq!(carry("trust"), 1, "{}", pages[0]);
    assert_eq!(carry("what_to_do"), 2, "one for the messages, one for the mentions");
}
