//! Tests for Needs you: everything waiting on a person, from every room, in one list. They drive a
//! real server over HTTP and are written from the rules in docs/needs-you.md. A comment such as
//! `// N6` above a test names the rule it covers.

use std::sync::{Arc, Mutex};

use agenthangout::{app, db::Db, needs_you, worlds, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";
const WAITING: &str = "Claude needs your permission to use Bash";

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
        assert_eq!(self.status, StatusCode::OK, "{} {}", self.status, self.body);
        self.body
    }

    #[track_caller]
    fn status(self, status: StatusCode) -> Value {
        assert_eq!(self.status, status, "{}", self.body);
        self.body
    }
}

struct Who {
    key: String,
    id: String,
}

/// Ann's room with her bot Fern in it, and Bo, who has a room of his own and has joined Ann's.
struct Town {
    ann: Who,
    room: String,
    fern: String,
    fern_id: String,
    bo: Who,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
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
        worlds::ensure_commons(&state).unwrap();
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

    async fn put(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::PUT, path, Some(body)).await
    }

    async fn delete(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::DELETE, path, None).await
    }

    /// Someone new with a room of their own, and the code of that room's invite link.
    async fn someone(&self, name: &str) -> (String, String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (
            text(&made["hangout"]["id"]),
            text(&made["invite_url"]).rsplit('/').next().unwrap().to_string(),
            Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) },
        )
    }

    async fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let made = self
            .post(&owner.key, &format!("/api/hangouts/{room}/agents"), json!({ "name": name }))
            .await
            .ok();
        (text(&made["agent"]["id"]), text(&made["token"]))
    }

    async fn join(&self, who: &Who, code: &str) {
        self.post(&who.key, &format!("/api/invites/{code}/join"), json!({})).await.ok();
    }

    /// Ann and her bot Fern alone in Ann's room, and Bo in a room of his own.
    async fn alone(&self) -> (Town, String) {
        let (room, code, ann) = self.someone("Ann").await;
        let (fern_id, fern) = self.bot(&ann, &room, "Fern").await;
        let (_, _, bo) = self.someone("Bo").await;
        (Town { ann, room, fern, fern_id, bo }, code)
    }

    /// The same, with Bo joined to Ann's room.
    async fn town(&self) -> Town {
        let (town, code) = self.alone().await;
        self.join(&town.bo, &code).await;
        town
    }

    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        self.post(token, "/mcp", call).await.ok()["result"].clone()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result = self.call(token, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn refused(&self, token: &str, name: &str, arguments: Value) -> String {
        let result = self.call(token, name, arguments).await;
        assert_eq!(result["isError"], true, "{name} should have been refused: {result}");
        text(&result["content"][0]["text"])
    }

    async fn needs(&self, who: &Who) -> Value {
        self.get(&who.key, "/api/needs-you").await.ok()
    }

    async fn wait(&self, token: &str, about: &str) {
        self.tool(token, "waiting_on_owner", json!({ "about": about })).await;
    }
}

fn count(list: &Value, field: &str) -> usize {
    list[field].as_array().unwrap_or_else(|| panic!("no {field} in {list}")).len()
}

async fn start_receiver() -> (String, Arc<Mutex<Vec<Value>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let store = received.clone();
    let app = axum::Router::new().route(
        "/hook",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let store = store.clone();
            async move { store.lock().unwrap().push(body) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, received)
}

/// The pings that have arrived so far, waiting a moment for any still on their way.
async fn arrived(received: &Arc<Mutex<Vec<Value>>>) -> Vec<Value> {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    received.lock().unwrap().clone()
}

// N1
#[tokio::test]
async fn needs_you_is_for_people() {
    let server = Server::start().await;
    let town = server.town().await;
    let empty = server.needs(&town.ann).await;
    for field in ["questions", "check_ins", "waiting", "requests", "invites", "apprentices"] {
        assert_eq!(count(&empty, field), 0, "{field}: {empty}");
    }
    assert_eq!(empty["count"], 0);
    server.get(&town.fern, "/api/needs-you").await.status(StatusCode::FORBIDDEN);
    server.get("", "/api/needs-you").await.status(StatusCode::UNAUTHORIZED);
}

// N2, N3: a question shows in its owner's list, from any room, and nobody else's, until answered.
#[tokio::test]
async fn a_question_from_your_bot_is_in_your_list_and_nobody_elses() {
    let server = Server::start().await;
    let town = server.town().await;
    let asked = server
        .tool(
            &town.fern,
            "ask_owner",
            json!({ "hangout_id": town.room, "question": "Are you free Friday?" }),
        )
        .await;

    let ann = server.needs(&town.ann).await;
    assert_eq!(count(&ann, "questions"), 1, "{ann}");
    let question = &ann["questions"][0];
    assert_eq!(question["id"], asked["id"]);
    assert_eq!(question["bot"], "Fern");
    assert_eq!(question["question"], "Are you free Friday?");
    assert_eq!(question["room"]["id"], json!(town.room));
    assert_eq!(question["room"]["name"], "Ann's room");
    assert_eq!(ann["count"], 1);
    // R45: Bo is in the room, and it's still not his.
    let bo = server.needs(&town.bo).await;
    assert_eq!(count(&bo, "questions"), 0, "{bo}");
    assert!(!bo.to_string().contains("Friday"), "{bo}");

    let path = format!("/api/owner-questions/{}/answer", text(&asked["id"]));
    server.post(&town.ann.key, &path, json!({ "answer": "Yes" })).await.ok();
    assert_eq!(count(&server.needs(&town.ann).await, "questions"), 0);
}

// N2, N3: a check-in is in the list of each person who may answer it, and nobody on its own side.
#[tokio::test]
async fn a_check_in_is_listed_for_whoever_may_answer_it() {
    let server = Server::start().await;
    let town = server.town().await;
    let asked = server
        .tool(&town.fern, "check_in", json!({ "hangout_id": town.room, "about": "Book a table" }))
        .await;

    let bo = server.needs(&town.bo).await;
    assert_eq!(count(&bo, "check_ins"), 1, "{bo}");
    let check_in = &bo["check_ins"][0];
    assert_eq!(check_in["id"], asked["id"]);
    assert_eq!(check_in["asker"], "Fern");
    assert_eq!(check_in["about"], "Book a table");
    assert_eq!(check_in["room"]["name"], "Ann's room");
    // Her own side can't approve it, so it isn't asking Ann.
    assert_eq!(count(&server.needs(&town.ann).await, "check_ins"), 0);

    // Answered, it's gone, and answering again is refused with a reason (N3).
    let path = format!("/api/check-ins/{}/answer", text(&asked["id"]));
    server.post(&town.bo.key, &path, json!({ "response": "go" })).await.ok();
    assert_eq!(count(&server.needs(&town.bo).await, "check_ins"), 0);
    let again = server.post(&town.bo.key, &path, json!({ "response": "go" })).await;
    assert!(again.status(StatusCode::CONFLICT)["error"].as_str().unwrap().contains("answered"));
}

// N2: a check-in asked of someone is only in that person's list.
#[tokio::test]
async fn a_check_in_asked_of_someone_else_isnt_in_your_list() {
    let server = Server::start().await;
    let town = server.town().await;
    let (_, _, cy) = server.someone("Cy").await;
    let code = server.get(&town.ann.key, &format!("/api/hangouts/{}", town.room)).await.ok()
        ["hangout"]["invite_code"]
        .clone();
    server.join(&cy, &text(&code)).await;
    server
        .tool(
            &town.fern,
            "check_in",
            json!({ "hangout_id": town.room, "about": "Book a table", "to": "Cy" }),
        )
        .await;
    assert_eq!(count(&server.needs(&cy).await, "check_ins"), 1);
    assert_eq!(count(&server.needs(&town.bo).await, "check_ins"), 0);
}

// N5: only its owner sees a bot waiting, and in My bots too.
#[tokio::test]
async fn a_bot_waiting_somewhere_else_shows_only_to_its_owner() {
    let server = Server::start().await;
    let town = server.town().await;
    server.wait(&town.fern, WAITING).await;

    let ann = server.needs(&town.ann).await;
    assert_eq!(count(&ann, "waiting"), 1, "{ann}");
    assert_eq!(ann["waiting"][0]["bot"]["id"], json!(town.fern_id));
    assert_eq!(ann["waiting"][0]["bot"]["name"], "Fern");
    assert_eq!(ann["waiting"][0]["about"], WAITING);
    assert_eq!(ann["count"], 1);
    let bots = server.get(&town.ann.key, "/api/me/bots").await.ok();
    assert_eq!(bots["bots"][0]["waiting"]["about"], WAITING, "{bots}");

    // Nobody else hears of it: not Bo, not the room, not a bot.
    assert_eq!(count(&server.needs(&town.bo).await, "waiting"), 0);
    let room = server.get(&town.bo.key, &format!("/api/hangouts/{}", town.room)).await.ok();
    assert!(!room.to_string().contains(WAITING), "{room}");
    let inbox = server.tool(&town.fern, "inbox", json!({})).await;
    assert!(!inbox.to_string().contains(WAITING), "{inbox}");

    // A newer one replaces it, and it's at most 200 characters.
    server.wait(&town.fern, "Waiting for you to pick a branch").await;
    let ann = server.needs(&town.ann).await;
    assert_eq!(count(&ann, "waiting"), 1);
    assert_eq!(ann["waiting"][0]["about"], "Waiting for you to pick a branch");
    let long = "x".repeat(needs_you::MAX_ABOUT_CHARS + 1);
    server.refused(&town.fern, "waiting_on_owner", json!({ "about": long })).await;
    // A person has nothing to wait on.
    let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "waiting_on_owner", "arguments": { "about": WAITING } } });
    assert_ne!(server.post(&town.ann.key, "/mcp", call).await.status, StatusCode::OK);
}

// N5: a house bot can't say it's waiting (R47).
#[tokio::test]
async fn a_house_bot_cant_say_its_waiting() {
    let server = Server::start().await;
    let made = server
        .post("", "/api/cities", json!({ "name": "Elm", "your_name": "Ann", "create_code": CODE }))
        .await
        .ok();
    let ann = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
    let city = text(&made["city"]["id"]);
    let built = server
        .post(&ann.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Ann House" }))
        .await
        .ok();
    let house = text(&built["house"]["id"]);
    let butler = server
        .post(&ann.key, &format!("/api/houses/{house}/house-bot"), json!({ "name": "Butler" }))
        .await
        .ok();
    server.refused(&text(&butler["token"]), "waiting_on_owner", json!({ "about": WAITING })).await;
    assert_eq!(count(&server.needs(&ann).await, "waiting"), 0);
}

// N6: reading doesn't clear it, writing does, and so do clearing it, dismissing it and a day.
#[tokio::test]
async fn a_wait_clears_when_the_bot_does_something_and_not_when_it_only_reads() {
    let server = Server::start().await;
    let town = server.town().await;
    let waiting = |list: Value| count(&list, "waiting");

    server.wait(&town.fern, WAITING).await;
    // A watcher keeps reading while the agent is stuck.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    server.tool(&town.fern, "inbox", json!({})).await;
    server.tool(&town.fern, "read_room", json!({ "hangout_id": town.room })).await;
    server.tool(&town.fern, "events_since", json!({})).await;
    assert_eq!(waiting(server.needs(&town.ann).await), 1, "reading cleared it");
    // Saying something is the bot doing something.
    server.tool(&town.fern, "say", json!({ "hangout_id": town.room, "text": "back" })).await;
    assert_eq!(waiting(server.needs(&town.ann).await), 0, "a write didn't clear it");

    // With no about, the bot clears it itself.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    server.wait(&town.fern, WAITING).await;
    assert_eq!(waiting(server.needs(&town.ann).await), 1);
    let cleared = server.tool(&town.fern, "waiting_on_owner", json!({})).await;
    assert_eq!(cleared["waiting"], false);
    assert_eq!(waiting(server.needs(&town.ann).await), 0);

    // Its owner dismisses it, and nobody else can.
    server.wait(&town.fern, WAITING).await;
    let path = format!("/api/needs-you/waiting/{}", town.fern_id);
    server.delete(&town.bo.key, &path).await.status(StatusCode::NOT_FOUND);
    server.delete(&town.fern, &path).await.status(StatusCode::FORBIDDEN);
    assert_eq!(waiting(server.needs(&town.ann).await), 1);
    server.delete(&town.ann.key, &path).await.ok();
    assert_eq!(waiting(server.needs(&town.ann).await), 0);

    // After a day it stops showing by itself.
    server.wait(&town.fern, WAITING).await;
    server.state.db.age_owner_wait(&town.fern_id, needs_you::SHOWN_SECS - 60).unwrap();
    assert_eq!(waiting(server.needs(&town.ann).await), 1);
    server.state.db.age_owner_wait(&town.fern_id, 120).unwrap();
    assert_eq!(waiting(server.needs(&town.ann).await), 0, "a day old and still showing");
}

// N7: one ping when a wait starts, none while it's showing, and it isn't a write.
#[tokio::test]
async fn its_owner_is_pinged_once_when_a_wait_starts() {
    let server = Server::start().await;
    let town = server.town().await;
    let (url, received) = start_receiver().await;
    server.put(&town.ann.key, "/api/notify", json!({ "url": url })).await.ok();
    let (bo_url, bo_received) = start_receiver().await;
    server.put(&town.bo.key, "/api/notify", json!({ "url": bo_url })).await.ok();

    for _ in 0..3 {
        server.wait(&town.fern, WAITING).await;
    }
    let pings: Vec<Value> = arrived(&received)
        .await
        .into_iter()
        .filter(|ping| ping["event"] == "owner.waiting")
        .collect();
    assert_eq!(pings.len(), 1, "{pings:?}");
    assert!(text(&pings[0]["text"]).contains(WAITING), "{}", pings[0]);
    assert!(arrived(&bo_received).await.iter().all(|ping| ping["event"] != "owner.waiting"));

    // It never counts as a write, so a hook calling it often isn't held to the write cap, and it
    // doesn't clear itself.
    for _ in 0..40 {
        server.wait(&town.fern, WAITING).await;
    }
    assert_eq!(count(&server.needs(&town.ann).await, "waiting"), 1);
}

// N8: a check-in nobody could answer is refused, with the way to ask your owner instead.
#[tokio::test]
async fn a_check_in_nobody_here_could_answer_is_refused() {
    let server = Server::start().await;
    let (town, code) = server.alone().await;
    let refused = server
        .refused(
            &town.fern,
            "check_in",
            json!({ "hangout_id": town.room, "about": "Book a table" }),
        )
        .await;
    assert!(refused.contains("ask_owner"), "{refused}");
    // Over HTTP too, and from a person on their own.
    let path = format!("/api/hangouts/{}/check-ins", town.room);
    server
        .post(&town.ann.key, &path, json!({ "about": "Book a table" }))
        .await
        .status(StatusCode::CONFLICT);
    // Once someone from another side is here, it's fine.
    server.join(&town.bo, &code).await;
    server
        .tool(&town.fern, "check_in", json!({ "hangout_id": town.room, "about": "Book a table" }))
        .await;
}

// N9: the room page says who a waiting check-in is waiting for, and why a Go or Hold was refused.
#[test]
fn the_room_page_says_why_a_check_in_cant_be_answered() {
    let app = include_str!("../web/app.js");
    assert!(app.contains("Waiting for someone on another side to answer"));
    let card = app.split_once("function checkInCard(checkIn) {").unwrap().1;
    let card = card.split_once("\n  }").unwrap().0;
    assert!(card.contains("const error = formError();"), "a refused answer says nothing");
    assert!(card.contains("act(error, "), "a refused answer says nothing");
}

// N10: an invite from a friend is in the list, and requests and apprentices come from the same
// place as their own panels.
#[tokio::test]
async fn invites_requests_and_apprentices_are_listed_too() {
    let server = Server::start().await;
    let (room, _, ann) = server.someone("Ann").await;
    let (_, other, _) = server.someone("Dee").await;
    let (_, _, cy) = server.someone("Cy").await;
    // Sharing Dee's room makes Ann and Cy friends (FR1).
    server.join(&cy, &other).await;
    server.join(&ann, &other).await;
    server
        .post(
            &ann.key,
            &format!("/api/hangouts/{room}/invite-friend"),
            json!({ "person_id": cy.id }),
        )
        .await
        .ok();
    let list = server.needs(&cy).await;
    assert_eq!(count(&list, "invites"), 1, "{list}");
    assert_eq!(list["invites"][0]["from"], "Ann");
    assert_eq!(list["count"], 1);
    let me = server.get(&cy.key, "/api/me").await.ok();
    assert_eq!(list["invites"], me["invited"], "not the same as /api/me");

    let code = include_str!("../src/needs_you.rs");
    assert!(code.contains("crate::spaces::requests_for(&state, &caller.person_id)?"));
    assert!(code.contains("crate::friends::invited(&state, &caller.person_id)?"));
    assert!(code.contains("crate::bonds::asking(&state, &caller.person_id)?"));
    let spaces = include_str!("../src/spaces.rs");
    assert!(spaces.contains("json!({ \"requests\": requests_for(&state, &caller.person_id)? })"));
}

// N4: the button is on the map and in every room, at every width, just before Account.
#[test]
fn needs_you_is_in_both_bars_just_before_account() {
    let app = include_str!("../web/app.js");
    for view in ["worldView", "roomView"] {
        let view = app.split_once(&format!("async function {view}(")).unwrap().1;
        let bar = view.split_once("frame.actions.append(").unwrap().1.split_once("));").unwrap().0;
        let needs = bar.find("needs.toggle").unwrap_or_else(|| panic!("no Needs you in {bar}"));
        let account = bar.find("account.toggle").unwrap();
        assert!(needs < account, "Needs you isn't before Account in {bar}");
        let between = &bar[needs + "needs.toggle".len()..account];
        assert!(!between.contains(".toggle"), "something is between them in {bar}");
        // A room's own buttons for these have gone, and its check-ins are a row in More.
        assert!(!bar.contains("questionsToggle"), "Questions is still in {bar}");
        assert!(!bar.contains("checkInsToggle"), "Check-ins is still in {bar}");
    }
    let needs = app.split_once("function needsYouPanel(").unwrap().1;
    let needs = needs.split_once("\n}\n").unwrap().0;
    assert!(needs.contains("panels.toggle('needs', 'Needs you'"), "it isn't called Needs you");
    assert!(!needs.contains("className: 'sec'") && !needs.contains("className: 'mid'"));
    assert!(needs.contains("Nothing needs you right now."));
    assert!(needs.contains("api('/api/needs-you')"));
    assert!(needs.contains("30000"), "it doesn't ask again every 30 seconds");
    assert!(app.contains("moreButton('checkins', 'Check-ins', ''),"), "no Check-ins row in More");
}
