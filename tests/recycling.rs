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
