#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIOS_DIR="${SCRIPT_DIR}/bios"

mkdir -p "${BIOS_DIR}"

for file in seabios.bin vgabios.bin; do
    if [ ! -f "${BIOS_DIR}/${file}" ]; then
        echo "==> Downloading ${file}..."
        curl -sSfL -o "${BIOS_DIR}/${file}" \
            "https://github.com/copy/v86/raw/master/bios/${file}"
    fi
done

echo "==> BIOS files ready"
