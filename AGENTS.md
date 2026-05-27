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
  `std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn Fn(...) -> ... + Send + Sync>>>>`.
  Calls clone the inner `Arc` while holding `crate::builtin::lock_func`, then
  release the mutex before invoking the function. This is required for recursive
  function values. Do not reintroduce `Rc<RefCell<...>>`; keep the representation
  thread-safe so goroutine lowering can share the same value model.
- Ordinary Go function literals lower to borrowing Rust closures so local
  captures can be mutated across calls. Only function literals being stored
  behind generated function types should use `move`, because those are stored
  behind the shared `Arc<Mutex<Option<Arc<dyn Fn(...) -> ... + Send + Sync>>>>`
  representation.
- Expected-type expression lowering owns Go function-value coercions. Function
  literals and named or selector function items passed to `func(...)`-typed
  arguments or assignments are wrapped as shared function cells by casting the
  inner `Box` to `Box<dyn FnMut(...) -> ... + Send>`; do not cast the outer
  `Arc`, because Rust rejects non-primitive casts between `Arc` instantiations.
- Function literals use IR capture analysis for shared mutable captures. Mutable
  outer captures discovered anywhere in a block, including callback arguments,
  returned closures, goroutines, and function literals nested inside composite
  literals, are promoted to `Arc<Mutex<T>>` in the enclosing block. Any `move`
  closure that captures those cells must clone the `Arc` before constructing the
  closure so later outer-scope reads still see the same storage.
- Assignments and compound assignments to shared captures must evaluate the RHS
  into a temporary before locking the LHS cell, so expressions like
  `x = x + 1` and `x += x` do not try to acquire the same `Mutex` twice.
- Addressable non-Copy binding initializers are cloned for `var` and `:=`
  declarations. This preserves Go value-copy semantics for struct/string/array
  bindings and avoids Rust moves such as `d := c` invalidating later uses of
  `c`. Function values and pointers stay cheap-copy through their existing
  representations.
- Go pointer values lower to `Arc<Mutex<T>>` cells. Locals whose address is
  taken are promoted through the IR addressability analysis into the same cell
  representation, so `p := &x`, `*p = v`, and later reads of `x` observe the
  shared storage. Borrowed pointer parameters may still lower to `&mut T` when
  the existing escape analysis proves the pointer does not escape.
- Map literals, comma-ok map indexes, map assignments, and `delete` calls must
  compile keys and values with the expected map key/value Go types. This keeps
  `map[string]T{"k": v}`, `m["k"]`, and `delete(m, "k")` on owned `String`
  keys instead of accidentally inferring `&str` keys from Rust literals.
- String `+=` lowers to `String::push_str(&rhs)` rather than Rust `+=`, because
  Go accepts string operands by value while Rust's `String` add-assign expects a
  borrowed string slice.
- Main-package package-level vars are injected as startup locals in `main()`.
  Preserve explicit Go types there: typed initializers must be compiled with the
  expected type and emitted with a Rust type annotation, and typed zero values
  should use the same default-expression path as local var declarations.
- Runtime interface downcast hooks (`__gors_as_any`) are part of the generated
  interface contract. DCE must preserve the hook on reachable traits and trait
  impls, and any injected structural stdlib helper that implements a Go
  interface, such as `os.File` for `io.Writer`, must implement the hook too.
- Backward `goto Label` targeting the immediately labeled statement is still
  lowered by wrapping that statement in a generated Rust labeled `loop` and
  translating the `goto` to `continue 'Label`. Scope-safe forward gotos whose
  target is a direct label in the same block lower through an IR-planned
  generated state loop; IR identifies direct-block locals that cross state
  segments, and the backend hoists typed zero-value bindings before rewriting the
  original declarations to segment-local assignments. Broader forward gotos still
  require full CFG restructuring in the IR before backend lowering.
- Go expression switches without `fallthrough` lower to an exclusive Rust
  `if`/`else` chain inside a generated label so Rust can see moved case values
  are branch-local. Switches containing `fallthrough` still lower through an
  explicit selected-case slot plus a fallthrough flag. Both paths preserve
  source-order case expression evaluation, let `default` appear anywhere while
  still running only when no case matches, and map unlabeled case-level `break`
  to the generated Rust switch block label.
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
  get a tail `panic!("gors: missing return")` fallback unless lowering already
  ended the block with a Rust tail value expression. Go rejects reachable
  missing-return paths, but valid Go control-flow constructs and bodyless stdlib
  fallbacks can still need a Rust tail expression after lowering.
- Named result parameters are declared before a synthetic labeled function-exit
  block. Explicit and bare `return` statements inside that block assign the
  named results and break to the exit label, then the final Rust return reads
  the named results after RAII defer guards have been dropped. This preserves
  Go's ordering where deferred calls can mutate named results before the caller
  sees them.
- Deferred calls are pushed onto a function-scoped LIFO stack after evaluating
  the function value/receiver arguments that the current lowering can save.
  Dropping that stack at function exit preserves Go's nested-block defer timing
  and keeps named-result mutation before the final Rust return.
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
`rust-test-integration-run` executes generated-program fixtures in a Rayon pool
with 16 MiB worker stacks, matching the lexer/parser integration stack budget.
Large stdlib fixtures such as `gostdlib_net_http` can overflow the default test
thread stack while parsing and compiling real Go stdlib packages.
Each generated-program worker starts its Go reference `go run` child before the
generated Rust compile/run path so Go, gors, and rustc work overlap across the
whole Rayon pool. By default the run harness uses twice the detected CPU count
because workers often block on child processes and filesystem work; keep the
`GORS_TEST_RUN_THREADS` override as the exact concurrency control for local CPU
saturation experiments. Keep child-process capture on temp files plus polling
and kill-on-abort behavior so parallel fail-fast does not deadlock on
stdout/stderr pipes, and still wait for the Go reference before reporting
generated Rust failures so invalid Go fixtures skip instead of failing gors.

### Environment variables for test tuning

- `GORS_TEST_LIMIT=N` — cap number of files tested
- `GORS_TEST_FILTER=substring` — only test matching files
- `GORS_TEST_VERBOSE=1` — show progress
- `GORS_TEST_FAIL_FAST=1` — cancel queued/running integration work after the first failure where supported
- `GORS_TEST_THREADS=N` — worker threads for lexer/parser integration tests
  and an explicit generated-program run-test fallback
- `GORS_TEST_RUN_THREADS=N` — worker threads for generated-program run tests;
  defaults to `GORS_TEST_THREADS` when set, otherwise twice all available CPUs.
  Use this run-specific override for exact CPU-saturation experiments; higher
  values can increase reported CPU use while slowing the suite through
  allocation and cache contention.
- `GORS_TEST_GO_RUN_TIMEOUT_SECS=N` — override the generated-program harness
  timeout for Go reference runs (default: 30 seconds)
- `GORS_TEST_GENERATED_RUN_TIMEOUT_SECS=N` — override the generated-program
  harness timeout for compiled Rust program runs (default: 10 seconds)

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
startup, Rust compilation, and program execution. The default `/playground`
Hello World example is part of that integration contract: it must auto-compile
through the wasm worker before the VM run step. `make web-test-integration`
installs Chromium by default; CI passes
`PLAYWRIGHT_INSTALL_ARGS="--with-deps chromium"` so browser system
dependencies are installed after `web-install`. The Playwright web-server
startup timeout must account for cold v86 rootfs extraction plus webpack's first
bundle on hosted runners; do not shrink it back to a short dev-server default
without validating CI cold-start timing.
Playwright integration should start its own webpack server by default; only set
`PLAYWRIGHT_REUSE_EXISTING_SERVER=1` for deliberate manual reuse. This prevents
local dev servers from masking missing dependencies or disappearing while
`npm ci` rewrites `www/node_modules`. It also uses `GORS_WEB_TEST_PORT`
(default `18080`) instead of the human dev-server port `8080`, so local browser
sessions on `http://localhost:8080` do not collide with CI-parity tests.
The Playwright-owned webpack server disables live reload/watch; the VM run test
can take several minutes and must not reset the playground while it is waiting
for the Linux VM to finish.
`make web-lint` runs before `wasm-pack build`, so TypeScript checked by that
target must not depend on generated declarations under `www/wasm/pkg/`; define a
small local interface for the wasm-bindgen surface when lint needs those types.
The webpack dev server must accept both `127.0.0.1` and `localhost` hosts,
because Playwright uses the former while local browser testing commonly uses the
latter.
Webpack source maps are disabled by default, including during `npm run dev`, to
avoid browser DevTools exhausting source-map `Map` state on the large generated
bundle. Set `GORS_WEB_SOURCE_MAPS=1` only when intentionally debugging webpack
bundle source maps. The playground also caps client-side source-map indexing for
very large compiler outputs; when the cap is exceeded, Rust output remains
visible but hover/cursor mapping is disabled for that result.

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
Package-level function signatures and method signatures live in separate
`TypeEnv` namespaces. Methods must be registered only as receiver-qualified keys
such as `StringSlice.Search`, never as plain `Search`, because Go permits package
functions and methods to share the same simple name and call-site lowering needs
the package function signature for `func(...)` argument coercions.

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
Functions with result parameters are validated against that completion analysis
before lowering; do not restore backend-only missing-return panic insertion as
the sole enforcement mechanism.
It follows Go's terminating-statement rules rather than generic Rust
reachability: statement lists are classified by their final non-empty statement,
labeled statements inherit the labeled statement's completion, built-in `panic`
calls terminate, empty `select {}` and no-condition non-range `for` loops can
terminate control flow, and `for`/`switch`/`select` termination must reject only
`break` statements that refer to that specific construct. Keep nested breakable
statements label-aware so an unlabeled `break` inside a nested switch/select/loop
does not make the outer construct complete.
IR also owns capture and goto discovery. Keep extending those analyses in
`ir.rs` before adding backend-only statement walkers; codegen may still carry
temporary guards for legacy lowering, but the semantic decision should come from
the IR.
IR goto validation rejects undefined labels, jumps into nested blocks, and
forward gotos that would skip same-block local declarations before the Rust
state-machine lowering hoists locals for valid forward jumps; do not use
hoisting to make Go-invalid control flow compile. Goto validation recurses into
function literals with a fresh label scope, and checks switch/select clause
statement lists as implicit blocks for declaration-skipping jumps.
IR branch validation rejects `break`, `continue`, and `fallthrough` placements
that Go disallows before Rust lowering. Labeled `break`/`continue` must target
an enclosing breakable statement or loop respectively, and `fallthrough` is
accepted only as the final non-empty top-level statement of a non-final
expression-switch case. Branch validation recurses into function literals with a
fresh branch context, because labels, loops, and switches outside the literal do
not enclose its body.
IR statement-context validation rejects non-call/non-receive expression
statements, type conversions used as statements, and builtins that the Go spec
forbids in statement context (`append`, `cap`, `complex`, `imag`, `len`, `make`,
`new`, `real`, and the corresponding `unsafe` builtins). Keep it type-env aware
so shadowed predeclared names are not treated as builtins.
IR assignment validation applies type checks to ordinary assignments and to
redeclarations within `:=`: existing non-blank names on the left side of a short
variable declaration must receive values assignable to their original type.
IR nil assignability validation treats `nil` as valid only for known nilable
targets (pointer, function, slice, map, channel, and interface types) in
assignments, var initializers, return statements, channel sends, ordinary call
arguments, and builtin `append`/`delete` values. Bare inference from `nil` such
as `x := nil` or `var x = nil` is rejected before backend lowering.
IR expression validation rejects blank identifier uses as values or types while
still allowing `_` in assignment targets, short declarations, range assignment
targets, and blank declarations/import aliases.
Shared file validation rejects unused local variables in function bodies for
single-file and complete-program compilation while allowing unused parameters,
receivers, named results, package-level variables, blank bindings, and local
const/type declarations. A plain assignment to a bare identifier does not count
as use; reads, compound assignments, increments/decrements, and uses from nested
function literals do.
`compile_with_source_map()` must use the same single-file validation and import
package-name resolution as `compile()` before lowering; source-map generation
must not bypass Go spec checks.
IR label validation rejects duplicate labels and labels that are never targeted
by `goto`, labeled `break`, or labeled `continue`. Label scope is the enclosing
function body; do not count labels or label uses inside nested function
literals, but do validate each nested function literal's labels in its own
scope before lowering.
IR range-clause validation rejects too many iteration variables before backend
lowering: channels and integer ranges permit one effective binding, while
function ranges are capped by the yield callback arity. A blank second binding
is treated as absent per the Go spec. Known non-rangeable operands such as
bools, floats, complex values, pointers, and functions without the iterator
yield signature are rejected in IR before backend lowering; unknown or
unresolved named operands remain permissive until type inference can prove
their shape.
IR condition validation rejects known non-boolean `if` and conditional `for`
expressions after simple-statement bindings have been recorded; unknown or
unresolved named conditions stay permissive until type inference can prove them
invalid.
IR send-statement validation rejects known non-channel channel operands before
backend lowering and rejects sends to known receive-only channels; send-value
assignability rejects simple known scalar mismatches such as sending `string` to
`chan int`, but stays permissive for aggregate, pointer, unknown, named,
interface, nil-like values, and numeric constants because the current type
environment does not preserve full Go assignability or untyped-constant
information.
IR receive validation rejects known non-channel receive operands in statement,
assignment, and value-declaration contexts. Receive expression type inference
returns the channel element type for known channel operands so boolean channel
receives are valid in `if`/`for` conditions. Known send-only channel receives
and ranges are rejected in IR; broader nested receive validation is still
limited by legacy expression traversal.
IR range-clause validation treats `for ... = range ...` as assignment:
preexisting iteration variables must be assignable from the produced key/value
types. `for ... := range ...` introduces range-scoped variables with the
iteration value types instead.
IR select communication validation rejects non-communication `case` statements
before backend lowering. A select case may be default, a send statement, a
receive expression statement, or an `=`/`:=` receive assignment; short receive
declarations require identifier left-hand sides.
IR addressability follows the Go spec rule rather than treating every selector
or index expression as assignable: constants and unshadowed predeclared
identifiers are not addressable, map/string indexes are not addressable, array
indexes require an addressable array operand, and field selectors require an
addressable value or a pointer operand when the target type is known. Shadowed
predeclared names are addressable when the type environment has recorded their
binding. IR block lowering updates a cloned type environment for local `var`,
`const`, `:=`, and `for ... := range` bindings so later expressions in the same
lowering pass see local shadowing. Selector targets with unknown type
information remain permissive until IR local type flow is complete; this keeps
real stdlib code compiling instead of rejecting valid selector assignments
because the legacy type environment has not learned every local type yet.
IR assignment validation rejects non-assignable left operands before backend
lowering; blank identifiers and map-index operands are valid assignment targets,
but string indexes, literals, calls, constants, and unshadowed predeclared names
are not. Short variable declarations reject non-identifier left operands in
plain assignments and range clauses. Plain `=` assignments also reject simple
known scalar mismatches, including values forwarded from a single multi-result
function call, through the conservative assignability helper shared with
channel sends and returns.
IR value-declaration validation rejects simple known scalar mismatches for
explicitly typed `var` initializers, including values forwarded from a single
multi-result function call, using that same conservative assignability helper.
IR const-declaration validation uses the same conservative helper for explicitly
typed const initializers, so known scalar mismatches such as assigning a string
constant to an `int` const are rejected before backend lowering.
Const declarations also reject known runtime initializers such as user-function
calls or references to known variables; ambiguous imported selectors and
unsafe-style constants stay permissive until the type environment can prove
their value category.
IR return validation rejects simple known scalar mismatches for explicit result
expressions and single multi-result function calls, using the same conservative
assignability helper as channel send validation. It remains permissive for
aggregate, pointer, unknown, named, interface, nil-like values, and numeric
constants until the type environment preserves full assignability details.
IR statement validation checks unshadowed builtin `clear`, `close`, and `delete`
calls in expression, `go`, and `defer` statement contexts: `clear` requires one
map or slice argument, `close` requires one send-capable channel argument, and
`delete` requires a map plus an assignable key.
IR expression validation also walks top-level declarations and function bodies
for unshadowed builtin calls. `len` accepts string, array, slice, map, and
channel operands; `cap` accepts array, slice, and channel operands; `copy`
requires a destination slice plus a source slice with matching element type,
with the Go `[]byte`/`string` exception; `append` requires a destination slice
and assignable elements or a matching spread slice, with the Go `[]byte`/string
spread exception; `make` requires a slice, map, or channel type with the
spec-defined argument counts and integer-like size arguments; `new` rejects
spread calls, missing/extra arguments, and `nil`; `complex`, `real`, and `imag`
enforce the spec's complex-number operand shape; `min` and `max` require at
least one ordered numeric/string argument and reject spread calls; `panic`,
`recover`, `print`, and `println` enforce their fixed arity/spread rules.
Function literal bodies are included in IR expression validation with their
parameter/result bindings seeded so shadowed predeclared names stay shadowed.
The same IR expression pass validates ordinary function and method calls whose
signature is known to `TypeEnv`: fixed-arity calls must match parameter count,
single multi-result calls may forward results to matching parameters, variadic
calls validate fixed arguments plus element/spread assignability, and function
literals are checked from their AST signature. Unknown callees stay permissive
until type inference can prove their signature. Return statements must walk
returned expressions before only checking result count/type, so `return f(bad)`
gets the same call validation as assignments and expression statements. Type
conversion calls are a separate IR validation path: they require exactly one
single-valued argument and reject spread arguments before backend lowering.
Backend assignment lowering must use the checked assignment-lhs path, including
`++`/`--` and `for ... = range` targets, so known non-addressable operands fail
as compiler errors instead of falling back to arbitrary expression codegen. IR
validation also checks `++`/`--` directly: the operand must be addressable or a
map index, may not be `_`, and must have numeric type when known.
Index-expression validation is intentionally conservative around generics and
unknown named operands, but rejects known non-indexable operands, non-integer
array/slice/string indexes, and map keys that are not assignable to the map key
type.
Slice-expression validation follows the same boundary: known non-sliceable
operands fail, bounds must have integer type when known, and full slice
expressions on strings are rejected before lowering.
Compound assignments are validated in IR after left-side/addressability and
value-count checks: the right operand must be assignable to the left type, `+=`
allows numeric and string left operands, arithmetic compound ops require numeric
left operands, bitwise/remainder ops require integer left operands, and shifts
require integer left and right operands.
Binary expression validation is conservative for unknown/named operands, but it
checks known operands for logical bool operators, numeric/string `+`, numeric
arithmetic, integer bitwise/remainder, integer shifts, comparable equality, and
ordered numeric/string comparisons. Integer-only binary operators must still
accept integer-valued untyped numeric literals such as `1e9` when the other
operand has an integer type.
Unary expression validation checks known operands for numeric `+`/`-`, boolean
`!`, integer `^`, addressable `&`, pointer dereference `*`, and receive-capable
`<-`; unresolved named/unknown operands stay permissive.
Select lowering appends synthetic `break;` statements to multi-case arms, so
case-body statements embedded before that break must be emitted as non-tail Rust
statements; otherwise block expression bodies can make Syn report `expected ;`.
IR statement validation rejects `++`/`--` operands with known non-numeric types
before backend lowering; unresolved named/unknown operand types stay permissive
until type inference can prove them invalid. Map-index `++`/`--` is valid per
the Go spec and lowers through the map entry API rather than the normal
addressable-lvalue path.

The generated-code fallback pruner must preserve control-flow containers while
removing only unsupported reflection-dependent branches. When it prunes a local
initialized from unsupported reflection, it also drops later statements in that
block that depend on the pruned binding so generated Rust remains type-checkable.

Thread-local `TYPE_ENV` is populated in `compile()` and consulted via
`get_var_go_type()`, `is_type_interface()`, `get_func_returns()`.
Package-level string constants are also tracked in `TypeEnv` so generated
owned-`String` constant functions are scoped per package; do not use a global
cross-package string-constant set for identifier lowering.

Variadic `...any` calls are lowered to normal `Vec::from([..])` expressions,
not `vec![..]` macros, so dependency discovery and later AST passes can see
module references inside variadic arguments.

Deferred calls evaluate their argument expressions at the `defer` statement, not
inside the generated drop guard. The compiler saves deferred function values and
arguments in per-defer temporaries, cloning addressable non-Copy argument values
where needed so later statements can still use or mutate the original Go
variable.

Function-literal capture analysis lives in `gors/src/compiler/ir.rs` and uses a
lexical scope stack rather than whole-body declaration/reference set subtraction.
Keep nested shadowing cases there: a name declared in an inner block must not
mask a later reference to an outer captured name, and nested function literals
must propagate their free-variable uses to the enclosing literal.

Go function-typed values use the shared function-value representation
`Arc<Mutex<Option<Arc<dyn Fn...>>>>` consistently. If type inference learns that
a short declaration or `var` initializer is a `func` value, compile the
initializer with that expected Go type so calls use the same `lock_func` lowering
as named function-typed variables and returned function values.

Function signature validation is an IR-fronted compiler check in
`gors/src/compiler/ir.rs`. It rejects duplicate non-blank parameter/result names,
mixed named and unnamed parameter/result lists, variadic results, non-final or
multi-name variadic parameters, and receivers that are variadic or declare other
than one parameter before backend lowering.
Receiver-type validation uses the package type environment and rejects method
receiver bases that are undefined, unnamed, interfaces, or pointer types.
Method signature validation rejects method declarations with their own type
parameter list; receiver type parameters belong on the receiver type instead.
Generic type parameter declarations are validated in the same IR layer:
function and type declaration type-parameter lists must have explicit names and
constraints, non-blank type parameter names must be unique, receiver generic
argument lists must use identifiers, and receiver type-parameter names share the
method signature uniqueness set. Receiver type-parameter arity is checked
against `TypeEnv`'s recorded type declaration arity, including rejecting type
arguments on non-generic receiver bases. `TypeEnv` also tracks alias syntax and
instantiated-alias targets so receiver aliases are rejected when the alias is
generic or denotes an instantiated generic type, including through pointer
indirections.
Type declarations involving type parameters are also checked in IR: type
definitions cannot define directly from any in-scope type parameter, while a
generic alias cannot alias a type parameter declared by that same alias
declaration.
Single-file and multi-package compile entrypoints run the same IR validation
helper before Rust AST lowering.
The same IR validation layer rejects duplicate non-blank struct field names,
duplicate methods for a receiver base type, and method names that collide with
fields on the same struct base type before Rust emission.
Top-level declaration validation rejects duplicate package-block names across
const, var, type, and function declarations while ignoring `_` and receiver
methods. The package-block name `init` is special: multiple `func init()`
declarations are allowed and do not introduce a binding, but non-function
top-level `init` declarations and `init` functions with type parameters,
parameters, or results are rejected in IR. Package clause validation rejects
the blank package name `_` before backend lowering.
Executable multi-file/package compilation also rejects a `package main` program
with no top-level `func main` before Rust generation, while the lower-level
single-file compiler entrypoint remains permissive for partial snippet tests.
IR declaration validation also rejects duplicate non-blank names within a
single grouped or multi-name const, var, or type declaration, including local
declaration statements.
Import names are file-block bindings: IR rejects duplicate normal import names
across all import declarations in the file and import names that conflict with
package block declarations. Default import names come from the imported package
clause when the compiler has resolved package metadata, so versioned paths such
as `math/rand/v2` bind as `rand` rather than the path base. Blank and dot
imports are ignored by this conservative name check. Because gors merges
package ASTs before validation, import-name validation groups imports by their
original source file positions rather than treating the merged AST as one file
block. Single-file `compile()` and complete `compile_program_multi()` builds
also reject unused normal imports by looking for same-file qualified selectors;
single-file `compile()` resolves stdlib package names before that validation so
versioned stdlib paths use their package clause name. This check intentionally
stays out of `compile_with_type_env*`, which is used for root-pruned stdlib ASTs
where pruning can leave otherwise-unused imports.
For package `main`, the same signature validation rejects `func main` when it
declares type parameters, parameters, or results.
Short variable declarations are also checked there for duplicate non-blank names
on the left side and for introducing at least one new non-blank name in the
current lexical block. The no-new-name check is scope-based rather than
`TypeEnv`-based so nested short declarations can still shadow outer bindings.
Regular local const, var, and type declarations use the same lexical-block
model to reject redeclaring parameters, named results, or earlier local
declarations in the same block while still allowing nested-block shadowing and
valid short redeclarations with at least one new name.
Assignment arity is validated in the same IR statement pass before backend
lowering. It distinguishes single-valued expressions from real multi-valued
function calls, map indexes, channel receives, and type assertions so invalid
forms such as `x := pair()` or `x, ok := slice[0]` do not reach Rust codegen as
tuple destructuring or comma-ok lowering.
Return statement arity is also validated before backend lowering. Empty returns
are allowed only for functions with no results or named result parameters, a
single return expression may forward a matching multi-valued function call, and
explicit multi-expression returns must contain only single-valued expressions.
Type switch guards are validated against the spec grammar before lowering:
only `x.(type)` and `identifier := x.(type)` forms are accepted, with exactly
one non-blank guard identifier when the short declaration form is used.
Const and var declaration initializer arity is validated before backend
lowering. Const specs must match identifier/value counts, omitted const
expressions inherit the previous non-empty expression list in the same const
group, and var initializers reuse assignment-style single/multi-valued counts.
The same statement validation rejects short variable declarations in a `for`
post statement; Go only permits them in init/simple statement positions.
Switch, type-switch, and select statements reject multiple `default` clauses in
the same IR-fronted statement validation pass.
Blank labels (`_:`) are valid placeholder labels but do not define branch/goto
targets and are ignored by duplicate/unused label checks and goto-state planning.

Range-over-function support is IR-classified as a function range and backend
lowered by synthesizing the Go `yield` callback as the same shared function
value representation. Normal function items still call directly; only actual
function-typed values should use `lock_func` call lowering. Unlabeled
`break`/`continue` in the loop body return `false`/`true` from the synthesized
callback, and `return` fills a per-loop return slot, stops iteration, and
returns from the enclosing function after the range-function call. Variables
mutated by the synthesized callback are included in the block's shared-capture
set before declarations are lowered, and the callback clones those shared cells
before entering its `move` closure.

Fixed Rust types derived from `GoType` are built as `syn` AST paths directly
rather than reparsed with `parse_quote!`; this keeps the wasm stdlib compile
path from crashing inside Syn's type parser.
Assignment and compound-assignment lowering should also construct `syn`
assignment/binary expression nodes directly when either side is dynamic; do not
round-trip generated assignment tokens back through `parse_quote!`.

IR validation treats `nil` as assignable/comparable only to nilable types
(pointer, func, slice, map, channel, interface, `any`, `error`, or unresolved
unknowns). Use `TypeEnv::resolve_alias()` and named interface metadata before
deciding nilability; named structs and named numeric/string/bool aliases must
not silently accept `nil`.
Comparison validation has its own assignability check: typed numeric operands
with different types are not comparable merely because both are numeric, while
untyped constants are allowed when representable by the other operand's type.
Non-shift arithmetic and bitwise binary validation enforces the related operator
rule: operand types must be identical unless one side is an untyped constant
that can be converted to the other side's type. `min` and `max` reuse the same
expression-aware compatibility rule after checking that all arguments are
ordered and all numeric or all string. Complex types are numeric for arithmetic
and equality, but not ordered: `<`, `<=`, `>`, `>=`, `min`, and `max` must
reject `complex64`/`complex128`.
Initializer/return validation, equal-count assignment validation, sends, direct
call arguments, `append`, `delete`, expression switch cases, range assignment
targets, composite literal element/key/value/field checks, map index keys, and
index/slice bounds must be expression-aware: typed numeric values are not
assignable across numeric types without an explicit conversion, while
representable untyped constants are allowed. Statement validation seeds its
cloned `TypeEnv` with the current function signature before checking assignment
semantics, because the
compiler-wide pre-scan registers parameter/result names globally and stdlib
functions reuse names such as `hi`/`lo`. Multi-return forwarding still uses the
conservative type-only fallback. Keep unresolved/named types conservative until
imported named return types are package-qualified end-to-end; otherwise
reachable stdlib methods such as `reflect.Value.Field` can appear as `Value` in
importing packages.
`make` size arguments follow the same constant rules as indices: literal
constants must be non-negative integer constants, and two constant slice bounds
must satisfy `len <= cap`; non-constant integer values remain runtime-checked.
Type conversion validation rejects concrete invalid conversions between known
predeclared types, but stays conservative for named and unknown types so generic
underlying-type conversion support can continue to compile real packages.
Untyped integer-valued literal assignability checks must enforce target integer
bounds (`byte = 256`, `byte = 256.0`, and `uint = -1` are invalid) before
falling back to broad constant-kind compatibility. Rune literals are integer
constants for this purpose, so escaped rune values must also be checked against
the target bounds (`byte = '\u0100'` is invalid).
Zero imaginary constants such as `0i` are representable by real numeric types;
nonzero imaginary constants must still be rejected for real targets.
Float constant assignment/conversion must reject overflow for the target float
type (`float64(1e1000)` is invalid) while preserving underflow-to-zero cases
such as `float64(-1e-1000)`.
Type conversions only remain compile-time constants when the conversion result
is a scalar constant type; conversions such as `[]byte("go")` are runtime values
and must not take the untyped-constant assignability path.
Range over an untyped integer constant with a preexisting iteration variable
uses the iteration variable's type, but the range expression itself must still
be representable by that type (`byte` over `256` is invalid).
Shift validation uses separate left-operand and count rules. The right operand
may be an integer-valued untyped constant such as `1.0`, but the left operand
only accepts an integer-valued float constant when the shift count is also
constant; `_ = 1.0 << s` must be rejected without a typed assignment context.

Imaginary literals are treated as untyped complex constants in the Go front end
and lower through `crate::builtin::complex128`; expected `complex64` constant
contexts use the builtin `complex64` constructor instead of a Rust cast.
Complex arithmetic with constant real operands must coerce those operands
through expected-type lowering to the complex side's type so expressions such as
`1 + 2i` and `z + 3` generate `Complex*` operations rather than Rust numeric
casts.
The const evaluator also has a `ConstValue::Complex` path for top-level complex
constants; keep typed `complex64` constants on `crate::builtin::complex64`
instead of emitting a `Complex128` initializer.

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
Pointer dereference lvalues (`*p = x`, `(*p)++`) lower through the IR
addressability path to shared-cell assignments for owning pointers and direct
`&mut T` dereferences for borrowed pointer parameters.
IR expression validation checks composite literals before code generation for
map key/value assignability, required map keys, array/slice index keys, struct
field names, duplicate simple constant keys, and struct field value
assignability. Keep these checks conservative when the type environment cannot
prove the literal's underlying type.
Map type validation rejects non-comparable key types, including slice, map, and
function keys as well as arrays or structs that recursively contain
non-comparable fields. Reuse the same comparability helper for equality and
expression-switch validation so the semantic rule stays consistent.
Array type validation rejects obvious invalid lengths such as negative numeric
literals, non-representable numeric literals, strings, and `nil`. It stays
conservative for identifiers and compound constant expressions until constant
evaluation is represented explicitly in IR.
Expression-switch validation checks the switch tag and case expressions before
case-body compilation: nil tags are rejected, tags and cases must be comparable,
case expressions must be single-valued, duplicate literal/predeclared/known-const
case values are rejected, and each case must be comparable to the tag or to
implicit `true` when the tag is omitted.
Type-switch validation checks semantic constraints after the guard shape check:
the guard operand must be an interface, `nil` and concrete case types must not
be duplicated, and obvious concrete cases for named interfaces must implement
the interface method set.
Type-assertion validation applies the same interface operand and implementor
checks for `x.(T)`: the operand must be an interface, and obvious concrete
assertion targets must implement the named source interface.
Interface type validation rejects duplicate directly declared method names while
recursing through method signatures. Embedded interface duplicate detection still
needs fuller interface method-set modeling.

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
Stdlib resolution must not rely on catching compiler panics. Parser/compiler
gaps should return normal errors and be logged as skips; actual panics should
fail the invoking test or build so wasm does not turn them into `unreachable`
traps.
Root-specific resolved-module cache contention should fall back to uncached
resolution on the waiting worker instead of blocking on the cache `RwLock`;
the duplicate cold work keeps fixture-level integration parallelism saturated.

Stdlib output is pruned at item level from roots such as `crate::fmt::Println`.
Imports whose source references were lowered away may be pruned from generated
modules rather than preserved solely because the Go import existed, but pruning
must not hide a source-level unused normal import. Such imports are rejected
before Rust generation unless they use the blank identifier for side effects.
Pruning must not be used as a substitute for compiling reachable stdlib code
generically.

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

- Closure support is partial; function values use shared `Arc<Mutex<Option<Arc<dyn Fn...>>>>` cells rather than a full Go environment object.
- Arbitrary forward `goto` is not fully supported; direct-label block gotos lower through an IR-planned state loop with direct-local hoisting, while gotos that require broader CFG restructuring remain unsupported.
- `reflect` is not fully supported; currently only the pieces needed by pruned stdlib paths compile reliably
- Source maps can track multiple files in the main package, but imported/local
  package modules do not yet get separate source-map output.

## Conventions

- Lints are workspace-level in `Cargo.toml` — `panic`, `unwrap_used`, `expect_used` are denied
- Test modules use `#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]`
- No comments unless the WHY is non-obvious
- Prefer editing existing files over creating new ones
