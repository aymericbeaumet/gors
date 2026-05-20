#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE_NAME="gors-v86-rustc"
OUTPUT_DIR="${SCRIPT_DIR}/dist"
MANIFEST="${OUTPUT_DIR}/manifest.json"
IMAGE_SIZE_MB=1200

mkdir -p "${OUTPUT_DIR}"

# Immutable: skip if any image + manifest already exist
if [ -f "${MANIFEST}" ] && ls "${OUTPUT_DIR}"/ext2-*.img 1>/dev/null 2>&1; then
    echo "==> ext2 image already exists (immutable), skipping build"
    exit 0
fi

echo "==> Building Docker image (i386/debian + rustc + wasm32 target)..."
docker build --platform linux/386 -t "${IMAGE_NAME}" "${SCRIPT_DIR}"

echo "==> Exporting container filesystem..."
CONTAINER_ID=$(docker create --platform linux/386 "${IMAGE_NAME}")
docker export "${CONTAINER_ID}" > "${OUTPUT_DIR}/rootfs.tar"
docker rm "${CONTAINER_ID}" > /dev/null

TMP_IMAGE="${OUTPUT_DIR}/ext2-tmp.img"

echo "==> Creating ext2 disk image (${IMAGE_SIZE_MB}MB)..."
docker run --rm --privileged --platform linux/amd64 \
    -v "${OUTPUT_DIR}:/output" \
    alpine:3.20 sh -c "
        apk add --no-cache e2fsprogs tar >/dev/null 2>&1 &&
        dd if=/dev/zero of=/output/ext2-tmp.img bs=1M count=${IMAGE_SIZE_MB} 2>/dev/null &&
        mkfs.ext4 -F -q /output/ext2-tmp.img &&
        mkdir -p /mnt/img &&
        mount -o loop /output/ext2-tmp.img /mnt/img &&
        tar xf /output/rootfs.tar -C /mnt/img &&
        echo '::sysinit:/bin/mount -t proc proc /proc' > /mnt/img/etc/inittab &&
        echo '::sysinit:/bin/mount -t sysfs sysfs /sys' >> /mnt/img/etc/inittab &&
        echo '::sysinit:/bin/mount -t devtmpfs devtmpfs /dev' >> /mnt/img/etc/inittab &&
        echo 'ttyS0::respawn:/bin/sh' >> /mnt/img/etc/inittab &&
        umount /mnt/img
    "

rm -f "${OUTPUT_DIR}/rootfs.tar"

# Content-hash the image for immutable caching
HASH=$(shasum -a 256 "${TMP_IMAGE}" | cut -c1-16)
FINAL_NAME="ext2-${HASH}.img"
mv "${TMP_IMAGE}" "${OUTPUT_DIR}/${FINAL_NAME}"

# Write manifest so the frontend knows the filename
printf '{"image":"%s"}\n' "${FINAL_NAME}" > "${MANIFEST}"

echo "==> Image created: ${OUTPUT_DIR}/${FINAL_NAME}"
ls -lh "${OUTPUT_DIR}/${FINAL_NAME}"
echo "==> Done!"
