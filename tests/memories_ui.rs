//! MM16: a bot's feelings drawn on the Trust panel's graph, run from tests/memories_ui.cjs. The bot
//! saves its memories through MCP on a real server, its owner reads them from the MM15 route, and
//! that reply is handed to the script, which checks its stub has the same fields and draws it.

mod support;

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
    state: AppState,
    path: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path));
        }
    }
}

impl Server {
    async fn start() -> Self {
        let path = support::db_path("memories-ui").to_str().unwrap().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state = AppState::new(
            Db::open(&path).unwrap(),
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
        Self { base, client: Client::new(), state, path }
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
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn ok(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Value {
        let (status, body) = self.send(token, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        body
    }

    async fn remember(&self, token: &str, arguments: Value) -> Value {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "remember", "arguments": arguments } });
        let result = support::structured(
            self.ok(token, Method::POST, "/mcp", Some(call)).await["result"].clone(),
        );
        assert!(result["isError"].is_null(), "remember failed: {result}");
        result["structuredContent"].clone()
    }

    /// Marks the bot as having slept an hour ago, so `slept_at` is set without walking it home.
    fn slept(&self, bot: &str) {
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        conn.execute(
            "INSERT INTO bot_sleep (bot_id, slept_at) VALUES (?1, ?2)",
            rusqlite::params![bot, agenthangout::now() - 3600],
        )
        .unwrap();
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

#[tokio::test]
async fn mm16_trust_graph_draws_bot_memories() {
    let server = Server::start().await;
    // Ann's Cafe with Bo in it; Fern is Ann's bot and Rover is Bo's, both in the Cafe.
    let room = json!({ "name": "Ann's Cafe", "your_name": "Ann", "create_code": CODE });
    let made = server.ok("", Method::POST, "/api/hangouts", Some(room)).await;
    let (cafe, ann, ann_key) =
        (text(&made["hangout"]["id"]), text(&made["person"]["id"]), text(&made["owner_key"]));
    let code = text(&made["invite_url"]).rsplit('/').next().unwrap().to_string();
    let join = format!("/api/invites/{code}/join");
    let bo = text(
        &server.ok("", Method::POST, &join, Some(json!({ "your_name": "Bo" }))).await["person"]
            ["id"],
    );
    let (fern, fern_token) = support::legacy_bot(&server.state, &ann, &cafe, "Fern");
    let (rover, _) = support::legacy_bot(&server.state, &bo, &cafe, "Rover");

    let note = |subject: String, words: &str, feeling: &str, closeness: i64| json!({ "text": words, "subject": subject, "note": true, "feeling": feeling, "closeness": closeness });
    let evil = "<img src=x onerror=alert(1)>";
    server.remember(&fern_token, note(format!("person:{bo}"), evil, "fond", 2)).await;
    server
        .remember(&fern_token, note(format!("agent:{}", rover.id), "Rover hums.", "wary", -1))
        .await;
    let on_room =
        json!({ "text": "Busy at noon.", "subject": format!("room:{cafe}"), "note": true });
    server.remember(&fern_token, on_room).await;
    server.remember(&fern_token, json!({ "text": "Painted a fox.", "importance": 4 })).await;
    let gone = server.remember(&fern_token, json!({ "text": "Forget me." })).await;
    server.slept(&fern.id);

    // The bot can't read its owner's view; the owner deletes one memory and reads the rest.
    let route = format!("/api/agents/{}/memories", fern.id);
    assert_eq!(server.send(&fern_token, Method::GET, &route, None).await.0, StatusCode::FORBIDDEN);
    let forget = format!("{route}/{}", text(&gone["id"]));
    let deleted = server.ok(&ann_key, Method::DELETE, &forget, None).await;
    assert_eq!(deleted, json!({ "ok": true, "deleted": gone["id"] }));
    let view = server.ok(&ann_key, Method::GET, &route, None).await;
    assert_eq!(view["edges"].as_array().unwrap().len(), 2, "{view}");

    let file = std::env::temp_dir().join(format!(
        "memories-ui-{}-{}.json",
        std::process::id(),
        agenthangout::auth::new_id()
    ));
    let handed = json!({
        "view": view,
        "owner": ann,
        "bot": fern.id,
        "people": [{ "id": ann, "name": "Ann" }, { "id": bo, "name": "Bo" }],
        "bots": [
            { "id": fern.id, "name": "Fern", "owner_id": ann },
            { "id": rover.id, "name": "Rover", "owner_id": bo },
        ],
    });
    std::fs::write(&file, handed.to_string()).unwrap();
    let output = std::process::Command::new("node")
        .arg("tests/memories_ui.cjs")
        .arg(&file)
        .output()
        .expect("node runs");
    let _ = std::fs::remove_file(&file);
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
