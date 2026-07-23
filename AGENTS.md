# AGENTS.md — Guidelines for AI Agents

Keep this file current when an architectural decision, invariant, or
non-obvious operating constraint changes.

## Project

gors is a Go-to-Rust compiler written in Rust. The compiler has completed an
intentional hard cutover. There is one supported architecture:

    Go source
      -> scanner and parser
      -> Go AST
      -> semantic analysis and typed HIR
      -> explicit-order Go MIR and verification
      -> representation-neutral Go MIR transforms and reverification
      -> mandatory Rust representation lowering
      -> verified Rust IR
      -> terminal Rust syn emission
      -> prettyplease
      -> Rust source

Backward compatibility with the removed compiler is not a goal. Prefer a clear
unsupported diagnostic over fallback to an old lowering path.

## Non-negotiable compiler boundaries

### One backend

- The typed HIR, Go MIR, and Rust IR pipeline is the only production compiler.
- Do not add a direct Go AST to syn path, per-node fallback, compatibility
  adapter, feature flag, or second backend.
- Delete obsolete code instead of leaving dormant legacy modules in the tree.
- A Go construct not represented by the new semantic model must fail with a
  structured source diagnostic.

### Go AST

- The AST is a parser product, not a backend IR.
- Semantic identity, typing, evaluation order, and Rust representation do not
  belong in parser nodes.
- Backend code may consume the AST only while constructing semantic facts and
  HIR. MIR and later phases must not retain Go AST references.
- Incremental parse products must own or reference-count their source snapshot
  and must be independently evictable per file. `Box::leak`, leaked arenas, and
  self-referential `'static` ASTs are forbidden in the query database.
- Raw source ownership lives in `compiler::input`. `SourceContent` and
  `SourceSnapshot` expose immutable bytes and indexing metadata, never parse
  methods; the compiler file-projection query alone calls
  `parser::parse_file` to create an ephemeral borrowed AST.
- Parse files independently. Multi-file package composition belongs in the
  semantic package index, not an AST merge that invalidates every file.
- The production input boundary is `ProgramInput` -> `SourceFileInput` ->
  `SourceSnapshot` -> query-owned file projection. `SourceSnapshot::from_source`
  validates the fixed-width physical byte domain but deliberately does not
  validate Go syntax, so syntax errors remain parse-query outputs. A projection
  creates one temporary AST borrowing only that file's immutable snapshot and
  publishes owned semantic products. The parser-owned `ParsedProgram` and
  package-graph layer were deleted; do not recreate either a package-wide AST
  or a parser-side program model.

### Typed HIR

HIR is the canonical semantic representation. It must:

- use stable definition and source-file identities, and stable local and node
  identities once the incremental syntax layer can supply them;
- retain compact compiler-owned `SourceRef` values on diagnosable nodes, never
  byte ranges, paths, or adjusted coordinates;
- resolve names before MIR construction;
- represent exact Go types and exact untyped constants;
- distinguish definitions, aliases, instantiations, values, places, and
  constants without string-path inference;
- make unsupported constructs explicit.

Do not add an Unknown escape hatch plus side tables. Extend the type algebra or
reject the program.

HIR should be smaller and more semantic than the Go AST. It must not become a
second copy of every parser node.

Stable means stable across revisions: IDs may not be allocation counters,
vector indexes, byte offsets, or traversal ordinals. Inserting or reordering an
unrelated declaration must not renumber existing definitions or invalidate
their queries. Persistent keys use structured workspace, package, file, owner,
and declaration identities with collision-checked interning.

The current identity boundary implements stable `WorkspaceId`, `PackageId`,
`FileId`, and package-owned `DefId` keys. `NodeId`, `LocalId`, and
`BasicBlockId` remain owner-local dense indexes and are intentionally
nonpersistent until reusable syntax anchors exist. They may appear inside one
revision's HIR or IR, but must not become independent query or CAS keys.

### Explicit-order MIR

MIR owns executable semantics. It must make these facts explicit before Rust is
generated:

- Go evaluation and assignment order;
- temporary lifetime and sequence points;
- control-flow edges;
- places and projections;
- calls, returns, panic edges, and deferred work;
- aliasing, escape, and effects facts used by representation lowering.

Go MIR contains Go executable meaning, not Rust ownership operations. Reads do
not become Rust moves or clones here. Every MIR transform must preserve types,
`SourceRef` provenance, panic order, and effects and must be checkable by a verifier.
The transform set may begin empty; exact-Go constant folding, CFG
simplification, and DCE belong here when introduced, never inside target
representation lowering.

### Rust representation lowering

Rust representation lowering is the mandatory bridge from verified Go MIR to
verified Rust IR. It is not an optional optimizer and there is no flag or path
that bypasses it. This stage owns:

- concrete Rust value and runtime-ABI representations;
- storage classes, initialization, and drop points;
- explicit copy, clone, move, shared-borrow, and mutable-borrow decisions;
- control-flow structuring and runtime intrinsic selection;
- proof-backed no-copy, last-use, escape, alias, and effect refinements.

The initial policy is deliberately conservative: copy values that are actually
`Copy`, clone owned non-`Copy` values, and keep unstructured control flow when a
structured form has not been proven. As analyses improve, this same mandatory
stage makes output more idiomatic without changing Go behavior or adding a
post-syntax repair pass.

Rust IR is semantic and syntax-independent. It must retain `SourceRef`
provenance, make every ownership and representation decision explicit, and pass
its own verifier before emission. Both Rust syntax emission and any future
direct object backend consume this same product; Rust IR must not contain syn
nodes or require reparsing generated Rust. Representation lowering must reject
invalid MIR; it must never repair it.

Rust-IR effect summaries include both preserved Go-observable effects and
effects introduced by the selected representation. Go strings use immutable
static-or-shared backing with an explicit byte range: compiler literals allocate
nothing, clones share backing, and concatenation may reuse a uniquely owned
dynamic byte buffer, when capacity permits, only after representation lowering
proves a last-use move. Remaining allocation or growth must stay explicit. The
verifier derives or checks effects from explicit operations; lowering must not
blindly copy the Go-MIR summary.

### Terminal syn emitter

- syn is an output syntax tree, never a semantic IR.
- The emitter renders verified Rust IR as Rust syntax. It must not discover Go
  types, repair evaluation order, perform reachability, infer ownership or
  representation, or recognize stdlib functions by generated Rust shape.
- Do not add semantic post-syn passes. Formatting is the only normal operation
  after emission.
- Keep syn and quote usage confined to the emitter and the narrow public output
  facade. Any other occurrence requires an explicit architectural review.

### Runtime boundary

A runtime library is expected, but its boundary is strict:

- allowed: Go value representations, allocation and alias identity, interfaces,
  maps, slices, strings, pointers, channels, goroutines, panic/defer/recover,
  scheduler support, and explicit host-resource primitives;
- forbidden: Rust replacements for public Go stdlib functions or methods,
  generated-module patching, and helpers that compensate for missing semantic
  lowering.

Runtime entry points form a versioned ABI. Language-intrinsic intent must be
explicit and typed in HIR and Go MIR rather than inferred from names; mandatory
Rust representation lowering selects an exact typed ABI operation in Rust IR.
Every operation is documented and tested independently. Stdlib packages remain
Go source compiled through the same frontend as user packages.

That Rust-IR selection is the hard-cut boundary. Rust IR carries canonical
`PrimitiveOp` and `RuntimeOp` values from `gors-runtime-abi`; do not add
compiler-local operation shadows, print plans, signature tables, effect tables,
or runtime-symbol matches. Print intrinsics expand into ordered single-operation
runtime-call blocks during representation lowering. Wrapping integer arithmetic
is a typed primitive emitted directly as Rust wrapping operations, while only
operations that need a versioned runtime symbol remain `RuntimeOp` values.

Verification derives each function's canonical `RuntimeRequirement` from its
explicit operations and constants. Verified function products retain that set,
and package products union the cached function sets in stable operation-ID
order. Runtime value representations such as `GoString` are not yet separately
sliceable requirements, so artifact packaging must link the runtime
unconditionally rather than incorrectly omitting it for a type-only use.

`gors-runtime-abi` owns the canonical typed boundary. Its target-neutral
`RuntimeAbiManifest` defines runtime value types, exact operation signatures,
symbols, semantic contract version, and required capabilities; compiler query
keys retain its typed `ContractIdentity`, never a scraped version string. A
separate `RuntimeArtifactManifest` composes that contract with the exact target
model, provided capabilities, and implementation hash. Target or implementation
changes select another artifact without pretending the language contract
changed. `RuntimeType` names semantic ABI categories, not generated Rust path
spellings; runtime crate paths belong to artifact selection and terminal
emission so changing packaging does not pretend the language contract changed.

The semantic compiler does not parse, inject, or patch runtime source. The
remaining source-bundled bootstrap packaging is transitional and must be removed
in one hard cut: generated Rust will reference typed Rust-IR runtime operations,
and artifact packaging will select, validate, and link one content-addressed
precompiled runtime artifact. Do not add a second optional sidecar path beside
the bundled module.

### Resolver boundary

The resolver is source metadata only. It may expose:

- whether a pinned Go SDK package exists;
- its build-selected Go source files;
- its build-selected `//go:embed` assets and content hashes;
- its classified cgo, Go assembly, and system-object inputs;
- its direct import paths;
- a stable generated module name.

The resolver must not type-check packages, emit or patch Rust, recover partial
declarations, or cache generated Rust or syn trees. Incremental work belongs in
the compiler-owned semantic query database keyed by source and explicit build
inputs, not in resolver-local caches.

Do not ship the entire uncompressed Go SDK as Rust string constants in every
native and Wasm compiler artifact. The target distribution is a small canonical
package/file/import/hash index plus content-addressed compressed source shards.
Native builds load verified shards from a sidecar store with bounded caching;
Wasm fetches immutable shards on first reachable import and may cache those
input shards in browser storage. Query keys depend on reachable file hashes,
not one global SDK-content fingerprint.

The build host selects only the downloadable Go SDK archive. Cargo target
OS/architecture select Go source build constraints (`wasm32-unknown-unknown`
maps to `GOOS=js GOARCH=wasm`); target metadata must never fall back to the host.

### Incremental and parallel foundation

Incrementality and parallelism are correctness-relevant architecture, not a
late optimization layer. The compiler must be organized around one explicitly
owned, demand-driven red-green query database with fine-grained dependency
edges. Public API and implementation fingerprints are separate so private body
edits do not re-type-check importers.

- Query values are immutable, deterministic, reference-counted, cost-accounted,
  and evictable under an explicit memory budget.
- An on-disk content-addressed semantic cache uses canonical encoding, checksums,
  atomic publication, schema validation, and complete compiler, SDK, runtime
  contract, source, and dependency keys. Artifact target, format, toolchain,
  capabilities, and implementation identity belong only to post-Rust-IR link
  and executable cache keys. Corruption is a cache miss.
- A canonical package DAG exposes ready work. One bounded global job budget and
  one work-stealing scheduler cover parsing, semantics, MIR, Rust
  representation lowering, codegen, external tools, and linking; nested phase
  pools are forbidden.
- Cancellation prevents obsolete revisions from publishing results. Foreground
  work can preempt speculation, and memory backpressure reduces concurrency.
- Scheduling order and worker count must not affect IDs, diagnostics, stage
  fingerprints, dumps, or output bytes. Publication is sorted by stable key.
- Compiler semantics may not depend on mutable process globals, thread-local
  contexts, the current working directory, or ambient environment reads.

Current checkpoint: the Salsa-backed `compiler::db` facade owns explicit
source, package, and build inputs. Its tracked path parses each file projection
once, sharing that temporary AST between indexing and semantic lowering, and
reaches function-relative typed HIR, per-definition verified MIR, and mandatory
normalized/reverified MIR, configured verified Rust IR, and deterministic
package Rust-IR assembly. Function verification reads only its own and direct
callees' signatures; unrelated declaration or signature edits must leave a leaf
function's HIR, MIR, normalized MIR, and Rust IR green. Production program
compilation delegates to `CompilerSession`; convenience functions create a
short-lived session, while the browser worker retains one explicitly across
edits. Native retained sessions may share one explicit `CompilerHost`: it owns
one lazy fixed-capacity job pool. A tracked per-definition root-input digest
covers physical-location-free typed HIR fingerprints, self and direct-callee
signatures, Rust representation config, and executable role. Retained sessions fan out only
roots whose digest changed, prune removed or renamed roots, publish readiness
only after every revision snapshot joins, and bypass the wave for exact and
comment-only edits. Free convenience calls, default sessions, and Wasm remain
inline; parallel sessions require an explicit host or job budget, and the CLI
owns an explicit positive job budget. Queries must not create nested pools or
submit scheduler work. This is not yet the global scheduler for parsing,
external codegen, linking, cancellation, or memory admission, and a native
daemon or watch mode still does not exist. Terminal syn emission
remains outside the semantic queries. Parsing and semantic projection are still
file-granular, although tracked function fields, compact per-definition
`SourceRef` values, and separate definition source tables allow unchanged
sibling stage products to backdate. Query and scheduler counters
are not a memory budget, complete cancellation protocol, global scheduler, or
persistent CAS; do not claim those target properties from the current kernel.

Semantic source inputs are immutable `SourceContent` values containing text,
line indexes, and a content digest under a stable logical file identity. Exact
physical paths and browser URIs are presentation state outside Salsa. Moving an
unchanged checkout while preserving package identity and package-relative
logical filenames must execute zero semantic queries, request no Salsa
cancellation, and schedule no new worker wave; terminal diagnostics and source
maps still use the path snapshot installed for that request. A package-relative
filename change creates a new `FileId` and remains semantic, because filenames
can affect Go build selection. Raw Salsa snapshots are scheduler-internal and
must never escape to callers that could retain them across an input mutation.

Token positions carry an explicit `SourceOrigin`. Initial filenames, Windows
paths, and URIs are retained byte-for-byte; explicit `//line` filenames are
resolved lexically against the initial source directory without host-platform
`Path` normalization. Stable file identity still comes from the manifest's
logical path. Physical comment coordinates are derived from byte offsets and
`SourceContent`, and query-owned import issues retain explicit virtual origins.
The crate-level `source` module owns checked fixed-width `TextSize`, half-open
`TextRange`, one-based physical line/UTF-8-byte-column coordinates, adjusted
Go display coordinates, and presentation-path rebasing. Stable semantic file
identity is deliberately absent from this frontend layer;
`compiler::provenance::FileRange` is the compiler-owned pairing of `FileId`
and `TextRange`. The `source`, `scanner`, `parser`, and `token` modules must
never depend on `compiler`; the semantic compiler consumes their products in
one direction. `SourceContent` construction rejects source lengths outside the
u32 byte-offset domain and stores fixed-width line starts. Adjusted Go display
columns are explicitly `Hidden` or `Known`, so a two-field
`//line file:line` directive cannot conflate hidden column zero with a physical
byte position. The scanner now records a typed
`SourceCoordinateMap` and ordered `LineDirectiveSegment`s during its existing
lexical pass while maintaining independent physical and adjusted counters;
both `Scanner` and its production `IntoIter` expose the consumed map without a
second scan. Directive transitions at EOF are ignored, matching
`go/token.File`, and empty two-field filenames clear the adjusted filename
while empty explicit-column forms retain it. The one public `parse_file` path
now publishes `ParsedFile`, pairing its ephemeral borrowed AST with the complete
scanner-built map; every public `ParserError` owns the consumed map, an exact
physical zero-width `TextRange`, and a separate adjusted filename plus typed
`LogicalLineColumn`. The file-projection query retains the same map on both
success and failure and exposes it as an ordinary query output; it never
rescans or reconstructs line directives.

Semantic provenance has completed its hard cut. HIR, Go MIR, and Rust IR retain
only compact, owner-scoped `SourceRef` values. A separately tracked,
revision-local `DefinitionSourceTable` maps those references to physical
`FileRange` values for the current source revision; it is not embedded in a
semantic stage product. Moving tokens with whitespace or comments may replace
the table while leaving unchanged semantic products green. Stage fingerprint
schema v2 encodes only `SourceRef` for source provenance, never a physical
range, filename, adjusted coordinate, or definition source table. Dense node
and local references are still revision-local and therefore are not persistent
CAS identities until reusable syntax anchors exist.

`DiagnosticLocation` keeps the three ownership cases explicit:
`Physical(FileRange)` for a frontend byte anchor, `Source(SourceRef)` for a
diagnostic retained by semantic or IR products, and `Synthetic` when no source
exists. The session resolves a `SourceRef` through the current definition source
table and applies the `//line` coordinate map and current presentation path only
when publishing a user-facing diagnostic. Source maps instead consume physical
source ranges and physical line/byte-column coordinates; `//line` projection is
display-only and must never rewrite source-map origins. `SourceSpan`,
`FunctionProvenance`, the old `compiler::db::provenance` module, and arithmetic
function-relative rebasing were deleted. Do not recreate them, collapse these
domains again, or restore an AST-only parse compatibility entry point.

The file projection owns decoded direct-import occurrences, structured invalid
imports, and owned source comments from its one ephemeral parse. Those products
contain `FileId` and content-relative provenance, never checkout paths. Import
paths are sorted and deduplicated only at the package-analysis boundary;
occurrence products preserve source order and duplicates.

`CompilerSession` and the free compiler facade now accept the syntax-unvalidated
`ProgramInput` model directly. The CLI uses `workspace` to select and read raw
command-line files exactly once; Wasm constructs a raw browser manifest without
a presentation-layer parse. Syntax failures are therefore parse-query outputs
inside the retained session. The browser also consumes query-owned comments, so
imports, invalid-import facts, semantic projection, and comment projection share
the file projection's single ephemeral parse. CLI timing report schema v5
reports `cli.source_load` before `cli.cache_lookup` on both hits and misses:
every invocation loads one immutable `LoadedProgram` and `InputSnapshot`, then
uses that exact revision for cache comparison and, on a miss, compilation.

Workspace and package identities are enum-tagged `WorkspaceKey` and
`PackageKey` values encoded directly by the collision-checked semantic interner;
do not flatten them into caller-constructed strings. `ProgramInput` is the
caller-owned package catalog for one invocation. The bootstrap session installs
only its entry package manifest; unrelated catalog packages must not become
Salsa `SourceInput` or `PackageInput` values, retain database bytes, or advance
the query revision. Future import expansion must materialize reachable package
inputs on demand from direct-import facts and resolver metadata.

Session input installation is one delta transaction. The compiler journals only
changed, inserted, and removed source inputs, includes stale-file removal in the
same rollback boundary, and rolls mutations back in reverse application order.
Exact no-op installs must not call a Salsa setter or synthesize rollback snapshots.
Do not restore the deleted O(all-active-files) snapshot/rollback path or an
all-`program.packages()` installation loop. Future reachable-package admission
must extend this transaction rather than constructing a session-side graph.

The filesystem workspace loader requires an explicit caller-owned
`WorkspaceKey`; it must never synthesize an ad-hoc identity or derive semantic
identity from a checkout path. The CLI boundary owns the stable `gors-cli`
workspace key used for command-line builds and runs.

The raw workspace loader deliberately performs no recursive module or import
discovery. The deleted parser package graph has no compatibility shim. Rebuild
module resolution as query-owned manifest expansion from decoded direct-import
facts and resolver source metadata; never reintroduce parser recursion or a
second pre-query parse.

Source mappings and diagnostics are ordinary explicit outputs. The current
`SourceMapPlan` follows that rule and is safe to build or consume independently;
its Go origins come from physical definition-source-table ranges, not adjusted
`//line` display coordinates. Preserve that ownership model when it becomes a
query result. Do not reintroduce a thread-local source-map context or create
another exception to the no-global-state rule.

The bootstrap mapper now separates original Go names from generated Rust lookup
tokens, but formatted-token matching is not the target source-map architecture.
Terminal emission must eventually return exact stable emission anchors paired
with Rust-IR provenance; rendering resolves those
anchors to generated byte ranges, and Source Map v3 conversion consumes that
explicit product. Do not infer mappings by matching identifier text or token
occurrence order.

The production performance target is to outperform the hermetic Go compiler
pinned by `.go-version` on behavior-validated cold builds and recurring warm
builds. Compiler-only and runnable-artifact timings are reported separately.
Cold, no-op warm, leaf edit, dependency private-body edit, and dependency API
edit are distinct scenarios. The exact measurement, promotion, hardware, and
terminal-backend decision rules live in `COMPILER_PERFORMANCE.md` and are
architectural requirements.

As of 2026-07-22, `perf/acceptance-v1.json` has zero promoted scenarios. The
repository therefore makes no faster-than-Go claim and enforces no earned
latency budget yet; correctness, determinism, and invalidation assertions still
apply while measurements remain trend evidence.

## Initial migration frontier

The first authoritative backend slice intentionally supports a narrow executable
subset:

- one source file in one package, with no imports;
- primitive `bool`, 64-bit bootstrap `int`, and byte-string values;
- exact scalar constants;
- free functions, parameters, named results, and locals;
- direct function calls and print or println intrinsics;
- scalar expressions, assignments, returns, if, for, break, and continue.

The current executable claim covers non-panicking scalar executions only.
Dynamic division or remainder by zero and negative dynamic shifts still reach
Rust `panic_any`; replace that bootstrap boundary with versioned Go
panic/process semantics and process-level differential tests before claiming
those faulting executions as compliant.

This is a bootstrap frontier, not a compatibility claim. Until implemented in
HIR and MIR, expect explicit failures for:

- multi-file and imported package compilation, including the Go stdlib;
- package variables, declared composite types, methods, and generics;
- arrays, slices, maps, structs, pointers, interfaces, and function values;
- range, switch, type switch, select, labels, closures, defer, panic/recover,
  goroutines, and channels;
- unsafe and host-resource integration.

Narrow integer types, unsigned integers, and floating-point values are also
explicitly unsupported until their exact Go conversion, overflow, comparison,
and runtime representation rules exist in HIR and MIR.

Regressions against the former backend are accepted during the cutover. Do not
hide them by routing a fixture through removed code.

The existing Go-spec, stdlib, repository, and arbitrary-program fixtures are a
prioritized backlog and differential oracle. Pre-cutover conformance reports
are historical artifacts and are not evidence for the authoritative backend.
Only a complete, unfiltered rerun may establish a new baseline.

The narrow frontier does not suspend performance architecture. Owned parse
products, stable cross-revision identities, query boundaries, deterministic
stage fingerprints, and cost/invalidation tests must land before the feature
surface becomes large. Do not defer them until stdlib compliance.

## Source organization

- `compiler/mod.rs` is an orchestration and public-facade module. Canonical
  stages live directly below `compiler/`; do not add a redundant `backend/`
  namespace when only one backend exists.
- Split implementation by semantic responsibility, not arbitrary line ranges.
  A module should have one reason to change and a narrow internal API.
- First-party source files have a hard limit of 1,000 physical lines. Prefer
  roughly 300 to 700 lines; approaching the limit is a signal to extract a
  coherent module.
- Substantial tests live in sibling `tests.rs` files or integration-test
  modules. A large implementation file must not also carry a large inline test
  module.
- `gors-cli/src/main.rs` is dispatch only. Command orchestration, option
  definitions, cache-path policy, generated-output publication, diagnostics,
  and the `rustc` boundary live in focused sibling modules; do not rebuild a
  monolithic CLI entrypoint.
- Generated and vendored sources are excluded from the size budget. Any other
  exception requires a documented architectural reason and an explicit guard
  entry; grandfathering a large file is not a reason.

## Parser contract

`gors/src/parser/functions.rs::Parser::parse_type_parameters()` returns the
private `parser::TypeParameterParse` enum for bracketed forms it consumes. Keep
slice and array prefixes, `[]T` and `[N]T`, as explicit enum variants rather
than sentinel `ast::FieldList` values. Function declarations may convert
consumed prefixes into invalid type-parameter lists so semantic signature
validation can report a signature error instead of making parsing fail early.

Parser and scanner acceptance are independent of backend conformance. Do not
weaken parser behavior to fit the bootstrap backend.

## Repository map

    gors/
      src/
        artifact/           terminal runtime and artifact packaging inputs
        scanner/            Go tokenization
        parser/             independent Go file parsing
        ast/                parser-owned Go AST
        compiler/
          db/               demand-driven compiler queries and telemetry
          input/            syntax-unvalidated compiler input manifests
          fingerprint/      canonical stage encoders and fingerprints
          session.rs        reusable production compilation session
          semantic/         name resolution, typing, and typed HIR construction
          mir/              explicit-order lowering, data model, and verifier
          rust_ir/          explicit Rust representation and ownership IR
          lowering/         mandatory Go MIR to verified Rust IR lowering
          provenance.rs     SourceRef and revision-local physical source tables
          emit.rs           terminal Rust syntax emission
          hir.rs            typed high-level IR
          ids.rs            compiler semantic identities
          types.rs          exact Go type model
        resolve/            embedded Go SDK source metadata only
        printer/            syn formatting and file layout
        source/             frontend-neutral physical and adjusted coordinates
        sourcemap/          Go to Rust source maps
        workspace/          raw filesystem source selection and loading
        token/              Go token definitions
        error.rs            user-facing parse diagnostics
        lib.rs              library entry point
      tests/
        test_integration_go_repositories.rs
        test_integration_go_spec.rs
        test_integration_go_stdlib.rs
        test_integration_go_programs.rs
        common/
        fixtures/
        tools/go_oracle/
    gors-cli/               commands and generated-artifact cache manifests
    gors-runtime-abi/       typed runtime contract and artifact identities
    gors-runtime/           runtime ABI and Go value representations
    perf/                   native performance evidence schemas, corpus, and gates
    www/                    browser application

The exact public compiler facade may remain temporarily small enough for the
CLI and Wasm callers, but every entry point must delegate to the same backend.
An API wrapper is acceptable; an alternate semantic path is not.

`COMPILER_PERFORMANCE.md` is the normative performance and incremental
architecture contract. `COMPILER_AUDIT.md` records the broader replacement
decision and roadmap.

## Development workflow

Fast local checks:

    cargo fmt --all
    cargo check --workspace
    cargo test -p gors --lib

Repository gates:

    make rust-lint
    make rust-build
    make rust-test-unit

Run a focused generated-program fixture while expanding the frontier:

    make rust-test-integration-go-spec-fixture FIXTURE=<fixture>
    make rust-test-integration-go-stdlib-fixture FIXTURE=<fixture>

The broad generated-program suites are expected to expose migration backlog
until their constructs have native HIR and MIR support. A red unsupported
fixture is actionable coverage; it is not permission to restore legacy code.

Performance certification is opt-in and belongs on dedicated, normalized
workers. Once any scenario is promoted under `COMPILER_PERFORMANCE.md`, its
locked median and p95 budgets become mandatory non-regression acceptance gates.
Never publish a faster-than-Go claim from browser cache-hit timings, filtered
fixtures, compiler-only timings, or a run that excludes terminal codegen/linking.

The web compiler must use the same Rust backend. Browser caching may cache
source inputs and final complete outputs, but must not introduce a persistent
generated-Rust resolver archive or a separate semantic implementation.

## Architectural guard searches

Run these after compiler-architecture changes. Each match must be removed or
explained by the named boundary.

The enforced aggregate check is:

    bash scripts/check-compiler-architecture.sh

Legacy compiler modules or imports should be absent:

    rg -n 'compiler::(ir|typeinfer|passes)|mod (ir|typeinfer|passes)' gors gors-cli www

Generated-Rust resolver/cache concepts should be absent:

    rg -ni 'resolver.?cache|resolved.?module|partial.?declaration|type.?environment.?cache' gors gors-cli www

Semantic syn manipulation should be limited to the terminal emitter and public
output facade:

    rg -n 'syn::|quote!|parse_quote!' gors/src/compiler

The old model must not reappear under a new name:

    rg -ni 'post.?syn|rust.?ast.?pass|ast.?to.?syn|fallback.?lower' gors/src

Mixed or arithmetically rebased semantic provenance must remain deleted:

    rg -n 'SourceSpan|FunctionProvenance|make_function_relative|rebase_function_diagnostic' gors/src/compiler

Also inspect all unsupported diagnostics before claiming support:

    rg -n 'GORS2001|unsupported' gors/src/compiler

## Change rules

- Extend semantic types and HIR before adding syntax emission.
- Add or update MIR validation whenever a new instruction or terminator is
  introduced.
- Add or update Rust IR validation whenever a representation or ownership
  operation is introduced.
- Add a negative diagnostic test and a positive stage or execution test with
  each new language construct.
- Preserve deterministic identity allocation, iteration order, diagnostics,
  representation-lowering results, and generated syntax.
- Add an incremental invalidation assertion and query-cost observation with
  each new language construct; a semantically correct feature with coarse or
  unbounded invalidation is incomplete.
- Keep stage products schema-versioned, canonically fingerprinted, and
  optionally dumpable without checkout-specific paths.
- Do not add phase-local worker pools or unbounded spawning. All work consumes
  the compiler database's single global job and memory budgets.
- Never infer semantics from generated Rust identifiers, doc markers, or syn
  tree shape.
- Never add a stdlib-package-name conditional to codegen.
- Never publish conformance percentages from filtered runs.
- Remove obsolete modules, tests, configuration, and documentation in the same
  change that replaces them.

## 2026 architecture direction

Development order is:

1. Preserve the completed destructive cutover and independent parser contract,
   and install machine-readable stage and performance measurement.
2. Preserve the completed owned per-file snapshot boundary, stable
   workspace/package/file/definition keys, and `SourceRef`/source-table split.
   Add reusable syntax anchors so schema-v2 physical-location-free stage
   fingerprints can become persistent CAS identities, then harden the completed
   production session route through HIR, MIR, and Rust representation queries
   with bounded invalidation and retained-session entry points.
3. Expand exact types, constants, generics, control-flow MIR, places, calls,
   effects, representation facts, both IR verifiers, and runtime ABI through
   fine-grained queries; every feature includes invalidation and cost coverage.
4. Build the canonical package DAG, deterministic global scheduler,
   cancellation, memory eviction, and on-disk semantic CAS while compiling the
   pinned stdlib generically.
5. Run the terminal Rust feasibility gate as soon as representative package
   codegen exists. If rustc plus linking makes the target impossible, replace
   the production artifact path with direct fast codegen from the same verified
   Rust IR; never create a second semantic pipeline.
6. Grow proof-driven Rust representation lowering, differential fuzzing, and
   performance work throughout every phase. Promote cold and warm thresholds
   immediately when earned; after promotion they are mandatory acceptance
   criteria.

Compatibility is measured against Go behavior, not against Rust emitted by the
deleted compiler.
