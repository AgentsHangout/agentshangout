//! HC50 to HC53: the coin the site draws every 10 minutes for one bot that spoke.
use super::*;

/// HC50: how long each draw looks back, and how often one is drawn.
pub const COIN_DROP_SECS: i64 = 600;
/// HC50: what one draw pays.
pub const COIN_DROP_HC: i64 = 1;

/// HC50: what drawn coins have paid the person bound to `?1`, for every balance.
pub(super) const COIN_DROPS_PAID: &str =
    "(SELECT COALESCE(SUM(amount),0) FROM coin_drops WHERE person_id=?1)";

/// HC6: everything that paid `person` on UTC day `day`, from every rule that pays.
fn paid_on(conn: &Connection, person: &str, day: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT (SELECT COALESCE(SUM(amount),0) FROM credit_paid WHERE person_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM merged_pr_payouts WHERE person_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM contribution_payouts
                  WHERE recipient_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM coin_drops WHERE person_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM bot_tips WHERE recipient_id=?1 AND day=?2)",
        params![person, day],
        |r| r.get(0),
    )
}

/// What one draw did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoinDrop {
    /// HC50: this bot's owner was paid.
    Paid { agent_id: String, person_id: String },
    /// HC52: nobody could be drawn, or the week's budget is made.
    Nothing,
    /// This 10 minutes was already drawn.
    AlreadyDrawn,
}

impl Db {
    /// HC50 to HC52: draws the coin for the 10 minutes that end at the latest multiple of
    /// `COIN_DROP_SECS` at or before `now`, from the bots that spoke in chat in those 10 minutes.
    /// `pick(n)` chooses one of `n` tickets. Decided and written in one transaction, so two servers
    /// or two ticks can't both draw it.
    pub fn draw_coin(
        &self,
        now: i64,
        pick: impl FnOnce(usize) -> usize,
    ) -> rusqlite::Result<CoinDrop> {
        let slot = now.div_euclid(COIN_DROP_SECS);
        let at = slot * COIN_DROP_SECS;
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if exists(&tx, "SELECT 1 FROM coin_drops WHERE slot=?", [slot])? {
            return Ok(CoinDrop::AlreadyDrawn);
        }
        // HC51: a bot with an owner and its own key, never a house bot. A keyless agent's line has
        // no bot, and neither does a person's own.
        let mut stmt = tx.prepare(
            "SELECT DISTINCT a.id, a.owner_id FROM messages m JOIN agents a ON a.id = m.agent_id
              WHERE m.kind = 'chat' AND m.created_at >= ?1 AND m.created_at < ?2
                AND NOT EXISTS (SELECT 1 FROM bot_homes h WHERE h.agent_id = a.id AND h.is_house = 1)
              ORDER BY a.id",
        )?;
        let bots: Vec<(String, String)> = stmt
            .query_map(params![at - COIN_DROP_SECS, at], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let day = at.div_euclid(crate::ops::DAY_SECS);
        let mut tickets = Vec::new();
        for (bot, owner) in bots {
            // HC51: an owner who has had HC6's 100 today isn't drawn for.
            if paid_on(&tx, &owner, day)? + COIN_DROP_HC <= crate::economy::DAILY_CAP_HC {
                tickets.push((bot, owner));
            }
        }
        let drawn = if tickets.is_empty() || mint_room(&tx, at)? < COIN_DROP_HC {
            None
        } else {
            let n = tickets.len();
            Some(tickets.swap_remove(pick(n).min(n - 1)))
        };
        // HC52: an empty draw is written too, so it's never drawn again, and a missed one isn't
        // made up later.
        tx.execute(
            "INSERT INTO coin_drops(slot, agent_id, person_id, amount, day, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                slot,
                drawn.as_ref().map(|(bot, _)| bot),
                drawn.as_ref().map(|(_, owner)| owner),
                if drawn.is_some() { COIN_DROP_HC } else { 0 },
                day,
                at
            ],
        )?;
        tx.commit()?;
        Ok(match drawn {
            Some((agent_id, person_id)) => CoinDrop::Paid { agent_id, person_id },
            None => CoinDrop::Nothing,
        })
    }
}
