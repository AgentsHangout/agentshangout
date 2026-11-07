//! CP1–CP7, CP15–CP17: real HTTP/MCP camera access, photo settings, images, revocation, quotas
//! and deleting.
mod support;

use agenthangout::camera::{PhotoStore, StoredPhoto};
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
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, &owner.room, name);
        (agent.id, token)
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
async fn public_photo_is_png_but_private_text_and_bots_with_photos_off_are_absent() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (id, token) = s.bot(&ann, "Dotliver Tree").await;
    let (hidden, hidden_token) = s.bot(&ann, "Hidden Camera Sentinel").await;
    // CP2: on by default, so nobody had to turn anything on. A bot with photos off can't take one.
    s.consent(&ann, &hidden, false).await;
    assert_eq!(s.capture(&hidden_token, &ann.room, "room").await["isError"], true);
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
async fn an_owner_keeps_ten_photos_and_expiry_frees_them() {
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
                std::slice::from_ref(&s.state.db.camera_subject("bot", &id).unwrap().unwrap()),
                at
            )
            .unwrap()
            .is_stored());
    }
    // CP16: an eleventh rotates one out rather than keeping eleven.
    photo.id = "over-quota".into();
    let stored = s
        .state
        .db
        .store_room_photo(
            &photo,
            &ann.id,
            std::slice::from_ref(&s.state.db.camera_subject("bot", &id).unwrap().unwrap()),
            at,
        )
        .unwrap();
    assert!(matches!(stored, PhotoStore::Stored { rotated: Some(_) }), "{stored:?}");
    let live = std::iter::once(original.to_string())
        .chain((0..9).map(|n| format!("quota-{n}")))
        .chain(["over-quota".to_string()])
        .filter(|id| s.state.db.room_photo(id, at).unwrap().is_some())
        .count();
    assert_eq!(live, 10);
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
            std::slice::from_ref(&s.state.db.camera_subject("bot", &id).unwrap().unwrap()),
            expired
        )
        .unwrap()
        .is_stored());
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

impl Server {
    async fn photo_status(&self, meta: &Value) -> StatusCode {
        self.client.get(meta["image_url"].as_str().unwrap()).send().await.unwrap().status()
    }
}

#[tokio::test]
async fn people_with_the_floor_open_are_in_photos_until_they_leave_themselves_out() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (_, token) = s.bot(&ann, "Fern").await;
    s.state.presence.saw(&ann.room, &ann.id, agenthangout::now());
    let together = s.capture(&token, &ann.room, "room").await;
    let meta = &together["structuredContent"];
    assert_eq!(meta["people"], json!([{"name":"Ann"}]), "{together}");
    assert!(meta["caption"].as_str().unwrap().contains("Ann"));
    assert!(!meta.to_string().contains(&ann.id));
    // Only a person changes their own setting, and leaving out takes their photos down.
    assert_eq!(
        s.request(&token, Method::PUT, "/api/camera/me", json!({"allowed":false})).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(s.ok(&ann.key, Method::GET, "/api/camera/me", json!({})).await["allowed"], true);
    s.ok(&ann.key, Method::PUT, "/api/camera/me", json!({"allowed":false})).await;
    assert_eq!(s.photo_status(meta).await, StatusCode::NOT_FOUND);
    let without = s.capture(&token, &ann.room, "room").await;
    assert_eq!(without["structuredContent"]["people"], json!([]), "{without}");
    // On again brings back nothing taken before, and a photo she wasn't in stays up.
    s.ok(&ann.key, Method::PUT, "/api/camera/me", json!({"allowed":true})).await;
    assert_eq!(s.photo_status(meta).await, StatusCode::NOT_FOUND);
    assert_eq!(s.photo_status(&without["structuredContent"]).await, StatusCode::OK);
    let again = s.capture(&token, &ann.room, "room").await;
    assert_eq!(again["structuredContent"]["people"], json!([{"name":"Ann"}]), "{again}");
}

#[tokio::test]
async fn whoever_runs_a_room_can_turn_its_photos_off_and_that_takes_them_all_down() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    s.public(&ann, true).await;
    let (_, token) = s.bot(&ann, "Fern").await;
    let path = format!("/api/floors/{}/camera", ann.room);
    assert_eq!(s.ok(&ann.key, Method::GET, &path, json!({})).await["allowed"], true);
    let before = s.capture(&token, &ann.room, "room").await;
    assert!(before["isError"].is_null(), "{before}");
    let meta = before["structuredContent"].clone();
    assert!(!s.request(&bo.key, Method::PUT, &path, json!({"allowed":false})).await.0.is_success());
    assert!(!s.request(&token, Method::PUT, &path, json!({"allowed":false})).await.0.is_success());
    assert_eq!(s.photo_status(&meta).await, StatusCode::OK);
    s.ok(&ann.key, Method::PUT, &path, json!({"allowed":false})).await;
    assert_eq!(s.photo_status(&meta).await, StatusCode::NOT_FOUND);
    let refused = s.capture(&token, &ann.room, "room").await;
    assert_eq!(refused["isError"], true, "{refused}");
    s.ok(&ann.key, Method::PUT, &path, json!({"allowed":true})).await;
    assert_eq!(s.photo_status(&meta).await, StatusCode::NOT_FOUND);
    assert!(s.capture(&token, &ann.room, "room").await["isError"].is_null());
}
#[tokio::test]
async fn photos_draw_a_bot_at_what_it_uses_until_it_stops_or_the_use_runs_out() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (sitter, token) = s.bot(&ann, "Sitter").await;
    let (shy, shy_token) = s.bot(&ann, "Shy Sentinel").await;
    s.bot(&ann, "Idle").await;
    let mut stools = Vec::new();
    for x in [30, 10] {
        let placed = s
            .ok(
                &ann.key,
                Method::POST,
                &format!("/api/floors/{}/items", ann.room),
                json!({"kind":"stool","x":x,"y":5}),
            )
            .await;
        stools.push(placed["id"].as_str().unwrap().to_string());
    }
    s.ok(&token, Method::POST, &format!("/api/items/{}/use", stools[0]), json!({})).await;
    s.ok(&shy_token, Method::POST, &format!("/api/items/{}/use", stools[1]), json!({})).await;
    // CP2: a bot left out of the photo isn't in it at all, using something or not.
    s.consent(&ann, &shy, false).await;
    let doing = |photo: &Value| {
        assert!(!photo.to_string().contains("Shy Sentinel"), "{photo}");
        photo["structuredContent"]["bots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| (b["name"].as_str().unwrap().to_string(), b["doing"].clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let photo = s.capture(&token, &ann.room, "room").await;
    assert_eq!(
        doing(&photo),
        [("Idle".into(), Value::Null), ("Sitter".into(), json!("sitting on the stool"))].into()
    );
    // I33, CP9: once it stops it's off the stool. It has just acted, so it's active, and an active
    // bot sits at the table as on the page; the idle one still stands with nothing to say.
    let at_table = || -> std::collections::BTreeMap<String, Value> {
        [("Idle".into(), Value::Null), ("Sitter".into(), json!("sitting at the table"))].into()
    };
    s.ok(&token, Method::DELETE, "/api/items/use", json!({})).await;
    let photo = s.capture(&token, &ann.room, "room").await;
    assert_eq!(doing(&photo), at_table(), "{photo}");
    // A use that ran out counts as stopped.
    let at = agenthangout::now();
    s.state
        .db
        .use_item(&sitter, &stools[0], at - 1, at - 200, |_| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    let photo = s.capture(&token, &ann.room, "room").await;
    assert_eq!(doing(&photo), at_table(), "{photo}");
}

impl Server {
    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        self.ok(token,Method::POST,"/mcp",json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await["result"].clone()
    }
    /// Stores a copy of `like` for `owner` straight into the database, so a test isn't held up by
    /// the three captures a minute.
    fn seed(&self, like: &StoredPhoto, id: &str, owner: &str, bot: &str, expires_at: i64) {
        let photo = StoredPhoto {
            id: id.into(),
            room_epoch: like.room_epoch.clone(),
            room_id: like.room_id.clone(),
            created_at: like.created_at,
            expires_at,
            png: like.png.clone(),
            metadata: like.metadata.clone(),
        };
        let subject = self.state.db.camera_subject("bot", bot).unwrap().unwrap();
        assert!(self
            .state
            .db
            .store_room_photo(&photo, owner, std::slice::from_ref(&subject), agenthangout::now())
            .unwrap()
            .is_stored());
    }
    async fn image_status(&self, id: &str) -> StatusCode {
        self.client.get(format!("{}/photos/{id}.png", self.base)).send().await.unwrap().status()
    }
}

/// CP15: a refusal says which it is, so a bot can stop retrying.
#[tokio::test]
async fn a_photo_refused_for_settings_says_photo_access_changed() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (id, token) = s.bot(&ann, "Fern").await;
    s.consent(&ann, &id, false).await;
    let refused = s.capture(&token, &ann.room, "room").await;
    assert_eq!(refused["isError"], true, "{refused}");
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("photo_access_changed: "), "{text}");
    assert_eq!(refused["structuredContent"]["code"], "photo_access_changed", "{refused}");
    s.consent(&ann, &id, true).await;
    let path = format!("/api/floors/{}/camera", ann.room);
    s.ok(&ann.key, Method::PUT, &path, json!({"allowed":false})).await;
    let refused = s.capture(&token, &ann.room, "room").await;
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("photo_access_changed: "), "{refused}");
    assert_eq!(refused["structuredContent"]["code"], "photo_access_changed", "{refused}");
}

/// CP16: an owner's eleventh photo takes the place of their oldest, whose URL stops working. The
/// ten are shared by every bot of that owner.
#[tokio::test]
async fn an_eleventh_photo_rotates_out_the_owners_oldest() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (fern, fern_token) = s.bot(&ann, "Fern").await;
    let (_, moss_token) = s.bot(&ann, "Moss").await;
    let first = s.capture(&fern_token, &ann.room, "room").await;
    assert!(first["isError"].is_null(), "{first}");
    let oldest = first["structuredContent"]["id"].as_str().unwrap().to_string();
    assert!(first["structuredContent"]["rotated"].is_null(), "{first}");
    let like = s.state.db.room_photo(&oldest, agenthangout::now()).unwrap().unwrap();
    for n in 0..9 {
        s.seed(&like, &format!("kept-{n}"), &ann.id, &fern, like.expires_at + 1 + n);
    }
    let eleventh = s.capture(&moss_token, &ann.room, "room").await;
    assert!(eleventh["isError"].is_null(), "{eleventh}");
    assert_eq!(eleventh["structuredContent"]["rotated"], oldest.as_str(), "{eleventh}");
    assert_eq!(s.image_status(&oldest).await, StatusCode::NOT_FOUND);
    for n in 0..9 {
        assert_eq!(s.image_status(&format!("kept-{n}")).await, StatusCode::OK, "kept-{n}");
    }
    assert_eq!(s.photo_status(&eleventh["structuredContent"]).await, StatusCode::OK);
}

/// CP16: two hundred photos on the site refuse with photo_quota_full and say when the next frees,
/// rather than taking down someone else's photo.
#[tokio::test]
async fn a_full_site_refuses_with_photo_quota_full_and_when_the_next_frees() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (fern, token) = s.bot(&ann, "Fern").await;
    let first = s.capture(&token, &ann.room, "room").await;
    let first_id = first["structuredContent"]["id"].as_str().unwrap().to_string();
    let like = s.state.db.room_photo(&first_id, agenthangout::now()).unwrap().unwrap();
    // Ann keeps the one she took; nineteen others keep ten each, and one keeps nine.
    let mut soonest = like.expires_at;
    for p in 0..20 {
        let person = format!("other-{p}");
        s.state.db.create_person(&person, &person, &person, &person, 1).unwrap();
        for n in 0..if p == 19 { 9 } else { 10 } {
            let expires = like.expires_at + 500 - p * 10 - n;
            soonest = soonest.min(expires);
            s.seed(&like, &format!("{person}-{n}"), &person, &fern, expires);
        }
    }
    let refused = s.capture(&token, &ann.room, "room").await;
    assert_eq!(refused["isError"], true, "{refused}");
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("photo_quota_full: "), "{text}");
    let next = agenthangout::ops::iso(soonest);
    assert!(text.contains(&next), "{text}");
    let details = &refused["structuredContent"];
    assert_eq!(details["code"], "photo_quota_full", "{refused}");
    assert_eq!(details["kept"], 200, "{refused}");
    assert_eq!(details["next_frees_at"], next.as_str(), "{refused}");
    assert_eq!(s.image_status("other-0-0").await, StatusCode::OK);
    assert_eq!(s.image_status(&first_id).await, StatusCode::OK);
}

/// CP17: the owner, or any of the owner's bots, can take one of the owner's photos down early.
/// Anyone else is told it isn't there.
#[tokio::test]
async fn delete_photo_is_for_the_owner_and_their_bots_and_not_found_for_anyone_else() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    s.public(&ann, true).await;
    let (fern, fern_token) = s.bot(&ann, "Fern").await;
    let (_, moss_token) = s.bot(&ann, "Moss").await;
    let (_, stranger_token) = s.bot(&bo, "Stranger").await;
    let taken = s.capture(&fern_token, &ann.room, "room").await;
    let id = taken["structuredContent"]["id"].as_str().unwrap().to_string();
    let like = s.state.db.room_photo(&id, agenthangout::now()).unwrap().unwrap();
    s.seed(&like, "second", &ann.id, &fern, like.expires_at);
    // Another person's bot, and another person, get what a missing photo gets.
    let refused = s.tool(&stranger_token, "delete_photo", json!({"photo_id":id})).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(refused["content"][0]["text"], "not found", "{refused}");
    let missing = s.tool(&stranger_token, "delete_photo", json!({"photo_id":"nope"})).await;
    assert_eq!(missing["content"][0]["text"], "not found", "{missing}");
    let path = format!("/api/photos/{id}");
    let (status, body) = s.request(&bo.key, Method::DELETE, &path, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(s.image_status(&id).await, StatusCode::OK);
    // The owner's other bot can.
    let deleted = s.tool(&moss_token, "delete_photo", json!({"photo_id":id})).await;
    assert!(deleted["isError"].is_null(), "{deleted}");
    assert_eq!(deleted["structuredContent"]["deleted"], id.as_str(), "{deleted}");
    assert_eq!(s.image_status(&id).await, StatusCode::NOT_FOUND);
    let again = s.tool(&moss_token, "delete_photo", json!({"photo_id":id})).await;
    assert_eq!(again["content"][0]["text"], "not found", "{again}");
    // So can the owner, as a person.
    s.ok(&ann.key, Method::DELETE, "/api/photos/second", json!({})).await;
    assert_eq!(s.image_status("second").await, StatusCode::NOT_FOUND);
    assert_eq!(
        s.request(&ann.key, Method::DELETE, "/api/photos/second", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
}

/// CP16: captures that store at once still leave the owner exactly ten, each rotating out a
/// different photo.
#[tokio::test]
async fn parallel_photos_for_one_owner_leave_exactly_ten() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    s.public(&ann, true).await;
    let (fern, token) = s.bot(&ann, "Fern").await;
    let first = s.capture(&token, &ann.room, "room").await;
    let first_id = first["structuredContent"]["id"].as_str().unwrap().to_string();
    let like = s.state.db.room_photo(&first_id, agenthangout::now()).unwrap().unwrap();
    for n in 0..9 {
        s.seed(&like, &format!("seed-{n}"), &ann.id, &fern, like.expires_at + 1 + n);
    }
    let subject = s.state.db.camera_subject("bot", &fern).unwrap().unwrap();
    let at = agenthangout::now();
    let rotated: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let (db, like, subject, owner) = (&s.state.db, &like, &subject, &ann.id);
                scope.spawn(move || {
                    let photo = StoredPhoto {
                        id: format!("race-{n}"),
                        room_epoch: like.room_epoch.clone(),
                        room_id: like.room_id.clone(),
                        created_at: at,
                        expires_at: like.expires_at + 100 + n,
                        png: like.png.clone(),
                        metadata: like.metadata.clone(),
                    };
                    let subjects = std::slice::from_ref(subject);
                    match db.store_room_photo(&photo, owner, subjects, at).unwrap() {
                        PhotoStore::Stored { rotated: Some(old) } => old,
                        other => panic!("race-{n} was not stored by rotating: {other:?}"),
                    }
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    let mut distinct = rotated.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 8, "{rotated:?}");
    let all = std::iter::once(first_id.clone())
        .chain((0..9).map(|n| format!("seed-{n}")))
        .chain((0..8).map(|n| format!("race-{n}")));
    let live = all.filter(|id| s.state.db.room_photo(id, at).unwrap().is_some()).count();
    assert_eq!(live, 10);
    // The oldest eight went: the first photo and seed-0 to seed-6.
    assert!(rotated.contains(&first_id), "{rotated:?}");
    for id in ["seed-7", "seed-8"] {
        assert!(s.state.db.room_photo(id, at).unwrap().is_some(), "{id}");
    }
}
