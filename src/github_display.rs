//! Showing a linked GitHub account: the rules in docs/accounts-and-trust.md (A70 to A75).
//!
//! Linking is not showing. A person turns each section on, and only then does anyone else see
//! it, read from a cache of public fields that is refreshed lazily from GitHub's public API.
//! Nothing here holds a token: the refresh calls GitHub without one, as the sign-in code reads
//! an account's id and login and drops the token (A47).

use std::time::Duration;

use reqwest::{header, redirect, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How old a cache row may be before a view refreshes it (A72).
const STALE_AFTER_SECS: i64 = 6 * 60 * 60;
/// How long a failed refresh keeps later views from trying again (A72).
const RETRY_AFTER_SECS: i64 = 60 * 60;
const TIMEOUT: Duration = Duration::from_secs(5);
/// GitHub's API refuses a request that doesn't say who's asking.
const USER_AGENT: &str = "agentshangout";
/// The repo whose merged pull requests the badge counts.
const BADGE_REPO: &str = "AgentsHangout/agentshangout";
/// Repos fetched per page, and pages fetched at most: a huge account can't stall the refresh.
const REPOS_PER_PAGE: usize = 100;
const MAX_REPO_PAGES: i64 = 3;
/// Repos shown, by stars, most first.
const TOP_REPOS_SHOWN: usize = 6;
/// Longer than any username GitHub gives out (same bound as the sign-in code).
const MAX_LOGIN_CHARS: usize = 64;

/// What a person chose to show, one switch per section, all off until they turn one on (A70).
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct GitHubDisplay {
    pub show_repos: bool,
    pub show_stars: bool,
    pub show_languages: bool,
    pub show_badge: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct DisplayPatch {
    pub show_repos: Option<bool>,
    pub show_stars: Option<bool>,
    pub show_languages: Option<bool>,
    pub show_badge: Option<bool>,
    /// A93: show the linked Discord name on the card. Kept apart from the GitHub switches.
    pub show_discord: Option<bool>,
}

impl GitHubDisplay {
    pub fn any(&self) -> bool {
        self.show_repos || self.show_stars || self.show_languages || self.show_badge
    }
}

/// One public repo, with only the fields the cache keeps (A71).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repo {
    pub name: String,
    pub html_url: String,
    pub stargazers_count: i64,
    pub language: Option<String>,
    pub description: Option<String>,
}

/// One language's share of those repos, most repos first.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LanguageCount {
    pub language: String,
    pub count: i64,
}

/// What GitHub sends for a repo: only what the cache may keep, plus the flags it filters on.
/// Everything else GitHub sends is ignored, so it can't be kept by accident.
#[derive(Deserialize)]
struct RawRepo {
    owner: PublicIdentity,
    name: String,
    html_url: String,
    stargazers_count: i64,
    language: Option<String>,
    description: Option<String>,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    private: bool,
}

#[derive(Deserialize)]
struct PublicIdentity {
    id: u64,
    login: String,
}

/// Bound public responses before deserializing; an upstream body cannot consume unlimited memory.
async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Option<T> {
    const MAX_BYTES: usize = 2 * 1024 * 1024;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len().checked_add(chunk.len())? > MAX_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).ok()
}

async fn identity_matches(client: &Client, base: &str, login: &str, id: u64) -> Option<()> {
    let response = client
        .get(format!("{base}/users/{login}"))
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if response.status() != StatusCode::OK {
        return None;
    }
    let identity: PublicIdentity = bounded_json(response).await?;
    (identity.id == id && identity.login.eq_ignore_ascii_case(login)).then_some(())
}

/// What the search API sends for a count query.
#[derive(Deserialize)]
struct SearchResult {
    total_count: i64,
}

/// A row of `github_display_cache`: one GitHub account's public display data (A72).
#[derive(Clone, Debug)]
pub struct CachedDisplay {
    pub github_user_id: String,
    pub github_login: String,
    pub repos_json: String,
    pub top_languages_json: String,
    pub merged_pr_count: i64,
    pub fetched_at: i64,
    pub repos_etag: Option<String>,
    pub search_etag: Option<String>,
    pub last_attempt_at: i64,
}

/// A refresh lease, invalidated by changing display consent or replacing/unlinking the identity.
pub struct RefreshClaim {
    pub id: String,
    pub github_user_id: String,
    pub github_login: String,
    pub existing: Option<CachedDisplay>,
}

/// Whether a view refreshes this cache row now (A72): no row, or a row older than six hours
/// whose last attempt wasn't within the last hour.
pub fn should_refresh(cache: Option<&CachedDisplay>, now: i64) -> bool {
    match cache {
        None => true,
        Some(row) => {
            now - row.fetched_at >= STALE_AFTER_SECS
                && (row.last_attempt_at == 0 || now - row.last_attempt_at >= RETRY_AFTER_SECS)
        }
    }
}

/// The repos a cache row holds, as structs. A row that doesn't parse is no repos, not an error.
pub fn repos_of(cache: &CachedDisplay) -> Vec<Repo> {
    serde_json::from_str(&cache.repos_json).unwrap_or_default()
}

/// The languages a cache row holds, as structs.
pub fn languages_of(cache: &CachedDisplay) -> Vec<LanguageCount> {
    serde_json::from_str(&cache.top_languages_json).unwrap_or_default()
}

/// Counts the languages across repos, most repos first, ties broken by name so the order holds.
pub fn top_languages(repos: &[Repo]) -> Vec<LanguageCount> {
    let mut counts: Vec<(String, i64)> = Vec::new();
    for repo in repos {
        if let Some(language) = &repo.language {
            if let Some(found) = counts.iter_mut().find(|(name, _)| name == language) {
                found.1 += 1;
            } else {
                counts.push((language.clone(), 1));
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts.into_iter().map(|(language, count)| LanguageCount { language, count }).collect()
}

/// Stars received: the total across the repos shown, each of which already carries its own
/// count (A71).
pub fn stars_received(repos: &[Repo]) -> i64 {
    repos.iter().map(|repo| repo.stargazers_count).sum()
}

fn login_fits(login: &str) -> bool {
    (1..=MAX_LOGIN_CHARS).contains(&login.chars().count())
        && login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !login.starts_with('-')
        && !login.ends_with('-')
}

/// Refreshes one account's cache row from GitHub's public API, without any token. `None` means
/// the refresh failed somewhere, and the caller keeps the stale row (A72). Each call keeps its
/// search etag. Repo pages are fetched unconditionally because page one's etag cannot validate later pages.
pub async fn refresh(
    api_base: &str,
    github_user_id: &str,
    login: &str,
    existing: Option<&CachedDisplay>,
    now: i64,
) -> Option<CachedDisplay> {
    if !login_fits(login) {
        return None;
    }
    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .ok()?;
    let id: u64 = github_user_id.parse().ok()?;
    identity_matches(&client, api_base, login, id).await?;
    let (repos, repos_etag) = fetch_repos(&client, api_base, login, id).await?;
    let (merged_pr_count, search_etag) =
        fetch_merged_prs(&client, api_base, login, existing).await?;
    // The search API is login-based. Check again after all reads, and never cache a reassigned login.
    identity_matches(&client, api_base, login, id).await?;
    let languages = top_languages(&repos);
    Some(CachedDisplay {
        github_user_id: github_user_id.to_string(),
        github_login: login.to_string(),
        repos_json: serde_json::to_string(&repos).ok()?,
        top_languages_json: serde_json::to_string(&languages).ok()?,
        merged_pr_count,
        fetched_at: now,
        repos_etag,
        search_etag,
        last_attempt_at: now,
    })
}

async fn fetch_repos(
    client: &Client,
    api_base: &str,
    login: &str,
    github_user_id: u64,
) -> Option<(Vec<Repo>, Option<String>)> {
    let mut repos = Vec::new();
    let mut etag = None;
    for page in 1..=MAX_REPO_PAGES {
        let page_text = page.to_string();
        let request = client
            .get(format!("{api_base}/users/{login}/repos"))
            .query(&[("per_page", "100"), ("page", page_text.as_str())])
            .header(header::ACCEPT, "application/vnd.github+json");
        let response = request.send().await.ok()?;
        match response.status() {
            StatusCode::OK => {
                if page == 1 {
                    etag = etag_of(&response);
                }
                let raw: Vec<RawRepo> = bounded_json(response).await?;
                if raw.iter().any(|repo| {
                    repo.owner.id != github_user_id || !repo.owner.login.eq_ignore_ascii_case(login)
                }) {
                    return None;
                }
                let fresh = raw.len();
                let kept: Vec<Repo> = raw
                    .into_iter()
                    .filter(|repo| !repo.fork && !repo.archived && !repo.private)
                    .map(|repo| Repo {
                        name: repo.name,
                        html_url: repo.html_url,
                        stargazers_count: repo.stargazers_count,
                        language: repo.language,
                        description: repo.description,
                    })
                    .collect();
                repos.extend(kept);
                if fresh < REPOS_PER_PAGE {
                    break;
                }
            }
            _ => return None,
        }
    }
    Some((repos, etag))
}

async fn fetch_merged_prs(
    client: &Client,
    api_base: &str,
    login: &str,
    existing: Option<&CachedDisplay>,
) -> Option<(i64, Option<String>)> {
    let query = format!("type:pr is:merged author:{login} repo:{BADGE_REPO}");
    let mut request = client
        .get(format!("{api_base}/search/issues"))
        .query(&[("q", query)])
        .header(header::ACCEPT, "application/vnd.github+json");
    if let Some(etag) = existing.and_then(|row| row.search_etag.as_deref()) {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.ok()?;
    match response.status() {
        StatusCode::OK => {
            let etag = etag_of(&response);
            let found: SearchResult = bounded_json(response).await?;
            if found.total_count < 0 {
                return None;
            }
            Some((found.total_count, etag))
        }
        StatusCode::NOT_MODIFIED => {
            let kept = existing.map(|row| row.merged_pr_count).unwrap_or(0);
            let etag = existing.and_then(|row| row.search_etag.clone());
            Some((kept, etag))
        }
        _ => None,
    }
}

fn etag_of(response: &reqwest::Response) -> Option<String> {
    response.headers().get(header::ETAG)?.to_str().ok().map(str::to_string)
}

/// The sections a public view shows for this person: only what they turned on (A70, A74).
/// The repos section is the six most-starred, while the star total counts them all (A71).
pub fn public_sections(display: &GitHubDisplay, cache: &CachedDisplay) -> Value {
    let repos = repos_of(cache);
    let mut shown = json!({});
    if display.show_repos {
        let mut top = repos.clone();
        top.sort_by(|a, b| {
            b.stargazers_count.cmp(&a.stargazers_count).then_with(|| a.name.cmp(&b.name))
        });
        top.truncate(TOP_REPOS_SHOWN);
        shown["repos"] = json!(top);
    }
    if display.show_stars {
        shown["stars"] = json!({ "total": stars_received(&repos) });
    }
    if display.show_languages {
        shown["languages"] = json!(languages_of(cache));
    }
    if display.show_badge {
        shown["badge"] = json!({ "merged_prs": cache.merged_pr_count });
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(now: i64) -> CachedDisplay {
        CachedDisplay {
            github_user_id: "1001".to_string(),
            github_login: "ann-codes".to_string(),
            repos_json: "[]".to_string(),
            top_languages_json: "[]".to_string(),
            merged_pr_count: 0,
            fetched_at: now,
            repos_etag: None,
            search_etag: None,
            last_attempt_at: 0,
        }
    }

    #[test]
    fn a_fresh_row_never_refreshes_and_a_stale_one_does() {
        let now = 10_000_000;
        assert!(should_refresh(None, now));
        let fresh = row(now);
        assert!(!should_refresh(Some(&fresh), now));
        let stale = CachedDisplay { fetched_at: now - STALE_AFTER_SECS - 1, ..fresh.clone() };
        assert!(should_refresh(Some(&stale), now));
        let backed_off = CachedDisplay { last_attempt_at: now - 60, ..stale.clone() };
        assert!(!should_refresh(Some(&backed_off), now));
        let retry_due = CachedDisplay { last_attempt_at: now - RETRY_AFTER_SECS - 1, ..stale };
        assert!(should_refresh(Some(&retry_due), now));
    }

    #[test]
    fn languages_count_and_order() {
        let repos = vec![
            Repo {
                name: "a".into(),
                html_url: "".into(),
                stargazers_count: 0,
                language: Some("Rust".into()),
                description: None,
            },
            Repo {
                name: "b".into(),
                html_url: "".into(),
                stargazers_count: 0,
                language: Some("Rust".into()),
                description: None,
            },
            Repo {
                name: "c".into(),
                html_url: "".into(),
                stargazers_count: 0,
                language: Some("Go".into()),
                description: None,
            },
            Repo {
                name: "d".into(),
                html_url: "".into(),
                stargazers_count: 0,
                language: None,
                description: None,
            },
        ];
        let counted = top_languages(&repos);
        assert_eq!(counted.len(), 2);
        assert_eq!(counted[0].language, "Rust");
        assert_eq!(counted[0].count, 2);
        assert_eq!(counted[1].language, "Go");
        assert_eq!(counted[1].count, 1);
        assert_eq!(stars_received(&repos), 0);
    }
}
