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

// N9: a check-in its own side is waiting on says who it's waiting for, and a refused Go or Hold
// says why.
#[test]
fn a_check_in_says_who_its_waiting_for_and_why_an_answer_was_refused() {
    let app = include_str!("../web/app.js").replace("\r\n", "\n");
    let answered = app.split_once("  function answeredCheckIn(checkIn) {").unwrap().1;
    let answered = answered.split_once("\n  }").unwrap().0;
    assert!(answered.contains("'Waiting for someone on another side to answer'"));
    let card = app.split_once("  function checkInItem(checkIn) {").unwrap().1;
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

// N14: every way a bot is told how to behave says to ask through the site, even when it also asks
// in its own chat, and to take the question back if it's answered there.
#[tokio::test]
async fn bots_are_told_to_ask_through_the_site_and_never_only_in_their_own_chat() {
    let server = Server::start().await;
    let (town, _) = server.alone().await;
    let hello = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
    let init = server.post(&town.fern, "/mcp", hello).await.ok();
    let instructions = text(&init["result"]["instructions"]);
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    let tools = server.post(&town.fern, "/mcp", list).await.ok();
    let ask = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "ask_owner");
    let ask = text(&ask.unwrap()["description"]);
    for words in [&instructions, &ask] {
        assert!(words.contains("even one you also ask in your own chat window"), "{words}");
        assert!(words.contains("Never ask only in your own chat."), "{words}");
        assert!(
            words.contains("If they answer you there first, call withdraw_question."),
            "{words}"
        );
    }
    let app = include_str!("../web/app.js");
    assert!(app.contains("Ask me every question with ask_owner, so it reaches my Needs you, even if you also ask me here."));
    let guide = include_str!("../web/agents.md").replace("\r\n", "\n");
    assert!(guide.contains("**Always ask through the site.**"));
    assert!(guide.contains("**Your own chat as well, never instead.**"));
    assert!(guide.contains("call\n  `withdraw_question` so it leaves Needs you."));
}

// N15: an owner answers a wait in words, and the bot gets it in its inbox like any answer.
#[tokio::test]
async fn an_owner_can_answer_a_wait_and_the_bot_hears_it() {
    let server = Server::start().await;
    let town = server.town().await;
    let push = "Claude needs your OK to push the overview PR";
    server.wait(&town.fern, push).await;
    let path = format!("/api/needs-you/waiting/{}/answer", town.fern_id);

    // Nobody else can, and not a bot, and not with nothing to say.
    server
        .post(&town.bo.key, &path, json!({ "answer": "Yes" }))
        .await
        .status(StatusCode::NOT_FOUND);
    let fern = server.post(&town.fern, &path, json!({ "answer": "Yes" })).await;
    assert_eq!(fern.status, StatusCode::FORBIDDEN, "{}", fern.body);
    server
        .post(&town.ann.key, &path, json!({ "answer": " " }))
        .await
        .status(StatusCode::BAD_REQUEST);
    assert_eq!(count(&server.needs(&town.ann).await, "waiting"), 1, "a refusal leaves it waiting");

    let answered =
        server.post(&town.ann.key, &path, json!({ "answer": "Yes, push it" })).await.ok();
    assert_eq!(answered["question"], push, "{answered}");
    assert_eq!(answered["answer"], "Yes, push it");

    // The wait has gone, and the bot has the answer with its wait as the question.
    let ann = server.needs(&town.ann).await;
    assert_eq!(count(&ann, "waiting"), 0, "{ann}");
    assert_eq!(count(&ann, "questions"), 0, "{ann}");
    let inbox = server.tool(&town.fern, "inbox", json!({})).await;
    let answers = inbox["owner_answers"].as_array().unwrap();
    assert!(
        answers.iter().any(|a| a["question"] == push && a["answer"] == "Yes, push it"),
        "{inbox}"
    );
    // And it's in what was answered today.
    let today = server.get(&town.ann.key, "/api/needs-you/answered").await.ok();
    assert!(today.to_string().contains("Yes, push it"), "{today}");

    // Once it's gone there's nothing to answer.
    server
        .post(&town.ann.key, &path, json!({ "answer": "Again" }))
        .await
        .status(StatusCode::NOT_FOUND);
}

// N15: the tool says it isn't for questions, and the card says it can be answered here.
#[tokio::test]
async fn a_wait_is_not_for_questions_and_its_card_can_be_answered() {
    let server = Server::start().await;
    let (town, _) = server.alone().await;
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    let tools = server.post(&town.fern, "/mcp", list).await.ok();
    let wait = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "waiting_on_owner");
    let wait = text(&wait.unwrap()["description"]);
    assert!(
        wait.contains("Not for questions: anything your owner could answer in words"),
        "{wait}"
    );
    assert!(wait.contains("goes to ask_owner"), "{wait}");
    let app = include_str!("../web/app.js");
    assert!(app.contains("/api/needs-you/waiting/${encodeURIComponent(wait.bot.id)}/answer"));
    assert!(app.contains("'aria-label': `Reply to ${wait.bot.name}`"));
}

// N4: the button is on the map and in every room, at every width, just before Account.
#[test]
fn needs_you_is_in_both_bars_just_before_account() {
    let app = &include_str!("../web/app.js").replace("\r\n", "\n");
    for view in ["worldView", "roomView"] {
        let view = app.split_once(&format!("async function {view}(")).unwrap().1;
        let bar = view.split_once("frame.actions.append(").unwrap().1.split_once("));").unwrap().0;
        let needs = bar.find("needs.toggle").unwrap_or_else(|| panic!("no Needs you in {bar}"));
        let account = bar.find("account.toggle").unwrap();
        assert!(needs < account, "Needs you isn't before Account in {bar}");
        let between = &bar[needs + "needs.toggle".len()..account];
        assert!(!between.contains(".toggle"), "something is between them in {bar}");
        // A room's own buttons for these have gone.
        assert!(!bar.contains("questionsToggle"), "Questions is still in {bar}");
        assert!(!bar.contains("checkInsToggle"), "Check-ins is still in {bar}");
    }
    // And so have its own panels for them: Needs you is the one place, Answered the way back.
    for gone in ["panels.add('checkins'", "panels.add('questions'", "moreButton('checkins'"] {
        assert!(!app.contains(gone), "{gone} is still on the page");
    }
    assert!(app.contains("if (snapshot.owner_questions.length) panels.open('needs');"));
    let needs = app.split_once("function needsYouPanel(").unwrap().1;
    let needs = needs.split_once("\n}\n").unwrap().0;
    assert!(needs.contains("panels.toggle('needs', 'Needs you'"), "it isn't called Needs you");
    assert!(!needs.contains("className: 'sec'") && !needs.contains("className: 'mid'"));
    assert!(needs.contains("Nothing needs you right now."));
    assert!(needs.contains("api('/api/needs-you')"));
    assert!(needs.contains("30000"), "it doesn't ask again every 30 seconds");
    assert!(needs.contains("[['waiting', 'Waiting'], ['answered', 'Answered']]"), "no tabs");
    assert!(needs.contains("api('/api/needs-you/answered')"), "Answered doesn't ask");
    // H3: answering here can still keep the answer for the person's bots.
    assert!(needs.contains("'Remember this for my bots'"));
    assert!(needs.contains("from_question_id: question.id"));
}

// N11: a plan waits in the list of everyone in its room who hasn't answered it, and the city's
// trips, scenes and passes come from where a bot's inbox has them for its owner.
#[tokio::test]
async fn a_plan_to_answer_is_listed_until_its_answered() {
    let server = Server::start().await;
    let town = server.town().await;
    let starts = agenthangout::ops::iso(agenthangout::now() + 86_400);
    let plan = server
        .post(
            &town.ann.key,
            &format!("/api/hangouts/{}/events", town.room),
            json!({ "title": "Tacos", "starts_at": starts }),
        )
        .await
        .ok();
    let bo = server.needs(&town.bo).await;
    assert_eq!(count(&bo, "plans"), 1, "{bo}");
    assert_eq!(bo["plans"][0]["id"], plan["id"]);
    assert_eq!(bo["plans"][0]["title"], "Tacos");
    assert_eq!(bo["plans"][0]["room"]["name"], "Ann's room");
    assert_eq!(bo["count"], 1);
    // Proposing it was Ann's yes, so it isn't waiting on her.
    assert_eq!(count(&server.needs(&town.ann).await, "plans"), 0);
    let rsvp = format!("/api/events/{}/rsvp", text(&plan["id"]));
    server.post(&town.bo.key, &rsvp, json!({ "response": "maybe" })).await.ok();
    assert_eq!(count(&server.needs(&town.bo).await, "plans"), 0);

    for field in ["trips", "scenes", "passes"] {
        assert_eq!(count(&bo, field), 0, "{field}: {bo}");
    }
    let code = include_str!("../src/needs_you.rs");
    assert!(code.contains("crate::city::trips_to_answer(&state, &caller, now)?"));
    assert!(code.contains("crate::city::scenes_to_answer(&state, &caller, now)?"));
    assert!(code.contains("crate::city::passes_to_accept(&state, &caller)?"));
}

// N12: a mention waits in the list of the person mentioned, and only theirs, until they open the
// room or clear it.
#[tokio::test]
async fn a_mention_waits_until_the_room_is_opened_or_its_cleared() {
    let server = Server::start().await;
    let town = server.town().await;
    let room = format!("/api/hangouts/{}/messages", town.room);
    server.post(&town.ann.key, &room, json!({ "text": "@Bo are you in for Friday?" })).await.ok();
    let bo = server.needs(&town.bo).await;
    assert_eq!(count(&bo, "mentions"), 1, "{bo}");
    let mention = &bo["mentions"][0];
    assert_eq!(mention["by"], "Ann");
    assert_eq!(mention["text"], "@Bo are you in for Friday?");
    assert_eq!(mention["room"]["id"], json!(town.room));
    assert_eq!(bo["count"], 1);
    // Nobody else has it, and a person's own words don't mention them.
    assert_eq!(count(&server.needs(&town.ann).await, "mentions"), 0);
    server.post(&town.bo.key, &room, json!({ "text": "@Bo note to self" })).await.ok();
    assert_eq!(count(&server.needs(&town.bo).await, "mentions"), 1);

    // Opening the room is seeing it.
    server.get(&town.bo.key, &format!("/api/hangouts/{}", town.room)).await.ok();
    assert_eq!(count(&server.needs(&town.bo).await, "mentions"), 0);

    // A bot naming its owner mentions them too, and one can be cleared by hand, by them only.
    server.tool(&town.fern, "say", json!({ "hangout_id": town.room, "text": "@Ann done" })).await;
    let ann = server.needs(&town.ann).await;
    assert_eq!(count(&ann, "mentions"), 1, "{ann}");
    assert_eq!(ann["mentions"][0]["by"], "Fern");
    let path = format!("/api/needs-you/mentions/{}", ann["mentions"][0]["id"]);
    server.delete(&town.bo.key, &path).await.status(StatusCode::NOT_FOUND);
    server.delete(&town.fern, &path).await.status(StatusCode::FORBIDDEN);
    server.delete(&town.ann.key, &path).await.ok();
    assert_eq!(count(&server.needs(&town.ann).await, "mentions"), 0);
}

// N13: what was handled in the last day stays where it can be found, for each person what's theirs.
#[tokio::test]
async fn what_was_answered_stays_for_a_day() {
    let server = Server::start().await;
    let town = server.town().await;
    let find = |list: &Value, field: &str, id: &Value| -> Value {
        list[field]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == *id)
            .unwrap_or_else(|| panic!("{id} isn't in {field}: {list}"))
            .clone()
    };
    let question = server
        .tool(&town.fern, "ask_owner", json!({ "hangout_id": town.room, "question": "Tacos?" }))
        .await;
    let path = format!("/api/owner-questions/{}/answer", text(&question["id"]));
    server.post(&town.ann.key, &path, json!({ "answer": "Yes please" })).await.ok();
    let check_in = server
        .tool(&town.fern, "check_in", json!({ "hangout_id": town.room, "about": "Book it" }))
        .await;
    let path = format!("/api/check-ins/{}/answer", text(&check_in["id"]));
    server.post(&town.bo.key, &path, json!({ "response": "go" })).await.ok();
    let waiting = server
        .tool(&town.fern, "check_in", json!({ "hangout_id": town.room, "about": "Pay for it" }))
        .await;

    let ann = server.get(&town.ann.key, "/api/needs-you/answered").await.ok();
    assert_eq!(count(&ann, "questions"), 1, "{ann}");
    assert_eq!(ann["questions"][0]["answer"], "Yes please");
    assert_eq!(ann["questions"][0]["closed"], Value::Null);
    // Her side's check-ins: the one Bo said go to, and the one still waiting on another side.
    assert_eq!(count(&ann, "check_ins"), 2, "{ann}");
    let went = find(&ann, "check_ins", &check_in["id"]);
    assert_eq!(went["status"], "go");
    assert_eq!(went["answered_by"], "Bo");
    assert_eq!(went["yours"], true);
    assert_eq!(find(&ann, "check_ins", &waiting["id"])["status"], "waiting");

    // Bo sees the one he answered, and none of Ann's questions. The waiting one is under Waiting.
    let bo = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
    assert_eq!(count(&bo, "questions"), 0, "{bo}");
    assert_eq!(count(&bo, "check_ins"), 1, "{bo}");
    assert_eq!(bo["check_ins"][0]["id"], check_in["id"]);
    assert_eq!(count(&server.needs(&town.bo).await, "check_ins"), 1);

    // A dismissed question is there too, saying so.
    let asked = server
        .tool(&town.fern, "ask_owner", json!({ "hangout_id": town.room, "question": "Movie?" }))
        .await;
    let dismiss = format!("/api/owner-questions/{}/dismiss", text(&asked["id"]));
    server.post(&town.ann.key, &dismiss, json!({})).await.ok();
    let ann = server.get(&town.ann.key, "/api/needs-you/answered").await.ok();
    assert_eq!(find(&ann, "questions", &asked["id"])["closed"], "dismissed");

    server.get(&town.fern, "/api/needs-you/answered").await.status(StatusCode::FORBIDDEN);
    server.get("", "/api/needs-you/answered").await.status(StatusCode::UNAUTHORIZED);
}

// N12: stored notifications cannot retain access after room membership is revoked.
#[tokio::test]
async fn revoked_room_mentions_are_not_visible_in_needs_you() {
    let server = Server::start().await;
    let town = server.town().await;
    server
        .post(
            &town.ann.key,
            &format!("/api/hangouts/{}/messages", town.room),
            json!({"text": "@Bo confidential room message"}),
        )
        .await
        .ok();
    assert_eq!(count(&server.needs(&town.bo).await, "mentions"), 1);
    server.state.db.remove_member(&town.room, &town.bo.id).unwrap();
    let list = server.needs(&town.bo).await;
    assert_eq!(count(&list, "mentions"), 0);
    assert!(!list.to_string().contains("confidential room message"));
    assert!(!list.to_string().contains(&town.room));
}

// N13: handling time, not creation time, controls the Answered window.
#[tokio::test]
async fn old_check_ins_handled_today_stay_visible() {
    let server = Server::start().await;
    let town = server.town().await;
    let now = agenthangout::now();
    for (id, expires) in
        [("answered-old", now + 60), ("expired-old", now - 10), ("expired-yesterday", now - 90_000)]
    {
        server
            .state
            .db
            .insert_check_in(&agenthangout::db::NewCheckIn {
                id,
                hangout_id: &town.room,
                asked_by: &town.ann.id,
                asker: "Fern",
                agent_id: Some(&town.fern_id),
                about: id,
                to_person: None,
                needs_human: false,
                expires_at: expires,
                now: now - 172_800,
            })
            .unwrap();
    }
    server
        .post(&town.bo.key, "/api/check-ins/answered-old/answer", json!({"response":"go"}))
        .await
        .ok();
    let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
    let ids: Vec<_> =
        list["check_ins"].as_array().unwrap().iter().map(|c| text(&c["id"])).collect();
    assert!(ids.contains(&"answered-old".to_string()), "{list}");
    assert!(ids.contains(&"expired-old".to_string()), "{list}");
    assert!(!ids.contains(&"expired-yesterday".to_string()), "{list}");
    let answered =
        list["check_ins"].as_array().unwrap().iter().find(|c| c["id"] == "answered-old").unwrap();
    assert!(answered["handled_at"].as_i64().unwrap() >= now);
}

// N3: moving cards must preserve a person's ability to override a bot's answer.
#[tokio::test]
async fn answered_bot_check_ins_keep_the_human_override() {
    let server = Server::start().await;
    let town = server.town().await;
    let (_, bo_bot) = server.bot(&town.bo, &town.room, "Spruce").await;
    for (first, override_to) in [("go", "hold"), ("hold", "go")] {
        let check_in = server
            .tool(&town.fern, "check_in", json!({"hangout_id":town.room,"about":"Choose"}))
            .await;
        let path = format!("/api/check-ins/{}/answer", text(&check_in["id"]));
        server.post(&bo_bot, &path, json!({"response":first})).await.ok();
        let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
        let card = list["check_ins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == check_in["id"])
            .unwrap();
        assert_eq!(card["can_answer"], true);
        let own = server.get(&town.ann.key, "/api/needs-you/answered").await.ok();
        let own_card = own["check_ins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == check_in["id"])
            .unwrap();
        assert_eq!(own_card["can_answer"], false);
        server.post(&town.bo.key, &path, json!({"response":override_to})).await.ok();
        let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
        let card = list["check_ins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == check_in["id"])
            .unwrap();
        assert_eq!(card["can_answer"], false);
    }
}

// N11: consent cards carry the exact destination and the limits of what is being accepted.
#[tokio::test]
async fn scene_and_pass_cards_include_destination_and_consent_context() {
    let server = Server::start().await;
    let town = server.town().await;
    let made = server
        .post(&town.ann.key, "/api/cities", json!({"name":"Elm", "create_code":CODE}))
        .await
        .ok();
    let city = text(&made["city"]["id"]);
    let built = server
        .post(&town.ann.key, &format!("/api/cities/{city}/houses"), json!({"name":"Ann House"}))
        .await
        .ok();
    let house = text(&built["house"]["id"]);
    let now = agenthangout::now();
    server.state.db.insert_scene("movie", &house, "Movie Night", now).unwrap();
    server
        .state
        .db
        .insert_scene_request(&agenthangout::db::NewSceneRequest {
            id: "scene-request",
            scene_id: "movie",
            house_id: &house,
            asked_by: "Bo",
            status: "waiting",
            answered_by: None,
            expires_at: now + 600,
            now,
        })
        .unwrap();
    server
        .state
        .db
        .insert_pass(&agenthangout::db::NewPass {
            id: "pass-offer",
            house_id: &house,
            agent_id: &town.fern_id,
            max_minutes: 45,
            granted_by: "Ann",
            now,
        })
        .unwrap();
    let list = server.needs(&town.ann).await;
    let scene = &list["scenes"][0];
    assert_eq!(scene["house_id"], house);
    assert_eq!(scene["house_name"], "Ann House");
    assert_eq!(scene["asked_by"], "Bo");
    assert_eq!(scene["expires_at"], now + 600);
    let pass = &list["passes"][0];
    assert_eq!(pass["house_id"], house);
    assert_eq!(pass["house_name"], "Ann House");
    assert_eq!(pass["granted_by"], "Ann");
    assert_eq!(pass["max_minutes"], 45);
}

#[tokio::test]
async fn other_sides_waiting_items_cannot_hide_handled_check_ins() {
    let server = Server::start().await;
    let town = server.town().await;
    let now = agenthangout::now();
    for n in 0..60 {
        let id = format!("waiting-{n}");
        server
            .state
            .db
            .insert_check_in(&agenthangout::db::NewCheckIn {
                id: &id,
                hangout_id: &town.room,
                asked_by: &town.ann.id,
                asker: "Fern",
                agent_id: Some(&town.fern_id),
                about: "Waiting",
                to_person: None,
                needs_human: false,
                expires_at: now + 600,
                now,
            })
            .unwrap();
    }
    server
        .state
        .db
        .insert_check_in(&agenthangout::db::NewCheckIn {
            id: "handled",
            hangout_id: &town.room,
            asked_by: &town.ann.id,
            asker: "Fern",
            agent_id: Some(&town.fern_id),
            about: "Handled",
            to_person: None,
            needs_human: false,
            expires_at: now + 600,
            now: now - 100,
        })
        .unwrap();
    server.state.db.answer_check_in("handled", "go", "Bo", false, "", now - 10).unwrap();
    let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
    assert_eq!(count(&list, "check_ins"), 1, "{list}");
    assert_eq!(list["check_ins"][0]["id"], "handled");
}

#[tokio::test]
async fn old_bot_answers_remain_overridable_but_old_human_answers_age_out() {
    let server = Server::start().await;
    let town = server.town().await;
    let now = agenthangout::now();
    for (id, by_bot) in [("old-human", false), ("old-bot", true)] {
        server
            .state
            .db
            .insert_check_in(&agenthangout::db::NewCheckIn {
                id,
                hangout_id: &town.room,
                asked_by: &town.ann.id,
                asker: "Fern",
                agent_id: Some(&town.fern_id),
                about: id,
                to_person: None,
                needs_human: false,
                expires_at: now - 100,
                now: now - 180_000,
            })
            .unwrap();
        server.state.db.answer_check_in(id, "go", "Bo side", by_bot, "", now - 90_000).unwrap();
    }
    let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
    assert_eq!(count(&list, "check_ins"), 1, "{list}");
    assert_eq!(list["check_ins"][0]["id"], "old-bot");
    assert_eq!(list["check_ins"][0]["can_answer"], true);
    server
        .post(&town.bo.key, "/api/check-ins/old-bot/answer", json!({"response":"hold"}))
        .await
        .ok();
    let list = server.get(&town.bo.key, "/api/needs-you/answered").await.ok();
    assert_eq!(list["check_ins"][0]["status"], "hold");
    assert_eq!(list["check_ins"][0]["can_answer"], false);
    assert!(list["check_ins"][0]["handled_at"].as_i64().unwrap() >= now);
}
