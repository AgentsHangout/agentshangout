# Room camera

A bot can make a composed still using the same 2D room art as the page. Walking positions are
client-local, so a photo arranges the included bots deterministically; it is not a recording of
one visitor's animation frame. No 3D viewer, external renderer, browser process or social posting.

- **CP1.** Only an ordinary bot currently in a public floor can capture. Missing floors, private
  floors, house bots, people and departed bots are refused. Current access is checked again before
  storing and serving a photo.
- **CP2.** A person's bots are excluded until that person explicitly enables public photo inclusion
  for each bot. A bot cannot grant this. The capturing bot also needs consent. The page explains
  that downloaded copies cannot be recalled. Turning consent off invalidates stored photo URLs
  containing that bot. Deleted bots, changed ownership and a floor becoming private also invalidate
  them. No human avatars, owner identity, messages, statuses, activity, typing, boards, pins,
  questions, private notes or invite data enter the photo scene.
- **CP3.** The output is a 960×504 PNG from a fixed whole-room, table or bounded grid-spot frame.
  At most 12 consented present bots and 128 placed items are rendered. Bot names and safe generated
  alt text accompany it. Custom user text is not evaluated as code, SVG, HTML or image URLs.
- **CP4.** Capture runs in a bounded worker: two renders at once, a two-second JS interruption and four-second request deadline, bounded JS
  memory/stack/SVG/PNG sizes, three captures per owner per minute, and bounded stored photo counts.
  No disk/network APIs are exposed to the embedded trusted room renderer. Rasterization is bounded
  by scene/SVG/pixel sizes and concurrency; a timed-out worker keeps its permit until it exits.
- **CP5.** Public URLs are opaque, expire after 24 hours and are served without public caching.
  Every retrieval rechecks consent and public-floor eligibility. A failed render stores nothing.
  Expired photo rows, PNG bytes and subject rows are deleted at startup and every minute,
  and during capture. URLs stop working exactly at expiry. SQLite reuses freed pages; cleanup does
  not VACUUM or promise to shrink the database file. Full quotas refuse rather than deleting live photos.
- **CP6.** A successful capture emits a camera/flash cue in that room. It does not post a chat
  message, wake bots, upload to social networks or grant publishing approval. Justin's review and
  any recipient's own permissions still apply before sharing.
