//! RW1-RW6: public GitHub is read-only; local attribution is explicit and verified.
use agenthangout::{
    auth, db::Db, now, research_labs::NewLab, research_work, sign_in::GitHubApp, AppState, Apps,
    Config,
};
use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
fn db() -> Db {
    let d = Db::open(":memory:").unwrap();
    d.create_person("owner", "Owner", &auth::hash_secret("owner-token"), "feed", now()).unwrap();
    d.create_agent("bot", "owner", "Bot", &auth::hash_secret("bot-token"), now()).unwrap();
    d
}
#[test]
fn rw1_rw2_cards_deduplicate_bound_and_keep_only_public_fields() {
    let values = json!([{"number":3,"title":"Fix","state":"open","body":"secret discussion","assignees":["private"],"pull_request":{}},{"number":3,"title":"Updated","state":"closed","pull_request":{}},{"number":4,"title":"Idea","state":"open"}]);
    let c = research_work::cards("org/repo", &values).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].title, "Updated");
    assert_eq!(c[0].source, "https://github.com/org/repo/pull/3");
    assert_eq!(c[1].kind, "issue");
    let serialized = serde_json::to_string(&c).unwrap();
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("assignees"));
    assert!(!serialized.contains("ci"));
    assert!(research_work::canonical_repo("a/../../localhost").is_err());
    assert!(research_work::canonical_repo("https://evil/x").is_err());
    let many =
        Value::Array((1..=150).map(|n| json!({"number":n,"title":"x","state":"open"})).collect());
    assert_eq!(research_work::cards("a/b", &many).unwrap().len(), 100);
}
#[test]
fn rw2_refresh_reservation_failure_and_empty_success() {
    let d = db();
    assert!(d.reserve_research_work_refresh("org/repo", 1000).unwrap());
    assert!(!d.reserve_research_work_refresh("org/repo", 1001).unwrap());
    let c = research_work::cards("org/repo", &json!([{"number":1,"title":"x","state":"open"}]))
        .unwrap();
    d.finish_research_work_refresh("org/repo", Some(&c), 1000).unwrap();
    assert!(d.reserve_research_work_refresh("org/repo", 1300).unwrap());
    d.finish_research_work_refresh("org/repo", None, 1300).unwrap();
    let s = d.research_work_snapshot("org/repo").unwrap().unwrap();
    assert!(s.stale);
    assert_eq!(s.fetched_at, 1000);
    assert_eq!(s.cards.len(), 1);
    assert!(d.reserve_research_work_refresh("org/repo", 1600).unwrap());
    d.finish_research_work_refresh("org/repo", Some(&[]), 1600).unwrap();
    let s = d.research_work_snapshot("org/repo").unwrap().unwrap();
    assert!(!s.stale);
    assert!(s.cards.is_empty());
}
async fn setup() -> (AppState, auth::Caller, auth::Caller, Arc<Mutex<Value>>) {
    let upstream = Arc::new(Mutex::new(
        json!({"number":7,"user":{"id":42},"body":"Agents Hangout bot: bot","merged":false}),
    ));
    async fn meta(State(v): State<Arc<Mutex<Value>>>) -> Json<Value> {
        Json(
            json!({"private":v.lock().unwrap()["private"].as_bool().unwrap_or(false),"full_name":"org/repo"}),
        )
    }
    async fn issues(State(v): State<Arc<Mutex<Value>>>) -> (StatusCode, Json<Value>) {
        if v.lock().unwrap()["fail"] == true {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})))
        } else {
            (StatusCode::OK, Json(json!([{"number":9,"title":"Research idea","state":"open"}])))
        }
    }
    async fn pr(State(v): State<Arc<Mutex<Value>>>) -> Json<Value> {
        Json(v.lock().unwrap().clone())
    }
    let router = Router::new()
        .route("/repos/org/repo", get(meta))
        .route("/repos/org/repo/issues", get(issues))
        .route("/repos/org/repo/pulls/7", get(pr))
        .with_state(upstream.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let d = db();
    d.set_sign_in_link("owner", "github", "42", "owner-login", now()).unwrap();
    d.create_research_lab(&NewLab {
        id: "lab",
        name: "Lab",
        purpose: "Research",
        invite_code: "invite",
        owner_id: "owner",
        agent_id: Some("bot"),
        github_owner: "org",
        github_repo: "repo",
        god_mode: true,
        at: now(),
    })
    .unwrap();
    let mut app = GitHubApp::from_settings(Some("client".into()), Some("secret".into())).unwrap();
    app.api_url = format!("http://{addr}");
    let state = AppState::with_apps(
        d,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
        Apps { github: Some(app), discord: None },
    );
    let owner = auth::Caller {
        person_id: "owner".into(),
        agent_id: None,
        name: "Owner".into(),
        secret_hash: auth::hash_secret("owner-token"),
        is_house: false,
    };
    let bot = auth::Caller {
        person_id: "owner".into(),
        agent_id: Some("bot".into()),
        name: "Bot".into(),
        secret_hash: auth::hash_secret("bot-token"),
        is_house: false,
    };
    (state, owner, bot, upstream)
}
#[tokio::test]
async fn rw4_rw6_only_verified_attributed_merged_contributions_earn_swag() {
    let (state, _owner, bot, upstream) = setup().await;
    let pending =
        research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
            .await
            .unwrap();
    assert_eq!(pending["verified_merged"], false);
    assert!(pending["award"].is_null());
    upstream.lock().unwrap()["merged"] = json!(true);
    let merged =
        research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
            .await
            .unwrap();
    assert_eq!(merged["award"], "research_contributor");
    assert_eq!(state.db.research_work_contributions("lab").unwrap().len(), 1);
    upstream.lock().unwrap()["user"]["id"] = json!(43);
    assert_eq!(
        research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
            .await
            .unwrap_err()
            .status,
        StatusCode::FORBIDDEN
    );
    upstream.lock().unwrap()["user"]["id"] = json!(42);
    upstream.lock().unwrap()["body"] = json!("unattributed");
    assert!(research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
        .await
        .is_err());
    assert!(state.db.research_work_contributions("other-lab").unwrap().is_empty());
}
#[tokio::test]
async fn rw5_owner_can_disable_default_participation_bot_cannot_reenable() {
    let (state, owner, bot, _) = setup().await;
    assert!(state.db.research_work_enabled("lab", "bot").unwrap());
    research_work::assign(&state, &owner, "lab", "bot", false).unwrap();
    assert!(!state.db.research_work_enabled("lab", "bot").unwrap());
    assert!(research_work::assign(&state, &bot, "lab", "bot", true).is_err());
    assert!(research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
        .await
        .is_err());
    research_work::assign(&state, &owner, "lab", "bot", true).unwrap();
    assert!(state.db.research_work_enabled("lab", "bot").unwrap());
    state.db.remove_agent_from_room("lab", "bot").unwrap();
    assert!(research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
        .await
        .is_err());
}
async fn serve(state: AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(state)).await.unwrap() });
    format!("http://{addr}")
}
#[tokio::test]
async fn rw1_rw2_http_workboard_refresh_failure_and_private_metadata() {
    let (state, _, _, upstream) = setup().await;
    let old = research_work::cards(
        "org/repo",
        &json!([{"number":5,"title":"Last verified card","state":"open"}]),
    )
    .unwrap();
    let at = now() - 600;
    state.db.reserve_research_work_refresh("org/repo", at).unwrap();
    state.db.finish_research_work_refresh("org/repo", Some(&old), at).unwrap();
    upstream.lock().unwrap()["fail"] = json!(true);
    let base = serve(state).await;
    let c = reqwest::Client::new();
    let r = c
        .get(format!("{base}/api/research-labs/lab/work"))
        .bearer_auth("owner-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["stale"], true);
    assert_eq!(v["items"][0]["title"], "Last verified card");
    assert_eq!(v["fetched_at"], at);
    let (state, _, _, upstream) = setup().await;
    upstream.lock().unwrap()["private"] = json!(true);
    let base = serve(state.clone()).await;
    let v: Value = c
        .get(format!("{base}/api/research-labs/lab/work"))
        .bearer_auth("owner-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["stale"], true);
    assert_eq!(v["items"], json!([]));
    assert!(state.db.research_work_snapshot("org/repo").unwrap().unwrap().cards.is_empty());
}
#[tokio::test]
async fn rw5_http_owner_preference_and_quiet_sign() {
    let (state, _, bot, _) = setup().await;
    let base = serve(state.clone()).await;
    let c = reqwest::Client::new();
    let route = format!("{base}/api/research-labs/lab/bots/bot");
    let work: Value = c
        .get(format!("{base}/api/research-labs/lab/work"))
        .bearer_auth("bot-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        work["your_participation"],
        json!({"enabled":true,"github_linked":true,"bot_name":"Bot","attribution_line":"Agents Hangout bot: bot"})
    );
    assert_eq!(work["bots"], json!([]));
    let r = c
        .put(&route)
        .bearer_auth("owner-token")
        .json(&json!({"enabled":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(!state.db.research_work_enabled("lab", "bot").unwrap());
    let r =
        c.put(&route).bearer_auth("bot-token").json(&json!({"enabled":true})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    c.put(&route).bearer_auth("owner-token").json(&json!({"enabled":true})).send().await.unwrap();
    state
        .db
        .place_item(
            &agenthangout::db::NewFloorItem {
                id: "quiet",
                hangout_id: "lab",
                kind: "quiet",
                x: 10,
                y: 10,
                facing: 0,
                placed_by: "owner",
                now: now(),
            },
            |_, _, _| Ok::<_, ()>(()),
        )
        .unwrap()
        .unwrap();
    let error =
        research_work::contribution(&state, &bot, "lab", "https://github.com/org/repo/pull/7")
            .await
            .unwrap_err();
    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(error.message.contains("quiet"));
}
