//! Local work metadata contains no GitHub credentials (RW1-RW4).
use super::*;
use crate::research_work::{Card, Snapshot};
use serde_json::{json, Value};
impl Db {
    pub fn research_work_identity(&self, owner: &str) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT account_id FROM sign_in_links WHERE person_id=? AND site='github'",
                [owner],
                |r| r.get(0),
            )
            .optional()
    }
    pub fn research_work_enabled(&self, lab: &str, agent: &str) -> rusqlite::Result<bool> {
        self.conn().query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM research_work_disabled_bots WHERE lab_id=?1 AND agent_id=?2)",
            params![lab, agent],
            |r| r.get(0),
        )
    }
    pub fn set_research_work_enabled(
        &self,
        lab: &str,
        agent: &str,
        enabled: bool,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if enabled {
            tx.execute(
                "DELETE FROM research_work_disabled_bots WHERE lab_id=?1 AND agent_id=?2",
                params![lab, agent],
            )?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO research_work_disabled_bots VALUES(?1,?2)",
                params![lab, agent],
            )?;
        }
        tx.commit()
    }
    pub fn research_work_snapshot(&self, repo: &str) -> rusqlite::Result<Option<Snapshot>> {
        self.conn().query_row("SELECT cards_json,fetched_at,attempted_at,stale FROM research_work_cache WHERE repo=?",[repo],|r|Ok(Snapshot{cards:serde_json::from_str(&r.get::<_,String>(0)?).unwrap_or_default(),fetched_at:r.get(1)?,attempted_at:r.get(2)?,stale:r.get(3)?})).optional()
    }
    pub fn reserve_research_work_refresh(&self, repo: &str, at: i64) -> rusqlite::Result<bool> {
        let conn = self.conn();
        Ok(conn.execute("INSERT INTO research_work_cache(repo,attempted_at) VALUES(?1,?2) ON CONFLICT(repo) DO UPDATE SET attempted_at=excluded.attempted_at WHERE research_work_cache.attempted_at<=?2-300",params![repo,at])?==1)
    }
    pub fn finish_research_work_refresh(
        &self,
        repo: &str,
        cards: Option<&[Card]>,
        at: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        if let Some(cards) = cards {
            conn.execute("UPDATE research_work_cache SET cards_json=?2,fetched_at=?3,stale=0 WHERE repo=?1 AND attempted_at=?3",params![repo,serde_json::to_string(cards).unwrap(),at])?;
        } else {
            conn.execute(
                "UPDATE research_work_cache SET stale=1 WHERE repo=?1 AND attempted_at=?2",
                params![repo, at],
            )?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)] // Explicit stored fields, like the other DB insert methods.
    pub fn record_research_work_contribution(
        &self,
        lab_id: &str,
        number: u64,
        agent: &str,
        identity: &str,
        url: &str,
        at: i64,
        merged: bool,
    ) -> rusqlite::Result<bool> {
        let number = i64::try_from(number).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        // Another bot cannot replace attribution of an already recorded pull request.
        let changed=tx.execute("INSERT INTO research_work_contributions VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(lab_id,number) DO UPDATE SET recorded_at=excluded.recorded_at WHERE research_work_contributions.agent_id=?3 AND research_work_contributions.github_user_id=?4",params![lab_id,number,agent,identity,url,at])?==1;
        if changed && merged {
            tx.execute(
                "INSERT OR IGNORE INTO research_work_awards VALUES(?1,?2,?3,?4)",
                params![lab_id, number, agent, at],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
    pub fn research_work_contributions(&self, lab_id: &str) -> rusqlite::Result<Vec<Value>> {
        let conn = self.conn();
        let mut q=conn.prepare("SELECT c.number,c.agent_id,c.github_user_id,c.url,c.recorded_at,a.verified_at,b.name,p.name FROM research_work_contributions c JOIN agents b ON b.id=c.agent_id JOIN people p ON p.id=b.owner_id LEFT JOIN research_work_awards a ON a.lab_id=c.lab_id AND a.number=c.number WHERE c.lab_id=? ORDER BY c.recorded_at DESC,c.number DESC LIMIT 100")?;
        let rows = q.query_map([lab_id],|r|{let verified:Option<i64>=r.get(5)?;Ok(json!({"number":r.get::<_,i64>(0)?,"agent_id":r.get::<_,String>(1)?,"bot_name":r.get::<_,String>(6)?,"owner_name":r.get::<_,String>(7)?,"github_user_id":r.get::<_,String>(2)?,"source":r.get::<_,String>(3)?,"recorded_at":r.get::<_,i64>(4)?,"verified_merged":verified.is_some(),"award":verified.map(|_|"research_contributor"),"swag":verified.map(|_|"research_contributor_badge"),"verified_at":verified}))})?;
        rows.collect()
    }
    /// RW10: a lab's label names, in the order its owner gave them.
    pub fn research_work_areas(&self, lab_id: &str) -> rusqlite::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT label, area FROM research_work_areas WHERE lab_id = ? ORDER BY position",
        )?;
        let rows = stmt.query_map([lab_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// RW10: replaces a lab's label names in one step.
    pub fn set_research_work_areas(
        &self,
        lab_id: &str,
        areas: &[(String, String)],
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM research_work_areas WHERE lab_id = ?", [lab_id])?;
        for (position, (label, area)) in areas.iter().enumerate() {
            tx.execute(
                "INSERT INTO research_work_areas (lab_id, label, area, position) VALUES (?, ?, ?, ?)",
                params![lab_id, label, area, position as i64],
            )?;
        }
        tx.commit()
    }

    /// RW8: the claims on a lab's items that haven't run out, oldest first.
    pub fn research_work_claims(
        &self,
        lab_id: &str,
        now: i64,
    ) -> rusqlite::Result<Vec<crate::research_work::Claim>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT number, claimer, agent_id IS NOT NULL, name, pairing, since, until
             FROM research_work_claims WHERE lab_id = ? AND until > ? ORDER BY since, claimer",
        )?;
        let rows = stmt.query_map(params![lab_id, now], |r| {
            Ok(crate::research_work::Claim {
                number: r.get::<_, i64>(0)? as u64,
                claimer: r.get(1)?,
                is_bot: r.get(2)?,
                name: r.get(3)?,
                pairing: r.get(4)?,
                since: r.get(5)?,
                until: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// RW8: claims or renews an item. Leading needs nobody else leading it; pairing joins
    /// whoever is. False, and nothing written, if someone else leads and this isn't pairing.
    #[allow(clippy::too_many_arguments)] // Explicit stored fields, like the other insert methods.
    pub fn claim_research_work(
        &self,
        lab_id: &str,
        number: u64,
        claimer: &str,
        person_id: &str,
        agent_id: Option<&str>,
        name: &str,
        pairing: bool,
        now: i64,
        until: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if !pairing {
            let taken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM research_work_claims WHERE lab_id = ?1 AND number = ?2
                   AND claimer <> ?3 AND pairing = 0 AND until > ?4)",
                params![lab_id, number as i64, claimer, now],
                |r| r.get(0),
            )?;
            if taken {
                return Ok(false);
            }
        }
        tx.execute(
            "INSERT INTO research_work_claims
                 (lab_id, number, claimer, person_id, agent_id, name, pairing, since, until)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (lab_id, number, claimer) DO UPDATE SET name = excluded.name,
                 pairing = excluded.pairing, until = excluded.until,
                 since = CASE WHEN research_work_claims.until > ?8
                              THEN research_work_claims.since ELSE excluded.since END",
            params![lab_id, number as i64, claimer, person_id, agent_id, name, pairing, now, until],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// RW8: drops this claimer's claim on an item. False if they had none.
    pub fn unclaim_research_work(
        &self,
        lab_id: &str,
        number: u64,
        claimer: &str,
    ) -> rusqlite::Result<bool> {
        let gone = self.conn().execute(
            "DELETE FROM research_work_claims WHERE lab_id = ? AND number = ? AND claimer = ?",
            params![lab_id, number as i64, claimer],
        )?;
        Ok(gone == 1)
    }

    /// Moves a lab's claims back in time, for tests of them running out (RW8).
    pub fn age_research_work_claims(&self, lab_id: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE research_work_claims SET since = since - ?2, until = until - ?2 WHERE lab_id = ?1",
            params![lab_id, by],
        )?;
        Ok(())
    }
    /// RW9: what GitHub last said about a pull request's CI and reviews, if it was ever read.
    pub fn research_work_pr_state(
        &self,
        repo: &str,
        number: u64,
    ) -> rusqlite::Result<Option<crate::research_work::PrState>> {
        self.conn()
            .query_row(
                "SELECT ci, review, checked_at, attempted_at, stale FROM research_work_pr_state
                 WHERE repo = ? AND number = ?",
                params![repo, number as i64],
                |r| {
                    Ok(crate::research_work::PrState {
                        ci: r.get(0)?,
                        review: r.get(1)?,
                        checked_at: r.get(2)?,
                        attempted_at: r.get(3)?,
                        stale: r.get(4)?,
                    })
                },
            )
            .optional()
    }

    /// RW9: claims the next read of a pull request's state, at most once every five minutes.
    pub fn reserve_research_work_pr_check(
        &self,
        repo: &str,
        number: u64,
        at: i64,
    ) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "INSERT INTO research_work_pr_state (repo, number, attempted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (repo, number) DO UPDATE SET attempted_at = excluded.attempted_at
             WHERE research_work_pr_state.attempted_at <= ?3 - 300",
            params![repo, number as i64, at],
        )? == 1)
    }

    /// RW9: stores what a read found, or marks the old state stale when it failed.
    pub fn finish_research_work_pr_check(
        &self,
        repo: &str,
        number: u64,
        found: Option<(&str, &str)>,
        at: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        match found {
            Some((ci, review)) => conn.execute(
                "UPDATE research_work_pr_state SET ci = ?3, review = ?4, checked_at = ?5, stale = 0
                 WHERE repo = ?1 AND number = ?2 AND attempted_at = ?5",
                params![repo, number as i64, ci, review, at],
            )?,
            None => conn.execute(
                "UPDATE research_work_pr_state SET stale = 1
                 WHERE repo = ?1 AND number = ?2 AND attempted_at = ?3",
                params![repo, number as i64, at],
            )?,
        };
        Ok(())
    }
    /// Moves a repo's last pull request reads back in time, for tests of the five-minute limit (RW9).
    pub fn age_research_work_pr_checks(&self, repo: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE research_work_pr_state SET attempted_at = attempted_at - ?2 WHERE repo = ?1",
            params![repo, by],
        )?;
        Ok(())
    }
}
