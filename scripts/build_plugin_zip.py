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


def validate_instance(data, schema, root_schema=None):
    if root_schema is None:
        root_schema = schema

    # Resolve $ref
    if "$ref" in schema:
        ref = schema["$ref"]
        if ref.startswith("#/"):
            curr = root_schema
            for part in ref[2:].split("/"):
                curr = curr[part]
            return validate_instance(data, curr, root_schema)
        raise NotImplementedError(f"External ref unsupported: {ref}")

    # oneOf
    if "oneOf" in schema:
        matches = 0
        last_err = None
        for sub in schema["oneOf"]:
            try:
                validate_instance(data, sub, root_schema)
                matches += 1
            except (ValueError, TypeError) as e:
                last_err = e
        if matches != 1:
            raise ValueError(f"Expected exactly 1 oneOf match, got {matches}. Last error: {last_err}")
        return

    # const & enum
    if "const" in schema and data != schema["const"]:
        raise ValueError(f"Value {data!r} does not equal const {schema['const']!r}")
    if "enum" in schema and data not in schema["enum"]:
        raise ValueError(f"Value {data!r} not in enum {schema['enum']!r}")

    # type
    expected_type = schema.get("type")
    if expected_type:
        type_map = {
            "string": str,
            "object": dict,
            "array": list,
            "integer": int,
            "number": (int, float),
            "boolean": bool,
            "null": type(None),
        }
        py_type = type_map.get(expected_type)
        if py_type:
            if expected_type in ("integer", "number") and isinstance(data, bool):
                raise TypeError(f"Expected {expected_type}, got bool: {data!r}")
            if not isinstance(data, py_type):
                raise TypeError(f"Expected {expected_type}, got {type(data).__name__}: {data!r}")

    # string constraints
    if isinstance(data, str):
        if "minLength" in schema and len(data) < schema["minLength"]:
            raise ValueError(f"String too short: {len(data)} < {schema['minLength']}")
        if "maxLength" in schema and len(data) > schema["maxLength"]:
            raise ValueError(f"String too long: {len(data)} > {schema['maxLength']}")
        if "pattern" in schema and not re.search(schema["pattern"], data):
            raise ValueError(f"String {data!r} does not match pattern {schema['pattern']}")

    # array constraints
    if isinstance(data, list) and "items" in schema:
        for item in data:
            validate_instance(item, schema["items"], root_schema)

    # object constraints
    if isinstance(data, dict):
        for req in schema.get("required", []):
            if req not in data:
                raise ValueError(f"Missing required property: {req}")

        props = schema.get("properties", {})
        additional = schema.get("additionalProperties", True)

        for k, v in data.items():
            if k in props:
                validate_instance(v, props[k], root_schema)
            else:
                if additional is False:
                    raise ValueError(f"Disallowed additional property: {k}")
                elif isinstance(additional, dict):
                    validate_instance(v, additional, root_schema)


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

    # Strict recursive validation using vendored schemas (works standalone without third-party deps)
    validate_instance(plugin, plugin_schema)
    validate_instance(mcp, mcp_schema)

    # Additional operational checks: no hardcoded Authorization headers
    servers = mcp.get("mcpServers", {})
    for s_name, cfg in servers.items():
        headers = cfg.get("headers", {})
        if isinstance(headers, dict):
            for h_key in headers:
                if h_key.lower() == "authorization":
                    raise ValueError("mcp.json must not bundle hardcoded Authorization headers")

    # If jsonschema engine is also available in environment, run it as secondary verification
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

    # Text mode on Windows would write CRLF, and the file would show up as changed.
    with open(SHA256_PATH, "w", encoding="utf-8", newline="\n") as f:
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
