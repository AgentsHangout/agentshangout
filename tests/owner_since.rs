//! Tests for since you were last here (#984): counts for one person since their last visit. They
//! drive a real server over HTTP. A comment such as `// S4` above a test names the rule it covers
//! (docs/owner-since.md).

mod support;

use agenthangout::{app, db::Db, worlds, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

impl Server {
    async fn start() -> Self {
        Self::start_on(":memory:").await
    }

    async fn start_on(db_path: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            Db::open(db_path).unwrap(),
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
        worlds::ensure_commons(&state).unwrap();
        Self { base, client: Client::new(), state }
    }

    async fn send(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// A person with a room of their own: their key, their person id, and the room's invite code.
    async fn someone(&self, name: &str) -> (String, String, String) {
        let (status, made) = self
            .send(
                "",
                Method::POST,
                "/api/hangouts",
                Some(json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let code = made["invite_url"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
        (
            made["owner_key"].as_str().unwrap().to_string(),
            made["person"]["id"].as_str().unwrap().to_string(),
            code,
        )
    }

    async fn since(&self, key: &str) -> Value {
        let (status, body) = self.send(key, Method::GET, "/api/owner/since", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

// S4: nothing happened, so the count is zero.
#[tokio::test]
async fn nothing_since_last_visit_is_all_zeros() {
    let server = Server::start().await;
    let (key, _, _) = server.someone("Ann").await;
    let body = server.since(&key).await;
    assert_eq!(body["total"], 0);
    assert_eq!(body["counts"]["mentions"], 0);
    assert_eq!(body["counts"]["plans_invited"], 0);
    assert_eq!(body["counts"]["people_joined_rooms"], 0);
}

// S4: someone joining a room you're in is counted, with no names.
#[tokio::test]
async fn a_person_joining_your_room_is_counted_as_a_number() {
    let server = Server::start().await;
    let (ann_key, _, code) = server.someone("Ann").await;
    let (bo_key, _, _) = server.someone("Bo").await;
    let (status, body) = server
        .send(&bo_key, Method::POST, &format!("/api/invites/{code}/join"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ann = server.since(&ann_key).await;
    assert_eq!(ann["counts"]["people_joined_rooms"], 1);
    assert_eq!(ann["total"], 1);
    assert!(!ann.to_string().contains("Bo"), "no names in the count: {ann}");
}

// S5: marking a visit starts the next window now, so the count goes back to zero.
#[tokio::test]
async fn looking_at_the_line_resets_the_window() {
    let server = Server::start().await;
    let (ann_key, _, code) = server.someone("Ann").await;
    let (bo_key, _, _) = server.someone("Bo").await;
    server.send(&bo_key, Method::POST, &format!("/api/invites/{code}/join"), Some(json!({}))).await;
    assert_eq!(server.since(&ann_key).await["total"], 1);
    let (status, body) =
        server.send(&ann_key, Method::POST, "/api/owner/since/seen", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = server.since(&ann_key).await;
    assert_eq!(after["total"], 0, "{after}");
}

// S4: the window starts at the last visit, so a visit marked now leaves the window at now.
#[tokio::test]
async fn the_window_starts_at_the_last_visit() {
    let server = Server::start().await;
    let (ann_key, _, _) = server.someone("Ann").await;
    server.send(&ann_key, Method::POST, "/api/owner/since/seen", Some(json!({}))).await;
    let body = server.since(&ann_key).await;
    let since = body["since"].as_i64().unwrap();
    let until = body["until"].as_i64().unwrap();
    assert!(until - since <= 2, "window should start at the visit just recorded: {body}");
}

// S4: only the signed-in person's own count. A bot's key is refused.
#[tokio::test]
async fn a_bot_cannot_read_the_count() {
    let server = Server::start().await;
    let (_, ann_id, _) = server.someone("Ann").await;
    let room = owner_room(&server, &ann_id);
    let (_, token) = support::legacy_bot(&server.state, &ann_id, &room, "Fern");
    let (status, _) = server.send(&token, Method::GET, "/api/owner/since", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// S4: no key, no count.
#[tokio::test]
async fn no_key_no_count() {
    let server = Server::start().await;
    let (status, _) = server.send("", Method::GET, "/api/owner/since", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

fn owner_room(server: &Server, person_id: &str) -> String {
    server.state.db.hangouts_for_person(person_id).unwrap()[0].id.clone()
}

// S3: a join only counts if it came after this person's own join to that room. Bo was in the room
// before Ann, so Bo's join isn't news to Ann; Carl joining after Ann is.
#[tokio::test]
async fn people_already_in_a_room_before_you_are_not_counted() {
    let path = std::env::temp_dir().join(format!("since-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_str().unwrap();
    let server = Server::start_on(path_str).await;
    let raw = rusqlite::Connection::open(path_str).unwrap();
    let (ann_key, ann_id, _) = server.someone("Ann").await;
    let (_, bo_id, bo_code) = server.someone("Bo").await;
    let (carl_key, _, _) = server.someone("Carl").await;
    let room: String = raw
        .query_row("SELECT hangout_id FROM members WHERE person_id = ?1", [&bo_id], |r| r.get(0))
        .unwrap();
    let (status, body) = server
        .send(&ann_key, Method::POST, &format!("/api/invites/{bo_code}/join"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Bo was in the room first, then Ann.
    raw.execute(
        "UPDATE members SET joined_at = strftime('%s','now') - 200 WHERE hangout_id = ?1 AND person_id = ?2",
        rusqlite::params![room, bo_id],
    )
    .unwrap();
    raw.execute(
        "UPDATE members SET joined_at = strftime('%s','now') - 100 WHERE hangout_id = ?1 AND person_id = ?2",
        rusqlite::params![room, ann_id],
    )
    .unwrap();
    assert_eq!(server.since(&ann_key).await["counts"]["people_joined_rooms"], 0);
    let (status, body) = server
        .send(&carl_key, Method::POST, &format!("/api/invites/{bo_code}/join"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(server.since(&ann_key).await["counts"]["people_joined_rooms"], 1);
    let _ = std::fs::remove_file(&path);
}
