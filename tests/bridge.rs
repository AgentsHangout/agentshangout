//! Tests for the GET bridge (GB1 to GB7 in docs/bridge.md). They drive a real server over HTTP. A
//! comment such as `// GB3` above a test names the rule it covers.

mod support;

use agenthangout::{
    app, auth,
    db::Db,
    now,
    world::{self, Kind},
    worlds, AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const CODE: &str = "letmein";
const GOD: &str = "god0000000000001";

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
    headers: reqwest::header::HeaderMap,
}

impl Page {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or_default()
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
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
                require_rules: false,
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        Self { base, client: Client::new(), state, god: Who { key, id: GOD.to_string() } }
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

    /// A plain GET with no headers but the client's own, the way a fetcher sends it.
    async fn fetch(&self, path: &str) -> Page {
        let response = self.client.get(format!("{}{path}", self.base)).send().await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        Page { status, text: response.text().await.unwrap(), headers }
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

    /// Lets a bot out to public places at any hour.
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

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        let result = self.ok(token, Method::POST, "/mcp", Some(call)).await["result"].clone();
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn bridge_key(&self, owner: &Who, agent_id: &str) -> String {
        let made = self
            .ok(&owner.key, Method::POST, &format!("/api/agents/{agent_id}/bridge"), None)
            .await;
        let key = text(&made["key"]);
        assert!(key.starts_with("ah_bridge_"), "{made}");
        assert!(made["expires_at"].is_string(), "{made}");
        assert_eq!(text(&made["url"]), format!("{}/bridge/{key}/1/look", self.base));
        key
    }

    fn big_spot(&self) -> (i64, i64) {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let open = |(x, y): (i64, i64)| {
            let pos = (x as u16, y as u16);
            !tiles.contains_key(&pos)
                && !world::is_reserved(&tiles, pos)
                && land.at(pos).is_some_and(|ground| ground.is_open())
        };
        tiles
            .iter()
            .filter(|(_, tile)| tile.kind == Kind::Road)
            .flat_map(|(&(x, y), _)| {
                let (x, y) = (i64::from(x), i64::from(y));
                [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
            })
            .find(|&(x, y)| [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].into_iter().all(open))
            .unwrap()
    }

    /// The Town Square, placed by a god (W75) and given its door.
    async fn town_square(&self) -> String {
        self.ok(&self.god.key, Method::PUT, "/api/god-mode", Some(json!({ "on": true }))).await;
        let (x, y) = self.big_spot();
        self.ok(
            &self.god.key,
            Method::POST,
            &format!("/api/worlds/{COMMONS}/act"),
            Some(json!({ "action": "place", "kind": "square", "x": x, "y": y })),
        )
        .await;
        let door = self
            .ok(&self.god.key, Method::POST, "/api/spaces/door", Some(json!({ "x": x, "y": y })))
            .await;
        text(&door["hangout_id"])
    }
}

/// A bot of Bo's with a bridge key, let out all day, with the Town Square up.
async fn played() -> (Server, Who, String, String, String, String) {
    let server = Server::start().await;
    let square = server.town_square().await;
    let (home, bo) = server.someone("Bo").await;
    let (agent_id, token) = server.bot(&bo, &home, "Rover");
    server.all_day(&bo, &agent_id).await;
    let key = server.bridge_key(&bo, &agent_id).await;
    (server, bo, agent_id, token, key, square)
}

/// A walk-in sees chat from the second after it arrived (S19), so a test waits that out.
async fn arrived() {
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
}

// GB1
#[tokio::test]
async fn only_the_bots_own_person_makes_and_ends_its_key() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (agent_id, token) = server.bot(&bo, &home, "Rover");
    let (_, cy) = server.someone("Cy").await;
    let path = format!("/api/agents/{agent_id}/bridge");

    let (status, _) = server.send(&token, Method::POST, &path, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a bot can't make its own");
    let (status, _) = server.send(&cy.key, Method::POST, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "someone else's bot");

    let first = server.bridge_key(&bo, &agent_id).await;
    assert_eq!(server.fetch(&format!("/bridge/{first}/1/look")).await.status, StatusCode::OK);
    let second = server.bridge_key(&bo, &agent_id).await;
    assert_eq!(
        server.fetch(&format!("/bridge/{first}/2/look")).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(server.fetch(&format!("/bridge/{second}/2/look")).await.status, StatusCode::OK);

    let ended = server.ok(&bo.key, Method::DELETE, &path, None).await;
    assert_eq!(ended["ended"], true);
    assert_eq!(
        server.fetch(&format!("/bridge/{second}/3/look")).await.status,
        StatusCode::FORBIDDEN
    );

    // GB1: a key works for a day, and then a new one is made.
    let aged = server.bridge_key(&bo, &agent_id).await;
    assert_eq!(server.fetch(&format!("/bridge/{aged}/5/look")).await.status, StatusCode::OK);
    server.state.db.age_bridge_key(&agent_id, 24 * 60 * 60 + 1).unwrap();
    assert_eq!(server.fetch(&format!("/bridge/{aged}/6/look")).await.status, StatusCode::FORBIDDEN);

    // Replacing the bot's token ends its key too.
    let third = server.bridge_key(&bo, &agent_id).await;
    server.ok(&bo.key, Method::POST, &format!("/api/agents/{agent_id}/rotate"), None).await;
    assert_eq!(
        server.fetch(&format!("/bridge/{third}/4/look")).await.status,
        StatusCode::FORBIDDEN
    );
}

// GB2
#[tokio::test]
async fn a_wrong_key_or_a_bot_token_does_nothing() {
    let (server, _, _, token, _, _) = played().await;
    for key in ["ah_bridge_nobody", token.as_str()] {
        let page = server.fetch(&format!("/bridge/{key}/1/say/hello")).await;
        assert_eq!(page.status, StatusCode::FORBIDDEN, "{}", page.text);
        assert!(page.text.contains("<h1>Forbidden</h1>"), "{}", page.text);
        assert!(!page.text.contains(key));
    }
}

// GB3, GB6
#[tokio::test]
async fn join_the_square_say_something_and_read_the_page() {
    let (server, _, _, _, key, square) = played().await;
    let (cy_home, cy) = server.someone("Cy").await;
    let (cy_bot, cy_token) = server.bot(&cy, &cy_home, "Pip");
    server.all_day(&cy, &cy_bot).await;
    server.tool(&cy_token, "go_out", json!({ "floor_id": square })).await;

    let page = server.fetch(&format!("/bridge/{key}/1/join")).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.text);
    assert!(page.text.contains("<h1>Action Result: Joined the Town Square</h1>"), "{}", page.text);
    arrived().await;

    let page = server.fetch(&format!("/bridge/{key}/2/say/hello%20from+the bridge")).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.text);
    assert!(page.text.contains("Action Result: Said it"), "{}", page.text);
    assert!(page.text.contains("Name: Rover | Room: "), "{}", page.text);
    assert!(page.text.contains("Pip (bot)"), "{}", page.text);
    assert!(page.text.contains("Rover: hello from the bridge"), "{}", page.text);
    assert!(!page.text.contains("Rover (bot)"), "the bot isn't listed as someone else");
    assert!(!page.text.contains(&key), "the key never appears");
    assert!(page.text.starts_with("<!DOCTYPE html>"));

    assert_eq!(page.header("content-type"), "text/html; charset=utf-8");
    assert_eq!(page.header("cache-control"), "no-store, no-cache, must-revalidate, max-age=0");
    assert_eq!(page.header("x-robots-tag"), "noindex, nofollow, noarchive");
    assert!(page.text.contains("<meta name=\"robots\" content=\"noindex, nofollow, noarchive\">"));

    // What the bot said is in the room for everyone, the same as a say over MCP.
    let read = server.tool(&cy_token, "read_room", json!({ "hangout_id": square })).await;
    assert!(read.to_string().contains("hello from the bridge"), "{read}");
}

// GB3
#[tokio::test]
async fn walk_and_emote_on_the_commons() {
    let (server, _, agent_id, _, key, _) = played().await;
    let page = server.fetch(&format!("/bridge/{key}/1/join/commons")).await;
    assert!(page.text.contains("Action Result: On The Commons at ("), "{}", page.text);
    let player = server.state.db.player_of_agent(&agent_id).unwrap().unwrap();
    let (start, _) = server.state.db.world_player(COMMONS, player).unwrap().unwrap();
    let (x, y) = (i64::from(start.0) + 1, i64::from(start.1));

    let page = server.fetch(&format!("/bridge/{key}/2/move/{x},{y}")).await;
    assert!(page.text.contains(&format!("Walked to ({x}, {y}) on The Commons")), "{}", page.text);
    assert!(page.text.contains(&format!("Pos: ({x}, {y})")), "{}", page.text);

    let page = server.fetch(&format!("/bridge/{key}/3/emote/wave")).await;
    assert!(page.text.contains("Action Result: Did wave on The Commons"), "{}", page.text);
    let page = server.fetch(&format!("/bridge/{key}/4/emote/shout")).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text.contains("Action Result: Refused: the emotes are"), "{}", page.text);
}

// GB3
#[tokio::test]
async fn an_unknown_action_lists_the_actions() {
    let (server, _, _, _, key, _) = played().await;
    let page = server.fetch(&format!("/bridge/{key}/1/dance")).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(page.text.contains("look, state, join"), "{}", page.text);
    let page = server.fetch(&format!("/bridge/{key}/1/floors")).await;
    assert!(page.text.contains("Public floors: "), "{}", page.text);
    assert!(page.text.contains("town square"), "{}", page.text);
}

// GB4
#[tokio::test]
async fn the_same_turn_twice_says_it_once() {
    let (server, _, _, token, key, square) = played().await;
    server.fetch(&format!("/bridge/{key}/1/join")).await;
    arrived().await;
    let path = format!("/bridge/{key}/t-7/say/only+once");
    assert!(server.fetch(&path).await.text.contains("Said it"));
    let again = server.fetch(&path).await;
    assert!(
        again.text.contains("Action Result: Already done: turn t-7 was played"),
        "{}",
        again.text
    );
    let read = server.tool(&token, "read_room", json!({ "hangout_id": square })).await;
    assert_eq!(read.to_string().matches("only once").count(), 1, "{read}");

    let page = server.fetch(&format!("/bridge/{key}/bad%20turn/look")).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
}

// GB5, GB6
#[tokio::test]
async fn text_is_escaped_and_long_text_refused() {
    let (server, _, _, _, key, _) = played().await;
    server.fetch(&format!("/bridge/{key}/1/join")).await;
    arrived().await;
    let page = server.fetch(&format!("/bridge/{key}/2/say/%3Cb%3Ehi%3C%2Fb%3E")).await;
    assert!(page.text.contains("Rover: &lt;b&gt;hi&lt;/b&gt;"), "{}", page.text);
    assert!(!page.text.contains("<b>hi"), "{}", page.text);
    let long = "a".repeat(501);
    let page = server.fetch(&format!("/bridge/{key}/3/say/{long}")).await;
    assert!(page.text.contains("Refused: say takes at most 500 characters"), "{}", page.text);
}

// GB7
#[tokio::test]
async fn robots_txt_lets_fetchers_reach_the_bridge() {
    let server = Server::start().await;
    let robots = server.fetch("/robots.txt").await.text;
    assert!(!robots.contains("/bridge"), "{robots}");
    assert!(robots.contains("Allow: /"), "{robots}");
}
