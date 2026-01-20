#!/bin/bash
#
# Export fuzzing crashes as test files
#
# This script takes crash inputs from AFL and saves them as .go test files
# in gors-cli/tests/files/fuzz_*.go for regression testing.
#
# Usage:
#   ./scripts/export-crashes.sh [target]
#
# If no target is specified, exports crashes from all targets.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(dirname "$FUZZ_DIR")"
TEST_FILES_DIR="${PROJECT_ROOT}/gors-cli/tests/files"
SYNC_DIR="${FUZZ_DIR}/sync"

export_target() {
    local TARGET="$1"
    local TARGET_SYNC_DIR="${SYNC_DIR}/${TARGET}"
    
    if [ ! -d "$TARGET_SYNC_DIR" ]; then
        echo "No fuzzing output found for target '$TARGET'"
        return 0
    fi
    
    local CRASH_COUNT=0
    local EXPORT_COUNT=0
    
    # Find all crash files across all fuzzer instances
    for CRASH_DIR in "$TARGET_SYNC_DIR"/*/crashes; do
        if [ ! -d "$CRASH_DIR" ]; then
            continue
        fi
        
        for CRASH_FILE in "$CRASH_DIR"/*; do
            if [ ! -f "$CRASH_FILE" ] || [[ "$(basename "$CRASH_FILE")" == "README.txt" ]]; then
                continue
            fi
            
            CRASH_COUNT=$((CRASH_COUNT + 1))
            
            # Generate a unique filename based on content hash
            local HASH=$(sha256sum "$CRASH_FILE" | cut -c1-8)
            local OUT_FILE="${TEST_FILES_DIR}/fuzz_${TARGET}_${HASH}.go"
            
            # Skip if already exported
            if [ -f "$OUT_FILE" ]; then
                echo "  Already exported: $OUT_FILE"
                continue
            fi
            
            # Check if the file content is valid UTF-8
            if ! iconv -f UTF-8 -t UTF-8 "$CRASH_FILE" > /dev/null 2>&1; then
                echo "  Skipping non-UTF-8 crash: $(basename "$CRASH_FILE")"
                continue
            fi
            
            # Copy the crash file
            cp "$CRASH_FILE" "$OUT_FILE"
            EXPORT_COUNT=$((EXPORT_COUNT + 1))
            echo "  Exported: $OUT_FILE"
        done
    done
    
    echo "Target '$TARGET': Found $CRASH_COUNT crashes, exported $EXPORT_COUNT new files"
}

# Ensure test files directory exists
mkdir -p "$TEST_FILES_DIR"

if [ $# -eq 0 ]; then
    # Export all targets
    echo "Exporting crashes from all targets..."
    echo ""
    for TARGET in scanner parser roundtrip; do
        export_target "$TARGET"
    done
else
    # Export specified target
    export_target "$1"
fi

echo ""
echo "Done. Test files are in: $TEST_FILES_DIR"
echo ""
echo "To run tests with the new files:"
echo "  cargo test --package gors-cli"
