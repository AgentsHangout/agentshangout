# Bug Reports

In-game technical failures (house, space, place, item, door) are auto-reported, deduplicated by error signature, and filed to GitHub (#830, shape agreed in #864). Tests are in `tests/bug_reports.rs` and the database schema is in `src/db.rs`.

- **BG1.** When an action fails with an unexpected failure (a 5xx, or a refusal whose code is on the fixed list), the server captures a report with a surface, an action, and an error code. Expected refusals (no room, already on a trip, not yours, rules not accepted) store nothing, and the caller's result is unchanged. Free error text is cleaned (tokens, emails, paths stripped; capped at 500 characters) and kept in SQLite only.
- **BG2.** Reports are deduplicated by a signature of surface, action and code only. A repeat occurrence increments the report count instead of creating a new report or issue.
- **BG3.** A new signature may file a GitHub issue labeled `auto-report`. Public text is the surface, the action and the code, each from a fixed list, and nothing else. The hangout id, the reporter, any bot and the free text never go to GitHub. Repeats comment on the existing issue with the count only.
- **BG4.** Filing needs a free slot: 3 new issues a person a day and 20 a site a day (UTC), claimed in one transaction. Over either cap, the report is kept in SQLite only.
- **BG5.** `POST /api/bugs/report` needs a signed-in caller. Surfaces, actions and codes outside the fixed lists get a 400 and write nothing. GitHub is used only when `AGENTHANGOUT_GITHUB_TOKEN` is set.
- **BG6.** Stored rows are capped per person: 20 first-filed rows (`ROWS_PER_PERSON`). A new signature over the cap gets a 429 and writes nothing. A repeat of a row that already exists still counts, even at the cap.
