//! HC36 to HC42: contributions a bot offers on a board card, and what the site pays when the card's
//! owner accepts one.
use super::*;

/// HC36: what a contribution can be.
pub const CONTRIBUTION_KINDS: [&str; 4] = ["review", "correction", "source", "summary"];
/// HC36: how many contributions one card takes, so it can't be buried.
const CONTRIBUTIONS_A_CARD: i64 = 20;
/// HC37: what an accepted contribution pays its bot's owner.
pub const CONTRIBUTION_HC: i64 = 5;
/// HC38: the most the whole site pays for contributions in 7 days, however many people there are.
pub const CONTRIBUTION_BUDGET_A_WEEK: i64 = 200;
/// HC39: how many accepted contributions pay one person in a UTC day.
pub const CONTRIBUTIONS_A_DAY: i64 = 3;
/// HC40: how many payouts there are between the same two people in 7 days, whichever way.
pub const PAIR_CONTRIBUTIONS_A_WEEK: i64 = 2;

/// HC37: what accepted contributions have paid the person bound to `?1`, for every balance.
pub(super) const CONTRIBUTIONS_PAID: &str =
    "(SELECT COALESCE(SUM(amount),0) FROM contribution_payouts WHERE recipient_id=?1)";

/// HC36: why `bot` can't offer a contribution on `card` now, if it can't.
fn offer_refusal(
    conn: &Connection,
    card: &str,
    bot: &str,
    owner: &str,
) -> rusqlite::Result<Option<&'static str>> {
    let card_owner: Option<String> = conn
        .query_row("SELECT person_id FROM board_cards WHERE id=?", [card], |r| r.get(0))
        .optional()?;
    let Some(card_owner) = card_owner else {
        return Ok(Some("that card is gone"));
    };
    if card_owner == owner {
        return Ok(Some("a card your owner wrote can't pay you for a contribution"));
    }
    if !exists(conn, "SELECT 1 FROM agents WHERE id=?1 AND owner_id=?2", params![bot, owner])? {
        return Ok(Some("bot is no longer available"));
    }
    if exists(conn, "SELECT 1 FROM contribution_payouts WHERE card_id=?", [card])? {
        return Ok(Some("this card has already accepted a contribution"));
    }
    if exists(
        conn,
        "SELECT 1 FROM card_contributions WHERE card_id=?1 AND bot_id=?2",
        params![card, bot],
    )? {
        return Ok(Some("this bot has already offered a contribution on this card"));
    }
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM card_contributions WHERE card_id=?", [card], |r| {
            r.get(0)
        })?;
    if count >= CONTRIBUTIONS_A_CARD {
        return Ok(Some("this card has 20 contributions"));
    }
    Ok(None)
}

/// HC37 to HC41: why accepting `contribution` on `card` wouldn't pay, if it wouldn't, or who it pays.
fn accept_refusal(
    conn: &Connection,
    card: &str,
    acceptor: &str,
    contribution: &str,
    now: i64,
) -> rusqlite::Result<Result<String, &'static str>> {
    let day = crate::ops::DAY_SECS;
    if !exists(
        conn,
        "SELECT 1 FROM board_cards WHERE id=?1 AND person_id=?2",
        params![card, acceptor],
    )? {
        return Ok(Err("only the card's owner can accept a contribution"));
    }
    if exists(conn, "SELECT 1 FROM contribution_payouts WHERE card_id=?", [card])? {
        return Ok(Err("this card has already accepted a contribution"));
    }
    if exists(conn, "SELECT 1 FROM contribution_payouts WHERE contribution_id=?", [contribution])? {
        return Ok(Err("that contribution has already been paid"));
    }
    // The bot still has to be its owner's, so a bot handed on can't pay its old owner.
    let recipient: Option<String> = conn
        .query_row(
            "SELECT c.owner_id FROM card_contributions c
               JOIN agents b ON b.id = c.bot_id AND b.owner_id = c.owner_id
              WHERE c.id = ?1 AND c.card_id = ?2 AND c.owner_id <> ?3",
            params![contribution, card, acceptor],
            |r| r.get(0),
        )
        .optional()?;
    let Some(recipient) = recipient else {
        return Ok(Err("that contribution is unavailable"));
    };
    // HC41: as HC29, accounts made for the purpose can't pay each other.
    let old_enough: i64 = conn.query_row(
        "SELECT COUNT(*) FROM people WHERE id IN (?1, ?2) AND created_at <= ?3",
        params![acceptor, recipient, now - super::COMMONS_ACCOUNT_DAYS * day],
        |r| r.get(0),
    )?;
    if old_enough < 2 {
        return Ok(Err("both accounts have to be at least 7 days old for a contribution to pay"));
    }
    // HC38: the site's budget, over the last 7 days.
    let week: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0) FROM contribution_payouts WHERE created_at > ?",
        [now - 7 * day],
        |r| r.get(0),
    )?;
    if week + CONTRIBUTION_HC > CONTRIBUTION_BUDGET_A_WEEK {
        return Ok(Err(
            "the site's budget for contributions is used up this week: try again later",
        ));
    }
    // HC39: a few a day for each person, and HC6's cap with everything else that paid that day.
    let today = now.div_euclid(day);
    let (count_today, paid_today): (i64, i64) = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM contribution_payouts WHERE recipient_id=?1 AND day=?2),
                (SELECT COALESCE(SUM(amount),0) FROM contribution_payouts
                  WHERE recipient_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM credit_paid WHERE person_id=?1 AND day=?2)
              + (SELECT COALESCE(SUM(amount),0) FROM coin_drops WHERE person_id=?1 AND day=?2)",
        params![recipient, today],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if count_today >= CONTRIBUTIONS_A_DAY {
        return Ok(Err("that contribution's owner has been paid for 3 contributions today"));
    }
    if paid_today + CONTRIBUTION_HC > crate::economy::DAILY_CAP_HC {
        return Ok(Err("that contribution's owner has been paid all a day can pay"));
    }
    // HC40: two people can't keep paying each other.
    let pair: i64 = conn.query_row(
        "SELECT COUNT(*) FROM contribution_payouts
          WHERE created_at > ?3
            AND ((acceptor_id=?1 AND recipient_id=?2) OR (acceptor_id=?2 AND recipient_id=?1))",
        params![acceptor, recipient, now - 7 * day],
        |r| r.get(0),
    )?;
    if pair >= PAIR_CONTRIBUTIONS_A_WEEK {
        return Ok(Err("you two have paid each other for 2 contributions this week"));
    }
    Ok(Ok(recipient))
}

impl Db {
    /// HC36: a check before anything is counted (K14); offering checks again in its transaction.
    pub fn contribution_refusal(
        &self,
        card: &str,
        bot: &str,
        owner: &str,
    ) -> rusqlite::Result<Option<&'static str>> {
        offer_refusal(&self.conn(), card, bot, owner)
    }

    /// HC36: `bot` offers a contribution on `card`. It pays nothing by itself (HC42).
    #[allow(clippy::too_many_arguments)]
    pub fn offer_contribution(
        &self,
        id: &str,
        card: &str,
        bot: &str,
        owner: &str,
        kind: &str,
        text: &str,
        now: i64,
    ) -> rusqlite::Result<Option<&'static str>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(reason) = offer_refusal(&tx, card, bot, owner)? {
            return Ok(Some(reason));
        }
        tx.execute(
            "INSERT INTO card_contributions(id,card_id,bot_id,owner_id,kind,text,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id, card, bot, owner, kind, text, now],
        )?;
        tx.commit()?;
        Ok(None)
    }

    /// Who `contribution` on `card` would pay, so what they're owed is worked out first (HC6).
    pub fn contribution_owner(
        &self,
        card: &str,
        contribution: &str,
    ) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT owner_id FROM card_contributions WHERE id=?1 AND card_id=?2",
                params![contribution, card],
                |r| r.get(0),
            )
            .optional()
    }

    /// HC37 to HC41: the card's owner accepts `contribution`, and the site pays its bot's owner.
    /// Every check and the payout are one immediate transaction, and the payout's keys allow one
    /// per card and one per contribution, so a retry or two at once pay once.
    pub fn accept_contribution(
        &self,
        card: &str,
        acceptor: &str,
        contribution: &str,
        now: i64,
    ) -> rusqlite::Result<Option<&'static str>> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let recipient = match accept_refusal(&tx, card, acceptor, contribution, now)? {
            Ok(recipient) => recipient,
            Err(reason) => return Ok(Some(reason)),
        };
        // HC44: the site's payout is new credits, so it comes out of this week's budget too.
        if super::mint_room(&tx, now)? < CONTRIBUTION_HC {
            return Ok(Some("the site has made all its credits for this week"));
        }
        tx.execute(
            "INSERT INTO contribution_payouts
                 (card_id,contribution_id,acceptor_id,recipient_id,amount,day,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                card,
                contribution,
                acceptor,
                recipient,
                CONTRIBUTION_HC,
                now.div_euclid(crate::ops::DAY_SECS),
                now
            ],
        )?;
        tx.commit()?;
        Ok(None)
    }

    /// HC36, HC43: the contributions on `card`, oldest first, and which one was accepted.
    pub fn card_contributions(&self, card: &str) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.kind, c.text, c.owner_id, COALESCE(b.name,'Bot who left'),
                    COALESCE(p.name,'Someone who left'),
                    EXISTS (SELECT 1 FROM contribution_payouts x WHERE x.contribution_id = c.id)
               FROM card_contributions c
               LEFT JOIN agents b ON b.id = c.bot_id
               LEFT JOIN people p ON p.id = c.owner_id
              WHERE c.card_id = ?
              ORDER BY c.created_at, c.id",
        )?;
        let rows = stmt.query_map([card], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "kind": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
                "owner_id": r.get::<_, String>(3)?,
                "bot_name": r.get::<_, String>(4)?,
                "owner_name": r.get::<_, String>(5)?,
                "accepted": r.get::<_, bool>(6)?,
            }))
        })?;
        rows.collect()
    }

    /// HC37: whether `card` has already paid for a contribution.
    pub fn contribution_paid_on(&self, card: &str) -> rusqlite::Result<bool> {
        exists(&self.conn(), "SELECT 1 FROM contribution_payouts WHERE card_id=?", [card])
    }

    /// Moves when an account was made back in time, for tests of HC41.
    pub fn age_person(&self, person_id: &str, by: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE people SET created_at = created_at - ?2 WHERE id = ?1",
            params![person_id, by],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = crate::ops::DAY_SECS;
    /// Every account below is 30 days old by then, past HC41.
    const NOW: i64 = 30 * DAY;

    fn card(db: &Db, id: &str, owner: &str) {
        db.conn()
            .execute(
                "INSERT INTO board_cards(id,board_id,title,body,person_id,created_at,updated_at)
                 VALUES(?1,'board','Card','',?2,0,0)",
                params![id, owner],
            )
            .unwrap();
    }

    fn person(db: &Db, id: &str, made_at: i64) {
        db.create_person(id, id, &format!("{id}-key"), &format!("{id}-feed"), made_at).unwrap();
        db.create_agent(&format!("b-{id}"), id, id, &format!("t-{id}"), 0).unwrap();
    }

    /// Four people, p1 to p4, each with a bot `b-p1` and so on, and cards `p1-0` to `p4-7`.
    fn fixture() -> Db {
        let db = Db::open(":memory:").unwrap();
        for id in ["p1", "p2", "p3", "p4"] {
            person(&db, id, 0);
        }
        db.create_hangout("h1", "Lab", "code1", "p1", 0).unwrap();
        db.conn()
            .execute(
                "INSERT INTO boards(id,hangout_id,kind,name,created_by,created_at)
                 VALUES('board','h1','discussion','Notes','p1',0)",
                [],
            )
            .unwrap();
        for owner in ["p1", "p2", "p3", "p4"] {
            for n in 0..8 {
                card(&db, &format!("{owner}-{n}"), owner);
            }
        }
        db
    }

    /// The bot of `owner` offers a source on `card`.
    fn offer(db: &Db, card: &str, owner: &str) -> String {
        let id = format!("{card}-{owner}");
        let bot = format!("b-{owner}");
        assert_eq!(
            db.offer_contribution(&id, card, &bot, owner, "source", "a paper", NOW).unwrap(),
            None
        );
        id
    }

    fn paid(db: &Db, person: &str) -> i64 {
        db.credit_totals(person, NOW.div_euclid(DAY)).unwrap().paid
    }

    fn payouts(db: &Db) -> i64 {
        db.conn().query_row("SELECT COUNT(*) FROM contribution_payouts", [], |r| r.get(0)).unwrap()
    }

    // HC36: a bot offers one per card, only for its own owner, never on its owner's card, and a
    // card that paid takes no more. Offering pays nothing (HC42).
    #[test]
    fn a_bot_offers_one_contribution_a_card_and_never_on_its_owners() {
        let db = fixture();
        let offered = |id: &str, card: &str, bot: &str, owner: &str| {
            db.offer_contribution(id, card, bot, owner, "review", "words", NOW).unwrap()
        };
        assert_eq!(
            offered("own", "p2-0", "b-p2", "p2"),
            Some("a card your owner wrote can't pay you for a contribution")
        );
        let id = offer(&db, "p1-0", "p2");
        assert_eq!(
            offered("again", "p1-0", "b-p2", "p2"),
            Some("this bot has already offered a contribution on this card")
        );
        assert_eq!(offered("stolen", "p1-0", "b-p3", "p2"), Some("bot is no longer available"));
        assert_eq!(paid(&db, "p2"), 0, "offering pays nothing");
        assert_eq!(db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(), None);
        assert_eq!(
            offered("late", "p1-0", "b-p3", "p3"),
            Some("this card has already accepted a contribution")
        );
    }

    // HC37: only the card's owner accepts, the site pays the bot's owner, and the card's owner
    // spends nothing.
    #[test]
    fn the_cards_owner_accepts_and_the_site_pays_the_bots_owner() {
        let db = fixture();
        let id = offer(&db, "p1-0", "p2");
        for not_owner in ["p2", "p3"] {
            assert_eq!(
                db.accept_contribution("p1-0", not_owner, &id, NOW).unwrap(),
                Some("only the card's owner can accept a contribution")
            );
        }
        assert_eq!(db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(), None);
        assert_eq!(paid(&db, "p2"), CONTRIBUTION_HC);
        let ann = db.credit_totals("p1", 0).unwrap();
        assert_eq!((ann.paid, ann.spent), (0, 0), "the site paid, not Ann");
        // HC43: the history line names no card, person or bot.
        let history = db.credit_history("p2", 50).unwrap();
        let line = &history[0];
        assert_eq!((line.kind.as_str(), line.amount, line.at), ("contribution", 5, NOW));
        assert!(line.city.is_none() && line.pack.is_none() && line.bot.is_none());
    }

    // HC37: a retry pays once, and so do acceptances at once, of one contribution or of two on
    // the same card.
    #[test]
    fn a_retry_or_two_acceptances_at_once_pay_once() {
        let db = std::sync::Arc::new(fixture());
        let id = offer(&db, "p1-0", "p2");
        let other = offer(&db, "p1-0", "p3");
        assert_eq!(db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(), None);
        for retry in [&id, &other] {
            assert_eq!(
                db.accept_contribution("p1-0", "p1", retry, NOW + 1).unwrap(),
                Some("this card has already accepted a contribution")
            );
        }
        assert_eq!((paid(&db, "p2"), paid(&db, "p3"), payouts(&db)), (5, 0, 1));

        let first = offer(&db, "p1-1", "p2");
        let second = offer(&db, "p1-1", "p3");
        let handles: Vec<_> = [first.clone(), first, second]
            .into_iter()
            .map(|id| {
                let db = db.clone();
                std::thread::spawn(move || db.accept_contribution("p1-1", "p1", &id, NOW).unwrap())
            })
            .collect();
        let accepted =
            handles.into_iter().map(|h| h.join().unwrap()).filter(Option::is_none).count();
        assert_eq!(accepted, 1);
        assert_eq!(payouts(&db), 2);
        assert_eq!(paid(&db, "p2") + paid(&db, "p3"), 10);
    }

    // HC38: the whole site pays at most so much a week, and the week moves on.
    #[test]
    fn the_site_pays_only_so_much_for_contributions_a_week() {
        let db = fixture();
        for n in 0..CONTRIBUTION_BUDGET_A_WEEK / CONTRIBUTION_HC {
            db.conn()
                .execute(
                    "INSERT INTO contribution_payouts
                         (card_id,contribution_id,acceptor_id,recipient_id,amount,day,created_at)
                     VALUES(?1,?1,NULL,NULL,?2,0,?3)",
                    params![format!("old-{n}"), CONTRIBUTION_HC, NOW - DAY],
                )
                .unwrap();
        }
        let id = offer(&db, "p1-0", "p2");
        assert_eq!(
            db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(),
            Some("the site's budget for contributions is used up this week: try again later")
        );
        assert_eq!(paid(&db, "p2"), 0, "nothing is written");
        assert_eq!(db.accept_contribution("p1-0", "p1", &id, NOW + 6 * DAY + 1).unwrap(), None);
    }

    // HC39: a few a day for each person, and HC6's cap with everything else that paid that day.
    #[test]
    fn a_person_is_paid_for_three_a_day_within_the_days_cap() {
        let db = fixture();
        // From different people, so HC40 doesn't stop it first.
        for card in ["p1-0", "p3-0", "p1-1"] {
            let id = offer(&db, card, "p2");
            assert_eq!(db.accept_contribution(card, &card[..2], &id, NOW).unwrap(), None);
        }
        let id = offer(&db, "p4-0", "p2");
        assert_eq!(
            db.accept_contribution("p4-0", "p4", &id, NOW).unwrap(),
            Some("that contribution's owner has been paid for 3 contributions today")
        );
        assert_eq!(db.accept_contribution("p4-0", "p4", &id, NOW + DAY).unwrap(), None);

        // Cy has had 96 today from a gathering: 5 more would pass HC6's 100, so it's refused, not
        // paid in part.
        let today = NOW.div_euclid(DAY);
        db.conn()
            .execute(
                "INSERT INTO credit_paid(person_id,kind,source_id,city_id,amount,day,created_at)
                 VALUES('p3','gathering','g','c1',96,?1,?2)",
                params![today, NOW],
            )
            .unwrap();
        let id = offer(&db, "p1-2", "p3");
        assert_eq!(
            db.accept_contribution("p1-2", "p1", &id, NOW).unwrap(),
            Some("that contribution's owner has been paid all a day can pay")
        );
        // Paid the next day, it counts toward that day's 100 for whatever settles after it.
        assert_eq!(db.accept_contribution("p1-2", "p1", &id, NOW + DAY).unwrap(), None);
        let source =
            CreditSource { kind: CreditKind::Gathering, source: "next".into(), at: NOW + DAY };
        db.settle_credits("p3", &[("c1".into(), source)], |_| 100, 100, NOW + DAY).unwrap();
        assert_eq!(paid(&db, "p3"), 96 + 5 + 95);
    }

    // HC40: the same two people pay each other for two a week, whichever way.
    #[test]
    fn two_people_pay_each_other_for_two_contributions_a_week() {
        let db = fixture();
        for card in ["p1-0", "p1-1"] {
            let id = offer(&db, card, "p2");
            assert_eq!(db.accept_contribution(card, "p1", &id, NOW).unwrap(), None);
        }
        let back = offer(&db, "p2-0", "p1");
        assert_eq!(
            db.accept_contribution("p2-0", "p2", &back, NOW + DAY).unwrap(),
            Some("you two have paid each other for 2 contributions this week")
        );
        let id = offer(&db, "p3-0", "p2");
        assert_eq!(db.accept_contribution("p3-0", "p3", &id, NOW).unwrap(), None, "Cy isn't Ann");
        assert_eq!(db.accept_contribution("p2-0", "p2", &back, NOW + 7 * DAY + 1).unwrap(), None);
    }

    // HC41: both accounts are at least 7 days old when it's accepted.
    #[test]
    fn accounts_under_a_week_old_neither_pay_nor_are_paid() {
        let db = fixture();
        for id in ["y1", "y2"] {
            person(&db, id, NOW - 7 * DAY + 1);
        }
        card(&db, "y1-0", "y1");
        let refused =
            Some("both accounts have to be at least 7 days old for a contribution to pay");
        let to_young = offer(&db, "p1-0", "y2");
        assert_eq!(db.accept_contribution("p1-0", "p1", &to_young, NOW).unwrap(), refused);
        let from_young = offer(&db, "y1-0", "p2");
        assert_eq!(db.accept_contribution("y1-0", "y1", &from_young, NOW).unwrap(), refused);
        assert_eq!(db.accept_contribution("y1-0", "y1", &from_young, NOW + 1).unwrap(), None);
    }

    // HC42: what a contribution paid buys packs but never funds a question, and paid stays paid
    // when the card or either account goes, so nothing pays twice.
    #[test]
    fn contribution_credits_never_fund_a_question_and_paid_stays_paid() {
        let db = fixture();
        let id = offer(&db, "p1-0", "p2");
        assert_eq!(db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(), None);
        assert_eq!(
            db.fund_question("q", "p2-0", "p2", 5, NOW).unwrap(),
            Some(
                "credits from a first gathering, a starter question, a contribution, a merged pull request or a drawn coin \
                 buy packs but can't fund a question"
            )
        );
        assert!(matches!(
            db.spend_credits("p2", "a-pack", 5, None, NOW).unwrap(),
            Spend::Spent { .. }
        ));

        db.conn().execute("DELETE FROM board_cards WHERE id='p1-0'", []).unwrap();
        db.delete_person("p2", "Bo").unwrap();
        let row: (Option<String>, Option<String>) = db
            .conn()
            .query_row(
                "SELECT acceptor_id, recipient_id FROM contribution_payouts WHERE card_id='p1-0'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (Some("p1".into()), None), "the row stays, without Bo");
        let left: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM card_contributions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "the contribution's text went with the card and its owner");
        card(&db, "p1-0", "p1");
        assert_eq!(
            db.accept_contribution("p1-0", "p1", &id, NOW).unwrap(),
            Some("this card has already accepted a contribution")
        );
    }
}
