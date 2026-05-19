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
cargo test              # ~0.2s, runs unit tests + fast integration tests
```

### Slow tests (explicit)

```bash
cargo test -- --ignored  # Full program execution + lexer/parser conformance
```

Slow tests are marked `#[ignore]` in `tests/programs.rs` and `tests/lexer_parser.rs`.

### Adding a test program

1. Create a directory in `tests/fixtures/go_programs/` (e.g., `my_feature/`)
2. Add `main.go` (and optionally `go.mod` for multi-package programs)
3. The test framework auto-discovers it and compares output with `go run`

### Environment variables for test tuning

- `GORS_TEST_LIMIT=N` — cap number of files tested
- `GORS_TEST_FILTER=substring` — only test matching files
- `GORS_TEST_VERBOSE=1` — show progress

## Compiler passes (in order)

1. `map_type` — Go types → Rust types (int→isize, string→String, etc.)
2. `type_conversion` — type calls to casts (`int(x)` → `x as isize`)
3. `hoist_use` — extract multi-segment paths to `use` declarations
4. `simplify_return` — remove trailing `return` (Rust style)
5. `flatten_block` — flatten single-expression nested blocks

Imported packages skip `hoist_use`.

## Known limitations

- Only `fmt.Println` and `fmt.Print` with single arguments are supported
- No struct types, methods, interfaces, slices, closures, or variadic functions
- No type declarations or multiple return values
- `reflect` package is infeasible to transpile — stdlib packages using it must be hand-written
- Source maps are single-file only (not yet supported for multi-file output)

## Conventions

- Lints are workspace-level in `Cargo.toml` — `panic`, `unwrap_used`, `expect_used` are denied
- Test modules use `#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- No comments unless the WHY is non-obvious
- Prefer editing existing files over creating new ones
- `func Add(a, b int)` shorthand not supported by parser — use `func Add(a int, b int)`
