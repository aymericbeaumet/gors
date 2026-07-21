# Compiler compliance and performance audit

Date: 2026-07-20
Branch: `perf/compiler-compliance-feedback-loop`
Reference toolchains: Go 1.26.3, Rust 1.96.0

## Scope and non-negotiable boundary

This audit covers the Go scanner/parser/compiler/printer pipeline, generated
program integration harness, CLI feedback loop, and browser/Wasm compiler. The
Go language reference for conformance work is the official
[Go specification](https://go.dev/ref/spec).

The standard-library package graph is resolved and lowered from the pinned Go
SDK through the generic pipeline. This branch adds no package- or
function-specific Rust implementation of a Go stdlib API; compliance fixes land
in generic parsing, typing, lowering, reachability, or language representation.
The persistent resolver cache and browser seed cache mechanically generated
compiler output rather than providing an alternate Rust stdlib. Existing
runtime/ABI primitives and targeted host-resource shims remain limited to the
project's documented boundary and preserve the surrounding generated module.

## Highest-impact findings

1. **The old conformance report could overstate support.** Filtered runs could
   update canonical reports, underscore-prefixed fixtures were skipped by an
   implicit naming convention, previous stdlib passing rows were retained, and
   generated programs compared stdout without requiring matching exit status
   and stderr.
2. **Cold stdlib compilation dominated the feedback loop.** Root sets could
   grow over one compilation and trigger repeated lowering of the same large
   package, while resolved modules and type environments existed only in
   process memory.
3. **CLI cache identity mixed cheap lookup facts with expensive semantic
   validation.** Recursive `GORSPATH` inspection would make a nominal warm
   lookup proportional to the whole search tree, while generated-output writes
   needed a cross-process publication boundary.
4. **Concurrency stopped at coarse package boundaries.** Local packages,
   stdlib type scans, and stdlib resolution did not consistently share one
   deterministic job budget.
5. **The browser discarded its hottest state and stale work could block the
   newest edit.** Worker replacement lost resolver state, source maps crossed as
   nested objects, and synchronous stable Wasm could not consume a cancellation
   message while compiling.
6. **The spec matrix omitted important representation pressure.** Arbitrary Go
   string bytes, exact wide integers, slice capacity/backing behavior, map
   identity/nil writes, and stored method-value receiver capture needed generic
   compiler or runtime-representation fixes.

## Implemented results

### Truthful compliance evidence

- Every generated-program suite has an explicit `fixtures.json` policy.
  Underscore names no longer imply a hidden skip.
- The runner treats Go spawn failures, non-zero exits, and timeouts as harness
  failures. Runnable checks require both programs to succeed and compare raw
  stdout and stderr bytes.
- Filtered, limited, diagnostic, cancelled, or otherwise partial runs cannot
  write canonical reports. Report updates require a complete run plus
  `GORS_UPDATE_CONFORMANCE_REPORTS=1`, and reports are rebuilt from fresh
  evidence rather than merged with stale passing rows.
- The Go-spec source matrix now contains 211 explicit cases, all 211 supported
  and marked passing. Its reduced repros are ordinary runnable fixtures; there
  is no non-running bucket or naming-based skip for matrix coverage.
- New executable coverage exercises assignment evaluation and two-phase writes,
  control-flow evaluation, defer ordering and saved arguments, per-iteration
  loop variables, map lookup/nil behavior, arbitrary string bytes, slice
  capacity and append behavior, wide constants, and method-value capture.
- The canonical JSON report is authoritative only after the complete
  unfiltered suite regenerates it. Focused fixture runs are development
  evidence, never permission to publish a partial report.

### Generic language and runtime fixes

- Non-declaration multi-assignment stages supported target places, evaluates and
  coerces all RHS values into temporaries, and only then writes left to right.
  This covers ordinary lvalues, pointer dereferences/selectors, map keys, index
  targets, multi-result calls, and supported comma-ok forms.
- Go strings retain the generated `String` ABI but use a reversible
  escaped-scalar representation for arbitrary byte sequences. Generic helpers
  own literal construction, raw byte recovery, byte `len`/index/slice,
  `[]byte`/`[]rune` conversion, invalid-UTF-8 range decoding, raw output, and
  bytewise lexical comparison.
- Compiler-only `ExactInt` values use arbitrary-precision `BigInt` for integer
  literal parsing, constant arithmetic, shifts, bitwise operations,
  comparisons, `iota`, `min`/`max`, and representability checks. Exact values
  survive declaration inheritance and imported `TypeEnv` serialization without
  adding a big-integer dependency to generated programs.
- Ordinary maps lower to generic, nil-capable `GorsMap<K, V>` handles. Copying a
  map shares its allocation; nil reads and empty operations remain valid; nil
  insertion/update panics; the existing source-body-driven deep-clone rewrite
  remains independent of ordinary map assignment.
- Generic slice construction validates Go bounds, preserves observable
  length/capacity, and zero-initializes legal reslices into uninitialized
  capacity. Direct local slice aliases carry base/offset/capacity facts so
  overlapping writes synchronize while attached, append within capacity keeps
  the backing relationship, and append beyond capacity detaches. This is
  compiler data flow, not a package-specific special case.
- Method values evaluate their receiver once. Value receivers snapshot a Go
  value copy, pointer receivers retain pointer-cell identity, and interface
  receivers clone their boxed dynamic value into owned storage before the
  closure is constructed.
- Named-result functions with `defer` put the labeled function body inside the
  Go panic boundary, publish the recover payload before the defer stack drops,
  and read the final named results after recovering defers can mutate them.
- Builtin DCE computes an iterative dependency closure. Reachability and
  generic builtin-root expansion repeat until no newly retained builtin impl or
  helper implies another root, preventing indirectly required pointer/string
  helpers from being pruned.
- Panic/recover payloads and panic-hook suppression depth remain thread-local,
  so one goroutine's recover boundary cannot silence an unrelated thread's
  diagnostics.

### Compiler and CLI feedback loop

- Resolver entries use single-flight `OnceLock` cells. An initialized rooted
  entry can satisfy a subset request; selection chooses the smallest
  deterministic superset and compiler DCE still prunes from the actual roots.
  Unfiltered, uninitialized, uncacheable, and cross-package entries are not
  substituted.
- Reachable Go declarations are discovered with an indexed event work queue
  rather than repeatedly scanning every declaration to a fixed point.
- Resolver archives persist mechanically generated Rust modules and serialized
  type environments. Imports validate the exact schema, Go SDK, embedded
  stdlib, compiler source/dependencies, target, profile, and resolver ABI
  fingerprint. Only the `parallel` and `wasm-threads` scheduling features are
  normalized, allowing deterministic stable/threaded cache reuse without
  relaxing any semantic invalidation.
- CLI build/run caches validate source snapshots, eligible directory
  membership, generated file hashes, source maps, compiler identity, and
  executables. Cache publication is atomic and bounded by age, entry count, and
  bytes.
- The pre-parse `GORSPATH` key hashes search-root identity and order without
  recursively scanning directory contents. The post-parse `InputSnapshot`
  validates the exact resolved files and eligible `.go` membership that can
  affect the program.
- Generated output uses an output-directory file lock. Changed files are
  prepared and synced in same-directory temporaries, leaf modules are
  atomically published before coordinator files, stale artifacts are removed
  under the lock, and the manifest is published last.
- `--jobs`, `GORS_JOBS`, and `--timings-json` expose the deterministic task
  budget and machine-readable phase/cache telemetry. Local packages, stdlib
  type scans, and independent stdlib packages use native task pools without
  moving non-`Send` `syn` trees between workers.
- Per-invocation rustc incremental directories were removed because they grew
  the integration cache without helping the single-file rustc invocation
  model.

### Browser and Wasm path

- One persistent worker owns the Wasm instance, resolver state, and one
  source-keyed 16 MiB output LRU. Source maps cross the worker boundary as a
  transferred packed `Uint32Array` rather than nested structured-clone data.
- The worker persists at most one 64 MiB validated resolver archive in
  IndexedDB. On a miss it tries the deterministic gzip seed generated by the
  stable release Wasm compiler. The threaded preview shares that seed through
  the scheduling-independent resolver ABI; incompatible or corrupt state is
  rejected by Rust and deleted.
- Requests are latest-only. If stable synchronous Wasm remains blocked on a
  superseded compile after a short grace period, the main-thread controller
  replaces that worker and starts the newest request with persistent resolver
  state available again. The threaded preview is deliberately excluded from
  this termination path because its controller owns nested Rayon workers; it
  discards stale results after completion.
- Scanner and compiler diagnostics retain UTF-8 byte columns internally.
  Source Map v3, packed browser mappings, generated Rust token positions,
  comment mappings, hover spans, and Monaco diagnostics use zero-based UTF-16
  code-unit columns, with conversion performed against the exact source line at
  the browser/source-map boundary.
- Production remains the stable single-threaded artifact because GitHub Pages
  cannot provide the cross-origin isolation required by shared-memory Wasm.
  The opt-in `wasm-bindgen-rayon` preview uses the same deterministic
  string/reparse compiler architecture and requires COOP/COEP,
  `crossOriginIsolated`, and `SharedArrayBuffer`. Its Rayon pool is capped at
  four workers after reserving one logical CPU for the controller/UI, avoiding
  the severe shared-memory contention observed with unbounded browser core
  counts.

## Final performance measurements

Measurements below use the final working tree on
`perf/compiler-compliance-feedback-loop`, based on commit `6102ee417a57`.
The host is macOS/Darwin 25.5 arm64 on an Apple M4 Pro with 14 logical/physical
cores and 48 GiB RAM. Native builds use Rust 1.96.0 release; browser runs use
Playwright's Chromium 148.0.7778.96. "Cold" means a fresh gors application
cache, not a purged OS page cache.

### Native CLI

The benchmark source imports `cmp`, `fmt`, and `math/bits`. The jobs=1 and
jobs=14 cases use independent cache and output roots; their 21 generated files
are byte-identical.

| Cache state | Jobs | Total | Compile phase | Result |
| --- | ---: | ---: | ---: | --- |
| Fresh compiler + resolver cache | 1 | 117.99 s | 117.80 s | baseline |
| Fresh compiler + resolver cache | 14 | 95.29 s | 95.11 s | 1.24x / 19.2% faster |
| Edited project, resolver archive reused | 14 | 2.27 s | 1.70 s | 42.0x faster than cold jobs=14 |
| Exact output-cache repeat | 1 | 13 ms | skipped | worker-count-independent hit |

The remaining cold cost is mostly package-internal and sequential lowering;
threads improve it materially but do not make cold stdlib generation
interactive. Persistent generic resolver output is the decisive feedback-loop
improvement.

### Browser/Wasm

The deterministic schema-4 seed contains 34 generated modules and 58 type
environments: 3,600,164 bytes raw and 461,124 bytes gzip-compressed. Generating
it from the pinned SDK took 92.38 s once at build time. Runtime results:

| Runtime/cache state | Total | Compile phase |
| --- | ---: | ---: |
| Stable Wasm, bundled-seed first compile | 2.361 s | 1.796 s |
| Stable Wasm, persisted resolver + edited source | 2.365 s | 1.790 s |
| Stable Wasm, exact output-cache repeat | 1.30 ms | cache hit |
| Threaded Wasm (4 Rayon workers), bundled-seed first compile | 2.265 s | 1.710 s |
| Threaded Wasm, persisted resolver + edited source | 2.091 s | 1.580 s |
| Threaded Wasm, exact output-cache repeat | 1.09 ms | cache hit |

Before the resolver ABI was normalized, threaded Wasm rejected the stable seed
and the same first compile took 155.57 s. Strict stable-to-threaded seed reuse
therefore removes that roughly 69x failure mode. The full compiler fingerprints
remain different while both builds emit the same resolver fingerprint, and
Playwright byte-compares stable/threaded output for predeclared, seeded stdlib,
and post-seed resolver cases.

Warm cached type environments and modules no longer enter the Rayon pool. That
reduced the threaded first compile from 3.33 s to 2.26 s; the final seeded run
was 4-12% faster than stable on the two edited-source samples. Shared-memory
scheduling is still workload-sensitive: a deliberately uncached four-package
expansion measured 168 ms stable versus 298 ms threaded. Production therefore
remains stable single-threaded because the current host cannot provide
cross-origin isolation; the threaded artifact remains opt-in rather than being
presented as a universal speedup.

### Test feedback loop

- `cargo test -p gors --lib`: 461.43 s test time before duplicate large-stdlib
  unit proxies, 29.40 s after focused synthetic/local replacements (15.7x).
- Complete Go-spec run with 50 cold fixture-cache misses: 12.49 s after the
  one-time release integration-binary build.
- Immediate complete Go-spec rerun: 50/50 fixture-cache hits, 1.43 s test time
  and 1.62 s wall time.
- Browser compiler worker regression: stable seed restored by threaded Wasm,
  four-worker pool initialized, compile and latest-only scheduling complete in
  1.2 s for the focused case.

Representative measurement commands:

```sh
XDG_CACHE_HOME=/tmp/gors-final-bench/cache-j1 target/release/gors build \
  --jobs 1 --output /tmp/gors-final-bench/output-j1 \
  --timings-json /tmp/gors-final-bench/cold-j1.json project-a
XDG_CACHE_HOME=/tmp/gors-final-bench/cache-j14 target/release/gors build \
  --jobs 14 --output /tmp/gors-final-bench/output-j14 \
  --timings-json /tmp/gors-final-bench/cold-j14.json project-a
GORS_COMPILER_BENCHMARK=1 GORS_WEB_COMPILER_PREBUILT=1 \
  npm --prefix www run test:compiler -- --grep "browser compiler benchmark"
GORS_WASM_THREADS=1 GORS_COMPILER_BENCHMARK=1 \
  GORS_WEB_COMPILER_PREBUILT=1 npm --prefix www run test:compiler -- \
  --grep "browser compiler benchmark"
RUST_TEST_INTEGRATION_PROFILE=release make rust-test-integration-go-spec
```

## Validation contract

The final branch gate is:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p gors --no-default-features --all-targets -- -D warnings
cargo test --workspace --lib --bins --examples
GORS_UPDATE_CONFORMANCE_REPORTS=1 RUST_TEST_INTEGRATION_PROFILE=release \
  make rust-test-integration-go-spec
npm --prefix www run format:check
npm --prefix www run lint
npm --prefix www run test:unit
npm --prefix www run test:compiler
npm --prefix www run test:compiler:threads
```

After the complete Go-spec run, both the source matrix and regenerated canonical
report must contain 211 supported/passing cases, zero non-passing cases, and
100% coverage. Scheduled coverage-guided fuzzing remains separate from the fast
deterministic corpus/property replay used on pull requests.
