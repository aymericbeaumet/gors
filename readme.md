# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)

[gors](https://github.com/aymericbeaumet/gors) is an experimental Go toolchain
written in Rust. It scans and parses Go source, lowers the Go AST to a Rust
`syn` AST, applies compiler passes, and emits formatted Rust source. Try it at
[gors.aymericbeaumet.com](https://gors.aymericbeaumet.com).

The standard-library package graph is resolved and lowered from the pinned Go
SDK through the generic compiler pipeline. Compatibility work must improve
generic parsing, typing, lowering, reachability, or language/runtime
primitives; it must not reimplement a Go stdlib API in Rust or add
package/function-specific compiler behavior. Existing runtime/ABI and targeted
host-resource shims remain limited to the boundary documented in
[`AGENTS.md`](AGENTS.md).

## Components

- Scanner and parser for Go source and AST construction
- Go AST to Rust `syn` AST compiler
- Generic embedded Go SDK package resolution and reachability pruning
- Rust source printer with Go-to-Rust source-map support
- CLI and persistent browser/Wasm compiler surfaces

## Install

With Homebrew:

```bash
brew tap aymericbeaumet/tap
brew install gors
```

With Cargo:

```bash
cargo install --git https://github.com/aymericbeaumet/gors.git gors-cli
```

Or from a checkout:

```bash
cargo install --path gors-cli
```

## Usage

```bash
# Transpile into a multi-file Rust crate.
gors build --output generated-rust main.go

# Transpile, compile, and run.
gors run main.go

# Advanced scanner/parser inspection.
gors tokens main.go
gors ast main.go
```

`run` also accepts multiple files, directories, and module package paths. Values
after `--` are forwarded to the generated program:

```bash
gors run main.go -- --flag value
```

For example:

```go
package main

import "fmt"

func main() {
    fmt.Println("Hello, World!")
}
```

```console
$ gors run hello.go
Hello, World!
```

## Fast feedback

`build` and `run` cache validated compiler output; `run` also caches the compiled
Rust executable. The bounded cache invalidates on source/module, compiler, SDK,
CLI, toolchain, target, configuration, or output changes.

```bash
gors build --jobs 8 --timings-json timings.json main.go
gors run --jobs 8 --timings-json timings.json main.go
```

The job count resolves from `--jobs`, then `GORS_JOBS`, then available CPU
parallelism. `--timings-json` records phase durations and cache events after a
successful command; `GORS_PROFILE=1` prints phase timings to stderr.

## Development

The workspace pins Rust 1.96.0 and its Go SDK. The Rust build downloads and
verifies that SDK under `$CARGO_HOME/gors-cache/`; do not substitute a system Go
toolchain for integration-oracle results.

```bash
# Broad build, lint, unit, and integration gate.
make all

# Stable deterministic corpus/property replay.
make fuzz-test

# Browser development server.
make dev
```

For a focused [Go specification](https://go.dev/ref/spec) fixture:

```bash
make rust-test-integration-go-spec-fixture FIXTURE=assignment_two_phase
```

The generated-program oracle requires the pinned Go program and generated Rust
program to succeed, then compares stdout and stderr byte-for-byte. Canonical
reports are written only by complete, unfiltered runs:

```bash
make conformance-report
make conformance-check
```

Production browser compilation uses a persistent worker and stable
single-threaded Wasm. An opt-in cross-origin-isolated threaded preview is
available for local benchmarking:

```bash
npm --prefix www run serve:compiler-preview:threads
```

See [the Wasm notes](www/wasm/readme.md),
[threaded preview contract](www/wasm/threads-preview.md), and
[fuzzing guide](fuzz/readme.md) for details.

## License

MIT
