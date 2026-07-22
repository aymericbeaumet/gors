# Compiler architecture audit and replacement plan

Date: 2026-07-22
Status: hard cutover complete; semantic foundation in progress
Compatibility policy: no compatibility with the deleted backend

## Executive decision

The previous compiler architecture was not a robust base for Go or stdlib
compliance. Correctness was distributed across direct Go AST to syn lowering,
large Rust-syntax transformation passes, resolver-side recompilation and
patching, string-shaped identities, and runtime exceptions. The repository
contained an IR, but production lowering did not use it as the authoritative
semantic representation.

That made each compliance fix expensive and fragile:

- there was no single stage at which Go meaning was complete;
- evaluation order and value semantics could be repaired after Rust syntax had
  already erased the evidence needed to reason about them;
- generated Rust shape became an accidental API between compiler passes;
- the resolver was a second compiler and a cache of generated implementation;
- optimizations and correctness repairs were interleaved;
- stdlib progress rewarded package-specific symptoms rather than language-level
  completeness.

The replacement is deliberately destructive:

    Go AST
      -> semantic analysis
      -> typed HIR
      -> explicit-order Go MIR and verification
      -> representation-neutral Go MIR transforms and reverification
      -> mandatory Rust representation lowering
      -> verified Rust IR
      -> terminal syn emitter
      -> formatting

The direct AST to syn backend, post-syn semantic passes, shadow IR, generated
Rust resolver, partial-package recovery, and generated-Rust resolver caches are
removed. Unsupported programs receive a structured diagnostic. They never
fall back.

## Direct answers

### Is the compiler pipeline robust?

The old one was not. Its output could work, but the architecture could not
localize semantic responsibility or make transformations independently
verifiable. The replacement pipeline can become robust because each boundary
has one canonical product and one owner.

The new pipeline is initially much less feature-complete. That is intentional:
small, explicit, and structurally correct is a better compliance base than a
large backend whose invariants are implicit.

### Should gors use IR?

Yes. Go semantics require an intermediate form that can represent evaluation
order, aliasing, places, exact types, control flow, panic edges, interface
identity, and ownership decisions before Rust syntax is chosen.

The old IR should not be preserved merely because it was called IR. A
source-shaped, lossy mirror that is bypassed by codegen adds complexity without
authority. The replacement uses three purpose-specific forms:

- typed HIR for resolved Go meaning;
- Go MIR for executable order, control flow, effects, places, and storage;
- Rust IR for explicit representation, ABI, ownership, and control-flow
  realization decisions.

If a fact is required after HIR, it must be represented in HIR, Go MIR, or Rust
IR rather than reconstructed from names or syn.

### Should output at each step be simpler?

Yes. Every stage now produces only what its immediate consumers need. There is
no all-purpose TypeEnv, no partial generated package, and no Rust tree used as a
semantic scratchpad.

### Should there be a final optimizer?

No optional final optimizer and no post-syn rewrite. What the old design called
optimization is actually mandatory **Rust representation lowering**. It
converts verified Go MIR into verified Rust IR while choosing explicit copy,
clone, move, borrow, storage, ABI, runtime, and control-flow strategies. Those
decisions require types, effects, places, alias facts, and use-def information
and cannot safely be inferred from emitted Rust syntax.

The first implementation should be conservative rather than clever: copy
actual `Copy` values, clone owned non-`Copy` values, and preserve explicit MIR
control flow. As analyses improve, the same non-optional lowering becomes more
idiomatic and removes proven redundant clones. The emitter receives every
ownership and representation decision already explicit; its job is syntax
selection, not discovery.

### Should there be a runtime helper library?

Yes. A compact, versioned runtime ABI is the right owner for Go language value
representations, concurrency, panic machinery, and host resources.

It is not the right owner for public Go stdlib behavior. The stdlib remains Go
source and must exercise the generic frontend. A helper that implements
fmt.Printf, os.Open, or another public API would hide compiler incompleteness.
An intrinsic that allocates a Go map, manipulates a slice header, schedules a
goroutine, or writes raw bytes to a host descriptor is an appropriate runtime
boundary.

## Canonical stage products

| Stage | Canonical product | Must contain | Must not contain |
| --- | --- | --- | --- |
| Scanner and parser | Go AST | source spelling, syntax, comments, positions | Rust representation or inferred semantics |
| Semantic analysis | semantic index plus typed HIR | stable identities, scopes, exact types and constants, resolved calls, source spans | syn, generated Rust paths, Unknown recovery |
| Go MIR construction | verified explicit-order Go MIR | blocks, terminators, places, temporaries, effects, panic edges, and Go call facts | Go AST references, parser ambiguity, or Rust move/clone choices |
| Go MIR transforms | reverified Go MIR | representation-neutral exact-Go canonicalization and proofs | Rust ownership, ABI, syntax, or unverified graph rewrites |
| Rust representation lowering | verified Rust IR | concrete representations, runtime ABI, storage, copy/clone/move/borrow uses, drop points, and Rust control-flow plan | syn nodes, syntax heuristics, or semantic repair |
| Emitter | syn file | deterministic Rust syntax for verified Rust IR | type inference, representation choice, ownership inference, reachability, or Go evaluation-order recovery |
| Printer | Rust source and maps | formatting, file layout, final mappings | compiler semantics |

Each boundary needs a verifier or a validation contract. Phase-local dumps must
be deterministic so failures can be reduced and compared.

The cutover facade now models its packaged result as an explicit entry unit plus
a deterministic module map, and source mapping is returned as an explicit
`SourceMapPlan`. The printer no longer owns an output cache or a post-syn module
ordering transform. Generated-output manifests are owned by the CLI artifact
cache, not the semantic compiler. These are the intended boundaries: packaging
and mappings are values, deterministic order is established before publication,
and the printer remains terminal formatting rather than another compiler stage.

The current source-map contents are still bootstrap debt: the plan records only
package and function landmarks, then retokenizes formatted Rust and matches
identifier text by occurrence. That cannot survive mangling, duplicate names,
generated helpers, or broad statement coverage. Replace it with exact emitter
anchors paired with Rust-IR provenance and generated byte ranges; keep the
explicit plan ownership, but delete token-text guessing before broad package
support.

## Required semantic invariants

### Identity

Definitions, packages, types, methods, fields, locals, instantiations, and
source files need stable typed IDs. Import paths and user-visible names remain
metadata, not internal identity.

Distinct packages with the same package clause must never collide. Aliases
refer to canonical definitions rather than manufacturing structurally similar
types.

### Types and constants

The type algebra must model Go types exactly enough for identity, assignability,
method sets, generic constraints, and ABI lowering. It must retain:

- defined type versus alias identity;
- pointer and value method sets;
- exact fixed-array lengths;
- generic parameters and substitutions;
- interface type sets and dynamic representation;
- nil-capable versus non-nil values;
- exact untyped constants until contextual conversion.

There must be no Unknown type that silently reaches MIR. Recovery types may
exist for IDE diagnostics, but code generation must reject a poisoned program.

### Evaluation and effects

MIR must make Go sequence points explicit. In particular:

- assignment destinations are prepared in Go order;
- right-hand sides are evaluated before writes where required;
- multi-assignment writes occur left to right;
- call arguments and receivers are evaluated exactly once;
- map, index, pointer, and selector projections retain identity;
- defer arguments are captured at defer time;
- panic and recover edges are represented, not inferred;
- goroutine and closure captures have explicit storage and escape facts.

Effects include at least may-read, may-write, may-call, may-allocate, may-block,
and may-panic. Representation lowering uses effects instead of recognizing
callee names.

### Determinism

Stable inputs must produce byte-identical HIR, Go MIR, Rust IR, diagnostics, and
Rust output regardless of worker count. Ordered data structures or explicit
stable sorting are mandatory at publication boundaries.

## Rust representation lowering design

Rust representation lowering is a mandatory, verified conversion, not an
optional optimization profile. Its bootstrap policy is intentionally boring:

1. Accept only verified Go MIR with valid types, places, dominance, effects,
   panic edges, terminators, and instruction-level provenance.
2. Map each Go type, place, call, and intrinsic to one explicit Rust/runtime
   representation without changing Go evaluation order or control flow.
3. Emit `Copy` only for values whose selected representation is actually
   `Copy`; conservatively clone owned non-`Copy` values and retain explicit
   runtime handles.
4. Preserve the MIR control-flow graph unless a structure-preserving conversion
   is mechanically required by the terminal target.
5. Build syntax-independent Rust IR with explicit ownership, ABI, storage,
   runtime operations, effects, panic behavior, and source provenance.
6. Verify that Rust IR before any terminal emitter can consume it.

The Rust-IR effect summary must conservatively include implementation effects
introduced by the chosen representation as well as preserved Go effects. For
example, the bootstrap `Vec<u8>` string representation makes literal
construction and conservative clone operations allocate even though a Go
source-level string read is not itself an allocation. Those costs must remain
visible to verification and later no-copy decisions.

Representation-neutral Go transformations remain a separately verified MIR
boundary. Exact-Go constant folding, CFG simplification, DCE, and alias-safe
load/store elimination belong in MIR transforms when introduced; they must not
be hidden inside Rust representation lowering. The transform set may initially
be empty, but the production path always verifies the resulting MIR and exposes
no user switch to select a second pipeline.

No-copy is not a text substitution. A Go value assignment may become a Rust
move only if the source has no later observable use, aliases retain Go
behavior, and destruction order remains irrelevant. Otherwise it is a copy,
clone, borrow, or runtime-handle operation chosen from semantic facts.

The current Rust IR implements one deliberately narrow proof-backed refinement:
backwards CFG liveness marks an owned non-`Copy` slot read as
`ProvenLastUseMove` only when the slot is dead on every successor path. Loop
backedges and alternate live branches retain conservative clones. The Rust-IR
verifier recomputes this plan, and terminal emission realizes the move with a
checked slot take. This is useful foundation, not a claim that general
no-copy, borrowing, escape analysis, or storage planning is complete.

Later representation refinements can add:

- sparse conditional constant propagation;
- scalar replacement of non-escaping aggregates;
- bounds-check elimination using range facts;
- interface devirtualization from closed-world reachability;
- representation-aware inlining;
- loop simplification and invariant-code motion;
- allocation sinking and stack promotion;
- profile-guided cost models.

Every refinement needs differential execution coverage and verification. There
is no production flag that bypasses representation lowering; a test-only
conservative policy may be used as an oracle, but it is not another compiler
path or a user-visible mode.

## Runtime ABI

The runtime should expose small typed primitives selected explicitly by Rust IR.
The target contracts are:

| Area | Runtime responsibility |
| --- | --- |
| Strings | arbitrary Go bytes, byte length/index/slice, comparison, host I/O conversion |
| Slices | nil header, length, capacity, shared backing identity, append, bounds checks |
| Maps | nil state, shared identity, key hashing/equality, iteration state, mutation |
| Pointers | nil, alias identity, projections, safe ownership of shared storage |
| Interfaces | dynamic type identity, method table, owned value, equality and assertions |
| Function values | nil state, closure environment, recursive and concurrent calls |
| Channels | queueing, blocking, close, select registration |
| Goroutines | scheduling, wakeup, panic propagation policy |
| Panic/defer/recover | frame-local defer stack, payload, unwind and recover boundary |
| Host resources | raw process, filesystem, clock, entropy, and descriptor operations |

The ABI must be versioned and covered by Rust-level unit tests plus small Go
differential fixtures. Generated code should call runtime primitives directly;
the compiler must not patch emitted stdlib modules afterward.

The semantic compiler must not parse or inject runtime source. Bootstrap Rust
artifact packaging copies the exact versioned runtime module directly, while a
production native artifact path should link a precompiled ABI object or crate.
That removes repeated runtime parsing today and repeated runtime compilation in
the target architecture without moving public stdlib behavior behind helpers.

Representation policy should be centralized. HIR describes Go types and MIR
captures Go places, value uses, control flow, and effects. Mandatory Rust
representation lowering alone chooses concrete storage, ownership operations,
runtime ABI calls, and Rust types. Representation details must not leak back
into name resolution or Go MIR.

## Packages, stdlib, and incremental compilation

The embedded SDK resolver now stops at source discovery. The target package
pipeline is:

1. Build a canonical import graph from user and pinned SDK sources.
2. Assign stable PackageId values from canonical import identities.
3. Parse and index package declarations.
4. Resolve and type-check through demand-driven semantic queries.
5. Build and verify HIR and Go MIR for reachable declarations.
6. Apply representation-neutral MIR transforms and reverify the result.
7. Perform mandatory Rust representation lowering, verify Rust IR, and emit
   each package deterministically.

Reachability belongs on semantic definitions, before Rust emission. It must not
scan syn paths.

Source-only is the right semantic boundary, but the current distribution is not
the right physical one. In the audited native selection, build generation
embedded 1,615 files and about 17.7 MB of raw Go source; the Wasm selection
embedded about 17.1 MB. Those bytes dominate artifact constant/data sections
and one global SDK fingerprint invalidates every package when any SDK file
changes. Replace the monolithic Rust `include_str!` table with a compact
canonical package/file/import/embed-asset/hash index and content-addressed
compressed package shards. Native compilers load verified sidecar shards lazily
under a bounded cache; Wasm fetches immutable reachable shards and caches only
those source inputs. Semantic queries depend on reachable file and asset hashes.

Incremental compilation should cache semantic query results, not generated Rust
resolver modules. Query keys include source content, build tags, target,
toolchain, compiler schema, package identity, and semantic dependencies.
Serialized entries require schema validation and deterministic encoding.

Program parsing now owns immutable, reference-counted per-file snapshots and
creates only temporary AST views borrowing one snapshot. Structured,
collision-checked keys provide stable workspace, package, file, and
package-owned definition identities; moving a named definition between files
in the same package does not change its `DefId`. Node, local, and basic-block
IDs are still dense owner-local indexes, deliberately nonpersistent until an
incremental syntax layer can provide reusable anchors. They must not become
independent query or CAS keys.

Canonical stage fingerprint encoders also exist, but their current HIR, MIR,
and Rust-IR encodings retain source-span provenance and revision-local dense
indexes. They are deterministic validation and telemetry products, not yet
provenance-free semantic CAS identities. Separate semantic content from
diagnostic provenance before cross-process reuse.

The first Salsa-backed `compiler::db` kernel owns explicit source, package, and
build inputs and keeps Salsa handles behind a compiler-owned facade. Its
tracked file projection shares one temporary parse between indexing and
semantic lowering, separates package public API from bodies, builds
function-relative typed HIR, and reaches per-definition verified MIR plus
mandatory normalized and reverified MIR, configured verified Rust IR, and
deterministic package Rust-IR assembly. Production `compile_program` delegates
to `CompilerSession`; convenience calls create a short-lived session, while
long-lived callers can retain the same session across edits. Terminal syn
emission consumes the verified package outside the semantic query graph.
Parsing and semantic projection remain file-granular, though tracked function
fields and function-relative provenance allow unchanged sibling products to
backdate. That is useful incremental reuse, not the final owned incremental
syntax boundary or a cross-process cache.

Build configuration now derives the compiler's runtime ABI identity from the
single numeric ABI version in `gors-runtime` instead of carrying a stale
parallel string. Target, pinned Go version, and runtime ABI are explicit query
inputs. Configuration granularity still needs evidence: a changed target must
not invalidate target-independent syntax and semantic facts.

The target is one explicitly owned, demand-driven red-green query database.
Each query records fine-grained dependency edges automatically; public API and
implementation fingerprints remain separate so a private dependency edit does
not re-type-check importers. Immutable query values are memory-accounted and
evictable. An on-disk content-addressed semantic cache uses deterministic
encoding, atomic publication, checksums, and complete schema, source, SDK,
target, runtime ABI, and dependency keys.

Parallelism operates on ready package-DAG nodes and query boundaries. One
bounded global job budget covers discovery, parsing, semantics, MIR,
Rust representation lowering, emission, external codegen, and linking.
Deterministic work stealing, cancellation, foreground priority, and memory
backpressure replace phase-specific pools. Scheduling must not affect IDs,
diagnostics, stage fingerprints, dumps, or output.

The complete incremental architecture, performance measurement protocol, and
faster-than-Go promotion rules are normative in `COMPILER_PERFORMANCE.md`.

The stdlib is the strongest language-compliance workload, not the first
bootstrap target. Restore it by implementing generic language features in
dependency order. Do not special-case a failing package.

## Current bootstrap frontier and accepted regressions

The initial authoritative backend slice intentionally targets one import-free
source file with:

- primitive `bool`, 64-bit bootstrap `int`, byte-string values, and exact
  constants representable by those types;
- free functions and direct calls;
- parameters, named results, and local bindings;
- scalar unary and binary expressions;
- assignment, return, if, for, break, continue, print, and println.

Only non-panicking executions of that scalar slice are a current behavior
claim. Dynamic integer division or remainder by zero and negative dynamic
shifts have explicit panic effects, but the bootstrap runtime still realizes
them through Rust `panic_any`. A versioned Go panic boundary plus process-level
differential checks for exit status and raw stderr must replace that boundary
before those faulting executions count as compliant.

Everything outside that slice must fail clearly. Immediate backlog includes
multi-file packages, imports, package variables, declared and composite types,
methods, generics, pointers, interfaces, arrays, slices, maps, function values,
closures, range, switch, select, defer, panic/recover, goroutines, channels,
unsafe, and host resources.

Narrow and unsigned integers and floating-point values are deliberately outside
the executable frontier until the type model, conversions, overflow behavior,
and runtime ABI represent their exact Go semantics.

This cutover knowingly regresses most generated-program and stdlib fixtures.
Those fixtures are retained as an ordered migration inventory. Pre-cutover
pass counts and performance measurements are invalid for the new compiler and
must not appear as current evidence.

The leaked `'static` program AST regression has been removed: immutable source
snapshots are reference counted per file and packages never merge their ASTs.
Stable workspace, package, file, and definition identities are also installed.
Reusable syntax anchors and persistent node/local identities remain a
pre-expansion requirement, as does separating semantic fingerprints from
source provenance. Fine-grained query boundaries must be proven by invalidation
tests before broad compliance work.
Incrementality and parallel performance are part of each feature's definition
of done rather than a post-compliance project.

The token-guessing bootstrap source mapper is a third pre-expansion replacement:
instruction provenance must flow through Rust IR to exact emitted anchors rather
than being rediscovered from formatted identifier text.

The monolithic embedded SDK source table is a fourth: replace it before imports
become a hot path so SDK contents are lazy, bounded, target-correct, and keyed at
reachable-file granularity.

The source-only `gors build` command is a fifth product boundary: competitive
artifact certification cannot promote a harness-composed transpile-plus-rustc
shortcut. The default build command must atomically publish the validated
runnable artifact, while an explicit inspection mode may continue to export
generated Rust.

The Rust `panic_any` realization of dynamic arithmetic faults is a sixth:
replace it with the versioned runtime's Go panic/process boundary and compare
observable failure behavior against the pinned Go toolchain before broadening
the executable compliance claim.

## Foundational P0 backlog

Before broad stdlib work can be considered scalable, finish these foundations:

- reusable owned incremental syntax anchors and provenance-free semantic
  fingerprints, while retaining diagnostic/source-map provenance separately;
- retained-session adoption by long-lived Wasm, editor, and build-daemon entry
  points and
  bounded per-definition invalidation beyond the current file-granular
  parse/semantic projection;
- a checksummed cross-process semantic CAS with canonical schemas and atomic
  publication;
- one global scheduler and job budget spanning queries, external codegen, and
  linking, with revision cancellation and memory backpressure;
- the precompiled-runtime experiment and terminal rustc feasibility gate, with
  an aggressive switch to direct codegen from the same Rust IR if it cannot win;
- content-addressed reachable SDK shards instead of the monolithic embedded
  source table; and
- generic language, package, and runtime-ABI breadth sufficient to compile and
  benchmark representative stdlib dependency graphs without special cases.

A failing fixture must be classified as one of:

- parser or scanner defect;
- unsupported semantic feature;
- HIR construction defect;
- MIR semantic or verification defect;
- runtime ABI defect;
- emitter defect;
- Rust toolchain or harness defect.

That classification is the feedback loop the old architecture lacked.

## Deletion and enforcement checklist

The cutover is not complete while any of these remain:

- direct Go AST to syn lowering;
- the old source-shaped IR or TypeEnv inference system;
- compiler semantic thread-local state;
- post-syn coercion, ownership, reachability, or host-patching passes;
- resolver compilation, partial declaration recovery, or syn generation;
- generated Rust and serialized TypeEnv resolver caches;
- browser cache seed archives for generated resolver output;
- compatibility flags, per-node fallback, or dormant legacy modules;
- documentation or tests that present old conformance reports as current.

Guard searches:

    rg -n 'compiler::(ir|typeinfer|passes)|mod (ir|typeinfer|passes)' gors gors-cli www
    rg -ni 'resolver.?cache|partial.?declaration|type.?environment.?cache' gors gors-cli www
    rg -ni 'post.?syn|rust.?ast.?pass|ast.?to.?syn|fallback.?lower' gors/src
    rg -n 'syn::|quote!|parse_quote!' gors/src/compiler

Matches in the final search are permitted only in the terminal emitter and
narrow output facade. Generated syntax must never become an input to semantics.

## Maintainability and module topology

The replacement must not reproduce the deleted 30,000-line compiler under a
new filename. Pipeline ownership is visible in the directory tree: semantic
analysis, HIR, Go MIR construction and verification, Rust representation
lowering, Rust IR verification, terminal emission, runtime ABI, source maps,
and printing are separate modules with narrow direction-of-travel dependencies.
Because there is one backend, there is no redundant `compiler/backend`
namespace. Source-map facilities live under the unambiguous `sourcemap` module;
the old `mapping` name has no compatibility alias.

First-party code is subject to a checked 1,000-physical-line hard limit, with
300 to 700 lines preferred. Large test modules are separate sibling files.
`scripts/check-source-layout.sh`, invoked by `make rust-lint`, rejects oversized
files, inline-test growth in already-large Rust modules, the obsolete backend
directory, and the old source-map directory. Generated, vendored, and fixture
sources are the only routine exclusions.

`scripts/check-compiler-architecture.sh`, also invoked by `make rust-lint`,
rejects legacy compiler imports and directories, resolver codegen/cache terms,
semantic thread-local state, post-syn/fallback lowering, syn dependencies
outside the terminal boundary, and emitter dependencies on HIR, Go MIR, or the
lowering implementation.

The limit is a backstop, not a design technique. Splits follow semantic
responsibility and keep internals private; numbered fragments or arbitrary
line-range shards do not satisfy the architecture.

## Validation strategy

### Per-stage tests

- parser oracle tests remain independent and should not regress;
- semantic tests assert identity, typing, constants, diagnostics, and spans;
- HIR snapshots cover resolved meaning, not formatting;
- MIR snapshots cover order, places, effects, and control flow;
- verifier tests deliberately construct invalid MIR;
- emitter snapshots exercise verified Rust IR forms without reparsing Go;
- runtime tests exercise value representations and panic or concurrency edges.

### Differential tests

Run generated Rust and the pinned Go toolchain from the same source and compare:

- exit status;
- raw stdout and stderr bytes;
- panic behavior where observable;
- deterministic results for deterministic programs.

Add property and metamorphic tests for assignment order, aliases, integer
constants, string bytes, slice capacity, maps, method sets, interfaces,
closures, defer, and concurrency. Fuzz every parser and IR boundary, including
serialized query data when incremental compilation arrives.

### Conformance reporting

Only complete unfiltered runs may publish a baseline. A report row should carry
the first failing stage and diagnostic code so aggregate numbers drive
architecture work instead of concealing it. Filtered runs are local evidence
only.

### Performance reporting

Track compiler-engine and runnable-artifact latency as separate lanes. The
artifact lane includes terminal codegen and linking; a fast HIR-to-Rust-source
measurement is not a fast build. Required scenarios are cold first build,
no-op warm build, leaf body edit, dependency private-body edit, and dependency
public-API edit. Report raw and normalized p50 and p95, paired ratios against
the hermetic Go compiler pinned by `.go-version`, confidence intervals, peak
memory, cache and query events, HIR and MIR size, generated or object size,
external codegen and link time, runtime throughput, allocation count, clone
count, and binary size.

Certification uses a versioned hermetic corpus, at least 50 paired samples per
scenario across three independent sessions, randomized pair order, fixed global
worker budgets, stable hardware classes, calibration and host-health rules, and
behavior validation of every artifact. No competitive number is claimed from
filtered runs or a changed input that reused a stale output.

A scenario is first achieved only when gors p50 and p95 are at most 95% of the
pinned Go values and the upper bootstrap confidence bound for the median paired
ratio is below 1.00. On first achievement its evidence and budgets are promoted
to a versioned manifest. That scenario is then a mandatory non-regression gate;
promotion is not postponed until every language feature is complete. See
`COMPILER_PERFORMANCE.md` for the exact protocol and migration rules.

No scenario is currently promoted. The performance harness and acceptance
schema are active infrastructure, but gors has not earned a faster-than-Go
claim for either cold or warm builds.

## Phased 2026 roadmap

### Phase 0 — hard cutover (complete)

Deliver:

- one production backend and one public facade;
- typed HIR, explicit-order Go MIR, both IR verifiers, mandatory Rust
  representation lowering, and the terminal emitter;
- source-only resolver;
- deletion of every legacy and generated-Rust cache path;
- stable structured diagnostics for unsupported source;
- a small import-free end-to-end golden suite;
- a machine-readable native benchmark evidence schema plus the normative cold
  and warm performance contract.

Exit gate: workspace build and unit checks pass, guard searches find no legacy
path, and unsupported fixtures cannot execute an alternate backend.

### Phase 1 — semantic and query foundation (in progress)

Deliver:

- preserve the completed owned and evictable per-file parse boundary with no
  leaked source revisions or package-wide AST merge;
- preserve stable workspace, package, file, and definition keys, then add
  reusable syntax anchors and stable local/node identities;
- complete canonical packages, scopes, aliases, and declaration semantics;
- complete exact constant evaluation and representability;
- named and composite types, method sets, interfaces, and generics;
- the first demand-driven red-green query database with exact dependency edges,
  public API versus body fingerprints, and deterministic phase dumps;
- machine-readable per-stage timing, fingerprint, invalidation, and memory
  telemetry emitted by that query database;
- poisoned-program rejection before MIR.

Exit gate: semantic fixtures match the Go oracle for the supported declaration
and type-system surface, no Unknown reaches MIR, unrelated edits preserve IDs,
and no-op, body-edit, and API-edit invalidation tests prove the intended query
reuse.

### Phase 2 — executable Go MIR

Deliver:

- complete places and projections;
- calls, multiple results, two-phase assignment, loops, range, and switches;
- closures and function values;
- defer, panic/recover, goroutines, channels, and select;
- escape, alias, liveness, and effect analyses;
- mandatory Go MIR verifier after construction;
- explicit panic effects and unwind edges for every potentially panicking
  operation;
- per-feature query invalidation and cost tests so semantic progress cannot
  silently create package-wide recomputation.

Exit gate: the core language fixture matrix executes equivalently through the
single mandatory Go MIR to Rust IR path, and mutation tests prove both verifiers
reject malformed products.

### Phase 3 — runtime ABI

Deliver:

- canonical string, slice, map, pointer, interface, and function-value models;
- concurrency and panic runtime;
- host-resource primitive layer;
- versioned intrinsic manifest and ABI compatibility checks;
- portable native and Wasm implementations where applicable.

Exit gate: runtime-focused differential and stress suites pass under sanitizers,
threaded execution, and Wasm constraints.

### Phase 4 — packages and generic stdlib

Deliver:

- multi-file package initialization and canonical import graph;
- demand-driven cross-package semantic queries;
- reachability before MIR emission;
- deterministic work stealing under one bounded global job budget;
- cancellation, foreground priority, and memory-budget backpressure;
- a checksummed, schema-versioned content-addressed semantic and codegen cache;
- stdlib compilation from pinned Go source without API replacements.

Exit gate: complete stdlib fixture runs publish a fresh replacement-backend
baseline, every failure names its first compiler stage, worker-count output is
byte-identical, and cold/no-op/leaf/dependency-body/dependency-API benchmark
traces demonstrate fine-grained reuse even before they beat Go.

### Phase 5 — Rust representation refinement and artifact decision

Deliver:

- expand no-copy and semantic-move planning beyond the current CFG-liveness
  last-use move slice;
- scalar replacement, bounds-check elimination, devirtualization, and inlining;
- runtime representation specialization where Go behavior permits;
- output stability, rustc-time, clone-count, and runtime performance budgets;
- one mandatory, deterministic Rust IR lowering policy with readable canonical
  stage dumps;
- an early terminal Rust feasibility gate that compares rustc-plus-link's lower
  bound with the entire pinned Go build;
- direct fast object or machine-code generation from the same verified Rust IR
  if the Rust syntax terminal route cannot satisfy the cold and edited-build
  target.

Exit gate: representation refinements improve performance and generated
idiomaticness without changing differential behavior, no emitter heuristic is
needed for correctness, and the production artifact backend has an
evidence-backed path to the faster-than-Go threshold. A direct backend is a
terminal codegen replacement, never a second semantic pipeline.

### Phase 6 — SOTA feedback loop

Deliver:

- continuous differential fuzzing against the pinned Go toolchain;
- automatic fixture reduction and first-failing-stage classification;
- deterministic replay artifacts containing source, HIR, MIR, and diagnostics;
- per-query incremental invalidation tests;
- profile-guided representation cost-model experiments;
- published conformance and performance dashboards based only on fresh runs;
- immediate promotion of each cold or warm scenario once its p50, p95, and
  reproducibility threshold is earned.

Exit gate: compiler changes can be evaluated by semantic stage, compatibility,
compile cost, generated-code cost, and runtime cost in one reproducible run.
Every promoted performance scenario is a mandatory non-regression acceptance
gate, including both cold first builds and recurring warm edits.

## Definition of success

The compiler is a credible 2026 architecture when:

- every Go semantic decision has exactly one owning stage;
- HIR and MIR are authoritative and independently verifiable;
- syn is terminal;
- unsupported source fails explicitly without fallback;
- the runtime exposes a small language ABI rather than a shadow stdlib;
- package and incremental caches store semantic facts with exact invalidation;
- parse products release obsolete source revisions and semantic identities stay
  stable across unrelated edits;
- deterministic parallel queries operate under bounded global job and memory
  budgets with cancellation and on-disk semantic reuse;
- optimizations are proof-driven and behaviorally differential-tested;
- stdlib compliance rises through generic language support;
- output is deterministic, measurable, and inspectable at every stage;
- the production artifact path beats the pinned Go compiler under the promoted
  cold and warm acceptance contract, or is still explicitly reported as a
  target rather than a claim.

The compatibility target is Go behavior. The deleted compiler is not a target.
