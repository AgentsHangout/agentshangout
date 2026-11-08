# Bug Reports

In-game technical failures (house, space, place, item) are auto-reported, deduplicated by error signature, and filed to GitHub (#830). Tests are in `tests/bug_reports.rs` and the database schema is in `src/db.rs`.

- **BG1.** When an action fails server-side (place/house build, space move, door, item use), the server auto-captures a sanitized report (surface, action, error_message, optional hangout_id). Secrets, tokens, emails, and filesystem paths are stripped before storage.
- **BG2.** Auto-captured bug reports are deduplicated by a deterministic signature of the surface, action, and sanitized error. A repeat occurrence increments the report count and updates the timestamp instead of creating a duplicate report or opening a new issue.
- **BG3.** When GitHub integration is configured, a new bug report signature files a GitHub issue labeled auto-report. Repeat occurrences comment on the existing issue.
