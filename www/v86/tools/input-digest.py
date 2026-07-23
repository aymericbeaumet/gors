#!/usr/bin/env python3
"""Hash exact V86 image inputs using stable repository-relative records."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import stat
import sys


def frame(hasher: "hashlib._Hash", value: bytes) -> None:
    hasher.update(len(value).to_bytes(8, "little"))
    hasher.update(value)


def files(root: Path, inputs: list[str]) -> list[Path]:
    selected: set[Path] = set()
    for name in inputs:
        path = (root / name).resolve()
        path.relative_to(root)
        if path.is_dir():
            selected.update(item for item in path.rglob("*") if item.is_file())
        elif path.is_file():
            selected.add(path)
        else:
            raise FileNotFoundError(f"V86 image input does not exist: {name}")
    return sorted(selected, key=lambda path: path.relative_to(root).as_posix())


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: input-digest.py <repository-root> <path>...", file=sys.stderr)
        return 2
    root = Path(sys.argv[1]).resolve()
    inputs = [value for value in sys.argv[2:] if not value.startswith("--fact=")]
    facts = sorted(value.removeprefix("--fact=") for value in sys.argv[2:] if value.startswith("--fact="))
    hasher = hashlib.sha256(b"gors-v86-image-input-v1\0")
    for fact in facts:
        frame(hasher, b"fact")
        frame(hasher, fact.encode())
    for path in files(root, inputs):
        frame(hasher, b"file")
        relative = path.relative_to(root).as_posix().encode()
        metadata = path.stat()
        frame(hasher, relative)
        frame(hasher, stat.S_IMODE(metadata.st_mode).to_bytes(4, "little"))
        frame(hasher, path.read_bytes())
    print(hasher.hexdigest())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
