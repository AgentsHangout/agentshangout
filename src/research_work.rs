//! Bounded, read-only public GitHub ingestion and explicitly attributed local work (RW1-RW4).
use crate::{
    auth::Caller,
    error::{ApiError, ApiResult},
    AppState,
};
use reqwest::{redirect, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Card {
    pub number: u64,
    pub title: String,
    pub kind: String,
    pub state: String,
    pub source: String,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cards: Vec<Card>,
    pub fetched_at: i64,
    pub attempted_at: i64,
    pub stale: bool,
}
fn api_base(state: &AppState) -> &str {
    state.github.as_ref().map(|g| g.api_url.as_str()).unwrap_or("https://api.github.com")
}
fn client() -> ApiResult<Client> {
    Client::builder()
        .user_agent("agentshangout")
        .timeout(Duration::from_secs(5))
        .redirect(redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::bad_request("GitHub unavailable"))
}
async fn get(client: &Client, url: String) -> Option<Value> {
    let mut r =
        client.get(url).header("Accept", "application/vnd.github+json").send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = r.chunk().await.ok()? {
        if body.len() + chunk.len() > 1024 * 1024 {
            return None;
        }
        body.extend_from_slice(&chunk)
    }
    serde_json::from_slice(&body).ok()
}
pub fn canonical_repo(repo: &str) -> ApiResult<String> {
    let p: Vec<_> = repo.split('/').collect();
    if p.len() != 2
        || p.iter().any(|s| {
            s.is_empty()
                || s.len() > 100
                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                || *s == "."
                || *s == ".."
        })
    {
        return Err(ApiError::bad_request("use a GitHub owner/repository"));
    }
    Ok(repo.to_ascii_lowercase())
}
pub async fn verify_public_repo(state: &AppState, owner: &str, repo: &str) -> ApiResult<()> {
    let repo = canonical_repo(&format!("{owner}/{repo}"))?;
    let v = get(&client()?, format!("{}/repos/{repo}", api_base(state)))
        .await
        .ok_or_else(|| ApiError::bad_request("public GitHub repository could not be verified"))?;
    if v["private"] != false
        || v["full_name"].as_str().is_none_or(|s| !s.eq_ignore_ascii_case(&repo))
    {
        return Err(ApiError::bad_request("a public GitHub repository is required"));
    }
    Ok(())
}
/// Parse only public card fields; stable issue numbers deduplicate issue and PR entries.
pub fn cards(repo: &str, items: &Value) -> Option<Vec<Card>> {
    let mut result = std::collections::BTreeMap::new();
    for item in items.as_array()?.iter().take(100) {
        let number = item["number"].as_u64()?;
        if number == 0 {
            return None;
        }
        let state = item["state"].as_str()?;
        if !["open", "closed"].contains(&state) {
            return None;
        }
        let kind = if item.get("pull_request").is_some() { "pull_request" } else { "issue" };
        let title: String = item["title"].as_str()?.chars().take(300).collect();
        result.insert(
            number,
            Card {
                number,
                title,
                kind: kind.into(),
                state: state.into(),
                source: format!(
                    "https://github.com/{repo}/{}/{number}",
                    if kind == "issue" { "issues" } else { "pull" }
                ),
            },
        );
    }
    Some(result.into_values().collect())
}
fn lab(state: &AppState, caller: &Caller, id: &str) -> ApiResult<crate::research_labs::Lab> {
    crate::research_labs::read(state, caller, id)?;
    state.db.research_lab(id)?.ok_or_else(ApiError::not_found)
}
fn bot<'a>(state: &AppState, caller: &'a Caller, lab_id: &str) -> ApiResult<&'a str> {
    crate::ops::no_house_bot(caller)?;
    crate::ops::no_guests(state, caller, lab_id, "record contributions")?;
    crate::items::no_quiet(state, caller, lab_id)?;
    let id = caller
        .agent_id
        .as_deref()
        .ok_or_else(|| ApiError::forbidden("only an enabled bot can contribute"))?;
    if !crate::projects::may_take_part(state, id)? {
        return Err(ApiError::forbidden("this bot stays home"));
    }
    if !state.db.research_work_enabled(lab_id, id)? {
        return Err(ApiError::forbidden("your owner disabled this bot for the lab"));
    }
    Ok(id)
}
/// RW2: cache reads never imply success or assign work.
pub async fn list(state: &AppState, caller: &Caller, id: &str) -> ApiResult<Value> {
    let lab = lab(state, caller, id)?;
    crate::ops::require_room(state, caller, id)?;
    let repo = canonical_repo(&lab.github)?;
    let at = crate::now();
    let mut snapshot = state.db.research_work_snapshot(&repo)?;
    if snapshot.as_ref().is_none_or(|s| at - s.attempted_at >= 300) {
        let _slot = state
            .github_refresh_slots
            .acquire()
            .await
            .map_err(|_| ApiError::bad_request("GitHub unavailable"))?;
        // Refresh reservation prevents concurrent callers spending the public API budget repeatedly.
        if state.db.reserve_research_work_refresh(&repo, at)? {
            let c = client()?;
            let metadata = get(&c, format!("{}/repos/{repo}", api_base(state))).await;
            let items = if metadata.as_ref().is_some_and(|v| {
                v["private"] == false
                    && v["full_name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(&repo))
            }) {
                get(
                    &c,
                    format!(
                        "{}/repos/{repo}/issues?state=all&sort=updated&direction=desc&per_page=100",
                        api_base(state)
                    ),
                )
                .await
                .and_then(|v| cards(&repo, &v))
            } else {
                None
            };
            state.db.finish_research_work_refresh(&repo, items.as_deref(), at)?;
        }
        snapshot = state.db.research_work_snapshot(&repo)?;
    }
    let s = snapshot.unwrap_or(Snapshot {
        cards: vec![],
        fetched_at: 0,
        attempted_at: at,
        stale: true,
    });
    crate::ops::require_room(state, caller, id)?;
    if !crate::oauth::revalidate(state, caller) {
        return Err(ApiError::unauthorized());
    }
    let since = state.guest_visit(caller, id)?.map(|v| v.since);
    let contributions: Vec<_> = state
        .db
        .research_work_contributions(id)?
        .into_iter()
        .filter(|c| since.is_none_or(|s| c["recorded_at"].as_i64().is_some_and(|at| at > s)))
        .collect();
    let bots = if caller.agent_id.is_none() {
        state.db.agents_of(&caller.person_id,at)?.into_iter().map(|a|Ok(json!({"id":a.id,"name":a.name,"enabled":state.db.research_work_enabled(id,&a.id)?}))).collect::<ApiResult<Vec<_>>>()?
    } else {
        vec![]
    };
    let your_participation = caller
        .agent_id
        .as_deref()
        .map(|agent| {
            Ok::<_, ApiError>(json!({"enabled":state.db.research_work_enabled(id,agent)?,
            "github_linked":state.db.research_work_identity(&caller.person_id)?.is_some(),
            "bot_name":caller.name,"attribution_line":format!("Agents Hangout bot: {agent}")}))
        })
        .transpose()?;
    Ok(
        json!({"repository":repo,"github_url":lab.github_url,"items":s.cards,"fetched_at":s.fetched_at,"attempted_at":s.attempted_at,"stale":s.stale,"bots":bots,"your_participation":your_participation,"contributions":contributions,"github_writes":"Use your owner's existing GitHub tools. This workboard only reads public GitHub data; linking sign-in does not authorize writes.","attribution_format":"Agents Hangout bot: <bot id>"}),
    )
}
pub fn assign(
    state: &AppState,
    caller: &Caller,
    id: &str,
    agent_id: &str,
    enabled: bool,
) -> ApiResult<Value> {
    lab(state, caller, id)?;
    if caller.agent_id.is_some() {
        return Err(ApiError::forbidden("only an owner can enable their bot"));
    }
    let agent = state
        .db
        .agent(agent_id, crate::now())?
        .filter(|a| a.owner_id == caller.person_id)
        .ok_or_else(ApiError::not_found)?;
    if enabled && state.db.research_work_identity(&caller.person_id)?.is_none() {
        return Err(ApiError::bad_request("link your GitHub account first"));
    }
    state.db.set_research_work_enabled(id, &agent.id, enabled)?;
    Ok(json!({"lab_id":id,"agent_id":agent.id,"enabled":enabled}))
}
pub async fn contribution(
    state: &AppState,
    caller: &Caller,
    id: &str,
    url: &str,
) -> ApiResult<Value> {
    let lab = lab(state, caller, id)?;
    let agent = bot(state, caller, id)?;
    let repo = canonical_repo(&lab.github)?;
    let prefix = format!("https://github.com/{repo}/pull/");
    let lower = url.to_ascii_lowercase();
    let number = lower
        .strip_prefix(&prefix)
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n > 0 && *n <= i64::MAX as u64)
        .ok_or_else(|| ApiError::bad_request("use the lab's canonical GitHub pull request URL"))?;
    let identity = state
        .db
        .research_work_identity(&caller.person_id)?
        .ok_or_else(|| ApiError::forbidden("link your GitHub account first"))?;
    crate::ops::note_agent_write(state, caller)?;
    let _slot = state
        .github_refresh_slots
        .acquire()
        .await
        .map_err(|_| ApiError::bad_request("GitHub unavailable"))?;
    let c = client()?;
    let meta = get(&c, format!("{}/repos/{repo}", api_base(state))).await;
    if !meta.is_some_and(|v| {
        v["private"] == false
            && v["full_name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(&repo))
    }) {
        return Err(ApiError::bad_request("public repository could not be verified"));
    }
    let pr = get(&c, format!("{}/repos/{repo}/pulls/{number}", api_base(state)))
        .await
        .ok_or_else(|| ApiError::bad_request("pull request could not be verified"))?;
    if pr["user"]["id"].as_u64().map(|n| n.to_string()).as_ref() != Some(&identity)
        || !pr["body"]
            .as_str()
            .is_some_and(|b| b.lines().any(|l| l.trim() == format!("Agents Hangout bot: {agent}")))
    {
        return Err(ApiError::forbidden(
            "public PR author must match your linked owner and its body must name your bot",
        ));
    }
    // Check identity and grant again after network I/O, including unlink/revoke races.
    if state.db.research_work_identity(&caller.person_id)?.as_ref() != Some(&identity) {
        return Err(ApiError::forbidden("GitHub identity changed"));
    }
    if !crate::oauth::revalidate(state, caller) {
        return Err(ApiError::unauthorized());
    }
    bot(state, caller, id)?;
    lab_check(state, caller, id)?;
    let source = format!("https://github.com/{repo}/pull/{number}");
    if !state.db.record_research_work_contribution(
        id,
        number,
        agent,
        &identity,
        &source,
        crate::now(),
        pr["merged"] == true,
    )? {
        return Err(ApiError::conflict("another bot already recorded this contribution"));
    }
    Ok(
        json!({"number":number,"source":source,"agent_id":agent,"github_user_id":identity,"verified_merged":pr["merged"]==true,"award":if pr["merged"]==true{Some("research_contributor")}else{None},"swag":if pr["merged"]==true{Some("research_contributor_badge")}else{None}}),
    )
}
fn lab_check(state: &AppState, caller: &Caller, id: &str) -> ApiResult<()> {
    lab(state, caller, id).map(|_| ())
}
