//! CR12 in docs/cars.md: a bot that drives in says it pulled up in its car, where a bot that walks
//! in says it came in (S79).

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
        Town { club, home, rover: agent.id, token }
    }
}

fn lines(server: &Server, room: &str) -> Vec<String> {
    server.state.db.messages(room, 0, 50).unwrap().into_iter().map(|line| line.body).collect()
}

// CR12: the drive's arrival says which car the bot pulled up in.
#[tokio::test]
async fn a_bot_that_drives_in_pulled_up_in_its_car() {
    let server = Server::start().await;
    let town = server.town().await;
    let said = server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.club })).await;
    let car = text(&said["driving"]["car"]);
    let drive = server.state.db.drive_of(&town.rover).unwrap().unwrap();
    assert!(lines(&server, &town.club).iter().all(|line| !line.contains("Rover")));
    travel::settle_trips(&server.state, drive.arrive_at);
    let all = lines(&server, &town.club);
    assert_eq!(all.last().unwrap(), &format!("Rover pulled up in {car}."), "{all:?}");
    assert!(all.iter().all(|line| !line.contains("came in")), "{all:?}");
}

// CR12: a bot that takes itself onto a floor without a drive still came in.
#[tokio::test]
async fn a_bot_that_walks_in_still_came_in() {
    let server = Server::start().await;
    let town = server.town().await;
    server.state.db.remove_agent_from_room(&town.home, &town.rover).unwrap();
    server.tool(&town.token, "go_to_room", json!({ "hangout_id": town.home })).await;
    let all = lines(&server, &town.home);
    assert!(all.contains(&"Rover came in.".to_string()), "{all:?}");
    assert!(all.iter().all(|line| !line.contains("pulled up")), "{all:?}");
}
