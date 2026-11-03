//! CP1–CP6: real HTTP/MCP camera access, consent, images and revocation.
use agenthangout::{app, db::Db, AppState, Config};
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
    async fn ok(&self, key: &str, method: Method, path: &str, body: Value) -> Value {
        let (s, v) = self.request(key, method, path, body).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
    async fn owner(&self, name: &str) -> Who {
        let v = self
            .ok(
                "",
                Method::POST,
                "/api/hangouts",
                json!({"name":format!("{name} room"),"your_name":name,"create_code":"qa"}),
            )
            .await;
        Who {
            key: v["owner_key"].as_str().unwrap().into(),
            id: v["person"]["id"].as_str().unwrap().into(),
            room: v["hangout"]["id"].as_str().unwrap().into(),
        }
    }
    async fn bot(&self, owner: &Who, name: &str) -> (String, String) {
        let v = self
            .ok(
                &owner.key,
                Method::POST,
                &format!("/api/hangouts/{}/agents", owner.room),
                json!({"name":name}),
            )
            .await;
        (v["agent"]["id"].as_str().unwrap().into(), v["token"].as_str().unwrap().into())
    }
    async fn consent(&self, owner: &Who, bot: &str, on: bool) {
        self.ok(
            &owner.key,
            Method::PUT,
            &format!("/api/agents/{bot}/camera"),
            json!({"allowed":on}),
        )
        .await;
    }
    async fn public(&self, owner: &Who, on: bool) {
        self.ok(
            &owner.key,
            Method::PUT,
            &format!("/api/floors/{}", owner.room),
            json!({"access":if on{"public"}else{"private"},"create_code":"qa"}),
        )
        .await;
    }
    async fn capture(&self, token: &str, room: &str, frame: &str) -> Value {
        self.ok(token,Method::POST,"/mcp",json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"take_snapshot","arguments":{"hangout_id":room,"frame":frame}}})).await["result"].clone()
    }
}
#[tokio::test]
async fn public_photo_is_png_but_private_text_and_unconsented_bots_are_absent() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (id, token) = s.bot(&ann, "Dotliver Tree").await;
    let (_, hidden) = s.bot(&ann, "Hidden Camera Sentinel").await;
    let denied = s.capture(&token, &ann.room, "room").await;
    assert_eq!(denied["isError"], true);
    s.consent(&ann, &id, true).await;
    s.ok(
        &ann.key,
        Method::POST,
        &format!("/api/hangouts/{}/messages", ann.room),
        json!({"text":"PRIVATE CHAT SENTINEL"}),
    )
    .await;
    let mut cues = s.state.hub.subscribe();
    let photo = s.capture(&token, &ann.room, "room").await;
    let cue = cues.try_recv().unwrap();
    assert_eq!(cue.kind, "camera");
    assert_eq!(serde_json::from_str::<Value>(&cue.data).unwrap()["bot_id"], id);
    assert!(photo["isError"].is_null(), "{photo}");
    assert_eq!(photo["content"][1]["type"], "image");
    assert_eq!(photo["content"][1]["mimeType"], "image/png");
    let meta = &photo["structuredContent"];
    assert_eq!(meta["bots"].as_array().unwrap().len(), 1);
    assert_eq!(meta["bots"][0]["name"], "Dotliver Tree");
    assert!(!meta.to_string().contains("PRIVATE CHAT"));
    assert!(!meta.to_string().contains("Hidden Camera"));
    assert!(!meta.to_string().contains(&ann.id));
    assert!(!meta.to_string().contains(&hidden));
    let response = s.client.get(meta["image_url"].as_str().unwrap()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let png = response.bytes().await.unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 960);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 504);
    if let Ok(path) = std::env::var("CAMERA_TEST_IMAGE") {
        std::fs::write(path, &png).unwrap();
    }
    s.consent(&ann, &id, false).await;
    assert_eq!(
        s.client.get(meta["image_url"].as_str().unwrap()).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn camera_owner_gate_private_floor_and_public_url_revocation_hold() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    let (id, token) = s.bot(&ann, "Fern").await;
    let path = format!("/api/agents/{id}/camera");
    assert_eq!(
        s.request(&token, Method::PUT, &path, json!({"allowed":true})).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        s.request(&bo.key, Method::PUT, &path, json!({"allowed":true})).await.0,
        StatusCode::NOT_FOUND
    );
    s.consent(&ann, &id, true).await;
    assert_eq!(s.capture(&token, &ann.room, "room").await["isError"], true);
    assert_eq!(s.request(&ann.key, Method::POST, "/mcp", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"take_snapshot","arguments":{"hangout_id":ann.room}}})).await.0, StatusCode::FORBIDDEN);
    s.public(&ann, true).await;
    let photo = s.capture(&token, &ann.room, "table").await;
    assert!(photo["isError"].is_null(), "{photo}");
    let meta = &photo["structuredContent"];
    let photo_id = meta["id"].as_str().unwrap();
    assert!(s
        .state
        .db
        .room_photo(photo_id, meta["expires_at"].as_i64().unwrap())
        .unwrap()
        .is_none());
    s.public(&ann, false).await;
    assert_eq!(
        s.client.get(meta["image_url"].as_str().unwrap()).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn quota_and_expiry_are_enforced_without_deleting_live_photos() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (id, token) = s.bot(&ann, "Fern").await;
    s.consent(&ann, &id, true).await;
    let result = s.capture(&token, &ann.room, "room").await;
    assert!(result["isError"].is_null(), "{result}");
    let original = result["structuredContent"]["id"].as_str().unwrap();
    let at = agenthangout::now();
    let mut photo = s.state.db.room_photo(original, at).unwrap().unwrap();
    for n in 0..9 {
        photo.id = format!("quota-{n}");
        assert!(s
            .state
            .db
            .store_room_photo(
                &photo,
                &ann.id,
                std::slice::from_ref(&s.state.db.camera_subject(&id).unwrap().unwrap()),
                at
            )
            .unwrap());
    }
    photo.id = "over-quota".into();
    assert!(!s
        .state
        .db
        .store_room_photo(
            &photo,
            &ann.id,
            std::slice::from_ref(&s.state.db.camera_subject(&id).unwrap().unwrap()),
            at
        )
        .unwrap());
    assert!(s.state.db.room_photo(original, at).unwrap().is_some());
    let expired = photo.expires_at;
    photo.id = "after-expiry".into();
    photo.created_at = expired;
    photo.expires_at = expired + 86400;
    assert!(s
        .state
        .db
        .store_room_photo(
            &photo,
            &ann.id,
            std::slice::from_ref(&s.state.db.camera_subject(&id).unwrap().unwrap()),
            expired
        )
        .unwrap());
    assert!(s.state.db.room_photo(original, expired).unwrap().is_none());
}

#[tokio::test]
async fn malformed_frames_and_away_bots_cannot_capture() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (id, token) = s.bot(&ann, "Fern").await;
    s.consent(&ann, &id, true).await;
    assert_eq!(s.capture(&token, &ann.room, "3d").await["isError"], true);
    assert_eq!(s.capture(&token, &ann.room, "spot").await["isError"], true);
    let bo = s.owner("Bo").await;
    assert_eq!(s.capture(&token, &bo.room, "room").await["isError"], true);
    s.state.db.remove_member(&ann.room, &ann.id).unwrap();
    assert_eq!(s.capture(&token, &ann.room, "room").await["isError"], true);
}
