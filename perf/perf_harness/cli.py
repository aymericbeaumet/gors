from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from .model import (
    EvidenceError,
    gate_acceptance,
    read_json,
    validate_acceptance,
    validate_corpus,
)
from .runner import MINIMUM_SAMPLES, MINIMUM_SESSIONS, PerformanceRun, RunOptions, write_result


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return parsed


def add_measurement_arguments(parser: argparse.ArgumentParser, mode: str) -> None:
    root = repository_root()
    parser.add_argument(
        "--output",
        type=Path,
        default=root / "target" / "perf" / f"{mode}.json",
        help="machine-readable evidence path",
    )
    parser.add_argument(
        "--run-root",
        type=Path,
        default=None,
        help="isolated source, cache, and artifact workspace",
    )
    parser.add_argument("--samples", type=positive_integer, default=MINIMUM_SAMPLES)
    parser.add_argument("--sessions", type=positive_integer, default=MINIMUM_SESSIONS)
    parser.add_argument("--seed", type=int, default=20260722)
    parser.add_argument(
        "--jobs", type=positive_integer, default=max(1, os.cpu_count() or 1), dest="job_budget"
    )
    parser.add_argument("--hardware-class", default="unclassified-local")
    parser.add_argument("--go-experiment", default="")
    parser.add_argument("--dedicated", action="store_true")
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="allow an undersampled harness exercise; its result can never promote",
    )
    parser.add_argument("--gors", type=Path, help="explicit gors executable override")
    parser.add_argument("--go", type=Path, help="explicit pinned Go executable override")
    parser.add_argument("--rustc", type=Path, help="explicit pinned rustc executable override")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(
        prog="gors-perf",
        description="Paired native performance evidence and non-regression gates",
    )
    commands = root.add_subparsers(dest="command", required=True)
    baseline = commands.add_parser(
        "baseline", help="capture a trend baseline; never eligible for promotion"
    )
    add_measurement_arguments(baseline, "baseline")
    certify = commands.add_parser(
        "certify", help="run the dedicated certification protocol"
    )
    add_measurement_arguments(certify, "certification")
    gate = commands.add_parser("gate", help="enforce all promoted acceptance budgets")
    gate.add_argument(
        "--acceptance", type=Path, default=repository_root() / "perf" / "acceptance-v1.json"
    )
    gate.add_argument("--result", type=Path, help="current certification evidence")
    commands.add_parser("validate", help="validate checked-in schemas, corpus, and acceptance data")
    return root


def validate_protocol_arguments(arguments: argparse.Namespace) -> None:
    if arguments.sessions > arguments.samples:
        raise EvidenceError("sessions cannot exceed samples")
    undersampled = arguments.samples < MINIMUM_SAMPLES or arguments.sessions < MINIMUM_SESSIONS
    if undersampled and not arguments.smoke:
        raise EvidenceError(
            f"undersampled runs require --smoke; promotion requires {MINIMUM_SAMPLES} samples "
            f"and {MINIMUM_SESSIONS} sessions"
        )
    if arguments.command == "certify" and not arguments.smoke:
        if not arguments.dedicated:
            raise EvidenceError("certification requires --dedicated")
        if arguments.hardware_class == "unclassified-local":
            raise EvidenceError("certification requires a named --hardware-class")


def run_measurement(arguments: argparse.Namespace) -> int:
    validate_protocol_arguments(arguments)
    root = repository_root()
    run_root = arguments.run_root or (
        root
        / "target"
        / "perf"
        / "runs"
        / f"{arguments.command}-{time.time_ns()}-{os.getpid()}"
    )
    run_root = run_root.resolve()
    if run_root.exists() and any(run_root.iterdir()):
        raise EvidenceError(f"run root must be empty to preserve cold-cache semantics: {run_root}")
    options = RunOptions(
        repository_root=root,
        corpus_manifest=root / "perf" / "corpus" / "v1" / "manifest.json",
        output_path=arguments.output.resolve(),
        run_root=run_root,
        mode="baseline" if arguments.command == "baseline" else "certification",
        samples=arguments.samples,
        sessions=arguments.sessions,
        seed=arguments.seed,
        smoke=arguments.smoke,
        dedicated=arguments.dedicated,
        hardware_class=arguments.hardware_class,
        job_budget=arguments.job_budget,
        go_experiment=arguments.go_experiment,
        gors_path=arguments.gors.resolve() if arguments.gors else None,
        go_path=arguments.go.resolve() if arguments.go else None,
        rustc_path=arguments.rustc.resolve() if arguments.rustc else None,
    )
    print(
        f"gors-perf: {options.mode}, {options.samples} pairs/scenario across "
        f"{options.sessions} sessions",
        file=sys.stderr,
    )
    result = PerformanceRun(options).execute()
    write_result(options.output_path, result)
    summary = {
        "result": str(options.output_path),
        "resultId": result["resultId"],
        "promotionEligible": result["promotionEligible"],
        "promotionBlockers": result["promotionBlockers"],
        "scenarios": [
            {
                "workloadId": item["workloadId"],
                "scenario": item["scenario"],
                "gorsP50Ns": item["summary"]["normalized"]["gors"]["p50Ns"],
                "goP50Ns": item["summary"]["normalized"]["go"]["p50Ns"],
                "pairedRatioCiUpper": item["summary"]["normalized"]["pairedRatio"][
                    "bootstrapMedianCi95"
                ]["upper"],
                "achieved": item["achievement"]["achieved"],
            }
            for item in result["workloads"]
        ],
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


def validate_checked_in_files() -> dict[str, Any]:
    root = repository_root()
    schemas = sorted((root / "perf" / "schema").glob("*.schema.json"))
    if not schemas:
        raise EvidenceError("no performance schemas are checked in")
    schema_ids: set[str] = set()
    for path in schemas:
        schema = read_json(path)
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise EvidenceError(f"{path} does not declare JSON Schema 2020-12")
        schema_id = schema.get("$id")
        if not isinstance(schema_id, str) or schema_id in schema_ids:
            raise EvidenceError(f"{path} has a missing or duplicate $id")
        schema_ids.add(schema_id)
    corpus = validate_corpus(root / "perf" / "corpus" / "v1" / "manifest.json")
    acceptance = read_json(root / "perf" / "acceptance-v1.json")
    validate_acceptance(acceptance)
    if acceptance.get("corpusDigest") != corpus.get("digest"):
        raise EvidenceError("acceptance manifest has a stale corpus digest")
    return {
        "schemas": len(schemas),
        "corpusDigest": corpus["digest"],
        "workloads": len(corpus["workloads"]),
        "promotedScenarios": len(acceptance["promotedScenarios"]),
    }


def current_commit(root: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise EvidenceError("cannot determine the current repository commit")
    return completed.stdout.decode().strip()


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    try:
        if arguments.command in ("baseline", "certify"):
            return run_measurement(arguments)
        if arguments.command == "validate":
            print(json.dumps(validate_checked_in_files(), indent=2, sort_keys=True))
            return 0
        if arguments.command == "gate":
            root = repository_root()
            validate_checked_in_files()
            report = gate_acceptance(
                acceptance_path=arguments.acceptance.resolve(),
                current_result_path=arguments.result.resolve() if arguments.result else None,
                repository_root=root,
                expected_commit=current_commit(root),
            )
            print(json.dumps(report, indent=2, sort_keys=True))
            return 0
        raise EvidenceError(f"unknown command {arguments.command}")
    except (EvidenceError, OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"gors-perf: error: {error}", file=sys.stderr)
        return 1
