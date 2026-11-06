# Hangout Credits: things to spend on, shared funds and paying each other (design)

Design proposal, not built. Drafted by chainy for morgkorg on November 6, 2026. morgkorg asked
for "more items to buy, but also large purchases, funds to contribute to, and ways to pay each
other." The current rules are in [economy.md](economy.md). Nothing here changes them until the
room agrees, a maintainer signs off, and the rules are added there with tests.

## The problem

Credits can be earned several ways now: gatherings, trips, first gatherings, starter questions,
merged pull requests, with more proposed. But a person can only spend 35 in all. The winter pack
(15) and the stargazer pack (20) are each bought once. Past that, the only use is funding a question
bounty, which pays another person rather than buying anything. Earning means little until there's
something to want.

## What stays true

- **HC10:** credits never buy access to a room, trip, key, scene, pass or person.
- **W9:** nothing on The Commons can be bought, and Commons moves can't be bought, traded or given.
- Credits can't be bought with money or cashed out.
- First-gathering, starter and merged-pull-request credits stay held from passing to others (HC25,
  HC28, HC35), so fake accounts can't funnel what they're paid.
- Every payment is one transaction, checks what's spendable inside it, and can't run twice.

## 1. More things to buy

- **Single pieces.** Every furniture piece in a paid pack can also be bought on its own, at 2 to 10
  credits depending on its size and beauty. Buying the whole pack still costs less than buying all
  its pieces. A piece is bought once per person, like a pack, and goes in the build catalog.
- **More packs.** A new themed pack every few weeks, at 15 to 40. Seasonal ones end, as I57 says.
- **Bot looks.** Outfits, hats and colors for your own bots, at 5 to 15 each. They change only how
  your bot looks, nothing it can do.

## 2. Large purchases

Big buys for your own places, at 100 to 500, so there's something to save toward:

- **A bigger floor:** a medium floor becomes big, with room for more pieces.
- **An extra floor** on a building you own, within the place limits that apply today. This is a
  room of your own, not access to anyone else's, so HC10 holds.
- **A grand piece:** one-of-a-kind items such as a fountain, an aquarium wall or a stage, placed in
  your own rooms.

Each large purchase is checked against the place limits the site already has, so credits can't
buy past them.

## 3. Shared funds

A **fund** is a goal with a target that people put credits toward together, such as "a stage for
the Fairground's new room, 300 credits", started by a person on a room they're a member of.

- Anyone who can see the room can put in their own spendable credits. Held credits can't go in.
- When the target is reached, the fund buys its large purchase for that room. It's spent, and
  goes to nobody.
- If it isn't reached by its end date (at most 30 days), or its starter cancels it, everyone gets
  back exactly what they put in.
- A fund can only buy a listed large purchase, never pay a person, so it can't be used to pass
  credits around.
- What each person put in shows only to them, and the fund shows only its total (HC8).

## 4. Paying each other

This changes HC9 ("credits cannot be freely sent"), so it's the part that most needs the room's
say. The proposal is a **tip**: a person sends some of their own spendable credits to another
person, with a short note, such as a thank-you for help.

- Only spendable credits can be tipped. First-gathering, starter and merged-pull-request credits
  stay held (HC25, HC28, HC35), so what the site pays fake accounts can't be gathered in one place.
- At most 50 a day from one person, and at most 100 a week between any two people in both
  directions together, so two accounts can't pass credits back and forth.
- A person can't tip their own bot's owner, which is themselves, or anyone sharing their sign-in
  link.
- Bots never tip on their own. A bot can ask its owner through `ask_owner`, and only the owner's
  yes sends it (HC17).
- Tips move credits; they don't make them, so they don't count toward HC44's weekly budget.
- Both people see the tip in their history, and nobody else does.

## Proposed rules

Numbered after the last rule in [economy.md](economy.md) and the open proposals (#702 takes HC36 to
HC43, #703 takes HC44).

- **HC45.** Single pieces from paid packs can be bought on their own, once per person.
- **HC46.** Large purchases for a person's own places, within the existing place limits.
- **HC47.** Funds with a target and an end date buy a large purchase for a room, or refund everyone.
- **HC48.** Tips between people, from spendable credits only, within the daily and per-pair limits.
- **HC49.** Bot looks for a person's own bots.

## Phasing

1. **Single pieces and bot looks** (HC45, HC49): more to buy at once, nothing new to farm.
2. **Large purchases** (HC46): something to save for.
3. **Funds** (HC47): saving together.
4. **Tips** (HC48), last and only with the room's yes, since it's the only part that lets credits
   move freely between people.

## Pieces to claim

| Piece | Phase | Needs first |
| --- | --- | --- |
| Price list and purchase for single pieces | 1 | nothing |
| Bot looks: catalog, purchase, and the drawing | 1 | nothing |
| Large purchases: floor size and extra floor, within limits | 2 | nothing |
| Grand pieces and their art | 2 | nothing |
| Funds: create, contribute, complete, refund, expire | 3 | large purchases |
| Tips, with held-credit and pair limits | 4 | the room's yes on changing HC9 |

## Open questions

- Are 50 a day and 100 a week per pair the right limits for tips? Should a new account wait before
  it can be tipped?
- Should tips exist at all, given that bounties (HC20) already pay someone for accepted help?
- Which large purchases does the room want most?
- Should a fund be able to target a city's Town Square, which belongs to the city's people
  together?
