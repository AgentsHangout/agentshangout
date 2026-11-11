//! HC59 to HC64: a floor pools credits for one pack or one coin piece. Server routes only.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use agenthangout::{
    app, auth,
    db::{CreditKind, CreditSource, Db, NewPut, PutIn},
    items, now, AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

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

struct Bot {
    id: String,
    token: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

fn keys_of(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> =
        value.as_object().unwrap().keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::with_write_clock(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: Some(CODE.to_string()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
                require_rules: false,
            },
            now,
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self { base, client: Client::new(), state, addresses: AtomicUsize::new(1) }
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

    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        let room = text(&made["hangout"]["id"]);
        let who = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
        let items =
            self.get(&who.key, &format!("/api/hangouts/{room}")).await.ok()["items"].clone();
        for item in items.as_array().unwrap() {
            let id = text(&item["id"]);
            self.send(&who.key, Method::DELETE, &format!("/api/items/{id}"), None).await.ok();
        }
        (room, who)
    }

    async fn join(&self, owner: &Who, room: &str, name: &str) -> Who {
        let code = text(
            &self.get(&owner.key, &format!("/api/hangouts/{room}")).await.ok()["hangout"]
                ["invite_code"],
        );
        let body = self
            .post("", &format!("/api/invites/{code}/join"), json!({ "your_name": name }))
            .await
            .ok();
        Who { key: text(&body["owner_key"]), id: text(&body["person"]["id"]) }
    }

    fn bot(&self, owner: &Who, room: &str, name: &str) -> Bot {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        Bot { id: agent.id, token }
    }

    fn pay(&self, person: &str, source: &str, worth: fn(CreditKind) -> i64) {
        self.state
            .db
            .settle_credits(
                person,
                &[(
                    "seed".into(),
                    CreditSource { kind: CreditKind::Gathering, source: source.into(), at: now() },
                )],
                worth,
                100,
                now(),
            )
            .unwrap();
    }

    async fn balance(&self, token: &str) -> i64 {
        self.get(token, "/api/credits").await.ok()["balance"].as_i64().unwrap()
    }

    async fn credits(&self, token: &str) -> Value {
        self.get(token, "/api/credits").await.ok()
    }
}

fn credits40(_: CreditKind) -> i64 {
    40
}

fn credits80(_: CreditKind) -> i64 {
    80
}

fn credits20(_: CreditKind) -> i64 {
    20
}

fn fund_body(body: &Value) -> &Value {
    body.get("fund").unwrap_or(body)
}

const SHOWN: [&str; 8] =
    ["created_at", "ends_at", "id", "item", "status", "target", "total", "yours"];

// HC59: the target is the item's price, one open fund per floor per item, and a fund can't buy a
// car, a person, a free pack or a grand piece (HC65).
#[tokio::test]
async fn a_fund_targets_the_items_price_and_one_item_once() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.pay(&ann.id, "seed", credits40);
    let winter = items::pack_of("winter").unwrap();
    let started = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .ok();
    assert_eq!(started["item"], "winter");
    assert_eq!(started["target"], winter.cost);
    assert_eq!(started["total"], 0);
    assert_eq!(started["status"], "open");
    assert_eq!(
        started["ends_at"].as_i64().unwrap() - started["created_at"].as_i64().unwrap(),
        30 * 86_400
    );
    assert_eq!(keys_of(&started), SHOWN);

    let again = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .refused(StatusCode::CONFLICT);
    assert!(text(&again["error"]).contains("open fund"), "{again}");

    for item in [
        "car:beetle",
        "person:ann",
        "fall",
        "nope",
        "piece:koi_pond",
        "piece:grand_piano",
        "piece:dragon_statue",
        "piece:carousel",
    ] {
        let refused = server
            .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": item }))
            .await
            .refused(StatusCode::BAD_REQUEST);
        assert!(!text(&refused["error"]).is_empty(), "{item}");
    }
    let piece = format!("piece:{}", winter.items[0].kind);
    let piece_fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": piece }))
        .await
        .ok();
    assert_eq!(piece_fund["target"], items::piece_price(winter.items[0].kind).unwrap());
}

// HC59: three open funds a person started, and nothing for an item the floor already unlocked.
#[tokio::test]
async fn three_open_funds_and_nothing_already_unlocked() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.pay(&ann.id, "seed", credits80);
    let start = |item: &'static str| {
        let server = &server;
        let ann = ann.clone();
        let room = room.clone();
        async move {
            server
                .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": item }))
                .await
        }
    };
    let winter = start("winter").await.ok();
    start("stargazer").await.ok();
    let snowman = start("piece:snowman").await.ok();
    let fourth = start("piece:moon_lamp").await.refused(StatusCode::CONFLICT);
    assert!(text(&fourth["error"]).contains("3 open"), "{fourth}");

    server
        .post(&ann.key, &format!("/api/funds/{}/cancel", text(&winter["id"])), json!({}))
        .await
        .ok();
    start("piece:moon_lamp").await.ok();

    // Filling the snowman fund unlocks that piece, so another fund for it is refused.
    let price = items::piece_price("snowman").unwrap();
    let done = server
        .post(
            &ann.key,
            &format!("/api/funds/{}/put", text(&snowman["id"])),
            json!({ "amount": price }),
        )
        .await
        .ok();
    assert_eq!(fund_body(&done)["status"], "complete");
    let repeat = start("piece:snowman").await.refused(StatusCode::CONFLICT);
    assert!(text(&repeat["error"]).contains("already has"), "{repeat}");
}

// HC60: held credits go in, the same ones that buy a pack.
#[tokio::test]
async fn held_credits_can_go_into_a_fund() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.pay(&ann.id, "first:only", credits20);
    let price = items::piece_price("snowman").unwrap();
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:snowman" }))
        .await
        .ok();
    let id = text(&fund["id"]);
    let put = server
        .post(&ann.key, &format!("/api/funds/{id}/put"), json!({ "amount": price }))
        .await
        .ok();
    assert_eq!(put["put"], price);
    assert_eq!(fund_body(&put)["status"], "complete");
    assert_eq!(server.balance(&ann.key).await, 20 - price);
}

// HC60: a bot puts in only with its owner's cited yes, and only within the daily limit.
#[tokio::test]
async fn a_bot_puts_in_only_on_its_owners_cited_yes() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.pay(&ann.id, "seed", credits80);
    let scout = server.bot(&ann, &room, "Scout");
    server
        .put(&ann.key, &format!("/api/credits/bots/{}", scout.id), json!({ "daily": 4 }))
        .await
        .ok();
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .ok();
    let id = text(&fund["id"]);
    let put = format!("/api/funds/{id}/put");

    // A room message that says yes is not a yes, and neither is a missing question.
    server
        .post(
            &scout.token,
            &format!("/api/hangouts/{room}/messages"),
            json!({ "text": "yes, buy winter" }),
        )
        .await
        .ok();
    let room_yes = server
        .post(&scout.token, &put, json!({ "amount": 2 }))
        .await
        .refused(StatusCode::FORBIDDEN);
    assert!(text(&room_yes["error"]).contains("ask_owner"), "{room_yes}");

    let ask = |question: String| {
        let server = &server;
        let room = room.clone();
        let token = scout.token.clone();
        async move {
            text(
                &server
                    .post(
                        &token,
                        &format!("/api/hangouts/{room}/owner-questions"),
                        json!({ "question": question }),
                    )
                    .await
                    .ok()["id"],
            )
        }
    };
    let unanswered = ask("May I put credits into the winter fund?".to_string()).await;
    server
        .post(&scout.token, &put, json!({ "amount": 2, "asked": unanswered }))
        .await
        .refused(StatusCode::FORBIDDEN);
    let said_no = ask("May I put credits into the winter fund?".to_string()).await;
    server
        .post(
            &ann.key,
            &format!("/api/owner-questions/{said_no}/answer"),
            json!({ "answer": "no" }),
        )
        .await
        .ok();
    server
        .post(&scout.token, &put, json!({ "amount": 2, "asked": said_no }))
        .await
        .refused(StatusCode::FORBIDDEN);
    let other = ask("May I buy something else?".to_string()).await;
    server
        .post(&ann.key, &format!("/api/owner-questions/{other}/answer"), json!({ "answer": "yes" }))
        .await
        .ok();
    server
        .post(&scout.token, &put, json!({ "amount": 2, "asked": other }))
        .await
        .refused(StatusCode::FORBIDDEN);

    let yes = ask("May I put credits into the winter fund?".to_string()).await;
    server
        .post(&ann.key, &format!("/api/owner-questions/{yes}/answer"), json!({ "answer": "Yes" }))
        .await
        .ok();
    let taken = server.post(&scout.token, &put, json!({ "amount": 3, "asked": yes })).await.ok();
    assert_eq!(taken["put"], 3);
    // The limit is 4 a day, and 3 are already in, so 3 more is over it.
    let over = server
        .post(&scout.token, &put, json!({ "amount": 3, "asked": yes }))
        .await
        .refused(StatusCode::FORBIDDEN);
    assert!(text(&over["error"]).contains("may spend today"), "{over}");
    // The owner still can.
    server.post(&ann.key, &put, json!({ "amount": 1 })).await.ok();
}

// HC61: two puts at once clamp to what's left and complete the fund once.
#[tokio::test]
async fn a_race_clamps_each_put_and_completes_the_fund_once() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    server.pay(&ann.id, "seed", credits40);
    server.pay(&bo.id, "bo-seed", credits40);
    let before_ann = server.balance(&ann.key).await;
    let before_bo = server.balance(&bo.key).await;
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .ok();
    let id = text(&fund["id"]);
    let target = fund["target"].as_i64().unwrap();
    server
        .post(&ann.key, &format!("/api/funds/{id}/put"), json!({ "amount": target - 4 }))
        .await
        .ok();

    let state = server.state.clone();
    let fund_id = id.clone();
    let ann_id = ann.id.clone();
    let bo_id = bo.id.clone();
    let (left, right) = std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            state.db.put_in_fund(&NewPut {
                id: &auth::new_id(),
                fund_id: &fund_id,
                person_id: &ann_id,
                agent_id: None,
                amount: 6,
                now: now(),
            })
        });
        let two = scope.spawn(|| {
            state.db.put_in_fund(&NewPut {
                id: &auth::new_id(),
                fund_id: &fund_id,
                person_id: &bo_id,
                agent_id: None,
                amount: 6,
                now: now(),
            })
        });
        (one.join().unwrap().unwrap(), two.join().unwrap().unwrap())
    });
    let taken = |put: &PutIn| match put {
        PutIn::Taken { taken, fund } => {
            assert_eq!(fund.status, "complete");
            *taken
        }
        other => panic!("expected one put to finish it, got {other:?}"),
    };
    let (finished, refused) = match (&left, &right) {
        (done @ PutIn::Taken { .. }, PutIn::NotOpen) => (done, &right),
        (PutIn::NotOpen, done @ PutIn::Taken { .. }) => (done, &left),
        other => panic!("the race should complete once: {other:?}"),
    };
    assert_eq!(taken(finished), 4, "the second 6 is clamped to the 4 left");
    assert!(matches!(refused, PutIn::NotOpen));

    let listed = server.get(&ann.key, &format!("/api/floors/{room}/funds")).await.ok();
    let shown = &listed["funds"][0];
    assert_eq!(shown["status"], "complete");
    assert_eq!(shown["total"], target);
    let late = server
        .post(&bo.key, &format!("/api/funds/{id}/put"), json!({ "amount": 1 }))
        .await
        .refused(StatusCode::CONFLICT);
    assert!(text(&late["error"]).contains("no longer open"), "{late}");
    let spent =
        (before_ann - server.balance(&ann.key).await) + (before_bo - server.balance(&bo.key).await);
    assert_eq!(spent, target, "a race can't take more than the target");
    let cancel = server
        .post(&ann.key, &format!("/api/funds/{id}/cancel"), json!({}))
        .await
        .refused(StatusCode::CONFLICT);
    assert!(text(&cancel["error"]).contains("already complete"), "{cancel}");
    assert_eq!(
        (before_ann - server.balance(&ann.key).await) + (before_bo - server.balance(&bo.key).await),
        target,
        "cancelling a complete fund gives nothing back"
    );
}

// HC61: a pack fund unlocks what the pack places, not the coin pieces it only sells.
#[tokio::test]
async fn a_pack_fund_unlocks_only_what_the_pack_places() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    let scout = server.bot(&ann, &room, "Scout");
    server.pay(&ann.id, "seed", credits40);
    let upstairs = text(
        &server
            .post(&ann.key, &format!("/api/spaces/{room}/floors"), json!({ "name": "Up" }))
            .await
            .ok()["id"],
    );
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .ok();
    let id = text(&fund["id"]);
    let done =
        server.post(&ann.key, &format!("/api/funds/{id}/put"), json!({ "amount": 100 })).await.ok();
    assert_eq!(done["put"], fund["target"].as_i64().unwrap(), "clamped to the price");
    assert_eq!(fund_body(&done)["status"], "complete");

    server.place_ok(&ann.key, &room, "snowman", (31, 2)).await;
    server.place(&ann.key, &room, "snow_globe", (33, 2)).await.refused(StatusCode::FORBIDDEN);
    server.put(&ann.key, &format!("/api/floors/{room}"), json!({ "paint": "frost" })).await.ok();
    server.put(&ann.key, &format!("/api/floors/{room}"), json!({ "flooring": "snow" })).await.ok();
    // The pack didn't give the coin piece away, so a fund for it can still start.
    server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:snow_globe" }))
        .await
        .ok();
    let placed = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:snowman" }))
        .await
        .refused(StatusCode::CONFLICT);
    assert!(text(&placed["error"]).contains("already has"), "{placed}");

    // Builders on this floor can use the unlock. A walk-in, a bot, and another floor cannot.
    server.place_ok(&bo.key, &room, "snowman", (34, 2)).await;
    server.open(&ann, &room).await.ok();
    let (_, eve) = server.someone("Eve").await;
    server.post(&eve.key, &format!("/api/floors/{room}/walk-in"), json!({})).await.ok();
    server.place(&eve.key, &room, "pine_tree", (31, 4)).await.refused(StatusCode::FORBIDDEN);
    server.place(&scout.token, &room, "pine_tree", (32, 4)).await.refused(StatusCode::FORBIDDEN);
    let upstairs_no = server
        .place(&ann.key, &upstairs, "pine_tree", (31, 2))
        .await
        .refused(StatusCode::FORBIDDEN);
    assert!(text(&upstairs_no["error"]).contains("credits"), "{upstairs_no}");
}

// HC62: cancel gives every put-in back, and the sweep does too, once, without anyone opening the floor.
#[tokio::test]
async fn refunds_come_back_on_cancel_and_once_on_the_sweep() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    server.pay(&ann.id, "seed", credits40);
    server.pay(&bo.id, "bo-seed", credits40);
    let before_ann = server.balance(&ann.key).await;
    let before_bo = server.balance(&bo.key).await;

    let cancel_fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:sled" }))
        .await
        .ok();
    let cancel_id = text(&cancel_fund["id"]);
    // Less than the piece's price, so the fund stays open and cancel still has something to return.
    server
        .post(&ann.key, &format!("/api/funds/{cancel_id}/put"), json!({ "amount": 1 }))
        .await
        .ok();
    server.post(&bo.key, &format!("/api/funds/{cancel_id}/put"), json!({ "amount": 1 })).await.ok();
    assert!(server.balance(&ann.key).await < before_ann);
    let cancelled =
        server.post(&ann.key, &format!("/api/funds/{cancel_id}/cancel"), json!({})).await.ok();
    assert_eq!(cancelled["status"], "refunded");
    assert_eq!(server.balance(&ann.key).await, before_ann);
    assert_eq!(server.balance(&bo.key).await, before_bo);
    server
        .post(&ann.key, &format!("/api/funds/{cancel_id}/cancel"), json!({}))
        .await
        .refused(StatusCode::CONFLICT);
    server
        .post(&bo.key, &format!("/api/funds/{cancel_id}/cancel"), json!({}))
        .await
        .refused(StatusCode::FORBIDDEN);

    // Expiry doesn't wait for anyone to open the floor. The sweep is what the ticker runs.
    let expiring = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:sled" }))
        .await
        .ok();
    let expiring_id = text(&expiring["id"]);
    server
        .post(&bo.key, &format!("/api/funds/{expiring_id}/put"), json!({ "amount": 1 }))
        .await
        .ok();
    assert_eq!(server.balance(&bo.key).await, before_bo - 1);
    let end = expiring["ends_at"].as_i64().unwrap();
    assert_eq!(server.state.db.expire_funds(end - 1).unwrap(), 0);
    assert_eq!(server.balance(&bo.key).await, before_bo - 1);
    assert_eq!(server.state.db.expire_funds(end).unwrap(), 1);
    assert_eq!(server.balance(&bo.key).await, before_bo);
    assert_eq!(server.balance(&ann.key).await, before_ann);
    assert_eq!(server.state.db.expire_funds(end + 10).unwrap(), 0);
    assert_eq!(server.balance(&bo.key).await, before_bo);
}

// HC63: a person sees their own put-in. The fund shows the total and the target, not anyone else's.
#[tokio::test]
async fn a_person_sees_their_own_put_in_and_only_the_total() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let bo = server.join(&ann, &room, "Bo").await;
    let (_, stranger) = server.someone("Cara").await;
    server.pay(&ann.id, "seed", credits40);
    server.pay(&bo.id, "bo-seed", credits40);
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "winter" }))
        .await
        .ok();
    let id = text(&fund["id"]);
    server.post(&ann.key, &format!("/api/funds/{id}/put"), json!({ "amount": 4 })).await.ok();
    server.post(&bo.key, &format!("/api/funds/{id}/put"), json!({ "amount": 3 })).await.ok();

    let ann_list = server.get(&ann.key, &format!("/api/floors/{room}/funds")).await.ok();
    let ann_fund = &ann_list["funds"][0];
    assert_eq!(keys_of(ann_fund), SHOWN);
    assert_eq!(ann_fund["yours"], 4);
    assert_eq!(ann_fund["total"], 7);
    assert_eq!(ann_fund["target"], fund["target"]);
    let raw = ann_list.to_string();
    assert!(!raw.contains(&bo.id), "{raw}");
    assert!(!raw.contains(&ann.id), "{raw}");
    assert!(!raw.contains("\"puts\""), "{raw}");

    let bo_fund = &server.get(&bo.key, &format!("/api/floors/{room}/funds")).await.ok()["funds"][0];
    assert_eq!(bo_fund["yours"], 3);
    assert_eq!(bo_fund["total"], 7);
    assert!(!bo_fund.to_string().contains(&ann.id));

    server
        .get(&stranger.key, &format!("/api/floors/{room}/funds"))
        .await
        .refused(StatusCode::NOT_FOUND);
    // A public floor is visible to someone who isn't a member. They can put in, not start one,
    // and the unlock still doesn't let them build.
    server
        .put(
            &ann.key,
            &format!("/api/floors/{room}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();
    let seen = server.get(&stranger.key, &format!("/api/floors/{room}/funds")).await.ok();
    assert_eq!(seen["funds"][0]["total"], 7);
    assert_eq!(seen["funds"][0]["yours"], 0);
    server
        .post(&stranger.key, &format!("/api/floors/{room}/funds"), json!({ "item": "stargazer" }))
        .await
        .refused(StatusCode::FORBIDDEN);
}

// HC64: pooling credits mints nothing, and a bystander is untouched.
#[tokio::test]
async fn a_fund_mints_nothing() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    let (_, bystander) = server.someone("Dee").await;
    server.pay(&ann.id, "seed", credits40);
    server.pay(&bystander.id, "dee-seed", credits20);
    let ann_before = server.credits(&ann.key).await;
    let dee_before = server.balance(&bystander.key).await;
    let fund = server
        .post(&ann.key, &format!("/api/floors/{room}/funds"), json!({ "item": "piece:sled" }))
        .await
        .ok();
    let price = fund["target"].as_i64().unwrap();
    server
        .post(
            &ann.key,
            &format!("/api/funds/{}/put", text(&fund["id"])),
            json!({ "amount": price }),
        )
        .await
        .ok();
    let ann_after = server.credits(&ann.key).await;
    assert_eq!(ann_after["balance"], ann_before["balance"].as_i64().unwrap() - price);
    assert_eq!(ann_after["earned_today"], ann_before["earned_today"]);
    assert_eq!(ann_after["gatherings"], ann_before["gatherings"]);
    assert_eq!(server.balance(&bystander.key).await, dee_before);
    let history = ann_after["history"].as_array().unwrap();
    let line = history.iter().find(|line| line["kind"] == "fund").unwrap();
    assert_eq!(line["amount"], -price);
    assert_eq!(line["pack"], "piece:sled");
    assert!(line["bot"].is_null());
}

impl Server {
    async fn open(&self, owner: &Who, room: &str) -> Reply {
        self.put(
            &owner.key,
            &format!("/api/floors/{room}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
    }

    async fn place(&self, token: &str, room: &str, kind: &str, (x, y): (i64, i64)) -> Reply {
        self.post(
            token,
            &format!("/api/floors/{room}/items"),
            json!({ "kind": kind, "x": x, "y": y }),
        )
        .await
    }

    async fn place_ok(&self, token: &str, room: &str, kind: &str, at: (i64, i64)) {
        self.place(token, room, kind, at).await.ok();
    }
}
