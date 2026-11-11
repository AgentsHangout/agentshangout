//! GM9 (#905): a solo game play hints that a bot here can be challenged. #1119: the hint only
//! follows duel settings. Nobody is hinted unless the player's own duels are on, and the bot it
//! names must have its duels on too.

mod support;

use agenthangout::{
    auth::hash_secret,
    db::{Db, DuelAllow},
    items, now, AppState, Config,
};
use serde_json::{json, Value};

const CODE: &str = "letmein";

async fn post(base: &str, key: &str, path: &str, body: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.text().await.unwrap());
    response.json().await.unwrap()
}

struct World {
    state: AppState,
    ann: String,
    arcade: String,
}

/// Ann's arcade with a cabinet in it, and a bot called Faraway in another room.
async fn world() -> World {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: Some(CODE.to_string()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: Vec::new(),
            require_rules: false,
        },
    );
    let served = state.clone();
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(served)).await });
    let made = post(
        &base,
        "",
        "/api/hangouts",
        json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }),
    )
    .await;
    let key = made["owner_key"].as_str().unwrap().to_string();
    let ann = made["person"]["id"].as_str().unwrap().to_string();
    let arcade =
        post(&base, &key, "/api/hangouts", json!({ "name": "Arcade", "create_code": CODE })).await
            ["hangout"]["id"]
            .as_str()
            .unwrap()
            .to_string();
    let den = post(&base, &key, "/api/hangouts", json!({ "name": "Den", "create_code": CODE }))
        .await["hangout"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    support::legacy_bot(&state, &ann, &den, "Faraway");
    state
        .db
        .place_item::<()>(
            &agenthangout::db::NewFloorItem {
                id: "cabinet-1",
                hangout_id: &arcade,
                kind: "arcade_cabinet",
                x: 10,
                y: 6,
                facing: 0,
                placed_by: &ann,
                now: now(),
            },
            |_, _, _| Ok(()),
        )
        .unwrap()
        .unwrap();
    World { state, ann, arcade }
}

/// A bot in the arcade, with its duels set to `allow` ("room" or "off"), plays the cabinet.
fn play(w: &World, name: &str, allow: &str, challenge: Option<&str>) -> Value {
    let (bot, token) = support::legacy_bot(&w.state, &w.ann, &w.arcade, name);
    let set = DuelAllow { allow: allow.to_string(), daily_cap: 5 };
    w.state.db.set_duel_allow(&bot.id, &set, now()).unwrap();
    let caller = w.state.authenticate(&hash_secret(&token)).unwrap().unwrap();
    items::use_item_against(&w.state, &caller, "cabinet-1", challenge).unwrap()
}

#[tokio::test]
async fn a_solo_play_names_one_bot_here_and_only_then() {
    let w = world().await;
    // Alone (Faraway is in another room): no hint.
    assert!(play(&w, "Scout", "room", None).get("challenge_hint").is_none());
    // Scout is here now, with duels on too: the hint names it, and only it.
    let hint = play(&w, "Rex", "room", None)["challenge_hint"].as_str().unwrap().to_string();
    assert_eq!(hint, "Add challenge: \"Scout\" to play someone here; the room hears who won.");
    // First by name, never the caller or the bot elsewhere.
    let hint = play(&w, "Cleo", "room", None)["challenge_hint"].as_str().unwrap().to_string();
    assert!(hint.contains("\"Rex\"") && !hint.contains("Faraway"), "{hint}");
    // A challenge gets no hint (GM11: both owners allow duels here, so it asks).
    let (fresh, _) = support::legacy_bot(&w.state, &w.ann, &w.arcade, "Fresh");
    let (zed, token) = support::legacy_bot(&w.state, &w.ann, &w.arcade, "Zed");
    for bot in [&fresh.id, &zed.id] {
        let room = DuelAllow { allow: "room".to_string(), daily_cap: 5 };
        w.state.db.set_duel_allow(bot, &room, now()).unwrap();
    }
    let zed = w.state.authenticate(&hash_secret(&token)).unwrap().unwrap();
    let asked = items::use_item_against(&w.state, &zed, "cabinet-1", Some("Fresh")).unwrap();
    assert!(asked.get("challenge_hint").is_none() && asked["status"] == "pending", "{asked}");
}

#[tokio::test]
async fn no_hint_when_my_own_duels_are_off() {
    let w = world().await;
    // Scout is here with duels on, but Rex's own duels are off: no hint at all.
    assert!(play(&w, "Scout", "room", None).get("challenge_hint").is_none());
    let rex = play(&w, "Rex", "off", None);
    assert!(rex.get("challenge_hint").is_none(), "{rex}");
    assert_eq!(rex["ok"], true);
}

#[tokio::test]
async fn no_hint_when_only_the_other_bots_duels_are_off() {
    let w = world().await;
    // Rex's duels are on, but the only other bot here, Scout, has its duels off: no hint.
    assert!(play(&w, "Scout", "off", None).get("challenge_hint").is_none());
    let rex = play(&w, "Rex", "room", None);
    assert!(rex.get("challenge_hint").is_none(), "{rex}");
    assert_eq!(rex["ok"], true);
}

#[tokio::test]
async fn with_both_duels_on_the_hint_names_that_bot() {
    let w = world().await;
    assert!(play(&w, "Scout", "room", None).get("challenge_hint").is_none());
    let rex = play(&w, "Rex", "room", None);
    assert_eq!(
        rex["challenge_hint"].as_str().unwrap(),
        "Add challenge: \"Scout\" to play someone here; the room hears who won."
    );
}

// #1119: a bot whose duels are friends-only is named only to a bot it's friends with.
#[tokio::test]
async fn a_friends_only_bot_is_named_only_to_its_friend() {
    let w = world().await;
    let (rex, token) = support::legacy_bot(&w.state, &w.ann, &w.arcade, "Rex");
    let room = DuelAllow { allow: "room".to_string(), daily_cap: 5 };
    w.state.db.set_duel_allow(&rex.id, &room, now()).unwrap();
    let friends = DuelAllow { allow: "friends".to_string(), daily_cap: 5 };
    let (pal, _) = support::legacy_bot(&w.state, &w.ann, &w.arcade, "Pal");
    let (stranger, _) = support::legacy_bot(&w.state, &w.ann, &w.arcade, "Stranger");
    for bot in [&pal.id, &stranger.id] {
        w.state.db.set_duel_allow(bot, &friends, now()).unwrap();
    }
    w.state.db.request_explicit_friend("bot", &rex.id, &pal.id, now()).unwrap();
    assert!(w.state.db.accept_explicit_friend("bot", &rex.id, &pal.id).unwrap());
    let caller = w.state.authenticate(&hash_secret(&token)).unwrap().unwrap();
    let reply = items::use_item_against(&w.state, &caller, "cabinet-1", None).unwrap();
    assert_eq!(
        reply["challenge_hint"].as_str().unwrap(),
        "Add challenge: \"Pal\" to play someone here; the room hears who won."
    );
}
