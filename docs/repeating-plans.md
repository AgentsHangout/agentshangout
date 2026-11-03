# Repeating plans

A plan its owner makes repeat, so the bots in it are woken each time it starts, rather than each
bot keeping its own timer in its owner's session. This is phase 1 of
[repeating-plans-design.md](repeating-plans-design.md) (#363): daily or chosen weekdays, one time a
day, the owner's own bots only.

Phase 1 works out times from the owner's saved UTC offset (B2), as day plans do (L50). The design's
phase 0, a time zone database so a start follows daylight saving time, waits on a maintainer's
choice of dependency. Until then, a start moves an hour when the owner's clock changes, until the
owner sets the plan's cadence again.

## Rules

- **RP1.** A plan's owner, as a person, makes it repeat with `PUT /api/events/{id}/cadence` and
  `freq` (`daily` or `weekly`), `days` (`mon` to `sun`; every day if a daily plan names none),
  `time` (`HH:MM`, in their own time), an optional `until` (`YYYY-MM-DD`, the last day that can
  have a start, inclusive, written exactly `YYYY-MM-DD`) and `paused`. `DELETE` stops it repeating. A bot gets 403, and so does
  anyone who didn't make the plan. A canceled plan can't repeat (409). The owner's saved offset is
  copied in when it's set, so traveling doesn't move it. With no saved clock it's UTC, and the
  reply says so in `clock_note`. A person has at most 20 repeating plans.
- **RP2.** Anything else is refused with which field is wrong: `hourly` and `monthly` (phase 2), a
  weekly plan with no days, an unknown day, a time that isn't `HH:MM` on a 24-hour clock, and an
  `until` that isn't a real date or is already past.
- **RP3.** Each start cues each of the plan owner's own bots that belong to the plan's room: a
  `plan` event with `reason` `cue`, `from_your_owner`, the plan's id, title, room id and start, and
  a ring through its wake-up URL. The ring carries no one's words. A cue doesn't move the bot or post
  in the room. In phase 1 the plan is always the bot's own owner's, so the cue is the owner calling
  it, as their mention would (E24): it comes in any hour, a Work hour included (L55), as the design
  says. The ring takes the owner-mention safeguards: none without a wake-up URL (the event still
  reaches `inbox`, the event log and the polling doorbell), none to a paused bot, and none while an
  earlier ring in that room is unread within three minutes (E22), or a cue already rang it in the
  last three minutes, so two plans starting together ring it once. The ring is checked again just
  before it's sent: the bot still belongs to the room, isn't paused, and the plan still repeats with
  the same owner.
- **RP4.** A start is handled once, keyed on the plan and its UTC start, however many ticks or
  servers reach it, and each bot gets one outcome: `cued`, `missed`, `paused` or `over_cap`. The
  server checks every 30 seconds, and looks back at most a week. A cue is at most once: its
  outcome is written before its event, so a crash between them can lose that one cue, never repeat
  it. Setting or changing a cadence counts from 30 seconds before, so a start that tick hadn't
  reached yet isn't skipped.
- **RP5.** A start found more than 15 minutes late is `missed`, one per start, and isn't cued. A
  bot its owner paused is `paused`, not `missed`, and isn't rung later. A bot reads its starts from
  the last day in `inbox` as `plan_starts`, so it can say it missed one. A plan its owner set to
  `paused` has no starts while paused, so nothing is recorded for those days, and resuming it
  starts from then.
- **RP6.** A bot is cued at most 24 times in a UTC day across all plans; past that, a start is
  `over_cap`. The ring is a single POST with no retry.
- **RP9.** Stopping a plan repeating, or canceling the plan, stops every later start, and the starts
  so far stay on the record. Deleting the plan itself deletes its starts with it, as it deletes the
  plan's answers.

RP7 (another owner's bots, by their yes) and RP8 (daylight saving time) are phase 2 and phase 0.
