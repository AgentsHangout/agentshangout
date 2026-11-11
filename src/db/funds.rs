//! HC59 to HC64 (#1056): a floor pools credits for one pack or one coin piece.
//!
//! A put-in is written to `credit_spends` as `fund:<put id>` in the same transaction, so the
//! credits are held (and an older binary still subtracts them) and a second put can't spend them
//! again. Completing leaves those rows, which is the spend, and records the floor's unlock.
//! Refunding deletes them, so every put-in comes back exactly. The sweep is idempotent: a fund
//! stops being `open` the first time it is refunded.

use rusqlite::{params, OptionalExtension, Transaction};

use super::{exists, Connection, Db};

/// HC59: how long a fund stays open.
pub const FUND_DAYS: i64 = 30;
/// HC59: how many open funds one person may have started.
pub const OPEN_FUNDS: i64 = 3;
/// The `credit_spends.pack` prefix for a put-in. The rest is the put's id, so one person can put
/// in more than once (the spend table is unique per person and pack).
pub const FUND_SPEND: &str = "fund:";

/// The floor, the item, the status and who started it. Enough to route a put or a cancel
/// without listing anyone's contribution.
pub struct FundHead {
    pub hangout_id: String,
    pub item: String,
    pub status: String,
    pub starter_id: Option<String>,
}

/// One fund, as its floor shows it: the total and the target, plus what this person put in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundView {
    pub id: String,
    pub hangout_id: String,
    pub item: String,
    pub target: i64,
    pub total: i64,
    pub yours: i64,
    /// Whether the person reading it started it, so only they're offered Cancel (HC62).
    pub started_by_you: bool,
    pub status: String,
    pub ends_at: i64,
    pub created_at: i64,
}

/// HC59: a fund a member is starting.
pub struct NewFund<'a> {
    pub id: &'a str,
    pub hangout_id: &'a str,
    pub starter_id: &'a str,
    pub item: &'a str,
    pub target: i64,
    /// Pack names whose unlock already covers `item`, when `item` is a piece those packs place.
    pub covered_by: &'a [&'a str],
    pub now: i64,
}

/// HC60: credits going into an open fund.
pub struct NewPut<'a> {
    pub id: &'a str,
    pub fund_id: &'a str,
    pub person_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub amount: i64,
    /// The `ask_owner` question id, when a bot is putting in. One id pays for one put.
    pub asked: Option<&'a str>,
    pub now: i64,
}

/// What a put-in did. Refusals write nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PutIn {
    Taken {
        taken: i64,
        fund: FundView,
    },
    /// No fund with that id.
    Missing,
    /// Complete or already refunded, so nothing more goes in.
    NotOpen,
    /// Still open, but its end has passed. The sweep refunds it.
    Ended,
    TooFew {
        balance: i64,
    },
    OverBotLimit {
        left_today: i64,
    },
    /// Clamped to what's left, and nothing was left.
    NothingLeft,
    /// HC60: this `asked` id already paid for a put.
    YesUsed,
}

fn is_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

/// The same balance `spend_credits` reads, so held credits (a first gathering, a merged pull
/// request, a drawn coin) can go in, and credits already held by a put cannot go in twice.
fn balance_of(conn: &Connection, person_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT {}", Db::balance_sql("?1")), [person_id], |row| row.get(0))
}

fn load_view(conn: &Connection, id: &str, person_id: &str) -> rusqlite::Result<Option<FundView>> {
    conn.query_row(
        "SELECT f.id, f.hangout_id, f.item, f.target, f.status, f.ends_at, f.created_at,
                COALESCE((SELECT SUM(amount) FROM fund_puts WHERE fund_id = f.id), 0),
                COALESCE((SELECT SUM(amount) FROM fund_puts
                           WHERE fund_id = f.id AND person_id = ?2), 0),
                COALESCE(f.starter_id = ?2, 0)
           FROM funds f WHERE f.id = ?1",
        params![id, person_id],
        |row| {
            Ok(FundView {
                id: row.get(0)?,
                hangout_id: row.get(1)?,
                item: row.get(2)?,
                target: row.get(3)?,
                status: row.get(4)?,
                ends_at: row.get(5)?,
                created_at: row.get(6)?,
                total: row.get(7)?,
                yours: row.get(8)?,
                started_by_you: row.get(9)?,
            })
        },
    )
    .optional()
}

/// HC62: mark one open fund refunded and give every put-in back. False if it was not open.
fn refund_open(tx: &Transaction, id: &str, now: i64) -> rusqlite::Result<bool> {
    let changed = tx.execute(
        "UPDATE funds SET status = 'refunded', settled_at = ?1
          WHERE id = ?2 AND status = 'open'",
        params![now, id],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    tx.execute(
        "DELETE FROM credit_spends WHERE pack IN
            (SELECT ?1 || id FROM fund_puts WHERE fund_id = ?2)",
        params![FUND_SPEND, id],
    )?;
    Ok(true)
}

impl Db {
    /// HC59: starts a fund. The checks and the insert are one immediate transaction.
    pub fn start_fund(&self, fund: &NewFund) -> rusqlite::Result<Result<FundView, &'static str>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if exists(
            &tx,
            "SELECT 1 FROM floor_unlocks WHERE hangout_id = ?1 AND item = ?2",
            params![fund.hangout_id, fund.item],
        )? {
            return Ok(Err("this floor already has that"));
        }
        for pack in fund.covered_by {
            if exists(
                &tx,
                "SELECT 1 FROM floor_unlocks WHERE hangout_id = ?1 AND item = ?2",
                params![fund.hangout_id, pack],
            )? {
                return Ok(Err("this floor already has that"));
            }
        }
        if exists(
            &tx,
            "SELECT 1 FROM funds WHERE hangout_id = ?1 AND item = ?2 AND status = 'open'",
            params![fund.hangout_id, fund.item],
        )? {
            return Ok(Err("this floor already has an open fund for that"));
        }
        let open: i64 = tx.query_row(
            "SELECT COUNT(*) FROM funds WHERE starter_id = ?1 AND status = 'open'",
            [fund.starter_id],
            |row| row.get(0),
        )?;
        if open >= OPEN_FUNDS {
            return Ok(Err("you have 3 open funds; cancel or finish one first"));
        }
        let ends_at = fund.now + FUND_DAYS * crate::ops::DAY_SECS;
        if let Err(error) = tx.execute(
            "INSERT INTO funds (id, hangout_id, starter_id, item, target, status, created_at, ends_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7)",
            params![
                fund.id,
                fund.hangout_id,
                fund.starter_id,
                fund.item,
                fund.target,
                fund.now,
                ends_at
            ],
        ) {
            if is_constraint(&error) {
                return Ok(Err("this floor already has an open fund for that"));
            }
            return Err(error);
        }
        let view = load_view(&tx, fund.id, fund.starter_id)?.expect("the fund was just inserted");
        tx.commit()?;
        Ok(Ok(view))
    }

    /// HC60 and HC61: puts credits in, clamped to what's left, and completes the fund in this same
    /// transaction when the target is reached.
    pub fn put_in_fund(&self, put: &NewPut) -> rusqlite::Result<PutIn> {
        if put.amount < 1 {
            return Ok(PutIn::NothingLeft);
        }
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let found: Option<(String, String, i64, String, i64)> = tx
            .query_row(
                "SELECT hangout_id, item, target, status, ends_at FROM funds WHERE id = ?1",
                [put.fund_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let Some((hangout, item, target, status, ends_at)) = found else {
            return Ok(PutIn::Missing);
        };
        if status != "open" {
            return Ok(PutIn::NotOpen);
        }
        if ends_at <= put.now {
            return Ok(PutIn::Ended);
        }
        let raised: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM fund_puts WHERE fund_id = ?1",
            [put.fund_id],
            |row| row.get(0),
        )?;
        let left = target - raised;
        if left < 1 {
            return Ok(PutIn::NothingLeft);
        }
        // HC60: one yes, one put. Checked in this transaction, with the unique index behind it.
        if let Some(asked) = put.asked {
            if exists(&tx, "SELECT 1 FROM fund_puts WHERE asked = ?1", [asked])? {
                return Ok(PutIn::YesUsed);
            }
        }
        // HC61: never more than what's left, so two puts at once can't overfill or complete twice.
        let taken = put.amount.min(left);
        let balance = balance_of(&tx, put.person_id)?;
        if balance < taken {
            return Ok(PutIn::TooFew { balance });
        }
        if let Some(agent_id) = put.agent_id {
            let day_start = put.now.div_euclid(crate::ops::DAY_SECS) * crate::ops::DAY_SECS;
            let (limit, spent_today): (i64, i64) = tx.query_row(
                "SELECT COALESCE((SELECT daily FROM spend_limits WHERE agent_id = ?1), 0),
                        (SELECT COALESCE(SUM(amount), 0) FROM credit_spends
                          WHERE agent_id = ?1 AND created_at >= ?2)",
                params![agent_id, day_start],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if spent_today + taken > limit {
                return Ok(PutIn::OverBotLimit { left_today: (limit - spent_today).max(0) });
            }
        }
        if let Err(error) = tx.execute(
            "INSERT INTO fund_puts (id, fund_id, person_id, agent_id, amount, asked, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![put.id, put.fund_id, put.person_id, put.agent_id, taken, put.asked, put.now],
        ) {
            // The check above and this index agree. A race that passes the check still stops here.
            if is_constraint(&error) {
                if let Some(asked) = put.asked {
                    if exists(&tx, "SELECT 1 FROM fund_puts WHERE asked = ?1", [asked])? {
                        return Ok(PutIn::YesUsed);
                    }
                }
            }
            return Err(error);
        }
        tx.execute(
            "INSERT INTO credit_spends (person_id, pack, amount, agent_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![put.person_id, format!("{FUND_SPEND}{}", put.id), taken, put.agent_id, put.now],
        )?;
        if taken == left {
            let completed = tx.execute(
                "UPDATE funds SET status = 'complete', settled_at = ?1
                  WHERE id = ?2 AND status = 'open'",
                params![put.now, put.fund_id],
            )?;
            if completed == 1 {
                tx.execute(
                    "INSERT INTO floor_unlocks (hangout_id, item, fund_id, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![hangout, item, put.fund_id, put.now],
                )?;
            }
        }
        let view = load_view(&tx, put.fund_id, put.person_id)?.expect("the fund is still there");
        tx.commit()?;
        Ok(PutIn::Taken { taken, fund: view })
    }

    /// HC62: the starter cancels an open fund and every put-in comes back. Refused once it has
    /// completed, or if this person didn't start it.
    pub fn cancel_fund(
        &self,
        id: &str,
        starter_id: &str,
        now: i64,
    ) -> rusqlite::Result<Result<FundView, &'static str>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let found: Option<(String, Option<String>)> = tx
            .query_row("SELECT status, starter_id FROM funds WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        let Some((status, starter)) = found else {
            return Ok(Err("no such fund"));
        };
        if starter.as_deref() != Some(starter_id) {
            return Ok(Err("you didn't start this fund"));
        }
        if status == "complete" {
            return Ok(Err("this fund is already complete"));
        }
        if status != "open" {
            return Ok(Err("this fund is already refunded"));
        }
        if !refund_open(&tx, id, now)? {
            return Ok(Err("this fund is already complete"));
        }
        let view = load_view(&tx, id, starter_id)?.expect("the fund is still there");
        tx.commit()?;
        Ok(Ok(view))
    }

    /// HC62: refunds every open fund whose end has passed. A second run finds none still open, so
    /// it refunds nothing extra. This is what the coin-drop ticker calls, whether or not anyone
    /// has the floor open.
    pub fn expire_funds(&self, now: i64) -> rusqlite::Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM funds WHERE status = 'open' AND ends_at <= ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([now], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut refunded = 0;
        for id in ids {
            if refund_open(&tx, &id, now)? {
                refunded += 1;
            }
        }
        tx.commit()?;
        Ok(refunded)
    }

    /// HC63: the floor's funds, each with its total and what `person_id` put in. No other person's
    /// put-in is in here.
    pub fn funds_on(&self, hangout_id: &str, person_id: &str) -> rusqlite::Result<Vec<FundView>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT f.id, f.hangout_id, f.item, f.target, f.status, f.ends_at, f.created_at,
                    COALESCE((SELECT SUM(amount) FROM fund_puts WHERE fund_id = f.id), 0),
                    COALESCE((SELECT SUM(amount) FROM fund_puts
                               WHERE fund_id = f.id AND person_id = ?2), 0),
                    COALESCE(f.starter_id = ?2, 0)
               FROM funds f
              WHERE f.hangout_id = ?1
              ORDER BY CASE f.status WHEN 'open' THEN 0 ELSE 1 END, f.created_at, f.id",
        )?;
        let rows = stmt.query_map(params![hangout_id, person_id], |row| {
            Ok(FundView {
                id: row.get(0)?,
                hangout_id: row.get(1)?,
                item: row.get(2)?,
                target: row.get(3)?,
                status: row.get(4)?,
                ends_at: row.get(5)?,
                created_at: row.get(6)?,
                total: row.get(7)?,
                yours: row.get(8)?,
                started_by_you: row.get(9)?,
            })
        })?;
        rows.collect()
    }

    /// The floor, the item, the status and who started it.
    pub fn fund_row(&self, id: &str) -> rusqlite::Result<Option<FundHead>> {
        self.conn()
            .query_row(
                "SELECT hangout_id, item, status, starter_id FROM funds WHERE id = ?1",
                [id],
                |row| {
                    Ok(FundHead {
                        hangout_id: row.get(0)?,
                        item: row.get(1)?,
                        status: row.get(2)?,
                        starter_id: row.get(3)?,
                    })
                },
            )
            .optional()
    }

    /// Whether this floor has `item` unlocked, by that exact key (`winter`, or `piece:snowman`).
    pub fn has_floor_unlock(&self, hangout_id: &str, item: &str) -> rusqlite::Result<bool> {
        exists(
            &self.conn(),
            "SELECT 1 FROM floor_unlocks WHERE hangout_id = ?1 AND item = ?2",
            params![hangout_id, item],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worth20(_: crate::db::CreditKind) -> i64 {
        20
    }

    fn pay(db: &Db, person: &str, source: &str, at: i64) {
        db.settle_credits(
            person,
            &[(
                "seed".into(),
                crate::db::CreditSource {
                    kind: crate::db::CreditKind::Gathering,
                    source: source.into(),
                    at,
                },
            )],
            worth20,
            100,
            at,
        )
        .unwrap();
    }

    // HC62: a put after the end, before anyone opens the floor, is refused, and the ticker's
    // sweep gives the credits back once. A second sweep adds nothing.
    #[test]
    fn a_fund_past_its_end_refunds_on_the_sweep_and_only_once() {
        let db = Db::open(":memory:").unwrap();
        let at = 1_700_000_000;
        db.create_person("p1", "Ann", "hash", "feed", at).unwrap();
        db.create_hangout("room", "Room", "code", "p1", at).unwrap();
        pay(&db, "p1", "seed", at);
        let fund = db
            .start_fund(&NewFund {
                id: "fund-1",
                hangout_id: "room",
                starter_id: "p1",
                item: "winter",
                target: 15,
                covered_by: &[],
                now: at,
            })
            .unwrap()
            .unwrap();
        assert_eq!(fund.ends_at - fund.created_at, FUND_DAYS * crate::ops::DAY_SECS);
        assert!(matches!(
            db.put_in_fund(&NewPut {
                id: "put-1",
                fund_id: "fund-1",
                person_id: "p1",
                agent_id: None,
                amount: 6,
                asked: None,
                now: at,
            })
            .unwrap(),
            PutIn::Taken { taken: 6, .. }
        ));
        db.conn().execute("UPDATE funds SET ends_at = ?1 WHERE id = 'fund-1'", [at]).unwrap();
        let balance_held = crate::economy::credits(&db, "p1", at).unwrap().balance;
        assert_eq!(balance_held, 14);
        assert_eq!(
            db.put_in_fund(&NewPut {
                id: "put-2",
                fund_id: "fund-1",
                person_id: "p1",
                agent_id: None,
                amount: 9,
                asked: None,
                now: at,
            })
            .unwrap(),
            PutIn::Ended
        );
        assert_eq!(crate::economy::credits(&db, "p1", at).unwrap().balance, balance_held);
        // Nobody has to open the floor. The sweep is the ticker's call.
        assert_eq!(db.expire_funds(at).unwrap(), 1);
        assert_eq!(crate::economy::credits(&db, "p1", at).unwrap().balance, 20);
        assert_eq!(db.expire_funds(at).unwrap(), 0);
        assert_eq!(crate::economy::credits(&db, "p1", at + 5).unwrap().balance, 20);
    }
}
