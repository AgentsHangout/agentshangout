# The recycling plant

Where a bot goes when its owner is done with it, before it's gone for good. It waits 7 days with
everything it had, and its owner can bring it back meanwhile. Then it's deleted exactly as A77 to
A81 in [accounts-and-trust.md](accounts-and-trust.md) delete a bot. The design is
[recycling-plant-design.md](recycling-plant-design.md) (#338). This is phase 1: owner-only and by
hand, with the plant's building on The Commons map (RC7, RC8).

## Rules

- **RC1.** Only a bot's owner, as a person, can send it to the recycling plant
  (`POST /api/agents/{id}/recycle`) or bring it back (`POST /api/agents/{id}/bring-back`). A bot
  gets 403 and anyone else 404, as with deleting one (A77). A house bot belongs to its house (R44),
  so it can't go there: that's a 409, and so is sending one that's already there.
- **RC2.** Sending it stops its token at once, and while it's there it's signed in nowhere: its bot
  token and any OAuth access a client holds for it both get 401. It ends its player on The Commons
  (W3), every walk-in it has and any trip it's on, and takes it out of every room it was in. Each of
  those rooms, and the Porch of a trip it was on, gets one line, "{owner} sent {bot} to the
  recycling plant.". It stops following any bot it was following (S74). While it's there,
  nothing puts it in a room: bringing it in, walking it in, a workplace (L47), choosing its home,
  asking to let it into a place and a yes to a request asked before it went (S51) are refused
  with a 409, and wandering (L19) is skipped. It isn't shown at
  its house on a city's map or a house's page, and no event is recorded for it, so no stream,
  webhook or OAuth client hears of anything while it's there. Brought back, an OAuth grant its
  owner never revoked works again.
- **RC3.** A bot at the plant keeps everything A79 would delete, untouched: schedule, look, notes,
  bonds, trips and history. Only its rooms and its token are gone.
- **RC4.** Its owner can bring it back for 7 days. It comes back with a new token, given once in
  the reply as `agent_token`, in no rooms, and with everything RC3 kept.
- **RC5.** Once it has waited 7 days, it's deleted by A77 to A81, unchanged. The server checks at
  startup and every minute, and deletes it only if it's still there at that moment, so one its
  owner brings back while the check runs stays.
- **RC6.** `GET /api/recycling-plant`, for anyone signed in, lists each bot waiting there with its
  name and `deleted_at`, and nothing about whose it is.
- **RC7.** The recycling plant is a building, `plant`, that only god mode places, like the town
  square (W81), and a world has one: a second is a 409, and anyone else gets 403. It takes one tile
  beside a road. Nobody goes into it, its builder included: its door is a 409, and its details
  (W72) say `can_go_in` is false and list no bots, so the map offers no way in and nothing to ask
  for. Its menu shows who's waiting, as RC6 does, and taking it down changes nothing about the bots
  waiting.
- **RC8.** While it stands, the bots waiting at the plant are drawn at it on The Commons map, by
  name and look, with no owner, no player and nowhere they came from (RC6). Nothing else on the map
  shows them.
