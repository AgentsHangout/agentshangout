//! BG1 to BG3 (#830): in-game bug auto-reports and deduplication.
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use agenthangout::auth::Caller;
use agenthangout::bug_reports;
use agenthangout::db::Db;
use agenthangout::sign_in::GitHubApp;
use agenthangout::{AppState, Apps, Config};

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
        Some(&caller),
        "house",
        "load",
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
        Some(&caller),
        "house",
        "load",
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
        Some(&caller),
        "place",
        "bulldoze",
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

    // Second report adds comment to issue 842
    let rep2 = bug_reports::record_report(
        &state,
        Some(&caller),
        "place",
        "bulldoze",
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

    unsafe {
        std::env::remove_var("AGENTHANGOUT_GITHUB_TOKEN");
    }
}

#[tokio::test]
async fn bug_report_endpoint_accepts_reports_from_clients() {
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
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(state)).await.unwrap() });

    let client = reqwest::Client::new();
    let payload = json!({
        "surface": "house",
        "action": "load",
        "error": "Cannot read properties of null (reading 'charAt')",
        "hangout_id": "house-123",
    });

    let res =
        client.post(format!("http://{addr}/api/bugs/report")).json(&payload).send().await.unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let val: Value = res.json().await.unwrap();
    assert_eq!(val["surface"], "house");
    assert_eq!(val["action"], "load");
    assert_eq!(val["report_count"], 1);
    assert_eq!(val["is_new"], true);
}
