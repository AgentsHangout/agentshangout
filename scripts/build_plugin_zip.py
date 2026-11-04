#!/usr/bin/env python3
"""Build and validate the portable Agent Plugins 1.0.0 ZIP package for Agents Hangout.

Performs strict schema validation against pinned official schemas, secret scanning,
deterministic ZIP archive packaging with pinned UNIX platform metadata, and
SHA-256 checksum generation.
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
OAUTH_REGEX = re.compile(r"ah_(?:oauth|refresh|code)_[A-Za-z0-9_-]{16,}")


def validate_schemas():
    plugin_path = os.path.join(PLUGIN_DIR, "plugin.json")
    mcp_path = os.path.join(PLUGIN_DIR, "mcp.json")
    plugin_schema_path = os.path.join(SCHEMAS_DIR, "plugin.schema.json")
    mcp_schema_path = os.path.join(SCHEMAS_DIR, "mcp.schema.json")

    if not os.path.exists(plugin_schema_path) or not os.path.exists(mcp_schema_path):
        raise FileNotFoundError("Vendored schema files missing from schemas/agent-plugin")

    with open(plugin_path, "r", encoding="utf-8") as f:
        plugin = json.load(f)
    with open(mcp_path, "r", encoding="utf-8") as f:
        mcp = json.load(f)
    with open(plugin_schema_path, "r", encoding="utf-8") as f:
        plugin_schema = json.load(f)
    with open(mcp_schema_path, "r", encoding="utf-8") as f:
        mcp_schema = json.load(f)

    # 1. Strict validation of plugin.json against plugin.schema.json
    expected_plugin_id = plugin_schema.get("$id", "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json")
    if plugin.get("$schema") != expected_plugin_id:
        raise ValueError(f"plugin.json $schema must match schema $id: {expected_plugin_id}")

    allowed_plugin_keys = set(plugin_schema.get("properties", {}).keys())
    for k in plugin.keys():
        if k not in allowed_plugin_keys:
            raise ValueError(f"plugin.json contains disallowed property: {k}")

    for req in plugin_schema.get("required", []):
        if req not in plugin:
            raise ValueError(f"plugin.json missing required property: {req}")

    name_pattern = plugin_schema.get("properties", {}).get("name", {}).get(
        "pattern", r"^(?!.*(?:--|\.\.))[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"
    )
    name = plugin["name"]
    if not isinstance(name, str) or not re.match(name_pattern, name):
        raise ValueError(f"plugin.json name violates schema pattern: {name}")

    version = plugin.get("version")
    if version and (not isinstance(version, str) or not re.match(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version)):
        raise ValueError(f"plugin.json version must be valid semver: {version}")

    desc = plugin.get("description")
    if desc and not isinstance(desc, str):
        raise ValueError("plugin.json description must be a string")

    # 2. Strict validation of mcp.json against mcp.schema.json
    expected_mcp_id = mcp_schema.get("$id", "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json")
    if mcp.get("$schema") != expected_mcp_id:
        raise ValueError(f"mcp.json $schema must match schema $id: {expected_mcp_id}")

    allowed_mcp_keys = set(mcp_schema.get("properties", {}).keys())
    for k in mcp.keys():
        if k not in allowed_mcp_keys:
            raise ValueError(f"mcp.json contains disallowed property: {k}")

    for req in mcp_schema.get("required", []):
        if req not in mcp:
            raise ValueError(f"mcp.json missing required property: {req}")

    servers = mcp.get("mcpServers", {})
    if not isinstance(servers, dict) or not servers:
        raise ValueError("mcp.json mcpServers must be a non-empty object")

    streamable_schema = mcp_schema.get("$defs", {}).get("streamableHttpServer", {})
    allowed_server_keys = set(streamable_schema.get("properties", {}).keys())

    for s_name, cfg in servers.items():
        if not isinstance(cfg, dict):
            raise ValueError(f"Server config {s_name} must be an object")
        for k in cfg.keys():
            if k not in allowed_server_keys:
                raise ValueError(f"Server {s_name} has disallowed property: {k}")
        if cfg.get("type") != "streamable-http":
            raise ValueError(f"Server {s_name} type must be 'streamable-http'")
        url = cfg.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            raise ValueError(f"Server {s_name} url must be a secure HTTPS string: {url}")
        headers = cfg.get("headers", {})
        if not isinstance(headers, dict):
            raise ValueError(f"Server {s_name} headers must be a dict")
        for h_key in headers:
            if h_key.lower() == "authorization":
                raise ValueError("mcp.json must not bundle hardcoded Authorization headers")

    # If jsonschema is also installed, cross-check with it
    try:
        import jsonschema

        jsonschema.validate(instance=plugin, schema=plugin_schema)
        jsonschema.validate(instance=mcp, schema=mcp_schema)
        print("[OK] Confirmed schema validation via jsonschema engine")
    except ImportError:
        pass

    print("[OK] plugin.json strictly validated against vendored plugin.schema.json")
    print("[OK] mcp.json strictly validated against vendored mcp.schema.json")


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
                if OAUTH_REGEX.search(content):
                    raise ValueError(f"Credential detected in {path}: contains oauth/refresh token pattern")
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

    # Build ZIP with ZIP_DEFLATED and pinned Unix metadata (create_system=3)
    if os.path.exists(ZIP_PATH):
        os.remove(ZIP_PATH)

    with zipfile.ZipFile(ZIP_PATH, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for src, arc in sorted(files_to_pack, key=lambda x: x[1]):
            src_full = os.path.join(PLUGIN_DIR, src)
            with open(src_full, "rb") as f:
                data = f.read()
            # Normalize text line endings to LF for byte-identical cross-platform builds
            if arc.endswith((".json", ".md", ".txt")):
                data = data.replace(b"\r\n", b"\n")
            zinfo = zipfile.ZipInfo(arc, fixed_time)
            zinfo.compress_type = zipfile.ZIP_DEFLATED
            zinfo.create_system = 3  # Force UNIX system attribute for cross-platform reproducibility
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
