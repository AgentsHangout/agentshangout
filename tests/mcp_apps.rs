//! Tests for MCP Apps: interactive rooms and collaboration boards (docs/mcp-apps.md, MA1 to MA8).

mod support;

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
    state: AppState,
    addresses: AtomicUsize,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    #[track_caller]
    fn ok(self) -> Value {
        assert_eq!(self.status, StatusCode::OK, "{}", self.body);
        self.body
    }

    #[track_caller]
    fn refused(self, status: StatusCode) -> Value {
        assert_eq!(self.status, status, "{}", self.body);
        self.body
    }
}

#[derive(Clone)]
struct Who {
    key: String,
    id: String,
}

#[derive(Clone)]
#[allow(dead_code)]
struct Bot {
    id: String,
    token: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

fn who(body: &Value) -> Who {
    Who { key: text(&body["owner_key"]), id: text(&body["person"]["id"]) }
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: Some(CODE.to_string()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self { base, client: Client::new(), state, addresses: AtomicUsize::new(1) }
    }

    async fn send(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Reply {
        let n = self.addresses.fetch_add(1, Ordering::Relaxed);
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("x-forwarded-for", format!("198.51.{}.{}", n / 250, n % 250 + 1));
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        Reply { status, body: serde_json::from_str(&text).unwrap_or(Value::String(text)) }
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::POST, path, Some(body)).await
    }

    async fn put(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::PUT, path, Some(body)).await
    }

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s floor"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (text(&made["hangout"]["id"]), who(&made))
    }

    async fn bot(&self, owner: &Who, room: &str, name: &str) -> Bot {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        Bot { id: agent.id, token }
    }

    async fn mcp(&self, token: &str, method: &str, params: Value) -> Reply {
        self.post(
            token,
            "/mcp",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            }),
        )
        .await
    }
}

#[tokio::test]
async fn ma1_resources_capability_and_list() {
    let server = Server::start().await;
    let (room, justin) = server.someone("Justin").await;
    let scout = server.bot(&justin, &room, "AntiGravity Miner").await;

    // MA1: initialize returns resources capability and ui extension
    let init_res = server
        .mcp(
            &scout.token,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {}
            }),
        )
        .await
        .ok();

    let caps = &init_res["result"]["capabilities"];
    assert_eq!(caps["resources"]["subscribe"], false);
    assert_eq!(caps["resources"]["listChanged"], false);
    assert_eq!(
        caps["extensions"]["io.modelcontextprotocol/ui"]["mimeType"],
        "text/html;profile=mcp-app"
    );

    // MA1: resources/list returns ui://agentshangout/board.html
    let list_res = server.mcp(&scout.token, "resources/list", json!({})).await.ok();
    let resources = list_res["result"]["resources"].as_array().expect("resources array");
    // I96 adds the how-to-play guide beside the board, and JB9 the bot's role.
    assert_eq!(resources.len(), 3);
    let resources: Vec<_> =
        resources.iter().filter(|r| r["uri"] == "ui://agentshangout/board.html").collect();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["mimeType"], "text/html;profile=mcp-app");
    assert_eq!(resources[0]["name"], "Agents Hangout Room & Collaboration Board");
}

#[tokio::test]
async fn ma2_resources_read_and_security() {
    let server = Server::start().await;
    let (room, justin) = server.someone("Justin").await;
    let scout = server.bot(&justin, &room, "AntiGravity Miner").await;

    // MA2: read valid resource
    let read_res = server
        .mcp(&scout.token, "resources/read", json!({ "uri": "ui://agentshangout/board.html" }))
        .await
        .ok();

    let contents = read_res["result"]["contents"].as_array().expect("contents array");
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], "ui://agentshangout/board.html");
    assert_eq!(contents[0]["mimeType"], "text/html;profile=mcp-app");
    let html = contents[0]["text"].as_str().expect("html text");
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Agents Hangout"));

    // MA6: Check CSP and security invariants
    assert!(html.contains("default-src 'none'"));
    assert!(!html.contains("<script src="));
    assert!(!html.contains(&scout.token));
    assert!(!html.contains(&justin.key));

    // MA2: Unknown resource returns error -32602
    let unknown_res = server
        .mcp(
            &scout.token,
            "resources/read",
            json!({ "uri": "ui://agentshangout/nonexistent.html" }),
        )
        .await
        .refused(StatusCode::OK);

    assert!(unknown_res["error"]["message"].as_str().unwrap().contains("resource not found"));
}

#[tokio::test]
async fn ma3_ma4_ma5_dedicated_tool_and_text_fallback() {
    let server = Server::start().await;
    let (room, justin) = server.someone("Justin").await;
    let scout = server.bot(&justin, &room, "AntiGravity Miner").await;
    let (other_room, outsider) = server.someone("Outsider").await;
    let other_bot = server.bot(&outsider, &other_room, "Other Bot").await;

    // Set a whiteboard note
    server
        .put(
            &justin.key,
            &format!("/api/hangouts/{room}/whiteboard"),
            json!({ "text": "Building MCP Apps for rooms" }),
        )
        .await
        .ok();

    // Create an issues board and link it
    let board = server
        .post(
            &justin.key,
            &format!("/api/hangouts/{room}/boards"),
            json!({ "kind": "issues", "name": "Sprint Board" }),
        )
        .await
        .ok();
    let board_id = text(&board["id"]);
    server
        .put(
            &justin.key,
            &format!("/api/boards/{board_id}"),
            json!({ "github": "AgentsHangout/agentshangout" }),
        )
        .await
        .ok();

    // Create a card and set its github issue
    let card = server
        .post(
            &scout.token,
            &format!("/api/boards/{board_id}/cards"),
            json!({
                "title": "Support interactive MCP App",
                "text": "Embedded ui:// resource with text fallback and host bridge."
            }),
        )
        .await
        .ok();
    let card_id = text(&card["id"]);
    server
        .put(&scout.token, &format!("/api/cards/{card_id}"), json!({ "github_issue": 265 }))
        .await
        .ok();

    // MA3: Tool is in tools/list with _meta.ui.resourceUri
    let tools_res = server.mcp(&scout.token, "tools/list", json!({})).await.ok();
    let tools = tools_res["result"]["tools"].as_array().expect("tools");
    let open_tool = tools
        .iter()
        .find(|t| t["name"] == "open_collaboration_board")
        .expect("open_collaboration_board tool");

    assert_eq!(open_tool["_meta"]["ui"]["resourceUri"], "ui://agentshangout/board.html");

    // MA3 & MA4: Calling tool returns content, structuredContent, and _meta
    let call_res = server
        .mcp(
            &scout.token,
            "tools/call",
            json!({
                "name": "open_collaboration_board",
                "arguments": { "hangout_id": room }
            }),
        )
        .await
        .ok();

    let result = &call_res["result"];
    assert_eq!(result["_meta"]["ui"]["resourceUri"], "ui://agentshangout/board.html");

    // MA4: Non-UI text fallback
    let text_fb = result["content"][0]["text"].as_str().expect("text fallback");
    assert!(text_fb.contains("# Room: Justin's floor"));
    assert!(text_fb.contains("Whiteboard: Building MCP Apps for rooms"));
    assert!(text_fb.contains("### Sprint Board (issues, GitHub: AgentsHangout/agentshangout)"));
    assert!(text_fb.contains("- [#265] Support interactive MCP App [open]"));

    // structuredContent check
    let sc = &result["structuredContent"];
    assert_eq!(sc["hangout"]["name"], "Justin's floor");
    assert_eq!(sc["whiteboard"]["text"], "Building MCP Apps for rooms");
    let boards = sc["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0]["name"], "Sprint Board");
    assert_eq!(boards[0]["github"], "AgentsHangout/agentshangout");
    let cards = boards[0]["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["title"], "Support interactive MCP App");
    assert_eq!(cards[0]["github_issue"], 265);

    // MA5: Unauthorized access is refused (404 room not found)
    let unauth_res = server
        .mcp(
            &other_bot.token,
            "tools/call",
            json!({
                "name": "open_collaboration_board",
                "arguments": { "hangout_id": room }
            }),
        )
        .await
        .ok();

    assert_eq!(unauth_res["result"]["isError"], true);
    assert!(unauth_res["result"]["content"][0]["text"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn ma7_ma8_embedded_app_script_bridge() {
    let server = Server::start().await;
    let (room, justin) = server.someone("Justin").await;
    let scout = server.bot(&justin, &room, "AntiGravity Miner").await;

    let read_res = server
        .mcp(&scout.token, "resources/read", json!({ "uri": "ui://agentshangout/board.html" }))
        .await
        .ok();

    let html = read_res["result"]["contents"][0]["text"].as_str().unwrap();

    // MA7: bridge handles host lifecycle handshake & notifications
    assert!(html.contains("addEventListener('message'"));
    assert!(html.contains("event.source !== window.parent"));
    assert!(html.contains("ui/initialize"));
    assert!(html.contains("appCapabilities"));
    assert!(html.contains("appInfo"));
    assert!(html.contains("ui/notifications/initialized"));
    assert!(html.contains("ui/notifications/tool-result"));
    assert!(html.contains("open_collaboration_board"));

    // MA8: user-initiated handoff via ui/message without unprompted fallback or retry
    assert!(html.contains("ui/message"));
    assert!(!html.contains("ui/sendInput"));
    assert!(html.contains("Send to Chat"));

    // MA7 & MA8: Node runtime tests exercising handshake, correlation, no-retry, and security
    let output = std::process::Command::new("node")
        .arg("tests/mcp_apps_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn ma5_open_collaboration_board_does_not_clear_unread_mentions() {
    let server = Server::start().await;
    let (room, justin) = server.someone("Justin").await;
    let scout = server.bot(&justin, &room, "ScoutBot").await;
    let other = server.bot(&justin, &room, "OtherBot").await;

    // Bot mentions Justin in the room
    server
        .post(
            &scout.token,
            &format!("/api/hangouts/{room}/messages"),
            json!({ "text": "@Justin please review this board" }),
        )
        .await;

    // Verify Justin has unread person mention
    let mentions = server.state.db.person_mentions(&justin.id, 0).unwrap();
    assert_eq!(mentions.len(), 1, "mention should be present");

    // Calling open_collaboration_board via bot token does NOT clear person mentions
    let call_res = server
        .mcp(
            &other.token,
            "tools/call",
            json!({
                "name": "open_collaboration_board",
                "arguments": { "hangout_id": room }
            }),
        )
        .await
        .ok();

    assert_eq!(call_res["result"]["isError"], Value::Null);

    let mentions_after_bot = server.state.db.person_mentions(&justin.id, 0).unwrap();
    assert_eq!(
        mentions_after_bot.len(),
        1,
        "mention must NOT be cleared by bot open_collaboration_board"
    );

    // Calling open_collaboration_board as a human caller must also NOT clear person mentions (MA5)
    let human_caller =
        server.state.authenticate(&agenthangout::auth::hash_secret(&justin.key)).unwrap().unwrap();
    assert!(!human_caller.is_agent(), "must be human caller");
    let human_res = agenthangout::mcp::open_collaboration_board(
        &server.state,
        &human_caller,
        &json!({ "hangout_id": room }),
    );
    assert!(human_res.is_ok(), "human open_collaboration_board call succeeded");

    // Verify mention remains unread (non-mutating read promise preserved, unlike ops::snapshot)
    let mentions_after_human = server.state.db.person_mentions(&justin.id, 0).unwrap();
    assert_eq!(
        mentions_after_human.len(),
        1,
        "mention must NOT be cleared by human open_collaboration_board"
    );
}
