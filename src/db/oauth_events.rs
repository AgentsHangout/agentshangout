//! E32-E34: grant identity and durable, ordered delivery progress.
use super::*;
use crate::oauth::Grant;

#[derive(Clone)]
pub(crate) struct OAuthSubscription {
    pub id: String,
    pub grant_id: String,
    pub arguments: String,
    pub url: String,
    pub secret: String,
    pub previous_secret: Option<String>,
    pub previous_until: Option<i64>,
    pub expires_at: i64,
    pub verified_at: i64,
    pub cursor: i64,
    pub active: bool,
    pub attempts: i64,
    pub next_attempt: i64,
}

const SUBSCRIPTION_COLS: &str = "id,grant_id,arguments,url,secret,previous_secret,previous_until,expires_at,verified_at,cursor,active,attempts,next_attempt";

fn subscription(row: &Row<'_>) -> rusqlite::Result<OAuthSubscription> {
    Ok(OAuthSubscription {
        id: row.get(0)?,
        grant_id: row.get(1)?,
        arguments: row.get(2)?,
        url: row.get(3)?,
        secret: row.get(4)?,
        previous_secret: row.get(5)?,
        previous_until: row.get(6)?,
        expires_at: row.get(7)?,
        verified_at: row.get(8)?,
        cursor: row.get(9)?,
        active: row.get(10)?,
        attempts: row.get(11)?,
        next_attempt: row.get(12)?,
    })
}

impl Db {
    pub(crate) fn oauth_subscription(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<OAuthSubscription>> {
        self.conn()
            .query_row(
                &format!("SELECT {SUBSCRIPTION_COLS} FROM oauth_event_subscriptions WHERE id=?"),
                [id],
                subscription,
            )
            .optional()
    }
    pub(crate) fn oauth_subscription_count(&self, grant: &str, at: i64) -> rusqlite::Result<i64> {
        self.conn().query_row(
            "SELECT COUNT(*) FROM oauth_event_subscriptions WHERE grant_id=?1 AND expires_at>?2",
            params![grant, at],
            |r| r.get(0),
        )
    }
    pub(crate) fn oauth_history_truncated(
        &self,
        agent: &str,
        cursor: i64,
    ) -> rusqlite::Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT discarded_through FROM oauth_event_retention WHERE agent_id=?",
                [agent],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some_and(|discarded| cursor < discarded))
    }
    pub(crate) fn oauth_live_grant(&self, id: &str, at: i64) -> rusqlite::Result<Option<Grant>> {
        self.conn()
            .query_row(super::oauth::LIVE_GRANT, params![id, at], super::oauth::grant)
            .optional()
    }

    pub(crate) fn oauth_subscriptions(&self) -> rusqlite::Result<Vec<OAuthSubscription>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {SUBSCRIPTION_COLS} FROM oauth_event_subscriptions"))?;
        let rows = stmt.query_map([], subscription)?;
        rows.collect()
    }

    pub(crate) fn oauth_save_subscription(&self, s: &OAuthSubscription) -> rusqlite::Result<()> {
        self.conn().execute("INSERT INTO oauth_event_subscriptions(id,grant_id,arguments,url,secret,previous_secret,previous_until,expires_at,verified_at,cursor,active,attempts,next_attempt) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) ON CONFLICT(id) DO UPDATE SET secret=excluded.secret,previous_secret=excluded.previous_secret,previous_until=excluded.previous_until,expires_at=excluded.expires_at,verified_at=excluded.verified_at,active=1,attempts=0,next_attempt=0", params![s.id,s.grant_id,s.arguments,s.url,s.secret,s.previous_secret,s.previous_until,s.expires_at,s.verified_at,s.cursor,s.active,s.attempts,s.next_attempt])?;
        Ok(())
    }

    pub(crate) fn oauth_delete_subscription(&self, id: &str, grant: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "DELETE FROM oauth_event_subscriptions WHERE id=?1 AND grant_id=?2",
            params![id, grant],
        )?;
        Ok(())
    }

    pub(crate) fn oauth_subscription_progress(
        &self,
        s: &OAuthSubscription,
    ) -> rusqlite::Result<()> {
        self.conn().execute("UPDATE oauth_event_subscriptions SET cursor=?2,active=?3,attempts=?4,next_attempt=?5 WHERE id=?1", params![s.id,s.cursor,s.active,s.attempts,s.next_attempt])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{now, AppState, Config};

    fn setup(db: Db) -> AppState {
        let at = now();
        db.create_person("owner", "Owner", "key", "feed", at).unwrap();
        db.create_person("other", "Other", "other-key", "other-feed", at).unwrap();
        db.create_agent("bot", "owner", "Bot", "legacy", at).unwrap();
        db.oauth_register("client", "Client", "[]", at).unwrap();
        let g = Grant {
            id: "grant".into(),
            owner_id: "owner".into(),
            agent_id: "bot".into(),
            client_id: "client".into(),
            resource: "https://server.example/mcp".into(),
            scope: "mcp:bot mcp:events".into(),
            expires_at: at + 300,
        };
        db.oauth_issue_code(&g, "code", "redirect", "challenge", at).unwrap();
        db.oauth_save_subscription(&OAuthSubscription {
            id: "sub".into(),
            grant_id: g.id,
            arguments: "{}".into(),
            url: "https://callback.example/events".into(),
            secret: "whsec_YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=".into(),
            previous_secret: None,
            previous_until: None,
            expires_at: at + 300,
            verified_at: at,
            cursor: 7,
            active: true,
            attempts: 1,
            next_attempt: at + 10,
        })
        .unwrap();
        AppState::new(
            db,
            Config {
                public_url: "https://server.example".into(),
                create_code: None,
                allow_private_webhooks: true,
                keeper_id: None,
                god_ids: vec![],
            },
        )
    }

    #[tokio::test]
    async fn expired_revoked_reassigned_deleted_and_wrong_scope_grants_cancel_pending_delivery() {
        for change in [
            "UPDATE mcp_oauth_grants SET expires_at=0",
            "UPDATE mcp_oauth_grants SET revoked=1",
            "UPDATE agents SET owner_id='other'",
            "DELETE FROM agents",
            "UPDATE mcp_oauth_grants SET scope='wrong'",
            "UPDATE mcp_oauth_grants SET resource='https://other.example/mcp'",
            "UPDATE oauth_event_subscriptions SET expires_at=0",
        ] {
            let state = setup(Db::open(":memory:").unwrap());
            state.db.conn().execute(change, []).unwrap();
            crate::events::oauth::deliver(&state).await.unwrap();
            assert!(state.db.oauth_subscriptions().unwrap().is_empty(), "{change}");
        }
    }

    #[test]
    fn restart_keeps_identity_cursor_retry_and_signing_rotation_and_renewal_cannot_skip() {
        let path = std::env::temp_dir().join(format!("oauth-events-{}.db", crate::auth::new_id()));
        let config;
        {
            let state = setup(Db::open(path.to_str().unwrap()).unwrap());
            config = state.config.clone();
            let mut sub = state.db.oauth_subscriptions().unwrap().remove(0);
            sub.previous_secret = Some("old-key".into());
            sub.previous_until = Some(now() + 60);
            sub.cursor = 999;
            state.db.oauth_save_subscription(&sub).unwrap();
            assert_eq!(state.db.oauth_subscriptions().unwrap()[0].cursor, 7);
            sub.cursor = 42;
            sub.attempts = 2;
            state.db.oauth_subscription_progress(&sub).unwrap();
        }
        {
            let state = AppState::new(Db::open(path.to_str().unwrap()).unwrap(), config);
            let sub = state.db.oauth_subscriptions().unwrap().remove(0);
            assert_eq!(sub.id, "sub");
            assert_eq!(sub.grant_id, "grant");
            assert_eq!(sub.cursor, 42);
            assert_eq!(sub.attempts, 2);
            assert_eq!(sub.previous_secret.as_deref(), Some("old-key"));
            assert!(sub.next_attempt > now());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn discarded_history_is_reported_after_rows_are_deleted() {
        let state = setup(Db::open(":memory:").unwrap());
        let id =
            state.db.insert_bot_event("bot", "mention", None, "old", "{}", 1, 0).unwrap().unwrap();
        state.db.insert_bot_event("bot", "mention", None, "new", "{}", now(), 2).unwrap();
        assert!(state.db.oauth_history_truncated("bot", 0).unwrap());
        assert!(!state.db.oauth_history_truncated("bot", id).unwrap());
        assert!(!state.db.oauth_history_truncated("other-bot", 0).unwrap());
    }

    #[tokio::test]
    async fn subscription_lifetime_is_capped_by_grant_and_retries_keep_the_same_event() {
        use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
        use serde_json::{json, Value};
        let state = setup(Db::open(":memory:").unwrap());
        state.db.conn().execute("UPDATE mcp_oauth_grants SET expires_at=?", [now() + 60]).unwrap();
        let pair = crate::oauth::TokenPair {
            access_hash: "access".into(),
            refresh_hash: "refresh".into(),
        };
        let g = state
            .db
            .oauth_exchange(
                "code",
                "client",
                "https://server.example/mcp",
                "redirect",
                "challenge",
                &pair,
                now(),
            )
            .unwrap()
            .unwrap();
        let caller = crate::oauth::authenticate_mcp(&state, "access").unwrap().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let attempts = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let router = Router::new()
            .route(
                "/",
                post(
                    |State(attempts): State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
                     Json(body): Json<Value>| async move {
                        if body["type"] == "verification" {
                            return (StatusCode::OK, Json(json!({"challenge":body["challenge"]})));
                        }
                        attempts.lock().unwrap().push(body);
                        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})))
                    },
                ),
            )
            .with_state(attempts.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let params = json!({"name":"mention","delivery":{"mode":"webhook","url":url,"secret":"whsec_YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE="},"ttlMs":null});
        let result = crate::events::oauth::subscribe(&state, &caller, &params).await.unwrap();
        assert_eq!(result["refreshBefore"], crate::ops::iso(g.expires_at));
        let id = result["id"].as_str().unwrap();
        // Use an existing accessible room so retries carry real room data.
        state.db.conn().execute("INSERT INTO hangouts(id,name,invite_code,created_by,created_at) VALUES('room','Room','invite','owner',?)",[now()]).unwrap();
        state.db.add_agent_to_room("room", "bot").unwrap();
        let event_id = state
            .db
            .insert_bot_event("bot", "mention", Some("room"), "retry", "{}", now(), 0)
            .unwrap()
            .unwrap();
        for expected in 1..=3 {
            state
                .db
                .conn()
                .execute("UPDATE oauth_event_subscriptions SET next_attempt=0 WHERE id=?", [id])
                .unwrap();
            crate::events::oauth::deliver(&state).await.unwrap();
            assert_eq!(attempts.lock().unwrap().len(), expected);
        }
        crate::events::oauth::deliver(&state).await.unwrap();
        let delivered = attempts.lock().unwrap();
        assert_eq!(delivered.len(), 3);
        assert!(delivered.iter().all(|e| e["eventId"] == format!("evt_{event_id}")));
        let sub = state.db.oauth_subscriptions().unwrap().into_iter().find(|s| s.id == id).unwrap();
        assert!(!sub.active);
        assert_eq!(sub.cursor, 0);
    }
}
