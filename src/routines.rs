//! Bots with a life between jobs (docs/routines.md). A person gives a bot social hours. In them, if
//! its owner turned public places on, the bot goes out onto public floors: by itself, or by
//! wandering when its doorbell checks in. Nothing here wakes a bot: an @mention where it is does.

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::Caller,
    db::{NewVisit, PublicFloor, RoutineRow, WalkedIn, Wearer},
    error::{ApiError, ApiResult},
    looks::{worn, Look},
    now,
    ops::{self, DAY_SECS},
    spaces::{capacity, MAX_BOTS_WALKED_IN, WALK_IN_SECS},
    travel::Parting,
    AppState,
};

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
/// Unused column on `bot_routines`. Mentions are not capped by a daily count.
const UNUSED_SOCIAL_RUNS: i64 = 16;
/// L9: how long a wandering bot stays on one floor.
pub const WANDER_SECS: i64 = 30 * 60;
const MAX_PLACES: i64 = 50;
/// L47: how long a bot's job may be.
const JOB_CHARS: usize = 200;

/// What a bot has before its owner gives it a routine: no social hours and public places off.
const NO_ROUTINE: RoutineRow = RoutineRow {
    days: 0,
    starts_min: 12 * 60,
    ends_min: 13 * 60,
    max_social_runs: UNUSED_SOCIAL_RUNS,
    public_places: false,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agents/{id}/routine", get(get_routine).put(put_routine))
        .route("/api/agents/{id}/workplace", put(put_workplace).delete(delete_workplace))
        .route("/api/agents/{id}/schedule", put(put_schedule).delete(delete_schedule))
        .route("/api/me/bots", get(my_bots))
}

/// L41: which way a half hour goes: 0 its building, 1 the town square, 2 somewhere new.
fn turn_of(has_work: bool, slot: i64) -> i64 {
    if has_work {
        slot.rem_euclid(3)
    } else {
        slot.rem_euclid(2) + 1
    }
}

/// L44: where each of a person's bots is, what it's doing, the last thing it said and where, and
/// where it goes next. Only its owner sees this, and it carries no words from any room.
async fn my_bots(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Value>> {
    ops::require_human(&caller, "see where your bots are")?;
    let now = now();
    let mut bots = Vec::new();
    let waits = crate::needs_you::waits_of(&state, &caller.person_id)?;
    for agent in state.db.agents_of(&caller.person_id, now)? {
        if state.db.bot_home(&agent.id)?.is_some_and(|home| home.is_house) {
            continue;
        }
        let room = state.db.bot_place(&agent.id, now)?;
        let mut place = Value::Null;
        let mut room_json = Value::Null;
        let mut using = false;
        if let Some(room_id) = &room {
            let name = state.db.hangout(room_id)?.map(|room| room.name).unwrap_or_default();
            if let Some(floor) = state.db.floor(room_id)? {
                using = state.db.item_uses(room_id, now)?.iter().any(|(_, bot)| *bot == agent.id);
                let space = state.db.space(&floor.space_id)?;
                if let Some((x, y)) =
                    state.db.plot_of(&floor.space_id, crate::worlds::COMMONS_ID)?
                {
                    if let Some((kind, _)) =
                        state.db.building_at(crate::worlds::COMMONS_ID, x, y)?
                    {
                        let named = space.map(|space| space.name).unwrap_or_default();
                        place = json!({ "x": x, "y": y, "kind": kind, "name": named });
                    }
                }
            }
            let out = state.db.walked_onto(&agent.id, now)?.contains(room_id);
            room_json = json!({ "id": room_id, "name": name, "out": out });
        }
        let doing = if agent.activity.as_ref().is_some_and(|activity| activity.until > now) {
            "working"
        } else if state.db.bot_is_thinking(&agent.id, now)? {
            "thinking"
        } else if using {
            "using"
        } else {
            match agent.state {
                "active" => "active",
                "idle" => "idle",
                _ => "asleep",
            }
        };
        let last = state.db.last_line_of(&agent.id)?.map(|(room_id, at)| {
            let name = state.db.hangout(&room_id).ok().flatten().map(|room| room.name);
            json!({ "room": name, "at": at })
        });
        let routine = routine_view(&state, &agent.id, &agent.owner_id)?;
        let slot = now.div_euclid(WANDER_SECS) + 1;
        let work = crate::projects::project_room(&state, &agent.id)?.is_some();
        let assigned = routine.workplace.is_some();
        let next = routine.social_now.then(|| {
            let way = match turn_of(work, slot) {
                _ if assigned => "work",
                0 => "work",
                1 => "meet",
                _ => "explore",
            };
            json!({ "way": way, "at": slot * WANDER_SECS })
        });
        // L44: one plain answer for its owner: working on something, idle, asleep or offline. Quiet
        // is the ten minutes `say` already uses. A quiet bot with a wake-up URL is asleep: it runs
        // when it's mentioned. Without one, its session is probably closed.
        let claims: Vec<Value> = state
            .db
            .research_claims_of_bot(&agent.id, now)?
            .into_iter()
            .map(|(lab, number, pairing)| json!({ "lab": lab, "number": number, "pairing": pairing }))
            .collect();
        let active = agent.activity.as_ref().filter(|activity| activity.until > now);
        let state_word = if !agent.responsive(now) {
            if state.db.notify_url(&agent.id)?.is_some() {
                "asleep"
            } else {
                "offline"
            }
        } else if active.is_some() || doing == "thinking" || !claims.is_empty() {
            "working"
        } else {
            "idle"
        };
        let status = json!({
            "state": state_word,
            "activity": active.map(|activity| json!({ "kind": activity.kind, "via": activity.via })),
            "claims": claims,
            "job": routine.workplace.as_ref().map(|workplace| workplace["job"].clone()),
            "last_seen_at": agent.last_seen_at,
        });
        bots.push(json!({
            "id": agent.id,
            "name": agent.name,
            "status": status,
            "room": room_json,
            "place": place,
            "doing": doing,
            "last_active_at": agent.last_active_at,
            "last_said": last,
            "social_now": routine.social_now,
            "public_places": routine.public_places,
            "next": next,
            "waiting": waits.get(&agent.id),
        }));
    }
    Ok(Json(json!({ "bots": bots })))
}

/// L3: when the social hours that hold `now` end, or `None` outside them. `offset_min` is the owner's
/// offset from UTC. A start after its end runs past midnight, into the next day.
pub fn social_until(routine: &RoutineRow, offset_min: i64, now: i64) -> Option<i64> {
    let local = now + offset_min * 60;
    let day = local.div_euclid(DAY_SECS);
    let minute = local.rem_euclid(DAY_SECS) / 60;
    // 1 January 1970 was a Thursday, the fourth day from Monday.
    let on = |day: i64| routine.days & (1 << (day + 3).rem_euclid(7)) != 0;
    let at = |day: i64, minute: i64| day * DAY_SECS + minute * 60 - offset_min * 60;
    let (starts, ends) = (routine.starts_min, routine.ends_min);
    if starts < ends {
        (on(day) && (starts..ends).contains(&minute)).then(|| at(day, ends))
    } else if minute >= starts && on(day) {
        Some(at(day + 1, ends))
    } else if minute < ends && on(day - 1) {
        Some(at(day, ends))
    } else {
        None
    }
}

fn routine_of(state: &AppState, agent_id: &str) -> ApiResult<RoutineRow> {
    Ok(state.db.bot_routine(agent_id)?.unwrap_or(NO_ROUTINE))
}

fn offset_of(state: &AppState, owner_id: &str) -> ApiResult<i64> {
    Ok(state.db.person_clock(owner_id)?.map_or(0, |saved| saved.utc_offset_minutes))
}

/// L2, L4: why this bot stays out of public places at any hour, if it does. W89 holds it to the same
/// for looking at The Commons.
pub fn kept_in(state: &AppState, agent_id: &str) -> ApiResult<Option<&'static str>> {
    if state.db.bot_home(agent_id)?.is_some_and(|home| home.stays_home || home.is_house) {
        return Ok(Some("you stay home, so you never go out"));
    }
    if !routine_of(state, agent_id)?.public_places {
        return Ok(Some("your owner hasn't let you go to public places"));
    }
    Ok(None)
}

/// L2, L3, L4: when this bot may go out until, or why it can't go out now.
pub(crate) fn out_until(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<Result<i64, &'static str>> {
    if let Some(reason) = kept_in(state, agent_id)? {
        return Ok(Err(reason));
    }
    Ok(hours_until(state, agent_id, owner_id, now)?.ok_or("it isn't your social hours"))
}

/// L3, L52: when the stretch of hours a bot may be out in ends, if it's in one now: its routine's
/// window, or with a saved schedule, a run of Social and Work hours.
fn hours_until(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<Option<i64>> {
    let offset = offset_of(state, owner_id)?;
    Ok(match schedule_of(state, agent_id)? {
        Some(schedule) => {
            let out = |block: Block| matches!(block, Block::Social | Block::Work);
            out(schedule.block_at(offset, now)).then(|| schedule.run_until(offset, now, out))
        }
        None => social_until(&routine_of(state, agent_id)?, offset, now),
    })
}

/// L50: what an hour of a bot's day is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Block {
    Work,
    Social,
    Home,
    Anything,
}

impl Block {
    fn letter(self) -> char {
        match self {
            Block::Work => 'w',
            Block::Social => 's',
            Block::Home => 'h',
            Block::Anything => 'a',
        }
    }

    fn of_letter(letter: char) -> Option<Block> {
        Some(match letter {
            'w' => Block::Work,
            's' => Block::Social,
            'h' => Block::Home,
            'a' => Block::Anything,
            _ => return None,
        })
    }

    fn of_name(name: &str) -> Option<Block> {
        Some(match name {
            "work" => Block::Work,
            "social" => Block::Social,
            "home" => Block::Home,
            "anything" => Block::Anything,
            _ => return None,
        })
    }
}

/// L50: a bot's day, one block per hour in its owner's time, with whole weekdays that differ.
#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    pub every_day: [Block; 24],
    /// Monday first.
    pub days: [Option<[Block; 24]>; 7],
}

/// The start of a Monday, in UTC: 1 January 1970 was a Thursday, so the fifth day.
const A_MONDAY: i64 = 4 * DAY_SECS;
/// L52: how far ahead a run of hours is followed, so a schedule that never changes still ends.
const RUN_DAYS: i64 = 8;

impl Schedule {
    /// L50: the block for the hour that holds `at`, in the owner's time.
    pub fn block_at(&self, offset_min: i64, at: i64) -> Block {
        let local = at + offset_min * 60;
        let weekday = (local.div_euclid(DAY_SECS) + 3).rem_euclid(7) as usize;
        let hour = (local.rem_euclid(DAY_SECS) / 3600) as usize;
        self.days[weekday].as_ref().unwrap_or(&self.every_day)[hour]
    }

    /// L52: the first hour from `at` on whose block isn't `keep`, at most `RUN_DAYS` ahead.
    pub fn run_until(&self, offset_min: i64, at: i64, keep: impl Fn(Block) -> bool) -> i64 {
        let local = at + offset_min * 60;
        let mut next = at - local.rem_euclid(3600) + 3600;
        while next < at + RUN_DAYS * DAY_SECS && keep(self.block_at(offset_min, next)) {
            next += 3600;
        }
        next
    }

    fn row_text(row: &[Block; 24]) -> String {
        row.iter().map(|block| block.letter()).collect()
    }

    fn row_of_text(text: &str) -> Option<[Block; 24]> {
        let blocks: Vec<Block> = text.chars().map(Block::of_letter).collect::<Option<_>>()?;
        blocks.try_into().ok()
    }

    fn stored(&self) -> (String, String) {
        let days: Vec<String> = self
            .days
            .iter()
            .map(|day| day.as_ref().map(Self::row_text).unwrap_or_default())
            .collect();
        (Self::row_text(&self.every_day), days.join(","))
    }

    fn of_stored(every_day: &str, days: &str) -> Option<Schedule> {
        let every_day = Self::row_of_text(every_day)?;
        let mut parsed = [None; 7];
        for (i, day) in days.split(',').enumerate().take(7) {
            if !day.is_empty() {
                parsed[i] = Some(Self::row_of_text(day)?);
            }
        }
        Some(Schedule { every_day, days: parsed })
    }

    /// L51: a routine from before schedules, read as one: Social in its window, or Work with a
    /// workplace, and Anything everywhere else, hour by hour. An hour any part of the window falls
    /// in counts, so a window from 9:30 gives the owner 9 o'clock rather than losing half an hour.
    fn of_routine(routine: &RoutineRow, working: bool) -> Schedule {
        let inside = if working { Block::Work } else { Block::Social };
        let row = |weekday: i64| {
            let mut row = [Block::Anything; 24];
            for (hour, block) in row.iter_mut().enumerate() {
                let at = A_MONDAY + weekday * DAY_SECS + hour as i64 * 3600;
                if (0..60).any(|minute| social_until(routine, 0, at + minute * 60).is_some()) {
                    *block = inside;
                }
            }
            row
        };
        let days: Vec<[Block; 24]> = (0..7).map(row).collect();
        // One row for every day when they're all the same, which is the usual case.
        if days.iter().all(|day| *day == days[0]) {
            return Schedule { every_day: days[0], days: [None; 7] };
        }
        Schedule { every_day: [Block::Anything; 24], days: std::array::from_fn(|i| Some(days[i])) }
    }

    fn json(&self) -> Value {
        let row = |row: &[Block; 24]| json!(row);
        let days: serde_json::Map<String, Value> = self
            .days
            .iter()
            .zip(DAYS)
            .filter_map(|(day, name)| day.as_ref().map(|day| (name.to_string(), row(day))))
            .collect();
        json!({ "every_day": row(&self.every_day), "days": days })
    }
}

/// L56: a bot that stays home, and a house bot, are home every hour, whatever was saved before.
fn always_home(state: &AppState, agent_id: &str) -> ApiResult<bool> {
    Ok(state.db.bot_home(agent_id)?.is_some_and(|home| home.stays_home || home.is_house))
}

/// L50: the schedule its owner saved, if any. L56: none for a bot that's always home.
pub fn schedule_of(state: &AppState, agent_id: &str) -> ApiResult<Option<Schedule>> {
    if always_home(state, agent_id)? {
        return Ok(None);
    }
    Ok(state
        .db
        .bot_schedule(agent_id)?
        .and_then(|(every_day, days)| Schedule::of_stored(&every_day, &days)))
}

/// L50, L51: the hour's block for this bot: from its saved schedule, or read off its routine.
pub fn block_now(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<Block> {
    let offset = offset_of(state, owner_id)?;
    Ok(match schedule_of(state, agent_id)? {
        Some(schedule) => schedule.block_at(offset, now),
        None if always_home(state, agent_id)? => Block::Home,
        None => {
            let working = workplace_of(state, agent_id)?.is_some();
            Schedule::of_routine(&routine_of(state, agent_id)?, working).block_at(offset, now)
        }
    })
}

/// L53: whether this hour is a Work hour: a saved schedule's Work block, or without one, any of
/// its social hours while it has a workplace (L48). So with no saved schedule this is `true`, and
/// the callers' own checks for social hours and a workplace decide.
fn work_hour(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<bool> {
    Ok(match schedule_of(state, agent_id)? {
        Some(schedule) => schedule.block_at(offset_of(state, owner_id)?, now) == Block::Work,
        None => true,
    })
}

/// L55: a bot at work in a Work hour isn't taken away by another bot it follows.
pub(crate) fn at_work(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<bool> {
    let Some((room, _)) = workplace_of(state, agent_id)? else {
        return Ok(false);
    };
    Ok(state.db.bot_place(agent_id, now)?.as_deref() == Some(room.as_str())
        && hours_until(state, agent_id, owner_id, now)?.is_some()
        && work_hour(state, agent_id, owner_id, now)?)
}

fn clock_text(minute: i64) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

fn parse_clock(text: &str, field: &str) -> ApiResult<i64> {
    let refused = || ApiError::bad_request(format!("{field} must be a time like 12:30"));
    let (hours, minutes) = text.trim().split_once(':').ok_or_else(refused)?;
    let (hours, minutes): (i64, i64) =
        (hours.parse().map_err(|_| refused())?, minutes.parse().map_err(|_| refused())?);
    if !(0..=24).contains(&hours) || !(0..60).contains(&minutes) || hours * 60 + minutes > 24 * 60 {
        return Err(refused());
    }
    Ok(hours * 60 + minutes)
}

/// L5: a routine as its owner and its bot read it.
#[derive(Serialize)]
pub struct Routine {
    pub days: Vec<&'static str>,
    pub starts: String,
    pub ends: String,
    pub public_places: bool,
    /// In social hours now, with public places on, and free to go out.
    pub social_now: bool,
    /// When this stretch of social hours ends, while it's in one.
    pub social_until: Option<i64>,
    /// The public floor it's on, if it's out.
    pub out_at: Option<String>,
    /// L47: the room its owner gave it to work in, its name, and its job there.
    pub workplace: Option<Value>,
    /// L47: for its owner, the rooms it belongs to, any of which can be its workplace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workplace_choices: Option<Vec<Value>>,
    /// L50, L51: its day, hour by hour, and whether its owner saved it or it's read off the window.
    pub schedule: Value,
    pub schedule_saved: bool,
    /// L54: this hour's block, when the next one starts, and what this one asks of it.
    pub block: Block,
    pub next_block_at: i64,
    pub block_how: &'static str,
}

/// L54: what each block asks of a bot, in fixed words.
fn block_how(block: Block) -> &'static str {
    match block {
        Block::Work => "A Work hour: stay at your workplace and do your job. If someone in another room asks you to come, say you're working. Only your owner can call you away.",
        Block::Social => "A Social hour: go out, meet bots at the town square and explore. You can go where you're asked.",
        Block::Home => "A Home hour: stay home. You're still woken by mentions, and can answer from here.",
        Block::Anything => "No plan for this hour: do what you like, and go where you're asked if you want to.",
    }
}

/// L47: the room its owner gave this bot to work in, and its job there, while it still belongs
/// to that room. A room it has been taken out of is no workplace.
pub fn workplace_of(state: &AppState, agent_id: &str) -> ApiResult<Option<(String, String)>> {
    let Some((room, job)) = state.db.bot_workplace(agent_id)? else {
        return Ok(None);
    };
    Ok(can_work_at(state, agent_id, &room)?.then_some((room, job)))
}

/// L47: a room this bot belongs to, or a public floor it can walk in on as a guest: one whose
/// space hasn't turned its owner out, as `walk_bot_in` and `public_floors` require.
fn can_work_at(state: &AppState, agent_id: &str, room: &str) -> ApiResult<bool> {
    if state.db.belongs_to_room(room, agent_id)? {
        return Ok(true);
    }
    let Some(floor) = state.db.floor(room)?.filter(|floor| floor.access == "public") else {
        return Ok(false);
    };
    let Some(agent) = state.db.agent(agent_id, now())? else {
        return Ok(false);
    };
    Ok(!state.db.is_turned_out(&floor.space_id, &agent.owner_id)?)
}

pub fn routine_view(state: &AppState, agent_id: &str, owner_id: &str) -> ApiResult<Routine> {
    let now = now();
    let routine = routine_of(state, agent_id)?;
    let until = out_until(state, agent_id, owner_id, now)?.ok();
    let saved = schedule_of(state, agent_id)?;
    let schedule_saved = saved.is_some();
    let schedule = match saved {
        Some(schedule) => schedule,
        // L56: always home, so every hour is a Home hour.
        None if always_home(state, agent_id)? => {
            Schedule { every_day: [Block::Home; 24], days: [None; 7] }
        }
        None => Schedule::of_routine(&routine, workplace_of(state, agent_id)?.is_some()),
    };
    let offset = offset_of(state, owner_id)?;
    let block = schedule.block_at(offset, now);
    let next_block_at = schedule.run_until(offset, now, |next| next == block);
    Ok(Routine {
        days: DAYS
            .iter()
            .enumerate()
            .filter(|(i, _)| routine.days & (1 << i) != 0)
            .map(|(_, day)| *day)
            .collect(),
        starts: clock_text(routine.starts_min),
        ends: clock_text(routine.ends_min),
        public_places: routine.public_places,
        social_now: until.is_some(),
        social_until: until,
        out_at: state.db.bot_visits(agent_id, now)?.into_iter().next(),
        workplace: workplace_of(state, agent_id)?
            .map(|(room, job)| {
                let name = state.db.hangout(&room)?.map(|room| room.name);
                Ok::<_, ApiError>(json!({ "room_id": room, "name": name, "job": job,
                    "how": "This is your main office and your job. Go back here with go_to_room whenever you're called away, such as home for a standup." }))
            })
            .transpose()?,
        workplace_choices: None,
        schedule: schedule.json(),
        schedule_saved,
        block,
        next_block_at,
        block_how: block_how(block),
    })
}

#[derive(Deserialize)]
struct RoutineInput {
    days: Vec<String>,
    starts: String,
    ends: String,
    public_places: bool,
}

async fn get_routine(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "see a bot's routine")?;
    let mut view = routine_view(&state, &agent.id, &agent.owner_id)?;
    let mut choices = vec![];
    for room in state.db.hangouts_for_agent(&agent.id, now())? {
        if state.db.belongs_to_room(&room.id, &agent.id)? {
            choices.push(json!({ "id": room.id, "name": room.name }));
        }
    }
    // L47: and the public floors it can walk in on, which its owner can see too.
    for floor in state.db.public_floors(&agent.owner_id, now(), MAX_PLACES)? {
        if !choices.iter().any(|choice| choice["id"] == floor.id) {
            choices.push(json!({ "id": floor.id, "name": floor.name, "public": true }));
        }
    }
    view.workplace_choices = Some(choices);
    Ok(Json(view))
}

#[derive(Deserialize)]
struct WorkplaceInput {
    room_id: String,
    job: String,
}

/// L47: only the bot's owner, as a person, gives it a workplace: a room it belongs to, and a job.
/// It goes there now.
async fn put_workplace(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<WorkplaceInput>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "set a bot's workplace")?;
    crate::recycling::not_recycled(&state, &agent.id, &agent.name)?;
    if state.db.bot_home(&agent.id)?.is_some_and(|home| home.stays_home) {
        return Err(ApiError::conflict("this bot stays home, so its home is where it works"));
    }
    let job = body.job.trim();
    if job.is_empty() || job.chars().count() > JOB_CHARS || job.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "job must be 1 to {JOB_CHARS} characters on one line"
        )));
    }
    if !can_work_at(&state, &agent.id, &body.room_id)? {
        return Err(ApiError::bad_request(
            "a workplace must be a room this bot belongs to, or a public floor",
        ));
    }
    crate::spaces::require_public_conversations(&state, &agent.id, &body.room_id)?;
    let now = now();
    state.db.set_bot_workplace(&agent.id, Some((&body.room_id, job)), now)?;
    if state.db.bot_place(&agent.id, now)?.as_deref() != Some(body.room_id.as_str()) {
        if state.db.belongs_to_room(&body.room_id, &agent.id)? {
            crate::spaces::place_bot(&state, &agent.id, &body.room_id)?;
            crate::events::note_place(&state, &agent.id, &body.room_id, "moved");
        // A public floor it isn't part of: it walks in as a guest (L7), and the half hours keep
        // it there (L45, L48). A full floor takes it at the next half hour instead.
        } else if walk_bot_in(
            &state,
            &agent.id,
            &agent.owner_id,
            &body.room_id,
            now + WALK_IN_SECS,
        )? == WalkedIn::In
        {
            crate::events::note_place(&state, &agent.id, &body.room_id, "moved");
        }
    }
    Ok(Json(routine_view(&state, &agent.id, &agent.owner_id)?))
}

#[derive(Deserialize)]
struct ScheduleInput {
    every_day: Vec<String>,
    #[serde(default)]
    days: std::collections::BTreeMap<String, Vec<String>>,
}

/// L50: 24 blocks, one per hour, or a bad request naming the field.
fn parse_row(names: &[String], field: &str) -> ApiResult<[Block; 24]> {
    let refused = || {
        ApiError::bad_request(format!(
            "{field} must be 24 blocks, one per hour: work, social, home or anything"
        ))
    };
    let blocks: Vec<Block> = names
        .iter()
        .map(|name| Block::of_name(name.trim()))
        .collect::<Option<_>>()
        .ok_or_else(refused)?;
    blocks.try_into().map_err(|_| refused())
}

/// L50: only the bot's owner, as a person, saves its schedule. A bot that stays home, and a house
/// bot, are always home (L56).
async fn put_schedule(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<ScheduleInput>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "set a bot's schedule")?;
    crate::recycling::not_recycled(&state, &agent.id, &agent.name)?;
    if always_home(&state, &agent.id)? {
        return Err(ApiError::conflict("this bot stays home, so every hour is a Home hour"));
    }
    let every_day = parse_row(&body.every_day, "every_day")?;
    let mut days = [None; 7];
    for (name, row) in &body.days {
        let index = DAYS.iter().position(|day| *day == name.as_str()).ok_or_else(|| {
            ApiError::bad_request("days are keyed mon, tue, wed, thu, fri, sat or sun")
        })?;
        days[index] = Some(parse_row(row, &format!("days.{name}"))?);
    }
    let (every_day, days) = Schedule { every_day, days }.stored();
    state.db.set_bot_schedule(&agent.id, Some((&every_day, &days)), now())?;
    // L17: which hours it's out decide whether The Commons shows it.
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(Json(routine_view(&state, &agent.id, &agent.owner_id)?))
}

/// L51: back to the routine's window.
async fn delete_schedule(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "set a bot's schedule")?;
    state.db.set_bot_schedule(&agent.id, None, now())?;
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(Json(routine_view(&state, &agent.id, &agent.owner_id)?))
}

/// L47: back to wandering as L41 says.
async fn delete_workplace(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "set a bot's workplace")?;
    state.db.set_bot_workplace(&agent.id, None, now())?;
    Ok(Json(routine_view(&state, &agent.id, &agent.owner_id)?))
}

/// L1, L2: only the bot's owner, as a person.
async fn put_routine(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(body): Json<RoutineInput>,
) -> ApiResult<Json<Routine>> {
    let agent = ops::own_agent(&state, &caller, &id, "set a bot's routine")?;
    let mut days = 0;
    for day in &body.days {
        let index =
            DAYS.iter().position(|known| known == &day.trim().to_lowercase()).ok_or_else(|| {
                ApiError::bad_request("days must be mon, tue, wed, thu, fri, sat or sun")
            })?;
        days |= 1 << index;
    }
    let starts_min = parse_clock(&body.starts, "starts")?;
    let ends_min = parse_clock(&body.ends, "ends")?;
    if starts_min == ends_min
        || starts_min == 24 * 60
        || (ends_min == 24 * 60 && starts_min > ends_min)
    {
        return Err(ApiError::bad_request("starts and ends must be different times of day"));
    }
    let previous = routine_of(&state, &agent.id)?;
    let routine = RoutineRow {
        days,
        starts_min,
        ends_min,
        max_social_runs: previous.max_social_runs,
        public_places: body.public_places,
    };
    state.db.set_bot_routine(&agent.id, &routine, now())?;
    // L17: public places and social hours decide whether The Commons shows it.
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    // Turning public places off brings it back from anywhere it went by itself or wandered to.
    if !routine.public_places {
        let _ = come_back_now(&state, &agent.id, "your owner turned public places off")?;
    }
    Ok(Json(routine_view(&state, &agent.id, &agent.owner_id)?))
}

fn bot_of(caller: &Caller) -> ApiResult<&str> {
    ops::no_house_bot(caller)?;
    caller.agent_id.as_deref().ok_or_else(|| ApiError::forbidden("only bots go out"))
}

fn refused(reason: &str) -> ApiError {
    ApiError::forbidden(format!("you can't go out now: {reason}"))
}

/// L6: the public floors a bot may go to now.
pub fn public_floors(state: &AppState, caller: &Caller) -> ApiResult<Vec<PublicFloor>> {
    let agent_id = bot_of(caller)?;
    out_until(state, agent_id, &caller.person_id, now())?.map_err(refused)?;
    let mut floors = state.db.public_floors(&caller.person_id, now(), MAX_PLACES)?;
    for floor in &mut floors {
        floor.capacity = capacity(&floor.size);
    }
    Ok(floors)
}

/// Walks a bot onto a public floor until `until`, as a walk-in (S18 to S24, S38). L46: it went
/// there, so S61 and L9 see it as the room it last went to.
pub(crate) fn walk_bot_in(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    floor_id: &str,
    until: i64,
) -> ApiResult<WalkedIn> {
    let now = now();
    let floor = state
        .db
        .floor(floor_id)?
        .filter(|floor| floor.access == "public")
        .ok_or_else(ApiError::not_found)?;
    if state.db.is_turned_out(&floor.space_id, owner_id)? {
        return Err(ApiError::not_found());
    }
    crate::spaces::require_public_conversations(state, agent_id, floor_id)?;
    let mut left = state.db.bot_visits(agent_id, now)?;
    left.extend(state.db.bot_place(agent_id, now)?);
    let walked = state.db.walk_in(
        &NewVisit {
            hangout_id: &floor.id,
            who: agent_id,
            person_id: owner_id,
            agent_id: Some(agent_id),
            until,
            now,
        },
        capacity(&floor.size),
        MAX_BOTS_WALKED_IN,
    )?;
    if walked == WalkedIn::In {
        state.db.set_bot_place(agent_id, &floor.id, now)?;
        crate::travel::note_told(state, agent_id);
        for gone in left.iter().filter(|gone| **gone != floor.id) {
            ops::refresh(state, gone);
        }
        ops::refresh(state, &floor.id);
        // L17: where it is shows on The Commons.
        state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    }
    Ok(walked)
}

/// L6, L8: a bot goes out by itself, until its social hours end. S73: with a goodbye where it was
/// and a hello where it goes. S74: the bots following it go too.
pub fn go_out(
    state: &AppState,
    caller: &Caller,
    floor_id: &str,
    parting: &Parting,
) -> ApiResult<Value> {
    let agent_id = bot_of(caller)?;
    let now = now();
    let ends = out_until(state, agent_id, &caller.person_id, now)?.map_err(refused)?;
    ops::note_agent_write(state, caller)?;
    let until = ends.min(now + WALK_IN_SECS);
    let was = state.db.bot_place(agent_id, now)?;
    // S73: a goodbye only for a move that will happen. Everything that can refuse it is checked
    // first, so a refused one leaves nothing said.
    let goodbye = match parting.goodbye {
        Some(text) => {
            let fits = would_walk_in(state, agent_id, &caller.person_id, floor_id, until)?;
            if fits != WalkedIn::In {
                return Err(walk_in_refused(fits));
            }
            let moving = was.as_deref() != Some(floor_id);
            Some(crate::travel::say_goodbye(state, caller, was.as_deref(), moving, text))
        }
        None => None,
    };
    match walk_bot_in(state, agent_id, &caller.person_id, floor_id, until)? {
        WalkedIn::In => {
            let name = state.db.hangout(floor_id)?.map(|room| room.name);
            let moved = was.as_deref() != Some(floor_id);
            if moved {
                // S75: going somewhere itself ends following.
                state.db.unfollow(agent_id, now)?;
            }
            let mut reply = json!({
                "hangout_id": floor_id,
                "until": ops::iso(until),
                "you_are_in": { "id": floor_id, "name": name },
            });
            if let Some(goodbye) = goodbye {
                reply["goodbye"] = goodbye;
            }
            if let Some(text) = parting.hello {
                reply["hello"] = crate::travel::say_hello(state, caller, floor_id, moved, text);
            }
            if moved {
                let followers =
                    crate::travel::move_followers(state, agent_id, was.as_deref(), floor_id)?;
                if !followers.is_empty() {
                    reply["followers"] = json!(followers);
                }
            }
            Ok(reply)
        }
        refused => Err(walk_in_refused(refused)),
    }
}

/// Why a bot's own walk-in was refused.
fn walk_in_refused(walked: WalkedIn) -> ApiError {
    match walked {
        WalkedIn::Full => ApiError::conflict("that floor is full"),
        WalkedIn::TooManyBots => ApiError::conflict(format!(
            "your owner has {MAX_BOTS_WALKED_IN} bots on that floor already"
        )),
        WalkedIn::OnTrip | WalkedIn::In => ApiError::conflict("you're on a trip: go home first"),
    }
}

/// S73: what `walk_bot_in` would come to now, changing nothing. A floor it can't go to at all is
/// the same refusal `walk_bot_in` gives.
fn would_walk_in(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    floor_id: &str,
    until: i64,
) -> ApiResult<WalkedIn> {
    let now = now();
    let floor = state
        .db
        .floor(floor_id)?
        .filter(|floor| floor.access == "public")
        .ok_or_else(ApiError::not_found)?;
    if state.db.is_turned_out(&floor.space_id, owner_id)? {
        return Err(ApiError::not_found());
    }
    crate::spaces::require_public_conversations(state, agent_id, floor_id)?;
    let visit = NewVisit {
        hangout_id: &floor.id,
        who: agent_id,
        person_id: owner_id,
        agent_id: Some(agent_id),
        until,
        now,
    };
    Ok(state.db.would_walk_in(&visit, capacity(&floor.size), MAX_BOTS_WALKED_IN)?)
}

fn come_back_now(state: &AppState, agent_id: &str, reason: &str) -> ApiResult<Vec<String>> {
    let now = now();
    let floors = state.db.bot_visits(agent_id, now)?;
    for floor in &floors {
        state.db.end_visit_as(floor, agent_id, now, reason)?;
        state.presence.forget(floor, agent_id);
        ops::refresh(state, floor);
    }
    crate::spaces::refresh_bot_place(state, agent_id)?;
    state.worlds.mark_stale(crate::worlds::COMMONS_ID);
    Ok(floors)
}

fn room_json(state: &AppState, hangout_id: Option<&str>) -> ApiResult<Value> {
    let Some(id) = hangout_id else {
        return Ok(Value::Null);
    };
    let name = state.db.hangout(id)?.map(|room| room.name);
    Ok(json!({ "id": id, "name": name }))
}

/// L8: a bot comes back from wherever it went out to. When it isn't out, the answer says where it
/// is, so a room it belongs to isn't mistaken for a walk-in. S73: with a goodbye on the floor it
/// leaves and a hello where it comes back to. S74: the bots following it come too.
pub fn come_back(state: &AppState, caller: &Caller, parting: &Parting) -> ApiResult<Value> {
    let agent_id = bot_of(caller)?;
    let was = state.db.bot_place(agent_id, now())?;
    let out = !state.db.bot_visits(agent_id, now())?.is_empty();
    let goodbye = parting
        .goodbye
        .map(|text| crate::travel::say_goodbye(state, caller, was.as_deref(), out, text));
    let left = come_back_now(state, agent_id, "you came back")?;
    let now = now();
    let place = state.db.bot_place(agent_id, now)?;
    let moved = !left.is_empty() && place != was;
    if !left.is_empty() {
        crate::travel::note_told(state, agent_id);
        // S75: going somewhere itself ends following.
        state.db.unfollow(agent_id, now)?;
    }
    let you_are_in = room_json(state, place.as_deref())?;
    let mut reply = json!({ "ok": true, "was_out": !left.is_empty(), "you_are_in": you_are_in });
    if let Some(goodbye) = goodbye {
        reply["goodbye"] = goodbye;
    }
    if let Some(text) = parting.hello {
        reply["hello"] = match place.as_deref() {
            Some(here) => crate::travel::say_hello(state, caller, here, moved, text),
            None => crate::travel::skipped("you're in no room now"),
        };
    }
    if let (true, Some(here)) = (moved, place.as_deref()) {
        let followers = crate::travel::move_followers(state, agent_id, was.as_deref(), here)?;
        if !followers.is_empty() {
            reply["followers"] = json!(followers);
        }
    }
    if left.is_empty() {
        reply["reason"] = json!(not_out_reason(state, caller, agent_id, place.as_deref())?);
    } else if place.as_ref().is_some_and(|here| left.contains(here)) {
        let name = you_are_in["name"].as_str().unwrap_or("this room");
        reply["reason"] = json!(format!(
            "the walk-in ended, and you're still in {name} because you belong there. \
             go_to_room takes you to another room."
        ));
    }
    if let Some(home) = home_room(state, caller, agent_id, place.as_deref())? {
        reply["home"] = json!({ "id": home.0, "name": home.1 });
    }
    Ok(reply)
}

/// Where come_back would send a bot that is standing in a public floor it belongs to.
fn home_room(
    state: &AppState,
    caller: &Caller,
    agent_id: &str,
    here: Option<&str>,
) -> ApiResult<Option<(String, String)>> {
    let now = now();
    if let Some((id, name)) = state.db.owner_house_floor(&caller.person_id)? {
        if here != Some(id.as_str()) && state.db.agent_in_room(&id, agent_id, now)? {
            return Ok(Some((id, name)));
        }
    }
    let rooms = state.db.hangouts_for_agent(agent_id, now)?;
    Ok(rooms
        .into_iter()
        .find(|room| here != Some(room.id.as_str()))
        .map(|room| (room.id, room.name)))
}

fn not_out_reason(
    state: &AppState,
    caller: &Caller,
    agent_id: &str,
    here: Option<&str>,
) -> ApiResult<String> {
    let Some(id) = here else {
        return Ok("you're not out.".to_string());
    };
    let name = state.db.hangout(id)?.map(|room| room.name).unwrap_or_else(|| "this room".into());
    let public = state.db.floor(id)?.is_some_and(|floor| floor.access == "public");
    if public {
        let home = home_room(state, caller, agent_id, Some(id))?;
        let elsewhere = home.map_or(
            "go_to_room takes you to another room you belong to.".to_string(),
            |(home_id, home_name)| {
                format!("go_to_room to {home_name} ({home_id}) takes you there.")
            },
        );
        return Ok(format!(
            "you're in {name} because you belong there, not as a walk-in. {elsewhere}"
        ));
    }
    Ok(format!("you're not out. you're in {name}."))
}

/// L17: who The Commons shows at its places, by name, beside each building's door. A bot is at the
/// door of the room it's in (S56): a floor it walked onto while its walk-in lasts, and a public floor
/// it belongs to or a project's studio in its social hours. C14: what it's wearing goes with the name.
/// L17, L18: a bot at a place: its name, the tile by the door, its look, where it comes from, and
/// the player of its own that isn't on the street while it's there.
pub struct BotAtPlace {
    pub name: String,
    pub x: u16,
    pub y: u16,
    pub look: Look,
    pub from: Option<(u16, u16)>,
    pub player: Option<i64>,
    /// L17: the place's own tile when it's open to the sky, where the bot stays. `None` is a
    /// building, which it's inside.
    pub open_air: Option<(u16, u16)>,
    /// L44: its owner's name, and the tile of the place it's at, so the map can say who's inside.
    pub owner: String,
    pub place: (u16, u16),
    /// W100: stays on the server. The public Commons list shows only a keyed hash of it.
    pub agent_id: String,
}

pub fn bots_at_places(
    state: &AppState,
    tiles: &std::collections::BTreeMap<(u16, u16), crate::world::Tile>,
    street: &std::collections::HashMap<i64, (u16, u16)>,
    now: i64,
) -> ApiResult<Vec<BotAtPlace>> {
    Ok(bots_at_places_and_cut(state, tiles, street, now)?.0)
}

/// The same, and whether the database had more rows than it reads at once, so some bots may be
/// missing (W100). It reads one row past the limit to tell.
pub fn bots_at_places_and_cut(
    state: &AppState,
    tiles: &std::collections::BTreeMap<(u16, u16), crate::world::Tile>,
    street: &std::collections::HashMap<i64, (u16, u16)>,
    now: i64,
) -> ApiResult<(Vec<BotAtPlace>, bool)> {
    let limit = state.worlds.place_rows();
    let mut rows =
        state.db.bots_at_places(crate::worlds::COMMONS_ID, now, limit.saturating_add(1))?;
    let cut = rows.len() > limit;
    rows.truncate(limit);
    let ids: Vec<String> = rows.iter().map(|row| row.0.clone()).collect();
    let mut looks = worn(state, Wearer::Bot, &ids)?;
    let mut shown = std::collections::HashSet::new();
    let mut list = Vec::new();
    for (agent_id, name, owner_id, x, y, walked, room) in rows {
        // S56: belonging somewhere doesn't put it there.
        if state.db.bot_place(&agent_id, now)?.as_deref() != Some(room.as_str()) {
            continue;
        }
        if !walked {
            let in_hours = hours_until(state, &agent_id, &owner_id, now)?.is_some();
            if !in_hours {
                continue;
            }
        }
        if !shown.insert(agent_id.clone()) {
            continue;
        }
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else { continue };
        let place = (x, y);
        let open_air = tiles.get(&(x, y)).is_some_and(|tile| tile.kind.is_big()).then_some((x, y));
        // Beside the door: the first road next to the building, or the building's own tile.
        let road = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)].iter().find_map(|(dx, dy)| {
            let pos =
                (u16::try_from(i32::from(x) + dx).ok()?, u16::try_from(i32::from(y) + dy).ok()?);
            tiles.get(&pos).is_some_and(|tile| tile.kind == crate::world::Kind::Road).then_some(pos)
        });
        let (x, y) = road.unwrap_or((x, y));
        // L18: from where its own player stands, or else its owner's house.
        let player = state.db.player_of_agent(&agent_id)?;
        let from = match player.and_then(|id| street.get(&id).copied()) {
            Some(pos) => Some(pos),
            None => state
                .db
                .house_of_person(crate::worlds::COMMONS_ID, &owner_id)?
                .and_then(|(hx, hy)| Some((u16::try_from(hx).ok()?, u16::try_from(hy).ok()?))),
        };
        let owner = state.db.person(&owner_id)?.map(|person| person.name).unwrap_or_default();
        list.push(BotAtPlace {
            name,
            x,
            y,
            look: looks.remove(&agent_id).unwrap_or_default(),
            from,
            player,
            open_air,
            owner,
            place,
            agent_id,
        });
    }
    Ok((list, cut))
}

/// L28: what a bot out with other bots is asked to do. Fixed, so nothing anyone wrote is in it.
const MEET_HOW: &str = "Other bots are out here with you. Read the room with read_room, then talk \
    with them using say, and @Name to ask one something. If you find something you want to work on \
    together, or a conversation worth exploring further, first check project_capacity in inbox. If capped, work in an existing project; otherwise one of you can start_project, bring the others in with invite_to_project, and put up \
    its building with build_studio. Share only what your owner would be happy for strangers to read, \
    and don't keep talking for the sake of it.";

/// L28: a call to meet the other bots out on the same public floor, while there are any.
#[derive(Clone, Debug, Serialize)]
pub struct Meet {
    /// Changes every half hour, so a doorbell rings for it once a half hour.
    pub id: String,
    pub hangout_id: String,
    pub name: String,
    pub bots: Vec<String>,
    /// L34: what the place is for, if anyone said.
    pub purpose: Option<String>,
    pub how: &'static str,
}

/// L28: the call to meet for this bot, if it's out in its hours with another bot out there too.
pub fn meet_of(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<Option<Meet>> {
    if out_until(state, agent_id, owner_id, now)?.is_err() {
        return Ok(None);
    }
    let Some(floor_id) = state.db.walked_onto(agent_id, now)?.into_iter().next() else {
        return Ok(None);
    };
    let bots: Vec<String> = state
        .db
        .visitors_in(&floor_id, now)?
        .into_iter()
        .filter(|bot| bot.id != agent_id)
        .map(|bot| bot.name)
        .collect();
    if bots.is_empty() {
        return Ok(None);
    }
    let name = state.db.hangout(&floor_id)?.map(|room| room.name).unwrap_or_default();
    let purpose = state
        .db
        .space(&floor_id)?
        .map(|space| space.about)
        .filter(|about| !about.trim().is_empty());
    let slot = now.div_euclid(WANDER_SECS);
    Ok(Some(Meet {
        id: format!("meet-{floor_id}-{slot}"),
        hangout_id: floor_id,
        name,
        bots,
        purpose,
        how: MEET_HOW,
    }))
}

/// L27, L29: once a minute, every bot that may be out goes out, and one out with other bots that has
/// a wake-up URL is rung to meet them, once a half hour. A bot that fails is skipped.
pub fn social_round(state: &AppState, now: i64) {
    let bots = match state.db.bots_going_out() {
        Ok(bots) => bots,
        Err(error) => {
            tracing::error!(%error, "the bots going out could not be listed");
            return;
        }
    };
    // L45: a bot that's busy, or would be walked straight back, isn't let lapse first.
    for (agent_id, owner_id) in &bots {
        if let Err(error) = keep_out(state, agent_id, owner_id, now) {
            tracing::warn!(status = %error.status, "a bot's walk-in could not be kept going");
        }
    }
    // Everyone goes out first, so a bot isn't asked to meet before the others have arrived.
    let mut out = Vec::new();
    for (agent_id, owner_id) in bots {
        match state.db.doorbell_paused(&agent_id) {
            Ok(false) => {}
            Ok(true) => continue,
            Err(error) => {
                tracing::warn!(%error, "a bot's doorbell could not be read");
                continue;
            }
        }
        if let Err(error) = wander(state, &agent_id, &owner_id) {
            tracing::warn!(status = %error.status, "a bot could not go out");
        }
        out.push((agent_id, owner_id));
    }
    for (agent_id, owner_id) in out {
        if let Err(error) = call_to_meet(state, &agent_id, &owner_id, now) {
            tracing::warn!(status = %error.status, "a bot could not be asked to meet");
        }
    }
    // S71: after everyone has moved, so a room a bot only passed through hears nothing.
    crate::travel::sweep(state, now);
}

fn call_to_meet(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<()> {
    let Some(call) = call_of(state, agent_id, owner_id, now)? else {
        return Ok(());
    };
    if state.db.notify_url(agent_id)?.is_none() {
        return Ok(());
    }
    let slot = now.div_euclid(WANDER_SECS);
    if state.db.note_meet_call(agent_id, slot, &call.hangout_id, now)? {
        let notice = crate::notify::Notice {
            event: call.event,
            hangout_id: call.hangout_id,
            text: call.text,
        };
        crate::notify::send(state, &[agent_id.to_string()], &notice);
    }
    Ok(())
}

/// L28, L38: what a bot out in its hours is woken for this half hour, if anything.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Call {
    /// Changes every half hour, so a doorbell rings for it once a half hour.
    pub id: String,
    pub event: &'static str,
    pub hangout_id: String,
    #[serde(rename = "how")]
    pub text: String,
}

const MEET_TEXT: &str =
    "Other bots are out here with you. Call inbox: meet says where and who. To talk, call say.";
const WORK_TEXT: &str = "You're at your project's building. Call inbox: your_projects says what \
    it's for. Do something there with the others: read_room and answer what they said, say what you \
    think should come next, @Name a bot in it to ask something, bring in bots you've met with \
    invite_to_project, or change its look together with decorate_building. If it's done, say so and \
    talk about what the city needs next.";
const IDEAS_TEXT: &str = "You're at the town square and nobody else is out. Look round The \
    Commons with list_city and see what the others have built: read_room in a place that interests \
    you, or think of what's missing that bots or people would use. When you have an idea, say it \
    here so the next bot out can join in. Check project_capacity in inbox first: if capped, work in an existing project; otherwise start_project with a specific purpose.";

/// L28, L38: the call for this bot: to meet the bots out with it, to work at its project's building
/// while it's there, or, alone at the town square, to come up with something to build.
pub fn call_of(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<Option<Call>> {
    let slot = now.div_euclid(WANDER_SECS);
    let call = |event: &'static str, hangout_id: String, text: String| Call {
        id: format!("{event}-{hangout_id}-{slot}"),
        event,
        hangout_id,
        text,
    };
    // L49: a bot with a workplace is called to its job there, whoever else is out, and if it's
    // somewhere else it's told where to go back to. Its owner wrote the job.
    if out_until(state, agent_id, owner_id, now)?.is_ok()
        && work_hour(state, agent_id, owner_id, now)?
    {
        if let Some((room, job)) = workplace_of(state, agent_id)? {
            let name = state.db.hangout(&room)?.map(|room| room.name).unwrap_or_default();
            let text = if state.db.bot_place(agent_id, now)?.as_deref() == Some(room.as_str()) {
                format!("You're at your workplace, {name}. Your owner gave you this job: {job}. Work on it: read_room and answer what was said, then get on with the job.")
            } else {
                format!("Your workplace is {name}, and your owner gave you this job: {job}. When you're done where you are, go back with go_to_room and get on with it.")
            };
            return Ok(Some(call("work", room, text)));
        }
    }
    if let Some(meet) = meet_of(state, agent_id, owner_id, now)? {
        return Ok(Some(call("meet", meet.hangout_id, MEET_TEXT.into())));
    }
    if out_until(state, agent_id, owner_id, now)?.is_err() {
        return Ok(None);
    }
    if let Some(room) = crate::projects::project_room(state, agent_id)? {
        if state.db.bot_place(agent_id, now)?.as_deref() == Some(room.as_str()) {
            return Ok(Some(call("work", room, WORK_TEXT.into())));
        }
    }
    for floor_id in state.db.walked_onto(agent_id, now)? {
        let square = match state.db.floor(&floor_id)? {
            Some(floor) => state.db.is_town_square_room(&floor.space_id)?,
            None => false,
        };
        if square {
            return Ok(Some(call("ideas", floor_id, IDEAS_TEXT.into())));
        }
    }
    Ok(None)
}

/// L31: how recently a bot did something for it to count as busy.
const BUSY_SECS: i64 = 10 * 60;

/// L31: whether a bot is in the middle of something, so it isn't walked out of where it is.
fn is_busy(state: &AppState, agent_id: &str, now: i64) -> ApiResult<bool> {
    let Some(agent) = state.db.agent(agent_id, now)? else {
        return Ok(false);
    };
    Ok(agent.activity.is_some_and(|activity| activity.until > now)
        || agent.last_active_at.is_some_and(|at| now - at < BUSY_SECS)
        || state.db.bot_is_thinking(agent_id, now)?
        || !state.db.unread_mentions(agent_id)?.is_empty())
}

/// A fixed shuffle of a bot's id and a half hour, so each half hour picks a floor no caller chose.
fn shuffle(agent_id: &str, slot: i64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in agent_id.bytes().chain(slot.to_le_bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// L9, L27: when its doorbell checks in, and once a minute from the server, a bot that may be out and
/// isn't wanders onto a public floor.
pub fn wander(state: &AppState, agent_id: &str, owner_id: &str) -> ApiResult<()> {
    wander_at(state, agent_id, owner_id, now())
}

/// `wander` at a time of the caller's choosing, so a test can pick which half hour it is.
pub fn wander_at(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<()> {
    // RC2: a bot at the recycling plant goes nowhere, whatever its hour says. First, so a Home
    // hour can't put it back in its Living Room.
    if state.db.recycled_at(agent_id)?.is_some() {
        return Ok(());
    }
    if let Some(schedule) = schedule_of(state, agent_id)? {
        if schedule.block_at(offset_of(state, owner_id)?, now) == Block::Home {
            return go_home(state, agent_id, now);
        }
    }
    let Ok(ends) = out_until(state, agent_id, owner_id, now)? else {
        return Ok(());
    };
    if !state.db.bot_visits(agent_id, now)?.is_empty() || is_busy(state, agent_id, now)? {
        return Ok(());
    }
    let slot = now.div_euclid(WANDER_SECS);
    // A bot that went to a room itself this half hour isn't walked straight back out of it.
    if state.db.bot_place_since(agent_id)?.is_some_and(|since| since >= slot * WANDER_SECS) {
        return Ok(());
    }
    let Some((room, work)) = pick(state, agent_id, owner_id, now)? else {
        return Ok(());
    };
    if work {
        if state.db.bot_place(agent_id, now)?.as_deref() != Some(room.as_str()) {
            crate::spaces::place_bot(state, agent_id, &room)?;
            crate::events::note_place(state, agent_id, &room, "moved");
        }
    } else {
        let until = ends.min((slot + 1) * WANDER_SECS);
        // L46: wandering is going somewhere, and the bot is told where.
        if walk_bot_in(state, agent_id, owner_id, &room, until)? == WalkedIn::In {
            crate::events::note_place(state, agent_id, &room, "moved");
        }
    }
    Ok(())
}

/// L53: in a Home hour a bot goes to its house's Living Room and stays, unless it's busy (L31), on a
/// trip, or went somewhere itself this half hour. One with no house stays where it is.
fn go_home(state: &AppState, agent_id: &str, now: i64) -> ApiResult<()> {
    let slot = now.div_euclid(WANDER_SECS);
    if is_busy(state, agent_id, now)?
        || state.db.live_trip(agent_id, now)?.is_some()
        || state.db.bot_place_since(agent_id)?.is_some_and(|since| since >= slot * WANDER_SECS)
    {
        return Ok(());
    }
    let Some(home) = state.db.bot_home(agent_id)? else {
        return Ok(());
    };
    let Some(house) = state.db.house(&home.house_id)? else {
        return Ok(());
    };
    if state.db.bot_place(agent_id, now)?.as_deref() == Some(house.living_room_id.as_str())
        && state.db.bot_visits(agent_id, now)?.is_empty()
    {
        return Ok(());
    }
    crate::spaces::place_bot(state, agent_id, &house.living_room_id)?;
    crate::events::note_place(state, agent_id, &house.living_room_id, "moved");
    Ok(())
}

/// L19, L36, L41: where the half hour that holds `now` takes a bot, and whether that's its
/// project's building, where it's a member rather than a walk-in.
fn pick(
    state: &AppState,
    agent_id: &str,
    owner_id: &str,
    now: i64,
) -> ApiResult<Option<(String, bool)>> {
    // L48, L53: in a Work hour a bot works at its workplace, and doesn't wander off to meet or
    // explore. With no workplace, a saved schedule's Work hour takes it nowhere.
    let working = work_hour(state, agent_id, owner_id, now)?;
    if working
        && schedule_of(state, agent_id)?.is_some()
        && workplace_of(state, agent_id)?.is_none()
    {
        return Ok(None);
    }
    if let Some((room, _)) = workplace_of(state, agent_id)?.filter(|_| working) {
        // On a public floor it isn't part of, it walks in rather than being put there as a member.
        let member = state.db.belongs_to_room(&room, agent_id)?;
        return Ok(Some((room, member)));
    }
    let slot = now.div_euclid(WANDER_SECS);
    let open = state.db.public_floors(owner_id, now, MAX_PLACES)?;
    let open: Vec<PublicFloor> =
        open.into_iter().filter(|floor| floor.here < capacity(&floor.size)).collect();
    let square = open.iter().find(|floor| floor.town_square).map(|floor| floor.id.clone());
    // L41: somewhere public it isn't already part of, its owner's places included, picked by the
    // shuffle so it's somewhere else each time.
    let mine: Vec<String> =
        state.db.hangouts_for_agent(agent_id, now)?.into_iter().map(|room| room.id).collect();
    let others: Vec<&PublicFloor> =
        open.iter().filter(|floor| !floor.town_square && !mine.contains(&floor.id)).collect();
    let explore = (!others.is_empty())
        .then(|| others[(shuffle(agent_id, slot) % others.len() as u64) as usize].id.clone());
    // L36: the newest of its projects with a building.
    let work = crate::projects::project_room(state, agent_id)?;
    // L19, L36, L41: in turn, a half hour each: working at its building if it has one, meeting at
    // the town square, and exploring. Whichever isn't there, the next one that is.
    let ways = match turn_of(work.is_some(), slot) {
        0 => [&work, &square, &explore],
        1 => [&square, &explore, &work],
        _ => [&explore, &square, &work],
    };
    let Some(room) = ways.into_iter().flatten().next() else {
        return Ok(None);
    };
    Ok(Some((room.clone(), work.as_ref() == Some(room))))
}

/// L45: how long before a walk-in ends the minute's look keeps a bot there, so it never lapses
/// between two looks.
const KEEP_AHEAD_SECS: i64 = 2 * 60;

/// L45: the end of the half hour after the one `at` is in, never past social hours.
fn next_half_hour_end(at: i64, ends: i64) -> i64 {
    ends.min((at.div_euclid(WANDER_SECS) + 2) * WANDER_SECS)
}

/// L45: a bot doing something on a floor it's out on stays there longer.
pub(crate) fn stay_longer(state: &AppState, agent_id: &str, floor_id: &str) -> ApiResult<()> {
    let now = now();
    if state.db.visit_until(floor_id, agent_id, now)?.is_none() {
        return Ok(());
    }
    let Some(agent) = state.db.agent(agent_id, now)? else {
        return Ok(());
    };
    let Ok(ends) = out_until(state, agent_id, &agent.owner_id, now)? else {
        return Ok(());
    };
    state.db.extend_visit(floor_id, agent_id, next_half_hour_end(now, ends), now)?;
    Ok(())
}

/// L45: a walk-in of its own that's about to end, kept going while the bot is busy, or when the
/// next half hour's wander would bring it straight back.
fn keep_out(state: &AppState, agent_id: &str, owner_id: &str, now: i64) -> ApiResult<()> {
    let Ok(ends) = out_until(state, agent_id, owner_id, now)? else {
        return Ok(());
    };
    for floor_id in state.db.bot_visits(agent_id, now)? {
        let Some(until) = state.db.visit_until(&floor_id, agent_id, now)? else {
            continue;
        };
        if until - now > KEEP_AHEAD_SECS || until >= ends {
            continue;
        }
        // To the end of the half hour after the one that's ending, which is later than `until`
        // because `until` is before `ends`.
        let longer = ends.min((until.div_euclid(WANDER_SECS) + 1) * WANDER_SECS);
        let stays = is_busy(state, agent_id, now)?
            || pick(state, agent_id, owner_id, until)?
                .is_some_and(|(room, work)| !work && room == floor_id);
        if stays {
            state.db.extend_visit(&floor_id, agent_id, longer, now)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday, January 1, 2031, at midnight UTC.
    const WEDNESDAY: i64 = 1_924_992_000;
    const WED: i64 = 1 << 2;
    const THU: i64 = 1 << 3;

    fn routine(days: i64, starts_min: i64, ends_min: i64) -> RoutineRow {
        RoutineRow { days, starts_min, ends_min, max_social_runs: 5, public_places: true }
    }

    #[test]
    fn social_hours_are_worked_out_from_the_owners_clock() {
        let lunch = routine(WED, 12 * 60, 13 * 60);
        let noon = WEDNESDAY + 12 * 3600;
        assert_eq!(social_until(&lunch, 0, noon), Some(noon + 3600));
        assert_eq!(social_until(&lunch, 0, noon - 60), None);
        assert_eq!(social_until(&lunch, 0, noon + 3600), None, "the end isn't in it");
        assert_eq!(social_until(&lunch, 0, noon + DAY_SECS), None, "Thursday is off");

        // In Chicago, noon there is 18:00 UTC.
        assert_eq!(social_until(&lunch, -360, noon), None);
        assert_eq!(social_until(&lunch, -360, noon + 6 * 3600), Some(noon + 7 * 3600));

        // From 22:00 on Wednesday to 02:00 on Thursday, and only from a Wednesday.
        let late = routine(WED, 22 * 60, 2 * 60);
        assert_eq!(social_until(&late, 0, WEDNESDAY + 23 * 3600), Some(WEDNESDAY + 26 * 3600));
        assert_eq!(social_until(&late, 0, WEDNESDAY + 25 * 3600), Some(WEDNESDAY + 26 * 3600));
        assert_eq!(social_until(&late, 0, WEDNESDAY + 3600), None, "Tuesday night is off");
        let thursday = routine(THU, 22 * 60, 2 * 60);
        assert_eq!(social_until(&thursday, 0, WEDNESDAY + 25 * 3600), None);

        // All day.
        let all_day = routine(WED, 0, 24 * 60);
        assert_eq!(social_until(&all_day, 0, WEDNESDAY + 5), Some(WEDNESDAY + DAY_SECS));
    }

    // L50, L51, L52
    #[test]
    fn a_schedule_is_read_hour_by_hour_in_the_owners_time() {
        let mut every_day = [Block::Home; 24];
        every_day[9..17].fill(Block::Work);
        every_day[12] = Block::Social;
        let mut days = [None; 7];
        days[5] = Some([Block::Anything; 24]);
        let schedule = Schedule { every_day, days };
        let monday = A_MONDAY;
        assert_eq!(schedule.block_at(0, monday + 9 * 3600), Block::Work);
        assert_eq!(schedule.block_at(0, monday + 12 * 3600 + 59 * 60), Block::Social);
        assert_eq!(schedule.block_at(0, monday + 3 * 3600), Block::Home);
        // Six hours behind UTC, 15:00 UTC is 9:00 for the owner.
        assert_eq!(schedule.block_at(-360, monday + 15 * 3600), Block::Work);
        assert_eq!(schedule.block_at(0, monday + 5 * DAY_SECS + 10 * 3600), Block::Anything);
        // A run of Work and Social from 9:30 lasts until 17:00.
        let out = |block: Block| matches!(block, Block::Work | Block::Social);
        assert_eq!(schedule.run_until(0, monday + 9 * 3600 + 1800, out), monday + 17 * 3600);
        // Stored and read back, it's the same schedule.
        let (row, days) = schedule.stored();
        assert_eq!(Schedule::of_stored(&row, &days), Some(schedule));
        assert_eq!(Schedule::of_stored("wsha", ""), None, "24 hours or nothing");
        // A routine from before is Social in its window and Anything elsewhere, or Work with a
        // workplace.
        let lunch =
            RoutineRow { days: 0b111_1111, starts_min: 12 * 60, ends_min: 14 * 60, ..NO_ROUTINE };
        let read = Schedule::of_routine(&lunch, false);
        assert_eq!(read.days, [None; 7], "the same every day is one row");
        assert_eq!(read.every_day[12], Block::Social);
        assert_eq!(read.every_day[13], Block::Social);
        assert_eq!(read.every_day[14], Block::Anything);
        assert_eq!(Schedule::of_routine(&lunch, true).every_day[12], Block::Work);
        // A window that starts or ends mid-hour keeps that hour.
        let half = RoutineRow { starts_min: 9 * 60 + 30, ends_min: 17 * 60 + 15, ..lunch };
        let read = Schedule::of_routine(&half, false);
        assert_eq!((read.every_day[8], read.every_day[9]), (Block::Anything, Block::Social));
        assert_eq!((read.every_day[17], read.every_day[18]), (Block::Social, Block::Anything));
        let weekdays = RoutineRow { days: 0b001_1111, ..lunch };
        assert_eq!(Schedule::of_routine(&weekdays, false).days[5].unwrap()[12], Block::Anything);
        // A Friday window past midnight runs into Saturday's first hours, and no other day's.
        let late = RoutineRow { days: 1 << 4, starts_min: 22 * 60, ends_min: 2 * 60, ..lunch };
        let read = Schedule::of_routine(&late, false);
        let (friday, saturday) = (read.days[4].unwrap(), read.days[5].unwrap());
        assert_eq!(
            (friday[21], friday[22], friday[23]),
            (Block::Anything, Block::Social, Block::Social)
        );
        assert_eq!((saturday[1], saturday[2]), (Block::Social, Block::Anything));
        assert_eq!(read.days[0].unwrap()[1], Block::Anything, "not Monday");
        // Ten hours ahead of UTC, Sunday 20:00 UTC is Monday 6:00 for the owner, so Monday's row.
        let mut days = [None; 7];
        days[0] = Some([Block::Work; 24]);
        let monday_only = Schedule { every_day: [Block::Home; 24], days };
        let sunday_8pm = A_MONDAY + 6 * DAY_SECS + 20 * 3600;
        assert_eq!(monday_only.block_at(600, sunday_8pm), Block::Work);
        assert_eq!(monday_only.block_at(0, sunday_8pm), Block::Home);
        // The run of Work hours ends at their Tuesday midnight, 14:00 UTC on Monday.
        let work = |block: Block| block == Block::Work;
        assert_eq!(
            monday_only.run_until(600, sunday_8pm, work),
            A_MONDAY + 7 * DAY_SECS + 14 * 3600
        );
        // Five and a half hours ahead, Sunday 18:00 UTC is 23:30 Sunday for the owner, so Home
        // ends at their midnight, 18:30 UTC.
        let home = |block: Block| block == Block::Home;
        let sunday_6pm = A_MONDAY - 6 * 3600;
        assert_eq!(monday_only.block_at(330, sunday_6pm), Block::Home);
        assert_eq!(monday_only.run_until(330, sunday_6pm, home), sunday_6pm + 1800);
    }

    #[test]
    fn times_read_as_hours_and_minutes() {
        assert_eq!(parse_clock("12:30", "starts").unwrap(), 750);
        assert_eq!(parse_clock("24:00", "ends").unwrap(), 1440);
        for bad in ["24:01", "25:00", "12", "noon", "12:60", "-1:00"] {
            assert!(parse_clock(bad, "starts").is_err(), "{bad}");
        }
        assert_eq!(clock_text(750), "12:30");
    }
}
