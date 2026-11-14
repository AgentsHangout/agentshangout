//! #1195: a full read_room leaves out item fields at their default value.

mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

#[tokio::test]
async fn full_read_room_leaves_out_default_item_fields() {
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
        .json(&json!({ "name": "Shelf Room", "your_name": "Ann", "create_code": CODE }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = made["hangout"]["id"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let (_, token) = support::legacy_bot(&state, &ann, &room, "ShelfBot");

    let place = |id: &str, kind: &str, x: i64, facing: i64| {
        let item = agenthangout::db::NewFloorItem {
            id,
            hangout_id: &room,
            kind,
            x,
            y: 0,
            facing,
            placed_by: &ann,
            now: now(),
        };
        state.db.place_item(&item, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();
    };
    // A plain stool at its defaults, and a bookshelf (holds notes) turned to facing 1.
    place(&agenthangout::auth::new_id(), "stool", 1, 0);
    place(&agenthangout::auth::new_id(), "bookshelf", 3, 1);

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "read_room", "arguments": { "hangout_id": room } }
    });
    let reply: Value = client
        .post(format!("{base}/mcp"))
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
    let items = result["structuredContent"]["items"].as_array().unwrap();
    let find = |kind: &str| items.iter().find(|item| item["kind"] == kind).unwrap().clone();

    // Default-valued fields are absent; id, kind and position stay.
    let stool = find("stool");
    assert!(stool["id"].is_string() && stool["x"].is_number() && stool["y"].is_number(), "{stool}");
    for field in ["facing", "notes", "used_by", "holds_notes"] {
        assert!(stool.get(field).is_none(), "{field} should be absent: {stool}");
    }

    // Non-default values are kept.
    let shelf = find("bookshelf");
    assert_eq!(shelf["facing"], 1, "{shelf}");
    assert_eq!(shelf["holds_notes"], true, "{shelf}");
    assert!(shelf.get("notes").is_none(), "no notes, so absent: {shelf}");
    assert!(shelf.get("used_by").is_none(), "nobody using it, so absent: {shelf}");
}
