//! Hangout Credits. The rules are HC1 to HC19, HC22 and HC24 in docs/economy.md.
//!
//! What could pay is worked out from plans and trips, the same way standing and a city's growth
//! are. Once something pays it's written to `credit_paid` and kept (HC11, HC12), and spending on a
//! pack is written to `credit_spends` (HC15). No table holds a balance: it's what was paid less what
//! was spent, worked out on read (HC1).

use serde::Serialize;

use crate::{
    auth::Caller,
    db::{CreditKind, CreditLine, CreditSource, Db, Spend},
    error::{ApiError, ApiResult},
    items::{kind_of, pack_of, PACKS},
    ops::DAY_SECS,
};

/// HC2: a gathering pays each person who said yes.
pub const GATHERING_HC: i64 = 20;
/// HC3 and HC4: a good trip pays the visiting owner and the resident who let the bot in.
pub const GOOD_TRIP_HC: i64 = 5;
/// HC6: the most anyone is credited for things that happened on one UTC day.
pub const DAILY_CAP_HC: i64 = 100;
/// HC5: how long someone has been in a city before it pays them anything.
pub const GATE_MEMBER_DAYS: i64 = 30;
/// HC22: how many lines of history `credits` returns.
pub const HISTORY_LINES: i64 = 50;
/// HC17: how long an owner's yes through `ask_owner` lets their bot spend.
pub const ASKED_FOR_SECS: i64 = DAY_SECS;

/// What a person has, worked out now. Only they and their bots see it (HC8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Credits {
    pub balance: i64,
    /// The caller's open question reservations, for cancellation even after leaving a room.
    pub open_questions: Vec<serde_json::Value>,
    /// Credited for things that happened today (UTC), at most `DAILY_CAP_HC`.
    pub earned_today: i64,
    pub daily_cap: i64,
    /// Everything ever spent (HC15).
    pub spent: i64,
    /// How many of each thing paid at least in part, after the gate and the cap.
    pub gatherings: i64,
    pub good_trips: i64,
    pub hosted_trips: i64,
    /// Cities they're in that don't pay them anything new yet (HC5, HC13), by id.
    pub cities_not_earning: Vec<String>,
    /// HC14: every pack that costs credits, and whether they have it.
    pub packs: Vec<PackOffer>,
    /// HC17: their bots, and how much each may spend a day.
    pub bots: Vec<BotLimit>,
    /// HC22: the latest things that paid them and that they spent, newest first.
    pub history: Vec<CreditLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackOffer {
    pub pack: &'static str,
    pub cost: i64,
    pub bought: bool,
    /// Its season has passed, so it can't be bought (I57).
    pub over: bool,
    /// HC24: what it looks like, so the shop can show it before it's bought.
    pub paint: &'static str,
    pub flooring: &'static str,
    pub pieces: Vec<PackPiece>,
}

/// HC24: one piece of a pack, where the pack puts it, and how much floor it takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PackPiece {
    pub kind: &'static str,
    pub label: &'static str,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BotLimit {
    pub agent_id: String,
    pub name: String,
    pub daily: i64,
    pub spent_today: i64,
}

fn worth(kind: CreditKind) -> i64 {
    match kind {
        CreditKind::Gathering => GATHERING_HC,
        CreditKind::GoodTrip | CreditKind::HostedTrip => GOOD_TRIP_HC,
    }
}

/// HC5 and HC13: when a city started paying this person, if it has. Only what happened from then
/// on pays, so the 30 days can't be skipped by collecting for them afterwards.
fn city_pays_from(
    db: &Db,
    city_id: &str,
    person_id: &str,
    now: i64,
) -> rusqlite::Result<Option<i64>> {
    db.credit_gate_met_at(city_id, person_id, GATE_MEMBER_DAYS * DAY_SECS, now)
}

/// Works out a person's credits now (HC1 to HC7, HC11 to HC15). Anything new that pays is written
/// down first, so what's paid stays paid (HC12).
pub fn credits(db: &Db, person_id: &str, now: i64) -> rusqlite::Result<Credits> {
    let mut out = Credits { daily_cap: DAILY_CAP_HC, ..Credits::default() };
    let mut sources: Vec<(String, CreditSource)> = Vec::new();
    for city in db.cities_for_person(person_id)? {
        if let Some(from) = city_pays_from(db, &city.id, person_id, now)? {
            let paying = db.credit_sources(&city.id, person_id, now)?;
            sources.extend(
                paying
                    .into_iter()
                    .filter(|source| source.at >= from)
                    .map(|source| (city.id.clone(), source)),
            );
        } else {
            out.cities_not_earning.push(city.id);
        }
    }
    db.settle_credits(person_id, &sources, worth, DAILY_CAP_HC, now)?;
    let totals = db.credit_totals(person_id, now.div_euclid(DAY_SECS))?;
    out.balance = totals.paid - totals.spent;
    out.open_questions = db.open_questions(person_id)?;
    out.earned_today = totals.paid_today;
    out.spent = totals.spent;
    out.gatherings = totals.gatherings;
    out.good_trips = totals.good_trips;
    out.hosted_trips = totals.hosted_trips;
    let bought = db.bought_packs(person_id)?;
    out.packs = PACKS
        .iter()
        .filter(|pack| pack.cost > 0)
        .map(|pack| PackOffer {
            pack: pack.pack,
            cost: pack.cost,
            bought: bought.iter().any(|word| word == pack.pack),
            over: pack.over,
            paint: pack.paint,
            flooring: pack.flooring,
            pieces: pack
                .items
                .iter()
                .filter_map(|item| {
                    let kind = kind_of(item.kind)?;
                    Some(PackPiece {
                        kind: item.kind,
                        label: kind.label,
                        x: item.x,
                        y: item.y,
                        w: kind.w,
                        h: kind.h,
                    })
                })
                .collect(),
        })
        .collect();
    out.bots = db
        .spend_limits(person_id, now)?
        .into_iter()
        .map(|(agent_id, name, daily, spent_today)| BotLimit { agent_id, name, daily, spent_today })
        .collect();
    out.history = db.credit_history(person_id, HISTORY_LINES)?;
    Ok(out)
}

/// `GET /api/credits` and the `credits` tool: the caller's own credits, or their owner's for a
/// bot (HC8). A house bot belongs to a house, not a person, so it has none.
pub fn credits_of(db: &Db, caller: &Caller, now: i64) -> ApiResult<Credits> {
    if caller.is_house {
        return Err(ApiError::not_found());
    }
    Ok(credits(db, &caller.person_id, now)?)
}

/// HC14 to HC17: buys a pack that costs credits. A person buys for themselves. A bot buys for its
/// owner, only with `asked`, the id of an `ask_owner` question it asked about this pack in the last
/// day that its owner answered yes to, and only within the daily limit its owner set.
pub fn spend(
    db: &Db,
    caller: &Caller,
    word: &str,
    asked: Option<&str>,
    now: i64,
) -> ApiResult<Credits> {
    if caller.is_house {
        return Err(ApiError::not_found());
    }
    let pack = pack_of(word)
        .filter(|pack| pack.cost > 0)
        .ok_or_else(|| ApiError::bad_request("that isn't a pack you can buy: see credits"))?;
    if pack.over {
        return Err(ApiError::conflict("that pack's season is over, so it can't be bought"));
    }
    if let Some(agent_id) = &caller.agent_id {
        owner_said_yes(db, caller, agent_id, pack.pack, asked, now)?;
    }
    // What's paid but not written down yet counts, so work it out first.
    credits(db, &caller.person_id, now)?;
    match db.spend_credits(
        &caller.person_id,
        pack.pack,
        pack.cost,
        caller.agent_id.as_deref(),
        now,
    )? {
        Spend::Spent { .. } => Ok(credits(db, &caller.person_id, now)?),
        Spend::AlreadyHave => Err(ApiError::conflict("you already have that pack")),
        Spend::TooFew { balance } => Err(ApiError::conflict(format!(
            "that pack costs {} credits and the balance is {balance}",
            pack.cost
        ))),
        Spend::OverBotLimit { left_today } => Err(ApiError::forbidden(format!(
            "that's more than this bot may spend today ({left_today} left). Its owner sets the \
             limit on their Your credits card"
        ))),
    }
}

/// HC17: a bot spends only on its owner's own yes. A check-in, a room message or a note never
/// counts, because only `ask_owner` reaches the owner and nobody else can answer it.
fn owner_said_yes(
    db: &Db,
    caller: &Caller,
    agent_id: &str,
    pack: &str,
    asked: Option<&str>,
    now: i64,
) -> ApiResult<()> {
    let refused = || {
        ApiError::forbidden(format!(
            "a bot spends its owner's credits only on their yes. Ask with ask_owner, naming the \
             {pack} pack, then pass that question's id as asked once they say yes"
        ))
    };
    let question = db.owner_question(asked.ok_or_else(refused)?)?.ok_or_else(refused)?;
    let yes = question.answer.as_deref().is_some_and(said_yes);
    let recent = question.answered_at.is_some_and(|at| now - at <= ASKED_FOR_SECS);
    let about_it = question.question.to_lowercase().contains(pack);
    if question.agent_id != agent_id
        || question.owner_id != caller.person_id
        || !yes
        || !recent
        || !about_it
    {
        return Err(refused());
    }
    Ok(())
}

/// Whether an answer reads as a yes. Anything unclear is a no, so a bot has to ask again.
fn said_yes(answer: &str) -> bool {
    let first =
        answer.trim().split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase();
    matches!(first.as_str(), "yes" | "y" | "yeah" | "yep" | "ok" | "okay" | "sure" | "buy")
}

/// HC17: an owner sets how much one of their bots may spend a day.
pub fn set_bot_limit(
    db: &Db,
    caller: &Caller,
    agent_id: &str,
    daily: i64,
    now: i64,
) -> ApiResult<Credits> {
    if caller.is_agent() {
        return Err(ApiError::forbidden("only a bot's owner sets what it may spend"));
    }
    if !(0..=1_000).contains(&daily) {
        return Err(ApiError::bad_request("a daily limit is from 0 to 1000 credits"));
    }
    if !db.set_spend_limit(&caller.person_id, agent_id, daily)? {
        return Err(ApiError::not_found());
    }
    Ok(credits(db, &caller.person_id, now)?)
}

#[cfg(test)]
mod tests {
    use super::said_yes;

    #[test]
    fn only_a_plain_yes_lets_a_bot_spend() {
        for yes in ["Yes", "yes, buy it", "OK", "y", "Sure thing", "buy"] {
            assert!(said_yes(yes), "{yes}");
        }
        for no in ["no", "not yet", "hold", "", "maybe yes", "nope", "yesterday"] {
            assert!(!said_yes(no), "{no}");
        }
    }
}
