#!/usr/bin/env python3
"""Publish the V86 rootfs manifest with its exact runtime provider facts."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import sys

from rootfs_evidence import inspect_rootfs, load_json_bytes_strict


MANIFEST_SCHEMA = 1
_SHA256 = re.compile(r"[0-9a-f]{64}")


def main() -> int:
    if len(sys.argv) != 6:
        print(
            "usage: write-manifest.py <input-digest> <provider.json> "
            "<rootfs.json> <rootfs-flat> <manifest.json>",
            file=sys.stderr,
        )
        return 2
    input_digest, provider_name, index_name, blobs_name, output_name = sys.argv[1:]
    if _SHA256.fullmatch(input_digest) is None:
        raise ValueError("input digest must be lowercase SHA-256")
    provider_path = Path(provider_name)
    provider_bytes = provider_path.read_bytes()
    provider = load_json_bytes_strict(provider_bytes, str(provider_path))
    if not isinstance(provider, dict):
        raise ValueError("runtime provider manifest must be a JSON object")
    manifest = {
        "inputDigest": input_digest,
        "rootfs": inspect_rootfs(Path(index_name), Path(blobs_name)),
        "runtimeProvider": provider,
        "runtimeProviderSha256": hashlib.sha256(provider_bytes).hexdigest(),
        "schemaVersion": MANIFEST_SCHEMA,
        "type": "9p",
    }
    Path(output_name).write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
