//! MU1 to MU12 in docs/meetups.md. Bots suggest a place and a time to bots they can see, answer, and
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
        Self::start_on(":memory:").await
    }

    async fn start_on(path: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state = AppState::new(
            Db::open(path).unwrap(),
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

// MU6: a nudge is claimed before it is sent, so it is sent once.
#[tokio::test]
async fn two_sweeps_at_once_nudge_once_and_a_claimed_nudge_is_not_sent_again() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 60 * 60;
    let sweeps: Vec<_> = (0..4)
        .map(|_| {
            let state = server.state.clone();
            tokio::task::spawn_blocking(move || meetups::sweep(&state, start - 60))
        })
        .collect();
    for sweep in sweeps {
        sweep.await.unwrap();
    }
    let nudges = |changes: Vec<String>| changes.iter().filter(|c| *c == "meetup_now").count();
    assert_eq!(nudges(server.changes(&town.fern).await), 1);
    assert_eq!(nudges(server.changes(&town.rover).await), 1);
    meetups::sweep(&server.state, start);
    assert_eq!(nudges(server.changes(&town.fern).await), 1);

    // A claim made and never told (a crash between the two) is not told later: at most once.
    let id = text(&server.propose(&town, json!(["Fern"]), 61).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 61 * 60;
    assert_eq!(server.state.db.claim_nudges(start - 60).unwrap().len(), 2);
    meetups::sweep(&server.state, start - 60);
    assert_eq!(nudges(server.changes(&town.fern).await), 1);
}

// MU6: a cancelled meetup is not nudged, and the ring is asked again before it goes.
#[tokio::test]
async fn a_cancelled_meetup_gives_no_nudge_and_a_paused_bot_is_not_rung() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 60 * 60;

    // An unanswered invitation may ring; once the bot says yes, only a nudge may.
    assert!(!meetups::may_ring(&server.state, &town.fern_id, &id, "meetup_invited"));
    assert!(meetups::may_ring(&server.state, &town.fern_id, &id, "meetup_now"));
    server.state.db.set_doorbell_paused(&town.fern_id, true).unwrap();
    assert!(!meetups::may_ring(&server.state, &town.fern_id, &id, "meetup_now"));
    // Paused, it still gets the event.
    meetups::sweep(&server.state, start - 60);
    assert_eq!(server.changes(&town.fern).await.last().unwrap(), "meetup_now");
    server.state.db.set_doorbell_paused(&town.fern_id, false).unwrap();
    assert!(meetups::may_ring(&server.state, &town.fern_id, &id, "meetup_now"));
    server.tool(&town.rover, "cancel_meetup", json!({ "meetup_id": id })).await;
    assert!(!meetups::may_ring(&server.state, &town.fern_id, &id, "meetup_now"));

    // Cancelled before the sweep: nobody is nudged.
    let other = text(&server.propose(&town, json!(["Fern"]), 90).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": other, "answer": "yes" })).await;
    server.tool(&town.rover, "cancel_meetup", json!({ "meetup_id": other })).await;
    let later = now() + 90 * 60;
    assert!(server.state.db.claim_nudges(later - 60).unwrap().is_empty());
}

async fn set_trust(server: &Server, owner: &Who, bot: &str, wake: &str, invite: &str) {
    let mode = |m: &str| json!({ "mode": m, "people": [], "bots": [] });
    let body = json!({ "wake": mode(wake), "invite_public": mode(invite), "invite_private": mode(invite) });
    server
        .send(&owner.key, Method::PUT, &format!("/api/agents/{bot}/trust"), Some(body))
        .await
        .ok();
}

// MU11: the invitee's owner decides who may invite it; their own bots are theirs to arrange.
#[tokio::test]
async fn an_invitee_whose_owner_restricted_invites_cannot_be_invited_by_other_owners_bots() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    set_trust(&server, &town.ann, &town.fern_id, "participants", "owner").await;
    let said = server
        .refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Fern"]), 30))
        .await;
    assert!(said.contains("not allowed you to invite"), "{said}");
    assert!(server.inbox_meetups(&town.fern).await.is_empty());
    // Only the owner as a person is exempt, as in rooms: Bo's own Pip, with the same setting,
    // is refused to Rover too, and open again once Bo allows it.
    set_trust(&server, &town.bo, &town.pip_id, "owner", "owner").await;
    server.refused(&town.rover, "propose_meetup", asking(&town.cafe, json!(["Pip"]), 30)).await;
    set_trust(&server, &town.bo, &town.pip_id, "owner", "participants").await;
    server.propose(&town, json!([town.pip_id]), 30).await;
    // Opening it up again lets Rover invite Fern.
    set_trust(&server, &town.ann, &town.fern_id, "participants", "participants").await;
    server.propose(&town, json!(["Fern"]), 30).await;
}

// MU11: the ring is judged again against the bot's wake setting when it is about to go.
#[tokio::test]
async fn an_owner_who_restricts_waking_after_the_invite_stops_the_ring() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern", town.pip_id]), 60).await["id"]);
    let ring = |bot: &str, change: &str| meetups::may_ring(&server.state, bot, &id, change);
    assert!(ring(&town.fern_id, "meetup_invited"));
    set_trust(&server, &town.ann, &town.fern_id, "owner", "participants").await;
    assert!(!ring(&town.fern_id, "meetup_invited"));
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    assert!(!ring(&town.fern_id, "meetup_now"), "a yes doesn't undo the owner's setting");
    set_trust(&server, &town.ann, &town.fern_id, "participants", "participants").await;
    assert!(ring(&town.fern_id, "meetup_now"));
    server.state.db.set_wakes_only_for_owner(&town.fern_id, true).unwrap();
    assert!(!ring(&town.fern_id, "meetup_now"));
    // A sibling bot is no exception: Pip's owner restricting waking stops Rover's invitation ring.
    assert!(ring(&town.pip_id, "meetup_invited"));
    set_trust(&server, &town.bo, &town.pip_id, "owner", "participants").await;
    assert!(!ring(&town.pip_id, "meetup_invited"));
    // The proposer's own nudge is its own plan, whatever its wake setting says.
    server.state.db.set_wakes_only_for_owner(&town.rover_id, true).unwrap();
    set_trust(&server, &town.bo, &town.rover_id, "owner", "owner").await;
    assert!(ring(&town.rover_id, "meetup_now"));
    // The event is there for every one of these.
    assert_eq!(server.changes(&town.fern).await, ["meetup_invited"]);
}

// MU11: a meetup ring and a person's accepted wake (E31) hold each other in a room for three minutes.
#[tokio::test]
async fn a_meetup_ring_and_a_persons_wake_hold_each_other() {
    let server = Server::start().await;
    let town = server.town().await;
    let at = now();
    let db = &server.state.db;
    let recent = |bot: &str, room: &str, t: i64| db.person_wake_recent(bot, room, t - 180).unwrap();
    assert!(db.claim_meetup_ring(&town.fern_id, &town.cafe, at - 180, at).unwrap());
    assert!(recent(&town.fern_id, &town.cafe, at + 60), "it holds a person's mention");
    assert!(!db.claim_meetup_ring(&town.fern_id, &town.cafe, at + 60 - 180, at + 60).unwrap());
    // Another bot, another room, and three minutes on are free.
    assert!(db.claim_meetup_ring(&town.pip_id, &town.cafe, at + 60 - 180, at + 60).unwrap());
    assert!(db.claim_meetup_ring(&town.fern_id, &town.bo_room, at + 60 - 180, at + 60).unwrap());
    assert!(db.claim_meetup_ring(&town.fern_id, &town.cafe, at + 181 - 180, at + 181).unwrap());

    // The other way: a person's wake accepted a minute ago holds the meetup ring.
    let line = server
        .send(
            &town.ann.key,
            Method::POST,
            &format!("/api/hangouts/{}/messages", town.cafe),
            Some(json!({ "text": "hello" })),
        )
        .await
        .ok();
    let message = line["id"].as_i64().unwrap();
    db.note_person_wake(&town.rover_id, message, &town.cafe, at).unwrap();
    assert!(!db.claim_meetup_ring(&town.rover_id, &town.cafe, at + 60 - 180, at + 60).unwrap());
    assert!(db.claim_meetup_ring(&town.rover_id, &town.cafe, at + 181 - 180, at + 181).unwrap());
}

// MU6: a nudge claimed, then the meetup cancelled or the bot saying no, is not written at all.
#[tokio::test]
async fn a_nudge_claimed_before_a_cancellation_or_a_no_writes_no_event() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    server.all_day(&town.bo, &town.pip_id, true).await;
    let id = text(&server.propose(&town, json!(["Fern", town.pip_id]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    server.tool(&town.pip, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 60 * 60;
    let claimed = server.state.db.claim_nudges(start - 60).unwrap();
    assert_eq!(claimed.len(), 3);
    // After the claim and before the write: Pip says no.
    server.tool(&town.pip, "answer_meetup", json!({ "meetup_id": id, "answer": "no" })).await;
    let write = |meetup: &str, bot: &str| {
        agenthangout::events::note_meetup(
            &server.state,
            bot,
            &town.cafe,
            &format!("meetup:{meetup}:meetup_now:{bot}::"),
            json!({ "meetup_id": meetup, "change": "meetup_now" }),
            Some((meetup, bot)),
        )
    };
    assert!(!write(&id, &town.pip_id));
    assert!(write(&id, &town.fern_id), "a bot that still says yes is nudged");
    // And a cancellation after the claim writes nothing for anyone.
    let other = text(&server.propose(&town, json!(["Fern"]), 90).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": other, "answer": "yes" })).await;
    server.state.db.claim_nudges(now() + 90 * 60 - 60).unwrap();
    server.tool(&town.rover, "cancel_meetup", json!({ "meetup_id": other })).await;
    assert!(!write(&other, &town.fern_id));
    assert!(!write(&other, &town.rover_id));
    let changes = server.changes(&town.fern).await;
    assert_eq!(changes.iter().filter(|c| *c == "meetup_now").count(), 1);
    assert!(server.changes(&town.pip).await.iter().all(|c| c != "meetup_now"));
}

// MU12: the owner can take their bot out until the last late nudge could go; the bots, only until
// the start.
#[tokio::test]
async fn an_owner_can_cancel_until_the_late_nudges_end() {
    let server = Server::start().await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let fern_part = format!("/api/agents/{}/meetups/{id}", town.fern_id);
    let rover_part = format!("/api/agents/{}/meetups/{id}", town.rover_id);

    // Ten minutes after the start a late nudge is still due, and Ann can still stop Fern's.
    server.state.db.set_meetup_start(&id, now() - 10 * 60).unwrap();
    assert_eq!(server.inbox_meetups(&town.fern).await.len(), 1);
    server.refused(&town.rover, "cancel_meetup", json!({ "meetup_id": id })).await;
    server.send(&town.ann.key, Method::DELETE, &fern_part, None).await.ok();
    meetups::sweep(&server.state, now());
    assert!(server.changes(&town.fern).await.iter().all(|c| c != "meetup_now"));
    assert!(server.changes(&town.rover).await.contains(&"meetup_now".to_string()));
    // Bo ends it for all while Rover's late nudge window is open.
    server.send(&town.bo.key, Method::DELETE, &rover_part, None).await.ok();

    // Past the window nothing is left to stop.
    let id2 = text(&server.propose(&town, json!(["Fern"]), 60).await["id"]);
    server.state.db.set_meetup_start(&id2, now() - 31 * 60).unwrap();
    let path = format!("/api/agents/{}/meetups/{id2}", town.rover_id);
    let late = server.send(&town.bo.key, Method::DELETE, &path, None).await;
    assert_eq!(late.status, StatusCode::CONFLICT);
}

// MU6: the check and the write are one transaction, so a cancellation on another connection to the
// same database (another server) can't land between them.
#[tokio::test]
async fn a_cancellation_through_another_connection_beats_a_claimed_nudge() {
    let path = std::env::temp_dir().join(format!("meetups-{}.db", agenthangout::auth::new_id()));
    let file = path.to_str().unwrap().to_string();
    let server = Server::start_on(&file).await;
    let town = server.town().await;
    server.rover_out(&town).await;
    let id = text(&server.propose(&town, json!(["Fern"]), 60).await["id"]);
    server.tool(&town.fern, "answer_meetup", json!({ "meetup_id": id, "answer": "yes" })).await;
    let start = now() + 60 * 60;
    assert_eq!(server.state.db.claim_nudges(start - 60).unwrap().len(), 2);

    // Another server cancels, then this one goes on to write what it had claimed.
    let other = Db::open(&file).unwrap();
    other.cancel_meetup_row(&id).unwrap();
    let written = agenthangout::events::note_meetup(
        &server.state,
        &town.fern_id,
        &town.cafe,
        &format!("meetup:{id}:meetup_now:{}::", town.fern_id),
        json!({ "meetup_id": id }),
        Some((id.as_str(), town.fern_id.as_str())),
    );
    assert!(!written);
    assert!(server.changes(&town.fern).await.iter().all(|c| c != "meetup_now"));
    drop(server);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{file}{suffix}"));
    }
}
