"""Read-only, interval-based Android UI frame sample. Never resets system/app stats.

Open the app and repeat the gesture being investigated during the sample. These
are Android window/compositor timings, not a JavaScript CPU trace or proof of a
particular bottleneck. No app data, screenshots, or credentials are captured.
"""

import argparse
import json
import re
import shutil
import subprocess
import time
from pathlib import Path


PACKAGE = "com.agentshangout.android"


def adb_output(adb, serial, *args):
    return subprocess.run(
        [adb, "-s", serial, "shell", *args],
        check=True, capture_output=True, text=True, timeout=20,
    ).stdout


def foreground(adb, serial):
    activities = adb_output(adb, serial, "dumpsys", "activity", "activities")
    return any(PACKAGE in line for line in activities.splitlines()
               if "topResumedActivity" in line)


def snapshot(adb, serial):
    text = adb_output(adb, serial, "dumpsys", "gfxinfo", PACKAGE)
    pid = re.search(r"Graphics info for pid (\d+)", text)
    frames = re.search(r"Total frames rendered: (\d+)", text)
    janky = re.search(r"^Janky frames: (\d+)", text, re.MULTILINE)
    histogram = re.search(r"^HISTOGRAM: (.+)$", text, re.MULTILINE)
    if not all((pid, frames, janky, histogram)):
        raise RuntimeError("No complete app frame statistics are available")
    return {
        "pid": int(pid[1]),
        "frames": int(frames[1]),
        "janky": int(janky[1]),
        "histogram": {int(ms): int(count) for ms, count in
                      re.findall(r"(\d+)ms=(\d+)", histogram[1])},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--adb", default=shutil.which("adb") or str(
        Path.home() / "AppData/Local/Android/Sdk/platform-tools/adb.exe"))
    parser.add_argument("--seconds", type=int, default=15, choices=range(1, 61), metavar="1..60")
    parser.add_argument("--out", type=Path, help="Optional JSON output outside the repository")
    args = parser.parse_args()
    if not foreground(args.adb, args.serial):
        raise RuntimeError("Open Agents Hangout first; its activity is not in the foreground")
    before = snapshot(args.adb, args.serial)
    print(f"Sampling {args.seconds}s. Exercise the same gesture throughout.", flush=True)
    started = time.monotonic()
    for _ in range(args.seconds):
        time.sleep(1)
        if not foreground(args.adb, args.serial):
            raise RuntimeError("App left the foreground; sample discarded")
    after = snapshot(args.adb, args.serial)
    if before["pid"] != after["pid"] or after["frames"] < before["frames"]:
        raise RuntimeError("App restarted or frame statistics reset; sample discarded")
    frames = after["frames"] - before["frames"]
    if not frames:
        raise RuntimeError("No new Android window frames; sample cannot measure this interaction")
    histogram = {ms: count - before["histogram"].get(ms, 0)
                 for ms, count in after["histogram"].items()}
    if any(count < 0 for count in histogram.values()):
        raise RuntimeError("Frame histogram reset; sample discarded")
    result = {
        "duration_seconds": round(time.monotonic() - started, 2),
        "frames": frames,
        "deadline_janky_frames": after["janky"] - before["janky"],
        "histogram_ms": histogram,
        "scope": "Android window frames; not a JavaScript renderer CPU trace",
    }
    result["deadline_janky_percent"] = round(100 * result["deadline_janky_frames"] / frames, 2)
    print(json.dumps(result, indent=2))
    if args.out:
        args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
