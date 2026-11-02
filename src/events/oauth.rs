//! OAuth subscriptions use the grant, never an expiring access token, as their principal.
use super::*;
use crate::db::oauth_events::OAuthSubscription;
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{OwnedMutexGuard, Semaphore};

const MAX_OUTBOUND: usize = 8;

/// Identity locks live only while an operation holds or waits for them. The registry mutex is
/// never held across an await; a callback can block only its own subscription identity.
pub(crate) struct Runtime {
    locks: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
    outbound: Semaphore,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { locks: Mutex::new(HashMap::new()), outbound: Semaphore::new(MAX_OUTBOUND) }
    }
}

impl Runtime {
    fn identity(&self, key: String) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    async fn subscription(&self, id: &str) -> OwnedMutexGuard<()> {
        self.identity(format!("subscription:{id}")).lock_owned().await
    }

    fn try_subscription(&self, id: &str) -> Option<OwnedMutexGuard<()>> {
        self.identity(format!("subscription:{id}")).try_lock_owned().ok()
    }

    async fn grant_commit(&self, grant: &str) -> OwnedMutexGuard<()> {
        self.identity(format!("grant:{grant}")).lock_owned().await
    }
}

fn check_limit(state: &AppState, grant: &str, id: &str) -> Result<(), Fail> {
    let at = now();
    if state.db.oauth_subscription(id).map_err(internal)?.is_none_or(|s| s.expires_at <= at)
        && state.db.oauth_subscription_count(grant, at).map_err(internal)? >= MAX_SUBSCRIPTIONS
    {
        return Err(Fail {
            code: -32025,
            message: "ResourceExhausted",
            data: json!({"limit":"subscriptions"}),
        });
    }
    Ok(())
}

fn denied() -> Fail {
    Fail { code: -32024, message: "Forbidden", data: json!({}) }
}

fn live(state: &AppState, caller: &Caller) -> Result<crate::oauth::Grant, Fail> {
    let g = state
        .db
        .oauth_access(&caller.secret_hash, &format!("{}/mcp", state.config.public_url), now())
        .map_err(internal)?
        .ok_or_else(denied)?;
    if caller.agent_id.as_deref() != Some(&g.agent_id) || caller.person_id != g.owner_id {
        return Err(denied());
    }
    if !crate::oauth::webhook_scope(&g.scope) {
        // Refresh can't add it, so say what would: a new connection that asks for both.
        return Err(Fail {
            code: -32024,
            message: "Forbidden",
            data: json!({
                "reason": "this connection was not granted mention webhooks; connect again asking for mcp:bot mcp:events and allow them",
                "required_scope": "mcp:bot mcp:events",
            }),
        });
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
    let _guard = state.oauth_events.subscription(&id).await;
    // Waiting for an in-flight delivery must not extend the credential's authorization.
    let (g, _, _, _) = identity(state, caller, params)?;
    check_limit(state, &g.id, &id)?;
    let existing = state.db.oauth_subscription(&id).map_err(internal)?;
    let existing = existing.as_ref();
    let at = now();
    // Cache only the same principal, callback and key for five minutes. A replacement key must
    // prove possession again; another grant's verification never authorizes this one.
    let verified = existing.is_some_and(|s| s.secret == secret && s.verified_at + GRACE_SECS > at);
    if !verified {
        let _permit = state.oauth_events.outbound.acquire().await.map_err(internal)?;
        identity(state, caller, params)?;
        verify_endpoint_with_id(state, &url, secret, Some(&id)).await.map_err(|mut e| {
            if e.message == "CallbackEndpointError" {
                e.code = -32015;
            }
            e
        })?;
    }
    // Different identities can verify concurrently. Reserve capacity only while committing;
    // this grant lock never spans callback work and closes the concurrent-create count race.
    let _commit = state.oauth_events.grant_commit(&g.id).await;
    // A grant can be revoked, or lose the room, while the callback is being verified.
    let (g, _, _, _) = identity(state, caller, params)?;
    check_limit(state, &g.id, &id)?;
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
            .map(|_| now() + GRACE_SECS)
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
    let _guard = state.oauth_events.subscription(&id).await;
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

/// One bounded pass. Each identity is reloaded under its own lock; overlapping passes skip
/// busy identities rather than queue stale snapshots. Outbound work shares a fixed global bound.
pub async fn deliver(state: &AppState) -> Result<(), rusqlite::Error> {
    let results =
        stream::iter(state.db.oauth_subscriptions()?.into_iter().map(|snapshot| async move {
            let Some(_guard) = state.oauth_events.try_subscription(&snapshot.id) else {
                return Ok(());
            };
            let Some(sub) = state.db.oauth_subscription(&snapshot.id)? else {
                return Ok(());
            };
            deliver_one(state, sub).await
        }))
        .buffer_unordered(MAX_OUTBOUND)
        .collect::<Vec<_>>()
        .await;
    results.into_iter().collect()
}

async fn deliver_one(state: &AppState, mut sub: OAuthSubscription) -> Result<(), rusqlite::Error> {
    if sub.expires_at <= now() || permitted(state, &sub).is_none() {
        state.db.oauth_delete_subscription(&sub.id, &sub.grant_id)?;
        return Ok(());
    }
    if !sub.active || sub.next_attempt > now() {
        return Ok(());
    }
    let Some(g) = permitted(state, &sub) else {
        return Ok(());
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
        return Ok(());
    };
    if !visible(state, &g.agent_id, row)? {
        sub.cursor = row.id;
        sub.attempts = 0;
        state.db.oauth_subscription_progress(&sub)?;
        return Ok(());
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
        return Ok(());
    }
    let Ok(_permit) = state.oauth_events.outbound.acquire().await else {
        return Ok(());
    };
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
        return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn queued_subscription() -> (AppState, Caller, String) {
        let at = now();
        let db = db::Db::open(":memory:").unwrap();
        db.create_person("owner", "Owner", "key", "feed", at).unwrap();
        db.create_agent("bot", "owner", "Bot", "legacy", at).unwrap();
        db.create_hangout("room", "Room", "invite", "owner", at).unwrap();
        db.add_agent_to_room("room", "bot").unwrap();
        db.oauth_register("client", "Client", "[]", at).unwrap();
        let g = crate::oauth::Grant {
            id: "grant".into(),
            owner_id: "owner".into(),
            agent_id: "bot".into(),
            client_id: "client".into(),
            resource: "https://server.example/mcp".into(),
            scope: "mcp:bot mcp:events".into(),
            expires_at: at + 300,
        };
        db.oauth_issue_code(&g, "code", "redirect", "challenge", at).unwrap();
        db.oauth_exchange(
            "code",
            "client",
            &g.resource,
            "redirect",
            "challenge",
            &crate::oauth::TokenPair {
                access_hash: "access".into(),
                refresh_hash: "refresh".into(),
            },
            at,
        )
        .unwrap()
        .unwrap();
        let arguments = json!({"hangout_id":"room"}).to_string();
        let url = "http://127.0.0.1:9/callback";
        let id = format!("oauth_{}", subscription_id(&g.id, url, "mention", &arguments));
        db.oauth_save_subscription(&OAuthSubscription {
            id: id.clone(),
            grant_id: g.id,
            arguments,
            url: url.into(),
            secret: format!("whsec_{}", b64_encode(&[b'a'; 32])),
            previous_secret: None,
            previous_until: None,
            expires_at: at + 300,
            verified_at: at,
            cursor: 0,
            active: true,
            attempts: 0,
            next_attempt: 0,
        })
        .unwrap();
        db.insert_bot_event("bot", "mention", Some("room"), "pending", "{}", at, 0).unwrap();
        let state = AppState::new(
            db,
            crate::Config {
                public_url: "https://server.example".into(),
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
            },
        );
        let caller = crate::oauth::authenticate_mcp(&state, "access").unwrap().unwrap();
        (state, caller, id)
    }

    async fn wait_for_identity(state: &AppState, id: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while state.oauth_events.try_subscription(id).is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn queued_verification_rechecks_revocation_without_replacing_the_signing_key() {
        let (state, caller, id) = queued_subscription();
        let permits = state.oauth_events.outbound.acquire_many(MAX_OUTBOUND as u32).await.unwrap();
        let worker = state.clone();
        let pending = tokio::spawn(async move {
            subscribe(&worker,&caller,&json!({"name":"mention","arguments":{"hangout_id":"room"},"delivery":{"mode":"webhook","url":"http://127.0.0.1:9/callback","secret":format!("whsec_{}",b64_encode(&[b'b';32]))}})).await
        });
        wait_for_identity(&state, &id).await;
        state.db.oauth_revoke("grant", "owner").unwrap();
        drop(permits);
        assert_eq!(pending.await.unwrap().unwrap_err().code, -32024);
        let sub = state.db.oauth_subscription(&id).unwrap().unwrap();
        assert_eq!(sub.secret, format!("whsec_{}", b64_encode(&[b'a'; 32])));
        assert_eq!(sub.cursor, 0);
    }

    #[tokio::test]
    async fn queued_delivery_rechecks_revocation_and_room_removal_before_connecting() {
        for revoke in [true, false] {
            let (state, _, id) = queued_subscription();
            let permits =
                state.oauth_events.outbound.acquire_many(MAX_OUTBOUND as u32).await.unwrap();
            let worker = state.clone();
            let pending = tokio::spawn(async move { deliver(&worker).await.unwrap() });
            wait_for_identity(&state, &id).await;
            if revoke {
                state.db.oauth_revoke("grant", "owner").unwrap();
            } else {
                state.db.remove_agent_from_room("room", "bot").unwrap();
            }
            drop(permits);
            pending.await.unwrap();
            let sub = state.db.oauth_subscription(&id).unwrap().unwrap();
            assert_eq!(
                (sub.cursor, sub.attempts),
                (0, 0),
                "no connection attempt after access loss"
            );
            deliver(&state).await.unwrap();
            assert!(state.db.oauth_subscription(&id).unwrap().is_none());
        }
    }
}
