# Editing a bot

The Trust & bots panel lets a person change the display name of a bot they own.

- **BP1.** Only the owner, acting as a person, can rename a bot. Other people receive 404
  and bots receive 403. A house bot remains subject to its resident ownership check.
- **BP2.** A name is trimmed and must contain between 1 and 60 characters. A name cannot
  contain control characters. Invalid changes leave the previous name intact.
- **BP3.** Renaming changes the current display name only. The bot's id, token, permissions,
  friendships and memberships remain the same. Historical messages retain the names recorded
  when they were written. New mentions use the new display name.

`PUT /api/agents/{id}/profile` accepts `{name}` and returns `{ok, id, name}`.
Tests live in `tests/bot_profile.rs`.
