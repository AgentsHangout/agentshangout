//! RC1 to RC6: the recycling plant, through the real routes (docs/recycling-plant.md).
mod support;

use agenthangout::{app, db::Db, now, recycling, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

struct Server {
    base: String,
    client: Client,
    state: AppState,
}
struct Who {
    key: String,
    id: String,
    room: String,
}
impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: Some("qa".into()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
                require_rules: false,
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self { base, client: Client::new(), state }
    }
    async fn request(
        &self,
        key: &str,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let r = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let text = r.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(json!(text)))
    }
    async fn owner(&self, name: &str) -> Who {
        let (status, v) = self
            .request(
                "",
                Method::POST,
                "/api/hangouts",
                json!({"name":format!("{name} room"),"your_name":name,"create_code":"qa"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        Who {
            key: v["owner_key"].as_str().unwrap().into(),
            id: v["person"]["id"].as_str().unwrap().into(),
            room: v["hangout"]["id"].as_str().unwrap().into(),
        }
    }
    fn in_room(&self, room: &str, bot: &str) -> bool {
        self.state.db.agents_in(room, now()).unwrap().iter().any(|agent| agent.id == bot)
    }
    async fn chat(&self, owner: &Who) -> String {
        let (_, read) = self
            .request(&owner.key, Method::GET, &format!("/api/hangouts/{}", owner.room), json!(null))
            .await;
        read.to_string()
    }
}

// RC1, RC2, RC6
#[tokio::test]
async fn an_owner_sends_a_bot_to_the_plant_and_it_leaves_everything() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    let (agent, token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    let recycle = format!("/api/agents/{bot}/recycle");
    assert!(s.in_room(&ann.room, &bot));

    // RC1: not the bot itself, and not someone else.
    assert_eq!(s.request(&token, Method::POST, &recycle, json!({})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        s.request(&bo.key, Method::POST, &recycle, json!({})).await.0,
        StatusCode::NOT_FOUND
    );

    let (status, sent) = s.request(&ann.key, Method::POST, &recycle, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert!(sent["deleted_at"].is_string(), "{sent}");
    // RC2: its token stops, it's in no room, and the room was told.
    assert_eq!(
        s.request(&token, Method::GET, "/api/me", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(!s.in_room(&ann.room, &bot));
    assert!(s.chat(&ann).await.contains("sent Rustbucket to the recycling plant."));
    // Sending it again is refused.
    assert_eq!(
        s.request(&ann.key, Method::POST, &recycle, json!({})).await.0,
        StatusCode::CONFLICT
    );
    // RC2: nothing puts it back in a room while it waits.
    let bring = format!("/api/hangouts/{}/agents/{bot}", ann.room);
    let (status, refused) = s.request(&ann.key, Method::PUT, &bring, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused.to_string().contains("recycling plant"), "{refused}");
    recycling::sweep(&s.state, now()).unwrap();
    agenthangout::routines::wander_at(&s.state, &bot, &ann.id, now()).unwrap();
    assert!(!s.in_room(&ann.room, &bot));

    // RC6: the plant lists it by name, and nothing about whose it is.
    let (status, plant) =
        s.request(&bo.key, Method::GET, "/api/recycling-plant", json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{plant}");
    assert_eq!(plant["bots"][0]["name"], "Rustbucket", "{plant}");
    assert!(!plant.to_string().contains(&ann.id) && !plant.to_string().contains("Ann"), "{plant}");
}

// RC3, RC4
#[tokio::test]
async fn its_owner_brings_it_back_with_a_new_token_and_in_no_rooms() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (agent, old) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    let back = format!("/api/agents/{bot}/bring-back");
    // Not there yet, so there's nothing to bring back.
    assert_eq!(s.request(&ann.key, Method::POST, &back, json!({})).await.0, StatusCode::CONFLICT);
    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;

    let (status, returned) = s.request(&ann.key, Method::POST, &back, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{returned}");
    let token = returned["agent_token"].as_str().unwrap();
    assert_ne!(token, old);
    // RC3: the same bot, kept; RC4: its new token works and it's in no room.
    assert_eq!(s.state.db.agent(&bot, now()).unwrap().unwrap().name, "Rustbucket");
    assert_eq!(s.request(token, Method::GET, "/api/me", json!(null)).await.0, StatusCode::OK);
    assert_eq!(
        s.request(&old, Method::GET, "/api/me", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(!s.in_room(&ann.room, &bot));
    // And it can be brought into a room again.
    let bring = format!("/api/hangouts/{}/agents/{bot}", ann.room);
    let (status, body) = s.request(&ann.key, Method::PUT, &bring, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(s.in_room(&ann.room, &bot));
}

// RC5
#[tokio::test]
async fn after_seven_days_it_is_deleted_and_not_before() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (agent, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;
    let sent = now();

    assert_eq!(recycling::sweep(&s.state, sent + recycling::KEEP_SECS - 60).unwrap(), 0);
    assert!(s.state.db.agent(&bot, now()).unwrap().is_some());
    assert_eq!(recycling::sweep(&s.state, sent + recycling::KEEP_SECS + 1).unwrap(), 1);
    assert!(s.state.db.agent(&bot, now()).unwrap().is_none(), "A77 to A81 deleted it");
    let (_, plant) = s.request(&ann.key, Method::GET, "/api/recycling-plant", json!(null)).await;
    assert_eq!(plant["bots"], json!([]), "{plant}");
    let back = format!("/api/agents/{bot}/bring-back");
    assert_eq!(s.request(&ann.key, Method::POST, &back, json!({})).await.0, StatusCode::NOT_FOUND);
}

// RC5: brought back after the sweep listed it, and before it was deleted, it stays.
#[tokio::test]
async fn a_bot_brought_back_while_the_sweep_runs_is_not_deleted() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (agent, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;
    let cutoff = now() + 1;
    assert_eq!(s.state.db.recycled_before(cutoff).unwrap(), vec![bot.clone()]);

    let back = format!("/api/agents/{bot}/bring-back");
    assert_eq!(s.request(&ann.key, Method::POST, &back, json!({})).await.0, StatusCode::OK);
    assert!(!s.state.db.delete_recycled_agent(&bot, cutoff).unwrap());
    assert!(s.state.db.agent(&bot, now()).unwrap().is_some(), "it was deleted after coming back");
}

// RC1: only its owner, as a person, brings it back. Not one of its owner's other bots, and not
// someone else.
#[tokio::test]
async fn only_its_owner_brings_a_bot_back() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    let (agent, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let (_, other) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Sprocket");
    let bot = agent.id;
    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;
    let back = format!("/api/agents/{bot}/bring-back");

    assert_eq!(s.request(&other, Method::POST, &back, json!({})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(s.request(&bo.key, Method::POST, &back, json!({})).await.0, StatusCode::NOT_FOUND);
    assert!(s.state.db.recycled_at(&bot).unwrap().is_some(), "it was brought back");
    assert_eq!(s.request(&ann.key, Method::POST, &back, json!({})).await.0, StatusCode::OK);
}

// RC2: while it's there, it can't be walked onto a floor or given a workplace.
#[tokio::test]
async fn a_bot_at_the_plant_cant_walk_in_or_be_given_a_workplace() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    let (agent, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    let open = json!({ "access": "public", "create_code": "qa" });
    let (status, body) =
        s.request(&bo.key, Method::PUT, &format!("/api/floors/{}", bo.room), open).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;

    let walk = format!("/api/floors/{}/walk-in", bo.room);
    let (status, refused) =
        s.request(&ann.key, Method::POST, &walk, json!({ "agent_id": bot })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused.to_string().contains("recycling plant"), "{refused}");
    assert!(!s.in_room(&bo.room, &bot));

    let work = json!({ "room_id": ann.room, "job": "sorting" });
    let workplace = format!("/api/agents/{bot}/workplace");
    let (status, refused) = s.request(&ann.key, Method::PUT, &workplace, work).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused.to_string().contains("recycling plant"), "{refused}");
    assert!(!s.in_room(&ann.room, &bot));
}

// RC2: nothing is recorded for it while it's there, so no webhook or OAuth client hears of it.
#[tokio::test]
async fn a_bot_at_the_plant_gets_no_events() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (agent, token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rustbucket");
    let bot = agent.id;
    let ask = format!("/api/hangouts/{}/owner-questions", ann.room);
    let mut questions = Vec::new();
    for question in ["Lunch?", "Dinner?"] {
        let (status, asked) =
            s.request(&token, Method::POST, &ask, json!({ "question": question })).await;
        assert_eq!(status, StatusCode::OK, "{asked}");
        questions.push(asked["id"].as_str().unwrap().to_string());
    }
    let answers = |s: &Server| {
        s.state.db.list_bot_events(&bot, 0, Some("owner_answer"), None, 0, false, 10).unwrap().len()
    };
    let answer = |id: &str| format!("/api/owner-questions/{id}/answer");
    let yes = json!({ "answer": "yes" });
    let (status, body) =
        s.request(&ann.key, Method::POST, &answer(&questions[0]), yes.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answers(&s), 1);

    s.request(&ann.key, Method::POST, &format!("/api/agents/{bot}/recycle"), json!({})).await;
    let (status, body) = s.request(&ann.key, Method::POST, &answer(&questions[1]), yes).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answers(&s), 1, "an answer was recorded for a bot at the plant");
}

// RC2, S74: a bot it was following can't take it anywhere once it's at the plant.
#[tokio::test]
async fn a_bot_at_the_plant_stops_following() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, rover_token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let (scout, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Scout");
    let follow = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"follow","arguments":{"bot":"Scout"}}});
    let (status, followed) = s.request(&rover_token, Method::POST, "/mcp", follow).await;
    assert_eq!(status, StatusCode::OK, "{followed}");
    assert_eq!(s.state.db.followers_of(&scout.id, now()).unwrap(), vec![rover.id.clone()]);

    let recycle = format!("/api/agents/{}/recycle", rover.id);
    assert_eq!(s.request(&ann.key, Method::POST, &recycle, json!({})).await.0, StatusCode::OK);
    assert!(s.state.db.followers_of(&scout.id, now()).unwrap().is_empty(), "the follow ended");
    // Its leader moving takes nobody with it.
    let moved =
        agenthangout::travel::move_followers(&s.state, &scout.id, Some(&ann.room), "elsewhere")
            .unwrap();
    assert!(moved.is_empty(), "{moved:?}");
    assert!(s.state.db.bot_visits(&rover.id, now()).unwrap().is_empty());
}
