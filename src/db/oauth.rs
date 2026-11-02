//! OAuth state transitions are serialized transactions, including refresh replay revocation.
use super::*;
use crate::oauth::{ClientRecord, Grant, TokenPair};

pub(super) fn grant(row: &Row<'_>) -> rusqlite::Result<Grant> {
    Ok(Grant {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        agent_id: row.get(2)?,
        client_id: row.get(3)?,
        resource: row.get(4)?,
        scope: row.get(5)?,
        expires_at: row.get(6)?,
    })
}
pub(super) const LIVE_GRANT: &str = "SELECT g.id,g.owner_id,g.agent_id,g.client_id,g.resource,g.scope,g.expires_at FROM mcp_oauth_grants g JOIN agents a ON a.id=g.agent_id AND a.owner_id=g.owner_id WHERE g.id=?1 AND g.revoked=0 AND g.expires_at>?2 AND NOT EXISTS(SELECT 1 FROM bot_homes h WHERE h.agent_id=a.id AND h.is_house=1)";
fn cleanup(conn: &Connection, at: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM mcp_oauth_requests WHERE expires_at<=?", [at])?;
    conn.execute("DELETE FROM mcp_oauth_grants WHERE expires_at<=?", [at])?;
    // Retain refresh/code replay markers until their grant expires.
    conn.execute("DELETE FROM mcp_oauth_clients WHERE created_at<?1 AND NOT EXISTS(SELECT 1 FROM mcp_oauth_grants g WHERE g.client_id=mcp_oauth_clients.id)", [at-30*86400])?;
    Ok(())
}
fn insert_tokens(conn: &Connection, g: &Grant, pair: &TokenPair, at: i64) -> rusqlite::Result<()> {
    for (hash, kind, expiry) in [
        (&pair.access_hash, "access", (at + 3600).min(g.expires_at)),
        (&pair.refresh_hash, "refresh", g.expires_at),
    ] {
        conn.execute(
            "INSERT INTO mcp_oauth_tokens(hash,grant_id,kind,expires_at) VALUES(?1,?2,?3,?4)",
            params![hash, g.id, kind, expiry],
        )?;
    }
    Ok(())
}
impl Db {
    pub(crate) fn oauth_register(
        &self,
        id: &str,
        name: &str,
        redirects: &str,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let conn = self.conn();
        cleanup(&conn, at)?;
        if conn.query_row("SELECT count(*) FROM mcp_oauth_clients", [], |r| r.get::<_, i64>(0))?
            >= 10000
        {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO mcp_oauth_clients VALUES(?1,?2,?3,?4)",
            params![id, name, redirects, at],
        )?;
        Ok(true)
    }
    pub(crate) fn oauth_client(&self, id: &str) -> rusqlite::Result<Option<ClientRecord>> {
        self.conn()
            .query_row("SELECT name,redirects FROM mcp_oauth_clients WHERE id=?", [id], |r| {
                Ok(ClientRecord { name: r.get(0)?, redirects: r.get(1)? })
            })
            .optional()
    }
    pub(crate) fn oauth_request(
        &self,
        nonce: &str,
        owner: &str,
        session: &str,
        payload: &str,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let conn = self.conn();
        cleanup(&conn, at)?;
        if conn.query_row(
            "SELECT count(*) FROM mcp_oauth_requests WHERE owner_id=?",
            [owner],
            |r| r.get::<_, i64>(0),
        )? >= 20
        {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO mcp_oauth_requests VALUES(?1,?2,?3,?4,?5)",
            params![nonce, owner, session, payload, at + 600],
        )?;
        Ok(true)
    }
    pub(crate) fn oauth_take_request(
        &self,
        nonce: &str,
        owner: &str,
        session: &str,
        at: i64,
    ) -> rusqlite::Result<Option<String>> {
        self.conn().query_row("DELETE FROM mcp_oauth_requests WHERE nonce_hash=?1 AND owner_id=?2 AND session_hash=?3 AND expires_at>?4 RETURNING payload",params![nonce,owner,session,at],|r|r.get(0)).optional()
    }
    pub(crate) fn oauth_issue_code(
        &self,
        g: &Grant,
        code: &str,
        redirect: &str,
        challenge: &str,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        cleanup(&tx, at)?;
        if tx.query_row(
            "SELECT count(*) FROM mcp_oauth_grants WHERE owner_id=?",
            [&g.owner_id],
            |r| r.get::<_, i64>(0),
        )? >= 100
        {
            return Ok(false);
        }
        let inserted=tx.execute("INSERT INTO mcp_oauth_grants SELECT ?1,?2,?3,?4,?5,?6,?7,?8,0 WHERE EXISTS(SELECT 1 FROM agents WHERE id=?3 AND owner_id=?2) AND NOT EXISTS(SELECT 1 FROM bot_homes WHERE agent_id=?3 AND is_house=1)",params![g.id,g.owner_id,g.agent_id,g.client_id,g.resource,g.scope,at,g.expires_at])?;
        if inserted == 0 {
            return Ok(false);
        }
        tx.execute("INSERT INTO mcp_oauth_codes(hash,grant_id,redirect_uri,challenge,expires_at) VALUES(?1,?2,?3,?4,?5)",params![code,g.id,redirect,challenge,at+300])?;
        tx.commit()?;
        Ok(true)
    }
    // Explicit bindings are intentionally separate so a caller cannot omit one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn oauth_exchange(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        redirect: &str,
        challenge: &str,
        pair: &TokenPair,
        at: i64,
    ) -> rusqlite::Result<Option<Grant>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let row:Option<(String,i64)>=tx.query_row("SELECT c.grant_id,c.used FROM mcp_oauth_codes c JOIN mcp_oauth_grants g ON g.id=c.grant_id WHERE c.hash=?1 AND g.client_id=?2 AND g.resource=?3 AND c.redirect_uri=?4 AND c.challenge=?5 AND c.expires_at>?6",params![hash,client,resource,redirect,challenge,at],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((id, used)) = row else { return Ok(None) };
        if used != 0 {
            tx.execute("UPDATE mcp_oauth_grants SET revoked=1 WHERE id=?", [id])?;
            tx.commit()?;
            return Ok(None);
        }
        let g = tx.query_row(LIVE_GRANT, params![id, at], grant).optional()?;
        let Some(g) = g else { return Ok(None) };
        tx.execute("UPDATE mcp_oauth_codes SET used=1 WHERE hash=?", [hash])?;
        insert_tokens(&tx, &g, pair, at)?;
        tx.commit()?;
        Ok(Some(g))
    }
    pub(crate) fn oauth_refresh(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        pair: &TokenPair,
        at: i64,
    ) -> rusqlite::Result<Option<Grant>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let row:Option<(String,i64)>=tx.query_row("SELECT t.grant_id,t.used FROM mcp_oauth_tokens t JOIN mcp_oauth_grants g ON g.id=t.grant_id WHERE t.hash=?1 AND t.kind='refresh' AND t.expires_at>?2 AND g.client_id=?3 AND g.resource=?4",params![hash,at,client,resource],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((id, used)) = row else { return Ok(None) };
        if used != 0 {
            tx.execute("UPDATE mcp_oauth_grants SET revoked=1 WHERE id=?", [id])?;
            tx.commit()?;
            return Ok(None);
        }
        let Some(g) = tx.query_row(LIVE_GRANT, params![id, at], grant).optional()? else {
            return Ok(None);
        };
        tx.execute(
            "DELETE FROM mcp_oauth_tokens WHERE grant_id=?1 AND kind='access' AND expires_at<=?2",
            params![g.id, at],
        )?;
        let count: i64 =
            tx.query_row("SELECT count(*) FROM mcp_oauth_tokens WHERE grant_id=?", [&g.id], |r| {
                r.get(0)
            })?;
        if count >= 4094 {
            tx.execute("UPDATE mcp_oauth_grants SET revoked=1 WHERE id=?", [&g.id])?;
            tx.commit()?;
            return Ok(None);
        }
        tx.execute("UPDATE mcp_oauth_tokens SET used=1 WHERE hash=?", [hash])?;
        insert_tokens(&tx, &g, pair, at)?;
        tx.commit()?;
        Ok(Some(g))
    }
    pub(crate) fn oauth_access(
        &self,
        hash: &str,
        resource: &str,
        at: i64,
    ) -> rusqlite::Result<Option<Grant>> {
        let conn = self.conn();
        let id:Option<String>=conn.query_row("SELECT grant_id FROM mcp_oauth_tokens WHERE hash=?1 AND kind='access' AND expires_at>?2 AND used=0",params![hash,at],|r|r.get(0)).optional()?;
        match id {
            Some(id) => Ok(conn
                .query_row(LIVE_GRANT, params![id, at], grant)
                .optional()?
                .filter(|g| g.resource == resource)),
            None => Ok(None),
        }
    }
    pub(crate) fn oauth_revoke_token(&self, hash: &str, client: &str) -> rusqlite::Result<()> {
        self.conn().execute("UPDATE mcp_oauth_grants SET revoked=1 WHERE client_id=?1 AND id IN(SELECT grant_id FROM mcp_oauth_tokens WHERE hash=?2)",params![client,hash])?;
        Ok(())
    }
    pub(crate) fn oauth_revoke(&self, id: &str, owner: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE mcp_oauth_grants SET revoked=1 WHERE id=?1 AND owner_id=?2",
            params![id, owner],
        )?;
        Ok(())
    }
    pub(crate) fn oauth_grants(
        &self,
        owner: &str,
        at: i64,
    ) -> rusqlite::Result<Vec<(String, String, String)>> {
        let conn = self.conn();
        let mut stmt=conn.prepare("SELECT g.id,c.name,a.name FROM mcp_oauth_grants g JOIN mcp_oauth_clients c ON c.id=g.client_id JOIN agents a ON a.id=g.agent_id WHERE g.owner_id=?1 AND g.revoked=0 AND g.expires_at>?2 ORDER BY g.created_at DESC")?;
        let rows = stmt.query_map(params![owner, at], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const AT: i64 = 1000;
    fn setup() -> Db {
        let db = Db::open(":memory:").unwrap();
        db.create_person("owner", "Owner", "owner-key", "feed", AT).unwrap();
        db.create_person("other", "Other", "other-key", "other-feed", AT).unwrap();
        db.create_agent("bot", "owner", "Bot", "legacy", AT).unwrap();
        db.oauth_register("client", "Client", "[\"https://client.example/callback\"]", AT).unwrap();
        db
    }
    fn issue(db: &Db, id: &str) -> Grant {
        let g = Grant {
            id: id.into(),
            owner_id: "owner".into(),
            agent_id: "bot".into(),
            client_id: "client".into(),
            resource: "https://server.example/mcp".into(),
            scope: "mcp:bot".into(),
            expires_at: AT + 30 * 86400,
        };
        assert!(db.oauth_issue_code(&g, id, "redirect", "challenge", AT).unwrap());
        g
    }
    fn pair(id: &str) -> TokenPair {
        TokenPair { access_hash: format!("a{id}"), refresh_hash: format!("r{id}") }
    }
    fn exchange(db: &Db, id: &str, at: i64) -> Option<Grant> {
        db.oauth_exchange(
            id,
            "client",
            "https://server.example/mcp",
            "redirect",
            "challenge",
            &pair(id),
            at,
        )
        .unwrap()
    }
    #[test]
    fn expiry_applies_to_consent_codes_access_refresh_and_absolute_grant() {
        let db = setup();
        db.oauth_request("nonce", "owner", "session", "payload", AT).unwrap();
        assert!(db.oauth_take_request("nonce", "owner", "session", AT + 600).unwrap().is_none());
        issue(&db, "expired-code");
        assert!(exchange(&db, "expired-code", AT + 300).is_none());
        let g = issue(&db, "valid");
        exchange(&db, "valid", AT).unwrap();
        assert!(db.oauth_access("avalid", &g.resource, AT + 3599).unwrap().is_some());
        assert!(db.oauth_access("avalid", &g.resource, AT + 3600).unwrap().is_none());
        assert!(db
            .oauth_refresh("rvalid", "client", &g.resource, &pair("next"), g.expires_at)
            .unwrap()
            .is_none());
    }
    #[test]
    fn current_owner_and_bot_existence_are_checked_and_deletion_cascades() {
        let db = setup();
        let g = issue(&db, "grant");
        exchange(&db, "grant", AT).unwrap();
        db.conn().execute("UPDATE agents SET owner_id='other' WHERE id='bot'", []).unwrap();
        assert!(db.oauth_access("agrant", &g.resource, AT).unwrap().is_none());
        assert!(db
            .oauth_refresh("rgrant", "client", &g.resource, &pair("new"), AT)
            .unwrap()
            .is_none());
        db.conn().execute("DELETE FROM agents WHERE id='bot'", []).unwrap();
        for table in ["mcp_oauth_grants", "mcp_oauth_codes", "mcp_oauth_tokens"] {
            assert_eq!(
                db.conn()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
    #[test]
    fn simultaneous_redemption_issues_once_and_replay_revokes_the_family() {
        let db = std::sync::Arc::new(setup());
        issue(&db, "code");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut workers = vec![];
        for i in 0..8 {
            let db = db.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                db.oauth_exchange(
                    "code",
                    "client",
                    "https://server.example/mcp",
                    "redirect",
                    "challenge",
                    &pair(&i.to_string()),
                    AT,
                )
                .unwrap()
                .is_some()
            }));
        }
        assert_eq!(workers.into_iter().map(|w| usize::from(w.join().unwrap())).sum::<usize>(), 1);
        assert_eq!(
            db.conn()
                .query_row("SELECT count(*) FROM mcp_oauth_tokens", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.conn()
                .query_row("SELECT revoked FROM mcp_oauth_grants WHERE id='code'", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn simultaneous_refresh_issues_once_and_replay_revokes_the_family() {
        let db = std::sync::Arc::new(setup());
        let g = issue(&db, "code");
        exchange(&db, "code", AT).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut workers = vec![];
        for i in 0..8 {
            let db = db.clone();
            let barrier = barrier.clone();
            let resource = g.resource.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                db.oauth_refresh("rcode", "client", &resource, &pair(&i.to_string()), AT)
                    .unwrap()
                    .is_some()
            }));
        }
        assert_eq!(workers.into_iter().map(|w| usize::from(w.join().unwrap())).sum::<usize>(), 1);
        assert_eq!(
            db.conn()
                .query_row("SELECT count(*) FROM mcp_oauth_tokens", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert!(db.oauth_access("acode", &g.resource, AT).unwrap().is_none());
    }
    #[test]
    fn retained_grants_and_token_rows_have_hard_bounds_even_after_revocation() {
        let db = setup();
        for i in 0..100 {
            let id = i.to_string();
            issue(&db, &id);
            db.oauth_revoke(&id, "owner").unwrap();
        }
        let g = Grant {
            id: "too-many".into(),
            owner_id: "owner".into(),
            agent_id: "bot".into(),
            client_id: "client".into(),
            resource: "resource".into(),
            scope: "mcp:bot".into(),
            expires_at: AT + 10000,
        };
        assert!(!db.oauth_issue_code(&g, "code", "redirect", "challenge", AT).unwrap());
        let db = setup();
        let g = issue(&db, "bounded");
        exchange(&db, "bounded", AT).unwrap();
        {
            let conn = db.conn();
            for i in 0..4092 {
                conn.execute(
                    "INSERT INTO mcp_oauth_tokens VALUES(?1,'bounded','refresh',?2,1)",
                    params![format!("used{i}"), g.expires_at],
                )
                .unwrap();
            }
        }
        assert!(db
            .oauth_refresh("rbounded", "client", &g.resource, &pair("overflow"), AT)
            .unwrap()
            .is_none());
        assert!(db.oauth_access("abounded", &g.resource, AT).unwrap().is_none());
        assert_eq!(
            db.conn()
                .query_row("SELECT count(*) FROM mcp_oauth_tokens", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4094
        );
    }
    #[test]
    fn invalid_exchange_bindings_do_not_consume_valid_code() {
        let db = setup();
        let g = issue(&db, "bound");
        for (client, resource, redirect, challenge) in [
            ("other", g.resource.as_str(), "redirect", "challenge"),
            ("client", "other", "redirect", "challenge"),
            ("client", g.resource.as_str(), "other", "challenge"),
            ("client", g.resource.as_str(), "redirect", "other"),
        ] {
            assert!(db
                .oauth_exchange("bound", client, resource, redirect, challenge, &pair("bad"), AT)
                .unwrap()
                .is_none());
        }
        assert!(exchange(&db, "bound", AT).is_some());
    }
}
