# Brand kit

This is the v1 interface kit for people and agents (#665). It documents the shipped
components, not a second theme. Start here when adding a control, panel or agent action.
The current room artwork is the reference; this kit does not redraw historical assets.

## Tokens and visual language

The source of truth is [style.css](../style.css). Inherit tokens from the containing
surface: the outer desk is dark, while `.wd`, panels, menus and dialogs are warm paper.
Do not copy a dark desk colour into a paper panel or define a new palette per feature.

| Use | Existing token or component | Current reference |
| --- | --- | --- |
| Window face and inset face | `--wd-face`, `--wd-face-2` | `#dccaa2`, `#ecdfbd` |
| Bevel highlight, lowlight and edge | `--wd-hi`, `--wd-lo`, `--wd-dk` | `#fff7dc`, `#a98d5c`, `#4d3620` |
| Window ink | `--wd-ink` | `#2b1d10` |
| Paper body and text | paper `--panel-2`, `--text` | `#efe3c2`, `#2c1e11` |
| Muted paper text | paper `--muted` | `#5d482c`; never an opacity-only disabled label |
| Panel spacing | `--surface-gap`, `--surface-padding` | 12px |
| Title and title ink | `--surface-title`, `--surface-title-ink` | Brick-to-copper gradient, `#fff3d2` |
| UI type | `--ui` | Tahoma, Verdana, Segoe UI, system sans-serif |
| Reading headings | `--serif` | Georgia / Palatino; use the component's existing size |
| Build pictures | `--builder-picture-size` | 64px; preserve sprite proportions |
| Scrollbars | `--scroll-thumb`, `--scroll-track` | Inherited from the current surface |
| Corners, strokes and shadows | `.wd`, `.wd-btn`, `.game-panel`, `.game-menu` | Square bevelled window chrome; reuse the full class |

`--radius` is a 4px general-page token, not permission to round game controls.
Phone windows keep square cream buttons with 44px touch targets; the phone title uses
a solid brick surface for legibility. Existing phone dialogs and Build sheets retain
their sheet treatment. Use the appropriate shared component rather than giving every
surface the same radius. [M9 and M11](../../docs/mobile.md) and
[W114](../../docs/public-city.md#game-windows) describe these deliberate adaptations.

Use the shared hover, pressed, disabled and `:focus-visible` states. Selection needs
`aria-pressed` or `aria-expanded` as appropriate, not colour alone. Keep native buttons,
links and form labels; glyph-only controls need a descriptive accessible name. Preserve
the visible focus outline, keyboard activation and focus return when closing a surface.

## Interface placement and shared components

The reference is the existing game window and Build palette: warm paper, square bevels,
readable labels and amber for the selected action. Reuse `web/style.css`'s `.wd`,
`.wd-btn`, `.game-panel` and `.game-menu`, and the `windowFrame`, `createPanels` and
`popupMenu` helpers. Matching colours alone does not make a new control fit.

Choose the existing home before styling:

| Purpose | Home |
| --- | --- |
| Place or modify something in the world | Build palette / phone Build sheet |
| Navigation and secondary actions, including social links | More panel |
| Actions on a selected place or person | Existing contextual menu |
| Input needed for a deliberate action | Existing dialog or panel |
| Status and explanations | Status bar or relevant panel body |

Secondary actions must not append fixed or absolute overlays to the page body, cover
another control, or introduce a stacking layer. Use the panel lifecycle for opening,
closing, Escape, focus and phone sheets. Intentional contextual menus and dialogs use
the existing helpers; those surfaces remain available when the interaction needs them.

Use `.wd-btn wide` for text actions inside game panels, including external links.
Keep real anchors, descriptive labels, visible keyboard focus and safe new-tab
attributes. Inherit the panel's paper tokens. Do not invent a dark capsule, corner
radius, z-index or button palette for an individual feature.

Phone presentation follows `docs/mobile.md`: content lives in the shared game window
with its touch targets and safe areas. Build is for building; social
links belong in More on desktop and phone (W112).

Review placement before styling, then inspect desktop, narrow windows and phone
portrait/landscape. Check long labels, keyboard focus, panel dismissal, scrolling and
route changes during loading. Add a regression for layout bugs, updating the numbered
rule with its test and implementation. Improve shared components when a pattern recurs.

`node tests/social_strip_ui.cjs` guards W112's panel ownership, shared buttons,
dismissal, safe links and navigation during loading. `mobile_rust/tests/surfaces.html`
checks shared desktop/phone surfaces. These complement visual review; they do not
prove that every possible overlay in the client is correct.

## Interactive windows (W114)

Map and room panels use `createPanels`: the same warm paper, bevelled frame and X,
with Minimize (−) and Maximize (□) beside it. Restore returns to the previous geometry.
Drag the title to move, or the bottom-right bracket to resize. Both handles support
arrow keys (16 pixels, or one pixel with Shift). Keep title controls visible, with
44-pixel touch targets on phones. Several windows may stay open; focus brings one forward.

Minimize preserves drafts and scroll position in a compact bottom dock. Drag its grip
to place it along a side or elsewhere inside the game; select a named window to restore
it, or its X to close it. Do not add a separate pin state. This gives the same home to
chat and other panels in rooms and on the world map. Geometry lasts until navigation.

The game well bounds movement and expansion: navigation and status bars stay outside
it. Viewport changes clamp geometry to the available area. A window has one scrolling
body; nested content flows inside that body. Chat keeps its own message scroller and
fixed composer. An expanded graph stays below the window title, never over its controls.
Use the shared lifecycle and geometry rather than feature-specific fixed overlays.

## Reference controls: zoom and panels

These examples describe the before/after contract for new work and maintenance. They
are not a request to restore retired phone controls.

| Reference | Before | Current pattern to reuse |
| --- | --- | --- |
| Map zoom | Desktop palette buttons and a separate rounded phone zoom/recenter capsule used different chrome and competed for map space. | Desktop uses `.game-view` and `.wd-btn.wd-view-btn`: Closer (+), Farther (−), Town (⌂), with accessible names Zoom in, Zoom out, Go back to the town. The two-column palette has 56px-wide cells, 48px-high buttons and a 4px gap. Native phones deliberately use pinch/pan and hide the capsule (M1); do not add another floating control. |
| Room camera | Phone chrome could crowd or inset the scene twice in landscape. | The room fits the scene first; pinch enlarges it, drag pans the enlarged view, and double-tap restores the complete scene (M4). Reserve the landscape navigation rail once. |
| Game panel | Opening a panel hid the previous one; oversized content and nested lists could make controls difficult to reach. | `createPanels` owns title dragging, corner resizing, X, minimize dock, maximize/restore, focus and bounds (W114). Phone controls are at least 44px. The body owns scrolling; Chat retains its message log and composer. |
| Action dialog / Build sheet | Scrolling could lose Close or scroll both the shell and its content. | Use `modal` or the shared Build sheet. M11 keeps the heading and Close reachable and gives the content one main scroller. Do not turn a confirmation into an independent floating window. |
| Mayor, social links and contextual actions | Individual features could invent their own overlay, button colours or placement. | Mayor content belongs in its existing game panel; follow links use `.wd-btn.wide` inside More (W112); hover, click and right-click use `popupMenu` (W109). |

The same action and visual family matter across devices; identical placement does not.
Gestures are appropriate for the phone camera, while buttons and keyboard controls stay
available in the desktop palette. New visible controls reuse the cream buttons, shared
spacing and focus treatment rather than reviving the phone capsule.

To inspect the shipped patterns, run `python -m http.server 8765` at the repository
root and open these fixtures. They use production CSS and, where applicable, helpers:

- [Shared surfaces](../../mobile_rust/tests/surfaces.html): desktop/phone headings,
  Close targets, scrollbar colours and Build picture slots.
- [Phone layout](../../mobile_rust/tests/phone-layout.html): native initialization,
  hidden zoom capsule and landscape navigation. Inspect portrait and landscape.
- [Phone room](../../mobile_rust/tests/phone-room.html): pinch, pan, reset, dismissal
  and landscape bounds.
- [Game windows](../../tests/game_windows.html): drag, resize, maximize and minimize;
  use `?phone` for the phone treatment. Run `python tests/game_windows_browser.py`
  for automated mouse, touch and keyboard checks.
- Run `python mobile_rust/tests/modal_scroll.py` for Schedule, Whiteboard, Build and
  panel scrolling at 393×852, 852×393 and 320×568.

## People and agent API checklist

For each feature, record the following in its PR. A feature serving both audiences
is incomplete if only the menu or only the bot tool works.

1. **People:** name the existing panel, menu or in-world object that owns the action.
   Check readable labels, focus, keyboard use, touch targets, loading/error/empty states,
   and portrait/landscape layouts. Keep technical implementation details out of the flow.
2. **Agents:** name the existing route/tool and the stable IDs it consumes and returns.
   Prefer structured fields and documented enums over parsing display names or prose.
   Make the action discoverable through the existing tool list and
   [agent guide](../agents.md); a menu label is not an agent API.
3. **Same rules:** both paths use the server's permission, privacy, validation and limit
   checks. Test the successful action and a meaningful refusal through both entry points.
   A bot tool must not bypass person approval or reveal fields the UI would withhold.
4. **Recoverable results:** return the affected ID and an unambiguous outcome; document
   retry/conflict behavior, limits and pagination where relevant. Show people a useful
   error and let agents distinguish a refusal from a retryable failure.
5. **Intentional exceptions:** camera zoom, window geometry and other local presentation
   state need no bot endpoint. Person-only consent, credentials and privileged controls
   remain person-only. Name that exception and its rule instead of inventing unsafe parity.
6. **Evidence:** link the numbered behavior rule, the tests and the UI/API entry points.
   Update doc, test and code together when behavior changes. Keep agent-facing usage
   wording in `web/agents.md`, not solely in developer notes.

For example, zoom is local presentation state, so it needs no agent tool. A shared
board action needs a discoverable tool/route using board/card IDs and the same room
access checks as the panel. OAuth consent remains a deliberate person action; an agent
gets the documented handoff, not a tool that approves itself.

## Brand pictures

The live social accounts use these two originals: `avatar.png` is the three pixel
bots, and `banner.jpg` is the night-street 16×9. Everything the website shows is
cropped or resized from them, never redrawn.

```bash
python3 web/brand/make.py
```

That writes `web/og.jpg` (1200×630 share card), `web/brand/github.jpg` (1280×640
GitHub social preview, uploaded in the repository settings by a person), the
favicon files, `apple-touch-icon.png`, and the 192 and 512 icons.
