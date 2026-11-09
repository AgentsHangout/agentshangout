//! #575: the pages start without throwing. A real test server runs, and tests/page_smoke.mjs loads
//! the shipped web/*.js in Node for the map, a room and the home page, signed in as a person. A
//! runtime error while a page builds itself, like #558's `const done` used before it was declared,
//! fails here instead of on the live site. The map is loaded once more by someone with god mode on,
//! so what only they get (W75, A114) is built too.
mod support;

use agenthangout::{app, auth, db::Db, now, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

#[tokio::test]
async fn pages_start_without_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let db = Db::open(":memory:").unwrap();
    let god = "godsmoke00000001";
    let god_key = auth::new_owner_key();
    db.create_person(god, "God", &auth::hash_secret(&god_key), &auth::new_feed_token(), now())
        .unwrap();
    db.set_god_mode(god, true, now()).unwrap();
    let state = AppState::new(
        db,
        Config {
            public_url: base.clone(),
            create_code: Some("test".into()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![god.into()],
            require_rules: false,
        },
    );
    let served = state.clone();
    let server = tokio::spawn(async move { axum::serve(listener, app(served)).await });
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
    // S86: a bot of Owner's in the room, so its crew row (picture, badge, Bonds) is built too.
    let (scout, _) =
        support::legacy_bot(&state, owner["person"]["id"].as_str().unwrap(), room, "Scout");
    // GM20: a season it won, so the row's champion line is built as well.
    let line = "Season 1 is over: Scout won it, with 1 day won and 1 duel win.";
    state.db.record_season_end(1, room, &[&scout.id], (1, 1), line, now()).unwrap();
    // V12: an open vote, so its card is drawn over the chat.
    Client::new()
        .post(format!("{base}/api/hangouts/{room}/votes"))
        .bearer_auth(owner["owner_key"].as_str().unwrap())
        .json(&json!({"question":"Lunch?", "options":["Tacos","Pizza"], "minutes":30}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    // F58: a friend invited to one of Owner's plans by name, so their Needs you draws an invited
    // plan when the home page loads, and that code runs here too.
    let friend_key = invited_friend(&base, &key, room).await;
    for (key, path) in [
        (key.clone(), "/".to_string()),
        (key.clone(), "/world/commons".to_string()),
        (key.clone(), format!("/h/{room}")),
        (friend_key, "/".to_string()),
        (god_key, "/world/commons".to_string()),
    ] {
        let base = base.clone();
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

/// Owner proposes a plan and invites a friend to it by name; the friend's key is returned. They're
/// friends through the friend's own room, which Owner joins (FR1).
async fn invited_friend(base: &str, owner_key: &str, room: &str) -> String {
    let client = Client::new();
    let send = |key: &str, path: &str, body: Value| {
        client.post(format!("{base}{path}")).bearer_auth(key).json(&body).send()
    };
    let json = |reply: reqwest::Response| async move {
        reply.error_for_status().unwrap().json::<Value>().await.unwrap()
    };
    let plan = json(
        send(
            owner_key,
            &format!("/api/hangouts/{room}/events"),
            json!({"title":"Smoke tacos","starts_at":"2031-06-06T18:00:00Z"}),
        )
        .await
        .unwrap(),
    )
    .await;
    let theirs = json(
        send(
            "",
            "/api/hangouts",
            json!({"name":"Friend place","your_name":"Friend","create_code":"test"}),
        )
        .await
        .unwrap(),
    )
    .await;
    let code = theirs["invite_url"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
    json(send(owner_key, &format!("/api/invites/{code}/join"), json!({})).await.unwrap()).await;
    let friend = theirs["person"]["id"].as_str().unwrap();
    let plan_id = plan["id"].as_str().unwrap();
    let sent = json(
        send(owner_key, &format!("/api/events/{plan_id}/invitees"), json!({"person_ids":[friend]}))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(sent["invited"].as_array().map(Vec::len), Some(1), "{sent}");
    let friend_key = theirs["owner_key"].as_str().unwrap().to_string();
    let needs =
        client.get(format!("{base}/api/needs-you")).bearer_auth(&friend_key).send().await.unwrap();
    let needs = json(needs).await;
    assert_eq!(needs["invited_plans"].as_array().map(Vec::len), Some(1), "{needs}");
    friend_key
}
