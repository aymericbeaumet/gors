# Compiler performance architecture and acceptance contract

Date: 2026-07-22
Status: architecture target; no competitive performance claim has been earned
Comparator: the hermetic Go compiler pinned by `.go-version`

## Target

The production goal is unambiguous: gors must compile behaviorally equivalent
programs faster than the pinned Go compiler, both on the first cold build and
on recurring warm builds. Performance is a design constraint while language
support is added, not a cleanup phase after stdlib compliance.

No current result satisfies that claim. The authoritative backend is still a
single-file bootstrap. The CLI now separates generated-Rust reuse from exact
terminal executable reuse, but it has no cross-process semantic query CAS, and
the Rust source plus rustc path has not been shown capable of winning an
equivalent end-to-end comparison. Until the contract below is satisfied, use
"target" rather than "faster than Go" in project material.

`perf/acceptance-v1.json` currently contains zero promoted scenarios. The
acceptance gate therefore validates its schema but enforces no earned latency
budget yet.

Correctness remains a prerequisite. A fast build that rejects a supported
workload, returns a stale artifact, changes Go-observable behavior, or omits
work performed by the comparator is a failed sample.

## Two metric lanes

Performance reports must keep compiler-engine latency separate from artifact
latency. Neither may be substituted for the other.

| Lane | gors interval | Go comparison | What it establishes |
| --- | --- | --- | --- |
| Compiler engine | admitted immutable source snapshot through verified Rust IR and the selected terminal codegen product, excluding artifact persistence and linking | pinned `go tool compile` on an equivalent import-free package or an explicitly documented package-object harness | frontend, analysis, mandatory representation lowering, and codegen cost; diagnostic because terminal products differ |
| End-to-end artifact | process invocation through an atomically published runnable executable, including discovery, reads, parsing, queries, codegen, generated Rust when applicable, rustc, linking, cache validation, and writes | pinned `go build` through a runnable executable under equivalent cache and target conditions | the user-visible competitive claim |

Internal stage telemetry must also report parsing, indexing, type checking, HIR,
Go MIR, Rust representation lowering, Rust IR verification, terminal emission,
external codegen, linking, cache lookup, serialization, and writes separately.
Stage timings explain the result; they
must never be summed selectively to manufacture an end-to-end win.

Runtime throughput, executable size, debug information, and peak runtime memory
are separate quality lanes. Compile-time acceptance must record them so that a
faster compiler cannot silently produce unusably slow or incomplete artifacts,
but they do not replace the compile-latency contract.

## Required scenarios

The benchmark corpus must be checked in, hermetic, deterministic, network-free,
and versioned by content digest. It must contain representative small, medium,
and large package DAGs once those features are supported. Each artifact is run
against a deterministic oracle before its timing is accepted.

Every workload runs these scenarios:

1. **Cold first build.** Start a new process with empty compiler semantic CAS,
   artifact cache, rustc cache, and `GOCACHE`. Toolchains, module source, and SDK
   source are already installed; downloads are never part of either sample.
2. **No-op warm build.** Start a new process after a successful build with all
   persistent caches and the prior artifact present. This measures validation
   and reuse, not an in-process shortcut. A persistent-session no-op metric may
   be reported additionally, under a different name. An exact executable hit
   must validate and run without resolving or materializing the runtime
   provider, locating or probing rustc through rustup, inventorying its target
   rustlib, or reconstructing a link plan.
3. **Leaf implementation edit.** Change the body of a leaf declaration without
   changing its exported semantic fingerprint. The executable must contain an
   observable edit sentinel so stale-output reuse fails validation.
4. **Dependency implementation edit.** Change a private body in an imported
   package without changing that package's public API. Dependents should remain
   semantically green even though the changed codegen unit and final link may
   need work.
5. **Dependency API edit.** Change a public signature or type fact and update
   its callers. Only the affected reverse-dependency slice may become red;
   unrelated packages must remain reusable.

Cold, no-op warm, leaf edit, and dependency API edit are mandatory competitive
acceptance scenarios. The dependency implementation edit is also mandatory for
the incremental architecture because it distinguishes fine-grained semantic
reuse from whole-package invalidation.

## Reproducible measurement protocol

The certification harness is opt-in and runs on dedicated performance workers,
not shared correctness CI. It must emit raw machine-readable samples as well as
the summary.

- Pin gors commit and profile, Go version, Go experiment flags, Rust toolchain,
  runtime ABI, target triple, linker, benchmark corpus digest, and cache schema.
- Treat generated Rust and executable profiles as separate identities. Debug
  and release must reuse byte-identical generated Rust for the same semantic
  input, while each profile has a distinct terminal action and executable; a
  profile switch may not force semantic recompilation or reuse the other
  profile's binary.
- Use the default user-facing artifact mode for the end-to-end comparison. Do
  not compare a debug-only gors shortcut with a materially different Go
  artifact without labeling it as a diagnostic experiment.
- Give both compilers the same explicit global job budget. Record physical and
  logical core counts; run a full-physical-core certification lane and retain a
  single-worker diagnostic lane for scheduler analysis.
- Run at least 50 measured, paired gors/Go samples per scenario, split across at
  least three independent sessions. Discard setup and calibration iterations.
  Randomize each pair's order with a recorded seed so thermal and temporal drift
  do not consistently favor one tool.
- Report raw and hardware-normalized median (p50), p95, median absolute
  deviation, paired ratios, and bootstrap 95% confidence intervals. Preserve
  every sample; do not remove outliers after viewing the result. A run may be
  rejected only by a predeclared host-health rule.
- Record wall time, CPU time, peak resident memory, bytes read and written,
  process count, query hit and miss counts, invalidated query keys by category,
  terminal artifact bytes, and stage fingerprints.
- Run artifacts and compare exit status and raw output. Changed-input scenarios
  must prove that the new sentinel is present before their timings enter the
  summary.

Cold means empty content caches, not an artificial network install or an
unverifiable claim about the kernel page cache. Alternate compiler order,
pre-stage identical source and toolchain files, and record filesystem state.
Dropping the host page cache is allowed only when the same privileged,
documented operation precedes both sides of every pair.

### Hardware and noise normalization

The primary comparison is a paired ratio on the same host. Promotion runs use a
named hardware class that fixes CPU model and microcode, RAM, storage, OS and
kernel, power policy, and container or VM image. The worker must be idle,
connected to stable power, and free of thermal throttling.

A pinned CPU, memory, and filesystem calibration suite runs before and after
each session. Normalized duration is derived from the recorded calibration
factor, but raw duration and the paired same-host ratio remain visible.
Predeclared rejection rules cover excessive calibration drift, frequency
throttling, swapping, and unrelated host load. Cross-machine synthetic scores
must not be used to turn a loss into a win.

## Achievement and promotion

A scenario first achieves the competitive target only when all of these hold on
every required workload and the authoritative hardware class:

- behavior and artifact validation pass;
- normalized gors p50 is at most 95% of the pinned Go p50;
- normalized gors p95 is at most 95% of the pinned Go p95;
- the upper bound of the paired bootstrap 95% confidence interval for the
  median gors/Go ratio is below 1.00;
- the result reproduces in at least three independent sessions.

The project may claim "faster cold and warm compilation than the pinned Go
compiler" only after the end-to-end artifact lane achieves that result for the
cold build and every mandatory warm scenario. Compiler-engine wins are useful
but cannot establish the product claim by themselves.

Promotion is irreversible by default. The change that first achieves a
scenario must add its corpus digest, hardware class, worker budget, raw evidence
location, and locked normalized p50 and p95 budgets to a versioned acceptance
manifest. The locked budgets get at most 3% headroom over the reproduced result
and may never exceed the pinned Go budgets. From that change onward:

- the promoted scenario is a mandatory non-regression gate;
- a change fails if it exceeds either its locked normalized budget or loses the
  same-host Go comparison after the prescribed rerun policy;
- confirmed improvements may tighten the budget;
- toolchain, hardware-class, workload, or schema changes require a new paired
  calibration and explicit baseline migration;
- a threshold may be relaxed only by an explicit architecture decision with
  published evidence, never by silently rewriting history.

Before promotion, the same benchmark remains a required trend report for
compiler-architecture changes. Query invalidation and deterministic-output
assertions are correctness gates from the moment the query system lands; they
do not wait for a speed threshold.

## Incremental source and identity foundation

### Owned, non-leaking parse products

Each semantic input revision is an immutable, reference-counted
`SourceContent` containing source text, a line index, and a content digest.
`SourceSnapshot` pairs that allocation with one user-facing diagnostic path;
both types belong to `compiler::input` and expose no parser convenience
methods. The compiler stores that presentation path outside Salsa. During its
single scanner pass, the parser publishes ephemeral non-comment token
observations alongside the borrowed AST. The file projection partitions them
into compact owned header/body token streams, normalizes explicit and inserted
semicolons, and excludes trivia and physical coordinates from their
fingerprints. Revision-local declaration ranges live in a separate
`FunctionLayout`. Dropping the last semantic owner releases those streams and
source content independently of presentation state.

`ProgramInput` is the production syntax-unvalidated manifest. Each
`SourceFileInput` owns a reference-counted snapshot; the tracked file projection
creates a temporary AST borrowing one snapshot while the query executes and
publishes no self-reference or `'static` fiction. The parser-owned program and
package graph were deleted with no compatibility shim. The red-green database
tracks the owned per-definition streams, but parsing and semantic lowering
remain file-granular. A trivia edit still executes both queries; unchanged
signature/body products and downstream HIR, MIR, and Rust IR backdate. This is
not incremental parsing. The next boundary is semantic lowering over the owned
per-definition syntax, while preserving the per-file release boundary and
remaining free of self-referential unsafe code.

Parse one file per query. Package merging belongs in semantic indexing, not in
an AST concatenation step, so a one-file edit cannot invalidate every parse
product in its package.

### Stable identities across revisions

Persistent identities are structured semantic keys, not allocation counters,
vector indexes, byte offsets, or traversal ordinals. At minimum:

- `WorkspaceId` identifies the canonical source root;
- `PackageId` derives from the canonical import identity and build context;
- `FileId` derives from package identity plus canonical logical file path;
- `DefId` derives from its owner path, declaration kind, stable declared name,
  receiver identity, and disambiguating semantic key;
- local and syntax identities derive from a stable owner plus an incrementally
  matched syntax anchor and role.

Stable, collision-checked `WorkspaceId`, `PackageId`, `FileId`, and
package-owned `DefId` keys now implement the persistent part of this contract.
A schema-tagged canonical encoder consumes enum-tagged `WorkspaceKey` and
`PackageKey` values directly; callers do not manufacture flattened string
identities. The selected entry manifest is installed under those identities;
catalog packages receive compiler identities only when reachability demands
their admission.
A named definition keeps its identity when it moves between files in one
package, while identical package-clause names at distinct import paths remain
distinct. `NodeId`, `LocalId`, and `BasicBlockId` are still dense owner-local
indexes and explicitly nonpersistent. New declarations may not renumber
unrelated definitions; node/local/block reuse waits for incrementally matched
syntax anchors. Serialized cache keys must retain a complete structured key or
a schema-versioned digest with collision evidence.

## Demand-driven red-green query database

One explicitly owned `CompilerDatabase` is the authority for a compilation
session. It contains immutable inputs, interners, query storage, scheduler,
cancellation generation, diagnostics, memory budget, and cache handle. Compiler
semantics may not depend on thread-local state, process-global mutable state,
current working directory, ambient environment reads, or worker count.

Representative query layers are:

    source_snapshot(FileId)
      -> parse_file(FileId)
      -> package_header(PackageId)
      -> package_scope(PackageId)
      -> public_api_fingerprint(PackageId)
      -> resolve_body(DefId)
      -> type_check_body(DefId)
      -> hir_body(DefId)
      -> mir_body(DefId)
      -> analyze_mir(DefId)
      -> transform_mir(DefId, MirPolicyVersion)
      -> verify_mir(DefId)
      -> lower_rust_ir(DefId, RepresentationPolicyVersion)
      -> verify_rust_ir(DefId)
      -> codegen_unit(CodegenUnitId, ArtifactProfile)
      -> link_artifact(ArtifactId)

Queries are pure functions of declared inputs and other queries. While
executing, the database records exact dependency edges automatically. On a new
revision it uses red-green validation: validate dependencies, recompute only a
red query, compare its stable result fingerprint, and mark downstream queries
green when the value is semantically unchanged.

Public API fingerprints and implementation fingerprints are separate. A
private body edit may invalidate that body's HIR, Go MIR, analyses, Rust IR, and codegen,
but it must not invalidate importer type checking when the public API remains
green. Generic instantiations, interface method sets, constant values, inline
summaries, runtime ABI use, and codegen dependencies each require explicit
edges; a package-wide timestamp edge is not acceptable.

Cycles produce explicit query-cycle diagnostics. Recovery values may support
continued diagnostics, but poisoned results are never serialized as successful
codegen inputs.

## Package DAG and deterministic parallel execution

Header parsing constructs a canonical package DAG before body work. Ready
packages, independent definitions, MIR analyses, and codegen units may execute
in parallel through one deterministic work-stealing scheduler.

There is one bounded global job budget for discovery, parsing, semantics, Go
MIR, Rust representation lowering, emission, external tools, and linking.
Nested queries borrow from
that budget instead of creating phase-specific pools or oversubscribing the
machine. External rustc or linker processes consume budget tokens too.

Work stealing may change execution order but never publication order. Stable
query keys own result slots; diagnostics and artifacts are sorted by stable
identity at publication boundaries. The same inputs must produce byte-identical
diagnostics, query fingerprints, HIR dumps, MIR dumps, and artifacts with one
worker or many workers. A worker-count determinism matrix is mandatory.

Interactive revisions carry a cancellation token. Obsolete work stops at query
and pass safepoints and may not publish partial results. High-priority foreground
queries can preempt speculative work. Backpressure from the shared memory budget
must reduce concurrency before the process swaps or is killed.

## Memory and on-disk semantic cache

In-memory query values are immutable, reference-counted, cost-accounted, and
evictable. Eviction considers recomputation cost, recency, size, and whether a
value is pinned by an active query. Source snapshots and parse trees from old
revisions cannot remain reachable merely through diagnostics or interners.

The persistent cache is a content-addressed store of deterministic semantic and
codegen blobs plus a small query index. Every key includes all relevant inputs:
compiler schema and build identity, target, mandatory representation-policy
version, pinned Go SDK, build tags, package and file identity, source digest,
runtime ABI, and dependency fingerprints. Entries use canonical encoding,
checksums, atomic publication,
concurrent-reader safety, and schema validation. Corruption or incompatibility
is a cache miss, not a compiler error.

The CAS is bounded by configurable memory and disk budgets and supports
cost-aware LRU eviction. A future remote CAS may exchange the same immutable
blobs, but local correctness cannot depend on it. Do not serialize generated
Rust resolver archives, syn trees as semantic state, ambient paths, or live
compiler objects. The existing CLI generated-Rust and terminal manifests remain
outer artifact caches; neither is the semantic query database.

## Fingerprints, dumps, and invalidation tests

Every published stage product has a deterministic schema-versioned fingerprint
and an opt-in canonical dump. A trace records query key, dependency keys, red or
green state, cache tier, execution duration, result size, peak live bytes, and
cancellation. Source paths in dumps use canonical logical identities so
different checkout roots do not perturb the result.

Current HIR, Go-MIR, and Rust-IR fingerprint schema v2 encodes only compact
`SourceRef` values for source provenance. Physical ranges, presentation paths,
adjusted `//line` coordinates, and revision-local definition source tables are
excluded, so relocating unchanged tokens with whitespace or comments does not
by itself perturb the stage digest. These digests are not persistent CAS keys
yet because node and local references still use revision-local dense indexes.

Required invalidation assertions include:

| Change | Must become red | Must remain green |
| --- | --- | --- |
| no-op rebuild | cache validation and requested artifact lookup only | all semantic and codegen queries |
| leaf body edit | changed file parse, changed body semantics, its MIR and codegen, final link | unrelated declarations and package public API |
| dependency private-body edit | changed dependency body and codegen, final link | importer resolution and type checking |
| dependency public-API edit | changed API plus only affected reverse dependencies | unrelated packages and unrelated bodies in affected packages |
| build-tag or target change | queries whose declared configuration input changes | target-independent source and syntax facts where valid |

Each newly supported language feature must add a positive semantic test, a
negative diagnostic test, a MIR verifier test when applicable, an invalidation
test, and a query-cost or benchmark observation. Compliance work that creates
coarse invalidation or unbounded stage growth is incomplete.

### Generated Rust and terminal action identity

The CLI has two independently validated cache domains. `GeneratedRustIdentity`
owns the generated-source schema, driver source-selection schema and
configuration, compiler/SDK build facts, and typed runtime contract. Admission
separately compares the immutable source snapshot captured for the invocation.
Neither may contain runtime provider bytes or provenance, runtime
compatibility, the terminal Rust toolchain or linker, output profile, target
CPU, target features, or an executable action.

Terminal state is separately replaceable and bound to the admitted
generated-Rust identity and `RuntimeDependency`. Provider selection lives here.
A `RustcAction` currently owns an ordered command snapshot; its
`RustcActionIdentity` includes its program, working directory, complete argv,
every generated-Rust filename and content hash, runtime artifact path and
implementation hash, runtime link-plan and compatibility identities, the exact
recorded rustc metadata snapshot, profile, target, explicit portable CPU and
feature policy, and output/publication paths. Debug and release actions
intentionally diverge while consuming the same generated source.

This is not yet a hermetic terminal action. Direct `rustc` execution inherits
ambient environment, chooses the linker and platform SDK/system inputs
implicitly, and the rustc snapshot is a metadata identity rather than a content
identity. The same current action key can therefore produce different bytes.
Before any scenario promotion, replace this with an immutable terminal
toolchain/environment product that content-identifies rustc, its codegen/sysroot
inputs, the exact linker and platform SDK inputs, passes an absolute linker,
owns scratch policy, and uses `env_clear()` plus an ordered explicit
environment. Preserve the cheaper exact-warm path: stored action facts can be
reconstructed without touching a historical toolchain, while terminal misses
perform live admission.

Warm executable admission checks the current source snapshot, generated
manifest, terminal record, action identity, executable content, and executable
mode before any generated-Rust read or live runtime/toolchain resolution. A
valid hit runs immediately without reading runtime bytes or statting rustc. Missing, corrupt,
or stale terminal state does not invalidate generated Rust; only the terminal
selection/action is rebuilt. A generated-source-only consumer may reselect the
provider after generated admission because it still needs a current link
descriptor.

## Terminal Rust feasibility gate

Rust source is an inspectable bootstrap target, but rustc and linking may impose
a cold or edited-build floor above the entire Go build. Optimizing the gors
frontend cannot overcome that floor. This must be tested before full stdlib
compliance, not discovered afterward.

Once the package DAG, representative codegen units, runtime ABI, and timing
schema exist, run a terminal-backend feasibility study on the performance
corpus. For cold, leaf-edit, dependency-body-edit, and dependency-API-edit
scenarios, measure rustc plus link separately and compute the best possible
end-to-end bound by treating all earlier gors stages as zero cost.

The Rust terminal path fails the feasibility gate if, in two consecutive
certification cycles, either:

- its external-codegen-plus-link p50 or p95 alone exceeds 95% of the matching
  full pinned-Go build; or
- the measured pipeline plus an evidence-backed representation-improvement
  forecast cannot satisfy the achievement threshold within the next
  architecture milestone.

On failure, do not spend successive milestones micro-optimizing emission. Build
a direct fast object or machine-code backend, with Cranelift as the initial
candidate, from the same verified Rust IR and versioned runtime ABI. This is a
terminal codegen replacement, not a second parser, type checker, HIR, Go MIR,
Rust representation lowering, or semantic fallback. The backend must consume
the Rust IR's explicit ABI and ownership decisions and pass the same
differential and verifier gates.

An evidence-backed alternative such as a persistent compiler service,
precompiled runtime, package-granular object CAS, and a faster rustc codegen
backend may receive one time-bounded milestone if its lower bound can still meet
the target. If it cannot, the direct backend becomes the production artifact
path. Rust source emission may remain as an inspection/export mode, but it must
not determine semantics or be counted as the competitive production path.

## Current implementation gaps

The following current mechanisms are useful bootstrap behavior but are not the
architecture described here:

- compiler input storage separates canonical semantic `SourceContent` from user-facing
  physical paths. An unchanged checkout-root move preserves all semantic
  products, executes no query, requests no Salsa cancellation, and schedules no
  worker wave while repackaging terminal maps and diagnostics with the current
  request path. `CompilerSession` and the free facade now accept
  syntax-unvalidated `ProgramInput`; one raw-loader invocation reads each
  selected file once, and Wasm builds a direct manifest. Syntax-invalid
  revisions therefore enter the retained query session. Imports, structured
  invalid-import facts, semantic projection, and browser comments share one
  query-owned ephemeral parse. There is still no reusable incremental syntax
  tree with stable syntax anchors or explicit parse-product memory accounting;
- the bootstrap session installs only the entry manifest from the caller-owned
  `ProgramInput` catalog. Unrelated packages create no Salsa inputs, retain no
  database bytes, and execute no queries. The raw loader intentionally performs
  no recursive import or module discovery; reachable expansion must be rebuilt
  as query-owned manifest admission from direct-import facts and resolver
  metadata, not as a parser compatibility layer or session-side package graph;
- the frontend-neutral `source` layer now enforces fixed-width byte lengths and
  exposes checked `TextSize`, half-open `TextRange`, physical positions,
  adjusted coordinates, and presentation-path rebasing without depending on
  the compiler. `compiler::provenance::FileRange` separately pairs those ranges
  with stable `FileId` values. HIR, Go MIR, and Rust IR retain only compact
  owner-scoped `SourceRef` values; a separately tracked revision-local
  `DefinitionSourceTable` maps them to current physical ranges.
  `DiagnosticLocation` distinguishes direct physical, retained source-reference,
  and synthetic anchors. The session resolves and applies `//line` plus the
  current presentation path only when publishing diagnostics, while source maps
  use physical source positions. The remaining provenance gap is terminal exact
  emission anchors: the bootstrap source map still covers function landmarks
  through formatted-token matching;
- workspace, package, file, and definition IDs are stable; node, local, and
  basic-block IDs are still revision-local dense indexes and cannot be
  persistent query or CAS keys;
- canonical HIR, MIR, and Rust-IR fingerprint schema v2 includes symbolic
  `SourceRef` provenance but excludes its physical source table and all
  presentation coordinates. The scheduler root-input digest consumes that same
  physical-location-free HIR fingerprint plus exact direct-callee signatures,
  representation config, and executable role. Revision-local dense node and
  local identities still prevent treating either digest as a persistent CAS key;
- the production `CompilerSession` now reaches function-relative typed HIR,
  per-definition verified and normalized Go MIR, configured verified Rust IR,
  and package assembly with exact self/direct-callee signature dependencies;
  parse/semantic projection remains file-granular, convenience entry points
  retain no session across calls, and no native daemon/watch owner exists. An
  explicit shareable `CompilerHost` now owns one lazy bounded native pool for
  cold/changed per-definition Rust-IR roots; exact no-op and comment-only
  revisions bypass the wave, one body edit offers only that definition, API
  edits also offer actual callers whose signature dependencies changed,
  Wasm/default/free calls stay inline, parallelism requires an explicit host or
  budget, and every revision-scoped snapshot is joined before readiness is
  published or input mutation resumes;
- program installation journals only changed, inserted, and removed sources,
  includes stale-file deletion in the same reversible transaction, and gives
  exact no-op installs no Salsa setter or cancellation. The mutation set is the
  entry manifest today and must expand only with proven reachable imports;
- the browser worker explicitly retains one `CompilerSession` across changed
  edits, consumes query-owned comments, and uses its exact-output cache only
  when that artifact matches the currently installed successful source
  revision; this is a real warm semantic path, but it is not the native artifact
  certification boundary;
- the CLI independently validates generated output and per-profile executable
  action state, but does not reuse semantic queries after an edit. Every
  invocation now loads one immutable source snapshot before cache comparison
  and reuses that exact snapshot for miss compilation; warm generated-output
  and executable hits therefore still pay honest source admission cost;
- the runtime sidecar/link hard cut is complete for native artifacts. Compiler
  and printer products carry only the target-neutral dependency, while the CLI
  verifies and materializes one fixed-recipe precompiled rlib, publishes its
  exact schema-2 producer, compatibility, provider, and link identities, and
  supplies one `--extern` at every native rustc boundary. Producer provenance
  is evidence only; host-neutral target-sysroot compatibility drives selection.
  Native provider production and consumption resolve the same exact pinned
  rustup compiler, independent of Cargo's build compiler. Generated-output
  reuse does not depend on that provider. Exact executable hits validate their
  recorded `RustcActionIdentity` and binary before terminal resolution and
  bypass provider materialization, rustup lookup, rustc probing, target-rustlib
  inventory, and link-plan reselection;
- Unix internal executable admission is descriptor-based and no-follow,
  requires stable metadata while hashing, records mode in the terminal
  manifest, canonicalizes new compiler outputs to `0o755`, verifies inode
  continuity across rename, and syncs the file and directory before commit.
  A byte-identical artifact with execute permission removed is a terminal miss;
- dynamic divide/remainder-by-zero and negative-shift faults currently unwind
  through Rust `panic_any`, so those executions do not yet have Go-compatible
  process behavior and cannot enter behavior-validated performance evidence;
- atomic query counters, Salsa execution and cancellation-request counters,
  scheduler wave evidence, and invalidation tests exist, and CLI/performance
  timings record the exact compiler job budget. CLI timing report schema v5
  records `cli.source_load` before `cli.cache_lookup` for hits and misses;
  it does not separately phase cache pruning and lock waits, generated and
  executable admission, runtime/toolchain selection, terminal input validation,
  internal/manifest publication, identity rechecks, or public publication.
  Result schema v4 does not type or revalidate those internal timings, and
  `--jobs` does not yet constrain rustc/LLVM/linker work. Reports therefore do
  not yet expose causally complete terminal evidence, complete dependency
  traces, retained memory, or a foreground cancellation protocol;
- `gors build` now owns production rustc/link and durable atomic executable
  publication. Its portable production policy is `opt-level=2`, LTO disabled,
  debug info disabled, `target-cpu=generic`, and an empty requested target
  feature set. Performance result schema v4 invokes that command directly and
  rejects the deleted generated-source/link-descriptor/external-rustc driver.
  This makes new end-to-end evidence structurally promotable, but no scenario
  has yet met the statistical acceptance threshold;
- no single global scheduler, cancellation generation, memory budget, or
  semantic CAS yet spans the compiler and external artifact tools;
- the build embeds roughly 17 MB of raw selected SDK source into each compiler
  artifact behind one coarse global SDK fingerprint instead of loading
  content-addressed reachable package shards;
- browser worker timings are valuable warm-query UI telemetry, but they are not
  the native cold/warm artifact certification protocol.

These are P0 foundations, not optional tuning: owned incremental syntax and
CAS-ready semantic fingerprints; query-owned module/import discovery; exact
emitter anchors from Rust-IR source references; cross-process semantic CAS; one
global scheduler with cancellation and memory backpressure; a precompiled
runtime and terminal-rustc feasibility decision; reachable content-addressed
SDK shards; and enough generic language and package support to benchmark real
stdlib work.

Some cutover foundations already point in the correct direction: source mapping
is an explicit `SourceMapPlan`, `CompiledProgram` separates its entry from a
deterministic module map, and the printer has no output cache or post-syn
ordering transform. Preserve those explicit value boundaries as they move into
queries; do not reintroduce hidden context or semantic printer work. The current
plan's formatted-token name matching is not provenance-correct, however, and
must be replaced by exact emitter anchors before source maps become cached query
products.

Do not rename any of these mechanisms to "incremental compilation." Replace
them at the owning boundary and delete the obsolete path in the same change.

## Delivery order

1. Install the machine-readable benchmark schema, stage tracing, hermetic
   corpus, and current losing baseline. Never invent or backfill measurements.
2. Preserve owned per-file snapshots, stable workspace/package/file/definition
   keys, and the completed `SourceRef`/definition-source-table split; add
   reusable syntax anchors before treating schema-v2 stage digests as CAS
   identities.
3. Harden the production-session route from tracked file/package facts through
   configured Rust IR, extend retained sessions beyond the browser to native
   editor/build-daemon owners, and prove bounded invalidation before broad
   language expansion.
4. Add the package DAG, API/body fingerprints, global scheduler, cancellation,
   memory budgets, and local semantic CAS while generic language support grows.
5. Run the terminal Rust feasibility gate as soon as representative package
   codegen exists; replace the production artifact backend aggressively if the
   lower bound cannot win.
6. Add proof-driven representation refinements together with invalidation and
   cost tests, and promote each competitive scenario immediately when it earns
   the target.

This order deliberately intertwines compliance, incrementality, parallelism,
and representation quality. A late cache or late representation refinement
cannot repair unstable identities, coarse dependencies, leaked source
revisions, or an artifact backend whose irreducible cost already exceeds the
competitor.

## Primary design references

- Salsa's [overview](https://salsa-rs.github.io/salsa/overview.html) and
  [incremental algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)
  define the in-process red-green kernel now hidden behind `compiler::db`.
  Salsa is an implementation mechanism, not the public compiler architecture or
  a substitute for the repository's scheduler, memory, CAS, and artifact
  contracts.
- Salsa's [durability](https://salsa-rs.github.io/salsa/reference/durability.html)
  and [tuning](https://salsa-rs.github.io/salsa/tuning.html) guidance informs
  explicit high-durability build inputs and future memo-eviction measurements;
  every optimization still requires gors-specific invalidation evidence.
- The Rust compiler's documented [red-green incremental query
  algorithm](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
  motivates dependency recording, result fingerprints, and green downstream
  reuse after a recomputed input proves semantically unchanged.
- Go's own [action-ID and content-ID build
  design](https://go.dev/src/cmd/go/internal/work/buildid.go) is the comparison
  baseline for safe artifact reuse and makes clear that gors must invalidate on
  compiler and complete action inputs, not timestamps.
- Cranelift's [`ObjectModule`](https://docs.rs/cranelift-object/latest/cranelift_object/struct.ObjectModule.html)
  confirms that a verified-representation-IR-to-object path can remain an
  ordinary terminal backend producing relocatable object files rather than a
  second semantic compiler.
- LLVM's current [distributed ThinLTO
  design](https://www.llvm.org/docs/DTLTO.html) reinforces the separation between
  a compact global summary/index and independently schedulable backend jobs. It
  is a useful codegen-unit model, not permission to add a second scheduler or
  hide link cost from the acceptance metric.
