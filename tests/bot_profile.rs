//! BP1-BP3: identity survives owner edits, while strangers and bots cannot edit it.
mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

#[tokio::test]
async fn owner_renames_without_changing_identity_and_invalid_or_unauthorized_edits_do_nothing() {
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
            require_rules: false,
        },
    );
    let served = state.clone();
    let server = tokio::spawn(async move { axum::serve(listener, app(served)).await });
    let client = Client::new();
    let owner: Value = client
        .post(format!("{base}/api/hangouts"))
        .json(&json!({"name":"Home", "your_name":"Owner", "create_code":"test"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let stranger: Value = client
        .post(format!("{base}/api/hangouts"))
        .json(&json!({"name":"Elsewhere", "your_name":"Stranger", "create_code":"test"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = owner["hangout"]["id"].as_str().unwrap();
    let (bot, token) =
        support::legacy_bot(&state, owner["person"]["id"].as_str().unwrap(), room, "Before");
    let path = format!("{base}/api/agents/{}/profile", bot.id);
    for (credential, status) in [
        (stranger["owner_key"].as_str().unwrap(), StatusCode::NOT_FOUND),
        (token.as_str(), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            client
                .put(&path)
                .bearer_auth(credential)
                .json(&json!({"name":"Stolen"}))
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
    let key = owner["owner_key"].as_str().unwrap();
    for invalid in [String::new(), " ".into(), "a".repeat(61), "bad\nname".into()] {
        assert_eq!(
            client
                .put(&path)
                .bearer_auth(key)
                .json(&json!({"name":invalid}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(state.db.agent(&bot.id, now()).unwrap().unwrap().name, "Before");
    }
    let updated: Value = client
        .put(&path)
        .bearer_auth(key)
        .json(&json!({"name":"  New name  "}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated["name"], "New name");
    assert_eq!(updated["id"], bot.id);
    let authenticated =
        state.authenticate(&agenthangout::auth::hash_secret(&token)).unwrap().unwrap();
    assert_eq!(authenticated.name, "New name");
    assert_eq!(authenticated.agent_id.as_deref(), Some(bot.id.as_str()));
    assert!(state.db.hangouts_for_agent(&bot.id, now()).unwrap().iter().any(|h| h.id == room));
    server.abort();
}
