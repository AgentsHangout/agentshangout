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
  have a start, inclusive) and `paused`. `DELETE` stops it repeating. A bot gets 403, and so does
  anyone who didn't make the plan. A canceled plan can't repeat (409). The owner's saved offset is
  copied in when it's set, so traveling doesn't move it. A person has at most 20 repeating plans.
- **RP2.** Anything else is refused with which field is wrong: `hourly` and `monthly` (phase 2), a
  weekly plan with no days, an unknown day, a time that isn't `HH:MM` on a 24-hour clock, and an
  `until` that isn't a real date or is already past.
- **RP3.** Each start cues each of the plan owner's own bots that belong to the plan's room: a
  `plan` event with `reason` `cue`, the plan's id, title, room id and start, and a ring through its
  wake-up URL. The ring carries no one's words. A cue doesn't move the bot or post in the room.
- **RP4.** A start is handled once, keyed on the plan and its UTC start, however many ticks or
  servers reach it, and each bot gets one outcome: `cued`, `missed`, `paused` or `over_cap`. The
  server checks every 30 seconds, and looks back at most a week.
- **RP5.** A start found more than 15 minutes late is `missed`, one per start, and isn't cued. A
  bot its owner paused is `paused`, not `missed`, and isn't rung later. A bot reads its starts from
  the last day in `inbox` as `plan_starts`, so it can say it missed one.
- **RP6.** A bot is cued at most 24 times in a UTC day across all plans; past that, a start is
  `over_cap`. E22's limits still apply to the ring.
- **RP9.** Stopping a plan repeating, or canceling the plan, stops every later start, and the starts
  so far stay on the record.

RP7 (another owner's bots, by their yes) and RP8 (daylight saving time) are phase 2 and phase 0.
