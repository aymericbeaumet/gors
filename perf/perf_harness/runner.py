from __future__ import annotations

import datetime as dt
import hashlib
import json
import math
import os
import platform
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .model import (
    REQUIRED_SCENARIOS,
    RESULT_SCHEMA_VERSION,
    achievement,
    configuration_fingerprint,
    result_id,
    summarize_pairs,
    validate_corpus,
)
from .native import (
    ARTIFACT_DRIVER,
    ARTIFACT_DRIVER_PRODUCTION,
    command_output,
    discover_toolchains,
    measure_plan,
    pipeline_plan,
    validate_behavior,
)


MINIMUM_SAMPLES = 50
MINIMUM_SESSIONS = 3
CALIBRATION_VERSION = "python-sha256-fsync-v1"


@dataclass(frozen=True)
class RunOptions:
    repository_root: Path
    corpus_manifest: Path
    output_path: Path
    run_root: Path
    mode: str
    samples: int
    sessions: int
    seed: int
    smoke: bool
    dedicated: bool
    hardware_class: str
    job_budget: int
    go_experiment: str
    gors_path: Path | None = None
    go_path: Path | None = None
    rustc_path: Path | None = None


def run_calibration(root: Path) -> dict[str, Any]:
    root.mkdir(parents=True, exist_ok=True)
    payload = bytes(range(256)) * 32
    started = time.perf_counter_ns()
    digest = hashlib.sha256()
    for index in range(2048):
        digest.update(payload)
        digest.update(index.to_bytes(4, "little"))
    calibration_file = root / "calibration.bin"
    content = digest.digest() * (4 * 1024 * 1024 // digest.digest_size)
    with calibration_file.open("wb") as handle:
        handle.write(content)
        handle.flush()
        os.fsync(handle.fileno())
    read_digest = hashlib.sha256(calibration_file.read_bytes()).hexdigest()
    calibration_file.unlink()
    ended = time.perf_counter_ns()
    return {
        "version": CALIBRATION_VERSION,
        "durationNs": ended - started,
        "outputDigest": read_digest,
        "bytesWritten": len(content),
        "bytesRead": len(content),
    }


def hardware_facts() -> dict[str, Any]:
    memory_bytes = None
    if sys.platform == "darwin":
        completed = subprocess.run(
            ["sysctl", "-n", "hw.memsize"], stdout=subprocess.PIPE, check=False
        )
        if completed.returncode == 0:
            memory_bytes = int(completed.stdout.strip())
    elif Path("/proc/meminfo").is_file():
        match = re.search(r"^MemTotal:\s+(\d+) kB$", Path("/proc/meminfo").read_text(), re.M)
        if match:
            memory_bytes = int(match.group(1)) * 1024
    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "logicalCores": os.cpu_count(),
        "memoryBytes": memory_bytes,
        "pythonVersion": platform.python_version(),
    }


def repository_state(root: Path) -> dict[str, Any]:
    commit = command_output(["git", "rev-parse", "HEAD"], cwd=root)
    status = command_output(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=root)
    return {
        "commit": commit,
        "dirty": bool(status),
        "statusSha256": hashlib.sha256(status.encode()).hexdigest(),
    }


def runtime_contract_identity(gors_version: str) -> str:
    match = re.search(r"\bruntime-contract=([0-9a-f]{64})\b", gors_version)
    if match is None:
        raise RuntimeError("gors version does not report a runtime contract identity")
    return match.group(1)


def distribute_samples(samples: int, sessions: int) -> list[int]:
    base, remainder = divmod(samples, sessions)
    return [base + (1 if index < remainder else 0) for index in range(sessions)]


class PerformanceRun:
    def __init__(self, options: RunOptions):
        self.options = options
        self.corpus = validate_corpus(options.corpus_manifest)
        self.toolchains = discover_toolchains(
            repository_root=options.repository_root,
            gors_path=options.gors_path,
            go_path=options.go_path,
            rustc_path=options.rustc_path,
        )
        self.repository = repository_state(options.repository_root)

    def _copy_source(self, workload: dict[str, Any], revision: str, destination: Path) -> None:
        field = "baseSource" if revision == "base" else "leafSource"
        source = self.options.corpus_manifest.parent / workload[field]
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)

    def _measure_pair(
        self,
        *,
        workload: dict[str, Any],
        scenario: str,
        pair_root: Path,
        order: list[str],
    ) -> dict[str, Any]:
        sides = {compiler: pair_root / compiler for compiler in ("gors", "go")}
        sources = {compiler: sides[compiler] / "main.go" for compiler in sides}
        artifacts: dict[str, Path] = {}
        setup_validation = None
        for compiler in ("gors", "go"):
            self._copy_source(workload, "base", sources[compiler])
            plan, artifact = pipeline_plan(
                compiler,
                side_root=sides[compiler],
                source=sources[compiler],
                toolchains=self.toolchains,
                job_budget=self.options.job_budget,
                go_experiment=self.options.go_experiment,
            )
            artifacts[compiler] = artifact
            if scenario in ("no_op", "leaf_edit"):
                measure_plan(plan)
        if scenario == "leaf_edit":
            setup_validation = validate_behavior(
                artifacts["gors"], artifacts["go"], workload["baseBehavior"]
            )
            if not setup_validation["passed"]:
                raise RuntimeError("leaf-edit setup artifacts failed base behavior validation")
            for compiler in ("gors", "go"):
                self._copy_source(workload, "leaf", sources[compiler])

        measurements: dict[str, dict[str, Any]] = {}
        for compiler in order:
            plan, artifact = pipeline_plan(
                compiler,
                side_root=sides[compiler],
                source=sources[compiler],
                toolchains=self.toolchains,
                job_budget=self.options.job_budget,
                go_experiment=self.options.go_experiment,
            )
            artifacts[compiler] = artifact
            measurements[compiler] = measure_plan(plan)
        behavior_spec = (
            workload["leafBehavior"] if scenario == "leaf_edit" else workload["baseBehavior"]
        )
        behavior = validate_behavior(artifacts["gors"], artifacts["go"], behavior_spec)
        if not behavior["passed"]:
            raise RuntimeError(
                f"behavior validation failed for {workload['id']}:{scenario} in {pair_root}"
            )
        return {
            "order": order,
            "gors": measurements["gors"],
            "go": measurements["go"],
            "behavior": behavior,
            "setupValidation": setup_validation,
        }

    def _measurement(self, workload: dict[str, Any], scenario: str) -> dict[str, Any]:
        rng = random.Random(f"{self.options.seed}:{workload['id']}:{scenario}")
        session_counts = distribute_samples(self.options.samples, self.options.sessions)
        sessions: list[dict[str, Any]] = []
        for session_index, count in enumerate(session_counts):
            session_root = (
                self.options.run_root
                / workload["id"]
                / scenario
                / f"session-{session_index + 1:02d}"
            )
            before = run_calibration(session_root / "calibration-before")
            samples: list[dict[str, Any]] = []
            for pair_index in range(count):
                order = ["gors", "go"]
                rng.shuffle(order)
                sample = self._measure_pair(
                    workload=workload,
                    scenario=scenario,
                    pair_root=session_root / f"pair-{pair_index + 1:03d}",
                    order=order,
                )
                sample["pairIndex"] = pair_index + 1
                samples.append(sample)
            after = run_calibration(session_root / "calibration-after")
            center = math.sqrt(before["durationNs"] * after["durationNs"])
            drift = abs(after["durationNs"] / before["durationNs"] - 1.0)
            sessions.append(
                {
                    "sessionIndex": session_index + 1,
                    "calibration": {
                        "before": before,
                        "after": after,
                        "centerNs": center,
                        "driftFraction": drift,
                        "healthPassed": drift <= 0.10,
                        "normalizationFactor": None,
                    },
                    "samples": samples,
                }
            )
        reference = median([session["calibration"]["centerNs"] for session in sessions])
        flattened: list[dict[str, Any]] = []
        for session in sessions:
            factor = session["calibration"]["centerNs"] / reference
            session["calibration"]["normalizationFactor"] = factor
            for sample in session["samples"]:
                for compiler in ("gors", "go"):
                    sample[compiler]["normalizedWallNs"] = sample[compiler]["wallNs"] / factor
                flattened.append(sample)
        summary = {
            "raw": summarize_pairs(flattened, seed=self.options.seed, duration_field="wallNs"),
            "normalized": summarize_pairs(flattened, seed=self.options.seed),
        }
        validation_passed = all(sample["behavior"]["passed"] for sample in flattened)
        host_health_passed = all(
            session["calibration"]["healthPassed"] for session in sessions
        )
        return {
            "workloadId": workload["id"],
            "scenario": scenario,
            "validation": {
                "passed": validation_passed,
                "sampleCount": len(flattened),
                "leafSentinelRequired": scenario == "leaf_edit",
            },
            "hostHealthPassed": host_health_passed,
            "sessions": sessions,
            "summary": summary,
            "achievement": achievement(
                summary["normalized"], validation_passed and host_health_passed
            ),
        }

    def execute(self) -> dict[str, Any]:
        measurements = [
            self._measurement(workload, scenario)
            for workload in self.corpus["workloads"]
            for scenario in REQUIRED_SCENARIOS
        ]
        protocol_blockers: list[str] = []
        if self.options.mode != "certification":
            protocol_blockers.append("baseline mode cannot promote")
        if self.options.smoke:
            protocol_blockers.append("smoke overrides cannot promote")
        if self.options.samples < MINIMUM_SAMPLES:
            protocol_blockers.append(f"fewer than {MINIMUM_SAMPLES} paired samples per scenario")
        if self.options.sessions < MINIMUM_SESSIONS:
            protocol_blockers.append(f"fewer than {MINIMUM_SESSIONS} independent sessions")
        if not self.options.dedicated:
            protocol_blockers.append("worker was not declared dedicated")
        if self.options.hardware_class == "unclassified-local":
            protocol_blockers.append("hardware class is unclassified")
        if self.repository["dirty"]:
            protocol_blockers.append("repository worktree is dirty")
        if not ARTIFACT_DRIVER_PRODUCTION:
            protocol_blockers.append(
                "bootstrap artifact driver is not the default user-facing artifact command"
            )
        protocol_blockers.append("exact process byte-I/O counters are unavailable")
        protocol_blockers.append("exact process-tree counts are unavailable")
        protocol_blockers.append("semantic stage fingerprints are unavailable")
        if not any(item["achievement"]["achieved"] for item in measurements):
            protocol_blockers.append("no measured scenario achieved the competitive thresholds")
        claim_blockers = list(protocol_blockers)
        unsupported = [
            entry["id"] for entry in self.corpus["scenarioSupport"] if not entry["supported"]
        ]
        if unsupported:
            claim_blockers.append(
                "mandatory dependency scenarios remain unsupported: " + ", ".join(unsupported)
            )
        if not all(item["achievement"]["achieved"] for item in measurements):
            claim_blockers.append("not every measured workload and scenario achieved the target")
        result = self._result(measurements, protocol_blockers, claim_blockers)
        result["configurationFingerprint"] = configuration_fingerprint(result)
        result["resultId"] = result_id(result)
        return result

    def _result(
        self,
        measurements: list[dict[str, Any]],
        protocol_blockers: list[str],
        claim_blockers: list[str],
    ) -> dict[str, Any]:
        toolchains = self.toolchains
        return {
            "schemaVersion": RESULT_SCHEMA_VERSION,
            "kind": "gors.performance.result",
            "resultId": "",
            "configurationFingerprint": "",
            "createdAt": dt.datetime.now(dt.UTC).isoformat().replace("+00:00", "Z"),
            "mode": self.options.mode,
            "promotionEligible": not protocol_blockers,
            "promotionBlockers": protocol_blockers,
            "fullClaimEligible": not claim_blockers,
            "fullClaimBlockers": claim_blockers,
            "repository": self.repository,
            "corpus": {
                "version": self.corpus["corpusVersion"],
                "digest": self.corpus["digest"],
                "scenarioSupport": self.corpus["scenarioSupport"],
            },
            "protocol": {
                "lane": "end_to_end_artifact",
                "artifactDriver": ARTIFACT_DRIVER,
                "artifactDriverProduction": ARTIFACT_DRIVER_PRODUCTION,
                "interval": "before gors process through atomic executable publication",
                "executionTimed": False,
                "samples": self.options.samples,
                "sessions": self.options.sessions,
                "seed": self.options.seed,
                "smoke": self.options.smoke,
                "minimumPromotionSamples": MINIMUM_SAMPLES,
                "minimumPromotionSessions": MINIMUM_SESSIONS,
                "calibrationVersion": CALIBRATION_VERSION,
                "gorsCacheSchema": 3,
                "ioByteCountersAvailable": False,
                "processTreeCountersAvailable": False,
                "stageFingerprintsAvailable": False,
            },
            "environment": {
                "hardwareClass": self.options.hardware_class,
                "dedicated": self.options.dedicated,
                "jobBudget": self.options.job_budget,
                "facts": hardware_facts(),
            },
            "toolchains": {
                "gors": {
                    "path": str(toolchains.gors),
                    "version": toolchains.gors_version,
                    "sha256": hashlib.sha256(toolchains.gors.read_bytes()).hexdigest(),
                    "runtimeContractIdentity": runtime_contract_identity(
                        toolchains.gors_version
                    ),
                },
                "go": {
                    "path": str(toolchains.go),
                    "goroot": str(toolchains.goroot),
                    "version": toolchains.go_version,
                    "sha256": hashlib.sha256(toolchains.go.read_bytes()).hexdigest(),
                    "experiment": self.options.go_experiment,
                },
                "rustc": {
                    "path": str(toolchains.rustc),
                    "channel": toolchains.rust_channel,
                    "versionVerbose": toolchains.rustc_version,
                    "target": toolchains.rust_target,
                    "sha256": hashlib.sha256(toolchains.rustc.read_bytes()).hexdigest(),
                },
                "linker": {
                    "path": toolchains.linker_path,
                    "version": toolchains.linker_version,
                    "sha256": toolchains.linker_sha256,
                },
            },
            "workloads": measurements,
        }


def median(values: list[float]) -> float:
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return float(ordered[middle])
    return float((ordered[middle - 1] + ordered[middle]) / 2)


def write_result(path: Path, result: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=path.parent, delete=False
    ) as handle:
        json.dump(result, handle, indent=2, sort_keys=True)
        handle.write("\n")
        temporary = Path(handle.name)
    os.replace(temporary, path)
