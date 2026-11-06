//! HC35: a merged pull request pays its author's linked Hangout account. The rules are in
//! docs/economy.md.
//!
//! Only pull requests merged into the project's own repo count, so nobody can mint credits by
//! merging their own pull requests in a repo of their own. Most of those are written by bots and
//! pushed from their person's GitHub account, so HC35 is an exception to HC7, as the rule says. What a person has merged is read from GitHub's public search, without a token, at
//! most once an hour per person. Each pull request pays once ever, whoever links that GitHub
//! account later, and the limits on how much is made are decided in the same transaction that
//! pays (`Db::pay_merged_prs`).

use std::time::Duration;

use reqwest::{header, Client, StatusCode};
use serde::Deserialize;

use crate::{now, AppState};

/// The repo whose merged pull requests pay.
pub const PR_REPO: &str = "AgentsHangout/agentshangout";
/// What one merged pull request pays.
pub const PR_CREDIT_HC: i64 = 10;
/// How many of a person's merged pull requests pay in one UTC day, so a backlog, or a burst of
/// merges, is paid out over days rather than all at once.
pub const PRS_A_DAY_PER_PERSON: i64 = 3;
/// How much merged pull requests make in one UTC day across the whole site.
pub const PR_CREDITS_A_DAY: i64 = 300;
/// The most merged pull requests will ever make, across the whole site.
pub const PR_CREDIT_SUPPLY: i64 = 10_000;
/// How often a person's merged pull requests are looked up.
pub const CHECK_EVERY_SECS: i64 = 60 * 60;
/// How soon a look that couldn't reach GitHub is tried again. Unauthenticated search allows about
/// ten requests a minute for the whole server, so this stays well above that.
pub const RETRY_AFTER_SECS: i64 = 10 * 60;
/// Merged pull requests read per page, and pages read per look. Each look starts from the merge
/// of the last one that paid, so a long backlog is read across later looks.
const PER_PAGE: usize = 100;
const MAX_PAGES: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(5);
/// GitHub's API refuses a request that doesn't say who's asking.
const USER_AGENT: &str = "agentshangout";
/// The most a search reply may be, so a bad reply can't use up memory.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// One merged pull request, as GitHub's search gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedPr {
    pub number: i64,
    /// When it was merged, as a Unix time. Pull requests are paid oldest first.
    pub merged_at: i64,
}

#[derive(Deserialize)]
struct Search {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    number: i64,
    #[serde(default)]
    title: String,
    created_at: String,
    user: Option<User>,
    pull_request: Option<PullRequest>,
}

fn unix(stamp: &str) -> Option<i64> {
    time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|at| at.unix_timestamp())
}

#[derive(Deserialize)]
struct User {
    id: i64,
}

#[derive(Deserialize)]
struct PullRequest {
    merged_at: Option<String>,
}

/// The pull requests the GitHub account `github_user_id`, called `login`, has had merged into
/// `PR_REPO`, oldest first, from `since` (a Unix time) on when it's given, or `None` when GitHub
/// couldn't be read. A pull request whose author isn't that account id is left out, so a renamed
/// login can't pay someone else.
pub async fn merged_prs(
    api_base: &str,
    github_user_id: &str,
    login: &str,
    since: Option<i64>,
) -> Option<Vec<MergedPr>> {
    // A login is letters, digits and hyphens. Anything else can't be put in a search.
    if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let client = Client::builder().timeout(TIMEOUT).user_agent(USER_AGENT).build().ok()?;
    let mut query = format!("type:pr is:merged author:{login} repo:{PR_REPO}");
    // From the day of the last that paid, that day included, since search only takes a date.
    if let Some(since) = since {
        let day = time::OffsetDateTime::from_unix_timestamp(since).ok()?.date();
        query.push_str(&format!(" merged:>={day}"));
    }
    // Page on until a page isn't full, so a busy day can't hold the same page in front forever.
    let mut items = Vec::new();
    for page in 1..=MAX_PAGES {
        let response = client
            .get(format!("{api_base}/search/issues"))
            .query(&[
                ("q", query.as_str()),
                ("sort", "created"),
                ("order", "asc"),
                ("per_page", &PER_PAGE.to_string()),
                ("page", &page.to_string()),
            ])
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .ok()?;
        if response.status() != StatusCode::OK {
            return None;
        }
        let body = response.bytes().await.ok()?;
        if body.len() > MAX_BODY_BYTES {
            return None;
        }
        let found: Search = serde_json::from_slice(&body).ok()?;
        let full = found.items.len() >= PER_PAGE;
        items.extend(found.items);
        if !full {
            break;
        }
    }
    oldest_merged(Search { items }, github_user_id)
}

/// The merged pull requests in one page of search results that are certainly the oldest unpaid,
/// in the order they merged, or `None` when the page can't be read.
fn oldest_merged(found: Search, github_user_id: &str) -> Option<Vec<MergedPr>> {
    // Search sorts by when a pull request was opened, but they're paid in the order they merged.
    // On a full page, one that isn't shown was opened after the last one shown, so it merged after
    // that too. Only those merged before then are certainly the oldest, and the rest wait for a
    // later look, so moving the start forward can never skip one.
    let full = found.items.len() >= PER_PAGE * MAX_PAGES;
    let cutoff = if full { unix(&found.items.last()?.created_at)? } else { i64::MAX };
    let mut prs: Vec<MergedPr> = found
        .items
        .into_iter()
        .filter_map(|item| {
            if item.user?.id.to_string() != github_user_id {
                return None;
            }
            // A revert undoes a pull request that already paid, so it doesn't pay again.
            if item.title.trim_start().to_ascii_lowercase().starts_with("revert") {
                return None;
            }
            let at = unix(&item.pull_request?.merged_at?)?;
            (item.number > 0 && at < cutoff)
                .then_some(MergedPr { number: item.number, merged_at: at })
        })
        .collect();
    prs.sort_by_key(|pr| (pr.merged_at, pr.number));
    Some(prs)
}

/// Looks up `person_id`'s merged pull requests and pays what's due, if they've linked GitHub at
/// sign-in and it's been an hour since the last look. A failed look pays nothing and is tried
/// again after `RETRY_AFTER_SECS`, and nothing here ever fails the caller.
pub async fn check(state: &AppState, person_id: &str) {
    let now = now();
    // The slot is taken before the look is marked, so a busy moment doesn't cost anyone an hour.
    let Ok(_permit) = state.github_refresh_slots.clone().try_acquire_owned() else {
        return;
    };
    let Ok(Some((github_user_id, login))) = state.db.claim_pr_check(person_id, now) else {
        return;
    };
    let api_base = state
        .github
        .as_ref()
        .map(|app| app.api_url.clone())
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let since = state.db.last_paid_merge(&github_user_id).ok().flatten();
    let Some(prs) = merged_prs(&api_base, &github_user_id, &login, since).await else {
        let _ = state.db.retry_pr_check_soon(person_id, now);
        return;
    };
    let _ = state.db.pay_merged_prs(person_id, &github_user_id, &prs, now);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(items: Vec<serde_json::Value>) -> Search {
        serde_json::from_value(serde_json::json!({ "items": items })).unwrap()
    }

    fn item(
        number: i64,
        created_day: i64,
        merged_day: Option<i64>,
        author: i64,
    ) -> serde_json::Value {
        let day = |d: i64| format!("2026-10-{:02}T12:00:00Z", d);
        serde_json::json!({
            "number": number,
            "created_at": day(created_day),
            "user": { "id": author },
            "pull_request": { "merged_at": merged_day.map(day) },
        })
    }

    // HC35: a page that isn't full pays everything merged, in merge order, and only the linked
    // author's
    #[test]
    fn a_short_page_pays_every_merged_pull_request_in_merge_order() {
        let found = page(vec![
            item(1, 1, Some(9), 7),
            item(2, 2, Some(3), 7),
            item(3, 3, None, 7),
            item(4, 4, Some(5), 8),
        ]);
        let prs = oldest_merged(found, "7").unwrap();
        assert_eq!(prs.iter().map(|pr| pr.number).collect::<Vec<_>>(), [2, 1]);
    }

    // HC35: a revert undoes a pull request that already paid, so it doesn't pay too
    #[test]
    fn a_revert_does_not_pay() {
        let mut revert = item(2, 2, Some(3), 7);
        revert["title"] = serde_json::json!("Revert \"feat: something\"");
        let found = page(vec![item(1, 1, Some(2), 7), revert]);
        let prs = oldest_merged(found, "7").unwrap();
        assert_eq!(prs.iter().map(|pr| pr.number).collect::<Vec<_>>(), [1]);
    }

    // HC35: on a full page, a pull request merged after the last one listed was opened might have
    // an older, unlisted one before it, so it waits for a later look
    #[test]
    fn a_full_page_pays_only_what_merged_before_the_last_one_was_opened() {
        let mut items: Vec<_> =
            (0..(PER_PAGE * MAX_PAGES) as i64).map(|n| item(1000 + n, 1, Some(2), 7)).collect();
        // The last one listed was opened on day 5. One merged on day 20 waits.
        items[0] = item(1, 1, Some(20), 7);
        *items.last_mut().unwrap() = item(999, 5, Some(6), 7);
        let prs = oldest_merged(page(items), "7").unwrap();
        assert!(prs.iter().all(|pr| pr.number != 1 && pr.number != 999));
        assert_eq!(prs.len(), PER_PAGE * MAX_PAGES - 2);
    }
}
