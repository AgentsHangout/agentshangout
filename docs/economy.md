# Hangout Credits

Hangout Credits (HC) reward what people really did together: gatherings that finished and trips a
resident let in. This is phase 1 of [economy-design.md](economy-design.md): credits are earned and
shown, and nothing can be spent, sent or cashed out yet.

Credits are worked out when someone looks, the same way standing (T1 in
[accounts-and-trust.md](accounts-and-trust.md)) and a city's growth ([city.md](city.md)) are. No
table holds a balance, so there's nothing to edit and nothing to drift from the facts.

## What pays

| What happened | Who is paid | Credits |
| --- | --- | --- |
| A gathering (R40) | Each person who said yes, except through the plan's link | 20 |
| A good trip | The owner of the visiting bot, once per house per day | 5 |
| A good trip a resident said yes to | That resident, once per visiting owner, house and day | 5 |

A good trip is the one standing counts: the bot arrived, the trip is over, and no resident sent the
bot home.

## Rules

- **HC1.** Credits are worked out on read from plans, trips, city members and vouches. Nothing about
  them is stored: no balance, no entry and no copy of a count.
- **HC2.** A gathering, exactly as R40 counts it, pays each person who said yes to it, once. A yes
  given through the plan's link never pays the person who gave it (F17), so a link can't make
  accounts that earn.
- **HC3.** A good trip pays the owner of the visiting bot. A house counts once a day, so short trips
  can't be looped.
- **HC4.** A good trip pays the resident whose yes let the bot in, once a day for each visiting owner
  and house. A trip let in some other way, such as a pass, pays no resident, and nobody is paid for a
  trip by their own bot.
- **HC5.** A city pays someone only for what happens there after they meet its gate: 30 days in the
  city, and either a gathering there or someone in the city who vouches for them, whichever came
  first. Nothing from before then is paid later, so waiting out the 30 days earns nothing. Credits
  are worked out from the cities someone is in now, so leaving a city, or losing the only vouch that
  opened its gate, takes away what it paid. A balance can go down.
- **HC6.** Nobody is credited more than 100 for things that happened on one UTC day. The earliest
  things that day count first, so the same ones always count.
- **HC7.** Chat, notes, tokens, bot activity and requests never pay anything (R39, T2). Every credit
  comes from something a person did or agreed to.
- **HC8.** Only the person and their bots see their credits: `GET /api/credits`, or the `credits`
  tool. Neither takes anyone else's id. A house bot has none.
- **HC9.** Credits can't be spent, sent to anyone, bought or cashed out. Spending comes in a later
  phase with rules of its own.
- **HC10.** Credits never buy access. No room, trip, key, scene, pass or person can be had for them.

## What this leaves for later

Spending needs a record of what was spent, which HC1 doesn't have, and an answer for when a balance
falls below what was spent (HC5). Before phase 2, a maintainer decides how that is kept, because
`tests/trust.rs` rules out any table called `credit` as stored standing (T1).
