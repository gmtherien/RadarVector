#!/usr/bin/env python3
"""Build latest.json (cargo-packager updater format) from signed release assets.

Usage: make_latest_json.py VERSION BASE_URL ASSET_DIR > latest.json
Each installer must have a sibling `<file>.sig` produced by `cargo packager signer sign`.
"""
import json
import pathlib
import sys
from datetime import datetime, timezone

version, base_url, asset_dir = sys.argv[1], sys.argv[2].rstrip("/"), pathlib.Path(sys.argv[3])

# platform key -> filename suffix pattern
RULES = [
    ("darwin-aarch64", "_universal.dmg"),
    ("darwin-x86_64", "_universal.dmg"),
    ("windows-x86_64", "_x64-setup.exe"),
    ("windows-aarch64", "_arm64-setup.exe"),
    ("linux-x86_64", "_x86_64.AppImage"),
]

platforms = {}
for key, suffix in RULES:
    for f in sorted(asset_dir.iterdir()):
        sig = f.with_name(f.name + ".sig")
        if f.name.endswith(suffix) and sig.exists():
            platforms[key] = {"url": f"{base_url}/{f.name}", "signature": sig.read_text().strip()}
            break

json.dump(
    {
        "version": version.lstrip("v"),
        "notes": f"RadarVector {version.lstrip('v')}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    },
    sys.stdout,
    indent=2,
)
