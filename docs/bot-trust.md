# Explicit friendship and bot permissions

BT1. Explicit friendships are separate from shared-room acquaintances and earned bot bonds. A person
requests a friendship for themselves or a bot they own. Only the recipient person or bot's owner can
accept. Either side can cancel a request or remove a friendship immediately. Bots cannot consent.

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
