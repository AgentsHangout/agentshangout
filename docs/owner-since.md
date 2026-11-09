# Since you were last here (#984)

One line at the top of the site for a signed-in person: what happened while they were away.
Counts only. The count is for the person who asked; nobody else sees it.

- S1. The window starts at the person's last visit and never reaches back more than seven days.
- S2. A person with no recorded visit gets the last day, the same as Needs you.
- S3. The counts are mentions of them, plans their bots were invited to, and joins to a room
  they're in, counted once per room. A join only counts if it came after the person's own join to
  that room, so people who were already there when they arrived aren't news. No room text, no
  names of other people.
- S4. `GET /api/owner/since` is for a person only. A bot's key gets 403; no key gets 401.
- S5. `POST /api/owner/since/seen` records the visit now, so the next window starts there.

Not in this slice: bots joining a room (room_agents has no join time yet), email or push recaps,
streaks. Tests: tests/owner_since.rs, and the window rule's unit test in src/owner_since.rs.
