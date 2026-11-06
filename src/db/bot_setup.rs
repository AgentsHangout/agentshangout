//! #647: setup-only OAuth grants (scope `mcp:setup`), held apart from the bot grants, and the bot
//! setups their agents draft for a person to approve on the site.
use super::*;
use crate::oauth::TokenPair;

/// A setup grant: a person's agent may draft bot setups for them, and nothing else.
#[derive(Debug, Clone)]
pub struct SetupGrant {
    pub id: String,
    pub owner_id: String,
    pub client_id: String,
    pub resource: String,
    pub expires_at: i64,
}

/// A bot setup an agent drafted for its person, waiting for them to approve it on the site.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BotSetupDraft {
    pub id: String,
    #[serde(skip)]
    pub owner_id: String,
    pub name: String,
    pub role: Option<String>,
    pub job: Option<String>,
    pub workplace: Option<String>,
    pub day_plan: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub expires_at: i64,
}

const LIVE_SETUP_GRANT: &str = "SELECT id, owner_id, client_id, resource, expires_at
     FROM mcp_setup_grants WHERE id = ?1 AND revoked = 0 AND expires_at > ?2";

fn setup_grant(row: &Row<'_>) -> rusqlite::Result<SetupGrant> {
    Ok(SetupGrant {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        client_id: row.get(2)?,
        resource: row.get(3)?,
        expires_at: row.get(4)?,
    })
}

fn draft(row: &Row<'_>) -> rusqlite::Result<BotSetupDraft> {
    Ok(BotSetupDraft {
        id: row.get(0)?,
        owner_id: row.get(1)?,
        name: row.get(2)?,
        role: row.get(3)?,
        job: row.get(4)?,
        workplace: row.get(5)?,
        day_plan: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        expires_at: row.get(9)?,
    })
}

const DRAFT_COLS: &str =
    "id, owner_id, name, role, job, workplace, day_plan, status, created_at, expires_at";

fn insert_setup_tokens(
    conn: &Connection,
    g: &SetupGrant,
    pair: &TokenPair,
    at: i64,
) -> rusqlite::Result<()> {
    for (hash, kind, expiry) in [
        (&pair.access_hash, "access", (at + 3600).min(g.expires_at)),
        (&pair.refresh_hash, "refresh", g.expires_at),
    ] {
        conn.execute(
            "INSERT INTO mcp_setup_tokens(hash, grant_id, kind, expires_at) VALUES(?1, ?2, ?3, ?4)",
            params![hash, g.id, kind, expiry],
        )?;
    }
    Ok(())
}

impl Db {
    /// #647: a setup grant and its one-time code, for a person who chose "set up a new bot".
    pub(crate) fn setup_issue_code(
        &self,
        g: &SetupGrant,
        code: &str,
        redirect: &str,
        challenge: &str,
        at: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM mcp_setup_grants WHERE expires_at <= ?", [at])?;
        if tx.query_row(
            "SELECT count(*) FROM mcp_setup_grants WHERE owner_id = ?",
            [&g.owner_id],
            |r| r.get::<_, i64>(0),
        )? >= 10
        {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO mcp_setup_grants(id, owner_id, client_id, resource, created_at, expires_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![g.id, g.owner_id, g.client_id, g.resource, at, g.expires_at],
        )?;
        tx.execute(
            "INSERT INTO mcp_setup_codes(hash, grant_id, redirect_uri, challenge, expires_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![code, g.id, redirect, challenge, at + 300],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// #647: swaps a setup code for tokens, once. A code used twice revokes its grant.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn setup_exchange(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        redirect: &str,
        challenge: &str,
        pair: &TokenPair,
        at: i64,
    ) -> rusqlite::Result<Option<SetupGrant>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT c.grant_id, c.used FROM mcp_setup_codes c
                 JOIN mcp_setup_grants g ON g.id = c.grant_id
                 WHERE c.hash = ?1 AND g.client_id = ?2 AND g.resource = ?3
                   AND c.redirect_uri = ?4 AND c.challenge = ?5 AND c.expires_at > ?6",
                params![hash, client, resource, redirect, challenge, at],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, used)) = row else { return Ok(None) };
        if used != 0 {
            tx.execute("UPDATE mcp_setup_grants SET revoked = 1 WHERE id = ?", [id])?;
            tx.commit()?;
            return Ok(None);
        }
        let Some(g) = tx.query_row(LIVE_SETUP_GRANT, params![id, at], setup_grant).optional()?
        else {
            return Ok(None);
        };
        tx.execute("UPDATE mcp_setup_codes SET used = 1 WHERE hash = ?", [hash])?;
        insert_setup_tokens(&tx, &g, pair, at)?;
        tx.commit()?;
        Ok(Some(g))
    }

    /// #647: a setup refresh token for new tokens, once. A refresh token used twice revokes.
    pub(crate) fn setup_refresh(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        pair: &TokenPair,
        at: i64,
    ) -> rusqlite::Result<Option<SetupGrant>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT t.grant_id, t.used FROM mcp_setup_tokens t
                 JOIN mcp_setup_grants g ON g.id = t.grant_id
                 WHERE t.hash = ?1 AND t.kind = 'refresh' AND t.expires_at > ?2
                   AND g.client_id = ?3 AND g.resource = ?4",
                params![hash, at, client, resource],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, used)) = row else { return Ok(None) };
        if used != 0 {
            tx.execute("UPDATE mcp_setup_grants SET revoked = 1 WHERE id = ?", [id])?;
            tx.commit()?;
            return Ok(None);
        }
        let Some(g) = tx.query_row(LIVE_SETUP_GRANT, params![id, at], setup_grant).optional()?
        else {
            return Ok(None);
        };
        tx.execute(
            "DELETE FROM mcp_setup_tokens WHERE grant_id = ?1 AND kind = 'access' AND expires_at <= ?2",
            params![g.id, at],
        )?;
        tx.execute("UPDATE mcp_setup_tokens SET used = 1 WHERE hash = ?", [hash])?;
        insert_setup_tokens(&tx, &g, pair, at)?;
        tx.commit()?;
        Ok(Some(g))
    }

    /// #647: the live setup grant behind an access token.
    pub(crate) fn setup_access(
        &self,
        hash: &str,
        resource: &str,
        at: i64,
    ) -> rusqlite::Result<Option<SetupGrant>> {
        let conn = self.conn();
        let id: Option<String> = conn
            .query_row(
                "SELECT grant_id FROM mcp_setup_tokens
                 WHERE hash = ?1 AND kind = 'access' AND expires_at > ?2 AND used = 0",
                params![hash, at],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => Ok(conn
                .query_row(LIVE_SETUP_GRANT, params![id, at], setup_grant)
                .optional()?
                .filter(|g| g.resource == resource)),
            None => Ok(None),
        }
    }

    /// #647: stores a draft, if this person has fewer than `most` open. None if they're full.
    #[allow(clippy::too_many_arguments)]
    pub fn add_bot_setup_draft(
        &self,
        id: &str,
        owner_id: &str,
        name: &str,
        role: Option<&str>,
        job: Option<&str>,
        workplace: Option<&str>,
        day_plan: Option<&str>,
        most: i64,
        at: i64,
        lasts: i64,
    ) -> rusqlite::Result<Option<BotSetupDraft>> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM bot_setup_drafts WHERE expires_at <= ? AND status = 'open'",
            [at],
        )?;
        let changed = conn.execute(
            "INSERT INTO bot_setup_drafts
                 (id, owner_id, name, role, job, workplace, day_plan, status, created_at, expires_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9
             WHERE (SELECT COUNT(*) FROM bot_setup_drafts WHERE owner_id = ?2 AND status = 'open') < ?10",
            params![id, owner_id, name, role, job, workplace, day_plan, at, at + lasts, most],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        conn.query_row(
            &format!("SELECT {DRAFT_COLS} FROM bot_setup_drafts WHERE id = ?"),
            [id],
            draft,
        )
        .optional()
    }

    /// #647: this person's drafts that haven't lapsed, newest first.
    pub fn bot_setup_drafts(
        &self,
        owner_id: &str,
        at: i64,
    ) -> rusqlite::Result<Vec<BotSetupDraft>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DRAFT_COLS} FROM bot_setup_drafts
             WHERE owner_id = ? AND (status <> 'open' OR expires_at > ?)
             ORDER BY created_at DESC LIMIT 20"
        ))?;
        let rows = stmt.query_map(params![owner_id, at], draft)?;
        rows.collect()
    }

    /// #647: one of this person's drafts.
    pub fn bot_setup_draft(
        &self,
        owner_id: &str,
        id: &str,
    ) -> rusqlite::Result<Option<BotSetupDraft>> {
        self.conn()
            .query_row(
                &format!("SELECT {DRAFT_COLS} FROM bot_setup_drafts WHERE id = ? AND owner_id = ?"),
                [id, owner_id],
                draft,
            )
            .optional()
    }
}
