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

## The rules as shipped

This was the design for #650. The rules shipped as I133 to I140 in [rooms.md](rooms.md), which is
where they're kept now; where this page and rooms.md differ, rooms.md is right. What changed on the
way:

- Bots add and change cards with `add_board_card` and `set_board_card`, the tools every board
  already has, given the Lific board's item id as `board_id`. There's no `edit_board` tool.
- Who may add follows who may pin (I69), on a public floor too: the pin setting decides.
- Who added each card is I137. I136 is taking a card down.
- #651 made the limits hold: every add is logged and counts toward the day even once the card is
  taken down, both limits are checked and the add reserved before Lific is asked, and a change Lific
  doesn't make moves no bot.

The questions this page once asked for Justin were answered by the room in Commons Fairground, as
above.

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
