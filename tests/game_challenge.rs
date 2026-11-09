//! GM8, GM10 to GM13 (#876, #931): a challenge asks another bot for a duel, which is played only
//! when that bot accepts, inside both owners' allows. Written from docs/games.md.

mod support;

use std::cmp::Ordering;

use agenthangout::{
    auth::{hash_secret, Caller},
    db::{Db, NewFloorItem, NewPlay, PlayCaps},
    error::ApiError,
    games, items, now, AppState, Config,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

const CODE: &str = "letmein";

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

/// A request's status and body.
async fn send(
    base: &str,
    key: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request =
        reqwest::Client::new().request(method, format!("{base}{path}")).bearer_auth(key);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn post(base: &str, key: &str, path: &str, body: Value) -> Value {
    let (status, body) = send(base, key, Method::POST, path, Some(body)).await;
    assert_eq!(status, 200, "{path}: {body}");
    body
}

/// A tool call's result, as a bot's client gets it.
async fn tool(base: &str, token: &str, name: &str, arguments: Value) -> Value {
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    post(base, token, "/mcp", call).await["result"].clone()
}

/// Ann's Arcade with an arcade cabinet in it, her Den, and Bo, who has a room of his own.
struct World {
    base: String,
    state: AppState,
    key: String,
    ann: String,
    arcade: String,
    den: String,
    bo_key: String,
    bo: String,
}

async fn world() -> World {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
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
    tokio::spawn(async move { axum::serve(listener, agenthangout::app(served)).await });
    let person = async |name: &str| {
        let body =
            json!({ "name": format!("{name}'s room"), "your_name": name, "create_code": CODE });
        let made = post(&base, "", "/api/hangouts", body).await;
        (text(&made["owner_key"]), text(&made["person"]["id"]))
    };
    let (key, ann) = person("Ann").await;
    let (bo_key, bo) = person("Bo").await;
    let room = async |name: &str| {
        let body = json!({ "name": name, "create_code": CODE });
        text(&post(&base, &key, "/api/hangouts", body).await["hangout"]["id"])
    };
    let (arcade, den) = (room("Arcade").await, room("Den").await);
    state
        .db
        .place_item::<()>(
            &NewFloorItem {
                id: "cabinet-1",
                hangout_id: &arcade,
                kind: "arcade_cabinet",
                x: 10,
                y: 6,
                facing: 0,
                placed_by: &ann,
                now: now(),
            },
            |_, _, _| Ok(()),
        )
        .unwrap()
        .unwrap();
    World { base, state, key, ann, arcade, den, bo_key, bo }
}

impl World {
    /// A bot of `owner`'s in `room`: its id, its token, and it as a caller.
    fn bot(&self, owner: &str, room: &str, name: &str) -> (String, String, Caller) {
        let (bot, token) = support::legacy_bot(&self.state, owner, room, name);
        let caller = self.state.authenticate(&hash_secret(&token)).unwrap().unwrap();
        (bot.id, token, caller)
    }

    async fn allow(&self, key: &str, bot: &str, body: Value) -> (u16, Value) {
        send(&self.base, key, Method::PUT, &format!("/api/agents/{bot}/duels"), Some(body)).await
    }

    fn challenge(&self, caller: &Caller, name: &str) -> Result<Value, ApiError> {
        items::use_item_against(&self.state, caller, "cabinet-1", Some(name))
    }

    async fn answer(&self, token: &str, duel: &str, answer: &str) -> (u16, Value) {
        let path = format!("/api/duels/{duel}/answer");
        send(&self.base, token, Method::POST, &path, Some(json!({ "answer": answer }))).await
    }

    async fn inbox(&self, token: &str) -> Value {
        send(&self.base, token, Method::GET, "/api/inbox", None).await.1
    }

    fn lines(&self) -> Vec<String> {
        let said = self.state.db.messages(&self.arcade, 0, 50).unwrap();
        said.into_iter().map(|message| message.body).collect()
    }

    /// How many plays one of Ann's bots has had at the cabinet this hour.
    fn plays(&self, agent: &str) -> usize {
        let at_least = |n: i64| {
            let caps = PlayCaps { per_hour: n, cooldown_secs: 0 };
            let who = (self.ann.as_str(), Some(agent));
            let next = self.state.db.next_play_at(who, &self.arcade, "arcade_cabinet", now(), caps);
            next.unwrap().is_some()
        };
        (1..=games::CAPS.per_hour).filter(|n| at_least(*n)).count()
    }
}

/// A refusal's status, text and code.
fn code(error: ApiError) -> (u16, String, Value) {
    let code = error.details.as_ref().map_or(Value::Null, |details| details["code"].clone());
    (error.status.as_u16(), error.message, code)
}

// GM8, GM10 to GM13: off until both owners allow it, a challenge only asks, and the duel is the
// two bots' business until the bot asked accepts it. Then it plays as GM8 always has.
#[tokio::test]
async fn a_challenge_only_asks_and_an_accept_plays_it() {
    let w = world().await;
    let (scout, scout_token, scout_caller) = w.bot(&w.ann, &w.arcade, "Scout");
    let (rex, rex_token, _) = w.bot(&w.ann, &w.arcade, "Rex");
    let (_, pip_token, _) = w.bot(&w.ann, &w.arcade, "Pip");
    w.bot(&w.ann, &w.den, "Faraway");

    // GM10: off with a cap of 5 until the owner says, and only the owner reads or sets it.
    let path = format!("/api/agents/{scout}/duels");
    let off = json!({ "allow": "off", "daily_cap": 5 });
    assert_eq!(send(&w.base, &w.key, Method::GET, &path, None).await, (200, off));
    assert_eq!(send(&w.base, &scout_token, Method::GET, &path, None).await.0, 403);
    assert_eq!(send(&w.base, &w.bo_key, Method::GET, &path, None).await.0, 404);
    assert_eq!(w.allow(&w.bo_key, &scout, json!({ "allow": "room" })).await.0, 404);
    for bad in [
        json!({ "allow": "friends" }),
        json!({ "allow": "room", "daily_cap": 0 }),
        json!({ "allow": "room", "daily_cap": 21 }),
    ] {
        assert_eq!(w.allow(&w.key, &scout, bad.clone()).await.0, 400, "{bad}");
    }

    // GM8: refused before anything is written: a bot that isn't here, and oneself.
    for name in ["Faraway", "Nobody", "Scout"] {
        assert!(w.challenge(&scout_caller, name).is_err(), "{name}");
    }
    // GM11: the caller's own allow, then the other bot's. The code is where the text starts.
    let args = json!({ "item_id": "cabinet-1", "challenge": "Rex" });
    let refused = tool(&w.base, &scout_token, "use_item", args).await;
    assert_eq!(refused["structuredContent"]["code"], "duels_off", "{refused}");
    assert!(text(&refused["content"][0]["text"]).starts_with("duels_off: "), "{refused}");
    assert_eq!(w.allow(&w.key, &scout, json!({ "allow": "room" })).await.0, 200);
    let refused = code(w.challenge(&scout_caller, "Rex").unwrap_err());
    assert_eq!((refused.0, &refused.2), (403, &json!("opponent_duels_off")), "{}", refused.1);
    assert!(refused.1.starts_with("opponent_duels_off: "));
    let set = w.allow(&w.key, &rex, json!({ "allow": "room", "daily_cap": 2 })).await;
    assert_eq!(set, (200, json!({ "allow": "room", "daily_cap": 2 })));
    // A cap left out stays as it was.
    assert_eq!(w.allow(&w.key, &rex, json!({ "allow": "room" })).await.1["daily_cap"], 2);

    // GM11: asking writes a waiting duel and nothing else, and asking again is the same duel.
    let asked = w.challenge(&scout_caller, "rex").unwrap();
    assert_eq!((&asked["status"], &asked["opponent"]), (&json!("pending"), &json!("Rex")));
    let duel = text(&asked["duel_id"]);
    assert_eq!(w.challenge(&scout_caller, "Rex").unwrap()["duel_id"], duel.as_str());
    assert!(w.lines().is_empty());
    assert_eq!((w.plays(&scout), w.plays(&rex)), (0, 0));

    // GM12: only the bot it's for has it waiting, and only while its owner allows duels.
    let inbox = tool(&w.base, &rex_token, "inbox", json!({})).await;
    let duels = &inbox["structuredContent"]["duels"];
    assert_eq!(duels["waiting"][0]["duel_id"], duel.as_str(), "{duels}");
    assert_eq!(duels["waiting"][0]["from"], "Scout");
    assert!(text(&duels["how"]).contains("answer_duel"));
    assert_eq!(w.inbox(&scout_token).await["duels"], json!({ "allow": "room", "daily_cap": 5 }));
    assert!(w.inbox(&pip_token).await.get("duels").is_none());

    // GM13: nobody else answers it, the bot that asked included.
    for token in [&pip_token, &scout_token] {
        assert_eq!(w.answer(token, &duel, "accept").await.0, 404);
    }

    // GM8: the accept plays it: a play each and one line, with Scout, who challenged, as A.
    let args = json!({ "duel_id": duel, "answer": "accept" });
    let won = tool(&w.base, &rex_token, "answer_duel", args).await["structuredContent"].clone();
    let (b, a) =
        (won["score"].as_i64().unwrap(), won["opponent"]["result"]["score"].as_i64().unwrap());
    assert_eq!(won["opponent"]["bot"], "Scout", "{won}");
    assert_eq!(
        (&won["remaining"], &won["opponent"]["result"]["remaining"]),
        (&json!(5), &json!(5))
    );
    let expected = match a.cmp(&b) {
        Ordering::Greater => format!("Scout beat Rex at the arcade cabinet, {a} to {b}."),
        Ordering::Less => format!("Rex beat Scout at the arcade cabinet, {b} to {a}."),
        Ordering::Equal => format!("Scout and Rex tied at the arcade cabinet, {a} each."),
    };
    assert_eq!(w.lines(), [expected]);
    assert_eq!((w.plays(&scout), w.plays(&rex)), (1, 1));
    assert_eq!(w.state.db.duel_rolls(&duel).unwrap(), Some((a, b)), "kept for the week's board");
    let again = w.answer(&rex_token, &duel, "accept").await;
    let closed = (again.0, &again.1["code"], &again.1["status"]);
    assert_eq!(closed, (409, &json!("duel_closed"), &json!("accepted")));

    // GM13: within 10 minutes the pair waits. A refused accept leaves the duel waiting, and a
    // decline closes it with nothing played or posted.
    let next = text(&w.challenge(&scout_caller, "Rex").unwrap()["duel_id"]);
    assert_ne!(next, duel);
    let cooling = w.answer(&rex_token, &next, "accept").await;
    assert_eq!((cooling.0, &cooling.1["code"]), (429, &json!("duel_cooldown")), "{}", cooling.1);
    assert_eq!(w.inbox(&rex_token).await["duels"]["waiting"][0]["duel_id"], next.as_str());
    let declined = w.answer(&rex_token, &next, "decline").await;
    assert_eq!(declined, (200, json!({ "ok": true, "duel_id": next, "status": "declined" })));
    assert_eq!(w.lines().len(), 1);
    assert_eq!((w.plays(&scout), w.plays(&rex)), (1, 1));
    assert!(w.inbox(&rex_token).await["duels"].get("waiting").is_none());

    // GM11: at the smaller of the two caps for today, asking is refused.
    w.allow(&w.key, &rex, json!({ "allow": "room", "daily_cap": 1 })).await;
    let capped = code(w.challenge(&scout_caller, "Rex").unwrap_err());
    assert_eq!((capped.0, &capped.2), (429, &json!("duel_cap")), "{}", capped.1);
    assert!(capped.1.starts_with("duel_cap: "));
}

// GM11, GM13, A103: a duel whose time ran out is a no; GM3 still holds at the accept, which then
// writes nothing; and across a block a duel looks asked but never reaches the other bot.
#[tokio::test]
async fn an_expired_unplayable_or_blocked_duel_writes_nothing() {
    let w = world().await;
    let (pip, pip_token, pip_caller) = w.bot(&w.ann, &w.arcade, "Pip");
    let (quo, quo_token, quo_caller) = w.bot(&w.ann, &w.arcade, "Quo");
    for bot in [&pip, &quo] {
        assert_eq!(w.allow(&w.key, bot, json!({ "allow": "room" })).await.0, 200);
    }

    // Expired: gone from the inbox, and too late to accept.
    let late = text(&w.challenge(&pip_caller, "Quo").unwrap()["duel_id"]);
    w.state.db.set_duel_expiry(&late, now() - 1).unwrap();
    assert!(w.inbox(&quo_token).await["duels"].get("waiting").is_none());
    let (status, body) = w.answer(&quo_token, &late, "accept").await;
    let closed = (status, &body["code"], &body["status"]);
    assert_eq!(closed, (409, &json!("duel_closed"), &json!("expired")));

    // GM3: Quo has had its six plays this hour. Its own accept is 429; Pip's accept of Quo's duel
    // is 409 and isn't told why. Neither writes a play, and both duels keep waiting.
    for n in 0..6 {
        let play = NewPlay {
            hangout_id: &w.arcade,
            item_id: "cabinet-1",
            kind: "arcade_cabinet",
            agent_id: Some(&quo),
            person_id: &w.ann,
            score: n,
            week: games::week_of(now()),
            now: now() - 3000 + n * 100,
        };
        w.state.db.record_play(&play, games::CAPS).unwrap();
    }
    let to_quo = text(&w.challenge(&pip_caller, "Quo").unwrap()["duel_id"]);
    let to_pip = text(&w.challenge(&quo_caller, "Pip").unwrap()["duel_id"]);
    assert_eq!(w.answer(&quo_token, &to_quo, "accept").await.0, 429);
    let (status, body) = w.answer(&pip_token, &to_pip, "accept").await;
    assert_eq!((status, text(&body["error"])), (409, "Quo can't play right now".to_string()));
    assert_eq!(w.plays(&pip), 0);
    assert_eq!(w.inbox(&quo_token).await["duels"]["waiting"][0]["duel_id"], to_quo.as_str());
    assert_eq!(w.inbox(&pip_token).await["duels"]["waiting"][0]["duel_id"], to_pip.as_str());
    assert!(w.lines().is_empty());

    // A103: across a block the caller is answered the same, but Tad never sees the duel and
    // can't answer it. Asked again with the block gone, it's real.
    let (tad, tad_token, _) = w.bot(&w.bo, &w.arcade, "Tad");
    assert_eq!(w.allow(&w.bo_key, &tad, json!({ "allow": "room" })).await.0, 200);
    post(&w.base, &w.key, "/api/blocks", json!({ "person_id": w.bo })).await;
    let asked = w.challenge(&pip_caller, "Tad").unwrap();
    assert_eq!(asked["status"], "pending", "{asked}");
    let quiet = text(&asked["duel_id"]);
    assert!(w.inbox(&tad_token).await["duels"].get("waiting").is_none());
    assert_eq!(w.answer(&tad_token, &quiet, "accept").await.0, 404);
    send(&w.base, &w.key, Method::DELETE, &format!("/api/blocks/{}", w.bo), None).await;
    assert_eq!(w.challenge(&pip_caller, "Tad").unwrap()["duel_id"], quiet.as_str());
    assert_eq!(w.inbox(&tad_token).await["duels"]["waiting"][0]["duel_id"], quiet.as_str());
}

// GM13: the pair's cap is checked in the same write as the accept. Two duels wait, one each way,
// with one left under the cap, and both bots accept at once: exactly one is played.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_accepts_at_once_at_the_cap_play_only_one() {
    let w = world().await;
    for round in 0..6 {
        let (ace, ace_token, ace_caller) = w.bot(&w.ann, &w.arcade, &format!("Ace {round}"));
        let (bee, bee_token, bee_caller) = w.bot(&w.ann, &w.arcade, &format!("Bee {round}"));
        for bot in [&ace, &bee] {
            let set = w.allow(&w.key, bot, json!({ "allow": "room", "daily_cap": 1 })).await;
            assert_eq!(set.0, 200);
        }
        let to_bee = text(&w.challenge(&ace_caller, &format!("Bee {round}")).unwrap()["duel_id"]);
        let to_ace = text(&w.challenge(&bee_caller, &format!("Ace {round}")).unwrap()["duel_id"]);
        let answers = join_all(
            [(&bee_token, &to_bee), (&ace_token, &to_ace)]
                .map(|(token, duel)| w.answer(token, duel, "accept")),
        )
        .await;
        let played = answers.iter().filter(|(status, _)| *status == 200).count();
        assert_eq!(played, 1, "round {round}: {answers:?}");
        let refused = answers.iter().find(|(status, _)| *status != 200).unwrap();
        assert_eq!((refused.0, &refused.1["code"]), (429, &json!("duel_cap")), "round {round}");
        assert_eq!((w.plays(&ace), w.plays(&bee)), (1, 1), "round {round}");
    }
    assert_eq!(w.lines().len(), 6);
}
