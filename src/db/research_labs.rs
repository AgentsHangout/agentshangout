use super::*;
use crate::research_labs::{Lab, NewLab};

fn lab_from_row(row: &Row) -> rusqlite::Result<Lab> {
    let id: String = row.get(0)?;
    let github = format!("{}/{}", row.get::<_, String>(4)?, row.get::<_, String>(5)?);
    Ok(Lab {
        floor_id: id.clone(),
        id: id.clone(),
        lab_id: id.clone(),
        project_id: id,
        name: row.get(1)?,
        purpose: row.get(2)?,
        owner_id: row.get(3)?,
        github_url: format!("https://github.com/{github}"),
        github,
        created_at: row.get(6)?,
    })
}
const LAB_SELECT: &str = "SELECT l.space_id,s.name,s.about,s.owner_id,l.github_owner,l.github_repo,l.created_at FROM research_labs l JOIN spaces s ON s.id=l.space_id JOIN floors f ON f.hangout_id=l.space_id";

impl Db {
    /// RL7: standing public lab doors only. Plot records alone can outlive a building.
    pub fn mapped_research_labs(
        &self,
        world_id: &str,
    ) -> rusqlite::Result<Vec<(i64, i64, String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT d.x,d.y,l.github_owner,l.github_repo FROM research_labs l
             JOIN spaces s ON s.id=l.space_id JOIN floors f ON f.hangout_id=l.space_id
             JOIN space_plots d ON d.space_id=l.space_id
             JOIN world_every_place b ON b.world_id=d.world_id AND b.x=d.x AND b.y=d.y
             JOIN players p ON p.id=b.player_id
             WHERE d.world_id=? AND f.access='public' AND s.kind='third'
               AND p.person_id=s.owner_id AND b.kind!='house' ORDER BY d.y,d.x",
        )?;
        let rows = stmt.query_map([world_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect()
    }

    pub fn person_created_at(&self, person: &str) -> rusqlite::Result<Option<i64>> {
        self.conn()
            .query_row("SELECT created_at FROM people WHERE id=?", [person], |r| r.get(0))
            .optional()
    }
    pub fn research_lab(&self, id: &str) -> rusqlite::Result<Option<Lab>> {
        self.conn()
            .query_row(&format!("{LAB_SELECT} WHERE l.space_id=?"), [id], lab_from_row)
            .optional()
    }
    pub fn research_labs(&self) -> rusqlite::Result<Vec<Lab>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{LAB_SELECT} WHERE f.access='public' ORDER BY l.created_at,l.space_id LIMIT 200"
        ))?;
        let rows = stmt.query_map([], lab_from_row)?;
        rows.collect()
    }
    /// Eligibility, quota and provisioning share one transaction: no partial room on refusal.
    pub fn create_research_lab(&self, new: &NewLab<'_>) -> rusqlite::Result<Option<&'static str>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if !new.god_mode {
            let age: i64 =
                tx.query_row("SELECT created_at FROM people WHERE id=?", [new.owner_id], |r| {
                    r.get(0)
                })?;
            let friends: i64 = tx.query_row(
                &format!("SELECT COUNT(*) FROM people WHERE id!=?1 AND id IN {FRIENDS_OF}"),
                [new.owner_id],
                |r| r.get(0),
            )?;
            if new.at.saturating_sub(age) < crate::research_labs::MIN_AGE_SECS || friends < 30 {
                return Ok(Some("eligibility"));
            }
            let held: i64 = tx.query_row(
                &format!(
                    "SELECT COUNT(*) FROM {HELD_SPACES} WHERE s.owner_id=? AND s.kind='third'"
                ),
                [new.owner_id],
                |r| r.get(0),
            )?;
            if held >= crate::spaces::MAX_THIRD_SPACES {
                return Ok(Some("capacity"));
            }
        }
        let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM research_labs WHERE github_owner=?1 COLLATE NOCASE AND github_repo=?2 COLLATE NOCASE)", params![new.github_owner,new.github_repo], |r|r.get(0))?;
        if duplicate {
            return Ok(Some("repository"));
        }
        insert_space(&tx, new.id, "third", new.name, new.invite_code, new.owner_id, new.at)?;
        tx.execute("UPDATE spaces SET about=? WHERE id=?", params![new.purpose, new.id])?;
        tx.execute("UPDATE floors SET access='public' WHERE hangout_id=?", [new.id])?;
        tx.execute(
            "INSERT INTO projects(space_id,agent_id,created_at) VALUES(?,?,?)",
            params![new.id, new.agent_id, new.at],
        )?;
        if let Some(agent) = new.agent_id {
            tx.execute(
                "INSERT INTO room_agents(hangout_id,agent_id) VALUES(?,?)",
                params![new.id, agent],
            )?;
        }
        tx.execute(
            "INSERT INTO research_labs VALUES(?,?,?,?)",
            params![new.id, new.github_owner, new.github_repo, new.at],
        )?;
        for (kind, name) in [
            ("discussion", "Research discussion"),
            ("jobs", "Research tasks"),
            ("issues", "GitHub issues"),
        ] {
            let linked = kind == "issues";
            tx.execute("INSERT INTO boards(id,hangout_id,kind,name,github_owner,github_repo,created_by,created_at) VALUES(?,?,?,?,?,?,?,?)",params![crate::auth::new_id(),new.id,kind,name,linked.then_some(new.github_owner),linked.then_some(new.github_repo),new.owner_id,new.at])?;
        }
        tx.commit()?;
        Ok(None)
    }
}
