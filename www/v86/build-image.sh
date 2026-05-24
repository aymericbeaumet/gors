#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE_NAME="gors-v86-rustc"
OUTPUT_DIR="${SCRIPT_DIR}/dist"
TOOLS_DIR="${SCRIPT_DIR}/tools"
MANIFEST="${OUTPUT_DIR}/manifest.json"

mkdir -p "${OUTPUT_DIR}"

# Immutable: skip if manifest and rootfs already exist
if [ -f "${MANIFEST}" ] && [ -d "${OUTPUT_DIR}/rootfs-flat" ] && [ -f "${OUTPUT_DIR}/rootfs.json" ]; then
    echo "==> 9p filesystem already exists (immutable), skipping build"
    exit 0
fi

# Clean old artifacts
rm -rf "${OUTPUT_DIR}/rootfs-flat" "${OUTPUT_DIR}/rootfs.json" "${MANIFEST}"
rm -f "${OUTPUT_DIR}"/ext2-*.img.gz "${OUTPUT_DIR}"/vmlinuz-*.bin "${OUTPUT_DIR}"/initramfs-*.bin

echo "==> Building Docker image (alpine + rustc native i686)..."
docker build --platform linux/386 -t "${IMAGE_NAME}" "${SCRIPT_DIR}"

echo "==> Exporting container filesystem..."
CONTAINER_ID=$(docker create --platform linux/386 "${IMAGE_NAME}")
docker export "${CONTAINER_ID}" > "${OUTPUT_DIR}/rootfs.tar"
docker rm "${CONTAINER_ID}" > /dev/null

echo "==> Generating 9p filesystem (fs2json + copy-to-sha256)..."
python3 "${TOOLS_DIR}/fs2json.py" --out "${OUTPUT_DIR}/rootfs.json" "${OUTPUT_DIR}/rootfs.tar"
mkdir -p "${OUTPUT_DIR}/rootfs-flat"
python3 "${TOOLS_DIR}/copy-to-sha256.py" "${OUTPUT_DIR}/rootfs.tar" "${OUTPUT_DIR}/rootfs-flat"

rm -f "${OUTPUT_DIR}/rootfs.tar"

# Write manifest
printf '{"type":"9p"}\n' > "${MANIFEST}"

echo "==> Done!"
echo "    rootfs.json: $(wc -c < "${OUTPUT_DIR}/rootfs.json" | tr -d ' ') bytes"
echo "    rootfs-flat: $(find "${OUTPUT_DIR}/rootfs-flat" -type f | wc -l | tr -d ' ') files"
