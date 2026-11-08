//! BG1 to BG3 (#830): in-game bug auto-reports and deduplication.
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use reqwest::{redirect, Client};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::auth::{Caller, MaybeCaller};
pub use crate::db::{BugReportRecord, NewBugReport};
use crate::error::ApiResult;
use crate::AppState;

/// BG1: sanitizes error messages by redacting auth tokens, emails, and filesystem paths,
/// and capping the length to 500 characters.
pub fn sanitize_error(raw: &str) -> String {
    let mut out = Vec::new();
    for word in raw.split_whitespace() {
        let clean = word.trim_matches(|c: char| {
            c.is_ascii_punctuation() && c != '_' && c != '\\' && c != '/' && c != '@'
        });
        if clean.starts_with("ah_") {
            out.push("[REDACTED_TOKEN]");
        } else if clean.contains('@') && clean.split('@').nth(1).map_or(false, |d| d.contains('.'))
        {
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
    let mut res = out.join(" ");
    if res.len() > 500 {
        res.truncate(497);
        res.push_str("...");
    }
    res
}

/// BG2: compute deterministic signature for deduplication
pub fn compute_signature(surface: &str, action: &str, sanitized_error: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(surface.trim().to_lowercase().as_bytes());
    hasher.update(b":");
    hasher.update(action.trim().to_lowercase().as_bytes());
    hasher.update(b":");
    hasher.update(sanitized_error.trim().as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>()[..32].to_string()
}

/// Client for GitHub API
fn client() -> Option<Client> {
    Client::builder()
        .user_agent("agentshangout")
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
        .redirect(redirect::Policy::none())
        .build()
        .ok()
}

/// BG1 to BG3: Record a bug report in the database and optionally file/comment on GitHub.
pub async fn record_report(
    state: &AppState,
    caller: Option<&Caller>,
    surface: &str,
    action: &str,
    error: &str,
    hangout_id: Option<&str>,
    now: i64,
) -> ApiResult<BugReportRecord> {
    let sanitized = sanitize_error(error);
    let signature = compute_signature(surface, action, &sanitized);
    let reporter_id = caller.map(|c| c.person_id.as_str());

    let new_report = NewBugReport {
        signature: &signature,
        surface,
        action,
        error_message: &sanitized,
        hangout_id,
        reporter_id,
    };

    let mut record = state.db.record_bug_report(&new_report, now)?;

    // BG3: File or comment on GitHub if configured
    let _ = file_or_comment_github(state, &mut record, now).await;

    Ok(record)
}

/// BG3: File a new GitHub issue or comment on an existing one
pub async fn file_or_comment_github(
    state: &AppState,
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

    let token =
        std::env::var("AGENTHANGOUT_GITHUB_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN")).ok();

    let Some(token) = token.filter(|t| !t.is_empty()) else {
        // No token configured: record in SQLite only
        return Ok(());
    };

    if record.is_new {
        let title = format!(
            "[Auto-report] {} {} failed: {}",
            record.surface, record.action, record.error_message
        );
        let title = if title.len() > 100 { format!("{}...", &title[..97]) } else { title };
        let body = format!(
            "**Surface:** `{}`\n**Action:** `{}`\n**Error:** `{}`\n**Hangout:** `{}`\n**Signature:** `{}`\n\n*Auto-reported in-game error (#830).* Tags: auto-report",
            record.surface,
            record.action,
            record.error_message,
            record.hangout_id.as_deref().unwrap_or("none"),
            record.signature
        );
        let url = format!("{}/repos/{}/issues", api_base, repo);
        let payload = json!({
            "title": title,
            "body": body,
            "labels": ["auto-report"],
        });
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
    } else if let Some(issue_num) = record.github_issue {
        let url = format!("{}/repos/{}/issues/{}/comments", api_base, repo, issue_num);
        let body = format!(
            "Repeat occurrence: failure seen {} times (latest at {}). Surface: `{}`, Action: `{}`.",
            record.report_count, now, record.surface, record.action
        );
        let payload = json!({ "body": body });
        let _ = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .json(&payload)
            .send()
            .await;
    }

    Ok(())
}

/// Helper to auto-capture errors on failed actions without modifying the error returned to caller.
pub async fn capture_if_failed<T>(
    state: &AppState,
    caller: Option<&Caller>,
    surface: &'static str,
    action: &'static str,
    hangout_id: Option<&str>,
    now: i64,
    res: ApiResult<T>,
) -> ApiResult<T> {
    if let Err(ref err) = res {
        let _ = record_report(state, caller, surface, action, &err.message, hangout_id, now).await;
    }
    res
}

#[derive(Debug, Deserialize)]
pub struct BugReportPayload {
    pub surface: String,
    pub action: String,
    pub error: String,
    pub hangout_id: Option<String>,
}

pub async fn report_bug_endpoint(
    State(state): State<AppState>,
    MaybeCaller(caller): MaybeCaller,
    Json(payload): Json<BugReportPayload>,
) -> ApiResult<Json<BugReportRecord>> {
    let now = crate::now();
    let record = record_report(
        &state,
        caller.as_ref(),
        &payload.surface,
        &payload.action,
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
