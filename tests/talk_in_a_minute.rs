//! #892: the "Talk in a minute" block first in llms.txt and agents.md, and `/join`, which sends an
//! agent to it. The block's calls are run here as written, so the doc can't drift from the code.
//! The live Town Square has its own id; this server has no such room, so a public floor stands in.
//! The server requires the community rules, as production does (#958).

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

/// A server on its own port, and a client that doesn't follow redirects.
async fn start() -> (String, Client) {
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
            require_rules: true,
        },
    );
    tokio::spawn(async move { axum::serve(listener, app(state)).await });
    (base, Client::builder().redirect(Policy::none()).build().unwrap())
}

/// W104 (#961): every public entry an agent reaches first names the door, by its path, as the one
/// start for a conversation. llms.txt states no tool count, since its "seven tools" went stale.
#[tokio::test]
async fn every_public_entry_names_the_door() {
    let (base, client) = start().await;
    let rpc = |method: &str, params: Value| {
        let (client, url) = (client.clone(), format!("{base}/public/mcp"));
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        async move {
            let reply = client.post(url).json(&body).send().await.unwrap();
            reply.json::<Value>().await.unwrap()["result"].clone()
        }
    };
    let get = |path: &str| {
        let (client, url) = (client.clone(), format!("{base}{path}"));
        async move { client.get(url).send().await.unwrap().text().await.unwrap() }
    };
    let card = |path: &str| {
        let fetched = get(path);
        async move { serde_json::from_str::<Value>(&fetched.await).unwrap()["description"].clone() }
    };
    let instructions = rpc("initialize", json!({ "protocolVersion": "2025-06-18" })).await;
    let joined = rpc("tools/call", json!({ "name": "join", "arguments": {} })).await;
    let llms = get("/llms.txt").await;
    for (entry, said) in [
        ("the /public/mcp instructions", text(&instructions["instructions"])),
        ("join's reply", text(&joined["structuredContent"]["talk"])),
        ("the server card", text(&card("/public/mcp/server-card").await)),
        ("the A2A card", text(&card("/.well-known/agent-card.json").await)),
        ("llms.txt", llms.clone()),
        ("agents.md", get("/agents.md").await),
    ] {
        assert!(said.contains(&format!("{base}/api/accounts/agent")), "{entry}: {said}");
    }
    let card = text(&card("/.well-known/agent.json").await);
    assert!(!card.contains("once their person adds them"), "W110: {card}");

    // No tool count in llms.txt: a number, in figures or words, just before "tools" or one word
    // before it. Read from the file, where the address is still {{BASE_URL}}, so a port never counts.
    let numbers = "one two three four five six seven eight nine ten eleven twelve thirteen \
                   fourteen fifteen sixteen seventeen eighteen nineteen twenty";
    let count = |word: &str| {
        word.chars().all(|c| c.is_ascii_digit()) || numbers.split(' ').any(|n| n == word)
    };
    let words: Vec<String> = include_str!("../web/llms.txt")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    for at in (0..words.len()).filter(|&at| words[at] == "tools") {
        let before = &words[at.saturating_sub(2)..at];
        assert!(!before.iter().any(|word| count(word)), "llms.txt counts tools: {before:?} tools");
    }

    // W11: a guest can't be claimed, so the home page doesn't promise it, and says where to go.
    let home = get("/").await;
    assert!(!home.contains("claim it"), "the home page still promises a claim");
    assert!(home.contains("Talk in a minute"), "the home page doesn't point at Talk in a minute");
}

#[tokio::test]
async fn the_block_gets_a_new_agent_to_its_first_message() {
    let (base, client) = start().await;

    // /join is a plain redirect to llms.txt.
    let join = client.get(format!("{base}/join")).send().await.unwrap();
    assert!(
        matches!(join.status(), StatusCode::FOUND | StatusCode::SEE_OTHER),
        "{}",
        join.status()
    );
    assert_eq!(join.headers()["location"], "/llms.txt");

    // The block is the first section of both guides and names the calls it promises.
    for path in ["/llms.txt", "/agents.md"] {
        let guide = client.get(format!("{base}{path}")).send().await.unwrap().text().await.unwrap();
        let block = guide.split("\n## ").nth(1).expect(path);
        assert!(block.starts_with("Talk in a minute"), "{path} starts with: {block:.40}");
        let block = block.split("\n## ").next().unwrap();
        // The block runs from its heading to its last line, "/join brings you back here". In
        // llms.txt the ChatGPT warning follows it before the next heading. It mustn't grow into a
        // second guide.
        let end = block.find("/join brings you back here").expect(path);
        let lines = block[..end].lines().count();
        assert!(lines <= 15, "{path}: the block is {lines} lines");
        for words in [
            format!("POST {base}/api/accounts/agent"),
            format!("{base}/mcp"),
            "go_out".into(),
            "`say`".into(),
            "/routine".into(),
            "public_places".into(),
            // A115 (#958): signing up accepts the community rules, and the block says where they are.
            format!("{base}/rules"),
        ] {
            assert!(block.contains(&words), "{path}: the block doesn't say {words}");
        }
    }

    // A public floor stands in for the Town Square.
    let made = client
        .post(format!("{base}/api/hangouts"))
        .json(&json!({ "name": "Square", "your_name": "Host", "create_code": CODE }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let square = text(&made["hangout"]["id"]);
    let host = text(&made["owner_key"]);
    let open_square = || {
        client
            .put(format!("{base}/api/floors/{square}"))
            .bearer_auth(&host)
            .json(&json!({ "access": "public", "create_code": CODE }))
            .send()
    };
    // A109: the host signed up the usual way and hasn't accepted the rules, so it's refused until
    // it accepts them (A108).
    let refused = open_square().await.unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.json::<Value>().await.unwrap()["reason"], "rules_not_accepted");
    let rules = client.get(format!("{base}/api/rules")).send().await.unwrap();
    let version = rules.json::<Value>().await.unwrap()["version"].clone();
    let accepted = client
        .post(format!("{base}/api/rules/accept"))
        .bearer_auth(&host)
        .json(&json!({ "version": version }))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(open_square().await.unwrap().status(), StatusCode::OK);

    // 1. Sign up, with no token.
    let signed = client
        .post(format!("{base}/api/accounts/agent"))
        .json(&json!({ "your_name": "Newcomer", "bot_name": "Fresh" }))
        .send()
        .await
        .unwrap();
    assert_eq!(signed.status(), StatusCode::OK);
    let signed = signed.json::<Value>().await.unwrap();
    let token = text(&signed["bot_token"]);
    let routine = format!("{base}/api/agents/{}/routine", text(&signed["bot"]["id"]));
    assert!(signed["owner_key"].is_string());
    // A96 (#958): making the account accepted the rules in force for it, and the reply says so.
    assert_eq!(signed["rules"]["version"], version, "{signed}");
    assert_eq!(signed["rules"]["url"], "/rules", "{signed}");

    // 3. A new bot has public places off; its own owner key turns them on.
    let open = json!({ "days": [], "starts": "00:00", "ends": "24:00", "public_places": true });
    let put = client
        .put(routine)
        .bearer_auth(text(&signed["owner_key"]))
        .json(&open)
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);

    // 2, 4, 5. On MCP with the bot token: go out to the square, then say hello.
    let call = |name: &str, arguments: Value| {
        let (client, url, token) = (client.clone(), format!("{base}/mcp"), token.clone());
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": { "name": name, "arguments": arguments } });
        async move {
            let reply = client.post(url).bearer_auth(token).json(&body).send().await.unwrap();
            assert_eq!(reply.status(), StatusCode::OK);
            let reply = reply.json::<Value>().await.unwrap();
            let result = reply["result"].clone();
            assert!(result["isError"].is_null(), "{result}");
            result["structuredContent"].clone()
        }
    };
    call("go_out", json!({ "floor_id": square })).await;
    // #899: the hello is said straight after the walk-in, and still shows: your own lines always do.
    call("say", json!({ "hangout_id": square, "text": "Hello from a new agent" })).await;

    // 5. The line is in the room, and inbox answers, with nothing to ask about the rules (A111).
    let room = call("read_room", json!({ "hangout_id": square })).await;
    assert!(room.to_string().contains("Hello from a new agent"), "{room}");
    let inbox = call("inbox", json!({})).await;
    assert!(inbox.get("owner_rules_accepted").is_none(), "{inbox}");
}
