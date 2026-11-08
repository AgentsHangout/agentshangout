//! BG1 to BG3 (#830, shape from #864): in-game bug auto-reports and deduplication.
//!
//! Public GitHub issues carry only the surface, the action and a short error code, each from a
//! fixed list. The hangout id, the reporter, any bot and the cleaned free text stay in SQLite.
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use reqwest::{redirect, Client};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::auth::Caller;
pub use crate::db::{BugReportRecord, NewBugReport};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// Surfaces a report may name. Anything else is refused with a 400.
pub const SURFACES: &[&str] = &["house", "space", "place", "item", "door"];
/// Actions a report may name. Anything else is refused with a 400.
pub const ACTIONS: &[&str] =
    &["build", "move", "load", "bulldoze", "use", "open", "place", "remove"];
/// Short error codes a report may carry to GitHub. Anything else is refused with a 400.
pub const CODES: &[&str] =
    &["unknown", "not_found", "forbidden", "invalid", "conflict", "closed", "limit", "unavailable"];

/// New GitHub issues one person may open a day (UTC).
pub const ISSUES_PER_PERSON_PER_DAY: i64 = 3;
/// New GitHub issues the site may open a day (UTC), across everyone.
pub const ISSUES_PER_SITE_PER_DAY: i64 = 20;
/// Stored report rows one person may first-file. A signature is one row, and the signatures
/// come from the fixed lists, so this bounds what any one signed-in caller can add to SQLite.
pub const ROWS_PER_PERSON: i64 = 20;

const SANITIZED_MAX_CHARS: usize = 500;

/// BG1: redacts auth tokens, emails and filesystem paths from free text, and caps it at 500
/// characters. The result is kept in SQLite only, never sent to GitHub.
pub fn sanitize_error(raw: &str) -> String {
    let mut out = Vec::new();
    for word in raw.split_whitespace() {
        let clean = word.trim_matches(|c: char| {
            c.is_ascii_punctuation() && c != '_' && c != '\\' && c != '/' && c != '@'
        });
        if clean.starts_with("ah_") {
            out.push("[REDACTED_TOKEN]");
        } else if clean.contains('@') && clean.split('@').nth(1).is_some_and(|d| d.contains('.')) {
            out.push("[REDACTED_EMAIL]");
        } else if (clean.len() >= 3
            && clean.chars().nth(1) == Some(':')
            && clean.chars().nth(2) == Some('\\'))
            || (clean.starts_with('/')
                && (clean.starts_with("/home")
                    || clean.starts_with("/Users")
                    || clean.starts_with("/workspace")
                    || clean.starts_with("/root")
                    || clean.starts_with("/var")
                    || clean.starts_with("/tmp")))
        {
            out.push("[PATH]");
        } else {
            out.push(word);
        }
    }
    let res = out.join(" ");
    if res.chars().count() > SANITIZED_MAX_CHARS {
        let mut cut: String = res.chars().take(SANITIZED_MAX_CHARS - 3).collect();
        cut.push_str("...");
        cut
    } else {
        res
    }
}

/// Refuses any surface, action or code that is not on the fixed lists.
pub fn check_fields(surface: &str, action: &str, code: &str) -> ApiResult<()> {
    if !SURFACES.contains(&surface) {
        return Err(ApiError::bad_request("unknown surface"));
    }
    if !ACTIONS.contains(&action) {
        return Err(ApiError::bad_request("unknown action"));
    }
    if !CODES.contains(&code) {
        return Err(ApiError::bad_request("unknown code"));
    }
    Ok(())
}

/// BG2: a deterministic signature of surface, action and code only, so the same failure
/// dedupes whatever free text came with it.
pub fn compute_signature(surface: &str, action: &str, code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(surface.as_bytes());
    hasher.update(b":");
    hasher.update(action.as_bytes());
    hasher.update(b":");
    hasher.update(code.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>()[..32].to_string()
}

fn client() -> Option<Client> {
    Client::builder()
        .user_agent("agentshangout")
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
        .redirect(redirect::Policy::none())
        .build()
        .ok()
}

/// BG1 to BG3: record a bug report in SQLite, and file or comment on GitHub with only the
/// surface, action and code. A new issue needs a free slot under both daily caps (UTC).
#[allow(clippy::too_many_arguments)]
pub async fn record_report(
    state: &AppState,
    caller: &Caller,
    surface: &str,
    action: &str,
    code: &str,
    error_text: &str,
    hangout_id: Option<&str>,
    now: i64,
) -> ApiResult<BugReportRecord> {
    check_fields(surface, action, code)?;
    let sanitized = sanitize_error(error_text);
    let signature = compute_signature(surface, action, code);

    let new_report = NewBugReport {
        signature: &signature,
        surface,
        action,
        code,
        error_message: &sanitized,
        hangout_id,
        reporter_id: Some(caller.person_id.as_str()),
    };

    let Some(mut record) = state.db.record_bug_report(&new_report, now, ROWS_PER_PERSON)? else {
        return Err(ApiError::too_many_requests(
            "you've filed as many new bug reports as you can; repeats of old ones still count",
        ));
    };
    let _ = file_or_comment_github(state, &caller.person_id, &mut record, now).await;
    Ok(record)
}

/// Repeat occurrences that get a GitHub comment: the 2nd, then the 10th, 100th, 1000th and so
/// on. A repeat that isn't one of these is kept in SQLite only, so a signed-in caller who
/// replays one failure can't post a comment per call. The count is bumped inside the same
/// IMMEDIATE transaction as the row, so each count is seen by exactly one request.
pub fn comments_on_repeat(report_count: i64) -> bool {
    if report_count < 2 {
        return false;
    }
    if report_count == 2 {
        return true;
    }
    let mut n = report_count;
    while n % 10 == 0 {
        n /= 10;
    }
    n == 1
}

/// BG3: file a new GitHub issue if this report won a slot under the caps, or comment on the
/// existing issue for a repeat that `comments_on_repeat` allows. Nothing else leaves the server.
pub async fn file_or_comment_github(
    state: &AppState,
    person_id: &str,
    record: &mut BugReportRecord,
    now: i64,
) -> ApiResult<()> {
    let Some(client) = client() else {
        return Ok(());
    };
    let api_base =
        state.github.as_ref().map(|g| g.api_url.as_str()).unwrap_or("https://api.github.com");
    let repo = std::env::var("AGENTHANGOUT_GITHUB_REPO")
        .unwrap_or_else(|_| "AgentsHangout/agentshangout".to_string());
    // Only this variable is read. A token that merely happens to be in the environment is not
    // a reason to post as the bot.
    let Some(token) = std::env::var("AGENTHANGOUT_GITHUB_TOKEN").ok().filter(|t| !t.is_empty())
    else {
        return Ok(());
    };

    if record.is_new {
        let day = now.div_euclid(86_400);
        let won = state.db.claim_bug_issue_slot(
            record.id,
            person_id,
            day,
            ISSUES_PER_PERSON_PER_DAY,
            ISSUES_PER_SITE_PER_DAY,
        )?;
        if !won {
            return Ok(());
        }
        let title =
            format!("[Auto-report] {} {} failed: {}", record.surface, record.action, record.code);
        let body = format!(
            "**Surface:** `{}`\n**Action:** `{}`\n**Code:** `{}`\n**Signature:** `{}`\n\n*Auto-reported in-game failure (#830).*",
            record.surface, record.action, record.code, record.signature
        );
        let url = format!("{}/repos/{}/issues", api_base, repo);
        let payload = json!({ "title": title, "body": body, "labels": ["auto-report"] });
        if let Ok(res) = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .json(&payload)
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(val) = res.json::<serde_json::Value>().await {
                    if let Some(num) = val["number"].as_i64() {
                        record.github_issue = Some(num);
                        let _ = state.db.set_bug_report_github_issue(record.id, num);
                    }
                }
            }
        }
    } else if let (Some(issue_num), true) =
        (record.github_issue, comments_on_repeat(record.report_count))
    {
        let url = format!("{}/repos/{}/issues/{}/comments", api_base, repo, issue_num);
        let body = format!(
            "Repeat occurrence: seen {} times (latest at {}). Surface: `{}`, Action: `{}`, Code: `{}`.",
            record.report_count, now, record.surface, record.action, record.code
        );
        let _ = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .json(&json!({ "body": body }))
            .send()
            .await;
    }
    Ok(())
}

/// Auto-capture on a failed in-game action. The error text stays in SQLite; the code is the
/// error's fixed reason when it has one, else "unknown". The caller's result is unchanged.
#[allow(clippy::too_many_arguments)]
pub async fn capture_if_failed<T>(
    state: &AppState,
    caller: &Caller,
    surface: &'static str,
    action: &'static str,
    hangout_id: Option<&str>,
    now: i64,
    res: ApiResult<T>,
) -> ApiResult<T> {
    if let Err(ref err) = res {
        let code = err.reason.filter(|r| CODES.contains(r)).unwrap_or("unknown");
        let _ = record_report(state, caller, surface, action, code, &err.message, hangout_id, now)
            .await;
    }
    res
}

#[derive(Debug, Deserialize)]
pub struct BugReportPayload {
    pub surface: String,
    pub action: String,
    pub code: String,
    /// Free text, kept in SQLite only after cleaning.
    #[serde(default)]
    pub error: String,
    pub hangout_id: Option<String>,
}

/// POST /api/bugs/report: signed-in callers only.
pub async fn report_bug_endpoint(
    State(state): State<AppState>,
    caller: Caller,
    Json(payload): Json<BugReportPayload>,
) -> ApiResult<Json<BugReportRecord>> {
    let now = crate::now();
    let record = record_report(
        &state,
        &caller,
        &payload.surface,
        &payload.action,
        &payload.code,
        &payload.error,
        payload.hangout_id.as_deref(),
        now,
    )
    .await?;
    Ok(Json(record))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/bugs/report", post(report_bug_endpoint))
}
