//! I162 (#964): the join funnel's counts. Each person or bot's first arrival is kept with its entry
//! path and UTC day; the steps after it are read from the messages that already exist.

use super::*;

/// One row of the funnel: arrivals for one path and kind, and how far they got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunnelRow {
    pub path: String,
    pub kind: String,
    /// Arrivals in the window.
    pub arrivals: i64,
    /// Arrivals with a chat line of their own at any time since.
    pub said: i64,
    /// Arrivals whose chat line got a reply from someone else at any time since.
    pub replied: i64,
    /// Arrivals old enough that their 7 days after arrival are all over.
    pub eligible_for_return: i64,
    /// Of those, arrivals with a chat line on one of the 7 days after their arrival day.
    pub returned_within_7_days: i64,
}

impl Db {
    /// I162: records an actor's first arrival. A second call changes nothing, so the first path
    /// is the one that counts.
    pub fn note_arrival(
        &self,
        actor_id: &str,
        kind: &str,
        path: &str,
        at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO arrivals (actor_id, kind, path, day, at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![actor_id, kind, path, at.div_euclid(86_400), at],
        )?;
        Ok(())
    }

    /// I162: the funnel for arrivals on or after `since_day`, grouped by path and kind. `today` is
    /// the current UTC day, so a cohort counts for return only once its 7 days are complete.
    pub fn funnel(&self, since_day: i64, today: i64) -> rusqlite::Result<Vec<FunnelRow>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT a.path, a.kind, COUNT(*),
                    SUM(EXISTS (
                        SELECT 1 FROM messages m
                        WHERE (m.agent_id IS a.actor_id OR (m.agent_id IS NULL AND m.person_id IS a.actor_id))
                          AND m.kind = 'chat')),
                    SUM(EXISTS (
                        SELECT 1 FROM message_replies r
                        JOIN messages t ON t.id = r.reply_to
                        JOIN messages m ON m.id = r.message_id
                        WHERE (t.agent_id IS a.actor_id OR (t.agent_id IS NULL AND t.person_id IS a.actor_id))
                          AND t.kind = 'chat'
                          AND NOT (m.agent_id IS a.actor_id OR (m.agent_id IS NULL AND m.person_id IS a.actor_id)))),
                    SUM(a.day + 8 <= ?2),
                    SUM(a.day + 8 <= ?2 AND EXISTS (
                        SELECT 1 FROM messages m
                        WHERE (m.agent_id IS a.actor_id OR (m.agent_id IS NULL AND m.person_id IS a.actor_id))
                          AND m.kind = 'chat'
                          AND m.created_at >= (a.day + 1) * 86400
                          AND m.created_at < (a.day + 8) * 86400))
             FROM arrivals a
             WHERE a.day >= ?1
             GROUP BY a.path, a.kind
             ORDER BY a.path, a.kind",
        )?;
        let rows = statement.query_map(params![since_day, today], |row| {
            Ok(FunnelRow {
                path: row.get(0)?,
                kind: row.get(1)?,
                arrivals: row.get(2)?,
                said: row.get(3)?,
                replied: row.get(4)?,
                eligible_for_return: row.get(5)?,
                returned_within_7_days: row.get(6)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn db() -> Db {
        let db = Db::open(":memory:").unwrap();
        db.create_person("p", "Pat", "k", "f", 0).unwrap();
        db.create_agent("b", "p", "Bot", "t", 0).unwrap();
        db.create_person("q", "Quin", "k2", "f2", 0).unwrap();
        db.create_hangout("h", "Room", "code", "p", 0).unwrap();
        db
    }

    fn chat(db: &Db, person: Option<&str>, agent: Option<&str>, at: i64) -> i64 {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO messages (hangout_id, person_id, agent_id, speaker, kind, body, created_at)
             VALUES ('h', ?1, ?2, 'x', 'chat', 'hi', ?3)",
            params![person, agent, at],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn first_arrival_wins_and_steps_are_read_from_messages() {
        let db = db();
        // Pat arrives by the door on day 10 and Quin walks in on the same day.
        db.note_arrival("p", "person", "door", 10 * DAY + 5).unwrap();
        db.note_arrival("p", "person", "web_signup", 10 * DAY + 9).unwrap();
        db.note_arrival("q", "person", "walk_in", 10 * DAY).unwrap();
        db.note_arrival("b", "bot", "door", 10 * DAY).unwrap();

        // Pat says something on day 10, and Quin replies to it on day 12.
        let said = chat(&db, Some("p"), None, 10 * DAY + 100);
        let reply_id = chat(&db, Some("q"), None, 12 * DAY);
        db.conn()
            .execute(
                "INSERT INTO message_replies (message_id, reply_to) VALUES (?1, ?2)",
                params![reply_id, said],
            )
            .unwrap();
        // Pat is back on day 14. Quin's own line doesn't count as Pat's reply.
        chat(&db, Some("p"), None, 14 * DAY + 60);

        let rows = db.funnel(0, 19).unwrap();
        let find = |path: &str, kind: &str| {
            rows.iter().find(|r| r.path == path && r.kind == kind).cloned().unwrap()
        };
        let door_people = find("door", "person");
        assert_eq!(door_people.arrivals, 1);
        assert_eq!(door_people.said, 1);
        assert_eq!(door_people.replied, 1);
        assert_eq!(door_people.eligible_for_return, 1);
        assert_eq!(door_people.returned_within_7_days, 1);
        // The web signup was recorded second, so it never appears.
        assert!(rows.iter().all(|r| r.path != "web_signup"));

        let walk = find("walk_in", "person");
        // Quin wrote the reply, so she said something; nobody replied to her.
        assert_eq!((walk.arrivals, walk.said, walk.replied), (1, 1, 0));
        // Her line on day 12 is inside the 7 days after she arrived on day 10.
        assert_eq!(walk.returned_within_7_days, 1);
        assert_eq!(find("door", "bot").arrivals, 1);
    }

    #[test]
    fn a_bots_line_is_not_its_owners() {
        let db = db();
        db.note_arrival("p", "person", "door", 10 * DAY).unwrap();
        db.note_arrival("b", "bot", "door", 10 * DAY).unwrap();
        // Only the bot speaks: its line carries Pat's person_id, but Pat hasn't said anything.
        chat(&db, Some("p"), Some("b"), 10 * DAY + 60);
        let rows = db.funnel(0, 30).unwrap();
        let find = |kind: &str| rows.iter().find(|r| r.kind == kind).cloned().unwrap();
        assert_eq!((find("person").said, find("bot").said), (0, 1));
    }

    #[test]
    fn a_cohort_counts_for_return_only_after_its_seven_days() {
        let db = db();
        db.note_arrival("p", "person", "door", 10 * DAY).unwrap();
        chat(&db, Some("p"), None, 11 * DAY + 1);
        // Day 17 is the last of its 7 days, so day 18 is the first day it's eligible; on day 17
        // the window isn't over yet.
        let early = db.funnel(0, 17).unwrap();
        assert_eq!((early[0].eligible_for_return, early[0].returned_within_7_days), (0, 0));
        let later = db.funnel(0, 18).unwrap();
        assert_eq!((later[0].eligible_for_return, later[0].returned_within_7_days), (1, 1));
    }

    #[test]
    fn a_line_on_the_arrival_day_or_after_seven_days_is_not_a_return() {
        let db = db();
        db.note_arrival("p", "person", "door", 10 * DAY).unwrap();
        chat(&db, Some("p"), None, 10 * DAY + 3600);
        chat(&db, Some("p"), None, 18 * DAY + 1);
        let rows = db.funnel(0, 30).unwrap();
        assert_eq!(rows[0].said, 1);
        assert_eq!(rows[0].returned_within_7_days, 0);
    }

    #[test]
    fn arrivals_before_the_window_are_left_out() {
        let db = db();
        db.note_arrival("p", "person", "door", 3 * DAY).unwrap();
        db.note_arrival("q", "person", "door", 9 * DAY).unwrap();
        let rows = db.funnel(5, 12).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].arrivals, 1);
    }

    #[test]
    fn the_new_paths_are_their_own_rows_and_the_first_one_wins() {
        let db = db();
        // A bot that came in through the door and later joins the map keeps its door arrival.
        db.note_arrival("b", "bot", "door", 10 * DAY).unwrap();
        db.note_arrival("b", "bot", "map_join", 11 * DAY).unwrap();
        db.note_arrival("c", "bot", "owner_made", 10 * DAY).unwrap();
        db.note_arrival("d", "bot", "mcp_setup", 10 * DAY).unwrap();
        db.note_arrival("e", "person", "oauth", 10 * DAY).unwrap();
        let rows = db.funnel(0, 20).unwrap();
        let count = |path: &str, kind: &str| {
            rows.iter().find(|r| r.path == path && r.kind == kind).map(|r| r.arrivals).unwrap_or(0)
        };
        assert_eq!(count("door", "bot"), 1);
        assert_eq!(count("map_join", "bot"), 0);
        assert_eq!(count("owner_made", "bot"), 1);
        assert_eq!(count("mcp_setup", "bot"), 1);
        assert_eq!(count("oauth", "person"), 1);
    }

    #[test]
    fn an_anonymous_map_join_is_a_guest_arrival_and_its_steps_stay_empty() {
        let db = db();
        db.note_arrival("player:7", "guest", "map_join", 10 * DAY).unwrap();
        db.note_arrival("player:7", "guest", "map_join", 11 * DAY).unwrap();
        db.note_arrival("player:8", "guest", "map_join", 10 * DAY + 3).unwrap();
        let rows = db.funnel(0, 20).unwrap();
        let guest = rows.iter().find(|r| r.kind == "guest").unwrap();
        assert_eq!(guest.path, "map_join");
        assert_eq!(guest.arrivals, 2);
        // Guests can't write chat lines, so nothing reads as said, replied or returned.
        assert_eq!((guest.said, guest.replied, guest.returned_within_7_days), (0, 0, 0));
    }

    #[test]
    fn an_arrivals_table_from_before_guests_is_widened_and_keeps_its_rows() {
        let path = std::env::temp_dir().join(format!("croakie-964-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_str().unwrap();
        {
            let conn = Connection::open(path_str).unwrap();
            conn.execute_batch(
                "CREATE TABLE arrivals (
                    actor_id TEXT PRIMARY KEY,
                    kind     TEXT NOT NULL CHECK (kind IN ('person', 'bot')),
                    path     TEXT NOT NULL,
                    day      INTEGER NOT NULL,
                    at       INTEGER NOT NULL
                 );
                 CREATE INDEX arrivals_by_day ON arrivals(day);
                 INSERT INTO arrivals VALUES ('old', 'bot', 'door', 10, 864000);",
            )
            .unwrap();
        }
        // Opening it runs the real migration: the old row stays and guests can be counted.
        let db = Db::open(path_str).unwrap();
        db.note_arrival("player:1", "guest", "map_join", 11 * DAY).unwrap();
        let rows = db.funnel(0, 20).unwrap();
        let count =
            |kind: &str| rows.iter().filter(|r| r.kind == kind).map(|r| r.arrivals).sum::<i64>();
        assert_eq!((count("bot"), count("guest")), (1, 1));
        drop(db);
        // Opening it again leaves the wider table as it is.
        let db = Db::open(path_str).unwrap();
        assert_eq!(db.funnel(0, 20).unwrap().iter().map(|r| r.arrivals).sum::<i64>(), 2);
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
