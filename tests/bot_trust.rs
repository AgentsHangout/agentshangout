//! Numbered permission and consent rules in docs/bot-trust.md.
mod support;

use agenthangout::{
    app, auth,
    bot_trust::{self, Permission, Trust},
    db::{Db, Message, Wearer},
    now, ops, AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

struct Fixture {
    state: AppState,
    base: String,
    client: Client,
}
impl Fixture {
    async fn new() -> Self {
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: "http://test".into(),
                create_code: Some("qa".into()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        for name in ["alice", "bob", "eve"] {
            state
                .db
                .create_person(name, name, &auth::hash_secret(name), &format!("feed-{name}"), now())
                .unwrap();
        }
        state.db.create_hangout("shared", "Shared", "shared-code", "alice", now()).unwrap();
        state.db.add_member("shared", "bob", now()).unwrap();
        state.db.create_hangout("secret", "Secret", "secret-code", "eve", now()).unwrap();
        for (id, owner) in [("a", "alice"), ("b", "bob"), ("e", "eve")] {
            state.db.create_agent(id, owner, id, &auth::hash_secret(id), now()).unwrap();
        }
        state.db.add_agent_to_room("shared", "a").unwrap();
        state.db.add_agent_to_room("shared", "b").unwrap();
        state.db.add_agent_to_room("secret", "e").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let served = state.clone();
        tokio::spawn(async move {
            axum::serve(listener, app(served)).await.unwrap();
        });
        Self { state, base, client: Client::new() }
    }
    async fn call(
        &self,
        key: &str,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.json().await.unwrap();
        (status, body)
    }
    fn message(&self, person: Option<&str>, bot: Option<&str>) -> Message {
        Message {
            id: 1,
            hangout_id: "shared".into(),
            person_id: person.map(str::to_owned),
            agent_id: bot.map(str::to_owned),
            speaker: "Speaker".into(),
            kind: "chat".into(),
            body: "@a".into(),
            created_at: now(),
            reply_to: None,
            thread_root: None,
        }
    }
    fn config(&self, mode: &str) {
        let p = Permission { mode: mode.into(), people: vec![], bots: vec![] };
        self.state
            .db
            .save_bot_trust(
                "a",
                &Trust { wake: p.clone(), invite_public: p.clone(), invite_private: p },
            )
            .unwrap();
    }
}

// BT1: only the recipient's person consents, for people and bots; crossed requests do not accept.
#[tokio::test]
async fn explicit_friendship_requires_recipient_consent_and_can_be_revoked() {
    let f = Fixture::new().await;
    for (kind, from, to, sender, recipient) in
        [("person", "alice", "bob", "alice", "bob"), ("bot", "a", "b", "alice", "bob")]
    {
        let body = json!({"kind":kind,"from_id":from,"to_id":to});
        assert_eq!(
            f.call(sender, Method::POST, "/api/trust/friendships", body.clone()).await.0,
            StatusCode::OK
        );
        assert!(!f.state.db.explicit_friends(kind, from, to).unwrap());
        assert_eq!(
            f.call(sender, Method::POST, "/api/trust/friendships/accept", body.clone()).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.call("b", Method::POST, "/api/trust/friendships/accept", body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.call(
                recipient,
                Method::POST,
                "/api/trust/friendships",
                json!({"kind":kind,"from_id":to,"to_id":from})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(!f.state.db.explicit_friends(kind, from, to).unwrap());
        assert_eq!(
            f.call(recipient, Method::POST, "/api/trust/friendships/accept", body.clone()).await.0,
            StatusCode::OK
        );
        assert!(f.state.db.explicit_friends(kind, to, from).unwrap());
        assert_eq!(
            f.call(sender, Method::DELETE, "/api/trust/friendships", body).await.0,
            StatusCode::OK
        );
        assert!(!f.state.db.explicit_friends(kind, from, to).unwrap());
    }
}

// BT2, BT3: only the owner can inspect/write configuration; strangers and private metadata stay out.
#[tokio::test]
async fn settings_and_graph_are_owner_scoped() {
    let f = Fixture::new().await;
    assert_eq!(
        f.call("bob", Method::GET, "/api/agents/a/trust", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.call("a", Method::GET, "/api/trust", json!({})).await.0, StatusCode::FORBIDDEN);
    let (_, view) = f.call("alice", Method::GET, "/api/trust", json!({})).await;
    assert!(view["bots"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b.get("activity").is_none() && b.get("doorbell").is_none() && b["id"] != "e"));
    assert!(view["people"].as_array().unwrap().iter().all(|p| p["id"] != "eve"));
    assert_eq!(
        f.call(
            "alice",
            Method::POST,
            "/api/trust/friendships",
            json!({"kind":"person","from_id":"alice","to_id":"eve"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, default) = f.call("alice", Method::GET, "/api/agents/a/trust", json!({})).await;
    assert_eq!(default["trust"]["wake"]["mode"], "participants");
    assert_eq!(
        f.call("bob", Method::PUT, "/api/agents/a/trust", default["trust"].clone()).await.0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = default["trust"].clone();
    invalid["wake"]["mode"] = json!("everyone");
    assert_eq!(
        f.call("alice", Method::PUT, "/api/agents/a/trust", invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(!f.state.db.is_member("secret", "alice").unwrap());
}

// BT2, BT4: defaults, owner override, selected bot identity, friends and immediate revocation.
#[tokio::test]
async fn wake_gates_distinguish_people_bots_and_accepted_friends() {
    let f = Fixture::new().await;
    let bot = f.state.db.agent("a", now()).unwrap().unwrap();
    assert!(bot_trust::may_wake(&f.state, &bot, &f.message(Some("bob"), None)).unwrap());
    f.config("owner");
    assert!(!bot_trust::may_wake(&f.state, &bot, &f.message(Some("bob"), None)).unwrap());
    assert!(bot_trust::may_wake(&f.state, &bot, &f.message(Some("alice"), None)).unwrap());
    f.config("selected");
    let mut trust = f.state.db.bot_trust("a").unwrap();
    trust.wake.bots.push("b".into());
    f.state.db.save_bot_trust("a", &trust).unwrap();
    assert!(bot_trust::may_wake(&f.state, &bot, &f.message(None, Some("b"))).unwrap());
    assert!(!bot_trust::may_wake(&f.state, &bot, &f.message(Some("bob"), None)).unwrap());
    f.config("friends");
    f.state.db.request_explicit_friend("person", "alice", "bob", now()).unwrap();
    assert!(!bot_trust::may_wake(&f.state, &bot, &f.message(Some("bob"), None)).unwrap());
    f.state.db.accept_explicit_friend("person", "alice", "bob").unwrap();
    assert!(bot_trust::may_wake(&f.state, &bot, &f.message(None, Some("b"))).unwrap());
    f.state.db.remove_explicit_friend("person", "alice", "bob").unwrap();
    assert!(!bot_trust::may_wake(&f.state, &bot, &f.message(None, Some("b"))).unwrap());
}

// BT4, BT5: delegated private invitations require consent and existing owner access; pauses win.
#[tokio::test]
async fn delegated_bring_does_not_expand_private_visibility() {
    let f = Fixture::new().await;
    let bob = f.state.authenticate(&auth::hash_secret("bob")).unwrap().unwrap();
    assert!(ops::bring_agent(&f.state, &bob, "shared", "a").is_err());
    let mut trust = Trust::default();
    trust.invite_private.mode = "selected".into();
    trust.invite_private.people.push("bob".into());
    f.state.db.save_bot_trust("a", &trust).unwrap();
    ops::bring_agent(&f.state, &bob, "shared", "a").unwrap();
    f.state.db.set_doorbell_paused("a", true).unwrap();
    assert!(ops::bring_agent(&f.state, &bob, "shared", "a").is_err());
    f.state.db.set_doorbell_paused("a", false).unwrap();
    f.state.db.add_member("secret", "bob", now()).unwrap();
    assert!(ops::bring_agent(&f.state, &bob, "secret", "a").is_err());
    assert!(!f.state.db.is_member("secret", "alice").unwrap());
}

// BT4: permission is checked again when queued or retried wakes leave, and by polling doorbells.
#[tokio::test]
async fn queued_delivery_and_polling_honor_revocation_pause_and_only_me() {
    let f = Fixture::new().await;
    let bob = f.state.authenticate(&auth::hash_secret("bob")).unwrap().unwrap();
    let alice = f.state.authenticate(&auth::hash_secret("alice")).unwrap().unwrap();
    let m = ops::post_message(&f.state, &bob, "shared", "chat", "@a hello").unwrap();
    let owner = ops::post_message(&f.state, &alice, "shared", "chat", "@a hello").unwrap();
    assert!(bot_trust::delivery_allowed(&f.state, "a", &[m.id]).unwrap());
    f.config("owner");
    assert!(!bot_trust::delivery_allowed(&f.state, "a", &[m.id]).unwrap());
    let (_, checked) = f
        .call(
            "a",
            Method::POST,
            "/api/agents/a/trust/wake-check",
            json!({"message_ids":[m.id,owner.id]}),
        )
        .await;
    assert_eq!(checked["allowed"], json!([owner.id]));
    assert_eq!(
        f.call("b", Method::POST, "/api/agents/a/trust/wake-check", json!({"message_ids":[m.id]}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    f.config("participants");
    f.state.db.set_wakes_only_for_owner("a", true).unwrap();
    assert!(!bot_trust::delivery_allowed(&f.state, "a", &[m.id]).unwrap());
    assert!(bot_trust::delivery_allowed(&f.state, "a", &[owner.id]).unwrap());
    f.state.db.set_doorbell_paused("a", true).unwrap();
    assert!(!bot_trust::delivery_allowed(&f.state, "a", &[owner.id]).unwrap());
    f.state.db.set_doorbell_paused("a", false).unwrap();
    f.state.db.remove_agent_from_room("shared", "a").unwrap();
    assert!(!bot_trust::delivery_allowed(&f.state, "a", &[owner.id]).unwrap());
}

// BT4: revoking the saved destination or key cancels already queued requests.
#[tokio::test]
async fn queued_wake_cannot_use_a_replaced_destination_or_key() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let f = Fixture::new().await;
    let bob = f.state.authenticate(&auth::hash_secret("bob")).unwrap().unwrap();
    let m = ops::post_message(&f.state, &bob, "shared", "chat", "@a hello").unwrap();
    assert!(bot_trust::delivery_allowed(&f.state, "a", &[m.id]).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/callback", listener.local_addr().unwrap());
    let router = axum::Router::new().route(
        "/callback",
        axum::routing::post(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    for change in ["clear", "replace", "key"] {
        f.state.db.set_notify_url("a", Some(&url)).unwrap();
        f.state.db.set_notify_auth("a", Some("old-key")).unwrap();
        let (done, received) = tokio::sync::oneshot::channel();
        agenthangout::notify::send_result(
            &f.state,
            "a",
            &agenthangout::notify::Notice {
                event: "mention",
                hangout_id: "shared".into(),
                text: "mention".into(),
            },
            true,
            vec![m.id],
            move |_, result| {
                let _ = done.send(result.accepted());
            },
        );
        match change {
            "clear" => f.state.db.set_notify_url("a", None).unwrap(),
            "replace" => f.state.db.set_notify_url("a", Some("http://127.0.0.1:9/new")).unwrap(),
            _ => f.state.db.set_notify_auth("a", Some("new-key")).unwrap(),
        }
        assert!(
            !tokio::time::timeout(std::time::Duration::from_secs(2), received)
                .await
                .unwrap()
                .unwrap(),
            "{change}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{change}");
    }
}

#[tokio::test]
async fn wake_retry_stops_when_the_saved_key_changes_after_first_attempt() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let f = Fixture::new().await;
    let bob = f.state.authenticate(&auth::hash_secret("bob")).unwrap().unwrap();
    let m = ops::post_message(&f.state, &bob, "shared", "chat", "@a hello").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let callback_state = f.state.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/callback", listener.local_addr().unwrap());
    let router = axum::Router::new().route(
        "/callback",
        axum::routing::post(move || {
            let counter = counter.clone();
            let state = callback_state.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                state.db.set_notify_auth("a", Some("replacement")).unwrap();
                StatusCode::SERVICE_UNAVAILABLE
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    f.state.db.set_notify_url("a", Some(&url)).unwrap();
    f.state.db.set_notify_auth("a", Some("old-key")).unwrap();
    let (done, received) = tokio::sync::oneshot::channel();
    agenthangout::notify::send_result(
        &f.state,
        "a",
        &agenthangout::notify::Notice {
            event: "mention",
            hangout_id: "shared".into(),
            text: "mention".into(),
        },
        false,
        vec![m.id],
        move |_, result| {
            let _ = done.send(result.accepted());
        },
    );
    assert!(!tokio::time::timeout(std::time::Duration::from_secs(4), received)
        .await
        .unwrap()
        .unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// BT2, BT4: public permission is independent from private permission and cannot bypass only-me.
#[tokio::test]
async fn public_and_private_delegation_are_independent() {
    let f = Fixture::new().await;
    f.state
        .db
        .change_floor(&agenthangout::db::FloorChange {
            id: "shared",
            name: None,
            access: Some("public"),
            size: None,
            paint: None,
            flooring: None,
            keep_bots_of: "alice",
            private_only: &[],
        })
        .unwrap();
    let bob = f.state.authenticate(&auth::hash_secret("bob")).unwrap().unwrap();
    let mut trust = Trust::default();
    trust.invite_public.mode = "selected".into();
    trust.invite_public.people.push("bob".into());
    f.state.db.save_bot_trust("a", &trust).unwrap();
    ops::bring_agent(&f.state, &bob, "shared", "a").unwrap();
    f.state.db.create_hangout("private2", "Private", "private2-code", "alice", now()).unwrap();
    f.state.db.add_member("private2", "bob", now()).unwrap();
    assert!(ops::bring_agent(&f.state, &bob, "private2", "a").is_err());
    f.state.db.set_wakes_only_for_owner("a", true).unwrap();
    assert!(ops::bring_agent(&f.state, &bob, "shared", "a").is_err());
}

// BT4: project invitations use the target's private gate before granting the owner's membership.
#[tokio::test]
async fn project_invites_enforce_bot_selection_before_membership_changes() {
    let f = Fixture::new().await;
    let caller = f.state.authenticate(&auth::hash_secret("b")).unwrap().unwrap();
    let project = agenthangout::projects::start_project(&f.state, &caller, "Work", None).unwrap();
    let id = project["project_id"].as_str().unwrap();
    f.state.db.set_bot_place("b", "shared", now()).unwrap();
    f.state.db.set_bot_place("a", "shared", now()).unwrap();
    f.config("owner");
    assert!(agenthangout::projects::invite_to_project(&f.state, &caller, id, "a").is_err());
    assert!(!f.state.db.is_member(id, "alice").unwrap());
    let mut trust = f.state.db.bot_trust("a").unwrap();
    trust.invite_private.mode = "selected".into();
    trust.invite_private.bots.push("b".into());
    f.state.db.save_bot_trust("a", &trust).unwrap();
    agenthangout::projects::invite_to_project(&f.state, &caller, id, "a").unwrap();
    assert!(f.state.db.is_member(id, "alice").unwrap());
}

// BT1: deleting a bot removes both directions of its consent records.
#[tokio::test]
async fn deleting_a_bot_cleans_explicit_friendships() {
    let f = Fixture::new().await;
    f.state.db.request_explicit_friend("bot", "a", "b", now()).unwrap();
    f.state.db.accept_explicit_friend("bot", "a", "b").unwrap();
    f.state.db.delete_agent("b").unwrap();
    assert!(!f.state.db.explicit_friends("bot", "a", "b").unwrap());
    assert!(f.state.db.explicit_friendships("alice").unwrap().is_empty());
}

// BT2, BT5: a bot delegate needs explicit permission and actual destination membership.
#[tokio::test]
async fn bot_delegates_can_bring_only_with_current_permission() {
    let f = Fixture::new().await;
    let path = "/api/hangouts/shared/agents/a";
    assert_eq!(f.call("b", Method::PUT, path, json!({})).await.0, StatusCode::NOT_FOUND);
    let mut trust = Trust::default();
    trust.invite_private.mode = "selected".into();
    trust.invite_private.bots.push("b".into());
    assert_eq!(
        f.call("alice", Method::PUT, "/api/agents/a/trust", serde_json::to_value(&trust).unwrap())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(f.call("b", Method::PUT, path, json!({})).await.0, StatusCode::OK);
    trust.invite_private.bots.clear();
    assert_eq!(
        f.call("alice", Method::PUT, "/api/agents/a/trust", serde_json::to_value(&trust).unwrap())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(f.call("b", Method::PUT, path, json!({})).await.0, StatusCode::NOT_FOUND);
}

// BT5: a bot that's only visiting a room, here by walking onto a public floor, can't bring a bot in,
// even one whose owner chose it.
#[tokio::test]
async fn a_visiting_bot_delegate_cannot_bring_a_bot() {
    let f = Fixture::new().await;
    let made =
        |name: &str| json!({"name":format!("{name} room"),"your_name":name,"create_code":"qa"});
    let (_, ann) = f.call("", Method::POST, "/api/hangouts", made("Ann")).await;
    let (key, room) = (ann["owner_key"].as_str().unwrap(), ann["hangout"]["id"].as_str().unwrap());
    let (_, other) = f.call(key, Method::POST, "/api/hangouts", made("Ann's other")).await;
    let (pal, _) = support::legacy_bot(
        &f.state,
        ann["person"]["id"].as_str().unwrap(),
        other["hangout"]["id"].as_str().unwrap(),
        "Pal",
    );
    let (_, bob) = f.call("", Method::POST, "/api/hangouts", made("Bob")).await;
    let bob_key = bob["owner_key"].as_str().unwrap();
    let (rover, rover_token) = support::legacy_bot(
        &f.state,
        bob["person"]["id"].as_str().unwrap(),
        bob["hangout"]["id"].as_str().unwrap(),
        "Rover",
    );
    let public = json!({"access":"public","create_code":"qa"});
    assert_eq!(
        f.call(key, Method::PUT, &format!("/api/floors/{room}"), public).await.0,
        StatusCode::OK
    );
    let walked = f
        .call(
            bob_key,
            Method::POST,
            &format!("/api/floors/{room}/walk-in"),
            json!({"agent_id":rover.id}),
        )
        .await;
    assert_eq!(walked.0, StatusCode::OK, "{}", walked.1);
    assert!(f.state.db.agent_in_room(room, &rover.id, now()).unwrap(), "Rover is visiting");
    let mut trust = Trust::default();
    trust.invite_public.mode = "selected".into();
    trust.invite_public.bots.push(rover.id.clone());
    let set = f
        .call(
            key,
            Method::PUT,
            &format!("/api/agents/{}/trust", pal.id),
            serde_json::to_value(&trust).unwrap(),
        )
        .await;
    assert_eq!(set.0, StatusCode::OK, "{}", set.1);

    let brought = f
        .call(
            &rover_token,
            Method::PUT,
            &format!("/api/hangouts/{room}/agents/{}", pal.id),
            json!({}),
        )
        .await;
    assert_ne!(brought.0, StatusCode::OK, "{}", brought.1);
    assert!(!f.state.db.agent_belongs_to_room(room, &pal.id).unwrap());
}

// E41: monitoring names only rooms the owner may see; a private room someone else brought the bot
// into is only counted.
#[tokio::test]
async fn monitoring_never_names_a_private_room_its_owner_isnt_in() {
    let f = Fixture::new().await;
    f.state.db.add_agent_to_room("secret", "a").unwrap();
    let (status, body) =
        f.call("alice", Method::GET, "/api/agents/a/monitoring", json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names: Vec<_> =
        body["eligible_rooms"].as_array().unwrap().iter().map(|r| r["name"].clone()).collect();
    assert_eq!(names, [json!("Shared")], "{body}");
    assert_eq!(body["other_rooms"], 1, "{body}");
    assert!(!body.to_string().contains("Secret") && !body.to_string().contains("secret"), "{body}");
}

// BT6: the view carries what each person and bot in it wears, chosen or not, and nobody outside it.
#[tokio::test]
async fn the_trust_view_says_what_its_people_and_bots_wear() {
    let f = Fixture::new().await;
    f.state.db.change_look(Wearer::Bot, "a", &[("head", Some("wide"))]).unwrap();
    let (status, view) = f.call("alice", Method::GET, "/api/trust", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["looks"]["bots"]["a"]["head"], "wide", "{view}");
    assert!(view["looks"]["people"]["alice"].is_object(), "{view}");
    let bots = view["looks"]["bots"].as_object().unwrap();
    assert!(!bots.contains_key("e"), "{view}");
    assert!(!view["looks"]["people"].as_object().unwrap().contains_key("eve"), "{view}");
    for id in bots.keys() {
        assert!(view["bots"].as_array().unwrap().iter().any(|b| &b["id"] == id), "{id}");
    }
}
