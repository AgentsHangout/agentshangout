//! Tests for the usage counts, written from the rules in docs/usage.md (U1 to U21). A comment such as
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

/// The four fixed names of U17 and the route of the events read hold the letters of a person's name
/// ("events"), so they are taken out before looking for the people. Nothing else is.
fn without_line_names(output: &str) -> String {
    agenthangout::usage::LINES
        .iter()
        .fold(output.replace("/api/events", ""), |text, name| text.replace(name, ""))
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
        let output = without_line_names(&output);
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
                ..Default::default()
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

fn hour_row(report: &Value, hour: i64) -> &Value {
    let stamp = format!("{}T{:02}:00Z", civil_date(hour.div_euclid(24)), hour.rem_euclid(24));
    report["by_hour"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["hour"] == stamp.as_str())
        .unwrap_or_else(|| panic!("no hour {stamp} in {report}"))
}

/// A day number as a date, worked out here so the test does not lean on the code it checks.
fn civil_date(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

// U15
#[tokio::test]
async fn people_and_bots_are_counted_for_each_hour() {
    use agenthangout::usage::Who;
    let f = Fixture::new().await;
    let now_hour = now().div_euclid(3600);
    let (first, second) = (now_hour - 2, now_hour - 1);
    let at = |hour: i64| hour * 3600 + 5;
    for who in [Who::Person("alice"), Who::Person("bob"), Who::Person("alice"), Who::Bot("a")] {
        f.state.usage.record(at(first), "rest", "/api/me", who, StatusCode::OK, Some(1));
    }
    assert_eq!(f.state.usage.held_hour(), (2, 1));
    // The next hour starts from nothing, so alice counts again there and only there.
    f.state.usage.record(at(second), "rest", "/api/me", Who::Person("alice"), StatusCode::OK, None);
    assert_eq!(f.state.usage.held_hour(), (1, 0));
    f.state.usage.flush(&f.state);
    let mut stored = f.state.db.usage_active_hours(0).unwrap();
    stored.retain(|(hour, _, _)| *hour == first || *hour == second);
    stored.sort();
    // Only two numbers for each hour were written.
    assert_eq!(stored, vec![(first, 2, 1), (second, 1, 0)]);
    // After a restart the hour carries on from the larger number.
    let again = [(first, 1, 4)];
    f.state
        .db
        .write_usage(&UsageRows { version: "x", active_hours: &again, ..Default::default() })
        .unwrap();
    let mut stored = f.state.db.usage_active_hours(first).unwrap();
    stored.sort();
    assert_eq!(stored[0], (first, 2, 4));
    // The report has every hour of the window, one entry each: 7 days back, and 30 when asked.
    let (_, report) = f.get("alice", "/api/usage").await;
    let hours = report["by_hour"].as_array().unwrap().len() as i64;
    let first_day = now().div_euclid(DAY) - 6;
    assert_eq!(hours, now_hour + 1 - first_day * 24);
    let (_, month) = f.get("alice", "/api/usage?days=30").await;
    assert!(month["by_hour"].as_array().unwrap().len() as i64 > 29 * 24);
    let row = hour_row(&report, first);
    assert_eq!((row["active_people"].as_i64(), row["active_bots"].as_i64()), (Some(2), Some(4)));
    assert_eq!(row["calls"], 4);
    assert_eq!(hour_row(&report, second)["active_people"], 1);
}

// U15
#[tokio::test]
async fn an_idle_server_lets_go_of_the_hour_when_it_ends() {
    use agenthangout::usage::Who;
    let f = Fixture::new().await;
    let earlier = now() - 7200;
    f.state.usage.record(earlier, "rest", "/api/me", Who::Person("alice"), StatusCode::OK, None);
    f.state.usage.record(earlier, "rest", "/api/me", Who::Bot("a"), StatusCode::OK, None);
    assert_eq!(f.state.usage.held_hour(), (1, 1));
    // No request comes in. The minute timer rolls the hour.
    f.state.usage.roll(now());
    assert_eq!(f.state.usage.held_hour(), (0, 0));
    f.state.usage.flush(&f.state);
    let stored = f.state.db.usage_active_hours(0).unwrap();
    assert_eq!(stored, vec![(earlier.div_euclid(3600), 1, 1)]);
}

// U16
#[test]
fn response_times_fall_in_fixed_buckets() {
    use agenthangout::usage::{bucket_of, percentile, BUCKETS, EDGES_MS};
    assert_eq!(bucket_of(0), 0);
    assert_eq!(bucket_of(1), 0);
    assert_eq!(bucket_of(2), 1);
    assert_eq!(bucket_of(3), 2);
    assert_eq!(bucket_of(10_000), EDGES_MS.len() - 1);
    assert_eq!(bucket_of(10_001), EDGES_MS.len());
    assert_eq!(bucket_of(u64::MAX), BUCKETS - 1);
    let mut hist = [0i64; BUCKETS];
    assert_eq!(percentile(&hist, 0.5), None);
    hist[bucket_of(3)] = 60;
    hist[bucket_of(40)] = 30;
    hist[bucket_of(3000)] = 10;
    assert_eq!(percentile(&hist, 0.5), Some(5));
    assert_eq!(percentile(&hist, 0.9), Some(50));
    assert_eq!(percentile(&hist, 0.99), Some(5000));
    // Never lower than the truth, and off by no more than the bucket.
    let mut one = [0i64; BUCKETS];
    one[bucket_of(130)] = 1;
    assert_eq!(percentile(&one, 0.5), Some(200));
    // A time over the last edge reads as the last edge.
    let mut slow = [0i64; BUCKETS];
    slow[bucket_of(99_999)] = 3;
    assert_eq!(percentile(&slow, 0.5), Some(10_000));
}

// U16
#[tokio::test]
async fn the_report_has_a_median_and_a_90th_percentile_from_buckets() {
    use agenthangout::usage::Who;
    let f = Fixture::new().await;
    let at = now();
    let times = std::iter::repeat_n(3u64, 60)
        .chain(std::iter::repeat_n(40, 30))
        .chain(std::iter::repeat_n(3000, 10));
    for millis in times {
        f.state.usage.record(at, "rest", "/api/probe", Who::Nobody, StatusCode::OK, Some(millis));
    }
    // A wait that stays open adds no time.
    f.state.usage.record(at, "rest", "/api/probe", Who::Nobody, StatusCode::OK, None);
    let (_, report) = f.get("alice", "/api/usage").await;
    let probe = row(&report, "by_tool", "name", "/api/probe");
    assert_eq!(probe["calls"], 101);
    assert_eq!((probe["p50_ms"].as_u64(), probe["p90_ms"].as_u64()), (Some(5), Some(50)));
    let today = report["by_day"].as_array().unwrap().last().unwrap().clone();
    assert!(today["p50_ms"].is_u64() && today["p90_ms"].is_u64(), "{today}");
    assert!(report["by_version"][0]["p50_ms"].is_u64());
    assert!(hour_row(&report, at.div_euclid(3600))["p90_ms"].is_u64());
    // What is stored is 14 numbers for the hour, not the times.
    let stored = f.state.db.usage_latency(0).unwrap();
    let probe = stored.iter().find(|r| r.name == "/api/probe").unwrap();
    assert_eq!(probe.hist.iter().sum::<i64>(), 100);
}

// U18
#[tokio::test]
async fn memory_for_response_times_and_counts_is_bounded() {
    use agenthangout::usage::{Who, BUCKETS};
    let f = Fixture::new().await;
    let at = now();
    // A hundred thousand calls to one route are one histogram.
    for _ in 0..100_000 {
        f.state.usage.record(at, "rest", "/api/one", Who::Nobody, StatusCode::OK, Some(7));
    }
    assert_eq!(f.state.usage.held_counts().0, 1);
    assert_eq!(std::mem::size_of::<[i64; BUCKETS]>(), 14 * 8);
    // Names without end stop at the cap, and the ones already there keep counting.
    for n in 0..25_000 {
        let name = format!("/api/n{n}");
        f.state.usage.record(at, "rest", &name, Who::Nobody, StatusCode::OK, Some(7));
    }
    let (histograms, _) = f.state.usage.held_counts();
    assert!(histograms <= 20_000, "{histograms}");
    f.state.usage.record(at, "rest", "/api/one", Who::Nobody, StatusCode::OK, Some(7));
    // Lines are four names, whatever happens.
    for _ in 0..1000 {
        f.state.usage.note_line(at, "events_delivered", 1);
    }
    assert_eq!(f.state.usage.held_counts().1, 1);
    f.state.usage.flush(&f.state);
    assert_eq!(f.state.usage.held_counts(), (0, 0));
    let stored = f.state.db.usage_latency(0).unwrap();
    let one = stored.iter().find(|r| r.name == "/api/one").unwrap();
    assert_eq!(one.hist.iter().sum::<i64>(), 100_001);
}

async fn receiver() -> String {
    use axum::{http::StatusCode as Code, routing::post, Router};
    let router = Router::new()
        .route("/ok", post(|| async { Code::NO_CONTENT }))
        .route("/bad", post(|| async { Code::INTERNAL_SERVER_ERROR }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    base
}

// U17
#[tokio::test]
async fn events_long_polls_and_webhooks_are_counted_on_their_own() {
    let f = Fixture::new().await;
    // An event for bot a: bob speaks to it by name in the room they share.
    let said = f
        .call("bob", Method::POST, "/api/hangouts/shared/messages", json!({"text": "@a hello"}))
        .await;
    assert_eq!(said.0, StatusCode::OK, "{}", said.1);
    let (status, events) = f.get("a", "/api/events").await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let delivered = events["events"].as_array().unwrap().len() as i64;
    assert!(delivered >= 1, "{events}");
    let cursor = events["cursor"].as_str().unwrap().to_string();
    // A read that does not wait is not a long-poll. One that waits and finds nothing still is.
    assert_eq!(f.get("a", &format!("/api/events?since={cursor}")).await.0, StatusCode::OK);
    let waited = f.get("a", &format!("/api/events?since={cursor}&wait=1")).await;
    assert_eq!(waited.0, StatusCode::OK, "{}", waited.1);
    // A refused request is not served.
    assert_ne!(f.get("a", "/api/events?since=nope&wait=1").await.0, StatusCode::OK);
    // Webhooks: one answered, one refused with a 500, one nobody answers.
    let base = receiver().await;
    for path in ["ok", "bad"] {
        agenthangout::notify::test_wake_up(&f.state, &format!("{base}/{path}"), None).await;
    }
    agenthangout::notify::test_wake_up(&f.state, "http://127.0.0.1:9/nobody", None).await;

    let (_, report) = f.get("alice", "/api/usage").await;
    let lines = &report["lines"];
    assert_eq!(lines["events_delivered"], delivered, "{report}");
    assert_eq!(lines["long_polls"], 1, "{report}");
    assert_eq!(lines["webhooks_sent"], 3, "{report}");
    assert_eq!(lines["webhooks_failed"], 2, "{report}");
    let today = report["by_day"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(today["lines"], *lines);
    // Only the four names are in the database.
    let names: std::collections::BTreeSet<String> =
        f.state.db.usage_lines(0).unwrap().into_iter().map(|(_, name, _)| name).collect();
    assert!(names.iter().all(|name| agenthangout::usage::LINES.contains(&name.as_str())));
}

// U17
#[tokio::test]
async fn events_pushed_live_on_a_stream_are_counted_in_events_delivered() {
    use futures_util::StreamExt;

    let f = Fixture::new().await;
    let response = f
        .client
        .post(format!("{}/mcp", f.base))
        .bearer_auth("b")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "stream-b",
            "method": "events/stream",
            "params": { "name": "mention" }
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());

    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    let mut posted = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let chunk = tokio::time::timeout(left, stream.next()).await;
        let Ok(Some(Ok(bytes))) = chunk else { break };
        buf.push_str(&String::from_utf8_lossy(&bytes));
        if !posted && buf.contains("notifications/events/active") {
            let said = f
                .call(
                    "alice",
                    reqwest::Method::POST,
                    "/api/hangouts/shared/messages",
                    serde_json::json!({"text": "@b live on stream"}),
                )
                .await;
            assert_eq!(said.0, reqwest::StatusCode::OK, "{}", said.1);
            posted = true;
        }
        if buf.contains("notifications/events/event") {
            break;
        }
    }
    assert!(posted, "active notification was not seen on stream: {buf}");
    assert!(buf.contains("notifications/events/event"), "event was not seen on stream: {buf}");

    let (_, report) = f.get("alice", "/api/usage").await;
    assert_eq!(report["lines"]["events_delivered"], 1, "{report}");
}

// U6, U14
#[tokio::test]
async fn the_new_numbers_hold_no_text_names_or_ids() {
    let f = Fixture::new().await;
    let said = "GREEN-HERON-MESSAGE";
    f.call("bob", Method::POST, "/api/hangouts/shared/messages", json!({"text": said})).await;
    f.tool("a", "say", json!({"hangout_id": "shared", "text": said})).await;
    f.get("a", "/api/events").await;
    f.get("eve", "/api/hangouts/secret").await;
    f.get("eve", "/api/hangouts/nothing-like-it").await;
    let (_, server) = f.get("alice", "/api/usage?days=30").await;
    let output = without_line_names(&server.to_string());
    for forbidden in [
        said,
        "Secret Plans",
        "secret-code",
        "shared-code",
        "feed-",
        "ah_bot_",
        "eve",
        "bob",
        "nothing-like-it",
    ] {
        assert!(!output.contains(forbidden), "{forbidden} in {output}");
    }
    // What was stored for the hour is the route pattern and numbers.
    for stored in f.state.db.usage_latency(0).unwrap() {
        assert!(!format!("{} {}", stored.name, stored.version).contains("nothing-like-it"));
        assert!(matches!(stored.surface.as_str(), "rest" | "mcp" | "public_mcp"));
    }
    // The hour's two numbers are numbers, and nothing else is kept about who they were.
    for (_, people, bots) in f.state.db.usage_active_hours(0).unwrap() {
        assert!(people >= 0 && bots >= 0);
    }
    // Still only for the people on the list.
    assert_eq!(f.get("bob", "/api/usage").await.0, StatusCode::NOT_FOUND);
}

// U20
#[tokio::test]
async fn the_new_numbers_are_deleted_after_thirty_days() {
    let f = Fixture::new().await;
    let at = now();
    let write = |at: i64| {
        let hour = at.div_euclid(3600);
        let hours = [(hour, 3, 1)];
        let mut hist = [0i64; agenthangout::usage::BUCKETS];
        hist[2] = 4;
        f.state
            .db
            .write_usage(&UsageRows {
                version: "0.0.1",
                active_hours: &hours,
                lines: vec![(hour, "long_polls", 2)],
                latency: vec![(hour, "rest", "/api/me", hist)],
                ..Default::default()
            })
            .unwrap();
    };
    write(at - 31 * DAY);
    write(at - 29 * DAY);
    assert_eq!(f.state.db.usage_active_hours(0).unwrap().len(), 2);
    f.state.db.sweep_usage(at).unwrap();
    assert_eq!(f.state.db.usage_active_hours(0).unwrap().len(), 1);
    assert_eq!(f.state.db.usage_lines(0).unwrap().len(), 1);
    assert_eq!(f.state.db.usage_latency(0).unwrap().len(), 1);
    // Written twice, the buckets add up.
    write(at - 29 * DAY);
    assert_eq!(f.state.db.usage_latency(0).unwrap()[0].hist[2], 8);
    assert_eq!(f.state.db.usage_lines(0).unwrap()[0].2, 4);
}
