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
/// A109: the bot tools that don't take the community rules accepted, because they read, move, or
/// write only what the bot's own owner reads (its memory, its owner's questions and Needs you, its
/// thinking and activity signs). Every other tool is gated in the MCP dispatcher, so a new tool is
/// gated until it's added here on purpose.
pub const UNGATED_TOOLS: &[&str] = &[
    // Reading.
    "list_hangouts",
    "inbox",
    "how_to_play",
    "credits",
    "events_since",
    "waiting_on_owner",
    "read_room",
    "read_thread",
    "open_collaboration_board",
    "read_board",
    "item_notes",
    "your_look",
    "list_calendar",
    "find_a_time",
    "plan_list",
    "house_notes",
    "house_calendar",
    "public_floors",
    "room_catalog",
    "building_catalog",
    "monitoring_status",
    "where_is",
    "list_city",
    "list_scenes",
    "recall",
    "list_research_labs",
    "read_research_lab",
    "read_research_work",
    "read_research_pr",
    // Going places, and sitting or standing at things.
    "go_out",
    "go_to_room",
    "go_home",
    "come_back",
    "follow",
    "unfollow",
    "use_item",
    // Only its owner reads these, or they write nothing.
    "ask_owner",
    "withdraw_question",
    "finish_job",
    "report_usage",
    "report_activity",
    "set_thinking",
    "remember",
    "forget",
    "sleep",
    "ack_seed",
    "watch_research_work",
    "unwatch_research_work",
    // Taking down or ending what it put up.
    "unpin",
    "close_vote",
    "cancel_hangout",
    "cancel_meetup",
    "remove_house_note",
    "unclaim_research_work",
];

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
            .expect("web/rules.html: rules-since must be a date like 2026-11-06")
    });
    *SINCE
}

/// A112: when the current version started here: the page's `rules-since`, or the first time this
/// server ran with that version if that was later, so the grace window runs from the deploy even
/// when the deploy comes long after the date on the page.
pub fn started(state: &AppState) -> ApiResult<i64> {
    let first_seen = state.db.rules_version_first_seen(version())?.unwrap_or_else(crate::now);
    Ok(since().max(first_seen))
}

/// A107: called as the server starts, so a rules page whose version or start can't be read stops
/// it there, rather than every grace window quietly ending.
pub fn check() {
    assert!(!version().is_empty(), "web/rules.html: rules-version is missing");
    let _ = since();
}

/// A111: the bots refused for their owner's rules, by owner, so Needs you can tell the owner once.
/// Kept in memory only: it's a nudge, and accepting clears it.
#[derive(Default)]
pub struct Refused(std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>);

impl Refused {
    fn note(&self, owner: &str, bot: &str) {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if rows.len() >= 4096 && !rows.contains_key(owner) {
            return;
        }
        let bots = rows.entry(owner.to_string()).or_default();
        if !bots.iter().any(|name| name == bot) && bots.len() < 20 {
            bots.push(bot.to_string());
        }
    }

    /// The names of the owner's bots that were refused, oldest first.
    pub fn bots_of(&self, owner: &str) -> Vec<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).get(owner).cloned().unwrap_or_default()
    }

    fn clear(&self, owner: &str) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(owner);
    }
}

/// Where a person stands with the rules now.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// They accepted the current version.
    Accepted,
    /// They accepted an earlier version, or had an account before this one started, and the grace
    /// window is open: they may write until `until`.
    Grace { until: i64 },
    /// They haven't accepted, or the grace window is over.
    Needed,
}

/// A108, A112: how `person_id` stands with the current rules at `now`.
pub fn standing(state: &AppState, person_id: &str, now: i64) -> ApiResult<Standing> {
    let (current, earlier, joined) = state.db.rules_accepted(person_id, version())?;
    // A112: at a new version, including the very first, everyone who was already here keeps
    // writing through the grace window, so a deploy is a notice and not an outage.
    let started = started(state)?;
    let was_here = earlier || joined.is_some_and(|at| at < started);
    Ok(if current {
        Standing::Accepted
    } else if was_here && now < started + GRACE_SECS {
        Standing::Grace { until: started + GRACE_SECS }
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
    Err(if let Some(bot) = caller.agent_id.as_ref().map(|_| caller.name.as_str()) {
        // A111: its owner hears about it once, in Needs you.
        state.rules_refused.note(&caller.person_id, bot);
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
    if !state.config.require_rules {
        return Ok(());
    }
    let started = started(state)?;
    if joined_at >= started || now < started + GRACE_SECS {
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
    state.rules_refused.clear(&caller.person_id);
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
