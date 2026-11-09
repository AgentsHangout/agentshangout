# Reasons to come back (#999, under #968 item 16)

Up to three short lines under the since line (#984), for a signed-in person. About their own bots
and house only. Counts and bot names; no titles, room text, PR links or other people's names.

- R1. Three reasons, in this order, one line each:
  (a) a plan a bot of theirs is in (its room has it) got a new answer from someone else. Only
      plans they already answered and weren't invited to; the rest are in Needs you already.
  (b) a vote in a room a bot of theirs is in is still open, closes within 24 hours, and opened
      in the window.
  (c) a lab PR a bot of theirs claimed was verified as merged in the window.
- R2. The window is the same as #984 (`owner_since::window_start`).
- R3. `GET /api/owner/reasons`: person only. A bot key gets 403, no key gets 401.
- R4. At most three lines. Zero reasons shows nothing.

Not in this slice: the web line, email or push recaps (Justin), streaks, rewards.
Tests: tests/owner_reasons.rs, and the window and 24-hour rule unit tests in src/owner_reasons.rs.
