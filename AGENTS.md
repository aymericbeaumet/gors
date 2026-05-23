# AGENTS.md — Guidelines for AI Agents

> **Keep this file current.** When you make architectural decisions, discover
> non-obvious constraints, or learn something that would save a future agent
> time, update the relevant section below.

## Project overview

gors is a Go-to-Rust transpiler written in Rust. It parses Go source code into
an AST, compiles it to a Rust `syn` AST, applies transformation passes, and
generates formatted Rust source code.

Pipeline: Go source → scanner → parser → Go AST → compiler → Rust AST → passes → printer → Rust source

## Repository layout

```
gors/
  src/
    scanner/       # Go lexer (token stream)
    parser/        # Go parser (Go AST), import resolution, go.mod support
    ast/           # Go AST data structures
    compiler/      # Go AST → Rust syn AST conversion + transformation passes
      passes/      # Post-compilation Rust→Rust AST transforms
      manifest.rs  # Build manifest for incremental compilation
    printer/      # syn AST → formatted Rust source via prettyplease
    toolchain/     # Hermetic Go toolchain download and management
    mapping/       # Source map tracking (Go ↔ Rust position mapping)
    token/         # Go token types
    error.rs       # Diagnostic formatting
    lib.rs         # Library entrypoint
  tests/
    run.rs         # Program execution tests (compile Go → run Rust, compare output)
    lexer.rs       # Lexer conformance vs Go reference
    parser.rs      # Parser conformance vs Go reference
    common.rs      # Shared test infrastructure
    fixtures/
      go_programs/ # Test programs (auto-discovered, each dir = one test)
      go_sources/  # Go source files for lexer/parser conformance
gors-cli/
  src/main.rs      # CLI: ast, build, run, tokens subcommands
gors-builtin/
  src/lib.rs       # Go builtin helpers embedded as generated builtin.rs
```

## Compilation model

### Multi-file output (current)

`compile_program_multi()` produces a `CompiledProgram` with individual modules:
- Each Go package → individual `.rs` file
- Naming: `import_path.replace('/', "__")` + `.rs` (e.g., `example/math` → `example__math.rs`)
- `lib.rs` declares all modules with `#[path]` attributes
- `main.rs` includes `lib.rs` and contains main function items
- Stdlib modules are resolved lazily from the embedded Go SDK archive and
  filtered to reachable root symbols before being compiled to Rust. Do not add
  package-specific or function-specific Rust replacements for Go stdlib APIs;
  treat stdlib packages as ordinary Go code and fix the generic transpilation
  path when they fail. Runtime support is allowed only for language/runtime
  primitives or host resources, and must not encode the behavior of a stdlib
  function or method.

### Cross-module references

- `prefix_sibling_paths` rewrites references to sibling packages as `crate::pkg::Symbol`
- `hoist_use` lifts multi-segment paths to `use` statements (only for main package)
- `hoist_use` detects name collisions and keeps paths qualified when ambiguous
- Local package names that collide with any known stdlib module use an
  import-path-derived Rust module name (`example/math` → `example__math`) and
  import rewrites preserve the original Go selector name in source lowering.
- Package-level vars in imported/transpiled packages are emitted as concrete
  `std::sync::LazyLock<T>` statics. Main-package vars are still injected into
  `main()` as startup locals.
- Named `[]byte` types are newtypes, but the compiler also emits helper impls
  (`Len`, `Cap`, `StringValue`, `AsRef<[u8]>`, `AsMut<[u8]>`, and `Append`
  variants) so stdlib code can use them like Go byte slices.

### Incremental builds

- `.gors_manifest.json` tracks content hashes per module
- `compute_content_hash()` concatenates sorted Go source files → SHA-256
- Unchanged modules are skipped during `build`
- Files tracked by the previous manifest but absent from the new generated
  output are removed, so DCE/module-pruning changes do not leave stale `.rs`
  files in the output directory.

## Stdlib system

Go stdlib imports are resolved from the embedded Go SDK archive through
`gors/src/go_stdlib.rs`; the old handwritten stdlib modules have been removed.
Import-path-to-module naming is generic (`unicode/utf8` → `unicode__utf8`, Rust
keywords get a trailing `_`).

`gors-builtin/src/lib.rs` implements Go predeclared builtin support and is copied
into every generated Rust program as `builtin.rs`. It must not contain
handwritten implementations of specific Go stdlib packages such as `fmt`,
`strings`, or `sort`.

Stdlib coverage tests are generic compiler tests. The Go stdlib is used because
it is broad, real Go code; any fix needed for `fmt`, `strings`, `sort`, or
another package should improve parsing, type inference, code generation,
reachability, or backend/runtime primitives for arbitrary Go packages. Do not
make a stdlib test pass by reimplementing that stdlib function, method, type, or
constant in Rust, or by adding package-name-specific lowering rules.

The `ParsedProgram.stdlib_imports` field tracks which stdlib packages a program
uses directly. `compile_program_multi()` scans those packages for type
information, compiles user/local code first, then resolves embedded stdlib
packages on demand from the actual cross-module symbols that remain after
reachability pruning.

Stdlib resolution is root-specific and cached by import path plus reachable
symbol set. The resolver parses selected Go files only when the package is
needed, filters unused top-level AST declarations before compiling, and caches
type environments, transitive imports, and resolved token streams. Direct
imports with no surviving references should not force module generation.

## Go toolchain

gors downloads its own Go toolchain to `~/.local/share/gors/toolchains/` (or
platform equivalent via `dirs` crate). Pinned version in
`gors/src/toolchain/mod.rs::DEFAULT_GO_VERSION`. Called via `toolchain::ensure()` at
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

Conformance tests in `gors/tests/lexer.rs` and `gors/tests/parser.rs` are gated
behind the `integration` Cargo feature flag. Without `--features integration`
they are `#[ignore]`d. CI runs them via `make test-integrations`.

### Adding a test program

1. Create a directory in `gors/tests/fixtures/go_programs/` (e.g., `my_feature/`)
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
- The Go toolchain is hermetically downloaded (pinned version in `gors/src/toolchain/mod.rs`)
- Transpiles Go → Rust and compiles with `rustc`, not `go build`

## Type inference

`gors/src/compiler/typeinfer.rs` provides a `TypeEnv` that pre-scans Go AST files
before compilation to collect variable types, function signatures, struct fields,
and interface declarations. The `GoType` enum represents Go types. Used during
code generation for type-aware decisions (string indexing, numeric casts,
interface detection).

Thread-local `TYPE_ENV` is populated in `compile()` and consulted via
`get_var_go_type()`, `is_type_interface()`, `get_func_returns()`.
Package-level string constants are also tracked in `TypeEnv` so generated
owned-`String` constant functions are scoped per package; do not use a global
cross-package string-constant set for identifier lowering.

Variadic `...any` calls are lowered to normal `Vec::from([..])` expressions,
not `vec![..]` macros, so dependency discovery and later AST passes can see
module references inside variadic arguments.

## Compiler passes (in order)

Main package (`pass()`):
1. `map_type` — Go types → Rust types (int→isize, string→String, etc.)
2. `type_conversion` — type calls to casts (`int(x)` → `x as isize`)
3. `inject_channel` — channel send/receive
4. `inline_errors` — error value handling
5. `nil_check` — nil comparisons → Default::default() / is_empty()
6. `string_lit` — string literal `.to_string()` in assignments/returns/method args
7. `trait_param` — generic trait parameter handling
8. `hoist_use` — extract multi-segment paths to `use` declarations
9. `simplify_return` — remove trailing `return` (Rust style)
10. `flatten_block` — flatten single-expression nested blocks
11. `index_cast` — array/slice index expressions cast to usize
12. `interface_param` — (placeholder) interface type parameter handling
13. `coerce_types` — len()/cap() → isize cast, float-to-int typed locals

Imported packages (`pass_for_imported_package()`): only map_type, type_conversion,
simplify_return, flatten_block.

## Stdlib system — embedded Go source

Go stdlib is embedded in the `gors` crate binary data via `gors/build.rs`, which
downloads Go 1.24.3 SDK and packs `go/src/**/*.go` (excluding tests, vendor,
cmd) into `go_stdlib.tar.gz`.
All stdlib/internal packages in the archive are available through the generic
resolver; build tags and GOOS/GOARCH filename suffixes are filtered for the host
target before parsing.

The resolver caches parsed package selection, type environments, transitive
imports, and root-specific resolved module token streams. Per-file stdlib
parser/compiler skips are quiet by default; set `GORS_STDLIB_TRACE=1` to see
resolver decisions and skipped files.

Stdlib output is pruned at item level from roots such as `crate::fmt::Println`.
Direct imports with no surviving references should be pruned rather than
preserved solely because the Go import existed, but pruning must not be used as
a substitute for compiling reachable stdlib code generically.

Generated Rust files start with a `//! Generated by gors. Do not edit.`
rustdoc header, immediately followed by the printer-level lint prelude that
denies `dead_code`, `unused_imports`, `unused_macros`, and `unsafe_code`, while
still allowing Go naming via `nonstandard_style`; one blank line separates the
prelude from generated code. Dependency modules are emitted alphabetically by
Rust module name, and generated items/methods are ordered with public functions
before private functions.

## Known limitations

- No closures or variadic function definitions
- No string concatenation with `+` (needs type inference)
- No `for range` over strings (uses `.iter()` instead of `.chars()`)
- `any` type maps to `Box<dyn Any>` but auto-boxing at assignment sites requires manual wrapping
- `reflect` is not fully supported; currently only the pieces needed by pruned stdlib paths compile reliably
- Source maps are single-file only (not yet supported for multi-file output)
- `complex128`/`complex64` types conflict with builtin function names in map_type pass
- Interface types as function parameters need `impl Trait` or `&dyn Trait` wrapping
- Trait downcasting (`x.(InterfaceName)`) only works for concrete types, not trait objects

## Conventions

- Lints are workspace-level in `Cargo.toml` — `panic`, `unwrap_used`, `expect_used` are denied
- Test modules use `#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- No comments unless the WHY is non-obvious
- Prefer editing existing files over creating new ones
- `func Add(a, b int)` shorthand not supported by parser — use `func Add(a int, b int)`
