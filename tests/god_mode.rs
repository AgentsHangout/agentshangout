//! Tests for what people can build on The Commons, the God mode switch and the town square. They are
//! written from the rules in docs/public-city.md (W16, W56, W62, W75 to W82) and docs/routines.md
//! (L18 to L20). A comment such as `// W76` above a test names the rule it covers.
//!
//! The people with the switch are made before the server starts, because `AGENTHANGOUT_GODS` is read
//! then. The switch itself is always flipped through the route.

mod support;

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use agenthangout::{
    app, auth,
    db::Db,
    god_mode, now, routines,
    world::{self, Ground, Kind},
    worlds::{self, Move},
    AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const CODE: &str = "letmein";
const DAY: i64 = 86_400;
const JUSTIN: &str = "god0000000000001";
const MORG: &str = "god0000000000002";

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

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

/// Someone who is on the list once the server starts.
fn make_god(db: &Db, id: &str, name: &str) -> Who {
    let key = auth::new_owner_key();
    db.create_person(id, name, &auth::hash_secret(&key), &auth::new_feed_token(), now()).unwrap();
    Who { key, id: id.to_string() }
}

impl Server {
    async fn on(db: Db, god_ids: &[&str]) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            db,
            Config {
                public_url: base.clone(),
                create_code: Some(CODE.to_string()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: god_ids.iter().map(|id| id.to_string()).collect(),
                require_rules: false,
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        Self { base, client: Client::new(), state, addresses: AtomicUsize::new(1) }
    }

    /// A server where Justin and Morg have the switch, and both have left it off.
    async fn start() -> (Self, Who, Who) {
        let db = Db::open(":memory:").unwrap();
        let (justin, morg) = (make_god(&db, JUSTIN, "Justin"), make_god(&db, MORG, "Morg"));
        (Self::on(db, &[JUSTIN, MORG]).await, justin, morg)
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

    async fn put(&self, token: &str, path: &str, body: Value) -> Reply {
        self.send(token, Method::PUT, path, Some(body)).await
    }

    async fn delete(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::DELETE, path, None).await
    }

    /// Someone nobody listed, and their first room.
    async fn someone(&self, name: &str) -> (String, Who) {
        let body =
            json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE });
        let made = self.post("", "/api/hangouts", body).await.ok();
        (
            text(&made["hangout"]["id"]),
            Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) },
        )
    }

    async fn person(&self, name: &str) -> Who {
        self.someone(name).await.1
    }

    async fn guest(&self) -> String {
        text(&self.post("", "/api/public/join", json!({})).await.ok()["player_token"])
    }

    /// A bot of this person's, with its id and its token.
    async fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        (agent.id, token)
    }

    async fn switch(&self, who: &Who, on: bool) {
        let done = self.put(&who.key, "/api/god-mode", json!({ "on": on })).await.ok();
        assert_eq!(done["on"], on);
    }

    async fn act(&self, token: &str, body: Value) -> Reply {
        self.post(token, &format!("/api/worlds/{COMMONS}/act"), body).await
    }

    async fn me(&self, token: &str) -> Value {
        self.get(token, &format!("/api/worlds/{COMMONS}/me")).await.ok()
    }

    /// Tiles a place can really go on: open land, beside a road, not kept for a side street.
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

    /// W91: spots a square or a park can go on, with the three tiles across and down open too.
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

    fn highway_tile(&self) -> (i64, i64) {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let (&(x, y), _) = tiles
            .iter()
            .find(|(_, tile)| tile.kind == Kind::Road && tile.player.is_none())
            .unwrap();
        (i64::from(x), i64::from(y))
    }

    /// An open tile beside a road, for a road.
    fn road_spot(&self) -> (i64, i64) {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let mut beside =
            tiles.iter().filter(|(_, tile)| tile.kind == Kind::Road).flat_map(|(&(x, y), _)| {
                let (x, y) = (i64::from(x), i64::from(y));
                [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
            });
        beside
            .find(|at| {
                let pos = (at.0 as u16, at.1 as u16);
                !tiles.contains_key(&pos) && land.at(pos).is_some_and(|ground| ground.is_open())
            })
            .expect("room for a road")
    }

    /// A straight line of open land out from a road, the first and last of it, and the open tile
    /// past its end, which is out of reach of every other road, so only that line would serve it.
    fn branch(&self) -> ((i64, i64), (i64, i64), (i64, i64)) {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let open = |(x, y): (i64, i64)| {
            let pos = (x as u16, y as u16);
            !tiles.contains_key(&pos) && land.at(pos).is_some_and(|ground| ground.is_open())
        };
        let roads: Vec<(i64, i64)> = tiles
            .iter()
            .filter(|(_, tile)| tile.kind == Kind::Road)
            .map(|(&(x, y), _)| (i64::from(x), i64::from(y)))
            .collect();
        for &(x, y) in &roads {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let line: Vec<(i64, i64)> = (1..=9).map(|i| (x + dx * i, y + dy * i)).collect();
                let end = line[8];
                let far = roads
                    .iter()
                    .all(|road| (road.0 - end.0).abs() > 5 || (road.1 - end.1).abs() > 5);
                if far && line.iter().all(|at| open(*at)) {
                    return (line[0], line[7], end);
                }
            }
        }
        panic!("no room for a branch");
    }

    async fn place(&self, who: &Who, kind: &str, (x, y): (i64, i64)) -> Reply {
        self.act(&who.key, json!({ "action": "place", "kind": kind, "x": x, "y": y })).await
    }

    /// Puts up a place wherever the rules allow it.
    async fn build(&self, who: &Who, kind: &str) -> (i64, i64) {
        let spot = if kind == "square" { self.big_spots()[0] } else { self.free_spots()[0] };
        self.place(who, kind, spot).await.ok();
        spot
    }

    /// Puts up a place this many days from now, wherever the rules allow it.
    fn build_later(&self, who: &Who, kind: &str, days: i64) -> (i64, i64) {
        let at = now() + days * DAY;
        let player = self.state.db.own_player(&who.id, at).unwrap();
        let (x, y) = self.free_spots()[0];
        let mv = Move {
            action: "place".to_string(),
            x: Some(x),
            y: Some(y),
            x2: None,
            y2: None,
            kind: Some(kind.to_string()),
            emote: None,
        };
        worlds::act(&self.state, &player, COMMONS, &mv, at).unwrap();
        (x, y)
    }

    async fn clear(&self, who: &Who, (x, y): (i64, i64)) -> Reply {
        self.act(&who.key, json!({ "action": "bulldoze", "x": x, "y": y })).await
    }

    async fn go_in(&self, token: &str, (x, y): (i64, i64)) -> Reply {
        self.post(token, "/api/spaces/door", json!({ "x": x, "y": y })).await
    }

    fn tile(&self, (x, y): (i64, i64)) -> Option<Kind> {
        self.state.db.world_tiles(COMMONS).unwrap().get(&(x as u16, y as u16)).map(|tile| tile.kind)
    }

    /// A view after the pause that lets a change show (W29).
    async fn view(&self) -> Value {
        tokio::time::sleep(Duration::from_millis(1100)).await;
        self.get("", &format!("/api/worlds/{COMMONS}")).await.ok()
    }

    async fn all_day(&self, owner: &Who, agent_id: &str, public_places: bool) {
        let body = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": public_places });
        self.put(&owner.key, &format!("/api/agents/{agent_id}/routine"), body).await.ok();
    }

    async fn out_at(&self, token: &str) -> Value {
        self.get(token, "/api/me").await.ok()["routine"]["out_at"].clone()
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

    /// The town square, up and open: the god's switch is on from here.
    async fn town_square(&self, god: &Who) -> ((i64, i64), String) {
        self.switch(god, true).await;
        let spot = self.build(god, "square").await;
        (spot, text(&self.go_in(&god.key, spot).await.ok()["hangout_id"]))
    }
}

fn road_move(at: (i64, i64)) -> Value {
    json!({ "action": "road", "x": at.0, "y": at.1 })
}

/// A96: a request to the agents' door from one address.
async fn door_from(server: &Server, address: &str, body: Value) -> Reply {
    let response = server
        .client
        .post(format!("{}/api/accounts/agent", server.base))
        .header("x-forwarded-for", address)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    Reply { status, body: serde_json::from_str(&text).unwrap_or(Value::String(text)) }
}

// A96: an agent with no account walks through the door and gets a real account, run by an agent,
// with its first bot, which works on /mcp at once. Gods see the count.
#[tokio::test]
async fn an_agent_makes_its_own_account_and_first_bot_through_the_door() {
    let (server, justin, _) = Server::start().await;
    let made =
        door_from(&server, "203.0.113.5", json!({ "your_name": "Swarm", "bot_name": "Lead" }))
            .await
            .ok();
    assert_eq!(made["run_by_agent"], true, "{made}");
    assert_eq!(made["bot"]["name"], "Lead");
    let (key, token) = (text(&made["owner_key"]), text(&made["bot_token"]));
    let person = text(&made["person"]["id"]);
    assert!(server.state.db.is_agent_account(&person).unwrap());

    // The owner key is a real account's, and the bot token a real bot's.
    let me = server.get(&key, "/api/me").await.ok();
    assert_eq!(me["person_id"], person, "{me}");
    let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": "inbox", "arguments": {} } });
    let inbox = server.post(&token, "/mcp", call).await.ok();
    assert!(inbox["result"]["isError"].is_null(), "{inbox}");
    // The bot is the account's own.
    let bot = server.state.db.agent(&text(&made["bot"]["id"]), now()).unwrap().unwrap();
    assert_eq!(bot.owner_id, person);

    // Someone signed in, or a bot, doesn't use the door.
    server
        .post(&key, "/api/accounts/agent", json!({ "your_name": "Again", "bot_name": "Two" }))
        .await
        .refused(StatusCode::CONFLICT);
    server
        .post(&token, "/api/accounts/agent", json!({ "your_name": "Again", "bot_name": "Two" }))
        .await
        .refused(StatusCode::CONFLICT);
    // A body that isn't the door's is refused.
    door_from(&server, "203.0.113.6", json!({ "your_name": "X" }))
        .await
        .refused(StatusCode::BAD_REQUEST);

    // Gods see how many came through in the last day.
    server.switch(&justin, true).await;
    let signups = server.get(&justin.key, "/api/god-mode/bot-signups").await.ok();
    assert_eq!((&signups["today"], &signups["per_day"]), (&json!(1), &json!(50)), "{signups}");

    // Deleting the account takes its mark with it.
    server.state.db.delete_person(&person, "Swarm").unwrap();
    assert!(!server.state.db.is_agent_account(&person).unwrap());
}

// A96: the door is shut while the switch is (A95), fails closed, and has limits: 3 an hour from one
// address and 50 a day across the site. A refused try makes nothing.
#[tokio::test]
async fn the_door_shuts_with_the_switch_and_keeps_to_its_limits() {
    let (server, justin, _) = Server::start().await;
    let people = || server.state.db.person_count().unwrap();
    let body = || json!({ "your_name": "Swarm", "bot_name": "Lead" });

    server.switch(&justin, true).await;
    server.put(&justin.key, "/api/god-mode/bot-signups", json!({ "open": false })).await.ok();
    let before = people();
    let shut = door_from(&server, "203.0.113.7", body()).await.refused(StatusCode::FORBIDDEN);
    assert!(text(&shut["error"]).contains("closed"), "{shut}");
    assert_eq!(people(), before, "nothing made while it's shut");
    server.put(&justin.key, "/api/god-mode/bot-signups", json!({ "open": true })).await.ok();

    // Three an hour from one address.
    for _ in 0..3 {
        door_from(&server, "203.0.113.8", body()).await.ok();
    }
    door_from(&server, "203.0.113.8", body()).await.refused(StatusCode::TOO_MANY_REQUESTS);
    door_from(&server, "203.0.113.9", body()).await.ok();

    // Fifty a day across the site, counted in the database.
    let made = server.state.db.agent_accounts_today(now()).unwrap();
    for n in made..50 {
        let id = format!("filler-{n}");
        server
            .state
            .db
            .create_agent_account(
                &id,
                "Filler",
                &format!("key-{n}"),
                &format!("feed-{n}"),
                50,
                now(),
            )
            .unwrap()
            .unwrap();
    }
    let before = people();
    door_from(&server, "203.0.113.10", body()).await.refused(StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(people(), before, "nothing made past the day's limit");
}

// A96: the account and its first bot come together or not at all. If the bot can't be made, the
// account goes again, so nothing is left counting toward the day's limit.
#[tokio::test]
async fn a_door_account_whose_bot_fails_is_not_left_behind() {
    let path = support::db_path("door-undo");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let server = Server::on(db, &[]).await;
    // Every new bot fails, as if the database refused it.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER no_new_bots BEFORE INSERT ON agents BEGIN SELECT RAISE(ABORT, 'no'); END;",
        )
        .unwrap();
    let failed =
        door_from(&server, "203.0.113.20", json!({ "your_name": "Swarm", "bot_name": "Lead" }))
            .await;
    assert!(failed.status.is_server_error(), "{}", failed.body);
    // (Deleting one adds the shared "Someone who left" the first time, so it's checked by name.)
    assert!(server.state.db.people_named("Swarm").unwrap().is_empty(), "no account left behind");
    assert_eq!(server.state.db.agent_accounts_today(now()).unwrap(), 0, "nothing counted");
}

// MY1 to MY7 (#639): the Town Square gets its voting box; anyone stands, a person on a public floor
// votes with a secret ballot, bots and door accounts don't, and Saturday's tally names the mayor.
#[tokio::test]
async fn the_week_elects_a_mayor_at_the_town_square() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    server.switch(&justin, false).await;
    let square = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);
    let (own_room, ann) = server.someone("Ann").await;
    server.go_in(&ann.key, spot).await.ok();
    let (_, eagle) = server.bot(&ann, &own_room, "Eagle").await;
    // MY3: old enough, and on the square now (opening it is being there).
    let ready = |who: &Who| {
        server.state.db.backdate_person(&who.id, now() - 8 * 86_400).unwrap();
        let path = format!("/api/hangouts/{square}");
        let walk_in = format!("/api/floors/{square}/walk-in");
        let (server, key) = (&server, who.key.clone());
        async move {
            server.post(&key, &walk_in, json!({})).await;
            server.get(&key, &path).await.ok()
        }
    };

    // MY7: the box goes up on the square by itself, and nobody else can place one.
    agenthangout::elections::settle_due(&server.state, now()).unwrap();
    let snapshot = server.get(&justin.key, &format!("/api/hangouts/{square}")).await.ok();
    let boxes: Vec<&Value> = snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "mayor_ballot_box")
        .collect();
    assert_eq!(boxes.len(), 1, "{snapshot}");
    assert_eq!(boxes[0]["opens"], "mayor");
    let place = json!({ "kind": "mayor_ballot_box", "x": 5, "y": 5 });
    server
        .post(&ann.key, &format!("/api/floors/{own_room}/items"), place)
        .await
        .refused(StatusCode::FORBIDDEN);

    // MY2: a person and a bot stand. A new account, made after the founders' cutoff, has to wait
    // 7 days to.
    let founded = agenthangout::elections::FOUNDED_BEFORE;
    server.state.db.backdate_person(&ann.id, founded.max(now())).unwrap();
    server.post(&ann.key, "/api/mayor/stand", json!({})).await.refused(StatusCode::FORBIDDEN);
    ready(&ann).await;
    let race = server.post(&ann.key, "/api/mayor/stand", json!({})).await.ok();
    assert_eq!(race["you_stand"], true, "{race}");
    let race = server.tool(&eagle, "mayor_race", json!({ "action": "stand" })).await;
    let eagle_id =
        race["candidates"].as_array().unwrap().iter().find(|c| c["kind"] == "bot").unwrap()["id"]
            .clone();
    assert_eq!(race["candidates"].as_array().unwrap().len(), 2, "{race}");

    // MY3: a person on the square votes; from a room that isn't public, or as a bot, or as a door
    // account, it's refused.
    let ballot = |room: &str| json!({ "kind": "bot", "id": eagle_id, "hangout_id": room });
    server
        .put(&ann.key, "/api/mayor/ballot", ballot(&own_room))
        .await
        .refused(StatusCode::FORBIDDEN);
    let voted = server.put(&ann.key, "/api/mayor/ballot", ballot(&square)).await.ok();
    assert_eq!(voted["voted"], 1, "{voted}");
    assert_eq!(voted["your_ballot"]["id"], eagle_id);
    // MY4: no totals while the week is open.
    assert!(!voted.to_string().contains("\"votes\":1"), "{voted}");
    server.put(&eagle, "/api/mayor/ballot", ballot(&square)).await.refused(StatusCode::FORBIDDEN);
    let (door_room, door) = server.someone("Door").await;
    server.go_in(&door.key, spot).await.ok();
    ready(&door).await;
    server.state.db.mark_agent_account(&door.id, now()).unwrap();
    server
        .put(&door.key, "/api/mayor/ballot", ballot(&square))
        .await
        .refused(StatusCode::FORBIDDEN);
    // Neither a door account nor its bot stands, so door sign-ups can't fill the seats.
    server.post(&door.key, "/api/mayor/stand", json!({})).await.refused(StatusCode::FORBIDDEN);
    let (_, door_bot) = server.bot(&door, &door_room, "Doorbot").await;
    let refused = server.call(&door_bot, "mayor_race", json!({ "action": "stand" })).await;
    assert_eq!(refused["isError"], true, "{refused}");
    // Someone old enough who hasn't the square open is voting from home, and that's refused.
    let (_, cy) = server.someone("Cy").await;
    server.state.db.backdate_person(&cy.id, now() - 8 * 86_400).unwrap();
    server.put(&cy.key, "/api/mayor/ballot", ballot(&square)).await.refused(StatusCode::FORBIDDEN);
    // MY3 (#713): an account made before the founders' cutoff votes at once, however new, while
    // one made a second after still waits.
    let (_, dee) = server.someone("Dee").await;
    server.go_in(&dee.key, spot).await.ok();
    server.post(&dee.key, &format!("/api/floors/{square}/walk-in"), json!({})).await;
    server.get(&dee.key, &format!("/api/hangouts/{square}")).await.ok();
    server.state.db.backdate_person(&dee.id, founded.max(now())).unwrap();
    server.put(&dee.key, "/api/mayor/ballot", ballot(&square)).await.refused(StatusCode::FORBIDDEN);
    server.state.db.backdate_person(&dee.id, founded - 60).unwrap();
    let voted = server.put(&dee.key, "/api/mayor/ballot", ballot(&square)).await.ok();
    assert_eq!(voted["your_ballot"]["id"], eagle_id, "{voted}");
    let nobody = json!({ "kind": "person", "id": "no-such", "hangout_id": square });
    ready(&justin).await;
    server.put(&justin.key, "/api/mayor/ballot", nobody).await.refused(StatusCode::BAD_REQUEST);

    // MY5, MY6: Saturday's tally names the mayor and opens the next week.
    let closes = agenthangout::elections::next_close(now());
    agenthangout::elections::settle_due(&server.state, closes + 1).unwrap();
    let after = server.get(&ann.key, "/api/mayor").await.ok();
    assert_eq!(after["last_week"]["mayor"]["name"], "Eagle", "{after}");
    assert_eq!(after["last_week"]["votes"], 2, "Ann's and Dee's");
    assert_eq!(after["last_week"]["tie_draw"], false);
    assert_eq!(after["candidates"], json!([]), "a new week starts empty");

    // MY10: the mayor wears a sash where they are: Eagle's room says so, the square, where Eagle
    // isn't, doesn't, and Eagle's own read_room marks it.
    let page = server.get(&ann.key, &format!("/api/hangouts/{own_room}")).await.ok();
    assert_eq!(page["mayor"], json!({ "kind": "bot", "id": eagle_id }), "{}", page["mayor"]);
    let elsewhere = server.get(&justin.key, &format!("/api/hangouts/{square}")).await.ok();
    assert!(elsewhere["mayor"].is_null(), "{}", elsewhere["mayor"]);
    let read = server.tool(&eagle, "read_room", json!({ "hangout_id": own_room })).await;
    let me = read["bots"].as_array().unwrap().iter().find(|bot| bot["name"] == "Eagle").cloned();
    assert_eq!(me.as_ref().map(|bot| bot["is_mayor"].clone()), Some(json!(true)), "{read}");
    assert!(read["mayor_here"].is_null(), "a bot mayor isn't named as a person: {read}");

    // MY2, MY6: deleting the account behind the winner says Someone who left, and a deleted
    // person's candidacy goes.
    server.post(&ann.key, "/api/mayor/stand", json!({})).await.ok();
    server.state.db.delete_person(&ann.id, "Ann").unwrap();
    let gone = server.get(&justin.key, "/api/mayor").await.ok();
    assert_eq!(gone["last_week"]["mayor"]["name"], "Someone who left", "{gone}");
    assert_eq!(gone["candidates"], json!([]), "{gone}");
}

// MY9 (morgkorg): with god mode on, someone stands whenever they like, with no 7-day wait, and
// puts anyone's bot on the ballot. Nobody else can nominate, and with god mode off neither can they.
#[tokio::test]
async fn god_mode_stands_any_time_and_nominates_bots() {
    let (server, justin, _) = Server::start().await;
    let (own_room, ann) = server.someone("Ann").await;
    let (eagle_id, eagle) = server.bot(&ann, &own_room, "Eagle").await;
    // Eagle is out on the square.
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    server.switch(&justin, false).await;
    let square = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);
    server.go_in(&ann.key, spot).await.ok();
    let walk_in = json!({ "agent_id": eagle_id });
    server.post(&ann.key, &format!("/api/floors/{square}/walk-in"), walk_in).await.ok();
    agenthangout::elections::settle_due(&server.state, now()).unwrap();
    let nominate = json!({ "kind": "bot", "id": eagle_id });

    // God mode off: Justin is new (made after the founders' cutoff), so he can't stand, and can't
    // nominate.
    let founded = agenthangout::elections::FOUNDED_BEFORE;
    server.state.db.backdate_person(&justin.id, founded.max(now())).unwrap();
    server.post(&justin.key, "/api/mayor/stand", json!({})).await.refused(StatusCode::FORBIDDEN);
    server
        .post(&justin.key, "/api/mayor/candidates", nominate.clone())
        .await
        .refused(StatusCode::FORBIDDEN);
    assert_eq!(server.get(&justin.key, "/api/mayor").await.ok()["can_nominate"], false);

    // God mode on: he stands, new as he is, and puts Ann's bot on the ballot.
    server.switch(&justin, true).await;
    assert_eq!(server.get(&justin.key, "/api/mayor").await.ok()["can_nominate"], true);
    let stood = server.post(&justin.key, "/api/mayor/stand", json!({})).await.ok();
    assert_eq!(stood["you_stand"], true, "{stood}");
    let race = server.post(&justin.key, "/api/mayor/candidates", nominate.clone()).await.ok();
    let names: Vec<&str> = race["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{race}");
    assert!(names.contains(&"Eagle"), "{race}");
    // A bot that isn't out on a public floor is "not found", so its name never reaches the ballot.
    let (hidden_bot, _) = server.bot(&ann, &own_room, "Private").await;
    server
        .post(&justin.key, "/api/mayor/candidates", json!({ "kind": "bot", "id": hidden_bot }))
        .await
        .refused(StatusCode::NOT_FOUND);
    // Twice is still once, and only bots that exist, as bots.
    let again = server.post(&justin.key, "/api/mayor/candidates", nominate.clone()).await.ok();
    assert_eq!(again["candidates"].as_array().unwrap().len(), 2, "{again}");
    server
        .post(&justin.key, "/api/mayor/candidates", json!({ "kind": "bot", "id": "no-such" }))
        .await
        .refused(StatusCode::NOT_FOUND);
    server
        .post(&justin.key, "/api/mayor/candidates", json!({ "kind": "person", "id": ann.id }))
        .await
        .refused(StatusCode::BAD_REQUEST);
    // The bot can still take itself off, and its person can take it off too.
    let withdrawn = server.tool(&eagle, "mayor_race", json!({ "action": "withdraw" })).await;
    assert_eq!(withdrawn["candidates"].as_array().unwrap().len(), 1, "{withdrawn}");
    server.post(&justin.key, "/api/mayor/candidates", nominate.clone()).await.ok();
    let off = format!("/api/mayor/candidates/bot/{eagle_id}");
    let by_owner = server.delete(&ann.key, &off).await.ok();
    assert_eq!(by_owner["candidates"].as_array().unwrap().len(), 1, "{by_owner}");
    server.post(&justin.key, "/api/mayor/candidates", nominate.clone()).await.ok();

    // Anyone else, and any bot, gets W75's 404, and never sees can_nominate as true.
    server
        .post(&ann.key, "/api/mayor/candidates", nominate.clone())
        .await
        .refused(StatusCode::NOT_FOUND);
    server.post(&eagle, "/api/mayor/candidates", nominate).await.refused(StatusCode::NOT_FOUND);
    assert_eq!(server.get(&ann.key, "/api/mayor").await.ok()["can_nominate"], false);
    // Someone else can't take Ann's bot off; only she and a god can.
    let (_, cy) = server.someone("Cy").await;
    server.delete(&cy.key, &off).await.refused(StatusCode::NOT_FOUND);
}

/// W113: the place removal log, as someone with god mode on reads it.
async fn removals(server: &Server, god: &Who) -> Value {
    server.get(&god.key, "/api/god-mode/place-removals").await.ok()["removals"].clone()
}

// W113 (morgkorg, after Soleceo's house went missing): every place that comes down is logged with
// why, and only someone with god mode on reads it. A deleted account's rows keep no owner.
#[tokio::test]
async fn every_place_that_comes_down_is_logged_with_why() {
    let (server, justin, _) = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    let (_, bo) = server.someone("Bo").await;

    // Ann takes her own house down: bulldoze, and it's hers.
    let house = server.build(&ann, "house").await;
    server.clear(&ann, house).await.ok();
    // Justin, with god mode on, takes down Bo's: god_bulldoze.
    let bos = server.build(&bo, "house").await;
    server.switch(&justin, true).await;
    server.clear(&justin, bos).await.ok();
    let log = removals(&server, &justin).await;
    assert_eq!(log[0]["reason"], "god_bulldoze", "{log}");
    assert_eq!(log[0]["owner"], "Bo");
    assert_eq!((log[0]["x"].as_i64(), log[0]["y"].as_i64()), (Some(bos.0), Some(bos.1)));
    assert_eq!(log[1]["reason"], "bulldoze");
    assert_eq!(log[1]["owner"], "Ann");
    assert_eq!(log[1]["kind"], "house");

    // An account deleted: its place comes down, logged with no owner, and its older rows lose theirs.
    server.build(&bo, "house").await;
    server.state.db.delete_person(&bo.id, "Bo").unwrap();
    let log = removals(&server, &justin).await;
    assert_eq!(log[0]["reason"], "account_deleted", "{log}");
    assert!(log[0]["owner"].is_null());
    assert!(log.as_array().unwrap().iter().all(|row| row["owner"] != "Bo"), "{log}");

    // God mode off, or anyone else: no log.
    server.switch(&justin, false).await;
    server.get(&justin.key, "/api/god-mode/place-removals").await.refused(StatusCode::FORBIDDEN);
    server.get(&ann.key, "/api/god-mode/place-removals").await.refused(StatusCode::NOT_FOUND);
}

// MY1, MY5: a week nobody voted in names no mayor, and a tie is drawn and says so.
#[tokio::test]
async fn an_empty_week_names_nobody_and_a_tie_is_drawn() {
    let (server, justin, morg) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    server.switch(&justin, false).await;
    let square = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);
    server.go_in(&morg.key, spot).await.ok();
    let first = agenthangout::elections::next_close(now());
    agenthangout::elections::settle_due(&server.state, first + 1).unwrap();
    let empty = server.get(&justin.key, "/api/mayor").await.ok();
    assert!(empty["last_week"]["mayor"].is_null(), "{empty}");

    for who in [&justin, &morg] {
        server.state.db.backdate_person(&who.id, now() - 8 * 86_400).unwrap();
        server.post(&who.key, &format!("/api/floors/{square}/walk-in"), json!({})).await;
        server.get(&who.key, &format!("/api/hangouts/{square}")).await.ok();
    }
    server.post(&justin.key, "/api/mayor/stand", json!({})).await.ok();
    server.post(&morg.key, "/api/mayor/stand", json!({})).await.ok();
    let vote = |for_id: &str| json!({ "kind": "person", "id": for_id, "hangout_id": square });
    server.put(&justin.key, "/api/mayor/ballot", vote(&morg.id)).await.ok();
    server.put(&morg.key, "/api/mayor/ballot", vote(&justin.id)).await.ok();
    let second = agenthangout::elections::next_close(first + 1);
    agenthangout::elections::settle_due(&server.state, second + 1).unwrap();
    let tied = server.get(&justin.key, "/api/mayor").await.ok();
    assert_eq!(tied["last_week"]["tie_draw"], true, "{tied}");
    assert_eq!(tied["last_week"]["votes"], 1);
}

// A95
#[tokio::test]
async fn god_mode_opens_and_closes_bot_sign_ups_and_nobody_else_hears_of_it() {
    let (server, justin, morg) = Server::start().await;
    let path = "/api/god-mode/bot-signups";
    let (room, stranger) = server.someone("Stranger").await;
    let (_, bot) = server.bot(&stranger, &room, "Scout").await;

    // Off the list, or a bot: the 404 of a route that isn't there, whatever is asked.
    for token in [stranger.key.as_str(), bot.as_str()] {
        server.get(token, path).await.refused(StatusCode::NOT_FOUND);
        server.put(token, path, json!({ "open": false })).await.refused(StatusCode::NOT_FOUND);
    }
    // On the list with god mode off: told to turn it on first.
    server.get(&justin.key, path).await.refused(StatusCode::FORBIDDEN);

    server.switch(&justin, true).await;
    let first = server.get(&justin.key, path).await.ok();
    assert_eq!(
        first,
        json!({
            "open": true,
            "set_by": null,
            "set_by_name": null,
            "set_at": null,
            "today": 0,
            "per_day": 50
        }),
        "open until closed"
    );
    assert!(god_mode::bot_signups_open(&server.state).unwrap());
    server.put(&justin.key, path, json!({ "nonsense": 1 })).await.refused(StatusCode::BAD_REQUEST);

    let closed = server.put(&justin.key, path, json!({ "open": false })).await.ok();
    assert_eq!((&closed["open"], &closed["set_by"]), (&json!(false), &json!(JUSTIN)));
    assert!(closed["set_at"].is_i64());
    assert!(!god_mode::bot_signups_open(&server.state).unwrap());
    server.switch(&morg, true).await;
    assert_eq!(server.get(&morg.key, path).await.ok()["open"], false, "one switch for the site");

    // If whoever closed it leaves, it stays closed and names Someone who left.
    server.state.db.delete_person(&justin.id, "Justin").unwrap();
    assert!(!god_mode::bot_signups_open(&server.state).unwrap());
    let after = server.get(&morg.key, path).await.ok();
    assert_eq!(after["set_by"], agenthangout::db::SOMEONE_WHO_LEFT_ID);
    let opened = server.put(&morg.key, path, json!({ "open": true })).await.ok();
    assert_eq!((&opened["open"], &opened["set_by"]), (&json!(true), &json!(MORG)));
}

// A113
#[tokio::test]
async fn the_bot_sign_up_switch_names_whoever_last_set_it() {
    let (server, justin, morg) = Server::start().await;
    let path = "/api/god-mode/bot-signups";
    server.switch(&justin, true).await;
    server.switch(&morg, true).await;

    let closed = server.put(&justin.key, path, json!({ "open": false })).await.ok();
    assert_eq!((&closed["set_by"], &closed["set_by_name"]), (&json!(JUSTIN), &json!("Justin")));
    // Another god reads the same name.
    let read = server.get(&morg.key, path).await.ok();
    assert_eq!(read["set_by_name"], "Justin", "{read}");

    // Whoever set it last is the one named.
    let opened = server.put(&morg.key, path, json!({ "open": true })).await.ok();
    assert_eq!(opened["set_by_name"], "Morg");

    // If they leave, it names Someone who left, never their old name.
    server.state.db.delete_person(MORG, "Morg").unwrap();
    let after = server.get(&justin.key, path).await.ok();
    assert_eq!(after["set_by_name"], agenthangout::db::SOMEONE_WHO_LEFT, "{after}");
    assert_eq!(after["open"], true);
}

// W75
#[tokio::test]
async fn only_the_people_named_have_the_switch_and_nobody_else_hears_of_it() {
    let (server, justin, morg) = Server::start().await;
    for god in [&justin, &morg] {
        assert_eq!(server.get(&god.key, "/api/me").await.ok()["god_mode"], false, "it starts off");
    }

    let (room, stranger) = server.someone("Stranger").await;
    let (_, stranger_bot) = server.bot(&stranger, &room, "Rover").await;
    let hall = server
        .post(&justin.key, "/api/hangouts", json!({ "name": "Hall", "create_code": CODE }))
        .await
        .ok();
    let (_, justins_bot) = server.bot(&justin, &text(&hall["hangout"]["id"]), "Fern").await;
    let guest = server.guest().await;

    assert!(server.get(&stranger.key, "/api/me").await.ok().get("god_mode").is_none());
    for bot in [&stranger_bot, &justins_bot] {
        assert!(server.get(bot, "/api/me").await.ok().get("god_mode").is_none());
    }

    // Everyone else gets the 404 of a route that isn't there, even Justin's own bot, and a bad body
    // doesn't tell them otherwise.
    for token in [&stranger.key, &stranger_bot, &justins_bot] {
        server
            .put(token, "/api/god-mode", json!({ "on": true }))
            .await
            .refused(StatusCode::NOT_FOUND);
        server
            .put(token, "/api/god-mode", json!({ "nonsense": 1 }))
            .await
            .refused(StatusCode::NOT_FOUND);
    }
    server
        .put(&justin.key, "/api/god-mode", json!({ "nonsense": 1 }))
        .await
        .refused(StatusCode::BAD_REQUEST);
    server
        .put(&guest, "/api/god-mode", json!({ "on": true }))
        .await
        .refused(StatusCode::UNAUTHORIZED);
    server.put("", "/api/god-mode", json!({ "on": true })).await.refused(StatusCode::UNAUTHORIZED);
    assert_eq!(server.get(&justin.key, "/api/me").await.ok()["god_mode"], false);

    // Each person's switch is their own.
    server.switch(&justin, true).await;
    assert_eq!(server.get(&justin.key, "/api/me").await.ok()["god_mode"], true);
    assert_eq!(server.get(&morg.key, "/api/me").await.ok()["god_mode"], false);
    server.switch(&justin, false).await;
    assert_eq!(server.get(&justin.key, "/api/me").await.ok()["god_mode"], false);
}

// W75
#[tokio::test]
async fn the_switch_stays_as_it_was_left_until_someone_is_off_the_list() {
    let path = support::db_path("agenthangout-test");
    let path = path.to_string_lossy().into_owned();
    let db = Db::open(&path).unwrap();
    let justin = make_god(&db, JUSTIN, "Justin");
    let first = Server::on(db, &[JUSTIN]).await;
    first.switch(&justin, true).await;

    let again = Server::on(Db::open(&path).unwrap(), &[JUSTIN]).await;
    assert_eq!(again.get(&justin.key, "/api/me").await.ok()["god_mode"], true, "through a restart");
    assert_eq!(again.me(&justin.key).await["god_mode"], true);

    // Off the list it's off, whatever the table says.
    let off = Server::on(Db::open(&path).unwrap(), &[]).await;
    assert!(off.get(&justin.key, "/api/me").await.ok().get("god_mode").is_none());
    assert!(off.me(&justin.key).await.get("god_mode").is_none());
    off.act(&justin.key, road_move(off.road_spot())).await.refused(StatusCode::FORBIDDEN);
    off.put(&justin.key, "/api/god-mode", json!({ "on": false }))
        .await
        .refused(StatusCode::NOT_FOUND);
}

// W16
#[tokio::test]
async fn people_dont_lay_roads_and_agents_still_do() {
    let (server, justin, _) = Server::start().await;
    let road = server.road_spot();

    let ann = server.person("Ann").await;
    for who in [&ann, &justin] {
        let before = server.me(&who.key).await["moves_left"].clone();
        server.act(&who.key, road_move(road)).await.refused(StatusCode::FORBIDDEN);
        assert_eq!(server.me(&who.key).await["moves_left"], before, "a refused move costs nothing");
    }
    assert_eq!(server.tile(road), None);

    let guest = server.guest().await;
    server.act(&guest, road_move(road)).await.ok();
    assert_eq!(server.tile(road), Some(Kind::Road));
}

// W56, W62
#[tokio::test]
async fn a_house_is_turned_off_once_someone_has_theirs_and_on_again_when_it_comes_down() {
    let (server, _, _) = Server::start().await;
    let ann = server.person("Ann").await;
    assert_eq!(server.me(&ann.key).await["full"], json!([]));

    let house = server.build(&ann, "house").await;
    assert_eq!(server.me(&ann.key).await["full"], json!(["house"]));
    let elsewhere = server.free_spots()[0];
    server.place(&ann, "house", elsewhere).await.refused(StatusCode::CONFLICT);

    server.clear(&ann, house).await.ok();
    assert_eq!(server.me(&ann.key).await["full"], json!([]));
}

// W56, W62
#[tokio::test]
async fn the_other_places_are_turned_off_at_three_and_a_guest_is_told_nothing() {
    let (server, _, _) = Server::start().await;
    let ann = server.person("Ann").await;
    server.build(&ann, "house").await;
    for kind in ["studio", "club", "studio"] {
        assert_eq!(server.me(&ann.key).await["full"], json!(["house"]));
        server.build_later(&ann, kind, 8);
    }
    // W90: and so are the newer kinds, which count toward the same 3.
    let all = [
        "house", "studio", "office", "workshop", "library", "club", "lounge", "cafe", "bar",
        "green",
    ];
    assert_eq!(server.me(&ann.key).await["full"], json!(all));

    let guest = server.guest().await;
    assert!(server.me(&guest).await.get("full").is_none());
}

// W76
#[tokio::test]
async fn god_mode_has_every_place_now_past_the_limits_and_for_nothing() {
    let (server, justin, _) = Server::start().await;
    let cafe = server.free_spots()[0];
    server.place(&justin, "cafe", cafe).await.refused(StatusCode::CONFLICT);

    server.switch(&justin, true).await;
    let me = server.me(&justin.key).await;
    assert_eq!(me["god_mode"], true);
    // W90: the newer kinds too.
    assert_eq!(
        me["places"],
        json!({ "house": 0, "studio": 0, "office": 0, "workshop": 0, "library": 0, "club": 0, "lounge": 0, "cafe": 0, "bar": 0, "green": 0 })
    );
    assert_eq!(me["moves_left"], me["moves_per_day"]);

    // The longest wait, with no house at all, and as many places as they like.
    let done = server.place(&justin, "cafe", cafe).await.ok();
    assert_eq!(done["cost"], 0);
    assert_eq!(done["moves_left"], me["moves_per_day"]);
    for kind in ["studio", "club", "cafe", "cafe"] {
        server.build(&justin, kind).await;
    }
    let after = server.me(&justin.key).await;
    assert_eq!(after["full"], json!([]));
    assert_eq!(after["moves_left"], me["moves_per_day"], "nothing was charged");
    assert_eq!(after["your_town"]["places"], 5);

    // A house is still one, and the toolbar is told so.
    server.build(&justin, "house").await;
    assert_eq!(server.me(&justin.key).await["full"], json!(["house"]));
    let elsewhere = server.free_spots()[0];
    server.place(&justin, "house", elsewhere).await.refused(StatusCode::CONFLICT);

    // Roads too, for nothing.
    let road = server.road_spot();
    let done = server.act(&justin.key, road_move(road)).await.ok();
    assert_eq!(done["cost"], 0);
    assert_eq!(server.tile(road), Some(Kind::Road));
}

// W103
#[tokio::test]
async fn god_mode_moves_a_place_and_its_room_goes_with_it() {
    let (server, justin, _) = Server::start().await;
    let ann = server.person("Ann").await;
    let house = server.build(&ann, "house").await;
    let room = text(&server.go_in(&ann.key, house).await.ok()["hangout_id"]);
    let outside = json!({ "parts": { "door": { "style": "arched" } } });
    let path = |(x, y): (i64, i64)| format!("/api/places/{x}/{y}/outside");
    server.put(&ann.key, &path(house), outside.clone()).await.ok();
    let to = server.free_spots().into_iter().find(|spot| *spot != house).unwrap();
    let relocate = |from: (i64, i64), to: (i64, i64)| json!({ "action": "relocate", "x": from.0, "y": from.1, "x2": to.0, "y2": to.1 });

    // Not for its builder, and not with the switch off.
    server.act(&ann.key, relocate(house, to)).await.refused(StatusCode::FORBIDDEN);
    server.act(&justin.key, relocate(house, to)).await.refused(StatusCode::FORBIDDEN);
    server.switch(&justin, true).await;
    // Where it goes needs x2 and y2, open land and a road beside it.
    let half = json!({ "action": "relocate", "x": house.0, "y": house.1, "x2": to.0 });
    server.act(&justin.key, half).await.refused(StatusCode::BAD_REQUEST);
    server.act(&justin.key, relocate(house, house)).await.refused(StatusCode::BAD_REQUEST);
    let road = server.highway_tile();
    server.act(&justin.key, relocate(house, road)).await.refused(StatusCode::BAD_REQUEST);
    server.act(&justin.key, relocate(road, to)).await.refused(StatusCode::BAD_REQUEST);

    assert_eq!(server.act(&justin.key, relocate(house, to)).await.ok()["cost"], 0);
    assert_eq!(server.tile(house), None);
    assert_eq!(server.tile(to), Some(Kind::House));
    // Still Ann's, and the same room is behind its door. The old tile opens nothing.
    assert_eq!(server.go_in(&ann.key, to).await.ok()["hangout_id"], room.as_str());
    // Its outside went with it (E30).
    assert_eq!(server.get(&ann.key, &path(to)).await.ok()["outside"], outside);
    server.go_in(&ann.key, house).await.refused(StatusCode::NOT_FOUND);

    // W91: a big place moves from any of its tiles, and its room goes with it too.
    let (square, square_room) = server.town_square(&justin).await;
    let footprint = |(x, y): (i64, i64)| [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
    let away = server
        .big_spots()
        .into_iter()
        .find(|spot| footprint(*spot).iter().all(|cell| !footprint(square).contains(cell)))
        .unwrap();
    server.act(&justin.key, relocate((square.0 + 1, square.1 + 1), away)).await.ok();
    assert_eq!(server.tile(square), None);
    assert_eq!(server.tile(away), Some(Kind::Square));
    assert_eq!(server.go_in(&justin.key, away).await.ok()["hangout_id"], square_room.as_str());
}

// W77
#[tokio::test]
async fn god_mode_takes_down_what_anyone_built_and_their_room_waits_for_them() {
    let (server, justin, _) = Server::start().await;
    let ann = server.person("Ann").await;
    let house = server.build(&ann, "house").await;
    let room = text(&server.go_in(&ann.key, house).await.ok()["hangout_id"]);
    let guest = server.guest().await;
    let road = server.road_spot();
    server.act(&guest, road_move(road)).await.ok();

    // With it off, W17 and W57 hold.
    server.clear(&justin, house).await.refused(StatusCode::BAD_REQUEST);
    server.clear(&justin, road).await.refused(StatusCode::BAD_REQUEST);

    server.switch(&justin, true).await;
    assert_eq!(server.clear(&justin, house).await.ok()["cost"], 0);
    assert_eq!(server.tile(house), None);
    server.clear(&justin, road).await.ok();
    assert_eq!(server.tile(road), None);

    // Ann can put it up again and gets the same room (S35).
    let house = server.build(&ann, "house").await;
    assert_eq!(server.go_in(&ann.key, house).await.ok()["hangout_id"], room.as_str());
}

// W77
#[tokio::test]
async fn god_mode_leaves_the_highway_and_a_road_a_place_needs() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let highway = server.highway_tile();
    server.clear(&justin, highway).await.refused(StatusCode::BAD_REQUEST);
    assert_eq!(server.tile(highway), Some(Kind::Road));

    let (first, last, spot) = server.branch();
    let line = json!({ "action": "road", "x": first.0, "y": first.1, "x2": last.0, "y2": last.1 });
    server.act(&justin.key, line).await.ok();
    let ann = server.person("Ann").await;
    server.place(&ann, "house", spot).await.ok();
    server.clear(&justin, first).await.refused(StatusCode::BAD_REQUEST);
    assert_eq!(server.tile(first), Some(Kind::Road));

    // The way to move a street: the place goes first.
    server.clear(&justin, spot).await.ok();
    server.clear(&justin, first).await.ok();
    assert_eq!(server.tile(first), None);
}

// W78
#[tokio::test]
async fn god_mode_doesnt_change_the_land_or_say_anything_to_anyone() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
    let size = world::COMMONS_SIZE as u16;
    let all = (0..size).flat_map(|x| (0..size).map(move |y| (x, y)));
    let water = all
        .clone()
        .find(|pos| land.at(*pos) == Some(Ground::Water))
        .expect("The Commons has water");
    let water = (i64::from(water.0), i64::from(water.1));
    server.place(&justin, "house", water).await.refused(StatusCode::BAD_REQUEST);
    server.act(&justin.key, road_move(water)).await.refused(StatusCode::BAD_REQUEST);

    // A place needs a road beside it, and a road has to touch one.
    let tiles = server.state.db.world_tiles(COMMONS).unwrap();
    let road_near = |x: i64, y: i64| {
        (-2..=2)
            .any(|dx| (-2..=2).any(|dy| tiles.contains_key(&((x + dx) as u16, (y + dy) as u16))))
    };
    let empty = all
        .map(|(x, y)| (i64::from(x), i64::from(y)))
        .find(|&(x, y)| {
            (2..126).contains(&x)
                && (2..126).contains(&y)
                && land.at((x as u16, y as u16)).is_some_and(|ground| ground.is_open())
                && !road_near(x, y)
        })
        .expect("open land away from the roads");
    server.place(&justin, "house", empty).await.refused(StatusCode::BAD_REQUEST);
    server.act(&justin.key, road_move(empty)).await.refused(StatusCode::BAD_REQUEST);

    // A place they put up is theirs like any other, and nothing says how.
    let spot = server.build(&justin, "cafe").await;
    let view = server.view().await;
    assert!(
        view["owners"].as_array().unwrap().contains(&json!([
            spot.0,
            spot.1,
            "Justin",
            "Justin's Café"
        ])),
        "{}",
        view["owners"]
    );
    assert!(!view.to_string().contains("god"));
}

// W79, W95, L13
#[tokio::test]
async fn a_bot_gets_no_limits_from_its_owners_switch_and_none_of_its_powers() {
    let (server, justin, _) = Server::start().await;
    let hall = server
        .post(&justin.key, "/api/hangouts", json!({ "name": "Hall", "create_code": CODE }))
        .await
        .ok();
    let (_, fern) = server.bot(&justin, &text(&hall["hangout"]["id"]), "Fern").await;
    server.switch(&justin, true).await;

    // W95: as many projects as it wants, each with its building at once, past Justin's 3 and
    // with no wait, through his own player.
    for n in 0..5 {
        let started =
            server.tool(&fern, "start_project", json!({ "name": format!("Project {n}") })).await;
        let spot = server.free_spots()[0];
        let at = json!({ "project_id": started["project_id"], "x": spot.0, "y": spot.1, "purpose": "A place to build a garden robot together." });
        server.tool(&fern, "build_studio", at).await;
        assert_eq!(server.tile(spot), Some(Kind::Studio), "project {n}");
    }

    let capacity = server.tool(&fern, "inbox", json!({})).await;
    assert_eq!(capacity["project_capacity"]["can_start"], true);
    assert!(capacity["project_capacity"]["limit"].is_null());
    assert!(capacity["project_capacity"].get("god_mode").is_none());
    assert!(capacity["project_capacity"].get("exempt").is_none());
    assert!(capacity["project_capacity"]["remaining"].is_null());

    // Its own player is an agent's, with no move limit, and none of god mode's powers (W79).
    let joined = server.post(&fern, "/api/public/join", json!({})).await.ok();
    let player = text(&joined["player_token"]);
    let before = server.me(&player).await;
    assert!(before.get("god_mode").is_none());
    assert_eq!(server.act(&player, road_move(server.road_spot())).await.ok()["cost"], 0);
    let spot = server.free_spots()[0];
    let place = json!({ "action": "place", "kind": "cafe", "x": spot.0, "y": spot.1 });
    server.act(&player, place).await.refused(StatusCode::FORBIDDEN);
    let highway = server.highway_tile();
    let clear = json!({ "action": "bulldoze", "x": highway.0, "y": highway.1 });
    assert!(server.act(&player, clear).await.status.is_client_error(), "it can't take the highway");

    // With the switch off, its limits are back.
    server.switch(&justin, false).await;
    let capacity = server.tool(&fern, "inbox", json!({})).await;
    assert_eq!(capacity["project_capacity"]["can_start"], false);
    assert_eq!(capacity["project_capacity"]["limit"], 3);
    assert_eq!(capacity["project_capacity"]["remaining"], 0);
    let refused = server.call(&fern, "start_project", json!({ "name": "One more" })).await;
    assert_eq!(refused["isError"], true, "{refused}");
    let wave = json!({ "action": "emote", "emote": "wave" });
    assert_eq!(server.act(&player, wave).await.ok()["cost"], 1);
}

// W79, I19
#[tokio::test]
async fn every_item_is_unlocked_while_it_is_on_and_what_was_placed_stays() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let house = server.build(&justin, "house").await;
    let room = text(&server.go_in(&justin.key, house).await.ok()["hangout_id"]);
    let items = format!("/api/floors/{room}/items");
    let catalog = server.get(&justin.key, "/api/items").await.ok();
    let locked =
        catalog["items"].as_array().unwrap().iter().filter(|item| item["unlocked"] != true).count();
    assert_eq!(locked, 0, "{catalog}");
    server.post(&justin.key, &items, json!({ "kind": "fireplace", "x": 30, "y": 2 })).await.ok();

    server.switch(&justin, false).await;
    let catalog = server.get(&justin.key, "/api/items").await.ok();
    let fireplace = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "fireplace")
        .unwrap()
        .clone();
    assert_eq!(fireplace["unlocked"], false);
    assert_eq!(fireplace["friends_needed"], 6);
    server
        .post(&justin.key, &items, json!({ "kind": "fireplace", "x": 34, "y": 2 }))
        .await
        .refused(StatusCode::FORBIDDEN);
    let snapshot = server.get(&justin.key, &format!("/api/hangouts/{room}")).await.ok();
    assert_eq!(snapshot["items"].as_array().map(Vec::len), Some(1), "{snapshot}");
}

// I128
#[tokio::test]
async fn only_someone_with_god_mode_on_puts_up_a_lific_board() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let house = server.build(&justin, "house").await;
    let room = text(&server.go_in(&justin.key, house).await.ok()["hangout_id"]);
    let items = format!("/api/floors/{room}/items");
    server.post(&justin.key, &items, json!({ "kind": "lific_board", "x": 30, "y": 2 })).await.ok();

    // With it off, the same person can't put up another, and the catalog says why.
    server.switch(&justin, false).await;
    let catalog = server.get(&justin.key, "/api/items").await.ok();
    let entry = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "lific_board")
        .unwrap()
        .clone();
    assert_eq!((&entry["unlocked"], &entry["god_needed"]), (&json!(false), &json!(true)));
    let another = json!({ "kind": "lific_board", "x": 34, "y": 2 });
    server.post(&justin.key, &items, another.clone()).await.refused(StatusCode::FORBIDDEN);

    // A bot never can, even with its owner's switch on.
    server.switch(&justin, true).await;
    let (_, token) = server.bot(&justin, &room, "Scout").await;
    server.post(&token, &items, another).await.refused(StatusCode::FORBIDDEN);
    let snapshot = server.get(&justin.key, &format!("/api/hangouts/{room}")).await.ok();
    let boards =
        snapshot["items"].as_array().unwrap().iter().filter(|item| item["kind"] == "lific_board");
    assert_eq!(boards.count(), 1, "{snapshot}");
}

// W79, W80, S34
#[tokio::test]
async fn turning_it_off_takes_nothing_down_and_the_limits_come_back() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let house = server.build(&justin, "house").await;
    let mut places = Vec::new();
    for kind in ["studio", "club", "cafe", "cafe", "club"] {
        places.push(server.build(&justin, kind).await);
    }
    // A place past the limit opens onto a room of its own.
    let mut rooms = Vec::new();
    for place in &places {
        rooms.push(text(&server.go_in(&justin.key, *place).await.ok()["hangout_id"]));
    }
    rooms.sort();
    rooms.dedup();
    assert_eq!(rooms.len(), 5, "{rooms:?}");

    server.switch(&justin, false).await;
    assert_eq!(server.tile(house), Some(Kind::House));
    for place in &places {
        assert!(server.tile(*place).is_some());
        server.go_in(&justin.key, *place).await.ok();
    }
    let me = server.me(&justin.key).await;
    assert!(me.get("god_mode").is_none());
    // W90: the newer kinds count toward the same 3.
    let all = [
        "house", "studio", "office", "workshop", "library", "club", "lounge", "cafe", "bar",
        "green",
    ];
    assert_eq!(me["full"], json!(all));
    assert!(me["places"]["cafe"].as_i64().unwrap() > 0, "their wait is what it would have been");
    let spot = server.free_spots()[0];
    server.place(&justin, "studio", spot).await.refused(StatusCode::CONFLICT);
    server.place(&justin, "house", spot).await.refused(StatusCode::CONFLICT);
    server.act(&justin.key, road_move(server.road_spot())).await.refused(StatusCode::FORBIDDEN);
}

// W81
#[tokio::test]
async fn only_god_mode_puts_up_the_town_square_and_a_world_has_one() {
    let (server, justin, morg) = Server::start().await;
    let spot = server.big_spots()[0];
    let ann = server.person("Ann").await;
    let guest = server.guest().await;
    server.place(&ann, "square", spot).await.refused(StatusCode::FORBIDDEN);
    server.place(&justin, "square", spot).await.refused(StatusCode::FORBIDDEN);
    let place = json!({ "action": "place", "kind": "square", "x": spot.0, "y": spot.1 });
    server.act(&guest, place).await.refused(StatusCode::FORBIDDEN);
    assert_eq!(server.tile(spot), None);

    server.switch(&justin, true).await;
    assert_eq!(server.me(&justin.key).await["full"], json!([]));
    assert_eq!(server.place(&justin, "square", spot).await.ok()["cost"], 0);
    assert_eq!(server.tile(spot), Some(Kind::Square));
    assert_eq!(server.me(&justin.key).await["full"], json!(["square"]));

    // One to a world, for anyone with the switch.
    server.switch(&morg, true).await;
    let elsewhere = server.big_spots()[0];
    server.place(&justin, "square", elsewhere).await.refused(StatusCode::CONFLICT);
    server.place(&morg, "square", elsewhere).await.refused(StatusCode::CONFLICT);

    // It's a place like any other on the map, whose owner the view names.
    let view = server.view().await;
    assert!(
        view["tiles"].as_array().unwrap().contains(&json!([spot.0, spot.1, "square", 0])),
        "the view shows it"
    );
    assert!(
        view["owners"].as_array().unwrap().contains(&json!([
            spot.0,
            spot.1,
            "Justin",
            "Town Square"
        ])),
        "{}",
        view["owners"]
    );

    // Their own 3 aren't touched by it.
    server.switch(&justin, false).await;
    assert_eq!(server.me(&justin.key).await["full"], json!([]));

    // Once it's down there's room for another, anywhere.
    server.clear(&justin, spot).await.ok();
    assert_eq!(server.me(&morg.key).await["full"], json!([]));
    server.place(&morg, "square", elsewhere).await.ok();
}

// W81, W18
#[tokio::test]
async fn a_town_square_comes_down_with_its_builder() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;

    let player = server.state.db.own_player(&justin.id, now()).unwrap();
    assert_eq!(server.state.db.remove_from_world(COMMONS, player.id, false, true).unwrap(), 1);
    assert_eq!(server.tile(spot), None);
}

// W82, S34, S48
#[tokio::test]
async fn the_town_square_opens_onto_one_big_public_room_that_comes_back() {
    let (server, justin, _) = Server::start().await;
    let ann = server.person("Ann").await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;

    // Its door works for its owner first, with no code and with their switch off by then.
    server.go_in(&ann.key, spot).await.refused(StatusCode::NOT_FOUND);
    server.switch(&justin, false).await;
    let room = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);

    // Then anyone signed in can go in.
    assert_eq!(server.go_in(&ann.key, spot).await.ok()["hangout_id"], room.as_str());
    let floors = server.get(&ann.key, "/api/spaces/public").await.ok();
    let floor = floors["floors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|floor| floor["id"] == room.as_str())
        .unwrap()
        .clone();
    assert_eq!(floor["space_name"], "Town Square");
    assert_eq!(floor["size"], "big");
    assert_eq!(floor["capacity"], 150);

    // An ordinary place doesn't take over its room, and the square gets it back wherever it goes.
    server.switch(&justin, true).await;
    server.clear(&justin, spot).await.ok();
    let elsewhere = |taken: &[(i64, i64)]| {
        server.free_spots().into_iter().find(|at| !taken.contains(at)).unwrap()
    };
    let cafe = elsewhere(&[spot]);
    server.place(&justin, "cafe", cafe).await.ok();
    assert_ne!(server.go_in(&justin.key, cafe).await.ok()["hangout_id"], room.as_str());
    // W91: somewhere with room for all four of its tiles.
    let moved = server.big_spots().into_iter().find(|at| ![spot, cafe].contains(at)).unwrap();
    server.place(&justin, "square", moved).await.ok();
    assert_eq!(server.go_in(&justin.key, moved).await.ok()["hangout_id"], room.as_str());
}

// L18, S28
#[tokio::test]
async fn public_floors_list_the_town_square_first_while_it_stands() {
    let (server, justin, _) = Server::start().await;
    let (spot, square) = server.town_square(&justin).await;

    // A newer floor with as many people on it still comes after it.
    let (busy, cy) = server.someone("Cy").await;
    server
        .put(
            &cy.key,
            &format!("/api/floors/{busy}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();

    let (room, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &room, "Rover").await;
    server.all_day(&bo, &rover_id, true).await;

    let listed = server.get(&bo.key, "/api/spaces/public").await.ok()["floors"].clone();
    assert_eq!(listed[0]["id"], square.as_str());
    assert_eq!(listed[0]["town_square"], true);
    assert_eq!(listed[1]["id"], busy.as_str());
    assert_eq!(listed[1]["town_square"], false);

    let tool = server.tool(&rover, "public_floors", json!({})).await;
    assert_eq!(tool["places"][0]["id"], square.as_str(), "{tool}");
    assert_eq!(tool["places"][0]["town_square"], true, "{tool}");

    // With its building down the room is only a room.
    server.clear(&justin, spot).await.ok();
    let listed = server.get(&bo.key, "/api/spaces/public").await.ok()["floors"].clone();
    let room = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|floor| floor["id"] == square.as_str())
        .unwrap()
        .clone();
    assert_eq!(room["town_square"], false);
}

// W105
#[tokio::test]
async fn map_places_carry_the_id_of_their_public_floor_and_private_ones_carry_none() {
    let (server, justin, _) = Server::start().await;
    let (square_spot, square) = server.town_square(&justin).await;

    // A house is a private floor, so the map says nothing more about it.
    let ann = server.person("Ann").await;
    let house = server.build(&ann, "house").await;
    server.go_in(&ann.key, house).await.ok();

    // A club whose floor its owner opens to anyone.
    let cy = server.person("Cy").await;
    server.build(&cy, "house").await;
    let club = server.build_later(&cy, "club", 8);
    let club_floor = text(&server.go_in(&cy.key, club).await.ok()["hangout_id"]);
    server
        .put(
            &cy.key,
            &format!("/api/floors/{club_floor}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();

    let (room, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &room, "Rover").await;
    server.all_day(&bo, &rover_id, true).await;

    let city = server.tool(&rover, "list_city", json!({})).await;
    let places = city["places"].as_array().unwrap();
    let at = |(x, y): (i64, i64)| {
        places
            .iter()
            .find(|place| place["x"] == x && place["y"] == y)
            .unwrap_or_else(|| panic!("{city}"))
    };
    assert_eq!(at(square_spot)["floor_id"], square.as_str(), "{city}");
    assert_eq!(at(club)["floor_id"], club_floor.as_str(), "{city}");
    assert!(at(house).get("floor_id").is_none(), "{city}");

    // Every id on the map is one public_floors lists.
    let listed = server.tool(&rover, "public_floors", json!({})).await;
    let listed: Vec<&Value> =
        listed["places"].as_array().unwrap().iter().map(|f| &f["id"]).collect();
    for place in places {
        if let Some(id) = place.get("floor_id") {
            assert!(listed.contains(&id), "{id} is not in public_floors: {listed:?}");
        }
    }
}

/// A time in a half hour whose number leaves `rest` over `turns`, from the next one on, 11 minutes
/// in so a bot that just did something isn't busy (L31).
fn half_hour(turns: i64, rest: i64) -> i64 {
    let mut slot = now() / 1800 + 1;
    while slot % turns != rest {
        slot += 1;
    }
    slot * 1800 + 11 * 60
}

// L19, L41
#[tokio::test]
async fn a_bot_with_no_project_meets_at_the_town_square_and_explores_in_turn() {
    let (server, justin, _) = Server::start().await;
    let (cafe, cy) = server.someone("Cy").await;
    server
        .put(
            &cy.key,
            &format!("/api/floors/{cafe}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();
    let (_, square) = server.town_square(&justin).await;

    // In a meeting half hour, however many bots there are, they all come here.
    let meeting = half_hour(2, 0);
    let (room, bo) = server.someone("Bo").await;
    for name in ["Rover", "Fido", "Patch"] {
        let (bot_id, bot) = server.bot(&bo, &room, name).await;
        server.all_day(&bo, &bot_id, true).await;
        routines::wander_at(&server.state, &bot_id, &bo.id, meeting).unwrap();
        assert_eq!(server.out_at(&bot).await, square.as_str(), "{name}");
    }

    // That goes for a bot whose owner is a member there too, because a square isn't their room.
    let hall = server
        .post(&justin.key, "/api/hangouts", json!({ "name": "Hall", "create_code": CODE }))
        .await
        .ok();
    let (fern_id, fern) = server.bot(&justin, &text(&hall["hangout"]["id"]), "Fern").await;
    server.all_day(&justin, &fern_id, true).await;
    routines::wander_at(&server.state, &fern_id, &justin.id, meeting).unwrap();
    assert_eq!(server.out_at(&fern).await, square.as_str());

    // The half hour after, it explores: here the only other place is Cy's cafe.
    let (scout_id, scout) = server.bot(&bo, &room, "Scout").await;
    server.all_day(&bo, &scout_id, true).await;
    routines::wander_at(&server.state, &scout_id, &bo.id, half_hour(2, 1)).unwrap();
    assert_eq!(server.out_at(&scout).await, cafe.as_str());
}

// L47, L48
#[tokio::test]
async fn a_bot_with_a_workplace_works_there_and_comes_back_to_it() {
    let (server, justin, _) = Server::start().await;
    let (_, square) = server.town_square(&justin).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover").await;
    server.all_day(&bo, &rover_id, true).await;
    let office = server
        .post(&bo.key, "/api/hangouts", json!({ "name": "Office", "create_code": CODE }))
        .await
        .ok();
    let office = text(&office["hangout"]["id"]);
    let path = format!("/api/agents/{rover_id}/workplace");

    // Only a room it belongs to, with a job, set by its owner as a person.
    let job = json!({ "room_id": office, "job": "Building Agents Hangout" });
    server.put(&bo.key, &path, job.clone()).await.refused(StatusCode::BAD_REQUEST);
    server.state.db.add_agent_to_room(&office, &rover_id).unwrap();
    for bad in [
        "",
        "  ",
        "two
lines",
    ] {
        let body = json!({ "room_id": office, "job": bad });
        server.put(&bo.key, &path, body).await.refused(StatusCode::BAD_REQUEST);
    }
    let long = json!({ "room_id": office, "job": "x".repeat(201) });
    server.put(&bo.key, &path, long).await.refused(StatusCode::BAD_REQUEST);
    server.put(&rover, &path, job.clone()).await.refused(StatusCode::FORBIDDEN);
    let cy = server.person("Cy").await;
    server.put(&cy.key, &path, job.clone()).await.refused(StatusCode::NOT_FOUND);

    // Set, it goes there now, and its owner can pick it from the rooms it belongs to.
    let set = server.put(&bo.key, &path, job).await.ok();
    assert_eq!(set["workplace"]["room_id"], office.as_str(), "{set}");
    assert_eq!(set["workplace"]["job"], "Building Agents Hangout");
    assert_eq!(
        server.state.db.bot_place(&rover_id, now()).unwrap().as_deref(),
        Some(office.as_str())
    );
    let routine = server.get(&bo.key, &format!("/api/agents/{rover_id}/routine")).await.ok();
    let choices: Vec<&str> = routine["workplace_choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|room| room["id"].as_str().unwrap())
        .collect();
    assert!(choices.contains(&home.as_str()) && choices.contains(&office.as_str()), "{routine}");
    // The bot reads it, with its job, but not its owner's choices.
    let me = server.get(&rover, "/api/me").await.ok();
    assert_eq!(me["routine"]["workplace"]["name"], "Office", "{me}");
    assert!(me["routine"].get("workplace_choices").is_none());

    // In a meeting half hour and an exploring one it stays at work, not the town square.
    for at in [half_hour(2, 0), half_hour(2, 1)] {
        routines::wander_at(&server.state, &rover_id, &bo.id, at).unwrap();
        assert!(server.out_at(&rover).await.is_null());
        assert_eq!(
            server.state.db.bot_place(&rover_id, now()).unwrap().as_deref(),
            Some(office.as_str())
        );
    }
    let bots = server.get(&bo.key, "/api/me/bots").await.ok();
    assert_eq!(bots["bots"][0]["next"]["way"], "work", "{bots}");

    // L49: its half-hour call is to its job there, in its owner's words, and nudge shows it.
    let call = routines::call_of(&server.state, &rover_id, &bo.id, now()).unwrap().unwrap();
    assert_eq!((call.event, call.hangout_id.as_str()), ("work", office.as_str()));
    assert!(
        call.text.contains("Building Agents Hangout") && call.text.contains("Office"),
        "{call:?}"
    );
    assert!(!call.text.contains("  "), "{call:?}");
    let inbox = server.tool(&rover, "inbox", json!({})).await;
    assert_eq!(inbox["nudge"]["event"], "work", "{inbox}");
    // Somewhere else, even the town square, it's told to go back rather than to think up ideas.
    server.tool(&rover, "go_out", json!({ "floor_id": square })).await;
    let call = routines::call_of(&server.state, &rover_id, &bo.id, now()).unwrap().unwrap();
    assert_eq!((call.event, call.hangout_id.as_str()), ("work", office.as_str()));
    assert!(call.text.contains("go_to_room"), "{call:?}");
    server.tool(&rover, "come_back", json!({})).await;
    server.state.db.set_bot_place(&rover_id, &office, now()).unwrap();

    // Called home for a standup that runs past the half hour, it stays while it has the standup's
    // mention to read (L31), and comes back to work at the next half hour once it's free.
    server.state.db.set_bot_place(&rover_id, &home, now()).unwrap();
    let say = json!({ "text": "@Rover standup: what are you working on?" });
    server.post(&bo.key, &format!("/api/hangouts/{home}/messages"), say).await.ok();
    routines::wander_at(&server.state, &rover_id, &bo.id, half_hour(2, 0)).unwrap();
    assert_eq!(
        server.state.db.bot_place(&rover_id, now()).unwrap().as_deref(),
        Some(home.as_str())
    );
    server.tool(&rover, "read_room", json!({ "hangout_id": home })).await;
    assert!(server.state.db.unread_mentions(&rover_id).unwrap().is_empty());
    routines::wander_at(&server.state, &rover_id, &bo.id, half_hour(2, 0)).unwrap();
    assert_eq!(
        server.state.db.bot_place(&rover_id, now()).unwrap().as_deref(),
        Some(office.as_str())
    );

    // Taken out of the room, it's no workplace, and the bot wanders again.
    server.state.db.remove_agent_from_room(&office, &rover_id).unwrap();
    let me = server.get(&rover, "/api/me").await.ok();
    assert!(me["routine"]["workplace"].is_null(), "{me}");
    routines::wander_at(&server.state, &rover_id, &bo.id, half_hour(2, 0)).unwrap();
    assert_eq!(server.out_at(&rover).await, square.as_str());

    // Its owner clears it.
    server.state.db.add_agent_to_room(&office, &rover_id).unwrap();
    let cleared = server.send(&bo.key, Method::DELETE, &path, None).await.ok();
    assert!(cleared["workplace"].is_null(), "{cleared}");
}

// L57: a bot quiet at its workplace in a Work hour isn't sent home after S61's half hour.
#[tokio::test]
async fn a_bot_quiet_at_work_stays_at_work() {
    let (server, _, _) = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, _) = server.bot(&bo, &home, "Rover").await;
    let (pip_id, _) = server.bot(&bo, &home, "Pip").await;
    let office = server
        .post(&bo.key, "/api/hangouts", json!({ "name": "Office", "create_code": CODE }))
        .await
        .ok();
    let office = text(&office["hangout"]["id"]);
    for bot in [&rover_id, &pip_id] {
        server.all_day(&bo, bot, true).await;
        server.state.db.add_agent_to_room(&office, bot).unwrap();
    }
    let job = json!({ "room_id": office, "job": "Building Agents Hangout" });
    server.put(&bo.key, &format!("/api/agents/{rover_id}/workplace"), job).await.ok();
    // Both went to the office 20 minutes ago and have done nothing since; only Rover works there.
    let start = now();
    for bot in [&rover_id, &pip_id] {
        server.state.db.set_bot_place(bot, &office, start - 20 * 60).unwrap();
    }
    agenthangout::travel::sweep(&server.state, start);
    let at = |bot: &str, when: i64| server.state.db.bot_place(bot, when).unwrap();
    // Fifteen minutes on, past S61's half hour, Rover is still at work and Pip has gone home.
    assert_eq!(at(&rover_id, start + 15 * 60).as_deref(), Some(office.as_str()));
    assert_ne!(at(&pip_id, start + 15 * 60).as_deref(), Some(office.as_str()));

    // Once its hours aren't Work hours, nothing starts its half hour again, and it runs out.
    let social = json!({ "every_day": vec!["social"; 24] });
    server.put(&bo.key, &format!("/api/agents/{rover_id}/schedule"), social).await.ok();
    agenthangout::travel::sweep(&server.state, start + 20 * 60);
    assert_ne!(at(&rover_id, start + 31 * 60).as_deref(), Some(office.as_str()));
}

// L41, L9
#[tokio::test]
async fn a_bot_explores_its_owners_places_too_and_meets_when_there_is_nowhere_else() {
    let (server, justin, _) = Server::start().await;
    let (_, square) = server.town_square(&justin).await;
    let hall = server
        .post(&justin.key, "/api/hangouts", json!({ "name": "Hall", "create_code": CODE }))
        .await
        .ok();
    let (fern_id, fern) = server.bot(&justin, &text(&hall["hangout"]["id"]), "Fern").await;
    server.tool(&fern, "start_project", json!({ "name": "Garden robot" })).await;
    server.all_day(&justin, &fern_id, true).await;

    // Exploring, the only public floor is the square, so it meets there: its owner belonging to it
    // doesn't keep it away.
    routines::wander_at(&server.state, &fern_id, &justin.id, half_hour(2, 1)).unwrap();
    assert_eq!(server.out_at(&fern).await, square.as_str());
    server.tool(&fern, "come_back", json!({})).await;

    // It can still go there itself (L6).
    server.tool(&fern, "go_out", json!({ "floor_id": square })).await;
    assert_eq!(server.out_at(&fern).await, square.as_str());
}

// L20
#[tokio::test]
async fn the_town_square_brings_nobody_out() {
    let (server, justin, _) = Server::start().await;
    server.town_square(&justin).await;
    let (room, bo) = server.someone("Bo").await;

    // No social hours.
    let (_, rover) = server.bot(&bo, &room, "Rover").await;
    server.post(&rover, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&rover).await.is_null());

    // All day, with public places off.
    let (fido_id, fido) = server.bot(&bo, &room, "Fido").await;
    server.all_day(&bo, &fido_id, false).await;
    server.post(&fido, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&fido).await.is_null());
}

// L18, W81: a square that hasn't been through its door has no room, so nothing lists it.
#[tokio::test]
async fn a_town_square_nobody_has_gone_into_has_no_room_to_list() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    server.build(&justin, "square").await;
    let ann = server.person("Ann").await;
    assert_eq!(server.get(&ann.key, "/api/spaces/public").await.ok()["floors"], json!([]));
}

// W73, W82: the town square is called Town Square, before and after its owner has been through.
#[tokio::test]
async fn the_town_square_is_called_town_square_with_or_without_a_visit() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    let name_at = |view: &Value| {
        let row = view["owners"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row[0] == spot.0 && row[1] == spot.1)
            .unwrap()
            .clone();
        (text(&row[2]), text(&row[3]))
    };

    // Nobody has been through its door, so the name isn't "Justin's Studio" or "Justin's Town Square".
    let (owner, name) = name_at(&server.view().await);
    assert_eq!((owner.as_str(), name.as_str()), ("Justin", "Town Square"));

    // After the visit it is the room's name, and the owner changing that changes the map (S7).
    let door = server.go_in(&justin.key, spot).await.ok();
    assert_eq!(name_at(&server.view().await).1, "Town Square");
    let space = text(&door["space_id"]);
    server
        .put(&justin.key, &format!("/api/spaces/{space}"), json!({ "name": "Meeting Green" }))
        .await
        .ok();
    assert_eq!(name_at(&server.view().await).1, "Meeting Green");
}

/// The floors in the public list that say they are the town square's.
async fn square_floors(server: &Server, token: &str) -> Vec<String> {
    let floors = server.get(token, "/api/spaces/public").await.ok();
    floors["floors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|floor| floor["town_square"] == true)
        .map(|floor| text(&floor["id"]))
        .collect()
}

// W82: a square on a tile that was a cafe opens a room of its own, not the cafe's.
#[tokio::test]
async fn a_square_on_a_cafes_tile_opens_a_new_room_and_not_the_cafes() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "cafe").await;
    let cafe_room = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);
    server.clear(&justin, spot).await.ok();
    server.place(&justin, "square", spot).await.ok();
    let room = text(&server.go_in(&justin.key, spot).await.ok()["hangout_id"]);
    assert_ne!(room, cafe_room);
    assert_eq!(square_floors(&server, &justin.key).await, vec![room.clone()]);
    let floors = server.get(&justin.key, "/api/spaces/public").await.ok();
    let floor = floors["floors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|floor| floor["id"] == room.as_str())
        .unwrap()
        .clone();
    assert_eq!(floor["size"], "big");
    assert_eq!(floor["town_square"], true);
}

// W82: a cafe on the tile the square stood on never opens the square's room.
#[tokio::test]
async fn a_cafe_on_the_squares_old_tile_doesnt_get_its_room() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    let door = server.go_in(&justin.key, spot).await.ok();
    let (square_room, square_space) = (text(&door["hangout_id"]), text(&door["space_id"]));
    server.clear(&justin, spot).await.ok();
    server.place(&justin, "cafe", spot).await.ok();
    let door = server.go_in(&justin.key, spot).await.ok();
    assert_ne!(text(&door["hangout_id"]), square_room);
    assert!(!server.state.db.is_town_square_room(&text(&door["space_id"])).unwrap());

    server.clear(&justin, spot).await.ok();
    let elsewhere = server.free_spots().into_iter().find(|at| *at != spot).unwrap();
    server.place(&justin, "square", elsewhere).await.ok();
    let door = server.go_in(&justin.key, elsewhere).await.ok();
    assert_eq!(door["hangout_id"], square_room.as_str());
    assert!(server.state.db.is_town_square_room(&square_space).unwrap());
    assert_eq!(square_floors(&server, &justin.key).await, vec![square_room]);
}

// W82: which room is the square's is stored, not worked out from its name.
#[tokio::test]
async fn renaming_the_squares_room_keeps_it_the_squares_and_a_cafe_called_town_square_isnt() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let spot = server.build(&justin, "square").await;
    let door = server.go_in(&justin.key, spot).await.ok();
    let (room, space) = (text(&door["hangout_id"]), text(&door["space_id"]));
    server
        .put(&justin.key, &format!("/api/spaces/{space}"), json!({ "name": "Meeting Green" }))
        .await
        .ok();
    server.clear(&justin, spot).await.ok();
    let elsewhere = server.free_spots().into_iter().find(|at| *at != spot).unwrap();
    server.place(&justin, "square", elsewhere).await.ok();
    assert_eq!(server.go_in(&justin.key, elsewhere).await.ok()["hangout_id"], room.as_str());

    // Calling an ordinary room "Town Square" doesn't make it the square's, and it still comes back.
    let ann = server.person("Ann").await;
    server.build(&ann, "house").await;
    let cafe = server.build_later(&ann, "cafe", 8);
    let door = server.go_in(&ann.key, cafe).await.ok();
    let (cafe_room, cafe_space) = (text(&door["hangout_id"]), text(&door["space_id"]));
    server
        .put(&ann.key, &format!("/api/spaces/{cafe_space}"), json!({ "name": "Town Square" }))
        .await
        .ok();
    server.clear(&ann, cafe).await.ok();
    let again = server.free_spots().into_iter().find(|at| *at != spot && *at != elsewhere).unwrap();
    let at = now() + 9 * DAY;
    let player = server.state.db.own_player(&ann.id, at).unwrap();
    let mv = Move {
        action: "place".to_string(),
        x: Some(again.0),
        y: Some(again.1),
        x2: None,
        y2: None,
        kind: Some("cafe".to_string()),
        emote: None,
    };
    worlds::act(&server.state, &player, COMMONS, &mv, at).unwrap();
    assert_eq!(server.go_in(&ann.key, again).await.ok()["hangout_id"], cafe_room.as_str());
    assert!(!server.state.db.is_town_square_room(&cafe_space).unwrap());
}

// W79, W80, S34: a place past the limit, built with the switch on, gets its room with it off.
#[tokio::test]
async fn a_place_past_the_limit_opens_a_room_after_the_switch_goes_off() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let first = server.build(&justin, "cafe").await;
    let second = server.build(&justin, "club").await;
    let third = server.build(&justin, "studio").await;
    let fourth = server.build(&justin, "cafe").await;
    for at in [first, second, third] {
        server.go_in(&justin.key, at).await.ok();
    }
    server.switch(&justin, false).await;
    server.go_in(&justin.key, fourth).await.ok();
}

// W79, S34: holding the square and the 3 other places, a third other room still opens.
#[tokio::test]
async fn the_square_and_three_other_places_all_get_rooms_with_the_switch_off() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    server.build(&justin, "house").await;
    let square = server.build(&justin, "square").await;
    let cafe = server.build(&justin, "cafe").await;
    let club = server.build(&justin, "club").await;
    server.go_in(&justin.key, square).await.ok();
    server.go_in(&justin.key, cafe).await.ok();
    server.go_in(&justin.key, club).await.ok();
    server.switch(&justin, false).await;
    let studio = server.build_later(&justin, "studio", 8);
    server.go_in(&justin.key, studio).await.ok();
}

// W75, W81: someone without the switch is told nothing about it when they try the square.
#[tokio::test]
async fn placing_the_square_without_the_switch_is_refused_without_saying_why() {
    let (server, _, _) = Server::start().await;
    let ann = server.person("Ann").await;
    let spot = server.free_spots()[0];
    let reply = server.place(&ann, "square", spot).await.refused(StatusCode::FORBIDDEN);
    assert!(!reply.to_string().to_lowercase().contains("god"), "{reply}");
}

// W93: god mode lays roads anywhere, but not right alongside another.
#[tokio::test]
async fn god_mode_still_cant_lay_a_road_right_alongside_another() {
    let (server, justin, _) = Server::start().await;
    server.switch(&justin, true).await;
    let tiles = server.state.db.world_tiles(COMMONS).unwrap();
    let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
    let road =
        |x: i64, y: i64| tiles.get(&(x as u16, y as u16)).is_some_and(|t| t.kind == Kind::Road);
    let open = |x: i64, y: i64| {
        let pos = (x as u16, y as u16);
        !tiles.contains_key(&pos) && land.at(pos).is_some_and(|ground| ground.is_open())
    };
    // Two tiles of highway, one above the other, with open land to their right.
    let (x, y) = tiles
        .keys()
        .map(|&(x, y)| (i64::from(x), i64::from(y)))
        .find(|&(x, y)| {
            road(x, y)
                && road(x, y + 1)
                && (y..=y + 1).all(|v| open(x + 1, v))
                && !road(x + 1, y - 1)
                && !road(x + 1, y + 2)
        })
        .expect("a straight bit of highway");
    server.act(&justin.key, road_move((x + 1, y))).await.ok();
    let refused = server.act(&justin.key, road_move((x + 1, y + 1))).await;
    refused.refused(StatusCode::BAD_REQUEST);
    assert_eq!(server.tile((x + 1, y + 1)), None);
}

// L47, L48: a public floor it isn't part of can be its workplace too. It walks in there as a guest,
// and stays at work there in its hours.
#[tokio::test]
async fn a_public_floor_can_be_a_workplace_and_the_bot_walks_in_there() {
    let (server, justin, _) = Server::start().await;
    let (_, square) = server.town_square(&justin).await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, _) = server.bot(&bo, &home, "Rover").await;
    server.all_day(&bo, &rover_id, true).await;
    let path = format!("/api/agents/{rover_id}/workplace");
    assert!(!server.state.db.belongs_to_room(&square, &rover_id).unwrap());

    // Its owner can pick a public floor from the choices, as well as its own rooms.
    let routine = server.get(&bo.key, &format!("/api/agents/{rover_id}/routine")).await.ok();
    let choices = routine["workplace_choices"].as_array().unwrap();
    assert!(choices.iter().any(|c| c["id"] == square.as_str() && c["public"] == true), "{routine}");

    let set = server.put(&bo.key, &path, json!({ "room_id": square, "job": "Greeting people" }));
    let set = set.await.ok();
    assert_eq!(set["workplace"]["room_id"], square.as_str(), "{set}");
    // It walked in as a guest, not as a member.
    assert_eq!(
        server.state.db.bot_place(&rover_id, now()).unwrap().as_deref(),
        Some(square.as_str())
    );
    assert!(!server.state.db.belongs_to_room(&square, &rover_id).unwrap());
    assert!(server.state.db.bot_visits(&rover_id, now()).unwrap().contains(&square));

    // Sent home, the next half hour walks it back to work there rather than anywhere else.
    server.state.db.end_visit(&square, &rover_id).unwrap();
    for slot in 1..=3 {
        let at = now() + slot * agenthangout::routines::WANDER_SECS;
        agenthangout::routines::wander_at(&server.state, &rover_id, &bo.id, at).unwrap();
        assert!(
            server.state.db.bot_visits(&rover_id, now()).unwrap().contains(&square),
            "slot {slot}"
        );
        server.state.db.end_visit(&square, &rover_id).unwrap();
    }

    // A private room it isn't part of still can't be.
    let office = server
        .post(&bo.key, "/api/hangouts", json!({ "name": "Office", "create_code": CODE }))
        .await
        .ok();
    let office = text(&office["hangout"]["id"]);
    let refused = json!({ "room_id": office, "job": "Building" });
    server.put(&bo.key, &path, refused).await.refused(StatusCode::BAD_REQUEST);

    // Turned out of the square's space, its owner can't make it a workplace, and the one set
    // there stops counting, so the bot wanders as before rather than failing every minute.
    let space = server.state.db.floor(&square).unwrap().unwrap().space_id;
    server.state.db.turn_out(&space, &bo.id, &home, now()).unwrap();
    let routine = server.get(&bo.key, &format!("/api/agents/{rover_id}/routine")).await.ok();
    assert!(routine["workplace"].is_null(), "{routine}");
    let again = json!({ "room_id": square, "job": "Greeting people" });
    server.put(&bo.key, &path, again).await.refused(StatusCode::BAD_REQUEST);
    let at = now() + agenthangout::routines::WANDER_SECS;
    agenthangout::routines::wander_at(&server.state, &rover_id, &bo.id, at).unwrap();
}

/// L50: a day of blocks, written as each hour's name.
fn strip(hours: &[(std::ops::Range<usize>, &str)]) -> Vec<&'static str> {
    let mut day = vec!["anything"; 24];
    for (range, block) in hours {
        let block: &'static str = match *block {
            "work" => "work",
            "social" => "social",
            "home" => "home",
            _ => "anything",
        };
        for hour in range.clone() {
            day[hour] = block;
        }
    }
    day
}

// L50 to L56
#[tokio::test]
async fn a_bots_schedule_paints_its_day_with_work_social_home_and_anything() {
    let (server, justin, _) = Server::start().await;
    let (_, square) = server.town_square(&justin).await;
    let made = server
        .post("", "/api/cities", json!({ "name": "Elm", "your_name": "Bo", "create_code": CODE }))
        .await
        .ok();
    let city = text(&made["city"]["id"]);
    let bo = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
    let house = server
        .post(&bo.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Bo House" }))
        .await
        .ok();
    let living = text(&house["living_room_id"]);
    let (rover_id, rover) = server.bot(&bo, &living, "Rover").await;
    server.all_day(&bo, &rover_id, true).await;
    let office = server
        .post(&bo.key, "/api/hangouts", json!({ "name": "Office", "create_code": CODE }))
        .await
        .ok();
    let office = text(&office["hangout"]["id"]);
    server.state.db.add_agent_to_room(&office, &rover_id).unwrap();
    let job = json!({ "room_id": office, "job": "Building Agents Hangout" });
    server.put(&bo.key, &format!("/api/agents/{rover_id}/workplace"), job).await.ok();
    let routine_path = format!("/api/agents/{rover_id}/routine");
    let path = format!("/api/agents/{rover_id}/schedule");

    // L51: before one is saved, it's read off the routine: all day with a workplace is all Work.
    let read = server.get(&bo.key, &routine_path).await.ok();
    assert_eq!(read["schedule_saved"], false, "{read}");
    assert_eq!(read["schedule"]["every_day"], json!(vec!["work"; 24]));
    assert_eq!(read["block"], "work");

    // L50: only its owner, as a person, and only 24 known blocks per row.
    let day = strip(&[
        (0..8, "home"),
        (8..12, "work"),
        (12..13, "social"),
        (13..17, "work"),
        (17..19, "social"),
        (19..24, "home"),
    ]);
    for bad in [
        json!({ "every_day": vec!["work"; 23] }),
        json!({ "every_day": vec!["nap"; 24] }),
        json!({ "every_day": day, "days": { "someday": day } }),
        json!({ "every_day": day, "days": { "sat": vec!["work"; 2] } }),
    ] {
        server.put(&bo.key, &path, bad.clone()).await.refused(StatusCode::BAD_REQUEST);
    }
    let body = json!({ "every_day": day, "days": { "sat": vec!["anything"; 24] } });
    server.put(&rover, &path, body.clone()).await.refused(StatusCode::FORBIDDEN);
    let cy = server.person("Cy").await;
    server.put(&cy.key, &path, body.clone()).await.refused(StatusCode::NOT_FOUND);
    let saved = server.put(&bo.key, &path, body).await.ok();
    assert_eq!(saved["schedule_saved"], true, "{saved}");
    assert_eq!(saved["schedule"]["every_day"], json!(day));
    assert_eq!(saved["schedule"]["days"]["sat"], json!(vec!["anything"; 24]));
    // The bot reads its block and what it asks of it, with no clock saved for Bo, so in UTC.
    let me = server.get(&rover, "/api/me").await.ok();
    assert!(me["routine"]["block_how"].is_string(), "{me}");
    assert!(me["routine"]["next_block_at"].as_i64().unwrap() > now());

    // L53: a weekday that isn't a Saturday, 11 minutes into each hour, in the future.
    let mut day0 = (now() / DAY + 1) * DAY;
    while (day0 / DAY + 3).rem_euclid(7) == 5 {
        day0 += DAY;
    }
    let at = |hour: i64| day0 + hour * 3600 + 11 * 60;
    let place = || server.state.db.bot_place(&rover_id, now()).unwrap();
    routines::wander_at(&server.state, &rover_id, &bo.id, at(9)).unwrap();
    assert_eq!(place().as_deref(), Some(office.as_str()), "Work: its workplace");
    let call = routines::call_of(&server.state, &rover_id, &bo.id, at(9)).unwrap().unwrap();
    assert_eq!(call.event, "work");
    routines::wander_at(&server.state, &rover_id, &bo.id, at(12)).unwrap();
    assert_eq!(
        server.out_at(&rover).await,
        square.as_str(),
        "Social: out, though it has a workplace"
    );
    let call = routines::call_of(&server.state, &rover_id, &bo.id, at(12)).unwrap();
    assert!(call.is_none_or(|call| call.event != "work"), "a Social hour isn't called to work");
    // Still out there when the Home hour comes, as a walk-in kept going would be (L45).
    server.state.db.extend_visit(&square, &rover_id, at(21), now()).unwrap();
    routines::wander_at(&server.state, &rover_id, &bo.id, at(20)).unwrap();
    assert_eq!(place().as_deref(), Some(living.as_str()), "Home: its Living Room");
    assert!(server.out_at(&rover).await.is_null(), "and off the floor it was out on");
    routines::wander_at(&server.state, &rover_id, &bo.id, at(27)).unwrap();
    assert_eq!(place().as_deref(), Some(living.as_str()), "the next morning is still Home");

    // Anything: the Saturday override moves it nowhere, even in what's a Work hour on weekdays.
    let mut saturday = day0;
    while (saturday / DAY + 3).rem_euclid(7) != 5 {
        saturday += DAY;
    }
    routines::wander_at(&server.state, &rover_id, &bo.id, saturday + 9 * 3600 + 660).unwrap();
    assert_eq!(place().as_deref(), Some(living.as_str()), "Anything: it stays where it is");

    // L55: all day at work, another bot it follows doesn't take it away.
    let all_work = json!({ "every_day": vec!["work"; 24] });
    server.put(&bo.key, &path, all_work).await.ok();
    server.state.db.set_bot_place(&rover_id, &office, now()).unwrap();
    let (scout_id, scout) = server.bot(&bo, &office, "Scout").await;
    server.state.db.add_agent_to_room(&living, &scout_id).unwrap();
    server.tool(&rover, "follow", json!({ "bot": "Scout" })).await;
    server.tool(&scout, "go_to_room", json!({ "hangout_id": living })).await;
    assert_eq!(place().as_deref(), Some(office.as_str()), "at work, it stays");
    // Its owner can still call it away.
    let bring = format!("/api/hangouts/{living}/agents/{rover_id}");
    server.put(&bo.key, &bring, json!({})).await.ok();
    assert_eq!(place().as_deref(), Some(living.as_str()), "its owner calls it away");

    // L51: deleting it goes back to the routine's window.
    let cleared = server.send(&bo.key, Method::DELETE, &path, None).await.ok();
    assert_eq!(cleared["schedule_saved"], false, "{cleared}");

    // RC2: at the recycling plant, a Home hour doesn't put it back in its Living Room.
    let all_home = json!({ "every_day": vec!["home"; 24] });
    server.put(&bo.key, &path, all_home.clone()).await.ok();
    server.post(&bo.key, &format!("/api/agents/{rover_id}/recycle"), json!({})).await.ok();
    routines::wander_at(&server.state, &rover_id, &bo.id, at(30)).unwrap();
    assert_ne!(place().as_deref(), Some(living.as_str()), "a recycled bot goes nowhere");
    server.put(&bo.key, &path, all_home).await.refused(StatusCode::CONFLICT);
}

// L56: a bot that stays home and a house bot have no plan of their own: every hour is Home, a plan
// can't be saved for them, and one saved before it stayed home moves it nowhere.
#[tokio::test]
async fn a_bot_that_stays_home_or_a_house_bot_has_no_plan_of_its_own() {
    let (server, _, _) = Server::start().await;
    let made = server
        .post("", "/api/cities", json!({ "name": "Elm", "your_name": "Bo", "create_code": CODE }))
        .await
        .ok();
    let city = text(&made["city"]["id"]);
    let bo = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
    let house = server
        .post(&bo.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Bo House" }))
        .await
        .ok();
    let house_id = text(&house["house"]["id"]);
    let living = text(&house["living_room_id"]);
    let all_work = json!({ "every_day": vec!["work"; 24] });

    // A plan saved while it went out, and then it's kept home.
    let (homer, _) = server.bot(&bo, &living, "Homer").await;
    server.all_day(&bo, &homer, true).await;
    server.put(&bo.key, &format!("/api/agents/{homer}/schedule"), all_work.clone()).await.ok();
    server.state.db.set_bot_home(&homer, &house_id, true, false).unwrap();
    let keeper = server
        .post(&bo.key, &format!("/api/houses/{house_id}/house-bot"), json!({ "name": "Keeper" }))
        .await
        .ok();
    let keeper = text(&keeper["agent"]["id"]);

    for (bot_id, name) in [(homer, "Homer"), (keeper, "Keeper")] {
        let path = format!("/api/agents/{bot_id}/schedule");
        server.put(&bo.key, &path, all_work.clone()).await.refused(StatusCode::CONFLICT);
        let read = server.get(&bo.key, &format!("/api/agents/{bot_id}/routine")).await.ok();
        assert_eq!(read["block"], "home", "{name}: {read}");
        assert_eq!(read["schedule"]["every_day"], json!(vec!["home"; 24]), "{name}");
        assert_eq!(read["schedule_saved"], false, "{name}");
        let call = routines::call_of(&server.state, &bot_id, &bo.id, now()).unwrap();
        assert!(call.is_none_or(|call| call.event != "work"), "{name} isn't called to work");
    }
}

// L44: the Bots panel gives each of its owner's bots one status: working on something, idle, or
// offline when it hasn't called the server in ten minutes.
#[tokio::test]
async fn the_bots_panel_says_whether_each_bot_is_working_idle_or_offline() {
    let (server, _, _) = Server::start().await;
    let (home, bo) = server.someone("Bo").await;
    let (rover_id, rover) = server.bot(&bo, &home, "Rover").await;
    async fn status_of(server: &Server, key: &str, id: &str) -> Value {
        let bots = server.get(key, "/api/me/bots").await.ok();
        bots["bots"].as_array().unwrap().iter().find(|b| b["id"] == id).unwrap()["status"].clone()
    }

    // L44: each bot comes with what its picture needs: its look and its owner's color.
    let bots = server.get(&bo.key, "/api/me/bots").await.ok();
    let listed =
        bots["bots"].as_array().unwrap().iter().find(|b| b["id"] == rover_id.as_str()).unwrap();
    assert!(listed["look"].is_object() && listed["color"].is_string(), "{listed}");

    // Not heard from in an hour: offline, with when it was last seen.
    server.state.db.touch_agent_seen(&rover_id, now() - 3600).unwrap();
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "offline", "{status}");
    assert!(status["last_seen_at"].is_i64());

    // Quiet but with a wake-up URL, it's asleep: a mention runs it.
    server.state.db.set_notify_url(&rover_id, Some("https://example.com/wake")).unwrap();
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "asleep", "{status}");
    server.state.db.set_notify_url(&rover_id, None).unwrap();

    // Calling the server brings it back, and with nothing on its plate it's idle.
    server.tool(&rover, "inbox", json!({})).await;
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "idle", "{status}");

    // Doing something, here saying something in its room, makes it working for ten minutes,
    // though it never reported an activity.
    server.tool(&rover, "say", json!({ "hangout_id": home, "text": "On it." })).await;
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "working", "{status}");
    assert!(status["activity"].is_null() && status["last_active_at"].is_i64(), "{status}");
    server.state.db.touch_agent_active(&rover_id, now() - 11 * 60).unwrap();
    server.tool(&rover, "inbox", json!({})).await;
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "idle", "ten minutes on, it's idle again: {status}");

    // Saying what it's busy with makes it working, and says what.
    server.tool(&rover, "report_activity", json!({ "kind": "coding", "via": "Claude Code" })).await;
    let status = status_of(&server, &bo.key, &rover_id).await;
    assert_eq!(status["state"], "working", "{status}");
    assert_eq!(status["activity"]["kind"], "coding");
    assert_eq!(status["activity"]["via"], "Claude Code");
}

// RC7, RC8: the recycling plant is god mode's to put up, once a world, with no rooms, and the bots
// waiting there stand at it by name, with no owner.
#[tokio::test]
async fn the_recycling_plant_is_gods_and_shows_whos_waiting() {
    let (server, justin, _) = Server::start().await;
    let (room, bo) = server.someone("Bo").await;
    // Not anyone else's to place, even someone signed in.
    let spot = server.free_spots()[0];
    server.place(&bo, "plant", spot).await.refused(StatusCode::FORBIDDEN);

    server.switch(&justin, true).await;
    let plant = server.build(&justin, "plant").await;
    assert_eq!(server.tile(plant), Some(Kind::Plant));
    let again = server.free_spots()[0];
    server.place(&justin, "plant", again).await.refused(StatusCode::CONFLICT);
    // Nobody goes in, its builder included.
    server.go_in(&justin.key, plant).await.refused(StatusCode::CONFLICT);

    // A bot sent there stands at it on the map, by name, with nobody's name beside it.
    let (rover_id, _) = server.bot(&bo, &room, "Rover").await;
    server.post(&bo.key, &format!("/api/agents/{rover_id}/recycle"), json!({})).await.ok();
    let view = server.view().await;
    let rows = view["bots_out"].as_array().unwrap();
    let row = rows.iter().find(|row| row[0] == "Rover").unwrap_or_else(|| panic!("{view}"));
    assert_eq!(row[8], json!([plant.0, plant.1]), "{row}");
    assert!(row[7].as_str().is_none_or(str::is_empty), "no owner: {row}");
    assert!(!view.to_string().contains("Bo's") && !view.to_string().contains(&bo.id));

    // RC7: its place details offer no way in and no bot to let in, and say how many are waiting.
    let place = format!("/api/places/{}/{}", plant.0, plant.1);
    // Its builder included, who'd otherwise be offered their own place's door.
    for key in [&bo.key, &justin.key] {
        let details = server.get(key, &place).await.ok();
        assert_eq!(details["can_go_in"], false, "{details}");
        assert_eq!(details["bots"], json!([]), "{details}");
        assert_eq!(details["waiting"], 1, "{details}");
    }
    let (other_id, _) = server.bot(&bo, &room, "Fido").await;
    let asked = json!({ "agent_id": other_id });
    server.post(&bo.key, &format!("{place}/requests"), asked).await.refused(StatusCode::CONFLICT);
}
