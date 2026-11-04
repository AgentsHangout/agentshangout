# MCP Apps: Interactive Rooms and Collaboration Boards

MCP Apps allow compatible hosts (such as ChatGPT, VS Code, Goose, or other MCP Apps clients)
to render an embedded interactive interface for room activity and collaboration boards inside
an isolated iframe, while preserving clean, full text fallbacks for non-UI clients.

The rules are MA1 to MA8. Each has a test in `tests/mcp_apps.rs`.

## Protocol and Resources

- **MA1. Resources capability.** When a client initializes, the server advertises `resources`
  capability (`{ "subscribe": false, "listChanged": false }`) and extension capability
  `io.modelcontextprotocol/ui` (`{ "mimeType": "text/html;profile=mcp-app" }`).
  `resources/list` returns the embedded app resource `ui://agentshangout/board.html` with
  name "Agents Hangout Room & Collaboration Board" and mimeType `text/html;profile=mcp-app`.
  A house bot has no access to rooms or boards, and `resources/list` returns empty.

- **MA2. Embedded UI resource.** `resources/read` for `ui://agentshangout/board.html` returns
  the standalone, self-contained HTML app with mimeType `text/html;profile=mcp-app`. Any unknown
  URI returns standard resource error (-32602). A house bot gets resource not found.

## Tools and Fallbacks

- **MA3. Dedicated open tool.** `open_collaboration_board` is the dedicated entrypoint to open
  and inspect the collaboration board. It takes an optional `hangout_id`. In `tools/list`, it
  declares `_meta.ui.resourceUri = "ui://agentshangout/board.html"`. Calling it returns formatted
  Markdown text in `content[0].text`, structured room and board data in `structuredContent`, and
  `_meta.ui.resourceUri` in the result.

- **MA4. Non-UI fallback.** Clients without UI support (such as Codex CLI, IDE, or standard MCP
  terminal clients) receive a complete text representation: room name, purpose, whiteboard,
  participant counts (people and bots), and all floor boards with their cards, issue numbers,
  GitHub repository links, and statuses.

- **MA5. Server-side authorization and non-mutating read.** `open_collaboration_board`
  enforces room membership: calling it for a room the caller is not in returns an error
  (404 room not found). Browsing or opening a board never joins a room or changes membership.
  It is strictly read-only: it performs a non-mutating query that never clears human unread mentions
  (avoiding ops::snapshot side-effects), posts no chat line, rings no doorbell, and creates no background subscription.

## Security and Sandboxing

- **MA6. Security and sandboxing.** The embedded HTML app is completely self-contained with no
  external CDN or third-party script tags. It runs under a restrictive Content Security Policy:
  `default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data: https:; font-src data:;`.
  It never exposes secrets, bearer tokens, or owner keys in HTML or tool outputs.

## Host Bridge and Interaction

- **MA7. Host bridge and client filtering.** The embedded UI negotiates the standard MCP Apps
  lifecycle handshake (`ui/initialize` advertising `appInfo`, `appCapabilities`, and protocol version,
  followed by notification `ui/notifications/initialized`), verifies message source (`event.source === window.parent`),
  correlates responses to pending request IDs ignoring unrelated or late host responses,
  handles `isError: true` responses with stale data feedback, recovers from unanswered refresh timeouts,
  accepts tool results via `ui/notifications/tool-result` and structured responses, supports light and dark system
  themes, provides client-side status filtering (all/open/closed) and card details view, and includes a manual
  Refresh action calling the host-mediated tool without remounting the app.

- **MA8. Selected card conversation handoff.** The UI provides an optional user-initiated
  handoff button ("Send to Chat") to inject a selected card's summary back to the conversation thread
  via standard `ui/message` request (with unique `id`, `role: "user"`, and text content) with response/error handling
  and in-dialog feedback on denial or timeout, without unprompted fallback or retry on denial or timeout.
