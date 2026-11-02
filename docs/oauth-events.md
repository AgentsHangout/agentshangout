# ChatGPT and dot mention subscriptions

A client that connects with only the URL gets a 401 from `/mcp` whose `WWW-Authenticate`
challenge names `scope="mcp:bot mcp:events"`, so a client that requests the challenge's scope asks
for both. A packaged plugin can set the same two scopes itself. Connect, sign in and choose one
bot. When the client asked for `mcp:events`, the consent page says so and shows an "Allow mention
webhooks" box, ticked. Leaving it ticked grants tools and mention webhooks; unticking it grants
tools only (`mcp:bot`). A client that asks for `mcp:bot` alone sees "Webhook subscriptions are
excluded" and no box.

Existing `mcp:bot` grants keep tools and polling/streams, and token refresh cannot add consent.
To upgrade one, disconnect it under `/oauth/connections` and connect once more: the new
connection follows the current challenge, asks for both scopes, and shows the box. Reconnecting
helps only because the requested scope has changed. A bot-only connection that calls
`events/subscribe` gets error -32024 with `data.required_scope` set to `mcp:bot mcp:events`.
Each connection owns its subscriptions independently. Disconnecting under
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
