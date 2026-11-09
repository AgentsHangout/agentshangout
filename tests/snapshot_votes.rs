//! #940: read_room builds its own list of votes, so the snapshot under it doesn't build them.

mod support;

use agenthangout::{
    app,
    db::{Db, NewVote},
    now, ops, votes, AppState, Config,
};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";

#[tokio::test]
async fn read_room_lists_the_open_votes_once_and_snapshot_skips_them_only_for_it() {
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
    let (bot, token) = support::legacy_bot(&state, &ann, &room, "Fern");
    let t0 = now();
    for i in 0..3 {
        let (voter, _) = support::legacy_bot(&state, &ann, &room, &format!("Voter{i}"));
        let opts = vec!["Yes".to_string(), "No".to_string()];
        let cands = vec![None, None];
        state
            .db
            .open_vote(&NewVote {
                hangout_id: &room,
                agent_id: Some(&voter.id),
                person_id: None,
                opener: "Ann",
                question: &format!("Vote {i}?"),
                options: &opts,
                candidates: &cands,
                now: t0,
                closes_at: t0 + 86400,
                announcement: "a vote is open",
            })
            .unwrap()
            .expect("vote opened");
    }

    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "read_room", "arguments": { "hangout_id": room } } });
    let reply: Value = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(reply["result"]["isError"].is_null(), "{reply}");
    let shown = reply["result"]["structuredContent"]["votes"].as_array().unwrap();
    assert_eq!(shown.len(), 3, "{reply}");
    assert!(shown.iter().all(|vote| vote["open"] == json!(true)));

    let caller = agenthangout::auth::Caller {
        person_id: ann.clone(),
        agent_id: Some(bot.id.clone()),
        name: "Fern".into(),
        secret_hash: "x".into(),
        is_house: false,
    };
    let without = ops::snapshot_without_votes(&state, &caller, &room).unwrap();
    assert!(without.votes.is_empty());
    let with = ops::snapshot(&state, &caller, &room).unwrap();
    let direct = votes::for_room(&state, &caller, &room).unwrap();
    assert_eq!(with.votes, direct);
    assert_eq!(direct.len(), 3);
}
