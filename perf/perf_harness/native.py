from __future__ import annotations

import base64
import hashlib
import json
import math
import multiprocessing
import os
import platform
import re
import resource
import shutil
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .runtime_link import (
    LINK_DESCRIPTOR_SCHEMA_VERSION,
    RUNTIME_LINK_VALIDATION,
    canonical_rustc_release_record,
    canonical_target_libdir_record,
    expand_rustc_runtime_link,
    gors_runtime_contract_identity,
    rust_runtime_compatibility_identity,
    validate_runtime_link_descriptor,
)


ARTIFACT_DRIVER = "bootstrap-generated-rust-v3"
ARTIFACT_DRIVER_PRODUCTION = False
GORS_TIMING_REPORT_VERSION = 5
GORS_BUILD_CACHE_HIT_PHASES = (
    "cli.source_load",
    "cli.cache_lookup",
)
GORS_BUILD_CACHE_MISS_PHASES = (
    "cli.source_load",
    "cli.cache_lookup",
    "cli.compile",
    "cli.print",
    "cli.file_writes",
)


@dataclass(frozen=True)
class Toolchains:
    gors: Path
    go: Path
    goroot: Path
    rustc: Path
    rust_channel: str
    gors_version: str
    go_version: str
    rustc_version: str
    rust_target: str
    rust_pointer_width: int
    rust_endianness: str
    runtime_contract_identity: str
    rustc_target_libdir: Path
    rustc_release_record_sha256: str
    target_libdir_record_sha256: str
    runtime_compatibility_identity: str
    linker_path: str | None
    linker_version: str | None
    linker_sha256: str | None


def command_output(args: list[str], *, cwd: Path) -> str:
    completed = subprocess.run(
        args,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.decode(errors="replace").strip()
        raise RuntimeError(f"command failed ({completed.returncode}): {' '.join(args)}\n{stderr}")
    return completed.stdout.decode(errors="replace").strip()


def command_bytes(args: list[str], *, cwd: Path) -> bytes:
    completed = subprocess.run(
        args,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.decode(errors="replace").strip()
        raise RuntimeError(f"command failed ({completed.returncode}): {' '.join(args)}\n{stderr}")
    return completed.stdout


def host_go_platform() -> tuple[str, str]:
    os_name = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}.get(
        platform.system()
    )
    architecture = {
        "arm64": "arm64",
        "aarch64": "arm64",
        "AMD64": "amd64",
        "x86_64": "amd64",
    }.get(platform.machine())
    if os_name is None or architecture is None:
        raise RuntimeError(
            f"unsupported native performance host {platform.system()}/{platform.machine()}"
        )
    return os_name, architecture


def discover_toolchains(
    *,
    repository_root: Path,
    gors_path: Path | None,
    go_path: Path | None,
    rustc_path: Path | None,
) -> Toolchains:
    root = repository_root
    if gors_path is None:
        build_environment = os.environ.copy()
        for key in ("CARGO_ENCODED_RUSTFLAGS", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS"):
            build_environment.pop(key, None)
        build_environment["CARGO_INCREMENTAL"] = "0"
        subprocess.run(
            ["cargo", "build", "--locked", "--release", "--package", "gors-cli"],
            cwd=root,
            env=build_environment,
            check=True,
        )
        gors = root / "target" / "release" / ("gors.exe" if os.name == "nt" else "gors")
    else:
        gors = gors_path
    if not gors.is_file():
        raise RuntimeError(f"gors executable does not exist: {gors}")

    go_version_pin = (root / ".go-version").read_text(encoding="utf-8").strip()
    go_os, go_arch = host_go_platform()
    if go_path is None:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        goroot = cargo_home / "gors-cache" / f"go{go_version_pin}.{go_os}-{go_arch}" / "go"
        go = goroot / "bin" / ("go.exe" if os.name == "nt" else "go")
    else:
        go = go_path
        goroot = go.parent.parent
    if not go.is_file():
        raise RuntimeError(
            f"repo-pinned Go executable does not exist: {go}; build gors once to install it"
        )

    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))
    rust_channel = str(toolchain["toolchain"]["channel"])
    if rustc_path is None:
        rustc = Path(
            command_output(
                ["rustup", "which", "rustc", "--toolchain", rust_channel], cwd=root
            )
        )
    else:
        rustc = rustc_path
    if not rustc.is_file():
        raise RuntimeError(f"pinned rustc executable does not exist: {rustc}")

    gors_version = command_output([str(gors), "version"], cwd=root)
    go_version_output = command_output([str(go), "version"], cwd=root)
    match = re.search(r"\bgo(\d+\.\d+\.\d+)\b", go_version_output)
    if match is None or match.group(1) != go_version_pin:
        raise RuntimeError(
            f"Go version mismatch: expected {go_version_pin}, got {go_version_output}"
        )
    rustc_verbose_bytes = command_bytes([str(rustc), "-vV"], cwd=root)
    try:
        rustc_verbose = rustc_verbose_bytes.decode("utf-8")
    except UnicodeDecodeError as error:
        raise RuntimeError("pinned rustc -vV output is not UTF-8") from error
    rust_release = next(
        (
            line.removeprefix("release: ")
            for line in rustc_verbose.splitlines()
            if line.startswith("release: ")
        ),
        "unknown",
    )
    rust_target = next(
        (
            line.removeprefix("host: ")
            for line in rustc_verbose.splitlines()
            if line.startswith("host: ")
        ),
        "unknown",
    )
    if rust_target == "unknown":
        raise RuntimeError("pinned rustc -vV output has no host target")
    if rust_release != rust_channel:
        raise RuntimeError(f"Rust version mismatch: expected {rust_channel}, got {rust_release}")
    rust_cfg = command_output(
        [str(rustc), "--print", "cfg", "--target", rust_target], cwd=root
    )
    pointer_match = re.search(r'^target_pointer_width="(\d+)"$', rust_cfg, re.MULTILINE)
    endian_match = re.search(r'^target_endian="(little|big)"$', rust_cfg, re.MULTILINE)
    if pointer_match is None or endian_match is None:
        raise RuntimeError("pinned rustc target cfg lacks pointer-width or endianness facts")
    rust_pointer_width = int(pointer_match.group(1))
    rust_endianness = endian_match.group(1)
    target_libdir_value = command_output(
        [str(rustc), "--target", rust_target, "--print", "target-libdir"], cwd=root
    )
    rustc_target_libdir = Path(target_libdir_value)
    if not rustc_target_libdir.is_absolute() or not rustc_target_libdir.is_dir():
        raise RuntimeError(
            "pinned rustc returned an invalid target-libdir: "
            f"{rustc_target_libdir}"
        )
    rustc_release_record = canonical_rustc_release_record(rustc_verbose_bytes)
    target_libdir_record = canonical_target_libdir_record(rustc_target_libdir)
    runtime_compatibility_identity = rust_runtime_compatibility_identity(
        rustc_verbose_bytes,
        target_libdir_record,
        target_triple=rust_target,
        target_pointer_width=rust_pointer_width,
        target_endianness=rust_endianness,
    )
    linker = shutil.which("cc")
    linker_version = None
    linker_sha256 = None
    if linker:
        completed = subprocess.run(
            [linker, "--version"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False
        )
        linker_version = completed.stdout.decode(errors="replace").splitlines()[0]
        linker_sha256 = hashlib.sha256(Path(linker).resolve().read_bytes()).hexdigest()
    return Toolchains(
        gors=gors.resolve(),
        go=go.resolve(),
        goroot=goroot.resolve(),
        rustc=rustc.resolve(),
        rust_channel=rust_channel,
        gors_version=gors_version,
        go_version=go_version_output,
        rustc_version=rustc_verbose,
        rust_target=rust_target,
        rust_pointer_width=rust_pointer_width,
        rust_endianness=rust_endianness,
        runtime_contract_identity=gors_runtime_contract_identity(gors_version),
        rustc_target_libdir=rustc_target_libdir,
        rustc_release_record_sha256=hashlib.sha256(rustc_release_record).hexdigest(),
        target_libdir_record_sha256=hashlib.sha256(target_libdir_record).hexdigest(),
        runtime_compatibility_identity=runtime_compatibility_identity,
        linker_path=linker,
        linker_version=linker_version,
        linker_sha256=linker_sha256,
    )


def sanitized_environment(temp_dir: Path) -> dict[str, str]:
    environment: dict[str, str] = {}
    for key in ("PATH", "HOME", "USERPROFILE", "SystemRoot", "SDKROOT", "DEVELOPER_DIR"):
        if value := os.environ.get(key):
            environment[key] = value
    environment.update(
        {
            "LANG": "C",
            "LC_ALL": "C",
            "TZ": "UTC",
            "SOURCE_DATE_EPOCH": "0",
            "TMPDIR": str(temp_dir),
            "TMP": str(temp_dir),
            "TEMP": str(temp_dir),
        }
    )
    return environment


def _rss_bytes(value: int) -> int:
    return value if sys.platform == "darwin" else value * 1024


def _validate_gors_timing_evidence(timings: Any, expected_jobs: int) -> None:
    if not isinstance(timings, dict):
        raise RuntimeError("gors did not publish compiler timing evidence")
    version = timings.get("version")
    if type(version) is not int or version != GORS_TIMING_REPORT_VERSION:
        raise RuntimeError(
            "gors timing report schema is incompatible: "
            f"expected version {GORS_TIMING_REPORT_VERSION}, got {version!r}"
        )
    if timings.get("command") != "build":
        raise RuntimeError(
            "gors timing report command is incompatible with native evidence: "
            f"expected 'build', got {timings.get('command')!r}"
        )
    phases = timings.get("phases")
    if not isinstance(phases, list):
        raise RuntimeError("gors timing report does not contain a phase list")
    phase_names: list[str] = []
    for phase in phases:
        if not isinstance(phase, dict) or not isinstance(phase.get("name"), str):
            raise RuntimeError("gors timing phase evidence is malformed")
        duration_ms = phase.get("durationMs")
        if (
            isinstance(duration_ms, bool)
            or not isinstance(duration_ms, (int, float))
            or not math.isfinite(duration_ms)
            or duration_ms < 0
        ):
            raise RuntimeError(
                f"gors timing phase {phase['name']!r} has a malformed duration"
            )
        phase_names.append(phase["name"])
    cache_events = timings.get("cacheEvents")
    if not isinstance(cache_events, list):
        raise RuntimeError("gors timing report does not contain cache-event evidence")
    compiler_cache_events = [
        event
        for event in cache_events
        if isinstance(event, dict) and event.get("layer") == "compiler"
    ]
    if len(compiler_cache_events) != 1 or len(cache_events) != 1:
        raise RuntimeError(
            "gors timing report must contain exactly one unambiguous compiler cache event"
        )
    cache_hit = compiler_cache_events[0].get("hit")
    if type(cache_hit) is not bool:
        raise RuntimeError("gors compiler cache-event evidence is malformed")
    expected_phases = (
        GORS_BUILD_CACHE_HIT_PHASES if cache_hit else GORS_BUILD_CACHE_MISS_PHASES
    )
    if tuple(phase_names) != expected_phases:
        cache_state = "hit" if cache_hit else "miss"
        raise RuntimeError(
            f"gors {cache_state} timing phases are incompatible: "
            f"expected {list(expected_phases)!r}, got {phase_names!r}"
        )
    if timings.get("jobs") != expected_jobs:
        raise RuntimeError(
            "gors timing job budget does not match the certified plan: "
            f"expected {expected_jobs}, got {timings.get('jobs')}"
        )
    scheduler = timings.get("scheduler")
    if not isinstance(scheduler, dict):
        raise RuntimeError("gors did not publish scheduler timing evidence")
    scheduler_fields = (
        "serialWaves",
        "parallelWaves",
        "scheduledRoots",
        "snapshotsCreated",
        "poolStarts",
        "peakWorkers",
    )
    if any(
        type(scheduler.get(field)) is not int or scheduler[field] < 0
        for field in scheduler_fields
    ):
        raise RuntimeError("gors scheduler timing evidence is malformed")
    if cache_hit and any(scheduler[field] != 0 for field in scheduler_fields):
        raise RuntimeError("gors cache-hit scheduler evidence must be all zero")
    if scheduler["peakWorkers"] > expected_jobs:
        raise RuntimeError(
            "gors scheduler exceeded the certified job budget: "
            f"budget {expected_jobs}, peak {scheduler['peakWorkers']}"
        )
    if scheduler["poolStarts"] > 1:
        raise RuntimeError("gors started more than one compiler worker pool")
    if scheduler["parallelWaves"] == 0 and (
        scheduler["snapshotsCreated"] != 0
        or scheduler["peakWorkers"] != 0
        or scheduler["poolStarts"] != 0
    ):
        raise RuntimeError("gors serial scheduler evidence is incoherent")
    wave_count = scheduler["serialWaves"] + scheduler["parallelWaves"]
    if wave_count == 0 and scheduler["scheduledRoots"] != 0:
        raise RuntimeError("gors cache-hit scheduler evidence is incoherent")
    if wave_count > 0 and scheduler["scheduledRoots"] < wave_count:
        raise RuntimeError("gors scheduler root evidence is incoherent")
    if scheduler["parallelWaves"] > 0 and (
        scheduler["snapshotsCreated"] < 2 * scheduler["parallelWaves"]
        or scheduler["peakWorkers"] < 2
        or scheduler["poolStarts"] != 1
    ):
        raise RuntimeError("gors parallel scheduler evidence is incoherent")


def _measurement_worker(plan: dict[str, Any], sender: Any) -> None:
    try:
        usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
        commands: list[dict[str, Any]] = []
        runtime_link_evidence = None
        started = time.perf_counter_ns()
        for command in plan["commands"]:
            stage_started = time.perf_counter_ns()
            argv = list(command["argv"])
            command_runtime_link = None
            if runtime_link := command.get("runtimeLink"):
                if runtime_link_evidence is not None:
                    raise RuntimeError("measurement plan contains multiple runtime link descriptors")
                command_runtime_link = validate_runtime_link_descriptor(
                    Path(runtime_link["descriptorPath"]),
                    expected_contract_identity=runtime_link["expectedContractIdentity"],
                    expected_target_triple=runtime_link["expectedTargetTriple"],
                    expected_target_pointer_width=runtime_link[
                        "expectedTargetPointerWidth"
                    ],
                    expected_target_endianness=runtime_link["expectedTargetEndianness"],
                    expected_compatibility_identity=runtime_link[
                        "expectedCompatibilityIdentity"
                    ],
                    expected_rustc_release_record_sha256=runtime_link[
                        "expectedRustcReleaseRecordSha256"
                    ],
                    expected_target_libdir_record_sha256=runtime_link[
                        "expectedTargetLibdirRecordSha256"
                    ],
                    expected_runtime_cache_root=Path(runtime_link["runtimeCacheRoot"]),
                )
                argv = expand_rustc_runtime_link(argv, command_runtime_link)
                runtime_link_evidence = command_runtime_link
            completed = subprocess.run(
                argv,
                cwd=command["cwd"],
                env=command["env"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=600,
            )
            stage_ended = time.perf_counter_ns()
            commands.append(
                {
                    "stage": command["stage"],
                    "argv": argv,
                    "cwd": command["cwd"],
                    "environment": command["env"],
                    "durationNs": stage_ended - stage_started,
                    "exitCode": completed.returncode,
                    "stdoutBase64": base64.b64encode(completed.stdout).decode(),
                    "stderrBase64": base64.b64encode(completed.stderr).decode(),
                    "runtimeLink": command_runtime_link,
                }
            )
            if completed.returncode != 0:
                stderr = completed.stderr.decode(errors="replace")[-4000:]
                raise RuntimeError(f"{command['stage']} failed: {stderr}")
        if plan.get("runtimeLinkRequired") and runtime_link_evidence is None:
            raise RuntimeError("measurement plan did not admit a runtime link descriptor")
        if not plan.get("runtimeLinkRequired") and runtime_link_evidence is not None:
            raise RuntimeError("measurement plan admitted an unexpected runtime link descriptor")
        os.replace(plan["pendingArtifact"], plan["artifact"])
        ended = time.perf_counter_ns()
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
        timings = None
        timing_path = plan.get("gorsTimings")
        if timing_path and Path(timing_path).is_file():
            timings = json.loads(Path(timing_path).read_text(encoding="utf-8"))
        expected_jobs = plan.get("jobBudget")
        if expected_jobs is not None:
            _validate_gors_timing_evidence(timings, expected_jobs)
        sender.send(
            {
                "ok": True,
                "measurement": {
                    "wallNs": ended - started,
                    "normalizedWallNs": None,
                    "userCpuNs": round(
                        (usage_after.ru_utime - usage_before.ru_utime) * 1_000_000_000
                    ),
                    "systemCpuNs": round(
                        (usage_after.ru_stime - usage_before.ru_stime) * 1_000_000_000
                    ),
                    "peakRssBytes": _rss_bytes(usage_after.ru_maxrss),
                    "bytesRead": None,
                    "bytesWritten": None,
                    "ioInputBlocks": usage_after.ru_inblock - usage_before.ru_inblock,
                    "ioOutputBlocks": usage_after.ru_oublock - usage_before.ru_oublock,
                    "processCount": None,
                    "directProcessCount": len(plan["commands"]),
                    "artifactBytes": Path(plan["artifact"]).stat().st_size,
                    "commands": commands,
                    "internalTimings": timings,
                    "runtimeLink": runtime_link_evidence,
                },
            }
        )
    except BaseException as error:
        sender.send({"ok": False, "error": f"{type(error).__name__}: {error}"})
    finally:
        sender.close()


def measure_plan(plan: dict[str, Any], *, timeout_seconds: int = 1300) -> dict[str, Any]:
    context = multiprocessing.get_context("spawn")
    receiver, sender = context.Pipe(duplex=False)
    process = context.Process(target=_measurement_worker, args=(plan, sender))
    process.start()
    sender.close()
    if not receiver.poll(timeout_seconds):
        process.terminate()
        process.join()
        raise RuntimeError("performance sample exceeded its timeout")
    result = receiver.recv()
    receiver.close()
    process.join()
    if process.exitcode != 0 or not result.get("ok"):
        raise RuntimeError(result.get("error", f"measurement worker exited {process.exitcode}"))
    return result["measurement"]


def artifact_behavior(path: Path) -> dict[str, Any]:
    completed = subprocess.run(
        [str(path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=30
    )
    return {
        "exitCode": completed.returncode,
        "stdoutBase64": base64.b64encode(completed.stdout).decode(),
        "stderrBase64": base64.b64encode(completed.stderr).decode(),
        "stdoutSha256": hashlib.sha256(completed.stdout).hexdigest(),
        "stderrSha256": hashlib.sha256(completed.stderr).hexdigest(),
    }


def expected_behavior(specification: dict[str, Any]) -> dict[str, Any]:
    stdout = specification["stdoutUtf8"].encode()
    stderr = specification["stderrUtf8"].encode()
    return {
        "exitCode": specification["exitCode"],
        "stdoutBase64": base64.b64encode(stdout).decode(),
        "stderrBase64": base64.b64encode(stderr).decode(),
        "stdoutSha256": hashlib.sha256(stdout).hexdigest(),
        "stderrSha256": hashlib.sha256(stderr).hexdigest(),
    }


def validate_behavior(
    gors_artifact: Path,
    go_artifact: Path,
    specification: dict[str, Any],
) -> dict[str, Any]:
    expected = expected_behavior(specification)
    gors = artifact_behavior(gors_artifact)
    go = artifact_behavior(go_artifact)
    passed = gors == expected and go == expected and gors == go
    sentinel = specification.get("requiredSentinelUtf8")
    sentinel_present = True
    if sentinel is not None:
        encoded = sentinel.encode()
        sentinel_present = (
            encoded in base64.b64decode(gors["stderrBase64"])
            and encoded in base64.b64decode(go["stderrBase64"])
        )
        passed = passed and sentinel_present
    return {
        "passed": passed,
        "sentinelPresent": sentinel_present,
        "expected": expected,
        "gors": gors,
        "go": go,
    }


def rustc_arguments(source: Path, output: Path, target: str) -> list[str]:
    return [
        str(source),
        "--edition=2024",
        "--target",
        target,
        "-D",
        "unused_imports",
        "-D",
        "unused_macros",
        "-C",
        "overflow-checks=off",
        "-Ccodegen-units=1",
        "-Clto=fat",
        "-Copt-level=3",
        "-Ctarget-cpu=generic",
        "-Ctarget-feature=",
        "-o",
        str(output),
    ]


def pipeline_plan(
    compiler: str,
    *,
    side_root: Path,
    source: Path,
    toolchains: Toolchains,
    job_budget: int,
    go_experiment: str,
) -> tuple[dict[str, Any], Path]:
    artifact = side_root / ("program.exe" if os.name == "nt" else "program")
    pending = side_root / ("program.pending.exe" if os.name == "nt" else "program.pending")
    pending.unlink(missing_ok=True)
    temp_dir = side_root / "tmp"
    temp_dir.mkdir(parents=True, exist_ok=True)
    environment = sanitized_environment(temp_dir)
    if compiler == "gors":
        generated = side_root / "generated"
        timings = side_root / "gors-timings.json"
        timings.unlink(missing_ok=True)
        cache = side_root / "cache"
        generated.mkdir(parents=True, exist_ok=True)
        cache.mkdir(parents=True, exist_ok=True)
        environment["XDG_CACHE_HOME"] = str(cache)
        commands = [
            {
                "stage": "gors.compile_emit",
                "argv": [
                    str(toolchains.gors),
                    "build",
                    "--jobs",
                    str(job_budget),
                    str(source),
                    "--output",
                    str(generated),
                    "--timings-json",
                    str(timings),
                ],
                "cwd": str(side_root),
                "env": environment,
            },
            {
                "stage": "gors.external_rustc_link",
                "argv": [
                    str(toolchains.rustc),
                    *rustc_arguments(generated / "main.rs", pending, toolchains.rust_target),
                ],
                "cwd": str(side_root),
                "env": environment,
                "runtimeLink": {
                    "descriptorPath": str(generated / ".gors-link.json"),
                    "expectedContractIdentity": toolchains.runtime_contract_identity,
                    "expectedTargetTriple": toolchains.rust_target,
                    "expectedTargetPointerWidth": toolchains.rust_pointer_width,
                    "expectedTargetEndianness": toolchains.rust_endianness,
                    "expectedCompatibilityIdentity": (
                        toolchains.runtime_compatibility_identity
                    ),
                    "expectedRustcReleaseRecordSha256": (
                        toolchains.rustc_release_record_sha256
                    ),
                    "expectedTargetLibdirRecordSha256": (
                        toolchains.target_libdir_record_sha256
                    ),
                    "runtimeCacheRoot": str(cache / "gors" / "runtime"),
                },
            },
        ]
        return {
            "commands": commands,
            "pendingArtifact": str(pending),
            "artifact": str(artifact),
            "gorsTimings": str(timings),
            "jobBudget": job_budget,
            "runtimeLinkRequired": True,
            "runtimeLinkDescriptorSchema": LINK_DESCRIPTOR_SCHEMA_VERSION,
            "runtimeLinkValidation": RUNTIME_LINK_VALIDATION,
        }, artifact
    if compiler == "go":
        cache = side_root / "gocache"
        module_cache = side_root / "gomodcache"
        cache.mkdir(parents=True, exist_ok=True)
        module_cache.mkdir(parents=True, exist_ok=True)
        environment.update(
            {
                "CGO_ENABLED": "0",
                "GOCACHE": str(cache),
                "GOENV": "off",
                "GOEXPERIMENT": go_experiment,
                "GOFLAGS": "",
                "GOMAXPROCS": str(job_budget),
                "GOMODCACHE": str(module_cache),
                "GO111MODULE": "off",
                "GOROOT": str(toolchains.goroot),
                "GOTOOLCHAIN": "local",
            }
        )
        return {
            "commands": [
                {
                    "stage": "go.build_link",
                    "argv": [
                        str(toolchains.go),
                        "build",
                        "-p",
                        str(job_budget),
                        "-o",
                        str(pending),
                        str(source),
                    ],
                    "cwd": str(side_root),
                    "env": environment,
                }
            ],
            "pendingArtifact": str(pending),
            "artifact": str(artifact),
            "runtimeLinkRequired": False,
        }, artifact
    raise ValueError(f"unknown compiler {compiler}")
