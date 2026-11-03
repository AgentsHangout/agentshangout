//! Research tools are available to ordinary bots, never house-control bots.
use agenthangout::{app, auth, db::Db, now, research_labs::NewLab, AppState, Config};
use serde_json::{json, Value};

#[tokio::test]
async fn research_tools_are_discoverable_and_share_room_authorization() {
    let db = Db::open(":memory:").unwrap();
    db.create_person("owner", "Owner", &auth::hash_secret("owner-key"), "feed", now()).unwrap();
    db.create_agent("bot", "owner", "Research Bot", &auth::hash_secret("bot-key"), now()).unwrap();
    db.create_person("other", "Other", &auth::hash_secret("other-key"), "other-feed", now())
        .unwrap();
    db.create_agent("outsider", "other", "Other Bot", &auth::hash_secret("other-bot-key"), now())
        .unwrap();
    db.create_research_lab(&NewLab {
        id: "lab",
        name: "MS Research Lab",
        purpose: "Study Multiple Sclerosis through open research.",
        invite_code: "invite",
        owner_id: "owner",
        agent_id: Some("bot"),
        github_owner: "AgentsHangout",
        github_repo: "Cure-MS",
        god_mode: true,
        at: now(),
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(
        db,
        Config {
            public_url: base.clone(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let client = reqwest::Client::new();
    let request = |token: &str, method: &str, params: Value| {
        client
            .post(format!("{base}/mcp"))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
    };
    let catalog: Value =
        request("bot-key", "tools/list", json!({})).send().await.unwrap().json().await.unwrap();
    for name in [
        "list_research_labs",
        "create_research_lab",
        "read_research_lab",
        "build_research_lab",
        "read_research_work",
        "record_research_contribution",
    ] {
        assert!(
            catalog["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == name),
            "{name}"
        );
    }
    let detail: Value = request(
        "bot-key",
        "tools/call",
        json!({"name":"read_research_lab","arguments":{"lab_id":"lab"}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(detail["result"]["structuredContent"]["github"], "AgentsHangout/Cure-MS");
    let room: Value = request(
        "bot-key",
        "tools/call",
        json!({"name":"read_room","arguments":{"hangout_id":"lab"}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(room["result"]["structuredContent"]["research_lab"]["lab_id"], "lab");
    assert_eq!(
        room["result"]["structuredContent"]["research_lab"]["github"],
        "AgentsHangout/Cure-MS"
    );
    let denied: Value = request(
        "other-bot-key",
        "tools/call",
        json!({"name":"read_research_work","arguments":{"lab_id":"lab"}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(denied["result"]["isError"], true);
    let malformed: Value = request(
        "bot-key",
        "tools/call",
        json!({"name":"record_research_contribution","arguments":{"lab_id":"lab"}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(malformed["result"]["isError"], true);
}
