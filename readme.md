# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)

[gors](https://github.com/aymericbeaumet/gors) is an experimental Go-to-Rust
compiler written in Rust. It scans and parses Go source, builds typed HIR,
lowers executable semantics to verified Go MIR, reverifies representation-neutral
MIR transforms, performs mandatory Rust representation lowering into verified
Rust IR, and emits formatted Rust source.
Try it at
[gors.aymericbeaumet.com](https://gors.aymericbeaumet.com).

The compiler has completed a destructive architecture cutover. There is no
legacy backend or compatibility fallback. The current executable bootstrap
supports import-free programs with primitive values, free functions, scalar
expressions, assignments, `if`, and `for`; its current behavior claim is limited
to non-panicking executions, and unsupported Go constructs return a structured
diagnostic. Imports, composite types, methods, generics, Go-compatible panic
process behavior, and the Go stdlib are the active migration backlog. See [the architecture
audit](COMPILER_AUDIT.md) and [performance acceptance
contract](COMPILER_PERFORMANCE.md).

## Components

- Scanner and parser for Go source and AST construction
- Typed semantic HIR and explicit-order control-flow MIR
- Mandatory Rust representation lowering; its bootstrap policy copies `Copy`
  values and conservatively clones owned non-`Copy` values, while later proven
  move, borrow, ABI, and storage refinements remain owned by the same stage
- Verified Rust IR consumed by every terminal codegen path
- Terminal Rust `syn` emitter with no semantic syntax-repair passes
- Embedded Go SDK source metadata for future generic package compilation
- Rust source printer with Go-to-Rust source-map support
- Typed runtime contract plus one validated precompiled `gors-runtime` sidecar;
  generated Rust never embeds or recompiles runtime source
- CLI and browser/Wasm compiler surfaces

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
# Compile and atomically publish an optimized runnable executable.
gors build -o hello main.go

# Emit target-neutral Rust source for inspection.
gors emit-rust -o generated-rust main.go

# Transpile, compile, and run.
gors run main.go

# Advanced scanner/parser inspection.
gors tokens main.go
gors ast main.go
```

Values after `--` are forwarded to the generated program:

```bash
gors run main.go -- --flag value
```

For example:

```go
package main

func main() {
    total := 0
    for value := 0; value < 5; value++ {
        total += value
    }
    println(total)
}
```

```console
$ gors run sum.go
10
```

## Fast feedback

`build`, `emit-rust`, and `run` share one validated generated-Rust cache.
`build` and `run --release` also share the exact internal production
executable; changing `build -o` is only an atomic public copy and does not
relink. `emit-rust` resolves no runtime provider or Rust toolchain and exports
no terminal link descriptor. Runtime provider selection is terminal-only:
executable hits require the exact runtime link-plan identity and bypass provider
materialization and toolchain probing. Native provider production and
generated-program linking use the same exact pinned rustup toolchain, even when
Cargo itself was launched with another compiler.

`--timings-json timings.json` records phase durations and cache events after a
successful `build`, `emit-rust`, or `run`; `GORS_PROFILE=1` prints phase timings
to stderr.
`--jobs N` sets the compiler-owned worker budget. Ready per-definition work
already uses that bounded pool; parsing and finer semantic-query parallelism
remain part of the incremental compiler migration.

## Development

The workspace pins Rust 1.96.0 and its Go SDK. The Rust build downloads and
verifies that SDK under `$CARGO_HOME/gors-cache/`; do not substitute a system Go
toolchain for integration-oracle results.

```bash
# Build, lint, and unit gates for the cutover backend.
make rust-build rust-lint rust-test-unit

# Stable deterministic corpus/property replay.
make fuzz-test

# Browser development server.
make dev
```

For a focused [Go specification](https://go.dev/ref/spec) fixture:

```bash
make rust-test-integration-go-spec-fixture FIXTURE=assignment_two_phase
```

The generated-program oracle compares the pinned Go program with generated
Rust. Most integration fixtures currently produce explicit unsupported
diagnostics and remain the ordered migration backlog. Canonical conformance
reports are valid only after a complete, unfiltered run:

```bash
make conformance-report
make conformance-check
```

Browser compilation uses the same backend in a persistent single-threaded Wasm
worker. See [the Wasm notes](www/wasm/readme.md) and
[fuzzing guide](fuzz/readme.md) for details.

## License

MIT
