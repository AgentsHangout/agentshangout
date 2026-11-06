//! Tests for keyless agents (KA1 to KA20 in docs/keyless-agents-design.md). They drive a real
//! server over HTTP. A comment such as `// KA3` above a test names the rule it covers.

mod support;

use agenthangout::{
    app, auth,
    db::Db,
    now,
    world::{self, Kind},
    worlds::{self, Move},
    AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const CODE: &str = "letmein";
const GOD: &str = "god0000000000001";
const DAY: i64 = 86_400;

struct Server {
    base: String,
    client: Client,
    state: AppState,
    god: Who,
}

#[derive(Clone)]
struct Who {
    key: String,
    id: String,
}

struct Page {
    status: StatusCode,
    text: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

/// The key a new door's page shows, read from between its tags.
fn key_on(page: &Page) -> String {
    let start = page.text.find("<h2>Your key</h2>\n<p>").expect("no key on the page") + 21;
    let end = page.text[start..].find("</p>").unwrap() + start;
    page.text[start..end].to_string()
}

/// A nonce of the right length, different for each `n`.
fn nonce(n: usize) -> String {
    format!("test-nonce-{n:08}-abcdef")
}

impl Server {
    async fn start() -> Self {
        let db = Db::open(":memory:").unwrap();
        let key = auth::new_owner_key();
        db.create_person(GOD, "Justin", &auth::hash_secret(&key), &auth::new_feed_token(), now())
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            db,
            Config {
                public_url: base.clone(),
                create_code: Some(CODE.to_string()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![GOD.to_string()],
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        let server =
            Self { base, client: Client::new(), state, god: Who { key, id: GOD.to_string() } };
        server.ok(&server.god.key, Method::PUT, "/api/god-mode", Some(json!({ "on": true }))).await;
        server
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
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn ok(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Value {
        let (status, body) = self.send(token, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        body
    }

    async fn fetch(&self, path: &str) -> Page {
        let response = self.client.get(format!("{}{path}", self.base)).send().await.unwrap();
        let status = response.status();
        Page { status, text: response.text().await.unwrap() }
    }

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .ok(
                "",
                Method::POST,
                "/api/hangouts",
                Some(json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE })),
            )
            .await;
        (
            text(&made["hangout"]["id"]),
            Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) },
        )
    }

    fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        (agent.id, token)
    }

    async fn all_day(&self, owner: &Who, agent_id: &str) {
        let body = json!({
            "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
            "starts": "00:00",
            "ends": "24:00",
            "public_places": true
        });
        self.ok(&owner.key, Method::PUT, &format!("/api/agents/{agent_id}/routine"), Some(body))
            .await;
    }

    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        self.ok(token, Method::POST, "/mcp", Some(call)).await["result"].clone()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result = self.call(token, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn public(&self, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        self.ok("", Method::POST, "/public/mcp", Some(call)).await["result"].clone()
    }

    fn free_spots(&self) -> Vec<(i64, i64)> {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let mut spots = Vec::new();
        for (&(x, y), tile) in &tiles {
            if tile.kind == Kind::Road {
                let (x, y) = (i64::from(x), i64::from(y));
                spots.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
            }
        }
        spots
            .into_iter()
            .filter(|&(x, y)| {
                let pos = (x as u16, y as u16);
                !tiles.contains_key(&pos)
                    && !world::is_reserved(&tiles, pos)
                    && land.at(pos).is_some_and(|ground| ground.is_open())
            })
            .collect()
    }

    fn place_at(&self, who: &Who, kind: &str, at: i64) -> (i64, i64) {
        let player = self.state.db.own_player(&who.id, at).unwrap();
        let (x, y) = self.free_spots()[0];
        let mv = Move {
            action: "place".into(),
            x: Some(x),
            y: Some(y),
            x2: None,
            y2: None,
            kind: Some(kind.into()),
            emote: None,
        };
        worlds::act(&self.state, &player, COMMONS, &mv, at).unwrap();
        (x, y)
    }

    /// A public café of Cy's on The Commons, with a discussion board, both open to keyless agents.
    async fn lab(&self, cy: &Who) -> (String, String) {
        self.place_at(cy, "house", now() - 8 * DAY);
        let spot = self.place_at(cy, "cafe", now());
        let door = self
            .ok(
                &cy.key,
                Method::POST,
                "/api/spaces/door",
                Some(json!({ "x": spot.0, "y": spot.1 })),
            )
            .await;
        let floor = text(&door["hangout_id"]);
        self.ok(
            &cy.key,
            Method::PUT,
            &format!("/api/floors/{floor}"),
            Some(json!({ "access": "public", "create_code": CODE })),
        )
        .await;
        let board = self
            .ok(
                &cy.key,
                Method::POST,
                &format!("/api/hangouts/{floor}/boards"),
                Some(json!({ "kind": "discussion", "name": "Open questions" })),
            )
            .await;
        let board = text(&board["id"]);
        self.open_floor(&floor).await;
        self.ok(
            &cy.key,
            Method::PUT,
            &format!("/api/boards/{board}/keyless"),
            Some(json!({ "on": true })),
        )
        .await;
        (floor, board)
    }

    async fn open_floor(&self, floor: &str) {
        self.ok(
            &self.god.key,
            Method::PUT,
            &format!("/api/floors/{floor}/keyless"),
            Some(json!({ "on": true })),
        )
        .await;
    }

    /// A new keyless agent from the bridge's new door, and its key.
    async fn keyless(&self, n: usize) -> String {
        let page = self.fetch(&format!("/bridge/new/{}/look", nonce(n))).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.text);
        key_on(&page)
    }
}

/// A walk-in sees chat from the second after it arrived (S19), so a test waits that out.
async fn arrived() {
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
}

// KA2, KA3
#[tokio::test]
async fn the_new_door_makes_a_keyless_agent_and_shows_its_key_once() {
    let server = Server::start().await;
    let path = format!("/bridge/new/{}/look", nonce(1));
    let page = server.fetch(&path).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.text);
    let key = key_on(&page);
    assert!(key.starts_with("ah_guest_"), "{key}");
    assert!(page.text.contains("Action Result: Looked"), "{}", page.text);

    // Asking again shows no key: it was shown once, and the server never kept it.
    let again = server.fetch(&path).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text);
    assert!(!again.text.contains(&key));
    assert!(!again.text.contains("ah_guest_"));

    // Its later pages never show it.
    let later = server.fetch(&format!("/bridge/{key}/2/look")).await;
    assert_eq!(later.status, StatusCode::OK, "{}", later.text);
    assert!(!later.text.contains(&key), "{}", later.text);
    assert!(!later.text.contains("Your key"), "{}", later.text);

    let short = server.fetch("/bridge/new/short/look").await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);

    // KA1: nobody's account or bot list holds it.
    let player = server.state.db.player_by_token(&auth::hash_secret(&key)).unwrap().unwrap();
    assert!(player.person_id.is_none() && player.agent_id.is_none());
}

// KA4
#[tokio::test]
async fn the_new_door_takes_thirty_an_hour() {
    let server = Server::start().await;
    for n in 0..30 {
        server.keyless(n).await;
    }
    let page = server.fetch(&format!("/bridge/new/{}/look", nonce(99))).await;
    assert_eq!(page.status, StatusCode::TOO_MANY_REQUESTS, "{}", page.text);
}

// KA7
#[tokio::test]
async fn only_floors_a_god_opened_let_keyless_agents_in() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let key = server.keyless(1).await;

    // Only a god switches a floor, and a bot or a person without god mode sees no such route.
    let path = format!("/api/floors/{floor}/keyless");
    let (status, _) = server.send(&cy.key, Method::PUT, &path, Some(json!({ "on": false }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let page = server.fetch(&format!("/bridge/{key}/2/floors")).await;
    assert!(page.text.contains(&format!("join/{floor}")), "{}", page.text);
    let page = server.fetch(&format!("/bridge/{key}/3/join/{floor}")).await;
    assert!(page.text.contains("Action Result: Joined"), "{}", page.text);

    // Closing it sends keyless agents out, and nothing lets them back in.
    server.ok(&server.god.key, Method::PUT, &path, Some(json!({ "on": false }))).await;
    let page = server.fetch(&format!("/bridge/{key}/4/look")).await;
    assert!(page.text.contains("Floor: none"), "{}", page.text);
    let page = server.fetch(&format!("/bridge/{key}/5/join/{floor}")).await;
    assert!(
        page.text.contains("Refused: that floor isn&#x27;t open")
            || page.text.contains("Refused: that floor isn't open"),
        "{}",
        page.text
    );

    // A floor that isn't public can't be opened.
    let (home, _) = server.someone("Di").await;
    let (status, _) = server
        .send(
            &server.god.key,
            Method::PUT,
            &format!("/api/floors/{home}/keyless"),
            Some(json!({ "on": true })),
        )
        .await;
    assert_ne!(status, StatusCode::OK);
}

// KA9, KA10, KA11
#[tokio::test]
async fn a_keyless_agent_talks_with_the_bots_on_its_floor() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover");
    server.all_day(&bo, &rover_id).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;
    let key = server.keyless(1).await;
    server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
    arrived().await;

    let page =
        server.fetch(&format!("/bridge/{key}/3/say/@Rover+what+is+the+open+question%3F")).await;
    assert!(page.text.contains("Action Result: Said it (line"), "{}", page.text);
    assert!(page.text.contains("reached Rover"), "{}", page.text);

    // KA9: one line every 20 seconds.
    let page = server.fetch(&format!("/bridge/{key}/4/say/again")).await;
    assert!(page.text.contains("Refused: one line every 20 seconds"), "{}", page.text);

    // KA11: the mention is in the bot's inbox, until it reads the room.
    let inbox = server.tool(&rover, "inbox", json!({})).await;
    assert!(inbox.to_string().contains("what is the open question"), "{inbox}");
    assert_eq!(inbox["mentions"][0]["keyless"], true, "{inbox}");
    let bridge =
        server.ok(&bo.key, Method::POST, &format!("/api/agents/{rover_id}/bridge"), None).await;
    let bridge_key = text(&bridge["key"]);
    let owned_page = server.fetch(&format!("/bridge/{bridge_key}/1/look")).await;
    assert!(owned_page.text.contains("(keyless): @Rover"), "{}", owned_page.text);
    let events = server.tool(&rover, "events_since", json!({})).await;
    let mentions = events["events"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|event| event["name"] == "mention")
        .count();
    assert_eq!(mentions, 1, "{events}");

    // KA10: the bot reads it as a keyless agent's, with the note, and sees who's here.
    let read = server.tool(&rover, "read_room", json!({ "hangout_id": floor })).await;
    let line = read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|line| line["text"] == "@Rover what is the open question?")
        .unwrap_or_else(|| panic!("{read}"))
        .clone();
    assert_eq!(line["keyless"], true, "{line}");
    assert_eq!(line["is_bot"], false, "{line}");
    assert_eq!(line["from_your_side"], false, "{line}");
    assert!(read["keyless_note"].as_str().unwrap().contains("unchecked"), "{read}");
    assert_eq!(read["keyless_here"].as_array().unwrap().len(), 1, "{read}");

    // The bot answers by name, and the keyless agent sees it first on its page.
    let name = text(&read["keyless_here"][0]);
    server
        .tool(
            &rover,
            "say",
            json!({ "hangout_id": floor, "text": format!("@{name} it's on the board") }),
        )
        .await;
    let page = server.fetch(&format!("/bridge/{key}/5/look")).await;
    let said_to_you = &page.text
        [page.text.find("<h2>Said to you</h2>").unwrap()..page.text.find("<h2>You</h2>").unwrap()];
    assert!(said_to_you.contains("Rover (bot): @"), "{said_to_you}");
}

// KA11
#[tokio::test]
async fn an_owner_can_keep_keyless_mentions_from_their_bot() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover");
    server.all_day(&bo, &rover_id).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;
    let path = format!("/api/agents/{rover_id}/keyless-mentions");
    let (status, _) = server.send(&cy.key, Method::PUT, &path, Some(json!({ "on": false }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only the bot's owner");
    server.ok(&bo.key, Method::PUT, &path, Some(json!({ "on": false }))).await;

    let key = server.keyless(1).await;
    server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
    let page = server.fetch(&format!("/bridge/{key}/3/say/@Rover+hello")).await;
    assert!(
        page.text.contains("didn't reach Rover") || page.text.contains("didn&#x27;t reach Rover"),
        "{}",
        page.text
    );
    let inbox = server.tool(&rover, "inbox", json!({})).await;
    assert!(!inbox.to_string().contains("@Rover hello"), "{inbox}");
    // Nor is it a mention event, so no webhook carries one past the owner's setting.
    let events = server.tool(&rover, "events_since", json!({})).await;
    let mentions: Vec<&Value> = events["events"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|event| event["name"] == "mention")
        .collect();
    assert!(mentions.is_empty(), "{events}");
}

// KA12
#[tokio::test]
async fn ten_keyless_lines_in_a_row_wait_for_someone_else() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let mut keys = Vec::new();
    for n in 0..11 {
        let key = server.keyless(n).await;
        server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
        keys.push(key);
    }
    for (n, key) in keys.iter().take(10).enumerate() {
        let page = server.fetch(&format!("/bridge/{key}/3/say/line+{n}")).await;
        assert!(page.text.contains("Said it"), "{}", page.text);
    }
    let page = server.fetch(&format!("/bridge/{}/3/say/one+more", keys[10])).await;
    assert!(page.text.contains("Refused: the last 10 lines here are all keyless"), "{}", page.text);

    // A person speaking lets them talk again.
    server
        .ok(
            &cy.key,
            Method::POST,
            &format!("/api/hangouts/{floor}/messages"),
            Some(json!({ "text": "go on" })),
        )
        .await;
    let page = server.fetch(&format!("/bridge/{}/4/say/one+more", keys[10])).await;
    assert!(page.text.contains("Said it"), "{}", page.text);
}

// KA13
#[tokio::test]
async fn a_keyless_key_is_nothing_anywhere_else() {
    let server = Server::start().await;
    let key = server.keyless(1).await;
    let (status, _) = server
        .send(
            &key,
            Method::POST,
            "/mcp",
            Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = server.send(&key, Method::GET, "/api/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = server.send(&key, Method::POST, "/api/hangouts", Some(json!({}))).await;
    assert_ne!(status, StatusCode::OK);
    // A bridge action a keyless agent doesn't have is refused before anything happens.
    let page = server.fetch(&format!("/bridge/{key}/2/use/piano")).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
}

// KA14, KA15, KA16, KA17
#[tokio::test]
async fn keyless_agents_keep_notes_on_a_board_that_takes_them() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, board) = server.lab(&cy).await;
    let question = server
        .ok(
            &cy.key,
            Method::POST,
            &format!("/api/boards/{board}/cards"),
            Some(json!({ "title": "What lowers relapse rates?", "text": "Sources welcome" })),
        )
        .await;
    let question = text(&question["id"]);
    let key = server.keyless(1).await;
    server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;

    // It reads every card on that board, from before it arrived too.
    let page = server.fetch(&format!("/bridge/{key}/3/look")).await;
    assert!(page.text.contains("What lowers relapse rates?"), "{}", page.text);

    let page = server
        .fetch(&format!("/bridge/{key}/4/answer/{question}~Look+at+trial+X%2C+unverified"))
        .await;
    assert!(page.text.contains("answering"), "{}", page.text);
    let mine =
        page.text.split("Added card ").nth(1).unwrap().split(' ').next().unwrap().to_string();
    let page =
        server.fetch(&format!("/bridge/{key}/5/edit/{mine}~Trial+X+needs+a+DOI+check")).await;
    assert!(page.text.contains("Rewrote card"), "{}", page.text);

    // Someone else's card is not its to rewrite.
    let page = server.fetch(&format!("/bridge/{key}/6/edit/{question}~mine+now")).await;
    assert!(page.text.contains("Refused"), "{}", page.text);

    // People and bots see it, marked as a keyless agent's.
    let boards =
        server.ok(&cy.key, Method::GET, &format!("/api/hangouts/{floor}/boards"), None).await;
    let listed = &boards["boards"][0];
    assert_eq!(listed["takes_keyless_cards"], true, "{boards}");
    let card = &listed["keyless_cards"][0];
    assert_eq!(card["keyless"], true, "{card}");
    assert_eq!(card["answers"], question.as_str(), "{card}");
    assert_eq!(card["text"], "Trial X needs a DOI check", "{card}");
    // The room page formats unix seconds, as for the room's own cards (tests/keyless_ui.cjs).
    assert!(card["updated_at"].is_i64(), "{card}");

    // KA16: five a day.
    for n in 7..11 {
        let page = server.fetch(&format!("/bridge/{key}/{n}/card/Note+{n}~text")).await;
        assert!(page.text.contains("Added card"), "{}", page.text);
    }
    let page = server.fetch(&format!("/bridge/{key}/11/card/Too+many~text")).await;
    assert!(page.text.contains("Refused: 5 cards a day"), "{}", page.text);

    // KA17: whoever runs the space takes one down; a stranger can't.
    let (_, di) = server.someone("Di").await;
    let (status, _) =
        server.send(&di.key, Method::DELETE, &format!("/api/keyless-cards/{mine}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    server.ok(&cy.key, Method::DELETE, &format!("/api/keyless-cards/{mine}"), None).await;
}

// KA14
#[tokio::test]
async fn a_board_that_doesnt_take_keyless_cards_is_closed_to_them() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, board) = server.lab(&cy).await;
    server
        .ok(
            &cy.key,
            Method::PUT,
            &format!("/api/boards/{board}/keyless"),
            Some(json!({ "on": false })),
        )
        .await;
    let key = server.keyless(1).await;
    server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
    let page = server.fetch(&format!("/bridge/{key}/3/card/Hi~there")).await;
    assert!(page.text.contains("Refused: no board here takes"), "{}", page.text);
}

// KA20
#[tokio::test]
async fn a_god_removes_a_keyless_agent_and_what_it_said() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let key = server.keyless(1).await;
    server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
    server.fetch(&format!("/bridge/{key}/3/say/spam+spam")).await;
    let player = server.state.db.player_by_token(&auth::hash_secret(&key)).unwrap().unwrap();
    let path = format!("/api/keyless/{}/remove", player.id);
    let (status, _) =
        server.send(&cy.key, Method::POST, &path, Some(json!({ "take_down": true }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only a god");
    server.ok(&server.god.key, Method::POST, &path, Some(json!({ "take_down": true }))).await;
    assert_eq!(server.fetch(&format!("/bridge/{key}/4/look")).await.status, StatusCode::FORBIDDEN);
    assert!(server.state.db.keyless_message_ids(&floor, 0).unwrap().is_empty());
}

// KA2, KA9, KA15 over the public MCP endpoint
#[tokio::test]
async fn an_mcp_keyless_agent_plays_the_same_way() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let joined = server.public("join", json!({})).await;
    let token = text(&joined["structuredContent"]["player_token"]);
    let floors = server.public("floors", json!({ "player_token": token })).await;
    assert_eq!(floors["structuredContent"]["floors"][0]["floor_id"], floor.as_str(), "{floors}");
    server.public("enter", json!({ "player_token": token, "floor_id": floor })).await;
    arrived().await;
    let said =
        server.public("floor_say", json!({ "player_token": token, "text": "hello lab" })).await;
    assert_eq!(said["structuredContent"]["posted"], true, "{said}");
    let card = server
        .public(
            "add_card",
            json!({ "player_token": token, "title": "Where I got to", "text": "notes" }),
        )
        .await;
    assert_eq!(card["structuredContent"]["ok"], true, "{card}");
    let read = server.public("read_floor", json!({ "player_token": token })).await;
    let read = &read["structuredContent"];
    assert_eq!(read["lines"][0]["text"], "hello lab", "{read}");
    assert_eq!(read["boards"][0]["cards"][0]["title"], "Where I got to", "{read}");

    // KA1: a bot's own player isn't a keyless agent, whatever its token can do on the map.
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover");
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "join", "arguments": {} }
    });
    let bot_join = server.ok(&rover, Method::POST, "/public/mcp", Some(call)).await;
    let bot_player = text(&bot_join["result"]["structuredContent"]["player_token"]);
    let refused = server.public("floors", json!({ "player_token": bot_player })).await;
    assert_eq!(refused["isError"], true, "{refused}");
}

#[tokio::test]
// KA7, KA14, KA16
async fn edit_after_access_revoked() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, board) = server.lab(&cy).await;
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
    let made = server.public("add_card", json!({"player_token": key, "title": "note"})).await;
    let card = text(&made["structuredContent"]["card_id"]);
    for (owner, path, off, on) in [
        (
            &cy.key,
            format!("/api/boards/{board}/keyless"),
            json!({"on": false}),
            json!({"on": true}),
        ),
        (
            &server.god.key,
            format!("/api/floors/{floor}/keyless"),
            json!({"on": false}),
            json!({"on": true}),
        ),
        (
            &cy.key,
            format!("/api/floors/{floor}"),
            json!({"access": "private"}),
            json!({"access": "public", "create_code": CODE}),
        ),
    ] {
        server.ok(owner, Method::PUT, &path, Some(off)).await;
        let edited = server
            .public(
                "edit_card",
                json!({"player_token": key, "card_id": card, "text": "changed after revocation"}),
            )
            .await;
        assert_eq!(edited["isError"], true, "{path} still editable: {edited}");
        assert_ne!(
            server.state.db.keyless_card(&card).unwrap().unwrap().body,
            "changed after revocation"
        );
        server.ok(owner, Method::PUT, &path, Some(on)).await;
        server.open_floor(&floor).await;
        server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
    }
    server.public("leave", json!({"player_token": key})).await;
    let edited = server
        .public("edit_card", json!({"player_token": key, "card_id": card, "text": "after leaving"}))
        .await;
    assert_eq!(edited["isError"], true, "cannot edit a floor after leaving: {edited}");
}

#[tokio::test]
async fn private_interval_is_not_readable_on_reopen() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
    arrived().await;
    server
        .ok(
            &cy.key,
            Method::PUT,
            &format!("/api/floors/{floor}"),
            Some(json!({"access": "private"})),
        )
        .await;
    server
        .ok(
            &cy.key,
            Method::POST,
            &format!("/api/hangouts/{floor}/messages"),
            Some(json!({"text": "private interval secret"})),
        )
        .await;
    server
        .ok(
            &cy.key,
            Method::PUT,
            &format!("/api/floors/{floor}"),
            Some(json!({"access": "public", "create_code": CODE})),
        )
        .await;
    let read = server.public("read_floor", json!({"player_token": key})).await;
    assert!(
        !read.to_string().contains("private interval secret"),
        "private history exposed: {read}"
    );
}

#[tokio::test]
async fn full_board_note_is_available_to_fetcher() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, board) = server.lab(&cy).await;
    server.ok(&cy.key, Method::POST, &format!("/api/boards/{board}/cards"), Some(json!({"title": "Question", "text": format!("{} Crucial qualification and source DOI", "x".repeat(301))}))).await;
    let key = server.keyless(1).await;
    let page = server.fetch(&format!("/bridge/{key}/2/join/{floor}")).await;
    assert!(page.text.contains("Crucial qualification"), "qualification missing from bridge board");
}

#[tokio::test]
async fn keyless_context_has_provenance() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover");
    server.all_day(&bo, &rover_id).await;
    server.tool(&rover, "go_out", json!({"floor_id": floor})).await;
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
    arrived().await;
    let said = server
        .public("floor_say", json!({"player_token": key, "text": "unverified assertion"}))
        .await;
    let id = said["structuredContent"]["message_id"].as_i64().unwrap();
    server
        .ok(
            &cy.key,
            Method::POST,
            &format!("/api/hangouts/{floor}/messages"),
            Some(json!({"text": "@Rover please inspect", "reply_to": id})),
        )
        .await;
    let events = server.tool(&rover, "events_since", json!({})).await;
    let mention =
        events["events"].as_array().unwrap().iter().find(|e| e["name"] == "mention").unwrap();
    assert_eq!(mention["data"]["root"]["keyless"], true, "unmarked root: {mention}");
    server
        .ok(
            &cy.key,
            Method::POST,
            &format!("/api/hangouts/{floor}/messages"),
            Some(json!({"text": "@Rover inspect the earlier context too"})),
        )
        .await;
    let events = server.tool(&rover, "events_since", json!({})).await;
    let latest =
        events["events"].as_array().unwrap().iter().rev().find(|e| e["name"] == "mention").unwrap();
    let earlier = latest["data"]["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|line| line["message_id"] == id)
        .unwrap();
    assert_eq!(earlier["keyless"], true, "unmarked context: {earlier}");
}

#[tokio::test]
async fn message_webhook_respects_keyless_optout() {
    use axum::{routing::post, Json, Router};
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover");
    server.all_day(&bo, &rover_id).await;
    server.tool(&rover, "go_out", json!({"floor_id": floor})).await;
    server
        .ok(
            &bo.key,
            Method::PUT,
            &format!("/api/agents/{rover_id}/keyless-mentions"),
            Some(json!({"on": false})),
        )
        .await;
    let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let received = hits.clone();
    let hook = Router::new().route(
        "/",
        post(move |Json(body): Json<Value>| {
            let received = received.clone();
            async move {
                received.lock().unwrap().push(body.clone());
                Json(json!({"challenge": body["challenge"]}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });
    let subscribed = server.ok(&rover, Method::POST, "/mcp", Some(json!({"jsonrpc":"2.0", "id": 1, "method":"events/subscribe", "params": {"name":"message", "delivery":{"mode":"webhook", "url":url, "secret":"whsec_MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="}}}))).await;
    assert!(subscribed["error"].is_null(), "subscription failed: {subscribed}");
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token":key,"floor_id":floor})).await;
    server.public("floor_say", json!({"player_token":key,"text":"@Rover optout bypass"})).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let received = hits.lock().unwrap();
    assert!(
        !received.iter().any(|hit| hit["data"]["body"] == "@Rover optout bypass"),
        "keyless text delivered by webhook despite optout: {received:?}"
    );
}

#[tokio::test]
async fn parallel_says_obey_twenty_second_gap() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token":key,"floor_id":floor})).await;
    let gate = std::sync::Arc::new(std::sync::Barrier::new(24));
    let threads: Vec<_> = (0..24)
        .map(|n| {
            let state = server.state.clone();
            let key = key.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let agent = agenthangout::keyless::from_key(&state, &key).unwrap().unwrap();
                gate.wait();
                agenthangout::keyless::say(&state, &agent, &format!("parallel {n}"), None).is_ok()
            })
        })
        .collect();
    let posted =
        threads.into_iter().map(|thread| usize::from(thread.join().unwrap())).sum::<usize>();
    assert_eq!(posted, 1, "concurrent calls exceeded one line per 20s");
}

// KA5, KA8: activity cannot extend a URL credential past its replay window.
#[tokio::test]
async fn active_key_expires_before_old_turns_can_be_replayed() {
    let server = Server::start().await;
    let key = server.keyless(1).await;
    let player = server.state.db.player_by_token(&auth::hash_secret(&key)).unwrap().unwrap();
    for day in 1..7 {
        assert!(worlds::keyless_agent(&server.state, &key, player.created_at + day * DAY)
            .unwrap()
            .is_some());
    }
    assert!(worlds::keyless_agent(&server.state, &key, player.created_at + 7 * DAY)
        .unwrap()
        .is_none());
    assert!(!server
        .state
        .db
        .take_keyless_turn(player.id, "1", player.created_at + 7 * DAY)
        .unwrap());
}

// KA9: the hourly cap is independent of the 20-second gap and the loop stop.
#[tokio::test]
async fn sixty_lines_in_an_hour_are_refused() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let key = server.keyless(1).await;
    server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
    let player = server.state.db.player_by_token(&auth::hash_secret(&key)).unwrap().unwrap();
    for n in 0..60 {
        let line = server
            .state
            .db
            .insert_message(&agenthangout::db::NewMessage {
                hangout_id: &floor,
                person_id: None,
                agent_id: None,
                speaker: "Guest",
                kind: "chat",
                body: "earlier",
                now: now() - 60 - n * 21,
            })
            .unwrap();
        server.state.db.mark_keyless_message(line.id, player.id).unwrap();
    }
    let refused =
        server.public("floor_say", json!({"player_token": key, "text": "one more"})).await;
    assert!(refused.to_string().contains("60 lines an hour"), "{refused}");
}

// KA11: every delivery path uses both the sender and floor mention budgets.
#[tokio::test]
async fn mention_caps_limit_message_events_too() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, _) = server.lab(&cy).await;
    let bots: Vec<_> =
        (0..11).map(|n| server.bot(&cy, &floor, &format!("Researcher {n}"))).collect();
    let words = (0..11).map(|n| format!("@Researcher {n}")).collect::<Vec<_>>().join(", ");
    let mut reached = 0;
    for n in 1..=4 {
        let key = server.keyless(n).await;
        server.public("enter", json!({"player_token": key, "floor_id": floor})).await;
        let said = server.public("floor_say", json!({"player_token": key, "text": words})).await;
        let data = &said["structuredContent"];
        let count = data["reached"].as_array().unwrap().len();
        assert_eq!(count, if n <= 3 { 10 } else { 0 }, "{said}");
        reached += count;
    }
    assert_eq!(reached, 30);
    let mut message_events = 0;
    let mut mention_events = 0;
    for (_, token) in bots {
        let events = server.tool(&token, "events_since", json!({"since": "0"})).await;
        for event in events["events"].as_array().unwrap() {
            if event["data"]["keyless"] == true {
                match event["name"].as_str() {
                    Some("message") => message_events += 1,
                    Some("mention") => mention_events += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!(message_events, 30);
    assert_eq!(mention_events, 30);
}

// KA16: simultaneous authors cannot overfill a board or exceed their daily card budgets.
#[tokio::test]
async fn concurrent_cards_keep_the_board_and_author_caps() {
    let server = Server::start().await;
    let (_, cy) = server.someone("Cy").await;
    let (floor, board) = server.lab(&cy).await;
    let mut agents = Vec::new();
    for n in 1..=8 {
        let key = server.keyless(n).await;
        server.public("enter", json!({"player_token":key,"floor_id":floor})).await;
        agents.push(agenthangout::keyless::from_key(&server.state, &key).unwrap().unwrap());
    }
    let gate = std::sync::Arc::new(std::sync::Barrier::new(agents.len()));
    let threads: Vec<_> = agents
        .into_iter()
        .map(|agent| {
            let state = server.state.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                let added = (0..6)
                    .filter(|_| {
                        agenthangout::keyless::add_card(
                            &state, &agent, None, "Note", "Research", None,
                        )
                        .is_ok()
                    })
                    .count();
                assert!(added <= 5);
                added
            })
        })
        .collect();
    assert_eq!(threads.into_iter().map(|t| t.join().unwrap()).sum::<usize>(), 30);
    assert_eq!(server.state.db.keyless_cards_on(&board).unwrap().len(), 30);
}
