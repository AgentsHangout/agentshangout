//! BG1 to BG3 (#830): in-game bug auto-reports and deduplication.
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use agenthangout::auth::{self, Caller};
use agenthangout::bug_reports;
use agenthangout::db::Db;
use agenthangout::sign_in::GitHubApp;
use agenthangout::{AppState, Apps, Config};

/// The GitHub tests set a process-wide token, so they take turns.
static GH_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Reports now name a real person (a foreign key), so the tests create the people they report as.
fn seed_person(db: &Db, id: &str) {
    db.create_person(id, id, &auth::hash_secret(id), &format!("feed-{id}"), 1_800_000_000).unwrap();
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::test]
async fn bug_report_sanitizes_tokens_emails_and_paths() {
    let raw = "Error at C:\\Users\\takes\\secret\\file.rs: failed token ah_bot_secret12345 from user test@example.com";
    let sanitized = bug_reports::sanitize_error(raw);
    assert!(!sanitized.contains("ah_bot_secret12345"), "must strip token");
    assert!(!sanitized.contains("test@example.com"), "must strip email");
    assert!(!sanitized.contains("C:\\Users"), "must strip file path");
    assert!(sanitized.contains("[REDACTED_TOKEN]"));
    assert!(sanitized.contains("[REDACTED_EMAIL]"));
    assert!(sanitized.contains("[PATH]"));
}

#[tokio::test]
async fn bug_report_deduplicates_by_signature_and_bumps_count() {
    let db = Db::open(":memory:").unwrap();
    let state = AppState::new(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
            require_rules: false,
        },
    );
    seed_person(&state.db, "person-1");
    let caller = Caller {
        person_id: "person-1".into(),
        agent_id: None,
        name: "Person".into(),
        secret_hash: "secret".into(),
        is_house: false,
    };

    let t0 = now();
    // First report
    let rep1 = bug_reports::record_report(
        &state,
        &caller,
        "house",
        "load",
        "invalid",
        "cannot read properties of null (reading 'charAt')",
        Some("house-1"),
        t0,
    )
    .await
    .unwrap();

    assert!(rep1.is_new, "first occurrence must be marked as new");
    assert_eq!(rep1.report_count, 1);
    assert_eq!(rep1.surface, "house");
    assert_eq!(rep1.action, "load");

    // Second report with exact same error and action
    let t1 = t0 + 10;
    let rep2 = bug_reports::record_report(
        &state,
        &caller,
        "house",
        "load",
        "invalid",
        "cannot read properties of null (reading 'charAt')",
        Some("house-1"),
        t1,
    )
    .await
    .unwrap();

    assert!(!rep2.is_new, "repeat occurrence must NOT be marked as new");
    assert_eq!(rep2.id, rep1.id, "must reuse same report row");
    assert_eq!(rep2.signature, rep1.signature);
    assert_eq!(rep2.report_count, 2, "must increment report count");
    assert_eq!(rep2.updated_at, t1);

    // Verify DB list only has 1 record with count 2
    let list = state.db.list_bug_reports(10).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].report_count, 2);
}

#[tokio::test]
async fn bug_report_files_github_issue_and_comments_on_repeat() {
    let _gh = GH_ENV.lock().await;
    let mock_data = Arc::new(Mutex::new(Vec::<Value>::new()));

    async fn handle_issues(
        State(data): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        data.lock().unwrap().push(payload);
        Json(json!({ "number": 842 }))
    }

    async fn handle_comments(
        State(data): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        data.lock().unwrap().push(payload);
        Json(json!({ "id": 1001 }))
    }

    let router = Router::new()
        .route("/repos/AgentsHangout/agentshangout/issues", post(handle_issues))
        .route("/repos/AgentsHangout/agentshangout/issues/842/comments", post(handle_comments))
        .with_state(mock_data.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let db = Db::open(":memory:").unwrap();
    let mut app = GitHubApp::from_settings(Some("client".into()), Some("secret".into())).unwrap();
    app.api_url = format!("http://{addr}");

    let state = AppState::with_apps(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
            require_rules: false,
        },
        Apps { github: Some(app), discord: None },
    );

    unsafe {
        std::env::set_var("AGENTHANGOUT_GITHUB_TOKEN", "mock_gh_token");
    }

    seed_person(&state.db, "person-test");
    let caller = Caller {
        person_id: "person-test".into(),
        agent_id: None,
        name: "Tester".into(),
        secret_hash: "secret".into(),
        is_house: false,
    };

    let t0 = now();
    // First report creates GitHub issue
    let rep1 = bug_reports::record_report(
        &state,
        &caller,
        "place",
        "bulldoze",
        "invalid",
        "cannot bulldoze water tile",
        Some("room-abc"),
        t0,
    )
    .await
    .unwrap();

    assert_eq!(rep1.github_issue, Some(842));
    assert_eq!(mock_data.lock().unwrap().len(), 1);
    let issue_payload = mock_data.lock().unwrap()[0].clone();
    assert!(issue_payload["title"].as_str().unwrap().contains("place bulldoze failed"));
    let issue_text = issue_payload.to_string();
    assert!(!issue_text.contains("room-abc"), "hangout id must stay out of GitHub");
    assert!(!issue_text.contains("person-test"), "reporter must stay out of GitHub");
    assert!(!issue_text.contains("water tile"), "free text must stay out of GitHub");

    // Second report adds comment to issue 842
    let rep2 = bug_reports::record_report(
        &state,
        &caller,
        "place",
        "bulldoze",
        "invalid",
        "cannot bulldoze water tile",
        Some("room-abc"),
        t0 + 5,
    )
    .await
    .unwrap();

    assert_eq!(rep2.github_issue, Some(842));
    assert_eq!(rep2.report_count, 2);
    assert_eq!(mock_data.lock().unwrap().len(), 2);
    let comment_payload = mock_data.lock().unwrap()[1].clone();
    assert!(comment_payload["body"].as_str().unwrap().contains("seen 2 times"));
    assert!(!comment_payload.to_string().contains("room-abc"));

    unsafe {
        std::env::remove_var("AGENTHANGOUT_GITHUB_TOKEN");
    }
}

#[tokio::test]
async fn bug_report_endpoint_refuses_callers_who_are_not_signed_in() {
    let db = Db::open(":memory:").unwrap();
    let state = AppState::new(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
            require_rules: false,
        },
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let watch = state.clone();
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(state)).await.unwrap() });

    let client = reqwest::Client::new();
    let payload = json!({
        "surface": "house",
        "action": "load",
        "code": "invalid",
        "error": "Cannot read properties of null (reading 'charAt')",
        "hangout_id": "house-123",
    });

    let res =
        client.post(format!("http://{addr}/api/bugs/report")).json(&payload).send().await.unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(watch.db.list_bug_reports(10).unwrap().is_empty(), "nothing is written for a stranger");
}

fn test_state(github: Option<GitHubApp>) -> AppState {
    let db = Db::open(":memory:").unwrap();
    let config = Config {
        public_url: "http://localhost".into(),
        create_code: None,
        allow_private_webhooks: true,
        keeper_id: None,
        god_ids: vec![],
        require_rules: false,
    };
    match github {
        Some(app) => AppState::with_apps(db, config, Apps { github: Some(app), discord: None }),
        None => AppState::new(db, config),
    }
}

fn caller_for(person: &str) -> Caller {
    Caller {
        person_id: person.into(),
        agent_id: None,
        name: person.into(),
        secret_hash: "secret".into(),
        is_house: false,
    }
}

#[tokio::test]
async fn bug_report_refuses_fields_off_the_fixed_lists() {
    let state = test_state(None);
    let c = caller_for("p1");
    for (surface, action, code) in [
        ("house @everyone", "load", "invalid"),
        ("house", "load https://evil.example", "invalid"),
        ("house", "load", "bad code"),
        ("nowhere", "load", "invalid"),
    ] {
        let res =
            bug_reports::record_report(&state, &c, surface, action, code, "x", None, now()).await;
        assert!(res.is_err(), "{surface}/{action}/{code} must be refused");
        assert_eq!(res.unwrap_err().status, axum::http::StatusCode::BAD_REQUEST);
    }
    assert!(state.db.list_bug_reports(10).unwrap().is_empty(), "refused reports write nothing");
}

#[test]
fn sanitize_cuts_on_characters_not_bytes() {
    // 600 emoji: 4 bytes each, so a byte cut at 497 would land mid-character and panic.
    let raw: String = "🐸".repeat(600);
    let cleaned = bug_reports::sanitize_error(&raw);
    assert_eq!(cleaned.chars().count(), 500);
    assert!(cleaned.ends_with("..."));
    let accented = format!("{}é", "a".repeat(496));
    assert_eq!(bug_reports::sanitize_error(&accented).chars().count(), 497);
}

#[tokio::test]
async fn bug_report_per_person_and_site_caps_stop_github_issues_but_keep_rows() {
    let _gh = GH_ENV.lock().await;
    let mock_data = Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn handle(
        State(data): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        let mut v = data.lock().unwrap();
        v.push(payload);
        Json(json!({ "number": 900 + v.len() as i64 }))
    }
    let router = Router::new()
        .route("/repos/AgentsHangout/agentshangout/issues", post(handle))
        .with_state(mock_data.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let mut app = GitHubApp::from_settings(Some("client".into()), Some("secret".into())).unwrap();
    app.api_url = format!("http://{addr}");
    let state = test_state(Some(app));
    unsafe {
        std::env::set_var("AGENTHANGOUT_GITHUB_TOKEN", "mock_gh_token");
    }

    let t = 1_800_000_000; // a fixed UTC day
    let codes = ["unknown", "not_found", "forbidden", "invalid"];
    seed_person(&state.db, "alice");
    seed_person(&state.db, "carol");
    for i in 0..30 {
        seed_person(&state.db, &format!("bob{i}"));
    }
    let alice = caller_for("alice");
    let mut filed = 0;
    for code in codes {
        let rec = bug_reports::record_report(&state, &alice, "house", "load", code, "x", None, t)
            .await
            .unwrap();
        if rec.github_issue.is_some() {
            filed += 1;
        }
    }
    assert_eq!(
        filed,
        bug_reports::ISSUES_PER_PERSON_PER_DAY as usize,
        "alice is capped at 3 a day"
    );
    assert_eq!(state.db.list_bug_reports(10).unwrap().len(), 4, "every report is kept in SQLite");

    // Tomorrow resets the per-person cap.
    let rec = bug_reports::record_report(
        &state,
        &alice,
        "house",
        "load",
        "closed",
        "x",
        None,
        t + 86_400,
    )
    .await
    .unwrap();
    assert!(rec.github_issue.is_some());

    // The site cap counts everyone: fill it up from other people, then nobody gets a new issue.
    let mut site_full = false;
    for i in 0..30 {
        let who = caller_for(&format!("bob{i}"));
        let surface = bug_reports::SURFACES[i % bug_reports::SURFACES.len()];
        let action = bug_reports::ACTIONS[i % bug_reports::ACTIONS.len()];
        let rec = bug_reports::record_report(
            &state,
            &who,
            surface,
            action,
            "limit",
            "x",
            None,
            t + 86_400,
        )
        .await
        .unwrap();
        if rec.github_issue.is_none() {
            site_full = true;
            break;
        }
    }
    assert!(site_full, "the site-wide cap must stop issues");
    let carol = bug_reports::record_report(
        &state,
        &caller_for("carol"),
        "item",
        "use",
        "conflict",
        "x",
        None,
        t + 86_400,
    )
    .await
    .unwrap();
    assert!(carol.github_issue.is_none(), "over the site cap, SQLite only");

    unsafe {
        std::env::remove_var("AGENTHANGOUT_GITHUB_TOKEN");
    }
}

#[tokio::test]
async fn bug_report_slot_claims_are_atomic_under_concurrency() {
    // Twenty-five people race for the 20 site slots of one day. Only 20 may win.
    let db = Db::open(":memory:").unwrap();
    let state = AppState::new(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
            require_rules: false,
        },
    );
    let mut ids = Vec::new();
    for i in 0..25 {
        let rec = state
            .db
            .record_bug_report(
                &agenthangout::db::NewBugReport {
                    signature: &format!("sig{i}"),
                    surface: "house",
                    action: "load",
                    code: "unknown",
                    error_message: "x",
                    hangout_id: None,
                    reporter_id: None,
                },
                1_800_000_000,
                100,
            )
            .unwrap()
            .expect("no reporter, so no cap");
        ids.push(rec.id);
    }
    let db = &state.db;
    for i in 0..25 {
        seed_person(db, &format!("p{i}"));
    }
    let won: usize = std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let id = *id;
                scope.spawn(move || {
                    // Each person has a fresh cap of 3, so only the site cap of 20 can stop them.
                    db.claim_bug_issue_slot(id, &format!("p{i}"), 20_000, 3, 20).unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).filter(|w| *w).count()
    });
    assert_eq!(won, 20);
}

#[test]
fn repeat_comments_fire_only_at_2_then_powers_of_ten() {
    let hits: Vec<i64> = (1..=1000).filter(|n| bug_reports::comments_on_repeat(*n)).collect();
    assert_eq!(hits, vec![2, 10, 100, 1000]);
}

#[tokio::test]
async fn bug_report_repeats_post_only_a_few_github_comments() {
    // One signed-in caller replays the same failure 150 times. Only the 2nd, 10th and 100th
    // repeats may reach GitHub as comments; the rows still count every call.
    let _gh = GH_ENV.lock().await;
    let mock_data = Arc::new(Mutex::new(Vec::<Value>::new()));
    async fn handle_issues() -> Json<Value> {
        Json(json!({ "number": 845 }))
    }
    async fn handle_comments(
        State(data): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        data.lock().unwrap().push(payload);
        Json(json!({ "id": 1 }))
    }
    let router = Router::new()
        .route("/repos/AgentsHangout/agentshangout/issues", post(handle_issues))
        .route("/repos/AgentsHangout/agentshangout/issues/845/comments", post(handle_comments))
        .with_state(mock_data.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let mut app = GitHubApp::from_settings(Some("client".into()), Some("secret".into())).unwrap();
    app.api_url = format!("http://{addr}");
    let state = test_state(Some(app));
    unsafe {
        std::env::set_var("AGENTHANGOUT_GITHUB_TOKEN", "mock_gh_token");
    }

    seed_person(&state.db, "spammer");
    let spammer = caller_for("spammer");
    let t = 1_800_000_000;
    for i in 0..150 {
        bug_reports::record_report(&state, &spammer, "item", "use", "invalid", "x", None, t + i)
            .await
            .unwrap();
    }
    let comments = mock_data.lock().unwrap().len();
    assert_eq!(comments, 3, "comments only at counts 2, 10 and 100");
    assert_eq!(state.db.list_bug_reports(10).unwrap()[0].report_count, 150);

    unsafe {
        std::env::remove_var("AGENTHANGOUT_GITHUB_TOKEN");
    }
}

/// BG6 (#915): a person may first-file only so many rows; repeats of rows that already exist
/// still count, and a refused report writes nothing.
#[tokio::test]
async fn bug_report_rows_are_capped_per_person_but_repeats_still_count() {
    let state = test_state(None);
    seed_person(&state.db, "rower");
    seed_person(&state.db, "other");
    let c = caller_for("rower");
    let cap = bug_reports::ROWS_PER_PERSON;
    let mut first = Vec::new();
    for surface in ["house", "space", "place", "item", "door"] {
        for action in bug_reports::ACTIONS {
            for code in bug_reports::CODES {
                if (first.len() as i64) < cap {
                    let rec = bug_reports::record_report(
                        &state,
                        &c,
                        surface,
                        action,
                        code,
                        "x",
                        None,
                        now(),
                    )
                    .await
                    .expect("under the cap");
                    first.push((surface, action, code, rec.id));
                }
            }
        }
    }
    assert_eq!(first.len() as i64, cap);

    // The next new signature from the same person is refused, and nothing is stored for it.
    let err =
        bug_reports::record_report(&state, &c, "item", "bulldoze", "unknown", "x", None, now())
            .await
            .unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(state.db.list_bug_reports(1000).unwrap().len() as i64, cap);

    // A repeat of a row that already exists still counts, even at the cap.
    let (s, a, code, _) = first[0];
    let again = bug_reports::record_report(&state, &c, s, a, code, "x", None, now()).await.unwrap();
    assert_eq!(again.report_count, 2);

    // Another person is not affected by this person's cap.
    let other = caller_for("other");
    bug_reports::record_report(&state, &other, "item", "bulldoze", "unknown", "x", None, now())
        .await
        .expect("a different person has their own cap");
}

/// BG1 (#915): a failed in-game action is captured with its fixed code; a success is not.
#[tokio::test]
async fn capture_if_failed_records_the_failure_and_passes_the_result_through() {
    let state = test_state(None);
    seed_person(&state.db, "player");
    let c = caller_for("player");
    let failed: agenthangout::error::ApiResult<()> =
        Err(agenthangout::error::ApiError::conflict("that item is closed").with_reason("closed"));
    let out = bug_reports::capture_if_failed(&state, &c, "item", "use", None, now(), failed).await;
    assert_eq!(out.unwrap_err().status, axum::http::StatusCode::CONFLICT);
    let rows = state.db.list_bug_reports(10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].surface.as_str(), rows[0].action.as_str(), rows[0].code.as_str()),
        ("item", "use", "closed")
    );

    let ok: agenthangout::error::ApiResult<u8> = Ok(7);
    let out = bug_reports::capture_if_failed(&state, &c, "item", "use", None, now(), ok).await;
    assert_eq!(out.unwrap(), 7);
    assert_eq!(state.db.list_bug_reports(10).unwrap().len(), 1, "a success stores nothing");
}

/// BG1 (#915 follow-up): an expected refusal (no room, already on a trip) stores no report;
/// a 5xx or a refusal with a fixed code is still captured. The caller's result is unchanged.
#[tokio::test]
async fn capture_if_failed_skips_expected_refusals() {
    use agenthangout::error::{ApiError, ApiResult};
    use axum::http::StatusCode;

    let state = test_state(None);
    seed_person(&state.db, "player");
    let c = caller_for("player");

    let expected: Vec<ApiError> = vec![
        ApiError::conflict("there's no room there right now"),
        ApiError::conflict("bot is already on a trip, at Lake House"),
        ApiError::forbidden("that trip isn't this bot's"),
        ApiError::not_found(),
        // Coded but expected: the reason is not on the fixed list, so nothing is stored.
        ApiError::conflict("there's no room there right now").with_reason("no_room"),
        ApiError::forbidden("accept the rules first").with_reason("rules_not_accepted"),
    ];
    for err in expected {
        let status = err.status;
        let res: ApiResult<()> = Err(err);
        let out =
            bug_reports::capture_if_failed(&state, &c, "house", "build", None, now(), res).await;
        assert_eq!(out.unwrap_err().status, status, "the result passes through");
    }
    assert!(state.db.list_bug_reports(10).unwrap().is_empty(), "expected refusals store no report");

    let server: ApiResult<()> =
        Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "database error"));
    let out =
        bug_reports::capture_if_failed(&state, &c, "house", "build", None, now(), server).await;
    assert_eq!(out.unwrap_err().status, StatusCode::INTERNAL_SERVER_ERROR);
    let rows = state.db.list_bug_reports(10).unwrap();
    assert_eq!(rows.len(), 1, "a 5xx is stored");
    assert_eq!(rows[0].code, "unknown");

    let coded: ApiResult<()> = Err(ApiError::conflict("that item is closed").with_reason("closed"));
    let _ = bug_reports::capture_if_failed(&state, &c, "item", "use", None, now(), coded).await;
    assert_eq!(state.db.list_bug_reports(10).unwrap().len(), 2, "a fixed-code refusal is stored");
}
