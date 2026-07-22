from __future__ import annotations

import copy
import datetime as dt
import hashlib
import json
import math
import random
import statistics
from pathlib import Path
from typing import Any


RESULT_SCHEMA_VERSION = 1
ACCEPTANCE_SCHEMA_VERSION = 1
CORPUS_SCHEMA_VERSION = 1
REQUIRED_SCENARIOS = ("cold", "no_op", "leaf_edit")
UNSUPPORTED_DEPENDENCY_SCENARIOS = ("dependency_body_edit", "dependency_api_edit")


class EvidenceError(ValueError):
    """Raised when performance evidence is incomplete, stale, or incompatible."""


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceError(f"{path} must contain a JSON object")
    return value


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    try:
        return sha256_bytes(path.read_bytes())
    except OSError as error:
        raise EvidenceError(f"cannot hash {path}: {error}") from error


def corpus_digest(manifest_path: Path) -> str:
    manifest = read_json(manifest_path)
    digest_input = copy.deepcopy(manifest)
    digest_input.pop("digest", None)
    hasher = hashlib.sha256(b"gors-performance-corpus-v1\0")
    hasher.update(canonical_json(digest_input))
    root = manifest_path.parent.resolve()
    sources: set[str] = set()
    for workload in manifest.get("workloads", []):
        if not isinstance(workload, dict):
            raise EvidenceError("corpus workloads must be objects")
        for field in ("baseSource", "leafSource"):
            source = workload.get(field)
            if not isinstance(source, str) or not source:
                raise EvidenceError(f"corpus workload is missing {field}")
            sources.add(source)
    for relative in sorted(sources):
        path = (root / relative).resolve()
        if root not in path.parents:
            raise EvidenceError(f"corpus source escapes its root: {relative}")
        try:
            content = path.read_bytes()
        except OSError as error:
            raise EvidenceError(f"cannot read corpus source {relative}: {error}") from error
        hasher.update(relative.encode())
        hasher.update(b"\0")
        hasher.update(content)
        hasher.update(b"\0")
    return hasher.hexdigest()


def validate_corpus(manifest_path: Path) -> dict[str, Any]:
    manifest = read_json(manifest_path)
    if manifest.get("schemaVersion") != CORPUS_SCHEMA_VERSION:
        raise EvidenceError("unsupported corpus schemaVersion")
    expected_digest = corpus_digest(manifest_path)
    if manifest.get("digest") != expected_digest:
        raise EvidenceError(
            f"corpus digest mismatch: expected {expected_digest}, got {manifest.get('digest')}"
        )
    workloads = manifest.get("workloads")
    if not isinstance(workloads, list) or not workloads:
        raise EvidenceError("corpus must contain at least one workload")
    ids = [workload.get("id") for workload in workloads if isinstance(workload, dict)]
    if len(ids) != len(set(ids)) or any(not isinstance(item, str) or not item for item in ids):
        raise EvidenceError("corpus workload IDs must be unique non-empty strings")
    support = manifest.get("scenarioSupport")
    if not isinstance(support, list):
        raise EvidenceError("corpus scenarioSupport must be an array")
    support_by_id = {
        item.get("id"): item for item in support if isinstance(item, dict) and item.get("id")
    }
    for scenario in REQUIRED_SCENARIOS:
        if support_by_id.get(scenario, {}).get("supported") is not True:
            raise EvidenceError(f"required bootstrap scenario {scenario} must be supported")
    for scenario in UNSUPPORTED_DEPENDENCY_SCENARIOS:
        entry = support_by_id.get(scenario, {})
        if entry.get("supported") is not False or not entry.get("reason"):
            raise EvidenceError(f"{scenario} must carry an explicit unsupported reason")
    return manifest


def percentile(values: list[float], quantile: float) -> float:
    if not values:
        raise EvidenceError("cannot compute a percentile of no samples")
    if not 0.0 <= quantile <= 1.0:
        raise EvidenceError("quantile must be between zero and one")
    ordered = sorted(values)
    position = (len(ordered) - 1) * quantile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return float(ordered[lower])
    fraction = position - lower
    return float(ordered[lower] * (1.0 - fraction) + ordered[upper] * fraction)


def bootstrap_median_ci(
    values: list[float], *, seed: int, iterations: int = 10_000
) -> dict[str, float | int]:
    if not values:
        raise EvidenceError("cannot bootstrap no paired ratios")
    if iterations < 1:
        raise EvidenceError("bootstrap iterations must be positive")
    rng = random.Random(seed)
    count = len(values)
    medians = [
        statistics.median(values[rng.randrange(count)] for _ in range(count))
        for _ in range(iterations)
    ]
    return {
        "lower": percentile(medians, 0.025),
        "upper": percentile(medians, 0.975),
        "iterations": iterations,
    }


def summarize_pairs(
    samples: list[dict[str, Any]], *, seed: int, duration_field: str = "normalizedWallNs"
) -> dict[str, Any]:
    if not samples:
        raise EvidenceError("cannot summarize no samples")
    gors_values = [float(sample["gors"][duration_field]) for sample in samples]
    go_values = [float(sample["go"][duration_field]) for sample in samples]
    if any(value <= 0 for value in gors_values + go_values):
        raise EvidenceError("sample durations must be positive")
    ratios = [gors / go for gors, go in zip(gors_values, go_values, strict=True)]

    def compiler_summary(values: list[float]) -> dict[str, float]:
        middle = statistics.median(values)
        return {
            "p50Ns": percentile(values, 0.50),
            "p95Ns": percentile(values, 0.95),
            "madNs": statistics.median(abs(value - middle) for value in values),
        }

    return {
        "sampleCount": len(samples),
        "gors": compiler_summary(gors_values),
        "go": compiler_summary(go_values),
        "pairedRatio": {
            "p50": percentile(ratios, 0.50),
            "p95": percentile(ratios, 0.95),
            "bootstrapMedianCi95": bootstrap_median_ci(ratios, seed=seed),
        },
    }


def achievement(summary: dict[str, Any], behavior_passed: bool) -> dict[str, Any]:
    gors = summary["gors"]
    go = summary["go"]
    ci_upper = summary["pairedRatio"]["bootstrapMedianCi95"]["upper"]
    checks = {
        "behavior": behavior_passed,
        "p50AtMost95PercentOfGo": gors["p50Ns"] <= 0.95 * go["p50Ns"],
        "p95AtMost95PercentOfGo": gors["p95Ns"] <= 0.95 * go["p95Ns"],
        "pairedRatioCiUpperBelowOne": ci_upper < 1.0,
    }
    return {"achieved": all(checks.values()), "checks": checks}


def recompute_measurement(measurement: dict[str, Any], *, seed: int) -> dict[str, Any]:
    """Rebuild normalized evidence from raw timings and calibration records."""
    sessions = measurement.get("sessions")
    if not isinstance(sessions, list) or not sessions:
        raise EvidenceError("cannot recompute a measurement without raw sessions")

    centers: list[float] = []
    session_health: list[bool] = []
    for session in sessions:
        calibration = session.get("calibration", {})
        before = float(calibration.get("before", {}).get("durationNs", 0))
        after = float(calibration.get("after", {}).get("durationNs", 0))
        if before <= 0 or after <= 0:
            raise EvidenceError("calibration durations must be positive")
        center = math.sqrt(before * after)
        drift = abs(after / before - 1.0)
        healthy = drift <= 0.10
        if calibration.get("centerNs") != center:
            raise EvidenceError("stored calibration center does not match raw durations")
        if calibration.get("driftFraction") != drift:
            raise EvidenceError("stored calibration drift does not match raw durations")
        if calibration.get("healthPassed") is not healthy:
            raise EvidenceError("stored calibration health does not match raw durations")
        centers.append(center)
        session_health.append(healthy)

    reference = float(statistics.median(centers))
    flattened: list[dict[str, Any]] = []
    for session, center in zip(sessions, centers, strict=True):
        calibration = session["calibration"]
        factor = center / reference
        if calibration.get("normalizationFactor") != factor:
            raise EvidenceError("stored normalization factor does not match calibration")
        samples = session.get("samples")
        if not isinstance(samples, list):
            raise EvidenceError("raw performance session samples must be an array")
        for sample in samples:
            pair: dict[str, dict[str, float]] = {}
            for compiler in ("gors", "go"):
                measurement_side = sample.get(compiler, {})
                wall_ns = float(measurement_side.get("wallNs", 0))
                if wall_ns <= 0:
                    raise EvidenceError("raw performance durations must be positive")
                normalized = wall_ns / factor
                if measurement_side.get("normalizedWallNs") != normalized:
                    raise EvidenceError(
                        "stored normalized duration does not match raw timing and calibration"
                    )
                pair[compiler] = {
                    "wallNs": wall_ns,
                    "normalizedWallNs": normalized,
                }
            flattened.append(pair)

    summary = {
        "raw": summarize_pairs(flattened, seed=seed, duration_field="wallNs"),
        "normalized": summarize_pairs(flattened, seed=seed),
    }
    behavior_passed = measurement.get("validation", {}).get("passed") is True
    host_health_passed = all(session_health)
    return {
        "summary": summary,
        "hostHealthPassed": host_health_passed,
        "achievement": achievement(
            summary["normalized"], behavior_passed and host_health_passed
        ),
    }


def result_id(result: dict[str, Any]) -> str:
    value = copy.deepcopy(result)
    value.pop("resultId", None)
    return sha256_bytes(b"gors-performance-result-v1\0" + canonical_json(value))


def configuration_fingerprint(result: dict[str, Any]) -> str:
    protocol = result.get("protocol", {})
    environment = result.get("environment", {})
    toolchains = result.get("toolchains", {})
    value = {
        "resultSchemaVersion": result.get("schemaVersion"),
        "corpusDigest": result.get("corpus", {}).get("digest"),
        "artifactDriver": protocol.get("artifactDriver"),
        "calibrationVersion": protocol.get("calibrationVersion"),
        "gorsCacheSchema": protocol.get("gorsCacheSchema"),
        "ioByteCountersAvailable": protocol.get("ioByteCountersAvailable"),
        "processTreeCountersAvailable": protocol.get("processTreeCountersAvailable"),
        "stageFingerprintsAvailable": protocol.get("stageFingerprintsAvailable"),
        "hardwareClass": environment.get("hardwareClass"),
        "jobBudget": environment.get("jobBudget"),
        "go": {
            "version": toolchains.get("go", {}).get("version"),
            "sha256": toolchains.get("go", {}).get("sha256"),
            "experiment": toolchains.get("go", {}).get("experiment"),
        },
        "rustc": {
            "channel": toolchains.get("rustc", {}).get("channel"),
            "target": toolchains.get("rustc", {}).get("target"),
            "sha256": toolchains.get("rustc", {}).get("sha256"),
        },
        "runtimeAbiVersion": toolchains.get("gors", {}).get("runtimeAbiVersion"),
        "linker": {
            "version": toolchains.get("linker", {}).get("version"),
            "sha256": toolchains.get("linker", {}).get("sha256"),
        },
    }
    return sha256_bytes(b"gors-performance-configuration-v1\0" + canonical_json(value))


def validate_result(result: dict[str, Any]) -> None:
    if result.get("schemaVersion") != RESULT_SCHEMA_VERSION:
        raise EvidenceError("unsupported result schemaVersion")
    if result.get("kind") != "gors.performance.result":
        raise EvidenceError("unexpected result kind")
    if result.get("resultId") != result_id(result):
        raise EvidenceError("performance resultId does not match its content")
    if result.get("configurationFingerprint") != configuration_fingerprint(result):
        raise EvidenceError("performance configuration fingerprint does not match its inputs")
    protocol = result.get("protocol")
    if not isinstance(protocol, dict):
        raise EvidenceError("result protocol is missing")
    if protocol.get("samples") is None or protocol.get("sessions") is None:
        raise EvidenceError("result protocol must record sample and session counts")
    seed = protocol.get("seed")
    if not isinstance(seed, int):
        raise EvidenceError("result protocol must record an integer random seed")
    workloads = result.get("workloads")
    if not isinstance(workloads, list) or not workloads:
        raise EvidenceError("result workloads are missing")
    for workload in workloads:
        if not isinstance(workload, dict):
            raise EvidenceError("result workload entries must be objects")
        if workload.get("scenario") not in REQUIRED_SCENARIOS:
            raise EvidenceError("result contains an unexpected measured scenario")
        sessions = workload.get("sessions")
        if not isinstance(sessions, list) or not sessions:
            raise EvidenceError("each measured workload must retain raw sessions")
        if len(sessions) != protocol["sessions"]:
            raise EvidenceError("raw session count does not match the protocol")
        raw_count = sum(len(session.get("samples", [])) for session in sessions)
        summary = workload.get("summary", {})
        for lane in ("raw", "normalized"):
            if raw_count != summary.get(lane, {}).get("sampleCount"):
                raise EvidenceError(f"raw sample count does not match the {lane} summary")
        if raw_count != protocol["samples"]:
            raise EvidenceError("raw sample count does not match the protocol")
        if not workload.get("validation", {}).get("passed"):
            raise EvidenceError("result contains failed behavior validation")
        recomputed = recompute_measurement(workload, seed=seed)
        for field in ("summary", "hostHealthPassed", "achievement"):
            if workload.get(field) != recomputed[field]:
                raise EvidenceError(
                    f"stored performance {field} does not match recomputed raw evidence"
                )


def validate_acceptance(manifest: dict[str, Any]) -> None:
    if manifest.get("schemaVersion") != ACCEPTANCE_SCHEMA_VERSION:
        raise EvidenceError("unsupported acceptance schemaVersion")
    if manifest.get("resultSchemaVersion") != RESULT_SCHEMA_VERSION:
        raise EvidenceError("acceptance manifest targets an incompatible result schema")
    promoted = manifest.get("promotedScenarios")
    if not isinstance(promoted, list):
        raise EvidenceError("acceptance promotedScenarios must be an array")
    keys: set[tuple[str, str]] = set()
    for entry in promoted:
        if not isinstance(entry, dict):
            raise EvidenceError("promoted scenario entries must be objects")
        key = (entry.get("workloadId"), entry.get("scenario"))
        if key in keys:
            raise EvidenceError(f"duplicate promoted scenario {key}")
        keys.add(key)
        if entry.get("scenario") not in REQUIRED_SCENARIOS:
            raise EvidenceError(f"unsupported promoted scenario {entry.get('scenario')}")
        if entry.get("corpusDigest") != manifest.get("corpusDigest"):
            raise EvidenceError(f"promoted scenario {key} has a stale corpus digest")
        if not entry.get("baselineEvidence", {}).get("path"):
            raise EvidenceError(f"promoted scenario {key} has no baseline evidence path")
        fingerprint = entry.get("configurationFingerprint")
        if not isinstance(fingerprint, str) or len(fingerprint) != 64:
            raise EvidenceError(f"promoted scenario {key} has no configuration fingerprint")
        budgets = entry.get("budgets", {})
        for field in ("gorsNormalizedP50Ns", "gorsNormalizedP95Ns"):
            if not isinstance(budgets.get(field), (int, float)) or budgets[field] <= 0:
                raise EvidenceError(f"promoted scenario {key} has invalid {field}")


def parse_timestamp(value: Any) -> dt.datetime:
    if not isinstance(value, str):
        raise EvidenceError("evidence createdAt must be an ISO-8601 string")
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise EvidenceError(f"invalid evidence timestamp {value}") from error
    if parsed.tzinfo is None:
        raise EvidenceError("evidence timestamp must include a timezone")
    return parsed.astimezone(dt.UTC)


def find_measurement(result: dict[str, Any], workload_id: str, scenario: str) -> dict[str, Any]:
    for measurement in result.get("workloads", []):
        if measurement.get("workloadId") == workload_id and measurement.get("scenario") == scenario:
            return measurement
    raise EvidenceError(f"evidence is missing {workload_id}:{scenario}")


def require_promotion_protocol(result: dict[str, Any], label: str) -> None:
    if result.get("mode") != "certification" or not result.get("promotionEligible"):
        raise EvidenceError(f"{label} is not promotion-eligible certification evidence")
    protocol = result["protocol"]
    environment = result["environment"]
    if protocol.get("smoke"):
        raise EvidenceError(f"{label} is smoke evidence")
    if protocol.get("samples", 0) < 50 or protocol.get("sessions", 0) < 3:
        raise EvidenceError(f"{label} does not satisfy the 50-sample/3-session protocol")
    if not protocol.get("artifactDriverProduction"):
        raise EvidenceError(f"{label} uses a non-production artifact driver")
    if not protocol.get("ioByteCountersAvailable"):
        raise EvidenceError(f"{label} has no exact byte-I/O counters")
    if not protocol.get("processTreeCountersAvailable"):
        raise EvidenceError(f"{label} has no exact process-tree counters")
    if not protocol.get("stageFingerprintsAvailable"):
        raise EvidenceError(f"{label} has no stage fingerprints")
    if not environment.get("dedicated"):
        raise EvidenceError(f"{label} was not collected on a dedicated worker")
    if result.get("repository", {}).get("dirty"):
        raise EvidenceError(f"{label} was collected from a dirty worktree")


def gate_acceptance(
    *,
    acceptance_path: Path,
    current_result_path: Path | None,
    repository_root: Path,
    expected_commit: str,
    now: dt.datetime | None = None,
) -> dict[str, Any]:
    manifest = read_json(acceptance_path)
    validate_acceptance(manifest)
    promoted = manifest["promotedScenarios"]
    if not promoted:
        return {"passed": True, "checked": 0, "message": "no scenarios are promoted"}
    if current_result_path is None:
        raise EvidenceError("promoted scenarios require current certification evidence")
    current = read_json(current_result_path)
    validate_result(current)
    require_promotion_protocol(current, "current evidence")
    if current.get("repository", {}).get("commit") != expected_commit:
        raise EvidenceError("current evidence is stale for this repository commit")
    maximum_age = manifest.get("maximumCurrentEvidenceAgeHours", 24)
    checked_at = (now or dt.datetime.now(dt.UTC)).astimezone(dt.UTC)
    age = checked_at - parse_timestamp(current.get("createdAt"))
    if age < dt.timedelta(0) or age > dt.timedelta(hours=maximum_age):
        raise EvidenceError("current evidence is outside the permitted age window")

    checked = 0
    for entry in promoted:
        key = f"{entry['workloadId']}:{entry['scenario']}"
        evidence_ref = entry["baselineEvidence"]
        baseline_path = (repository_root / evidence_ref["path"]).resolve()
        if repository_root.resolve() not in baseline_path.parents:
            raise EvidenceError(f"baseline evidence for {key} escapes the repository")
        if not baseline_path.is_file():
            raise EvidenceError(f"baseline evidence for {key} is missing")
        if sha256_file(baseline_path) != evidence_ref.get("sha256"):
            raise EvidenceError(f"baseline evidence for {key} has a stale digest")
        baseline = read_json(baseline_path)
        validate_result(baseline)
        if baseline.get("resultId") != evidence_ref.get("resultId"):
            raise EvidenceError(f"baseline evidence for {key} has a stale result ID")
        require_promotion_protocol(baseline, f"baseline evidence for {key}")
        if entry.get("configurationFingerprint") != current.get("configurationFingerprint"):
            raise EvidenceError(f"current toolchain configuration changed for {key}")
        if entry.get("configurationFingerprint") != baseline.get("configurationFingerprint"):
            raise EvidenceError(f"baseline toolchain configuration changed for {key}")

        for field, actual in (
            ("corpusDigest", current.get("corpus", {}).get("digest")),
            ("hardwareClass", current.get("environment", {}).get("hardwareClass")),
            ("jobBudget", current.get("environment", {}).get("jobBudget")),
            ("artifactDriver", current.get("protocol", {}).get("artifactDriver")),
        ):
            if entry.get(field) != actual:
                raise EvidenceError(f"current evidence changed {field} for {key}")

        measurement = find_measurement(current, entry["workloadId"], entry["scenario"])
        if not measurement.get("validation", {}).get("passed"):
            raise EvidenceError(f"behavior validation failed for {key}")
        summary = measurement["summary"]["normalized"]
        budgets = entry["budgets"]
        baseline_measurement = find_measurement(
            baseline, entry["workloadId"], entry["scenario"]
        )
        baseline_behavior = baseline_measurement.get("validation", {}).get("passed", False)
        baseline_behavior = baseline_behavior and baseline_measurement.get(
            "hostHealthPassed", False
        )
        baseline_summary = baseline_measurement["summary"]["normalized"]
        if not achievement(baseline_summary, baseline_behavior)["achieved"]:
            raise EvidenceError(f"baseline never achieved the competitive target for {key}")
        if budgets["gorsNormalizedP50Ns"] > baseline_summary["gors"]["p50Ns"] * 1.03:
            raise EvidenceError(f"locked p50 budget exceeds promotion headroom for {key}")
        if budgets["gorsNormalizedP95Ns"] > baseline_summary["gors"]["p95Ns"] * 1.03:
            raise EvidenceError(f"locked p95 budget exceeds promotion headroom for {key}")
        if budgets["gorsNormalizedP50Ns"] > baseline_summary["go"]["p50Ns"]:
            raise EvidenceError(f"locked p50 budget exceeds the pinned Go baseline for {key}")
        if budgets["gorsNormalizedP95Ns"] > baseline_summary["go"]["p95Ns"]:
            raise EvidenceError(f"locked p95 budget exceeds the pinned Go baseline for {key}")
        if summary["gors"]["p50Ns"] > budgets["gorsNormalizedP50Ns"]:
            raise EvidenceError(f"promoted p50 budget regressed for {key}")
        if summary["gors"]["p95Ns"] > budgets["gorsNormalizedP95Ns"]:
            raise EvidenceError(f"promoted p95 budget regressed for {key}")
        if (
            summary["pairedRatio"]["bootstrapMedianCi95"]["upper"]
            >= budgets.get("maximumPairedRatioCiUpper", 1.0)
        ):
            raise EvidenceError(f"same-host Go comparison regressed for {key}")
        checked += 1
    return {"passed": True, "checked": checked, "message": "all promoted budgets passed"}
