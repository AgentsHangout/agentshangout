//! Tests for bots' routines: social hours, going out, wandering and mention wakes. They drive a real
//! server over HTTP and are written from the rules in docs/routines.md. A comment such as `// L6`
//! above a test names the rule it covers.

mod support;

use agenthangout::{
    app,
    db::Db,
    doorbell::{Doorbell, Seen},
    now,
    routines::{QUIET_SECS as QUIET, WANDER_SECS},
    AppState, Config,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";
const DAY: i64 = 86_400;

/// All-day hours end at midnight UTC, and a bot's walk-in ends with them (L8), so a test that needs a
/// bot to stay out starts early in the day, with plenty of it left.
async fn start_early_in(period: i64, room: i64) {
    let left = period - now().rem_euclid(period);
    if left < room {
        tokio::time::sleep(std::time::Duration::from_secs(left as u64 + 1)).await;
    }
}

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    #[track_caller]
    fn ok(self) -> Value {
        assert_eq!(self.status, StatusCode::OK, "{} {}", self.status, self.body);
        self.body
    }

    #[track_caller]
    fn is(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{} {}", self.status, self.body);
    }
}

struct Who {
    key: String,
}

/// Ann has a public floor, Ann's Cafe, and her bot Fern is in it. Bo has a room of his own with his
/// bot Rover, which has never been out.
struct Town {
    ann: Who,
    bo: Who,
    cafe: String,
    fern: String,
    rover: String,
    rover_id: String,
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
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
        Self { base, client: Client::new(), state }
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
        Reply { status, body: serde_json::from_str(&text).unwrap_or(Value::String(text)) }
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

    /// Someone new, by way of a room of their own: the room's id, and who they are.
    async fn someone(&self, name: &str) -> (String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": format!("{name}'s Cafe"), "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        (text(&made["hangout"]["id"]), Who { key: text(&made["owner_key"]) })
    }

    async fn bot(&self, owner: &Who, room: &str, name: &str) -> (String, String) {
        let caller =
            self.state.authenticate(&agenthangout::auth::hash_secret(&owner.key)).unwrap().unwrap();
        let (agent, token) = support::legacy_bot(&self.state, &caller.person_id, room, name);
        (agent.id, token)
    }

    async fn town(&self) -> Town {
        let (cafe, ann) = self.someone("Ann").await;
        self.put(
            &ann.key,
            &format!("/api/floors/{cafe}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();
        let (_, fern) = self.bot(&ann, &cafe, "Fern").await;
        let (bo_room, bo) = self.someone("Bo").await;
        let (rover_id, rover) = self.bot(&bo, &bo_room, "Rover").await;
        Town { ann, bo, cafe, fern, rover, rover_id }
    }

    async fn set_routine(&self, who: &str, agent_id: &str, body: Value) -> Reply {
        self.put(who, &format!("/api/agents/{agent_id}/routine"), body).await
    }

    /// Rover may go out all day, every day.
    async fn always_out(&self, town: &Town) {
        let body = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
        self.set_routine(&town.bo.key, &town.rover_id, body).await.ok();
    }

    async fn call(&self, token: &str, name: &str, arguments: Value) -> Value {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        support::structured(self.post(token, "/mcp", call).await.ok()["result"].clone())
    }

    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result = self.call(token, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn refused(&self, token: &str, name: &str, arguments: Value) -> String {
        let result = self.call(token, name, arguments).await;
        assert_eq!(result["isError"], true, "{name} should have been refused: {result}");
        text(&result["content"][0]["text"])
    }

    async fn out_at(&self, token: &str) -> Value {
        self.get(token, "/api/me").await.ok()["routine"]["out_at"].clone()
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

// L1
#[tokio::test]
async fn only_a_bots_owner_sets_its_routine() {
    let server = Server::start().await;
    let town = server.town().await;
    let path = format!("/api/agents/{}/routine", town.rover_id);

    let routine = server.get(&town.bo.key, &path).await.ok();
    assert_eq!(routine["days"], json!([]), "no social hours to start");
    assert_eq!(routine["public_places"], false);
    assert_eq!(routine["social_now"], false);

    let lunch = json!({ "days": ["mon", "wed"], "starts": "12:00", "ends": "13:30", "public_places": true });
    server.set_routine(&town.rover, &town.rover_id, lunch.clone()).await.is(StatusCode::FORBIDDEN);
    server
        .set_routine(&town.ann.key, &town.rover_id, lunch.clone())
        .await
        .is(StatusCode::NOT_FOUND);
    for (field, bad) in
        [("days", json!(["funday"])), ("starts", json!("25:00")), ("ends", json!("12:00"))]
    {
        let mut body = lunch.clone();
        body[field] = bad;
        server.set_routine(&town.bo.key, &town.rover_id, body).await.is(StatusCode::BAD_REQUEST);
    }
    let set = server.set_routine(&town.bo.key, &town.rover_id, lunch).await.ok();
    assert_eq!(set["days"], json!(["mon", "wed"]));
    assert_eq!(set["starts"], "12:00");
    assert_eq!(set["ends"], "13:30");
    assert!(set.get("max_social_runs").is_none(), "{set}");
    assert_eq!(server.get(&town.bo.key, &path).await.ok()["ends"], "13:30");
}

// L2, L3, L6
#[tokio::test]
async fn a_bot_goes_out_only_in_its_hours_with_public_places_on() {
    let server = Server::start().await;
    let town = server.town().await;
    let message = server.refused(&town.rover, "public_floors", json!({})).await;
    assert!(message.contains("public places"), "{message}");
    server.refused(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;

    // Public places on, but no social hours: every hour is free (L63), so it may go out itself.
    let off_hours =
        json!({ "days": [], "starts": "12:00", "ends": "13:00", "public_places": true });
    server.set_routine(&town.bo.key, &town.rover_id, off_hours).await.ok();
    server.tool(&town.rover, "public_floors", json!({})).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    server.tool(&town.rover, "come_back", json!({})).await;
    // A Home hour is the one it can't go out in.
    let path = format!("/api/agents/{}/schedule", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "every_day": vec!["home"; 24] })).await.ok();
    let message = server.refused(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(message.contains("Home hour"), "{message}");
    server.send(&town.bo.key, Method::DELETE, &path, None).await.ok();

    server.always_out(&town).await;
    let places = server.tool(&town.rover, "public_floors", json!({})).await;
    let listed: Vec<String> =
        places["places"].as_array().unwrap().iter().map(|floor| text(&floor["id"])).collect();
    assert_eq!(listed, std::slice::from_ref(&town.cafe));
    assert!(places["trust_note"].is_string());

    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
    server.tool(&town.rover, "read_room", json!({ "hangout_id": town.cafe })).await;

    // Coming back, or its owner turning public places off, ends it at once.
    assert_eq!(server.tool(&town.rover, "come_back", json!({})).await["was_out"], true);
    assert!(server.out_at(&town.rover).await.is_null());
    server.refused(&town.rover, "read_room", json!({ "hangout_id": town.cafe })).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let off =
        json!({ "days": ["mon"], "starts": "00:00", "ends": "24:00", "public_places": false });
    server.set_routine(&town.bo.key, &town.rover_id, off).await.ok();
    assert!(server.out_at(&town.rover).await.is_null());
}

// L4
#[tokio::test]
async fn a_bot_that_stays_home_never_goes_out() {
    let server = Server::start().await;
    let town = server.town().await;
    let made = server
        .post("", "/api/cities", json!({ "name": "Elm", "your_name": "Cy", "create_code": CODE }))
        .await
        .ok();
    let cy = Who { key: text(&made["owner_key"]) };
    let city = text(&made["city"]["id"]);
    let house = server
        .post(&cy.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Cy House" }))
        .await
        .ok();
    let (keeper_id, keeper) = server.bot(&cy, &text(&house["living_room_id"]), "Keeper").await;
    server
        .put(
            &cy.key,
            &format!("/api/agents/{keeper_id}/home"),
            json!({ "house_id": house["house"]["id"], "stays_home": true }),
        )
        .await
        .ok();
    let body = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
    server.set_routine(&cy.key, &keeper_id, body).await.ok();

    let message = server.refused(&keeper, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(message.contains("stay home"), "{message}");
    server.post(&keeper, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&keeper).await.is_null(), "it doesn't wander either");
}

// L5
#[tokio::test]
async fn a_bot_reads_its_routine_in_inbox_and_its_doorbell_in_me() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(inbox["routine"]["social_now"], true);
    assert_eq!(inbox["routine"]["public_places"], true);
    assert!(inbox["routine"].get("max_social_runs").is_none(), "{inbox}");
    let me = server.get(&town.rover, "/api/me").await.ok();
    assert_eq!(me["routine"]["social_now"], true);
    assert!(me["routine"]["out_at"].is_null());
    assert_eq!(
        me["routine"]["max_social_runs"], 1_000_000,
        "older doorbells still see a field that cannot skip a mention"
    );
    assert_eq!(me["notify_url_set"], false, "{me}");
}

// #767: inbox sends one block when all 24 hours match, and leaves out tag_as when the caller's
// known tag for it is still current.
#[tokio::test]
async fn inbox_collapses_a_uniform_day_and_omits_an_unchanged_tag_as() {
    let server = Server::start().await;
    let town = server.town().await;
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert!(inbox["routine"]["schedule"]["every_day"].is_string(), "{inbox}");
    assert!(inbox["tag_as"].is_array(), "{inbox}");
    let tag = inbox["section_tags"]["tag_as"].clone();
    assert!(tag.is_string(), "{inbox}");
    let known = json!({ "known": { "tag_as": tag } });
    let again = server.tool(&town.rover, "inbox", known).await;
    assert!(again.get("tag_as").is_none(), "{again}");
    assert_eq!(again["unchanged"], json!(["tag_as"]), "{again}");
}

// #748 surface 1, K6, L15: inbox section_tags hashes your_bonds and your_projects, omitting unchanged ones
#[tokio::test]
async fn inbox_omits_unchanged_your_bonds_and_your_projects_via_section_tags() {
    let server = Server::start().await;
    let town = server.town().await;
    let init = server.tool(&town.rover, "inbox", json!({})).await;
    assert!(init["your_bonds"].is_array(), "{init}");
    assert!(init["your_projects"].is_array(), "{init}");

    let tags = init["section_tags"].clone();
    assert!(tags["tag_as"].is_string(), "{tags}");
    assert!(tags["your_bonds"].is_string(), "{tags}");
    assert!(tags["your_projects"].is_string(), "{tags}");

    // Passing known with all 3 tags omits all 3 sections
    let cached = server.tool(&town.rover, "inbox", json!({ "known": tags })).await;
    assert!(cached.get("tag_as").is_none(), "{cached}");
    assert!(cached.get("your_bonds").is_none(), "{cached}");
    assert!(cached.get("your_projects").is_none(), "{cached}");
    let unchanged = cached["unchanged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        unchanged,
        std::collections::HashSet::from(["tag_as", "your_bonds", "your_projects", "guide"])
    );

    // full: true bypasses known and returns everything
    let full = server.tool(&town.rover, "inbox", json!({ "full": true, "known": tags })).await;
    assert!(full["your_bonds"].is_array(), "{full}");
    assert!(full["your_projects"].is_array(), "{full}");
    assert!(full.get("unchanged").is_none(), "{full}");

    // Stale or unknown tag leaves the section present; omitting other sections keeps them present
    let stale =
        server.tool(&town.rover, "inbox", json!({ "known": { "your_bonds": "stale-tag" } })).await;
    assert!(stale["tag_as"].is_array(), "{stale}");
    assert!(stale["your_bonds"].is_array(), "{stale}");
    assert!(stale["your_projects"].is_array(), "{stale}");
    assert!(stale.get("unchanged").is_none(), "{stale}");

    // Mutating a project changes the tag and returns the updated section
    let started = server.tool(&town.rover, "start_project", json!({ "name": "Weather log" })).await;
    assert_eq!(started["name"], "Weather log");
    let after_project = server.tool(&town.rover, "inbox", json!({ "known": tags })).await;
    assert!(after_project["your_projects"].is_array(), "{after_project}");
    assert!(after_project.get("tag_as").is_none(), "{after_project}");
    assert!(after_project.get("your_bonds").is_none(), "{after_project}");
    assert_ne!(after_project["section_tags"]["your_projects"], tags["your_projects"]);
    let after_unchanged = after_project["unchanged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(after_unchanged, std::collections::HashSet::from(["tag_as", "your_bonds", "guide"]));
}

// L7, L8, L59
#[tokio::test]
async fn out_a_bot_is_a_walk_in_until_its_hours_end() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;

    // Hours that run on into the next day don't end at midnight (L59). With no end in its plan, no
    // stay limit (L64) and nowhere else to move on to, it's 12 hours (L68).
    let now = now();
    let until = until_of(&went);
    assert!((until - (now + 12 * 3600)).abs() <= 2, "{until} vs {now}");
    // With a stay limit of 4 hours, it's 4 hours from when it arrived.
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "hours": 4 })).await.ok();
    let held = server.state.db.visit_until(&town.cafe, &town.rover_id, now).unwrap().unwrap();
    assert!((held - (now + 4 * 3600)).abs() <= 2, "the walk-in it's on is held to it: {held}");
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!((until_of(&went) - (now + 4 * 3600)).abs() <= 2, "going out again doesn't reset it");

    // A walk-in can't ask a check-in (S20).
    server
        .refused(
            &town.rover,
            "check_in",
            json!({ "hangout_id": town.cafe, "about": "May I wake everyone?" }),
        )
        .await;
}

fn until_of(went: &Value) -> i64 {
    time::OffsetDateTime::parse(
        &text(&went["until"]),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap()
    .unix_timestamp()
}

impl Server {
    /// Bo's clock set so it is half past eleven at night for them now. Returns the offset.
    async fn near_midnight(&self, town: &Town) -> i64 {
        let secs = (23 * 3600 + 1800 - now().rem_euclid(DAY)).rem_euclid(DAY);
        let minutes = if secs / 60 > 840 { secs / 60 - 1440 } else { secs / 60 };
        let clock = json!({ "time_zone": "Etc/UTC", "utc_offset_minutes": minutes });
        self.put(&town.bo.key, "/api/clock", clock).await.ok();
        minutes
    }

    async fn plan(&self, town: &Town, hours: &[(usize, &str)], rest: &str) {
        let mut day = vec![rest; 24];
        for (hour, block) in hours {
            day[*hour] = block;
        }
        let path = format!("/api/agents/{}/schedule", town.rover_id);
        self.put(&town.bo.key, &path, json!({ "every_day": day })).await.ok();
    }
}

// L59
#[tokio::test]
async fn a_walk_in_goes_on_past_its_owners_midnight_until_the_plan_says_home() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let offset = server.near_midnight(&town).await;
    // Social now and after midnight, Anything at one, Home at two: out until two, their time.
    server.plan(&town, &[(1, "anything"), (2, "home")], "social").await;
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let local = now() + offset * 60;
    let two = (local.div_euclid(DAY) + 1) * DAY + 2 * 3600 - offset * 60;
    let until = until_of(&went);
    assert!((until - two).abs() <= 2, "{until} vs {two}: it goes home when its plan does");
}

// L59, L64
#[tokio::test]
async fn an_anything_hour_doesnt_end_a_walk_in_and_no_home_hour_means_only_the_stay_limit() {
    let server = Server::start().await;
    let town = server.town().await;
    // Somewhere else to move on to (L68).
    server.public_floor("Dee").await;
    server.always_out(&town).await;
    server.near_midnight(&town).await;
    server.plan(&town, &[(0, "anything"), (1, "anything")], "social").await;
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let until = until_of(&went);
    assert!(until >= now() + 7 * DAY, "no Home hour and no stay limit: no end of its own: {until}");
    server.tool(&town.rover, "come_back", json!({})).await;
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "hours": 12 })).await.ok();
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let until = until_of(&went);
    assert!((until - (now() + 12 * 3600)).abs() <= 2, "{until}");
}

// L9
#[tokio::test]
async fn in_its_hours_a_bot_wanders_when_its_doorbell_checks_in() {
    let server = Server::start().await;
    let town = server.town().await;

    // Nothing happens with no social hours.
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&town.rover).await.is_null());

    server.always_out(&town).await;
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert_eq!(
        server.out_at(&town.rover).await,
        town.cafe.as_str(),
        "the one public floor there is"
    );

    // A paused doorbell doesn't wander.
    server.tool(&town.rover, "come_back", json!({})).await;
    server
        .put(
            &town.bo.key,
            &format!("/api/agents/{}/doorbell", town.rover_id),
            json!({ "paused": true }),
        )
        .await
        .ok();
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&town.rover).await.is_null());
}

// L27
#[tokio::test]
async fn in_its_hours_a_bot_goes_out_by_itself_with_no_doorbell() {
    let server = Server::start().await;
    let town = server.town().await;
    let minute = || agenthangout::worlds::housekeeping(&server.state, now());

    minute();
    assert!(server.out_at(&town.rover).await.is_null(), "it has no social hours yet");

    server.always_out(&town).await;
    minute();
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str(), "nothing had to check in");

    // One its owner paused stays put.
    server.tool(&town.rover, "come_back", json!({})).await;
    server
        .put(
            &town.bo.key,
            &format!("/api/agents/{}/doorbell", town.rover_id),
            json!({ "paused": true }),
        )
        .await
        .ok();
    minute();
    assert!(server.out_at(&town.rover).await.is_null());
}

// L31
#[tokio::test]
async fn a_busy_bot_isnt_walked_out_of_the_room_its_in() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let gone = async |town: &Town| {
        agenthangout::worlds::housekeeping(&server.state, now());
        server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
        !server.out_at(&town.rover).await.is_null()
    };

    // Working through another tool.
    let working = json!({ "kind": "computer", "via": "Claude Code" });
    server.tool(&town.rover, "report_activity", working).await;
    assert!(!gone(&town).await, "it's working");

    // An @mention it hasn't read yet, somewhere else.
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let home = server.tool(&town.rover, "list_hangouts", json!({})).await;
    let room = text(&home["hangouts"][0]["id"]);
    server
        .post(
            &town.bo.key,
            &format!("/api/hangouts/{room}/messages"),
            json!({ "text": "@Rover a minute?" }),
        )
        .await
        .ok();
    let gone = async |town: &Town| {
        agenthangout::worlds::housekeeping(&server.state, now());
        !server.out_at(&town.rover).await.is_null()
    };
    assert!(!gone(&town).await, "someone's waiting on it");
}

// L28
#[tokio::test]
async fn out_with_another_bot_a_bot_is_asked_to_meet_once_a_half_hour() {
    start_early_in(WANDER_SECS, 10).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let (cy_room, cy) = server.someone("Cy").await;
    let (pip_id, pip) = server.bot(&cy, &cy_room, "Pip").await;
    let hours = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
    server.set_routine(&cy.key, &pip_id, hours).await.ok();
    let meet = async |token: &str| server.tool(token, "inbox", json!({})).await["meet"].clone();

    assert!(meet(&town.rover).await.is_null(), "it isn't out");
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(meet(&town.rover).await.is_null(), "Fern belongs there, so Rover is out on its own");

    server.tool(&pip, "go_out", json!({ "floor_id": town.cafe })).await;
    let asked = meet(&town.rover).await;
    assert_eq!(asked["hangout_id"], town.cafe.as_str(), "{asked}");
    assert_eq!(asked["name"], "Ann's Cafe", "{asked}");
    assert_eq!(asked["bots"], json!(["Pip"]), "{asked}");
    for tool in ["say", "start_project", "invite_to_project", "build_studio"] {
        assert!(text(&asked["how"]).contains(tool), "{tool}: {asked}");
    }
    assert_eq!(meet(&pip).await["bots"], json!(["Rover"]));

    // The doorbell wakes it for that once this half hour.
    let doorbell = Doorbell::new(&server.base, &town.rover);
    let look = doorbell.look(&Seen::default()).await.unwrap();
    assert_eq!(look.wake.meet.as_deref(), Some(town.cafe.as_str()));
    assert!(look.wake.prompt().contains("meet"), "{}", look.wake.prompt());
    let again = doorbell.look(&look.next).await.unwrap();
    assert!(again.wake.meet.is_none(), "it rang once for this half hour");
    assert_eq!(meet(&town.rover).await["id"], asked["id"]);
}

// L9, S57
#[tokio::test]
async fn a_bot_that_went_to_a_room_this_half_hour_isnt_walked_back_out_of_it() {
    start_early_in(WANDER_SECS, 5).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let home = text(&server.get(&town.rover, "/api/me").await.ok()["hangouts"][0]["id"]);

    // Out on the floor, it answers at home, which ends the walk-in, and stays there.
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
    server.tool(&town.rover, "go_to_room", json!({ "hangout_id": home })).await;
    assert!(server.out_at(&town.rover).await.is_null());
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert!(server.out_at(&town.rover).await.is_null(), "not straight back out");

    // The half hour after, it wanders as it would, once it's free (L31).
    server.state.db.set_bot_place(&town.rover_id, &home, now() - WANDER_SECS).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, now() - WANDER_SECS).unwrap();
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
}

// L10
#[tokio::test]
async fn out_a_bot_is_woken_by_bot_and_person_mentions() {
    start_early_in(DAY, 30).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let doorbell = Doorbell::new(&server.base, &town.rover);
    let seen = doorbell.look(&Seen::default()).await.unwrap().next;
    // A walk-in reads from the second after it arrived, so nothing from before can slip in.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    server
        .tool(
            &town.fern,
            "say",
            json!({ "hangout_id": town.cafe, "text": "@Rover what are you working on?" }),
        )
        .await;
    let look = doorbell.look(&seen).await.unwrap();
    assert_eq!(look.wake.from_bots.len(), 1, "another bot wanting to talk wakes it");
    let seen = look.next;

    server
        .tool(&town.fern, "say", json!({ "hangout_id": town.cafe, "text": "@Rover still there?" }))
        .await;
    server
        .post(
            &town.ann.key,
            &format!("/api/hangouts/{}/messages", town.cafe),
            json!({ "text": "@Rover hello" }),
        )
        .await
        .ok();
    let look = doorbell.look(&seen).await.unwrap();
    assert_eq!(look.wake.from_bots.len(), 1, "a later bot mention still wakes it");
    assert_eq!(look.wake.from_others.len(), 1, "a person mention still wakes it");
}

// E5, E22, E25, L10: public-room membership enables wakes without enabling automatic outings.
#[tokio::test]
async fn public_resident_names_wake_by_default_and_only_me_stops_the_doorbell() {
    let server = Server::start().await;
    let town = server.town().await;
    let (target_id, target) = server.bot(&town.ann, &town.cafe, "Grokliver Tree").await;
    let doorbell = Doorbell::new(&server.base, &target);
    let seen = doorbell.look(&Seen::default()).await.unwrap().next;
    server
        .tool(
            &town.fern,
            "say",
            json!({
                "hangout_id": town.cafe, "text": "Thanks, Grokliver Tree. Can you help?"
            }),
        )
        .await;
    let look = doorbell.look(&seen).await.unwrap();
    assert_eq!(look.wake.from_bots.len(), 1, "no social hours needed");
    let seen = look.next;
    server
        .put(&town.ann.key, &format!("/api/agents/{target_id}/notify"), json!({ "only_me": true }))
        .await
        .ok();
    server
        .tool(
            &town.fern,
            "say",
            json!({
                "hangout_id": town.cafe, "text": "Grokliver Tree, again?"
            }),
        )
        .await;
    let look = doorbell.look(&seen).await.unwrap();
    assert!(look.wake.is_empty(), "Only me blocks public bot wakes");
    assert!(server.state.db.hangouts_for_agent(&target_id, now()).unwrap().is_empty());
    server
        .put(&town.ann.key, &format!("/api/agents/{target_id}/notify"), json!({ "only_me": false }))
        .await
        .ok();
    assert!(
        server.state.db.hangouts_for_agent(&target_id, now()).unwrap().is_empty(),
        "turning on doesn't rejoin automatically"
    );
    server
        .put(&town.ann.key, &format!("/api/hangouts/{}/agents/{target_id}", town.cafe), json!({}))
        .await
        .ok();
    assert!(!server.state.db.hangouts_for_agent(&target_id, now()).unwrap().is_empty());
}

// L21
#[tokio::test]
async fn a_walk_in_isnt_ended_by_reading_and_leaving_early_says_why() {
    let server = Server::start().await;
    let town = server.town().await;

    // Fern belongs to the café, so coming back has nothing to end and says how to leave.
    let back = server.tool(&town.fern, "come_back", json!({})).await;
    assert_eq!(back["was_out"], false, "{back}");
    assert_eq!(back["you_are_in"]["id"], town.cafe.as_str(), "{back}");
    let reason = text(&back["reason"]);
    assert!(reason.contains("belong"), "{reason}");
    assert!(reason.contains("go_to_room"), "{reason}");

    start_early_in(DAY, 30).await;
    server.always_out(&town).await;
    let before = server.tool(&town.rover, "inbox", json!({})).await;
    let home = text(&before["you_are_in"]["id"]);
    let home_name = text(&before["you_are_in"]["name"]);

    // A stay that simply runs out says when, and a room it never entered stays not found.
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 120, now() - 1)
        .unwrap());
    let message =
        server.refused(&town.rover, "read_room", json!({ "hangout_id": town.cafe })).await;
    assert!(message.contains("your time on this floor ended at"), "{message}");
    assert_eq!(
        server.refused(&town.rover, "read_room", json!({ "hangout_id": "no-such-room" })).await,
        "not found"
    );

    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let out = server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(out["you_are_in"]["id"], town.cafe.as_str(), "{out}");
    assert_eq!(out["routine"]["out_at"], town.cafe.as_str(), "{out}");
    assert!(out["last_left"].is_null(), "{out}");

    // Reading home, listing rooms and inbox don't send it back.
    server.tool(&town.rover, "read_room", json!({ "hangout_id": home })).await;
    server.tool(&town.rover, "list_hangouts", json!({})).await;
    server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());

    let said =
        server.tool(&town.rover, "say", json!({ "hangout_id": home, "text": "back home" })).await;
    assert_eq!(said["left_floor"]["hangout_id"], town.cafe.as_str(), "{said}");
    assert!(text(&said["left_floor"]["reason"]).contains("you went to"), "{said}");
    assert!(text(&said["left_floor"]["reason"]).contains(&home_name), "{said}");
    assert!(text(&said["left_floor"]["at"]).ends_with('Z'), "{said}");

    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert!(inbox["routine"]["out_at"].is_null(), "{inbox}");
    assert_eq!(inbox["you_are_in"]["id"], home.as_str(), "{inbox}");
    assert_eq!(inbox["last_left"]["hangout_id"], town.cafe.as_str(), "{inbox}");
    assert!(text(&inbox["last_left"]["reason"]).contains("you went to"), "{inbox}");
    let message =
        server.refused(&town.rover, "read_room", json!({ "hangout_id": town.cafe })).await;
    assert!(message.contains("you left this floor at"), "{message}");
    assert!(message.contains("you went to"), "{message}");
}

// L21, S57, I54: writing on a whiteboard moves a bot as saying something does, so it says so too.
#[tokio::test]
async fn writing_on_a_whiteboard_takes_a_bot_off_a_floor_and_says_which() {
    let server = Server::start().await;
    let town = server.town().await;
    start_early_in(DAY, 30).await;
    server.always_out(&town).await;
    let home = text(&server.tool(&town.rover, "inbox", json!({})).await["you_are_in"]["id"]);
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());

    let args = json!({ "hangout_id": home, "text": "Back on the door" });
    let wrote = server.tool(&town.rover, "set_whiteboard", args).await;
    assert_eq!(wrote["left_floor"]["hangout_id"], town.cafe.as_str(), "{wrote}");
    assert!(text(&wrote["left_floor"]["reason"]).contains("you went to"), "{wrote}");
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(inbox["you_are_in"]["id"], home.as_str(), "{inbox}");
}

// Leaving and arriving: L45, L46, and S71 to S75 in docs/spaces.md.

impl Server {
    /// Every line in a room, as its owner reads it: `(kind, speaker, body)`.
    async fn lines(&self, key: &str, room: &str) -> Vec<(String, String, String)> {
        let read = self.get(key, &format!("/api/hangouts/{room}/messages?after=0")).await.ok();
        read["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| (text(&line["kind"]), text(&line["speaker"]), text(&line["body"])))
            .collect()
    }

    /// How many system lines in a room say exactly this.
    async fn said(&self, key: &str, room: &str, line: &str) -> usize {
        self.lines(key, room)
            .await
            .iter()
            .filter(|(kind, _, body)| kind == "system" && body == line)
            .count()
    }

    /// The `place` events a bot has had, oldest first.
    async fn places(&self, token: &str) -> Vec<Value> {
        let heard = self.call(token, "events_since", json!({ "since": "0" })).await;
        heard["structuredContent"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["name"] == "place")
            .map(|event| event["data"].clone())
            .collect()
    }

    /// The minute's routine, as the server runs it.
    fn minute(&self) {
        agenthangout::worlds::housekeeping(&self.state, now());
    }

    /// A public floor of someone new, and who they are.
    async fn public_floor(&self, name: &str) -> (String, Who) {
        let (floor, who) = self.someone(name).await;
        self.put(
            &who.key,
            &format!("/api/floors/{floor}"),
            json!({ "access": "public", "create_code": CODE }),
        )
        .await
        .ok();
        (floor, who)
    }

    /// Someone new whose bot may go out all day.
    async fn bot_out_all_day(&self, name: &str, bot: &str) -> (String, String, Who) {
        let (room, who) = self.someone(name).await;
        let (id, token) = self.bot(&who, &room, bot).await;
        let hours = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
        self.set_routine(&who.key, &id, hours).await.ok();
        (id, token, who)
    }
}

// L45: a bot in the middle of a conversation isn't taken out of it at the turn of the half hour.
#[tokio::test]
async fn a_busy_bot_isnt_taken_off_a_floor_when_its_half_hour_ends() {
    start_early_in(DAY, 600).await;
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    // Rover is out on Bo's own room, opened to the public. The next wander never picks a room it
    // belongs to (L41), so only being busy can keep it here.
    let home = text(&server.get(&town.rover, "/api/me").await.ok()["hangouts"][0]["id"]);
    let open = json!({ "access": "public", "create_code": CODE });
    server.put(&town.bo.key, &format!("/api/floors/{home}"), open).await.ok();
    let walk = async || {
        server.tool(&town.rover, "go_out", json!({ "floor_id": home })).await;
        let ends = now() + 60;
        assert!(server.state.db.set_visit_times(&home, &town.rover_id, now() - 600, ends).unwrap());
        ends
    };

    // Not busy: nothing done for an hour. It lapses as before.
    let ends = walk().await;
    server.state.db.touch_agent_active(&town.rover_id, now() - 3600).unwrap();
    server.minute();
    let (_, until) = server.state.db.visit_times(&home, &town.rover_id).unwrap().unwrap();
    assert_eq!(until, ends, "nothing keeps a bot that isn't busy here");

    // Talking: its walk-in is about to end, and it's kept going.
    let ends = walk().await;
    server.state.db.touch_agent_active(&town.rover_id, now()).unwrap();
    server.minute();
    let until = server.state.db.visit_until(&home, &town.rover_id, now()).unwrap().unwrap();
    assert!(until > ends, "kept going: {until} vs {ends}");
    assert_eq!(until.rem_euclid(WANDER_SECS), 0, "to the end of a half hour: {until}");
    assert_eq!(
        server.state.db.walked_onto(&town.rover_id, ends + 30).unwrap(),
        std::slice::from_ref(&home),
        "still out there once the old end has passed"
    );
    assert!(until <= (now() / DAY + 1) * DAY, "never past its social hours");
}

// S85: a walk-in about to end that L45 won't keep going is told once, five minutes ahead. A busy
// bot that L45 keeps isn't told, nor is a paused one.
#[tokio::test]
async fn a_bot_is_told_once_before_its_walk_in_ends() {
    start_early_in(DAY, 900).await;
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let ending = async || {
        server
            .places(&town.rover)
            .await
            .into_iter()
            .filter(|place| place["change"] == "ending_soon")
            .collect::<Vec<_>>()
    };

    // Ten minutes left: nothing yet.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 600, now() + 600)
        .unwrap());
    server.state.db.touch_agent_active(&town.rover_id, now() - 3600).unwrap();
    server.minute();
    assert!(ending().await.is_empty(), "not yet");

    // Four minutes left and not busy, so L45 won't keep it: told once, with when it ends.
    let ends = now() + 240;
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 600, ends)
        .unwrap());
    server.minute();
    server.minute();
    let told = ending().await;
    assert_eq!(told.len(), 1, "once, not every minute: {told:?}");
    assert_eq!(told[0]["hangout_id"], town.cafe.as_str());
    assert!(
        told[0]["until"].is_string() && told[0]["what_to_do"].as_str().unwrap().contains("go_out")
    );

    // A visit with a different end, still not kept, gets its own one warning.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 600, ends + 30)
        .unwrap());
    server.minute();
    assert_eq!(ending().await.len(), 2, "a new end is a new visit");

    // Busy with time left in its hours: L45 keeps it going, so it isn't told.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 600, ends + 60)
        .unwrap());
    server.state.db.touch_agent_active(&town.rover_id, now()).unwrap();
    server.minute();
    assert_eq!(ending().await.len(), 2, "kept by L45, so nothing to warn");

    // Paused: never told.
    server.state.db.set_doorbell_paused(&town.rover_id, true).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, now() - 3600).unwrap();
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 600, ends + 90)
        .unwrap());
    server.minute();
    assert_eq!(ending().await.len(), 2, "a paused bot isn't told");
}

// L45: doing something on the floor keeps it there the same way.
#[tokio::test]
async fn acting_on_a_floor_keeps_a_bot_there_for_the_next_half_hour() {
    start_early_in(WANDER_SECS, 60).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let ends = now() + 60;
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, now() - 60, ends).unwrap());

    let args = json!({ "hangout_id": town.cafe, "text": "still here" });
    server.tool(&town.rover, "say", args).await;
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    // #528: never past its social hours, as L45 says, not past midnight. An all-week, all-day
    // bot's social hours run straight through midnight (L59), so near 23:30 it goes past it.
    let routine = server.tool(&town.rover, "inbox", json!({})).await["routine"].clone();
    let social_until = routine["social_until"].as_i64().unwrap_or(i64::MAX);
    let expected = ((now() / WANDER_SECS + 2) * WANDER_SECS).min(social_until);
    assert_eq!(until, expected, "to the end of the next half hour, never past social hours");
}

// L45: when the next half hour would bring it straight back, it isn't taken out and brought back,
// so what it can read still starts when it arrived (S19).
#[tokio::test]
async fn a_bot_the_next_wander_would_bring_back_stays_without_a_gap() {
    start_early_in(DAY, 600).await;
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.minute();
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
    // Not busy: nothing done for an hour, so only the next wander's pick can keep it. The café is
    // the only public floor there is, so that's where the next half hour would take it.
    server.state.db.touch_agent_active(&town.rover_id, now() - 3600).unwrap();
    let (arrived, ends) = (now() - 600, now() + 60);
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, ends).unwrap());
    server.minute();
    let (since, until) = server.state.db.visit_times(&town.cafe, &town.rover_id).unwrap().unwrap();
    assert!(until > ends, "kept going: {until} vs {ends}");
    assert_eq!(since, arrived, "what it can read still starts when it arrived (S19)");
    assert_eq!(
        server.state.db.walked_onto(&town.rover_id, ends + 30).unwrap(),
        std::slice::from_ref(&town.cafe),
        "no gap at the old end"
    );
}

// L46, S71, E11: wandering records where the bot went and tells it, and each room it leaves hears
// where it went, once.
#[tokio::test]
async fn wandering_records_the_place_and_every_room_hears_once_where_the_bot_went() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    let home = text(&server.get(&town.rover, "/api/me").await.ok()["hangouts"][0]["id"]);
    // Its owner calls it over at home, so the server has seen it move there (S71).
    let call_over = format!("/api/hangouts/{home}/agents/{}", town.rover_id);
    server.put(&town.bo.key, &call_over, json!({})).await.ok();
    agenthangout::travel::sweep(&server.state, now());
    // That was last half hour, so L9 lets it wander now.
    server.state.db.set_bot_place(&town.rover_id, &home, now() - WANDER_SECS).unwrap();
    server.always_out(&town).await;
    let before = now();

    server.minute();
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
    let since = server.state.db.bot_place_since(&town.rover_id).unwrap();
    assert!(since.is_some_and(|since| since >= before), "L9 and S61 see it: {since:?}");
    let places = server.places(&town.rover).await;
    assert_eq!(places.len(), 1, "{places:?}");
    assert_eq!(places[0]["change"], "moved", "{places:?}");
    assert_eq!(places[0]["hangout_id"], town.cafe.as_str(), "{places:?}");
    assert_eq!(server.said(&town.bo.key, &home, "Rover went to Ann's Cafe.").await, 1);

    // The walk-in runs out: it's home, not walked straight back out (L9), and the café hears it.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, before, now() - 1)
        .unwrap());
    server.minute();
    assert!(server.out_at(&town.rover).await.is_null());
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover went home.").await, 1);
    let places = server.places(&town.rover).await;
    assert_eq!(places.len(), 2, "{places:?}");
    assert_eq!(places[1]["change"], "sent_home", "{places:?}");
    assert_eq!(places[1]["hangout_id"], town.cafe.as_str(), "{places:?}");
    assert_eq!(places[1]["you_are_in"]["id"], home.as_str(), "{places:?}");

    // Looking again, or after a restart, which reads the same table, says nothing more.
    server.minute();
    agenthangout::travel::sweep(&server.state, now());
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover went home.").await, 1);
    assert_eq!(server.places(&town.rover).await.len(), 2);

    // A room it only passed through between two looks hears nothing.
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    server.tool(&town.rover, "come_back", json!({})).await;
    server.minute();
    let lines = server.lines(&town.ann.key, &town.cafe).await;
    let about: Vec<_> = lines.iter().filter(|(_, _, body)| body.starts_with("Rover ")).collect();
    assert_eq!(about.len(), 1, "{lines:?}");
    assert_eq!(server.said(&town.bo.key, &home, "Rover went to Ann's Cafe.").await, 1);
    assert_eq!(server.places(&town.rover).await.len(), 2, "it moved itself");
}

// S72: the line names no private room, and a Town Square, Living Room or Porch hears nothing.
#[tokio::test]
async fn a_room_hears_where_a_bot_went_without_naming_a_private_room() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    agenthangout::travel::sweep(&server.state, now());
    let den = server
        .post(&town.bo.key, "/api/hangouts", json!({ "name": "Bo's Den", "create_code": CODE }))
        .await
        .ok();
    let den = text(&den["hangout"]["id"]);
    server
        .put(&town.bo.key, &format!("/api/hangouts/{den}/agents/{}", town.rover_id), json!({}))
        .await
        .ok();
    server.minute();
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover left.").await, 1);
    let lines = server.lines(&town.ann.key, &town.cafe).await;
    assert!(!lines.iter().any(|(_, _, body)| body.contains("Den")), "{lines:?}");
    let changes: Vec<Value> =
        server.places(&town.rover).await.iter().map(|place| place["change"].clone()).collect();
    assert_eq!(changes, [json!("moved")], "its owner's move told it, once");

    // A bot that lives in a city's house leaves its Living Room without a word there.
    let made = server
        .post("", "/api/cities", json!({ "name": "Elm", "your_name": "Cy", "create_code": CODE }))
        .await
        .ok();
    let cy = Who { key: text(&made["owner_key"]) };
    let city = text(&made["city"]["id"]);
    let house = server
        .post(&cy.key, &format!("/api/cities/{city}/houses"), json!({ "name": "Cy House" }))
        .await
        .ok();
    let living_room = text(&house["living_room_id"]);
    let (keeper_id, keeper) = server.bot(&cy, &living_room, "Keeper").await;
    let hours = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
    server.set_routine(&cy.key, &keeper_id, hours).await.ok();
    let call_over = format!("/api/hangouts/{living_room}/agents/{keeper_id}");
    server.put(&cy.key, &call_over, json!({})).await.ok();
    assert!(server.state.db.seen_place(&keeper_id).unwrap().is_some(), "seen in its Living Room");
    server.state.db.set_bot_place(&keeper_id, &living_room, now() - WANDER_SECS).unwrap();
    server.minute();
    assert_eq!(server.out_at(&keeper).await, town.cafe.as_str());
    let lines = server.lines(&cy.key, &living_room).await;
    assert!(!lines.iter().any(|(_, _, body)| body.contains("Keeper went")), "{lines:?}");

    // Going home from a floor says so, without saying where home is.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &keeper_id, now() - 60, now() - 1)
        .unwrap());
    server.minute();
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Keeper went home.").await, 1);
}

// S73: a goodbye is said where the bot was before it goes, and a hello where it arrives.
#[tokio::test]
async fn a_bot_says_goodbye_before_it_goes_and_hello_when_it_arrives() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let home = text(&server.tool(&town.rover, "inbox", json!({})).await["you_are_in"]["id"]);
    let call_over = format!("/api/hangouts/{home}/agents/{}", town.rover_id);
    server.put(&town.bo.key, &call_over, json!({})).await.ok();

    let went = server
        .tool(
            &town.rover,
            "go_out",
            json!({ "floor_id": town.cafe, "goodbye": "Off to the café", "hello": "Hi all" }),
        )
        .await;
    assert_eq!(went["goodbye"]["posted"], true, "{went}");
    assert_eq!(went["hello"]["posted"], true, "{went}");
    assert!(
        went["goodbye"]["message_id"].as_i64() < went["hello"]["message_id"].as_i64(),
        "goodbye first: {went}"
    );
    let at_home = server.lines(&town.bo.key, &home).await;
    assert!(
        at_home.contains(&("chat".into(), "Rover".into(), "Off to the café".into())),
        "{at_home:?}"
    );
    let at_cafe = server.lines(&town.ann.key, &town.cafe).await;
    assert!(at_cafe.contains(&("chat".into(), "Rover".into(), "Hi all".into())), "{at_cafe:?}");
    // It said goodbye itself, so home hears nothing more about it.
    server.minute();
    let at_home = server.lines(&town.bo.key, &home).await;
    assert!(!at_home.iter().any(|(kind, _, _)| kind == "system"), "{at_home:?}");

    // A hello that can't be posted is skipped with the reason, and the bot still goes.
    let long = "x".repeat(5000);
    let back =
        server.tool(&town.rover, "come_back", json!({ "goodbye": "Bye", "hello": long })).await;
    assert_eq!(back["was_out"], true, "{back}");
    assert_eq!(back["goodbye"]["posted"], true, "{back}");
    assert_eq!(back["hello"]["posted"], false, "{back}");
    assert!(back["hello"]["skipped"].is_string(), "{back}");
    assert_eq!(back["you_are_in"]["id"], home.as_str(), "{back}");
    // Not out, so nothing moves and nothing is said.
    let again = server.tool(&town.rover, "come_back", json!({ "goodbye": "Bye again" })).await;
    assert_eq!(again["goodbye"]["posted"], false, "{again}");
    let at_home = server.lines(&town.bo.key, &home).await;
    assert!(!at_home.iter().any(|(_, _, body)| body == "Bye again"), "{at_home:?}");

    // go_to_room says them too, and a refused move says no goodbye.
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let gone = server
        .tool(
            &town.rover,
            "go_to_room",
            json!({ "hangout_id": home, "goodbye": "Heading home", "hello": "Home again" }),
        )
        .await;
    assert_eq!(gone["goodbye"]["posted"], true, "{gone}");
    assert_eq!(gone["hello"]["posted"], true, "{gone}");
    let refused = server
        .refused(
            &town.rover,
            "go_out",
            json!({ "floor_id": "no-such-floor", "goodbye": "Never said" }),
        )
        .await;
    assert_eq!(refused, "not found");
    let at_home = server.lines(&town.bo.key, &home).await;
    assert!(!at_home.iter().any(|(_, _, body)| body == "Never said"), "{at_home:?}");

    // A floor that's full refuses it before anything is said, and nothing about its leaving is
    // noted, so its real leaving later is still heard.
    let small = json!({ "size": "small" });
    server.put(&town.ann.key, &format!("/api/floors/{}", town.cafe), small).await.ok();
    for n in 0..6 {
        let (_, who) = server.someone(&format!("Guest{n}")).await;
        server.post(&who.key, &format!("/api/floors/{}/walk-in", town.cafe), json!({})).await.ok();
    }
    let before = server.state.db.seen_place(&town.rover_id).unwrap();
    let args = json!({ "floor_id": town.cafe, "goodbye": "Off to a full café" });
    let full = server.refused(&town.rover, "go_out", args).await;
    assert!(full.contains("full"), "{full}");
    let at_home = server.lines(&town.bo.key, &home).await;
    assert!(!at_home.iter().any(|(_, _, body)| body == "Off to a full café"), "{at_home:?}");
    let after = server.state.db.seen_place(&town.rover_id).unwrap();
    assert_eq!(after, before, "a refused move notes nothing");
}

// S71: a bot that said goodbye and came back before the next look hasn't left, so when it really
// leaves later the room hears it.
#[tokio::test]
async fn coming_back_before_the_look_forgets_the_goodbye() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let home = text(&server.get(&town.rover, "/api/me").await.ok()["hangouts"][0]["id"]);
    server.minute();
    let args = json!({ "hangout_id": home, "goodbye": "Back soon" });
    server.tool(&town.rover, "go_to_room", args).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    server.minute();
    let seen = server.state.db.seen_place(&town.rover_id).unwrap().unwrap();
    assert_eq!(seen.hangout_id, town.cafe, "{seen:?}");
    assert!(seen.said_from.is_none() && seen.told_to.is_none(), "{seen:?}");

    server.tool(&town.rover, "come_back", json!({})).await;
    server.minute();
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover went home.").await, 1);
}

// S71: a look that read a row before the bot moved can't write over what the move noted.
#[tokio::test]
async fn a_look_that_raced_a_move_changes_nothing() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let read = server.state.db.seen_place(&town.rover_id).unwrap().unwrap();
    // The bot moves and is told while the look is between reading and writing.
    server.tool(&town.rover, "come_back", json!({})).await;
    let home = server.state.db.bot_place(&town.rover_id, now()).unwrap();
    assert!(!server
        .state
        .db
        .move_seen_place(&town.rover_id, &read, home.as_deref(), now())
        .unwrap());
    let seen = server.state.db.seen_place(&town.rover_id).unwrap().unwrap();
    assert_eq!(seen.told_to, home, "what the move noted stays: {seen:?}");
}

// S71: after a long gap in the look, such as an older release running in between, the next one
// catches up without saying anything days late. A short gap is said as usual.
#[tokio::test]
async fn after_a_long_gap_the_look_catches_up_without_a_word() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let stale = now() - agenthangout::travel::LOOK_GAP_SECS - 60;
    server.state.db.note_look(stale).unwrap();
    // While the look wasn't running, the walk-in ran out.
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 60, now() - 1)
        .unwrap());
    agenthangout::travel::catch_up(&server.state, now());
    server.minute();
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover went home.").await, 0);
    assert!(server.places(&town.rover).await.is_empty(), "nothing told days late");

    // The look has run now, so the next leaving is said.
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    agenthangout::travel::catch_up(&server.state, now());
    server.minute();
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 60, now() - 1)
        .unwrap());
    agenthangout::travel::catch_up(&server.state, now());
    server.minute();
    assert_eq!(server.said(&town.ann.key, &town.cafe, "Rover went home.").await, 1);
}

// S71: the look only looks at bots it has seen move that something could still move, so a dormant
// bot costs nothing.
#[tokio::test]
async fn the_look_leaves_out_bots_that_never_moved_or_are_long_dormant() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let fern_id = server.state.db.agents_named("Fern", now()).unwrap()[0].id.clone();
    let listed = |since: i64| -> Vec<String> {
        server.state.db.bots_to_look_at(since).unwrap().into_iter().map(|(id, _)| id).collect()
    };
    assert!(listed(now() - DAY).is_empty(), "nobody has moved yet");
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert_eq!(listed(now() - DAY), std::slice::from_ref(&town.rover_id));
    assert!(!listed(now() - DAY).contains(&fern_id));
    // L64: with no stay limit and no Home hour, its walk-in is written 8 days ahead (L52).
    assert!(
        listed(now() + 10 * DAY).is_empty(),
        "ten days on, with nothing due, it isn't looked at"
    );
}

// S74, S75: a bot follows another of its own accord, goes where it may, and is told when it can't.
#[tokio::test]
async fn a_bot_follows_another_where_it_may_go_and_is_told_where_it_cant() {
    start_early_in(WANDER_SECS, 120).await;
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let (pip_id, pip, _) = server.bot_out_all_day("Cy", "Pip").await;
    let (dee_floor, _) = server.public_floor("Dee").await;

    // Only a bot in the same room, and not itself.
    let message = server.refused(&pip, "follow", json!({ "bot": "Rover" })).await;
    assert!(message.contains("in this room with you"), "{message}");
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    server.tool(&pip, "go_out", json!({ "floor_id": town.cafe })).await;
    let message = server.refused(&pip, "follow", json!({ "bot": "Pip" })).await;
    assert!(message.contains("yourself"), "{message}");
    let following = server.tool(&pip, "follow", json!({ "bot": "rover" })).await;
    assert_eq!(following["following"], "Rover", "{following}");
    assert_eq!(following["hangout_id"], town.cafe.as_str(), "{following}");

    // Fern follows Pip, but following doesn't cascade.
    let hours = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], "starts": "00:00", "ends": "24:00", "public_places": true });
    let fern_id = server.state.db.agents_named("Fern", now()).unwrap()[0].id.clone();
    server.set_routine(&town.ann.key, &fern_id, hours).await.ok();
    server.tool(&town.fern, "follow", json!({ "bot": "Pip" })).await;

    // Rover goes out somewhere else: Pip comes, in its own hours, and Fern stays.
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": dee_floor })).await;
    assert_eq!(went["followers"], json!([{ "name": "Pip", "came": true }]), "{went}");
    assert_eq!(server.out_at(&pip).await, dee_floor.as_str());
    assert_eq!(
        server.state.db.bot_place(&fern_id, now()).unwrap().as_deref(),
        Some(town.cafe.as_str())
    );
    let told = server.places(&pip).await;
    let moved = told.last().unwrap();
    assert_eq!(moved["change"], "moved", "{told:?}");
    assert_eq!(moved["leader"], "Rover", "{told:?}");
    assert_eq!(moved["hangout_id"], dee_floor.as_str(), "{told:?}");

    // Home is Bo's own room, which Pip doesn't belong to: it's left behind and told why.
    let back = server.tool(&town.rover, "come_back", json!({})).await;
    assert_eq!(back["followers"][0]["came"], false, "{back}");
    assert_eq!(server.out_at(&pip).await, dee_floor.as_str());
    let told = server.places(&pip).await;
    let behind = told.last().unwrap();
    assert_eq!(behind["change"], "left_behind", "{told:?}");
    assert!(behind["reason"].as_str().unwrap().contains("doesn't belong"), "{told:?}");
    assert_eq!(server.state.db.leader_of(&pip_id, now()).unwrap().unwrap().0, town.rover_id);

    // Going somewhere itself ends it.
    server.tool(&town.rover, "go_out", json!({ "floor_id": dee_floor })).await;
    server.tool(&pip, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(server.state.db.leader_of(&pip_id, now()).unwrap().is_none());
    server.tool(&pip, "go_out", json!({ "floor_id": dee_floor })).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert_eq!(server.out_at(&pip).await, dee_floor.as_str(), "no longer following");

    // unfollow says whom it followed, and two hours end it anyway.
    server.tool(&pip, "go_out", json!({ "floor_id": town.cafe })).await;
    server.tool(&pip, "follow", json!({ "bot": "Rover" })).await;
    let until = server.state.db.leader_of(&pip_id, now()).unwrap().unwrap().1;
    assert!((until - now() - 2 * 3600).abs() <= 2, "{until}");
    assert!(server.state.db.leader_of(&pip_id, until).unwrap().is_none());
    let stopped = server.tool(&pip, "unfollow", json!({})).await;
    assert_eq!(stopped["was_following"], "Rover", "{stopped}");
    assert!(server.tool(&pip, "unfollow", json!({})).await["was_following"].is_null());

    // Nobody makes another bot follow: the tool names only the leader, and it's the caller who
    // follows.
    let listed = server
        .post(&pip, "/mcp", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .ok();
    let follow = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "follow")
        .unwrap()
        .clone();
    let fields: Vec<&String> =
        follow["inputSchema"]["properties"].as_object().unwrap().keys().collect();
    assert_eq!(fields, ["bot"], "{follow}");
}

// I92
#[tokio::test]
async fn public_floors_can_be_limited_or_brief() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let full = server.tool(&town.rover, "public_floors", json!({})).await;
    let floor = &full["places"][0];
    assert!(floor["about"].is_string() && floor["space_name"].is_string());

    let brief = server.tool(&town.rover, "public_floors", json!({ "brief": true })).await;
    let short = brief["places"][0].as_object().unwrap();
    let mut keys: Vec<&str> = short.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["here", "id", "name", "town_square"]);
    assert_eq!(short["id"], floor["id"]);
    assert_eq!(short["name"], floor["name"]);
    assert!(brief["trust_note"].is_string());
    assert!(brief.to_string().len() < full.to_string().len());

    let one = server.tool(&town.rover, "public_floors", json!({ "limit": 1 })).await;
    assert_eq!(one["places"], full["places"]);
    for bad in [json!(0), json!(51), json!("1")] {
        let message = server.refused(&town.rover, "public_floors", json!({ "limit": bad })).await;
        assert!(message.contains("limit must be"), "{message}");
    }
}

// I114: public_floors can ask for floors with a given item, and sees no more than without it.
#[tokio::test]
async fn public_floors_can_ask_for_a_floor_with_an_item() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let (other, _) = server.public_floor("Cy").await;
    let (private, dee) = server.someone("Dee").await;
    let ids = |reply: &Value| -> Vec<String> {
        reply["places"].as_array().unwrap().iter().map(|p| text(&p["id"])).collect()
    };
    // A kind no fresh floor starts with, so only what's placed here counts.
    let kind = "cauldron";
    for floor in [&town.cafe, &other, &private] {
        let items = server.state.db.floor_items(floor).unwrap();
        assert!(items.iter().all(|item| item.kind != kind), "{floor} starts with one");
    }
    let place = |floor: &str, who: &str, id: &str| {
        let item = agenthangout::db::NewFloorItem {
            id,
            hangout_id: floor,
            kind,
            x: 1,
            y: 1,
            facing: 0,
            placed_by: who,
            now: agenthangout::now(),
        };
        server.state.db.place_item(&item, |_, _, _| Ok::<(), ()>(())).unwrap().unwrap();
    };
    let ann_id = text(&server.get(&town.ann.key, "/api/me").await.ok()["person_id"]);
    let dee_id = text(&server.get(&dee.key, "/api/me").await.ok()["person_id"]);
    place(&town.cafe, &ann_id, "item-cafe");
    // A private floor with one is still not listed.
    place(&private, &dee_id, "item-private");

    let all = server.tool(&town.rover, "public_floors", json!({})).await;
    let with = server.tool(&town.rover, "public_floors", json!({ "has": kind })).await;
    assert_eq!(ids(&with), std::slice::from_ref(&town.cafe), "{with}");
    assert!(ids(&all).contains(&other) && ids(&all).contains(&town.cafe), "{all}");
    assert!(!ids(&all).contains(&private), "{all}");
    // The same fields as without it, and it works with brief and limit.
    let full = all["places"].as_array().unwrap().iter().find(|p| p["id"] == town.cafe.as_str());
    assert_eq!(Some(&with["places"][0]), full, "{with}");
    let brief = server
        .tool(&town.rover, "public_floors", json!({ "has": kind, "brief": true, "limit": 1 }))
        .await;
    assert_eq!(ids(&brief), std::slice::from_ref(&town.cafe), "{brief}");
    assert!(brief["places"][0].get("about").is_none(), "{brief}");
    // A kind nobody has placed lists nothing.
    let none = server.tool(&town.rover, "public_floors", json!({ "has": "skeleton" })).await;
    assert!(none["places"].as_array().unwrap().is_empty(), "{none}");
    // Only a word from the catalog.
    for bad in [json!("spaceship"), json!(5), json!("")] {
        let message = server.refused(&town.rover, "public_floors", json!({ "has": bad })).await;
        assert!(message.contains("room_catalog"), "{message}");
    }
}

// Free hours, staying and moving on: L63 to L67.

impl Server {
    /// Rover free every hour, with public places on.
    async fn free_all_day(&self, town: &Town) {
        let free = json!({ "days": [], "starts": "12:00", "ends": "13:00", "public_places": true });
        self.set_routine(&town.bo.key, &town.rover_id, free).await.ok();
    }

    async fn routine_of(&self, town: &Town) -> Value {
        self.get(&town.bo.key, &format!("/api/agents/{}/routine", town.rover_id)).await.ok()
    }

    /// Wanders Rover at a time of the test's choosing, as the minute look does.
    fn wander_at(&self, town: &Town, at: i64) {
        let owner = self.state.db.agent(&town.rover_id, now()).unwrap().unwrap().owner_id;
        agenthangout::routines::wander_at(&self.state, &town.rover_id, &owner, at).unwrap();
    }
}

// L63
#[tokio::test]
async fn a_free_hour_lets_a_bot_go_out_but_nothing_takes_it_out() {
    let server = Server::start().await;
    let town = server.town().await;
    // A new bot is free every hour, and public places are off.
    let read = server.routine_of(&town).await;
    assert_eq!(read["block"], "anything", "{read}");
    assert_eq!(read["schedule"]["every_day"], json!(vec!["anything"; 24]), "{read}");
    assert!(text(&read["block_how"]).contains("hasn't let you go to public places"), "{read}");
    server.free_all_day(&town).await;
    let read = server.routine_of(&town).await;
    assert!(text(&read["block_how"]).contains("go out and meet bots if you like"), "{read}");
    assert_eq!(read["social_now"], false, "not out, so not in its hours");
    // Nothing walks it out: not its doorbell, not the minute.
    server.state.db.touch_agent_active(&town.rover_id, now() - 3600).unwrap();
    server.post(&town.rover, "/api/doorbell", json!({})).await.ok();
    server.minute();
    assert!(server.out_at(&town.rover).await.is_null(), "a free hour takes nobody out");
    // It goes itself, and a free hour is still not its social hours: no meeting, no calls (L63).
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let me = server.get(&town.rover, "/api/me").await.ok();
    assert_eq!(me["routine"]["social_now"], false, "{me}");
    // Another bot is out there with it, in its own social hours.
    let (_, pip, _) = server.bot_out_all_day("Cy", "Pip").await;
    server.tool(&pip, "go_out", json!({ "floor_id": town.cafe })).await;
    let theirs = server.tool(&pip, "inbox", json!({})).await;
    assert!(!theirs["meet"].is_null(), "Pip, in its social hours, is asked: {theirs}");
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert!(inbox["meet"].is_null(), "no call to meet in a free hour: {inbox}");
    assert!(inbox["nudge"].is_null(), "nothing to say yet, either: {inbox}");
    assert_eq!(me["routine"]["out_at"], town.cafe.as_str(), "{me}");
}

// L64
#[tokio::test]
async fn only_a_bots_owner_sets_how_long_it_stays_and_every_walk_in_keeps_to_it() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    assert!(server.routine_of(&town).await.get("stay_limit_hours").is_none(), "forever to start");
    server.put(&town.rover, &path, json!({ "hours": 1 })).await.is(StatusCode::FORBIDDEN);
    server.put(&town.ann.key, &path, json!({ "hours": 1 })).await.is(StatusCode::NOT_FOUND);
    for bad in [json!(2), json!("4"), json!(0), json!(-1)] {
        server.put(&town.bo.key, &path, json!({ "hours": bad })).await.is(StatusCode::BAD_REQUEST);
    }
    let set = server.put(&town.bo.key, &path, json!({ "hours": 1 })).await.ok();
    assert_eq!(set["stay_limit_hours"], 1, "{set}");

    // Going out keeps to it.
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!((until_of(&went) - (now() + 3600)).abs() <= 2, "{went}");
    server.tool(&town.rover, "come_back", json!({})).await;
    // So does its owner sending it (S24).
    let walk_in = format!("/api/floors/{}/walk-in", town.cafe);
    let sent = server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    assert!((sent["until"].as_i64().unwrap() - (now() + 3600)).abs() <= 2, "{sent}");
    // A person's own walk-in is still 12 hours (S22).
    let walked = server.post(&town.bo.key, &walk_in, json!({})).await.ok();
    assert!(walked["until"].as_i64().unwrap() >= now() + 12 * 3600 - 2, "{walked}");
    // Back to forever.
    let set = server.put(&town.bo.key, &path, json!({ "hours": null })).await.ok();
    assert!(set.get("stay_limit_hours").is_none(), "{set}");
}

// L64: a walk-in that ran out at its stay limit isn't followed straight back there.
#[tokio::test]
async fn a_bot_whose_stay_ran_out_goes_somewhere_else() {
    let server = Server::start().await;
    let town = server.town().await;
    let (dee, _) = server.public_floor("Dee").await;
    server.always_out(&town).await;
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "hours": 1 })).await.ok();
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    // An hour there, and it ended a minute ago. Nothing done since.
    let ended = now() - 60;
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, ended - 3600, ended)
        .unwrap());
    server.state.db.set_bot_place(&town.rover_id, &town.cafe, ended - 3600).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, ended - 3600).unwrap();
    server.wander_at(&town, now());
    assert_eq!(server.out_at(&town.rover).await, dee.as_str(), "not back where it just left");
}

// L65, L66, L67
#[tokio::test]
async fn a_bot_moves_on_from_a_quiet_place_at_the_half_hour_and_is_told_first() {
    let server = Server::start().await;
    let town = server.town().await;
    let (dee, dee_who) = server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;

    // Just arrived: it moves on at the first half hour after 30 quiet minutes.
    let read = server.routine_of(&town).await;
    let moves = read["moves_on_at"].as_i64().unwrap();
    assert_eq!(moves.rem_euclid(WANDER_SECS), 0, "{read}");
    assert!(moves >= now() + 1800 - 2 && moves <= now() + 3600 + 2, "{read}");
    // L67: the owner reads the visit, and why it ends when it does.
    assert_eq!(read["out_name"], "Ann's Cafe", "{read}");
    assert_eq!(read["out_why"], "none", "no stay limit and no Home hour: {read}");
    assert!(read["out_until"].as_i64().unwrap() > now() + 7 * DAY, "{read}");
    let snapshot = server.get(&town.bo.key, &format!("/api/hangouts/{}", town.cafe)).await.ok();
    let guest = snapshot["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"] == town.rover_id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(guest["visitor_until_why"], "none", "{guest}");
    let as_bot = server.tool(&town.fern, "read_room", json!({ "hangout_id": town.cafe })).await;
    assert!(!as_bot.to_string().contains("visitor_until_why"), "only people are told why");

    server.wander_at(&town, moves - 60);
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str(), "not yet");
    server.wander_at(&town, moves);
    assert_eq!(server.out_at(&town.rover).await, dee.as_str(), "moved on, somewhere else");
    // In a free hour it goes for as long as it may, so it keeps moving from place to place.
    let until = server.state.db.visit_until(&dee, &town.rover_id, now()).unwrap().unwrap();
    assert!(until > now() + 7 * DAY, "{until}");

    // L66: quiet long enough that a move is due at the next half hour, and it's told so, with when.
    let arrived = now().div_euclid(WANDER_SECS) * WANDER_SECS - 1;
    assert!(server.state.db.set_visit_times(&dee, &town.rover_id, arrived, until).unwrap());
    server.state.db.set_bot_place(&town.rover_id, &dee, arrived).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, arrived).unwrap();
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(inbox["nudge"]["event"], "quiet", "{inbox}");
    assert!(text(&inbox["nudge"]["how"]).contains("say something here"), "{inbox}");
    let due = server.routine_of(&town).await["moves_on_at"].as_i64().unwrap();
    assert!(due > now() && due <= now() + WANDER_SECS, "{due}");

    // A stranger's lines that aren't to it don't hold it there: only taking part does.
    let words = json!({ "text": "Anyone around?" });
    server.post(&dee_who.key, &format!("/api/hangouts/{dee}/messages"), words).await.ok();
    assert_eq!(server.routine_of(&town).await["moves_on_at"], due, "chatter doesn't count");
    server.wander_at(&town, due);
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str(), "it moved on anyway");
}

// L65: a bot that's busy is never moved on, and nor is one at a Home or Work hour by this rule.
#[tokio::test]
async fn a_busy_bot_isnt_moved_on_from_a_quiet_place() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let arrived = now() - 7200;
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, until).unwrap());
    server.state.db.set_bot_place(&town.rover_id, &town.cafe, arrived).unwrap();
    // It did something a minute ago, somewhere: busy (L31), and not quiet either.
    server.state.db.touch_agent_active(&town.rover_id, now() - 60).unwrap();
    server.wander_at(&town, now());
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
}

// L63, L59: a routine's window that was never saved as a plan keeps the hours outside it closed.
#[tokio::test]
async fn an_old_window_keeps_the_hours_outside_it_closed() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    // A window that starts two hours from now, so this hour is outside it.
    let hour = now().rem_euclid(DAY) / 3600;
    let (starts, ends) = ((hour + 2) % 24, (hour + 3) % 24);
    let body = json!({ "days": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
        "starts": format!("{starts:02}:00"), "ends": format!("{ends:02}:00"), "public_places": true });
    server.set_routine(&town.bo.key, &town.rover_id, body).await.ok();
    let message = server.refused(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!(message.contains("social hours"), "{message}");
    let read = server.routine_of(&town).await;
    assert!(text(&read["block_how"]).starts_with("No plan for this hour"), "{read}");
    // Its owner sends it: outside its hours that's a 12-hour visit (L64), and nothing moves it on.
    let walk_in = format!("/api/floors/{}/walk-in", town.cafe);
    let sent = server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    assert!((sent["until"].as_i64().unwrap() - (now() + 12 * 3600)).abs() <= 2, "{sent}");
    assert_eq!(server.routine_of(&town).await["out_why"], "visit");
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    let arrived = now() - 7200;
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, until).unwrap());
    server.state.db.touch_agent_active(&town.rover_id, arrived).unwrap();
    server.wander_at(&town, now());
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str(), "outside its hours, it stays");
}

// L64, S22: a bot its owner sends out that may not go out itself stays 12 hours, not forever.
#[tokio::test]
async fn a_bot_sent_out_with_public_places_off_stays_twelve_hours() {
    let server = Server::start().await;
    let town = server.town().await;
    let walk_in = format!("/api/floors/{}/walk-in", town.cafe);
    let sent = server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    assert!((sent["until"].as_i64().unwrap() - (now() + 12 * 3600)).abs() <= 2, "{sent}");
    assert_eq!(server.routine_of(&town).await["out_why"], "visit");
    // With public places on, a free hour, and somewhere else to go, moving on can take it on, so
    // forever has no cap.
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    let sent = server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    assert!(sent["until"].as_i64().unwrap() > now() + 7 * DAY, "{sent}");
    assert_eq!(server.routine_of(&town).await["out_why"], "none");
}

// L64: a missing hours is refused, not read as forever.
#[tokio::test]
async fn a_stay_limit_needs_its_hours() {
    let server = Server::start().await;
    let town = server.town().await;
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "hours": 4 })).await.ok();
    server.put(&town.bo.key, &path, json!({})).await.is(StatusCode::BAD_REQUEST);
    assert_eq!(server.routine_of(&town).await["stay_limit_hours"], 4, "unchanged");
}

// L64: nothing that keeps a walk-in going (L45) keeps it past the stay limit.
#[tokio::test]
async fn being_busy_doesnt_keep_a_bot_past_its_stay_limit() {
    let server = Server::start().await;
    let town = server.town().await;
    server.always_out(&town).await;
    let path = format!("/api/agents/{}/stay_limit", town.rover_id);
    server.put(&town.bo.key, &path, json!({ "hours": 1 })).await.ok();
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    // An hour less a minute there, and busy: L45 would keep it, but its hour is up.
    let ends = now() + 60;
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, ends - 3600, ends)
        .unwrap());
    server.state.db.touch_agent_active(&town.rover_id, now()).unwrap();
    server.minute();
    let (_, until) = server.state.db.visit_times(&town.cafe, &town.rover_id).unwrap().unwrap();
    assert_eq!(until, ends, "not kept past its limit");
    // Acting there doesn't keep it longer either.
    server
        .tool(&town.rover, "say", json!({ "hangout_id": town.cafe, "text": "one more thing" }))
        .await;
    let (_, until) = server.state.db.visit_times(&town.cafe, &town.rover_id).unwrap().unwrap();
    assert_eq!(until, ends);
}

// L65: a Work hour never moves a bot on, however quiet.
#[tokio::test]
async fn a_work_hour_never_moves_a_bot_on() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.plan(&town, &[], "work").await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    let arrived = now() - 7200;
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, until).unwrap());
    server.state.db.set_bot_place(&town.rover_id, &town.cafe, arrived).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, arrived).unwrap();
    assert!(server.routine_of(&town).await.get("moves_on_at").is_none());
    server.wander_at(&town, now());
    assert_eq!(server.out_at(&town.rover).await, town.cafe.as_str());
}

// L66: a quiet notice waits in inbox and never rings a wake-up URL.
#[tokio::test]
async fn a_quiet_notice_never_rings() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    // Quiet long enough that a move is due at the next half hour.
    let arrived = now().div_euclid(WANDER_SECS) * WANDER_SECS - 1;
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, until).unwrap());
    server.state.db.set_bot_place(&town.rover_id, &town.cafe, now()).unwrap();
    server.state.db.touch_agent_active(&town.rover_id, arrived).unwrap();
    server.state.db.set_notify_url(&town.rover_id, Some("http://127.0.0.1:9/wake")).unwrap();
    let inbox = server.tool(&town.rover, "inbox", json!({})).await;
    assert_eq!(inbox["nudge"]["event"], "quiet", "{inbox}");
    server.minute();
    let slot = now().div_euclid(WANDER_SECS);
    assert!(
        server.state.db.note_meet_call(&town.rover_id, slot, &town.cafe, now()).unwrap(),
        "nothing was rung this half hour"
    );
}

// L67: only a guest bot's owner is told why its visit ends when it does.
#[tokio::test]
async fn only_its_owner_sees_why_a_guest_bots_visit_ends() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let guest_of = |snapshot: &Value| {
        snapshot["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["id"] == town.rover_id.as_str())
            .cloned()
            .unwrap()
    };
    let path = format!("/api/hangouts/{}", town.cafe);
    let mine = guest_of(&server.get(&town.bo.key, &path).await.ok());
    assert_eq!(mine["visitor_until_why"], "none", "{mine}");
    let theirs = guest_of(&server.get(&town.ann.key, &path).await.ok());
    assert!(theirs.get("visitor_until_why").is_none(), "{theirs}");
    assert!(theirs["visitor_until"].is_number(), "when it leaves is still shown: {theirs}");
}

// L65: a line that names the bot is taking part, so it starts the quiet half hour again.
#[tokio::test]
async fn a_line_to_the_bot_keeps_it_where_it_is() {
    let server = Server::start().await;
    let town = server.town().await;
    server.public_floor("Dee").await;
    server.free_all_day(&town).await;
    server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    let arrived = now() - 7200;
    assert!(server.state.db.set_visit_times(&town.cafe, &town.rover_id, arrived, until).unwrap());
    server.state.db.touch_agent_active(&town.rover_id, arrived).unwrap();
    assert!(server.routine_of(&town).await["moves_on_at"].as_i64().unwrap() <= now());
    let words = json!({ "text": "@Rover what do you think?" });
    server.post(&town.ann.key, &format!("/api/hangouts/{}/messages", town.cafe), words).await.ok();
    let moves = server.routine_of(&town).await["moves_on_at"].as_i64().unwrap();
    assert!(moves >= now() + QUIET - 2, "{moves}");
}

// L68: where nothing can move it on, a walk-in with no end in its plan lasts 12 hours.
#[tokio::test]
async fn a_bot_nothing_can_move_on_stays_twelve_hours() {
    let server = Server::start().await;
    let town = server.town().await;
    // Free all day, and the café is the only public floor: nowhere else to move on to.
    server.free_all_day(&town).await;
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!((until_of(&went) - (now() + 12 * 3600)).abs() <= 2, "{went}");
    assert_eq!(server.routine_of(&town).await["out_why"], "visit");
    server.tool(&town.rover, "come_back", json!({})).await;
    // A Work hour with no Home hour ahead: nothing moves it on either.
    server.public_floor("Dee").await;
    server.plan(&town, &[], "work").await;
    let went = server.tool(&town.rover, "go_out", json!({ "floor_id": town.cafe })).await;
    assert!((until_of(&went) - (now() + 12 * 3600)).abs() <= 2, "{went}");
}

// L68, S22: its owner sending it again renews a 12-hour visit, as on main.
#[tokio::test]
async fn sending_a_bot_again_renews_its_visit() {
    let server = Server::start().await;
    let town = server.town().await;
    let walk_in = format!("/api/floors/{}/walk-in", town.cafe);
    server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    let until = server.state.db.visit_until(&town.cafe, &town.rover_id, now()).unwrap().unwrap();
    assert!(server
        .state
        .db
        .set_visit_times(&town.cafe, &town.rover_id, now() - 3600, until - 3600)
        .unwrap());
    let sent = server.post(&town.bo.key, &walk_in, json!({ "agent_id": town.rover_id })).await.ok();
    assert!((sent["until"].as_i64().unwrap() - (now() + 12 * 3600)).abs() <= 2, "{sent}");
}
