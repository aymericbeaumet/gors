#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}/dist"
TOOLS_DIR="${SCRIPT_DIR}/tools"
MANIFEST="${OUTPUT_DIR}/manifest.json"
PROVIDER_EXPORT="${OUTPUT_DIR}/runtime-provider.json"
IMAGE_ONLY="${GORS_V86_IMAGE_ONLY:-0}"
LOCK_DIR="${OUTPUT_DIR}/.build.lock"
CONTEXT_DIR=""
PUBLISH_DIR=""
BACKUP_DIR=""
CONTAINER_ID=""
PUBLICATION_COMMITTED=0
LOCK_HELD=0

cleanup() {
    if [[ "${PUBLICATION_COMMITTED}" != "1" && -n "${BACKUP_DIR}" \
        && -d "${BACKUP_DIR}" ]]; then
        for name in rootfs-flat rootfs.json runtime-provider.json manifest.json; do
            if [[ -e "${BACKUP_DIR}/${name}" ]]; then
                if [[ -e "${OUTPUT_DIR}/${name}" ]]; then
                    rm -rf "${OUTPUT_DIR:?}/${name}"
                fi
                mv "${BACKUP_DIR}/${name}" "${OUTPUT_DIR}/${name}"
            fi
        done
    fi
    if [[ -n "${CONTAINER_ID}" ]]; then
        docker rm -f "${CONTAINER_ID}" > /dev/null 2>&1 || true
    fi
    if [[ -n "${CONTEXT_DIR}" ]]; then
        rm -rf "${CONTEXT_DIR}"
    fi
    if [[ -n "${PUBLISH_DIR}" ]]; then
        rm -rf "${PUBLISH_DIR}"
    fi
    if [[ -n "${BACKUP_DIR}" ]]; then
        rm -rf "${BACKUP_DIR}"
    fi
    if [[ "${LOCK_HELD}" == "1" ]]; then
        rm -f "${LOCK_DIR}/pid"
        rmdir "${LOCK_DIR}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

mkdir -p "${OUTPUT_DIR}"
if ! mkdir "${LOCK_DIR}" 2>/dev/null; then
    echo "another V86 image build owns ${LOCK_DIR}" >&2
    exit 1
fi
LOCK_HELD=1
printf '%s\n' "$$" >"${LOCK_DIR}/pid"

DIGEST_INPUTS=(
    Cargo.lock
    gors-runtime/Cargo.toml
    gors-runtime/src
    gors-runtime-abi/Cargo.toml
    gors-runtime-abi/src
    gors-runtime-abi/examples
    www/v86/Cargo.v86.toml
    www/v86/Dockerfile
    www/v86/build-image.sh
    www/v86/rootfs
    www/v86/tools/copy-to-sha256.py
    www/v86/tools/fs2json.py
    www/v86/tools/input-digest.py
    www/v86/tools/rootfs_evidence.py
    www/v86/tools/verify-manifest.py
    www/v86/tools/write-manifest.py
)

echo "==> Resolving linux/386 Alpine base and package inputs..."
docker pull --platform linux/386 alpine:edge > /dev/null
ALPINE_IMAGE=$(docker image inspect --format '{{index .RepoDigests 0}}' alpine:edge)
ALPINE_IMAGE_ID=$(docker image inspect --format '{{.Id}}' alpine:edge)
APK_RESOLUTION=$(docker run --rm --platform linux/386 "${ALPINE_IMAGE}" \
    sh -eu -c 'apk update > /dev/null; apk policy alpine-base cargo ca-certificates jq linux-firmware-none linux-virt mold openrc rust')

INPUT_DIGEST=$(python3 "${TOOLS_DIR}/input-digest.py" \
    "${REPOSITORY_ROOT}" \
    "${DIGEST_INPUTS[@]}" \
    "--fact=alpine-image=${ALPINE_IMAGE}" \
    "--fact=alpine-image-id=${ALPINE_IMAGE_ID}" \
    "--fact=apk-resolution=${APK_RESOLUTION}")

if [[ "${IMAGE_ONLY}" != "1" ]] && python3 "${TOOLS_DIR}/verify-manifest.py" \
    "${INPUT_DIGEST}" \
    "${MANIFEST}" \
    "${PROVIDER_EXPORT}" \
    "${OUTPUT_DIR}/rootfs.json" \
    "${OUTPUT_DIR}/rootfs-flat" \
    quiet; then
    echo "==> V86 rootfs input digest ${INPUT_DIGEST} is current; skipping build"
    exit 0
fi

CONTEXT_DIR=$(mktemp -d "${TMPDIR:-/tmp}/gors-v86-context.XXXXXX")

cp "${SCRIPT_DIR}/Dockerfile" "${CONTEXT_DIR}/Dockerfile"
cp "${SCRIPT_DIR}/Cargo.v86.toml" "${CONTEXT_DIR}/Cargo.v86.toml"
cp "${REPOSITORY_ROOT}/Cargo.lock" "${CONTEXT_DIR}/Cargo.lock"
cp -R "${SCRIPT_DIR}/rootfs" "${CONTEXT_DIR}/rootfs"
mkdir -p "${CONTEXT_DIR}/gors-runtime" "${CONTEXT_DIR}/gors-runtime-abi"
cp "${REPOSITORY_ROOT}/gors-runtime/Cargo.toml" \
    "${CONTEXT_DIR}/gors-runtime/Cargo.toml"
cp -R "${REPOSITORY_ROOT}/gors-runtime/src" \
    "${CONTEXT_DIR}/gors-runtime/src"
cp "${REPOSITORY_ROOT}/gors-runtime-abi/Cargo.toml" \
    "${CONTEXT_DIR}/gors-runtime-abi/Cargo.toml"
cp -R "${REPOSITORY_ROOT}/gors-runtime-abi/src" \
    "${CONTEXT_DIR}/gors-runtime-abi/src"
mkdir -p "${CONTEXT_DIR}/gors-runtime-abi/examples"
cp "${REPOSITORY_ROOT}/gors-runtime-abi/examples/runtime_provider_manifest.rs" \
    "${CONTEXT_DIR}/gors-runtime-abi/examples/runtime_provider_manifest.rs"
cp "${REPOSITORY_ROOT}/gors-runtime-abi/examples/runtime_dependency_validator.rs" \
    "${CONTEXT_DIR}/gors-runtime-abi/examples/runtime_dependency_validator.rs"

CURRENT_INPUT_DIGEST=$(python3 "${TOOLS_DIR}/input-digest.py" \
    "${REPOSITORY_ROOT}" \
    "${DIGEST_INPUTS[@]}" \
    "--fact=alpine-image=${ALPINE_IMAGE}" \
    "--fact=alpine-image-id=${ALPINE_IMAGE_ID}" \
    "--fact=apk-resolution=${APK_RESOLUTION}")
if [[ "${CURRENT_INPUT_DIGEST}" != "${INPUT_DIGEST}" ]]; then
    echo "V86 image inputs changed while preparing the Docker context; retry" >&2
    exit 1
fi

IMAGE_NAME="gors-v86-rustc:${INPUT_DIGEST:0:16}"
echo "==> Building ${IMAGE_NAME} from minimal context (${INPUT_DIGEST})..."
docker build --platform linux/386 \
    --build-arg "ALPINE_IMAGE=${ALPINE_IMAGE}" \
    --build-arg "APK_RESOLUTION=${APK_RESOLUTION}" \
    -t "${IMAGE_NAME}" \
    "${CONTEXT_DIR}"

if [[ "${IMAGE_ONLY}" == "1" ]]; then
    echo "==> Image-only smoke build complete: ${IMAGE_NAME}"
    exit 0
fi

echo "==> Exporting verified container filesystem..."
PUBLISH_DIR=$(mktemp -d "${OUTPUT_DIR}/.publish.XXXXXX")
CONTAINER_ID=$(docker create --platform linux/386 "${IMAGE_NAME}")
docker cp "${CONTAINER_ID}:/usr/local/share/gors/runtime/provider.json" \
    "${PUBLISH_DIR}/runtime-provider.json"
docker export "${CONTAINER_ID}" > "${PUBLISH_DIR}/rootfs.tar"
docker rm "${CONTAINER_ID}" > /dev/null
CONTAINER_ID=""

echo "==> Generating 9p filesystem..."
python3 "${TOOLS_DIR}/fs2json.py" \
    --out "${PUBLISH_DIR}/rootfs.json" "${PUBLISH_DIR}/rootfs.tar"
mkdir -p "${PUBLISH_DIR}/rootfs-flat"
python3 "${TOOLS_DIR}/copy-to-sha256.py" \
    "${PUBLISH_DIR}/rootfs.tar" "${PUBLISH_DIR}/rootfs-flat"
rm -f "${PUBLISH_DIR}/rootfs.tar"

python3 "${TOOLS_DIR}/write-manifest.py" \
    "${INPUT_DIGEST}" \
    "${PUBLISH_DIR}/runtime-provider.json" \
    "${PUBLISH_DIR}/rootfs.json" \
    "${PUBLISH_DIR}/rootfs-flat" \
    "${PUBLISH_DIR}/manifest.json"

# Preserve the previous complete publication until every replacement exists.
# The manifest is moved last and is the publication commit marker.
BACKUP_DIR=$(mktemp -d "${OUTPUT_DIR}/.previous.XXXXXX")
for name in rootfs-flat rootfs.json runtime-provider.json manifest.json; do
    if [[ -e "${OUTPUT_DIR}/${name}" ]]; then
        mv "${OUTPUT_DIR}/${name}" "${BACKUP_DIR}/${name}"
    fi
done
mv "${PUBLISH_DIR}/rootfs-flat" "${OUTPUT_DIR}/rootfs-flat"
mv "${PUBLISH_DIR}/rootfs.json" "${OUTPUT_DIR}/rootfs.json"
mv "${PUBLISH_DIR}/runtime-provider.json" "${PROVIDER_EXPORT}"
mv "${PUBLISH_DIR}/manifest.json" "${MANIFEST}"
PUBLICATION_COMMITTED=1
rm -rf "${BACKUP_DIR}"
BACKUP_DIR=""
rmdir "${PUBLISH_DIR}"
PUBLISH_DIR=""

rm -f "${OUTPUT_DIR}"/ext2-*.img.gz \
      "${OUTPUT_DIR}"/vmlinuz-*.bin \
      "${OUTPUT_DIR}"/initramfs-*.bin

echo "==> Done"
echo "    input digest: ${INPUT_DIGEST}"
echo "    rootfs.json: $(wc -c < "${OUTPUT_DIR}/rootfs.json" | tr -d ' ') bytes"
echo "    rootfs-flat: $(find "${OUTPUT_DIR}/rootfs-flat" -type f | wc -l | tr -d ' ') files"
