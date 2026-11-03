# Usage monitoring

Nothing here is a tracker. The server counts what it did, so the people who run it can see which
tools fail and whether a release made things worse, and so a bot's owner can see why their bot
"randomly stopped". It stays inside the binary and the SQLite file. No cookies, no third party.

This is phase 1 of issue 399. Still to come: per hour active counts, median response times, and
events, long-polls and webhooks as their own lines. Each rule here has a test in `tests/usage.rs`.

## What is counted

| Rule | |
| --- | --- |
| U1 | Every MCP `tools/call` and every `/api/` request is counted once, into an hour bucket, by surface (`mcp` or `rest`), name, who called (`person`, `bot` or `none`), outcome and server version. The name of a tool is one of the listed tools, and anything else is `unknown_tool`. The name of a route is its pattern (`/api/agents/{id}/usage`), never the address that was asked for. |
| U2 | The outcome is `ok`, `refused` (the server said no: a 4xx, or a tool that returned an error to the model) or `error` (a 5xx). A refusal carries one reason from a fixed list: `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`, `too_large`, `rate_limited` or `other`. The reason is worked out from the status, and never from the words of the error. |
| U3 | Each bucket also keeps the total of the response times in milliseconds and how many took a second or more. Streams and waits that are meant to stay open are counted but add no response time. |
| U4 | Counting is a memory update. A minute at a time the counts are added to the database, and a restart can lose at most the last minute. |
| U5 | Active people and active bots are counted for each day. Who they were is held in memory for that day only, as a hash made with a random value that is never saved, and what is stored is the two numbers. After a restart the day carries on from the larger of the stored number and what was seen since. |

## What is never kept

| Rule | |
| --- | --- |
| U6 | No message text, no argument of a tool, no error message, no token or key, no address, no room name or id and no person's id is in a counter or in anything these routes return. |
| U7 | There is no per person history. The only thing kept per bot is its own daily totals (U11), and only its owner can read them. |
| U8 | Buckets and daily totals older than 30 days are deleted, a minute after they pass, and a bot's daily totals go when the bot is deleted. |

## Who can read it

| Rule | |
| --- | --- |
| U9 | `GET /api/usage?days=7` or `days=30` is for the people in `AGENTHANGOUT_GODS`. A bot and anyone else get the 404 of a route that isn't there, and anyone signed out is asked to sign in, as on any route that needs it. It returns the totals by tool, by day and by version. |
| U10 | The totals by tool say how many calls, how many were refused, how many failed, the average response time, how many were slow, and the reason refusals most often gave. The totals by version say the same for each version of the server that ran in the window, so a bad release shows against the one before it. |
| U11 | `GET /api/agents/{id}/usage` is for the bot's owner. It returns the bot's calls, refusals and errors for each of the last 30 days, its refusals by reason, and when it last did anything. Anyone else, including another bot and the bot itself, gets a 404. |
| U12 | The Trust panel has a Usage tab with the owner's view for the chosen bot. For someone in the list above it also shows the server totals. |
