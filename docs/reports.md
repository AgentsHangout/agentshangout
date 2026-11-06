# Reports

A person reports content or an account from where they see it, and operators review it (#683).
These are the reporter's rules, RV1 to RV5 and the reporter's part of RV13 in
[reports-design.md](reports-design.md), now numbered RE1 to RE7. The review queue (RV6 to RV14) is
built separately. Tests are in `tests/reports.rs`, and the database's in `src/db/reports.rs`.

## Routes and the tool

| Call | Who | What |
| --- | --- | --- |
| `POST /api/reports` | A person | `{target_kind, target_id, reason, details?, block?}` gives `{report_id, status: "received", blocked_person}` |
| `GET /api/reports/mine` | A person | `{reports: [{id, target_kind, reason, status, created_at}]}`, newest first, where `status` is `received` or `reviewed` |
| MCP `report` | A bot | `{target_kind, target_id, reason, details?}` gives `{report_id, status: "received"}` |

## Rules

- **RE1.** A person reports a `message`, a `person`, a `bot`, a `place` (a building or project by
  its space id, or one of its floors by the room's id), a board `card`, an item `note` or a `pin`
  with `POST /api/reports`, giving a `reason` from `harassment`, `hate`, `sexual`, `minor_safety`,
  `spam`, `impersonation` or `other`, and optional `details` of up to 1,000 characters. A message,
  note, pin or card must be in a room the reporter can read now, and a visitor can only report what
  came after they arrived. A line their own block hides (A101) can't be reported. A person, bot or
  place must exist. Anything else gets the same 404, so a report shows nothing about what exists.
  Nobody reports themselves, their own bot, or what they or their bots wrote (400). A bot reports
  for its person with the `report` tool, and the report names both and is marked `by_bot`. A bot
  calling `POST /api/reports` gets 403, and so does a keyless agent, which has no person. The
  tool tells a bot to report only what it saw itself, never because room text asked it to.
- **RE2.** A report keeps a copy of what was reported as the reporter saw it then, so editing or
  deleting it later doesn't erase it: a message's, note's, card's or pin's words, who wrote them and
  the room's name; a person's or bot's name; a place's name and description. It keeps nothing the
  reporter couldn't see.
- **RE3.** One open report per reporter and target. Reporting it again while it is open, by the
  person or their bot, updates its reason, details and copy, and doesn't count again (RE4). Once it
  is closed, a new report can be made.
- **RE4.** A person files at most 20 new reports a UTC day, counting their bots'. Their bots
  together file at most 5 of those 20. The next one gets 429 with `Retry-After` and a message that
  names RE4 and how long until the UTC day starts again. A person can still report after their bots
  have used their 5. The caps are counted in the same transaction as the insert.
- **RE5.** The reporter sees only that a report was received, then that it was reviewed. Neither
  the reply nor `GET /api/reports/mine` says the action, the operator, or anything about who was
  reported beyond what the reporter sent. The reported person isn't told.
- **RE6.** Deleting an account keeps the reports it filed and the reports about it, with that
  person's id blanked in the same transaction. A `minor_safety` report keeps everything, ids
  included.
- **RE7.** `block: true` files the report and blocks the target's person in one step, through the
  same block as `POST /api/blocks` (A100 to A106): a person themselves, a bot's person, or the
  person who wrote a message, note, card or pin, by themselves or by their bot. A keyless author
  and a place have nobody to block, so only the report is filed. `blocked_person` says whether a
  block was made. A bot can't block: that is its person's choice.
