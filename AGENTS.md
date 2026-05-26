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
    printer/       # syn AST → formatted Rust source via prettyplease
    resolve/       # Import-path resolution for embedded Go SDK packages
    toolchain/     # Hermetic Go toolchain download and management
    mapping/       # Source map tracking (Go ↔ Rust position mapping)
    token/         # Go token types
    error.rs       # Diagnostic formatting
    lib.rs         # Library entrypoint
  tests/
    test_integration_run.rs      # Program execution integration tests
    test_integration_lexer.rs    # Lexer conformance vs Go oracle
    test_integration_parser.rs   # Parser conformance vs Go oracle
    common.rs                    # Shared integration test infrastructure
    fixtures/
      go_programs/     # Runnable Go programs (auto-discovered, each dir = one test)
      go_files/        # Standalone Go source files for lexer/parser conformance
      go_repositories/ # Go repository submodules for lexer/parser conformance
    tools/
      go_oracle/ # Small Go helper that emits reference scanner/parser output
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
- Stdlib modules are resolved lazily from build-time generated Go SDK metadata
  and filtered to reachable root symbols before being compiled to Rust. Do not add
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
- Go function values stored in generated data structures and explicit local
  variables of `func(...)` type are reference-counted nil-capable cells:
  `std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnMut(...) -> ... + Send>>>>`.
  Calls go through `crate::builtin::lock_func` and unwrap the option at the call
  site. Do not reintroduce `Rc<RefCell<dyn FnMut>>`; keep the representation
  thread-safe so goroutine lowering can share the same value model.
- Ordinary Go function literals lower to borrowing Rust closures so local
  captures can be mutated across calls. Only function literals being stored
  behind generated function types should use `move`, because those are boxed
  behind the shared `Arc<Mutex<dyn FnMut(...) -> ... + Send>>` representation.
- Goroutine function literals use IR capture analysis. Mutable outer captures are
  promoted to `Arc<Mutex<T>>` in the enclosing block and cloned into the spawned
  closure so synchronized goroutine writes are visible after channel joins.
- Go expression switches lower through an explicit selected-case slot plus a
  fallthrough flag. This preserves source-order case expression evaluation,
  lets `default` appear anywhere while still running only when no case matches,
  executes only explicit `fallthrough` chains, and maps unlabeled case-level
  `break` to the generated Rust switch block label.
- `for` loops with post statements wrap the body in a generated labeled block
  whenever a matching `continue` is present. This covers both unlabeled
  continues and `continue label` targeting the current loop so Go's post clause
  still runs before the next iteration.
- Select statements wrap generated bodies in a labeled block and rewrite
  unlabeled select-case `break` statements to that label. Channel select
  readiness uses `Chan::try_recv` and `Chan::try_send`, so builtin DCE roots must
  preserve those methods whenever select lowering or channel helpers reference
  them.
- Non-void functions and function literals with no explicit final Rust `return`
  get a tail `panic!("gors: missing return")` fallback. Go rejects reachable
  missing-return paths, but valid Go control-flow constructs such as exhaustive
  switch returns still need a Rust tail expression after lowering.
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

Go stdlib imports are resolved as ordinary Go packages through the resolver in
`gors/src/resolve/mod.rs`, backed by build-time generated metadata from the
embedded Go SDK. The old handwritten stdlib modules have been removed.
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
Compiler-side stdlib/DCE reachability is also memoized by the Rust item token
stream, requested roots, and known module names; keep that key aligned with any
future reachability input that can change the kept item set.

## Go toolchain

The pinned Go SDK version lives in the repository root `.go-version` file. Do
not hardcode the Go version elsewhere unless the target format cannot reference
that file; when that happens, keep the duplicated value aligned.

`gors/build.rs` reads `.go-version`, downloads the matching Go SDK tarball,
verifies its `.sha256`, extracts it once under `$CARGO_HOME/gors-cache/`, and
uses that extracted SDK as the source for generated `go_stdlib.rs` metadata and
copied `go_stdlib_src/` files under Cargo `OUT_DIR`. The build exports
`gors::GO_VERSION` and `gors::STDLIB_VERSION` (`gostdlibx.y.z`) so generated
output manifests and `gors version` change when the embedded stdlib changes.
It must also rerun when `../gors-builtin/src/lib.rs` changes because compiler
tests and generated programs embed `builtin.rs` from that source.

Integration tests must not call a system `go`. `tests/common.rs::go_command()`
uses the extracted SDK `bin/go` from the `gors` build, with `GOTOOLCHAIN=local`,
for both `go_oracle` and `go run` comparisons. CI should not install Go via
`actions/setup-go`, as the pinned tarball is the source of truth.
GitHub Actions caches `$CARGO_HOME/gors-cache` as `~/.cargo/gors-cache`, keyed
by runner OS and root `.go-version`, so SDK download/extraction changes must keep
that cache path and key source aligned.

## Testing

### Unit tests

```bash
make rust-test-unit
```

`make rust-test-unit` runs the normal workspace test suite without integration
features. Compiler/printer/generator regression tests live inside the `gors`
crate as unit tests attached to the modules they cover, such as
`gors/src/printer/mod.rs` and `gors/src/compiler/manifest.rs`. Unit tests assert
in-process contracts only; they must not invoke `go`, `gors`, or `rustc`.
Shared integration test harness code lives in root `tests/common.rs`.
Integration test entrypoints live in root `tests/` and are wired into the
`gors` crate through explicit `[[test]]` entries in `gors/Cargo.toml`;
integration fixtures remain under `tests/fixtures/`.

`make all` is the local CI-parity gate. It depends on `make rust-build`,
`make rust-lint`, `make rust-test`, `make web-build`, `make web-lint`, and
`make web-test`, so a successful local run covers the same build/test/check
commands as CI. GitHub-only artifact upload and Pages deploy steps are
intentionally not represented locally.

CI runs on `pull_request` for PR branches and on `push` only for `master`.
Do not re-enable feature-branch push CI unless the duplicate PR/push checks are
actually needed.

`make rust-test` is the local full-suite test convenience command. It depends on
the split unit and integration targets below and should not redefine its own
combined Cargo command. CI should call the split `make rust-test-*` targets
below for clearer job boundaries and failure output.

### Integration tests

```bash
make rust-test-integration-lexer
make rust-test-integration-parser
make rust-test-integration-run
```

Integration tests use matching Make targets and Cargo feature gates:
`rust-test-integration-lexer` → `test_integration_lexer`,
`rust-test-integration-parser` → `test_integration_parser`, and
`rust-test-integration-run` → `test_integration_run`. Their integration-test
binary names match the feature gates and are declared in `gors/Cargo.toml`, so
the Make targets do not need extra test-name filters.

CI runs integration tests as single unsharded jobs with a 30-minute job timeout.
Do not split them into shard targets unless the test
contract changes again.

The integration binaries in root `tests/` are feature-gated as whole files:
lexer/parser integration targets scan the reference repositories from git
submodules, while `rust-test-integration-run` compares in-process generated Rust
program output with the pinned Go SDK's `go run`. Lexer/parser integration may
execute the batched Go fixture runner for reference output, but that runner must
be built with `tests/common.rs::go_command()` rather than system `go`; the gors
side should use library APIs in-process rather than spawning the `gors` CLI. CLI
argument and output-file writer contracts belong in `gors-cli` unit tests.
Compiler/printer/generator coverage belongs in module-local unit tests under
`gors/src/` unless it must execute generated Rust or compare against Go.
Lexer/parser corpus tests must compare files in bounded batches and discard Go
oracle output batch-by-batch; precollecting oracle output for every repository
file can exhaust hosted CI memory before progress is reported.

### Adding a test program

1. Create a directory in `tests/fixtures/go_programs/` (e.g., `my_feature/`)
2. Add `main.go` (and optionally `go.mod` for multi-package programs)
3. The test framework auto-discovers it and compares output with `go run`

For broad stdlib API coverage, prefer grouping related checks into one package
fixture such as `tests/fixtures/go_programs/stdlib/strings/main.go` rather than
creating one runnable fixture per function; `rust-test-integration-run` pays a
full transpile plus `rustc` execution cost per discovered program directory.
After adding or changing `gostdlib_` fixtures, run
`npm --prefix www run generate:gostdlib-report` from the repository root to
refresh the Svelte app's stdlib coverage report. The generator marks fixture-used
selectors as tested and derives untested package/symbol rows from the embedded
Go stdlib source copied to `go_stdlib_src` by the `gors` build.
The run harness caches generated-program binaries under
`target/gors-integration-run/` using a key derived from the generated Rust
source, `gors::STDLIB_VERSION`, `rustc -vV`, and the rustc flag set; keep
compiler-sensitive inputs in that key if the harness starts skipping more work.

### Environment variables for test tuning

- `GORS_TEST_LIMIT=N` — cap number of files tested
- `GORS_TEST_FILTER=substring` — only test matching files
- `GORS_TEST_VERBOSE=1` — show progress
- `GORS_TEST_FAIL_FAST=1` — cancel queued/running integration work after the first failure where supported

## Run patterns

From the workspace root, `cargo run -- ...` defaults to the `gors` CLI binary.
The root manifest uses `workspace.default-members` to keep the fuzz helper
binaries out of implicit default selection; use explicit `--workspace` or
`--package=fuzz` commands when checks need to include fuzz targets.

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
- Uses `GORSPATH` instead of `GOPATH`
- The embedded Go stdlib comes from the hermetically downloaded SDK pinned in `.go-version`
- Transpiles Go → Rust and compiles with `rustc`, not `go build`

## Web UI (`www/`)

The browser demo must not call the wasm compiler directly on the main thread.
`www/go2rust-compiler.ts` owns the async API and delegates transpilation to
`www/go2rust-worker.ts`; keep source-map data structured-cloneable and hydrate
UI lookup helpers on the main thread. The worker loads wasm through
`www/gors-wasm-loader.ts`, which instantiates `gors_bg.wasm` as an explicit
asset before wiring it into wasm-bindgen's generated JS glue. Do not switch the
worker back to the `wasm/pkg/gors.js` bundler entry without rechecking Chromium:
webpack's top-level async wasm module path can stall before the worker message
handler is installed.

`www/` is currently a webpack-hosted Svelte SPA, not SvelteKit. The wasm/v86
asset pipeline is wired through webpack, and app routes such as `/coverage` are
served by history fallback plus emitted static fallback HTML
(`coverage/index.html` and `404.html`). Treat a SvelteKit migration as a larger
asset-pipeline migration rather than a routing-only change.

The first-party browser/runtime code in `www/` is TypeScript. `make web-lint`
includes both ESLint and TypeScript/Svelte type checking, while
`make web-test-unit` runs Vitest and `make web-test-integration` runs the
Playwright browser test against the real default app pipeline, including VM
startup, Rust compilation, and program execution. `make web-test-integration`
installs Chromium by default; CI passes
`PLAYWRIGHT_INSTALL_ARGS="--with-deps chromium"` so browser system
dependencies are installed after `web-install`.

CI deploys `www/dist` with native GitHub Pages artifacts
(`actions/upload-pages-artifact` plus `actions/deploy-pages`) rather than by
force-pushing a generated `gh-pages` branch. The v86 root filesystem makes the
published site hundreds of MB, so branch-based deploys can fail during `git
push` with HTTP 408/timeouts. The repository Pages source must be set to
GitHub Actions (`build_type: workflow`) for this deploy path.

## Type inference

`gors/src/compiler/typeinfer.rs` provides a `TypeEnv` that pre-scans Go AST files
before compilation to collect variable types, function signatures, struct fields,
and interface declarations. The `GoType` enum represents Go types. Used during
code generation for type-aware decisions (string indexing, numeric casts,
interface detection).

`gors/src/compiler/ir.rs` is the typed Go IR layer being introduced between the
parser AST and Rust `syn` backend. Current compile entrypoints build this IR as
a semantic prepass before the legacy direct AST-to-syn lowering. Keep new
language-semantic work moving into the IR first, especially addressability,
capture modes, control-flow shape, and type-directed expression lowering; the
Rust backend should consume those semantics instead of rediscovering them with
ad hoc AST checks.
IR control-flow completion (`ast_block_completion`, `block_completion`,
`stmt_completion`) classifies whether lowered blocks can complete normally.
Use it for backend decisions that need Go reachability or return-shape
semantics instead of duplicating statement-shape checks in codegen.
It follows Go's terminating-statement rules rather than generic Rust
reachability: statement lists are classified by their final non-empty statement,
labeled statements inherit the labeled statement's completion, built-in `panic`
calls terminate, empty `select {}` and no-condition non-range `for` loops can
terminate control flow, and `for`/`switch`/`select` termination must reject only
`break` statements that refer to that specific construct. Keep nested breakable
statements label-aware so an unlabeled `break` inside a nested switch/select/loop
does not make the outer construct complete.

Thread-local `TYPE_ENV` is populated in `compile()` and consulted via
`get_var_go_type()`, `is_type_interface()`, `get_func_returns()`.
Package-level string constants are also tracked in `TypeEnv` so generated
owned-`String` constant functions are scoped per package; do not use a global
cross-package string-constant set for identifier lowering.

Variadic `...any` calls are lowered to normal `Vec::from([..])` expressions,
not `vec![..]` macros, so dependency discovery and later AST passes can see
module references inside variadic arguments.

Fixed Rust types derived from `GoType` are built as `syn` AST paths directly
rather than reparsed with `parse_quote!`; this keeps the wasm stdlib compile
path from crashing inside Syn's type parser.

Go slice parameters map to `Vec<T>` values unless the compiled body mutates the
slice's backing storage. The post-compile multi-module pass rewrites parameters
written through by index, or passed to another mutable slice parameter, to
`&mut Vec<T>` and rewrites call sites to borrow the caller's buffer. Do not apply
that rewrite to functions returning a slice; those need Go's returned slice
value semantics.
Slice expressions currently materialize owned `Vec` copies. Full slice
expressions (`a[low:high:max]`) preserve observable `len`/`cap` by reserving
capacity for `max-low`, but they still do not share the original Go backing
array; fixing shared backing-array semantics belongs in the IR/value model, not
in another ad hoc slice codegen special case.

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
downloads the SDK pinned in `.go-version`, extracts it under
`$CARGO_HOME/gors-cache/`, filters `go/src/**/*.go` (excluding tests, vendor,
cmd), copies selected files into `OUT_DIR/go_stdlib_src/`, and generates a
static `OUT_DIR/go_stdlib.rs` package table with per-package file lists and
direct stdlib imports. All stdlib/internal packages in that table are available
through the generic resolver. GOOS filtering follows the Rust target OS, but
GOARCH filtering uses a synthetic non-native `gors` architecture so
assembly-backed native stdlib files fall back to pure Go generic implementations
before parsing.

The resolver caches package file selection, type environments, transitive
imports, and root-specific resolved modules through shared `RwLock`/per-key
initialization state so parallel integration tests can reuse stdlib work.
Per-file stdlib parser/compiler skips are quiet by default; set
`GORS_STDLIB_TRACE=1` to see resolver decisions and skipped files.

Stdlib output is pruned at item level from roots such as `crate::fmt::Println`.
Direct imports with no surviving references should be pruned rather than
preserved solely because the Go import existed, but pruning must not be used as
a substitute for compiling reachable stdlib code generically.

Generated Rust files start with a `//! Generated by gors. Do not edit.`
rustdoc header, immediately followed by the printer-level lint prelude that
denies `dead_code`, `unused_imports`, `unused_macros`, and `unsafe_code`, while
still allowing Go naming via `nonstandard_style` and suppressing mechanical
generated-code warnings such as unused temporaries, redundant parentheses, and
unreachable branches; one blank line separates the prelude from generated code.
Dependency modules are emitted alphabetically by Rust module name, and generated
items/methods are ordered with public functions before private functions.
Preserve Go AST grouping when emitting nested binary expressions: Go and Rust
operator precedence differ for shifts and bitwise operators, so child binary
expressions need parentheses whenever Rust would otherwise regroup them.

## Known limitations

- Closure support is partial; recursive self-capturing function literals still need reentrant function-cell calls.
- `reflect` is not fully supported; currently only the pieces needed by pruned stdlib paths compile reliably
- Source maps are single-file only (not yet supported for multi-file output)

## Conventions

- Lints are workspace-level in `Cargo.toml` — `panic`, `unwrap_used`, `expect_used` are denied
- Test modules use `#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- No comments unless the WHY is non-obvious
- Prefer editing existing files over creating new ones
