//! #575: the pages start without throwing. A real test server runs, and tests/page_smoke.mjs loads
//! the shipped web/*.js in Node for the map, a room and the home page, signed in as a person. A
//! runtime error while a page builds itself, like #558's `const done` used before it was declared,
//! fails here instead of on the live site.
use agenthangout::{app, db::Db, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

#[tokio::test]
async fn pages_start_without_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: Some("test".into()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    let server = tokio::spawn(async move { axum::serve(listener, app(state)).await });
    let owner: Value = Client::new()
        .post(format!("{base}/api/hangouts"))
        .json(&json!({"name":"Smoke Cafe", "your_name":"Owner", "create_code":"test"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let key = owner["owner_key"].as_str().unwrap().to_string();
    let room = owner["hangout"]["id"].as_str().unwrap();
    for path in ["/".to_string(), "/world/commons".to_string(), format!("/h/{room}")] {
        let (base, key) = (base.clone(), key.clone());
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("node")
                .args(["tests/page_smoke.mjs", &base, &key, &path])
                .output()
                .expect("node runs")
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    server.abort();
}
