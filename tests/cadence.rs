//! RP1 to RP9: repeating plans, phase 1, through the real routes and the loop's own tick.
mod support;

use agenthangout::{app, cadence, db::Db, now, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const DAY: i64 = 86_400;

struct Server {
    base: String,
    client: Client,
    state: AppState,
}
struct Who {
    key: String,
    id: String,
    room: String,
}
impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: Some("qa".into()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self { base, client: Client::new(), state }
    }
    async fn request(
        &self,
        key: &str,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let r = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let text = r.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(json!(text)))
    }
    async fn owner(&self, name: &str) -> Who {
        let (status, v) = self
            .request(
                "",
                Method::POST,
                "/api/hangouts",
                json!({"name":format!("{name} room"),"your_name":name,"create_code":"qa"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        Who {
            key: v["owner_key"].as_str().unwrap().into(),
            id: v["person"]["id"].as_str().unwrap().into(),
            room: v["hangout"]["id"].as_str().unwrap().into(),
        }
    }
    async fn plan(&self, owner: &Who) -> String {
        let (status, plan) = self
            .request(
                &owner.key,
                Method::POST,
                &format!("/api/hangouts/{}/events", owner.room),
                json!({ "title": "Morning check-in", "starts_at": "2031-06-06T09:00:00Z" }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        plan["id"].as_str().unwrap().to_string()
    }
    fn outcomes(&self, bot: &str) -> Vec<String> {
        self.state
            .db
            .starts_of_bot(bot, 0)
            .unwrap()
            .into_iter()
            .map(|(_, _, _, _, outcome)| outcome)
            .collect()
    }
}

/// The start of the UTC day after `at`, plus `secs`.
fn tomorrow_at(at: i64, secs: i64) -> i64 {
    (at.div_euclid(DAY) + 1) * DAY + secs
}

// RP1, RP2: only the plan's owner, as a person, and only a cadence phase 1 knows.
#[tokio::test]
async fn only_the_plans_owner_makes_it_repeat_and_bad_cadences_are_refused() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = s.owner("Bo").await;
    let (_, token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let daily = json!({ "freq": "daily", "time": "09:00" });

    assert_eq!(s.request(&token, Method::PUT, &path, daily.clone()).await.0, StatusCode::FORBIDDEN);
    assert_ne!(s.request(&bo.key, Method::PUT, &path, daily.clone()).await.0, StatusCode::OK);
    for bad in [
        json!({ "freq": "hourly", "time": "09:00" }),
        json!({ "freq": "weekly", "time": "09:00" }),
        json!({ "freq": "daily", "time": "9am" }),
        json!({ "freq": "daily", "time": "24:00" }),
        json!({ "freq": "daily", "time": "09:00", "days": ["funday"] }),
        json!({ "freq": "daily", "time": "09:00", "until": "2020-01-01" }),
        json!({ "freq": "daily", "time": "09:00", "until": "2031-04-31" }),
    ] {
        let (status, body) = s.request(&ann.key, Method::PUT, &path, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} {body}");
    }
    let (status, set) = s.request(&ann.key, Method::PUT, &path, daily).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["time"], "09:00");
    assert_eq!(set["days"].as_array().unwrap().len(), 7);
    assert!(set["next_start"].is_string(), "{set}");
}

// RP3, RP4, RP5: each start cues the owner's bot in the room once, and a late one is missed.
#[tokio::test]
async fn each_start_cues_once_and_a_late_start_is_missed() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, rover_token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let (status, _) =
        s.request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:00" })).await;
    assert_eq!(status, StatusCode::OK);

    // Ann has saved no clock, so 9:00 is 9:00 UTC. Tomorrow at 9:05 it's due.
    let nine = tomorrow_at(now(), 9 * 3600);
    assert_eq!(cadence::tick(&s.state, nine - 60).unwrap(), 0, "not yet");
    assert_eq!(cadence::tick(&s.state, nine + 300).unwrap(), 1);
    // A second tick, or a second server, cues nobody again.
    assert_eq!(cadence::tick(&s.state, nine + 400).unwrap(), 0);
    assert_eq!(s.outcomes(&rover.id), ["cued"]);
    // The bot heard it as a plan event with reason cue and the room to go to.
    let events = s.request(&rover_token, Method::GET, "/api/events", json!(null)).await.1;
    let cue = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "plan" && event["data"]["reason"] == "cue")
        .cloned()
        .unwrap_or_else(|| panic!("no cue in {events}"));
    assert_eq!(cue["data"]["hangout_id"], ann.room.as_str());

    // The next day the loop first runs at 9:20: too late, so it's missed, not cued.
    assert_eq!(cadence::tick(&s.state, nine + DAY + 20 * 60).unwrap(), 0);
    assert_eq!(s.outcomes(&rover.id), ["missed", "cued"]);
    // inbox says so, for the day just gone.
    let (_, inbox) = s.request(&rover_token, Method::GET, "/api/inbox", json!(null)).await;
    assert!(inbox["plan_starts"].is_array(), "{inbox}");
}

// RP5, RP6, RP9: a paused bot is recorded paused; weekdays, until and canceling are kept to.
#[tokio::test]
async fn paused_bots_weekdays_until_and_ending_are_kept_to() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let nine = tomorrow_at(now(), 9 * 3600);
    // Only the weekday tomorrow is.
    let weekday = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        [((nine.div_euclid(DAY) + 3) % 7) as usize];
    let weekly = json!({ "freq": "weekly", "days": [weekday], "time": "09:00" });
    assert_eq!(s.request(&ann.key, Method::PUT, &path, weekly).await.0, StatusCode::OK);

    // Paused by its owner, the bot's start is recorded as paused, not missed.
    s.state.db.set_doorbell_paused(&rover.id, true).unwrap();
    cadence::tick(&s.state, nine + 60).unwrap();
    assert_eq!(s.outcomes(&rover.id), ["paused"]);
    s.state.db.set_doorbell_paused(&rover.id, false).unwrap();
    // The next day isn't one of its days.
    assert_eq!(cadence::tick(&s.state, nine + DAY + 60).unwrap(), 0);
    assert_eq!(s.outcomes(&rover.id).len(), 1);
    // A week on it's due again.
    assert_eq!(cadence::tick(&s.state, nine + 7 * DAY + 60).unwrap(), 1);

    // Ended, nothing more is cued, and what happened stays on the record.
    assert_eq!(s.request(&ann.key, Method::DELETE, &path, json!(null)).await.0, StatusCode::OK);
    assert_eq!(cadence::tick(&s.state, nine + 14 * DAY + 60).unwrap(), 0);
    assert_eq!(s.outcomes(&rover.id).len(), 2);
}

// RP6, RP5: past 24 cues in a UTC day a start is over_cap; a plan set to paused records nothing.
#[tokio::test]
async fn the_daily_cap_and_a_paused_plan() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let nine = tomorrow_at(now(), 9 * 3600);
    // 24 plans, each cueing Rover at 9:00, fill its day; a 25th is over the cap.
    for _ in 0..25 {
        let plan = s.plan(&ann).await;
        let path = format!("/api/events/{plan}/cadence");
        let (status, body) = s
            .request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:00" }))
            .await;
        if status != StatusCode::OK {
            // RP1: at most 20 repeating plans each.
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
            break;
        }
    }
    cadence::tick(&s.state, nine + 60).unwrap();
    assert_eq!(s.outcomes(&rover.id).len(), 20, "20 plans each, so 20 starts");
    assert!(s.outcomes(&rover.id).iter().all(|o| o == "cued"));
    // Fill the day to the cap directly, then one more start is over_cap.
    let extra = s.plan(&ann).await;
    for _ in 0..4 {
        let plan = s.plan(&ann).await;
        s.state.db.handle_start(&plan, nine + 30, &[(rover.id.clone(), "cued")], now()).unwrap();
    }
    s.state.db.end_cadence(&s.state.db.running_cadences().unwrap()[0].event_id).unwrap();
    let path = format!("/api/events/{extra}/cadence");
    assert_eq!(
        s.request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:30" }))
            .await
            .0,
        StatusCode::OK
    );
    cadence::tick(&s.state, nine + 31 * 60).unwrap();
    assert_eq!(s.outcomes(&rover.id)[0], "over_cap", "{:?}", s.outcomes(&rover.id));

    // A plan set to paused has no starts at all while paused.
    let quiet = s.plan(&ann).await;
    s.state.db.end_cadence(&s.state.db.running_cadences().unwrap()[0].event_id).unwrap();
    let path = format!("/api/events/{quiet}/cadence");
    let paused = json!({ "freq": "daily", "time": "10:00", "paused": true });
    assert_eq!(s.request(&ann.key, Method::PUT, &path, paused).await.0, StatusCode::OK);
    let before = s.outcomes(&rover.id).len();
    cadence::tick(&s.state, nine + 3600 + 60).unwrap();
    assert_eq!(s.outcomes(&rover.id).len(), before, "nothing recorded for a paused plan");
}
