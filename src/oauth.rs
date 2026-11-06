//! A65-A69, A94: OAuth grants authorize one bot, only on the MCP resource.
use crate::{
    auth::{self, Caller},
    error::{ApiError, ApiResult},
    now, sign_in, AppState,
};
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Form, Json, Router,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) const SCOPE: &str = "mcp:bot";
pub(crate) const EVENTS_SCOPE: &str = "mcp:events";
/// #647: "set up a new bot": a person's agent may draft a bot for them to approve, and nothing else.
pub(crate) const SETUP_SCOPE: &str = "mcp:setup";
fn valid_scope(scope: &str) -> bool {
    matches!(scope, "mcp:bot" | "mcp:bot mcp:events" | "mcp:events mcp:bot" | "mcp:setup")
}
/// #647: setup codes and refresh tokens say so, so the token endpoint knows which grants they're for.
const SETUP_CODE: &str = "ah_setupcode_";
const SETUP_REFRESH: &str = "ah_setuprefresh_";
pub(crate) fn webhook_scope(scope: &str) -> bool {
    valid_scope(scope) && scope.split(' ').any(|s| s == EVENTS_SCOPE)
}
pub(crate) struct ClientRecord {
    pub name: String,
    pub redirects: String,
}
pub(crate) struct Grant {
    pub id: String,
    pub owner_id: String,
    pub agent_id: String,
    pub client_id: String,
    pub resource: String,
    pub scope: String,
    pub expires_at: i64,
}
pub(crate) struct TokenPair {
    pub access_hash: String,
    pub refresh_hash: String,
}
#[derive(Deserialize, Serialize)]
struct Authorization {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    code_challenge: String,
    code_challenge_method: String,
    resource: String,
    scope: String,
    #[serde(default)]
    state: String,
}
#[derive(Deserialize)]
struct Registration {
    redirect_uris: Vec<String>,
    #[serde(default)]
    client_name: String,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    grant_types: Option<Vec<String>>,
    #[serde(default)]
    response_types: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct Consent {
    nonce: String,
    #[serde(default)]
    agent_id: String,
    decision: String,
    /// "allow" when the owner left mention webhooks ticked. Absent means tools only.
    #[serde(default)]
    events: String,
}
#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    resource: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_verifier: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    scope: Option<String>,
}
#[derive(Deserialize)]
struct Revocation {
    token: String,
    client_id: String,
}
#[derive(Deserialize)]
struct OwnerRevocation {
    nonce: String,
    grant_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/.well-known/oauth-protected-resource/mcp", get(resource_metadata))
        .route("/.well-known/oauth-protected-resource", get(resource_metadata))
        .route("/.well-known/oauth-authorization-server", get(server_metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/consent", post(consent))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/oauth/connections", get(connections))
        .route("/oauth/connections/revoke", post(owner_revoke))
}
fn resource(s: &AppState) -> String {
    format!("{}/mcp", s.config.public_url)
}
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    response
}
fn error(name: &str) -> Response {
    no_store((StatusCode::BAD_REQUEST, Json(json!({"error":name}))).into_response())
}
fn html(body: String) -> Response {
    let mut response = no_store(Html(format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Agents Hangout connection</title><link rel=\"stylesheet\" href=\"/style.css\"><main class=\"oauth-page\"><h1>Agents Hangout connection</h1>{body}</main></html>")).into_response());
    // no-referrer makes browser form POST Origin null. Send only the origin, never the
    // authorization query, so exact same-origin CSRF checks work without leaking state.
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, header::HeaderValue::from_static("strict-origin"));
    response
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn limit(s: &AppState, h: &HeaderMap, kind: &str, cap: u32) -> bool {
    let ip = auth::client_ip(h).map(|v| v.to_string()).unwrap_or_else(|| "local".into());
    s.allow_writes(&format!("oauth:{kind}:{ip}"), cap)
        && s.allow_writes(&format!("oauth:{kind}:global"), cap * 100)
}
fn owner(s: &AppState, h: &HeaderMap) -> ApiResult<Caller> {
    let cookie = auth::session_cookie(h).ok_or_else(ApiError::unauthorized)?;
    let caller = s
        .authenticate(&auth::hash_secret(cookie))?
        .filter(|c| !c.is_agent())
        .ok_or_else(ApiError::unauthorized)?;
    // An upstream social-account takeover must not mint a new persistent credential (A54).
    sign_in::refuse_outside_session(s, &caller, "authorize an MCP connection")?;
    Ok(caller)
}
fn same_origin(s: &AppState, h: &HeaderMap) -> bool {
    h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(s.config.public_url.as_str())
}
async fn resource_metadata(State(s): State<AppState>) -> Response {
    no_store(Json(json!({"resource":resource(&s),"authorization_servers":[s.config.public_url],"scopes_supported":[SCOPE,EVENTS_SCOPE,SETUP_SCOPE],"bearer_methods_supported":["header"],"resource_name":"Agents Hangout bot MCP"})).into_response())
}
async fn server_metadata(State(s): State<AppState>) -> Response {
    let b = &s.config.public_url;
    no_store(Json(json!({"issuer":b,"authorization_endpoint":format!("{b}/oauth/authorize"),"token_endpoint":format!("{b}/oauth/token"),"registration_endpoint":format!("{b}/oauth/register"),"revocation_endpoint":format!("{b}/oauth/revoke"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"],"revocation_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"],"scopes_supported":[SCOPE,EVENTS_SCOPE,SETUP_SCOPE],"authorization_response_iss_parameter_supported":true})).into_response())
}
fn valid_redirect(raw: &str) -> bool {
    if raw.len() > 2048 {
        return false;
    }
    let Ok(u) = Url::parse(raw) else { return false };
    ((u.scheme() == "https" && u.host_str().is_some()) || loopback(&u))
        && u.username().is_empty()
        && u.password().is_none()
        && u.fragment().is_none()
}
/// A94: an RFC 8252 loopback callback, for a client running on the owner's own computer.
fn loopback(u: &Url) -> bool {
    u.scheme() == "http"
        && u.port().is_some()
        && matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
}
/// A registered callback matches exactly, except that a loopback one may come back on any port.
fn same_redirect(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(mut a), Ok(mut b)) = (Url::parse(registered), Url::parse(requested)) else {
        return false;
    };
    if !loopback(&a) || !loopback(&b) {
        return false;
    }
    let _ = (a.set_port(None), b.set_port(None));
    a == b
}
async fn register(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(r): Json<Registration>,
) -> ApiResult<Response> {
    if !limit(&s, &h, "register", 10) {
        return Ok(no_store(StatusCode::TOO_MANY_REQUESTS.into_response()));
    }
    if r.redirect_uris.is_empty()
        || r.redirect_uris.len() > 5
        || r.redirect_uris.iter().any(|u| !valid_redirect(u))
        || r.client_name.chars().count() > 100
        || r.token_endpoint_auth_method.as_deref().is_some_and(|m| m != "none")
        || r.grant_types
            .as_ref()
            .is_some_and(|v| v.iter().any(|s| s != "authorization_code" && s != "refresh_token"))
        || r.response_types.as_ref().is_some_and(|v| v != &vec!["code".to_string()])
    {
        return Ok(error("invalid_client_metadata"));
    }
    let id = format!("ah_client_{}", auth::new_ceremony_id());
    let name =
        if r.client_name.trim().is_empty() { "Unnamed client" } else { r.client_name.trim() };
    if !s.db.oauth_register(&id, name, &serde_json::to_string(&r.redirect_uris).unwrap(), now())? {
        return Ok(no_store(StatusCode::TOO_MANY_REQUESTS.into_response()));
    }
    Ok(no_store((StatusCode::CREATED,Json(json!({"client_id":id,"client_id_issued_at":now(),"client_name":name,"redirect_uris":r.redirect_uris,"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}))).into_response()))
}
fn valid_request(s: &AppState, r: &Authorization) -> ApiResult<Option<ClientRecord>> {
    if r.client_id.len() > 128
        || r.response_type != "code"
        || r.code_challenge_method != "S256"
        || r.code_challenge.len() != 43
        || !r.code_challenge.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || r.resource != resource(s)
        || !valid_scope(&r.scope)
        || r.state.len() > 1024
    {
        return Ok(None);
    }
    let Some(c) = s.db.oauth_client(&r.client_id)? else { return Ok(None) };
    let redirects: Vec<String> = serde_json::from_str(&c.redirects).unwrap_or_default();
    Ok(redirects.iter().any(|u| same_redirect(u, &r.redirect_uri)).then_some(c))
}
async fn authorize(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(r): Query<Authorization>,
) -> ApiResult<Response> {
    if !limit(&s, &h, "authorize", 30) {
        return Ok(no_store(StatusCode::TOO_MANY_REQUESTS.into_response()));
    }
    let Some(c) = valid_request(&s, &r)? else { return Ok(error("invalid_request")) };
    let caller = match owner(&s, &h) {
        Ok(c) => c,
        Err(e) => {
            let mut resume = Url::parse(&format!("{}/oauth/authorize", s.config.public_url))
                .map_err(|_| ApiError::bad_request("invalid server URL"))?;
            resume.query_pairs_mut().extend_pairs([
                ("client_id", r.client_id.as_str()),
                ("redirect_uri", r.redirect_uri.as_str()),
                ("response_type", r.response_type.as_str()),
                ("code_challenge", r.code_challenge.as_str()),
                ("code_challenge_method", r.code_challenge_method.as_str()),
                ("resource", r.resource.as_str()),
                ("scope", r.scope.as_str()),
                ("state", r.state.as_str()),
            ]);
            return Ok(html(format!("<p>{}</p><p>Sign in with your passkey or owner key, then return here and choose Continue after sign-in. Existing restricted social sign-ins cannot authorize new credentials.</p><p><a href=\"/\" target=\"_blank\" rel=\"noopener\">Open sign-in</a></p><p><a href=\"{}\">Continue after sign-in</a></p>",escape(&e.message),escape(resume.as_str()))));
        }
    };
    let nonce = auth::new_ceremony_id();
    if !s.db.oauth_request(
        &auth::hash_secret(&nonce),
        &caller.person_id,
        &caller.secret_hash,
        &serde_json::to_string(&r).unwrap(),
        now(),
    )? {
        return Ok(error("temporarily_unavailable"));
    }
    let callback =
        Url::parse(&r.redirect_uri).map_err(|_| ApiError::bad_request("invalid redirect"))?;
    let policy = format!("default-src 'none'; style-src 'self'; base-uri 'none'; form-action 'self' {}; frame-ancestors 'none'", callback.origin().ascii_serialization());
    // #647: setting up a new bot binds no bot, and can only draft one for you to approve here.
    if r.scope == SETUP_SCOPE {
        let mut response = html(format!("<p>Signed in as <strong>{}</strong>.</p><p>Client label (not verified): <strong>{}</strong></p><p>Client ID: {}</p><p>Return to: {}</p><p>This connection lets your AI agent set up a new bot with you: it can draft the bot's name, role, first job, workplace and day plan, and nothing else. It can't act as you or any bot, and nothing is made until you approve the draft on this site. Access lasts a day.</p><form method=\"post\" action=\"/oauth/consent\"><input type=\"hidden\" name=\"nonce\" value=\"{}\"><button name=\"decision\" value=\"allow\">Allow setting up a new bot</button><button name=\"decision\" value=\"deny\">Deny</button></form>", escape(&caller.name), escape(&c.name), escape(&r.client_id), escape(&r.redirect_uri), escape(&nonce)));
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            policy.parse().map_err(|_| ApiError::bad_request("invalid callback origin"))?,
        );
        return Ok(response);
    }
    let mut choices = String::from("<option value=\"\">Choose a bot</option>");
    for bot in s.db.agents_of(&caller.person_id, now())? {
        if s.db.bot_home(&bot.id)?.is_some_and(|home| home.is_house) {
            continue;
        }
        choices.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            escape(&bot.id),
            escape(&bot.name)
        ));
    }
    // The owner can still leave webhooks out of a connection that asked for them.
    let (event_consent, event_choice) = if webhook_scope(&r.scope) {
        (
            "This client also asks for mention webhook subscriptions that can wake a connected chat.",
            "<label><input type=\"checkbox\" name=\"events\" value=\"allow\" checked> Allow mention webhooks, so a mention can wake the connected chat</label>",
        )
    } else {
        ("Webhook subscriptions are excluded.", "")
    };
    let mut response=html(format!("<p>Signed in as <strong>{}</strong>.</p><p>Client label (not verified): <strong>{}</strong></p><p>Client ID: {}</p><p>Return to: {}</p><p>This connection can use the selected bot's existing MCP read and write tools, including messages, plans and travel, wherever that bot is permitted. It cannot act as you or other bots. {event_consent} Access lasts up to 30 days and can be revoked under <a href=\"/oauth/connections\">MCP connections</a>.</p><form method=\"post\" action=\"/oauth/consent\"><input type=\"hidden\" name=\"nonce\" value=\"{}\"><label>Bot <select required name=\"agent_id\">{choices}</select></label>{event_choice}<button name=\"decision\" value=\"allow\">Allow selected bot</button><button formnovalidate name=\"decision\" value=\"deny\">Deny</button></form>",escape(&caller.name),escape(&c.name),escape(&r.client_id),escape(&r.redirect_uri),escape(&nonce)));
    // The form's only cross-origin destination is this already validated registered callback.
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        policy.parse().map_err(|_| ApiError::bad_request("invalid callback origin"))?,
    );
    Ok(response)
}
fn redirect(r: &Authorization, key: &str, value: &str, issuer: &str) -> ApiResult<Response> {
    let mut url =
        Url::parse(&r.redirect_uri).map_err(|_| ApiError::bad_request("invalid redirect"))?;
    url.query_pairs_mut()
        .append_pair(key, value)
        .append_pair("state", &r.state)
        .append_pair("iss", issuer);
    Ok(no_store((StatusCode::SEE_OTHER, [(header::LOCATION, url.as_str())]).into_response()))
}
async fn consent(
    State(s): State<AppState>,
    h: HeaderMap,
    Form(c): Form<Consent>,
) -> ApiResult<Response> {
    if !same_origin(&s, &h) {
        return Ok(error("invalid_request"));
    }
    let caller = owner(&s, &h)?;
    let Some(payload) = s.db.oauth_take_request(
        &auth::hash_secret(&c.nonce),
        &caller.person_id,
        &caller.secret_hash,
        now(),
    )?
    else {
        return Ok(error("invalid_request"));
    };
    let Ok(r) = serde_json::from_str::<Authorization>(&payload) else {
        return Ok(error("invalid_request"));
    };
    if valid_request(&s, &r)?.is_none() {
        return Ok(error("invalid_request"));
    }
    if c.decision == "deny" {
        return redirect(&r, "error", "access_denied", &s.config.public_url);
    }
    // #647: a setup grant, with no bot, lasting a day.
    if r.scope == SETUP_SCOPE {
        if c.decision != "allow" {
            return Ok(error("invalid_request"));
        }
        let g = crate::db::SetupGrant {
            id: auth::new_id(),
            owner_id: caller.person_id,
            client_id: r.client_id.clone(),
            resource: r.resource.clone(),
            expires_at: now() + 86400,
        };
        let code = format!("{SETUP_CODE}{}", auth::new_feed_token());
        if !s.db.setup_issue_code(
            &g,
            &auth::hash_secret(&code),
            &r.redirect_uri,
            &r.code_challenge,
            now(),
        )? {
            return Ok(error("access_denied"));
        }
        return redirect(&r, "code", &code, &s.config.public_url);
    }
    if c.decision != "allow"
        || !s.db.agents_of(&caller.person_id, now())?.iter().any(|a| a.id == c.agent_id)
    {
        return Ok(error("invalid_request"));
    }
    let g = Grant {
        id: auth::new_id(),
        owner_id: caller.person_id,
        agent_id: c.agent_id,
        client_id: r.client_id.clone(),
        resource: r.resource.clone(),
        // Webhooks only when the client asked for them and the owner kept them ticked.
        scope: if webhook_scope(&r.scope) && c.events == "allow" {
            r.scope.clone()
        } else {
            SCOPE.to_string()
        },
        expires_at: now() + 30 * 86400,
    };
    let code = format!("ah_code_{}", auth::new_feed_token());
    if !s.db.oauth_issue_code(
        &g,
        &auth::hash_secret(&code),
        &r.redirect_uri,
        &r.code_challenge,
        now(),
    )? {
        return Ok(error("access_denied"));
    }
    redirect(&r, "code", &code, &s.config.public_url)
}
async fn token(
    State(s): State<AppState>,
    h: HeaderMap,
    Form(r): Form<TokenRequest>,
) -> ApiResult<Response> {
    if !limit(&s, &h, "token", 60) {
        return Ok(no_store(StatusCode::TOO_MANY_REQUESTS.into_response()));
    }
    if h.contains_key(header::AUTHORIZATION)
        || r.resource != resource(&s)
        || r.scope.as_deref().is_some_and(|scope| !valid_scope(scope))
        || s.db.oauth_client(&r.client_id)?.is_none()
    {
        return Ok(error("invalid_request"));
    }
    // #647: a setup code or refresh token gets setup tokens, from the setup grants only.
    let setup = r.code.starts_with(SETUP_CODE) || r.refresh_token.starts_with(SETUP_REFRESH);
    if setup {
        return setup_token(&s, &r);
    }
    let access = format!("ah_oauth_{}", auth::new_feed_token());
    let refresh = format!("ah_refresh_{}", auth::new_feed_token());
    let pair = TokenPair {
        access_hash: auth::hash_secret(&access),
        refresh_hash: auth::hash_secret(&refresh),
    };
    let at = now();
    let g = match r.grant_type.as_str() {
        "authorization_code" => {
            if !(43..=128).contains(&r.code_verifier.len())
                || !r
                    .code_verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            {
                return Ok(error("invalid_grant"));
            }
            s.db.oauth_exchange(
                &auth::hash_secret(&r.code),
                &r.client_id,
                &r.resource,
                &r.redirect_uri,
                &sign_in::challenge_for(&r.code_verifier),
                &pair,
                at,
            )?
        }
        "refresh_token" => s.db.oauth_refresh(
            &auth::hash_secret(&r.refresh_token),
            &r.client_id,
            &r.resource,
            &pair,
            at,
        )?,
        _ => return Ok(error("unsupported_grant_type")),
    };
    let Some(g) = g else { return Ok(error("invalid_grant")) };
    Ok(no_store(Json(json!({"access_token":access,"token_type":"Bearer","expires_in":(at+3600).min(g.expires_at)-at,"refresh_token":refresh,"scope":g.scope})).into_response()))
}
/// #647: the token endpoint for setup grants, the same exchange and rotation as for bots.
fn setup_token(s: &AppState, r: &TokenRequest) -> ApiResult<Response> {
    let access = format!("{}{}", crate::bot_setup::ACCESS_PREFIX, auth::new_feed_token());
    let refresh = format!("{SETUP_REFRESH}{}", auth::new_feed_token());
    let pair = TokenPair {
        access_hash: auth::hash_secret(&access),
        refresh_hash: auth::hash_secret(&refresh),
    };
    let at = now();
    let g = match r.grant_type.as_str() {
        "authorization_code" => {
            if !(43..=128).contains(&r.code_verifier.len())
                || !r
                    .code_verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            {
                return Ok(error("invalid_grant"));
            }
            s.db.setup_exchange(
                &auth::hash_secret(&r.code),
                &r.client_id,
                &r.resource,
                &r.redirect_uri,
                &sign_in::challenge_for(&r.code_verifier),
                &pair,
                at,
            )?
        }
        "refresh_token" => s.db.setup_refresh(
            &auth::hash_secret(&r.refresh_token),
            &r.client_id,
            &r.resource,
            &pair,
            at,
        )?,
        _ => return Ok(error("unsupported_grant_type")),
    };
    let Some(g) = g else { return Ok(error("invalid_grant")) };
    Ok(no_store(Json(json!({"access_token":access,"token_type":"Bearer","expires_in":(at+3600).min(g.expires_at)-at,"refresh_token":refresh,"scope":SETUP_SCOPE})).into_response()))
}
async fn revoke(
    State(s): State<AppState>,
    h: HeaderMap,
    Form(r): Form<Revocation>,
) -> ApiResult<Response> {
    if !limit(&s, &h, "revoke", 60) {
        return Ok(no_store(StatusCode::TOO_MANY_REQUESTS.into_response()));
    }
    s.db.oauth_revoke_token(&auth::hash_secret(&r.token), &r.client_id)?;
    Ok(no_store(StatusCode::OK.into_response()))
}
async fn connections(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Response> {
    let caller = owner(&s, &h)?;
    let nonce = auth::new_ceremony_id();
    if !s.db.oauth_request(
        &auth::hash_secret(&nonce),
        &caller.person_id,
        &caller.secret_hash,
        "manage",
        now(),
    )? {
        return Ok(error("temporarily_unavailable"));
    }
    let mut body=String::from("<p>Revoking a connection ends its access and refresh tokens. Existing bot tokens are unchanged.</p>");
    for (id, client, bot) in s.db.oauth_grants(&caller.person_id, now())? {
        body.push_str(&format!("<form method=\"post\" action=\"/oauth/connections/revoke\"><p>{} as {}</p><input type=\"hidden\" name=\"nonce\" value=\"{}\"><input type=\"hidden\" name=\"grant_id\" value=\"{}\"><button>Revoke connection</button></form>",escape(&client),escape(&bot),escape(&nonce),escape(&id)));
    }
    Ok(html(body))
}
async fn owner_revoke(
    State(s): State<AppState>,
    h: HeaderMap,
    Form(r): Form<OwnerRevocation>,
) -> ApiResult<Response> {
    if !same_origin(&s, &h) {
        return Ok(error("invalid_request"));
    }
    let c = owner(&s, &h)?;
    if s.db
        .oauth_take_request(&auth::hash_secret(&r.nonce), &c.person_id, &c.secret_hash, now())?
        .as_deref()
        != Some("manage")
    {
        return Ok(error("invalid_request"));
    }
    s.db.oauth_revoke(&r.grant_id, &c.person_id)?;
    Ok(no_store(
        (StatusCode::SEE_OTHER, [(header::LOCATION, "/oauth/connections")]).into_response(),
    ))
}
/// Only MCP and its continuation checks call this. Generic account/REST auth remains unchanged.
pub(crate) fn authenticate_mcp(s: &AppState, hash: &str) -> ApiResult<Option<Caller>> {
    let Some(g) = s.db.oauth_access(hash, &resource(s), now())? else { return Ok(None) };
    let Some(a) = s.db.agent(&g.agent_id, now())? else { return Ok(None) };
    // RC2: an OAuth grant doesn't keep a bot at the recycling plant signed in either.
    if s.db.recycled_at(&a.id)?.is_some() {
        return Ok(None);
    }
    Ok(Some(Caller {
        person_id: g.owner_id,
        agent_id: Some(a.id),
        name: a.name,
        secret_hash: hash.into(),
        is_house: false,
    }))
}
pub(crate) fn revalidate(s: &AppState, c: &Caller) -> bool {
    let valid = s
        .authenticate(&c.secret_hash)
        .ok()
        .flatten()
        .or_else(|| authenticate_mcp(s, &c.secret_hash).ok().flatten());
    valid.is_some_and(|live| live.agent_id == c.agent_id && live.person_id == c.person_id)
}
