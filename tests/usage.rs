//! Tests for the usage counts, written from the rules in docs/usage.md (U1 to U12). A comment such as
//! `// U6` above a test names the rule it covers.
mod support;

use agenthangout::{
    app, auth,
    db::Db,
    now,
    usage::{HourRow, UsageRows},
    AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const DAY: i64 = 86_400;

struct Fixture {
    state: AppState,
    base: String,
    client: Client,
}

impl Fixture {
    /// Alice has the switch. Bob and Eve don't. Alice and Bob each have a bot, and Eve has a private
    /// room named Secret Plans.
    async fn new() -> Self {
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: "http://test".into(),
                create_code: Some("qa".into()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec!["alice".into()],
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
        state.db.create_hangout("secret", "Secret Plans", "secret-code", "eve", now()).unwrap();
        for (id, owner) in [("a", "alice"), ("b", "bob")] {
            state.db.create_agent(id, owner, id, &auth::hash_secret(id), now()).unwrap();
            state.db.add_agent_to_room("shared", id).unwrap();
        }
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
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if !key.is_empty() {
            request = request.bearer_auth(key);
        }
        let response = request.json(&body).send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    async fn get(&self, key: &str, path: &str) -> (StatusCode, Value) {
        self.call(key, Method::GET, path, json!(null)).await
    }

    /// A bot calls a tool, and says what the tool answered.
    async fn tool(&self, bot: &str, name: &str, arguments: Value) -> Value {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}});
        let (status, reply) = self.call(bot, Method::POST, "/mcp", body).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        reply["result"].clone()
    }
}

fn row<'a>(report: &'a Value, list: &str, key: &str, name: &str) -> &'a Value {
    report[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[key] == name)
        .unwrap_or_else(|| panic!("no {name} in {report}"))
}

// U1, U2
#[tokio::test]
async fn tool_calls_are_counted_by_name_and_outcome() {
    let f = Fixture::new().await;
    assert!(f.tool("a", "list_hangouts", json!({})).await["isError"].is_null());
    f.tool("a", "list_hangouts", json!({})).await;
    // A room the bot isn't in is refused, and the reason is a category.
    let refused = f.tool("a", "read_room", json!({"hangout_id": "secret"})).await;
    assert_eq!(refused["isError"], true);
    // A name nobody listed is counted under one fixed name.
    f.tool("a", "<script>made up by a stranger</script>", json!({})).await;

    let (status, report) = f.get("alice", "/api/usage").await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let tool = row(&report, "by_tool", "name", "list_hangouts");
    assert_eq!(tool["surface"], "mcp");
    assert_eq!((tool["calls"].as_i64(), tool["refused"].as_i64()), (Some(2), Some(0)));
    let read = row(&report, "by_tool", "name", "read_room");
    assert_eq!((read["calls"].as_i64(), read["refused"].as_i64()), (Some(1), Some(1)));
    assert!(read["top_refusal"].is_string());
    assert_eq!(row(&report, "by_tool", "name", "unknown_tool")["calls"], 1);
    assert!(!report.to_string().contains("stranger"));
    // The days and the version are there too (U10).
    let today = report["by_day"].as_array().unwrap().last().unwrap().clone();
    assert!(today["calls"].as_i64().unwrap() >= 4);
    assert_eq!(report["by_version"][0]["version"], agenthangout::usage::build());
}

// U1, U2, U3
#[tokio::test]
async fn rest_requests_are_counted_by_route_pattern() {
    let f = Fixture::new().await;
    assert_eq!(f.get("alice", "/api/me").await.0, StatusCode::OK);
    assert_eq!(f.get("eve", "/api/hangouts/shared").await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.get("eve", "/api/hangouts/nothing-like-it").await.0, StatusCode::NOT_FOUND);
    let (_, report) = f.get("alice", "/api/usage").await;
    let me = row(&report, "by_tool", "name", "/api/me");
    assert_eq!(me["surface"], "rest");
    assert_eq!((me["calls"].as_i64(), me["errors"].as_i64()), (Some(1), Some(0)));
    assert!(me["avg_ms"].is_i64());
    // Two different ids are one route, and the ids themselves aren't in the report.
    let room = report["by_tool"].as_array().unwrap().iter().find(|r| r["refused"] == 2).unwrap();
    assert!(room["name"].as_str().unwrap().contains('{'), "{room}");
    assert_eq!(room["top_refusal"], "not_found");
    assert!(!report.to_string().contains("nothing-like-it"));
}

// U5
#[tokio::test]
async fn active_people_and_bots_are_counted_once_a_day() {
    let f = Fixture::new().await;
    for _ in 0..3 {
        f.get("alice", "/api/me").await;
        f.get("bob", "/api/me").await;
        f.tool("a", "list_hangouts", json!({})).await;
    }
    f.get("a", "/api/me").await;
    let (_, report) = f.get("alice", "/api/usage").await;
    let today = report["by_day"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (today["active_people"].as_i64(), today["active_bots"].as_i64()),
        (Some(2), Some(1))
    );
    // Later the same day nothing is added for the same two people and the same bot.
    f.get("alice", "/api/me").await;
    let (_, report) = f.get("alice", "/api/usage").await;
    let today = report["by_day"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(today["active_people"], 2);
    // Only the numbers are stored (U7).
    assert_eq!(f.state.db.usage_days(0).unwrap().len(), 1);
}

// U6
#[tokio::test]
async fn no_text_names_or_secrets_are_in_the_output() {
    let f = Fixture::new().await;
    let said = "PURPLE-ELEPHANT-MESSAGE";
    f.call("bob", Method::POST, "/api/hangouts/shared/messages", json!({"body": said})).await;
    f.tool("a", "say", json!({"hangout_id": "shared", "text": said})).await;
    f.tool("a", "read_room", json!({"hangout_id": "secret"})).await;
    f.get("eve", "/api/hangouts/secret").await;
    let (_, server) = f.get("alice", "/api/usage?days=30").await;
    let (_, bot) = f.get("alice", "/api/agents/a/usage").await;
    for output in [server.to_string(), bot.to_string()] {
        for forbidden in [
            said,
            "Secret Plans",
            "secret-code",
            "shared-code",
            "feed-",
            "ah_owner_",
            "ah_bot_",
            "eve",
            "bob",
        ] {
            assert!(!output.contains(forbidden), "{forbidden} in {output}");
        }
    }
    // And no stored column holds any of it.
    let stored: Vec<String> = f
        .state
        .db
        .usage_hours(0)
        .unwrap()
        .iter()
        .map(|r| format!("{} {} {} {}", r.surface, r.name, r.reason, r.outcome))
        .collect();
    let stored = stored.join(" ");
    assert!(!stored.contains(said) && !stored.contains("Secret"));
}

// U9
#[tokio::test]
async fn only_the_people_on_the_list_read_the_server_usage() {
    let f = Fixture::new().await;
    assert_eq!(f.get("alice", "/api/usage").await.0, StatusCode::OK);
    assert_eq!(f.get("alice", "/api/usage?days=30").await.0, StatusCode::OK);
    assert_eq!(f.get("alice", "/api/usage?days=3").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(f.get("bob", "/api/usage").await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.get("eve", "/api/usage").await.0, StatusCode::NOT_FOUND);
    // Not even the bot of someone on the list.
    assert_eq!(f.get("a", "/api/usage").await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.get("", "/api/usage").await.0, StatusCode::UNAUTHORIZED);
}

// U11
#[tokio::test]
async fn only_the_owner_reads_a_bots_usage() {
    let f = Fixture::new().await;
    f.tool("a", "list_hangouts", json!({})).await;
    f.tool("a", "read_room", json!({"hangout_id": "secret"})).await;
    f.tool("b", "list_hangouts", json!({})).await;
    let (status, usage) = f.get("alice", "/api/agents/a/usage").await;
    assert_eq!(status, StatusCode::OK, "{usage}");
    let days = usage["days"].as_array().unwrap();
    assert_eq!(days.len(), 30);
    let today = days.last().unwrap();
    assert_eq!((today["calls"].as_i64(), today["refused"].as_i64()), (Some(2), Some(1)));
    let refusals: i64 =
        usage["refusals"].as_object().unwrap().values().map(|n| n.as_i64().unwrap()).sum();
    assert_eq!(refusals, 1);
    assert!(usage["last_activity_at"].as_i64().unwrap() > 0);
    for who in ["bob", "eve", "a", "b", ""] {
        let status = f.get(who, "/api/agents/a/usage").await.0;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::UNAUTHORIZED,
            "{who}: {status}"
        );
    }
    assert_eq!(f.get("bob", "/api/agents/a/usage").await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.get("alice", "/api/agents/nobody/usage").await.0, StatusCode::NOT_FOUND);
    // Someone on the list is not the owner either.
    assert_eq!(f.get("alice", "/api/agents/b/usage").await.0, StatusCode::NOT_FOUND);
}

// U8
#[tokio::test]
async fn counts_older_than_thirty_days_are_deleted() {
    let f = Fixture::new().await;
    let at = now();
    let write = |at: i64| {
        let days = [(at.div_euclid(DAY), 1, 1)];
        f.state
            .db
            .write_usage(&UsageRows {
                version: "0.0.1",
                hours: vec![HourRow {
                    hour: at.div_euclid(3600),
                    surface: "mcp",
                    name: "inbox",
                    caller: "bot",
                    outcome: "ok",
                    reason: "",
                    calls: 5,
                    millis: 50,
                    timed: 5,
                    slow: 0,
                }],
                bots: vec![("a", at.div_euclid(DAY), "ok", "", 5)],
                days: &days,
            })
            .unwrap();
    };
    write(at - 31 * DAY);
    write(at - 29 * DAY);
    assert_eq!(f.state.db.usage_hours(0).unwrap().len(), 2);
    f.state.db.sweep_usage(at).unwrap();
    let left = f.state.db.usage_hours(0).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].day, (at - 29 * DAY).div_euclid(DAY));
    assert_eq!(f.state.db.usage_days(0).unwrap().len(), 1);
    assert_eq!(f.state.db.usage_bot_days("a", 0).unwrap().len(), 1);
    // A bot's totals go with the bot.
    f.state.db.delete_agent("a").unwrap();
    assert!(f.state.db.usage_bot_days("a", 0).unwrap().is_empty());
}

// U5
#[tokio::test]
async fn an_idle_server_lets_go_of_the_day_when_it_ends() {
    let f = Fixture::new().await;
    let yesterday = now() - DAY;
    f.state.usage.record(
        yesterday,
        "rest",
        "/api/me",
        agenthangout::usage::Who::Person("alice"),
        StatusCode::OK,
        Some(1),
    );
    f.state.usage.record(
        yesterday,
        "rest",
        "/api/me",
        agenthangout::usage::Who::Person("bob"),
        StatusCode::OK,
        Some(1),
    );
    f.state.usage.record(
        yesterday,
        "rest",
        "/api/me",
        agenthangout::usage::Who::Bot("a"),
        StatusCode::OK,
        Some(1),
    );
    assert_eq!(f.state.usage.held(), (2, 1));
    // No request comes in. The minute timer rolls the day.
    f.state.usage.roll(now());
    assert_eq!(f.state.usage.held(), (0, 0));
    f.state.usage.flush(&f.state);
    let stored = f.state.db.usage_days(0).unwrap();
    assert_eq!(stored, vec![(yesterday.div_euclid(DAY), 2, 1)]);
}

// U1, U2
#[tokio::test]
async fn requests_refused_before_a_tool_is_named_are_counted() {
    let f = Fixture::new().await;
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"inbox","arguments":{}}});
    // Credentials nobody has.
    assert_eq!(
        f.call("ah_bot_nobody", Method::POST, "/mcp", body.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    // An owner key where a bot token goes.
    assert_eq!(f.call("alice", Method::POST, "/mcp", body.clone()).await.0, StatusCode::FORBIDDEN);
    // Another site's page.
    let refused = f
        .client
        .post(format!("{}/mcp", f.base))
        .bearer_auth("a")
        .header("origin", "http://elsewhere.example")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let (_, report) = f.get("alice", "/api/usage").await;
    let rejected = row(&report, "by_tool", "name", "rejected_request");
    assert_eq!((rejected["calls"].as_i64(), rejected["refused"].as_i64()), (Some(3), Some(3)));
    assert!(!report.to_string().contains("elsewhere"));
}

// U1
#[tokio::test]
async fn public_world_tool_calls_are_counted() {
    let f = Fixture::new().await;
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"how_am_i_doing","arguments":{}}});
    f.call("", Method::POST, "/public/mcp", body).await;
    let (_, report) = f.get("alice", "/api/usage").await;
    let public = row(&report, "by_tool", "name", "how_am_i_doing");
    assert_eq!(
        (public["surface"].as_str(), public["calls"].as_i64()),
        (Some("public_mcp"), Some(1))
    );
}
