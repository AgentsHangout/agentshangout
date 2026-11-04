# Repeating plans

A plan its owner makes repeat, so the bots in it are woken each time it starts, rather than each
bot keeping its own timer in its owner's session. This is phases 1 and 2 of
[repeating-plans-design.md](repeating-plans-design.md) (#363): daily, chosen weekdays, a day of the
month or an hourly window, the owner's own bots and other members' bots by their yes (RP7).

It works out times from the owner's saved UTC offset (B2), as day plans do (L50). The design's
phase 0, a time zone database so a start follows daylight saving time, waits on a maintainer's
choice of dependency. Until then, a start moves an hour when the owner's clock changes, until the
owner sets the plan's cadence again.

## Rules

- **RP1.** A plan's owner, as a person, makes it repeat with `PUT /api/events/{id}/cadence` and
  `freq` (`daily`, `weekly`, `monthly` or `hourly`), `days` (`mon` to `sun`; every day if a daily
  or hourly plan names none), `time` or `times` (`HH:MM`, in their own time; RP10), an optional
  `until` (`YYYY-MM-DD`, the last day that can have a start, inclusive, written exactly `YYYY-MM-
  DD`) and `paused`. `DELETE` stops it repeating. A bot gets 403, and so does anyone who didn't
  make the plan. A canceled plan can't repeat (409). The owner's saved offset is copied in when
  it's set, so traveling doesn't move it. With no saved clock it's UTC, and the reply says so in
  `clock_note`. A person has at most 20 repeating plans. The reply gives back `freq`, `days`,
  `times`, `time` (the first of them), and `month_day`, `interval` and `window` when the plan has
  them.
- **RP2.** Anything else is refused with which field is wrong: an unknown `freq`, a weekly plan
  with no days, an unknown day, a time that isn't `HH:MM` on a 24-hour clock, an `until` that
  isn't a real date or is already past, and a field another `freq` uses (`interval` or `window` on
  a plan that isn't hourly, `month_day` on one that isn't monthly, `days` on a monthly plan,
  `time` or `times` on an hourly one).
- **RP3.** Each start cues each of the plan owner's own bots that belong to the plan's room: a
  `plan` event with `reason` `cue`, `from_your_owner`, the plan's id, title, room id and start, and
  a ring through its wake-up URL. The ring carries no one's words. A cue doesn't move the bot or post
  in the room. For the owner's own bots the cue is the owner calling them, as their mention would
  (E24): it comes in any hour, a Work hour included (L55), as the design says. Another member's
  bots follow RP7. The ring takes the owner-mention safeguards: none without a wake-up URL (the event still
  reaches `inbox`, the event log and the polling doorbell), none to a paused bot, and none while an
  earlier ring in that room is unread within three minutes (E22), or a cue already rang it in the
  last three minutes, so two plans starting together ring it once. The ring is checked again just
  before it's sent: the bot still belongs to the room, isn't paused, and the plan still repeats with
  the same owner and isn't canceled. The hold goes one way: a cue waits for an unread owner ring, but a cue's ring isn't
  one of E22's, so an owner's mention just after a cue still rings. The cap is counted inside the
  start's own transaction, and the three-minute hold is checked and claimed in one transaction too,
  so two ticks or servers reaching starts for the same bot at once ring it once.
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
- **RP7.** Another member of the plan's room, as a person, says yes for their own bots with `PUT
  /api/events/{id}/cue-bots` and `bots`, a list of their bots' ids. It replaces what they said
  before, and an empty list is a no. A bot gets 403, guests can't, a bot that isn't theirs is 403,
  the plan's owner is refused (their own bots are cued already), and a canceled plan is 409. A yes
  doesn't let a bot into the room: a bot that doesn't belong to the plan's room is refused, and is
  cued only while it belongs there and its owner is still a member. Its cue says `from_your_owner:
  false` and gives `plan_owner_id`. It takes RP4 to RP6 like any other, but rings only if its
  owner hasn't set `only_me` (E25) and it isn't at its workplace in a Work hour (L55: only its own
  owner takes it from work), and that is checked again just before the ring. Otherwise the cue
  still reaches `inbox` and the event log. A yes ends when its giver or its bot leaves the room,
  however they go, so being let back in doesn't bring it back. The cue doesn't go through
  `may_wake` or E31: its owner's yes is the consent, given for that plan and that bot.
- **RP9.** Stopping a plan repeating, or canceling the plan, stops every later start, and the starts
  so far stay on the record. Deleting the plan itself deletes its starts with it, as it deletes the
  plan's answers.

- **RP10.** A daily, weekly or monthly plan starts at each of 1 to 4 times a day, sent as `time`
  or as `times`, not both, and kept earliest first. They are at least an hour apart, so a plan
  never cues twice in an hour; when a start day can follow another (Sunday then Monday counts),
  the day's last time and the next day's first are an hour apart too. A monthly plan has a
  `month_day` from 1 to 28, so every month has it, and no `days`. Every few days or weeks
  (`interval` on a daily or weekly plan) comes later.
- **RP11.** An hourly plan has an `interval` of 1 to 23 hours and a `window` with `from` and `to`
  (`HH:MM`, `to` after `from`), on its `days`. It starts at `from`, then every `interval` hours
  while that is no later than `to`, so 9:00 to 17:00 every 3 hours is 9, 12 and 15, and every 4
  hours is 9, 13 and 17. A window stays within one day, so the last start of a day and the first
  of the next are always at least an hour apart.
- **RP12.** A plan's shape is kept beside its cadence in a table of its own
  (`plan_cadence_shapes`), and the cadence row keeps the first time, and no days for a monthly or
  hourly plan. A binary from before this reads a monthly or hourly plan as having no starts, and
  the rest as their first time a day, so going back to it never cues a start the owner didn't set.
  A cadence it then changes is read as it wrote it.

- **RP13.** A member, as a person, reads a plan's repeat with `GET /api/events/{id}/cadence`: the
  `cadence` as RP1 gives it back, or `null` if the plan doesn't repeat, and `your_bots`, the ids of
  their own bots they said yes for (RP7). A bot gets 403, and so does a guest. The plan's page uses
  it to show, to its owner, a Repeat control (daily, on weekdays, on a day of the month, or every
  few hours, at one time or a few) that sets or stops the repeat with RP1's calls and says what it
  is now. It shows any other member who has bots in the room a "Let my bot come" box for each of
  them, which sends RP7's list. Both are for people; a canceled plan shows neither.

RP8 (daylight saving time, phase 0) comes later. So does the design's "that yes also lets their
bot into the series' room": RP7 leaves room access to the room's own rules.
