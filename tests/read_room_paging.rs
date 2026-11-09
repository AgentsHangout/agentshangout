//! Test for paging `read_room` forward from a cursor, written from I157 in docs/rooms.md.

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

// I157
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
    let unread = |id: i64| {
        state.db.unread_mentions(&fern.id).unwrap().iter().filter(|m| m.message_id == id).count()
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
    assert_eq!(unread(mention_id), 1, "a mention past the page stays unread");

    // The second page starts at the last id and holds the other 50.
    let page2 = read(json!({
        "hangout_id": room, "oldest_first": true, "after_message_id": first + 199, "limit": 200
    }))
    .await;
    let two = ids(&page2);
    assert_eq!(two, (first + 200..first + 250).collect::<Vec<_>>());
    assert_eq!(page2["has_more"], false, "{page2}");
    assert_eq!(unread(mention_id), 0, "the page that reaches the mention reads it");

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

    // A thread in a page whose reply comes later doesn't move the cursor past the page, and a
    // mention between the two stays unread until its page.
    let say = |body: &str| {
        state
            .db
            .insert_message(&NewMessage {
                hangout_id: &room,
                person_id: Some(&ann),
                agent_id: None,
                speaker: "Ann",
                kind: "chat",
                body,
                now: now(),
            })
            .unwrap()
    };
    let cursor = first + 249;
    let root = say("Thread root");
    say("Between");
    let last_of_page = say("End of page one");
    let named = say("Fern, over here");
    state.db.add_mention(&fern.id, &room, named.id, now()).unwrap();
    let spare = say("Spare line");
    let reply = NewMessage {
        hangout_id: &room,
        person_id: Some(&ann),
        agent_id: None,
        speaker: "Ann",
        kind: "chat",
        body: "Late reply",
        now: now(),
    };
    let late = state.db.insert_reply(&reply, root.id, None).unwrap().unwrap();
    state.db.add_mention(&fern.id, &room, late.id, now()).unwrap();
    let page = |after: i64| {
        read(json!({
            "hangout_id": room, "oldest_first": true, "after_message_id": after, "limit": 3
        }))
    };
    let one = page(cursor).await;
    assert_eq!(ids(&one), vec![root.id, root.id + 1, last_of_page.id], "{one}");
    assert_eq!(one["last_message_id"], last_of_page.id, "{one}");
    assert_eq!(one["has_more"], true);
    assert_eq!(unread(named.id), 1, "a mention past the page stays unread");
    assert_eq!(unread(late.id), 1);
    let two = page(last_of_page.id).await;
    assert_eq!(ids(&two), vec![named.id, spare.id, late.id], "{two}");
    // The reply is its own line carrying reply_to, and its root is not sent again.
    assert_eq!(two["messages"][2]["reply_to"], root.id, "{two}");
    assert_eq!(two["last_message_id"], late.id, "{two}");
    assert_eq!(two["has_more"], false);
    assert_eq!(unread(named.id), 0);
    assert_eq!(unread(late.id), 0, "a mention in a reply shown on the page is read");
    for reply in [&one, &two] {
        let got = ids(reply);
        assert!(got.windows(2).all(|pair| pair[0] < pair[1]), "{reply}");
    }
    assert!(ids(&one).iter().all(|id| *id > cursor));
    assert!(ids(&two).iter().all(|id| *id > last_of_page.id));
}

// I157 (#878): a catch-up read with no limit is the oldest 50 after the cursor. A batch over that cap
// resumes on the next call with nothing skipped, a long line is cut and marked, and a long line
// that mentions the bot keeps its full text.
#[tokio::test]
async fn read_room_catch_up_pages_fifty_at_a_time_and_cuts_long_lines_unless_they_name_the_bot() {
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

    let long = "x".repeat(600);
    let mut ids_in_order = Vec::new();
    let mut named = 0;
    for n in 1..=120 {
        let body = match n {
            60 => format!("Fern, {long} end of ask"),
            70 => format!("Long chatter {long}"),
            _ => format!("Line {n}"),
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
        if n == 60 {
            named = line.id;
            state.db.add_mention(&fern.id, &room, line.id, now()).unwrap();
        }
        ids_in_order.push(line.id);
    }
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

    // Each call takes the next 50 after the last id it got, so the batches add up to every line.
    let mut cursor = ids_in_order[0] - 1;
    let mut got: Vec<i64> = Vec::new();
    let mut calls = 0;
    loop {
        let page =
            read(json!({ "hangout_id": room, "oldest_first": true, "after_message_id": cursor }))
                .await;
        calls += 1;
        let batch = ids(&page);
        assert!(batch.len() <= 50, "{page}");
        if let Some(last) = batch.last() {
            cursor = *last;
        }
        got.extend(batch);
        if page["has_more"] == false {
            break;
        }
        assert_eq!(page["messages"].as_array().unwrap().len(), 50, "{page}");
        assert!(calls < 10, "paging never ends");
    }
    assert_eq!(calls, 3, "120 lines take three catch-up calls");
    assert_eq!(got, ids_in_order, "nothing skipped, nothing repeated");

    // The cut line is marked, the line that names the bot keeps its full text.
    let page2 = read(
        json!({ "hangout_id": room, "oldest_first": true, "after_message_id": ids_in_order[49] }),
    )
    .await;
    let lines = page2["messages"].as_array().unwrap();
    let mentioned = lines.iter().find(|m| m["id"] == named).unwrap();
    assert!(mentioned.get("cut").is_none(), "{mentioned}");
    assert_eq!(mentioned["text"], format!("Fern, {long} end of ask"), "{mentioned}");
    let chatter = lines.iter().find(|m| m["id"] == ids_in_order[69]).unwrap();
    assert_eq!(chatter["cut"], true, "{chatter}");
    assert_eq!(chatter["text"].as_str().unwrap().chars().count(), 501, "{chatter}");
    assert!(chatter["text"].as_str().unwrap().ends_with('…'), "{chatter}");
    let short = lines.iter().find(|m| m["id"] == ids_in_order[50]).unwrap();
    assert!(short.get("cut").is_none(), "{short}");
    let unread = state
        .db
        .unread_mentions(&fern.id)
        .unwrap()
        .iter()
        .filter(|m| m.message_id == named)
        .count();
    assert_eq!(unread, 0, "the page that reaches the mention reads it");
}
