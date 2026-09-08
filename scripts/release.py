"""Native release packaging and admission, shared by mise and GitHub Actions."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux", "amd64"),
    "aarch64-unknown-linux-gnu": ("linux", "arm64"),
    "x86_64-apple-darwin": ("macos", "amd64"),
    "aarch64-apple-darwin": ("macos", "arm64"),
    "x86_64-pc-windows-msvc": ("windows", "amd64"),
    "aarch64-pc-windows-msvc": ("windows", "arm64"),
}
MAX_ARCHIVE_FILE = 512 * 1024 * 1024


def read_toml(path):
    with path.open("rb") as source:
        return tomllib.load(source)


def rust_version():
    version = read_toml(ROOT / "rust-toolchain.toml")["toolchain"]["channel"]
    configured = read_toml(ROOT / "mise.toml")["tools"]["rust"]["version"]
    if configured != version:
        raise ValueError("mise Rust version must match rust-toolchain.toml")
    return version


def release_version(requested):
    package = read_toml(ROOT / "gors-cli/Cargo.toml")["package"]["version"]
    expected = f"v{package}"
    value = requested or expected
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", value):
        raise ValueError(f"invalid release version: {value!r}")
    if value != expected:
        raise ValueError(f"release tag {value} does not match CLI package version {expected}")
    return value


def archive_stem(target, version):
    operating_system, architecture = TARGETS[target]
    return f"gors-{version}-{operating_system}-{architecture}"


def archive_path(directory, target, version):
    extension = ".zip" if TARGETS[target][0] == "windows" else ".tar.gz"
    return directory / (archive_stem(target, version) + extension)


def digest(payload):
    return hashlib.sha256(payload).hexdigest()


def file_digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def json_bytes(value):
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def write_checksum(archive):
    checksum = f"{file_digest(archive)}  {archive.name}\n"
    Path(f"{archive}.sha256").write_text(checksum, encoding="utf-8", newline="\n")


def smoke_scope(target):
    if TARGETS[target][0] == "windows":
        return ["version", "emit-rust"]
    return ["version", "emit-rust", "build", "warm-build", "run-release"]


def package(binary, go_license, target, version, directory):
    windows = TARGETS[target][0] == "windows"
    binary_name = "gors.exe" if windows else "gors"
    files = {
        binary_name: binary.read_bytes(),
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "GO-LICENSE": go_license.read_bytes(),
        "README.md": (ROOT / "docs/releasing.md").read_bytes(),
    }
    manifest = {
        "schema": 1,
        "version": version,
        "target": target,
        "rust_toolchain": rust_version(),
        "go_sdk": (ROOT / ".go-version").read_text().strip(),
        "smoke_scope": smoke_scope(target),
        "files": {name: digest(payload) for name, payload in files.items()},
    }
    files["RELEASE.json"] = json_bytes(manifest)
    directory.mkdir(parents=True, exist_ok=True)
    archive = archive_path(directory, target, version)
    stem = archive_stem(target, version)
    with tempfile.TemporaryDirectory(dir=directory) as temporary:
        output = Path(temporary) / archive.name
        if windows:
            with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for name, payload in sorted(files.items()):
                    member = zipfile.ZipInfo(f"{stem}/{name}", date_time=(1980, 1, 1, 0, 0, 0))
                    member.create_system = 3
                    member.external_attr = (stat.S_IFREG | 0o644) << 16
                    member.compress_type = zipfile.ZIP_DEFLATED
                    bundle.writestr(member, payload)
        else:
            with output.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as bundle:
                    for name, payload in sorted(files.items()):
                        member = tarfile.TarInfo(f"{stem}/{name}")
                        member.size = len(payload)
                        member.mode = 0o755 if name == binary_name else 0o644
                        bundle.addfile(member, io.BytesIO(payload))
        os.replace(output, archive)
    write_checksum(archive)
    Path(f"{archive}.smoke.json").unlink(missing_ok=True)
    return archive


def archive_files(archive, target, version):
    stem = archive_stem(target, version)
    binary_name = "gors.exe" if TARGETS[target][0] == "windows" else "gors"
    expected = {binary_name, "LICENSE", "GO-LICENSE", "README.md", "RELEASE.json"}
    files = {}

    def admit(name, size, regular):
        relative = name.removeprefix(f"{stem}/")
        if not regular or relative not in expected or name != f"{stem}/{relative}" or relative in files:
            raise ValueError(f"invalid archive member {name!r}")
        if size <= 0 or size > MAX_ARCHIVE_FILE:
            raise ValueError(f"invalid archive member size for {name!r}")
        return relative

    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.infolist():
                mode = member.external_attr >> 16
                name = admit(member.filename, member.file_size, stat.S_ISREG(mode))
                files[name] = bundle.read(member)
    else:
        with tarfile.open(archive) as bundle:
            for member in bundle:
                name = admit(member.name, member.size, member.isfile())
                if name == binary_name and member.mode != 0o755:
                    raise ValueError("archive binary must have executable mode 0755")
                files[name] = bundle.extractfile(member).read()
    if files.keys() != expected:
        raise ValueError(f"archive file set is incomplete: {archive.name}")
    return files


def inspect_archive(archive, target, version):
    if not archive.is_file():
        raise ValueError(f"missing archive: {archive}")
    checksum = Path(f"{archive}.sha256")
    expected = f"{file_digest(archive)}  {archive.name}\n"
    if not checksum.is_file() or checksum.read_text(encoding="utf-8") != expected:
        raise ValueError(f"archive checksum mismatch: {archive.name}")
    files = archive_files(archive, target, version)
    manifest = json.loads(files.pop("RELEASE.json"))
    if manifest.get("schema") != 1 or manifest.get("target") != target or manifest.get("version") != version:
        raise ValueError(f"archive release identity mismatch: {archive.name}")
    if manifest.get("files") != {name: digest(payload) for name, payload in files.items()}:
        raise ValueError(f"archive member content hash mismatch: {archive.name}")
    if manifest.get("smoke_scope") != smoke_scope(target):
        raise ValueError(f"archive smoke scope mismatch: {archive.name}")
    if manifest.get("rust_toolchain") != rust_version() or manifest.get("go_sdk") != (ROOT / ".go-version").read_text().strip():
        raise ValueError(f"archive toolchain identity mismatch: {archive.name}")
    return manifest


def command(arguments, **kwargs):
    print("+ " + " ".join(map(str, arguments)), flush=True)
    return subprocess.run(arguments, check=True, text=True, **kwargs)


def native_target(target):
    result = command(["rustup", "run", rust_version(), "rustc", "-vV"], capture_output=True)
    host = next((line.removeprefix("host: ") for line in result.stdout.splitlines() if line.startswith("host: ")), None)
    if host != target:
        raise ValueError(f"release builds and smoke tests require native target {target}; rustc host is {host}")


def build(target, version, directory):
    native_target(target)
    arguments = ["rustup", "run", rust_version(), "cargo", "build", "--locked", "--release", "--package", "gors-cli", "--bin", "gors", "--target", target, "--message-format=json-render-diagnostics"]
    print("+ " + " ".join(arguments), flush=True)
    binary = None
    sdk_path = None
    with subprocess.Popen(arguments, cwd=ROOT, stdout=subprocess.PIPE, text=True) as process:
        for line in process.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                print(line, end="", flush=True)
                continue
            if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "gors" and message.get("executable"):
                binary = Path(message["executable"])
            elif message.get("reason") == "build-script-executed":
                for name, value in message.get("env", []):
                    if name == "GORS_BUILT_GO_SDK_PATH":
                        sdk_path = Path(value)
            elif message.get("reason") == "compiler-message":
                print(message.get("message", {}).get("rendered", ""), end="", flush=True)
        if process.wait() != 0:
            raise subprocess.CalledProcessError(process.returncode, arguments)
    if binary is None or sdk_path is None:
        raise ValueError("Cargo did not report the gors executable and embedded Go SDK provenance")
    archive = package(binary, sdk_path / "LICENSE", target, version, directory)
    print(archive, flush=True)


def write_smoke_receipt(archive, target, version):
    receipt = {"schema": 1, "target": target, "version": version, "archive_sha256": file_digest(archive), "checks": smoke_scope(target)}
    Path(f"{archive}.smoke.json").write_bytes(json_bytes(receipt))


def smoke(target, version, directory):
    native_target(target)
    archive = archive_path(directory, target, version)
    receipt = Path(f"{archive}.smoke.json")
    receipt.unlink(missing_ok=True)
    inspect_archive(archive, target, version)
    windows = TARGETS[target][0] == "windows"
    with tempfile.TemporaryDirectory(prefix="gors-release-smoke-") as temporary:
        working = Path(temporary)
        binary = working / ("gors.exe" if windows else "gors")
        for name, payload in archive_files(archive, target, version).items():
            (working / name).write_bytes(payload)
        binary.chmod(0o755)
        environment = os.environ.copy()
        environment.pop("GORS_GO_SDK_PATH", None)
        environment["XDG_CACHE_HOME"] = str(working / "cache")
        environment["LOCALAPPDATA"] = str(working / "cache")
        arguments = {"cwd": working, "env": environment, "capture_output": True}
        result = command([str(binary), "version"], **arguments)
        operating_system, architecture = TARGETS[target]
        go_os = "darwin" if operating_system == "macos" else operating_system
        if f"gors{version.removeprefix('v')} " not in result.stdout or f"{go_os}/{architecture}" not in result.stdout:
            raise ValueError(f"packaged CLI version/target mismatch: {result.stdout}")
        source = working / "main.go"
        source.write_text('package main\nfunc main() { total := 0; for i := 0; i < 5; i++ { total += i }; println(total) }\n', encoding="utf-8")
        command([str(binary), "emit-rust", "-o", "generated", "main.go"], **arguments)
        generated = (working / "generated/main.rs").read_text(encoding="utf-8")
        if "fn main" not in generated or "__gors_runtime" not in generated:
            raise ValueError("packaged CLI did not emit the expected Rust program and runtime dependency")
        if not windows:
            executable = working / "smoke-program"
            for _ in range(2):
                command([str(binary), "build", "-o", str(executable), "main.go"], **arguments)
                result = command([str(executable)], **arguments)
                if result.stdout + result.stderr != "10\n":
                    raise ValueError(f"packaged build produced incorrect output: {result}")
            result = command([str(binary), "run", "--release", "main.go"], **arguments)
            if result.stdout + result.stderr != "10\n":
                raise ValueError(f"packaged run produced incorrect output: {result}")
    write_smoke_receipt(archive, target, version)
    print(f"Passed packaged smoke for {target}: {', '.join(smoke_scope(target))}", flush=True)


def validate(directory, version):
    checksums = []
    expected_files = {"SHA256SUMS"}
    for target in TARGETS:
        archive = archive_path(directory, target, version)
        inspect_archive(archive, target, version)
        receipt_path = Path(f"{archive}.smoke.json")
        expected = {"schema": 1, "target": target, "version": version, "archive_sha256": file_digest(archive), "checks": smoke_scope(target)}
        if not receipt_path.is_file() or json.loads(receipt_path.read_bytes()) != expected:
            raise ValueError(f"missing or stale smoke receipt: {archive.name}")
        checksums.append(f"{file_digest(archive)}  {archive.name}\n")
        expected_files.update({archive.name, f"{archive.name}.sha256", receipt_path.name})
    unexpected = {path.name for path in directory.iterdir()} - expected_files
    if unexpected:
        raise ValueError(f"unexpected release files: {sorted(unexpected)}")
    output = directory / "SHA256SUMS"
    output.write_text("".join(sorted(checksums)), encoding="utf-8", newline="\n")
    print(f"Verified six release archives and smoke receipts: {output}", flush=True)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "smoke", "validate", "check-tag"])
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--version")
    parser.add_argument("--directory", type=Path, default=ROOT / "dist")
    options = parser.parse_args()
    version = release_version(options.version)
    if options.action == "check-tag":
        print(version)
    elif options.action == "validate":
        validate(options.directory.resolve(), version)
    else:
        if options.target is None:
            parser.error("--target is required for build and smoke")
        {"build": build, "smoke": smoke}[options.action](options.target, version, options.directory.resolve())


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"release: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError):
            if error.stdout:
                print(error.stdout, file=sys.stderr)
            if error.stderr:
                print(error.stderr, file=sys.stderr)
        sys.exit(1)
