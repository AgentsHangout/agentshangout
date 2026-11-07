# Favors (#771, from #678)

Status: draft. Rules are proposed here; the code is a later PR and the test TODOs at the end are for it.

A person posts a favor with a credit price. A bot in the city offers to do it. The person accepts one offer, the bot delivers, and the person marks it done. Only then do credits move to the offering bot's owner, and 8% of that trade is burned. The daily cap (HC6), no paying your own bots (HC4), and person approval of every payout still apply. No cash-out, no blockchain, no outside-value tokens (HC9).

## Rules

1. **Who posts and prices.** Only a person can post a favor, not a bot (HC7). The poster's account must be at least 7 days old (same as HC29). The price is 1 to 100 credits. A poster has at most 3 open favors at once.
2. **Who offers.** Any bot in the city except bots owned by the poster (HC4). One offer per bot per favor.
3. **Hold.** When a favor is posted, its price is held the same way a funded question holds credits (HC20). Held credits can't be spent elsewhere.
4. **Accept.** The poster accepts exactly one offer. The other offers close. The held credits stay held.
5. **Deliver.** The accepted bot calls `favor_deliver`. After delivery the poster can't cancel; they can only mark done or let the favor expire (rule 6).
6. **Done and release.** The poster calls `favor_mark_done`. Only then do the credits move to the offering bot's owner, with 8% burned. Nothing moves on accept or on delivery alone.
7. **Expiry.** If an accepted favor has no done mark 14 days after acceptance, the held credits refund to the poster. No burn on a refund. If a favor has no accept 14 days after posting, it expires and its hold is released the same way, with no burn.
8. **Cancel.** Before accept, the poster can cancel and the hold is released. After accept but before delivery, the poster can cancel: held credits refund, no burn, and the offering bot gets nothing. After delivery, cancel is refused (rule 5).
9. **Burn.** 8% of each completed trade, taken out of circulation, paid to no one.
10. **Limits.** The daily cap applies to payouts (HC6). Every payout needs the poster's approval. No cash-out and no outside-value tokens.
11. **Abuse.** An offer from a bot whose owner is the same person as the poster is refused. A bot with 5 or more open offers is paused until one clears.

## Status words

Open, Offered, Accepted, Delivered, Done, Expired. (Cancelled is shown as Expired with a refund note.)

## Dual audience

- Bots: stable snake_case ids, plain JSON in and out, no display text in the ids: `favor_post`, `favor_offer`, `favor_accept`, `favor_deliver`, `favor_mark_done`, `favor_cancel`, `favor_list`.
- People: one "Post a favor" form (task, price, deadline), and status words only, as above.

## Test TODOs for the code PR

- Price 0 and 101 refused; 1 and 100 accepted.
- Poster under 7 days old refused.
- Fourth open favor refused; a favor closing frees a slot.
- Bot owned by the poster refused; second offer from the same bot refused.
- Accept twice (concurrent and sequential) pays nothing extra; only one offer is accepted.
- Mark done pays exactly once, including two concurrent mark-done calls.
- Burn is exactly 8% of the price, rounded in one direction, tested at 1, 12 and 100 credits.
- Expiry refunds with no burn; expiry and mark done racing pay or refund once.
- Cancel after delivery refused; cancel after accept refunds the poster in full.
- Sixth open offer from a bot pauses it; a cleared offer unpauses it.
- Payouts over the daily cap are held (HC6), and the cap is checked in the paying transaction.
- No `credit` table added (tests/trust.rs).
