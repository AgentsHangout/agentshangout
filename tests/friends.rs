//! Tests for friends and inviting them by name. They drive a real server over HTTP and are written
//! from the rules in docs/friends.md. A comment such as `// FR3` above a test names the rule it covers.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use agenthangout::{app, db::Db, friends, now, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";
const LATER: &str = "2031-06-06T18:00:00-05:00";

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
}

#[derive(Clone)]
struct Who {
    key: String,
    id: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

fn code_of(url: &Value) -> String {
    text(url).rsplit('/').next().unwrap().to_string()
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
                require_rules: false,
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

    async fn get(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::GET, path, None).await
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::POST, path, Some(body)).await
    }

    /// A room started by someone new: its id, its link's code and who started it.
    async fn room(&self, name: &str) -> (String, String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (text(&made["hangout"]["id"]), code_of(&made["invite_url"]), who(&made))
    }

    async fn join(&self, code: &str, name: &str) -> Who {
        who(&self
            .post("", &format!("/api/invites/{code}/join"), json!({ "your_name": name }))
            .await
            .ok())
    }

    /// A city started by someone new: its id, its link's code and who started it.
    async fn city(&self, name: &str) -> (String, String, Who) {
        let made = self
            .post(
                "",
                "/api/cities",
                json!({ "name": "Elm", "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (text(&made["city"]["id"]), code_of(&made["invite_url"]), who(&made))
    }

    async fn join_city(&self, code: &str, name: &str) -> Who {
        who(&self
            .post("", &format!("/api/city-invites/{code}/join"), json!({ "your_name": name }))
            .await
            .ok())
    }

    async fn friends(&self, who: &Who) -> Vec<String> {
        let body = self.get(&who.key, "/api/friends").await.ok();
        body["friends"].as_array().unwrap().iter().map(|friend| text(&friend["name"])).collect()
    }

    async fn invite_to_room(&self, from: &Who, room: &str, to: &str) -> Reply {
        self.post(
            &from.key,
            &format!("/api/hangouts/{room}/invite-friend"),
            json!({ "person_id": to }),
        )
        .await
    }

    async fn invite_to_city(&self, from: &Who, city: &str, to: &str) -> Reply {
        self.post(
            &from.key,
            &format!("/api/cities/{city}/invite-friend"),
            json!({ "person_id": to }),
        )
        .await
    }

    async fn invited(&self, who: &Who) -> Vec<Value> {
        self.get(&who.key, "/api/me").await.ok()["invited"].as_array().unwrap().clone()
    }

    /// Ann's room with Bo in it, and a second room of Bo's with Cy, so Cy is Bo's friend and a
    /// stranger to Ann: Ann, Bo, Cy, and the room Bo can invite Cy to, which is Ann's.
    async fn three(&self) -> (String, Who, Who, Who) {
        let (room, code, ann) = self.room("Ann").await;
        let bo = self.join(&code, "Bo").await;
        let (_, other, cy) = self.room("Cy").await;
        let bo_again = self.post(&bo.key, &format!("/api/invites/{other}/join"), json!({})).await;
        assert_eq!(bo_again.status, StatusCode::OK, "{}", bo_again.body);
        (room, ann, bo, cy)
    }
}

// FR1
#[tokio::test]
async fn your_friends_are_the_people_you_share_a_room_with_not_a_city() {
    let server = Server::start().await;
    let (_, ann, bo, cy) = server.three().await;
    assert_eq!(server.friends(&ann).await, ["Bo"]);
    assert_eq!(server.friends(&bo).await, ["Ann", "Cy"]);
    assert_eq!(server.friends(&cy).await, ["Bo"]);

    // #574: sharing only a city, or its Town Square, isn't a tie, so neither can reach the other.
    let (_, code, dee) = server.city("Dee").await;
    let eve = server.join_city(&code, "Eve").await;
    assert!(server.friends(&dee).await.is_empty(), "a city alone doesn't make friends");
    assert!(server.friends(&eve).await.is_empty());
}

// FR1, #574: a stranger in the same city can't invite you or ask to be your friend, since both reach
// you. Saying yes to the same plan, or an accepted friendship, is a real tie.
#[tokio::test]
async fn a_stranger_in_the_same_city_cant_reach_you_until_there_is_a_real_tie() {
    let server = Server::start().await;
    let (_, code, dee) = server.city("Dee").await;
    let eve = server.join_city(&code, "Eve").await;
    let made = server
        .post(&dee.key, "/api/hangouts", json!({ "name": "Dee's den", "create_code": CODE }))
        .await
        .ok();
    let room = text(&made["hangout"]["id"]);
    // Dee can't invite Eve to Dee's room, and gets the same 404 as for nobody.
    assert_eq!(server.invite_to_room(&dee, &room, &eve.id).await.status, StatusCode::NOT_FOUND);
    // Nor ask Eve to be a friend.
    let ask = json!({ "kind": "person", "from_id": dee.id, "to_id": eve.id });
    assert_eq!(
        server.post(&dee.key, "/api/trust/friendships", ask).await.status,
        StatusCode::NOT_FOUND
    );

    // An accepted friendship is a tie.
    let at = agenthangout::now();
    server.state.db.request_explicit_friend("person", &dee.id, &eve.id, at).unwrap();
    server.state.db.accept_explicit_friend("person", &dee.id, &eve.id).unwrap();
    assert_eq!(server.friends(&dee).await, ["Eve"]);
    assert_eq!(server.friends(&eve).await, ["Dee"]);
    server.invite_to_room(&dee, &room, &eve.id).await.ok();
}

// FR1
#[tokio::test]
async fn being_friends_is_worked_out_on_read_so_leaving_a_room_ends_it() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    assert_eq!(server.friends(&ann).await, ["Bo"]);
    server.post(&bo.key, &format!("/api/hangouts/{room}/leave"), json!({})).await.ok();
    assert!(server.friends(&ann).await.is_empty());
    assert_eq!(server.invite_to_room(&ann, &room, &bo.id).await.status, StatusCode::NOT_FOUND);
}

// FR1
#[tokio::test]
async fn someone_who_answered_a_plans_link_is_a_friend_of_that_room_and_it_is_one_way() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    let made = server
        .post(
            &ann.key,
            &format!("/api/hangouts/{room}/events"),
            json!({ "title": "Tacos", "starts_at": LATER }),
        )
        .await
        .ok();
    let event = text(&made["id"]);
    let link = code_of(
        &server.post(&ann.key, &format!("/api/events/{event}/invite"), json!({})).await.ok()
            ["invite_url"],
    );
    let answer = json!({ "response": "yes", "your_name": "Dee" });
    let dee = who(&server.post("", &format!("/api/plan-invites/{link}/answer"), answer).await.ok());

    assert_eq!(server.friends(&ann).await, ["Bo", "Dee"]);
    assert_eq!(server.friends(&bo).await, ["Ann", "Dee"]);
    assert!(server.friends(&dee).await.is_empty(), "a plan's link doesn't show who is in its room");
    // They have an account, so they can be asked in properly.
    server.invite_to_room(&ann, &room, &dee.id).await.ok();
    assert_eq!(server.invited(&dee).await.len(), 1);
}

// FR2
#[tokio::test]
async fn a_bot_gets_no_list_of_people_and_can_invite_nobody() {
    let server = Server::start().await;
    let (room, ann, bo, cy) = server.three().await;
    let (_, token) = support::legacy_bot(&server.state, &bo.id, &room, "Scout");
    let bot = Who { key: token, id: String::new() };
    assert_eq!(server.get(&bot.key, "/api/friends").await.status, StatusCode::FORBIDDEN);
    assert_eq!(server.invite_to_room(&bot, &room, &cy.id).await.status, StatusCode::FORBIDDEN);
    assert!(server.invited(&cy).await.is_empty());
    assert_eq!(server.get("", "/api/friends").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(server.friends(&ann).await, ["Bo"]);
}

// FR3
#[tokio::test]
async fn a_stranger_gets_the_same_answer_as_nobody_and_someone_already_in_gets_409() {
    let server = Server::start().await;
    let (room, ann, bo, cy) = server.three().await;
    let stranger = server.invite_to_room(&ann, &room, &cy.id).await;
    let nobody = server.invite_to_room(&ann, &room, "no-such-person").await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);
    assert_eq!((nobody.status, &nobody.body), (stranger.status, &stranger.body));
    assert!(server.invited(&cy).await.is_empty());
    assert_eq!(server.invite_to_room(&ann, &room, &bo.id).await.status, StatusCode::CONFLICT);
    assert_eq!(server.invite_to_room(&ann, &room, &ann.id).await.status, StatusCode::NOT_FOUND);
    // A room they aren't in is one they can't see.
    let (other, _, _) = server.room("Dee").await;
    assert_eq!(server.invite_to_room(&ann, &other, &bo.id).await.status, StatusCode::NOT_FOUND);
    let empty =
        server.post(&ann.key, &format!("/api/hangouts/{room}/invite-friend"), json!({})).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
}

// FR3
#[tokio::test]
async fn only_whoever_looks_after_a_citys_link_can_invite_to_it_and_managed_rooms_have_no_invite() {
    let server = Server::start().await;
    let (city, code, ann) = server.city("Ann").await;
    let bo = server.join_city(&code, "Bo").await;
    let (room, other, _) = server.room("Cy").await;
    let cy = server.join(&other, "Dee").await;
    server.post(&bo.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    assert_eq!(server.invite_to_city(&bo, &city, &cy.id).await.status, StatusCode::FORBIDDEN);
    assert!(server.invited(&cy).await.is_empty());
    assert_eq!(server.invite_to_city(&cy, &city, &bo.id).await.status, StatusCode::NOT_FOUND);
    // A Town Square takes nobody by invite, even from the person who started the city.
    let square =
        text(&server.get(&ann.key, &format!("/api/cities/{city}")).await.ok()["city"]["square_id"]);
    assert_eq!(server.invite_to_room(&ann, &square, &bo.id).await.status, StatusCode::FORBIDDEN);
    // #574: Bo and Ann share only a city, so Bo can't invite Ann anywhere (the same 404 as nobody).
    assert_eq!(server.invite_to_room(&bo, &room, &ann.id).await.status, StatusCode::NOT_FOUND);
}

// FR4, FR5
#[tokio::test]
async fn an_invite_shows_the_normal_page_and_goes_away_once_they_are_in() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();

    // Nothing has let them in.
    assert_eq!(
        server.get(&cy.key, &format!("/api/hangouts/{room}")).await.status,
        StatusCode::NOT_FOUND
    );
    let waiting = server.invited(&cy).await;
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["from"], "Ann");
    assert_eq!(waiting[0]["name"], "Ann's room");
    assert_eq!(waiting[0]["kind"], "room");
    let path = text(&waiting[0]["path"]);
    let link = path.strip_prefix("/join/").unwrap();

    server.post(&cy.key, &format!("/api/invites/{link}/join"), json!({})).await.ok();
    assert_eq!(server.get(&cy.key, &format!("/api/hangouts/{room}")).await.status, StatusCode::OK);
    assert!(server.invited(&cy).await.is_empty());
}

// FR4
#[tokio::test]
async fn an_invite_gets_nobody_past_a_citys_rules_or_its_gate() {
    let server = Server::start().await;
    let (city, code, ann) = server.city("Ann").await;
    server.join_city(&code, "Bo").await;
    server
        .post(&ann.key, "/api/hangouts", json!({ "name": "Side", "create_code": CODE }))
        .await
        .ok();
    let side = server.get(&ann.key, "/api/me").await.ok()["hangouts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "Side")
        .unwrap()
        .clone();
    let side_code = text(
        &server.get(&ann.key, &format!("/api/hangouts/{}", text(&side["id"]))).await.ok()
            ["hangout"]["invite_code"],
    );
    let cy = server.join(&side_code, "Cy").await;
    let rules = json!({ "min_vouches": 1, "rules_text": "Be kind." });
    let set =
        server.send(&ann.key, Method::PUT, &format!("/api/cities/{city}/rules"), Some(rules)).await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);

    server.invite_to_city(&ann, &city, &cy.id).await.ok();
    let waiting = server.invited(&cy).await;
    assert_eq!(waiting[0]["kind"], "city");
    let path = text(&waiting[0]["path"]);
    let link = path.strip_prefix("/join-city/").unwrap();
    // The page asks what it always asks: the rules first, and then they wait at the gate.
    let no_rules = server.post(&cy.key, &format!("/api/city-invites/{link}/join"), json!({})).await;
    assert_eq!(no_rules.status, StatusCode::CONFLICT, "{}", no_rules.body);
    server
        .post(&cy.key, &format!("/api/city-invites/{link}/join"), json!({ "accept_rules": true }))
        .await
        .ok();
    assert_eq!(
        server.get(&cy.key, &format!("/api/cities/{city}")).await.status,
        StatusCode::NOT_FOUND
    );
}

// FR5
#[tokio::test]
async fn replacing_a_link_ends_the_invites_waiting_for_it_and_the_old_link_is_never_shown() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let (_, other, _) = server.room("Dee").await;
    let cy = server.join(&other, "Cy").await;
    server.post(&ann.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    assert_eq!(text(&server.invited(&cy).await[0]["path"]), format!("/join/{code}"));
    let fresh = code_of(
        &server
            .post(&ann.key, &format!("/api/hangouts/{room}/invite/rotate"), json!({}))
            .await
            .ok()["invite_url"],
    );
    assert!(
        server.invited(&cy).await.is_empty(),
        "the new link reached someone it wasn't given to"
    );

    // Someone who can see the link can ask again, and then it's the new link they're shown.
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    assert_eq!(text(&server.invited(&cy).await[0]["path"]), format!("/join/{fresh}"));
}

/// Ann's room, with Cy as a friend of Ann's from another room but not in this one.
async fn invited_friend(server: &Server) -> (String, Who, Who) {
    let (room, _, ann) = server.room("Ann").await;
    let (_, other, _) = server.room("Dee").await;
    let cy = server.join(&other, "Cy").await;
    server.post(&ann.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    (room, ann, cy)
}

// FR6
#[tokio::test]
async fn no_thanks_sticks_and_sending_again_looks_exactly_the_same_to_the_sender() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    let first = server.invite_to_room(&ann, &room, &cy.id).await.ok();
    assert_eq!(server.invite_to_room(&ann, &room, &cy.id).await.ok(), first, "one waiting already");
    assert_eq!(server.invited(&cy).await.len(), 1);

    let id = text(&server.invited(&cy).await[0]["id"]);
    // Only the person invited can say no.
    assert_eq!(
        server.post(&ann.key, &format!("/api/friend-invites/{id}/decline"), json!({})).await.status,
        StatusCode::NOT_FOUND
    );
    server.post(&cy.key, &format!("/api/friend-invites/{id}/decline"), json!({})).await.ok();
    assert!(server.invited(&cy).await.is_empty());

    assert_eq!(
        server.invite_to_room(&ann, &room, &cy.id).await.ok(),
        first,
        "saying no doesn't show"
    );
    assert!(server.invited(&cy).await.is_empty(), "and it stuck");
    assert_eq!(
        server.post(&cy.key, &format!("/api/friend-invites/{id}/decline"), json!({})).await.status,
        StatusCode::NOT_FOUND
    );
}

// FR6
#[tokio::test]
async fn an_invite_lapses_after_two_weeks_and_can_be_sent_again() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    let (db, then) = (&server.state.db, now());
    let waiting = friends::INVITE_SECS - 60;
    assert_eq!(db.friend_invites_for(&cy.id, then - waiting, 50).unwrap().len(), 1);
    assert!(db.friend_invites_for(&cy.id, then + 1, 50).unwrap().is_empty(), "it lapsed");
    // Sent again before it lapsed changes nothing, and after it lapsed it starts over.
    assert!(!db
        .add_friend_invite("x", "room", &room, &ann.id, &cy.id, then, then - friends::INVITE_SECS)
        .unwrap());
    let later = then + friends::INVITE_SECS + 60;
    assert!(db
        .add_friend_invite("x", "room", &room, &ann.id, &cy.id, later, later - friends::INVITE_SECS)
        .unwrap());
}

// FR6
#[tokio::test]
async fn an_invite_goes_away_when_the_person_who_sent_it_leaves() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    let (_, other, _) = server.room("Dee").await;
    let cy = server.join(&other, "Cy").await;
    server.post(&bo.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    assert_eq!(server.invited(&cy).await.len(), 1);
    server.post(&bo.key, &format!("/api/hangouts/{room}/leave"), json!({})).await.ok();
    assert!(server.invited(&cy).await.is_empty());
    assert_eq!(server.friends(&ann).await, Vec::<String>::new());
}

// FR7
#[tokio::test]
async fn a_person_can_send_twenty_invites_a_day() {
    let server = Server::start().await;
    let (room, _, ann) = server.room("Ann").await;
    let (_, other, _) = server.room("Dee").await;
    server.post(&ann.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    for n in 0..friends::INVITES_PER_DAY {
        let friend = server.join(&other, &format!("Friend {n}")).await;
        server.invite_to_room(&ann, &room, &friend.id).await.ok();
    }
    let one_more = server.join(&other, "One more").await;
    let reply = server.invite_to_room(&ann, &room, &one_more.id).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS, "{}", reply.body);
    assert!(server.invited(&one_more).await.is_empty());
}

// FR7, #633: an account an agent made through the door sends 5 a day.
#[tokio::test]
async fn an_account_run_by_an_agent_sends_five_invites_a_day() {
    let server = Server::start().await;
    let (room, _, ann) = server.room("Ann").await;
    let (_, other, _) = server.room("Dee").await;
    server.post(&ann.key, &format!("/api/invites/{other}/join"), json!({})).await.ok();
    server.state.db.mark_agent_account(&ann.id, agenthangout::now()).unwrap();
    for n in 0..friends::DOOR_INVITES_PER_DAY {
        let friend = server.join(&other, &format!("Friend {n}")).await;
        server.invite_to_room(&ann, &room, &friend.id).await.ok();
    }
    let one_more = server.join(&other, "One more").await;
    let reply = server.invite_to_room(&ann, &room, &one_more.id).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS, "{}", reply.body);
}

/// Ann's city with Bo in it, and Cy, a friend of Ann's from a room who isn't in the city.
async fn city_and_friend(server: &Server) -> (String, Who, Who) {
    let (city, code, ann) = server.city("Ann").await;
    server.join_city(&code, "Bo").await;
    server
        .post(&ann.key, "/api/hangouts", json!({ "name": "Side", "create_code": CODE }))
        .await
        .ok();
    let side = server.get(&ann.key, "/api/me").await.ok()["hangouts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "Side")
        .unwrap()
        .clone();
    let side_code = text(
        &server.get(&ann.key, &format!("/api/hangouts/{}", text(&side["id"]))).await.ok()
            ["hangout"]["invite_code"],
    );
    let cy = server.join(&side_code, "Cy").await;
    (city, ann, cy)
}

async fn ask_for_vouches(server: &Server, ann: &Who, city: &str) {
    let rules = json!({ "min_vouches": 1, "rules_text": "" });
    let set =
        server.send(&ann.key, Method::PUT, &format!("/api/cities/{city}/rules"), Some(rules)).await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);
}

async fn wait_at_the_gate(server: &Server, who: &Who) {
    let path = text(&server.invited(who).await[0]["path"]);
    let link = path.strip_prefix("/join-city/").unwrap();
    let joined = server
        .post(&who.key, &format!("/api/city-invites/{link}/join"), json!({ "accept_rules": true }))
        .await
        .ok();
    assert_eq!(joined["waiting"], true, "{joined}");
}

// FR5, T17
#[tokio::test]
async fn replacing_a_citys_link_keeps_out_someone_it_invited() {
    let server = Server::start().await;
    let (city, ann, cy) = city_and_friend(&server).await;
    server.invite_to_city(&ann, &city, &cy.id).await.ok();
    assert_eq!(server.invited(&cy).await.len(), 1);
    server.post(&ann.key, &format!("/api/cities/{city}/invite/rotate"), json!({})).await.ok();
    assert!(
        server.invited(&cy).await.is_empty(),
        "the new link reached someone it was replaced to keep out"
    );
    assert_eq!(
        server.get(&cy.key, &format!("/api/cities/{city}")).await.status,
        StatusCode::NOT_FOUND
    );
}

// FR5
#[tokio::test]
async fn someone_who_left_a_room_is_not_shown_its_new_link() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    let path = text(&server.invited(&cy).await[0]["path"]);
    let link = path.strip_prefix("/join/").unwrap();
    server.post(&cy.key, &format!("/api/invites/{link}/join"), json!({})).await.ok();
    server.post(&cy.key, &format!("/api/hangouts/{room}/leave"), json!({})).await.ok();
    server.post(&ann.key, &format!("/api/hangouts/{room}/invite/rotate"), json!({})).await.ok();
    assert!(server.invited(&cy).await.is_empty());
}

// FR6, T17
#[tokio::test]
async fn being_turned_away_at_the_gate_ends_an_invite() {
    let server = Server::start().await;
    let (city, ann, cy) = city_and_friend(&server).await;
    ask_for_vouches(&server, &ann, &city).await;
    server.invite_to_city(&ann, &city, &cy.id).await.ok();
    wait_at_the_gate(&server, &cy).await;
    server
        .post(&ann.key, &format!("/api/cities/{city}/gate/{}/decline", cy.id), json!({}))
        .await
        .ok();
    assert!(server.invited(&cy).await.is_empty());

    // Asking again counts as changing your mind, and so does clearing the whole gate afterwards.
    server.invite_to_city(&ann, &city, &cy.id).await.ok();
    wait_at_the_gate(&server, &cy).await;
    server.post(&ann.key, &format!("/api/cities/{city}/gate/clear"), json!({})).await.ok();
    assert!(server.invited(&cy).await.is_empty());
}

// FR6
#[tokio::test]
async fn sending_again_after_a_link_was_replaced_brings_an_invite_back_with_no_second_ping() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    server.post(&ann.key, &format!("/api/hangouts/{room}/invite/rotate"), json!({})).await.ok();
    // `send` pings only when this says the invite is new.
    let at = now();
    let fresh = server
        .state
        .db
        .add_friend_invite("unused", "room", &room, &ann.id, &cy.id, at, at - friends::INVITE_SECS)
        .unwrap();
    assert!(!fresh, "that would have pinged them a second time");
    assert_eq!(server.invited(&cy).await.len(), 1);
}

// FR6
#[tokio::test]
async fn no_thanks_still_sticks_after_a_link_is_replaced() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    let id = text(&server.invited(&cy).await[0]["id"]);
    server.post(&cy.key, &format!("/api/friend-invites/{id}/decline"), json!({})).await.ok();
    server.post(&ann.key, &format!("/api/hangouts/{room}/invite/rotate"), json!({})).await.ok();
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    assert!(server.invited(&cy).await.is_empty(), "their no didn't stick");
}

// FR7
#[tokio::test]
async fn replacing_a_link_gives_nobody_a_days_invites_back() {
    let server = Server::start().await;
    let (room, ann, cy) = invited_friend(&server).await;
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    server.post(&ann.key, &format!("/api/hangouts/{room}/invite/rotate"), json!({})).await.ok();
    assert_eq!(server.state.db.friend_invites_sent_since(&ann.id, now() - 60).unwrap(), 1);
}

// FR1, #574: saying yes or maybe to the same plan is a tie that outlasts the room, unlike sharing it.
#[tokio::test]
async fn people_who_said_yes_to_the_same_plan_stay_friends_after_one_leaves() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    let made = server
        .post(
            &ann.key,
            &format!("/api/hangouts/{room}/events"),
            json!({ "title": "Tacos", "starts_at": LATER }),
        )
        .await
        .ok();
    let event = text(&made["id"]);
    server
        .post(&bo.key, &format!("/api/events/{event}/rsvp"), json!({ "response": "yes" }))
        .await
        .ok();
    server.post(&bo.key, &format!("/api/hangouts/{room}/leave"), json!({})).await.ok();
    assert_eq!(server.friends(&ann).await, ["Bo"], "the plan they both said yes to is a tie");
    assert_eq!(server.friends(&bo).await, ["Ann"]);
}

// FR1, #574: a plan in a Town Square is a gathering the whole city can answer (R40), so saying yes
// to the same one isn't a tie.
#[tokio::test]
async fn saying_yes_to_the_same_town_square_plan_is_no_tie() {
    let server = Server::start().await;
    let (city, code, dee) = server.city("Dee").await;
    let eve = server.join_city(&code, "Eve").await;
    let square = server.state.db.city(&city).unwrap().unwrap().square_id;
    let made = server
        .post(
            &dee.key,
            &format!("/api/hangouts/{square}/events"),
            json!({ "title": "Picnic", "starts_at": LATER }),
        )
        .await
        .ok();
    let event = text(&made["id"]);
    for who in [&dee, &eve] {
        server
            .post(&who.key, &format!("/api/events/{event}/rsvp"), json!({ "response": "yes" }))
            .await
            .ok();
    }
    assert!(server.friends(&dee).await.is_empty(), "a gathering isn't a tie");
    assert!(server.friends(&eve).await.is_empty());
}

// FR1, #574: someone only named on a plan, who hasn't answered it, has no tie to its room (#571).
#[tokio::test]
async fn someone_named_on_a_plan_who_hasnt_answered_is_no_tie() {
    let server = Server::start().await;
    let (room, _, ann) = server.room("Ann").await;
    let (_, _, bo) = server.room("Bo").await;
    let made = server
        .post(
            &ann.key,
            &format!("/api/hangouts/{room}/events"),
            json!({ "title": "Tacos", "starts_at": LATER }),
        )
        .await
        .ok();
    let event = text(&made["id"]);
    server.state.db.add_plan_invitee(&event, &bo.id, 30, now()).unwrap();
    assert!(server.friends(&ann).await.is_empty(), "named, not answered");
    server.state.db.upsert_rsvp(&event, &bo.id, "yes", None, "", now()).unwrap();
    assert_eq!(server.friends(&ann).await, ["Bo"], "answering it is the tie");
}

// BT1, #574: a bot seen only in a Town Square can't be asked to be friends, and a pending request
// about a bot doesn't let its owner be asked either. An accepted one does.
#[tokio::test]
async fn a_stranger_cant_reach_you_through_your_bot() {
    let server = Server::start().await;
    let (city, code, dee) = server.city("Dee").await;
    let eve = server.join_city(&code, "Eve").await;
    let square = server.state.db.city(&city).unwrap().unwrap().square_id;
    let (dee_bot, _) = support::legacy_bot(&server.state, &dee.id, &square, "Dee bot");
    let (eve_bot, _) = support::legacy_bot(&server.state, &eve.id, &square, "Eve bot");
    let ask_bot = json!({ "kind": "bot", "from_id": dee_bot.id, "to_id": eve_bot.id });
    assert_eq!(
        server.post(&dee.key, "/api/trust/friendships", ask_bot).await.status,
        StatusCode::NOT_FOUND,
        "a bot seen in the Town Square isn't a tie"
    );

    // A request Dee managed to make anyway, still pending, isn't a tie to Eve.
    server.state.db.request_explicit_friend("bot", &dee_bot.id, &eve_bot.id, now()).unwrap();
    let ask_eve = json!({ "kind": "person", "from_id": dee.id, "to_id": eve.id });
    assert_eq!(
        server.post(&dee.key, "/api/trust/friendships", ask_eve.clone()).await.status,
        StatusCode::NOT_FOUND
    );
    // Once Eve accepts it, Dee may ask Eve too.
    server.state.db.accept_explicit_friend("bot", &dee_bot.id, &eve_bot.id).unwrap();
    server.post(&dee.key, "/api/trust/friendships", ask_eve).await.ok();
}

// A103: a friend invite across a block looks sent and counts toward the sender's day, but never
// reaches the other side. Sent again once the block is gone, it does.
#[tokio::test]
async fn a_friend_invite_across_a_block_looks_sent_and_reaches_nobody() {
    let server = Server::start().await;
    let (room, _ann, bo, cy) = server.three().await;
    server.post(&cy.key, "/api/blocks", json!({ "person_id": bo.id })).await.ok();

    let sent = server.invite_to_room(&bo, &room, &cy.id).await.ok();
    assert_eq!(sent["ok"], true, "it looks sent: {sent}");
    assert!(server.invited(&cy).await.is_empty(), "Cy's /api/me shows nothing");
    // It counts toward Bo's invites for the day, like any other.
    let since = agenthangout::now() - 60;
    assert_eq!(server.state.db.friend_invites_sent_since(&bo.id, since).unwrap(), 1);

    // The block coming off doesn't bring the old one out.
    let off = server.send(&cy.key, Method::DELETE, &format!("/api/blocks/{}", bo.id), None).await;
    assert_eq!(off.status, StatusCode::OK, "{}", off.body);
    assert!(server.invited(&cy).await.is_empty());
    // Sent again with no block between them, it's real.
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    let invited = server.invited(&cy).await;
    assert_eq!(invited.len(), 1, "{invited:?}");
}

// A103: an invite that ended at a block stays quiet when it's sent again across the block, and a
// real invite from someone else takes over one kept quiet, named as theirs.
#[tokio::test]
async fn a_resent_invite_across_a_block_stays_quiet_and_a_real_one_takes_over() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    let (_, dee_code, _dee) = server.room("Dee").await;
    let cy = server.join(&dee_code, "Cy").await;
    for who in [&ann, &bo] {
        let joined =
            server.post(&who.key, &format!("/api/invites/{dee_code}/join"), json!({})).await;
        assert_eq!(joined.status, StatusCode::OK, "{}", joined.body);
    }

    // Bo invites Cy for real, Cy blocks Bo, and Bo sends it again: it never comes back.
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    assert_eq!(server.invited(&cy).await.len(), 1);
    server.post(&cy.key, "/api/blocks", json!({ "person_id": bo.id })).await.ok();
    assert!(server.invited(&cy).await.is_empty());
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    assert!(server.invited(&cy).await.is_empty(), "the re-send stays quiet");

    // Ann, with no block, invites Cy to the same room: it's hers, and it arrives.
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    let invited = server.invited(&cy).await;
    assert_eq!(invited.len(), 1, "{invited:?}");
    assert_eq!(invited[0]["from"], "Ann", "{invited:?}");
}

// A103 with FR6: an invite the person turned down stays turned down. A quiet re-send across a
// block, then a real invite to the same place, doesn't bring it back.
#[tokio::test]
async fn a_declined_invite_stays_declined_through_a_quiet_resend() {
    let server = Server::start().await;
    let (room, code, ann) = server.room("Ann").await;
    let bo = server.join(&code, "Bo").await;
    let (_, dee_code, _dee) = server.room("Dee").await;
    let cy = server.join(&dee_code, "Cy").await;
    for who in [&ann, &bo] {
        let joined =
            server.post(&who.key, &format!("/api/invites/{dee_code}/join"), json!({})).await;
        assert_eq!(joined.status, StatusCode::OK, "{}", joined.body);
    }
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    let id = text(&server.invited(&cy).await[0]["id"]);
    server.post(&cy.key, &format!("/api/friend-invites/{id}/decline"), json!({})).await.ok();
    server.post(&cy.key, "/api/blocks", json!({ "person_id": bo.id })).await.ok();
    server.invite_to_room(&bo, &room, &cy.id).await.ok();
    server.invite_to_room(&ann, &room, &cy.id).await.ok();
    assert!(server.invited(&cy).await.is_empty(), "{:?}", server.invited(&cy).await);
}
