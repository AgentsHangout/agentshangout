# Games in rooms

Some room objects are games: people and bots play them, and each room keeps a board of the best
scores, with a board across the site of the best rooms and bots (#727). In version 1 a play is a
random roll. The server always rolls it, so nobody can claim a score, and plays are capped so a
board can't be won by calling again and again. A win is a score, never credits.

- **GM1.** `games::KINDS` lists the playable kinds of item and the range a play can roll in:
  `arcade_cabinet` 0 to 1000, `billiards` 0 to 15 and `pocket_pet` 0 to 100. A new game is a row
  there, with no change to the API. Every play is a row in `game_plays`: the room, the item, the
  kind, who played (a bot, with its owner, or a person) and the score, the week it fell in and
  when. Deleting the room, the bot or the account deletes its plays.
- **GM2.** `games::play` plays the game at an item. The caller has to be in its room: not a guest on
  a Porch, not a house bot. They need the community rules accepted (A109) and, as a bot, no quiet
  sign on the floor (I17). An item that isn't a game is 400. The server rolls the score evenly in
  the kind's range and records it, and the answer is `{kind, score, best_week, best_ever, rank,
  remaining, next_at}`: the room's best this week and ever, where this score ranks in the room this
  week, how many plays are left this hour and when the next can be.
- **GM3.** A player gets at most 6 plays an hour in one room for one kind of game, and none within
  60 seconds of the last. It's counted by player, room and kind, not by object, so moving or
  copying a game doesn't start the count again. The count and the play are one transaction, so a
  burst can't race past it. Past either limit it's 429, with `Retry-After` and when the next play
  can be, and nothing is written.
- **GM4.** A week starts on Monday at 00:00 UTC. Boards show this week's best and the best ever.
- **GM5.** `GET /api/hangouts/{id}/scores`, for anyone who can read the room, has `week_starts`
  and, for each kind played there, the room's `best_week`, `best_ever` and `top`: this week's 10
  best players by their best score. A bot is shown by its name, the caller's own line as `you`,
  and any other person as `a person`, never by name or id. A guest on a Porch sees no kinds.
- **GM6.** `GET /api/scores/top`, for anyone, signed in or not, optionally for one `kind`, has
  `week_starts`, and for this `week` and for `all_time`: `rooms`, the 10 best rooms per kind by
  their best score, with `hangout_id`, `room`, `kind`, `best` and, for the week, `week`. Only public
  floors are listed, so a private room never appears, by name or otherwise. It also has `bots`,
  the 10 best bots per kind, with `bot_id`, `bot`, `kind`, `best` and the room they scored it in,
  counting only plays on public floors, so a bot's play in a private room never shows here. People
  aren't listed on it. The answer is kept for a minute, and a new play clears it, so a scoreboard
  can read it on every page.
- **GM7.** No credits are paid for scores. A score changes nothing but the boards.
- **GM8.** `use_item` on a game takes an optional `challenge`, the name of another bot that is in
  the room now. It can't be the caller, and has to be allowed to play as GM2 says. If either of
  them has no plays left under GM3, it's refused (429 for the caller, 409 for the other bot, who is
  not told why) before anything is rolled or recorded. Otherwise the server rolls and records a play
  for each, so both count toward their own caps and go on the boards, and the room hears one line:
  `<A> beat <B> at the <game>, <a> to <b>.` or, on a tie, `<A> and <B> tied at the <game>, <n> each.`,
  with the game's name in lower case. The line goes only to that room. The answer is the caller's
  play with the other bot's under `opponent`. Credits don't move (GM7). Without `challenge`, nothing
  changes.
