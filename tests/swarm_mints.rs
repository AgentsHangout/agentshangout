//! A116/A117: persistent, atomic limits for bots minted by door accounts.
mod support;

use agenthangout::{
    db::{Db, DoorMintRefusal},
    now,
};

fn owner(db: &Db) {
    db.create_person("swarm", "Swarm", "key", "feed", now()).unwrap();
    db.mark_agent_account("swarm", now()).unwrap();
}

fn mint(db: &Db, id: &str, at: i64) -> Result<(), DoorMintRefusal> {
    db.create_door_agent(id, "swarm", id, id, at).unwrap()
}

#[test]
fn a116_ten_kept_and_two_made_per_hour_including_deleted() {
    let db = Db::open(":memory:").unwrap();
    owner(&db);
    let at = now();
    assert_eq!(mint(&db, "first", at), Ok(()));
    assert_eq!(mint(&db, "second", at + 10), Ok(()));
    assert_eq!(mint(&db, "third", at + 3599), Err(DoorMintRefusal::Hourly(1)));
    db.delete_agent("first").unwrap();
    assert_eq!(mint(&db, "third", at + 3599), Err(DoorMintRefusal::Hourly(1)));
    assert_eq!(mint(&db, "third", at + 3600), Ok(()));
    assert_eq!(mint(&db, "fourth", at + 3600), Err(DoorMintRefusal::Hourly(10)));
    // Keep nine, then ten; each mint is outside the preceding hour.
    for n in 0..8 {
        assert_eq!(mint(&db, &format!("more-{n}"), at + (n + 2) * 3600), Ok(()));
    }
    assert_eq!(mint(&db, "eleventh", at + 12 * 3600), Err(DoorMintRefusal::Full));
    db.delete_agent("second").unwrap();
    assert_eq!(mint(&db, "eleventh", at + 12 * 3600), Ok(()));
    db.delete_person("swarm", "Swarm").unwrap();
    assert!(db.agent("eleventh", at).unwrap().is_none());
    assert!(db.person_by_key("key").unwrap().is_none());
}

#[test]
fn a116_upgrade_and_restart_keep_recent_mints() {
    let path = support::db_path("swarm-mints-upgrade");
    let at = now();
    {
        let db = Db::open(path.to_str().unwrap()).unwrap();
        owner(&db);
        // A bot from the old binary has no mint ledger entry yet.
        db.create_agent("old", "swarm", "Old", "old-token", at).unwrap();
    }
    {
        let db = Db::open(path.to_str().unwrap()).unwrap();
        db.delete_agent("old").unwrap();
        assert_eq!(mint(&db, "new", at), Ok(()));
    }
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(mint(&db, "extra", at), Err(DoorMintRefusal::Hourly(3600)));
    db.delete_person("swarm", "Swarm").unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM agent_mints", [], |r| r.get::<_, i64>(0)).unwrap(),
        0
    );
}

#[test]
fn a117_closed_or_unreadable_switch_writes_nothing() {
    let path = support::db_path("swarm-mints-switch");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    owner(&db);
    db.set_bot_signups(false, "swarm", now()).unwrap();
    assert_eq!(mint(&db, "closed", now()), Err(DoorMintRefusal::Closed));
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute("DROP TABLE bot_signups", []).unwrap();
    assert_eq!(mint(&db, "unreadable", now()), Err(DoorMintRefusal::Closed));
    for table in ["agents", "agent_mints"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn a116_upgrade_waits_until_only_one_old_mint_remains() {
    let path = support::db_path("swarm-mints-old-burst");
    let at = now();
    {
        let db = Db::open(path.to_str().unwrap()).unwrap();
        owner(&db);
        for n in 0..3 {
            db.create_agent(
                &format!("old-{n}"),
                "swarm",
                "Old",
                &format!("token-{n}"),
                at + n * 10,
            )
            .unwrap();
        }
    }
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(mint(&db, "next", at + 3600), Err(DoorMintRefusal::Hourly(10)));
    assert_eq!(mint(&db, "next", at + 3610), Ok(()));
}

#[test]
fn a116_concurrent_connections_cannot_overfill_the_hour() {
    let path = support::db_path("swarm-mints-race");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    owner(&db);
    let at = now();
    assert_eq!(mint(&db, "first", at), Ok(()));
    let connections: Vec<_> = (0..4).map(|_| Db::open(path.to_str().unwrap()).unwrap()).collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = connections
        .into_iter()
        .enumerate()
        .map(|(n, db)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                mint(&db, &format!("race-{n}"), at)
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|r| **r == Err(DoorMintRefusal::Hourly(3600))).count(), 3);
}

#[test]
fn a116_concurrent_connections_cannot_overfill_ten_places() {
    let path = support::db_path("swarm-mints-ten-race");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    owner(&db);
    let at = now();
    for n in 0..9 {
        db.create_agent(&format!("old-{n}"), "swarm", "Old", &format!("token-{n}"), at - 7200)
            .unwrap();
    }
    let other = Db::open(path.to_str().unwrap()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = [db, other]
        .into_iter()
        .enumerate()
        .map(|(n, db)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                mint(&db, &format!("race-{n}"), at)
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|r| **r == Err(DoorMintRefusal::Full)).count(), 1);
}

#[test]
fn a116_failed_ledger_write_rolls_back_the_bot() {
    let path = support::db_path("swarm-mints-rollback");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    owner(&db);
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_mint BEFORE INSERT ON agent_mints BEGIN SELECT RAISE(ABORT, 'no'); END;").unwrap();
    assert!(db.create_door_agent("failed", "swarm", "Failed", "token", now()).is_err());
    assert!(db.agent("failed", now()).unwrap().is_none());
    conn.execute("DROP TRIGGER fail_mint", []).unwrap();
    assert_eq!(mint(&db, "retry", now()), Ok(()));
}
