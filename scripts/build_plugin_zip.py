#!/usr/bin/env python3
"""Build and validate the portable Agent Plugins 1.0.0 ZIP package for Agents Hangout.

Performs schema validation against pinned official schemas, secret scanning,
deterministic ZIP archive packaging, and SHA-256 checksum generation.
"""

import hashlib
import json
import os
import re
import sys
import zipfile

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
PLUGIN_DIR = os.path.join(ROOT_DIR, "starter", "agent-plugin")
SCHEMAS_DIR = os.path.join(ROOT_DIR, "schemas", "agent-plugin")
OUTPUT_DIR = os.path.join(ROOT_DIR, "web", "downloads")
ZIP_NAME = "agents-hangout-0.1.0.zip"
ZIP_PATH = os.path.join(OUTPUT_DIR, ZIP_NAME)
SHA256_PATH = os.path.join(OUTPUT_DIR, f"{ZIP_NAME}.sha256")

TOKEN_REGEX = re.compile(r"ah_bot_[A-Za-z0-9_-]{16,}")
OWNER_REGEX = re.compile(r"ah_owner_[A-Za-z0-9_-]{16,}")


def validate_schemas():
    plugin_path = os.path.join(PLUGIN_DIR, "plugin.json")
    mcp_path = os.path.join(PLUGIN_DIR, "mcp.json")

    with open(plugin_path, "r", encoding="utf-8") as f:
        plugin = json.load(f)
    with open(mcp_path, "r", encoding="utf-8") as f:
        mcp = json.load(f)

    # 1. plugin.json checks
    if plugin.get("$schema") != "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json":
        raise ValueError(f"Invalid plugin.json $schema: {plugin.get('$schema')}")
    name = plugin.get("name")
    if not name or not re.match(r"^[a-z0-9]+(-[a-z0-9]+)*$", name):
        raise ValueError(f"Invalid plugin name: {name}")
    version = plugin.get("version")
    if not version or not re.match(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        raise ValueError(f"Invalid semver version: {version}")
    if not plugin.get("description"):
        raise ValueError("Missing plugin description")
    print("[OK] plugin.json strictly matches Agent Plugins 1.0.0 manifest schema")

    # 2. mcp.json checks
    if mcp.get("$schema") != "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json":
        raise ValueError(f"Invalid mcp.json $schema: {mcp.get('$schema')}")
    servers = mcp.get("mcpServers", {})
    if not servers:
        raise ValueError("mcp.json must define at least one server under mcpServers")
    for s_name, cfg in servers.items():
        if cfg.get("type") != "streamable-http":
            raise ValueError(f"Server {s_name} must use 'streamable-http' transport")
        url = cfg.get("url")
        if not url or not url.startswith("https://"):
            raise ValueError(f"Server {s_name} URL must be secure HTTPS: {url}")
        # Ensure no bundled authorization headers in headers dict
        headers = cfg.get("headers", {})
        for h_key in headers:
            if h_key.lower() == "authorization":
                raise ValueError("mcp.json must not bundle hardcoded Authorization headers")
    print("[OK] mcp.json strictly matches Agent Plugins 1.0.0 MCP configuration schema")


def scan_for_secrets():
    for root, _, files in os.walk(PLUGIN_DIR):
        for file in files:
            path = os.path.join(root, file)
            with open(path, "r", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                if TOKEN_REGEX.search(content):
                    raise ValueError(f"Credential detected in {path}: contains bot token pattern")
                if OWNER_REGEX.search(content):
                    raise ValueError(f"Credential detected in {path}: contains owner token pattern")
                if "Bearer ah_bot_" in content or "Bearer ah_owner_" in content:
                    raise ValueError(f"Bearer token literal detected in {path}")
    print("[OK] Secret check passed: no bundled credentials or tokens found")


def build_zip():
    os.makedirs(OUTPUT_DIR, exist_ok=True)

    # Deterministic file timestamps (2026-01-01 00:00:00)
    fixed_time = (2026, 1, 1, 0, 0, 0)

    # Collect files relative to PLUGIN_DIR
    files_to_pack = [
        ("plugin.json", "plugin.json"),
        ("mcp.json", "mcp.json"),
        (os.path.join("skills", "agents-hangout", "SKILL.md"), "skills/agents-hangout/SKILL.md"),
    ]

    # Verify no path traversal in arcnames
    for src, arc in files_to_pack:
        if arc.startswith("/") or ".." in arc:
            raise ValueError(f"Illegal archive path: {arc}")

    # Build ZIP with ZIP_DEFLATED
    if os.path.exists(ZIP_PATH):
        os.remove(ZIP_PATH)

    with zipfile.ZipFile(ZIP_PATH, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for src, arc in sorted(files_to_pack, key=lambda x: x[1]):
            src_full = os.path.join(PLUGIN_DIR, src)
            with open(src_full, "rb") as f:
                data = f.read()
            zinfo = zipfile.ZipInfo(arc, fixed_time)
            zinfo.compress_type = zipfile.ZIP_DEFLATED
            zinfo.external_attr = 0o644 << 16
            zf.writestr(zinfo, data)

    # Calculate SHA-256
    with open(ZIP_PATH, "rb") as f:
        sha256 = hashlib.sha256(f.read()).hexdigest()

    with open(SHA256_PATH, "w", encoding="utf-8") as f:
        f.write(f"{sha256}  {ZIP_NAME}\n")

    print(f"[OK] Package created: {ZIP_PATH}")
    print(f"[OK] SHA-256 checksum: {sha256}")
    return sha256


def main():
    try:
        validate_schemas()
        scan_for_secrets()
        sha = build_zip()
        print("Agent Plugins package build completed successfully.")
    except Exception as e:
        print(f"Error building package: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
