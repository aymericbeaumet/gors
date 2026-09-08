# Native releases

Release archives contain `gors` (`gors.exe` on Windows), the project and Go SDK
licenses, this guide, and `RELEASE.json` with target, toolchain, and file hashes.
The CLI currently embeds its selected Go SDK sources and its precompiled runtime
provider. No separate Go installation or SDK source directory is required after
installation. Runtime materialization is handled by the CLI when linking a
generated program.

Extract the archive and put its executable on `PATH`. `gors version` identifies
the compiler, Go SDK, target, and runtime contract. `gors emit-rust -o generated
main.go` exports Rust without resolving a Rust compiler or linker.

On Linux and macOS, `gors build -o program main.go` and `gors run main.go` also
require Rust **1.96.0 installed through rustup**, its unmodified native target
standard libraries, and the platform C linker/toolchain. Use `rustup toolchain
install 1.96.0 --profile minimal`. Linux archives target GNU/Linux and are built
on Ubuntu 24.04; compatibility with older glibc distributions is not promised.
On macOS, install the Xcode Command Line Tools. The compiler admits the selected
toolchain and runtime compatibility before linking.

Windows archives provide source compilation (`emit-rust`) and inspection. Native
`build` and `run` remain unavailable because executable publication and terminal
toolchain admission currently fail closed on non-Unix hosts. Windows smoke
checks deliberately cover only `version` and `emit-rust`; their manifests and
smoke receipts record that scope. A successful Windows release build does not
claim native program execution support.

## Build and verify

[mise](https://mise.jdx.dev/) pins Rust and Python in `mise.toml`; its native Rust
backend installs and selects Rust through rustup. No Python packages are needed.
Install mise and the platform's native linker prerequisites, then run:

```sh
mise install
mise run release:test
mise run release:build --target aarch64-apple-darwin
mise run release:smoke --target aarch64-apple-darwin
```

Windows source builds require the Visual Studio C++ build tools and Windows SDK
with the native target's developer environment active. The ARM64 build also
needs `clang-cl` on `PATH` for the TLS dependency, and sets `CC` to that compiler.
See the dependency's [Windows build requirements](https://aws.github.io/aws-lc-rs/requirements/windows.html).

Each target is built and smoke-tested on a native runner:

| Platform | Architecture | Rust target | GitHub runner |
| --- | --- | --- | --- |
| Linux | amd64 | `x86_64-unknown-linux-gnu` | `ubuntu-24.04` |
| Linux | arm64 | `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| macOS | amd64 | `x86_64-apple-darwin` | `macos-15-intel` |
| macOS | arm64 | `aarch64-apple-darwin` | `macos-15` |
| Windows | amd64 | `x86_64-pc-windows-msvc` | `windows-2025` |
| Windows | arm64 | `aarch64-pc-windows-msvc` | `windows-11-arm` |

The version defaults to `v` plus `gors-cli/Cargo.toml`'s package version. An
explicit `--version` must match that version, including a prerelease suffix.
`release:check-tag --version v0.1.0` performs this check without building. The
build uses Cargo's locked release profile and collects the exact executable and
Go SDK license path from Cargo's JSON output.

Archives are written to `dist/gors-v<version>-<platform>-<architecture>.tar.gz`
(Unix) or `.zip` (Windows), with a `.sha256` file. Archive timestamps, ordering,
and permissions are normalized; this makes packaging deterministic for identical
input bytes, without claiming reproducible native compiler builds.

Smoke tests validate the archive and extract it into a temporary directory
outside the checkout. They check the executable's version and target and compile
a loop into Rust. Unix also builds and executes the packaged compiler's output,
repeats the public build, and runs the release profile. Only success writes the
archive-bound `.smoke.json` receipt. Repackaging deletes an old receipt.

After collecting all six jobs' output into one flat directory, run:

```sh
mise run release:validate --directory dist
```

Validation rejects missing targets, wrong versions, stale smoke receipts,
unexpected files, malformed archive entries, and mismatched archive or member
hashes. It writes the aggregate `dist/SHA256SUMS` only after every target passes.
Publish the six archives and `SHA256SUMS` together. Local validation and manual
workflow runs need no new branch or release tag; publication is a separate
tag-triggered workflow step.

## Publish

The release workflow runs the six-platform build and validation for relevant
pull requests and manual dispatches without publishing. Pushing a `v*` tag
publishes only when its version matches the CLI package and its commit is
reachable from `main`. Update the package version and merge the release changes
before tagging. Publication uploads the complete validated asset set to a draft,
then publishes it; an already published release cannot be overwritten by reruns.

Publication finishes after the six native archives and `SHA256SUMS` are public.
There is no Homebrew formula or tap update in this release pipeline; install from
the native archives. Building a release does not create a persistent branch.
