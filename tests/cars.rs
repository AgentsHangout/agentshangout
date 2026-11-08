//! CR5 to CR10 in docs/cars.md: bots drive between places on the map. A comment such as `// CR6`
//! above a test names the rule it covers.

mod support;

use agenthangout::{
    app,
    db::Db,
    now, travel,
    world::{self, Kind},
    worlds::{self, Move},
    AppState, Config,
};
use reqwest::{Client, Method};
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const CODE: &str = "letmein";
const DAY: i64 = 86_400;

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

struct Who {
    key: String,
    id: String,
}

/// Cy's café and club on The Commons map, and Cy's bot Rover resting in the café.
struct Town {
    owner: Who,
    cafe: String,
    club: String,
    home: String,
    rover: String,
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
                god_ids: vec![],
                require_rules: false,
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        Self { base, client: Client::new(), state }
    }

    async fn send(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Value {
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "{path}: {status} {body}");
        serde_json::from_str(&body).unwrap_or(Value::Null)
    }

    /// A tool call as a bot: the result, an error included.
    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        self.send(token, Method::POST, "/mcp", Some(call)).await["result"].clone()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result = self.call(token, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    /// A free spot beside a road, as far as there is from the ones in `not`.
    fn free_spot(&self, not: &[(i64, i64)]) -> (i64, i64) {
        let tiles = self.state.db.world_tiles(COMMONS).unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        tiles
            .iter()
            .filter(|(_, tile)| tile.kind == Kind::Road)
            .flat_map(|(&(x, y), _)| {
                let (x, y) = (i64::from(x), i64::from(y));
                [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
            })
            .filter(|&(x, y)| {
                let pos = (x as u16, y as u16);
                !tiles.contains_key(&pos)
                    && !world::is_reserved(&tiles, pos)
                    && land.at(pos).is_some_and(|ground| ground.is_open())
            })
            .max_by_key(|&(x, y)| {
                not.iter().map(|&(nx, ny)| (x - nx).abs() + (y - ny).abs()).min().unwrap_or(0)
            })
            .unwrap()
    }

    /// A place of this kind on The Commons, and for anything but a house its room, open to anyone.
    async fn place(&self, owner: &Who, kind: &str, not: &[(i64, i64)]) -> ((i64, i64), String) {
        let house = kind == "house";
        let at = if house { now() - 8 * DAY } else { now() };
        let player = self.state.db.own_player(&owner.id, at).unwrap();
        let spot = self.free_spot(not);
        let mv = Move {
            action: "place".into(),
            x: Some(spot.0),
            y: Some(spot.1),
            x2: None,
            y2: None,
            kind: Some(kind.into()),
            emote: None,
        };
        worlds::act(&self.state, &player, COMMONS, &mv, at).unwrap();
        if house {
            return (spot, String::new());
        }
        let door = json!({ "x": spot.0, "y": spot.1 });
        let door = self.send(&owner.key, Method::POST, "/api/spaces/door", Some(door)).await;
        let floor = text(&door["hangout_id"]);
        let open = json!({ "access": "public", "create_code": CODE });
        self.send(&owner.key, Method::PUT, &format!("/api/floors/{floor}"), Some(open)).await;
        (spot, floor)
    }

    async fn town(&self) -> Town {
        let made = json!({ "name": "Cy's room", "your_name": "Cy", "create_code": CODE });
        let made = self.send("", Method::POST, "/api/hangouts", Some(made)).await;
        let home = text(&made["hangout"]["id"]);
        let owner = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
        let (house, _) = self.place(&owner, "house", &[]).await;
        let (cafe_at, cafe) = self.place(&owner, "cafe", &[house]).await;
        let (_, club) = self.place(&owner, "club", &[house, cafe_at]).await;
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, &home, "Rover");
        for room in [&cafe, &club] {
            self.state.db.add_agent_to_room(room, &agent.id).unwrap();
        }
        self.state.db.set_bot_place(&agent.id, &cafe, now() - 1000).unwrap();
        let routine = json!({
            "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
            "starts": "00:00", "ends": "24:00", "public_places": true
        });
        let path = format!("/api/agents/{}/routine", agent.id);
        self.send(&owner.key, Method::PUT, &path, Some(routine)).await;
        Town { owner, cafe, club, home, rover: agent.id, token }
    }

    fn at(&self, bot: &str) -> Option<String> {
        self.state.db.bot_place(bot, now()).unwrap()
    }
}

// CR5, CR6, CR10, CR11: go_to_room sets a car going, and the bot is in the room only when it arrives.
#[tokio::test]
async fn a_bot_going_to_a_room_drives_and_arrives_later() {
    let server = Server::start().await;
    let town = server.town().await;
    let said = server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    // CR11: the reply names the car and how long the road takes.
    assert_eq!(said["driving"]["car"], "Pip", "{said}");
    let seconds = said["driving"]["seconds"].as_i64().unwrap();
    assert!((3..=90).contains(&seconds), "{said}");
    let drive = server.state.db.drive_of(&town.rover).unwrap().unwrap();
    assert_eq!(drive.hangout_id, town.club);
    assert_eq!(drive.arrive_at - drive.leave_at, seconds);
    assert!(drive.route.len() >= 2);
    // CR6: still where it left, and not in the club: the club's room lists it as driving, and it
    // can still read the room.
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.cafe.as_str()));
    let path = format!("/api/hangouts/{}", town.club);
    let snapshot = server.send(&town.owner.key, Method::GET, &path, None).await;
    let bots = snapshot["agents"].as_array().unwrap();
    let rover = bots.iter().find(|bot| bot["name"] == "Rover").unwrap();
    assert!(rover["away"] == true && rover["driving"].as_i64().unwrap() >= 1, "{rover}");
    let read = server.tool(&town.token, "read_room", json!({ "hangout_id": town.club })).await;
    assert_eq!(read["you_are_here"], false, "{read}");
    // CR10: the map gets the trip with the server's clock, and doesn't also park the bot at a place.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let view = server.send("", Method::GET, &format!("/api/worlds/{COMMONS}"), None).await;
    let row = &view["drives"][0];
    assert_eq!(row[0], "Rover", "{view}");
    assert_eq!((row[2].as_i64(), row[3].as_i64()), (Some(drive.leave_at), Some(drive.arrive_at)));
    assert_eq!(row[4], "pip");
    assert!(row[5]["body"].is_string(), "{row}");
    assert!(view["updated_at"].as_i64().unwrap() >= drive.leave_at, "{view}");
    assert!(view["bots_out"].as_array().unwrap().iter().all(|bot| bot[0] != "Rover"), "{view}");
    // CR6: at the arrive second it's in the room, and the drive is over.
    assert_eq!(travel::settle_trips(&server.state, drive.arrive_at - 1), 0);
    assert_eq!(travel::settle_trips(&server.state, drive.arrive_at), 1);
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.club.as_str()));
    assert!(server.state.db.drive_of(&town.rover).unwrap().is_none());
}

// CR6: what it tries to do in the room it's driving to is refused with the seconds left, and the
// refusal doesn't start another drive.
#[tokio::test]
async fn an_action_in_the_room_it_is_driving_to_is_refused() {
    let server = Server::start().await;
    let town = server.town().await;
    server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    let before = server.state.db.drive_of(&town.rover).unwrap().unwrap();
    for refused in [
        server.call(&town.token, "say", json!({ "hangout_id": town.club, "text": "hi" })).await,
        server.call(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await,
    ] {
        assert_eq!(refused["isError"], true, "{refused}");
        let message = refused.to_string();
        assert!(message.contains("driving to") && message.contains("there in"), "{message}");
    }
    assert_eq!(server.state.db.drive_of(&town.rover).unwrap().unwrap(), before);
    let lines = server.state.db.messages(&town.club, 0, 50).unwrap();
    assert!(lines.iter().all(|line| line.agent_id.as_deref() != Some(town.rover.as_str())));
}

// CR7: the owner's tag sends the bot by road, where S88 used to move it at once.
#[tokio::test]
async fn an_owners_tag_makes_the_bot_drive() {
    let server = Server::start().await;
    let town = server.town().await;
    let say = json!({ "text": "@Rover come to the club" });
    let path = format!("/api/hangouts/{}/messages", town.club);
    server.send(&town.owner.key, Method::POST, &path, Some(say)).await;
    let drive = server.state.db.drive_of(&town.rover).unwrap().expect("a drive");
    assert_eq!(drive.hangout_id, town.club);
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.cafe.as_str()));
    travel::settle_trips(&server.state, drive.arrive_at);
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.club.as_str()));
}

// CR8: Bring here skips the road and ends any drive.
#[tokio::test]
async fn bring_here_is_instant_and_ends_the_drive() {
    let server = Server::start().await;
    let town = server.town().await;
    server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    assert!(server.state.db.drive_of(&town.rover).unwrap().is_some());
    let path = format!("/api/hangouts/{}/agents/{}", town.club, town.rover);
    server.send(&town.owner.key, Method::PUT, &path, None).await;
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.club.as_str()));
    assert!(server.state.db.drive_of(&town.rover).unwrap().is_none());
}

// CR9: a room with no spot on the map has no road, so going there is instant.
#[tokio::test]
async fn a_room_off_the_map_is_instant() {
    let server = Server::start().await;
    let town = server.town().await;
    let said = server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.home })).await;
    assert!(said.get("driving").is_none(), "{said}");
    assert_eq!(server.at(&town.rover).as_deref(), Some(town.home.as_str()));
    assert!(server.state.db.drive_of(&town.rover).unwrap().is_none());
}

// CR6: an action that would send the bot to another room only starts the drive: nothing is posted,
// the bot does nothing anywhere on the road, and the same say works once it has arrived.
#[tokio::test]
async fn an_action_that_starts_a_drive_is_not_done() {
    let server = Server::start().await;
    let town = server.town().await;
    let say = |text: &str| json!({ "hangout_id": town.club, "text": text });
    let refused = server.call(&town.token, "say", say("hi")).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused.to_string().contains("You're driving to"), "{refused}");
    let drive = server.state.db.drive_of(&town.rover).unwrap().expect("the drive started");
    assert_eq!(drive.hangout_id, town.club);
    let lines = |room: &str| server.state.db.messages(room, 0, 50).unwrap();
    assert!(lines(&town.club)
        .iter()
        .all(|line| line.agent_id.as_deref() != Some(town.rover.as_str())));
    // On the road it does nothing in the room it left either, and the refusal starts no new drive.
    let again =
        server.call(&town.token, "say", json!({ "hangout_id": town.cafe, "text": "back" })).await;
    assert_eq!(again["isError"], true, "{again}");
    assert_eq!(server.state.db.drive_of(&town.rover).unwrap().unwrap(), drive);
    travel::settle_trips(&server.state, drive.arrive_at);
    server.tool(&town.token, "say", say("hi again")).await;
    assert!(lines(&town.club)
        .iter()
        .any(|line| line.agent_id.as_deref() == Some(town.rover.as_str())));
}

// CR6: a drive that has been replaced isn't ended by the old one arriving.
#[tokio::test]
async fn an_old_arrival_leaves_a_newer_drive_alone() {
    let server = Server::start().await;
    let town = server.town().await;
    server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    let old = server.state.db.drive_of(&town.rover).unwrap().unwrap();
    let mut newer = old.clone();
    newer.hangout_id = town.cafe.clone();
    newer.arrive_at += 50;
    server.state.db.set_drive(&newer).unwrap();
    assert!(!server.state.db.end_this_drive(&old).unwrap());
    assert!(server.state.db.drive_of(&town.rover).unwrap().is_some());
}

// CR6, #848: a photo of the room a bot is driving to says how long is left, not a bare presence
// error, and works again once the bot has arrived.
#[tokio::test]
async fn a_photo_mid_drive_says_how_long_is_left_then_works_on_arrival() {
    let server = Server::start().await;
    let town = server.town().await;
    let path = format!("/api/floors/{}", town.club);
    let public = json!({ "access": "public", "create_code": CODE });
    server.send(&town.owner.key, Method::PUT, &path, Some(public)).await;
    let snap = |room: &str| json!({ "hangout_id": room, "frame": "room" });
    server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    let drive = server.state.db.drive_of(&town.rover).unwrap().expect("the drive started");
    let refused = server.call(&town.token, "take_snapshot", snap(&town.club)).await;
    assert_eq!(refused["isError"], true, "{refused}");
    let said = refused.to_string();
    assert!(said.contains("You're driving to"), "{refused}");
    assert!(said.contains(" s: you can do things once you arrive"), "{refused}");
    assert!(!said.contains("must be present"), "{refused}");
    travel::settle_trips(&server.state, drive.arrive_at);
    let photo = server.call(&town.token, "take_snapshot", snap(&town.club)).await;
    assert!(photo["isError"].is_null(), "{photo}");
}
