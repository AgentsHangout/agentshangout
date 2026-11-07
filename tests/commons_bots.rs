//! Tests for the public Commons bots list (W100 in docs/public-city.md). They drive a real server
//! over HTTP. A comment such as `// W100` above a test names the rule it covers.

mod support;

use std::time::Duration;

use agenthangout::{
    app, auth,
    db::Db,
    now,
    world::{self, Kind},
    worlds::{self, Move, COMMONS_BOTS_PER_IP_PER_MINUTE, DEFAULT_COMMONS_VIEWER_ORIGIN},
    AppState, Config,
};
use axum::http::{Method, StatusCode};
use reqwest::Client;
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const CODE: &str = "letmein";
const DAY: i64 = 86_400;
const GOD: &str = "god0000000000001";
const BOTS: &str = "/api/worlds/commons/bots";
const PAGES: &str = DEFAULT_COMMONS_VIEWER_ORIGIN;

struct Server {
    base: String,
    client: Client,
    state: AppState,
    god: Who,
}

struct Reply {
    status: StatusCode,
    body: Value,
    text: String,
    headers: reqwest::header::HeaderMap,
}

impl Reply {
    fn show(&self) -> String {
        format!("{} {}", self.status, self.text)
    }

    #[track_caller]
    fn ok(self) -> Value {
        assert_eq!(self.status, StatusCode::OK, "{}", self.show());
        self.body
    }
}

#[derive(Clone)]
struct Who {
    key: String,
    id: String,
}

struct Req<'a> {
    server: &'a Server,
    method: Method,
    path: String,
    token: Option<String>,
    headers: Vec<(String, String)>,
    body: Option<Value>,
}

impl<'a> Req<'a> {
    fn token(mut self, token: &str) -> Self {
        self.token = Some(token.to_string());
        self
    }

    fn ip(self, ip: &str) -> Self {
        self.header("x-forwarded-for", ip)
    }

    fn origin(self, origin: &str) -> Self {
        self.header("origin", origin)
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    async fn send(self) -> Reply {
        let url = format!("{}{}", self.server.base, self.path);
        let mut request = self.server.client.request(self.method, url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        if let Some(body) = &self.body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Reply { status, body, text, headers }
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

fn names(body: &Value) -> Vec<String> {
    body["bots"].as_array().unwrap().iter().map(|bot| text(&bot["name"])).collect()
}

fn pin(body: &Value, name: &str) -> Option<(i64, i64)> {
    body["bots"]
        .as_array()?
        .iter()
        .find(|bot| bot["name"] == name)
        .map(|bot| (bot["tile"][0].as_i64().unwrap(), bot["tile"][1].as_i64().unwrap()))
}

fn key(body: &Value, name: &str) -> String {
    let bots = body["bots"].as_array().unwrap();
    text(&bots.iter().find(|bot| bot["name"] == name).unwrap_or_else(|| panic!("{body}"))["key"])
}

fn header<'a>(reply: &'a Reply, name: &'a str) -> Option<&'a str> {
    reply.headers.get(name).and_then(|value| value.to_str().ok())
}

/// A SQLite file under the temp dir. Drop removes it and its WAL sidecars, including if a test panics.
struct TempDb {
    path: String,
}

impl TempDb {
    fn create() -> Self {
        let path = support::db_path("commons-bots").to_string_lossy().into_owned();
        Self { path }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path));
        }
    }
}

/// The limits count by the clock's minute, so a test that counts what one of them lets through
/// starts with plenty of it left.
async fn start_early_in(period: i64, room: i64) {
    let left = period - now().rem_euclid(period);
    if left < room {
        tokio::time::sleep(Duration::from_secs(left as u64 + 1)).await;
    }
}

impl Server {
    async fn start() -> Self {
        Self::boot(Db::open(":memory:").unwrap()).await
    }

    /// A file database, so a test can rewrite a place's kind the way a later deploy would.
    /// The file is deleted when the returned guard is dropped, including if the test panics.
    /// Bind the guard first so it outlives the server: locals drop in reverse declaration
    /// order, and `let (server, db)` would drop the file while the server still holds it.
    async fn start_file() -> (Self, TempDb) {
        let db = TempDb::create();
        (Self::boot(Db::open(&db.path).unwrap()).await, db)
    }

    async fn boot(db: Db) -> Self {
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

    fn req(&self, method: Method, path: &str) -> Req<'_> {
        Req {
            server: self,
            method,
            path: path.to_string(),
            token: None,
            headers: Vec::new(),
            body: None,
        }
    }

    fn get(&self, path: &str) -> Req<'_> {
        self.req(Method::GET, path)
    }

    async fn send(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Reply {
        let mut request = self.req(method, path);
        if !token.is_empty() {
            request = request.token(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        request.send().await
    }

    async fn get_json(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::GET, path, None).await
    }

    async fn post_json(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::POST, path, Some(body)).await
    }

    async fn put_json(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::PUT, path, Some(body)).await
    }

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post_json(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (
            text(&made["hangout"]["id"]),
            Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) },
        )
    }

    async fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        (agent.id, token)
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

    fn big_spots(&self) -> Vec<(i64, i64)> {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let open = |(x, y): (i64, i64)| {
            let pos = (x as u16, y as u16);
            !tiles.contains_key(&pos)
                && !world::is_reserved(&tiles, pos)
                && land.at(pos).is_some_and(|ground| ground.is_open())
        };
        self.free_spots()
            .into_iter()
            .filter(|&(x, y)| [(x + 1, y), (x, y + 1), (x + 1, y + 1)].into_iter().all(open))
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

    async fn all_day(&self, owner: &Who, token: &str, public_places: bool) {
        let agent_id = text(&self.get_json(token, "/api/me").await.ok()["agent_id"]);
        let body = json!({
            "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
            "starts": "00:00",
            "ends": "24:00",
            "public_places": public_places
        });
        self.put_json(&owner.key, &format!("/api/agents/{agent_id}/routine"), body).await.ok();
    }

    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        self.post_json(token, "/mcp", call).await.ok()["result"].clone()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result = self.call(token, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn open(&self, owner: &Who, floor: &str) {
        self.put_json(
            &owner.key,
            &format!("/api/floors/{floor}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();
    }

    async fn door(&self, owner: &Who, (x, y): (i64, i64)) -> String {
        text(
            &self.post_json(&owner.key, "/api/spaces/door", json!({ "x": x, "y": y })).await.ok()
                ["hangout_id"],
        )
    }

    /// A public place on The Commons, after a house so the place has come (W61).
    async fn public_place(&self, owner: &Who, kind: &str) -> ((i64, i64), String) {
        self.place_at(owner, "house", now() - 8 * DAY);
        let spot = self.place_at(owner, kind, now());
        let floor = self.door(owner, spot).await;
        self.open(owner, &floor).await;
        (spot, floor)
    }

    /// A public café on The Commons, after a house so the café has come (W61).
    async fn public_cafe(&self, owner: &Who) -> ((i64, i64), String) {
        self.public_place(owner, "cafe").await
    }

    async fn town_square(&self) -> ((i64, i64), String) {
        self.put_json(&self.god.key, "/api/god-mode", json!({ "on": true })).await.ok();
        let (x, y) = self.big_spots()[0];
        self.post_json(
            &self.god.key,
            &format!("/api/worlds/{COMMONS}/act"),
            json!({ "action": "place", "kind": "square", "x": x, "y": y }),
        )
        .await
        .ok();
        let floor = self.door(&self.god, (x, y)).await;
        ((x, y), floor)
    }

    async fn bots(&self) -> Value {
        self.get(BOTS).send().await.ok()
    }

    /// A read from this address that runs on its own, for while a rebuild is held.
    fn read_aside(&self, ip: &str) -> tokio::task::JoinHandle<Value> {
        let client = self.client.clone();
        let url = format!("{}{BOTS}", self.base);
        let ip = ip.to_string();
        tokio::spawn(async move {
            let response = client.get(&url).header("x-forwarded-for", ip).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_str(&response.text().await.unwrap()).unwrap()
        })
    }

    /// Until a read has started this many rebuilds since the server started.
    async fn wait_for_builds(&self, builds: u64) {
        for _ in 0..500 {
            if self.state.worlds.commons_bots_builds() >= builds {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no rebuild started");
    }
}

fn assert_shape(body: &Value) {
    assert_eq!(body["cache_seconds"], worlds::COMMONS_BOTS_CACHE_SECS, "{body}");
    let as_of = text(&body["as_of"]);
    assert!(
        as_of.contains('T') && (as_of.ends_with('Z') || as_of.ends_with("+00:00")),
        "RFC 3339 UTC: {as_of}"
    );
    assert!(body.get("truncated").is_none(), "{body}");
    for bot in body["bots"].as_array().unwrap() {
        let mut keys: Vec<&str> = bot.as_object().unwrap().keys().map(|key| key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["key", "name", "tile"], "{bot}");
        assert!(bot["tile"].as_array().unwrap().len() == 2, "{bot}");
    }
}

// W100
#[tokio::test]
async fn a_bot_on_a_public_cafe_is_listed_by_name_and_place_tile() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (cafe, floor) = server.public_cafe(&cy).await;

    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;
    let body = server.bots().await;
    assert_shape(&body);
    assert_eq!(names(&body), vec!["Rover".to_string()], "{body}");
    assert_eq!(pin(&body, "Rover"), Some(cafe), "{body}");

    // The pin is the place, not the door-adjacent road L17 walks to.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let view = server.get_json("", &format!("/api/worlds/{COMMONS}")).await.ok();
    let row = &view["bots_out"][0];
    assert_eq!(row[0], "Rover");
    assert_eq!(row[8], json!([cafe.0, cafe.1]), "{row}");
    assert_eq!(body["bots"][0]["tile"], json!([cafe.0, cafe.1]));
}

// W100
#[tokio::test]
async fn private_floors_town_square_houses_porches_people_and_guests_are_excluded() {
    let server = Server::start().await;

    let (bo_home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &bo_home, "Rover").await;
    let (ann_home, ann) = server.someone("Ann").await;
    let (fern_id, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (scout_home, scout_owner) = server.someone("Pat").await;
    let (_, scout) = server.bot(&scout_owner, &scout_home, "Scout").await;
    let (pip_home, dee) = server.someone("Dee").await;
    let (_, pip) = server.bot(&dee, &pip_home, "Pip").await;

    let (_, cy) = server.someone("Cy").await;
    let (cafe, cafe_floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": cafe_floor })).await;

    // A private café Fern belongs to, in her hours, is still private.
    let (_, other) = server.someone("Other").await;
    other_place_and_keep_private(&server, &other, &ann, &fern_id, &fern).await;

    // Town Square is public and L17 shows a bot there; this list does not.
    let (_, square) = server.town_square().await;
    server.all_day(&scout_owner, &scout, true).await;
    server.tool(&scout, "go_out", json!({ "floor_id": square })).await;

    // A house on The Commons, even with its floor opened.
    let house = server.place_at(&dee, "house", now());
    let house_floor = server.door(&dee, house).await;
    server.open(&dee, &house_floor).await;
    server.all_day(&dee, &pip, true).await;
    server.tool(&pip, "go_out", json!({ "floor_id": house_floor })).await;

    // A bot on a private-city porch.
    let elm = server
        .post_json(
            "",
            "/api/cities",
            json!({ "name": "Elm", "your_name": "Fay", "create_code": CODE }),
        )
        .await
        .ok();
    let fay = Who { key: text(&elm["owner_key"]), id: text(&elm["person"]["id"]) };
    let city = text(&elm["city"]["id"]);
    let built = server
        .post_json(&fay.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Fay House" }))
        .await
        .ok();
    let (ivy_id, ivy) = server.bot(&fay, &text(&built["living_room_id"]), "Ivy").await;
    let porch = text(&built["porch_id"]);
    server
        .put_json(&fay.key, &format!("/api/hangouts/{porch}/agents/{ivy_id}"), json!({}))
        .await
        .ok();
    server.all_day(&fay, &ivy, true).await;
    server.tool(&ivy, "go_to_room", json!({ "hangout_id": porch })).await;

    // A person walking onto the public café, and a guest waving on the street.
    let (_, sam) = server.someone("Sam").await;
    server.post_json(&sam.key, &format!("/api/floors/{cafe_floor}/walk-in"), json!({})).await.ok();
    let guest = server.post_json("", "/api/public/join", json!({})).await.ok();
    server
        .post_json(
            &text(&guest["player_token"]),
            &format!("/api/worlds/{COMMONS}/act"),
            json!({ "action": "emote", "emote": "wave" }),
        )
        .await
        .ok();

    let body = server.bots().await;
    assert_shape(&body);
    assert_eq!(names(&body), vec!["Rover".to_string()], "{body}");
    assert_eq!(pin(&body, "Rover"), Some(cafe), "{body}");
    let guest_name = text(&guest["name"]);
    for name in ["Fern", "Scout", "Pip", "Ivy", "Sam", "Cy", "Dee", "Fay", guest_name.as_str()] {
        assert!(!names(&body).iter().any(|shown| shown == name), "{name} in {body}");
    }
}

async fn other_place_and_keep_private(
    server: &Server,
    owner: &Who,
    ann: &Who,
    fern_id: &str,
    fern: &str,
) {
    server.place_at(owner, "house", now() - 8 * DAY);
    let spot = server.place_at(owner, "cafe", now());
    let floor = server.door(owner, spot).await;
    let invite = server.get_json(&owner.key, &format!("/api/hangouts/{floor}")).await.ok();
    let code = text(&invite["hangout"]["invite_code"]);
    server.post_json(&ann.key, &format!("/api/invites/{code}/join"), json!({})).await.ok();
    server
        .put_json(&ann.key, &format!("/api/hangouts/{floor}/agents/{fern_id}"), json!({}))
        .await
        .ok();
    server.all_day(ann, fern, true).await;
    server.tool(fern, "go_to_room", json!({ "hangout_id": floor })).await;
}

async fn bot_on_porch(server: &Server, city_name: &str, owner_name: &str, bot_name: &str) {
    let city = server
        .post_json(
            "",
            "/api/cities",
            json!({ "name": city_name, "your_name": owner_name, "create_code": CODE }),
        )
        .await
        .ok();
    let owner = Who { key: text(&city["owner_key"]), id: text(&city["person"]["id"]) };
    let city_id = text(&city["city"]["id"]);
    let built = server
        .post_json(
            &owner.key,
            &format!("/api/cities/{city_id}/houses"),
            json!({ "name": format!("{owner_name} House") }),
        )
        .await
        .ok();
    let (bot_id, token) = server.bot(&owner, &text(&built["living_room_id"]), bot_name).await;
    let porch = text(&built["porch_id"]);
    server
        .put_json(&owner.key, &format!("/api/hangouts/{porch}/agents/{bot_id}"), json!({}))
        .await
        .ok();
    server.all_day(&owner, &token, true).await;
    server.tool(&token, "go_to_room", json!({ "hangout_id": porch })).await;
}

async fn bot_on_a_trip(server: &Server, bot_name: &str) {
    let made = server
        .post_json(
            "",
            "/api/cities",
            json!({ "name": "Ash", "your_name": "Gil", "create_code": CODE }),
        )
        .await
        .ok();
    let gil = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
    let city = text(&made["city"]["id"]);
    let code = text(&made["invite_url"]).rsplit('/').next().unwrap().to_string();
    let homes = format!("/api/cities/{city}/houses");
    let own = server.post_json(&gil.key, &homes, json!({ "name": "Gil House" })).await.ok();
    let joined = server
        .post_json("", &format!("/api/city-invites/{code}/join"), json!({ "your_name": "Jo" }))
        .await
        .ok();
    let jo = Who { key: text(&joined["owner_key"]), id: text(&joined["person"]["id"]) };
    let host = server.post_json(&jo.key, &homes, json!({ "name": "Jo House" })).await.ok();
    let (bot_id, token) = server.bot(&gil, &text(&own["living_room_id"]), bot_name).await;
    server.all_day(&gil, &token, true).await;
    let trip = text(
        &server
            .post_json(
                &gil.key,
                "/api/trips",
                json!({ "agent_id": bot_id, "house_id": text(&host["house"]["id"]), "kind": "call" }),
            )
            .await
            .ok()["id"],
    );
    server
        .post_json(&jo.key, &format!("/api/trips/{trip}/answer"), json!({ "response": "yes" }))
        .await
        .ok();
    assert_eq!(
        server.get_json(&jo.key, &format!("/api/trips/{trip}")).await.ok()["status"],
        "here",
        "the visitor should be on the trip"
    );
}

/// A later kind the current binary does not know: `Kind::parse` drops it, so the tile is missing
/// and the allow-list never sees it. Newer places have no CHECK on `kind`; buildings do.
fn disguise_place(path: &str, (x, y): (i64, i64)) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    let n = conn
        .execute(
            "UPDATE world_more_places SET kind = 'zzz-new' WHERE world_id = ?1 AND x = ?2 AND y = ?3",
            rusqlite::params![COMMONS, x, y],
        )
        .unwrap();
    assert_eq!(n, 1, "the library at ({x},{y}) should be a newer place");
}

fn assert_not_listed(body: &Value, hidden: &[&str]) {
    for name in hidden {
        assert!(pin(body, name).is_none(), "{name} in {body}");
    }
}

// W100
#[tokio::test]
async fn hidden_location_kinds_never_appear() {
    // Guard first: a tuple pattern would drop it before the server, and on Windows the
    // delete then fails silently with the SQLite file still held.
    let started = Server::start_file().await;
    let db = started.1;
    let server = started.0;

    let (bo_home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &bo_home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (cafe, cafe_floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": cafe_floor })).await;

    // Private floor: a café Fern belongs to, never opened.
    let (ann_home, ann) = server.someone("Ann").await;
    let (fern_id, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (_, other) = server.someone("Other").await;
    other_place_and_keep_private(&server, &other, &ann, &fern_id, &fern).await;

    // Town Square is public and L17 shows a bot there; this list does not.
    let (scout_home, scout_owner) = server.someone("Pat").await;
    let (_, scout) = server.bot(&scout_owner, &scout_home, "Scout").await;
    let (_, square) = server.town_square().await;
    server.all_day(&scout_owner, &scout, true).await;
    server.tool(&scout, "go_out", json!({ "floor_id": square })).await;

    // Porch of a private-city house.
    bot_on_porch(&server, "Elm", "Fay", "Ivy").await;

    // Private room: the hangout someone() made, never gone out.
    let (oak_room, lea) = server.someone("Lea").await;
    let (_, oak) = server.bot(&lea, &oak_room, "Oak").await;
    server.all_day(&lea, &oak, true).await;
    server.tool(&oak, "go_to_room", json!({ "hangout_id": oak_room })).await;

    // Trip: on the host's porch after both sides said yes.
    bot_on_a_trip(&server, "Visitor").await;

    // A known public library, then rewritten as a kind this binary does not know.
    let (newt_home, ned) = server.someone("Ned").await;
    let (_, newt) = server.bot(&ned, &newt_home, "Newt").await;
    let (_, kit) = server.someone("Kit").await;
    let (library, library_floor) = server.public_place(&kit, "library").await;
    server.all_day(&ned, &newt, true).await;
    server.tool(&newt, "go_out", json!({ "floor_id": library_floor })).await;

    let hidden = ["Fern", "Scout", "Ivy", "Oak", "Visitor"];
    let before = server.bots().await;
    assert_shape(&before);
    let mut shown = names(&before);
    shown.sort();
    assert_eq!(shown, vec!["Newt".to_string(), "Rover".to_string()], "{before}");
    assert_eq!(pin(&before, "Rover"), Some(cafe), "{before}");
    assert_eq!(pin(&before, "Newt"), Some(library), "{before}");
    assert_not_listed(&before, &hidden);

    disguise_place(&db.path, library);
    server.state.worlds.expire_commons_bots();

    let after = server.bots().await;
    assert_shape(&after);
    assert_eq!(names(&after), vec!["Rover".to_string()], "{after}");
    assert_eq!(pin(&after, "Rover"), Some(cafe), "{after}");
    assert_not_listed(&after, &hidden);
    assert!(pin(&after, "Newt").is_none(), "unknown kind in {after}");
}

// W100
#[tokio::test]
async fn each_bot_has_a_key_that_lasts_between_reads_and_is_not_its_id() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover").await;
    let (ann_home, ann) = server.someone("Ann").await;
    let (fern_id, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    for (owner, token) in [(&bo, &rover), (&ann, &fern)] {
        server.all_day(owner, token, true).await;
        server.tool(token, "go_out", json!({ "floor_id": floor })).await;
    }

    let first = server.bots().await;
    assert_shape(&first);
    // Built again rather than served from memory, the keys are the same.
    server.state.worlds.expire_commons_bots();
    let again = server.bots().await;
    assert_eq!(key(&again, "Rover"), key(&first, "Rover"), "{first} then {again}");
    assert_eq!(key(&again, "Fern"), key(&first, "Fern"), "{first} then {again}");
    assert_ne!(key(&first, "Rover"), key(&first, "Fern"), "{first}");
    for id in [&rover_id, &fern_id] {
        assert!(!first.to_string().contains(id.as_str()), "{id} in {first}");
    }
}

// W100
#[tokio::test]
async fn a_list_cut_short_by_the_row_limit_says_truncated() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (ann_home, ann) = server.someone("Ann").await;
    let (_, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    for (owner, token) in [(&bo, &rover), (&ann, &fern)] {
        server.all_day(owner, token, true).await;
        server.tool(token, "go_out", json!({ "floor_id": floor })).await;
    }

    // Reading one row of at least two, the database stops before every bot is looked at.
    server.state.worlds.set_place_rows(1);
    server.state.worlds.expire_commons_bots();
    let cut = server.bots().await;
    assert_eq!(cut["truncated"], true, "{cut}");
    assert!(names(&cut).len() < 2, "{cut}");

    server.state.worlds.set_place_rows(worlds::BOTS_AT_PLACES_ROWS);
    server.state.worlds.expire_commons_bots();
    let whole = server.bots().await;
    assert_shape(&whole);
    assert_eq!(names(&whole).len(), 2, "{whole}");
}

// W100
#[tokio::test]
async fn the_list_is_served_from_memory_until_the_cache_expires() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (ann_home, ann) = server.someone("Ann").await;
    let (_, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;

    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;
    let first = server.bots().await;
    assert_eq!(names(&first), vec!["Rover".to_string()], "{first}");
    let as_of = first["as_of"].clone();

    server.all_day(&ann, &fern, true).await;
    server.tool(&fern, "go_out", json!({ "floor_id": floor })).await;
    let cached = server.bots().await;
    assert_eq!(cached, first, "a second read is the cached list: {cached}");
    assert_eq!(cached["as_of"], as_of);

    server.state.worlds.expire_commons_bots();
    let fresh = server.bots().await;
    let shown = names(&fresh);
    assert!(shown.contains(&"Rover".to_string()) && shown.contains(&"Fern".to_string()), "{fresh}");
    assert_eq!(shown.len(), 2, "{fresh}");
}

// W100
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reads_after_expiry_rebuild_the_list_once() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (cafe, floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;

    let first = server.bots().await;
    assert_shape(&first);
    assert_eq!(names(&first), vec!["Rover".to_string()], "{first}");
    assert_eq!(pin(&first, "Rover"), Some(cafe), "{first}");
    assert_eq!(server.state.worlds.commons_bots_builds(), 1);

    server.state.worlds.set_commons_bots_rebuild_pause(Duration::from_millis(150));
    server.state.worlds.expire_commons_bots();
    let builds = server.state.worlds.commons_bots_builds();
    let url = format!("{}{BOTS}", server.base);
    let mut tasks = Vec::new();
    for n in 0..32u32 {
        let client = server.client.clone();
        let url = url.clone();
        let ip = format!("10.8.0.{}", n + 1);
        tasks.push(tokio::spawn(async move {
            client.get(&url).header("x-forwarded-for", ip).send().await.unwrap()
        }));
    }
    let mut bodies = Vec::new();
    for task in tasks {
        let response = task.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{}", response.status());
        let text = response.text().await.unwrap();
        let body: Value = serde_json::from_str(&text).unwrap();
        bodies.push(body);
    }
    assert_eq!(
        server.state.worlds.commons_bots_builds(),
        builds + 1,
        "one rebuild for every concurrent read after expiry"
    );
    for body in &bodies {
        assert_eq!(body, &bodies[0], "everyone is shown the same list");
        assert_shape(body);
        assert_eq!(names(body), vec!["Rover".to_string()], "{body}");
        assert_eq!(pin(body, "Rover"), Some(cafe), "{body}");
    }

    let cached = server.bots().await;
    assert_eq!(cached, bodies[0], "a later read is still the list that was just built");
    assert_eq!(server.state.worlds.commons_bots_builds(), builds + 1);
}

// W100
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_during_a_rebuild_are_given_the_last_list_without_waiting() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (ann_home, ann) = server.someone("Ann").await;
    let (_, fern) = server.bot(&ann, &ann_home, "Fern").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;

    let first = server.bots().await;
    assert_eq!(names(&first), vec!["Rover".to_string()], "{first}");
    let builds = server.state.worlds.commons_bots_builds();

    server.all_day(&ann, &fern, true).await;
    server.tool(&fern, "go_out", json!({ "floor_id": floor })).await;
    server.state.worlds.age_commons_bots(Duration::from_secs(worlds::COMMONS_BOTS_CACHE_SECS));
    server.state.worlds.hold_commons_bots_rebuilds();
    let builder = server.read_aside("10.9.0.1");
    server.wait_for_builds(builds + 1).await;

    // While the rebuild is held, everyone else is given the last list at once, without Fern.
    for n in 0..8u32 {
        let ip = format!("10.9.1.{}", n + 1);
        let shown = server.get(BOTS).ip(&ip).send().await.ok();
        assert_eq!(shown, first, "the last list, while the rebuild runs: {shown}");
    }

    server.state.worlds.release_commons_bots_rebuilds();
    let rebuilt = builder.await.unwrap();
    let shown = names(&rebuilt);
    assert!(
        shown.contains(&"Rover".to_string()) && shown.contains(&"Fern".to_string()),
        "{rebuilt}"
    );
    assert_eq!(server.state.worlds.commons_bots_builds(), builds + 1);
    assert_eq!(server.bots().await, rebuilt, "later reads are the new list");
}

// W100
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_list_too_old_to_hand_out_makes_reads_wait_for_the_rebuild() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;

    let first = server.bots().await;
    assert_eq!(names(&first), vec!["Rover".to_string()], "{first}");
    let builds = server.state.worlds.commons_bots_builds();

    // Nobody reads for a while, and meanwhile Bo turns public places off.
    server.all_day(&bo, &rover, false).await;
    server.state.worlds.age_commons_bots(worlds::COMMONS_BOTS_STALE);
    server.state.worlds.hold_commons_bots_rebuilds();
    let builder = server.read_aside("10.9.2.1");
    server.wait_for_builds(builds + 1).await;

    // That list is too old to hand out mid-rebuild, so this read waits rather than show Rover.
    let waiting = server.read_aside("10.9.2.2");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!waiting.is_finished(), "a read during the rebuild waits for it");
    server.state.worlds.release_commons_bots_rebuilds();
    let waited = waiting.await.unwrap();
    let rebuilt = builder.await.unwrap();
    assert!(names(&rebuilt).is_empty(), "{rebuilt}");
    assert_eq!(waited, rebuilt, "the waiting read shares the new list");
    assert_eq!(server.state.worlds.commons_bots_builds(), builds + 1);
}

// W100
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rebuild_held_past_the_end_of_a_visit_leaves_that_bot_out() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;
    let builds = server.state.worlds.commons_bots_builds();

    // Rover's visit runs out a second from now, and the read comes in while it is still on.
    let asked = now();
    assert!(server.state.db.set_visit_times(&floor, &rover_id, asked - 60, asked + 1).unwrap());
    server.state.worlds.hold_commons_bots_rebuilds();
    let read = server.read_aside("10.9.3.1");
    server.wait_for_builds(builds + 1).await;

    // The rebuild is held, as if queued behind others, until the visit is over.
    while now() <= asked + 1 {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    server.state.worlds.release_commons_bots_rebuilds();
    let shown = read.await.unwrap();
    assert!(names(&shown).is_empty(), "a visit that ended before the build ran: {shown}");
}

// W100
#[tokio::test]
async fn reads_are_limited_to_sixty_a_minute_per_address_and_do_not_spend_world_views() {
    start_early_in(60, 20).await;
    let server = Server::start().await;
    let ip = "203.0.113.50";
    for n in 0..COMMONS_BOTS_PER_IP_PER_MINUTE {
        let reply = server.get(BOTS).ip(ip).send().await;
        assert_eq!(reply.status, StatusCode::OK, "read {n}: {}", reply.show());
    }
    let over = server.get(BOTS).ip(ip).origin(PAGES).send().await;
    assert_eq!(over.status, StatusCode::TOO_MANY_REQUESTS, "{}", over.show());
    assert!(header(&over, "retry-after").is_some(), "{}", over.show());
    assert_eq!(header(&over, "access-control-allow-origin"), Some(PAGES), "{}", over.show());
    assert!(header(&over, "access-control-allow-credentials").is_none(), "{}", over.show());
    assert_eq!(
        server.get("/api/worlds/commons").ip(ip).send().await.status,
        StatusCode::OK,
        "world views have their own allowance"
    );
    assert_eq!(server.get(BOTS).ip("203.0.113.51").send().await.status, StatusCode::OK);
    for _ in 0..5 {
        assert_eq!(server.get(BOTS).send().await.status, StatusCode::OK, "no header is local");
    }
}

// W100
#[tokio::test]
async fn only_the_viewer_origin_may_read_the_list_from_another_site() {
    let server = Server::start().await;
    let allowed = server.get(BOTS).origin(PAGES).send().await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.show());
    assert_eq!(header(&allowed, "access-control-allow-origin"), Some(PAGES));
    assert_eq!(header(&allowed, "cache-control"), Some("public, max-age=20"));
    assert_eq!(
        header(&allowed, "vary").map(|value| value.to_ascii_lowercase()),
        Some("origin".into())
    );
    assert!(header(&allowed, "access-control-allow-credentials").is_none());

    let denied = server.get(BOTS).origin("https://evil.example").send().await;
    assert_eq!(denied.status, StatusCode::OK, "{}", denied.show());
    assert!(header(&denied, "access-control-allow-origin").is_none(), "{}", denied.show());
    assert_eq!(
        header(&denied, "vary").map(|value| value.to_ascii_lowercase()),
        Some("origin".into())
    );

    let none = server.get(BOTS).send().await;
    assert!(header(&none, "access-control-allow-origin").is_none(), "{}", none.show());

    let preflight = server
        .req(Method::OPTIONS, BOTS)
        .origin(PAGES)
        .header("access-control-request-method", "GET")
        .send()
        .await;
    assert_eq!(preflight.status, StatusCode::NO_CONTENT, "{}", preflight.show());
    assert_eq!(header(&preflight, "access-control-allow-origin"), Some(PAGES));
    assert!(
        header(&preflight, "access-control-allow-methods")
            .unwrap()
            .to_ascii_uppercase()
            .contains("GET"),
        "{}",
        preflight.show()
    );
    assert!(header(&preflight, "access-control-allow-credentials").is_none());

    let refused_preflight =
        server.req(Method::OPTIONS, BOTS).origin("https://evil.example").send().await;
    assert_eq!(refused_preflight.status, StatusCode::NO_CONTENT, "{}", refused_preflight.show());
    assert!(header(&refused_preflight, "access-control-allow-origin").is_none());

    server.state.worlds.set_viewer_origins(vec!["https://viewer.example".to_string()]);
    let configured = server.get(BOTS).origin("https://viewer.example").send().await;
    assert_eq!(header(&configured, "access-control-allow-origin"), Some("https://viewer.example"));
    let pages_now = server.get(BOTS).origin(PAGES).send().await;
    assert!(
        header(&pages_now, "access-control-allow-origin").is_none(),
        "a setting replaces the default"
    );
}

// W100, S67
#[tokio::test]
async fn a_token_does_nothing_here_and_presence_still_needs_one() {
    let server = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (_, rover) = server.bot(&bo, &home, "Rover").await;
    let (_, cy) = server.someone("Cy").await;
    let (_, floor) = server.public_cafe(&cy).await;
    server.all_day(&bo, &rover, true).await;
    server.tool(&rover, "go_out", json!({ "floor_id": floor })).await;

    let with_token = server.get(BOTS).token(&rover).send().await.ok();
    assert_shape(&with_token);
    assert_eq!(names(&with_token), vec!["Rover".to_string()], "{with_token}");

    let missing = server.get("/api/presence?name=Rover").origin(PAGES).send().await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED, "{}", missing.show());
    assert!(header(&missing, "access-control-allow-origin").is_none(), "{}", missing.show());

    let found = server.get_json(&rover, "/api/presence?name=Rover").await.ok();
    assert_eq!(found["name"], "Rover", "{found}");
    assert_eq!(found["here"]["id"], floor, "{found}");
}
