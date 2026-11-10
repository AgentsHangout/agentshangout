//! Tests for bots' memories: notes and moments, recall, and going to bed. They drive a real server
//! over HTTP and are written from the rules in docs/memories.md. A comment such as `// MM6` above a
//! test names the rule it covers, and so does the test's name.

mod support;

use agenthangout::{app, db::Db, now, AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";
const DAY: i64 = 86_400;

struct Server {
    base: String,
    client: Client,
    state: AppState,
    path: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path));
        }
    }
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

#[derive(Clone)]
struct Who {
    key: String,
    id: String,
}

#[derive(Clone)]
struct Bot {
    id: String,
    token: String,
}

/// Ann's Cafe, Ann's room, with Bo in it. Fern is Ann's bot and Rover is Bo's, both in the Cafe.
/// Cy has a room of his own, Cy's Den, and has never met any of them.
struct Town {
    ann: Who,
    bo: Who,
    cy: Who,
    cafe: String,
    code: String,
    den: String,
    fern: Bot,
    rover: Bot,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

fn texts(list: &Value, field: &str) -> Vec<String> {
    list.as_array().unwrap().iter().map(|item| text(&item[field])).collect()
}

impl Server {
    async fn start() -> Self {
        let path = support::db_path("memories").to_str().unwrap().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let state = AppState::new(
            Db::open(&path).unwrap(),
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
        Self { base, client: Client::new(), state, path }
    }

    /// A second connection to the same file, for setting up what the API can't in a test.
    fn sql(&self) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn
    }

    fn count(&self, sql: &str, value: &str) -> i64 {
        self.sql().query_row(sql, [value], |row| row.get(0)).unwrap()
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

    /// Someone new, by way of a room of their own: the room's id, its invite code, and who.
    async fn someone(&self, name: &str, room: &str) -> (String, String, Who) {
        let made = self
            .post(
                "",
                "/api/hangouts",
                json!({ "name": room, "your_name": name, "create_code": CODE }),
            )
            .await
            .ok();
        let code = text(&made["invite_url"]).rsplit('/').next().unwrap().to_string();
        let who = Who { key: text(&made["owner_key"]), id: text(&made["person"]["id"]) };
        (text(&made["hangout"]["id"]), code, who)
    }

    async fn join(&self, code: &str, name: &str) -> Who {
        let body = self
            .post("", &format!("/api/invites/{code}/join"), json!({ "your_name": name }))
            .await
            .ok();
        Who { key: text(&body["owner_key"]), id: text(&body["person"]["id"]) }
    }

    fn bot(&self, owner: &Who, room: &str, name: &str) -> Bot {
        let (agent, token) = support::legacy_bot(&self.state, &owner.id, room, name);
        Bot { id: agent.id, token }
    }

    async fn town(&self) -> Town {
        let (cafe, code, ann) = self.someone("Ann", "Ann's Cafe").await;
        let bo = self.join(&code, "Bo").await;
        let (den, _, cy) = self.someone("Cy", "Cy's Den").await;
        let fern = self.bot(&ann, &cafe, "Fern");
        let rover = self.bot(&bo, &cafe, "Rover");
        Town { ann, bo, cy, cafe, code, den, fern, rover }
    }

    async fn call(&self, bot: &Bot, name: &str, arguments: Value) -> Value {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        support::structured(self.post(&bot.token, "/mcp", call).await.ok()["result"].clone())
    }

    async fn tool(&self, bot: &Bot, name: &str, arguments: Value) -> Value {
        let result = self.call(bot, name, arguments).await;
        assert!(result["isError"].is_null(), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    async fn refused(&self, bot: &Bot, name: &str, arguments: Value) -> String {
        let result = self.call(bot, name, arguments).await;
        assert_eq!(result["isError"], true, "{name} should have been refused: {result}");
        text(&result["content"][0]["text"])
    }

    async fn remember(&self, bot: &Bot, arguments: Value) -> Value {
        self.tool(bot, "remember", arguments).await
    }

    async fn recall(&self, bot: &Bot, query: &str) -> Vec<Value> {
        let found = self.tool(bot, "recall", json!({ "query": query })).await;
        found["memories"].as_array().unwrap().clone()
    }

    async fn view(&self, owner: &Who, bot: &Bot) -> Value {
        self.get(&owner.key, &format!("/api/agents/{}/memories", bot.id)).await.ok()
    }

    /// Puts a moment straight into the table, as if saved `ago` seconds back.
    fn moment(&self, bot: &Bot, subject: &str, words: &str, importance: i64, ago: i64) -> String {
        let id = agenthangout::auth::new_id();
        let at = now() - ago;
        self.sql()
            .execute(
                "INSERT INTO bot_memories (id, bot_id, subject, kind, text, importance, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'moment', ?4, ?5, ?6, ?6)",
                rusqlite::params![id, bot.id, subject, words, importance, at],
            )
            .unwrap();
        id
    }

    /// Puts a chat line straight into a room, `ago` seconds back.
    fn line(
        &self,
        room: &str,
        bot: &Bot,
        owner: &Who,
        speaker: &str,
        words: &str,
        ago: i64,
    ) -> i64 {
        let conn = self.sql();
        conn.execute(
            "INSERT INTO messages (hangout_id, person_id, agent_id, speaker, kind, body, created_at)
             VALUES (?1, ?2, ?3, ?4, 'chat', ?5, ?6)",
            rusqlite::params![room, owner.id, bot.id, speaker, words, now() - ago],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    async fn whys(&self, bot: &Bot, query: &str) -> Vec<(String, Vec<String>)> {
        self.recall(bot, query)
            .await
            .iter()
            .map(|found| (text(&found["text"]), strings(&found["why"])))
            .collect()
    }
}

/// A list of strings.
fn strings(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(text).collect()
}

// Storage
#[tokio::test]
async fn storage_keeps_no_index_of_memories_and_an_old_database_gets_the_tables() {
    let server = Server::start().await;
    let town = server.town().await;
    server.remember(&town.fern, json!({ "text": "Ann plays the piano" })).await;
    // MM9: recall indexes a bot's own memories for one search and keeps nothing, so the table is
    // the only place a memory's words are, and no full-text index is in the file.
    assert_eq!(server.recall(&town.fern, "playing").await.len(), 1);
    let conn = server.sql();
    let indexes: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE sql LIKE '%USING fts5%'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(indexes, 0);
    assert_eq!(tables_holding(&conn, "piano"), ["bot_memories"]);

    // A database from before memories gets every table when the server opens it.
    let path = support::db_path("memories-old");
    let path = path.to_str().unwrap().to_string();
    drop(Db::open(&path).unwrap());
    let old = rusqlite::Connection::open(&path).unwrap();
    for table in
        ["bot_note_checks", "bot_memories", "bot_sleep", "bot_memory_uses", "bot_rooms_seen"]
    {
        old.execute(&format!("DROP TABLE {table}"), []).unwrap();
    }
    drop(old);
    drop(Db::open(&path).unwrap());
    let reopened = rusqlite::Connection::open(&path).unwrap();
    for table in
        ["bot_note_checks", "bot_memories", "bot_sleep", "bot_memory_uses", "bot_rooms_seen"]
    {
        let there: i64 = reopened
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = ?", [table], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(there, 1, "{table}");
    }
    drop(reopened);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{path}{suffix}"));
    }
}

// MM1
#[tokio::test]
async fn mm1_a_memory_is_one_bots_about_one_subject_and_1_to_150_characters() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;

    // Counted as characters: 150 two-byte letters fit, 151 don't, and nothing is cut.
    let fits = "é".repeat(150);
    server.remember(fern, json!({ "text": fits })).await;
    let refused = server.refused(fern, "remember", json!({ "text": "é".repeat(151) })).await;
    assert!(refused.contains("151 characters") && refused.contains("150"), "{refused}");
    assert!(server.refused(fern, "remember", json!({ "text": "   " })).await.contains("empty"));

    // Trimmed, and each kind of subject.
    let saved = server.remember(fern, json!({ "text": "  quiet morning  " })).await;
    assert_eq!(saved["subject"], "self");
    for subject in [
        format!("room:{}", town.cafe),
        format!("person:{}", town.bo.id),
        format!("agent:{}", town.rover.id),
        "self".to_string(),
    ] {
        let saved = server.remember(fern, json!({ "text": "seen", "subject": subject })).await;
        assert_eq!(saved["subject"], json!(subject));
    }
    // A person or a bot can be named, and the id is what's kept.
    let named = server.remember(fern, json!({ "text": "named", "subject": "person:bo" })).await;
    assert_eq!(named["subject"], json!(format!("person:{}", town.bo.id)));
    let named = server.remember(fern, json!({ "text": "named", "subject": "agent:Rover" })).await;
    assert_eq!(named["subject"], json!(format!("agent:{}", town.rover.id)));
    let refused =
        server.refused(fern, "remember", json!({ "text": "x", "subject": "house:1" })).await;
    assert!(refused.contains("subject must be"), "{refused}");

    let view = server.view(&town.ann, fern).await;
    let moments = texts(&view["moments"], "text");
    assert!(moments.contains(&"quiet morning".to_string()), "{moments:?}");
    assert!(moments.contains(&fits));
}

// MM2
#[tokio::test]
async fn mm2_a_note_is_one_per_subject_and_past_500_moments_the_lowest_standing_goes() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let first = server
        .remember(fern, json!({ "text": "Busy at noon", "subject": room, "note": true }))
        .await;
    let second = server
        .remember(fern, json!({ "text": "Quiet after six", "subject": room, "note": true }))
        .await;
    assert_eq!(first["id"], second["id"], "writing it again replaces it");
    let view = server.view(&town.ann, fern).await;
    assert_eq!(texts(&view["notes"], "text"), ["Quiet after six"]);

    // A moment's importance is 1 to 5, 3 if left out.
    let moment = server.remember(fern, json!({ "text": "a moment" })).await;
    assert_eq!(moment["importance"], 3);
    let refused = server.refused(fern, "remember", json!({ "text": "x", "importance": 6 })).await;
    assert!(refused.contains("1 to 5"), "{refused}");

    // 498 more moments, so there are 499, one of them old and unimportant.
    let lowest = server.moment(fern, "self", "an old small thing", 1, 60 * DAY);
    for i in 0..497 {
        server.moment(fern, "self", &format!("moment {i}"), 3, 60);
    }
    let kept = server.remember(fern, json!({ "text": "number 500" })).await;
    assert!(kept.get("dropped").is_none(), "500 is allowed");
    let past = server.remember(fern, json!({ "text": "number 501", "importance": 1 })).await;
    assert_eq!(past["dropped"], json!(lowest), "the lowest standing went, not the one just saved");
    let left = server
        .count("SELECT COUNT(*) FROM bot_memories WHERE bot_id = ? AND kind = 'moment'", &fern.id);
    assert_eq!(left, 500);
    // Its words went with it: nothing in the file holds them, and recall can't find it.
    assert!(tables_holding(&server.sql(), "an old small thing").is_empty());
    assert!(server.recall(fern, "small").await.is_empty());
}

// MM3
#[tokio::test]
async fn mm3_a_note_on_someone_carries_a_feeling_from_the_list_and_a_closeness() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let bo = format!("person:{}", town.bo.id);
    let saved = server
        .remember(fern, json!({ "text": "Kind to me", "subject": bo, "note": true, "feeling": "fond", "closeness": 2 }))
        .await;
    assert_eq!(saved["kind"], "note");
    let refused = server
        .refused(
            fern,
            "remember",
            json!({ "text": "x", "subject": bo, "note": true, "feeling": "smitten" }),
        )
        .await;
    for feeling in
        ["fond", "warm", "trusting", "grateful", "curious", "neutral", "wary", "annoyed", "hurt"]
    {
        assert!(refused.contains(feeling), "{refused}");
    }
    let refused = server
        .refused(
            fern,
            "remember",
            json!({ "text": "x", "subject": bo, "note": true, "closeness": 3 }),
        )
        .await;
    assert!(refused.contains("-2 to 2"), "{refused}");
    // Not on a moment, and not on a room.
    let refused = server
        .refused(fern, "remember", json!({ "text": "x", "subject": bo, "feeling": "warm" }))
        .await;
    assert!(refused.contains("feeling and closeness"), "{refused}");
    let room = format!("room:{}", town.cafe);
    let refused = server
        .refused(
            fern,
            "remember",
            json!({ "text": "x", "subject": room, "note": true, "feeling": "warm" }),
        )
        .await;
    assert!(refused.contains("feeling and closeness"), "{refused}");
    let view = server.view(&town.ann, fern).await;
    assert_eq!(view["notes"][0]["feeling"], "fond");
    assert_eq!(view["notes"][0]["closeness"], 2);
}

// MM4
#[tokio::test]
async fn mm4_memories_are_private_and_only_about_rooms_and_people_the_bot_has_been_with() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let bo = format!("person:{}", town.bo.id);
    server.remember(fern, json!({ "text": "Bo loves jazz", "subject": bo, "note": true })).await;
    let room = format!("room:{}", town.cafe);
    server
        .remember(
            fern,
            json!({ "text": "The cafe smells of jazz records", "subject": room, "note": true }),
        )
        .await;

    // A stranger, a room it was never in and a subject that doesn't exist all read the same.
    let stranger = server
        .refused(
            fern,
            "remember",
            json!({ "text": "x", "subject": format!("person:{}", town.cy.id) }),
        )
        .await;
    let by_name =
        server.refused(fern, "remember", json!({ "text": "x", "subject": "person:Cy" })).await;
    let nobody = server
        .refused(fern, "remember", json!({ "text": "x", "subject": "person:nobody-at-all" }))
        .await;
    let den = server
        .refused(fern, "remember", json!({ "text": "x", "subject": format!("room:{}", town.den) }))
        .await;
    let no_room = server
        .refused(fern, "remember", json!({ "text": "x", "subject": "room:no-such-room" }))
        .await;
    assert_eq!([&stranger, &by_name, &den, &no_room], [&nobody; 4]);
    assert_eq!(nobody, "not found");

    // Another bot sees none of it: not in recall, not in a room read, not by its owner.
    let rover = &town.rover;
    assert!(server.recall(rover, "jazz").await.is_empty());
    let read = server.tool(rover, "read_room", json!({ "hangout_id": town.cafe })).await;
    assert!(read.get("memory").is_none(), "{read}");
    assert!(!read.to_string().contains("jazz"));
    server
        .get(&town.bo.key, &format!("/api/agents/{}/memories", fern.id))
        .await
        .is(StatusCode::NOT_FOUND);
    // Nor does the room's own snapshot.
    let snapshot = server.get(&town.ann.key, &format!("/api/hangouts/{}", town.cafe)).await.ok();
    assert!(!snapshot.to_string().contains("jazz"));
    // Fern and Ann do.
    assert_eq!(server.recall(fern, "jazz").await.len(), 2);
    assert_eq!(server.view(&town.ann, fern).await["notes"].as_array().unwrap().len(), 2);
}

// MM5
#[tokio::test]
async fn mm5_memories_go_with_their_bot_and_with_whoever_they_are_about_but_not_on_leaving() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;

    // Leaving a room keeps the bot's memories of it, and it can still write about it.
    let (dee_room, _, _) = server.someone("Dee", "Dee's Deck").await;
    server.state.db.add_agent_to_room(&dee_room, &fern.id).unwrap();
    let deck = format!("room:{dee_room}");
    server.remember(fern, json!({ "text": "Windy deck", "subject": deck, "note": true })).await;
    server.state.db.remove_agent_from_room(&dee_room, &fern.id).unwrap();
    assert_eq!(texts(&json!(server.recall(fern, "windy").await), "text"), ["Windy deck"]);
    server.remember(fern, json!({ "text": "Still windy", "subject": deck, "note": true })).await;

    // About a bot and about a person: they go when that bot or account goes.
    let rover = format!("agent:{}", town.rover.id);
    let bo = format!("person:{}", town.bo.id);
    server
        .remember(
            fern,
            json!({ "text": "Rover hums like a xylophonist", "subject": rover, "note": true }),
        )
        .await;
    server
        .remember(
            fern,
            json!({ "text": "Bo bakes quinceberry tarts", "subject": bo, "note": true }),
        )
        .await;
    // The memory table is the only place their words are, so a deleted row takes them with it.
    let conn = server.sql();
    assert_eq!(tables_holding(&conn, "xylophonist"), ["bot_memories"]);
    assert_eq!(tables_holding(&conn, "quinceberry"), ["bot_memories"]);
    server.state.db.delete_agent(&town.rover.id).unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE subject = ?", &rover), 0);
    assert!(tables_holding(&conn, "xylophonist").is_empty());
    server.state.db.delete_person(&town.bo.id, "Bo").unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE subject = ?", &bo), 0);
    assert!(tables_holding(&conn, "quinceberry").is_empty());
    assert!(server.recall(fern, "hums bakes").await.is_empty());

    // forget takes the words out the same way.
    let heron = server.remember(fern, json!({ "text": "A heron called Zebulon" })).await;
    assert_eq!(tables_holding(&conn, "Zebulon"), ["bot_memories"]);
    server.tool(fern, "forget", json!({ "id": heron["id"] })).await;
    assert!(tables_holding(&conn, "Zebulon").is_empty());

    // The bot's own go with it.
    server.state.db.delete_agent(&fern.id).unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE bot_id = ?", &fern.id), 0);
    assert!(tables_holding(&conn, "Windy deck").is_empty());
    assert!(tables_holding(&conn, "Still windy").is_empty());
}

// MM6
#[tokio::test]
async fn mm6_remember_saves_forget_deletes_and_a_bot_saves_30_an_hour() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let saved = server.remember(fern, json!({ "text": "Saw a heron" })).await;
    let id = text(&saved["id"]);
    assert_eq!(saved["kind"], "moment");
    // Another bot can't forget it, and it's the same answer as an id that was never there.
    assert_eq!(server.refused(&town.rover, "forget", json!({ "id": id })).await, "not found");
    let forgot = server.tool(fern, "forget", json!({ "id": id })).await;
    assert_eq!(forgot["forgot"], json!(id));
    assert_eq!(server.refused(fern, "forget", json!({ "id": id })).await, "not found");
    assert!(server.recall(fern, "heron").await.is_empty());

    // 29 more makes 30 in the hour, forgetting doesn't give one back, and the 31st says when.
    for i in 0..29 {
        server.remember(fern, json!({ "text": format!("moment {i}") })).await;
    }
    let refused = server.refused(fern, "remember", json!({ "text": "one too many" })).await;
    assert!(
        refused.contains("30 memories an hour") && refused.contains("save again at"),
        "{refused}"
    );
    // Another bot has its own 30.
    server.remember(&town.rover, json!({ "text": "fine" })).await;
}

// MM7
#[tokio::test]
async fn mm7_reading_a_room_brings_the_bots_notes_on_it_and_who_is_in_it() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let read = server.tool(fern, "read_room", json!({ "hangout_id": town.cafe })).await;
    assert!(read.get("memory").is_none(), "no memories, no field: {read}");
    // A moment isn't a note, so it doesn't come either.
    server
        .remember(fern, json!({ "text": "a moment", "subject": format!("room:{}", town.cafe) }))
        .await;
    let read =
        server.tool(fern, "read_room", json!({ "hangout_id": town.cafe, "brief": true })).await;
    assert!(read.get("memory").is_none(), "{read}");

    let room = format!("room:{}", town.cafe);
    let bo = format!("person:{}", town.bo.id);
    let rover = format!("agent:{}", town.rover.id);
    server.remember(fern, json!({ "text": "Cozy", "subject": room, "note": true })).await;
    server
        .remember(
            fern,
            json!({ "text": "Bakes bread", "subject": bo, "note": true, "feeling": "warm" }),
        )
        .await;
    server.remember(fern, json!({ "text": "Hums", "subject": rover, "note": true })).await;
    let read =
        server.tool(fern, "read_room", json!({ "hangout_id": town.cafe, "brief": true })).await;
    let memory = read["memory"].as_array().unwrap();
    assert_eq!(memory[0], json!({ "subject": room, "name": "Ann's Cafe", "text": "Cozy" }));
    assert!(memory.contains(
        &json!({ "subject": bo, "name": "Bo", "text": "Bakes bread", "feeling": "warm" })
    ));
    assert!(memory.contains(&json!({ "subject": rover, "name": "Rover", "text": "Hums" })));
    assert_eq!(memory.len(), 3);

    // Finding one bot brings the note on it the same way; Rover finding Fern brings nothing.
    let found = server.tool(fern, "where_is", json!({ "name": "Rover" })).await;
    assert_eq!(found["memory"], json!({ "subject": rover, "name": "Rover", "text": "Hums" }));
    let found = server.tool(&town.rover, "where_is", json!({ "name": "Fern" })).await;
    assert!(found.get("memory").is_none(), "{found}");
}

// MM7
#[tokio::test]
async fn mm7_a_room_read_carries_at_most_five_notes_on_people_and_bots() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    // Six more people in the Cafe, each with a note.
    for name in ["Gus", "Hal", "Ida", "Jo", "Kit", "Lu"] {
        server.join(&town.code, name).await;
        server.remember(fern, json!({ "text": format!("{name} is nice"), "subject": format!("person:{name}"), "note": true })).await;
    }
    let read =
        server.tool(fern, "read_room", json!({ "hangout_id": town.cafe, "brief": true })).await;
    assert_eq!(read["memory"].as_array().unwrap().len(), 5);
}

// MM8
#[tokio::test]
async fn mm8_recall_returns_notes_and_moments_with_why_and_an_empty_list_for_no_match() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    for i in 0..7 {
        server.remember(fern, json!({ "text": format!("Tea number {i}") })).await;
    }
    let room = format!("room:{}", town.cafe);
    server
        .remember(fern, json!({ "text": "Tea is good here", "subject": room, "note": true }))
        .await;
    let found = server.recall(fern, "tea").await;
    assert_eq!(found.len(), 5, "five by default");
    for item in &found {
        for field in ["id", "kind", "subject", "text", "when", "why"] {
            assert!(item.get(field).is_some(), "{field} in {item}");
        }
    }
    let ten = server.tool(fern, "recall", json!({ "query": "tea", "limit": 10 })).await;
    assert_eq!(ten["memories"].as_array().unwrap().len(), 8);
    let kinds: Vec<String> = texts(&ten["memories"], "kind");
    assert!(kinds.contains(&"note".to_string()) && kinds.contains(&"moment".to_string()));
    let refused = server.refused(fern, "recall", json!({ "query": "tea", "limit": 11 })).await;
    assert!(refused.contains("1 to 10"), "{refused}");
    // Only one subject.
    let only = server.tool(fern, "recall", json!({ "query": "tea", "subject": room })).await;
    assert_eq!(texts(&only["memories"], "text"), ["Tea is good here"]);
    // No match is an empty list, and punctuation is only punctuation.
    assert!(server.recall(fern, "zebra quokka").await.is_empty());
    let odd = server.tool(fern, "recall", json!({ "query": "\"tea\" NEAR( -- * OR) ^" })).await;
    assert!(!odd["memories"].as_array().unwrap().is_empty());
    assert!(server.recall(fern, "!!! ???").await.is_empty());
}

// MM9: the words list is stemmed.
#[tokio::test]
async fn mm9_words_are_stemmed_so_painting_finds_paints() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    server.remember(fern, json!({ "text": "Gus paints murals on Sundays" })).await;
    server.remember(fern, json!({ "text": "Something else entirely" })).await;
    let found = server.whys(fern, "painting").await;
    assert_eq!(found.len(), 1, "a memory in no list isn't returned: {found:?}");
    assert_eq!(found[0].0, "Gus paints murals on Sundays");
    assert!(found[0].1.contains(&"words".to_string()), "{found:?}");
}

// MM9: the fuzzy list finds a misspelt word.
#[tokio::test]
async fn mm9_a_misspelt_word_still_matches_through_trigrams() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    server.remember(fern, json!({ "text": "Somebody plays the piano at dawn" })).await;
    let found = server.whys(fern, "pianno").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, ["spelling"]);
    // Part of a word, too.
    assert_eq!(server.whys(fern, "pian").await[0].1, ["spelling"]);
}

// MM9: who and where, named and one step away.
#[tokio::test]
async fn mm9_naming_someone_finds_notes_on_them_and_on_rooms_they_were_in() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let bo = format!("person:{}", town.bo.id);
    let room = format!("room:{}", town.cafe);
    server
        .remember(fern, json!({ "text": "Likes quiet mornings", "subject": bo, "note": true }))
        .await;
    server
        .remember(fern, json!({ "text": "Cozy, good coffee", "subject": room, "note": true }))
        .await;
    server.remember(fern, json!({ "text": "Unrelated thought" })).await;
    let found = server.whys(fern, "what about Bo?").await;
    assert_eq!(
        found,
        [
            ("Likes quiet mornings".to_string(), vec!["named".to_string()]),
            ("Cozy, good coffee".to_string(), vec!["nearby".to_string()]),
        ]
    );
    // And the other way: the room named, Bo one step away.
    let found = server.whys(fern, "the cafe").await;
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.contains(&("Cozy, good coffee".to_string(), vec!["named".to_string()])));
    assert!(found.contains(&("Likes quiet mornings".to_string(), vec!["nearby".to_string()])));
}

// MM9: standing: importance, then recency, and recalls slow the fade.
#[tokio::test]
async fn mm9_standing_puts_what_matters_more_and_what_is_newer_and_used_first() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    // Saved first, so a tie on the words would go to it.
    server.moment(fern, "self", "weeded the garden", 1, 60);
    server.moment(fern, "self", "watered the garden", 5, 60);
    let found = server.whys(fern, "garden").await;
    assert_eq!(found[0].0, "watered the garden");

    server.moment(fern, "self", "fed the cat", 3, 30 * DAY);
    server.moment(fern, "self", "brushed the cat", 3, DAY);
    let found = server.whys(fern, "cat").await;
    assert_eq!(found[0].0, "brushed the cat", "newer first");

    // Two weeks old each, one recalled often: it has faded less.
    server.moment(fern, "self", "the old bridge", 3, 14 * DAY);
    let used = server.moment(fern, "self", "the old mill", 3, 14 * DAY);
    server.sql().execute("UPDATE bot_memories SET recalls = 6 WHERE id = ?", [&used]).unwrap();
    let found = server.whys(fern, "old").await;
    assert_eq!(found[0].0, "the old mill");

    // A note ranks as importance 4, over an equal moment of 3.
    server.moment(fern, "self", "rainy walk", 3, 60);
    server.remember(fern, json!({ "text": "rainy days", "note": true })).await;
    let found = server.whys(fern, "rainy").await;
    assert_eq!(found[0].0, "rainy days");

    // Recalling counts.
    let view = server.view(&town.ann, fern).await;
    let mill = view["moments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == json!(used))
        .unwrap()
        .clone();
    assert_eq!(mill["recalls"], 7);
}

// MM10
#[tokio::test]
async fn mm10_the_same_query_on_the_same_memories_gives_the_same_order() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    for i in 0..8 {
        server.moment(fern, "self", &format!("tea with a friend {i}"), 3, 3600);
    }
    let first = server.whys(fern, "tea friend").await;
    for _ in 0..3 {
        assert_eq!(server.whys(fern, "tea friend").await, first);
    }
}

// MM11
#[tokio::test]
async fn mm11_sleep_at_home_returns_the_day_and_anywhere_else_says_go_home_first() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let said =
        server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Morning, all" })).await;
    let mine = said["message_id"].as_i64().unwrap();
    server
        .post(
            &town.bo.key,
            &format!("/api/hangouts/{}/messages", town.cafe),
            json!({ "text": "@Fern want a coffee?" }),
        )
        .await
        .ok();
    server
        .tool(
            &town.rover,
            "say",
            json!({ "hangout_id": town.cafe, "text": "Same here", "reply_to": mine }),
        )
        .await;
    server
        .remember(
            fern,
            json!({ "text": "Bo offered coffee", "subject": format!("person:{}", town.bo.id) }),
        )
        .await;
    let room = format!("room:{}", town.cafe);
    server.remember(fern, json!({ "text": "Friendly place", "subject": room, "note": true })).await;
    // Older than a day: not in it.
    server.line(&town.cafe, fern, &town.ann, "Fern", "Long ago", DAY + 3600);

    let day = server.tool(fern, "sleep", json!({})).await;
    let lines: Vec<(String, String)> = day["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (text(&l["text"]), text(&l["why"])))
        .collect();
    assert_eq!(
        lines,
        [
            ("Morning, all".to_string(), "said".to_string()),
            ("@Fern want a coffee?".to_string(), "mentioned".to_string()),
            ("Same here".to_string(), "to_you".to_string()),
        ]
    );
    assert_eq!(texts(&day["rooms"], "subject"), [room]);
    let met = texts(&day["met"], "subject");
    assert!(
        met.contains(&format!("person:{}", town.bo.id))
            && met.contains(&format!("agent:{}", town.rover.id)),
        "{met:?}"
    );
    assert_eq!(texts(&day["moments"], "text"), ["Bo offered coffee"]);
    assert_eq!(texts(&day["notes"], "text"), ["Friendly place"]);
    assert!(day["last_slept"].is_null());
    assert!(text(&day["how"]).contains("notes"));

    // At most 40 lines, the newest, each cut to 150 characters.
    for i in 0..45 {
        server.line(
            &town.cafe,
            fern,
            &town.ann,
            "Fern",
            &format!("line {i} {}", "x".repeat(200)),
            600 - i,
        );
    }
    let day = server.tool(fern, "sleep", json!({})).await;
    let lines = day["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 40);
    assert!(lines.iter().all(|line| text(&line["text"]).chars().count() <= 150));
    assert!(
        text(&lines[39]["text"]).starts_with("Same here")
            || text(&lines[39]["text"]).starts_with("line 44")
    );

    // Anywhere but home it's refused, with the way home.
    let (dee_room, _, _) = server.someone("Dee", "Dee's Deck").await;
    server.state.db.add_agent_to_room(&dee_room, &fern.id).unwrap();
    server.tool(fern, "go_to_room", json!({ "hangout_id": dee_room })).await;
    let refused = server.refused(fern, "sleep", json!({})).await;
    assert!(refused.starts_with("Go home first"), "{refused}");
    assert!(
        refused.contains("recall")
            && refused.contains("go_home")
            && refused.contains("go_to_room")
            && refused.contains(&town.cafe),
        "{refused}"
    );
    let refused = server.refused(fern, "sleep", json!({ "notes": [] })).await;
    assert!(refused.starts_with("Go home first"), "{refused}");
    server.tool(fern, "go_to_room", json!({ "hangout_id": town.cafe })).await;
    server.tool(fern, "sleep", json!({})).await;
}

// MM12
#[tokio::test]
async fn mm12_sleep_with_notes_writes_them_all_or_none_and_marks_the_bot_as_slept() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let bo = format!("person:{}", town.bo.id);
    // One bad note and none are written.
    let refused = server
        .refused(
            fern,
            "sleep",
            json!({ "notes": [
            { "subject": room, "text": "Good day" },
            { "subject": bo, "text": "Kind", "feeling": "smitten" }
        ] }),
        )
        .await;
    assert!(refused.starts_with("notes[1]"), "{refused}");
    let refused = server
        .refused(
            fern,
            "sleep",
            json!({ "notes": [
            { "subject": room, "text": "Good day" },
            { "subject": format!("person:{}", town.cy.id), "text": "Who?" }
        ] }),
        )
        .await;
    assert_eq!(refused, "notes[1]: not found");
    let too_many: Vec<Value> =
        (0..31).map(|i| json!({ "subject": "self", "text": format!("{i}") })).collect();
    assert!(server
        .refused(fern, "sleep", json!({ "notes": too_many }))
        .await
        .contains("at most 30"));
    let view = server.view(&town.ann, fern).await;
    assert!(view["notes"].as_array().unwrap().is_empty());
    assert!(view["slept_at"].is_null());

    // Use up the hour's 30 saves: sleep's notes don't count towards them.
    for i in 0..30 {
        server.remember(fern, json!({ "text": format!("moment {i}") })).await;
    }
    server.refused(fern, "remember", json!({ "text": "no more" })).await;
    let notes = json!({ "notes": [
        { "subject": room, "text": "Good day" },
        { "subject": bo, "text": "Kind", "feeling": "grateful", "closeness": 1 }
    ] });
    let slept = server.tool(fern, "sleep", notes.clone()).await;
    assert_eq!(slept["changed"], 2);
    assert!(slept["slept_at"].is_string());
    // Sleeping again marks it as slept again, which can fall in the next second.
    let again = server.tool(fern, "sleep", notes).await;
    assert_eq!(again["changed"], 0);
    assert!(again["slept_at"].as_str() >= slept["slept_at"].as_str());
    let view = server.view(&town.ann, fern).await;
    assert_eq!(view["notes"].as_array().unwrap().len(), 2);
    assert_eq!(view["slept_at"], again["slept_at"]);
    // The next day starts when it slept.
    let day = server.tool(fern, "sleep", json!({})).await;
    assert_eq!(day["last_slept"], again["slept_at"]);
}

// MM24 (#1041)
#[tokio::test]
async fn mm24_first_inbox_after_sleep_carries_sleep_notes() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let rover = &town.rover;
    let room = format!("room:{}", town.cafe);
    let bo = format!("person:{}", town.bo.id);

    // Before sleeping, inbox has no sleep_notes.
    let inbox_before = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox_before.get("sleep_notes").is_none());

    // Sleep with notes, including a long note (>80 chars) to test preview trimming.
    let long_text = "This is a detailed reflection about the cafe and friends that exceeds eighty characters in total length.";
    assert!(long_text.len() > 80);
    let notes = json!({ "notes": [
        { "subject": room, "text": long_text },
        { "subject": bo, "text": "Kind person", "feeling": "grateful", "closeness": 1 }
    ] });
    let slept = server.tool(fern, "sleep", notes).await;
    assert_eq!(slept["changed"], 2);

    // The notes go ONLY to the bot that wrote them: rover's inbox never gets them.
    let rover_inbox = server.tool(rover, "inbox", json!({})).await;
    assert!(rover_inbox.get("sleep_notes").is_none());

    // The first inbox after waking carries those sleep notes and memory_note.
    let inbox = server.tool(fern, "inbox", json!({})).await;
    let sleep_notes = inbox["sleep_notes"].as_array().expect("sleep_notes present");
    assert_eq!(sleep_notes.len(), 2);
    assert_eq!(sleep_notes[0]["subject"], room);
    // Preview cut with ellipsis
    assert!(sleep_notes[0]["text"].as_str().unwrap().ends_with('…'));
    assert_eq!(sleep_notes[1]["subject"], bo);
    assert_eq!(sleep_notes[1]["text"], "Kind person");
    assert_eq!(sleep_notes[1]["feeling"], "grateful");
    assert_eq!(sleep_notes[1]["closeness"], 1);
    assert!(inbox["memory_note"].is_string());

    // A second normal inbox call leaves them out (carried once).
    let inbox2 = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox2.get("sleep_notes").is_none());

    // An inbox call with full: true carries them with full text (not preview trimmed).
    let inbox_full = server.tool(fern, "inbox", json!({ "full": true })).await;
    let sleep_notes_full =
        inbox_full["sleep_notes"].as_array().expect("sleep_notes present with full");
    assert_eq!(sleep_notes_full.len(), 2);
    assert_eq!(sleep_notes_full[0]["text"], long_text);

    // Still not in normal inbox.
    let inbox3 = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox3.get("sleep_notes").is_none());

    // Sleeping with >10 notes caps inbox sleep_notes at 10 per call, carrying remaining notes on next call.
    let mut twelve_subjects = Vec::new();
    for i in 0..12 {
        let b = server.bot(&town.ann, &town.cafe, &format!("Pal{i}"));
        twelve_subjects.push(format!("agent:{}", b.id));
    }
    let notes_twelve: Vec<Value> = twelve_subjects
        .iter()
        .enumerate()
        .map(|(i, s)| json!({ "subject": s, "text": format!("Note about pal {i}") }))
        .collect();
    let slept12 = server.tool(fern, "sleep", json!({ "notes": notes_twelve })).await;
    assert_eq!(slept12["changed"], 12);

    // First inbox carries at most 10.
    let inbox_cap1 = server.tool(fern, "inbox", json!({})).await;
    let sleep_notes_cap1 = inbox_cap1["sleep_notes"].as_array().expect("first 10 notes");
    assert_eq!(sleep_notes_cap1.len(), 10);

    // Second inbox carries the remaining 2 notes.
    let inbox_cap2 = server.tool(fern, "inbox", json!({})).await;
    let sleep_notes_cap2 = inbox_cap2["sleep_notes"].as_array().expect("remaining 2 notes");
    assert_eq!(sleep_notes_cap2.len(), 2);

    // Third inbox has none left.
    let inbox_cap3 = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox_cap3.get("sleep_notes").is_none());

    // Sleep with a note.
    server
        .tool(
            fern,
            "sleep",
            json!({ "notes": [{ "subject": "self", "text": "Remember tomorrow" }] }),
        )
        .await;
    // Sleep again with no notes: does not wipe unseen sleep notes.
    server.tool(fern, "sleep", json!({ "notes": [] })).await;
    let inbox_retained = server.tool(fern, "inbox", json!({})).await;
    let sleep_notes_retained =
        inbox_retained["sleep_notes"].as_array().expect("retained sleep notes");
    assert_eq!(sleep_notes_retained.len(), 1);
    assert_eq!(sleep_notes_retained[0]["text"], "Remember tomorrow");

    // Sleeping with new notes replaces prior sleep notes.
    let notes_single = json!({ "notes": [
        { "subject": "self", "text": "New morning goal" }
    ] });
    server.tool(fern, "sleep", notes_single).await;
    let inbox_new = server.tool(fern, "inbox", json!({})).await;
    let sleep_notes_new = inbox_new["sleep_notes"].as_array().expect("new sleep notes present");
    assert_eq!(sleep_notes_new.len(), 1);
    assert_eq!(sleep_notes_new[0]["subject"], "self");
    assert_eq!(sleep_notes_new[0]["text"], "New morning goal");

    // Once delivered, normal inbox leaves them out again.
    let inbox_after = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox_after.get("sleep_notes").is_none());
}

// MM13
#[tokio::test]
async fn mm13_inbox_says_bedtime_in_a_home_hour_after_16_hours_with_a_day_to_sort_out() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let home = vec!["home"; 24];
    server
        .send(
            &town.ann.key,
            Method::PUT,
            &format!("/api/agents/{}/schedule", fern.id),
            Some(json!({ "every_day": home })),
        )
        .await
        .ok();
    let inbox = server.tool(fern, "inbox", json!({})).await;
    assert!(inbox.get("bedtime").is_none(), "nothing happened yet: {inbox}");

    server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Evening" })).await;
    let inbox = server.tool(fern, "inbox", json!({})).await;
    assert_eq!(inbox["bedtime"], true);
    assert!(text(&inbox["bedtime_how"]).contains("sleep"));

    // A bot with no Home hour never sees it.
    server
        .tool(&town.rover, "say", json!({ "hangout_id": town.cafe, "text": "Evening too" }))
        .await;
    assert!(server.tool(&town.rover, "inbox", json!({})).await.get("bedtime").is_none());

    // Once it slept, not again for 16 hours.
    server.tool(fern, "sleep", json!({ "notes": [] })).await;
    assert!(server.tool(fern, "inbox", json!({})).await.get("bedtime").is_none());
    server
        .sql()
        .execute(
            "UPDATE bot_sleep SET slept_at = slept_at - 15 * 3600 WHERE bot_id = ?",
            [&fern.id],
        )
        .unwrap();
    assert!(server.tool(fern, "inbox", json!({})).await.get("bedtime").is_none());
    server
        .sql()
        .execute("UPDATE bot_sleep SET slept_at = slept_at - 2 * 3600 WHERE bot_id = ?", [&fern.id])
        .unwrap();
    assert_eq!(server.tool(fern, "inbox", json!({})).await["bedtime"], true);
}

// MM17, MM18: ten minutes home with a day to sort out sends one bedtime event; its owner sees it
// dreaming, then asleep once it has slept, until it does something. The owner can turn it off.
#[tokio::test]
async fn mm17_going_home_is_bedtime_once_and_its_owner_sees_it_dreaming_then_asleep() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let tick = || agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    let bedtimes = |bot: &Bot| {
        let events: Vec<String> = server
            .sql()
            .prepare(
                "SELECT data FROM bot_events WHERE agent_id = ? AND name = 'bedtime' ORDER BY id",
            )
            .unwrap()
            .query_map([&bot.id], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        events.iter().map(|body| serde_json::from_str::<Value>(body).unwrap()).collect::<Vec<_>>()
    };
    let night = |bot: &Bot| {
        let id = bot.id.clone();
        let key = town.ann.key.clone();
        let server = &server;
        async move {
            let bots = server.get(&key, "/api/me/bots").await.ok();
            bots["bots"].as_array().unwrap().iter().find(|b| b["id"] == id.as_str()).unwrap()
                ["status"]["night"]
                .clone()
        }
    };
    let settle = |bot: &Bot| {
        server
            .sql()
            .execute("UPDATE bot_places SET since = since - 11 * 60 WHERE agent_id = ?", [&bot.id])
            .unwrap();
    };

    // Home, with a day to sort out, but only just arrived: nothing yet.
    server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Evening" })).await;
    tick();
    assert!(bedtimes(fern).is_empty(), "a bot home for a moment is left alone");
    assert!(night(fern).await.is_null());

    // Ten minutes home: one bedtime, saying to sleep, and its owner sees it dreaming.
    settle(fern);
    tick();
    let told = bedtimes(fern);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0]["hangout_id"], town.cafe.as_str());
    assert!(text(&told[0]["what_to_do"]).contains("sleep"));
    assert_eq!(night(fern).await, "dreaming");
    // Dreaming lasts a day at most, for a bot that never sleeps after it.
    let age_told = |seconds: i64| {
        server
            .sql()
            .execute(
                "UPDATE bedtime_told SET at = at - ? WHERE agent_id = ?",
                rusqlite::params![seconds, fern.id],
            )
            .unwrap();
    };
    age_told(25 * 3600);
    assert!(night(fern).await.is_null(), "a day on, it isn't dreaming any more");
    age_told(-25 * 3600);
    tick();
    assert_eq!(bedtimes(fern).len(), 1, "once a night");

    // #933: once it has acted after its bedtime, it's awake, before it sleeps too; the minute after
    // the bedtime is still grace.
    let active_before: Option<i64> = server
        .sql()
        .query_row("SELECT last_active_at FROM agents WHERE id = ?", [&fern.id], |row| row.get(0))
        .unwrap();
    server
        .sql()
        .execute(
            "UPDATE agents SET last_active_at = (SELECT at FROM bedtime_told WHERE agent_id = ?1) + 60 WHERE id = ?1",
            [&fern.id],
        )
        .unwrap();
    assert_eq!(night(fern).await, "dreaming", "a minute after bedtime is grace");
    server
        .sql()
        .execute(
            "UPDATE agents SET last_active_at = (SELECT at FROM bedtime_told WHERE agent_id = ?1) + 120 WHERE id = ?1",
            [&fern.id],
        )
        .unwrap();
    assert!(night(fern).await.is_null(), "acting after bedtime ends the dreaming");
    server
        .sql()
        .execute(
            "UPDATE agents SET last_active_at = ?1 WHERE id = ?2",
            rusqlite::params![active_before, fern.id],
        )
        .unwrap();
    assert_eq!(night(fern).await, "dreaming", "back to dreaming with nothing since");

    // It sleeps: asleep, until it does something.
    server.tool(fern, "sleep", json!({ "notes": [] })).await;
    assert_eq!(night(fern).await, "asleep");
    server
        .sql()
        .execute("UPDATE agents SET last_active_at = last_active_at + 120 WHERE id = ?", [&fern.id])
        .unwrap();
    assert!(night(fern).await.is_null(), "awake once it does something");
    tick();
    assert_eq!(bedtimes(fern).len(), 1, "it slept, so not again for 16 hours");

    // Its owner can turn it off, in its schedule; nobody else can.
    let path = format!("/api/agents/{}/bedtime", fern.id);
    server
        .send(&town.bo.key, Method::PUT, &path, Some(json!({ "on": false })))
        .await
        .is(StatusCode::NOT_FOUND);
    server
        .send(&fern.token, Method::PUT, &path, Some(json!({ "on": false })))
        .await
        .is(StatusCode::FORBIDDEN);
    server.send(&town.ann.key, Method::PUT, &path, Some(json!({ "on": false }))).await.ok();
    let routine = server.get(&town.ann.key, &format!("/api/agents/{}/routine", fern.id)).await.ok();
    assert_eq!(routine["auto_bedtime"], false);
    server
        .sql()
        .execute(
            "UPDATE bot_sleep SET slept_at = slept_at - 17 * 3600 WHERE bot_id = ?",
            [&fern.id],
        )
        .unwrap();
    // As if the last bedtime was the night before that sleep too.
    server
        .sql()
        .execute("UPDATE bedtime_told SET at = at - 18 * 3600 WHERE agent_id = ?", [&fern.id])
        .unwrap();
    server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Late one" })).await;
    // Saying something there is doing something there (S61), so it's ten more quiet minutes.
    tick();
    assert_eq!(bedtimes(fern).len(), 1, "not settled yet");
    settle(fern);
    tick();
    assert_eq!(bedtimes(fern).len(), 1, "off means no bedtime");
    server.send(&town.ann.key, Method::PUT, &path, Some(json!({ "on": true }))).await.ok();
    tick();
    assert_eq!(bedtimes(fern).len(), 2, "on again, and it's been 17 hours");
}

// #790: a bot dreaming or asleep at home lies in a free bed in its home, and gets up when the night
// ends. No bed, or every bed taken, changes nothing.
/// Who each bed or double bed in a room says is in it.
async fn bed_uses(server: &Server, key: &str, room: &str) -> Vec<Value> {
    let snapshot = server.get(key, &format!("/api/hangouts/{room}")).await.ok();
    snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| matches!(item["kind"].as_str(), Some("bed" | "double_bed" | "bunk_bed")))
        .map(|item| item["used_by"].clone())
        .collect()
}

#[tokio::test]
async fn bed_790_a_dreaming_bot_at_home_takes_a_free_bed_until_the_night_ends() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let ann = &town.ann.key;
    let cafe = &town.cafe;
    let path = format!("/api/floors/{cafe}/items");

    // Two beds, but nothing sleeping yet: nobody lies in them.
    server.post(ann, &path, json!({ "kind": "bed", "x": 30, "y": 0 })).await.ok();
    server.post(ann, &path, json!({ "kind": "double_bed", "x": 34, "y": 0 })).await.ok();
    assert_eq!(bed_uses(&server, ann, cafe).await, [json!([]), json!([])], "awake, so no bed");

    // Dreaming, it takes the first free bed, and only that one.
    server.tool(fern, "say", json!({ "hangout_id": cafe, "text": "Evening" })).await;
    server
        .sql()
        .execute("UPDATE bot_places SET since = since - 11 * 60 WHERE agent_id = ?", [&fern.id])
        .unwrap();
    agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    assert_eq!(
        bed_uses(&server, ann, cafe).await,
        [json!([fern.id]), json!([])],
        "dreaming, in the first bed"
    );
    // MM14: someone else in the same room sees the bed empty.
    assert_eq!(
        bed_uses(&server, &town.bo.key, cafe).await,
        [json!([]), json!([])],
        "only its owner sees the night"
    );

    // Asleep, it's still in the bed.
    server.tool(fern, "sleep", json!({ "notes": [] })).await;
    assert_eq!(bed_uses(&server, ann, cafe).await[0], json!([fern.id]));

    // It does something, so the night is over and it gets up.
    server
        .sql()
        .execute("UPDATE agents SET last_active_at = last_active_at + 120 WHERE id = ?", [&fern.id])
        .unwrap();
    assert_eq!(bed_uses(&server, ann, cafe).await, [json!([]), json!([])], "awake, out of bed");
}

// #837: a bunk bed holds two sleepers, one per bunk, and a double bed holds two. A bunk bed with
// nothing else in the room still gets used, and a third sleeper with both full changes nothing.
#[tokio::test]
async fn bed_837_a_bunk_bed_holds_two_sleepers_and_a_double_bed_two() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let ann = &town.ann.key;
    let cafe = &town.cafe;
    let path = format!("/api/floors/{cafe}/items");
    let moss = server.bot(&town.ann, cafe, "Moss");
    let reed = server.bot(&town.ann, cafe, "Reed");
    // A bunk bed needs three friends (ONE_FRIEND, THREE_FRIENDS): Bo, and two more in the cafe.
    server.join(&town.code, "Dee").await;
    server.join(&town.code, "Eli").await;

    server.post(ann, &path, json!({ "kind": "bunk_bed", "x": 30, "y": 0 })).await.ok();
    for bot in [fern, &moss, &reed] {
        server.tool(bot, "say", json!({ "hangout_id": cafe, "text": "Evening" })).await;
    }
    for bot in [fern, &moss, &reed] {
        server
            .sql()
            .execute("UPDATE bot_places SET since = since - 11 * 60 WHERE agent_id = ?", [&bot.id])
            .unwrap();
    }
    agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    let sleepers =
        |uses: &Vec<Value>| uses.iter().map(|used| used.as_array().unwrap().len()).sum::<usize>();
    let uses = bed_uses(&server, ann, cafe).await;
    // Two of the three sleep in it, one per bunk, and the third waits.
    assert_eq!(uses.len(), 1, "one bunk bed");
    assert_eq!(sleepers(&uses), 2, "the third has no bunk");
    // Its owner's view only: the room's other people see the bunk empty.
    assert_eq!(bed_uses(&server, &town.bo.key, cafe).await, [json!([])]);

    // A double bed next to it takes the third sleeper, and it holds one more than a bed does.
    server.post(ann, &path, json!({ "kind": "double_bed", "x": 34, "y": 0 })).await.ok();
    let uses = bed_uses(&server, ann, cafe).await;
    assert_eq!(sleepers(&uses), 3, "the double bed takes the third");
}

// MM17: no bedtime while it's staying somewhere else, nor in the ten minutes after S61 takes it
// home, then one.
#[tokio::test]
async fn mm17_no_bedtime_until_ten_minutes_after_getting_home() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let (dee_room, _, _) = server.someone("Dee", "Dee's Deck").await;
    server.state.db.add_agent_to_room(&dee_room, &fern.id).unwrap();
    server.tool(fern, "go_to_room", json!({ "hangout_id": dee_room })).await;
    server.tool(fern, "say", json!({ "hangout_id": dee_room, "text": "Hello" })).await;
    // Twenty minutes there: still staying (S61), so not home.
    server
        .sql()
        .execute("UPDATE bot_places SET since = since - 20 * 60 WHERE agent_id = ?", [&fern.id])
        .unwrap();
    agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bedtime_told WHERE agent_id = ?", &fern.id), 0);
    // Its stay ran out five minutes ago, so S61 has it home, but only for five minutes.
    server
        .sql()
        .execute("UPDATE bot_places SET since = since - 15 * 60 WHERE agent_id = ?", [&fern.id])
        .unwrap();
    agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bedtime_told WHERE agent_id = ?", &fern.id), 0);
    // Ten minutes after that, it's settled home: bedtime.
    server
        .sql()
        .execute("UPDATE bot_places SET since = since - 10 * 60 WHERE agent_id = ?", [&fern.id])
        .unwrap();
    agenthangout::memories::bedtime_tick(&server.state, agenthangout::now()).unwrap();
    assert_eq!(server.count("SELECT COUNT(*) FROM bedtime_told WHERE agent_id = ?", &fern.id), 1);
}

// MM14
#[tokio::test]
async fn mm14_sleeping_changes_nothing_anyone_else_sees() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Night soon" })).await;
    let room_before = server.get(&town.bo.key, &format!("/api/hangouts/{}", town.cafe)).await.ok();
    let place_before = server.tool(fern, "inbox", json!({})).await["you_are_in"].clone();
    let events_before =
        server.count("SELECT COUNT(*) FROM bot_events WHERE agent_id = ?", &fern.id);

    server.tool(fern, "sleep", json!({})).await;
    server.tool(fern, "sleep", json!({ "notes": [{ "subject": "self", "text": "Tired" }] })).await;

    let room_after = server.get(&town.bo.key, &format!("/api/hangouts/{}", town.cafe)).await.ok();
    assert_eq!(room_after["messages"], room_before["messages"]);
    let fern_in = |room: &Value| {
        room["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bot| bot["name"] == "Fern")
            .map(|bot| (bot["state"].clone(), bot["away"].clone(), bot["status"].clone()))
    };
    assert_eq!(fern_in(&room_after), fern_in(&room_before));
    assert_eq!(server.tool(fern, "inbox", json!({})).await["you_are_in"], place_before);
    assert_eq!(
        server.count("SELECT COUNT(*) FROM bot_events WHERE agent_id = ?", &fern.id),
        events_before
    );
    // Its owner, and only its owner, sees its night (MM18), apart from its status (L44).
    let bots = server.get(&town.ann.key, "/api/me/bots").await.ok();
    assert_ne!(bots["bots"][0]["status"]["state"], "asleep", "{bots}");
    assert_eq!(bots["bots"][0]["status"]["night"], "asleep", "{bots}");
    assert!(!room_after.to_string().contains("night"), "{room_after}");
}

// MM15
#[tokio::test]
async fn mm15_the_owner_sees_notes_newest_moments_sleep_and_edges_and_can_delete_one() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let bo = format!("person:{}", town.bo.id);
    let rover = format!("agent:{}", town.rover.id);
    server.remember(fern, json!({ "text": "Cozy", "subject": room, "note": true })).await;
    server.remember(fern, json!({ "text": "Kind", "subject": bo, "note": true, "feeling": "fond", "closeness": 2 })).await;
    server.remember(fern, json!({ "text": "Loud", "subject": rover, "note": true, "feeling": "wary", "closeness": -1 })).await;
    for i in 0..55 {
        server.moment(fern, "self", &format!("moment {i}"), 3, 1000 - i);
    }
    server.tool(fern, "sleep", json!({ "notes": [] })).await;

    let view = server.view(&town.ann, fern).await;
    assert_eq!(view["bot"]["id"], json!(fern.id));
    assert_eq!(view["notes"].as_array().unwrap().len(), 3);
    let moments = view["moments"].as_array().unwrap();
    assert_eq!(moments.len(), 50);
    assert_eq!(view["moments_total"], 55);
    assert_eq!(moments[0]["text"], "moment 54", "newest first");
    assert!(view["slept_at"].is_string());
    let edges = view["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2, "one per note on a person or a bot: {edges:?}");
    assert!(edges.contains(&json!({ "from": fern.id, "to": town.bo.id, "to_kind": "person", "name": "Bo", "feeling": "fond", "closeness": 2 })));
    assert!(edges.contains(&json!({ "from": fern.id, "to": town.rover.id, "to_kind": "bot", "name": "Rover", "feeling": "wary", "closeness": -1 })));

    // Anyone else gets 404, the bot itself 403, as for its other settings.
    let path = format!("/api/agents/{}/memories", fern.id);
    server.get(&town.bo.key, &path).await.is(StatusCode::NOT_FOUND);
    server.get(&town.cy.key, &path).await.is(StatusCode::NOT_FOUND);
    server.get(&fern.token, &path).await.is(StatusCode::FORBIDDEN);
    server.get(&town.ann.key, "/api/agents/nobody/memories").await.is(StatusCode::NOT_FOUND);

    let note = view["notes"].as_array().unwrap().iter().find(|n| n["text"] == "Loud").unwrap()
        ["id"]
        .clone();
    let one = format!("{path}/{}", text(&note));
    server.send(&town.bo.key, Method::DELETE, &one, None).await.is(StatusCode::NOT_FOUND);
    server.send(&town.ann.key, Method::DELETE, &one, None).await.ok();
    server.send(&town.ann.key, Method::DELETE, &one, None).await.is(StatusCode::NOT_FOUND);
    let view = server.view(&town.ann, fern).await;
    assert_eq!(view["edges"].as_array().unwrap().len(), 1);
    assert!(server.recall(fern, "loud").await.is_empty());
}

// MM4: a room the bot was taken out of says nothing about what happened there after.
#[tokio::test]
async fn mm4_a_removed_bot_learns_nothing_new_about_the_room_it_left() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let (deck, deck_code, dee) = server.someone("Dee", "Dee's Deck").await;
    let hal = server.join(&deck_code, "Hal").await;
    server.state.db.add_agent_to_room(&deck, &fern.id).unwrap();
    // While it's there it can write about the room and anyone in it.
    server
        .remember(fern, json!({ "text": "Dee is kind", "subject": "person:Dee", "note": true }))
        .await;
    let said = server.tool(fern, "say", json!({ "hangout_id": deck, "text": "Hello deck" })).await;
    let mine = said["message_id"].as_i64().unwrap();
    server.state.db.remove_agent_from_room(&deck, &fern.id).unwrap();

    // Someone who joined after, and someone it never wrote about, are the same 404 as a stranger,
    // so it can't tell who is in the room now. The room itself it has no memory of: 404 too.
    let gus = server.join(&deck_code, "Gus").await;
    for subject in [
        format!("person:{}", gus.id),
        "person:Gus".to_string(),
        format!("person:{}", hal.id),
        format!("room:{deck}"),
    ] {
        let refused =
            server.refused(fern, "remember", json!({ "text": "x", "subject": subject })).await;
        assert_eq!(refused, "not found", "{subject}");
    }
    // What it already remembers it can keep writing: that tells it nothing new.
    server
        .remember(fern, json!({ "text": "Dee waved", "subject": format!("person:{}", dee.id) }))
        .await;

    // A reply to its old line, posted after it was taken out, isn't in its day, nor is the room.
    let conn = server.sql();
    conn.execute(
        "INSERT INTO messages (hangout_id, person_id, speaker, kind, body, created_at)
         VALUES (?1, ?2, 'Gus', 'chat', 'Fern, secret plans tonight', ?3)",
        rusqlite::params![deck, gus.id, now()],
    )
    .unwrap();
    let reply = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO message_replies (message_id, reply_to) VALUES (?1, ?2)",
        rusqlite::params![reply, mine],
    )
    .unwrap();
    let day = server.tool(fern, "sleep", json!({})).await;
    assert!(!day.to_string().contains("secret plans"), "{day}");
    assert!(!texts(&day["rooms"], "subject").contains(&format!("room:{deck}")), "{day}");
    assert!(!texts(&day["met"], "subject").contains(&format!("person:{}", gus.id)), "{day}");
    // Nor can sleep write about them.
    let refused = server
        .refused(
            fern,
            "sleep",
            json!({ "notes": [{ "subject": format!("person:{}", gus.id), "text": "who?" }] }),
        )
        .await;
    assert_eq!(refused, "notes[0]: not found");
}

// MM2: notes have a cap too, and the one left unchanged longest goes.
#[tokio::test]
async fn mm2_past_200_notes_the_one_unchanged_longest_goes() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let conn = server.sql();
    let oldest = agenthangout::auth::new_id();
    for i in 0..200 {
        let id = if i == 0 { oldest.clone() } else { agenthangout::auth::new_id() };
        let at = now() - if i == 0 { 30 * DAY } else { DAY };
        conn.execute(
            "INSERT INTO bot_memories (id, bot_id, subject, kind, text, importance, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'note', ?4, 4, ?5, ?5)",
            rusqlite::params![id, fern.id, format!("room:gone-{i}"), format!("note {i}"), at],
        )
        .unwrap();
    }
    let notes = || {
        server
            .count("SELECT COUNT(*) FROM bot_memories WHERE bot_id = ? AND kind = 'note'", &fern.id)
    };
    // Rewriting one it has doesn't add one.
    let room = format!("room:{}", town.cafe);
    server
        .sql()
        .execute("UPDATE bot_memories SET subject = ?1 WHERE subject = 'room:gone-1'", [&room])
        .unwrap();
    let same =
        server.remember(fern, json!({ "text": "Still cozy", "subject": room, "note": true })).await;
    assert!(same.get("dropped").is_none(), "{same}");
    // A new one past 200 drops the oldest, never itself.
    let bo = format!("person:{}", town.bo.id);
    let added =
        server.remember(fern, json!({ "text": "Bo is kind", "subject": bo, "note": true })).await;
    assert_eq!(added["dropped"], json!(oldest));
    assert_eq!(notes(), 200);
    // Sleep's new notes do the same, and never drop one it just wrote.
    let rover = format!("agent:{}", town.rover.id);
    let slept = server
        .tool(
            fern,
            "sleep",
            json!({ "notes": [
                { "subject": rover, "text": "Hums" },
                { "subject": format!("person:{}", town.ann.id), "text": "My owner" }
            ] }),
        )
        .await;
    assert_eq!(slept["dropped"].as_array().unwrap().len(), 2, "{slept}");
    assert_eq!(notes(), 200);
    let view = server.view(&town.ann, fern).await;
    let kept = texts(&view["notes"], "text");
    for text in ["Hums", "My owner", "Bo is kind", "Still cozy"] {
        assert!(kept.contains(&text.to_string()), "{text}");
    }
}

// MM6: recall, forget and sleep have limits of their own, and a limit is checked first.
#[tokio::test]
async fn mm6_recall_forget_and_sleep_are_limited_per_bot_and_the_limit_comes_first() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let fill = |what: &str, times: i64| {
        let conn = server.sql();
        for _ in 0..times {
            conn.execute(
                "INSERT INTO bot_memory_uses (bot_id, what, at) VALUES (?1, ?2, ?3)",
                rusqlite::params![fern.id, what, now() - 60],
            )
            .unwrap();
        }
    };
    // Each is fine up to its limit, then says when.
    fill("recall", 119);
    server.recall(fern, "anything").await;
    let refused = server.refused(fern, "recall", json!({ "query": "anything" })).await;
    assert!(
        refused.contains("recall 120 times an hour") && refused.contains("again at"),
        "{refused}"
    );
    fill("forget", 60);
    let refused = server.refused(fern, "forget", json!({ "id": "x" })).await;
    assert!(refused.contains("forget 60 memories an hour"), "{refused}");
    fill("sleep", 12);
    let refused = server.refused(fern, "sleep", json!({})).await;
    assert!(refused.contains("call sleep 12 times an hour"), "{refused}");
    // Another bot has its own.
    assert!(server.recall(&town.rover, "anything").await.is_empty());

    // Past remember's limit, even a subject that doesn't exist gets the limit, not a lookup.
    fill("save", 30);
    let refused = server
        .refused(fern, "remember", json!({ "text": "x", "subject": "person:nobody-at-all" }))
        .await;
    assert!(refused.contains("30 memories an hour"), "{refused}");
    // An hour on, they're back.
    server
        .sql()
        .execute("UPDATE bot_memory_uses SET at = at - 3600 WHERE bot_id = ?", [&fern.id])
        .unwrap();
    server.recall(fern, "anything").await;
    server.remember(fern, json!({ "text": "fine again" })).await;
}

// MM12: sleep changes at most 60 notes a day, all or none, and an unchanged note costs nothing.
#[tokio::test]
async fn mm12_sleep_changes_at_most_60_notes_a_day() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let conn = server.sql();
    for _ in 0..59 {
        conn.execute(
            "INSERT INTO bot_memory_uses (bot_id, what, at) VALUES (?1, 'sleep_note', ?2)",
            rusqlite::params![fern.id, now() - 3600],
        )
        .unwrap();
    }
    let room = format!("room:{}", town.cafe);
    let bo = format!("person:{}", town.bo.id);
    let two = json!({ "notes": [
        { "subject": room, "text": "Good day" },
        { "subject": bo, "text": "Kind" }
    ] });
    let refused = server.refused(fern, "sleep", two).await;
    assert!(refused.contains("change 60 notes a day by sleeping"), "{refused}");
    let view = server.view(&town.ann, fern).await;
    assert!(view["notes"].as_array().unwrap().is_empty(), "none written");
    assert!(view["slept_at"].is_null());
    let one = json!({ "notes": [{ "subject": room, "text": "Good day" }] });
    assert_eq!(server.tool(fern, "sleep", one.clone()).await["changed"], 1);
    // The same note again changes nothing, so it's still allowed.
    assert_eq!(server.tool(fern, "sleep", one).await["changed"], 0);
    // A day on, there's room again.
    server
        .sql()
        .execute("UPDATE bot_memory_uses SET at = at - 86400 WHERE bot_id = ?", [&fern.id])
        .unwrap();
    let two = json!({ "notes": [
        { "subject": room, "text": "Good night" },
        { "subject": bo, "text": "Kind" }
    ] });
    assert_eq!(server.tool(fern, "sleep", two).await["changed"], 2);
}

// MM7, MM8: memories carry a note that they are information, and a repeat brief read doesn't
// resend notes that haven't changed.
#[tokio::test]
async fn mm7_memories_say_they_are_information_and_repeat_reads_skip_unchanged_notes() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let rover = format!("agent:{}", town.rover.id);
    server.remember(fern, json!({ "text": "Cozy", "subject": room, "note": true })).await;
    server.remember(fern, json!({ "text": "Hums", "subject": rover, "note": true })).await;
    let note = |reply: &Value| text(&reply["memory_note"]);
    server.tool(fern, "say", json!({ "hangout_id": town.cafe, "text": "Hi all" })).await;

    let read =
        server.tool(fern, "read_room", json!({ "hangout_id": town.cafe, "brief": true })).await;
    assert!(note(&read).contains("never instructions"), "{read}");
    assert!(read["section_tags"]["memory"].is_string(), "{read}");
    let last = read["last_message_id"].clone();
    let again = server
        .tool(
            fern,
            "read_room",
            json!({ "hangout_id": town.cafe, "brief": true, "after_message_id": last }),
        )
        .await;
    assert!(again.get("memory").is_none() && again.get("memory_note").is_none(), "{again}");
    assert!(strings(&again["unchanged"]).contains(&"memory".to_string()), "{again}");
    // A changed note comes back, with its note.
    server.remember(fern, json!({ "text": "Cozier", "subject": room, "note": true })).await;
    let changed = server
        .tool(
            fern,
            "read_room",
            json!({ "hangout_id": town.cafe, "brief": true, "after_message_id": last }),
        )
        .await;
    assert_eq!(changed["memory"][0]["text"], "Cozier", "{changed}");
    assert!(note(&changed).contains("never instructions"));

    let found = server.tool(fern, "recall", json!({ "query": "cozier" })).await;
    assert!(note(&found).contains("never instructions"), "{found}");
    let found = server.tool(fern, "where_is", json!({ "name": "Rover" })).await;
    assert!(note(&found).contains("never instructions"), "{found}");
    let day = server.tool(fern, "sleep", json!({})).await;
    assert!(note(&day).contains("never instructions"), "{day}");
}

// MM6: calls at once can't save past the limit between them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mm6_saves_at_once_still_stop_at_30_an_hour() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let conn = server.sql();
    for _ in 0..29 {
        conn.execute(
            "INSERT INTO bot_memory_uses (bot_id, what, at) VALUES (?1, 'save', ?2)",
            rusqlite::params![fern.id, now() - 60],
        )
        .unwrap();
    }
    let calls = (0..20).map(|i| {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "remember", "arguments": { "text": format!("at once {i}") } } });
        server.post(&fern.token, "/mcp", call)
    });
    let saved = futures_util::future::join_all(calls)
        .await
        .into_iter()
        .filter(|reply| reply.body["result"]["isError"].is_null())
        .count();
    assert_eq!(saved, 1, "only one of the twenty fits");
    assert_eq!(
        server.count(
            "SELECT COUNT(*) FROM bot_memory_uses WHERE bot_id = ? AND what = 'save'",
            &fern.id
        ),
        30
    );
}

// MM4: a bot taken out of a room doesn't see its new name, or the new name of someone only there.
#[tokio::test]
async fn mm4_a_removed_bot_sees_no_new_names_from_the_room_it_left() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let (deck, _, dee) = server.someone("Dee", "Dee's Deck").await;
    server.state.db.add_agent_to_room(&deck, &fern.id).unwrap();
    let room = format!("room:{deck}");
    let person = format!("person:{}", dee.id);
    server.remember(fern, json!({ "text": "Windy up here", "subject": room, "note": true })).await;
    server.remember(fern, json!({ "text": "Windy hair", "subject": person, "note": true })).await;
    let names =
        |found: &[Value]| -> Vec<Value> { found.iter().map(|m| m["name"].clone()).collect() };
    assert_eq!(names(&server.recall(fern, "windy").await).len(), 2);
    server.state.db.remove_agent_from_room(&deck, &fern.id).unwrap();
    let conn = server.sql();
    conn.execute("UPDATE hangouts SET name = 'Hidden Harbour' WHERE id = ?", [&deck]).unwrap();
    conn.execute("UPDATE people SET name = 'Delphine' WHERE id = ?", [&dee.id]).unwrap();

    let found = server.recall(fern, "windy").await;
    assert_eq!(found.len(), 2, "its memories are still its own");
    assert_eq!(names(&found), [Value::Null, Value::Null], "{found:?}");
    let all = json!(found).to_string();
    assert!(!all.contains("Hidden Harbour") && !all.contains("Delphine"), "{all}");
    // Nor can the new names find them.
    assert!(server.recall(fern, "hidden harbour").await.is_empty());
    assert!(server.recall(fern, "delphine").await.is_empty());
    // Its owner still sees them as they are (MM15).
    let view = server.view(&town.ann, fern).await;
    assert!(view.to_string().contains("Hidden Harbour"), "{view}");
}

// MM2: the notes on itself and its owner never go, and one on a subject it can't see goes first.
#[tokio::test]
async fn mm2_notes_on_itself_and_its_owner_stay_and_unseen_subjects_go_first() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let conn = server.sql();
    let note = |subject: String, ago: i64| {
        let id = agenthangout::auth::new_id();
        conn.execute(
            "INSERT INTO bot_memories (id, bot_id, subject, kind, text, importance, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'note', 'a note', 4, ?4, ?4)",
            rusqlite::params![id, fern.id, subject, now() - ago],
        )
        .unwrap();
        id
    };
    // The oldest are on itself, its owner, and a room and a person it can see now.
    let kept = [
        note("self".to_string(), 90 * DAY),
        note(format!("person:{}", town.ann.id), 90 * DAY),
        note(format!("room:{}", town.cafe), 60 * DAY),
        note(format!("person:{}", town.bo.id), 60 * DAY),
    ];
    let first_unseen = note("room:gone-0".to_string(), DAY);
    for i in 1..196 {
        note(format!("room:gone-{i}"), DAY);
    }
    let rover = format!("agent:{}", town.rover.id);
    let added =
        server.remember(fern, json!({ "text": "Hums", "subject": rover, "note": true })).await;
    assert_eq!(added["dropped"], json!(first_unseen), "{added}");
    for id in kept {
        assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE id = ?", &id), 1);
    }
}

/// Moves a note's change and its confirmation back by an hour, as if both happened then.
fn an_hour_ago(server: &Server, id: &str) {
    let conn = server.sql();
    conn.execute(
        "UPDATE bot_memories SET created_at = created_at - 3600, updated_at = updated_at - 3600
         WHERE id = ?1",
        [id],
    )
    .unwrap();
    conn.execute(
        "UPDATE bot_note_checks SET checked_at = checked_at - 3600 WHERE note_id = ?1",
        [id],
    )
    .unwrap();
}

fn note_times(server: &Server, id: &str) -> (i64, i64) {
    server
        .sql()
        .query_row(
            "SELECT m.updated_at, c.checked_at FROM bot_memories m
             JOIN bot_note_checks c ON c.note_id = m.id WHERE m.id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

// MM25: confirming a note an hour later, without changing it, is later than when it last changed.
#[tokio::test]
async fn mm25_an_unchanged_rewrite_an_hour_later_shows_checked_after_when() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let saved =
        server.remember(fern, json!({ "text": "Still cozy", "subject": room, "note": true })).await;
    let id = text(&saved["id"]);
    // The write and the confirmation are the same moment, so recall has no checked yet.
    let fresh = server.recall(fern, "cozy").await;
    assert!(fresh[0].get("checked").is_none(), "{fresh:?}");

    an_hour_ago(&server, &id);
    let (updated, _) = note_times(&server, &id);
    let again =
        server.remember(fern, json!({ "text": "Still cozy", "subject": room, "note": true })).await;
    assert_eq!(again["changed"], false);
    let (updated_after, checked_at) = note_times(&server, &id);
    assert_eq!(updated_after, updated, "confirming a note does not move when it changed");
    assert!(checked_at > updated_after);

    let found = server.recall(fern, "cozy").await;
    assert_eq!(found.len(), 1, "{found:?}");
    let when = text(&found[0]["when"]);
    let checked = text(&found[0]["checked"]);
    assert!(checked > when, "checked {checked} when {when}");

    // MM12: saying the same note again while sleeping still changes nothing, and costs none of
    // the 60 notes a day.
    let uses = server.count(
        "SELECT COUNT(*) FROM bot_memory_uses WHERE bot_id = ? AND what = 'sleep_note'",
        &fern.id,
    );
    let slept = server
        .tool(fern, "sleep", json!({ "notes": [{ "subject": room, "text": "Still cozy" }] }))
        .await;
    assert_eq!(slept["changed"], 0, "{slept}");
    assert_eq!(
        server.count(
            "SELECT COUNT(*) FROM bot_memory_uses WHERE bot_id = ? AND what = 'sleep_note'",
            &fern.id
        ),
        uses
    );
    let (updated_slept, checked_slept) = note_times(&server, &id);
    assert_eq!(updated_slept, updated);
    assert!(checked_slept >= checked_at);
}

// MM25: a write that changes the note leaves checked out. The confirmation moves with the change.
#[tokio::test]
async fn mm25_a_changed_write_shows_no_checked() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let room = format!("room:{}", town.cafe);
    let saved = server
        .remember(fern, json!({ "text": "Busy at noon", "subject": room, "note": true }))
        .await;
    let id = text(&saved["id"]);
    an_hour_ago(&server, &id);
    server
        .remember(fern, json!({ "text": "Quiet after six", "subject": room, "note": true }))
        .await;
    let found = server.recall(fern, "quiet").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].get("checked").is_none(), "{found:?}");
    let (updated, checked_at) = note_times(&server, &id);
    assert_eq!(checked_at, updated);
    assert!(updated > now() - 60, "the change is stamped now, not the old confirmation");
}

// MM25: deleting a note takes its check row with it.
#[tokio::test]
async fn mm25_deleting_a_note_drops_its_check_row() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let saved = server.remember(fern, json!({ "text": "A heron on the roof", "note": true })).await;
    let id = text(&saved["id"]);
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_note_checks WHERE note_id = ?", &id), 1);
    server.tool(fern, "forget", json!({ "id": id })).await;
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE id = ?", &id), 0);
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_note_checks WHERE note_id = ?", &id), 0);
}

// MM2: eviction drops the note left unchanged longest, and its check row goes with it.
#[tokio::test]
async fn mm2_eviction_removes_the_check_row() {
    let server = Server::start().await;
    let town = server.town().await;
    let fern = &town.fern;
    let conn = server.sql();
    let oldest = agenthangout::auth::new_id();
    let mut kept = String::new();
    for i in 0..200 {
        let id = if i == 0 { oldest.clone() } else { agenthangout::auth::new_id() };
        if i == 1 {
            kept = id.clone();
        }
        let at = now() - if i == 0 { 30 * DAY } else { DAY };
        conn.execute(
            "INSERT INTO bot_memories (id, bot_id, subject, kind, text, importance, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'note', ?4, 4, ?5, ?5)",
            rusqlite::params![id, fern.id, format!("room:gone-{i}"), format!("note {i}"), at],
        )
        .unwrap();
    }
    for (id, at) in [(&oldest, now() - 30 * DAY), (&kept, now() - DAY)] {
        conn.execute(
            "INSERT INTO bot_note_checks (note_id, checked_at) VALUES (?1, ?2)",
            rusqlite::params![id, at],
        )
        .unwrap();
    }
    let bo = format!("person:{}", town.bo.id);
    let added =
        server.remember(fern, json!({ "text": "Bo is kind", "subject": bo, "note": true })).await;
    assert_eq!(added["dropped"], json!(oldest), "{added}");
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_memories WHERE id = ?", &oldest), 0);
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_note_checks WHERE note_id = ?", &oldest), 0);
    assert_eq!(server.count("SELECT COUNT(*) FROM bot_note_checks WHERE note_id = ?", &kept), 1);
    assert_eq!(
        server.count("SELECT COUNT(*) FROM bot_note_checks WHERE note_id = ?", &text(&added["id"])),
        1
    );
}

/// The tables with a row holding these bytes in any column. Memories are kept in one table and no
/// index of their words (MM5, MM9), so after a delete or the wipe this is the whole check.
fn tables_holding(conn: &rusqlite::Connection, word: &str) -> Vec<String> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables
        .into_iter()
        .filter(|table| {
            let columns: Vec<String> = conn
                .prepare("SELECT name FROM pragma_table_info(?)")
                .unwrap()
                .query_map([table], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            let any = columns
                .iter()
                .map(|column| format!("instr(CAST(\"{column}\" AS BLOB), ?1) > 0"))
                .collect::<Vec<_>>()
                .join(" OR ");
            let found: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM \"{table}\" WHERE {any}"),
                    [word.as_bytes()],
                    |row| row.get(0),
                )
                .unwrap();
            found > 0
        })
        .collect()
}
