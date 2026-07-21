#!/bin/bash
#
# Promote cargo-fuzz artifacts into the checked-in regression corpus.
#
# Run the resulting corpus through the stable replay tests before committing it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"
ARTIFACTS_DIR="${FUZZ_DIR}/artifacts"

is_known_target() {
    case "$1" in
        scanner|parser|roundtrip|compiler)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

export_target() {
    local TARGET="$1"
    local TARGET_ARTIFACT_DIR="${ARTIFACTS_DIR}/${TARGET}"
    local TARGET_CORPUS_DIR="${FUZZ_DIR}/corpus/${TARGET}"

    if [ ! -d "$TARGET_ARTIFACT_DIR" ]; then
        echo "No artifacts found for target '$TARGET'"
        return 0
    fi

    mkdir -p "$TARGET_CORPUS_DIR"
    local ARTIFACT_COUNT=0
    local EXPORT_COUNT=0
    local ARTIFACT
    local HASH
    local OUT_FILE

    for ARTIFACT in "$TARGET_ARTIFACT_DIR"/*; do
        if [ ! -f "$ARTIFACT" ]; then
            continue
        fi

        ARTIFACT_COUNT=$((ARTIFACT_COUNT + 1))
        if command -v sha256sum >/dev/null 2>&1; then
            HASH="$(sha256sum "$ARTIFACT" | cut -c1-16)"
        else
            HASH="$(shasum -a 256 "$ARTIFACT" | cut -c1-16)"
        fi
        OUT_FILE="${TARGET_CORPUS_DIR}/regression-${HASH}"

        if [ -f "$OUT_FILE" ]; then
            echo "  Already promoted: $OUT_FILE"
            continue
        fi

        cp "$ARTIFACT" "$OUT_FILE"
        EXPORT_COUNT=$((EXPORT_COUNT + 1))
        echo "  Promoted: $OUT_FILE"
    done

    echo "Target '$TARGET': found $ARTIFACT_COUNT artifacts, promoted $EXPORT_COUNT"
}

if [ $# -gt 1 ]; then
    echo "Usage: $0 [scanner|parser|roundtrip|compiler]"
    exit 1
fi

if [ $# -eq 0 ]; then
    echo "Promoting artifacts from all targets..."
    echo ""
    for TARGET in scanner parser roundtrip compiler; do
        export_target "$TARGET"
    done
else
    if ! is_known_target "$1"; then
        echo "Error: unknown target '$1'"
        exit 1
    fi
    export_target "$1"
fi

echo ""
echo "Run 'cargo test --package fuzz --test corpus' before committing promoted inputs."
