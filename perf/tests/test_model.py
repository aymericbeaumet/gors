from __future__ import annotations

import argparse
import copy
import datetime as dt
import json
import tempfile
import unittest
from pathlib import Path

from perf_harness.cli import repository_root, validate_checked_in_files, validate_protocol_arguments
from perf_harness.model import (
    GORS_CACHE_SCHEMA_VERSION,
    RESULT_SCHEMA_VERSION,
    EvidenceError,
    achievement,
    configuration_fingerprint,
    gate_acceptance,
    percentile,
    result_id,
    sha256_file,
    summarize_pairs,
    validate_corpus,
    validate_result,
)
from perf_harness.native import (
    ARTIFACT_DRIVER_PRODUCTION,
    GORS_BUILD_CACHE_HIT_PHASES,
    GORS_BUILD_CACHE_MISS_PHASES,
    GORS_TIMING_REPORT_VERSION,
    Toolchains,
    _validate_gors_timing_evidence,
    pipeline_plan,
)
from perf_harness.runner import runtime_contract_identity
from perf_harness.runtime_link import (
    LINK_DESCRIPTOR_SCHEMA_VERSION,
    LINK_OUTPUT_SCHEMA_VERSION,
    RUNTIME_ARTIFACT_FILENAME,
    RUNTIME_ARTIFACT_FORMAT,
    RUNTIME_CRATE_NAME,
    RUNTIME_LINK_VALIDATION,
    RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION,
    RUST_TARGET_LIBDIR_SCHEMA_VERSION,
)


def synthetic_runtime_link_evidence() -> dict:
    artifact_identity = "8" * 64
    artifact_path = f"/cache/runtime/{artifact_identity}/{RUNTIME_ARTIFACT_FILENAME}"
    implementation_hash = "7" * 64
    return {
        "validation": RUNTIME_LINK_VALIDATION,
        "descriptorPath": "/generated/.gors-link.json",
        "descriptorSha256": "9" * 64,
        "outputSchemaVersion": LINK_OUTPUT_SCHEMA_VERSION,
        "linkDescriptorSchemaVersion": LINK_DESCRIPTOR_SCHEMA_VERSION,
        "dependencySchemaVersion": 1,
        "externCrate": RUNTIME_CRATE_NAME,
        "artifactPath": artifact_path,
        "contractIdentity": "5" * 64,
        "operationIds": [14, 16],
        "targetTriple": "test-target",
        "targetPointerWidth": 64,
        "targetEndianness": "little",
        "format": RUNTIME_ARTIFACT_FORMAT,
        "runtimeCompatibilitySchemaVersion": RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION,
        "rustcReleaseRecordSha256": "b" * 64,
        "targetLibdirSchemaVersion": RUST_TARGET_LIBDIR_SCHEMA_VERSION,
        "targetLibdirRecordSha256": "c" * 64,
        "producerIdentity": "f" * 64,
        "compatibilityIdentity": "6" * 64,
        "implementationHash": implementation_hash,
        "artifactSha256": implementation_hash,
        "artifactIdentity": artifact_identity,
        "linkPlanIdentity": "a" * 64,
    }


def synthetic_measurement(wall_ns: float, compiler: str) -> dict:
    runtime_link = synthetic_runtime_link_evidence() if compiler == "gors" else None
    commands = []
    if compiler == "gors":
        commands.append(
            {
                "stage": "gors.compile_emit",
                "argv": ["gors", "build"],
                "runtimeLink": None,
            }
        )
        commands.append(
            {
                "stage": "gors.external_rustc_link",
                "argv": [
                    "rustc",
                    "main.rs",
                    "--extern",
                    f"{RUNTIME_CRATE_NAME}={runtime_link['artifactPath']}",
                ],
                "runtimeLink": runtime_link,
            }
        )
    else:
        commands.append(
            {
                "stage": "go.build_link",
                "argv": ["go", "build"],
                "runtimeLink": None,
            }
        )
    return {
        "wallNs": wall_ns,
        "normalizedWallNs": wall_ns,
        "commands": commands,
        "runtimeLink": runtime_link,
    }


def synthetic_result(*, commit: str, p50: float = 91.0, p95: float = 96.0) -> dict:
    sample_count = 50
    percentile_weight = 0.55
    gors_high = (p95 - (1.0 - percentile_weight) * p50) / percentile_weight
    go_high = (110.0 - (1.0 - percentile_weight) * 100.0) / percentile_weight
    gors_values = [p50] * 47 + [gors_high] * 3
    go_values = [100.0] * 47 + [go_high] * 3
    pairs = [
        {
            "gors": synthetic_measurement(gors, "gors"),
            "go": synthetic_measurement(go, "go"),
        }
        for gors, go in zip(gors_values, go_values, strict=True)
    ]
    sessions = []
    offset = 0
    for session_index, count in enumerate((17, 17, 16), start=1):
        sessions.append(
            {
                "sessionIndex": session_index,
                "calibration": {
                    "before": {"durationNs": 100.0},
                    "after": {"durationNs": 100.0},
                    "centerNs": 100.0,
                    "driftFraction": 0.0,
                    "healthPassed": True,
                    "normalizationFactor": 1.0,
                },
                "samples": pairs[offset : offset + count],
            }
        )
        offset += count
    summary = {
        "raw": summarize_pairs(pairs, seed=20260722, duration_field="wallNs"),
        "normalized": summarize_pairs(pairs, seed=20260722),
    }
    result = {
        "schemaVersion": RESULT_SCHEMA_VERSION,
        "kind": "gors.performance.result",
        "resultId": "",
        "configurationFingerprint": "",
        "createdAt": "2026-07-22T12:00:00Z",
        "mode": "certification",
        "promotionEligible": True,
        "promotionBlockers": [],
        "fullClaimEligible": False,
        "fullClaimBlockers": ["synthetic focused scenario"],
        "repository": {
            "commit": commit,
            "dirty": False,
            "statusSha256": "0" * 64,
        },
        "corpus": {
            "version": "bootstrap-v1",
            "digest": "1" * 64,
            "scenarioSupport": [],
        },
        "protocol": {
            "lane": "end_to_end_artifact",
            "artifactDriver": "production-artifact-v1",
            "artifactDriverProduction": True,
            "samples": sample_count,
            "sessions": 3,
            "seed": 20260722,
            "smoke": False,
            "calibrationVersion": "calibration-v1",
            "gorsCacheSchema": GORS_CACHE_SCHEMA_VERSION,
            "runtimeLinkDescriptorSchema": LINK_DESCRIPTOR_SCHEMA_VERSION,
            "runtimeLinkValidation": RUNTIME_LINK_VALIDATION,
            "runtimeCompatibilitySchemaVersion": (
                RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION
            ),
            "targetLibdirSchemaVersion": RUST_TARGET_LIBDIR_SCHEMA_VERSION,
            "ioByteCountersAvailable": True,
            "processTreeCountersAvailable": True,
            "stageFingerprintsAvailable": True,
        },
        "environment": {
            "hardwareClass": "test-worker-v1",
            "dedicated": True,
            "jobBudget": 4,
        },
        "toolchains": {
            "gors": {"runtimeContractIdentity": "5" * 64},
            "go": {"version": "go1.26.3", "sha256": "2" * 64, "experiment": ""},
            "rustc": {
                "channel": "1.96.0",
                "target": "test-target",
                "pointerWidth": 64,
                "endianness": "little",
                "targetLibdir": "/tool/lib/rustlib/test-target/lib",
                "runtimeCompatibilitySchemaVersion": (
                    RUST_RLIB_COMPATIBILITY_SCHEMA_VERSION
                ),
                "runtimeCompatibilityIdentity": "6" * 64,
                "rustcReleaseRecordSha256": "b" * 64,
                "targetLibdirSchemaVersion": RUST_TARGET_LIBDIR_SCHEMA_VERSION,
                "targetLibdirRecordSha256": "c" * 64,
                "sha256": "3" * 64,
            },
            "linker": {"version": "test-linker", "sha256": "4" * 64},
        },
        "workloads": [
            {
                "workloadId": "bootstrap_scalar",
                "scenario": "cold",
                "validation": {"passed": True},
                "hostHealthPassed": True,
                "sessions": sessions,
                "summary": summary,
                "achievement": achievement(summary["normalized"], True),
            }
        ],
    }
    result["configurationFingerprint"] = configuration_fingerprint(result)
    result["resultId"] = result_id(result)
    return result


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class RuntimeContractIdentityTests(unittest.TestCase):
    def test_extracts_typed_contract_hash_from_version_output(self) -> None:
        identity = "a5" * 32
        version = f"gors version gors0.1.0 gostdlib1.26.3 darwin/arm64 runtime-contract={identity}"

        self.assertEqual(runtime_contract_identity(version), identity)

    def test_rejects_legacy_version_only_output(self) -> None:
        with self.assertRaises(RuntimeError):
            runtime_contract_identity(
                "gors version gors0.1.0 gostdlib1.26.3 darwin/arm64"
            )


class RuntimeContractEvidenceTests(unittest.TestCase):
    def test_runtime_artifact_is_part_of_configuration_fingerprint(self) -> None:
        result = synthetic_result(commit="a" * 40)
        original = result["configurationFingerprint"]
        for workload in result["workloads"]:
            for session in workload["sessions"]:
                for sample in session["samples"]:
                    link = sample["gors"]["runtimeLink"]
                    link["implementationHash"] = "c" * 64
                    link["artifactIdentity"] = "d" * 64

        self.assertNotEqual(original, configuration_fingerprint(result))

    def test_release_and_target_libdir_records_are_configuration_inputs(self) -> None:
        result = synthetic_result(commit="a" * 40)
        original = result["configurationFingerprint"]
        result["toolchains"]["rustc"]["rustcReleaseRecordSha256"] = "d" * 64
        self.assertNotEqual(original, configuration_fingerprint(result))
        result["toolchains"]["rustc"]["rustcReleaseRecordSha256"] = "b" * 64
        result["toolchains"]["rustc"]["targetLibdirRecordSha256"] = "e" * 64
        self.assertNotEqual(original, configuration_fingerprint(result))

    def test_rejects_multiple_runtime_artifact_configurations(self) -> None:
        result = synthetic_result(commit="a" * 40)
        measurement = result["workloads"][0]["sessions"][0]["samples"][0]["gors"]
        link = measurement["runtimeLink"]
        artifact_identity = "d" * 64
        artifact_path = f"/cache/runtime/{artifact_identity}/{RUNTIME_ARTIFACT_FILENAME}"
        link["implementationHash"] = "c" * 64
        link["artifactSha256"] = "c" * 64
        link["artifactIdentity"] = artifact_identity
        link["artifactPath"] = artifact_path
        measurement["commands"][1]["argv"][-1] = (
            f"{RUNTIME_CRATE_NAME}={artifact_path}"
        )
        result["configurationFingerprint"] = configuration_fingerprint(result)
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "one exact runtime artifact"):
            validate_result(result)

    def test_rejects_multiple_runtime_producer_identities(self) -> None:
        result = synthetic_result(commit="a" * 40)
        result["workloads"][0]["sessions"][0]["samples"][0]["gors"][
            "runtimeLink"
        ]["producerIdentity"] = "e" * 64
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "one exact runtime producer"):
            validate_result(result)

    def test_rejects_missing_runtime_contract_identity(self) -> None:
        result = synthetic_result(commit="a" * 40)
        del result["toolchains"]["gors"]["runtimeContractIdentity"]

        with self.assertRaisesRegex(EvidenceError, "runtime contract identity"):
            validate_result(result)

    def test_rejects_malformed_runtime_contract_identities(self) -> None:
        for identity in ("5" * 63, "G" * 64, "A" * 64):
            with self.subTest(identity=identity):
                result = synthetic_result(commit="a" * 40)
                result["toolchains"]["gors"]["runtimeContractIdentity"] = identity

                with self.assertRaisesRegex(EvidenceError, "runtime contract identity"):
                    validate_result(result)

    def test_rejects_legacy_result_schema(self) -> None:
        result = synthetic_result(commit="a" * 40)
        result["schemaVersion"] = RESULT_SCHEMA_VERSION - 1

        with self.assertRaisesRegex(EvidenceError, "unsupported result schemaVersion"):
            validate_result(result)

    def test_rejects_missing_runtime_link_evidence(self) -> None:
        result = synthetic_result(commit="a" * 40)
        result["workloads"][0]["sessions"][0]["samples"][0]["gors"][
            "runtimeLink"
        ] = None
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "missing validated runtime-link"):
            validate_result(result)

    def test_rejects_stale_runtime_compatibility_evidence(self) -> None:
        result = synthetic_result(commit="a" * 40)
        link = result["workloads"][0]["sessions"][0]["samples"][0]["gors"][
            "runtimeLink"
        ]
        link["compatibilityIdentity"] = "d" * 64
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "compatibility evidence is stale"):
            validate_result(result)

    def test_rejects_stale_release_and_target_libdir_evidence(self) -> None:
        for field in ("rustcReleaseRecordSha256", "targetLibdirRecordSha256"):
            with self.subTest(field=field):
                result = synthetic_result(commit="a" * 40)
                link = result["workloads"][0]["sessions"][0]["samples"][0][
                    "gors"
                ]["runtimeLink"]
                link[field] = "d" * 64
                result["resultId"] = result_id(result)

                with self.assertRaisesRegex(EvidenceError, "compatibility evidence is stale"):
                    validate_result(result)

    def test_rejects_duplicate_runtime_extern_evidence(self) -> None:
        result = synthetic_result(commit="a" * 40)
        measurement = result["workloads"][0]["sessions"][0]["samples"][0]["gors"]
        extern_value = f"{RUNTIME_CRATE_NAME}={measurement['runtimeLink']['artifactPath']}"
        measurement["commands"][1]["argv"].extend(["--extern", extern_value])
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "exactly one --extern"):
            validate_result(result)


def gors_timing_report(*, cache_hit: bool, jobs: int = 4) -> dict:
    phase_names = GORS_BUILD_CACHE_HIT_PHASES if cache_hit else GORS_BUILD_CACHE_MISS_PHASES
    return {
        "version": GORS_TIMING_REPORT_VERSION,
        "command": "build",
        "jobs": jobs,
        "phases": [{"name": name, "durationMs": 1.0} for name in phase_names],
        "cacheEvents": [{"layer": "compiler", "hit": cache_hit}],
        "scheduler": {
            "serialWaves": 0,
            "parallelWaves": 0,
            "scheduledRoots": 0,
            "snapshotsCreated": 0,
            "poolStarts": 0,
            "peakWorkers": 0,
        },
    }


class StatisticsTests(unittest.TestCase):
    def test_percentile_interpolates_small_samples(self) -> None:
        self.assertEqual(percentile([10.0, 20.0], 0.50), 15.0)
        self.assertEqual(percentile([10.0, 20.0], 0.95), 19.5)

    def test_pair_summary_retains_ratio_confidence_interval(self) -> None:
        pairs = [
            {
                "gors": {"normalizedWallNs": value},
                "go": {"normalizedWallNs": 100.0},
            }
            for value in (80.0, 90.0, 100.0)
        ]
        summary = summarize_pairs(pairs, seed=7)
        self.assertEqual(summary["sampleCount"], 3)
        self.assertEqual(summary["pairedRatio"]["p50"], 0.9)
        self.assertEqual(summary["pairedRatio"]["bootstrapMedianCi95"]["iterations"], 10_000)

    def test_result_validation_recomputes_aggregates_from_raw_sessions(self) -> None:
        result = synthetic_result(commit="a" * 40)
        result["workloads"][0]["summary"]["normalized"]["gors"]["p50Ns"] = 1.0
        result["resultId"] = result_id(result)

        with self.assertRaisesRegex(EvidenceError, "recomputed raw evidence"):
            validate_result(result)


class ProtocolTests(unittest.TestCase):
    def arguments(self, *, smoke: bool) -> argparse.Namespace:
        return argparse.Namespace(
            command="certify",
            samples=2,
            sessions=1,
            smoke=smoke,
            dedicated=False,
            hardware_class="unclassified-local",
        )

    def test_small_override_requires_smoke_label(self) -> None:
        with self.assertRaisesRegex(EvidenceError, "require --smoke"):
            validate_protocol_arguments(self.arguments(smoke=False))

    def test_smoke_override_is_allowed_but_not_a_certification(self) -> None:
        validate_protocol_arguments(self.arguments(smoke=True))


class CheckedInDataTests(unittest.TestCase):
    def test_corpus_digest_and_explicit_frontier_are_current(self) -> None:
        root = repository_root()
        corpus = validate_corpus(root / "perf" / "corpus" / "v1" / "manifest.json")
        support = {entry["id"]: entry for entry in corpus["scenarioSupport"]}
        self.assertTrue(support["cold"]["supported"])
        self.assertFalse(support["dependency_body_edit"]["supported"])
        self.assertIn("imports", support["dependency_body_edit"]["reason"])

    def test_schema_and_zero_promotion_manifest_smoke(self) -> None:
        report = validate_checked_in_files()
        self.assertEqual(report["schemas"], 3)
        self.assertEqual(report["promotedScenarios"], 0)


class NativeBoundaryTests(unittest.TestCase):
    def test_scheduler_evidence_accepts_explicit_zero_cache_hit(self) -> None:
        _validate_gors_timing_evidence(gors_timing_report(cache_hit=True), 4)

    def test_scheduler_evidence_accepts_v5_cache_miss_phase_contract(self) -> None:
        _validate_gors_timing_evidence(gors_timing_report(cache_hit=False), 4)

    def test_scheduler_evidence_rejects_stale_timing_schema(self) -> None:
        timings = gors_timing_report(cache_hit=False)
        timings["version"] = GORS_TIMING_REPORT_VERSION - 1

        with self.assertRaisesRegex(RuntimeError, "expected version 5"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_cache_hit_without_source_load(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["phases"] = [
            phase for phase in timings["phases"] if phase["name"] != "cli.source_load"
        ]

        with self.assertRaisesRegex(RuntimeError, "hit timing phases are incompatible"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_cache_lookup_before_source_load(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["phases"].reverse()

        with self.assertRaisesRegex(RuntimeError, "hit timing phases are incompatible"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_cache_miss_without_source_load(self) -> None:
        timings = gors_timing_report(cache_hit=False)
        timings["phases"] = [
            phase for phase in timings["phases"] if phase["name"] != "cli.source_load"
        ]

        with self.assertRaisesRegex(RuntimeError, "miss timing phases are incompatible"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_ambiguous_cache_events(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["cacheEvents"].append({"layer": "compiler", "hit": False})

        with self.assertRaisesRegex(RuntimeError, "exactly one unambiguous"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_missing_cache_event(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["cacheEvents"] = []

        with self.assertRaisesRegex(RuntimeError, "exactly one unambiguous"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_cache_hit_with_compile_phases(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["phases"] = [
            {"name": name, "durationMs": 1.0} for name in GORS_BUILD_CACHE_MISS_PHASES
        ]

        with self.assertRaisesRegex(RuntimeError, "hit timing phases are incompatible"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_cache_hit_with_worker_activity(self) -> None:
        timings = gors_timing_report(cache_hit=True)
        timings["scheduler"]["serialWaves"] = 1
        timings["scheduler"]["scheduledRoots"] = 1

        with self.assertRaisesRegex(RuntimeError, "must be all zero"):
            _validate_gors_timing_evidence(timings, 4)

    def test_scheduler_evidence_rejects_budget_overrun(self) -> None:
        timings = gors_timing_report(cache_hit=False, jobs=2)
        timings["scheduler"] = {
            "serialWaves": 0,
            "parallelWaves": 1,
            "scheduledRoots": 8,
            "snapshotsCreated": 3,
            "poolStarts": 1,
            "peakWorkers": 3,
        }
        with self.assertRaisesRegex(RuntimeError, "exceeded the certified job budget"):
            _validate_gors_timing_evidence(timings, 2)

    def test_bootstrap_artifact_lane_includes_external_rustc_and_link(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            toolchains = Toolchains(
                gors=Path("/tool/gors"),
                go=Path("/tool/go"),
                goroot=Path("/tool/goroot"),
                rustc=Path("/tool/rustc"),
                rust_channel="1.96.0",
                gors_version="test",
                go_version="go version go1.26.3 test/test",
                rustc_version="test",
                rust_target="test-target",
                rust_pointer_width=64,
                rust_endianness="little",
                runtime_contract_identity="5" * 64,
                rustc_target_libdir=Path("/tool/lib/rustlib/test-target/lib"),
                rustc_release_record_sha256="b" * 64,
                target_libdir_record_sha256="c" * 64,
                runtime_compatibility_identity="6" * 64,
                linker_path="/tool/cc",
                linker_version="test-linker",
                linker_sha256="4" * 64,
            )
            plan, _ = pipeline_plan(
                "gors",
                side_root=root,
                source=root / "main.go",
                toolchains=toolchains,
                job_budget=1,
                go_experiment="",
            )
        self.assertFalse(ARTIFACT_DRIVER_PRODUCTION)
        self.assertEqual(
            [command["stage"] for command in plan["commands"]],
            ["gors.compile_emit", "gors.external_rustc_link"],
        )
        self.assertEqual(plan["jobBudget"], 1)
        self.assertIn("--jobs", plan["commands"][0]["argv"])
        jobs_index = plan["commands"][0]["argv"].index("--jobs")
        self.assertEqual(plan["commands"][0]["argv"][jobs_index + 1], "1")
        self.assertNotIn("--release", plan["commands"][0]["argv"])
        self.assertIn("-Clto=fat", plan["commands"][1]["argv"])
        self.assertIn("-Ctarget-cpu=generic", plan["commands"][1]["argv"])
        self.assertIn("-Ctarget-feature=", plan["commands"][1]["argv"])
        target_index = plan["commands"][1]["argv"].index("--target")
        self.assertEqual(plan["commands"][1]["argv"][target_index + 1], "test-target")
        self.assertNotIn("--extern", plan["commands"][1]["argv"])
        self.assertEqual(
            plan["commands"][1]["runtimeLink"]["descriptorPath"],
            str(root / "generated" / ".gors-link.json"),
        )
        self.assertEqual(
            plan["commands"][1]["runtimeLink"]["expectedCompatibilityIdentity"],
            "6" * 64,
        )
        self.assertEqual(
            plan["commands"][1]["runtimeLink"]["expectedTargetLibdirRecordSha256"],
            "c" * 64,
        )
        self.assertTrue(plan["runtimeLinkRequired"])
        self.assertEqual(
            plan["runtimeLinkDescriptorSchema"], LINK_DESCRIPTOR_SCHEMA_VERSION
        )
        self.assertEqual(plan["runtimeLinkValidation"], RUNTIME_LINK_VALIDATION)


class AcceptanceGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.commit = "a" * 40
        self.now = dt.datetime(2026, 7, 22, 13, 0, tzinfo=dt.UTC)
        self.baseline = synthetic_result(commit=self.commit, p50=90.0, p95=95.0)
        self.baseline_path = self.root / "evidence" / "baseline.json"
        write_json(self.baseline_path, self.baseline)
        self.current = synthetic_result(commit=self.commit)
        self.current_path = self.root / "current.json"
        write_json(self.current_path, self.current)
        self.acceptance = {
            "schemaVersion": 1,
            "resultSchemaVersion": RESULT_SCHEMA_VERSION,
            "corpusDigest": "1" * 64,
            "maximumCurrentEvidenceAgeHours": 24,
            "promotedScenarios": [
                {
                    "workloadId": "bootstrap_scalar",
                    "scenario": "cold",
                    "corpusDigest": "1" * 64,
                    "hardwareClass": "test-worker-v1",
                    "jobBudget": 4,
                    "artifactDriver": "production-artifact-v1",
                    "configurationFingerprint": self.baseline["configurationFingerprint"],
                    "baselineEvidence": {
                        "path": "evidence/baseline.json",
                        "sha256": sha256_file(self.baseline_path),
                        "resultId": self.baseline["resultId"],
                    },
                    "budgets": {
                        "gorsNormalizedP50Ns": 92.0,
                        "gorsNormalizedP95Ns": 97.0,
                        "maximumPairedRatioCiUpper": 1.0,
                    },
                }
            ],
        }
        self.acceptance_path = self.root / "acceptance.json"
        write_json(self.acceptance_path, self.acceptance)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def gate(self, result: Path | None = None) -> dict:
        return gate_acceptance(
            acceptance_path=self.acceptance_path,
            current_result_path=result,
            repository_root=self.root,
            expected_commit=self.commit,
            now=self.now,
        )

    def test_promoted_gate_refuses_missing_current_evidence(self) -> None:
        with self.assertRaisesRegex(EvidenceError, "require current"):
            self.gate(None)

    def test_promoted_gate_accepts_fresh_evidence_inside_locked_budgets(self) -> None:
        report = self.gate(self.current_path)
        self.assertTrue(report["passed"])
        self.assertEqual(report["checked"], 1)

    def test_promoted_budget_regression_fails(self) -> None:
        regressed = synthetic_result(commit=self.commit, p50=93.0, p95=96.0)
        regressed_path = self.root / "regressed.json"
        write_json(regressed_path, regressed)
        with self.assertRaisesRegex(EvidenceError, "p50 budget regressed"):
            self.gate(regressed_path)

    def test_stale_commit_fails(self) -> None:
        stale = copy.deepcopy(self.current)
        stale["repository"]["commit"] = "b" * 40
        stale["resultId"] = result_id(stale)
        stale_path = self.root / "stale.json"
        write_json(stale_path, stale)
        with self.assertRaisesRegex(EvidenceError, "stale for this repository commit"):
            self.gate(stale_path)

    def test_smoke_evidence_cannot_be_promoted_even_if_flag_is_tampered(self) -> None:
        smoke = copy.deepcopy(self.current)
        smoke["protocol"]["smoke"] = True
        smoke["resultId"] = result_id(smoke)
        smoke_path = self.root / "smoke.json"
        write_json(smoke_path, smoke)
        with self.assertRaisesRegex(EvidenceError, "smoke evidence"):
            self.gate(smoke_path)


if __name__ == "__main__":
    unittest.main()
