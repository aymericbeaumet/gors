#!/usr/bin/env python3
"""Canonical integrity evidence for one V86 9p root filesystem."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import stat
from typing import Any


ROOTFS_EVIDENCE_SCHEMA = 1
ROOTFS_INDEX_SCHEMA = 3
_BLOB_KEY = re.compile(r"[0-9a-f]{64}\.bin")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb", buffering=0) as source:
        for chunk in iter(lambda: source.read(128 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json_bytes_strict(content: bytes, source: str) -> Any:
    def object_without_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON field {key!r} in {source}")
            result[key] = value
        return result

    def reject_non_finite(value: str) -> None:
        raise ValueError(f"non-finite JSON number {value!r} in {source}")

    return json.loads(
        content.decode("utf-8"),
        object_pairs_hook=object_without_duplicates,
        parse_constant=reject_non_finite,
    )


def load_json_strict(path: Path) -> Any:
    return load_json_bytes_strict(path.read_bytes(), str(path))


def inspect_rootfs(index_path: Path, blob_directory: Path) -> dict[str, Any]:
    index_bytes = index_path.read_bytes()
    index = load_json_bytes_strict(index_bytes, str(index_path))
    if not isinstance(index, dict) or set(index) != {"fsroot", "size", "version"}:
        raise ValueError("rootfs index must contain exactly fsroot, size, and version")
    if type(index["version"]) is not int or index["version"] != ROOTFS_INDEX_SCHEMA:
        raise ValueError(f"rootfs index version must be {ROOTFS_INDEX_SCHEMA}")
    if type(index["size"]) is not int or index["size"] < 0:
        raise ValueError("rootfs index size must be a non-negative integer")

    referenced: set[str] = set()
    _collect_blob_keys(index["fsroot"], referenced, "fsroot")
    _verify_blob_directory(blob_directory, referenced)

    return {
        "blobCount": len(referenced),
        "blobSetIdentity": _blob_set_identity(referenced),
        "indexSha256": hashlib.sha256(index_bytes).hexdigest(),
        "schemaVersion": ROOTFS_EVIDENCE_SCHEMA,
    }


def _collect_blob_keys(nodes: Any, referenced: set[str], location: str) -> None:
    if not isinstance(nodes, list):
        raise ValueError(f"rootfs node collection {location} must be an array")
    for index, node in enumerate(nodes):
        node_location = f"{location}[{index}]"
        if not isinstance(node, list) or len(node) < 6:
            raise ValueError(f"rootfs node {node_location} is malformed")
        mode = node[3]
        if type(mode) is not int:
            raise ValueError(f"rootfs node {node_location} has a non-integer mode")
        if stat.S_ISDIR(mode):
            if len(node) != 7:
                raise ValueError(f"rootfs directory {node_location} has no child array")
            _collect_blob_keys(node[6], referenced, node_location)
        elif stat.S_ISREG(mode):
            if len(node) != 7 or not isinstance(node[6], str):
                raise ValueError(f"rootfs file {node_location} has no blob key")
            key = node[6]
            if _BLOB_KEY.fullmatch(key) is None:
                raise ValueError(f"rootfs file {node_location} has invalid blob key {key!r}")
            referenced.add(key)
        elif stat.S_ISLNK(mode):
            if len(node) != 7 or not isinstance(node[6], str):
                raise ValueError(f"rootfs symlink {node_location} has no target")


def _verify_blob_directory(blob_directory: Path, referenced: set[str]) -> None:
    if not blob_directory.is_dir():
        raise ValueError(f"rootfs blob directory is missing: {blob_directory}")
    entries = list(blob_directory.iterdir())
    for entry in entries:
        if entry.is_symlink() or not entry.is_file():
            raise ValueError(f"rootfs blob entry must be a regular file: {entry}")
    actual = {entry.name for entry in entries}
    missing = sorted(referenced - actual)
    extra = sorted(actual - referenced)
    if missing:
        raise ValueError(f"rootfs blob directory is missing {missing[0]!r}")
    if extra:
        raise ValueError(f"rootfs blob directory has unreferenced entry {extra[0]!r}")
    for key in sorted(referenced):
        content_hash = sha256_file(blob_directory / key)
        if content_hash != key.removesuffix(".bin"):
            raise ValueError(f"rootfs blob content does not match its key: {key}")


def _blob_set_identity(keys: set[str]) -> str:
    digest = hashlib.sha256(b"gors-v86-rootfs-blob-set-v1\0")
    for key in sorted(keys):
        encoded = key.encode("ascii")
        digest.update(len(encoded).to_bytes(8, "little"))
        digest.update(encoded)
    return digest.hexdigest()
