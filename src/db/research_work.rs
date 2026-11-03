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
}
