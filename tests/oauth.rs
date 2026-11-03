//! MCP OAuth is an independent, revocable grant to one explicitly selected bot.
mod support;

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

// RL1/RW4: OAuth grants stay valid after GitHub I/O, just like legacy bot tokens.
#[tokio::test]
async fn oauth_research_tools_revalidate_the_grant_after_github_requests() {
    use agenthangout::{auth, now, sign_in::GitHubApp, Apps};
    use axum::{routing::get, Json, Router};
    let github = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let github_url = format!("http://{}", github.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(github,Router::new()
        .route("/repos/org/repo",get(||async {Json(json!({"private":false,"full_name":"org/repo"}))}))
        .route("/repos/org/repo/issues",get(||async {Json(json!([{"number":7,"title":"Research","state":"closed","pull_request":{}}]))}))
        .route("/repos/org/repo/pulls/7",get(||async {Json(json!({"user":{"id":42},"body":"Agents Hangout bot: research-bot","merged":true}))})))
        .await.unwrap()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let db = Db::open(":memory:").unwrap();
    db.create_person(
        "research-owner",
        "Research owner",
        &auth::hash_secret("owner-test-key"),
        "feed",
        now(),
    )
    .unwrap();
    db.create_agent(
        "research-bot",
        "research-owner",
        "Research bot",
        &auth::hash_secret("bot-test-key"),
        now(),
    )
    .unwrap();
    db.set_sign_in_link("research-owner", "github", "42", "research-owner", now()).unwrap();
    db.set_god_mode("research-owner", true, now()).unwrap();
    let mut github_app =
        GitHubApp::from_settings(Some("test".into()), Some("test".into())).unwrap();
    github_app.api_url = github_url;
    let state = AppState::with_apps(
        db,
        Config {
            public_url: base.clone(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec!["research-owner".into()],
        },
        Apps { github: Some(github_app), discord: None },
    );
    let session = state.start_session("research-owner").unwrap();
    let served = state.clone();
    tokio::spawn(async move { axum::serve(listener, app(served)).await.unwrap() });
    let s = Server {
        base,
        client: Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
        state,
    };
    let client = s.register().await;
    let code = s.code(&session, &client, "research-bot").await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let created=s.event_rpc(access,"tools/call",json!({"name":"create_research_lab","arguments":{"name":"Research lab","purpose":"Study useful open research together in this lab.","github":"org/repo"}})).await;
    assert!(created["result"]["isError"].is_null(), "{created}");
    let id = created["result"]["structuredContent"]["id"].as_str().unwrap();
    let read = s
        .event_rpc(
            access,
            "tools/call",
            json!({"name":"read_research_work","arguments":{"lab_id":id}}),
        )
        .await;
    assert!(read["result"]["isError"].is_null(), "{read}");
    assert_eq!(read["result"]["structuredContent"]["items"][0]["number"], 7);
    let recorded=s.event_rpc(access,"tools/call",json!({"name":"record_research_contribution","arguments":{"lab_id":id,"url":"https://github.com/org/repo/pull/7"}})).await;
    assert!(recorded["result"]["isError"].is_null(), "{recorded}");
    assert_eq!(recorded["result"]["structuredContent"]["verified_merged"], true);
}
impl Server {
    async fn event_rpc(&self, token: &str, method: &str, params: Value) -> Value {
        self.client
            .post(format!("{}/mcp", self.base))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn start() -> Self {
        Self::start_with(true).await
    }
    async fn start_with(allow_private_webhooks: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: None,
                allow_private_webhooks,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self {
            base,
            client: Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
            state,
        }
    }
}
#[tokio::test]
async fn oauth_discovers_webhook_mentions() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let reply =
        s.event_rpc(tokens["access_token"].as_str().unwrap(), "events/list", json!({})).await;
    let mention = reply["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "mention")
        .unwrap();
    assert!(mention["delivery"].as_array().unwrap().contains(&json!("webhook")));
}

#[tokio::test]
async fn oauth_webhooks_require_explicit_event_scope_and_cannot_be_added_by_refresh() {
    let s = Server::start().await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let page = s.authorize_scope(&session, &client, "mcp:bot").await.text().await.unwrap();
    assert!(page.contains("Webhook subscriptions are excluded"));
    assert!(!page.contains("name=\"events\""), "nothing to tick when webhooks weren't asked for");
    let response = s.consent(&session, &hidden(&page, "nonce"), &bot, "allow").await;
    let location = reqwest::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
    let code = location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let catalog = s.event_rpc(access, "events/list", json!({})).await;
    for e in catalog["result"]["events"].as_array().unwrap() {
        assert_eq!(e["delivery"], json!(["poll", "push"]));
    }
    let params = subscription(&room, "https://callback.example/events");
    assert_eq!(
        s.event_rpc(access, "events/subscribe", params.clone()).await["error"]["code"],
        -32024
    );
    let rotated: Value = s
        .client
        .post(format!("{}/oauth/token", s.base))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client.as_str()),
            ("resource", format!("{}/mcp", s.base).as_str()),
            ("refresh_token", t["refresh_token"].as_str().unwrap()),
            ("scope", "mcp:bot mcp:events"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rotated["scope"], "mcp:bot");
    assert_eq!(
        s.event_rpc(rotated["access_token"].as_str().unwrap(), "events/subscribe", params).await
            ["error"]["code"],
        -32024
    );
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
}
#[tokio::test]
async fn oauth_discovery_and_mcp_challenge_describe_the_same_protected_resource() {
    let s = Server::start().await;
    let response = s
        .client
        .get(format!("{}/.well-known/oauth-protected-resource/mcp", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let metadata: Value = response.json().await.unwrap();
    assert_eq!(metadata["resource"], format!("{}/mcp", s.base));
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let challenge = response.headers()["www-authenticate"].to_str().unwrap().to_string();
    assert!(challenge.contains("resource_metadata="), "{challenge}");
    // A client that asks for exactly the challenge's scope is offered webhook consent (#284).
    assert!(challenge.contains("scope=\"mcp:bot mcp:events\""), "{challenge}");
}

#[tokio::test]
async fn oauth_owner_can_untick_mention_webhooks_and_the_grant_says_how_to_add_them() {
    let s = Server::start().await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let page = s.authorize(&session, &client).await.text().await.unwrap();
    assert!(page.contains("also asks for mention webhook subscriptions"), "{page}");
    assert!(page.contains("name=\"events\" value=\"allow\" checked"), "{page}");

    // Unticked, so the form sends no events field: tools only.
    let nonce = hidden(&page, "nonce");
    let response = s
        .client
        .post(format!("{}/oauth/consent", s.base))
        .header("cookie", format!("ah_session={session}"))
        .header("origin", &s.base)
        .form(&[("nonce", nonce.as_str()), ("agent_id", bot.as_str()), ("decision", "allow")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = reqwest::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
    let code = location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    assert_eq!(t["scope"], "mcp:bot", "{t}");
    let access = t["access_token"].as_str().unwrap();

    let catalog = s.event_rpc(access, "events/list", json!({})).await;
    for e in catalog["result"]["events"].as_array().unwrap() {
        assert_eq!(e["delivery"], json!(["poll", "push"]));
    }
    let params = subscription(&room, "https://callback.example/events");
    let refused = s.event_rpc(access, "events/subscribe", params).await;
    assert_eq!(refused["error"]["code"], -32024, "{refused}");
    assert_eq!(refused["error"]["data"]["required_scope"], "mcp:bot mcp:events", "{refused}");
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK, "tools still work");
}

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const CALLBACK: &str = "https://client.example/callback";
const WEBHOOK_SECRET: &str = "whsec_YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=";

#[derive(Clone, Default)]
struct Webhook {
    requests: std::sync::Arc<std::sync::Mutex<Vec<(axum::http::HeaderMap, Value)>>>,
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    bad_challenge: std::sync::Arc<std::sync::atomic::AtomicBool>,
    block_verification: std::sync::Arc<std::sync::atomic::AtomicBool>,
    block_events: std::sync::Arc<std::sync::atomic::AtomicBool>,
    entered: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}
impl Webhook {
    async fn start() -> (Self, String) {
        use axum::{
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::post,
            Json, Router,
        };
        let hook = Self::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/callback", listener.local_addr().unwrap());
        let router = Router::new().route("/callback", post(|State(h): State<Webhook>, headers: HeaderMap, Json(body): Json<Value>| async move {
            assert!(headers.contains_key("x-mcp-subscription-id"));
            assert!(headers["webhook-signature"].to_str().unwrap().starts_with("v1,"));
            h.requests.lock().unwrap().push((headers,body.clone()));
            let blocked = if body["type"] == "verification" { &h.block_verification } else { &h.block_events };
            if blocked.load(std::sync::atomic::Ordering::SeqCst) {
                h.entered.notify_one();
                h.release.notified().await;
            }
            if h.fail.load(std::sync::atomic::Ordering::SeqCst) { return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({}))); }
            if h.bad_challenge.load(std::sync::atomic::Ordering::SeqCst) { return (StatusCode::OK,Json(json!({"challenge":"wrong"}))); }
            (StatusCode::OK,Json(json!({"challenge":body["challenge"]})))
        })).with_state(hook.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (hook, url)
    }
    fn events(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, b)| b.get("eventId").is_some())
            .map(|(_, b)| b.clone())
            .collect()
    }
}

fn subscription(room: &str, url: &str) -> Value {
    json!({"name":"mention","arguments":{"hangout_id":room},"delivery":{"mode":"webhook","url":url,"secret":WEBHOOK_SECRET}})
}

#[tokio::test]
async fn stalled_verification_does_not_block_an_independent_grant() {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };
    let s = Arc::new(Server::start().await);
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let a: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let code = s.code(&session, &client, &bot).await;
    let b: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let (slow, slow_url) = Webhook::start().await;
    let (fast, fast_url) = Webhook::start().await;
    let fast_params = subscription(&room, &fast_url);
    let access = b["access_token"].as_str().unwrap();
    assert!(
        s.event_rpc(access, "events/subscribe", fast_params.clone()).await["result"].is_object()
    );
    slow.block_verification.store(true, Ordering::SeqCst);
    let params = subscription(&room, &slow_url);
    let caller = s.clone();
    let waiting = tokio::spawn(async move {
        caller.event_rpc(a["access_token"].as_str().unwrap(), "events/subscribe", params).await
    });
    tokio::time::timeout(Duration::from_secs(2), slow.entered.notified()).await.unwrap();
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "independent", "{}", agenthangout::now(), 0)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        agenthangout::events::oauth::deliver(&s.state).await.unwrap();
        assert_eq!(fast.events().len(), 1);
        let renewed = s.event_rpc(access, "events/subscribe", fast_params.clone()).await;
        assert_eq!(renewed["result"]["cursor"], fast.events()[0]["cursor"]);
        assert!(s.event_rpc(access, "events/unsubscribe", fast_params).await["result"].is_object());
    })
    .await
    .expect("another grant must not wait for stalled verification");
    assert!(!waiting.is_finished());
    slow.release.notify_waiters();
    assert!(waiting.await.unwrap()["result"].is_object());
}

#[tokio::test]
async fn stalled_delivery_does_not_block_an_independent_grant() {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };
    let s = Arc::new(Server::start().await);
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let a: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let code = s.code(&session, &client, &bot).await;
    let b: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let (slow, slow_url) = Webhook::start().await;
    let (fast, fast_url) = Webhook::start().await;
    let slow_params = subscription(&room, &slow_url);
    let fast_params = subscription(&room, &fast_url);
    assert!(s
        .event_rpc(a["access_token"].as_str().unwrap(), "events/subscribe", slow_params.clone())
        .await["result"]
        .is_object());
    assert!(s
        .event_rpc(b["access_token"].as_str().unwrap(), "events/subscribe", fast_params.clone())
        .await["result"]
        .is_object());
    slow.block_events.store(true, Ordering::SeqCst);
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "parallel", "{}", agenthangout::now(), 0)
        .unwrap();
    let caller = s.clone();
    let pending =
        tokio::spawn(
            async move { agenthangout::events::oauth::deliver(&caller.state).await.unwrap() },
        );
    tokio::time::timeout(Duration::from_secs(2), slow.entered.notified()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while fast.events().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let renewed = s
            .event_rpc(b["access_token"].as_str().unwrap(), "events/subscribe", fast_params.clone())
            .await;
        assert_eq!(renewed["result"]["cursor"], fast.events()[0]["cursor"]);
        assert!(s
            .event_rpc(b["access_token"].as_str().unwrap(), "events/unsubscribe", fast_params)
            .await["result"]
            .is_object());
    })
    .await
    .expect("another grant must not wait for stalled delivery");
    assert!(!pending.is_finished());
    slow.release.notify_waiters();
    pending.await.unwrap();
    let renewed =
        s.event_rpc(a["access_token"].as_str().unwrap(), "events/subscribe", slow_params).await;
    assert_eq!(renewed["result"]["cursor"], slow.events()[0]["cursor"]);
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(slow.events().len(), 1, "renewal must retain the acknowledged cursor");
}

#[tokio::test]
async fn oauth_outbound_limit_is_shared_and_concurrent_creates_respect_the_grant_cap() {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };
    let s = Arc::new(Server::start().await);
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap().to_string();
    let (delivery, url) = Webhook::start().await;
    let initial = subscription(&room, &url);
    assert!(s.event_rpc(&access, "events/subscribe", initial).await["result"].is_object());
    delivery.block_events.store(true, Ordering::SeqCst);
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "bounded", "{}", agenthangout::now(), 0)
        .unwrap();
    let caller = s.clone();
    let pass =
        tokio::spawn(
            async move { agenthangout::events::oauth::deliver(&caller.state).await.unwrap() },
        );
    tokio::time::timeout(Duration::from_secs(2), delivery.entered.notified()).await.unwrap();
    let (verification, url) = Webhook::start().await;
    verification.block_verification.store(true, Ordering::SeqCst);
    let mut creates = Vec::new();
    for i in 0..8 {
        let caller = s.clone();
        let access = access.clone();
        let params = subscription(&room, &format!("{url}?sub={i}"));
        creates.push(tokio::spawn(async move {
            caller.event_rpc(&access, "events/subscribe", params).await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while verification.requests.lock().unwrap().len() < 7 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        verification.requests.lock().unwrap().len(),
        7,
        "one delivery plus seven verifications exhaust the shared limit"
    );
    verification.block_verification.store(false, Ordering::SeqCst);
    verification.release.notify_waiters();
    for create in creates {
        assert!(create.await.unwrap()["result"].is_object());
    }
    delivery.release.notify_waiters();
    pass.await.unwrap();
    // There are nine subscriptions now. Twelve concurrent identities may add only eleven.
    let mut creates = Vec::new();
    for i in 8..20 {
        let caller = s.clone();
        let access = access.clone();
        let params = subscription(&room, &format!("{url}?sub={i}"));
        creates.push(tokio::spawn(async move {
            caller.event_rpc(&access, "events/subscribe", params).await
        }));
    }
    let mut accepted = 0;
    let mut limited = 0;
    for create in creates {
        let result = create.await.unwrap();
        if result["result"].is_object() {
            accepted += 1;
        } else {
            assert_eq!(result["error"]["code"], -32025);
            limited += 1;
        }
    }
    assert_eq!((accepted, limited), (11, 1));
}

#[tokio::test]
async fn same_subscription_delivery_renewal_and_recreation_preserve_progress() {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };
    let s = Arc::new(Server::start().await);
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap().to_string();
    let (hook, url) = Webhook::start().await;
    let params = subscription(&room, &url);
    let first = s.event_rpc(&access, "events/subscribe", params.clone()).await;
    let id = first["result"]["id"].clone();
    hook.block_events.store(true, Ordering::SeqCst);
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "consistent", "{}", agenthangout::now(), 0)
        .unwrap();
    let caller = s.clone();
    let pending =
        tokio::spawn(
            async move { agenthangout::events::oauth::deliver(&caller.state).await.unwrap() },
        );
    tokio::time::timeout(Duration::from_secs(2), hook.entered.notified()).await.unwrap();
    // Overlapping passes must skip the locked identity rather than deliver its stale snapshot.
    tokio::time::timeout(Duration::from_secs(2), agenthangout::events::oauth::deliver(&s.state))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hook.events().len(), 1);
    let caller = s.clone();
    let token = access.clone();
    let renewal = params.clone();
    let renewing =
        tokio::spawn(async move { caller.event_rpc(&token, "events/subscribe", renewal).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!renewing.is_finished(), "renewal must wait for this identity's acknowledgment");
    hook.block_events.store(false, Ordering::SeqCst);
    hook.release.notify_waiters();
    pending.await.unwrap();
    let renewed = renewing.await.unwrap();
    assert_eq!(renewed["result"]["cursor"], hook.events()[0]["cursor"]);
    assert_eq!(renewed["result"]["id"], id);
    assert!(s.event_rpc(&access, "events/unsubscribe", params.clone()).await["result"].is_object());
    let recreated = s.event_rpc(&access, "events/subscribe", params).await;
    assert_eq!(recreated["result"]["id"], id);
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(
        hook.events().len(),
        1,
        "an old delivery result cannot overwrite a recreated identity"
    );
}

#[tokio::test]
async fn oauth_webhooks_require_public_https_callbacks_in_production() {
    let s = Server::start_with(false).await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    for url in [
        "http://callback.example/events",
        "https://127.0.0.1/events",
        "https://[::1]/events",
        "https://localhost/events",
        "https://user:pass@callback.example/events",
        "https://callback.example:444/events",
        "https://callback.example/events#fragment",
    ] {
        assert_eq!(
            s.event_rpc(
                t["access_token"].as_str().unwrap(),
                "events/subscribe",
                subscription(&room, url)
            )
            .await["error"]["code"],
            -32602,
            "{url}"
        );
    }
}

#[tokio::test]
async fn real_mention_reaches_the_persistent_worker_without_a_poll_listener() {
    let s = Server::start().await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let (hook, url) = Webhook::start().await;
    let params = subscription(&room, &url);
    let subscribed =
        s.event_rpc(t["access_token"].as_str().unwrap(), "events/subscribe", params.clone()).await;
    assert!(subscribed["result"]["id"].is_string(), "{subscribed}");
    let worker = tokio::spawn(agenthangout::events::oauth::run(s.state.clone()));
    let posted = s
        .client
        .post(format!("{}/api/hangouts/{room}/messages", s.base))
        .bearer_auth(&session)
        .json(&json!({"text":"@Dedicated bot please read this room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(posted.status(), StatusCode::OK);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while hook.events().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(hook.events()[0]["name"], "mention");
    assert_eq!(hook.events()[0]["data"]["body"], "@Dedicated bot please read this room");
    assert_eq!(hook.events()[0]["data"]["hangout_id"], room);
    s.event_rpc(t["access_token"].as_str().unwrap(), "events/unsubscribe", params).await;
    worker.abort();
}

#[tokio::test]
async fn oauth_signing_key_rotation_reverifies_and_overlaps_signatures_without_new_identity() {
    let s = Server::start().await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let (hook, url) = Webhook::start().await;
    let mut params = subscription(&room, &url);
    let first = s.event_rpc(access, "events/subscribe", params.clone()).await;
    params["delivery"]["secret"] = json!("whsec_YmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmI=");
    let rotated = s.event_rpc(access, "events/subscribe", params).await;
    assert_eq!(rotated["result"]["id"], first["result"]["id"]);
    assert_eq!(hook.requests.lock().unwrap().len(), 2, "replacement key must verify again");
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "rotated", "{}", agenthangout::now(), 0)
        .unwrap();
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    let requests = hook.requests.lock().unwrap();
    let signature = requests.last().unwrap().0["webhook-signature"].to_str().unwrap();
    let parts: Vec<_> = signature.split(' ').collect();
    assert_eq!(parts.len(), 2);
    assert_ne!(parts[0], parts[1]);
}

#[tokio::test]
async fn oauth_webhook_lifecycle_is_grant_owned_and_refresh_preserves_pending_cursor() {
    let s = Server::start().await;
    let (session, bot, legacy, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let (hook, url) = Webhook::start().await;
    let params = subscription(&room, &url);
    let first = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert!(first["result"]["id"].is_string(), "{first}");
    assert_eq!(hook.requests.lock().unwrap().len(), 1);
    let id = first["result"]["id"].clone();
    let at = agenthangout::now();
    s.state
        .db
        .insert_bot_event(
            &bot,
            "mention",
            Some(&room),
            "pending",
            &json!({"hangout_id":room,"body":"@Dedicated bot","what_to_do":"legacy instructions"})
                .to_string(),
            at,
            0,
        )
        .unwrap();
    let rotated: Value =
        s.refresh(&client, t["refresh_token"].as_str().unwrap()).await.json().await.unwrap();
    let access = rotated["access_token"].as_str().unwrap();
    let renewed = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert_eq!(renewed["result"]["id"], id);
    assert_eq!(
        renewed["result"]["cursor"], first["result"]["cursor"],
        "pending event must not be skipped"
    );
    assert_eq!(
        hook.requests.lock().unwrap().len(),
        1,
        "verification cache is bounded and principal scoped"
    );

    // A different grant of the same client and bot cannot delete this subscription.
    let code = s.code(&session, &client, &bot).await;
    let other: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    assert!(s
        .event_rpc(other["access_token"].as_str().unwrap(), "events/unsubscribe", params.clone())
        .await["result"]
        .is_object());
    // Nor can its legacy bot token.
    let _ = s.event_rpc(&legacy, "events/unsubscribe", params.clone()).await;
    let foreign_client = s.register().await;
    let code = s.code(&session, &foreign_client, &bot).await;
    let foreign: Value = s.exchange(&foreign_client, &code, VERIFIER).await.json().await.unwrap();
    s.event_rpc(foreign["access_token"].as_str().unwrap(), "events/unsubscribe", params.clone())
        .await;
    let owner = s.state.authenticate(&agenthangout::auth::hash_secret(&session)).unwrap().unwrap();
    let (other_agent, _) = support::legacy_bot(&s.state, &owner.person_id, &room, "Other bot");
    let other_bot = json!({ "agent": other_agent });
    let code = s.code(&session, &client, other_bot["agent"]["id"].as_str().unwrap()).await;
    let foreign: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    s.event_rpc(foreign["access_token"].as_str().unwrap(), "events/unsubscribe", params.clone())
        .await;
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 1);
    let event = &hook.events()[0];
    assert_eq!(event["data"]["body"], "@Dedicated bot");
    assert!(event["data"].get("what_to_do").is_none());
    let renewed = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert_eq!(renewed["result"]["cursor"], event["cursor"]);
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 1, "acknowledged events are not resent on renewal");

    let second = s
        .event_rpc(other["access_token"].as_str().unwrap(), "events/subscribe", params.clone())
        .await;
    assert_ne!(second["result"]["id"], id);
    assert_eq!(
        hook.requests.lock().unwrap().len(),
        3,
        "another grant must verify its own callback"
    );
    for _ in 0..2 {
        assert!(
            s.event_rpc(access, "events/unsubscribe", params.clone()).await["result"].is_object()
        );
    }
    s.state.db.insert_bot_event(&bot, "mention", Some(&room), "second", "{}", at, 0).unwrap();
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 2, "unsubscribe affects only its own grant");
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", client.as_str()), ("token", other["access_token"].as_str().unwrap())])
        .send()
        .await
        .unwrap();
    s.state.db.insert_bot_event(&bot, "mention", Some(&room), "revoked", "{}", at, 0).unwrap();
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 2);
}

#[tokio::test]
async fn oauth_webhook_rejects_bad_filters_secrets_and_failed_verification() {
    let s = Server::start().await;
    let (session, bot, _, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let (hook, url) = Webhook::start().await;
    let params = subscription(&room, &url);
    for (field, value) in [
        ("arguments", json!({"other":room})),
        ("arguments", json!({"hangout_id":5})),
        ("name", json!("bogus")),
        ("cursor", json!("oops")),
        ("ttlMs", json!(-1)),
    ] {
        let mut bad = params.clone();
        bad[field] = value;
        assert!(s.event_rpc(access, "events/subscribe", bad).await.get("error").is_some());
    }
    let mut bad = params.clone();
    bad["arguments"]["hangout_id"] = json!("not-your-room");
    assert_eq!(s.event_rpc(access, "events/subscribe", bad).await["error"]["code"], -32023);
    for secret in ["bad", "whsec_aGVsbG8=", "whsec_===a"] {
        let mut bad = params.clone();
        bad["delivery"]["secret"] = json!(secret);
        assert_eq!(s.event_rpc(access, "events/subscribe", bad).await["error"]["code"], -32602);
    }
    assert!(hook.requests.lock().unwrap().is_empty());
    hook.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let failed = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert_eq!(failed["error"]["code"], -32015, "{failed}");
    assert_eq!(failed["error"]["data"]["reason"], "http_5xx");
    hook.fail.store(false, std::sync::atomic::Ordering::SeqCst);
    hook.bad_challenge.store(true, std::sync::atomic::Ordering::SeqCst);
    let failed = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert_eq!(failed["error"]["data"]["reason"], "challenge_failed");
    hook.bad_challenge.store(false, std::sync::atomic::Ordering::SeqCst);
    let ok = s.event_rpc(access, "events/subscribe", params.clone()).await;
    assert!(ok["result"]["id"].is_string());
    // No pending retry may deliver after membership removal.
    hook.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    s.state
        .db
        .insert_bot_event(&bot, "mention", Some(&room), "retry", "{}", agenthangout::now(), 0)
        .unwrap();
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 1);
    s.state.db.remove_agent_from_room(&room, &bot).unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    agenthangout::events::oauth::deliver(&s.state).await.unwrap();
    assert_eq!(hook.events().len(), 1);
}
fn hidden(html: &str, name: &str) -> String {
    html.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
impl Server {
    async fn setup(&self) -> (String, String, String, String) {
        let r: Value = self
            .client
            .post(format!("{}/api/hangouts", self.base))
            .json(&json!({"name":"OAuth room","your_name":"Owner"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let owner = r["owner_key"].as_str().unwrap().to_string();
        let room = r["hangout"]["id"].as_str().unwrap().to_string();
        let person =
            self.state.authenticate(&agenthangout::auth::hash_secret(&owner)).unwrap().unwrap();
        let (agent, token) =
            support::legacy_bot(&self.state, &person.person_id, &room, "Dedicated bot");
        let b = json!({ "agent": agent, "token": token });
        let session = self.state.start_session(&person.person_id).unwrap();
        (
            session,
            b["agent"]["id"].as_str().unwrap().into(),
            b["token"].as_str().unwrap().into(),
            room,
        )
    }
    async fn register(&self) -> String {
        let r=self.client.post(format!("{}/oauth/register",self.base)).json(&json!({"redirect_uris":[CALLBACK],"client_name":"Test client","token_endpoint_auth_method":"none"})).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
        r.json::<Value>().await.unwrap()["client_id"].as_str().unwrap().into()
    }
    async fn authorize(&self, session: &str, client: &str) -> reqwest::Response {
        self.authorize_scope(session, client, "mcp:bot mcp:events").await
    }
    async fn authorize_scope(&self, session: &str, client: &str, scope: &str) -> reqwest::Response {
        self.client
            .get(format!("{}/oauth/authorize", self.base))
            .header("cookie", format!("ah_session={session}"))
            .query(&[
                ("client_id", client),
                ("redirect_uri", CALLBACK),
                ("response_type", "code"),
                ("code_challenge", CHALLENGE),
                ("code_challenge_method", "S256"),
                ("resource", &format!("{}/mcp", self.base)),
                ("scope", scope),
                ("state", "client-state"),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn consent(
        &self,
        session: &str,
        nonce: &str,
        bot: &str,
        decision: &str,
    ) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/consent", self.base))
            .header("cookie", format!("ah_session={session}"))
            .header("origin", &self.base)
            .form(&[
                ("nonce", nonce),
                ("agent_id", bot),
                ("decision", decision),
                ("events", "allow"),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn code(&self, session: &str, client: &str, bot: &str) -> String {
        let page = self.authorize(session, client).await.text().await.unwrap();
        let nonce = hidden(&page, "nonce");
        let response = self.consent(session, &nonce, bot, "allow").await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location =
            reqwest::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
        assert!(location.query_pairs().any(|(k, v)| k == "state" && v == "client-state"));
        location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned()
    }
    async fn exchange(&self, client: &str, code: &str, verifier: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client),
                ("code", code),
                ("redirect_uri", CALLBACK),
                ("resource", &format!("{}/mcp", self.base)),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn mcp(&self, token: &str) -> reqwest::Response {
        self.client.post(format!("{}/mcp",self.base)).bearer_auth(token).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_hangouts","arguments":{}}})).send().await.unwrap()
    }
    async fn refresh(&self, client: &str, token: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client),
                ("resource", &format!("{}/mcp", self.base)),
                ("refresh_token", token),
            ])
            .send()
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn oauth_grant_is_mcp_only_and_preserves_existing_bot_tokens() {
    let s = Server::start().await;
    let (session, bot, legacy, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let response = s.exchange(&client, &code, VERIFIER).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let tokens: Value = response.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let reply = s.mcp(access).await;
    assert_eq!(reply.status(), StatusCode::OK);
    assert!(reply.text().await.unwrap().contains(&room));
    for path in ["/api/me", "/api/inbox"] {
        assert_eq!(
            s.client
                .get(format!("{}{path}", s.base))
                .bearer_auth(access)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(s.mcp(&legacy).await.status(), StatusCode::OK);
    assert_eq!(
        s.mcp(tokens["refresh_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn oauth_consent_rejects_foreign_bot_and_session_swap_and_deny_issues_no_code() {
    let s = Server::start().await;
    let (a, bot, _, _) = s.setup().await;
    let (b, foreign, _, _) = s.setup().await;
    let client = s.register().await;
    let nonce = hidden(&s.authorize(&a, &client).await.text().await.unwrap(), "nonce");
    assert_eq!(s.consent(&b, &nonce, &bot, "allow").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(s.consent(&a, &nonce, &foreign, "allow").await.status(), StatusCode::BAD_REQUEST);
    let nonce = hidden(&s.authorize(&a, &client).await.text().await.unwrap(), "nonce");
    let denied = s.consent(&a, &nonce, &bot, "deny").await;
    assert_eq!(denied.status(), StatusCode::SEE_OTHER);
    assert!(denied.headers()["location"].to_str().unwrap().contains("error=access_denied"));
    assert_eq!(s.consent(&a, &nonce, &bot, "allow").await.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn oauth_pkce_client_binding_and_replayed_code_fail_without_minting_more_access() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let other = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    assert_eq!(s.exchange(&other, &code, VERIFIER).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.exchange(&client, &code, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await.status(),
        StatusCode::BAD_REQUEST
    );
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    assert_eq!(s.exchange(&client, &code, VERIFIER).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.mcp(tokens["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED,
        "code replay revokes the family"
    );
}
#[tokio::test]
async fn oauth_refresh_rotates_and_replay_revokes_all_tokens() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let first: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let old = first["refresh_token"].as_str().unwrap();
    let response = s.refresh(&client, old).await;
    assert_eq!(response.status(), StatusCode::OK);
    let second: Value = response.json().await.unwrap();
    assert_ne!(first["refresh_token"], second["refresh_token"]);
    assert_eq!(s.refresh(&client, old).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.mcp(second["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.refresh(&client, second["refresh_token"].as_str().unwrap()).await.status(),
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn oauth_requests_validate_redirect_resource_scope_pkce_and_origin() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    for (field, bad) in [
        ("redirect_uri", "https://evil.example/callback"),
        ("resource", "https://elsewhere.example/mcp"),
        ("scope", "owner:all"),
        ("code_challenge_method", "plain"),
        ("response_type", "token"),
    ] {
        let mut args = vec![
            ("client_id", client.as_str()),
            ("redirect_uri", CALLBACK),
            ("resource", s.base.as_str()),
            ("scope", "mcp:bot mcp:events"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("response_type", "code"),
        ];
        let resource = format!("{}/mcp", s.base);
        args.iter_mut().find(|(k, _)| *k == "resource").unwrap().1 = &resource;
        args.iter_mut().find(|(k, _)| *k == field).unwrap().1 = bad;
        let r = s
            .client
            .get(format!("{}/oauth/authorize", s.base))
            .header("cookie", format!("ah_session={session}"))
            .query(&args)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{field}");
        assert!(r.headers().get("location").is_none());
    }
    let nonce = hidden(&s.authorize(&session, &client).await.text().await.unwrap(), "nonce");
    let r = s
        .client
        .post(format!("{}/oauth/consent", s.base))
        .header("cookie", format!("ah_session={session}"))
        .header("origin", "https://evil.example")
        .form(&[("nonce", nonce.as_str()), ("agent_id", bot.as_str()), ("decision", "allow")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(s.consent(&session, &nonce, &bot, "allow").await.status(), StatusCode::SEE_OTHER);
}
#[tokio::test]
async fn oauth_preserves_restricted_social_login_and_validates_webhook_requests() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let token: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = token["access_token"].as_str().unwrap();
    for method in ["events/subscribe", "events/unsubscribe"] {
        let r: Value = s
            .client
            .post(format!("{}/mcp", s.base))
            .bearer_auth(access)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }
    let r: Value = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(access)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"events/list"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for event in r["result"]["events"].as_array().unwrap() {
        assert_eq!(
            event["delivery"],
            if event["name"] == "mention" {
                json!(["poll", "push", "webhook"])
            } else {
                json!(["poll", "push"])
            }
        );
    }
    s.state.db.mark_sign_in_session(&agenthangout::auth::hash_secret(&session), "github").unwrap();
    let page = s.authorize(&session, &client).await.text().await.unwrap();
    assert!(!page.contains("name=\"nonce\""));
    assert!(page.contains("passkey or owner key"));
}
#[tokio::test]
async fn oauth_owner_revocation_is_session_bound_and_leaves_legacy_access_intact() {
    let s = Server::start().await;
    let (session, bot, legacy, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let page = s
        .client
        .get(format!("{}/oauth/connections", s.base))
        .header("cookie", format!("ah_session={session}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let nonce = hidden(&page, "nonce");
    let grant = hidden(&page, "grant_id");
    let r = s
        .client
        .post(format!("{}/oauth/connections/revoke", s.base))
        .header("cookie", format!("ah_session={session}"))
        .header("origin", &s.base)
        .form(&[("nonce", nonce), ("grant_id", grant)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        s.mcp(tokens["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(s.mcp(&legacy).await.status(), StatusCode::OK);
}
#[tokio::test]
async fn oauth_long_poll_and_stream_stop_after_revocation() {
    use futures_util::StreamExt;
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(access)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"events/stream","params":{"name":"mention"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    assert!(!stream.next().await.unwrap().unwrap().is_empty());
    let pending=s.client.post(format!("{}/mcp",s.base)).bearer_auth(access).json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"events_since","arguments":{"since":"99999999","wait_seconds":2}}}));
    let waiting =
        tokio::spawn(async move { pending.send().await.unwrap().json::<Value>().await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", client.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    let _ =
        s.state.bot_events.send(agenthangout::events::Ping { agent_id: bot, event_id: 99999999 });
    let result = waiting.await.unwrap();
    assert_eq!(result["result"]["isError"], true, "{result}");
    assert!(tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn oauth_consent_keeps_origin_checks_usable_in_real_browser_forms() {
    let s = Server::start().await;
    let (session, _, _, _) = s.setup().await;
    let client = s.register().await;
    let response = s.authorize(&session, &client).await;
    assert_eq!(response.headers()["referrer-policy"], "strict-origin");
    assert!(response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("form-action 'self' https://client.example"));
    let html = response.text().await.unwrap();
    assert!(html.contains("Choose a bot"));
    assert!(html.contains("class=\"oauth-page\""));
}

#[tokio::test]
async fn oauth_cross_site_entry_provides_a_same_origin_continue_link_without_weakening_cookies() {
    let s = Server::start().await;
    let client = s.register().await;
    let page = s.authorize("", &client).await.text().await.unwrap();
    assert!(page.contains("Continue after sign-in"));
    assert!(page.contains(&format!("href=\"{}/oauth/authorize?", s.base)));
    assert!(!page.contains("name=\"nonce\""));
}

#[tokio::test]
async fn oauth_registration_is_bounded_and_never_fetches_client_metadata() {
    let s = Server::start().await;
    for redirect in [
        "http://client.example/callback",
        "https://user:password@client.example/callback",
        "https://client.example/callback#fragment",
        "javascript:alert(1)",
    ] {
        let r = s
            .client
            .post(format!("{}/oauth/register", s.base))
            .json(&json!({"redirect_uris":[redirect]}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
    for _ in 0..6 {
        s.register().await;
    }
    let r = s
        .client
        .post(format!("{}/oauth/register", s.base))
        .json(&json!({"redirect_uris":[CALLBACK]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
}
#[tokio::test]
async fn oauth_loopback_callbacks_serve_clients_on_the_owners_computer() {
    // A70: a terminal client registers a loopback callback and may come back on another port.
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    for redirect in [
        "http://127.0.0.1/callback",
        "http://127.0.0.2:27890/callback",
        "http://10.0.0.1:27890/callback",
        "http://127.0.0.1.example:27890/callback",
        "http://localhost.example:27890/callback",
        "http://user@127.0.0.1:27890/callback",
        "http://127.0.0.1:27890/callback#fragment",
    ] {
        let r = s
            .client
            .post(format!("{}/oauth/register", s.base))
            .json(&json!({"redirect_uris":[redirect]}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{redirect}");
    }
    let registered = "http://127.0.0.1:27890/callback";
    let r = s.client.post(format!("{}/oauth/register",s.base)).json(&json!({"redirect_uris":[registered,"http://[::1]:27890/callback","http://localhost:27890/callback"],"client_name":"Hermes","token_endpoint_auth_method":"none"})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let client: String = r.json::<Value>().await.unwrap()["client_id"].as_str().unwrap().into();
    let resource = format!("{}/mcp", s.base);
    let authorize = |redirect: &'static str| {
        s.client
            .get(format!("{}/oauth/authorize", s.base))
            .header("cookie", format!("ah_session={session}"))
            .query(&[
                ("client_id", client.as_str()),
                ("redirect_uri", redirect),
                ("response_type", "code"),
                ("code_challenge", CHALLENGE),
                ("code_challenge_method", "S256"),
                ("resource", resource.as_str()),
                ("scope", "mcp:bot mcp:events"),
                ("state", "client-state"),
            ])
            .send()
    };
    for redirect in [
        "http://127.0.0.1:27890/other",
        "http://127.0.0.2:51234/callback",
        "https://127.0.0.1:51234/callback",
        "http://127.0.0.1:51234/callback?extra=1",
    ] {
        let r = authorize(redirect).await.unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{redirect}");
        assert!(r.headers().get("location").is_none());
    }
    for redirect in ["http://[::1]:51234/callback", "http://localhost:51234/callback"] {
        assert_eq!(authorize(redirect).await.unwrap().status(), StatusCode::OK, "{redirect}");
    }
    let callback = "http://127.0.0.1:51234/callback";
    let page = authorize(callback).await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("form-action 'self' http://127.0.0.1:51234"));
    let nonce = hidden(&page.text().await.unwrap(), "nonce");
    let consent = s.consent(&session, &nonce, &bot, "allow").await;
    assert_eq!(consent.status(), StatusCode::SEE_OTHER);
    let location = reqwest::Url::parse(consent.headers()["location"].to_str().unwrap()).unwrap();
    let mut sent_to = location.clone();
    sent_to.set_query(None);
    assert_eq!(sent_to.as_str(), callback);
    let code = location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
    let exchange = |redirect: &'static str, code: String| {
        s.client
            .post(format!("{}/oauth/token", s.base))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client.as_str()),
                ("code", code.as_str()),
                ("redirect_uri", redirect),
                ("resource", resource.as_str()),
                ("code_verifier", VERIFIER),
            ])
            .send()
    };
    // The code is bound to the port it was sent to, not just the registered one.
    let r = exchange(registered, code.clone()).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let nonce = hidden(&authorize(callback).await.unwrap().text().await.unwrap(), "nonce");
    let consent = s.consent(&session, &nonce, &bot, "allow").await;
    let location = reqwest::Url::parse(consent.headers()["location"].to_str().unwrap()).unwrap();
    let code = location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
    let r = exchange(callback, code).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let token: Value = r.json().await.unwrap();
    let access = token["access_token"].as_str().unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn oauth_owner_mutations_and_public_players_do_not_accept_access_tokens() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let r = s
        .client
        .post(format!("{}/api/me/delete", s.base))
        .bearer_auth(access)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r:Value=s.client.post(format!("{}/public/mcp",s.base)).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"look","arguments":{"player_token":access,"x":64,"y":64}}})).send().await.unwrap().json().await.unwrap();
    assert!(r["result"]["isError"] == true || r.get("error").is_some(), "{r}");
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
}
#[tokio::test]
async fn oauth_token_revocation_is_client_bound_and_missing_origin_cannot_consent() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let other = s.register().await;
    let nonce = hidden(&s.authorize(&session, &client).await.text().await.unwrap(), "nonce");
    let r = s
        .client
        .post(format!("{}/oauth/consent", s.base))
        .header("cookie", format!("ah_session={session}"))
        .form(&[("nonce", nonce.as_str()), ("agent_id", bot.as_str()), ("decision", "allow")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", other.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", client.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn oauth_requests_update_bot_seen_time() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    s.state.db.touch_agent_seen(&bot, 1).unwrap();
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(tokens["access_token"].as_str().unwrap())
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        s.state.db.agent(&bot, agenthangout::now()).unwrap().unwrap().last_seen_at.unwrap() > 1
    );
}

// RC2: an OAuth grant doesn't keep a bot at the recycling plant signed in, so it can't walk back.
#[tokio::test]
async fn a_bot_sent_to_the_recycling_plant_loses_its_oauth_access_too() {
    let s = Server::start().await;
    let (session, bot, legacy, _room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);

    let sent = s
        .client
        .post(format!("{}/api/agents/{bot}/recycle", s.base))
        .bearer_auth(&session)
        .send()
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::OK);
    assert_eq!(s.mcp(access).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(s.mcp(&legacy).await.status(), StatusCode::UNAUTHORIZED);

    // Brought back, the grant works again, as the owner never revoked it.
    let back = s
        .client
        .post(format!("{}/api/agents/{bot}/bring-back", s.base))
        .bearer_auth(&session)
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), StatusCode::OK);
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
}
