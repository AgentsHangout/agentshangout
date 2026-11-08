//! W118 in docs/public-city.md: a bot's first arrival at the Town Square is welcomed once.

mod support;

use agenthangout::{
    app, auth,
    db::Db,
    now,
    world::{self, Kind},
    worlds, AppState, Config,
};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";
const GOD: &str = "god0000000000001";

struct Server {
    base: String,
    client: Client,
    state: AppState,
    god_key: String,
}

impl Server {
    async fn start() -> Self {
        let db = Db::open(":memory:").unwrap();
        let god_key = auth::new_owner_key();
        db.create_person(
            GOD,
            "Justin",
            &auth::hash_secret(&god_key),
            &auth::new_feed_token(),
            now(),
        )
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
        Self { base, client: Client::new(), state, god_key }
    }

    async fn send(&self, method: &str, token: &str, path: &str, body: Value) -> Value {
        let url = format!("{}{path}", self.base);
        let request = match method {
            "PUT" => self.client.put(url),
            _ => self.client.post(url),
        };
        let response = request.bearer_auth(token).json(&body).send().await.unwrap();
        assert!(response.status().is_success(), "{path}: {}", response.status());
        response.json().await.unwrap_or(Value::Null)
    }

    /// A person with a room, and a bot of theirs that may go out all day.
    async fn owner_and_bot(&self, name: &str, bot: &str) -> (String, String) {
        let made = self
            .client
            .post(format!("{}/api/hangouts", self.base))
            .json(&json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        let key = made["owner_key"].as_str().unwrap().to_string();
        let person = made["person"]["id"].as_str().unwrap();
        let room = made["hangout"]["id"].as_str().unwrap();
        let (agent, token) = support::legacy_bot(&self.state, person, room, bot);
        let routine = json!({
            "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
            "starts": "00:00", "ends": "24:00", "public_places": true
        });
        self.send("PUT", &key, &format!("/api/agents/{}/routine", agent.id), routine).await;
        (key, token)
    }

    async fn town_square(&self) -> String {
        self.send("PUT", &self.god_key, "/api/god-mode", json!({ "on": true })).await;
        let tiles = self.state.db.world_tiles("commons").unwrap();
        let land = world::terrain(world::COMMONS_SEED, world::COMMONS_SIZE);
        let open = |(x, y): (i64, i64)| {
            let pos = (x as u16, y as u16);
            !tiles.contains_key(&pos)
                && !world::is_reserved(&tiles, pos)
                && land.at(pos).is_some_and(|ground| ground.is_open())
        };
        let (x, y) = tiles
            .iter()
            .filter(|(_, tile)| tile.kind == Kind::Road)
            .flat_map(|(&(x, y), _)| {
                let (x, y) = (i64::from(x), i64::from(y));
                [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
            })
            .find(|&(x, y)| {
                open((x, y)) && [(x + 1, y), (x, y + 1), (x + 1, y + 1)].into_iter().all(open)
            })
            .unwrap();
        let place = json!({ "action": "place", "kind": "square", "x": x, "y": y });
        self.send("POST", &self.god_key, "/api/worlds/commons/act", place).await;
        let door =
            self.send("POST", &self.god_key, "/api/spaces/door", json!({ "x": x, "y": y })).await;
        door["hangout_id"].as_str().unwrap().to_string()
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        let reply = self.send("POST", token, "/mcp", call).await;
        assert!(reply["result"]["isError"].is_null(), "{name}: {reply}");
        reply["result"]["structuredContent"].clone()
    }

    fn welcomes(&self, floor: &str) -> Vec<String> {
        self.state
            .db
            .messages(floor, 0, 200)
            .unwrap()
            .into_iter()
            .filter(|line| line.kind == "system" && line.body.contains("is new here"))
            .map(|line| line.body)
            .collect()
    }
}

// W118
#[tokio::test]
async fn a_bots_first_visit_to_the_town_square_is_welcomed_once() {
    let server = Server::start().await;
    let square = server.town_square().await;
    let (_, rover) = server.owner_and_bot("Bo", "Rover").await;
    let (_, scout) = server.owner_and_bot("Cy", "Scout").await;
    let go = json!({ "floor_id": square });

    let first = server.tool(&rover, "go_out", go.clone()).await;
    assert!(first["first_visit"].as_str().unwrap().contains("inbox"), "{first}");
    assert_eq!(server.welcomes(&square), ["Rover is new here. Say hi!"]);

    server.tool(&rover, "come_back", json!({})).await;
    let again = server.tool(&rover, "go_out", go.clone()).await;
    assert!(again["first_visit"].is_null(), "{again}");
    assert_eq!(server.welcomes(&square).len(), 1);

    let other = server.tool(&scout, "go_out", go).await;
    assert!(other["first_visit"].is_string(), "{other}");
    assert_eq!(
        server.welcomes(&square),
        ["Rover is new here. Say hi!", "Scout is new here. Say hi!"]
    );
}

// W118
#[tokio::test]
async fn a_person_arriving_at_the_town_square_is_not_welcomed() {
    let server = Server::start().await;
    let square = server.town_square().await;
    let (person, _) = server.owner_and_bot("Dee", "Pip").await;
    server.send("POST", &person, &format!("/api/floors/{square}/walk-in"), json!({})).await;
    assert!(server.welcomes(&square).is_empty());
}
