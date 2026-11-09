//! #942: a snapshot's furnishing names the bots using each item from the bots it already read
//! (`here_now`), and that must be the same set `bots_here` used to work out again.

mod support;

use agenthangout::{
    app,
    auth::Caller,
    db::{Db, Drive, NewFloorItem},
    items, now,
    world::{self, Kind},
    worlds::{self},
    AppState, Config,
};
use reqwest::{Client, Method};
use serde_json::{json, Value};

const COMMONS: &str = "commons";
const DAY: i64 = 86_400;
const CODE: &str = "letmein";

/// A free spot beside a road on The Commons, the way cars.rs finds one.
fn free_spot(server: &Server, not: &[(i64, i64)]) -> (i64, i64) {
    let tiles = server.state.db.world_tiles(COMMONS).unwrap();
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

struct Server {
    base: String,
    client: Client,
    state: AppState,
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
}

/// Cy's public room with a stool in it, and three bots: Mo sits on the stool, Pip is driving
/// in from home and is on the stool too, and Rover is in the room and using nothing.
struct Room {
    owner: String,
    owner_key: String,
    room: String,
    mo: String,
    pip: String,
}

async fn setup(server: &Server) -> Room {
    let made = json!({ "name": "Cy's room", "your_name": "Cy", "create_code": CODE });
    let made = server.send("", Method::POST, "/api/hangouts", Some(made)).await;
    let home = text(&made["hangout"]["id"]);
    let owner = text(&made["person"]["id"]);
    let owner_key = text(&made["owner_key"]);
    // Pip's home is a house on the map, and the drive runs from there to the room.
    let room = {
        let player = server.state.db.own_player(&owner, now()).unwrap();
        let house = free_spot(server, &[]);
        let mv = worlds::Move {
            action: "place".into(),
            x: Some(house.0),
            y: Some(house.1),
            x2: None,
            y2: None,
            kind: Some("house".into()),
            emote: None,
        };
        worlds::act(&server.state, &player, COMMONS, &mv, now() - 8 * DAY).unwrap();
        let door = json!({ "x": house.0, "y": house.1 });
        let door = server.send(&owner_key, Method::POST, "/api/spaces/door", Some(door)).await;
        let floor = text(&door["hangout_id"]);
        let open = json!({ "access": "public", "create_code": CODE });
        server.send(&owner_key, Method::PUT, &format!("/api/floors/{floor}"), Some(open)).await;
        floor
    };
    let (mo, _) = support::legacy_bot(&server.state, &owner, &room, "Mo");
    support::legacy_bot(&server.state, &owner, &room, "Rover");
    let (pip, _) = support::legacy_bot(&server.state, &owner, &home, "Pip");
    server.state.db.add_agent_to_room(&room, &pip.id).unwrap();
    let stool = NewFloorItem {
        id: "stool-1",
        hangout_id: &room,
        kind: "stool",
        x: 3,
        y: 3,
        facing: 0,
        placed_by: &owner,
        now: now(),
    };
    server.state.db.place_item(&stool, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();
    let stool = NewFloorItem { id: "stool-2", x: 4, ..stool };
    server.state.db.place_item(&stool, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();
    let later = now() + 600;
    let use_at = now();
    server
        .state
        .db
        .use_item(&mo.id, "stool-1", later, use_at, |_| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    // Pip is on the road to the room: its drive is set, and it uses the second stool.
    let drive = Drive {
        agent_id: pip.id.clone(),
        hangout_id: room.clone(),
        car_id: "pip".into(),
        route: vec![(0, 0), (1, 0)],
        leave_at: now() - 1,
        arrive_at: now() + 300,
    };
    server.state.db.set_drive(&drive).unwrap();
    server
        .state
        .db
        .use_item(&pip.id, "stool-2", later, use_at, |_| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    Room { owner, owner_key, room, mo: mo.id, pip: pip.id }
}

// #942 review: the snapshot's furnishing (which reads each bot's place once) must match what
// furnishing works out from `bots_here` on its own, with a bot driving in and a bot on a stool.
#[tokio::test]
async fn snapshot_users_match_bots_here_with_a_driving_bot() {
    let server = Server::start().await;
    let town = setup(&server).await;
    let path = format!("/api/hangouts/{}", town.room);
    let snapshot = server.send(&town.owner_key, Method::GET, &path, None).await;
    // The furnishing is flattened into the snapshot, so its fields sit at the top level.
    let furnishing = json!({
        "items": snapshot["items"].clone(),
        "room_mood": snapshot["room_mood"].clone(),
        "room_mood_note": snapshot["room_mood_note"].clone(),
        "room_beauty": snapshot["room_beauty"].clone(),
        "room_beauty_word": snapshot["room_beauty_word"].clone(),
    });

    let caller = Caller {
        person_id: town.owner.clone(),
        agent_id: None,
        name: "Cy".into(),
        secret_hash: String::new(),
        is_house: false,
    };
    let direct = items::furnishing(&server.state, &caller, &town.room, None).unwrap();
    let direct = serde_json::to_value(&direct).unwrap();
    assert_eq!(furnishing, direct, "snapshot and bots_here disagree");

    let items_out = furnishing["items"].as_array().unwrap();
    let used_by = |id: &str| -> Vec<String> {
        let item = items_out.iter().find(|item| item["id"] == id).unwrap();
        item["used_by"]
            .as_array()
            .map(|bots| bots.iter().map(|bot| text(bot)).collect())
            .unwrap_or_default()
    };
    assert_eq!(used_by("stool-1"), vec![town.mo.clone()], "{furnishing}");
    // Whatever bot_place says about Pip, the snapshot and bots_here (checked above) agree on it.
    let pip_here =
        server.state.db.bot_place(&town.pip, now()).unwrap().as_deref() == Some(town.room.as_str());
    assert_eq!(used_by("stool-2").contains(&town.pip), pip_here, "{furnishing}");
}
