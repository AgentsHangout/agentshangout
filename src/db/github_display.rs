//! Privacy and refresh state are checked under one connection lock/transaction.
use super::*;
use crate::github_display::{self, CachedDisplay, GitHubDisplay, RefreshClaim};
use serde_json::{json, Value};
fn context(
    conn: &Connection,
    person: &str,
) -> rusqlite::Result<Option<(String, String, GitHubDisplay)>> {
    conn.query_row("SELECT l.account_id,l.account_name,d.show_repos,d.show_stars,d.show_languages,d.show_badge FROM sign_in_links l JOIN github_display d ON d.person_id=l.person_id WHERE l.person_id=? AND l.site='github' AND (d.show_repos!=0 OR d.show_stars!=0 OR d.show_languages!=0 OR d.show_badge!=0)",[person],|r|Ok((r.get(0)?,r.get(1)?,GitHubDisplay{show_repos:r.get::<_,i64>(2)?!=0,show_stars:r.get::<_,i64>(3)?!=0,show_languages:r.get::<_,i64>(4)?!=0,show_badge:r.get::<_,i64>(5)?!=0}))).optional()
}
fn cached(conn: &Connection, id: &str) -> rusqlite::Result<Option<CachedDisplay>> {
    conn.query_row("SELECT github_user_id,github_login,repos_json,top_languages_json,merged_pr_count,fetched_at,repos_etag,search_etag,last_attempt_at FROM github_display_cache WHERE github_user_id=?",[id],|r|Ok(CachedDisplay{github_user_id:r.get(0)?,github_login:r.get(1)?,repos_json:r.get(2)?,top_languages_json:r.get(3)?,merged_pr_count:r.get(4)?,fetched_at:r.get(5)?,repos_etag:r.get(6)?,search_etag:r.get(7)?,last_attempt_at:r.get(8)?})).optional()
}
impl Db {
    /// Apply only supplied fields under the same transaction that invalidates refresh claims.
    pub fn patch_github_display(
        &self,
        person: &str,
        patch: &crate::github_display::DisplayPatch,
        at: i64,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("UPDATE github_display_cache SET last_attempt_at=0 WHERE github_user_id IN (SELECT github_user_id FROM github_display_refresh_claims WHERE person_id=?)", [person])?;
        tx.execute("DELETE FROM github_display_refresh_claims WHERE person_id=?", [person])?;
        tx.execute("INSERT INTO github_display(person_id,show_repos,show_stars,show_languages,show_badge,updated_at) VALUES(?1,COALESCE(?2,0),COALESCE(?3,0),COALESCE(?4,0),COALESCE(?5,0),?6) ON CONFLICT(person_id) DO UPDATE SET show_repos=COALESCE(?2,show_repos),show_stars=COALESCE(?3,show_stars),show_languages=COALESCE(?4,show_languages),show_badge=COALESCE(?5,show_badge),updated_at=?6", params![person,patch.show_repos,patch.show_stars,patch.show_languages,patch.show_badge,at])?;
        tx.commit()
    }

    pub fn github_public_snapshot(&self, person: &str) -> rusqlite::Result<Value> {
        let conn = self.conn();
        let Some((id, login, display)) = context(&conn, person)? else {
            return Ok(json!({"linked":false}));
        };
        let cache = cached(&conn, &id)?
            .filter(|row| row.fetched_at > 0 && row.github_login.eq_ignore_ascii_case(&login));
        let mut shown = cache
            .as_ref()
            .map(|row| github_display::public_sections(&display, row))
            .unwrap_or(json!({}));
        shown["linked"] = json!(true);
        shown["login"] = json!(login);
        Ok(shown)
    }
    pub fn claim_github_display_refresh(
        &self,
        person: &str,
        at: i64,
    ) -> rusqlite::Result<Option<RefreshClaim>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let Some((id, login, _)) = context(&tx, person)? else { return Ok(None) };
        let old = cached(&tx, &id)?;
        let rename_due = old.as_ref().is_some_and(|row| {
            !row.github_login.eq_ignore_ascii_case(&login)
                && (row.last_attempt_at == 0 || at - row.last_attempt_at >= 3600)
        });
        if !github_display::should_refresh(old.as_ref(), at) && !rename_due {
            return Ok(None);
        }
        let claim = RefreshClaim {
            id: crate::auth::new_id(),
            github_user_id: id,
            github_login: login,
            existing: old,
        };
        tx.execute("INSERT INTO github_display_refresh_claims VALUES(?1,?2,?3,?4,?5) ON CONFLICT(person_id) DO UPDATE SET claim_id=excluded.claim_id,github_user_id=excluded.github_user_id,github_login=excluded.github_login,claimed_at=excluded.claimed_at",params![person,claim.id,claim.github_user_id,claim.github_login,at])?;
        tx.execute("INSERT INTO github_display_cache(github_user_id,github_login,last_attempt_at) VALUES(?1,?2,?3) ON CONFLICT(github_user_id) DO UPDATE SET last_attempt_at=excluded.last_attempt_at",params![claim.github_user_id,claim.github_login,at])?;
        tx.commit()?;
        Ok(Some(claim))
    }
    pub fn finish_github_display_refresh(
        &self,
        person: &str,
        claim: &RefreshClaim,
        row: Option<&CachedDisplay>,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let owns:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM github_display_refresh_claims WHERE person_id=?1 AND claim_id=?2 AND github_user_id=?3 AND github_login=?4)",params![person,claim.id,claim.github_user_id,claim.github_login],|r|r.get(0))?;
        let current = context(&tx, person)?;
        let valid = owns
            && current.as_ref().is_some_and(|(id, login, _)| {
                id == &claim.github_user_id && login == &claim.github_login
            });
        if valid {
            if let Some(row) = row.filter(|row| {
                row.github_user_id == claim.github_user_id
                    && row.github_login.eq_ignore_ascii_case(&claim.github_login)
            }) {
                tx.execute("UPDATE github_display_cache SET github_login=?2,repos_json=?3,top_languages_json=?4,merged_pr_count=?5,fetched_at=?6,repos_etag=?7,search_etag=?8,last_attempt_at=?9 WHERE github_user_id=?1",params![row.github_user_id,row.github_login,row.repos_json,row.top_languages_json,row.merged_pr_count,row.fetched_at,row.repos_etag,row.search_etag,row.last_attempt_at])?;
            }
        }
        tx.execute(
            "DELETE FROM github_display_refresh_claims WHERE person_id=?1 AND claim_id=?2",
            params![person, claim.id],
        )?;
        tx.commit()?;
        Ok(valid)
    }
}
