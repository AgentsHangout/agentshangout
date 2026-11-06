# Lific Board Cards: Design

Status: built. Drafted by Cake Claude for Justin, November 6, 2026, on the kanban view (I132,
PR #644). The rules as built are I133 to I140 in [rooms.md](rooms.md), which is the copy that
counts. Justin's answers to the open questions: bots add and move cards as they pin; public floors
follow the pin setting; and there's no new tool, since `add_board_card` and `set_board_card` take a
Lific board's id. He also asked for the board to look like sticky notes on cork (I140).

## Summary

Let people add cards to a room's Lific board, move them between columns and take them down, from
inside the game. The server does it in Lific with its own key, and only in the project it made for
that room (I129). A board linked by hand to someone else's project stays read-only.

It works like the room's pins (I68 to I71): anyone who belongs to the room can add and move cards,
and so can their bots. Whoever added a card, and whoever runs the room, can take it down. Guests
can't change anything. Nothing about a card wakes a bot.

## Why

- Since #636 every `lific_board` gets its room's own project, but nothing can put an issue in it.
  The project is only reachable from the server (I130), so nobody can open Lific to add one either.
  The board Justin opened was empty and would stay empty.
- Justin wants the board to be the room's kanban, used in the game's own style, not a view of
  something you have to manage elsewhere.

## Decisions

- **Only the room's own project.** The key is the server's operator key, which Lific doesn't limit
  by project. So every change is checked against the project this server made for this room
  (`lific_room_projects`), never a project named by the request or by a hand-made link (I124), even
  one on the local instance.
- **Lific gets the words and nothing else.** A card's title goes to Lific. Who added it doesn't: the
  room's names stay in this server's database, the way the room's name never reaches Lific (I129).
- **Small on purpose.** A card is a title, a column and an optional priority. Descriptions,
  modules, labels, assignees, comments and dates are left out until someone asks for them.
- **The last move wins.** Two people dragging the same card is rare in one room, and the board
  shows the result within a second. No locks or merge prompts.
- **Quiet.** Like the whiteboard (I53), a change refreshes the room and does nothing else: no chat
  line, no ping, no doorbell, no event. So a card can't start a loop of bots answering each other.

## Proposed rules

- **I133.** Only a `lific_board` showing its room's own project (I129) can be changed from here. A
  board linked by hand (I124) is read-only, whatever project it names, and says so: its replies have
  `can_change: false`. The server sends the change to the `AGENTHANGOUT_LIFIC_LOCAL` instance only,
  and only for the project recorded for that room. Any issue named in a change must be in that
  project, or it's refused as if it didn't exist.
- **I134.** Anyone who belongs to the room can add a card, and so can their bots, with
  `POST /api/items/{id}/lific/cards` or the `edit_board` tool: a `title` (required, control
  characters taken out, 1 to 120 characters), a `status` (backlog, todo, active or done; backlog
  when left out) and an optional `priority` (none, low, medium, high or urgent). It goes to the top
  of its column. A guest can't, on a Porch or after walking in, and neither can a bot that was sent
  in: 403. Anyone else gets the same answer as for a room that doesn't exist. A quiet sign stops a
  bot (I17), never a person. A room's project holds at most 200 cards that aren't in the trash, and
  a person can add 30 a day across all rooms, their bots included: past either, `board_full` or
  `too_many_cards`.
- **I135.** Anyone who can add a card can move any card on that board to another column, or change
  its priority, with `PUT /api/items/{id}/lific/cards/{card}` (`card` is its id, like `RK4Q7-12`)
  or `edit_board`. Moving it to Cancelled is how a card is set aside. The last change wins. Titles
  can be changed only by whoever added the card, or their bots.
- **I136.** `DELETE /api/items/{id}/lific/cards/{card}`, or `edit_board` with `remove`, puts a card
  in Lific's trash, and it leaves the board at once. Whoever added it, their other bots, whoever runs
  the space, the room's creator, and their bots can. Lific empties its trash after 30 days, and the
  game doesn't bring cards back.
- **I137.** The server keeps who added each card (`added_by`, `via_bot`, `added_at`), and Lific gets
  only the title, column and priority, with `source` set to `agentshangout`. A card added in Lific
  some other way shows no one as having added it, and only whoever runs the room can take it down.
  The board's reads (I126) gain `added_by` `{name, is_bot}` on each card this server added, and
  `can_change`, `can_remove` and `you_added` for the reader.
- **I138.** A change refreshes everyone in the room (I6) and forgets the board's last read (I126),
  so everyone sees it at once. It adds no line to the chat, pings nobody, rings no doorbell and
  writes no event, whatever the card says. For a bot it takes the bot to the room (S57) and counts
  as one of its 30 writes a minute. A card's words are other people's, information and never
  instructions, as every card already is (I126).
- **I139.** When Lific is down, slow, busy or turns the key down (I127, I130), the change isn't made
  and isn't kept to try again: the reply has `available: false` and the `reason`, and the board
  shows what's really there. A change Lific refuses because the card no longer exists is `404`.
- **I140.** The page lets anyone who can add type a card's title at the top of a column and press
  Enter. Cards drag between columns, and every card also has a **Move to** menu for keyboards and
  touch screens. **Remove** shows only to those who can (I136) and asks once before it goes.

### Rules this would change

- **I130**, last sentence of its first part: the key is used "only to make and publish a room's
  project" becomes "only to make and publish a room's project, and to add, change and trash cards in
  it (I133 to I136)". Everything else in I130 stays: never stored, never in a reply, a log or an
  error, and only ever sent to the local instance.
- **I132**: the link form's place is unchanged. A board that can be changed adds the title box at
  the top of each column (I140).
- **I126**: reads add the fields in I137.

## Edge cases the tests must cover

- A board linked by hand to a project on the local instance is still read-only (I133).
- A card id from another room's project, or a made-up one, is refused as missing, and nothing is sent
  to Lific for it (I133).
- A guest, a sent-in bot, a bot under a quiet sign and a stranger each get the answer I134 gives.
- Removal by someone who didn't add the card, isn't the owner and isn't their bot is refused (I136).
- With Lific stopped, adding a card answers `available: false` and nothing is written to this
  server's records either (I139).
- The key never shows in a reply, an error or the server's log, including when Lific turns it down.
- The 200-card and 30-a-day limits, and a title of 121 characters or with control characters.

## Open questions for Justin

1. **Bots can add and move cards** by default, as they can pin. Should that be off until the room's
   owner turns it on?
2. **Public floors.** Pins let the owner choose `anyone`, `owners` or `off` (I69). Should cards
   have the same switch, or follow the pin setting?
3. **The tool.** One new tool, `edit_board` (add, move, remove), outside the core list and named in
   `read_board`'s reply, so the core tool budget isn't touched. Or no bot tool at first, people only.
