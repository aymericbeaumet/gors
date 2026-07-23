from __future__ import annotations

import hashlib
import json
import os
import re
import stat
from pathlib import Path
from typing import Any


LINK_OUTPUT_SCHEMA_VERSION = 1
LINK_DESCRIPTOR_SCHEMA_VERSION = 2
DEPENDENCY_SCHEMA_VERSION = 1
ARTIFACT_SCHEMA_VERSION = 2
LINK_PLAN_SCHEMA_VERSION = 1
RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION = 2
RUST_TARGET_LIBDIR_SCHEMA_VERSION = 1
RUNTIME_LINK_VALIDATION = "strict-runtime-link-v2"
RUNTIME_CRATE_NAME = "__gors_runtime"
RUNTIME_ARTIFACT_FORMAT = "rust-rlib-v1"
RUNTIME_ARTIFACT_FILENAME = "lib__gors_runtime.rlib"
RUNTIME_OPERATION_IDS = frozenset((1, 2, 3, 8, 9, 10, 11, 13, 14, 15, 16, 17))

_RUST_RLIB_COMPATIBILITY_DOMAIN = b"gors.runtime-abi.rust-rlib-compatibility\0"
_RUST_TARGET_LIBDIR_DOMAIN = b"gors.runtime-abi.rust-target-libdir\0"
_ARTIFACT_DOMAIN = b"gors.runtime-abi.artifact\0"
_LINK_PLAN_DOMAIN = b"gors.runtime-abi.link-plan\0"
_RUNTIME_EDITION = "2024"
_RUNTIME_METADATA = "gors-runtime-v1"
_RUNTIME_OPT_LEVEL = "3"
_RUNTIME_CODEGEN_UNITS = "1"
_RUNTIME_PANIC_STRATEGY = "unwind"
_RUNTIME_EMBED_BITCODE = "yes"
_RUNTIME_REMAP_ROOT = "/gors-workspace"
_STANDARD_IO_CAPABILITY_TAG = 4
_HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")

_OUTPUT_FIELDS = frozenset(
    (
        "schema_version",
        "extern_crate",
        "artifact_path",
        "link_descriptor_schema_version",
        "dependency",
        "target_triple",
        "target_pointer_width",
        "target_endianness",
        "format",
        "producer_identity",
        "compatibility_identity",
        "implementation_hash",
        "artifact_identity",
        "link_plan_identity",
    )
)
_DEPENDENCY_FIELDS = frozenset(("schema_version", "contract", "operation_ids"))
RUNTIME_LINK_EVIDENCE_FIELDS = frozenset(
    (
        "validation",
        "descriptorPath",
        "descriptorSha256",
        "outputSchemaVersion",
        "linkDescriptorSchemaVersion",
        "dependencySchemaVersion",
        "externCrate",
        "artifactPath",
        "contractIdentity",
        "operationIds",
        "targetTriple",
        "targetPointerWidth",
        "targetEndianness",
        "format",
        "runtimeCompatibilitySchemaVersion",
        "rustcReleaseRecordSha256",
        "targetLibdirSchemaVersion",
        "targetLibdirRecordSha256",
        "producerIdentity",
        "compatibilityIdentity",
        "implementationHash",
        "artifactSha256",
        "artifactIdentity",
        "linkPlanIdentity",
    )
)


class RuntimeLinkEvidenceError(RuntimeError):
    """Raised when terminal runtime-link evidence is absent or incompatible."""


def gors_runtime_contract_identity(version_output: str) -> str:
    match = re.search(r"\bruntime-contract=([0-9a-f]{64})\b", version_output)
    if match is None:
        raise RuntimeLinkEvidenceError(
            "gors version does not report a runtime contract identity"
        )
    return match.group(1)


def _varint(value: int) -> bytes:
    if value < 0:
        raise ValueError("canonical lengths cannot be negative")
    encoded = bytearray()
    while True:
        low = value & 0x7F
        value >>= 7
        if value == 0:
            encoded.append(low)
            return bytes(encoded)
        encoded.append(low | 0x80)


def _bytes(value: bytes) -> bytes:
    return _varint(len(value)) + value


def _text(value: str) -> bytes:
    return _bytes(value.encode("utf-8"))


def _u16(value: int) -> bytes:
    return value.to_bytes(2, "big")


def _u32(value: int) -> bytes:
    return value.to_bytes(4, "big")


def _u64(value: int) -> bytes:
    return value.to_bytes(8, "big")


def _digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _digest_bytes(value: str, field: str) -> bytes:
    if not isinstance(value, str) or _HEX_SHA256.fullmatch(value) is None:
        raise RuntimeLinkEvidenceError(f"runtime link {field} must be lowercase SHA-256")
    return bytes.fromhex(value)


def _target_bytes(triple: str, pointer_width: int, endianness: str) -> bytes:
    if (
        not isinstance(triple, str)
        or not triple
        or not all(0x21 <= byte <= 0x7E for byte in triple.encode("utf-8"))
    ):
        raise RuntimeLinkEvidenceError(
            "runtime link target triple must contain only printable ASCII characters"
        )
    width_tag = {32: 1, 64: 2}.get(pointer_width)
    if width_tag is None:
        raise RuntimeLinkEvidenceError(
            f"runtime link target pointer width is unsupported: {pointer_width!r}"
        )
    endian_tag = {"little": 1, "big": 2}.get(endianness)
    if endian_tag is None:
        raise RuntimeLinkEvidenceError(
            f"runtime link target endianness is unsupported: {endianness!r}"
        )
    return _text(triple) + bytes((width_tag, endian_tag))


def canonical_rustc_release_record(rustc_verbose_version: bytes) -> bytes:
    """Remove exactly one nonempty `host: ` line like the ABI implementation."""
    if not rustc_verbose_version:
        raise RuntimeLinkEvidenceError("rustc -vV returned an empty compatibility record")
    try:
        source = rustc_verbose_version.decode("utf-8")
    except UnicodeDecodeError as error:
        raise RuntimeLinkEvidenceError("rustc -vV compatibility record is not UTF-8") from error

    lines = source.split("\n")
    trailing_newline = source.endswith("\n")
    if trailing_newline:
        lines.pop()
    host_count = 0
    canonical = bytearray()
    for index, raw_line in enumerate(lines):
        followed_by_newline = index + 1 < len(lines) or trailing_newline
        line = (
            raw_line.removesuffix("\r")
            if followed_by_newline
            else raw_line
        )
        if line.startswith("host: "):
            if not line.removeprefix("host: "):
                raise RuntimeLinkEvidenceError("rustc -vV has an empty host record")
            host_count += 1
            continue
        canonical.extend(line.encode("utf-8"))
        canonical.append(ord("\n"))
    if host_count != 1:
        raise RuntimeLinkEvidenceError(
            f"rustc -vV must contain exactly one host record, found {host_count}"
        )
    return bytes(canonical)


def _canonical_relative_path(root: Path, path: Path) -> str:
    try:
        relative = path.relative_to(root)
    except ValueError as error:
        raise RuntimeLinkEvidenceError(
            f"target rustlib inventory path escapes its root: {path}"
        ) from error
    parts = relative.parts
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise RuntimeLinkEvidenceError(
            f"target rustlib inventory path is not canonical: {relative}"
        )
    try:
        for part in parts:
            part.encode("utf-8")
    except UnicodeEncodeError as error:
        raise RuntimeLinkEvidenceError(
            f"target rustlib inventory path is not valid UTF-8: {relative!r}"
        ) from error
    return "/".join(parts)


def _canonical_symlink_target(root: Path, path: Path, target: str) -> str:
    target_path = Path(target)
    if target_path.is_absolute() or target_path.drive or target_path.root:
        raise RuntimeLinkEvidenceError(
            f"target rustlib symlink {path} has host-specific absolute target {target}"
        )
    link_relative = _canonical_relative_path(root, path)
    depth = max(len(link_relative.split("/")) - 1, 0)
    parts = []
    for part in target_path.parts:
        if part == ".":
            continue
        if part == "..":
            if depth == 0:
                raise RuntimeLinkEvidenceError(
                    f"target rustlib symlink {path} escapes its inventory root "
                    f"through {target}"
                )
            depth -= 1
        else:
            depth += 1
        parts.append(part)
    if any(part == "" for part in parts):
        raise RuntimeLinkEvidenceError(
            f"target rustlib symlink {path} has a noncanonical target {target!r}"
        )
    try:
        for part in parts:
            part.encode("utf-8")
    except UnicodeEncodeError as error:
        raise RuntimeLinkEvidenceError(
            f"target rustlib symlink target is not valid UTF-8: {target!r}"
        ) from error
    return "/".join(parts)


def _has_hash_suffixed_filename(relative: str) -> bool:
    filename = relative.rsplit("/", 1)[-1]
    stem = filename.rsplit(".", 1)[0] if "." in filename else filename
    _, separator, suffix = stem.rpartition("-")
    return bool(
        separator
        and len(suffix) == 16
        and all(character in "0123456789abcdef" for character in suffix)
    )


def canonical_target_libdir_record(root: Path) -> bytes:
    """Build the ABI's recursive, path-independent target rustlib inventory."""
    entries: list[tuple[str, int, int, str, bytes]] = []

    def collect(directory: Path) -> None:
        try:
            children = list(os.scandir(directory))
        except OSError as error:
            raise RuntimeLinkEvidenceError(
                f"cannot read target rustlib directory {directory}: {error}"
            ) from error
        for child in children:
            path = Path(child.path)
            relative = _canonical_relative_path(root, path)
            try:
                metadata = child.stat(follow_symlinks=False)
            except OSError as error:
                raise RuntimeLinkEvidenceError(
                    f"cannot inspect target rustlib entry {path}: {error}"
                ) from error
            mode = metadata.st_mode
            if stat.S_ISDIR(mode):
                entries.append((relative, ord("d"), 0, "", b""))
                collect(path)
            elif stat.S_ISREG(mode):
                content_hash = b""
                if not _has_hash_suffixed_filename(relative):
                    try:
                        content_hash = hashlib.sha256(path.read_bytes()).digest()
                    except OSError as error:
                        raise RuntimeLinkEvidenceError(
                            f"cannot hash target rustlib entry {path}: {error}"
                        ) from error
                entries.append((relative, ord("f"), metadata.st_size, "", content_hash))
            elif stat.S_ISLNK(mode):
                try:
                    target = os.readlink(path)
                except OSError as error:
                    raise RuntimeLinkEvidenceError(
                        f"cannot read target rustlib symlink {path}: {error}"
                    ) from error
                entries.append(
                    (
                        relative,
                        ord("l"),
                        0,
                        _canonical_symlink_target(root, path, target),
                        b"",
                    )
                )
            else:
                raise RuntimeLinkEvidenceError(
                    f"target rustlib inventory contains an unsupported file type: {path}"
                )

    collect(root)
    entries.sort(key=lambda entry: entry[0])
    record = bytearray(_RUST_TARGET_LIBDIR_DOMAIN)
    record.extend(_u32(RUST_TARGET_LIBDIR_SCHEMA_VERSION))
    record.extend(_varint(len(entries)))
    for relative, kind, size, symlink_target, content_hash in entries:
        record.extend(_text(relative))
        record.append(kind)
        record.extend(_u64(size))
        record.extend(_text(symlink_target))
        record.extend(_bytes(content_hash))
    return bytes(record)


def rust_runtime_compatibility_identity(
    rustc_verbose_version: bytes,
    target_libdir_record: bytes,
    *,
    target_triple: str,
    target_pointer_width: int,
    target_endianness: str,
) -> str:
    """Reproduce the ABI-owned host-neutral Rust rlib compatibility identity."""
    release_record = canonical_rustc_release_record(rustc_verbose_version)
    if not target_libdir_record:
        raise RuntimeLinkEvidenceError("target rustlib inventory must not be empty")
    record = bytearray(_RUST_RLIB_COMPATIBILITY_DOMAIN)
    record.extend(_u32(RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION))
    record.extend(_bytes(release_record))
    record.extend(_bytes(target_libdir_record))
    record.extend(
        _target_bytes(target_triple, target_pointer_width, target_endianness)
    )
    for value in (
        RUNTIME_CRATE_NAME,
        "rlib",
        _RUNTIME_EDITION,
        _RUNTIME_METADATA,
        _RUNTIME_OPT_LEVEL,
        _RUNTIME_CODEGEN_UNITS,
        _RUNTIME_PANIC_STRATEGY,
        _RUNTIME_EMBED_BITCODE,
        _RUNTIME_REMAP_ROOT,
    ):
        record.extend(_text(value))
    return _digest(bytes(record))


def runtime_artifact_identity(
    *,
    contract_identity: str,
    target_triple: str,
    target_pointer_width: int,
    target_endianness: str,
    compatibility_identity: str,
    implementation_hash: str,
) -> str:
    """Reproduce the one schema-2 StandardIo provider identity."""
    record = bytearray(_ARTIFACT_DOMAIN)
    record.extend(_u32(ARTIFACT_SCHEMA_VERSION))
    record.extend(_digest_bytes(contract_identity, "contract identity"))
    record.extend(
        _target_bytes(target_triple, target_pointer_width, target_endianness)
    )
    record.extend(_varint(1))
    record.extend(_u16(_STANDARD_IO_CAPABILITY_TAG))
    record.append(1)  # RuntimeArtifactFormat::RustRlibV1
    record.extend(_digest_bytes(compatibility_identity, "compatibility identity"))
    record.extend(_digest_bytes(implementation_hash, "implementation hash"))
    return _digest(bytes(record))


def runtime_link_plan_identity(
    *,
    contract_identity: str,
    operation_ids: list[int],
    target_triple: str,
    target_pointer_width: int,
    target_endianness: str,
    compatibility_identity: str,
    implementation_hash: str,
    artifact_identity: str,
) -> str:
    """Reproduce the canonical dependency-to-provider link-plan identity."""
    record = bytearray(_LINK_PLAN_DOMAIN)
    record.extend(_u32(LINK_PLAN_SCHEMA_VERSION))
    record.extend(_digest_bytes(contract_identity, "contract identity"))
    record.extend(_varint(len(operation_ids)))
    for operation_id in operation_ids:
        record.extend(_u16(operation_id))
    record.extend(
        _target_bytes(target_triple, target_pointer_width, target_endianness)
    )
    record.append(1)  # RuntimeArtifactFormat::RustRlibV1
    record.extend(_digest_bytes(compatibility_identity, "compatibility identity"))
    record.extend(_digest_bytes(artifact_identity, "artifact identity"))
    record.extend(_digest_bytes(implementation_hash, "implementation hash"))
    return _digest(bytes(record))


def _reject_duplicate_fields(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise RuntimeLinkEvidenceError(
                f"runtime link descriptor contains duplicate field {key!r}"
            )
        value[key] = item
    return value


def _read_descriptor(path: Path) -> tuple[dict[str, Any], bytes]:
    try:
        content = path.read_bytes()
    except OSError as error:
        raise RuntimeLinkEvidenceError(
            f"cannot read runtime link descriptor {path}: {error}"
        ) from error
    try:
        value = json.loads(content, object_pairs_hook=_reject_duplicate_fields)
    except (UnicodeDecodeError, json.JSONDecodeError, RuntimeLinkEvidenceError) as error:
        raise RuntimeLinkEvidenceError(
            f"runtime link descriptor {path} is malformed: {error}"
        ) from error
    if not isinstance(value, dict):
        raise RuntimeLinkEvidenceError("runtime link descriptor must be a JSON object")
    return value, content


def _require_exact_fields(value: dict[str, Any], expected: frozenset[str], label: str) -> None:
    actual = frozenset(value)
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        raise RuntimeLinkEvidenceError(
            f"runtime link {label} fields are incompatible: missing={missing}, extra={extra}"
        )


def _require_schema(value: Any, expected: int, label: str) -> None:
    if type(value) is not int or value != expected:
        raise RuntimeLinkEvidenceError(
            f"runtime link {label} schema is incompatible: expected {expected}, got {value!r}"
        )


def _file_sha256(path: Path) -> str:
    hasher = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                hasher.update(block)
    except OSError as error:
        raise RuntimeLinkEvidenceError(
            f"cannot hash runtime artifact {path}: {error}"
        ) from error
    return hasher.hexdigest()


def validate_runtime_link_descriptor(
    descriptor_path: Path,
    *,
    expected_contract_identity: str,
    expected_target_triple: str,
    expected_target_pointer_width: int,
    expected_target_endianness: str,
    expected_compatibility_identity: str,
    expected_rustc_release_record_sha256: str,
    expected_target_libdir_record_sha256: str,
    expected_runtime_cache_root: Path,
) -> dict[str, Any]:
    """Read and fully validate the CLI's terminal link descriptor."""
    descriptor, content = _read_descriptor(descriptor_path)
    _require_exact_fields(descriptor, _OUTPUT_FIELDS, "output")
    _require_schema(descriptor["schema_version"], LINK_OUTPUT_SCHEMA_VERSION, "output")
    _require_schema(
        descriptor["link_descriptor_schema_version"],
        LINK_DESCRIPTOR_SCHEMA_VERSION,
        "descriptor",
    )
    if descriptor["extern_crate"] != RUNTIME_CRATE_NAME:
        raise RuntimeLinkEvidenceError(
            f"runtime link extern crate is incompatible: {descriptor['extern_crate']!r}"
        )

    dependency = descriptor["dependency"]
    if not isinstance(dependency, dict):
        raise RuntimeLinkEvidenceError("runtime link dependency must be an object")
    _require_exact_fields(dependency, _DEPENDENCY_FIELDS, "dependency")
    _require_schema(
        dependency["schema_version"], DEPENDENCY_SCHEMA_VERSION, "dependency"
    )
    contract_identity = dependency["contract"]
    _digest_bytes(contract_identity, "contract identity")
    if contract_identity != expected_contract_identity:
        raise RuntimeLinkEvidenceError(
            "runtime link contract identity is stale: "
            f"expected {expected_contract_identity}, got {contract_identity}"
        )
    operation_ids = dependency["operation_ids"]
    if not isinstance(operation_ids, list) or any(
        type(operation_id) is not int for operation_id in operation_ids
    ):
        raise RuntimeLinkEvidenceError("runtime link operation IDs must be an integer array")
    if operation_ids != sorted(set(operation_ids)):
        raise RuntimeLinkEvidenceError(
            "runtime link operation IDs must be sorted and duplicate-free"
        )
    unknown_ids = sorted(set(operation_ids) - RUNTIME_OPERATION_IDS)
    if unknown_ids:
        raise RuntimeLinkEvidenceError(
            f"runtime link descriptor contains unknown operation IDs: {unknown_ids}"
        )

    target_triple = descriptor["target_triple"]
    target_pointer_width = descriptor["target_pointer_width"]
    target_endianness = descriptor["target_endianness"]
    if not isinstance(target_triple, str) or not target_triple:
        raise RuntimeLinkEvidenceError("runtime link target triple is malformed")
    if type(target_pointer_width) is not int:
        raise RuntimeLinkEvidenceError("runtime link target pointer width is malformed")
    _target_bytes(target_triple, target_pointer_width, target_endianness)
    expected_target = (
        expected_target_triple,
        expected_target_pointer_width,
        expected_target_endianness,
    )
    actual_target = (target_triple, target_pointer_width, target_endianness)
    if actual_target != expected_target:
        raise RuntimeLinkEvidenceError(
            f"runtime link target is stale: expected {expected_target!r}, got {actual_target!r}"
        )
    if descriptor["format"] != RUNTIME_ARTIFACT_FORMAT:
        raise RuntimeLinkEvidenceError(
            f"runtime link artifact format is incompatible: {descriptor['format']!r}"
        )

    compatibility_identity = descriptor["compatibility_identity"]
    producer_identity = descriptor["producer_identity"]
    _digest_bytes(producer_identity, "producer identity")
    _digest_bytes(compatibility_identity, "compatibility identity")
    if compatibility_identity != expected_compatibility_identity:
        raise RuntimeLinkEvidenceError(
            "runtime link compatibility identity is stale: "
            f"expected {expected_compatibility_identity}, got {compatibility_identity}"
        )
    _digest_bytes(expected_rustc_release_record_sha256, "rustc release-record digest")
    _digest_bytes(expected_target_libdir_record_sha256, "target-libdir record digest")
    implementation_hash = descriptor["implementation_hash"]
    artifact_identity = descriptor["artifact_identity"]
    link_plan_identity = descriptor["link_plan_identity"]
    _digest_bytes(implementation_hash, "implementation hash")
    _digest_bytes(artifact_identity, "artifact identity")
    _digest_bytes(link_plan_identity, "link-plan identity")

    artifact_path_value = descriptor["artifact_path"]
    if not isinstance(artifact_path_value, str) or not artifact_path_value:
        raise RuntimeLinkEvidenceError("runtime link artifact path is malformed")
    artifact_path = Path(artifact_path_value)
    if not artifact_path.is_absolute():
        raise RuntimeLinkEvidenceError("runtime link artifact path must be absolute")
    expected_artifact_path = (
        expected_runtime_cache_root / artifact_identity / RUNTIME_ARTIFACT_FILENAME
    )
    if artifact_path != expected_artifact_path:
        raise RuntimeLinkEvidenceError(
            "runtime link artifact path is stale: "
            f"expected {expected_artifact_path}, got {artifact_path}"
        )
    if artifact_path.is_symlink() or not artifact_path.is_file():
        raise RuntimeLinkEvidenceError(
            f"runtime link artifact is not a regular provider file: {artifact_path}"
        )
    artifact_payload_hash = _file_sha256(artifact_path)
    if artifact_payload_hash != implementation_hash:
        raise RuntimeLinkEvidenceError(
            "runtime link artifact payload is stale: "
            f"expected {implementation_hash}, got {artifact_payload_hash}"
        )

    computed_artifact_identity = runtime_artifact_identity(
        contract_identity=contract_identity,
        target_triple=target_triple,
        target_pointer_width=target_pointer_width,
        target_endianness=target_endianness,
        compatibility_identity=compatibility_identity,
        implementation_hash=implementation_hash,
    )
    if artifact_identity != computed_artifact_identity:
        raise RuntimeLinkEvidenceError(
            "runtime link artifact identity is stale: "
            f"expected {computed_artifact_identity}, got {artifact_identity}"
        )
    computed_link_plan_identity = runtime_link_plan_identity(
        contract_identity=contract_identity,
        operation_ids=operation_ids,
        target_triple=target_triple,
        target_pointer_width=target_pointer_width,
        target_endianness=target_endianness,
        compatibility_identity=compatibility_identity,
        implementation_hash=implementation_hash,
        artifact_identity=artifact_identity,
    )
    if link_plan_identity != computed_link_plan_identity:
        raise RuntimeLinkEvidenceError(
            "runtime link-plan identity is stale: "
            f"expected {computed_link_plan_identity}, got {link_plan_identity}"
        )

    return {
        "validation": RUNTIME_LINK_VALIDATION,
        "descriptorPath": str(descriptor_path),
        "descriptorSha256": _digest(content),
        "outputSchemaVersion": descriptor["schema_version"],
        "linkDescriptorSchemaVersion": descriptor["link_descriptor_schema_version"],
        "dependencySchemaVersion": dependency["schema_version"],
        "externCrate": descriptor["extern_crate"],
        "artifactPath": str(artifact_path),
        "contractIdentity": contract_identity,
        "operationIds": operation_ids,
        "targetTriple": target_triple,
        "targetPointerWidth": target_pointer_width,
        "targetEndianness": target_endianness,
        "format": descriptor["format"],
        "runtimeCompatibilitySchemaVersion": RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION,
        "rustcReleaseRecordSha256": expected_rustc_release_record_sha256,
        "targetLibdirSchemaVersion": RUST_TARGET_LIBDIR_SCHEMA_VERSION,
        "targetLibdirRecordSha256": expected_target_libdir_record_sha256,
        "producerIdentity": producer_identity,
        "compatibilityIdentity": compatibility_identity,
        "implementationHash": implementation_hash,
        "artifactSha256": artifact_payload_hash,
        "artifactIdentity": artifact_identity,
        "linkPlanIdentity": link_plan_identity,
    }


def expand_rustc_runtime_link(argv: list[str], evidence: dict[str, Any]) -> list[str]:
    """Append exactly one validated runtime `--extern` argument pair."""
    if any(argument == "--extern" or argument.startswith("--extern=") for argument in argv):
        raise RuntimeLinkEvidenceError(
            "rustc plan already contains an extern argument before descriptor admission"
        )
    expanded = [
        *argv,
        "--extern",
        f"{evidence['externCrate']}={evidence['artifactPath']}",
    ]
    if expanded.count("--extern") != 1:
        raise RuntimeLinkEvidenceError("rustc runtime link plan must contain exactly one --extern")
    return expanded
