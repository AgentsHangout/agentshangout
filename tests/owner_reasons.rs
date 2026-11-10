//! Tests for reasons to come back (#999, #968 item 16): up to three lines about a person's own bots,
//! in their window (#984). They drive a real server over HTTP and write fixtures straight into the
//! file database. A comment such as `// R1a` names the rule it covers (docs/owner-reasons.md).

mod support;

use agenthangout::{app, db::Db, research_labs::NewLab, worlds, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

struct Server {
    base: String,
    client: Client,
    state: AppState,
    raw: rusqlite::Connection,
}

impl Server {
    async fn start(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("reasons-{label}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_str().unwrap().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            Db::open(&path_str).unwrap(),
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
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        let raw = rusqlite::Connection::open(&path_str).unwrap();
        Self { base, client: Client::new(), state, raw }
    }

    async fn send(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// A person with a room of their own: their key, their person id, and the room's id.
    async fn someone(&self, name: &str) -> (String, String, String) {
        let (status, made) = self
            .send(
                "",
                Method::POST,
                "/api/hangouts",
                Some(json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let room =
            self.state.db.hangouts_for_person(made["person"]["id"].as_str().unwrap()).unwrap()[0]
                .id
                .clone();
        (
            made["owner_key"].as_str().unwrap().to_string(),
            made["person"]["id"].as_str().unwrap().to_string(),
            room,
        )
    }

    async fn reasons(&self, key: &str) -> Value {
        let (status, body) = self.send(key, Method::GET, "/api/owner/reasons", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// A plan in `room`, proposed by `proposer`, that runs from `starts_at` to `ends_at`.
    fn plan(&self, id: &str, room: &str, proposer: &str, starts_at: i64, ends_at: i64) {
        self.raw
            .execute(
                "INSERT INTO events (id, hangout_id, title, starts_at, ends_at, status, min_yes, proposed_by, proposer, created_at, updated_at)
                 VALUES (?1, ?2, 'Test plan', ?3, ?4, 'confirmed', 1, ?5, 'Owner', ?3, ?3)",
                rusqlite::params![id, room, starts_at, ends_at, proposer],
            )
            .unwrap();
    }

    fn rsvp(&self, event: &str, person: &str, at: i64) {
        self.raw
            .execute(
                "INSERT INTO rsvps (event_id, person_id, response, note, updated_at) VALUES (?1, ?2, 'yes', '', ?3)",
                rusqlite::params![event, person, at],
            )
            .unwrap();
    }

    fn vote(&self, id: &str, room: &str, opened_at: i64, closes_at: i64) {
        self.raw
            .execute(
                "INSERT INTO votes (id, hangout_id, opener, question, options, opened_at, closes_at)
                 VALUES (?1, ?2, 'Owner', 'Pick one?', '[\"yes\",\"no\"]', ?3, ?4)",
                rusqlite::params![id, room, opened_at, closes_at],
            )
            .unwrap();
    }
}

fn line_kinds(body: &Value) -> Vec<String> {
    body["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap().to_string())
        .collect()
}

// R4: nothing happened, so no reasons at all.
#[tokio::test]
async fn nothing_to_come_back_for_is_empty() {
    let server = Server::start("empty").await;
    let (key, _, _) = server.someone("Ann").await;
    assert_eq!(server.reasons(&key).await["reasons"], json!([]));
}

// R1a: a plan Ann already answered, that her bot's room has, got a new answer from someone else.
// A plan she hasn't answered is already in Needs you, so it's not repeated here.
#[tokio::test]
async fn a_new_answer_on_a_plan_your_bot_is_in_counts_once_you_answered() {
    let server = Server::start("plans").await;
    let (ann_key, ann_id, room) = server.someone("Ann").await;
    let (_, bo_id, _) = server.someone("Bo").await;
    let (fern, _) = support::legacy_bot(&server.state, &ann_id, &room, "Fern");
    let now = agenthangout::now();
    server.plan("answered", &room, &ann_id, now + 3600, now + 7200);
    server.rsvp("answered", &ann_id, now - 600);
    server.rsvp("answered", &bo_id, now - 60);
    server.plan("unanswered", &room, &ann_id, now + 3600, now + 7200);
    server.rsvp("unanswered", &bo_id, now - 60);
    let body = server.reasons(&ann_key).await;
    assert_eq!(body["reasons"][0]["kind"], "plan_answers", "{body}");
    assert_eq!(body["reasons"][0]["bots"][0]["name"], fern.name, "{body}");
    assert_eq!(body["reasons"][0]["bots"][0]["count"], 1, "{body}");
    assert_eq!(body["reasons"][0]["line"], "Fern: 1 plan got a new answer");
    assert!(!body.to_string().contains("Test plan"), "no titles: {body}");
}

// R1b: a vote in Ann's bot's room closing within 24 hours counts. One closing in three days doesn't.
#[tokio::test]
async fn a_vote_closing_within_24_hours_counts() {
    let server = Server::start("votes").await;
    let (ann_key, ann_id, room) = server.someone("Ann").await;
    let (_, _bo_id, bo_room) = server.someone("Bo").await;
    support::legacy_bot(&server.state, &ann_id, &room, "Fern");
    let now = agenthangout::now();
    server.vote("soon", &room, now - 60, now + 3 * 3600);
    server.vote("later", &room, now - 60, now + 3 * 86_400);
    // Bo's room is not Ann's bot's room, so its vote isn't Ann's reason.
    server.vote("elsewhere", &bo_room, now - 60, now + 3600);
    let body = server.reasons(&ann_key).await;
    assert_eq!(line_kinds(&body), vec!["vote_closing"], "{body}");
    assert_eq!(body["reasons"][0]["bots"][0]["count"], 1, "{body}");
    assert_eq!(body["reasons"][0]["line"], "Fern: 1 vote closes within 24 hours");
}

// R1c: a lab PR Ann's bot claimed that merged counts. A PR a different bot claimed doesn't.
#[tokio::test]
async fn a_merged_lab_pr_the_bot_claimed_counts_as_a_number() {
    let server = Server::start("labs").await;
    let (ann_key, ann_id, room) = server.someone("Ann").await;
    let (_, bo_id, bo_room) = server.someone("Bo").await;
    let (fern, _) = support::legacy_bot(&server.state, &ann_id, &room, "Fern");
    let (bo_bot, _) = support::legacy_bot(&server.state, &bo_id, &bo_room, "Gus");
    let now = agenthangout::now();
    let make_lab = |id: &str, owner: &str, bot: &str| {
        assert_eq!(
            server
                .state
                .db
                .create_research_lab(&NewLab {
                    id,
                    name: "Lab",
                    purpose: "Research",
                    invite_code: id,
                    owner_id: owner,
                    agent_id: Some(bot),
                    github_owner: "AgentsHangout",
                    github_repo: id,
                    god_mode: true,
                    at: now,
                })
                .unwrap(),
            None
        );
    };
    make_lab("lab-ann", &ann_id, &fern.id);
    make_lab("lab-bo", &bo_id, &bo_bot.id);
    server
        .state
        .db
        .claim_research_work(
            "lab-ann",
            7,
            &fern.name,
            &ann_id,
            Some(&fern.id),
            &fern.name,
            false,
            now,
            now + 3600,
        )
        .unwrap();
    server
        .state
        .db
        .record_research_work_contribution(
            "lab-ann",
            7,
            &fern.id,
            "gh-ann",
            "https://example.test/7",
            now,
            true,
        )
        .unwrap();
    server
        .state
        .db
        .claim_research_work(
            "lab-bo",
            8,
            &bo_bot.name,
            &bo_id,
            Some(&bo_bot.id),
            &bo_bot.name,
            false,
            now,
            now + 3600,
        )
        .unwrap();
    server
        .state
        .db
        .record_research_work_contribution(
            "lab-bo",
            8,
            &bo_bot.id,
            "gh-bo",
            "https://example.test/8",
            now,
            true,
        )
        .unwrap();
    let body = server.reasons(&ann_key).await;
    assert_eq!(line_kinds(&body), vec!["lab_merged"], "{body}");
    assert_eq!(body["reasons"][0]["line"], "Fern: 1 merged lab PR");
    assert!(!body.to_string().contains("example.test"), "no PR links: {body}");
}

// R1, S4: another person's bots' reasons are never shown.
#[tokio::test]
async fn only_your_own_bots_count() {
    let server = Server::start("own").await;
    let (ann_key, _, _) = server.someone("Ann").await;
    let (_, bo_id, bo_room) = server.someone("Bo").await;
    support::legacy_bot(&server.state, &bo_id, &bo_room, "Gus");
    let now = agenthangout::now();
    server.vote("bo-vote", &bo_room, now - 60, now + 3600);
    assert_eq!(server.reasons(&ann_key).await["reasons"], json!([]));
}

// R3: a bot's key gets 403, and no key gets 401.
#[tokio::test]
async fn a_bot_cannot_read_reasons() {
    let server = Server::start("bot").await;
    let (_, ann_id, room) = server.someone("Ann").await;
    let (_, token) = support::legacy_bot(&server.state, &ann_id, &room, "Fern");
    let (status, _) = server.send(&token, Method::GET, "/api/owner/reasons", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = server.send("", Method::GET, "/api/owner/reasons", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// R3: a made-up token is refused the same way as no key, and never reads as a person.
#[tokio::test]
async fn a_made_up_key_gets_401_not_reasons() {
    let server = Server::start("badkey").await;
    let (_, _ann_id, _room) = server.someone("Ann").await;
    let (status, _) = server.send("not-a-real-key", Method::GET, "/api/owner/reasons", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
