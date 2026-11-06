//! Tests for reports (docs/reports.md, RE1 to RE7). They drive a real server over HTTP. A comment
//! such as `// RE3` above a test names the rule it covers.

mod support;

use agenthangout::{app, auth, db::Db, now, worlds, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
    state: AppState,
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

struct Bot {
    id: String,
    token: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
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
        Self { base, client: Client::new(), state }
    }

    async fn send(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Reply {
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
        Reply { status, body: serde_json::from_str(&text).unwrap_or(Value::String(text)) }
    }

    async fn get(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::GET, path, None).await
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::POST, path, Some(body)).await
    }

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        let who = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
        (text(&made["hangout"]["id"]), who)
    }

    async fn join(&self, owner: &Who, room: &str, name: &str) -> Who {
        let snapshot = self.get(&owner.key, &format!("/api/hangouts/{room}")).await.ok();
        let code = text(&snapshot["hangout"]["invite_code"]);
        let path = format!("/api/invites/{code}/join");
        let made = self.post("", &path, json!({ "your_name": name })).await.ok();
        Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) }
    }

    fn bot(&self, owner: &Who, room: &str, name: &str) -> Bot {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        Bot { id: agent.id, token }
    }

    /// Someone with no room in common with anyone, made straight in the database.
    fn stranger(&self, n: usize) -> String {
        let id = format!("stranger{n:04}");
        let key = auth::hash_secret(&auth::new_owner_key());
        let name = format!("Stranger {n}");
        self.state.db.create_person(&id, &name, &key, &auth::new_feed_token(), now()).unwrap();
        id
    }

    async fn say(&self, who: &Who, room: &str, words: &str) -> String {
        let path = format!("/api/hangouts/{room}/messages");
        let line = self.post(&who.key, &path, json!({ "text": words })).await.ok();
        line["id"].as_i64().unwrap().to_string()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        });
        self.post(token, "/mcp", call).await.ok()["result"].clone()
    }

    /// The id of the newest line in a room with these words.
    fn line_with(&self, room: &str, words: &str) -> String {
        let lines = self.state.db.messages(room, 0, 100).unwrap();
        lines.iter().rev().find(|line| line.body.contains(words)).unwrap().id.to_string()
    }

    async fn report(&self, token: &str, body: Value) -> Reply {
        self.post(token, "/api/reports", body).await
    }

    fn row(&self, id: &Value) -> std::collections::BTreeMap<String, String> {
        self.state.db.report_row(id.as_i64().unwrap()).unwrap().unwrap()
    }
}

fn about(kind: &str, id: &str, reason: &str) -> Value {
    json!({ "target_kind": kind, "target_id": id, "reason": reason })
}

/// What a tool gave back, as JSON.
fn tool_body(result: &Value) -> Value {
    if !result["structuredContent"].is_null() {
        return result["structuredContent"].clone();
    }
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

// RE1, RE2, RE5: a message is reported, and the copy outlives the line.
#[tokio::test]
async fn a_reported_message_keeps_its_copy_after_the_line_is_gone() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    let line = server.say(&bo, &room, "something cruel").await;
    let mut body = about("message", &line, "harassment");
    body["details"] = json!("said to me twice");
    let reply = server.report(&ann.key, body).await.ok();
    // RE5: received, and nothing about Bo.
    let keys: Vec<&String> = reply.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["blocked_person", "report_id", "status"], "{reply}");
    assert_eq!(reply["status"], "received");
    assert_eq!(reply["blocked_person"], false);
    let row = server.row(&reply["report_id"]);
    assert_eq!(row["reporter_id"], ann.id);
    assert_eq!(row["target_person_id"], bo.id);
    assert_eq!(row["hangout_id"], room);
    assert_eq!(row["by_bot"], "0");
    assert_eq!(row["status"], "open");
    assert_eq!(row["details"], "said to me twice");
    assert!(!row.contains_key("closed_at"));
    // The line goes with Bo's account; the copy stays.
    server.state.db.delete_person(&bo.id, "Bo").unwrap();
    assert!(server.state.db.message(line.parse().unwrap()).unwrap().is_none());
    let row = server.row(&reply["report_id"]);
    let copy: Value = serde_json::from_str(&row["snapshot"]).unwrap();
    assert_eq!(copy["text"], "something cruel");
    assert_eq!(copy["speaker"], "Bo");
    // RE6: Bo's id came off a report that isn't about a minor's safety.
    assert!(!row.contains_key("target_person_id"), "{row:?}");
}

// RE1: a visitor sees only what came after they arrived, and anything unseen is the same 404.
#[tokio::test]
async fn what_the_caller_cant_see_is_not_found() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let before = server.say(&ann, &room, "before anyone walked in").await;
    let floor = json!({ "access": "public", "create_code": CODE });
    let path = format!("/api/floors/{room}");
    server.send(&ann.key, Method::PUT, &path, Some(floor)).await.ok();
    let (_, eve) = server.someone("Eve").await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    server.post(&eve.key, &format!("/api/floors/{room}/walk-in"), json!({})).await.ok();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let after = server.say(&ann, &room, "after Eve walked in").await;

    let refused = server.report(&eve.key, about("message", &before, "spam")).await;
    let not_found = refused.refused(StatusCode::NOT_FOUND);
    server.report(&eve.key, about("message", &after, "spam")).await.ok();
    // Someone who isn't in the room, an id that doesn't exist, and nonsense all read the same.
    let (_, zed) = server.someone("Zed").await;
    let outside = server.report(&zed.key, about("message", &after, "spam")).await;
    assert_eq!(outside.refused(StatusCode::NOT_FOUND), not_found);
    let missing = server.report(&ann.key, about("message", "999999", "spam")).await;
    assert_eq!(missing.refused(StatusCode::NOT_FOUND), not_found);
    for kind in ["message", "person", "bot", "place", "card", "note", "pin"] {
        let reply = server.report(&ann.key, about(kind, "no-such-thing", "spam")).await;
        assert_eq!(reply.refused(StatusCode::NOT_FOUND), not_found, "{kind}");
    }
    // A bad kind or reason is the caller's mistake, not a hint.
    let bad_kind = server.report(&ann.key, about("hangout", &room, "spam")).await;
    bad_kind.refused(StatusCode::BAD_REQUEST);
    let bad_reason = server.report(&ann.key, about("person", &eve.id, "rude")).await;
    bad_reason.refused(StatusCode::BAD_REQUEST);
    let mut long = about("person", &eve.id, "other");
    long["details"] = json!("x".repeat(1001));
    server.report(&ann.key, long).await.refused(StatusCode::BAD_REQUEST);
    // A place can be reported by its room's id.
    server.report(&eve.key, about("place", &room, "hate")).await.ok();
}

// RE1: a line the caller's own block hides isn't one they can report.
#[tokio::test]
async fn a_line_hidden_by_a_block_cant_be_reported() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    let line = server.say(&bo, &room, "hidden soon").await;
    server.post(&ann.key, "/api/blocks", json!({ "person_id": bo.id })).await.ok();
    server.report(&ann.key, about("message", &line, "spam")).await.refused(StatusCode::NOT_FOUND);
}

// RE1: never yourself, your bot, or what you or your bot wrote.
#[tokio::test]
async fn nobody_reports_themselves_or_their_own_bot() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let scout = server.bot(&ann, &room, "Scout");
    let mine = server.say(&ann, &room, "my own line").await;
    let said = json!({ "hangout_id": room, "text": "the bot line" });
    server.tool(&scout.token, "say", said).await;
    let bots = server.line_with(&room, "the bot line");
    let targets = [("person", &ann.id), ("bot", &scout.id), ("message", &mine), ("message", &bots)];
    for (kind, id) in targets {
        server.report(&ann.key, about(kind, id, "spam")).await.refused(StatusCode::BAD_REQUEST);
    }
    let own = server.tool(&scout.token, "report", about("person", &ann.id, "spam")).await;
    assert_eq!(own["isError"], true, "{own}");
}

// RE3: reporting again while it's open updates the one report and doesn't count twice.
#[tokio::test]
async fn reporting_again_updates_the_open_report() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    let target = server.stranger(1);
    let first = server.report(&ann.key, about("person", &target, "spam")).await.ok();
    let mut again = about("person", &target, "impersonation");
    again["details"] = json!("uses my name");
    let second = server.report(&ann.key, again).await.ok();
    assert_eq!(first["report_id"], second["report_id"]);
    let row = server.row(&first["report_id"]);
    assert_eq!(row["reason"], "impersonation");
    assert_eq!(row["details"], "uses my name");
    let mine = server.get(&ann.key, "/api/reports/mine").await.ok();
    assert_eq!(mine["reports"].as_array().unwrap().len(), 1);
    // Not counted again: 19 more new ones still fit.
    for n in 2..21 {
        let other = server.stranger(n);
        server.report(&ann.key, about("person", &other, "spam")).await.ok();
    }
    // And the same open one can still be brought up to date at the cap.
    server.report(&ann.key, about("person", &target, "hate")).await.ok();
}

// RE4: 20 a day for a person; the 21st is refused with how long to wait.
#[tokio::test]
async fn the_twenty_first_report_of_the_day_is_refused() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    for n in 0..20 {
        let target = server.stranger(n);
        server.report(&ann.key, about("person", &target, "spam")).await.ok();
    }
    let last = server.stranger(20);
    let refused = server.report(&ann.key, about("person", &last, "spam")).await;
    let body = refused.refused(StatusCode::TOO_MANY_REQUESTS);
    let message = text(&body["error"]);
    assert!(message.contains("RE4") && message.contains("UTC"), "{message}");
}

// RE1, RE4: a bot files for its person, marked by_bot, and its bots share 5 of the 20.
#[tokio::test]
async fn bots_file_by_bot_and_at_most_five_a_day() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let scout = server.bot(&ann, &room, "Scout");
    let pip = server.bot(&ann, &room, "Pip");
    let bo = server.join(&ann, &room, "Bo").await;
    let line = server.say(&bo, &room, "spam spam spam").await;
    let filed = server.tool(&scout.token, "report", about("message", &line, "spam")).await;
    assert_ne!(filed["isError"], true, "{filed}");
    let filed = tool_body(&filed);
    assert_eq!(filed["status"], "received");
    let row = server.row(&filed["report_id"]);
    assert_eq!(row["by_bot"], "1");
    assert_eq!(row["via_bot_id"], scout.id);
    assert_eq!(row["reporter_id"], ann.id);
    // Both bots together: four more, then the sixth is refused.
    for n in 0..4 {
        let target = server.stranger(n);
        let token = if n % 2 == 0 { &pip.token } else { &scout.token };
        let reply = server.tool(token, "report", about("person", &target, "spam")).await;
        assert_ne!(reply["isError"], true, "{reply}");
    }
    let sixth = server.stranger(10);
    let refused = server.tool(&pip.token, "report", about("person", &sixth, "spam")).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused.to_string().contains("RE4"), "{refused}");
    // Their person still can.
    server.report(&ann.key, about("person", &sixth, "spam")).await.ok();
    // A bot can't use the person's route, which is where blocking is.
    let as_bot = server.report(&scout.token, about("person", &bo.id, "spam")).await;
    as_bot.refused(StatusCode::FORBIDDEN);
}

// RE5: the reporter's list says received, then reviewed, and never the action.
#[tokio::test]
async fn my_reports_say_received_then_reviewed_and_nothing_else() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    let target = server.stranger(1);
    let reply = server.report(&ann.key, about("person", &target, "hate")).await.ok();
    let mine = server.get(&ann.key, "/api/reports/mine").await.ok();
    assert_eq!(mine["reports"][0]["status"], "received");
    let id = reply["report_id"].as_i64().unwrap();
    server.state.db.set_report_status(id, "actioned", now()).unwrap();
    let mine = server.get(&ann.key, "/api/reports/mine").await.ok();
    let report = &mine["reports"][0];
    assert_eq!(report["status"], "reviewed");
    let keys: Vec<&String> = report.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["created_at", "id", "reason", "status", "target_kind"], "{report}");
    assert!(!mine.to_string().contains("actioned"), "{mine}");
    assert!(!mine.to_string().contains(&target), "{mine}");
}

// RE7: report and block is the same block as POST /api/blocks, on the person behind the target.
#[tokio::test]
async fn report_and_block_blocks_the_person_behind_it() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    let cy = server.join(&ann, &room, "Cy").await;
    let dee = server.join(&ann, &room, "Dee").await;
    let pip = server.bot(&cy, &room, "Pip");
    let rex = server.bot(&dee, &room, "Rex");

    // A person's line blocks that person.
    let line = server.say(&bo, &room, "go away").await;
    let mut body = about("message", &line, "harassment");
    body["block"] = json!(true);
    assert_eq!(server.report(&ann.key, body).await.ok()["blocked_person"], true);
    // A bot's line blocks the bot's person.
    server.tool(&pip.token, "say", json!({ "hangout_id": room, "text": "buy now" })).await;
    let mut body = about("message", &server.line_with(&room, "buy now"), "spam");
    body["block"] = json!(true);
    assert_eq!(server.report(&ann.key, body).await.ok()["blocked_person"], true);
    // A bot blocks its person.
    let mut body = about("bot", &rex.id, "impersonation");
    body["block"] = json!(true);
    assert_eq!(server.report(&ann.key, body).await.ok()["blocked_person"], true);

    let listed = server.get(&ann.key, "/api/blocks").await.ok();
    let mut blocked: Vec<String> =
        listed["blocks"].as_array().unwrap().iter().map(|b| text(&b["person_id"])).collect();
    blocked.sort();
    let mut expected = vec![bo.id.clone(), cy.id.clone(), dee.id.clone()];
    expected.sort();
    assert_eq!(blocked, expected, "{listed}");
    // Without block, nobody is blocked.
    let eve = server.stranger(1);
    let plain = server.report(&ann.key, about("person", &eve, "spam")).await.ok();
    assert_eq!(plain["blocked_person"], false);
    assert_eq!(
        server.get(&ann.key, "/api/blocks").await.ok()["blocks"].as_array().unwrap().len(),
        3
    );
}

// RE1: a keyless agent has no person to report for.
#[tokio::test]
async fn a_keyless_agent_cant_report() {
    let server = Server::start().await;
    worlds::ensure_commons(&server.state).unwrap();
    let url = format!("{}/bridge/new/test-nonce-00000001-abcdef/look", server.base);
    let page = server.client.get(url).send().await.unwrap().text().await.unwrap();
    let start = page.find("<h2>Your key</h2>\n<p>").expect("no key on the page") + 21;
    let end = page[start..].find("</p>").unwrap() + start;
    let key = &page[start..end];
    let target = server.stranger(1);
    server.report(key, about("person", &target, "spam")).await.refused(StatusCode::FORBIDDEN);
    // And nobody at all is asked to sign in.
    server.report("", about("person", &target, "spam")).await.refused(StatusCode::UNAUTHORIZED);
}

// RE6: deleting an account blanks its id on reports, except minor_safety ones, which keep it.
#[tokio::test]
async fn a_deleted_account_comes_off_reports_except_minor_safety_ones() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    let (_, bo) = server.someone("Bo").await;
    let (_, cy) = server.someone("Cy").await;
    let id = |reply: Reply| reply.ok()["report_id"].clone();
    // Ann files one of each; Cy reports Ann both ways.
    let plain = id(server.report(&ann.key, about("person", &bo.id, "spam")).await);
    let other = server.stranger(1);
    let safety = id(server.report(&ann.key, about("person", &other, "minor_safety")).await);
    let about_ann = id(server.report(&cy.key, about("person", &ann.id, "hate")).await);
    let safety_about_ann =
        id(server.report(&cy.key, about("person", &ann.id, "minor_safety")).await);
    // That second one is the same open report brought up to date (RE3), so file it again later.
    assert_eq!(about_ann, safety_about_ann);
    let (room, dee) = server.someone("Dee").await;
    let ann_too = server.join(&dee, &room, "Ann2").await;
    let line = server.say(&ann_too, &room, "a line").await;
    let safety_line = id(server.report(&dee.key, about("message", &line, "minor_safety")).await);
    let plain_line = id(server.report(&dee.key, about("person", &ann_too.id, "spam")).await);

    server.state.db.delete_person(&ann.id, "Ann").unwrap();
    server.state.db.delete_person(&ann_too.id, "Ann2").unwrap();
    // As reporter.
    assert!(!server.row(&plain).contains_key("reporter_id"));
    assert_eq!(server.row(&safety)["reporter_id"], ann.id);
    // As target: Cy's report became minor_safety, so it keeps Ann.
    assert_eq!(server.row(&about_ann)["target_person_id"], ann.id);
    assert_eq!(server.row(&about_ann)["reporter_id"], cy.id);
    assert_eq!(server.row(&safety_line)["target_person_id"], ann_too.id);
    assert!(!server.row(&plain_line).contains_key("target_person_id"));
}
