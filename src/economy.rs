//! Hangout Credits, phase 1: earning only. The rules are HC1 to HC10 in docs/economy.md.
//!
//! Credits are worked out on read from plans and trips (HC1), the same way standing and a city's
//! growth are, so nothing here is stored, nothing can drift from the facts and there's no table to
//! edit. Phase 1 has no spending (HC9), so there's nothing yet that a stored ledger would be for.

use serde::Serialize;

use crate::{
    auth::Caller,
    db::{CreditKind, Db},
    error::{ApiError, ApiResult},
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

/// What a person has earned, worked out now. Only they and their bots see it (HC8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Credits {
    pub balance: i64,
    /// Credited for things that happened today (UTC), at most `DAILY_CAP_HC`.
    pub earned_today: i64,
    pub daily_cap: i64,
    /// How many of each thing paid at least in part, after the gate and the cap.
    pub gatherings: i64,
    pub good_trips: i64,
    pub hosted_trips: i64,
    /// Cities they're in that don't pay them yet (HC5), by id.
    pub cities_not_earning: Vec<String>,
}

/// HC5: when a city started paying this person, if it has. Only what happened from then on pays,
/// so the 30 days can't be skipped by collecting for them afterwards.
fn city_pays_from(
    db: &Db,
    city_id: &str,
    person_id: &str,
    now: i64,
) -> rusqlite::Result<Option<i64>> {
    db.credit_gate_met_at(city_id, person_id, GATE_MEMBER_DAYS * DAY_SECS, now)
}

/// Works out a person's credits now (HC1 to HC7).
pub fn credits(db: &Db, person_id: &str, now: i64) -> rusqlite::Result<Credits> {
    let mut out = Credits { daily_cap: DAILY_CAP_HC, ..Credits::default() };
    let mut sources = Vec::new();
    for city in db.cities_for_person(person_id)? {
        if let Some(from) = city_pays_from(db, &city.id, person_id, now)? {
            let paying = db.credit_sources(&city.id, person_id, now)?;
            sources.extend(paying.into_iter().filter(|source| source.at >= from));
        } else {
            out.cities_not_earning.push(city.id);
        }
    }
    // Earliest first across every city, so the cap (HC6) always keeps the same things.
    sources.sort_by(|a, b| (a.at, a.kind, &a.source).cmp(&(b.at, b.kind, &b.source)));
    let today = now.div_euclid(DAY_SECS);
    let (mut day, mut paid_that_day) = (i64::MIN, 0);
    for source in sources {
        let source_day = source.at.div_euclid(DAY_SECS);
        if source_day != day {
            (day, paid_that_day) = (source_day, 0);
        }
        let worth = match source.kind {
            CreditKind::Gathering => GATHERING_HC,
            CreditKind::GoodTrip | CreditKind::HostedTrip => GOOD_TRIP_HC,
        };
        let paid = worth.min(DAILY_CAP_HC - paid_that_day);
        if paid <= 0 {
            continue;
        }
        paid_that_day += paid;
        out.balance += paid;
        if source_day == today {
            out.earned_today += paid;
        }
        match source.kind {
            CreditKind::Gathering => out.gatherings += 1,
            CreditKind::GoodTrip => out.good_trips += 1,
            CreditKind::HostedTrip => out.hosted_trips += 1,
        }
    }
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
