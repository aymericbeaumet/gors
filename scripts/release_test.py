"""Release admission tests; no Rust build or network access required."""

import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "gors"
        self.binary.write_bytes(b"binary payload")
        self.binary.chmod(0o755)
        self.license = self.root / "go-license"
        self.license.write_text("Go license\n", encoding="utf-8")
        self.dist = self.root / "dist"

    def package(self, target):
        return release.package(self.binary, self.license, target, "v0.1.0", self.dist)

    def receipt(self, archive, target):
        release.write_smoke_receipt(archive, target, "v0.1.0")

    def test_all_six_packages_are_portable_and_deterministic(self):
        for target in release.TARGETS:
            with self.subTest(target=target):
                archive = self.package(target)
                before = archive.read_bytes()
                manifest = release.inspect_archive(archive, target, "v0.1.0")
                self.assertEqual(manifest["target"], target)
                self.assertIn("GO-LICENSE", manifest["files"])
                self.assertEqual(self.package(target).read_bytes(), before)

    def test_complete_release_requires_every_target_and_smoke(self):
        for target in release.TARGETS:
            archive = self.package(target)
            self.receipt(archive, target)
        sums = release.validate(self.dist, "v0.1.0")
        self.assertEqual(len(sums.read_text().splitlines()), 6)
        next(self.dist.glob("*.smoke.json")).unlink()
        with self.assertRaisesRegex(ValueError, "smoke receipt"):
            release.validate(self.dist, "v0.1.0")

    def test_incomplete_release_cannot_be_published(self):
        target = next(iter(release.TARGETS))
        self.receipt(self.package(target), target)
        with self.assertRaisesRegex(ValueError, "missing archive"):
            release.validate(self.dist, "v0.1.0")

    def test_archive_corruption_fails_checksum(self):
        target = next(iter(release.TARGETS))
        archive = self.package(target)
        archive.write_bytes(archive.read_bytes() + b"corruption")
        with self.assertRaisesRegex(ValueError, "checksum"):
            release.inspect_archive(archive, target, "v0.1.0")

    def test_archive_member_corruption_fails_manifest(self):
        target = next(iter(release.TARGETS))
        archive = self.package(target)
        with tarfile.open(archive) as source:
            members = [(member, source.extractfile(member).read()) for member in source]
        with tarfile.open(archive, "w:gz") as destination:
            for member, payload in members:
                if member.name.endswith("GO-LICENSE"):
                    payload = b"bad license"
                    member.size = len(payload)
                destination.addfile(member, io.BytesIO(payload))
        release.write_checksum(archive)
        with self.assertRaisesRegex(ValueError, "content hash"):
            release.inspect_archive(archive, target, "v0.1.0")

    def test_archive_traversal_is_rejected_before_extraction(self):
        target = next(iter(release.TARGETS))
        archive = self.package(target)
        with tarfile.open(archive, "w:gz") as destination:
            member = tarfile.TarInfo("../outside")
            member.size = 1
            destination.addfile(member, io.BytesIO(b"x"))
        release.write_checksum(archive)
        with self.assertRaisesRegex(ValueError, "archive member"):
            release.inspect_archive(archive, target, "v0.1.0")

    def test_stale_smoke_receipt_is_rejected(self):
        for target in release.TARGETS:
            self.receipt(self.package(target), target)
        receipt = next(self.dist.glob("*.smoke.json"))
        value = json.loads(receipt.read_text())
        value["archive_sha256"] = "0" * 64
        receipt.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "smoke receipt"):
            release.validate(self.dist, "v0.1.0")

    def test_wrong_target_is_rejected(self):
        targets = list(release.TARGETS)
        archive = self.package(targets[0])
        with self.assertRaises(ValueError):
            release.inspect_archive(archive, targets[1], "v0.1.0")

    def test_version_matches_cli_manifest(self):
        expected = "v" + release.read_toml(release.ROOT / "gors-cli/Cargo.toml")["package"]["version"]
        self.assertEqual(release.release_version(None), expected)
        with self.assertRaisesRegex(ValueError, "package version"):
            release.release_version("v99.0.0")
        with self.assertRaises(ValueError):
            release.release_version("../../escape")

    def test_target_requires_native_host(self):
        with patch.object(release, "command") as command:
            command.return_value.stdout = "rustc 1.96.0\nhost: aarch64-apple-darwin\n"
            with self.assertRaisesRegex(ValueError, "require native target"):
                release.native_target("x86_64-apple-darwin")

    def test_prerelease_tag_matches_package_version(self):
        with patch.object(release, "read_toml", return_value={"package": {"version": "0.2.0-rc.1+build.2"}}):
            self.assertEqual(release.release_version("v0.2.0-rc.1+build.2"), "v0.2.0-rc.1+build.2")
            with self.assertRaisesRegex(ValueError, "package version"):
                release.release_version("v0.2.0")

    def test_repackaging_invalidates_old_smoke_receipt(self):
        target = next(iter(release.TARGETS))
        archive = self.package(target)
        self.receipt(archive, target)
        self.assertTrue(Path(f"{archive}.smoke.json").exists())
        self.package(target)
        self.assertFalse(Path(f"{archive}.smoke.json").exists())

    def test_unexpected_archive_prevents_release_admission(self):
        for target in release.TARGETS:
            self.receipt(self.package(target), target)
        (self.dist / "old-release.zip").write_bytes(b"stale")
        with self.assertRaisesRegex(ValueError, "unexpected release files"):
            release.validate(self.dist, "v0.1.0")


if __name__ == "__main__":
    unittest.main()
