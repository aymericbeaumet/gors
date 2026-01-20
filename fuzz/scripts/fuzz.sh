#!/bin/bash
#
# Fuzzing script for gors
# Uses cargo-afl for coverage-guided fuzzing with multi-CPU support
#
# Usage:
#   ./scripts/fuzz.sh <target> [options]
#
# Targets: scanner, parser, roundtrip
#
# Options:
#   -j N    Number of parallel fuzzers (default: all CPUs)
#   -t SEC  Timeout per run in seconds (default: 1000ms = 1s)
#   -c      Continue previous fuzzing session
#   -h      Show this help

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(dirname "$FUZZ_DIR")"

# Default values
NUM_JOBS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
TIMEOUT="1000+"
CONTINUE=false

show_help() {
    echo "Fuzzing script for gors"
    echo ""
    echo "Usage: $0 <target> [options]"
    echo ""
    echo "Targets:"
    echo "  scanner    Fuzz the Go scanner/lexer"
    echo "  parser     Fuzz the Go parser"
    echo "  roundtrip  Fuzz parse->print->reparse cycle"
    echo ""
    echo "Options:"
    echo "  -j N    Number of parallel fuzzers (default: $NUM_JOBS)"
    echo "  -t MS   Timeout per run in milliseconds (default: $TIMEOUT)"
    echo "  -c      Continue previous fuzzing session"
    echo "  -h      Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 scanner              # Fuzz scanner with all CPUs"
    echo "  $0 parser -j 4          # Fuzz parser with 4 CPUs"
    echo "  $0 roundtrip -c         # Continue roundtrip fuzzing"
}

# Parse arguments
TARGET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -j)
            NUM_JOBS="$2"
            shift 2
            ;;
        -t)
            TIMEOUT="$2+"
            shift 2
            ;;
        -c)
            CONTINUE=true
            shift
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

# Validate target
case "$TARGET" in
    scanner|parser|roundtrip)
        ;;
    *)
        echo "Error: Unknown target '$TARGET'"
        show_help
        exit 1
        ;;
esac

BINARY="fuzz_${TARGET}"
CORPUS_DIR="${FUZZ_DIR}/corpus/${TARGET}"
OUT_DIR="${FUZZ_DIR}/out/${TARGET}"
SYNC_DIR="${FUZZ_DIR}/sync/${TARGET}"

# Check if cargo-afl is installed
if ! command -v cargo-afl &> /dev/null; then
    echo "cargo-afl is not installed. Installing..."
    cargo install afl
fi

# Build the fuzz target with AFL instrumentation
echo "Building $BINARY with AFL instrumentation..."
cd "$FUZZ_DIR"
cargo afl build --release --features afl-fuzz --bin "$BINARY"

FUZZ_BINARY="${PROJECT_ROOT}/target/release/${BINARY}"

if [ ! -f "$FUZZ_BINARY" ]; then
    echo "Error: Built binary not found at $FUZZ_BINARY"
    exit 1
fi

# Create output directories
mkdir -p "$OUT_DIR" "$SYNC_DIR"

# Check if we should resume or start fresh
if [ "$CONTINUE" = false ] && [ -d "$SYNC_DIR/fuzzer-main" ]; then
    echo "Previous fuzzing session found. Use -c to continue or remove $SYNC_DIR to start fresh."
    exit 1
fi

# Determine AFL input flag
if [ "$CONTINUE" = true ] && [ -d "$SYNC_DIR/fuzzer-main" ]; then
    INPUT_FLAG="-i-"
else
    INPUT_FLAG="-i $CORPUS_DIR"
fi

echo ""
echo "=== Fuzzing Configuration ==="
echo "Target:     $TARGET"
echo "Binary:     $FUZZ_BINARY"
echo "Corpus:     $CORPUS_DIR"
echo "Output:     $SYNC_DIR"
echo "CPUs:       $NUM_JOBS"
echo "Timeout:    $TIMEOUT ms"
echo "Continue:   $CONTINUE"
echo ""

# Function to cleanup background processes
cleanup() {
    echo ""
    echo "Stopping fuzzers..."
    jobs -p | xargs -r kill 2>/dev/null || true
    wait 2>/dev/null || true
    echo "Done."
}
trap cleanup EXIT

# Start the main fuzzer
echo "Starting main fuzzer..."
AFL_SKIP_CPUFREQ=1 cargo afl fuzz \
    $INPUT_FLAG \
    -o "$SYNC_DIR" \
    -M fuzzer-main \
    -t "$TIMEOUT" \
    -- "$FUZZ_BINARY" &

MAIN_PID=$!
sleep 2

# Start secondary fuzzers
if [ "$NUM_JOBS" -gt 1 ]; then
    for i in $(seq 2 "$NUM_JOBS"); do
        echo "Starting secondary fuzzer $i..."
        AFL_SKIP_CPUFREQ=1 cargo afl fuzz \
            $INPUT_FLAG \
            -o "$SYNC_DIR" \
            -S "fuzzer-$i" \
            -t "$TIMEOUT" \
            -- "$FUZZ_BINARY" &
        sleep 1
    done
fi

echo ""
echo "Fuzzing started with $NUM_JOBS parallel processes."
echo "Press Ctrl+C to stop."
echo ""
echo "Crashes will be saved to: $SYNC_DIR/*/crashes/"
echo "Use './scripts/export-crashes.sh $TARGET' to export crashes as test files."
echo ""

# Wait for main fuzzer
wait $MAIN_PID
