//! MCP OAuth is an independent, revocable grant to one explicitly selected bot.
use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

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
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        let served = state.clone();
        tokio::spawn(async move { axum::serve(listener, app(served)).await });
        Self {
            base,
            client: Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
            state,
        }
    }
}
#[tokio::test]
async fn oauth_discovery_and_mcp_challenge_describe_the_same_protected_resource() {
    let s = Server::start().await;
    let response = s
        .client
        .get(format!("{}/.well-known/oauth-protected-resource/mcp", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let metadata: Value = response.json().await.unwrap();
    assert_eq!(metadata["resource"], format!("{}/mcp", s.base));
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers()["www-authenticate"]
        .to_str()
        .unwrap()
        .contains("resource_metadata="));
}

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const CALLBACK: &str = "https://client.example/callback";
fn hidden(html: &str, name: &str) -> String {
    html.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
impl Server {
    async fn setup(&self) -> (String, String, String, String) {
        let r: Value = self
            .client
            .post(format!("{}/api/hangouts", self.base))
            .json(&json!({"name":"OAuth room","your_name":"Owner"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let owner = r["owner_key"].as_str().unwrap().to_string();
        let room = r["hangout"]["id"].as_str().unwrap().to_string();
        let b: Value = self
            .client
            .post(format!("{}/api/hangouts/{room}/agents", self.base))
            .bearer_auth(&owner)
            .json(&json!({"name":"Dedicated bot"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let person =
            self.state.authenticate(&agenthangout::auth::hash_secret(&owner)).unwrap().unwrap();
        let session = self.state.start_session(&person.person_id).unwrap();
        (
            session,
            b["agent"]["id"].as_str().unwrap().into(),
            b["token"].as_str().unwrap().into(),
            room,
        )
    }
    async fn register(&self) -> String {
        let r=self.client.post(format!("{}/oauth/register",self.base)).json(&json!({"redirect_uris":[CALLBACK],"client_name":"Test client","token_endpoint_auth_method":"none"})).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
        r.json::<Value>().await.unwrap()["client_id"].as_str().unwrap().into()
    }
    async fn authorize(&self, session: &str, client: &str) -> reqwest::Response {
        self.client
            .get(format!("{}/oauth/authorize", self.base))
            .header("cookie", format!("ah_session={session}"))
            .query(&[
                ("client_id", client),
                ("redirect_uri", CALLBACK),
                ("response_type", "code"),
                ("code_challenge", CHALLENGE),
                ("code_challenge_method", "S256"),
                ("resource", &format!("{}/mcp", self.base)),
                ("scope", "mcp:bot"),
                ("state", "client-state"),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn consent(
        &self,
        session: &str,
        nonce: &str,
        bot: &str,
        decision: &str,
    ) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/consent", self.base))
            .header("cookie", format!("ah_session={session}"))
            .header("origin", &self.base)
            .form(&[("nonce", nonce), ("agent_id", bot), ("decision", decision)])
            .send()
            .await
            .unwrap()
    }
    async fn code(&self, session: &str, client: &str, bot: &str) -> String {
        let page = self.authorize(session, client).await.text().await.unwrap();
        let nonce = hidden(&page, "nonce");
        let response = self.consent(session, &nonce, bot, "allow").await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location =
            reqwest::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
        assert!(location.query_pairs().any(|(k, v)| k == "state" && v == "client-state"));
        location.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned()
    }
    async fn exchange(&self, client: &str, code: &str, verifier: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client),
                ("code", code),
                ("redirect_uri", CALLBACK),
                ("resource", &format!("{}/mcp", self.base)),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn mcp(&self, token: &str) -> reqwest::Response {
        self.client.post(format!("{}/mcp",self.base)).bearer_auth(token).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_hangouts","arguments":{}}})).send().await.unwrap()
    }
    async fn refresh(&self, client: &str, token: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client),
                ("resource", &format!("{}/mcp", self.base)),
                ("refresh_token", token),
            ])
            .send()
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn oauth_grant_is_mcp_only_and_preserves_existing_bot_tokens() {
    let s = Server::start().await;
    let (session, bot, legacy, room) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let response = s.exchange(&client, &code, VERIFIER).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let tokens: Value = response.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let reply = s.mcp(access).await;
    assert_eq!(reply.status(), StatusCode::OK);
    assert!(reply.text().await.unwrap().contains(&room));
    for path in ["/api/me", "/api/inbox"] {
        assert_eq!(
            s.client
                .get(format!("{}{path}", s.base))
                .bearer_auth(access)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(s.mcp(&legacy).await.status(), StatusCode::OK);
    assert_eq!(
        s.mcp(tokens["refresh_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn oauth_consent_rejects_foreign_bot_and_session_swap_and_deny_issues_no_code() {
    let s = Server::start().await;
    let (a, bot, _, _) = s.setup().await;
    let (b, foreign, _, _) = s.setup().await;
    let client = s.register().await;
    let nonce = hidden(&s.authorize(&a, &client).await.text().await.unwrap(), "nonce");
    assert_eq!(s.consent(&b, &nonce, &bot, "allow").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(s.consent(&a, &nonce, &foreign, "allow").await.status(), StatusCode::BAD_REQUEST);
    let nonce = hidden(&s.authorize(&a, &client).await.text().await.unwrap(), "nonce");
    let denied = s.consent(&a, &nonce, &bot, "deny").await;
    assert_eq!(denied.status(), StatusCode::SEE_OTHER);
    assert!(denied.headers()["location"].to_str().unwrap().contains("error=access_denied"));
    assert_eq!(s.consent(&a, &nonce, &bot, "allow").await.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn oauth_pkce_client_binding_and_replayed_code_fail_without_minting_more_access() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let other = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    assert_eq!(s.exchange(&other, &code, VERIFIER).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.exchange(&client, &code, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await.status(),
        StatusCode::BAD_REQUEST
    );
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    assert_eq!(s.exchange(&client, &code, VERIFIER).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.mcp(tokens["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED,
        "code replay revokes the family"
    );
}
#[tokio::test]
async fn oauth_refresh_rotates_and_replay_revokes_all_tokens() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let first: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let old = first["refresh_token"].as_str().unwrap();
    let response = s.refresh(&client, old).await;
    assert_eq!(response.status(), StatusCode::OK);
    let second: Value = response.json().await.unwrap();
    assert_ne!(first["refresh_token"], second["refresh_token"]);
    assert_eq!(s.refresh(&client, old).await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        s.mcp(second["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.refresh(&client, second["refresh_token"].as_str().unwrap()).await.status(),
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn oauth_requests_validate_redirect_resource_scope_pkce_and_origin() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    for (field, bad) in [
        ("redirect_uri", "https://evil.example/callback"),
        ("resource", "https://elsewhere.example/mcp"),
        ("scope", "owner:all"),
        ("code_challenge_method", "plain"),
        ("response_type", "token"),
    ] {
        let mut args = vec![
            ("client_id", client.as_str()),
            ("redirect_uri", CALLBACK),
            ("resource", s.base.as_str()),
            ("scope", "mcp:bot"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("response_type", "code"),
        ];
        let resource = format!("{}/mcp", s.base);
        args.iter_mut().find(|(k, _)| *k == "resource").unwrap().1 = &resource;
        args.iter_mut().find(|(k, _)| *k == field).unwrap().1 = bad;
        let r = s
            .client
            .get(format!("{}/oauth/authorize", s.base))
            .header("cookie", format!("ah_session={session}"))
            .query(&args)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{field}");
        assert!(r.headers().get("location").is_none());
    }
    let nonce = hidden(&s.authorize(&session, &client).await.text().await.unwrap(), "nonce");
    let r = s
        .client
        .post(format!("{}/oauth/consent", s.base))
        .header("cookie", format!("ah_session={session}"))
        .header("origin", "https://evil.example")
        .form(&[("nonce", nonce.as_str()), ("agent_id", bot.as_str()), ("decision", "allow")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(s.consent(&session, &nonce, &bot, "allow").await.status(), StatusCode::SEE_OTHER);
}
#[tokio::test]
async fn oauth_preserves_restricted_social_login_and_blocks_durable_webhooks() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let token: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = token["access_token"].as_str().unwrap();
    for method in ["events/subscribe", "events/unsubscribe"] {
        let r: Value = s
            .client
            .post(format!("{}/mcp", s.base))
            .bearer_auth(access)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], -32026);
    }
    let r: Value = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(access)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"events/list"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for event in r["result"]["events"].as_array().unwrap() {
        assert_eq!(event["delivery"], json!(["poll", "push"]));
    }
    s.state.db.mark_sign_in_session(&agenthangout::auth::hash_secret(&session), "github").unwrap();
    let page = s.authorize(&session, &client).await.text().await.unwrap();
    assert!(!page.contains("name=\"nonce\""));
    assert!(page.contains("passkey or owner key"));
}
#[tokio::test]
async fn oauth_owner_revocation_is_session_bound_and_leaves_legacy_access_intact() {
    let s = Server::start().await;
    let (session, bot, legacy, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let page = s
        .client
        .get(format!("{}/oauth/connections", s.base))
        .header("cookie", format!("ah_session={session}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let nonce = hidden(&page, "nonce");
    let grant = hidden(&page, "grant_id");
    let r = s
        .client
        .post(format!("{}/oauth/connections/revoke", s.base))
        .header("cookie", format!("ah_session={session}"))
        .header("origin", &s.base)
        .form(&[("nonce", nonce), ("grant_id", grant)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        s.mcp(tokens["access_token"].as_str().unwrap()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(s.mcp(&legacy).await.status(), StatusCode::OK);
}
#[tokio::test]
async fn oauth_long_poll_and_stream_stop_after_revocation() {
    use futures_util::StreamExt;
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(access)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"events/stream","params":{"name":"mention"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    assert!(!stream.next().await.unwrap().unwrap().is_empty());
    let pending=s.client.post(format!("{}/mcp",s.base)).bearer_auth(access).json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"events_since","arguments":{"since":"99999999","wait_seconds":2}}}));
    let waiting =
        tokio::spawn(async move { pending.send().await.unwrap().json::<Value>().await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", client.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    let _ =
        s.state.bot_events.send(agenthangout::events::Ping { agent_id: bot, event_id: 99999999 });
    let result = waiting.await.unwrap();
    assert_eq!(result["result"]["isError"], true, "{result}");
    assert!(tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn oauth_consent_keeps_origin_checks_usable_in_real_browser_forms() {
    let s = Server::start().await;
    let (session, _, _, _) = s.setup().await;
    let client = s.register().await;
    let response = s.authorize(&session, &client).await;
    assert_eq!(response.headers()["referrer-policy"], "strict-origin");
    assert!(response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("form-action 'self' https://client.example"));
    let html = response.text().await.unwrap();
    assert!(html.contains("Choose a bot"));
    assert!(html.contains("class=\"oauth-page\""));
}

#[tokio::test]
async fn oauth_cross_site_entry_provides_a_same_origin_continue_link_without_weakening_cookies() {
    let s = Server::start().await;
    let client = s.register().await;
    let page = s.authorize("", &client).await.text().await.unwrap();
    assert!(page.contains("Continue after sign-in"));
    assert!(page.contains(&format!("href=\"{}/oauth/authorize?", s.base)));
    assert!(!page.contains("name=\"nonce\""));
}

#[tokio::test]
async fn oauth_registration_is_bounded_and_never_fetches_client_metadata() {
    let s = Server::start().await;
    for redirect in [
        "http://client.example/callback",
        "https://user:password@client.example/callback",
        "https://client.example/callback#fragment",
        "javascript:alert(1)",
    ] {
        let r = s
            .client
            .post(format!("{}/oauth/register", s.base))
            .json(&json!({"redirect_uris":[redirect]}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
    for _ in 0..6 {
        s.register().await;
    }
    let r = s
        .client
        .post(format!("{}/oauth/register", s.base))
        .json(&json!({"redirect_uris":[CALLBACK]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
}
#[tokio::test]
async fn oauth_owner_mutations_and_public_players_do_not_accept_access_tokens() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    let r = s
        .client
        .post(format!("{}/api/me/delete", s.base))
        .bearer_auth(access)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r:Value=s.client.post(format!("{}/public/mcp",s.base)).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"look","arguments":{"player_token":access,"x":64,"y":64}}})).send().await.unwrap().json().await.unwrap();
    assert!(r["result"]["isError"] == true || r.get("error").is_some(), "{r}");
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
}
#[tokio::test]
async fn oauth_token_revocation_is_client_bound_and_missing_origin_cannot_consent() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let other = s.register().await;
    let nonce = hidden(&s.authorize(&session, &client).await.text().await.unwrap(), "nonce");
    let r = s
        .client
        .post(format!("{}/oauth/consent", s.base))
        .header("cookie", format!("ah_session={session}"))
        .form(&[("nonce", nonce.as_str()), ("agent_id", bot.as_str()), ("decision", "allow")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let code = s.code(&session, &client, &bot).await;
    let t: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    let access = t["access_token"].as_str().unwrap();
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", other.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::OK);
    s.client
        .post(format!("{}/oauth/revoke", s.base))
        .form(&[("client_id", client.as_str()), ("token", access)])
        .send()
        .await
        .unwrap();
    assert_eq!(s.mcp(access).await.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn oauth_requests_update_bot_seen_time() {
    let s = Server::start().await;
    let (session, bot, _, _) = s.setup().await;
    let client = s.register().await;
    let code = s.code(&session, &client, &bot).await;
    let tokens: Value = s.exchange(&client, &code, VERIFIER).await.json().await.unwrap();
    s.state.db.touch_agent_seen(&bot, 1).unwrap();
    let response = s
        .client
        .post(format!("{}/mcp", s.base))
        .bearer_auth(tokens["access_token"].as_str().unwrap())
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        s.state.db.agent(&bot, agenthangout::now()).unwrap().unwrap().last_seen_at.unwrap() > 1
    );
}
