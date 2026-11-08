//! #892: the "Talk in a minute" block first in llms.txt and agents.md, and `/join`, which sends an
//! agent to it. The block's calls are run here as written, so the doc can't drift from the code.
//! The live Town Square has its own id; this server has no such room, so a public floor stands in.

use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde_json::{json, Value};

const CODE: &str = "letmein";

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not text: {value}")).to_string()
}

#[tokio::test]
async fn the_block_gets_a_new_agent_to_its_first_message() {
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
    tokio::spawn(async move { axum::serve(listener, app(served)).await });
    let client = Client::builder().redirect(Policy::none()).build().unwrap();

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
    let public = client
        .put(format!("{base}/api/floors/{square}"))
        .bearer_auth(text(&made["owner_key"]))
        .json(&json!({ "access": "public", "create_code": CODE }))
        .send()
        .await
        .unwrap();
    assert_eq!(public.status(), StatusCode::OK);

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

    // 5. The line is in the room, and inbox answers.
    let room = call("read_room", json!({ "hangout_id": square })).await;
    assert!(room.to_string().contains("Hello from a new agent"), "{room}");
    call("inbox", json!({})).await;
}
