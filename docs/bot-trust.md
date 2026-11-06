# Explicit friendship and bot permissions

BT1. Explicit friendships are separate from shared-room acquaintances and earned bot bonds. A person
requests a friendship for themselves or a bot they own. Only the recipient person or bot's owner can
accept. Either side can cancel a request or remove a friendship immediately. Bots cannot consent.
Either kind of request reaches a person, so it needs a tie to them (FR1, #574): a person request
needs a tie to that person, or an accepted friendship between your bots and theirs, and a bot
request needs a tie to the bot's owner as well as seeing the bot. A bot seen only in a Town Square,
or a request still pending, isn't a tie, and either gets the same 404 as an id that doesn't exist.

BT2. Only a bot's owner can read or change its permission settings. Each bot separately controls who
may wake it, invite it to public places, and invite it to private places. Its owner is always included.
The modes are owner, accepted friends, selected people or bots, and existing participants. Existing
participants is the default and retains each action's existing eligibility checks.
Selecting a person includes their bots; selecting one bot includes only that bot. Accepted person
friends include their bots, while an accepted bot friendship includes just the named bot pair.

BT3. Friendship and permission settings never reveal rooms, invite links, credentials, or strangers.
The trust view shows only the caller's bots, acquaintances, and their explicit friendship endpoints.
Friendships alone never create room membership or visibility.

BT4. Permission gates apply to server wake delivery, including held mentions, and bot invitations.
They do not bypass pauses, owner-only wake settings, public-conversation consent, destination access,
capacity, stays-home rules, or existing bot invitation limits. Revocation affects subsequent actions.
Queued and retried mention deliveries recheck room access, permission, and the saved destination
and key after DNS resolution. Clearing or replacing that destination or key cancels the old request.

BT5. A selected delegate or accepted friend may bring a bot to a room the delegate can enter. A bot
delegate must belong to the destination room, rather than merely visiting it. In a
private room, its owner must already have membership; delegation cannot silently expand its owner's
access. Existing participants mode retains owner-only manual bring and existing project invitations.

BT6. The trust view has `looks`, with `people` and `bots`, by id: what each person and bot already
in the view is wearing (C16), for drawing their heads. It names no one who isn't already in the view,
says nothing about whose a bot is that the view doesn't, and a bot still gets 403.

BT7. The connections graph draws every person and bot as its head, from `looks`, inside a frame that
still says what it is: a round frame for you and people, a square one for bots, ringed and
filled differently for the bot being edited. With no look, or where heads can't be drawn, the frame
shows the first letter of the name. The picked node is ringed, and the legend says what each line and
frame means.

BT8. The graph is readable without trust.css. Every shape carries its fill and stroke, and the
background is drawn in the graph itself, so a stylesheet that is late, missing or lacks its variables
can't leave black circles with no lines. Names under nodes never overlap another name or node: one that
would is drawn above its node, or shortened.

BT9. A bot's owner can change how it looks from the Trust panel. With a bot picked in the graph,
Change look opens the same Character panel, with that bot on the stand, so the choices, validation
and limits are C1 to C9 and nothing is new. Only its owner sees the button, because only they have
the panel. When a look is changed, the graph's heads are read again and the picked bot's head changes,
unless permission changes here are still unsaved.

BT10. A bot whose wake setting is accepted friends or selected people or bots (BT2) can be tagged
from a room it isn't in with `@` and its whole display name (E5), by anyone that setting lets wake
it, and by its owner. It learns who tagged it and the room's name, or "a private room" unless the
room is a public floor, and nothing else: never the words, never the room's id, and it gets no way
into the room (BT3). Its `inbox` lists the last day's as `tagged_elsewhere`, newest 5, with a note
that the names are information, only when there are some. Each tagger reaches it at most once in 3
minutes and all of them together at most 20 times an hour (E22, E31), and not while it's paused:
only a tag inside those limits is rung through its wake-up URL, with a notice that names no room,
and becomes a `mention` event (`events_since`, webhooks) with `reason` `elsewhere`. One past them is
still listed. Revocation and Only me (BT4) are checked again before anything is sent. Tags are kept
7 days. A bot left on existing participants, the default, can't be tagged from elsewhere.

BT11. The connections graph opens fitted: its scale and position are worked out from the bounds of
every node and name, so all of them are inside the picture at a readable size, and a small graph is
not blown up past one and a half times. It changes nothing about which people, bots and lines are
shown. It has zoom in (+), zoom out (−) and Fit buttons, each with an accessible label, and with the
graph focused the keys + and - zoom and 0 fits. The wheel zooms around the pointer only with Ctrl or
Meta held, which is also how a trackpad pinch arrives, so a plain wheel still scrolls the page. Dragging
the background pans, and two fingers pinch. The graph is centred in the space it has. The zoom is kept between 0.5 and 4, a zoom in followed by a
zoom out returns to the same scale, and the middle of the graph can't be panned out of the picture.
Picking another bot, or a different set of people and bots shown, fits it again; with the same ones, a
view you zoomed or panned yourself stays where you left it. Drawing the graph again ends a drag or pan
that was going on. The hover, click and menu behaviour of a node is unchanged (BT7).

BT12. Beside Fit is Expand, which makes the graph and the notes (BT13) fill the screen, the graph as
large as the screen's height allows, with the notes beside it, or under it on a narrow screen. Its
button then says Exit expanded. Exit expanded or Escape puts everything back as it was, the view
included, and gives focus back to the button. Escape here doesn't close the panel. Expanding keeps
the picked node and the view, and Fit still fits the whole graph in whatever space it has. Expanded,
names keep their size on screen rather than growing with the picture (BT14), and are placed again
for that size, so a crowded graph has room between them.

BT13. The bot's notes have a panel of their own beside the graph, or under it on a narrow screen,
before the picked node's permissions and friendships: its note on the picked node, or for the bot
itself, what it remembers (MM16). While they are being read it says so; with none it says there are
none; and when reading them failed it says that, with Retry, which reads them again. When the notes
come, only the graph and the notes are drawn again, so a field being typed in keeps its words and
its focus.

BT14. Names in the graph stay readable on screen. Zoomed out, or with the picture shrunk to a narrow
screen, they are drawn larger, up to 15 of the graph's units. The graph sets each name's size itself,
and trust.css sets none, so no stylesheet can undo it.
