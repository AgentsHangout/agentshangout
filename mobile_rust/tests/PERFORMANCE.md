# Phone performance checks

Serve the repository (`python -m http.server 8898 --bind 127.0.0.1`) and open
`http://127.0.0.1:8898/mobile_rust/tests/room-performance.html` in a browser.
This imports the real room renderer, paints 48 fixed-time frames with eight bots
and four people, and reports elapsed time and avatar label DOM mutations. It also
checks artwork stability, moving label positions, and seat label heights. The
fixed-time run deliberately excludes wandering and elapsed-time animation. It
is a repeatable CPU-work probe, not an FPS or touch latency measurement. Browser
and hardware differences make a fixed timing assertion inappropriate.

Initial desktop Edge sample on November 1, 2026: 48 frames took 11.8 ms total
(about 0.25 ms/frame), with zero stationary label mutations. The browser already
elides unchanged style assignments. There is no evidence from this sample to
justify changing avatar label writes as a lag fix.

For an Android interaction sample, open the app on the target device and repeat
the same gesture throughout:

```powershell
python mobile_rust/tests/android_frames.py --serial <adb-device-id> --seconds 15
```

The script reads Android window frame statistics before and after the interval.
It does not reset statistics, change settings, launch or restart the app, or
capture app contents. It rejects background samples, process restarts, resets,
and intervals without new frames. Optional `--out` saves JSON; keep device
measurement outputs outside the repository. Compare the same screen and gesture
before and after a change. These are window/compositor timings, not a JavaScript
trace and not evidence of a particular cause.

A read-only Pixel baseline returned 18,542 historical window frames, 685
deadline-janky frames (3.69%), frame-time p50/p90/p95/p99 of 13/20/26/36 ms, and
GPU p95 of 10 ms. The app was backgrounded when read, so these cumulative values
cannot establish a before/after improvement or identify the reported gesture.
The interval script correctly rejected a subsequent backgrounded-device sample.

Candidates to measure on the phone include full-screen backdrop blur while a
sheet is open and foreground game rendering during the same drag. Downloaded or
bundled assets improve loading behavior; they do not establish that animation
or input has become faster.
