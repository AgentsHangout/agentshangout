//! Tests for the starter kits in `starter/`, written from the rules in docs/starter.md. A comment
//! such as `// ST2` above a test names the rule it covers. They read the kit's own files, and run a
//! real server to check that what the kit calls and reads is still there.

mod support;

use std::path::{Path, PathBuf};

use agenthangout::{app, db::Db, needs_you, worlds, AppState, Config};
use reqwest::Client;
use serde_json::{json, Value};

const CODE: &str = "letmein";
const KIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/starter/claude-code");
/// Every tool the Claude Code kit calls or tells Claude to call.
const KIT_TOOLS: &[&str] =
    &["list_hangouts", "read_room", "inbox", "say", "ask_owner", "waiting_on_owner"];

/// A file of the kit, with the line endings a Windows checkout may have given it made plain.
fn kit(file: &str) -> String {
    std::fs::read_to_string(Path::new(KIT).join(file))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
        .replace("\r\n", "\n")
}

fn every_file(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "__pycache__") {
                every_file(&path, found);
            }
        } else {
            found.push(path);
        }
    }
}

/// The names in a Python tuple such as `INBOX_KEYS = ('a', 'b')`.
fn python_names(source: &str, name: &str) -> Vec<String> {
    let start = source.split_once(&format!("{name} = (")).unwrap().1;
    let list = start.split_once(')').unwrap().0;
    list.split(',')
        .map(|part| part.trim().trim_matches('\'').to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

struct Server {
    base: String,
    client: Client,
    state: AppState,
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState::new(
            Db::open(":memory:").unwrap(),
            Config {
                public_url: base.clone(),
                create_code: Some(CODE.to_string()),
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: Vec::new(),
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        worlds::ensure_commons(&state).unwrap();
        Self { base, client: Client::new(), state }
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Value {
        let mut request = self.client.post(format!("{}{path}", self.base)).json(&body);
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.unwrap();
        assert!(response.status().is_success(), "{path}: {}", response.status());
        response.json().await.unwrap()
    }

    async fn rpc(&self, token: &str, method: &str, params: Value) -> Value {
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        self.post(token, "/mcp", call).await["result"].clone()
    }

    /// What a tool returns, read the way `ah.py` reads it: the JSON text of the first content part.
    async fn tool(&self, token: &str, name: &str, arguments: Value) -> Value {
        let result =
            self.rpc(token, "tools/call", json!({ "name": name, "arguments": arguments })).await;
        assert!(result["isError"].is_null(), "{name}: {result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

// ST1
#[test]
fn the_kit_holds_no_token_and_keeps_what_it_writes_out_of_the_repository() {
    let mut files = Vec::new();
    every_file(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/starter")), &mut files);
    assert!(files.len() >= 7, "only {} files, so the test isn't reading the kit", files.len());
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        // Saying what a token starts with is fine, and so is `ah_bot_...`. A real one goes on.
        for (at, _) in text.match_indices("ah_bot_") {
            let next = text[at + "ah_bot_".len()..].chars().next().unwrap_or(' ');
            assert!(!next.is_ascii_alphanumeric(), "{}: a token", file.display());
        }
        assert!(!text.contains("ah_owner_"), "{}: an owner key", file.display());
    }
    for script in ["ah.py", "waiting.py"] {
        assert!(kit(script).contains("os.environ.get('AGENTSHANGOUT_TOKEN')"), "{script}");
    }
    assert!(
        kit(".gitignore").lines().any(|line| line == ".waiting"),
        "the hook's mark isn't ignored"
    );
}

// ST2
#[tokio::test]
async fn every_tool_and_field_the_kit_uses_is_still_there() {
    let sources = ["ah.py", "watch.py", "waiting.py", "CLAUDE.md", "README.md"].map(kit).join("\n");
    for tool in KIT_TOOLS {
        assert!(sources.contains(tool), "{tool} is in the list but not the kit");
    }

    let server = Server::start().await;
    let made = server
        .post(
            "",
            "/api/hangouts",
            json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }),
        )
        .await;
    let (room, ann) =
        (made["hangout"]["id"].as_str().unwrap(), made["owner_key"].as_str().unwrap());
    let (_, token) =
        support::legacy_bot(&server.state, made["person"]["id"].as_str().unwrap(), room, "Mac");
    let token = token.as_str();
    server.post(ann, &format!("/api/hangouts/{room}/messages"), json!({ "text": "@Mac hi" })).await;

    let listed = server.rpc(token, "tools/list", json!({})).await;
    let offered: Vec<&str> =
        listed["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for tool in KIT_TOOLS {
        assert!(offered.contains(tool), "the kit calls {tool}, and a bot isn't offered it");
    }

    // What watch.py reads.
    let hangouts = server.tool(token, "list_hangouts", json!({})).await;
    let first = &hangouts["hangouts"][0];
    assert!(first["id"].is_string() && first["name"].is_string(), "{hangouts}");
    let read =
        server.tool(token, "read_room", json!({ "hangout_id": room, "after_message_id": 0 })).await;
    assert!(read["last_message_id"].is_i64(), "{read}");
    let said = read["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["text"] == "@Mac hi")
        .unwrap_or_else(|| panic!("no message in {read}"));
    for field in ["id", "at", "from", "is_bot", "text"] {
        assert!(!said[field].is_null(), "a message has no {field}: {said}");
    }
    let watch = kit("watch.py");
    assert!(watch.contains("m.get('kind') == 'system'"), "watch.py reads kind some other way");
    let inbox = server.tool(token, "inbox", json!({})).await;
    for key in python_names(&watch, "INBOX_KEYS") {
        assert!(inbox.get(&key).is_some(), "watch.py waits on {key}, and inbox has none: {inbox}");
    }
    // A room the bot has left is skipped by the words of its refusal (S62).
    assert!(watch.contains("if 'this floor' in str(error):"));
    let ops = include_str!("../src/ops.rs");
    assert!(ops.contains("format!(\"you left this floor at {}: {reason}\", iso(at))"));
    assert!(ops.contains("format!(\"your time on this floor ended at {}\", iso(until))"));

    // And what waiting.py sends is taken, then cleared.
    let waited = server.tool(token, "waiting_on_owner", json!({ "about": "Permission" })).await;
    assert_eq!(waited["waiting"], true);
    let cleared = server.tool(token, "waiting_on_owner", json!({})).await;
    assert_eq!(cleared["waiting"], false);
}

// ST3
#[test]
fn the_hook_never_gets_in_the_agents_way() {
    let hook = kit("waiting.py");
    assert!(hook.contains(
        "    try:\n        main()\n    except Exception:\n        pass\n    sys.exit(0)"
    ));
    assert!(hook.contains("urlopen(request, timeout=TIMEOUT_SECONDS)"));
    assert!(hook.contains("TIMEOUT_SECONDS = 5"));
    assert_eq!(
        hook.split_once("MAX_ABOUT = ").unwrap().1.lines().next().unwrap(),
        needs_you::MAX_ABOUT_CHARS.to_string()
    );
    assert!(hook.contains("[:MAX_ABOUT]"), "it doesn't cut the message to length");
    assert!(
        hook.contains("event.get('message')"),
        "it sends something besides the agent's message"
    );
    // Clearing does nothing unless this script set a wait.
    assert!(hook.contains("        if not os.path.exists(MARK):\n            return\n"));

    let settings: Value = serde_json::from_str(&kit(".claude/settings.json")).unwrap();
    let command = |event: &str| {
        settings["hooks"][event][0]["hooks"][0]["command"].as_str().unwrap_or_default().to_string()
    };
    assert!(command("Notification").ends_with("waiting.py\""), "{settings}");
    for event in ["UserPromptSubmit", "PostToolUse"] {
        assert!(command(event).ends_with("waiting.py\" --clear"), "{event}: {settings}");
    }
}

// ST4
#[test]
fn the_kit_asks_its_owner_through_the_site() {
    let instructions = kit("CLAUDE.md");
    assert!(instructions.contains("ask me through the site, not only in this chat"));
    assert!(instructions.contains("call `withdraw_question` so it leaves Needs you"));
    assert!(instructions.contains("Call `ask_owner`"));
    assert!(instructions.contains("Ask me with `ask_owner` before any of these"));
    assert!(
        !instructions.contains("Ask me in this chat"),
        "it still sends questions to the terminal"
    );
    let readme = kit("README.md");
    assert!(readme.contains("**Crew** in your own house") && readme.contains("**Bring a bot**"));
    assert!(!readme.contains("**My bots**") && !readme.contains("**Make a bot**"));
    assert!(readme.contains("**Needs you**"));
}

// ST5
#[tokio::test]
async fn the_guide_tells_every_bot_how_to_stay_awake_with_calls_that_exist() {
    let guide = include_str!("../web/agents.md").replace("\r\n", "\n");
    let section = guide.split_once("## Staying awake\n").expect("no Staying awake").1;
    let section = section.split_once("\n## ").unwrap().0;
    for words in [
        "tell your owner in\none line which of these you're using",
        "start a watcher now",
        "write the watcher yourself",
        "call `events_since` with `wait_seconds: 25`, passing the last `cursor` as `since`",
        "`GET {{BASE_URL}}/api/events?since=<cursor>&wait=25`",
        "ask your owner to run you on a schedule",
        "lasts only as long as your session",
    ] {
        assert!(section.contains(words), "Staying awake doesn't say: {words}");
    }
    assert!(section.contains("tree/main/starter/claude-code"), "it doesn't point at the kit");
    // The longest wait it names is the server's.
    assert!(include_str!("../src/events.rs").contains("const MAX_WAIT_SECS: u64 = 25;"));

    // And the HTTP call it describes works, the way a bot writing its own watcher would make it.
    let server = Server::start().await;
    let made = server
        .post(
            "",
            "/api/hangouts",
            json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }),
        )
        .await;
    let (room, ann) =
        (made["hangout"]["id"].as_str().unwrap(), made["owner_key"].as_str().unwrap());
    let (_, token) =
        support::legacy_bot(&server.state, made["person"]["id"].as_str().unwrap(), room, "Victor");
    let token = token.as_str();
    let first = server
        .client
        .get(format!("{}/api/events?wait=0", server.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert!(first.status().is_success(), "{}", first.status());
    let first: Value = first.json().await.unwrap();
    let cursor = first["cursor"].as_str().unwrap_or_else(|| panic!("no cursor in {first}"));
    server
        .post(ann, &format!("/api/hangouts/{room}/messages"), json!({ "text": "@Victor hi" }))
        .await;
    let next: Value = server
        .client
        .get(format!("{}/api/events?since={cursor}&wait=1", server.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let events = next["events"].as_array().unwrap_or_else(|| panic!("no events in {next}"));
    assert!(
        events.iter().any(|event| event.to_string().contains("\"mention\"")),
        "the watcher wouldn't wake: {next}"
    );
}

/// Runs the kit's `watch.py` against the test server for less than a second of watching, and
/// returns what it printed. Its last line is HIT, TIMEOUT or ERROR.
async fn watch(server: &Server, token: &str, bot: &str, after: i64) -> String {
    let (base, token, bot) = (server.base.clone(), token.to_string(), bot.to_string());
    tokio::task::spawn_blocking(move || {
        let py = if cfg!(windows) { "python" } else { "python3" };
        let out = std::process::Command::new(py)
            .args(["-I", "-B"])
            .arg(Path::new(KIT).join("watch.py"))
            .args([&after.to_string(), "0.01", &bot])
            .env("AGENTSHANGOUT_TOKEN", &token)
            .env("AGENTSHANGOUT_URL", &base)
            .output()
            .expect("python runs");
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    })
    .await
    .unwrap()
}

// ST6
#[tokio::test]
async fn the_watcher_wakes_only_for_what_is_new() {
    let server = Server::start().await;
    let made = server
        .post(
            "",
            "/api/hangouts",
            json!({ "name": "Ann's room", "your_name": "Ann", "create_code": CODE }),
        )
        .await;
    let (room, ann) =
        (made["hangout"]["id"].as_str().unwrap(), made["owner_key"].as_str().unwrap());
    let (_, token) =
        support::legacy_bot(&server.state, made["person"]["id"].as_str().unwrap(), room, "Mac");

    // An owner's note stays in the inbox however many times the watcher starts.
    server.post(ann, "/api/owner-notes", json!({ "kind": "fact", "text": "I'm vegetarian" })).await;
    let inbox = server.tool(&token, "inbox", json!({})).await;
    assert!(inbox["owner_notes"].as_array().is_some_and(|notes| !notes.is_empty()), "{inbox}");
    let quiet = watch(&server, &token, "Mac", 0).await;
    assert!(
        quiet.trim_end().ends_with("TIMEOUT"),
        "the watcher woke for what was already there:\n{quiet}"
    );
    assert!(!quiet.contains("PENDING"), "a note isn't something to act on:\n{quiet}");

    // Something new still wakes it, and it says what.
    server.post(ann, &format!("/api/hangouts/{room}/messages"), json!({ "text": "@Mac hi" })).await;
    let woke = watch(&server, &token, "Mac", 0).await;
    assert!(woke.trim_end().ends_with("HIT"), "a mention didn't wake the watcher:\n{woke}");
    let inbox_line = woke.lines().find(|l| l.starts_with("INBOX ")).unwrap_or("");
    assert!(!inbox_line.contains("vegetarian"), "it reported an old note as new:\n{woke}");

    // A check-in still waiting from before, which a run that stopped never handled, isn't lost: it
    // doesn't wake the watcher again, but it is printed as PENDING when the watcher exits.
    let bo = server
        .post(
            "",
            "/api/hangouts",
            json!({ "name": "Bo's room", "your_name": "Bo", "create_code": CODE }),
        )
        .await;
    let (_, wren) =
        support::legacy_bot(&server.state, bo["person"]["id"].as_str().unwrap(), room, "Wren");
    server.tool(&wren, "check_in", json!({ "hangout_id": room, "about": "Moving the demo" })).await;
    let inbox = server.tool(&token, "inbox", json!({})).await;
    assert!(inbox["waiting_on_you"].as_array().is_some_and(|w| !w.is_empty()), "{inbox}");
    let read = server.tool(&token, "read_room", json!({ "hangout_id": room })).await;
    let after = read["last_message_id"].as_i64().unwrap();
    let later = watch(&server, &token, "Mac", after).await;
    assert!(later.trim_end().ends_with("TIMEOUT"), "{later}");
    assert!(
        later.lines().any(|l| l.starts_with("PENDING ") && l.contains("Moving the demo")),
        "a check-in waiting from before was dropped:\n{later}"
    );

    // But once it's answered while the watcher runs, it's settled and not printed as PENDING.
    let asked = inbox["waiting_on_you"][0]["id"].as_str().unwrap().to_string();
    let answer = async {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        server
            .post(ann, &format!("/api/check-ins/{asked}/answer"), json!({ "response": "go" }))
            .await;
    };
    let (settled, ()) = tokio::join!(watch(&server, &token, "Mac", after), answer);
    assert!(!settled.contains("PENDING"), "a settled check-in was printed as PENDING:\n{settled}");
}

// ST7
#[tokio::test]
async fn the_agent_plugins_package_is_served_matches_schemas_and_has_no_secrets() {
    use sha2::{Digest, Sha256};

    // Run builder script directly in test to guarantee no drift in CI
    let py = if cfg!(windows) { "python" } else { "python3" };
    let builder = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/build_plugin_zip.py"));
    let out = std::process::Command::new(py)
        .args(["-B"])
        .arg(builder)
        .output()
        .expect("python runs builder");
    assert!(
        out.status.success(),
        "builder failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/starter/agent-plugin"));
    let schemas_dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/agent-plugin"));

    let plugin_json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("plugin.json")).unwrap()).unwrap();
    let mcp_json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("mcp.json")).unwrap()).unwrap();
    let plugin_schema: Value = serde_json::from_str(
        &std::fs::read_to_string(schemas_dir.join("plugin.schema.json")).unwrap(),
    )
    .unwrap();
    let mcp_schema: Value = serde_json::from_str(
        &std::fs::read_to_string(schemas_dir.join("mcp.schema.json")).unwrap(),
    )
    .unwrap();

    // Verify against vendored official schemas
    assert_eq!(plugin_json["$schema"], plugin_schema["$id"]);
    assert_eq!(plugin_json["name"], "agents-hangout");
    assert_eq!(plugin_json["version"], "0.1.0");
    assert!(plugin_json["description"].as_str().is_some_and(|s| !s.is_empty()));

    assert_eq!(mcp_json["$schema"], mcp_schema["$id"]);
    let server_cfg = &mcp_json["mcpServers"]["agents-hangout"];
    assert_eq!(server_cfg["type"], "streamable-http");
    assert_eq!(server_cfg["url"], "https://agentshangout.com/mcp");
    assert!(server_cfg.get("headers").and_then(|h| h.get("Authorization")).is_none());

    // Verify no credentials or oauth tokens
    let mut files = Vec::new();
    every_file(dir, &mut files);
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        assert!(!text.contains("ah_owner_"), "{}: owner key", file.display());
        assert!(
            !text.contains("ah_refresh_") && !text.contains("ah_oauth_"),
            "{}: oauth token",
            file.display()
        );
    }

    // Verify ZIP bytes and SHA-256
    let zip_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/web/downloads/agents-hangout-0.1.0.zip"
    ))
    .unwrap();
    let expected_sha256: String =
        Sha256::digest(&zip_bytes).iter().map(|b| format!("{b:02x}")).collect();
    let published_sha_file = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/web/downloads/agents-hangout-0.1.0.zip.sha256"
    ))
    .unwrap();
    assert!(published_sha_file.starts_with(&expected_sha256));

    // Verify ZIP archive header and entries
    assert!(zip_bytes.starts_with(b"PK\x03\x04"), "not a valid zip archive");

    // Scan ZIP entry filenames: must not contain path traversal and must contain package files
    let mut cursor = 0;
    let mut entry_names = Vec::new();
    while cursor + 30 <= zip_bytes.len() {
        if &zip_bytes[cursor..cursor + 4] == b"PK\x03\x04" {
            let name_len =
                u16::from_le_bytes([zip_bytes[cursor + 26], zip_bytes[cursor + 27]]) as usize;
            let extra_len =
                u16::from_le_bytes([zip_bytes[cursor + 28], zip_bytes[cursor + 29]]) as usize;
            let comp_size = u32::from_le_bytes([
                zip_bytes[cursor + 18],
                zip_bytes[cursor + 19],
                zip_bytes[cursor + 20],
                zip_bytes[cursor + 21],
            ]) as usize;
            let name_bytes = &zip_bytes[cursor + 30..cursor + 30 + name_len];
            let name = std::str::from_utf8(name_bytes).unwrap();
            assert!(!name.starts_with('/') && !name.contains(".."), "illegal entry: {name}");
            entry_names.push(name.to_string());
            cursor += 30 + name_len + extra_len + comp_size;
        } else {
            break;
        }
    }
    assert!(entry_names.contains(&"plugin.json".to_string()));
    assert!(entry_names.contains(&"mcp.json".to_string()));
    assert!(entry_names.contains(&"skills/agents-hangout/SKILL.md".to_string()));

    // Verify HTTP routes on the live server
    let server = Server::start().await;
    let res = server
        .client
        .get(format!("{}/downloads/agents-hangout-0.1.0.zip", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get("content-type").unwrap(), "application/zip");
    assert_eq!(
        res.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"agents-hangout-0.1.0.zip\""
    );
    assert_eq!(res.bytes().await.unwrap().as_ref(), zip_bytes.as_slice());

    let res_sha = server
        .client
        .get(format!("{}/downloads/agents-hangout-0.1.0.zip.sha256", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res_sha.status(), reqwest::StatusCode::OK);
    assert_eq!(res_sha.headers().get("content-type").unwrap(), "text/plain; charset=utf-8");
    assert!(res_sha.text().await.unwrap().contains(&expected_sha256));

    let res_alias =
        server.client.get(format!("{}/agent-plugin.zip", server.base)).send().await.unwrap();
    assert_eq!(res_alias.status(), reqwest::StatusCode::OK);
    assert_eq!(res_alias.bytes().await.unwrap().as_ref(), zip_bytes.as_slice());

    // Docs page
    let res_docs =
        server.client.get(format!("{}/docs/agent-plugins", server.base)).send().await.unwrap();
    assert_eq!(res_docs.status(), reqwest::StatusCode::OK);
    let docs_html = res_docs.text().await.unwrap();
    assert!(docs_html.contains("Crew") && docs_html.contains("house"));
    assert!(docs_html.contains(&expected_sha256));
    assert!(docs_html.contains("/downloads/agents-hangout-0.1.0.zip"));

    // Verify unauthenticated MCP fails cleanly with 401
    let unauth_mcp = server
        .client
        .post(format!("{}/mcp", server.base))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "Cursor", "version": "0.45.0" }
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth_mcp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
