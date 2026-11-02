use agenthangout::{app, auth::hash_secret, db::Db, now, AppState, Config};
use reqwest::{Client, StatusCode};
use serde_json::json;

#[tokio::test]
async fn pn1_push_status_is_explicit_and_requires_a_session() {
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: false,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    state.db.create_person("person", "Person", &hash_secret("owner"), "feed", now()).unwrap();
    let session = state.start_session("person").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });
    let client = Client::new();
    let url = format!("{base}/api/push/status");
    assert_eq!(client.get(&url).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        client.get(&url).bearer_auth("owner").send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let response: serde_json::Value =
        client.get(&url).bearer_auth(&session).send().await.unwrap().json().await.unwrap();
    assert_eq!(response, json!({"configured":false,"admin":false,"owner_id":"person"}));
    assert_eq!(
        client
            .post(format!("{base}/api/push/test"))
            .bearer_auth(&session)
            .json(&json!({"device_id":"12345678-1234-1234-1234-123456789abc"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

struct Capture(std::sync::Mutex<Vec<agenthangout::push::Message>>);
impl agenthangout::push::Transport for Capture {
    fn deliver<'a>(
        &'a self,
        _: &'a str,
        m: &'a agenthangout::push::Message,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = agenthangout::push::Outcome> + Send + 'a>>
    {
        Box::pin(async move {
            self.0.lock().unwrap().push(m.clone());
            agenthangout::push::Outcome::Delivered
        })
    }
}
fn state() -> AppState {
    AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: false,
            keeper_id: Some("one".into()),
            god_ids: vec![],
        },
    )
}
fn person(state: &AppState, id: &str) -> String {
    state.db.create_person(id, id, &hash_secret(id), &format!("feed-{id}"), now()).unwrap();
    state.start_session(id).unwrap()
}
fn device(id: &str, owner: &str) -> agenthangout::db::PushDevice {
    agenthangout::db::PushDevice {
        id: id.into(),
        person_id: owner.into(),
        token: format!("private-fcm-token-for-{id}"),
        enabled: true,
        mentions: true,
        requests: true,
        announcements: true,
    }
}
async fn settle(state: &AppState) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.push_slots.available_permits() != 64 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("push queue did not drain");
}

// PN2: device ownership, capacity and token reassociation are atomic.
#[test]
fn pn2_ownership_capacity_and_sessions() {
    let s = state();
    let a = person(&s, "one");
    let b = person(&s, "two");
    let mut d = device("device", "one");
    assert!(s.db.put_push_device("one", &hash_secret(&a), &d).unwrap());
    d.token = "different-private-fcm-token".into();
    assert!(!s.db.put_push_device("two", &hash_secret(&b), &d).unwrap());
    s.db.delete_push_device("two", "device").unwrap();
    assert_eq!(s.db.push_devices(Some("one"), now()).unwrap().len(), 1);
    for i in 0..9 {
        assert!(s
            .db
            .put_push_device("one", &hash_secret(&a), &device(&format!("d{i}"), "one"))
            .unwrap());
    }
    assert!(!s.db.put_push_device("one", &hash_secret(&a), &device("overflow", "one")).unwrap());
    let other = s.start_session("one").unwrap();
    let first = s.db.push_devices(Some("one"), now()).unwrap().remove(0);
    assert!(s.db.put_push_device("one", &hash_secret(&other), &first).unwrap());
    s.db.delete_session(&hash_secret(&a)).unwrap();
    assert_eq!(s.db.push_devices(Some("one"), now()).unwrap().len(), 1);
    assert_eq!(
        s.db.push_devices(Some("one"), now() + agenthangout::auth::SESSION_SECS + 1).unwrap().len(),
        0
    );
    s.db.delete_person("one", "one").unwrap();
    assert!(s.db.push_devices(None, now()).unwrap().is_empty());
}
// PN3/PN4: webhook-independent push uses an event allowlist and never sends notice text.
#[tokio::test]
async fn pn3_generic_delivery_and_immediate_off() {
    let s = state();
    let session = person(&s, "one");
    let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(vec![])));
    assert!(s.push.set(capture.clone()).is_ok());
    let mut d = device("phone", "one");
    s.db.put_push_device("one", &hash_secret(&session), &d).unwrap();
    for event in [
        "mention",
        "owner_question.asked",
        "check_in.waiting",
        "plan.proposed",
        "owner_question.answered",
    ] {
        agenthangout::notify::send(
            &s,
            &["one".into(), "one".into()],
            &agenthangout::notify::Notice {
                event,
                hangout_id: "safe_room".into(),
                text: "PRIVATE QUESTION DO NOT LEAK".into(),
            },
        );
    }
    settle(&s).await;
    {
        let messages = capture.0.lock().unwrap();
        assert_eq!(messages.len(), 4);
        for m in messages.iter() {
            assert_eq!(m.owner_id, "one");
            assert_eq!(m.path, "/h/safe_room");
            assert!(!m.body.contains("PRIVATE"));
        }
    }
    agenthangout::notify::send(
        &s,
        &["one".into()],
        &agenthangout::notify::Notice {
            event: "owner_question.asked",
            hangout_id: "room".into(),
            text: "private".into(),
        },
    );
    d.enabled = false;
    s.db.put_push_device("one", &hash_secret(&session), &d).unwrap();
    agenthangout::notify::send(
        &s,
        &["one".into()],
        &agenthangout::notify::Notice {
            event: "mention",
            hangout_id: "../unsafe".into(),
            text: "private".into(),
        },
    );
    settle(&s).await;
    assert_eq!(capture.0.lock().unwrap().len(), 4);
}

// PN3, N10: what Needs you lists reaches the phone as a request, generic, opening the map.
#[tokio::test]
async fn pn3_what_needs_you_lists_is_a_request_on_the_phone() {
    let s = state();
    let session = person(&s, "one");
    let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(vec![])));
    assert!(s.push.set(capture.clone()).is_ok());
    s.db.put_push_device("one", &hash_secret(&session), &device("phone", "one")).unwrap();
    for event in ["owner.waiting", "place.asked", "friend.invite", "apprentice.asked"] {
        agenthangout::notify::send(
            &s,
            &["one".into()],
            &agenthangout::notify::Notice {
                event,
                hangout_id: String::new(),
                text: "PRIVATE DO NOT LEAK".into(),
            },
        );
    }
    settle(&s).await;
    let messages = capture.0.lock().unwrap();
    assert_eq!(messages.len(), 4);
    for m in messages.iter() {
        assert_eq!(m.category, "request");
        assert_eq!(m.path, "/");
        assert!(!m.body.contains("PRIVATE"));
    }
}

#[tokio::test]
async fn pn5_admin_and_device_endpoints() {
    let s = state();
    let one = person(&s, "one");
    let two = person(&s, "two");
    let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(vec![])));
    assert!(s.push.set(capture.clone()).is_ok());
    s.db.create_agent("bot", "one", "Bot", &hash_secret("bot-secret"), now()).unwrap();
    s.db.put_push_device("two", &hash_secret(&two), &device("second-phone", "two")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = s.clone();
    tokio::spawn(async move {
        axum::serve(listener, app(served)).await.unwrap();
    });
    let c = Client::new();
    let id = "12345678-1234-1234-1234-123456789abc";
    let body = json!({"token":"private-fcm-token-for-real-phone","enabled":true,"mentions":true,"requests":false,"announcements":false});
    let url = format!("{base}/api/push/devices/{id}");
    assert_eq!(
        c.put(&url).bearer_auth("bot-secret").json(&body).send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.put(&url).bearer_auth(&one).json(&body).send().await.unwrap().status(),
        StatusCode::OK
    );
    let mut other = body.clone();
    other["token"] = json!("different-private-fcm-token-other-phone");
    assert_eq!(
        c.put(&url).bearer_auth(&two).json(&other).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    c.delete(&url).bearer_auth(&two).send().await.unwrap();
    assert_eq!(s.db.push_devices(Some("one"), now()).unwrap().len(), 1);
    let test = format!("{base}/api/push/test");
    assert_eq!(
        c.post(&test)
            .bearer_auth(&two)
            .json(&json!({"device_id":id}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.post(&test)
            .bearer_auth(&one)
            .json(&json!({"device_id":id}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let announcement = format!("{base}/api/push/announcements");
    let text = json!({"title":"Maintenance","body":"The server will restart soon.\nThank you!"});
    assert_eq!(
        c.post(&announcement).bearer_auth(&two).json(&text).send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        c.post(&announcement)
            .bearer_auth(&one)
            .json(&json!({"title":"x".repeat(81),"body":"Text"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    for _ in 0..3 {
        assert_eq!(
            c.post(&announcement).bearer_auth(&one).json(&text).send().await.unwrap().status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        c.post(&announcement).bearer_auth(&one).json(&text).send().await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    settle(&s).await;
    let sent = capture.0.lock().unwrap();
    assert_eq!(sent.len(), 4);
    assert_eq!(sent.iter().filter(|m| m.category == "test" && m.owner_id == "one").count(), 1);
    assert_eq!(
        sent.iter()
            .filter(|m| m.category == "announcement"
                && m.owner_id == "two"
                && m.title == "Maintenance")
            .count(),
        3
    );
}

// PN2/PN4: token replacement races cannot delete the new address or duplicate its ownership.
#[test]
fn pn2_token_rotation_and_account_switch() {
    let s = state();
    let one = person(&s, "one");
    let two = person(&s, "two");
    let mut d = device("phone", "one");
    s.db.put_push_device("one", &hash_secret(&one), &d).unwrap();
    let old = d.token.clone();
    d.token = "new-private-fcm-token-for-phone".into();
    s.db.put_push_device("one", &hash_secret(&one), &d).unwrap();
    s.db.invalidate_push_token(&d.id, &old).unwrap();
    assert_eq!(s.db.push_devices(Some("one"), now()).unwrap().len(), 1);
    s.db.put_push_device("two", &hash_secret(&two), &d).unwrap();
    assert!(s.db.push_devices(Some("one"), now()).unwrap().is_empty());
    assert_eq!(s.db.push_devices(Some("two"), now()).unwrap().len(), 1);
    s.db.invalidate_push_token(&d.id, &d.token).unwrap();
    assert!(s.db.push_devices(None, now()).unwrap().is_empty());
}

// E30/PN3: a bot's owner ID is not evidence that its owner wrote the message.
#[tokio::test]
async fn pn3_own_bot_mention_reaches_phone_but_human_self_mention_does_not() {
    let s = state();
    let phone_session = person(&s, "one");
    // Another browser sign-in must not divert notifications from the phone's live session.
    let browser_session = s.start_session("one").unwrap();
    let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(vec![])));
    assert!(s.push.set(capture.clone()).is_ok());
    s.db.put_push_device("one", &hash_secret(&phone_session), &device("phone", "one")).unwrap();
    s.db.create_hangout("room", "Room", "test-invite-code", "one", now()).unwrap();
    s.db.add_member("room", "one", now()).unwrap();
    s.db.create_agent("bot", "one", "Bot", &hash_secret("test-bot-token"), now()).unwrap();
    s.db.add_agent_to_room("room", "bot").unwrap();
    let bot = s.authenticate(&hash_secret("test-bot-token")).unwrap().unwrap();
    let human = s.authenticate(&hash_secret(&browser_session)).unwrap().unwrap();

    agenthangout::ops::post_message(&s, &human, "room", "chat", "@one talking to myself").unwrap();
    settle(&s).await;
    assert!(capture.0.lock().unwrap().is_empty(), "human self-mentions stay quiet");

    agenthangout::ops::post_message(&s, &bot, "room", "chat", "@one private test text").unwrap();
    settle(&s).await;
    let sent = capture.0.lock().unwrap();
    assert_eq!(sent.len(), 1, "a person's own bot must reach their phone");
    assert_eq!(sent[0].category, "mention");
    assert_eq!(sent[0].owner_id, "one");
    assert_eq!(sent[0].path, "/h/room");
    assert!(!sent[0].body.contains("private test text"));
}

// PN3: exercise the real question operation, rather than just a constructed notice.
#[tokio::test]
async fn pn3_owner_question_operation_reaches_phone_without_webhook() {
    let s = state();
    let session = person(&s, "one");
    let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(vec![])));
    assert!(s.push.set(capture.clone()).is_ok());
    s.db.put_push_device("one", &hash_secret(&session), &device("phone", "one")).unwrap();
    s.db.create_hangout("room", "Room", "test-invite-code", "one", now()).unwrap();
    s.db.add_member("room", "one", now()).unwrap();
    s.db.create_agent("bot", "one", "Bot", &hash_secret("test-bot-token"), now()).unwrap();
    s.db.add_agent_to_room("room", "bot").unwrap();
    let caller = s.authenticate(&hash_secret("test-bot-token")).unwrap().unwrap();
    agenthangout::ops::ask_owner(
        &s,
        &caller,
        "room",
        "Private question must not appear on lockscreen",
    )
    .unwrap();
    settle(&s).await;
    let sent = capture.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].category, "request");
    assert_eq!(sent[0].owner_id, "one");
    assert_eq!(sent[0].path, "/h/room");
    assert!(!sent[0].body.contains("Private question"));
}
