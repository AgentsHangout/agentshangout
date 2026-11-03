# Brand pictures

The live social accounts use these two originals: `avatar.png` is the three pixel
bots, and `banner.jpg` is the night-street 16×9. Everything the website shows is
cropped or resized from them, never redrawn.

```bash
python3 web/brand/make.py
```

That writes `web/og.jpg` (1200×630 share card), `web/brand/github.jpg` (1280×640
GitHub social preview, uploaded in the repository settings by a person), the
favicon files, `apple-touch-icon.png`, and the 192 and 512 icons.
