"""Build public client assets for a human-approved GitHub release. Never publishes a release."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import zipfile


def bundle_version(files: dict) -> str:
    body = "".join(f"{name}\0{entry['sha256']}\0{entry['size']}\n" for name, entry in sorted(files.items()))
    return hashlib.sha256(body.encode()).hexdigest()


def package(web: Path, output: Path, release: str) -> dict:
    if not re.fullmatch(r"mobile-assets-[A-Za-z0-9._-]{1,80}", release):
        raise ValueError("Release tag must begin mobile-assets- and contain only letters, digits, dots, underscores or hyphens")
    sources = sorted(path for path in web.iterdir() if path.is_file() and (
        path.suffix in {".js", ".css", ".svg"} or path.name in {"index.html", "og.jpg"}))
    names = {path.name for path in sources}
    if not {"index.html", "app.js", "style.css"} <= names:
        raise ValueError("The game entry point, script and stylesheet must be present")
    for source in sources:
        if source.suffix == ".js":
            imports = re.findall(r"\bfrom\s*['\"]\./([^'\"]+)['\"]", source.read_text(encoding="utf-8"))
            if any(name not in names for name in imports):
                raise ValueError(f"{source.name} imports an asset outside this client bundle")
    files = {path.name: {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size}
             for path in sources}
    if len(files) > 256 or any(not 1 <= entry["size"] <= 8 * 1024 * 1024 for entry in files.values()) or sum(
            entry["size"] for entry in files.values()) > 32 * 1024 * 1024:
        raise ValueError("Client assets exceed the native client's safety limits")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / "android-client.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for source in sources:
            entry = zipfile.ZipInfo(source.name, date_time=(2020, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, source.read_bytes())
    if archive.stat().st_size > 16 * 1024 * 1024:
        raise ValueError("Compressed client archive exceeds the native client's safety limit")
    manifest = {"format": 1, "version": bundle_version(files), "api_contract": 1, "min_native": 1,
                "release": release, "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(), "files": files}
    (output / "android-client.json").write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", required=True, help="Approved immutable tag, e.g. mobile-assets-0.1.2")
    parser.add_argument("--out", required=True, type=Path, help="Directory to receive the two public release assets")
    parser.add_argument("--web", type=Path, default=Path(__file__).resolve().parents[2] / "web")
    args = parser.parse_args()
    result = package(args.web, args.out, args.release)
    print(f"Prepared {len(result['files'])} assets, client {result['version']}")
    print(f"Files: {args.out.resolve() / 'android-client.json'} and {args.out.resolve() / 'android-client.zip'}")
    print("No tag or release was created. A person must review and publish these assets.")
