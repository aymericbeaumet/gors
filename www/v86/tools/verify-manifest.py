#!/usr/bin/env python3
"""Reject stale or corrupt V86 rootfs cache publications."""

from __future__ import annotations

import hashlib
from pathlib import Path
import re
import sys

from rootfs_evidence import inspect_rootfs, load_json_bytes_strict, load_json_strict


MANIFEST_SCHEMA = 1
_SHA256 = re.compile(r"[0-9a-f]{64}")
_FIELDS = {
    "inputDigest",
    "rootfs",
    "runtimeProvider",
    "runtimeProviderSha256",
    "schemaVersion",
    "type",
}


def main() -> int:
    if len(sys.argv) != 7:
        print(
            "usage: verify-manifest.py <expected-input-digest> <manifest.json> "
            "<provider.json> <rootfs.json> <rootfs-flat> <quiet|verbose>",
            file=sys.stderr,
        )
        return 2
    expected_digest, manifest_name, provider_name, index_name, blobs_name, mode = (
        sys.argv[1:]
    )
    quiet = mode == "quiet"
    if mode not in {"quiet", "verbose"}:
        print("verification mode must be quiet or verbose", file=sys.stderr)
        return 2
    try:
        _verify(
            expected_digest,
            Path(manifest_name),
            Path(provider_name),
            Path(index_name),
            Path(blobs_name),
        )
    except (OSError, TypeError, ValueError) as error:
        if not quiet:
            print(f"V86 cache manifest rejected: {error}", file=sys.stderr)
        return 1
    return 0


def _verify(
    expected_digest: str,
    manifest_path: Path,
    provider_path: Path,
    index_path: Path,
    blob_directory: Path,
) -> None:
    if _SHA256.fullmatch(expected_digest) is None:
        raise ValueError("expected input digest must be lowercase SHA-256")
    for path in (manifest_path, provider_path, index_path):
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"publication input must be a regular file: {path}")
    manifest = load_json_strict(manifest_path)
    if not isinstance(manifest, dict) or set(manifest) != _FIELDS:
        raise ValueError("publication manifest has an unsupported field set")
    if type(manifest["schemaVersion"]) is not int or manifest["schemaVersion"] != MANIFEST_SCHEMA:
        raise ValueError(f"publication manifest schema must be {MANIFEST_SCHEMA}")
    if manifest["type"] != "9p":
        raise ValueError("publication manifest type must be 9p")
    if manifest["inputDigest"] != expected_digest:
        raise ValueError("publication input digest is stale")
    _validate_rootfs_record(manifest["rootfs"])

    provider_hash = manifest["runtimeProviderSha256"]
    if not isinstance(provider_hash, str) or _SHA256.fullmatch(provider_hash) is None:
        raise ValueError("runtime provider hash must be lowercase SHA-256")
    provider_bytes = provider_path.read_bytes()
    if hashlib.sha256(provider_bytes).hexdigest() != provider_hash:
        raise ValueError("runtime provider sidecar hash does not match the manifest")
    provider = load_json_bytes_strict(provider_bytes, str(provider_path))
    if not isinstance(provider, dict) or manifest["runtimeProvider"] != provider:
        raise ValueError("runtime provider sidecar does not match the manifest record")

    evidence = inspect_rootfs(index_path, blob_directory)
    if manifest["rootfs"] != evidence:
        raise ValueError("rootfs integrity evidence does not match the publication")


def _validate_rootfs_record(record: object) -> None:
    fields = {"blobCount", "blobSetIdentity", "indexSha256", "schemaVersion"}
    if not isinstance(record, dict) or set(record) != fields:
        raise ValueError("rootfs integrity evidence has an unsupported field set")
    if type(record["schemaVersion"]) is not int or record["schemaVersion"] != 1:
        raise ValueError("rootfs integrity evidence schema must be 1")
    if type(record["blobCount"]) is not int or record["blobCount"] <= 0:
        raise ValueError("rootfs blob count must be a positive integer")
    for field in ("blobSetIdentity", "indexSha256"):
        value = record[field]
        if not isinstance(value, str) or _SHA256.fullmatch(value) is None:
            raise ValueError(f"rootfs {field} must be lowercase SHA-256")


if __name__ == "__main__":
    raise SystemExit(main())
