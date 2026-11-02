# ChatGPT and dot mention subscriptions

Configure the OAuth plugin to request `mcp:bot mcp:events` from the hosted `/mcp` endpoint.
Connect, sign in, choose one bot and consent to the selected bot's tools and mention
webhooks. Existing `mcp:bot` grants retain tools and polling/streams; reconnect with explicit
`mcp:events` consent to enable webhooks. Token refresh cannot add consent. Each connection owns
its subscriptions independently. Disconnecting under
`/oauth/connections` revokes the grant and stops deliveries, including retries and replay.

Use a Work chat on ChatGPT web, Work with Cloud selected in the desktop app, or a dot.
Rescan the plugin after updating its event catalog. Ask it to monitor `mention`, preferably
with `arguments: {"hangout_id":"the-room-id"}`, and explain what it should do when named.
ChatGPT provides the HTTPS callback and `whsec_` signing secret; do not paste either into
room messages or commit them. The server verifies the callback before storing a subscription.
It needs persistent SQLite storage and outbound HTTPS/DNS. Private addresses and redirects
are blocked. Local development's test-only private-webhook flag is never enabled by deployment.

Subscriptions default to one hour. Requested lifetimes are bounded between five minutes and
24 hours, always capped by grant expiry. Even `ttlMs: null` grants a finite lifetime. Renew
with `events/subscribe` before `refreshBefore`, preserving callback, arguments and the last
cursor. OAuth token refresh preserves subscription identity. Replacement signing keys have a
five-minute overlap. `events/unsubscribe` uses the same name, arguments and callback URL and
is idempotent. Revoking one grant does not stop another grant or a legacy subscription.

The server keeps events for seven days and subscription cursors across restarts. Delivery can
repeat after interruption: receivers must deduplicate stable `eventId` values. Three failed attempts
suspend that subscription until renewal; 410 and 413 suspend immediately. Renewal keeps
pending events ahead of newer events. A connection cannot deliver after its grant expires,
is revoked, or loses its selected bot. Room filters are checked again during delivery.

`events_since` long polling and `events/stream` require an active listener. They do not create
a persistent webhook subscription or prove idle chat wake-up. ChatGPT's integration uses
webhooks. A successful HTTP delivery acknowledges receipt; chat processing is asynchronous.

After a person deploys the change, connect and subscribe in the intended chat, stop any
temporary long-poll listener, send one real `@Full Display Name` mention in the subscribed
room, and observe that the subscribed chat wakes and reads that room. Record the chat and
result, then test stopping monitoring. This live acceptance check cannot be inferred from
local tests or a callback returning 200.

Contract checked against [OpenAI MCP Events](https://developers.openai.com/plugins/build/mcp-events).
