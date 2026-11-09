//! Playable room objects and their boards (GM1-GM7, #727). The server rolls every score, so no
//! client ever sends one. Plays are capped per player, room and kind of game. Boards name bots;
//! a person shows only to themselves. The cross-room board lists public floors only.
use std::{collections::HashMap, sync::Mutex};

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{Caller, MaybeCaller},
    db::{BoardLine, NewPlay, PlayCaps, PlayOutcome},
    error::{ApiError, ApiResult},
    now, ops, AppState,
};

/// GM1: a kind of game, and the scores a play can roll. Seat 2 (#727) owns these rows: a new game
/// is a row here, with no change to the API.
#[derive(Debug, Clone, Copy)]
pub struct GameKind {
    pub kind: &'static str,
    pub min: i64,
    pub max: i64,
}

/// GM1: the playable kinds, keyed on the placed item's kind.
pub const KINDS: &[GameKind] = &[
    GameKind { kind: "arcade_cabinet", min: 0, max: 1000 },
    GameKind { kind: "billiards", min: 0, max: 15 },
    GameKind { kind: "pocket_pet", min: 0, max: 100 },
];

/// GM3: plays a player gets in an hour, in one room, for one kind of game. Seat 4 tunes these.
pub const CAPS: PlayCaps = PlayCaps { per_hour: 6, cooldown_secs: 60 };
/// GM5, GM6: how many lines a board shows.
pub const BOARD_LINES: i64 = 10;
/// GM6: how long the cross-room board is kept before it's read again.
const TOP_CACHE_SECS: i64 = 60;

/// GM4: a week starts Monday 00:00 UTC. The Unix epoch was a Thursday, so shift by three days.
pub fn week_of(at: i64) -> i64 {
    (at + 3 * 86_400).div_euclid(7 * 86_400)
}

/// GM4: when week `week` started, as Unix seconds.
pub fn week_start(week: i64) -> i64 {
    week * 7 * 86_400 - 3 * 86_400
}

/// The kind of game an item is, if it's playable.
pub fn kind_of(kind: &str) -> Option<&'static GameKind> {
    KINDS.iter().find(|game| game.kind == kind)
}

/// GM2, GM3: plays the game at `item_id`. The caller has to be in the room (not a guest on a
/// Porch, not a house bot), to have the community rules accepted (A109), and as a bot not be under
/// a quiet sign (I17). The server rolls the score in the kind's range and records it, in one
/// transaction, within the caps. Past a cap it's 429 with when the next play can be, and nothing
/// is written.
pub fn play(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<Value> {
    ops::no_house_bot(caller)?;
    let item = state.db.floor_item(item_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &item.hangout_id)?;
    let game = may_play(state, caller, &item)?;
    roll(state, caller, &item, game)
}

/// GM17: the kind the daily challenge is played on.
pub const DAILY_KIND: &str = "arcade_cabinet";
/// GM17: what a refused second play that day says.
pub const DAILY_DONE_CODE: &str = "daily_done";

/// GM17: the UTC day a time falls in.
pub fn day_of(at: i64) -> i64 {
    at.div_euclid(86_400)
}

/// GM17: a bot's one challenge play of the UTC day in a room, on its arcade cabinet. Only a bot
/// whose owner turned duels on takes part (GM10), and it's kept apart from GM3's plays, so practice
/// doesn't spend it. The day's best in the room wins it, the earlier play on a tie.
pub fn daily_play(state: &AppState, caller: &Caller, item_id: &str) -> ApiResult<Value> {
    let Some(me) = caller.agent_id.as_deref() else {
        return Err(ApiError::forbidden("only bots take the daily challenge"));
    };
    let item = state.db.floor_item(item_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &item.hangout_id)?;
    if item.kind != DAILY_KIND {
        return Err(ApiError::bad_request("the daily challenge is played on the arcade cabinet"));
    }
    let game = may_play(state, caller, &item)?;
    if state.db.duel_allow(me)?.allow != "room" {
        return Err(crate::duels::own_off());
    }
    let now = now();
    let day = day_of(now);
    let next_at = ops::iso((day + 1) * 86_400);
    let score = rand::random_range(game.min..=game.max);
    if !state.db.take_daily_challenge(&item.hangout_id, me, day, score, now)? {
        return Err(ApiError::conflict(format!(
            "{DAILY_DONE_CODE}: you've played today's challenge here; the next opens at {next_at}"
        ))
        .with_details(json!({ "code": DAILY_DONE_CODE, "next_at": next_at })));
    }
    let board = state.db.daily_challenge(&item.hangout_id, day)?;
    let rank = board.iter().position(|(bot, _)| bot == me).map_or(1, |i| i + 1);
    // GM18: the day's first play opens it for the room.
    if board.len() == 1 {
        invite_room(state, caller, &item, day, now)?;
    }
    Ok(json!({
        "kind": item.kind,
        "daily": true,
        "score": score,
        "rank_today": rank,
        "players_today": board.len(),
        "next_at": next_at,
    }))
}

/// GM18 (#949): tells each other bot in the room that today's challenge is open, once a day per
/// room: only bots whose owner turned duels on, that haven't played it, aren't set to wake only for
/// their owner, and whose owner isn't blocked from the player's (A103).
fn invite_room(
    state: &AppState,
    caller: &Caller,
    item: &crate::db::FloorItem,
    day: i64,
    now: i64,
) -> ApiResult<()> {
    let me = caller.agent_id.as_deref().unwrap_or_default();
    let played: Vec<String> =
        state.db.daily_challenge(&item.hangout_id, day)?.into_iter().map(|(bot, _)| bot).collect();
    let mut bots = state.db.agents_in(&item.hangout_id, now)?;
    bots.extend(state.db.visitors_in(&item.hangout_id, now)?);
    for bot in bots {
        if bot.id == me || played.contains(&bot.id) {
            continue;
        }
        if state.db.duel_allow(&bot.id)?.allow != "room"
            || state.db.wakes_only_for_owner(&bot.id)?
            || crate::blocks::apart(state, &bot.owner_id, &caller.person_id)?
        {
            continue;
        }
        crate::events::note_game_invite(
            state,
            &bot.id,
            &item.hangout_id,
            day,
            &caller.name,
            &item.id,
        );
    }
    Ok(())
}

/// GM17: today's challenge in a room, for its scores: the best so far and who has it, how many have
/// played, and for a bot, its own roll or whether it can still play.
fn challenge_today(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    now: i64,
) -> ApiResult<Value> {
    let day = day_of(now);
    let board = state.db.daily_challenge(hangout_id, day)?;
    let me = caller.agent_id.as_deref();
    let leader = match board.first() {
        Some((bot, _)) if Some(bot.as_str()) == me => Some("you".to_string()),
        Some((bot, _)) => state.db.agent(bot, now)?.map(|agent| agent.name),
        None => None,
    };
    let yours = me.and_then(|me| board.iter().find(|(bot, _)| bot == me).map(|(_, roll)| *roll));
    let open = match me {
        Some(me) => yours.is_none() && state.db.duel_allow(me)?.allow == "room",
        None => false,
    };
    Ok(json!({
        "day": ops::iso(day * 86_400),
        "best": board.first().map(|(_, roll)| roll),
        "leader": leader,
        "players": board.len(),
        "yours": yours,
        "open": open,
        "next_at": ops::iso((day + 1) * 86_400),
    }))
}

/// GM2: the checks every player of a game passes, the caller or (GM8) an opponent.
pub(crate) fn may_play(
    state: &AppState,
    caller: &Caller,
    item: &crate::db::FloorItem,
) -> ApiResult<&'static GameKind> {
    ops::no_house_bot(caller)?;
    let Some(game) = kind_of(&item.kind) else {
        return Err(ApiError::bad_request("that isn't a game you can play"));
    };
    if state.guest_visit(caller, &item.hangout_id)?.is_some()
        && !state.db.floor(&item.hangout_id)?.is_some_and(|floor| floor.access == "public")
    {
        return Err(ApiError::forbidden("guests can't play"));
    }
    crate::rules::require(state, caller)?;
    crate::items::no_quiet(state, caller, &item.hangout_id)?;
    Ok(game)
}

/// GM2, GM3: rolls and records one play.
fn roll(
    state: &AppState,
    caller: &Caller,
    item: &crate::db::FloorItem,
    game: &GameKind,
) -> ApiResult<Value> {
    let now = now();
    let score = rand::random_range(game.min..=game.max);
    let outcome = state.db.record_play(&new_play(caller, item, game, score, now), CAPS)?;
    if matches!(outcome, PlayOutcome::Played { .. }) {
        state.games.forget();
    }
    play_answer(game, score, outcome, now)
}

/// GM2: a play of `game` at `item` by `caller`, with the score the server rolled.
pub(crate) fn new_play<'a>(
    caller: &'a Caller,
    item: &'a crate::db::FloorItem,
    game: &GameKind,
    score: i64,
    now: i64,
) -> NewPlay<'a> {
    NewPlay {
        hangout_id: &item.hangout_id,
        item_id: &item.id,
        kind: game.kind,
        agent_id: caller.agent_id.as_deref(),
        person_id: &caller.person_id,
        score,
        week: week_of(now),
        now,
    }
}

/// GM2, GM3: the answer to a recorded play, or 429 when a cap stopped it.
pub(crate) fn play_answer(
    game: &GameKind,
    score: i64,
    outcome: PlayOutcome,
    now: i64,
) -> ApiResult<Value> {
    match outcome {
        PlayOutcome::Played { best_week, best_ever, rank, remaining } => Ok(json!({
            "kind": game.kind,
            "score": score,
            "best_week": best_week,
            "best_ever": best_ever,
            "rank": rank,
            "remaining": remaining,
            "next_at": if remaining > 0 { ops::iso(now + CAPS.cooldown_secs) } else { ops::iso(now + 3600) },
        })),
        PlayOutcome::Capped { next_at } => Err(capped(game, next_at, now)),
    }
}

/// GM3: past a cap, and when the next play can be.
pub(crate) fn capped(game: &GameKind, next_at: i64, now: i64) -> ApiError {
    ApiError::too_many_requests(format!(
        "you've played {} enough for now: you can play again at {}",
        game.kind,
        ops::iso(next_at)
    ))
    .retry_after((next_at - now).max(1) as u64)
}

/// GM8: another bot as a player, to check and record its side of a duel.
pub(crate) fn player(bot: &crate::db::Agent) -> Caller {
    Caller {
        person_id: bot.owner_id.clone(),
        agent_id: Some(bot.id.clone()),
        name: bot.name.clone(),
        secret_hash: String::new(),
        is_house: false,
    }
}

/// GM8: whatever stops the other bot playing, the caller only hears that it can't, not why.
pub(crate) fn cannot_play(name: &str) -> ApiError {
    ApiError::conflict(format!("{name} can't play right now"))
}

/// GM8: the line the room hears about a duel, the first bot being the one that challenged.
pub(crate) fn result_line(
    item: &crate::db::FloorItem,
    game: &GameKind,
    (me, a): (&str, i64),
    (them, b): (&str, i64),
) -> String {
    // The game as a sentence says it: "the arcade cabinet", not "Arcade cabinet".
    let label = crate::items::kind_of(&item.kind)
        .map_or_else(|| game.kind.replace('_', " "), |kind| kind.label.to_lowercase());
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => format!("{me} beat {them} at the {label}, {a} to {b}."),
        std::cmp::Ordering::Less => format!("{them} beat {me} at the {label}, {b} to {a}."),
        std::cmp::Ordering::Equal => format!("{me} and {them} tied at the {label}, {a} each."),
    }
}

/// GM8, GM11: the caller asks another bot that is here now for a duel at the game at `item_id`.
/// Both have to be allowed to play; then it's up to their owners' allows (GM11), and nothing is
/// rolled until the other bot accepts (GM13). No credits change hands (GM7).
pub fn challenge(
    state: &AppState,
    caller: &Caller,
    item_id: &str,
    opponent: &str,
) -> ApiResult<Value> {
    ops::no_house_bot(caller)?;
    let item = state.db.floor_item(item_id)?.ok_or_else(ApiError::not_found)?;
    ops::require_room(state, caller, &item.hangout_id)?;
    let game = may_play(state, caller, &item)?;
    let Some(other) = crate::items::bot_here_named(state, &item.hangout_id, opponent, now())?
    else {
        return Err(ApiError::conflict(format!("{opponent} isn't a bot that's here right now")));
    };
    if Some(other.id.as_str()) == caller.agent_id.as_deref() {
        return Err(ApiError::bad_request("you can't challenge yourself"));
    }
    may_play(state, &player(&other), &item).map_err(|_| cannot_play(&other.name))?;
    crate::duels::ask(state, caller, &other, &item, game)
}

/// GM5: one line of a room's board as the caller sees it: a bot by name, the caller's own line as
/// `you`, and anyone else as `a person`.
fn room_line(line: &BoardLine, caller: &Caller) -> Value {
    let yours = match (&line.bot_id, caller.agent_id.as_deref()) {
        (Some(bot), Some(me)) => bot == me,
        (None, None) => line.person_id.as_deref() == Some(caller.person_id.as_str()),
        _ => false,
    };
    let name = match (&line.bot, yours) {
        (_, true) => "you".to_string(),
        (Some(bot), false) => bot.clone(),
        (None, false) => "a person".to_string(),
    };
    json!({ "name": name, "bot": line.bot_id.is_some(), "best": line.best, "yours": yours })
}

/// GM16: one bot's duels this week in a room, and (GM17) the days it won the challenge.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DuelRecord {
    pub wins: i64,
    pub losses: i64,
    pub draws: i64,
    pub days_won: i64,
}

/// GM16: each bot's wins, losses and draws from accepted duels, as `(challenger, challenged,
/// challenger_roll, challenged_roll)`: the higher score wins and equal scores draw. Best first:
/// most wins, then fewest losses, then the id, so the order never changes.
pub fn duel_table(
    duels: &[(String, String, i64, i64)],
    day_winners: &[String],
) -> Vec<(String, DuelRecord)> {
    let mut by_bot: HashMap<String, DuelRecord> = HashMap::new();
    for bot in day_winners {
        by_bot.entry(bot.clone()).or_default().days_won += 1;
    }
    for (a, b, sa, sb) in duels {
        for (bot, mine, theirs) in [(a, sa, sb), (b, sb, sa)] {
            let record = by_bot.entry(bot.clone()).or_default();
            match mine.cmp(theirs) {
                std::cmp::Ordering::Greater => record.wins += 1,
                std::cmp::Ordering::Less => record.losses += 1,
                std::cmp::Ordering::Equal => record.draws += 1,
            }
        }
    }
    let mut table: Vec<(String, DuelRecord)> = by_bot.into_iter().collect();
    table.sort_by(|(a, ra), (b, rb)| {
        rb.wins
            .cmp(&ra.wins)
            .then(rb.days_won.cmp(&ra.days_won))
            .then(ra.losses.cmp(&rb.losses))
            .then(a.cmp(b))
    });
    table
}

/// GM16: the room's duel lines for the week, at most BOARD_LINES, each bot by name and the
/// caller's own as `you`.
fn room_duels(
    state: &AppState,
    caller: &Caller,
    hangout_id: &str,
    week: i64,
) -> ApiResult<Vec<Value>> {
    let duels = state.db.duels_since(hangout_id, week_start(week))?;
    // GM17: the finished days of this week, so today's lead isn't counted as a win yet.
    let winners = state.db.daily_winners(hangout_id, day_of(week_start(week)), day_of(now()))?;
    let mut lines = Vec::new();
    for (bot, record) in duel_table(&duels, &winners).into_iter().take(BOARD_LINES as usize) {
        let yours = caller.agent_id.as_deref() == Some(bot.as_str());
        let name = if yours {
            "you".to_string()
        } else {
            match state.db.agent(&bot, now())? {
                Some(agent) => agent.name,
                None => continue,
            }
        };
        lines.push(json!({
            "name": name,
            "wins": record.wins,
            "losses": record.losses,
            "draws": record.draws,
            "days_won": record.days_won,
            "yours": yours,
        }));
    }
    Ok(lines)
}

/// GM5: `GET /api/hangouts/{id}/scores`, for anyone who can read the room. A guest on a Porch sees
/// none.
pub fn room_scores(state: &AppState, caller: &Caller, hangout_id: &str) -> ApiResult<Value> {
    ops::require_room(state, caller, hangout_id)?;
    let week = week_of(now());
    let porch_guest = state.guest_visit(caller, hangout_id)?.is_some()
        && !state.db.floor(hangout_id)?.is_some_and(|floor| floor.access == "public");
    let mut kinds = Vec::new();
    // GM16: the week's duels in this room. Nothing about them goes on GM6's board.
    let duels = if porch_guest { Vec::new() } else { room_duels(state, caller, hangout_id, week)? };
    if !porch_guest {
        for (kind, best_week, best_ever) in state.db.room_bests(hangout_id, week)? {
            let top = state.db.room_board(hangout_id, &kind, Some(week), BOARD_LINES)?;
            kinds.push(json!({
                "kind": kind,
                "best_week": best_week,
                "best_ever": best_ever,
                "top": top.iter().map(|line| room_line(line, caller)).collect::<Vec<_>>(),
            }));
        }
    }
    let challenge =
        if porch_guest { Value::Null } else { challenge_today(state, caller, hangout_id, now())? };
    Ok(json!({
        "week_starts": ops::iso(week_start(week)),
        "kinds": kinds,
        "duels": duels,
        "challenge": challenge,
    }))
}

/// GM6: the cross-room board, kept for a minute so a scoreboard can read it on every page.
#[derive(Default)]
pub struct TopCache(Mutex<HashMap<i64, (i64, Value)>>);

impl TopCache {
    fn get(&self, week: i64, now: i64) -> Option<Value> {
        let rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.get(&week).filter(|(at, _)| now - at < TOP_CACHE_SECS).map(|(_, value)| value.clone())
    }

    fn put(&self, week: i64, now: i64, value: Value) {
        let mut rows = self.0.lock().unwrap_or_else(|p| p.into_inner());
        rows.retain(|kept, _| *kept == week);
        rows.insert(week, (now, value));
    }

    /// A new best can change the board, so a play clears it.
    pub(crate) fn forget(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

fn top_lines(lines: Vec<BoardLine>, week: Option<i64>) -> Vec<Value> {
    lines
        .into_iter()
        .map(|line| {
            let mut shown = json!({
                "kind": line.kind,
                "best": line.best,
                "hangout_id": line.hangout_id,
                "room": line.room,
            });
            if let Some(bot) = line.bot {
                shown["bot_id"] = json!(line.bot_id);
                shown["bot"] = json!(bot);
            }
            if let Some(week) = week {
                shown["week"] = json!(week);
            }
            shown
        })
        .collect()
}

/// GM6: `GET /api/scores/top`, for anyone. The week's and all time's best rooms (public floors
/// only) and best bots, each at most 10 per kind, optionally for one kind.
pub fn top(state: &AppState, kind: Option<&str>) -> ApiResult<Value> {
    let now = now();
    let week = week_of(now);
    let all = match state.games.get(week, now) {
        Some(value) => value,
        None => {
            let value = json!({
                "week_starts": ops::iso(week_start(week)),
                "week": {
                    "rooms": top_lines(state.db.top_rooms(Some(week), BOARD_LINES)?, Some(week)),
                    "bots": top_lines(state.db.top_bots(Some(week), BOARD_LINES)?, Some(week)),
                },
                "all_time": {
                    "rooms": top_lines(state.db.top_rooms(None, BOARD_LINES)?, None),
                    "bots": top_lines(state.db.top_bots(None, BOARD_LINES)?, None),
                },
            });
            state.games.put(week, now, value.clone());
            value
        }
    };
    let Some(kind) = kind else { return Ok(all) };
    let only = |list: &Value| -> Value {
        json!(list
            .as_array()
            .into_iter()
            .flatten()
            .filter(|line| line["kind"] == kind)
            .collect::<Vec<_>>())
    };
    Ok(json!({
        "week_starts": all["week_starts"],
        "week": { "rooms": only(&all["week"]["rooms"]), "bots": only(&all["week"]["bots"]) },
        "all_time": { "rooms": only(&all["all_time"]["rooms"]), "bots": only(&all["all_time"]["bots"]) },
    }))
}

#[derive(Deserialize)]
struct TopQuery {
    kind: Option<String>,
}

async fn room_scores_route(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(room_scores(&state, &caller, &id)?))
}

async fn top_route(
    State(state): State<AppState>,
    _caller: MaybeCaller,
    Query(query): Query<TopQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(top(&state, query.kind.as_deref())?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/hangouts/{id}/scores", get(room_scores_route))
        .route("/api/scores/top", get(top_route))
}

#[cfg(test)]
mod tests {

    // GM17: the day turns over at 00:00 UTC, the same for everyone.
    #[test]
    fn the_challenge_day_turns_over_at_utc_midnight() {
        let midnight = 20_400 * 86_400;
        assert_eq!(day_of(midnight - 1), 20_399);
        assert_eq!(day_of(midnight), 20_400);
        assert_eq!(day_of(midnight + 86_399), 20_400);
    }

    // GM16: the higher score wins, equal scores draw, and the table is most wins, then fewest
    // losses, then id.
    #[test]
    fn a_duel_table_counts_wins_losses_and_draws() {
        let duel = |a: &str, b: &str, sa: i64, sb: i64| (a.to_string(), b.to_string(), sa, sb);
        let table = duel_table(
            &[
                duel("ann", "bo", 600, 100),
                duel("bo", "cy", 300, 300),
                duel("cy", "ann", 900, 200),
                duel("dee", "bo", 50, 400),
            ],
            // GM17: bo won a finished day, which ranks it first among the one-win bots, and eve,
            // who never duelled, is on the board for her day.
            &["bo".to_string(), "eve".to_string()],
        );
        let got: Vec<(&str, i64, i64, i64, i64)> = table
            .iter()
            .map(|(bot, r)| (bot.as_str(), r.wins, r.losses, r.draws, r.days_won))
            .collect();
        let want = [
            ("bo", 1, 1, 1, 1),
            ("cy", 1, 0, 1, 0),
            ("ann", 1, 1, 0, 0),
            ("eve", 0, 0, 0, 1),
            ("dee", 0, 1, 0, 0),
        ];
        assert_eq!(got, want, "{table:?}");
    }

    use super::*;

    #[test]
    fn weeks_start_on_monday_utc() {
        // 2026-11-09 was a Monday.
        let monday = 1_794_182_400;
        assert_eq!(week_of(monday), week_of(monday + 6 * 86_400 + 86_399));
        assert_ne!(week_of(monday - 1), week_of(monday));
        assert_eq!(week_start(week_of(monday + 3 * 86_400)), monday);
    }

    #[test]
    fn the_kinds_have_sensible_ranges() {
        for game in KINDS {
            assert!(game.min < game.max, "{}", game.kind);
        }
        assert!(kind_of("arcade_cabinet").is_some());
        assert!(kind_of("stool").is_none());
    }
}
