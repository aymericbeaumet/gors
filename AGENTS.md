# AGENTS.md — Guidelines for AI Agents

> **Keep this file current.** When you make architectural decisions, discover
> non-obvious constraints, or learn something that would save a future agent
> time, update the relevant section below.

## Project overview

gors is a Go-to-Rust transpiler written in Rust. It parses Go source code into
an AST, compiles it to a Rust `syn` AST, applies transformation passes, and
generates formatted Rust source code.

Pipeline: Go source → scanner → parser → Go AST → compiler → Rust AST → passes → backend → Rust source

## Repository layout

```
src/
  scanner/       # Go lexer (token stream)
  parser/        # Go parser (Go AST), import resolution, go.mod support
  ast/           # Go AST data structures
  compiler/      # Go AST → Rust syn AST conversion + transformation passes
    passes/      # Post-compilation Rust→Rust AST transforms
    manifest.rs  # Build manifest for incremental compilation
  backend_rust/  # syn AST → formatted Rust source via prettyplease
  stdlib/        # Hand-written Rust implementations of Go stdlib packages
  toolchain/     # Hermetic Go toolchain download and management
  mapping/       # Source map tracking (Go ↔ Rust position mapping)
  token/         # Go token types
  error.rs       # Diagnostic formatting
  main.rs        # CLI: ast, build, run, tokens subcommands
  lib.rs         # Library entrypoint

tests/
  programs.rs    # Program execution tests (compile Go → run Rust, compare output)
  lexer_parser.rs # Lexer/parser conformance vs Go reference
  common.rs      # Shared test infrastructure
  fixtures/
    go_programs/ # Test programs (auto-discovered, each dir = one test)
    go_sources/  # Go source files for lexer/parser conformance
```

## Compilation model

### Multi-file output (current)

`compile_program_multi()` produces a `CompiledProgram` with individual modules:
- Each Go package → individual `.rs` file
- Naming: `import_path.replace('/', "__")` + `.rs` (e.g., `example/math` → `example__math.rs`)
- `lib.rs` declares all modules with `#[path]` attributes
- `main.rs` includes `lib.rs` and contains main function items
- Stdlib modules (e.g., `fmt`) are hand-written Rust, not transpiled from Go

### Cross-module references

- `prefix_sibling_paths` rewrites references to sibling packages as `crate::pkg::Symbol`
- `hoist_use` lifts multi-segment paths to `use` statements (only for main package)
- `hoist_use` detects name collisions and keeps paths qualified when ambiguous

### Incremental builds

- `.gors_manifest.json` tracks content hashes per module
- `compute_content_hash()` concatenates sorted Go source files → SHA-256
- Unchanged modules are skipped during `build`

## Stdlib system

Go stdlib imports are resolved via hand-written Rust modules in `src/stdlib/`.
Currently supported: `fmt` (Println, Print as generic functions).

The `ParsedProgram.stdlib_imports` field tracks which stdlib packages a program
uses. `compile_program_multi()` emits these as individual `.rs` files alongside
user code.

To add a new stdlib package: add a file in `src/stdlib/`, return items from
`module_items()`, register in `src/stdlib/mod.rs` `resolve_stdlib()`.

## Go toolchain

gors downloads its own Go toolchain to `~/.local/share/gors/toolchains/` (or
platform equivalent via `dirs` crate). Pinned version in
`src/toolchain/mod.rs::DEFAULT_GO_VERSION`. Called via `toolchain::ensure()` at
the start of `build` and `run` commands.

## Testing

### Fast tests (default)

```bash
cargo test              # Runs unit tests + program execution tests
```

### Lexer/parser conformance tests

```bash
cargo test --features integration  # Also runs lexer/parser conformance against Go reference
```

Conformance tests in `tests/lexer_parser.rs` are gated behind the `integration` Cargo
feature flag. Without `--features integration` they are `#[ignore]`d. CI runs them
via `make test-integrations`.

### Adding a test program

1. Create a directory in `tests/fixtures/go_programs/` (e.g., `my_feature/`)
2. Add `main.go` (and optionally `go.mod` for multi-package programs)
3. The test framework auto-discovers it and compares output with `go run`

### Environment variables for test tuning

- `GORS_TEST_LIMIT=N` — cap number of files tested
- `GORS_TEST_FILTER=substring` — only test matching files
- `GORS_TEST_VERBOSE=1` — show progress

## Run patterns

`gors run` supports the same invocation styles as `go run`:

| Pattern | Example | Description |
|---------|---------|-------------|
| Single file | `gors run main.go` | Compile and run a single Go file |
| Multiple files | `gors run main.go utils.go` | Explicit file list, all must be same package |
| Directory | `gors run .` | All `.go` files in the directory (go.mod aware) |
| Package path | `gors run ./cmd/server` | A specific sub-package within the module |

Arguments after the source paths are forwarded to the compiled program:
`gors run main.go -- --flag value`.

When the first argument ends with `.go`, all leading `.go` arguments are treated as
source files. Otherwise, the first argument is a directory/package path.

Key differences from `go run`:
- Uses `GORSPATH` (`~/.local/share/gors/toolchains/`) instead of `GOPATH`
- The Go toolchain is hermetically downloaded (pinned version in `src/toolchain/mod.rs`)
- Transpiles Go → Rust and compiles with `rustc`, not `go build`

## Compiler passes (in order)

1. `map_type` — Go types → Rust types (int→isize, string→String, etc.)
2. `type_conversion` — type calls to casts (`int(x)` → `x as isize`)
3. `hoist_use` — extract multi-segment paths to `use` declarations
4. `simplify_return` — remove trailing `return` (Rust style)
5. `flatten_block` — flatten single-expression nested blocks

Imported packages skip `hoist_use`.

## Known limitations

- `fmt.Println` and `fmt.Print` support up to 4 arguments (via Println2/Println3/Println4)
- No closures or variadic function definitions
- No string concatenation with `+` (needs type inference)
- No `for range` over strings (uses `.iter()` instead of `.chars()`)
- `reflect` package is infeasible to transpile — stdlib packages using it must be hand-written
- Source maps are single-file only (not yet supported for multi-file output)

## Conventions

- Lints are workspace-level in `Cargo.toml` — `panic`, `unwrap_used`, `expect_used` are denied
- Test modules use `#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- No comments unless the WHY is non-obvious
- Prefer editing existing files over creating new ones
- `func Add(a, b int)` shorthand not supported by parser — use `func Add(a int, b int)`
