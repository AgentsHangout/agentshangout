# Brand kit

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
