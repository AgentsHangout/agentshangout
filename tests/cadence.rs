//! RP1 to RP11: repeating plans, through the real routes and the loop's own tick.
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
    /// Each running cadence counts starts only from an hour before `nine`, as if it was set then,
    /// so how a test runs doesn't hang on the time of day: before 9:00 UTC, setting one at the real
    /// time would also make today's 9:00 a start.
    fn pin(&self, nine: i64) {
        for mut cadence in self.state.db.running_cadences().unwrap() {
            cadence.since = nine - 3600;
            self.state.db.set_cadence(&cadence).unwrap();
        }
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
        json!({ "freq": "yearly", "time": "09:00" }),
        // RP10: times.
        json!({ "freq": "daily" }),
        json!({ "freq": "daily", "time": "09:00", "times": ["13:00"] }),
        json!({ "freq": "daily", "times": ["08:00", "10:00", "12:00", "14:00", "16:00"] }),
        json!({ "freq": "daily", "times": ["09:00", "09:30"] }),
        json!({ "freq": "daily", "times": ["00:10", "23:30"] }),
        json!({ "freq": "daily", "time": "09:00", "interval": 2 }),
        json!({ "freq": "daily", "time": "09:00", "month_day": 1 }),
        // RP10: a monthly day.
        json!({ "freq": "monthly", "time": "10:00" }),
        json!({ "freq": "monthly", "time": "10:00", "month_day": 29 }),
        json!({ "freq": "monthly", "time": "10:00", "month_day": 0 }),
        json!({ "freq": "monthly", "time": "10:00", "month_day": 1, "days": ["mon"] }),
        // RP11: an hourly window.
        json!({ "freq": "hourly", "interval": 3 }),
        json!({ "freq": "hourly", "window": { "from": "09:00", "to": "17:00" } }),
        json!({ "freq": "hourly", "interval": 0, "window": { "from": "09:00", "to": "17:00" } }),
        json!({ "freq": "hourly", "interval": 24, "window": { "from": "09:00", "to": "17:00" } }),
        json!({ "freq": "hourly", "interval": 3, "window": { "from": "17:00", "to": "09:00" } }),
        json!({ "freq": "hourly", "interval": 3, "window": { "from": "9", "to": "17:00" } }),
    ] {
        let (status, body) = s.request(&ann.key, Method::PUT, &path, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} {body}");
    }
    let (status, set) = s.request(&ann.key, Method::PUT, &path, daily).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["time"], "09:00");
    assert_eq!(set["freq"], "daily");
    assert_eq!(set["days"].as_array().unwrap().len(), 7);
    assert!(set["next_start"].is_string(), "{set}");
}

// RP10, RP11: several times a day, a day of the month and an hourly window, each read back.
#[tokio::test]
async fn new_shapes_are_set_and_read_back() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");

    // Days that never follow each other can hold times under an hour apart across midnight.
    let (status, set) = s
        .request(
            &ann.key,
            Method::PUT,
            &path,
            json!({ "freq": "weekly", "days": ["mon", "thu"], "times": ["23:30", "00:10"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["times"], json!(["00:10", "23:30"]));
    assert_eq!(set["time"], "00:10");

    let (status, set) = s
        .request(
            &ann.key,
            Method::PUT,
            &path,
            json!({ "freq": "monthly", "month_day": 1, "times": ["10:00"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["freq"], "monthly");
    assert_eq!(set["month_day"], 1);
    assert_eq!(set["days"], json!([]));
    assert!(set["next_start"].as_str().unwrap().contains("-01T10:00"), "{set}");

    let hourly = json!({
        "freq": "hourly", "interval": 3, "days": ["mon", "tue", "wed", "thu", "fri"],
        "window": { "from": "09:00", "to": "17:00" }
    });
    let (status, set) = s.request(&ann.key, Method::PUT, &path, hourly).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["freq"], "hourly");
    assert_eq!(set["interval"], 3);
    assert_eq!(set["window"], json!({ "from": "09:00", "to": "17:00" }));
    assert_eq!(set["times"], json!(["09:00", "12:00", "15:00"]));
    assert_eq!(s.state.db.cadence(&plan).unwrap().unwrap().days, 0b001_1111);
}

// RP8: a cadence copies its owner's time zone when it's set, and keeps the saved offset beside it.
// A zone the server doesn't know isn't kept.
#[tokio::test]
async fn a_cadence_copies_its_owners_time_zone() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let clock = |zone: &str| json!({ "time_zone": zone, "utc_offset_minutes": -360 });
    let daily = json!({ "freq": "daily", "time": "09:00" });

    let (status, saved) =
        s.request(&ann.key, Method::PUT, "/api/clock", clock("America/Chicago")).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, set) = s.request(&ann.key, Method::PUT, &path, daily.clone()).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["time_zone"], "America/Chicago");
    assert_eq!(set["utc_offset_min"], -360);
    assert!(set.get("clock_note").is_none(), "{set}");
    let (_, read) = s.request(&ann.key, Method::GET, &path, json!(null)).await;
    assert_eq!(read["cadence"]["time_zone"], "America/Chicago", "{read}");

    // Moving doesn't move it: a new clock changes nothing until the cadence is set again.
    s.request(&ann.key, Method::PUT, "/api/clock", clock("America/Denver")).await;
    assert_eq!(
        s.state.db.cadence(&plan).unwrap().unwrap().time_zone.as_deref(),
        Some("America/Chicago")
    );

    let (status, _) = s.request(&ann.key, Method::PUT, "/api/clock", clock("Mars/Olympus")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, set) = s.request(&ann.key, Method::PUT, &path, daily).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["time_zone"], Value::Null);
    assert_eq!(set["utc_offset_min"], -360);
}

// RP10, RP11, RP4: each start of the day cues once, keyed on its own UTC start.
#[tokio::test]
async fn every_start_in_a_day_cues_once() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let twice = json!({ "freq": "daily", "times": ["09:00", "13:00"] });
    assert_eq!(s.request(&ann.key, Method::PUT, &path, twice).await.0, StatusCode::OK);
    let nine = tomorrow_at(now(), 9 * 3600);
    s.pin(nine);
    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 1);
    assert_eq!(cadence::tick(&s.state, nine + 3 * 3600).unwrap(), 0, "not 1 pm yet");
    assert_eq!(cadence::tick(&s.state, nine + 4 * 3600 + 60).unwrap(), 1);
    assert_eq!(s.outcomes(&rover.id), ["cued", "cued"]);

    // Every four hours from 9 to 5 is 9, 1 and 5, the window's end included.
    let hourly =
        json!({ "freq": "hourly", "interval": 4, "window": { "from": "09:00", "to": "17:00" } });
    assert_eq!(s.request(&ann.key, Method::PUT, &path, hourly).await.0, StatusCode::OK);
    let next = nine + DAY;
    s.pin(next);
    for hour in [0, 4, 8] {
        assert_eq!(cadence::tick(&s.state, next + hour * 3600 + 60).unwrap(), 1, "{hour}");
    }
    assert_eq!(cadence::tick(&s.state, next + 12 * 3600 + 60).unwrap(), 0);
    assert_eq!(s.outcomes(&rover.id).len(), 5);
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
    s.pin(nine);
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
    s.pin(nine);

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
    s.pin(nine);
    cadence::tick(&s.state, nine + 60).unwrap();
    assert_eq!(s.outcomes(&rover.id).len(), 20, "20 plans each, so 20 starts");
    assert!(s.outcomes(&rover.id).iter().all(|o| o == "cued"));
    // Fill the day to the cap directly, then one more start is over_cap.
    let extra = s.plan(&ann).await;
    for _ in 0..4 {
        let plan = s.plan(&ann).await;
        s.state
            .db
            .handle_start(
                &plan,
                nine + 30,
                &[(rover.id.clone(), "cued")],
                cadence::DAILY_CUES,
                now(),
            )
            .unwrap();
    }
    s.state.db.end_cadence(&s.state.db.running_cadences().unwrap()[0].event_id).unwrap();
    let path = format!("/api/events/{extra}/cadence");
    assert_eq!(
        s.request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:30" }))
            .await
            .0,
        StatusCode::OK
    );
    s.pin(nine);
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

// RP3: a start really rings the bot's wake-up URL, two plans starting together ring it once, and
// a paused bot isn't rung; with no wake-up URL the cue still reaches its event log.
#[tokio::test]
async fn a_cue_really_rings_once_and_never_a_paused_bot() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let hook = axum::Router::new().fallback(axum::routing::post(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/wake", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });
    let settle = || tokio::time::sleep(std::time::Duration::from_millis(400));

    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, rover_token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    s.state.db.set_notify_url(&rover.id, Some(&url)).unwrap();
    // Two plans at the same time.
    for _ in 0..2 {
        let plan = s.plan(&ann).await;
        let path = format!("/api/events/{plan}/cadence");
        let (status, _) = s
            .request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:00" }))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    let nine = tomorrow_at(now(), 9 * 3600);
    s.pin(nine);
    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 2, "both cued");
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "rung once for the two");

    // Paused the next day: recorded, never rung.
    s.state.db.set_doorbell_paused(&rover.id, true).unwrap();
    cadence::tick(&s.state, nine + DAY + 60).unwrap();
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    s.state.db.set_doorbell_paused(&rover.id, false).unwrap();

    // With no wake-up URL, the cue isn't rung but is still in its event log.
    s.state.db.set_notify_url(&rover.id, None).unwrap();
    assert_eq!(cadence::tick(&s.state, nine + 2 * DAY + 60).unwrap(), 2);
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let (_, events) = s.request(&rover_token, Method::GET, "/api/events", json!(null)).await;
    let cues = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["name"] == "plan" && event["data"]["reason"] == "cue")
        .count();
    assert!(cues >= 2, "{events}");
}

// RP3, RP4, RP6: two ticks at the same moment, as two servers would, take a start once and ring the
// bot once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_ticks_at_once_take_a_start_once_and_ring_once() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let hook = axum::Router::new().fallback(axum::routing::post(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/wake", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });

    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    s.state.db.set_notify_url(&rover.id, Some(&url)).unwrap();
    for _ in 0..3 {
        let plan = s.plan(&ann).await;
        let path = format!("/api/events/{plan}/cadence");
        let (status, _) = s
            .request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:00" }))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    let at = tomorrow_at(now(), 9 * 3600) + 60;
    s.pin(at - 60);
    let ticks: Vec<_> = (0..4)
        .map(|_| {
            let state = s.state.clone();
            tokio::task::spawn_blocking(move || cadence::tick(&state, at).unwrap())
        })
        .collect();
    let mut cued = 0;
    for tick in ticks {
        cued += tick.await.unwrap();
    }
    assert_eq!(cued, 3, "each of the three starts cued once, whichever tick took it");
    assert_eq!(s.outcomes(&rover.id).len(), 3);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "rung once for the three");
}

// RP3, RP9: a plan canceled after its start was taken doesn't ring: the send checks again.
#[tokio::test]
async fn a_cue_for_a_plan_canceled_before_it_sends_doesnt_ring() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let hook = axum::Router::new().fallback(axum::routing::post(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/wake", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });

    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    s.state.db.set_notify_url(&rover.id, Some(&url)).unwrap();
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");
    let (status, _) =
        s.request(&ann.key, Method::PUT, &path, json!({ "freq": "daily", "time": "09:00" })).await;
    assert_eq!(status, StatusCode::OK);
    let notice = agenthangout::notify::Notice {
        event: "plan",
        hangout_id: ann.room.clone(),
        text: "A plan you're in has started. Call inbox.".into(),
    };
    // While it stands, the send goes through.
    agenthangout::notify::send_cue(&s.state, &rover.id, &notice, &plan, &ann.id);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    // Canceled after the start was taken, the same send doesn't.
    let (status, body) =
        s.request(&ann.key, Method::POST, &format!("/api/events/{plan}/cancel"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    agenthangout::notify::send_cue(&s.state, &rover.id, &notice, &plan, &ann.id);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no ring for a canceled plan");
}

// RP4: a cadence counts starts from 30 seconds before it was set. Set just before 9:00, or within
// 30 seconds after, that day's 9:00 is a start and, ticked the next morning, is missed; set later,
// only the next day's is. Each case is set over HTTP, then dated as if set at that time of day.
#[tokio::test]
async fn a_cadence_counts_starts_from_30_seconds_before_it_was_set() {
    let nine = tomorrow_at(now(), 9 * 3600);
    for (set_at, want) in [
        (nine - 10, vec!["cued", "missed"]),
        (nine + 30, vec!["cued", "missed"]),
        (nine + 31, vec!["cued"]),
        (nine + 40, vec!["cued"]),
    ] {
        let s = Server::start().await;
        let ann = s.owner("Ann").await;
        let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
        let plan = s.plan(&ann).await;
        let path = format!("/api/events/{plan}/cadence");
        let before = now();
        let daily = json!({ "freq": "daily", "time": "09:00" });
        assert_eq!(s.request(&ann.key, Method::PUT, &path, daily).await.0, StatusCode::OK);
        let mut cadence = s.state.db.cadence(&plan).unwrap().unwrap();
        assert!((before - 30..=now() - 30).contains(&cadence.since), "set now, less 30 seconds");
        cadence.since = set_at - 30;
        s.state.db.set_cadence(&cadence).unwrap();
        cadence::tick(&s.state, nine + DAY + 300).unwrap();
        let mut got = s.outcomes(&rover.id);
        got.sort();
        assert_eq!(got, want, "set at 9:00 {:+} seconds", set_at - nine);
    }
}

/// A person with no room of their own in Ann's, made a member of `room`.
async fn member_of(s: &Server, room: &str, name: &str) -> Who {
    let who = s.owner(name).await;
    s.state.db.add_member(room, &who.id, now()).unwrap();
    who
}

fn cues(events: &Value) -> Vec<Value> {
    events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["name"] == "plan" && event["data"]["reason"] == "cue")
        .cloned()
        .collect()
}

// RP7: another member's bot is cued only with its owner's yes, only while it belongs to the room,
// and a no stops it.
#[tokio::test]
async fn another_members_bot_is_cued_only_with_their_yes() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = member_of(&s, &ann.room, "Bo").await;
    let cy = s.owner("Cy").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let (spot, spot_token) = support::legacy_bot(&s.state, &bo.id, &ann.room, "Spot");
    // Bo's other bot lives in Bo's own room, not Ann's.
    let (rex, _) = support::legacy_bot(&s.state, &bo.id, &bo.room, "Rex");
    let plan = s.plan(&ann).await;
    let daily = json!({ "freq": "daily", "time": "09:00" });
    let (status, _) =
        s.request(&ann.key, Method::PUT, &format!("/api/events/{plan}/cadence"), daily).await;
    assert_eq!(status, StatusCode::OK);
    let path = format!("/api/events/{plan}/cue-bots");
    let nine = tomorrow_at(now(), 9 * 3600);
    s.pin(nine);

    // Before any yes, only Ann's own bot is cued.
    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 1);
    assert_eq!(s.outcomes(&rover.id), ["cued"]);
    assert!(s.outcomes(&spot.id).is_empty());

    // Only Bo, as a person, says yes for Bo's bots, and only for ones already in the room.
    let yes = |bots: Vec<&str>| json!({ "bots": bots });
    let refused = [
        (&spot_token, yes(vec![&spot.id]), StatusCode::FORBIDDEN),
        (&ann.key, yes(vec![&rover.id]), StatusCode::BAD_REQUEST),
        (&bo.key, yes(vec![&rover.id]), StatusCode::FORBIDDEN),
        (&bo.key, yes(vec![&rex.id]), StatusCode::BAD_REQUEST),
    ];
    for (key, body, want) in refused {
        let (status, reply) = s.request(key, Method::PUT, &path, body.clone()).await;
        assert_eq!(status, want, "{body} {reply}");
    }
    let (status, reply) = s.request(&cy.key, Method::PUT, &path, yes(vec![])).await;
    assert_ne!(status, StatusCode::OK, "{reply}");
    let (status, reply) = s.request(&bo.key, Method::PUT, &path, yes(vec![&spot.id])).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["bots"], json!([spot.id]));

    // The next day Spot is cued too, and its cue says it isn't from its owner.
    assert_eq!(cadence::tick(&s.state, nine + DAY + 60).unwrap(), 2);
    assert_eq!(s.outcomes(&spot.id), ["cued"]);
    let (_, events) = s.request(&spot_token, Method::GET, "/api/events", json!(null)).await;
    let cue = cues(&events).pop().unwrap_or_else(|| panic!("no cue in {events}"));
    assert_eq!(cue["data"]["from_your_owner"], false);
    assert_eq!(cue["data"]["plan_owner_id"], ann.id.as_str());

    // A no stops it; past starts stay on the record.
    assert_eq!(s.request(&bo.key, Method::PUT, &path, yes(vec![])).await.0, StatusCode::OK);
    assert_eq!(cadence::tick(&s.state, nine + 2 * DAY + 60).unwrap(), 1);
    assert_eq!(s.outcomes(&spot.id), ["cued"]);

    // A yes for a bot that then leaves the room cues nothing.
    assert_eq!(s.request(&bo.key, Method::PUT, &path, yes(vec![&spot.id])).await.0, StatusCode::OK);
    s.state.db.remove_agent_from_room(&ann.room, &spot.id).unwrap();
    assert_eq!(cadence::tick(&s.state, nine + 3 * DAY + 60).unwrap(), 1);
    assert_eq!(s.outcomes(&spot.id), ["cued"]);
}

// RP7: another owner's plan rings their bot, but not past its owner's only_me (E25).
#[tokio::test]
async fn another_owners_cue_rings_unless_only_me() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let hook = axum::Router::new().fallback(axum::routing::post(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/wake", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });
    let settle = || tokio::time::sleep(std::time::Duration::from_millis(400));

    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = member_of(&s, &ann.room, "Bo").await;
    let (spot, spot_token) = support::legacy_bot(&s.state, &bo.id, &ann.room, "Spot");
    let plan = s.plan(&ann).await;
    let daily = json!({ "freq": "daily", "time": "09:00" });
    s.request(&ann.key, Method::PUT, &format!("/api/events/{plan}/cadence"), daily).await;
    let (status, _) = s
        .request(
            &bo.key,
            Method::PUT,
            &format!("/api/events/{plan}/cue-bots"),
            json!({ "bots": [spot.id] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let nine = tomorrow_at(now(), 9 * 3600);
    s.pin(nine);
    // Set after the plan is made, so making it rings nothing here.
    s.state.db.set_notify_url(&spot.id, Some(&url)).unwrap();
    settle().await;

    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 1);
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "rung with Bo's yes");

    // With only_me, it's still cued and in its events, but not rung.
    s.state.db.set_wakes_only_for_owner(&spot.id, true).unwrap();
    assert_eq!(cadence::tick(&s.state, nine + DAY + 60).unwrap(), 1);
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "only_me stops another owner's ring");
    let (_, events) = s.request(&spot_token, Method::GET, "/api/events", json!(null)).await;
    assert_eq!(cues(&events).len(), 2, "{events}");
}

// RP10: a monthly plan starts on its day through the loop, and not the day after.
#[tokio::test]
async fn a_monthly_plan_starts_on_its_day() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let (rover, _) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let plan = s.plan(&ann).await;
    let monthly = json!({ "freq": "monthly", "month_day": 1, "time": "09:00" });
    let (status, set) =
        s.request(&ann.key, Method::PUT, &format!("/api/events/{plan}/cadence"), monthly).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let first = time::OffsetDateTime::parse(
        set["next_start"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!((first.day(), first.hour()), (1, 9), "{set}");
    let nine = first.unix_timestamp();
    s.pin(nine);
    assert_eq!(cadence::tick(&s.state, nine - 60).unwrap(), 0, "not yet");
    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 1);
    assert_eq!(cadence::tick(&s.state, nine + DAY + 60).unwrap(), 0, "the 2nd has none");
    assert_eq!(s.outcomes(&rover.id), ["cued"]);
}

// RP7: a yes ends when its giver or its bot leaves the room, so letting them back in doesn't
// quietly bring it back.
#[tokio::test]
async fn a_yes_ends_when_its_giver_or_its_bot_leaves_the_room() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = member_of(&s, &ann.room, "Bo").await;
    let (spot, _) = support::legacy_bot(&s.state, &bo.id, &ann.room, "Spot");
    let plan = s.plan(&ann).await;
    let daily = json!({ "freq": "daily", "time": "09:00" });
    s.request(&ann.key, Method::PUT, &format!("/api/events/{plan}/cadence"), daily).await;
    let path = format!("/api/events/{plan}/cue-bots");
    let yes = json!({ "bots": [spot.id] });
    assert_eq!(s.request(&bo.key, Method::PUT, &path, yes.clone()).await.0, StatusCode::OK);
    let nine = tomorrow_at(now(), 9 * 3600);
    s.pin(nine);
    assert_eq!(cadence::tick(&s.state, nine + 60).unwrap(), 1);

    // Bo leaves, taking Spot, and both are let back in: no yes until Bo says it again.
    s.state.db.remove_member(&ann.room, &bo.id).unwrap();
    assert!(s.state.db.cue_bots_of(&plan, &bo.id).unwrap().is_empty());
    s.state.db.add_member(&ann.room, &bo.id, now()).unwrap();
    s.state.db.add_agent_to_room(&ann.room, &spot.id).unwrap();
    assert_eq!(cadence::tick(&s.state, nine + DAY + 60).unwrap(), 0);

    // The same when only the bot leaves and comes back.
    assert_eq!(s.request(&bo.key, Method::PUT, &path, yes).await.0, StatusCode::OK);
    s.state.db.remove_agent_from_room(&ann.room, &spot.id).unwrap();
    s.state.db.add_agent_to_room(&ann.room, &spot.id).unwrap();
    assert!(s.state.db.cue_bots_of(&plan, &bo.id).unwrap().is_empty());
    assert_eq!(cadence::tick(&s.state, nine + 2 * DAY + 60).unwrap(), 0);
    assert_eq!(s.outcomes(&spot.id), ["cued"]);
}

/// Ann's plan, repeating daily at `time`, with Bo's yes for `bot`.
async fn yes_plan_at(s: &Server, ann: &Who, bo: &Who, bot: &str, time: &str) {
    let plan = s.plan(ann).await;
    let cadence = json!({ "freq": "daily", "time": time });
    let path = format!("/api/events/{plan}/cadence");
    assert_eq!(s.request(&ann.key, Method::PUT, &path, cadence).await.0, StatusCode::OK);
    let path = format!("/api/events/{plan}/cue-bots");
    let yes = json!({ "bots": [bot] });
    assert_eq!(s.request(&bo.key, Method::PUT, &path, yes).await.0, StatusCode::OK);
}

// RP7, L55: another owner's plan doesn't ring a bot at its workplace in a Work hour, at the tick
// or at the check just before the ring; out of Work hours, it rings.
#[tokio::test]
async fn another_owners_cue_doesnt_ring_a_bot_at_work() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let hook = axum::Router::new().fallback(axum::routing::post(move || {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/wake", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, hook).await });
    let settle = || tokio::time::sleep(std::time::Duration::from_millis(400));

    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = member_of(&s, &ann.room, "Bo").await;
    let (spot, _) = support::legacy_bot(&s.state, &bo.id, &ann.room, "Spot");
    // Spot works in Ann's room every hour, and is there now.
    let job = json!({ "room_id": ann.room, "job": "Keeping the books" });
    let (status, reply) =
        s.request(&bo.key, Method::PUT, &format!("/api/agents/{}/workplace", spot.id), job).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let schedule = |block: &str| json!({ "every_day": vec![block; 24] });
    let schedule_path = format!("/api/agents/{}/schedule", spot.id);
    let (status, reply) = s.request(&bo.key, Method::PUT, &schedule_path, schedule("work")).await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    // A plan starting this minute, so the tick and the ring both look at the real time.
    let at = now();
    let time = format!("{:02}:{:02}", at.rem_euclid(DAY) / 3600, at.rem_euclid(3600) / 60);
    let start = at - at.rem_euclid(60);
    yes_plan_at(&s, &ann, &bo, &spot.id, &time).await;
    s.pin(start + 3600);
    settle().await;
    s.state.db.set_notify_url(&spot.id, Some(&url)).unwrap();
    assert_eq!(cadence::tick(&s.state, now()).unwrap(), 1, "cued all the same");
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 0, "not rung at work");

    // Out of Work hours, a second plan at the same time rings it.
    let (status, _) = s.request(&bo.key, Method::PUT, &schedule_path, schedule("social")).await;
    assert_eq!(status, StatusCode::OK);
    s.state.db.set_notify_url(&spot.id, None).unwrap();
    yes_plan_at(&s, &ann, &bo, &spot.id, &time).await;
    s.pin(start + 3600);
    settle().await;
    s.state.db.set_notify_url(&spot.id, Some(&url)).unwrap();
    assert_eq!(cadence::tick(&s.state, now()).unwrap(), 1);
    settle().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "rung out of Work hours");
}

// RP13: a member reads whether the plan repeats and which of their bots they said yes for; a bot, a
// stranger and a plan that doesn't repeat are handled.
#[tokio::test]
async fn a_member_reads_a_plans_repeat_and_their_own_yes() {
    let s = Server::start().await;
    let ann = s.owner("Ann").await;
    let bo = member_of(&s, &ann.room, "Bo").await;
    let cy = s.owner("Cy").await;
    let (_, rover_token) = support::legacy_bot(&s.state, &ann.id, &ann.room, "Rover");
    let (spot, _) = support::legacy_bot(&s.state, &bo.id, &ann.room, "Spot");
    let plan = s.plan(&ann).await;
    let path = format!("/api/events/{plan}/cadence");

    let (status, reply) = s.request(&bo.key, Method::GET, &path, json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(reply["cadence"].is_null());
    assert_eq!(reply["your_bots"], json!([]));

    let daily = json!({ "freq": "daily", "times": ["09:00", "13:00"] });
    assert_eq!(s.request(&ann.key, Method::PUT, &path, daily).await.0, StatusCode::OK);
    let cue = format!("/api/events/{plan}/cue-bots");
    let yes = json!({ "bots": [spot.id] });
    assert_eq!(s.request(&bo.key, Method::PUT, &cue, yes).await.0, StatusCode::OK);

    let (status, reply) = s.request(&bo.key, Method::GET, &path, json!(null)).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["cadence"]["freq"], "daily");
    assert_eq!(reply["cadence"]["times"], json!(["09:00", "13:00"]));
    assert_eq!(reply["your_bots"], json!([spot.id]));
    let (_, mine) = s.request(&ann.key, Method::GET, &path, json!(null)).await;
    assert_eq!(mine["your_bots"], json!([]), "Ann sees only her own yes");

    assert_eq!(
        s.request(&rover_token, Method::GET, &path, json!(null)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_ne!(s.request(&cy.key, Method::GET, &path, json!(null)).await.0, StatusCode::OK);
}
