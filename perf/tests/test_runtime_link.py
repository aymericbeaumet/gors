from __future__ import annotations

import copy
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path

from perf_harness.runtime_link import (
    LINK_DESCRIPTOR_SCHEMA_VERSION,
    RUNTIME_ARTIFACT_FILENAME,
    RUNTIME_CRATE_NAME,
    RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION,
    RUST_TARGET_LIBDIR_SCHEMA_VERSION,
    RuntimeLinkEvidenceError,
    canonical_rustc_release_record,
    canonical_target_libdir_record,
    expand_rustc_runtime_link,
    runtime_artifact_identity,
    runtime_link_plan_identity,
    rust_runtime_compatibility_identity,
    validate_runtime_link_descriptor,
)


class RuntimeLinkDescriptorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.runtime_cache = self.root / "cache" / "gors" / "runtime"
        self.descriptor_path = self.root / "generated" / ".gors-link.json"
        self.contract = "1a" * 32
        self.target_triple = "x86_64-unknown-linux-gnu"
        self.pointer_width = 64
        self.endianness = "little"
        self.rustc_verbose = b"rustc 1.96.0\nhost: x86_64-unknown-linux-gnu\n"
        self.target_libdir = self.root / "target-libdir"
        self.target_libdir.mkdir()
        (self.target_libdir / "libcore-0123456789abcdef.rmeta").write_bytes(
            b"metadata"
        )
        self.release_record = canonical_rustc_release_record(self.rustc_verbose)
        self.target_libdir_record = canonical_target_libdir_record(self.target_libdir)
        self.compatibility = rust_runtime_compatibility_identity(
            self.rustc_verbose,
            self.target_libdir_record,
            target_triple=self.target_triple,
            target_pointer_width=self.pointer_width,
            target_endianness=self.endianness,
        )
        self.payload = b"exact precompiled runtime rlib"
        self.producer = "0f" * 32
        self.implementation = hashlib.sha256(self.payload).hexdigest()
        self.artifact = runtime_artifact_identity(
            contract_identity=self.contract,
            target_triple=self.target_triple,
            target_pointer_width=self.pointer_width,
            target_endianness=self.endianness,
            compatibility_identity=self.compatibility,
            implementation_hash=self.implementation,
        )
        self.operation_ids = [14, 16]
        self.link_plan = runtime_link_plan_identity(
            contract_identity=self.contract,
            operation_ids=self.operation_ids,
            target_triple=self.target_triple,
            target_pointer_width=self.pointer_width,
            target_endianness=self.endianness,
            compatibility_identity=self.compatibility,
            implementation_hash=self.implementation,
            artifact_identity=self.artifact,
        )
        artifact_path = self.runtime_cache / self.artifact / RUNTIME_ARTIFACT_FILENAME
        artifact_path.parent.mkdir(parents=True)
        artifact_path.write_bytes(self.payload)
        self.descriptor = {
            "schema_version": 1,
            "extern_crate": RUNTIME_CRATE_NAME,
            "artifact_path": str(artifact_path),
            "link_descriptor_schema_version": LINK_DESCRIPTOR_SCHEMA_VERSION,
            "dependency": {
                "schema_version": 1,
                "contract": self.contract,
                "operation_ids": self.operation_ids,
            },
            "target_triple": self.target_triple,
            "target_pointer_width": self.pointer_width,
            "target_endianness": self.endianness,
            "format": "rust-rlib-v1",
            "producer_identity": self.producer,
            "compatibility_identity": self.compatibility,
            "implementation_hash": self.implementation,
            "artifact_identity": self.artifact,
            "link_plan_identity": self.link_plan,
        }

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_descriptor(self, descriptor: dict | None = None) -> None:
        self.descriptor_path.parent.mkdir(parents=True, exist_ok=True)
        self.descriptor_path.write_text(
            json.dumps(descriptor or self.descriptor), encoding="utf-8"
        )

    def validate(self) -> dict:
        return validate_runtime_link_descriptor(
            self.descriptor_path,
            expected_contract_identity=self.contract,
            expected_target_triple=self.target_triple,
            expected_target_pointer_width=self.pointer_width,
            expected_target_endianness=self.endianness,
            expected_compatibility_identity=self.compatibility,
            expected_rustc_release_record_sha256=hashlib.sha256(
                self.release_record
            ).hexdigest(),
            expected_target_libdir_record_sha256=hashlib.sha256(
                self.target_libdir_record
            ).hexdigest(),
            expected_runtime_cache_root=self.runtime_cache,
        )

    def test_valid_descriptor_expands_exactly_one_runtime_extern(self) -> None:
        self.write_descriptor()
        evidence = self.validate()
        expanded = expand_rustc_runtime_link(["rustc", "main.rs"], evidence)

        self.assertEqual(expanded.count("--extern"), 1)
        self.assertEqual(
            expanded[-2:],
            ["--extern", f"{RUNTIME_CRATE_NAME}={self.descriptor['artifact_path']}"],
        )
        self.assertEqual(evidence["implementationHash"], evidence["artifactSha256"])
        self.assertEqual(evidence["artifactIdentity"], self.artifact)
        self.assertEqual(evidence["linkPlanIdentity"], self.link_plan)
        self.assertEqual(evidence["producerIdentity"], self.producer)
        self.assertEqual(evidence["compatibilityIdentity"], self.compatibility)
        self.assertEqual(
            evidence["runtimeCompatibilitySchemaVersion"],
            RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION,
        )
        self.assertEqual(
            evidence["targetLibdirSchemaVersion"],
            RUST_TARGET_LIBDIR_SCHEMA_VERSION,
        )

    def test_missing_malformed_or_duplicate_descriptor_fails_closed(self) -> None:
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "cannot read"):
            self.validate()

        self.descriptor_path.parent.mkdir(parents=True)
        self.descriptor_path.write_text("not-json", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "malformed"):
            self.validate()

        self.descriptor_path.write_text(
            '{"schema_version":1,"schema_version":1}', encoding="utf-8"
        )
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "duplicate field"):
            self.validate()

    def test_unknown_fields_schemas_operations_and_contracts_fail_closed(self) -> None:
        mutations = []
        extra = copy.deepcopy(self.descriptor)
        extra["legacy_runtime_source"] = "forbidden"
        mutations.append((extra, "fields are incompatible"))
        schema = copy.deepcopy(self.descriptor)
        schema["schema_version"] = 2
        mutations.append((schema, "output schema is incompatible"))
        operation = copy.deepcopy(self.descriptor)
        operation["dependency"]["operation_ids"] = [14, 65535]
        mutations.append((operation, "unknown operation IDs"))
        unordered = copy.deepcopy(self.descriptor)
        unordered["dependency"]["operation_ids"] = [16, 14]
        mutations.append((unordered, "sorted and duplicate-free"))
        contract = copy.deepcopy(self.descriptor)
        contract["dependency"]["contract"] = "2b" * 32
        mutations.append((contract, "contract identity is stale"))

        for descriptor, message in mutations:
            with self.subTest(message=message):
                self.write_descriptor(descriptor)
                with self.assertRaisesRegex(RuntimeLinkEvidenceError, message):
                    self.validate()

    def test_stale_target_compatibility_payload_and_identities_fail_closed(self) -> None:
        target = copy.deepcopy(self.descriptor)
        target["target_pointer_width"] = 32
        self.write_descriptor(target)
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "target is stale"):
            self.validate()

        compatibility = copy.deepcopy(self.descriptor)
        compatibility["compatibility_identity"] = "3c" * 32
        self.write_descriptor(compatibility)
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "compatibility identity is stale"):
            self.validate()

        artifact_path = Path(self.descriptor["artifact_path"])
        artifact_path.write_bytes(b"tampered")
        self.write_descriptor()
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "payload is stale"):
            self.validate()
        artifact_path.write_bytes(self.payload)

        artifact = copy.deepcopy(self.descriptor)
        artifact["artifact_identity"] = "4d" * 32
        artifact["artifact_path"] = str(
            self.runtime_cache / artifact["artifact_identity"] / RUNTIME_ARTIFACT_FILENAME
        )
        replacement = Path(artifact["artifact_path"])
        replacement.parent.mkdir(parents=True)
        replacement.write_bytes(self.payload)
        self.write_descriptor(artifact)
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "artifact identity is stale"):
            self.validate()

        link_plan = copy.deepcopy(self.descriptor)
        link_plan["link_plan_identity"] = "5e" * 32
        self.write_descriptor(link_plan)
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "link-plan identity is stale"):
            self.validate()

    def test_preexisting_extern_is_rejected(self) -> None:
        self.write_descriptor()
        evidence = self.validate()
        with self.assertRaisesRegex(RuntimeLinkEvidenceError, "already contains"):
            expand_rustc_runtime_link(["rustc", "--extern", "other=lib.rlib"], evidence)


class RustCompatibilityIdentityTests(unittest.TestCase):
    def compatibility(self, rustc_verbose: bytes, inventory: bytes) -> str:
        return rust_runtime_compatibility_identity(
            rustc_verbose,
            inventory,
            target_triple="aarch64-unknown-linux-gnu",
            target_pointer_width=64,
            target_endianness="little",
        )

    def test_producer_host_is_excluded_but_compiler_release_is_not(self) -> None:
        inventory = b"target-sysroot-inventory"
        cross_host = self.compatibility(
            b"rustc 1.96.0\ncommit-hash: abc\nhost: x86_64-unknown-linux-gnu\n",
            inventory,
        )
        native = self.compatibility(
            b"rustc 1.96.0\ncommit-hash: abc\nhost: aarch64-unknown-linux-gnu\n",
            inventory,
        )
        other_compiler = self.compatibility(
            b"rustc 1.96.0\ncommit-hash: def\nhost: aarch64-unknown-linux-gnu\n",
            inventory,
        )

        self.assertEqual(cross_host, native)
        self.assertNotEqual(cross_host, other_compiler)
        self.assertNotIn(
            b"host:",
            canonical_rustc_release_record(
                b"rustc 1.96.0\nhost: x86_64-unknown-linux-gnu\n"
            ),
        )

    def test_release_record_requires_exactly_one_nonempty_host(self) -> None:
        for value in (
            b"rustc 1.96.0\n",
            b"rustc 1.96.0\nhost: \n",
            b"rustc 1.96.0\nhost: first\nhost: second\n",
        ):
            with self.subTest(value=value):
                with self.assertRaises(RuntimeLinkEvidenceError):
                    canonical_rustc_release_record(value)

    def test_release_record_matches_rust_line_terminator_rules(self) -> None:
        self.assertEqual(
            canonical_rustc_release_record(
                b"rustc 1.96.0\r\nhost: x86_64-unknown-linux-gnu\r\ncommit: abc\r"
            ),
            b"rustc 1.96.0\ncommit: abc\r\n",
        )

    def test_compatibility_rejects_noncanonical_target_triples(self) -> None:
        for triple in ("", "target with spaces", "targ\N{SNOWMAN}t"):
            with self.subTest(triple=triple):
                with self.assertRaises(RuntimeLinkEvidenceError):
                    rust_runtime_compatibility_identity(
                        b"rustc 1.96.0\nhost: x86_64-unknown-linux-gnu\n",
                        b"inventory",
                        target_triple=triple,
                        target_pointer_width=64,
                        target_endianness="little",
                    )

    def test_recursive_inventory_hashes_only_unhashed_regular_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            nested = root / "self-contained"
            nested.mkdir()
            metadata = root / "libcore-0123456789abcdef.rmeta"
            metadata.write_bytes(b"metadata")
            native = nested / "crt1.o"
            native.write_bytes(b"native-a")

            baseline = canonical_target_libdir_record(root)
            metadata.write_bytes(b"METADATA")
            self.assertEqual(baseline, canonical_target_libdir_record(root))

            native.write_bytes(b"native-b")
            self.assertNotEqual(baseline, canonical_target_libdir_record(root))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support is required")
    def test_inventory_tracks_relative_symlink_targets(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "first.o").write_bytes(b"first")
            (root / "second.o").write_bytes(b"second")
            alias = root / "current.o"
            os.symlink("first.o", alias)
            first = canonical_target_libdir_record(root)
            alias.unlink()
            os.symlink("second.o", alias)

            self.assertNotEqual(first, canonical_target_libdir_record(root))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support is required")
    def test_inventory_rejects_relative_symlink_escape(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            nested = root / "nested"
            nested.mkdir()
            os.symlink("../../outside.rlib", nested / "escape.rlib")

            with self.assertRaisesRegex(
                RuntimeLinkEvidenceError, "escapes its inventory root"
            ):
                canonical_target_libdir_record(root)


if __name__ == "__main__":
    unittest.main()
