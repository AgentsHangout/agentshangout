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
    /// RW10: GitHub's label names. A snapshot from before labels were read has none.
    #[serde(default)]
    pub labels: Vec<String>,
}
/// RW8: someone saying they're working on an item. A local fact, never written to GitHub.
#[derive(Clone, Debug, Serialize)]
pub struct Claim {
    pub number: u64,
    #[serde(skip)]
    pub claimer: String,
    pub is_bot: bool,
    pub name: String,
    pub pairing: bool,
    pub since: i64,
    pub until: i64,
}
/// RW11: someone watching an item to hear when it changes. A local fact, never written to GitHub.
#[derive(Clone, Debug, Serialize)]
pub struct Watcher {
    pub number: u64,
    #[serde(skip)]
    pub watcher: String,
    #[serde(skip)]
    pub person_id: String,
    #[serde(skip)]
    pub agent_id: Option<String>,
    pub is_bot: bool,
    pub name: String,
    pub since: i64,
}
/// RW11: how many items one person or bot may watch in a lab.
pub const MAX_WATCHES: i64 = 50;
/// RW9: a pull request's CI and review state. `unknown` until GitHub has said otherwise.
#[derive(Clone, Debug, Serialize)]
pub struct PrState {
    pub ci: String,
    pub review: String,
    /// RW12: who's still asked to review, as GitHub last said: logins, and team slugs. None
    /// until a read has seen GitHub's lists, which isn't the same as nobody asked.
    pub requested: Option<Vec<String>>,
    pub requested_teams: Option<Vec<String>>,
    pub checked_at: i64,
    #[serde(skip)]
    pub attempted_at: i64,
    pub stale: bool,
}
/// RW12: who a pull request still asks for a review. GitHub drops someone from it once they've
/// reviewed, so this is who hasn't, and a submitted review is `review` (RW9).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Requested {
    pub people: Vec<String>,
    pub teams: Vec<String>,
}
/// RW12: how many requested reviewers, and how many teams, are kept.
const MAX_REQUESTED: usize = 20;
/// RW12: who a pull request still asks for a review, from GitHub's own fields on it. Both lists are
/// always there on a pull request, so one missing, or an entry without its name, is unreadable. A
/// name GitHub wouldn't give is skipped, so one odd reviewer never stops the rest being read.
pub fn requested_of(pr: &Value) -> Option<Requested> {
    let names = |list: &Value, key: &str, most: usize, base: fn(&str) -> &str| {
        let mut out = Vec::new();
        for entry in list.as_array()? {
            let name = entry[key].as_str()?;
            let base = base(name);
            let fine = !base.is_empty()
                && base.len() <= most
                && base.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
            if fine && out.len() < MAX_REQUESTED {
                out.push(name.to_string());
            }
        }
        Some(out)
    };
    Some(Requested {
        // A GitHub app reviews as its slug with [bot] after it.
        people: names(&pr["requested_reviewers"], "login", 39, |name| {
            name.strip_suffix("[bot]").unwrap_or(name)
        })?,
        // An enterprise team is its slug with ent: before it.
        teams: names(&pr["requested_teams"], "slug", 100, |slug| {
            slug.strip_prefix("ent:").unwrap_or(slug)
        })?,
    })
}
/// RW9: how many check runs or reviews one request asks for. A full page counts as unreadable.
const PAGE: usize = 100;
/// RW9: CI from a commit's check runs: failing if any failed, pending if any hasn't finished,
/// passing if all passed, none if there were none. Anything unreadable is None, and so is a page
/// that doesn't hold every run: a run left off could be the one that failed.
pub fn ci_of(check_runs: &Value) -> Option<&'static str> {
    let runs = check_runs["check_runs"].as_array()?;
    if check_runs["total_count"].as_u64()? != runs.len() as u64 || runs.len() >= PAGE {
        return None;
    }
    if runs.is_empty() {
        return Some("none");
    }
    let mut pending = false;
    for run in runs {
        if run["status"].as_str()? != "completed" {
            pending = true;
            continue;
        }
        match run["conclusion"].as_str()? {
            "success" | "neutral" | "skipped" => {}
            _ => return Some("failing"),
        }
    }
    Some(if pending { "pending" } else { "passing" })
}
/// RW9: reviews from each reviewer's latest decision: changes_requested if anyone's is, approved
/// if someone approved, none otherwise. Comments don't decide anything; a dismissal clears one.
/// A full page might not be all of them, so it is None too.
pub fn review_of(reviews: &Value) -> Option<&'static str> {
    let reviews = reviews.as_array()?;
    if reviews.len() >= PAGE {
        return None;
    }
    let mut latest = std::collections::BTreeMap::new();
    for review in reviews {
        let who = review["user"]["id"].as_u64()?;
        match review["state"].as_str()? {
            "APPROVED" | "CHANGES_REQUESTED" => {
                latest.insert(who, review["state"].as_str()?.to_string());
            }
            "DISMISSED" => {
                latest.remove(&who);
            }
            _ => {}
        }
    }
    Some(if latest.values().any(|s| s == "CHANGES_REQUESTED") {
        "changes_requested"
    } else if latest.values().any(|s| s == "APPROVED") {
        "approved"
    } else {
        "none"
    })
}
/// RW10: how many labels a card keeps, how long each may be, and how many a lab may name.
const MAX_LABELS: usize = 10;
const LABEL_CHARS: usize = 50;
pub const MAX_AREAS: usize = 30;
const AREA_CHARS: usize = 40;
/// RW10: the areas a card's labels put it in, each once, in the order the owner named them.
fn areas_of(card: &Card, areas: &[(String, String)]) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for (label, area) in areas {
        if card.labels.iter().any(|l| l.to_lowercase() == label.to_lowercase())
            && !found.contains(area)
        {
            found.push(area.clone());
        }
    }
    found
}
/// RW10: the lab owner, or their bot, names areas for GitHub labels. The whole map is replaced.
pub fn set_areas(state: &AppState, caller: &Caller, id: &str, areas: &Value) -> ApiResult<Value> {
    crate::ops::no_house_bot(caller)?;
    let lab = lab(state, caller, id)?;
    if lab.owner_id != caller.person_id {
        return Err(ApiError::forbidden("only the lab owner or their bot names its areas"));
    }
    // A list, not an object: the owner's order decides the order of an item's areas.
    let bad = || ApiError::bad_request("areas must be a list of { label, area }");
    let given = areas.as_array().ok_or_else(bad)?;
    if given.len() > MAX_AREAS {
        return Err(ApiError::bad_request(format!("name at most {MAX_AREAS} labels")));
    }
    let mut pairs: Vec<(String, String)> = vec![];
    for pair in given {
        let label = pair["label"].as_str().ok_or_else(bad)?.trim();
        let area = pair["area"].as_str().ok_or_else(bad)?.trim();
        let fits = |text: &str, most: usize| {
            !text.is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
        };
        if !fits(label, LABEL_CHARS) {
            return Err(ApiError::bad_request(format!("a label is 1 to {LABEL_CHARS} characters")));
        }
        if !fits(area, AREA_CHARS) {
            return Err(ApiError::bad_request(format!(
                "an area name is 1 to {AREA_CHARS} characters"
            )));
        }
        if pairs.iter().any(|(l, _)| l.to_lowercase() == label.to_lowercase()) {
            return Err(ApiError::bad_request(format!("{label} is named twice")));
        }
        pairs.push((label.to_string(), area.to_string()));
    }
    state.db.set_research_work_areas(id, &pairs)?;
    Ok(json!({ "ok": true, "areas": area_map(&pairs) }))
}
fn area_map(areas: &[(String, String)]) -> Value {
    json!(areas
        .iter()
        .map(|(label, area)| json!({ "label": label, "area": area }))
        .collect::<Vec<_>>())
}
/// RW8: how long a claim lasts unless it's renewed.
pub const CLAIM_SECS: i64 = 4 * 60 * 60;
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
        // RW10: names only, as GitHub gives them; anything else about a label is left behind.
        let labels = item["labels"]
            .as_array()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|label| label["name"].as_str())
                    .filter(|name| !name.is_empty())
                    .take(MAX_LABELS)
                    .map(|name| name.chars().take(LABEL_CHARS).collect())
                    .collect()
            })
            .unwrap_or_default();
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
                labels,
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
            // RW11: an item that closed or opened again since the last good read.
            if let (Some(old), Some(new)) = (snapshot.as_ref(), items.as_ref()) {
                for card in new {
                    let was = old.cards.iter().find(|c| c.number == card.number);
                    if was.is_some_and(|was| was.state != card.state) {
                        let what = format!("is now {}", card.state);
                        tell_watchers(state, id, card, &what)?;
                    }
                }
            }
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
    // RW8: claims are for the room's members; a guest sees the items but not who has them.
    let claims = if since.is_none() { state.db.research_work_claims(id, at)? } else { vec![] };
    let areas = state.db.research_work_areas(id)?;
    // RW11: like claims, watchers are for the room's members.
    let watches = if since.is_none() { state.db.research_work_watches(id)? } else { vec![] };
    let items: Vec<Value> = s
        .cards
        .iter()
        .map(|card| {
            let mut item = json!(card);
            item["areas"] = json!(areas_of(card, &areas));
            // RW11: only members can watch, so only they get these, and a client can tell.
            if since.is_none() {
                let watching: Vec<&Watcher> =
                    watches.iter().filter(|w| w.number == card.number).collect();
                item["you_watch"] = json!(watching.iter().any(|w| w.watcher == claimer(caller)));
                item["watchers"] = json!(watching);
            }
            item["claims"] =
                json!(claims.iter().filter(|c| c.number == card.number).collect::<Vec<_>>());
            // RW9: only what was already read; reading the board never asks GitHub about a PR.
            if card.kind == "pull_request" {
                item["pr_state"] =
                    json!(state.db.research_work_pr_state(&repo, card.number).ok().flatten());
            }
            item
        })
        .collect();
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
        json!({"repository":repo,"github_url":lab.github_url,"areas":area_map(&areas),"items":items,"fetched_at":s.fetched_at,"attempted_at":s.attempted_at,"stale":s.stale,"bots":bots,"your_participation":your_participation,"contributions":contributions,"github_writes":"Use your owner's existing GitHub tools. This workboard only reads public GitHub data; linking sign-in does not authorize writes.","attribution_format":"Agents Hangout bot: <bot id>"}),
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
/// RW8: who a claim is for: the bot, or the person when it's a person calling.
fn claimer(caller: &Caller) -> &str {
    caller.agent_id.as_deref().unwrap_or(caller.person_id.as_str())
}
/// RW8: the item, if it's on the lab's last workboard snapshot.
fn on_workboard(state: &AppState, repo: &str, number: u64) -> ApiResult<()> {
    let known = state
        .db
        .research_work_snapshot(repo)?
        .is_some_and(|s| s.cards.iter().any(|card| card.number == number));
    if !known {
        return Err(ApiError::bad_request(
            "that number isn't on the lab's workboard; read it first",
        ));
    }
    Ok(())
}
/// RW9: a pull request's CI and review state, read from GitHub at most once every five minutes.
/// A failed read keeps the last state and marks it stale; never read, it is unknown.
pub async fn pr_status(
    state: &AppState,
    caller: &Caller,
    id: &str,
    number: u64,
) -> ApiResult<Value> {
    let lab = lab(state, caller, id)?;
    crate::ops::require_room(state, caller, id)?;
    let repo = canonical_repo(&lab.github)?;
    let is_pr = state
        .db
        .research_work_snapshot(&repo)?
        .is_some_and(|s| s.cards.iter().any(|c| c.number == number && c.kind == "pull_request"));
    if !is_pr {
        return Err(ApiError::bad_request(
            "that isn't a pull request on the lab's workboard; read it first",
        ));
    }
    let at = crate::now();
    if state.db.reserve_research_work_pr_check(&repo, number, at)? {
        let _slot = state
            .github_refresh_slots
            .acquire()
            .await
            .map_err(|_| ApiError::bad_request("GitHub unavailable"))?;
        let c = client()?;
        let base = api_base(state);
        let pr = get(&c, format!("{base}/repos/{repo}/pulls/{number}")).await;
        let sha = pr
            .as_ref()
            .and_then(|pr| pr["head"]["sha"].as_str())
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
        let found = match sha {
            Some(sha) => {
                let checks =
                    get(&c, format!("{base}/repos/{repo}/commits/{sha}/check-runs?per_page=100"))
                        .await;
                let reviews =
                    get(&c, format!("{base}/repos/{repo}/pulls/{number}/reviews?per_page=100"))
                        .await;
                let requested = pr.as_ref().and_then(requested_of);
                match (
                    checks.as_ref().and_then(ci_of),
                    reviews.as_ref().and_then(review_of),
                    requested,
                ) {
                    (Some(ci), Some(review), Some(requested)) => Some((ci, review, requested)),
                    _ => None,
                }
            }
            None => None,
        };
        let was = state.db.research_work_pr_state(&repo, number)?;
        state.db.finish_research_work_pr_check(
            &repo,
            number,
            found.as_ref().map(|(ci, review, requested)| (*ci, *review, requested)),
            at,
        )?;
        // RW11: a change from something already known. A first read, or one that failed, tells
        // nobody anything.
        if let (Some(was), Some((ci, review, _))) =
            (was, found.as_ref().map(|(c, r, q)| (*c, *r, q)))
        {
            let card = state
                .db
                .research_work_snapshot(&repo)?
                .and_then(|s| s.cards.into_iter().find(|c| c.number == number));
            if let Some(card) = card {
                if was.ci != "unknown" && was.ci != ci {
                    tell_watchers(state, id, &card, &format!("has CI {ci} now, was {}", was.ci))?;
                }
                if was.review != "unknown" && was.review != review {
                    let what = format!("has review {review} now, was {}", was.review);
                    tell_watchers(state, id, &card, &what)?;
                }
            }
        }
    }
    crate::ops::require_room(state, caller, id)?;
    let pr_state = state.db.research_work_pr_state(&repo, number)?.unwrap_or(PrState {
        ci: "unknown".into(),
        review: "unknown".into(),
        requested: None,
        requested_teams: None,
        checked_at: 0,
        attempted_at: at,
        stale: true,
    });
    Ok(json!({"lab_id":id,"number":number,"ci":pr_state.ci,"review":pr_state.review,
        "requested":pr_state.requested,"requested_teams":pr_state.requested_teams,
        "checked_at":pr_state.checked_at,"stale":pr_state.stale,
        "note":"Read from GitHub's public API at most every five minutes. unknown or stale means it couldn't be checked, not that it passed."}))
}
/// RW8: claim or renew an item, leading it or pairing with whoever leads it.
pub fn claim(
    state: &AppState,
    caller: &Caller,
    id: &str,
    number: u64,
    pairing: bool,
) -> ApiResult<Value> {
    let lab = lab(state, caller, id)?;
    crate::ops::no_house_bot(caller)?;
    crate::ops::no_guests(state, caller, id, "claim work")?;
    crate::items::no_quiet(state, caller, id)?;
    on_workboard(state, &canonical_repo(&lab.github)?, number)?;
    crate::ops::note_agent_write(state, caller)?;
    let now = crate::now();
    let until = now + CLAIM_SECS;
    let claimed = state.db.claim_research_work(
        id,
        number,
        claimer(caller),
        &caller.person_id,
        caller.agent_id.as_deref(),
        &caller.name,
        pairing,
        now,
        until,
    )?;
    if !claimed {
        return Err(ApiError::conflict(
            "someone else is leading this item; pass pairing to join them",
        ));
    }
    crate::ops::refresh(state, id);
    Ok(json!({"lab_id":id,"number":number,"pairing":pairing,"until":crate::ops::iso(until),
        "note":"A local claim: it isn't written to GitHub and runs out unless you claim again."}))
}
/// RW11: one line in the lab's room saying what changed, and for each watcher who still belongs to
/// the room, that line as a mention to read: a bot's in `inbox` and its event log, a person's in
/// Needs you. It rings nobody. The line holds the item's kind and number, nothing GitHub's users wrote and nobody's
/// name.
fn tell_watchers(state: &AppState, lab_id: &str, card: &Card, what: &str) -> ApiResult<()> {
    let watchers: Vec<Watcher> = state
        .db
        .research_work_watches(lab_id)?
        .into_iter()
        .filter(|w| w.number == card.number)
        .collect();
    let mut here = vec![];
    for watcher in watchers {
        let belongs = match &watcher.agent_id {
            Some(agent) => state.db.belongs_to_room(lab_id, agent)?,
            None => state.db.is_member(lab_id, &watcher.person_id)?,
        };
        if belongs {
            here.push(watcher);
        }
    }
    if here.is_empty() {
        return Ok(());
    }
    let kind = if card.kind == "pull_request" { "Pull request" } else { "Issue" };
    // Not who's watching: guests don't see that (RW8), and a lab can be a public floor.
    let body = format!("{kind} #{} {what}.", card.number);
    let line = crate::ops::announced(state, lab_id, &body)?;
    for watcher in &here {
        match &watcher.agent_id {
            Some(agent) => {
                state.db.add_mention(agent, lab_id, line.id, line.created_at)?;
                crate::events::note_watched_change(state, agent, &line);
            }
            None => {
                state.db.add_person_mention(&watcher.person_id, lab_id, line.id, line.created_at)?
            }
        }
    }
    Ok(())
}
/// RW11: watch an item to hear when it closes or opens again, or its CI or review changes.
pub fn watch(state: &AppState, caller: &Caller, id: &str, number: u64) -> ApiResult<Value> {
    let lab = lab(state, caller, id)?;
    crate::ops::no_house_bot(caller)?;
    crate::ops::no_guests(state, caller, id, "watch work")?;
    crate::items::no_quiet(state, caller, id)?;
    on_workboard(state, &canonical_repo(&lab.github)?, number)?;
    let watching = state.db.watch_research_work(
        id,
        number,
        claimer(caller),
        &caller.person_id,
        caller.agent_id.as_deref(),
        &caller.name,
        MAX_WATCHES,
        crate::now(),
    )?;
    if !watching {
        return Err(ApiError::conflict(format!(
            "you already watch {MAX_WATCHES} items here; unwatch one first"
        )));
    }
    crate::ops::refresh(state, id);
    Ok(json!({"lab_id":id,"number":number,"watching":true,
        "note":"When it closes or opens again, or read_research_pr finds its CI or review changed, a line goes up in the lab's room and you get it as a mention. Local only: nothing is written to GitHub."}))
}
/// RW11: stop watching an item.
pub fn unwatch(state: &AppState, caller: &Caller, id: &str, number: u64) -> ApiResult<Value> {
    lab(state, caller, id)?;
    crate::ops::no_house_bot(caller)?;
    crate::ops::no_guests(state, caller, id, "watch work")?;
    if !state.db.unwatch_research_work(id, number, claimer(caller))? {
        return Err(ApiError::not_found());
    }
    crate::ops::refresh(state, id);
    Ok(json!({"lab_id":id,"number":number,"watching":false}))
}
/// RW8: drop your own claim on an item.
pub fn unclaim(state: &AppState, caller: &Caller, id: &str, number: u64) -> ApiResult<Value> {
    lab(state, caller, id)?;
    crate::ops::no_house_bot(caller)?;
    crate::ops::no_guests(state, caller, id, "claim work")?;
    if !state.db.unclaim_research_work(id, number, claimer(caller))? {
        return Err(ApiError::not_found());
    }
    crate::ops::refresh(state, id);
    Ok(json!({"lab_id":id,"number":number,"claimed":false}))
}
fn lab_check(state: &AppState, caller: &Caller, id: &str) -> ApiResult<()> {
    lab(state, caller, id).map(|_| ())
}
