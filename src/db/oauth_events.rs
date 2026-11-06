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
    fn oauth_seed_health(&self, s: &OAuthSubscription) -> rusqlite::Result<()> {
        let args: serde_json::Value = serde_json::from_str(&s.arguments).unwrap_or_default();
        self.conn().execute("INSERT OR IGNORE INTO oauth_event_health(subscription_id,grant_id,owner_id,agent_id,client_name,scope,grant_expires_at,hangout_id,expires_at,last_renewed_at,cursor,state) SELECT ?1,g.id,g.owner_id,g.agent_id,c.name,g.scope,g.expires_at,?2,?3,0,?4,?5 FROM mcp_oauth_grants g JOIN mcp_oauth_clients c ON c.id=g.client_id WHERE g.id=?6",params![s.id,args["hangout_id"].as_str(),s.expires_at,s.cursor,if s.active{"active"}else{"suspended"},s.grant_id])?;
        Ok(())
    }
    pub(crate) fn oauth_health_cursor(&self, id: &str) -> rusqlite::Result<Option<i64>> {
        self.conn()
            .query_row("SELECT cursor FROM oauth_event_health WHERE subscription_id=? AND state!='stopped'", [id], |r| {
                r.get(0)
            })
            .optional()
    }

    pub(crate) fn oauth_record_outcome(
        &self,
        s: &OAuthSubscription,
        reason: &str,
        status: Option<u16>,
    ) -> rusqlite::Result<()> {
        self.oauth_seed_health(s)?;
        self.conn().execute("UPDATE oauth_event_health SET cursor=?2,state=?3,last_reason=?4,last_at=?5,http_status=?6 WHERE subscription_id=?1",params![s.id,s.cursor,if s.active {"active"}else{"suspended"},reason,crate::now(),status])?;
        Ok(())
    }

    pub(crate) fn oauth_retire_subscription(
        &self,
        s: &OAuthSubscription,
        reason: &str,
    ) -> rusqlite::Result<()> {
        self.oauth_seed_health(s)?;
        self.conn().execute(
            "UPDATE oauth_event_health SET cursor=?2,state=?3 WHERE subscription_id=?1",
            params![s.id, s.cursor, reason],
        )?;
        self.oauth_delete_subscription(&s.id, &s.grant_id)
    }

    pub(crate) fn oauth_monitoring(
        &self,
        owner: &str,
        agent: &str,
        at: i64,
    ) -> rusqlite::Result<serde_json::Value> {
        use serde_json::{json, Value};
        let mut connections: Vec<Value>;
        {
            let conn = self.conn();
            // Existing subscriptions have no recorded renewal time; report it as unknown.
            conn.execute("INSERT OR IGNORE INTO oauth_event_health(subscription_id,grant_id,owner_id,agent_id,client_name,scope,grant_expires_at,hangout_id,expires_at,last_renewed_at,cursor,state) SELECT s.id,g.id,g.owner_id,g.agent_id,c.name,g.scope,g.expires_at,json_extract(s.arguments,'$.hangout_id'),s.expires_at,0,s.cursor,CASE WHEN s.active=1 THEN 'active' ELSE 'suspended' END FROM oauth_event_subscriptions s JOIN mcp_oauth_grants g ON g.id=s.grant_id JOIN mcp_oauth_clients c ON c.id=g.client_id WHERE g.owner_id=?1 AND g.agent_id=?2",params![owner,agent])?;
            let mut stmt=conn.prepare("SELECT g.id,c.name,g.scope,g.expires_at,g.revoked FROM mcp_oauth_grants g JOIN mcp_oauth_clients c ON c.id=g.client_id WHERE g.owner_id=?1 AND g.agent_id=?2 ORDER BY g.created_at DESC")?;
            connections=stmt.query_map(params![owner,agent],|r|Ok(json!({"id":r.get::<_,String>(0)?,"client_name":r.get::<_,String>(1)?,"scope":r.get::<_,String>(2)?,"expires_at":crate::ops::iso(r.get(3)?),"status":if r.get::<_,bool>(4)? {"revoked"}else if r.get::<_,i64>(3)?<=at {"expired"}else{"active"},"subscriptions":[]})))?.collect::<Result<_,_>>()?;
            let mut stmt=conn.prepare("SELECT h.subscription_id,h.grant_id,h.client_name,h.scope,h.grant_expires_at,h.hangout_id,h.expires_at,h.last_renewed_at,COALESCE(s.cursor,h.cursor),h.state,h.last_reason,h.last_at,h.http_status,s.active FROM oauth_event_health h LEFT JOIN oauth_event_subscriptions s ON s.id=h.subscription_id WHERE h.owner_id=?1 AND h.agent_id=?2 ORDER BY h.last_renewed_at DESC")?;
            let rows=stmt.query_map(params![owner,agent],|r| {
                let expires:i64=r.get(6)?;
                let renewed:i64=r.get(7)?;
                let state:String=r.get(9)?;
                let active:Option<bool>=r.get(13)?;
                let reason:Option<String>=r.get(10)?;
                let last_at:Option<i64>=r.get(11)?;
                let status:Option<u16>=r.get(12)?;
                Ok((r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,json!({"id":r.get::<_,String>(0)?,"hangout_id":r.get::<_,Option<String>>(5)?,"expires_at":crate::ops::iso(expires),"refreshBefore":crate::ops::iso(expires),"last_renewed_at":(renewed>0).then(||crate::ops::iso(renewed)),"cursor":r.get::<_,i64>(8)?.to_string(),"status":if state=="stopped" {"stopped"}else if expires<=at {"expired"}else if active==Some(false) {"suspended"}else{&state},"last_delivery":reason.map(|reason|json!({"reason":reason,"at":last_at.map(crate::ops::iso),"http_status":status}))})))
            })?.collect::<Result<Vec<_>,_>>()?;
            for (grant, client, scope, expires, mut sub) in rows {
                if !connections.iter().any(|g| g["id"] == grant) {
                    connections.push(json!({"id":grant,"client_name":client,"scope":scope,"expires_at":crate::ops::iso(expires),"status":if expires<=at {"expired"}else{"revoked"},"subscriptions":[]}));
                }
                let g = connections.iter_mut().find(|g| g["id"] == grant).unwrap();
                if g["status"] != "active" {
                    sub["status"] = g["status"].clone();
                }
                g["subscriptions"].as_array_mut().unwrap().push(sub);
            }
        }
        for g in &mut connections {
            for s in g["subscriptions"].as_array_mut().unwrap() {
                if s["status"] == "active" {
                    if let Some(room) = s["hangout_id"].as_str() {
                        if !self.agent_in_room(room, agent, at)? {
                            s["status"] = json!("room_access_lost");
                        }
                    }
                }
            }
        }
        // E41: only rooms the owner may see are named; the rest are only counted.
        let (mut eligible, mut unseen) = (Vec::new(), 0);
        for room in self.hangouts_for_agent(agent, at)? {
            if !self.owner_sees_room(owner, &room.id)? {
                unseen += 1;
                continue;
            }
            let covered = connections.iter().any(|g| {
                g["subscriptions"].as_array().unwrap().iter().any(|s| {
                    s["status"] == "active"
                        && (s["hangout_id"].is_null() || s["hangout_id"] == room.id)
                })
            });
            eligible.push(json!({"id":room.id,"name":room.name,"covered":covered}));
        }
        let latest:Option<(i64,i64,Option<String>)>=self.conn().query_row(
            "SELECT e.id,e.created_at,e.hangout_id FROM bot_events e JOIN agents a ON a.id=e.agent_id WHERE e.agent_id=?1 AND a.owner_id=?2 AND e.name='mention' AND e.created_at>=?3 ORDER BY e.id DESC LIMIT 1",
            params![agent,owner,at-7*86400],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
        ).optional()?;
        let latest_mention = if let Some((id, recorded, room)) = latest {
            let room = match room {
                Some(room)
                    if self.agent_in_room(&room, agent, at)?
                        && self.owner_sees_room(owner, &room)? =>
                {
                    Some(room)
                }
                _ => None,
            };
            Some(
                json!({"event_id":format!("evt_{id}"),"recorded_at":crate::ops::iso(recorded),"hangout_id":room}),
            )
        } else {
            None
        };
        Ok(
            json!({"agent_id":agent,"latest_mention":latest_mention,"connections":connections,"eligible_rooms":eligible,"other_rooms":unseen,"wake_paused":self.doorbell_paused(agent)?,"wake_only_owner":self.wakes_only_for_owner(agent)?,"coverage_note":"Active subscription coverage; each sender must also pass current wake permissions.","host":{"destination_chat":"unknown","run_started":"unknown","room_read":"unknown","renewal_automation":"external"},"setup_url":"/oauth/connections"}),
        )
    }
    /// E41: a room the owner may name in their bot's diagnostics: one they belong to, or a public
    /// floor anyone can see.
    fn owner_sees_room(&self, owner: &str, room: &str) -> rusqlite::Result<bool> {
        Ok(self.is_member(room, owner)?
            || self
                .conn()
                .query_row(
                    "SELECT 1 FROM floors WHERE hangout_id = ? AND access = 'public'",
                    [room],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
    }
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
        let args: serde_json::Value = serde_json::from_str(&s.arguments).unwrap_or_default();
        self.conn().execute("INSERT INTO oauth_event_health(subscription_id,grant_id,owner_id,agent_id,client_name,scope,grant_expires_at,hangout_id,expires_at,last_renewed_at,cursor,state) SELECT ?1,g.id,g.owner_id,g.agent_id,c.name,g.scope,g.expires_at,?2,?3,?4,?5,'active' FROM mcp_oauth_grants g JOIN mcp_oauth_clients c ON c.id=g.client_id WHERE g.id=?6 ON CONFLICT(subscription_id) DO UPDATE SET expires_at=excluded.expires_at,last_renewed_at=excluded.last_renewed_at,state='active'",params![s.id,args["hangout_id"].as_str(),s.expires_at,crate::now(),s.cursor,s.grant_id])?;
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
        self.conn().execute(
            "UPDATE oauth_event_health SET cursor=?2,state=?3 WHERE subscription_id=?1",
            params![s.id, s.cursor, if s.active { "active" } else { "suspended" }],
        )?;
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
    fn monitoring_reports_only_safe_retained_mention_source_metadata() {
        let state = setup(Db::open(":memory:").unwrap());
        let at = now();
        assert!(state.db.oauth_monitoring("owner", "bot", at).unwrap()["latest_mention"].is_null());
        state
            .db
            .create_hangout("private-source", "Private source", "private-invite", "other", at)
            .unwrap();
        let id = state
            .db
            .insert_bot_event(
                "bot",
                "mention",
                Some("private-source"),
                "source",
                r#"{"body":"DO NOT EXPOSE"}"#,
                at - 5,
                0,
            )
            .unwrap()
            .unwrap();
        let value = state.db.oauth_monitoring("owner", "bot", at).unwrap();
        assert_eq!(value["latest_mention"]["event_id"], format!("evt_{id}"));
        assert_eq!(value["latest_mention"]["recorded_at"], crate::ops::iso(at - 5));
        assert!(value["latest_mention"]["hangout_id"].is_null());
        assert!(!value.to_string().contains("DO NOT EXPOSE"));
        assert!(!value.to_string().contains("private-source"));
        state.db.add_agent_to_room("private-source", "bot").unwrap();
        // E41: someone else's private room stays unnamed until the owner belongs to it too.
        assert!(state.db.oauth_monitoring("owner", "bot", at).unwrap()["latest_mention"]
            ["hangout_id"]
            .is_null());
        state.db.add_member("private-source", "owner", at).unwrap();
        assert_eq!(
            state.db.oauth_monitoring("owner", "bot", at).unwrap()["latest_mention"]["hangout_id"],
            "private-source"
        );
        state.db.remove_agent_from_room("private-source", "bot").unwrap();
        assert!(state.db.oauth_monitoring("owner", "bot", at).unwrap()["latest_mention"]
            ["hangout_id"]
            .is_null());
        assert!(state.db.oauth_monitoring("other", "bot", at).unwrap()["latest_mention"].is_null());
        assert!(state.db.oauth_monitoring("owner", "bot", at + 7 * 86400).unwrap()
            ["latest_mention"]
            .is_null());
    }

    #[test]
    fn monitoring_is_safe_and_expiry_retains_the_acknowledged_cursor() {
        let state = setup(Db::open(":memory:").unwrap());
        let mut sub = state.db.oauth_subscription("sub").unwrap().unwrap();
        sub.cursor = 42;
        state.db.oauth_record_outcome(&sub, "http_accepted", Some(200)).unwrap();
        state.db.oauth_retire_subscription(&sub, "expired").unwrap();
        assert!(state.db.oauth_subscription("sub").unwrap().is_none());
        assert_eq!(state.db.oauth_health_cursor("sub").unwrap(), Some(42));
        let value = state.db.oauth_monitoring("owner", "bot", now()).unwrap();
        assert_eq!(value["connections"][0]["subscriptions"][0]["status"], "expired");
        assert_eq!(value["connections"][0]["subscriptions"][0]["cursor"], "42");
        assert_eq!(
            value["connections"][0]["subscriptions"][0]["last_delivery"]["reason"],
            "http_accepted"
        );
        assert_eq!(value["host"]["run_started"], "unknown");
        assert!(!value.to_string().contains("callback.example"));
        assert!(!value.to_string().contains("whsec_"));
        assert!(state.db.oauth_monitoring("other", "bot", now()).unwrap()["connections"]
            .as_array()
            .unwrap()
            .is_empty());
        let caller = crate::auth::Caller {
            person_id: "owner".into(),
            agent_id: None,
            name: "Owner".into(),
            secret_hash: "key".into(),
            is_house: false,
        };
        assert!(crate::events::oauth::monitoring(&state, &caller, "bot").is_ok());
        let mut stranger = caller.clone();
        stranger.person_id = "other".into();
        assert_eq!(
            crate::events::oauth::monitoring(&state, &stranger, "bot").unwrap_err().status,
            axum::http::StatusCode::NOT_FOUND
        );
        let mut bot = caller;
        bot.agent_id = Some("bot".into());
        assert_eq!(
            crate::events::oauth::monitoring(&state, &bot, "bot").unwrap_err().status,
            axum::http::StatusCode::FORBIDDEN
        );
        state.db.create_hangout("private", "Secret room", "invite", "other", now()).unwrap();
        assert!(!state
            .db
            .oauth_monitoring("owner", "bot", now())
            .unwrap()
            .to_string()
            .contains("Secret room"));
        state.db.conn().execute("DELETE FROM agents WHERE id='bot'", []).unwrap();
        assert_eq!(state.db.oauth_health_cursor("sub").unwrap(), None);
    }

    /// A room the owner and the bot are both in, so E41 names it.
    fn shared_room(state: &AppState, id: &str) {
        state.db.create_hangout(id, id, &format!("invite-{id}"), "other", now()).unwrap();
        state.db.add_member(id, "owner", now()).unwrap();
        state.db.add_agent_to_room(id, "bot").unwrap();
    }

    /// E46: the connection's wake_warning as inbox would show it, at a given time.
    fn wake_warning(state: &AppState, at: i64) -> Option<serde_json::Value> {
        let view = state.db.oauth_monitoring("owner", "bot", at).unwrap();
        let connection =
            view["connections"].as_array().unwrap().iter().find(|c| c["id"] == "grant").cloned()?;
        crate::events::oauth::lapsed_coverage(&connection, &view["eligible_rooms"])
    }

    /// Another subscription on the same connection, with its own room filter and expiry.
    fn filtered(
        state: &AppState,
        from: &OAuthSubscription,
        id: &str,
        room: Option<&str>,
        expires_at: i64,
    ) {
        let mut sub = from.clone();
        sub.id = id.into();
        sub.arguments = serde_json::json!({ "hangout_id": room }).to_string();
        sub.expires_at = expires_at;
        state.db.oauth_save_subscription(&sub).unwrap();
    }

    #[test]
    fn idle_expiry_warns_and_renewing_the_same_subscription_clears_it_keeping_the_cursor() {
        let state = setup(Db::open(":memory:").unwrap());
        let at = now();
        // The grant outlives the subscription's lease, as a real connection's does.
        state.db.conn().execute("UPDATE mcp_oauth_grants SET expires_at=?", [at + 86400]).unwrap();
        shared_room(&state, "room");
        assert!(wake_warning(&state, at).is_none());
        // E46: no chat, no delivery attempt, nothing written: only time passes the lease.
        let later = at + 301;
        let warning = wake_warning(&state, later).expect("a lapsed lease on a live connection");
        let lapsed = &warning["subscriptions"][0];
        assert_eq!(lapsed["subscription_id"], "sub");
        assert_eq!(lapsed["status"], "expired");
        assert!(lapsed["hangout_id"].is_null());
        assert_eq!(lapsed["uncovered_rooms"], serde_json::json!([{"id":"room","name":"room"}]));
        assert!(!warning.to_string().contains("callback.example"));
        assert!(!warning.to_string().contains("whsec_"));
        // The connection itself still reads as on: the warning changes nothing.
        let view = state.db.oauth_monitoring("owner", "bot", later).unwrap();
        assert_eq!(view["connections"][0]["status"], "active");
        // Renewing the same subscription ends the warning and keeps its cursor (E42).
        let mut sub = state.db.oauth_subscription("sub").unwrap().unwrap();
        sub.expires_at = later + 3600;
        sub.cursor = 999;
        state.db.oauth_save_subscription(&sub).unwrap();
        assert!(wake_warning(&state, later).is_none());
        assert_eq!(state.db.oauth_subscription("sub").unwrap().unwrap().cursor, 7);
        assert_eq!(state.db.oauth_health_cursor("sub").unwrap(), Some(7));
    }

    #[test]
    fn a_lapsed_room_filter_names_only_its_own_uncovered_rooms() {
        let state = setup(Db::open(":memory:").unwrap());
        let at = now();
        state.db.conn().execute("UPDATE mcp_oauth_grants SET expires_at=?", [at + 86400]).unwrap();
        shared_room(&state, "a");
        shared_room(&state, "b");
        // The every-room subscription from setup is stopped on purpose, which is not a lapse.
        let all = state.db.oauth_subscription("sub").unwrap().unwrap();
        filtered(&state, &all, "sub-a", Some("a"), at + 300);
        filtered(&state, &all, "sub-b", Some("b"), at + 3600);
        state.db.oauth_retire_subscription(&all, "stopped").unwrap();
        let later = at + 301;
        let warning = wake_warning(&state, later).expect("room a lost its only subscription");
        let lapsed = warning["subscriptions"].as_array().unwrap();
        assert_eq!(lapsed.len(), 1);
        assert_eq!(lapsed[0]["subscription_id"], "sub-a");
        assert_eq!(lapsed[0]["hangout_id"], "a");
        // Room b is covered by its own active subscription, and isn't in sub-a's filter anyway.
        assert_eq!(lapsed[0]["uncovered_rooms"], serde_json::json!([{"id":"a","name":"a"}]));
        // Another active subscription covering room a means nothing is left uncovered.
        filtered(&state, &all, "sub-all", None, later + 3600);
        assert!(wake_warning(&state, later).is_none());
        let view = state.db.oauth_monitoring("owner", "bot", later).unwrap();
        assert!(view["eligible_rooms"].as_array().unwrap().iter().all(|r| r["covered"] == true));
    }

    #[test]
    fn a_stopped_or_revoked_connection_has_no_wake_warning() {
        let state = setup(Db::open(":memory:").unwrap());
        let at = now();
        state.db.conn().execute("UPDATE mcp_oauth_grants SET expires_at=?", [at + 86400]).unwrap();
        shared_room(&state, "room");
        let sub = state.db.oauth_subscription("sub").unwrap().unwrap();
        state.db.oauth_retire_subscription(&sub, "stopped").unwrap();
        // Stopped by its host on purpose: still `stopped` once its old expiry has passed.
        let view = state.db.oauth_monitoring("owner", "bot", at + 301).unwrap();
        assert_eq!(view["connections"][0]["subscriptions"][0]["status"], "stopped");
        assert!(wake_warning(&state, at + 301).is_none());
        let state = setup(Db::open(":memory:").unwrap());
        shared_room(&state, "room");
        state.db.oauth_revoke("grant", "owner").unwrap();
        assert!(wake_warning(&state, now() + 301).is_none());
    }

    #[test]
    fn restart_keeps_identity_cursor_retry_and_signing_rotation_and_renewal_cannot_skip() {
        let path = crate::db::test_db_path("oauth-events");
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
            // Simulate a database from before safe monitoring snapshots existed.
            state.db.conn().execute("DELETE FROM oauth_event_health", []).unwrap();
        }
        {
            let state = AppState::new(Db::open(path.to_str().unwrap()).unwrap(), config);
            let sub = state.db.oauth_subscriptions().unwrap().remove(0);
            assert_eq!(sub.id, "sub");
            assert_eq!(sub.grant_id, "grant");
            assert_eq!(sub.cursor, 42);
            assert_eq!(state.db.oauth_health_cursor("sub").unwrap(), Some(42));
            let health = state.db.oauth_monitoring("owner", "bot", now()).unwrap();
            assert!(health["connections"][0]["subscriptions"][0]["last_renewed_at"].is_null());
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
        {
            let delivered = attempts.lock().unwrap();
            assert_eq!(delivered.len(), 3);
            assert!(delivered.iter().all(|e| e["eventId"] == format!("evt_{event_id}")));
        }
        let sub = state.db.oauth_subscriptions().unwrap().into_iter().find(|s| s.id == id).unwrap();
        assert!(!sub.active);
        assert_eq!(sub.cursor, 0);
        let health = state.db.oauth_monitoring("owner", "bot", now()).unwrap();
        let monitored = health["connections"][0]["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        assert_eq!(monitored["status"], "suspended");
        assert_eq!(monitored["last_delivery"]["http_status"], 503);
        state
            .db
            .conn()
            .execute("UPDATE oauth_event_subscriptions SET expires_at=0 WHERE id=?", [id])
            .unwrap();
        crate::events::oauth::deliver(&state).await.unwrap();
        let renewed = crate::events::oauth::subscribe(&state, &caller, &params).await.unwrap();
        assert_eq!(renewed["id"], result["id"]);
        assert_eq!(renewed["cursor"], "0");
        crate::events::oauth::unsubscribe(&state, &caller, &params).await.unwrap();
        assert_eq!(state.db.oauth_health_cursor(id).unwrap(), None);
        let restarted = crate::events::oauth::subscribe(&state, &caller, &params).await.unwrap();
        assert_eq!(restarted["id"], result["id"]);
        assert_eq!(restarted["cursor"], event_id.to_string());
        state.db.oauth_revoke(&g.id, "owner").unwrap();
        let health = state.db.oauth_monitoring("owner", "bot", now()).unwrap();
        let monitored = health["connections"][0]["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        assert_eq!(monitored["status"], "revoked");
    }
}
