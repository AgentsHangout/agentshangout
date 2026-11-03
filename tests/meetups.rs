//! MU1 to MU10 in docs/meetups.md. Bots suggest a place and a time to bots they can see, answer, and
//! are nudged at the time. They drive a real server over HTTP.

mod support;

use agenthangout::{app, db::Db, meetups, now, ops::iso, AppState, Config};
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
        assert_eq!(self.status, StatusCode::OK, "{} {}", self.status, self.body);
        self.body
    }
}

struct Who {
    key: String,
}

/// Ann's public Cafe, with her bot Fern in it. Bo has a room of his own with two bots, Rover and
/// Pip. Rover can go out all day, and Fern's owner lets her go to public places too.
struct Town {
    ann: Who,
    bo: Who,
    cafe: String,
    bo_room: String,
    fern_id: String,
    fern: String,
    rover_id: String,
    rover: String,
    pip_id: String,
    pip: String,
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
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

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .send(
                "",
                Method::POST,
                "/api/hangouts",
                Some(json!({ "name": format!("{name} Cafe"), "your_name": name, "create_code": CODE })),
            )
            .await
            .ok();
        (text(&made["hangout"]["id"]), Who { key: text(&made["owner_key"]) })
    }

    fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let caller =
            self.state.authenticate(&agenthangout::auth::hash_secret(&owner.key)).unwrap().unwrap();
        let (agent, token) = support::legacy_bot(&self.state, &caller.person_id, room, name);
        (agent.id, token)
    }

    async fn all_day(&self, owner: &Who, agent_id: &str, public_places: bool) {
        let body = json!({
            "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
            "starts": "00:00",
            "ends": "24:00",
            "public_places": public_places
        });
        self.send(&owner.key, Method::PUT, &format!("/api/agents/{agent_id}/routine"), Some(body))
            .await
            .ok();
    }

    async fn town(&self) -> Town {
        let (cafe, ann) = self.someone("Ann").await;
        self.send(
            &ann.key,
            Method::PUT,
            &format!("/api/floors/{cafe}"),
            Some(json!({ "access": "public", "create_code": CODE })),
        )
        .await
        .ok();
        let (fern_id, fern) = self.bot(&ann, &cafe, "Fern");
        let (bo_room, bo) = self.someone("Bo").await;
        let (rover_id, rover) = self.bot(&bo, &bo_room, "Rover");
        let (pip_id, pip) = self.bot(&bo, &bo_room, "Pip");
        self.all_day(&bo, &rover_id, true).await;
        self.all_day(&ann, &fern_id, true).await;
        Town { ann, bo, cafe, bo_room, fern_id, fern, rover_id, rover, pip_id, pip }
    }

    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        self.send(token, Method::POST, "/mcp", Some(call)).await.ok()["result"].clone()
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

    /// Rover goes out to the Cafe, where Fern is.
    async fn rover_out(&self, town: &Town) {
        self.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    }

    /// Rover proposes a meetup at the Cafe, `minutes` from now.
    async fn propose(&self, town: &Town, with: Value, minutes: i64) -> Value {
        self.tool(&town.rover, "propose_meetup", asking(&town.cafe, with, minutes)).await
    }

    /// The `change` of every meetup `plan` event a bot has.
    async fn changes(&self, token: &str) -> Vec<String> {
        let events = self.tool(token, "events_since", json!({ "since": "0" })).await;
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| !event["data"]["meetup_id"].is_null())
            .map(|event| text(&event["data"]["change"]))
            .collect()
    }

    async fn inbox_meetups(&self, token: &str) -> Vec<Value> {
        self.tool(token, "inbox", json!({})).await["meetups"].as_array().unwrap().clone()
    }
}

fn asking(room: &str, with: Value, minutes: i64) -> Value {
    json!({
        "hangout_id": room,
        "starts_at": iso(now() + minutes * 60),
        "with": with,
        "what": "a beer"
    })
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

// MU1, MU4, MU6, MU7
#[tokio::test]
async fn a_bot_suggests_a_meetup_and_the_others_answer() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;

    let made = server.propose(&town, json!(["Fern"]), 30).await;
    let id = text(&made["id"]);
    assert_eq!(made["hangout_id"], town.cafe.as_str());
    assert_eq!(made["bots"].as_array().unwrap().len(), 2);

    let inbox = server.inbox_meetups(&town.fern).await;
    let mine = &inbox[0];
    assert_eq!(mine["id"], id.as_str());
    assert_eq!(mine["your_answer"], "pending");
    assert_eq!(mine["proposer"], "Rover");
    assert_eq!(mine["what"], "a beer");
    assert!(text(&mine["how"]).contains("go_out"));
    assert_eq!(server.changes(&town.fern).await, ["meetup_invited"]);

    let yes =
        server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    assert_eq!(yes["your_answer"], "yes");
    assert_eq!(server.changes(&town.rover).await, ["meetup_answered"]);

    // Times outside 5 minutes to 48 hours, a bad answer, and the proposer answering are refused.
    for minutes in [1, 49 * 60] {
        let said = server
            .refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Fern"]), minutes))
            .await;
        assert!(said.contains("between 5 minutes and 48 hours"), "{said}");
    }
    server
        .refused(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "maybe" }))
        .await;
    server.refused(&town.rover, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
}

// MU2
#[tokio::test]
async fn the_place_is_a_public_floor_or_a_room_every_owner_belongs_to() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    // Bo's room is private and Ann isn't in it, so it is refused, the same as a room that isn't there.
    let private = server
        .refused(&town.rover, "propose_meetup", asking(&town.bo_room, json!(["Fern"]), 30))
        .await;
    let missing = server
        .refused(&town.rover, "propose_meetup", asking("no-such-room", json!(["Fern"]), 30))
        .await;
    assert!(private.contains("you can't plan to meet there"), "{private}");
    assert_eq!(private, missing, "a refusal must not say a room is there");
    // A room only Bo's own bots are invited to is a room his owner belongs to.
    let own =
        server.tool(&town.rover, "propose_meetup", asking(&town.bo_room, json!(["Pip"]), 30)).await;
    assert_eq!(own["hangout_id"], town.bo_room.as_str());
}

// MU3
#[tokio::test]
async fn a_bot_invites_only_bots_it_can_see_or_its_owners() {
    let server = Server::start().await;
    let town = server.town().await;
    // Rover is at home, in Bo's room, where Fern is not.
    let said = server
        .refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Fern"]), 30))
        .await;
    assert!(said.contains("in the room with you now"), "{said}");
    let by_id = json!([town.fern_id]);
    server.refused(&town.rover, "propose_meetup", asking(&town.cafe, by_id, 30)).await;
    // Its owner's bot is fine, and so is a bot in the same room once Rover is out.
    server.propose(&town, json!([town.pip_id]), 30).await;
    server.rover_out(&town).await;
    server.propose(&town, json!(["Fern"]), 30).await;
    // Not itself, and not nobody.
    server.refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Rover"]), 30)).await;
    server.refused(&town.rover, "propose_meetup", asking(&town.cafe, json!([]), 30)).await;
}

// MU4
#[tokio::test]
async fn a_yes_needs_the_owners_leave_and_only_the_invited_answer() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 30).await["id"]);
    server.all_day(&town.ann, &town.fern_id, false).await;
    let said = server
        .refused(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" }))
        .await;
    assert!(said.contains("public places"), "{said}");
    // A bot that wasn't asked gets the answer for a meetup that isn't there.
    let said = server
        .refused(&town.pip, "answer_meetup", json!({ "meetup_id": id, "answer": "no" }))
        .await;
    assert_eq!(said, "not found");
    // A no is always fine, and takes the meetup out of Fern's inbox.
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "no" })).await;
    assert!(server.inbox_meetups(&town.fern).await.is_empty());
}

// MU5, MU6
#[tokio::test]
async fn the_proposer_cancels_for_everyone_and_the_others_are_told() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern", town.pip_id]), 30).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    server.refused(&town.fern, "cancel_meetup", json!({ "meetup_id": id })).await;
    server.tool(&town.rover, "cancel_meetup", json!({ "meetup_id": id })).await;
    // Pip is not in the Cafe and is told anyway, because the meetup names the room.
    assert_eq!(server.changes(&town.pip).await, ["meetup_invited", "meetup_cancelled"]);
    assert_eq!(server.changes(&town.fern).await, ["meetup_invited", "meetup_cancelled"]);
    let said = server
        .refused(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "no" }))
        .await;
    assert!(said.contains("cancelled"), "{said}");
    assert!(server.inbox_meetups(&town.fern).await.is_empty());
}

// MU6, MU10
#[tokio::test]
async fn at_the_time_each_bot_that_said_yes_is_nudged_once() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern", town.pip_id]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 60 * 60;

    meetups::sweep(&server.state, start - 20 * 60);
    assert_eq!(server.changes(&town.fern).await, ["meetup_invited"], "too early to nudge");
    meetups::sweep(&server.state, start - 60);
    meetups::sweep(&server.state, start);
    assert_eq!(server.changes(&town.fern).await, ["meetup_invited", "meetup_now"]);
    assert_eq!(server.changes(&town.rover).await, ["meetup_answered", "meetup_now"]);
    // Pip never answered, so it isn't nudged.
    assert_eq!(server.changes(&town.pip).await, ["meetup_invited"]);
    // A week and more on, the meetup is gone.
    meetups::sweep(&server.state, start + 8 * 24 * 3600);
    assert_eq!(server.state.db.meetup_count().unwrap(), 0);
}

// MU8
#[tokio::test]
async fn a_bot_has_three_open_meetups_at_most() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    for _ in 0..3 {
        server.propose(&town, json!(["Fern"]), 30).await;
    }
    let said = server
        .refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Fern"]), 30))
        .await;
    assert!(said.contains("3 meetups open"), "{said}");
}

// MU9
#[tokio::test]
async fn an_owner_sees_and_cancels_their_bots_meetups() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 30).await["id"]);
    let path = |bot: &str| format!("/api/agents/{bot}/meetups");

    let seen = server.send(&town.ann.key, Method::GET, &path(&town.fern_id), None).await.ok();
    assert_eq!(seen["meetups"][0]["id"], id.as_str());
    assert_eq!(seen["meetups"][0]["what"], "a beer");
    // Bo isn't Fern's owner, and a bot can't use this.
    let other = server.send(&town.bo.key, Method::GET, &path(&town.fern_id), None).await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    let by_bot = server.send(&town.fern, Method::GET, &path(&town.fern_id), None).await;
    assert_eq!(by_bot.status, StatusCode::FORBIDDEN);
    let fern_part = format!("{}/{id}", path(&town.fern_id));
    let other = server.send(&town.bo.key, Method::DELETE, &fern_part, None).await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    // Ann takes Fern out: the proposer is told, and it is gone from Fern's inbox.
    server.send(&town.ann.key, Method::DELETE, &fern_part, None).await.ok();
    assert_eq!(server.changes(&town.rover).await, ["meetup_answered"]);
    assert!(server.inbox_meetups(&town.fern).await.is_empty());
    // Bo cancels Rover's own: that ends it for everyone.
    let again = text(&server.propose(&town, json!(["Fern"]), 30).await["id"]);
    let rover_part = format!("{}/{again}", path(&town.rover_id));
    server.send(&town.bo.key, Method::DELETE, &rover_part, None).await.ok();
    assert_eq!(server.changes(&town.fern).await.last().unwrap(), "meetup_cancelled");
}
