//! OAuth subscriptions use the grant, never an expiring access token, as their principal.
use super::*;
use crate::db::oauth_events::OAuthSubscription;

fn denied() -> Fail {
    Fail { code: -32024, message: "Forbidden", data: json!({}) }
}

fn live(state: &AppState, caller: &Caller) -> Result<crate::oauth::Grant, Fail> {
    let g = state
        .db
        .oauth_access(&caller.secret_hash, &format!("{}/mcp", state.config.public_url), now())
        .map_err(internal)?
        .ok_or_else(denied)?;
    if !crate::oauth::webhook_scope(&g.scope)
        || caller.agent_id.as_deref() != Some(&g.agent_id)
        || caller.person_id != g.owner_id
    {
        return Err(denied());
    }
    Ok(g)
}

pub(crate) fn catalog(state: &AppState, caller: &Caller) -> Result<Value, Fail> {
    let webhooks = live(state, caller).is_ok();
    let mut catalog = list(caller)?;
    for event in catalog["events"].as_array_mut().unwrap() {
        if !webhooks || event["name"] != "mention" {
            event["delivery"] = json!(["poll", "push"]);
        }
        event["inputSchema"]["additionalProperties"] = json!(false);
        event["inputSchema"]["properties"]["hangout_id"]["minLength"] = json!(1);
    }
    Ok(catalog)
}

fn identity(
    state: &AppState,
    caller: &Caller,
    params: &Value,
) -> Result<(crate::oauth::Grant, String, String, String), Fail> {
    bot(caller)?;
    let g = live(state, caller)?;
    if known_name(params)? != "mention" {
        return Err(invalid("OAuth webhooks support mention events"));
    }
    if params.get("arguments").is_some_and(|args| !args.is_object()) {
        return Err(invalid("arguments must be an object"));
    }
    let (arguments, room) = canonical(params)?;
    if let Some(room) = room {
        in_room(state, caller, &room)?;
    }
    if params["delivery"]["mode"] != "webhook" {
        return Err(Fail {
            code: -32026,
            message: "Unsupported",
            data: json!({"reason":"events/subscribe delivers by webhook"}),
        });
    }
    let url = delivery_url(state, params)?;
    if reqwest::Url::parse(&url).map_err(|_| invalid("invalid callback URL"))?.fragment().is_some()
    {
        return Err(invalid("callback URL cannot have a fragment"));
    }
    let id = format!("oauth_{}", subscription_id(&g.id, &url, "mention", &arguments));
    Ok((g, id, arguments, url))
}

pub(crate) async fn subscribe(
    state: &AppState,
    caller: &Caller,
    params: &Value,
) -> Result<Value, Fail> {
    count_write(state, caller)?;
    let (_, id, arguments, url) = identity(state, caller, params)?;
    let secret = params["delivery"]["secret"]
        .as_str()
        .ok_or_else(|| invalid("delivery.secret is required"))?;
    let key = decode_whsec(secret)?;
    if format!("whsec_{}", b64_encode(&key)) != secret {
        return Err(invalid("secret must use canonical base64"));
    }
    let secs = grant_secs(params.get("ttlMs"))?;
    let requested_cursor =
        params.get("cursor").filter(|c| !c.is_null()).map(parse_cursor).transpose()?;
    // Serialize renewals against delivery progress; never overwrite a newer acknowledged cursor.
    let _guard = state.oauth_event_delivery.lock().await;
    // Waiting for an in-flight delivery must not extend the credential's authorization.
    let (g, _, _, _) = identity(state, caller, params)?;
    let subs = state.db.oauth_subscriptions().map_err(internal)?;
    let existing = subs.iter().find(|s| s.id == id);
    if existing.is_none()
        && subs.iter().filter(|s| s.grant_id == g.id && s.expires_at > now()).count()
            >= MAX_SUBSCRIPTIONS as usize
    {
        return Err(Fail {
            code: -32025,
            message: "ResourceExhausted",
            data: json!({"limit":"subscriptions"}),
        });
    }
    let at = now();
    // Cache only the same principal, callback and key for five minutes. A replacement key must
    // prove possession again; another grant's verification never authorizes this one.
    let verified = existing.is_some_and(|s| s.secret == secret && s.verified_at + GRACE_SECS > at);
    if !verified {
        verify_endpoint_with_id(state, &url, secret, Some(&id)).await.map_err(|mut e| {
            if e.message == "CallbackEndpointError" {
                e.code = -32015;
            }
            e
        })?;
    }
    // A grant can be revoked, or lose the room, while the callback is being verified.
    let (g, _, _, _) = identity(state, caller, params)?;
    let head = state.db.bot_event_head(&g.agent_id).map_err(internal)?;
    let cursor = existing.map(|s| s.cursor).unwrap_or(requested_cursor.unwrap_or(head));
    let changed = existing.filter(|s| s.secret != secret);
    let sub = OAuthSubscription {
        id: id.clone(),
        grant_id: g.id,
        arguments,
        url,
        secret: secret.into(),
        previous_secret: changed
            .map(|s| s.secret.clone())
            .or_else(|| existing.and_then(|s| s.previous_secret.clone())),
        previous_until: changed
            .map(|_| at + GRACE_SECS)
            .or_else(|| existing.and_then(|s| s.previous_until)),
        expires_at: (now() + secs).min(g.expires_at),
        verified_at: if verified { existing.unwrap().verified_at } else { now() },
        cursor,
        active: true,
        attempts: 0,
        next_attempt: 0,
    };
    state.db.oauth_save_subscription(&sub).map_err(internal)?;
    Ok(json!({"id":id,"refreshBefore":iso(sub.expires_at),"cursor":cursor.to_string(),
        "truncated":state.db.oauth_history_truncated(&g.agent_id,cursor).map_err(internal)? || state.db.bot_events_skipped(&g.agent_id,cursor,Some("mention"),None,now()-RETENTION_SECS,false).map_err(internal)?}))
}

pub(crate) async fn unsubscribe(
    state: &AppState,
    caller: &Caller,
    params: &Value,
) -> Result<Value, Fail> {
    count_write(state, caller)?;
    let (g, id, _, _) = identity(state, caller, params)?;
    let _guard = state.oauth_event_delivery.lock().await;
    live(state, caller)?;
    state.db.oauth_delete_subscription(&id, &g.id).map_err(internal)?;
    Ok(json!({}))
}

fn permitted(state: &AppState, sub: &OAuthSubscription) -> Option<crate::oauth::Grant> {
    let g = state.db.oauth_live_grant(&sub.grant_id, now()).ok().flatten()?;
    if !crate::oauth::webhook_scope(&g.scope)
        || g.resource != format!("{}/mcp", state.config.public_url)
    {
        return None;
    }
    let args: Value = serde_json::from_str(&sub.arguments).ok()?;
    if let Some(room) = args["hangout_id"].as_str() {
        if !state.db.agent_in_room(room, &g.agent_id, now()).ok()? {
            return None;
        }
    }
    Some(g)
}

/// One bounded pass; persisted attempt times and cursors survive process restarts. One process
/// owns this SQLite database. The shared mutex prevents overlapping passes and renewal races.
pub async fn deliver(state: &AppState) -> Result<(), rusqlite::Error> {
    let _guard = state.oauth_event_delivery.lock().await;
    for mut sub in state.db.oauth_subscriptions()? {
        if sub.expires_at <= now() || permitted(state, &sub).is_none() {
            state.db.oauth_delete_subscription(&sub.id, &sub.grant_id)?;
            continue;
        }
        if !sub.active || sub.next_attempt > now() {
            continue;
        }
        let Some(g) = permitted(state, &sub) else {
            continue;
        };
        let args: Value = serde_json::from_str(&sub.arguments).unwrap_or(json!({}));
        let rows = state.db.list_bot_events(
            &g.agent_id,
            sub.cursor,
            Some("mention"),
            args["hangout_id"].as_str(),
            now() - RETENTION_SECS,
            false,
            1,
        )?;
        let Some(row) = rows.first() else {
            continue;
        };
        if !visible(state, &g.agent_id, row)? {
            sub.cursor = row.id;
            sub.attempts = 0;
            state.db.oauth_subscription_progress(&sub)?;
            continue;
        }
        let mut event = occurrence(row);
        // Host-facing events carry application data, never embedded behavioral instructions.
        if let Some(data) = event["data"].as_object_mut() {
            data.remove("what_to_do");
            data.remove("trust");
        }
        let body = event.to_string();
        if body.len() > 262_144 {
            sub.active = false;
            state.db.oauth_subscription_progress(&sub)?;
            continue;
        }
        let at = now();
        let stamp = at.to_string();
        let event_id = format!("evt_{}", row.id);
        let signature = sign_header(
            &sub.secret,
            sub.previous_secret.as_deref(),
            sub.previous_until,
            &event_id,
            at,
            &body,
        );
        // Recheck immediately before every attempt, including a retry after restart.
        if permitted(state, &sub).is_none() || !visible(state, &g.agent_id, row)? {
            continue;
        }
        let posted = notify::post_body_checked(
            state,
            &sub.url,
            &[
                ("webhook-id", &event_id),
                ("webhook-timestamp", &stamp),
                ("webhook-signature", &signature),
                ("X-MCP-Subscription-Id", &sub.id),
            ],
            &body,
            || {
                sub.expires_at > now()
                    && permitted(state, &sub).is_some()
                    && visible(state, &g.agent_id, row).unwrap_or(false)
            },
        )
        .await;
        if posted.accepted() {
            sub.cursor = row.id;
            sub.attempts = 0;
            sub.next_attempt = 0;
        } else {
            sub.attempts += 1;
            sub.active = sub.attempts < 3 && !matches!(posted.status, Some(410 | 413));
            sub.next_attempt = now() + if sub.attempts == 1 { 2 } else { 10 };
        }
        state.db.oauth_subscription_progress(&sub)?;
    }
    Ok(())
}

pub async fn run(state: AppState) {
    let mut ticks = tokio::time::interval(Duration::from_secs(1));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        if let Err(error) = deliver(&state).await {
            tracing::error!(%error,"OAuth event delivery storage failed");
        }
    }
}
