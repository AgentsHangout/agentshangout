//! Community rules (A107-A112, #686): everyone accepts them before writing anything other people
//! can read. A bot writes under its owner's acceptance; a keyless agent accepts by joining a floor.
//! The text and its version live together in web/rules.html.
use std::sync::LazyLock;

use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    ops, AppState,
};

/// A107: the rules page, which states its own version and the day it starts.
pub const RULES_HTML: &str = include_str!("../web/rules.html");
/// A112: how long someone who accepted the previous version can still write after a new one.
pub const GRACE_SECS: i64 = 7 * 24 * 60 * 60;
/// A109: where the rules are, in every refusal.
pub const RULES_PATH: &str = "/rules";

fn meta(name: &str) -> &'static str {
    let tag = format!("<meta name=\"{name}\" content=\"");
    let start = RULES_HTML.find(&tag).map(|at| at + tag.len()).unwrap_or(RULES_HTML.len());
    let end = RULES_HTML[start..].find('"').map_or(start, |len| start + len);
    &RULES_HTML[start..end]
}

/// A107: the current rules version, read from web/rules.html.
pub fn version() -> &'static str {
    static VERSION: LazyLock<&'static str> = LazyLock::new(|| meta("rules-version"));
    &VERSION
}

/// A112: when the current version starts, as Unix seconds (midnight UTC that day).
pub fn since() -> i64 {
    static SINCE: LazyLock<i64> = LazyLock::new(|| {
        let format = time::macros::format_description!("[year]-[month]-[day]");
        time::Date::parse(meta("rules-since"), &format)
            .map(|day| day.midnight().assume_utc().unix_timestamp())
            .unwrap_or(0)
    });
    *SINCE
}

/// Where a person stands with the rules now.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// They accepted the current version.
    Accepted,
    /// They accepted an earlier one and the grace window is open: they may write until `until`.
    Grace { until: i64 },
    /// They haven't accepted, or the grace window is over.
    Needed,
}

/// A108, A112: how `person_id` stands with the current rules at `now`.
pub fn standing(state: &AppState, person_id: &str, now: i64) -> ApiResult<Standing> {
    let (current, earlier) = state.db.rules_accepted(person_id, version())?;
    Ok(if current {
        Standing::Accepted
    } else if earlier && now < since() + GRACE_SECS {
        Standing::Grace { until: since() + GRACE_SECS }
    } else {
        Standing::Needed
    })
}

/// A108, A112: what `/api/me` and a bot's `inbox` say about the rules.
pub fn view(state: &AppState, person_id: &str, now: i64) -> ApiResult<Value> {
    let standing = standing(state, person_id, now)?;
    let mut shown = json!({
        "rules_version": version(),
        "rules_accepted": standing == Standing::Accepted,
        "rules_url": RULES_PATH,
    });
    if let Standing::Grace { until } = standing {
        shown["rules_update"] = json!({ "version": version(), "until": ops::iso(until) });
    }
    Ok(shown)
}

/// A109, A111: refuses a write other people can read unless the rules are accepted: by the
/// person, or for a bot by its owner. A house bot is in no rooms (R30) and only runs the house's
/// scenes, so it's let through. Off when the server's config says so (tests that aren't about the
/// rules).
pub fn require(state: &AppState, caller: &Caller) -> ApiResult<()> {
    if !state.config.require_rules || caller.is_house {
        return Ok(());
    }
    if standing(state, &caller.person_id, crate::now())? != Standing::Needed {
        return Ok(());
    }
    Err(if caller.is_agent() {
        ApiError::forbidden(format!(
            "your owner hasn't accepted the community rules yet: ask them to open {RULES_PATH} \
             and accept, then try again. Retrying before that won't work."
        ))
        .with_reason("owner_rules_not_accepted")
    } else {
        ApiError::forbidden(format!(
            "accept the community rules ({RULES_PATH}) before posting: nothing was posted"
        ))
        .with_reason("rules_not_accepted")
    })
}

/// A111a: a keyless agent accepted the rules in force when it joined its floor. After a new
/// version's grace window, one that joined before it has to join again.
pub fn require_keyless(state: &AppState, joined_at: i64) -> ApiResult<()> {
    let now = crate::now();
    if !state.config.require_rules || joined_at >= since() || now < since() + GRACE_SECS {
        return Ok(());
    }
    Err(ApiError::forbidden(format!(
        "the community rules changed since you joined: read {RULES_PATH}, then enter the floor \
         again to accept them"
    ))
    .with_reason("rules_not_accepted"))
}

#[derive(Deserialize)]
struct Accept {
    version: String,
}

/// A108: a person accepts the current rules. Accepting any other version is refused.
async fn accept(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<Accept>,
) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "accept the community rules")?;
    if body.version.trim() != version() {
        return Err(ApiError::bad_request(format!(
            "the current rules are version {}: read {RULES_PATH} and accept that one",
            version()
        )));
    }
    let now = crate::now();
    state.db.accept_rules(&caller.person_id, version(), now)?;
    Ok(Json(view(&state, &caller.person_id, now)?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/rules/accept", post(accept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_and_its_start_are_read_from_the_page() {
        assert_eq!(version(), "2026-11");
        assert!(
            RULES_HTML.contains(&format!("Version {}", version())),
            "the text names its version"
        );
        assert!(since() > 1_790_000_000, "the start parses: {}", since());
    }
}
