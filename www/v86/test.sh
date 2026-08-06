#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEMPORARY=$(mktemp -d "${TMPDIR:-/tmp}/gors-v86-tests.XXXXXX")
cleanup() {
    rm -rf "${TEMPORARY}"
}
trap cleanup EXIT

sh -n \
    "${SCRIPT_DIR}/rootfs/gors-compile" \
    "${SCRIPT_DIR}/rootfs/gors-run" \
    "${SCRIPT_DIR}/rootfs/gors-runtime-publish" \
    "${SCRIPT_DIR}/rootfs/gors-runtime-verify" \
    "${SCRIPT_DIR}/rootfs/gors-warmup"
bash -n "${SCRIPT_DIR}/build-image.sh"
python3 -m py_compile \
    "${SCRIPT_DIR}/tools/copy-to-sha256.py" \
    "${SCRIPT_DIR}/tools/fs2json.py" \
    "${SCRIPT_DIR}/tools/input-digest.py" \
    "${SCRIPT_DIR}/tools/rootfs_evidence.py" \
    "${SCRIPT_DIR}/tools/verify-manifest.py" \
    "${SCRIPT_DIR}/tools/write-manifest.py"

[[ -x "${SCRIPT_DIR}/rootfs/gors-run" ]]
grep -q 'COPY --chmod=755 rootfs/gors-run /usr/local/bin/gors-run' \
    "${SCRIPT_DIR}/Dockerfile"
grep -Fq '${ID}.${NONCE}.run.status' "${SCRIPT_DIR}/rootfs/gors-run"
grep -Fq "printf 'GORS_RUN_DONE:%s\\n' \"\$NONCE\"" \
    "${SCRIPT_DIR}/rootfs/gors-run"
grep -Fq '32-character lowercase hexadecimal nonce' \
    "${SCRIPT_DIR}/rootfs/gors-compile"
python3 - \
    "${SCRIPT_DIR}/boot-contract.json" \
    "${SCRIPT_DIR}/rootfs/gors-compile" \
    "${SCRIPT_DIR}/rootfs/gors-run" \
    "${SCRIPT_DIR}/rootfs/gors-warmup" \
    "${SCRIPT_DIR}/../v86-boot-manifest-build.js" \
    "${SCRIPT_DIR}/../webpack.config.js" <<'PY'
import json
from pathlib import Path
import re
import sys

contract = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
compile_script = Path(sys.argv[2]).read_text(encoding="utf-8")
run_script = Path(sys.argv[3]).read_text(encoding="utf-8")
warmup_script = Path(sys.argv[4]).read_text(encoding="utf-8")
manifest_builder = Path(sys.argv[5]).read_text(encoding="utf-8")
webpack_config = Path(sys.argv[6]).read_text(encoding="utf-8")
protocol = contract["guestProtocol"]
vm = contract["vm"]

assert contract["schemaVersion"] == 1
assert protocol["schemaVersion"] == 1
assert protocol["compileCommand"] == "gors-compile"
assert protocol["runCommand"] == "gors-run"
assert protocol["jobDirectory"] == "tmp"
assert protocol["nonceHexLength"] == 32
assert protocol["bootReadyMarker"] in warmup_script
assert "printf '\\nGORS_BOOT_READY\\n'" in warmup_script
assert "/usr/local/bin/gors-runtime-verify" not in warmup_script
assert '"--smoke"' in warmup_script
assert "rustc --crate-name gors_warmup" in warmup_script
assert "/usr/local/bin/gors-runtime-verify" in compile_script
assert protocol["compileDonePrefix"] in compile_script
assert protocol["runDonePrefix"] in run_script
assert str(protocol["nonceHexLength"]) in compile_script
assert str(protocol["nonceHexLength"]) in run_script
assert vm["memorySizeBytes"] > 0
assert vm["maxSavedStateBytes"] >= vm["memorySizeBytes"]
assert "slice(0, 16)" not in manifest_builder
assert re.search(r"rootfs-\$\{rootfsPublication\.rootfs\.indexSha256\}", manifest_builder)
assert "v86/tools/verify-manifest.py" in webpack_config
assert "rootfsPublication.inputDigest" in webpack_config
assert "verifyEmittedV86BootAssets" in webpack_config
assert "PROCESS_ASSETS_STAGE_SUMMARIZE" in webpack_config
PY
grep -q '/usr/local/share/gors/runtime/producer.json' "${SCRIPT_DIR}/Dockerfile"
grep -q "'.producer_identity'" "${SCRIPT_DIR}/rootfs/gors-runtime-publish"
grep -q "'.producer_identity'" "${SCRIPT_DIR}/rootfs/gors-runtime-verify"

if [[ "$(grep -o -- '--extern' "${SCRIPT_DIR}/rootfs/gors-compile" | wc -l | tr -d ' ')" != "1" ]]; then
    echo "gors-compile must pass exactly one --extern" >&2
    exit 1
fi
if grep -Eq 'gors-runtime/src|cargo build|crate-type[[:space:]]+rlib' \
    "${SCRIPT_DIR}/rootfs/gors-compile"; then
    echo "gors-compile contains a forbidden source or on-demand runtime build path" >&2
    exit 1
fi
grep -q 'gors-runtime-dependency-validator' "${SCRIPT_DIR}/rootfs/gors-compile"
grep -q 'ALPINE_IMAGE_ID' "${SCRIPT_DIR}/build-image.sh"
grep -q 'apk-resolution=' "${SCRIPT_DIR}/build-image.sh"
grep -q 'verify-manifest.py' "${SCRIPT_DIR}/build-image.sh"
if grep -q 'RUST_VERSION' "${SCRIPT_DIR}/Dockerfile" "${SCRIPT_DIR}/build-image.sh"; then
    echo "V86 must select rustc from its digest-bound target package resolution" >&2
    exit 1
fi
grep -Fq -- '--build-arg "APK_RESOLUTION=${APK_RESOLUTION}"' \
    "${SCRIPT_DIR}/build-image.sh"
if [[ "$(grep -Fc 'test -n "${APK_RESOLUTION}"' "${SCRIPT_DIR}/Dockerfile")" != "3" ]]; then
    echo "every apk stage must consume the resolved package-set cache key" >&2
    exit 1
fi
python3 - "${SCRIPT_DIR}/Dockerfile" <<'PY'
from pathlib import Path
import sys

dockerfile = Path(sys.argv[1]).read_text(encoding="utf-8")
post_strip = dockerfile.split(
    "strip --strip-unneeded /usr/local/libexec/gors-runtime-dependency-validator",
    maxsplit=1,
)[1]
assert "/usr/local/bin/gors-runtime-publish" in post_strip
assert "/usr/local/bin/gors-runtime-verify" in post_strip
assert "/usr/local/bin/gors-warmup --smoke" in post_strip
PY
grep -Fq 'mktemp "${RUNTIME_DIR}/.provider.json.XXXXXX"' \
    "${SCRIPT_DIR}/rootfs/gors-runtime-publish"
python3 - "${SCRIPT_DIR}/build-image.sh" <<'PY'
from pathlib import Path
import sys

build = Path(sys.argv[1]).read_text(encoding="utf-8")
assert '.build.lock' in build
publish = build.split("# The manifest is moved last", maxsplit=1)[1]
assert publish.index('mv "${PUBLISH_DIR}/runtime-provider.json"') < publish.index(
    'mv "${PUBLISH_DIR}/manifest.json"'
)
PY

cargo build --quiet --package gors-runtime-abi \
    --example runtime_provider_manifest \
    --example runtime_dependency_validator
PROVIDER_TOOL="${REPOSITORY_ROOT}/target/debug/examples/runtime_provider_manifest"
REQUEST_TOOL="${REPOSITORY_ROOT}/target/debug/examples/runtime_dependency_validator"

rustc -vV >"${TEMPORARY}/rustc-vV"
printf '%s' 'test runtime payload' >"${TEMPORARY}/runtime.rlib"
mkdir -p "${TEMPORARY}/target-libdir"
printf '%s' 'test target library' >"${TEMPORARY}/target-libdir/libtest-0123456789abcdef.rlib"
"${PROVIDER_TOOL}" \
    auto \
    "${TEMPORARY}/rustc-vV" \
    "${TEMPORARY}/target-libdir" \
    i586-alpine-linux-musl \
    32 \
    little \
    "${TEMPORARY}/runtime.rlib" \
    "${TEMPORARY}/provider-a.json"
"${PROVIDER_TOOL}" \
    auto \
    "${TEMPORARY}/rustc-vV" \
    "${TEMPORARY}/target-libdir" \
    i586-alpine-linux-musl \
    32 \
    little \
    "${TEMPORARY}/runtime.rlib" \
    "${TEMPORARY}/provider-b.json"
cmp "${TEMPORARY}/provider-a.json" "${TEMPORARY}/provider-b.json"

python3 - "${TEMPORARY}/provider-a.json" "${TEMPORARY}/runtime.rlib" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

provider = json.loads(pathlib.Path(sys.argv[1]).read_text())
payload = pathlib.Path(sys.argv[2]).read_bytes()
assert provider["schema_version"] == 2
assert provider["runtime_dependency_schema_version"] == 1
assert re.fullmatch(r"[0-9a-f]{64}", provider["contract"])
assert provider["target_triple"] == "i586-alpine-linux-musl"
assert provider["target_pointer_width"] == 32
assert provider["target_endianness"] == "little"
assert provider["provided_capabilities"] == ["standard_io"]
assert provider["format"] == "rust-rlib-v1"
assert provider["extern_crate"] == "__gors_runtime"
assert provider["implementation_hash"] == hashlib.sha256(payload).hexdigest()
release_record = bytes.fromhex(provider["rustc_release_record_hex"])
assert b"rustc " in release_record
assert b"host: " not in release_record
assert re.fullmatch(r"(?:[0-9a-f]{2})+", provider["target_libdir_record_hex"])
assert re.fullmatch(r"[0-9a-f]{64}", provider["producer_identity"])
assert re.fullmatch(r"[0-9a-f]{64}", provider["compatibility_identity"])
assert re.fullmatch(r"[0-9a-f]{64}", provider["artifact_identity"])
assert "toolchain_identity" not in provider
assert provider["supported_operation_ids"] == [
    1, 2, 3, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34,
    35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
]
PY

PRODUCER_IDENTITY=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["producer_identity"])' \
    "${TEMPORARY}/provider-a.json")
printf '%s' 'changed target library' \
    >"${TEMPORARY}/target-libdir/libtest-0123456789abcdef.rlib"
"${PROVIDER_TOOL}" \
    "${PRODUCER_IDENTITY}" \
    "${TEMPORARY}/rustc-vV" \
    "${TEMPORARY}/target-libdir" \
    i586-alpine-linux-musl \
    32 \
    little \
    "${TEMPORARY}/runtime.rlib" \
    "${TEMPORARY}/provider-final.json"
python3 - "${TEMPORARY}/provider-a.json" "${TEMPORARY}/provider-final.json" <<'PY'
import json
from pathlib import Path
import sys

builder = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
final = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
assert final["producer_identity"] == builder["producer_identity"]
assert final["compatibility_identity"] != builder["compatibility_identity"]
assert final["artifact_identity"] != builder["artifact_identity"]
PY

CONTRACT=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["contract"])' \
    "${TEMPORARY}/provider-a.json")
SUPPORTED=1,2,3,8,9,10,11,13,14,15,16,17,18,19,20
validate() {
    "${REQUEST_TOOL}" "$1" 1 "${CONTRACT}" "${SUPPORTED}"
}
reject() {
    if validate "$1" > /dev/null 2>&1; then
        echo "invalid runtime request was accepted: $(cat "$1")" >&2
        exit 1
    fi
}

printf '{"schema_version":1,"contract":"%s","operation_ids":[2,14,16]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/valid.json"
validate "${TEMPORARY}/valid.json"

printf '{"schema_version":1,"schema_version":1,"contract":"%s","operation_ids":[]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/duplicate-key.json"
printf '{"schema_version":1,"contract":"%s","operation_ids":[],"extra":0}\n' \
    "${CONTRACT}" >"${TEMPORARY}/extra-key.json"
printf '{"schema_version":1,"contract":"%s","operation_ids":[14,2]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/unsorted.json"
printf '{"schema_version":1,"contract":"%s","operation_ids":[2,2]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/duplicate-op.json"
printf '{"schema_version":1,"contract":"%s","operation_ids":[4]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/unknown-op.json"
printf '{"schema_version":1,"contract":"%s","operation_ids":[],}\n' \
    "${CONTRACT}" >"${TEMPORARY}/trailing-comma.json"
printf '{"schema_version":1.0,"contract":"%s","operation_ids":[]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/float-schema.json"
printf '{"schema_version":1,"contract":"%s"}\n' \
    "${CONTRACT}" >"${TEMPORARY}/missing-key.json"
printf '{"schema_\\u0076ersion":1,"contract":"%s","operation_ids":[]}\n' \
    "${CONTRACT}" >"${TEMPORARY}/escaped-key.json"
printf '{"schema_version":1,"contract":"%064d","operation_ids":[]}\n' \
    0 >"${TEMPORARY}/wrong-contract.json"
python3 - "${TEMPORARY}/oversized.json" <<'PY'
from pathlib import Path
import sys

Path(sys.argv[1]).write_bytes(b" " * 4097)
PY
for request in \
    duplicate-key extra-key unsorted duplicate-op unknown-op trailing-comma \
    float-schema missing-key escaped-key wrong-contract oversized; do
    reject "${TEMPORARY}/${request}.json"
done

mkdir -p "${TEMPORARY}/digest"
printf 'one\n' >"${TEMPORARY}/digest/input"
DIGEST_A=$(python3 "${SCRIPT_DIR}/tools/input-digest.py" \
    "${TEMPORARY}/digest" input '--fact=base=one')
DIGEST_B=$(python3 "${SCRIPT_DIR}/tools/input-digest.py" \
    "${TEMPORARY}/digest" input '--fact=base=one')
[[ "${DIGEST_A}" == "${DIGEST_B}" ]]
printf 'two\n' >"${TEMPORARY}/digest/input"
DIGEST_CONTENT=$(python3 "${SCRIPT_DIR}/tools/input-digest.py" \
    "${TEMPORARY}/digest" input '--fact=base=one')
DIGEST_FACT=$(python3 "${SCRIPT_DIR}/tools/input-digest.py" \
    "${TEMPORARY}/digest" input '--fact=base=two')
[[ "${DIGEST_A}" != "${DIGEST_CONTENT}" ]]
[[ "${DIGEST_CONTENT}" != "${DIGEST_FACT}" ]]

mkdir -p "${TEMPORARY}/rootfs-source/nested" "${TEMPORARY}/rootfs-flat"
printf 'alpha\n' >"${TEMPORARY}/rootfs-source/alpha"
printf 'beta\n' >"${TEMPORARY}/rootfs-source/nested/beta"
COPYFILE_DISABLE=1 tar -cf "${TEMPORARY}/rootfs.tar" \
    -C "${TEMPORARY}/rootfs-source" .
if tar -tf "${TEMPORARY}/rootfs.tar" | grep -Eq '(^|/)\._'; then
    echo "rootfs fixture unexpectedly contains macOS AppleDouble metadata" >&2
    exit 1
fi
python3 "${SCRIPT_DIR}/tools/fs2json.py" \
    --out "${TEMPORARY}/rootfs.json" "${TEMPORARY}/rootfs.tar"
python3 "${SCRIPT_DIR}/tools/copy-to-sha256.py" \
    "${TEMPORARY}/rootfs.tar" "${TEMPORARY}/rootfs-flat"
python3 - "${TEMPORARY}/rootfs.json" "${TEMPORARY}/rootfs-flat" <<'PY'
import hashlib
import json
from pathlib import Path
import re
import stat
import sys

manifest = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
blob_directory = Path(sys.argv[2])
keys = []


def collect(nodes):
    for node in nodes:
        mode = node[3]
        if stat.S_ISDIR(mode):
            collect(node[6])
        elif stat.S_ISREG(mode):
            keys.append(node[6])


assert manifest["version"] == 3
collect(manifest["fsroot"])
assert len(keys) == 2
assert len(set(keys)) == 2
for key in keys:
    assert re.fullmatch(r"[0-9a-f]{64}\.bin", key), key
    payload = (blob_directory / key).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == key.removesuffix(".bin")
assert sorted(path.name for path in blob_directory.iterdir()) == sorted(keys)
PY

PUBLICATION_MANIFEST="${TEMPORARY}/manifest.json"
python3 "${SCRIPT_DIR}/tools/write-manifest.py" \
    "${DIGEST_A}" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs.json" \
    "${TEMPORARY}/rootfs-flat" \
    "${PUBLICATION_MANIFEST}"
python3 "${SCRIPT_DIR}/tools/verify-manifest.py" \
    "${DIGEST_A}" \
    "${PUBLICATION_MANIFEST}" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs.json" \
    "${TEMPORARY}/rootfs-flat" \
    verbose
mkdir -p "${TEMPORARY}/rootfs-flat-empty"
printf '%s\n' '{"fsroot":[],"size":0,"version":3}' \
    >"${TEMPORARY}/rootfs-empty.json"
if python3 "${SCRIPT_DIR}/tools/write-manifest.py" \
    "${DIGEST_A}" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs-empty.json" \
    "${TEMPORARY}/rootfs-flat-empty" \
    "${TEMPORARY}/manifest-empty.json" > /dev/null 2>&1; then
    echo "empty V86 rootfs publication was accepted" >&2
    exit 1
fi
if python3 "${SCRIPT_DIR}/tools/verify-manifest.py" \
    "${DIGEST_CONTENT}" \
    "${PUBLICATION_MANIFEST}" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs.json" \
    "${TEMPORARY}/rootfs-flat" \
    quiet; then
    echo "stale V86 input digest was accepted" >&2
    exit 1
fi
python3 - "${PUBLICATION_MANIFEST}" <<'PY'
import json
from pathlib import Path
import re
import sys

manifest = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
assert manifest["schemaVersion"] == 1
assert manifest["type"] == "9p"
assert manifest["rootfs"]["schemaVersion"] == 1
assert manifest["rootfs"]["blobCount"] == 2
assert re.fullmatch(r"[0-9a-f]{64}", manifest["rootfs"]["indexSha256"])
assert re.fullmatch(r"[0-9a-f]{64}", manifest["rootfs"]["blobSetIdentity"])
assert re.fullmatch(r"[0-9a-f]{64}", manifest["runtimeProviderSha256"])
PY

python3 - "${TEMPORARY}" <<'PY'
import hashlib
import json
from pathlib import Path
import shutil
import sys

root = Path(sys.argv[1])
source = root / "rootfs-flat"
keys = sorted(path.name for path in source.iterdir())
for case in ("missing", "truncated", "mutated", "extra"):
    shutil.copytree(source, root / f"rootfs-flat-{case}")

(root / "rootfs-flat-missing" / keys[0]).unlink()
truncated = root / "rootfs-flat-truncated" / keys[0]
truncated.write_bytes(truncated.read_bytes()[:-1])
mutated = root / "rootfs-flat-mutated" / keys[0]
payload = bytearray(mutated.read_bytes())
payload[0] ^= 0x01
mutated.write_bytes(payload)
extra_payload = b"unreferenced but correctly content-addressed blob"
extra_key = hashlib.sha256(extra_payload).hexdigest() + ".bin"
(root / "rootfs-flat-extra" / extra_key).write_bytes(extra_payload)

index = root / "rootfs.json"
(root / "rootfs-mutated.json").write_bytes(index.read_bytes() + b"\n")
provider = root / "provider-a.json"
(root / "provider-mutated.json").write_bytes(provider.read_bytes() + b"\n")
manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
manifest["rootfs"]["blobCount"] += 1
(root / "manifest-mutated.json").write_text(
    json.dumps(manifest, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

reject_publication() {
    local label=$1
    local manifest=$2
    local provider=$3
    local index=$4
    local blobs=$5
    if python3 "${SCRIPT_DIR}/tools/verify-manifest.py" \
        "${DIGEST_A}" "$manifest" "$provider" "$index" "$blobs" quiet; then
        echo "corrupt V86 publication was accepted: ${label}" >&2
        exit 1
    fi
}
for case in missing truncated mutated extra; do
    reject_publication \
        "$case" \
        "${PUBLICATION_MANIFEST}" \
        "${TEMPORARY}/provider-a.json" \
        "${TEMPORARY}/rootfs.json" \
        "${TEMPORARY}/rootfs-flat-${case}"
done
reject_publication \
    index-mutated \
    "${PUBLICATION_MANIFEST}" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs-mutated.json" \
    "${TEMPORARY}/rootfs-flat"
reject_publication \
    provider-mutated \
    "${PUBLICATION_MANIFEST}" \
    "${TEMPORARY}/provider-mutated.json" \
    "${TEMPORARY}/rootfs.json" \
    "${TEMPORARY}/rootfs-flat"
reject_publication \
    manifest-mutated \
    "${TEMPORARY}/manifest-mutated.json" \
    "${TEMPORARY}/provider-a.json" \
    "${TEMPORARY}/rootfs.json" \
    "${TEMPORARY}/rootfs-flat"

echo "V86 runtime provider static tests passed"
