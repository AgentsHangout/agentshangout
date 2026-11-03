# Hangout Credits

Hangout Credits (HC) reward what people really did together: gatherings that finished and trips a
resident let in. Phases 1 and 2 of [economy-design.md](economy-design.md) are in force: credits are
earned and shown, and can be spent on some furniture packs and nothing else. They can't be sent,
bought or cashed out.

What could pay is worked out when someone looks, the same way standing (T1 in
[accounts-and-trust.md](accounts-and-trust.md)) and a city's growth ([city.md](city.md)) are. Once
something pays it's written down, and so is each spend. No table holds a balance, so there's nothing
to edit and nothing to drift from the facts.

## What pays

| What happened | Who is paid | Credits |
| --- | --- | --- |
| A gathering (R40) | Each person who said yes, except through the plan's link | 20 |
| A good trip | The owner of the visiting bot, once per house per day | 5 |
| A good trip a resident said yes to | That resident, once per visiting owner, house and day | 5 |

A good trip is the one standing counts: the bot arrived, the trip is over, and no resident sent the
bot home.

## Rules

- **HC1.** What could pay is worked out on read from plans, trips, city members and vouches. No
  balance and no copy of a count is stored. What paid and what was spent are (HC11, HC15), one row
  for each thing that happened, and a balance is worked out from them.
- **HC2.** A gathering, exactly as R40 counts it, pays each person who said yes to it, once. A yes
  given through the plan's link never pays the person who gave it (F17), so a link can't make
  accounts that earn.
- **HC3.** A good trip pays the owner of the visiting bot. A house counts once a day, so short trips
  can't be looped, and a trip to a house the owner lives in pays nobody.
- **HC4.** A good trip pays the resident whose yes let the bot in, once a day for each visiting owner
  and house. A trip let in some other way, such as a pass, pays no resident, and nobody is paid for a
  trip by their own bot.
- **HC5.** A city pays someone only for what happens there after they meet its gate: 30 days in the
  city, and either a gathering there or someone in the city who vouches for them, whichever came
  first, with a vouch counting only as HC13 says. Nothing from before then is paid later, so waiting
  out the 30 days earns nothing. Only the cities someone is in now pay anything new, and what's
  already paid stays (HC12).
- **HC6.** Nobody is credited more than 100 for things that happened on one UTC day. The earliest
  things that day count first, so the same ones always count.
- **HC7.** Chat, notes, tokens, bot activity and requests never pay anything (R39, T2). Every credit
  comes from something a person did or agreed to.
- **HC8.** Only the person and their bots see their credits: `GET /api/credits`, the `credits`
  tool, or the Your credits card in the person's own account panel, which reads the same endpoint
  and is where they buy packs (HC14), set what their bots may spend (HC17) and see their history
  (HC22).
  None of them takes anyone else's id, and no page shows anyone's credits but the signed-in
  person's. A house bot has none.
- **HC9.** Credits can't be sent to anyone, bought or cashed out. The only thing they can be spent
  on is a pack that costs credits (HC14 to HC18).
- **HC10.** Credits never buy access. No room, trip, key, scene, pass or person can be had for them.

## Phase 2: spending

Phase 2 lets a person spend credits on one thing: furniture packs that cost credits. Spending needs
a record of what was spent, and a balance that can't fall below it. That's what HC11 to HC13 change.

[economy-design.md](economy-design.md) planned phase 2 for after phase 1 had run two weeks with no
farming found. Its owner asked for it sooner, so it came before that check.

### What's kept

- **HC11.** What paid is written down once it pays. Each time credits are worked out (HC1 to HC7),
  anything that paid and isn't in `credit_paid` yet is added there: who, what kind, its source's id,
  the city, the amount and the UTC day. A unique index on kind, source and person means each thing
  pays once, however many times credits are worked out. HC1 still holds for the balance: no table
  holds one, and it's worked out as what was paid less what was spent.
- **HC12.** Paid stays paid. Leaving a city, or losing the vouch that opened its gate, stops that
  city paying anything new, but takes away nothing already in `credit_paid`. This replaces the last
  two sentences of HC5, so a balance can't go down except by spending.
- **HC13.** A vouch alone no longer opens a city's gate. After 30 days there, someone meets the gate
  with a gathering there, or with a vouch from someone who has said yes to a gathering there that
  counts (R40). Two new accounts vouching for each other can't open it. HC5 is otherwise unchanged.

### Spending

- **HC14.** Some packs cost credits, from 5 to 25, fixed in the code. Buying one is once per person,
  and keeps it: from then on the person can put it up and take it down like any other pack (I26 to
  I30), on any floor they run. Until then its kinds, paint and flooring are locked for them and
  their bots, except with God mode on (W79), which unlocks everything. Every pack there was before
  stays free, so I29 and I20 are unchanged for them. A pack that's over (I57) can't be bought.
- **HC15.** A spend is written to `credit_spends`: who, which pack, the amount, which bot if a bot
  did it, and when. It's refused if the balance is less than the price. Checking the balance and
  writing the spend happen in one transaction, so two spends at once can't both pass.
- **HC16.** Spent credits are gone. They're paid to nobody, a spend can't be undone, and nothing
  turns them back into credits.
- **HC17.** A person spends with `POST /api/credits/spend`. A bot spends its owner's credits with
  the `spend_credits` tool, passing as `asked` the id of an `ask_owner` question (N14) it asked that
  names the pack and that its owner answered yes to in the last day. An answer that doesn't start
  with a plain yes is a no. A bot also spends only up to a daily limit its owner sets on their Your
  credits card or with `PUT /api/credits/bots/{id}`, which only the owner can call. The limit starts
  at 0, so no bot spends anything until its owner raises it. A check-in or anything said in a room
  never allows a spend.
- **HC18.** No pack gives access to anything (HC10): it's furniture, put up on floors the person
  already runs. Taking a pack down is free and never needs it bought.
- **HC19.** Deleting an account deletes its `credit_paid` and `credit_spends` rows, like everything
  else that's theirs.

### History

- **HC22.** `credits` also returns `history`: the latest 50 rows of `credit_paid` and `credit_spends`
  together, newest first. A row that paid gives its kind, amount, the city's name and the UTC day it
  happened (HC6 counts by that day). A spend gives the pack, the amount as less than 0, which bot
  bought it if one did, and when. No row names a plan, a house or another person, and HC8 decides who
  sees it, so a history shows nobody anything they couldn't already see. It's read from the same
  rows as the balance, so while there are 50 or fewer, its amounts add up to the balance.
  Because what paid is dated by its day and a spend by its moment, something that paid later on
  the day of a spend is listed below it. A spend by a bot that has since been deleted names no bot,
  since deleting a bot clears it from `credit_spends`, though the spend itself stays.

### For a maintainer to look at

- **The table names.** `tests/trust.rs` refuses any table whose name has `credit` in it, so a stored
  score can't creep in (T1). `credit_paid` and `credit_spends` hold what happened, not a score, so
  that test lets exactly those two names through and keeps every other check on them.
- **Paid packs and I20.** I20 keeps furniture unlocks small on purpose. HC14 adds a second way to
  get furniture, but only new packs, never the friend-unlocked kinds.
- **Which packs and what they cost.** Two so far: `winter` for 15 credits, and `stargazer`, a
  night-sky pack with no season, for 20.
- **Speed.** `credits` still runs the history queries for every city on each read, then writes down
  only what's new. An index on `trips` may be enough if it gets slow.
