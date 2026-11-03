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
            (
                StatusCode::OK,
                Json(json!([
                    {"number":9,"title":"Research idea","state":v.lock().unwrap()["issue9"].as_str().unwrap_or("open"),"labels":[{"name":"bug","color":"d73a4a"},{"name":"Map"}]},
                    {"number":12,"title":"Fix the plots","state":"open","pull_request":{}}
                ])),
            )
        }
    }
    async fn pr(State(v): State<Arc<Mutex<Value>>>) -> Json<Value> {
        Json(v.lock().unwrap().clone())
    }
    // RW9: pull request 12's head, check runs and reviews, or a failure when pr_fail is set.
    async fn pr12(State(v): State<Arc<Mutex<Value>>>) -> (StatusCode, Json<Value>) {
        if v.lock().unwrap()["pr_fail"] == true {
            return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
        }
        (StatusCode::OK, Json(json!({"number":12,"head":{"sha":"a".repeat(40)}})))
    }
    async fn checks(State(v): State<Arc<Mutex<Value>>>) -> Json<Value> {
        let runs = v.lock().unwrap()["checks"].clone();
        let runs = if runs.is_null() { json!([]) } else { runs };
        let total = runs.as_array().unwrap().len();
        Json(json!({"check_runs": runs, "total_count": total}))
    }
    async fn reviews(State(v): State<Arc<Mutex<Value>>>) -> Json<Value> {
        let reviews = v.lock().unwrap()["reviews"].clone();
        Json(if reviews.is_null() { json!([]) } else { reviews })
    }
    let router = Router::new()
        .route("/repos/org/repo/pulls/12", get(pr12))
        .route("/repos/org/repo/commits/{sha}/check-runs", get(checks))
        .route("/repos/org/repo/pulls/12/reviews", get(reviews))
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
#[tokio::test]
async fn rw8_claims_are_local_lead_or_pair_and_run_out() {
    let (state, owner, bot, _) = setup().await;
    // The workboard has to be read once for item 9 to be claimable.
    assert!(research_work::claim(&state, &bot, "lab", 9, false).is_err());
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["claims"], json!([]));
    assert!(research_work::claim(&state, &bot, "lab", 404, false).is_err(), "not on the board");

    // The bot leads; its owner can't lead too, but can pair.
    let led = research_work::claim(&state, &bot, "lab", 9, false).unwrap();
    assert_eq!(led["pairing"], false);
    let refused = research_work::claim(&state, &owner, "lab", 9, false).unwrap_err();
    assert_eq!(refused.status, StatusCode::CONFLICT);
    research_work::claim(&state, &owner, "lab", 9, true).unwrap();
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    let claims = read["items"][0]["claims"].as_array().unwrap().clone();
    assert_eq!(claims.len(), 2, "{claims:?}");
    assert_eq!(
        (claims[0]["name"].clone(), claims[0]["is_bot"].clone()),
        (json!("Bot"), json!(true))
    );
    assert_eq!(claims[1]["pairing"], true);
    // Renewing keeps when it started.
    let since = claims[0]["since"].clone();
    research_work::claim(&state, &bot, "lab", 9, false).unwrap();
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["claims"][0]["since"], since);

    // Unclaiming drops only your own, and a missing claim is not found.
    research_work::unclaim(&state, &owner, "lab", 9).unwrap();
    assert_eq!(
        research_work::unclaim(&state, &owner, "lab", 9).unwrap_err().status,
        StatusCode::NOT_FOUND
    );
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["claims"].as_array().unwrap().len(), 1);

    // Four hours on, the bot's claim has run out and the owner can lead.
    state.db.age_research_work_claims("lab", research_work::CLAIM_SECS + 1).unwrap();
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["claims"], json!([]));
    research_work::claim(&state, &owner, "lab", 9, false).unwrap();

    // A house bot, and someone outside the room, can't claim.
    let outsider = auth::Caller {
        person_id: "stranger".into(),
        agent_id: None,
        name: "Stranger".into(),
        secret_hash: String::new(),
        is_house: false,
    };
    assert!(research_work::claim(&state, &outsider, "lab", 9, true).is_err());
    let house = auth::Caller { is_house: true, ..bot.clone() };
    assert!(research_work::claim(&state, &house, "lab", 9, true).is_err());
}
#[test]
fn rw9_ci_and_review_are_read_conservatively() {
    let run = |status: &str, conclusion: Value| json!({"status":status,"conclusion":conclusion});
    let ci = |runs: Value| {
        let total = runs.as_array().unwrap().len();
        research_work::ci_of(&json!({ "check_runs": runs, "total_count": total }))
    };
    assert_eq!(ci(json!([])), Some("none"));
    assert_eq!(
        ci(json!([run("completed", json!("success")), run("completed", json!("skipped"))])),
        Some("passing")
    );
    assert_eq!(
        ci(json!([run("completed", json!("success")), run("in_progress", Value::Null)])),
        Some("pending")
    );
    assert_eq!(
        ci(json!([run("in_progress", Value::Null), run("completed", json!("failure"))])),
        Some("failing")
    );
    assert_eq!(ci(json!([run("completed", json!("cancelled"))])), Some("failing"));
    assert_eq!(research_work::ci_of(&json!({"message":"rate limited"})), None);
    // More runs than one page holds can't be read as passing: the missing one could have failed.
    let passed = run("completed", json!("success"));
    assert_eq!(research_work::ci_of(&json!({"check_runs":[passed.clone()],"total_count":2})), None);
    let page = Value::Array(vec![passed.clone(); 100]);
    assert_eq!(research_work::ci_of(&json!({"check_runs":page,"total_count":100})), None);
    assert_eq!(research_work::ci_of(&json!({"check_runs":[passed]})), None, "no total");
    let review = |who: u64, state: &str| json!({"user":{"id":who},"state":state});
    let of = |reviews: Value| research_work::review_of(&reviews);
    assert_eq!(of(json!([review(1, "COMMENTED")])), Some("none"));
    assert_eq!(of(json!([review(1, "APPROVED"), review(2, "COMMENTED")])), Some("approved"));
    assert_eq!(
        of(json!([review(1, "APPROVED"), review(2, "CHANGES_REQUESTED")])),
        Some("changes_requested")
    );
    // A reviewer's latest decision counts, and a dismissal clears theirs.
    assert_eq!(
        of(json!([review(2, "CHANGES_REQUESTED"), review(2, "APPROVED")])),
        Some("approved")
    );
    assert_eq!(of(json!([review(2, "CHANGES_REQUESTED"), review(2, "DISMISSED")])), Some("none"));
    assert_eq!(of(json!({"message":"not found"})), None);
    // A full page of reviews might hide a later change request.
    assert_eq!(of(Value::Array(vec![review(1, "APPROVED"); 100])), None);
}
#[tokio::test]
async fn rw9_pr_state_is_read_on_request_every_five_minutes_and_never_falsely_green() {
    let (state, owner, _bot, upstream) = setup().await;
    // Not before the board is read, and not for an issue.
    assert!(research_work::pr_status(&state, &owner, "lab", 12).await.is_err());
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][1]["number"], 12);
    assert!(read["items"][1]["pr_state"].is_null(), "reading the board never asks about a PR");
    assert!(read["items"][0].get("pr_state").is_none(), "issues have no PR state");
    assert!(research_work::pr_status(&state, &owner, "lab", 9).await.is_err());

    // GitHub is down on the first read: unknown and stale, not passing.
    upstream.lock().unwrap()["pr_fail"] = json!(true);
    let down = research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    assert_eq!(
        (down["ci"].clone(), down["review"].clone(), down["stale"].clone()),
        (json!("unknown"), json!("unknown"), json!(true))
    );

    // Five minutes on, it reads passing and approved.
    upstream.lock().unwrap()["pr_fail"] = json!(false);
    upstream.lock().unwrap()["checks"] = json!([{"status":"completed","conclusion":"success"}]);
    upstream.lock().unwrap()["reviews"] = json!([{"user":{"id":5},"state":"APPROVED"}]);
    state.db.age_research_work_pr_checks("org/repo", 301).unwrap();
    let up = research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    assert_eq!(
        (up["ci"].clone(), up["review"].clone(), up["stale"].clone()),
        (json!("passing"), json!("approved"), json!(false))
    );

    // Within five minutes it doesn't ask GitHub again, even though CI has started failing.
    upstream.lock().unwrap()["checks"] = json!([{"status":"completed","conclusion":"failure"}]);
    let cached = research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    assert_eq!(cached["ci"], "passing");
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][1]["pr_state"]["ci"], "passing");

    // Then a failed read keeps the last state but marks it stale.
    upstream.lock().unwrap()["pr_fail"] = json!(true);
    state.db.age_research_work_pr_checks("org/repo", 301).unwrap();
    let stale = research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    assert_eq!((stale["ci"].clone(), stale["stale"].clone()), (json!("passing"), json!(true)));

    // Someone outside the room can't read it.
    let outsider = auth::Caller {
        person_id: "stranger".into(),
        agent_id: None,
        name: "Stranger".into(),
        secret_hash: String::new(),
        is_house: false,
    };
    assert!(research_work::pr_status(&state, &outsider, "lab", 12).await.is_err());
}
#[test]
fn rw10_cards_keep_label_names_only() {
    let long = "x".repeat(80);
    let labels: Vec<Value> = (0..12).map(|n| json!({"name":format!("l{n}")})).collect();
    let values = json!([
        {"number":1,"title":"a","state":"open","labels":[{"name":"bug","color":"d73a4a","description":"secret"},{"name":""},{"name":long}]},
        {"number":2,"title":"b","state":"open","labels":labels},
        {"number":3,"title":"c","state":"open"}
    ]);
    let c = research_work::cards("org/repo", &values).unwrap();
    assert_eq!(c[0].labels, ["bug".to_string(), "x".repeat(50)]);
    assert_eq!(c[1].labels.len(), 10);
    assert!(c[2].labels.is_empty());
    let serialized = serde_json::to_string(&c).unwrap();
    assert!(!serialized.contains("d73a4a") && !serialized.contains("secret"));
    // A snapshot stored before labels were read still loads, with none.
    let old: research_work::Card = serde_json::from_value(
        json!({"number":5,"title":"t","kind":"issue","state":"open","source":"s"}),
    )
    .unwrap();
    assert!(old.labels.is_empty());
}
#[tokio::test]
async fn rw10_the_owner_names_areas_for_labels() {
    let (state, owner, bot, _) = setup().await;
    // Not alphabetical, so the order kept is the owner's, not the labels'.
    let areas = json!([
        {"label":"map","area":"City map"},
        {"label":"docs","area":"Writing"},
        {"label":"BUG","area":"Bugs"}
    ]);
    // The owner's bot names them; labels match in any case.
    let set = research_work::set_areas(&state, &bot, "lab", &areas).unwrap();
    assert_eq!(set["areas"], areas);
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["areas"], areas);
    assert_eq!(read["items"][0]["number"], 9);
    assert_eq!(read["items"][0]["labels"], json!(["bug", "Map"]));
    assert_eq!(read["items"][0]["areas"], json!(["City map", "Bugs"]));
    // Two labels can share an area, which is listed once.
    let shared = json!([{"label":"Map","area":"Fixes"},{"label":"bug","area":"Fixes"}]);
    research_work::set_areas(&state, &owner, "lab", &shared).unwrap();
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["areas"], json!(["Fixes"]));
    research_work::set_areas(&state, &owner, "lab", &areas).unwrap();
    assert_eq!(read["items"][1]["areas"], json!([]));

    // Bad maps are refused and change nothing.
    let pair = |label: &str, area: Value| json!({ "label": label, "area": area });
    for bad in [
        json!({"bug":"Bugs"}),
        json!(["bug"]),
        json!([pair("bug", json!(""))]),
        json!([pair("bug", json!(7))]),
        json!([{"label":"bug"}]),
        json!([pair("", json!("Bugs"))]),
        json!([pair("bug", json!("x".repeat(41)))]),
        json!([pair("bug", json!("Bugs")), pair("Bug", json!("Other"))]),
        Value::Array((0..31).map(|n| pair(&format!("l{n}"), json!("a"))).collect()),
    ] {
        let error = research_work::set_areas(&state, &owner, "lab", &bad).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["areas"], json!(["City map", "Bugs"]));

    // Only the owner and their bots; not a house bot or someone outside the room.
    let outsider = auth::Caller {
        person_id: "stranger".into(),
        agent_id: None,
        name: "Stranger".into(),
        secret_hash: String::new(),
        is_house: false,
    };
    assert!(research_work::set_areas(&state, &outsider, "lab", &areas).is_err());
    let house = auth::Caller { is_house: true, ..bot.clone() };
    assert!(research_work::set_areas(&state, &house, "lab", &areas).is_err());

    // An empty list clears them.
    research_work::set_areas(&state, &owner, "lab", &json!([])).unwrap();
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["areas"], json!([]));
    assert_eq!(read["items"][0]["areas"], json!([]));
}
#[tokio::test]
async fn rw11_watchers_hear_when_an_item_changes_and_nobody_is_rung() {
    let (state, owner, bot, upstream) = setup().await;
    // Not before the board is read, nor for a number that isn't on it.
    assert!(research_work::watch(&state, &bot, "lab", 9).is_err());
    research_work::list(&state, &owner, "lab").await.unwrap();
    assert!(research_work::watch(&state, &bot, "lab", 404).is_err());

    // The bot watches the issue and the pull request; its owner watches the pull request.
    research_work::watch(&state, &bot, "lab", 9).unwrap();
    research_work::watch(&state, &bot, "lab", 12).unwrap();
    research_work::watch(&state, &bot, "lab", 12).unwrap();
    research_work::watch(&state, &owner, "lab", 12).unwrap();
    let read = research_work::list(&state, &bot, "lab").await.unwrap();
    assert_eq!(read["items"][0]["you_watch"], true);
    let watchers = read["items"][1]["watchers"].as_array().unwrap().clone();
    assert_eq!(watchers.len(), 2, "watching again adds nobody: {watchers:?}");
    assert_eq!(
        (watchers[0]["name"].clone(), watchers[0]["is_bot"].clone()),
        (json!("Bot"), json!(true))
    );
    let read = research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(read["items"][0]["you_watch"], false);
    assert_eq!(read["items"][1]["you_watch"], true);

    // A first PR read, and a failed one, tell nobody anything.
    upstream.lock().unwrap()["checks"] = json!([{"status":"completed","conclusion":"success"}]);
    research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    upstream.lock().unwrap()["pr_fail"] = json!(true);
    state.db.age_research_work_pr_checks("org/repo", 301).unwrap();
    research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    assert!(state.db.unread_mentions("bot").unwrap().is_empty());

    // CI going from passing to failing is a line in the room, and a mention for each watcher.
    upstream.lock().unwrap()["pr_fail"] = json!(false);
    upstream.lock().unwrap()["checks"] = json!([{"status":"completed","conclusion":"failure"}]);
    state.db.age_research_work_pr_checks("org/repo", 301).unwrap();
    research_work::pr_status(&state, &owner, "lab", 12).await.unwrap();
    let mentions = state.db.unread_mentions("bot").unwrap();
    assert_eq!(mentions.len(), 1);
    let line = state.db.messages("lab", 0, 50).unwrap().into_iter().last().unwrap();
    assert_eq!(line.kind, "system");
    assert_eq!(line.body, "Pull request #12 has CI failing now, was passing.");
    assert_eq!(state.db.person_mentions("owner", 0).unwrap().len(), 1);

    // The issue closing on the next refresh is one line, a mention for the bot alone, with no
    // GitHub title and no watcher's name in it.
    upstream.lock().unwrap()["issue9"] = json!("closed");
    state.db.age_research_work_refresh("org/repo", 301).unwrap();
    research_work::list(&state, &owner, "lab").await.unwrap();
    let line = state.db.messages("lab", 0, 50).unwrap().into_iter().last().unwrap();
    assert_eq!(line.body, "Issue #9 is now closed.");
    assert!(!line.body.contains("Research idea"));
    assert_eq!(state.db.unread_mentions("bot").unwrap().len(), 2);
    assert_eq!(state.db.person_mentions("owner", 0).unwrap().len(), 1);

    // Nothing changed: no line.
    let before = state.db.messages("lab", 0, 50).unwrap().len();
    state.db.age_research_work_refresh("org/repo", 301).unwrap();
    research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(state.db.messages("lab", 0, 50).unwrap().len(), before);

    // Unwatching stops it, and a second unwatch isn't found.
    research_work::unwatch(&state, &bot, "lab", 9).unwrap();
    assert_eq!(
        research_work::unwatch(&state, &bot, "lab", 9).unwrap_err().status,
        StatusCode::NOT_FOUND
    );
    upstream.lock().unwrap()["issue9"] = json!("open");
    state.db.age_research_work_refresh("org/repo", 301).unwrap();
    research_work::list(&state, &owner, "lab").await.unwrap();
    assert_eq!(state.db.messages("lab", 0, 50).unwrap().len(), before);

    // A house bot, and someone outside the room, can't watch.
    let outsider = auth::Caller {
        person_id: "stranger".into(),
        agent_id: None,
        name: "Stranger".into(),
        secret_hash: String::new(),
        is_house: false,
    };
    assert!(research_work::watch(&state, &outsider, "lab", 9).is_err());
    let house = auth::Caller { is_house: true, ..bot.clone() };
    assert!(research_work::watch(&state, &house, "lab", 9).is_err());
}
