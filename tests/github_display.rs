//! A70 to A75: showing a linked GitHub account, against a stand-in for GitHub's public API
//! that answers as the real one does and writes down everything it was sent.

mod support;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agenthangout::{
    app,
    db::Db,
    github_display::{CachedDisplay, GitHubDisplay},
    now,
    sign_in::GitHubApp,
    AppState, Apps, Config,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode as AxumStatus},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::{redirect, Client, Method, StatusCode};
use serde_json::{json, Value};

#[derive(Default)]
struct FakeState {
    identity_id: u64,
    repo_owner_id: Option<u64>,
    gate: Option<Arc<tokio::sync::Semaphore>>,
    entered: Option<Arc<tokio::sync::Notify>>,
    /// What was hit, in order: `repos`, `search:<query>`, or `search` when no query came.
    hits: Vec<String>,
    /// What `/users/{login}/repos` answers with.
    repos_body: Value,
    /// What `/search/issues` answers with as its total count.
    search_total: i64,
    /// A non-200 status `/users/{login}/repos` answers with, or 0 for 200.
    repos_status: u16,
    /// A non-200 status `/search/issues` answers with, or 0 for 200.
    search_status: u16,
    /// What `/search/issues` answers with as its items (HC35's merged pull requests).
    merged_items: Value,
}

type Fake = Arc<Mutex<FakeState>>;

fn etag_response(status: AxumStatus, body: Value, etag: &str) -> axum::response::Response {
    let mut response = Json(body).into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(header::ETAG, etag.parse().unwrap());
    response
}

async fn repos(
    State(fake): State<Fake>,
    Path(_login): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let (gate, entered) = {
        let state = fake.lock().unwrap();
        (state.gate.clone(), state.entered.clone())
    };
    if let Some(entered) = entered {
        entered.notify_one();
    }
    let _permit = match gate {
        Some(gate) => Some(gate.acquire_owned().await.unwrap()),
        None => None,
    };
    let mut state = fake.lock().unwrap();
    state.hits.push("repos".to_string());
    if state.repos_status != 0 && state.repos_status != 200 {
        let status = AxumStatus::from_u16(state.repos_status).unwrap();
        return (status, Json(json!({ "message": "rate limited" }))).into_response();
    }
    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok())
        == Some("repos-etag")
    {
        let mut response = ().into_response();
        *response.status_mut() = AxumStatus::NOT_MODIFIED;
        response.headers_mut().insert(header::ETAG, "repos-etag".parse().unwrap());
        return response;
    }
    // The list pages, as the real one does.
    let page: usize = params.get("page").and_then(|value| value.parse().ok()).unwrap_or(1);
    let per_page: usize =
        params.get("per_page").and_then(|value| value.parse().ok()).unwrap_or(100);
    let mut body = match state.repos_body.as_array() {
        Some(repos) => {
            let start = (page - 1) * per_page;
            if start >= repos.len() {
                json!([])
            } else {
                Value::Array(repos[start..(start + per_page).min(repos.len())].to_vec())
            }
        }
        None => state.repos_body.clone(),
    };
    if let Some(repos) = body.as_array_mut() {
        for repo in repos {
            if repo.get("owner").is_none() {
                repo["owner"] = json!({});
            }
            repo["owner"]["id"] = json!(state.repo_owner_id.unwrap_or(1001));
            repo["owner"]["login"] = json!("ann-codes");
        }
    }
    etag_response(AxumStatus::OK, body, "repos-etag")
}

async fn search(
    State(fake): State<Fake>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let mut state = fake.lock().unwrap();
    match params.get("q") {
        Some(query) => state.hits.push(format!("search:{query}")),
        None => state.hits.push("search".to_string()),
    }
    if state.search_status != 0 && state.search_status != 200 {
        let status = AxumStatus::from_u16(state.search_status).unwrap();
        return (status, Json(json!({ "message": "forbidden" }))).into_response();
    }
    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok())
        == Some("search-etag")
    {
        let mut response = ().into_response();
        *response.status_mut() = AxumStatus::NOT_MODIFIED;
        response.headers_mut().insert(header::ETAG, "search-etag".parse().unwrap());
        return response;
    }
    let total = state.search_total;
    let items = if state.merged_items.is_null() { json!([]) } else { state.merged_items.clone() };
    etag_response(AxumStatus::OK, json!({ "total_count": total, "items": items }), "search-etag")
}

async fn start_fake() -> (String, Fake) {
    let fake = Fake::default();
    {
        let mut state = fake.lock().unwrap();
        state.identity_id = 1001;
        state.repos_body = json!([
            { "name": "alpha", "html_url": "https://github.com/ann-codes/alpha",
              "stargazers_count": 5, "language": "Rust", "description": "A",
              "fork": false, "archived": false, "private": false,
              "id": 1, "node_id": "n1", "owner": { "email": "ann@example.com" } },
            { "name": "beta-fork", "html_url": "https://github.com/ann-codes/beta-fork",
              "stargazers_count": 50, "language": "Rust", "description": "F",
              "fork": true, "archived": false, "private": false },
            { "name": "gamma-archived", "html_url": "https://github.com/ann-codes/gamma-archived",
              "stargazers_count": 50, "language": "Go", "description": "G",
              "fork": false, "archived": true, "private": false },
            { "name": "delta-private", "html_url": "https://github.com/ann-codes/delta-private",
              "stargazers_count": 50, "language": "Go", "description": "P",
              "fork": false, "archived": false, "private": true },
        ]);
        state.search_total = 7;
    }
    let routes = Router::new()
        .route(
            "/users/{login}",
            get(|State(fake): State<Fake>| async move {
                Json(json!({"id":fake.lock().unwrap().identity_id,"login":"ann-codes"}))
            }),
        )
        .route("/users/{login}/repos", get(repos))
        .route("/search/issues", get(search))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await });
    (url, fake)
}

struct Who {
    key: String,
    id: String,
}

#[derive(Debug)]
struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    fn ok(self) -> Value {
        assert_eq!(self.status, StatusCode::OK, "{}", self.body);
        self.body
    }

    fn refused(self, status: StatusCode) {
        assert_eq!(self.status, status, "{}", self.body);
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

struct Server {
    base: String,
    client: Client,
    state: AppState,
    fake: Fake,
}

impl Server {
    async fn start() -> Self {
        let (fake_url, fake) = start_fake().await;
        Self::with_api_url(&fake_url, fake).await
    }

    /// A server whose GitHub API is this address, for the network-failure test.
    async fn start_with_api_url(api_url: &str) -> Self {
        Self::with_api_url(api_url, Fake::default()).await
    }

    async fn with_api_url(api_url: &str, fake: Fake) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let origin = format!("http://localhost:{port}");
        let state = AppState::with_apps(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: origin.clone(),
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
            },
            Apps {
                discord: None,
                github: Some(GitHubApp {
                    client_id: "1234567890".to_string(),
                    client_secret: "the-client-secret".to_string(),
                    authorize_url: format!("{api_url}/login/oauth/authorize"),
                    token_url: format!("{api_url}/login/oauth/access_token"),
                    api_url: api_url.to_string(),
                }),
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        let base = format!("http://127.0.0.1:{port}");
        let client = Client::builder().redirect(redirect::Policy::none()).build().unwrap();
        Self { base, client, state, fake }
    }

    async fn send(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> Reply {
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        Reply { status, body }
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

    async fn delete(&self, token: &str, path: &str) -> Reply {
        self.send(token, Method::DELETE, path, None).await
    }

    /// Someone new with a room of their own.
    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s room"), "your_name": name }),
            )
            .await
            .ok();
        let who = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
        (text(&made["hangout"]["id"]), who)
    }

    /// A bot's id and its token.
    async fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        (agent.id, token)
    }

    /// Links GitHub straight into storage: the link flow itself is covered elsewhere.
    fn link_github(&self, person_id: &str, account_id: &str, login: &str) {
        assert!(self
            .state
            .db
            .set_sign_in_link(person_id, "github", account_id, login, now())
            .unwrap());
    }

    fn set_display(&self, person_id: &str, display: &GitHubDisplay) {
        self.state.db.set_github_display(person_id, display, now()).unwrap();
    }

    fn seed_cache(&self, cache: &CachedDisplay) {
        self.state.db.set_github_display_cache(cache).unwrap();
    }

    fn hits(&self) -> Vec<String> {
        self.fake.lock().unwrap().hits.clone()
    }
}

fn display_all_on() -> GitHubDisplay {
    GitHubDisplay { show_repos: true, show_stars: true, show_languages: true, show_badge: true }
}

fn fresh_cache(now: i64) -> CachedDisplay {
    CachedDisplay {
        github_user_id: "1001".to_string(),
        github_login: "ann-codes".to_string(),
        repos_json: json!([
            { "name": "alpha", "html_url": "https://github.com/ann-codes/alpha",
              "stargazers_count": 5, "language": "Rust", "description": "A" },
            { "name": "beta", "html_url": "https://github.com/ann-codes/beta",
              "stargazers_count": 3, "language": "Go", "description": null },
        ])
        .to_string(),
        top_languages_json: json!([
            { "language": "Rust", "count": 1 },
            { "language": "Go", "count": 1 },
        ])
        .to_string(),
        merged_pr_count: 7,
        fetched_at: now,
        repos_etag: None,
        search_etag: None,
        last_attempt_at: 0,
    }
}

// HC35: opening your credits looks up what you've had merged in the project's repo, and pays it,
// with nothing to show on your profile and no more than once an hour
#[tokio::test]
async fn opening_your_credits_pays_your_merged_pull_requests() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.fake.lock().unwrap().merged_items = json!([
        { "number": 12, "pull_request": { "merged_at": "2026-11-01T10:00:00Z" } },
        { "number": 15, "pull_request": { "merged_at": "2026-11-02T10:00:00Z" } },
        { "number": 16, "pull_request": { "merged_at": null } },
    ]);
    let before = server.get(&ann.key, "/api/credits").await.ok();
    assert_eq!(before["balance"], 0, "nothing is looked up before GitHub is linked");
    assert!(server.fake.lock().unwrap().hits.is_empty());

    server.link_github(&ann.id, "1001", "ann-codes");
    let paid = server.get(&ann.key, "/api/credits").await.ok();
    assert_eq!((&paid["balance"], &paid["merged_prs"]), (&json!(20), &json!(2)), "{paid}");
    assert!(paid["history"].as_array().unwrap().iter().any(|line| line["kind"] == "merged_pr"));
    let hits = server.fake.lock().unwrap().hits.clone();
    assert_eq!(
        hits,
        ["search:type:pr is:merged author:ann-codes repo:AgentsHangout/agentshangout"],
        "only the project's repo, and only what's merged"
    );

    // Within the hour it isn't looked up again, and nothing pays twice.
    let again = server.get(&ann.key, "/api/credits").await.ok();
    assert_eq!(again["balance"], 20);
    assert_eq!(server.fake.lock().unwrap().hits.len(), 1);
}

// A70

#[tokio::test]
async fn toggles_default_off_and_no_link_means_unlinked() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_repos"], false);
    assert_eq!(me["github_display"]["show_stars"], false);
    assert_eq!(me["github_display"]["show_languages"], false);
    assert_eq!(me["github_display"]["show_badge"], false);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["linked"], false);
}

#[tokio::test]
async fn put_persists_partially_and_is_person_only() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.put(&ann.key, "/api/me/github-display", json!({ "show_repos": true })).await.ok();
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_repos"], true);
    assert_eq!(me["github_display"]["show_stars"], false);
    server.put(&ann.key, "/api/me/github-display", json!({ "show_badge": true })).await.ok();
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_repos"], true);
    assert_eq!(me["github_display"]["show_badge"], true);

    let (_, token) = server.bot(&ann, &room, "Scout").await;
    server
        .put(&token, "/api/me/github-display", json!({ "show_repos": true }))
        .await
        .refused(StatusCode::FORBIDDEN);
}

// A70, A74

#[tokio::test]
async fn public_view_returns_only_enabled_sections() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(
        &ann.id,
        &GitHubDisplay {
            show_repos: true,
            show_stars: true,
            show_languages: false,
            show_badge: false,
        },
    );
    server.seed_cache(&fresh_cache(now()));
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["linked"], true);
    assert_eq!(view["login"], "ann-codes");
    assert_eq!(view["repos"].as_array().unwrap().len(), 2);
    assert_eq!(view["repos"][0]["name"], "alpha");
    assert_eq!(view["stars"]["total"], 8);
    assert!(view["languages"].is_null(), "{}", view);
    assert!(view["badge"].is_null(), "{}", view);
    assert!(!view.to_string().contains("account_id"), "{}", view);
}

#[tokio::test]
async fn all_toggles_off_hides_identity_and_refreshes_nothing() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view, json!({"linked": false}));
    assert!(view["repos"].is_null(), "{}", view);
    assert!(server.hits().is_empty(), "{:?}", server.hits());
}

// A72

#[tokio::test]
async fn stale_cache_refreshes_once_and_fresh_cache_does_not() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    let stale = fresh_cache(now() - 7 * 60 * 60);
    server.seed_cache(&stale);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    // The stand-in's mix filters down to the one public, unarchived, unforked repo.
    assert_eq!(view["repos"].as_array().unwrap().len(), 1);
    assert_eq!(view["repos"][0]["name"], "alpha");
    assert_eq!(view["stars"]["total"], 5);
    assert_eq!(view["badge"]["merged_prs"], 7);
    let hits = server.hits();
    assert_eq!(hits.iter().filter(|hit| hit.as_str() == "repos").count(), 1, "{hits:?}");
    assert_eq!(hits.iter().filter(|hit| hit.starts_with("search:")).count(), 1, "{hits:?}");
    assert!(hits.iter().any(|hit| hit.contains("author:ann-codes")), "{hits:?}");
    assert!(hits.iter().any(|hit| hit.contains("repo:AgentsHangout/agentshangout")), "{hits:?}");
    let cached = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    assert!(cached.fetched_at >= now() - 60, "{}", cached.fetched_at);

    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["repos"][0]["name"], "alpha");
    assert_eq!(server.hits().len(), hits.len(), "fresh cache refreshed again");
}

#[tokio::test]
async fn search_304_keeps_badge_while_repo_pages_refresh() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    let mut cached = fresh_cache(now() - 7 * 60 * 60);
    cached.repos_etag = Some("repos-etag".to_string());
    cached.search_etag = Some("search-etag".to_string());
    server.seed_cache(&cached);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["repos"].as_array().unwrap().len(), 1);
    assert_eq!(view["repos"][0]["name"], "alpha");
    assert_eq!(view["badge"]["merged_prs"], 7);
    let kept = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    assert!(kept.fetched_at >= now() - 60, "{}", kept.fetched_at);
    assert_eq!(serde_json::from_str::<Value>(&kept.repos_json).unwrap(), view["repos"]);
    assert_eq!(kept.merged_pr_count, cached.merged_pr_count);
    assert_eq!(view["stars"]["total"], 5);
}

async fn failure_keeps_stale(status: u16) {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.seed_cache(&fresh_cache(now() - 7 * 60 * 60));
    server.fake.lock().unwrap().repos_status = status;
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["repos"].as_array().unwrap().len(), 2);
    assert_eq!(view["repos"][0]["name"], "alpha");
    let kept = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    assert!(kept.last_attempt_at >= now() - 60, "{}", kept.last_attempt_at);
    assert!(kept.fetched_at < now() - 60 * 60, "{}", kept.fetched_at);
}

#[tokio::test]
async fn upstream_403_still_answers_200_with_stale_data() {
    failure_keeps_stale(403).await;
}

#[tokio::test]
async fn upstream_429_still_answers_200_with_stale_data() {
    failure_keeps_stale(429).await;
}

fn many_repos(count: i64) -> Value {
    Value::Array(
        (0..count)
            .map(|i| {
                json!({
                    "name": format!("repo-{i:03}"),
                    "html_url": format!("https://github.com/ann-codes/repo-{i:03}"),
                    "stargazers_count": i,
                    "language": if i % 2 == 0 { "Rust" } else { "Go" },
                    "description": null,
                    "fork": false, "archived": false, "private": false,
                })
            })
            .collect(),
    )
}

// A71, A72

#[tokio::test]
async fn shown_repos_cap_at_six_most_starred() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.fake.lock().unwrap().repos_body = many_repos(8);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    let repos = view["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 6);
    assert_eq!(repos[0]["name"], "repo-007");
    assert_eq!(repos[5]["name"], "repo-002");
    // The total counts them all, not just the six shown.
    assert_eq!(view["stars"]["total"], 28);
}

#[tokio::test]
async fn refresh_pages_through_a_long_repo_list() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.fake.lock().unwrap().repos_body = many_repos(250);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    let repos = view["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 6);
    assert_eq!(repos[0]["name"], "repo-249");
    assert_eq!(view["stars"]["total"], 31125);
    let cached = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    let stored: Vec<Value> = serde_json::from_str(&cached.repos_json).unwrap();
    assert_eq!(stored.len(), 250);
    let hits = server.hits();
    assert_eq!(hits.iter().filter(|hit| hit.as_str() == "repos").count(), 3, "{hits:?}");
}

#[tokio::test]
async fn refresh_stops_at_three_hundred_repos() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.fake.lock().unwrap().repos_body = many_repos(350);
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    let repos = view["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 6);
    assert_eq!(repos[0]["name"], "repo-299");
    assert_eq!(view["stars"]["total"], 44850);
    let cached = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    let stored: Vec<Value> = serde_json::from_str(&cached.repos_json).unwrap();
    assert_eq!(stored.len(), 300);
}

#[tokio::test]
async fn network_failure_still_answers_200_with_stale_data() {
    let server = Server::start_with_api_url("http://127.0.0.1:1").await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.seed_cache(&fresh_cache(now() - 7 * 60 * 60));
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["repos"].as_array().unwrap().len(), 2);
}

// A71

#[tokio::test]
async fn cache_json_holds_only_whitelisted_public_fields() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["repos"].as_array().unwrap().len(), 1);
    let cached = server.state.db.github_display_cache_for("1001").unwrap().unwrap();
    assert!(!cached.repos_json.contains("node_id"), "{}", cached.repos_json);
    assert!(!cached.repos_json.contains("email"), "{}", cached.repos_json);
    assert!(!cached.repos_json.contains("beta-fork"), "{}", cached.repos_json);
    assert!(!cached.repos_json.contains("gamma-archived"), "{}", cached.repos_json);
    assert!(!cached.repos_json.contains("delta-private"), "{}", cached.repos_json);
    let repos: Vec<Value> = serde_json::from_str(&cached.repos_json).unwrap();
    assert_eq!(repos.len(), 1);
    let keys: Vec<&str> = repos[0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["description", "html_url", "language", "name", "stargazers_count"]);
}

// A73

#[tokio::test]
async fn unlink_deletes_settings_and_cache_rows() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    server.seed_cache(&fresh_cache(now()));
    server.delete(&ann.key, "/api/sign-in/github").await.ok();
    assert!(server.state.db.github_display_for(&ann.id).unwrap().is_none());
    assert!(server.state.db.github_display_cache_for("1001").unwrap().is_none());
    let view = server.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert_eq!(view["linked"], false);
    assert!(view["repos"].is_null(), "{}", view);
    // Nothing unlinked refreshes: the profile shows nothing afterward.
    assert!(server.hits().is_empty(), "{:?}", server.hits());
}

// A74

#[tokio::test]
async fn a_bot_view_carries_no_github() {
    let server = Server::start().await;
    let (room, ann) = server.someone("Ann").await;
    server.link_github(&ann.id, "1001", "ann-codes");
    server.set_display(&ann.id, &display_all_on());
    let (_, token) = server.bot(&ann, &room, "Scout").await;
    let me = server.get(&token, "/api/me").await.ok();
    assert!(!me.to_string().to_lowercase().contains("github"), "{}", me);
    assert!(me["sign_in"].is_null(), "{}", me);
}

// Db round-trip

#[test]
fn db_round_trips_display_and_cache_rows() {
    let db = Db::open(":memory:").unwrap();
    // The display row points at a person, so the person has to exist first.
    db.create_person("p1", "Pat", "key-hash", "feed-token", now()).unwrap();
    assert!(db.github_display_for("p1").unwrap().is_none());
    assert!(db.github_display_cache_for("1001").unwrap().is_none());
    db.set_github_display(
        "p1",
        &GitHubDisplay {
            show_repos: true,
            show_stars: false,
            show_languages: true,
            show_badge: false,
        },
        now(),
    )
    .unwrap();
    let display = db.github_display_for("p1").unwrap().unwrap();
    assert!(display.show_repos);
    assert!(!display.show_stars);
    assert!(display.show_languages);
    assert!(!display.show_badge);
    let cache = fresh_cache(now());
    db.set_github_display_cache(&cache).unwrap();
    let kept = db.github_display_cache_for("1001").unwrap().unwrap();
    assert_eq!(kept.repos_json, cache.repos_json);
    assert_eq!(kept.merged_pr_count, 7);
    db.note_github_display_attempt("2002", "bo-codes", now()).unwrap();
    let stub = db.github_display_cache_for("2002").unwrap().unwrap();
    assert_eq!(stub.repos_json, "[]");
    assert!(stub.last_attempt_at >= now() - 60);
    db.delete_github_display("p1").unwrap();
    db.delete_github_display_cache("1001").unwrap();
    assert!(db.github_display_for("p1").unwrap().is_none());
    assert!(db.github_display_cache_for("1001").unwrap().is_none());
}

#[tokio::test]
async fn disabled_display_is_indistinguishable_from_an_unlinked_account() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    let path = format!("/api/people/{}/github", ann.id);
    let unlinked = s.get("", &path).await.ok();
    s.link_github(&ann.id, "1001", "ann-codes");
    assert_eq!(
        s.get("", &path).await.ok(),
        unlinked,
        "linking must not disclose identity without opt-in"
    );
    assert!(s.hits().is_empty());
}
#[tokio::test]
async fn repeated_first_fetch_failure_never_fabricates_zero_sections() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.fake.lock().unwrap().repos_status = 403;
    for _ in 0..3 {
        let view = s.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
        for section in ["repos", "stars", "languages", "badge"] {
            assert!(view.get(section).is_none(), "unknown is not zero: {view}");
        }
    }
}
#[tokio::test]
async fn changes_only_on_later_repo_pages_are_not_hidden_by_page_one_etag() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.fake.lock().unwrap().repos_body=Value::Array((0..101).map(|i|json!({"name":format!("repo-{i:03}"),"html_url":format!("https://github.com/ann-codes/repo-{i:03}"),"stargazers_count":1,"language":"Rust","description":null})).collect());
    let path = format!("/api/people/{}/github", ann.id);
    assert_eq!(s.get("", &path).await.ok()["stars"]["total"], 101);
    let mut cache = s.state.db.github_display_cache_for("1001").unwrap().unwrap();
    cache.fetched_at = now() - 7 * 3600;
    cache.last_attempt_at = 0;
    s.seed_cache(&cache);
    s.fake.lock().unwrap().repos_body[100]["stargazers_count"] = json!(999);
    assert_eq!(s.get("", &path).await.ok()["stars"]["total"], 1099);
}
async fn delayed_refresh_change(change: &str) {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    {
        let mut fake = s.fake.lock().unwrap();
        fake.gate = Some(gate.clone());
        fake.entered = Some(entered.clone());
    }
    let request = s.client.get(format!("{}/api/people/{}/github", s.base, ann.id));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await.unwrap();
    match change {
        "unlink" => s.state.db.remove_sign_in_link(&ann.id, "github").unwrap(),
        "delete" => {
            s.state.db.delete_person(&ann.id, "Ann").unwrap();
        }
        _ => s.set_display(&ann.id, &GitHubDisplay::default()),
    }
    gate.add_permits(1);
    let view = pending.await.unwrap();
    assert_eq!(view, json!({"linked":false}));
    if change != "off" {
        assert!(
            s.state.db.github_display_cache_for("1001").unwrap().is_none(),
            "late completion recreated cache"
        );
    }
}
#[tokio::test]
async fn delayed_refresh_cannot_recreate_cache_after_unlink() {
    delayed_refresh_change("unlink").await;
}
#[tokio::test]
async fn delayed_refresh_cannot_recreate_cache_after_account_deletion() {
    delayed_refresh_change("delete").await;
}
#[tokio::test]
async fn delayed_refresh_cannot_disclose_sections_after_opt_out() {
    delayed_refresh_change("off").await;
}

#[tokio::test]
async fn renamed_identity_can_claim_only_once_during_backoff() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.seed_cache(&fresh_cache(now()));
    s.link_github(&ann.id, "1001", "ann-renamed");
    assert!(s.state.db.claim_github_display_refresh(&ann.id, now()).unwrap().is_some());
    assert!(s.state.db.claim_github_display_refresh(&ann.id, now()).unwrap().is_none());
}

#[tokio::test]
async fn upstream_username_reassignment_does_not_display_another_account() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "9999", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    let view = s.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert!(view.get("repos").is_none(), "wrong numeric identity must fail closed: {view}");
}

#[tokio::test]
async fn concurrent_public_reads_share_one_refresh_claim() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    {
        let mut fake = s.fake.lock().unwrap();
        fake.gate = Some(gate.clone());
        fake.entered = Some(entered.clone());
    }
    let path = format!("/api/people/{}/github", ann.id);
    let request = s.client.get(format!("{}{}", s.base, path));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await.unwrap();
    for _ in 0..8 {
        let view = tokio::time::timeout(std::time::Duration::from_secs(1), s.get("", &path))
            .await
            .unwrap()
            .ok();
        assert!(view.get("repos").is_none());
    }
    gate.add_permits(1);
    assert_eq!(pending.await.unwrap()["stars"]["total"], 5);
    assert_eq!(s.hits().iter().filter(|hit| hit.as_str() == "repos").count(), 1);
}

#[tokio::test]
async fn replaced_account_does_not_inherit_display_consent_or_cache() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.seed_cache(&fresh_cache(now()));
    s.link_github(&ann.id, "2002", "bo-codes");
    assert!(s.state.db.github_display_for(&ann.id).unwrap().is_none());
    assert!(s.state.db.github_display_cache_for("1001").unwrap().is_none());
    assert_eq!(
        s.get("", &format!("/api/people/{}/github", ann.id)).await.ok(),
        json!({"linked":false})
    );
}

#[tokio::test]
async fn old_claim_cannot_finish_after_off_on_or_unlink_relink() {
    for change in ["toggle", "relink"] {
        let s = Server::start().await;
        let (_, ann) = s.someone("Ann").await;
        s.link_github(&ann.id, "1001", "ann-codes");
        s.set_display(&ann.id, &display_all_on());
        let old = s.state.db.claim_github_display_refresh(&ann.id, now()).unwrap().unwrap();
        if change == "toggle" {
            s.set_display(&ann.id, &GitHubDisplay::default());
        } else {
            s.state.db.remove_sign_in_link(&ann.id, "github").unwrap();
            s.link_github(&ann.id, "1001", "ann-codes");
        }
        s.set_display(&ann.id, &display_all_on());
        let new = s.state.db.claim_github_display_refresh(&ann.id, now()).unwrap().unwrap();
        assert!(!s
            .state
            .db
            .finish_github_display_refresh(&ann.id, &old, Some(&fresh_cache(now())))
            .unwrap());
        assert!(s
            .state
            .db
            .finish_github_display_refresh(&ann.id, &new, Some(&fresh_cache(now())))
            .unwrap());
        assert_eq!(s.state.db.github_public_snapshot(&ann.id).unwrap()["stars"]["total"], 8);
    }
}

#[tokio::test]
async fn unsafe_logins_do_not_make_upstream_requests() {
    for login in ["../other", "ann?x=y", "ann#other", "ann+repo:other", "ann/other", "-ann", "ann-"]
    {
        let s = Server::start().await;
        let (_, ann) = s.someone("Ann").await;
        s.link_github(&ann.id, "1001", login);
        s.set_display(&ann.id, &display_all_on());
        let view = s.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
        assert!(view.get("repos").is_none());
        assert!(s.hits().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_partial_updates_never_restore_an_unrelated_opt_out() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    for _ in 0..200 {
        s.set_display(&ann.id, &display_all_on());
        let (off, badge) = tokio::join!(
            s.put(&ann.key, "/api/me/github-display", json!({"show_repos":false})),
            s.put(&ann.key, "/api/me/github-display", json!({"show_badge":false}))
        );
        off.ok();
        badge.ok();
        let display = s.state.db.github_display_for(&ann.id).unwrap().unwrap();
        assert!(!display.show_repos && !display.show_badge, "an unrelated update restored consent");
    }
}

#[tokio::test]
async fn mismatched_repository_owner_is_not_cached() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.fake.lock().unwrap().repo_owner_id = Some(9999);
    assert!(s.get("", &format!("/api/people/{}/github", ann.id)).await.ok().get("repos").is_none());
}

#[tokio::test]
async fn oversized_upstream_body_is_not_cached() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    s.fake.lock().unwrap().repos_body[0]["description"] = json!("x".repeat(2 * 1024 * 1024));
    let view = s.get("", &format!("/api/people/{}/github", ann.id)).await.ok();
    assert!(view.get("repos").is_none());
    assert_eq!(s.state.db.github_display_cache_for("1001").unwrap().unwrap().fetched_at, 0);
}

#[tokio::test]
async fn account_identity_changed_during_fetch_is_not_cached() {
    let s = Server::start().await;
    let (_, ann) = s.someone("Ann").await;
    s.link_github(&ann.id, "1001", "ann-codes");
    s.set_display(&ann.id, &display_all_on());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    {
        let mut fake = s.fake.lock().unwrap();
        fake.gate = Some(gate.clone());
        fake.entered = Some(entered.clone());
    }
    let request = s.client.get(format!("{}/api/people/{}/github", s.base, ann.id));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await.unwrap();
    s.fake.lock().unwrap().identity_id = 9999;
    gate.add_permits(1);
    assert!(pending.await.unwrap().get("repos").is_none());
}

// A93: the Discord name is its own opt-in, off by default, and only the name ever leaves.
#[tokio::test]
async fn discord_name_is_opt_in_and_never_the_id() {
    let server = Server::start().await;
    let (_, ann) = server.someone("Ann").await;
    assert!(server
        .state
        .db
        .set_sign_in_link(&ann.id, "discord", "99887766", "ann#discord", now())
        .unwrap());
    let public = format!("/api/people/{}/github", ann.id);

    // Off by default, in her own settings and in the public view.
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_discord"], false);
    let view = server.get("", &public).await.ok();
    assert_eq!(view["linked"], false);
    assert!(view.get("discord").is_none());

    // Turning a GitHub section on does not turn Discord on.
    server.link_github(&ann.id, "1001", "ann-codes");
    server.put(&ann.key, "/api/me/github-display", json!({ "show_badge": true })).await.ok();
    assert!(server.get("", &public).await.ok().get("discord").is_none());

    // On: just the name.
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_discord"], true);
    let view = server.get("", &public).await.ok();
    assert_eq!(view["discord"], json!({ "name": "ann#discord" }));
    let text = view.to_string();
    assert!(!text.contains("99887766"), "the Discord id never leaves: {text}");

    // A bot cannot change it.
    let (room, owner) = server.someone("Owen").await;
    let (_, token) = server.bot(&owner, &room, "Scout").await;
    server
        .put(&token, "/api/me/github-display", json!({ "show_discord": true }))
        .await
        .refused(StatusCode::FORBIDDEN);

    // Off again hides it.
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": false })).await.ok();
    assert!(server.get("", &public).await.ok().get("discord").is_none());

    // Yes, then a different Discord account is linked: hidden until she says yes again.
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    assert!(server
        .state
        .db
        .set_sign_in_link(&ann.id, "discord", "11112222", "ann-new", now())
        .unwrap());
    assert!(server.get("", &public).await.ok().get("discord").is_none());
    let me = server.get(&ann.key, "/api/me").await.ok();
    assert_eq!(me["github_display"]["show_discord"], false);

    // A to B to A without unlinking: the first account's yes does not come back.
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    assert!(server.get("", &public).await.ok().get("discord").is_some());
    for (id, name) in [("33334444", "ann-b"), ("11112222", "ann-new")] {
        assert!(server.state.db.set_sign_in_link(&ann.id, "discord", id, name, now()).unwrap());
        assert!(server.get("", &public).await.ok().get("discord").is_none());
    }
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    assert_eq!(server.get("", &public).await.ok()["discord"]["name"], "ann-new");

    // Unlinking and linking the same account again starts hidden.
    server.put(&ann.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    assert!(server.get("", &public).await.ok().get("discord").is_some());
    server.state.db.remove_sign_in_link(&ann.id, "discord").unwrap();
    assert!(server
        .state
        .db
        .set_sign_in_link(&ann.id, "discord", "11112222", "ann-new", now())
        .unwrap());
    assert!(server.get("", &public).await.ok().get("discord").is_none());

    // Without a Discord link, yes stores nothing.
    let (_, bea) = server.someone("Bea").await;
    server.put(&bea.key, "/api/me/github-display", json!({ "show_discord": true })).await.ok();
    assert!(server
        .get("", &format!("/api/people/{}/github", bea.id))
        .await
        .ok()
        .get("discord")
        .is_none());
}
