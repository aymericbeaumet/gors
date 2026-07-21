#!/bin/bash
#
# Bounded cargo-fuzz runner for gors.
#
# Usage:
#   ./scripts/fuzz.sh <target> [options]
#
# Targets: scanner, parser, roundtrip, compiler
#
# Options:
#   -j N    Number of parallel libFuzzer workers (default: all CPUs)
#   -t SEC  Total fuzzing budget in seconds (default: 3600)
#   -h      Show this help

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"

NUM_JOBS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
DURATION_SECONDS=3600
FUZZ_TOOLCHAIN="${GORS_FUZZ_TOOLCHAIN:-nightly-2026-07-01}"

show_help() {
    echo "Fuzzing script for gors"
    echo ""
    echo "Usage: $0 <target> [options]"
    echo ""
    echo "Targets:"
    echo "  scanner    Fuzz the Go scanner/lexer"
    echo "  parser     Fuzz the Go parser"
    echo "  roundtrip  Fuzz deterministic parse/AST-snapshot output"
    echo "  compiler   Fuzz generic Go AST to Rust AST lowering"
    echo ""
    echo "Options:"
    echo "  -j N    Number of parallel fuzzers (default: $NUM_JOBS)"
    echo "  -t SEC  Total fuzzing budget in seconds (default: $DURATION_SECONDS)"
    echo "  -h      Show this help"
    echo ""
    echo "Environment:"
    echo "  GORS_FUZZ_TOOLCHAIN  Installed nightly to use (default: $FUZZ_TOOLCHAIN)"
    echo ""
    echo "Examples:"
    echo "  $0 scanner -t 300       # Fuzz scanner for five minutes"
    echo "  $0 parser -j 4 -t 1800  # Fuzz parser with four workers"
    echo "  $0 compiler -t 3600     # Fuzz compiler lowering for one hour"
}

TARGET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -j)
            if [[ $# -lt 2 ]]; then
                echo "Error: -j requires a worker count"
                exit 1
            fi
            NUM_JOBS="$2"
            shift 2
            ;;
        -t)
            if [[ $# -lt 2 ]]; then
                echo "Error: -t requires a duration"
                exit 1
            fi
            DURATION_SECONDS="$2"
            shift 2
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        -*)
            echo "Unknown option: $1"
            show_help
            exit 1
            ;;
        *)
            if [ -z "$TARGET" ]; then
                TARGET="$1"
            else
                echo "Multiple targets specified"
                show_help
                exit 1
            fi
            shift
            ;;
    esac
done

if [ -z "$TARGET" ]; then
    echo "Error: No target specified"
    show_help
    exit 1
fi

case "$TARGET" in
    scanner|parser|roundtrip|compiler)
        ;;
    *)
        echo "Error: Unknown target '$TARGET'"
        show_help
        exit 1
        ;;
esac

if ! [[ "$NUM_JOBS" =~ ^[1-9][0-9]*$ ]]; then
    echo "Error: jobs must be a positive integer"
    exit 1
fi
if ! [[ "$DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
    echo "Error: duration must be a positive integer"
    exit 1
fi

CORPUS_DIR="${FUZZ_DIR}/corpus/${TARGET}"
WORK_CORPUS_DIR="${FUZZ_DIR}/work-corpus/${TARGET}"
ARTIFACT_DIR="${FUZZ_DIR}/artifacts/${TARGET}"
mkdir -p "$CORPUS_DIR" "$WORK_CORPUS_DIR" "$ARTIFACT_DIR"

echo "Target: $TARGET"
echo "Seed corpus: $CORPUS_DIR"
echo "Working corpus: $WORK_CORPUS_DIR"
echo "Artifacts: $ARTIFACT_DIR"
echo "Workers: $NUM_JOBS"
echo "Budget: ${DURATION_SECONDS}s"
echo "Toolchain: $FUZZ_TOOLCHAIN"

cd "$FUZZ_DIR"
CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-off}" \
    cargo "+${FUZZ_TOOLCHAIN}" fuzz run --features fuzzing --codegen-units 16 \
    "$TARGET" "$WORK_CORPUS_DIR" "$CORPUS_DIR" -- \
    "-max_total_time=${DURATION_SECONDS}" \
    "-timeout=10" \
    "-rss_limit_mb=4096" \
    "-max_len=262144" \
    "-seed=1" \
    "-jobs=${NUM_JOBS}" \
    "-workers=${NUM_JOBS}"
