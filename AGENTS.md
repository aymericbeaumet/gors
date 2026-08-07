# AGENTS.md — Guidelines for AI Agents

Keep this file current when an architectural decision, invariant, or
non-obvious operating constraint changes.

## Project

gors is a Go-to-Rust compiler written in Rust. Its production architecture is:

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

Every supported construct travels through this complete pipeline. Constructs
outside the semantic model receive a structured source diagnostic.

## Non-negotiable compiler boundaries

### One backend

- The typed HIR, Go MIR, and Rust IR pipeline is the only production compiler.
- Do not add a direct Go AST to syn path, per-node fallback, compatibility
  adapter, feature flag, or second backend.
- Delete obsolete code instead of leaving dormant alternate modules in the tree.
- A Go construct not represented by the new semantic model must fail with a
  structured source diagnostic.

### General compilation only

- Every generated artifact must be produced by the general pipeline from the
  program's parsed source. Production code must never condition compilation
  behavior on fixture or test identity (names, paths), raw input text
  patterns, or input digests, and must never ship pre-written Rust output for
  specific inputs.
- Name-keyed semantics are limited to what the Go spec mandates: predeclared
  identifiers and builtins, `package main` and `func main`, `init`, the
  `unsafe` pseudo-package, and spec-defined shapes such as the range-over-func
  iterator signature. Decide on resolved identities, never on source text.
- Embedding compiler inputs (Go SDK source and metadata) is sanctioned;
  embedding or replaying outputs keyed to specific inputs is not. Caches may
  only replay artifacts the same pipeline produced earlier under a validated
  fingerprint.

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
  publishes owned semantic products. The parser exposes neither a
  `ParsedProgram` nor a package graph; package-wide ASTs and parser-side program
  models are forbidden.

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
String conversions to byte or rune slices always allocate fresh, non-nil slice
backing. String-to-rune decoding emits U+FFFD and advances one byte for invalid
UTF-8, and named string/rune-slice types retain their exact semantic type while
using the canonical runtime representation.

### Terminal syn emitter

- syn is an output syntax tree, never a semantic IR.
- The emitter renders verified Rust IR as Rust syntax. It must not discover Go
  types, repair evaluation order, perform reachability, infer ownership or
  representation, or recognize stdlib functions by generated Rust shape.
- Terminal emission may be split between `compiler/emit.rs` and focused modules
  under `compiler/emit/`; every such module remains Rust-IR-only and is covered
  by the same architecture checks.
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

Rust-IR operation selection is the canonical runtime boundary. Rust IR carries
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
model, provided capabilities, consumer compatibility identity, and
implementation hash. Immutable `RustRlibProducer` provenance retains the build
host and pre-build target sysroot but never participates in consumer selection;
`RustRlibCompatibility` removes only the `rustc -vV` host and binds the target
model plus canonical recursive target-libdir inventory. Never relabel producer
provenance after a cross-build or target-sysroot trim. `RuntimeType` names
semantic ABI categories, not generated Rust path spellings; runtime crate paths
belong to artifact selection and terminal emission so changing packaging does
not pretend the language contract changed.

The semantic compiler does not parse, inject, patch, or emit runtime source.
Every `CompiledProgram` and printer `GeneratedOutput` carries an unconditional
target-neutral `RuntimeDependency`; generated Rust references the absolute
external crate `::__gors_runtime` and contains no runtime module or source file.
Native distributions build the runtime once as a fixed-recipe rlib, embed its
exact bytes plus immutable producer provenance and a schema-2
`RuntimeArtifactManifest`, verify every identity, and atomically materialize it
in a content-addressed cache. The schema-2 terminal link descriptor carries
both `producer_identity` and `compatibility_identity`; only compatibility enters
artifact selection and the link-plan identity. Terminal consumers must select
one compatible `RuntimeLinkPlan` and pass exactly one
`--extern __gors_runtime=<artifact>` to rustc. Generated-Rust caches retain only
the dependency; provider and executable state are terminal products. There is
no bundled, optional, or fallback runtime path. Native provider production must
resolve `NATIVE_RUNTIME_RUST_TOOLCHAIN` through rustup rather than inheriting
Cargo's arbitrary `RUSTC`; generated-program compilation consumes that same
exact probed rustc path and revalidates its snapshot immediately before a
relink. Target-rustlib inventories reject absolute symlinks and relative
symlinks whose lexical resolution escapes the inventory root.

Native runtime CAS publication must also reject symlinks or other non-regular
nodes at the cache root, artifact-identity directory, lock, temporary, and
destination boundaries. Unix publication is descriptor-relative beneath an
opened real cache root, uses no-follow opens, verifies device/inode continuity,
and atomically renames within the opened artifact directory. Non-Unix
publication must reject reparse-backed redirects and fail closed before
removing anything except a validated corrupt destination. An outside target
must remain untouched even when it contains the exact expected payload.

The CLI cache hard split is complete. `GeneratedRustIdentity` composes the
generated-Rust schema fingerprint, CLI driver schema, source-selection
configuration, and typed runtime contract. Cache admission separately compares
the immutable `InputSnapshot` captured for that invocation. Both exclude
runtime provider bytes and provenance, target compatibility, the Rust toolchain
and linker, `RustcAction`, output profile, target CPU, and target features. The
generated manifest owns its identity and input snapshot, generated files and
their hashes, source-map presentation state, and `RuntimeDependency`; it owns
no provider or executable state.

The independently replaceable terminal manifest is bound back to that
generated identity and dependency. It owns the selected provider descriptor
and artifact path, an immutable `TerminalToolchain`, plus per-profile
executable records. The toolchain content-admits absolute rustc and linker
executables, records the target, ordered deterministic environment, and the
selected target-libdir metadata identity, and on Apple records the canonical
SDK selection plus its settings identity. A terminal miss revalidates that
target-libdir snapshot immediately before rustc execution. Compatibility
inventory schema v2 hashes every target-libdir file, including hash-suffixed
rustlibs; filenames and sizes are never content surrogates. The toolchain's
semantic identity projects rustc/linker path, content, target, environment, and
platform contract; revision and target-libdir snapshot facts are execution
admission guards, while the runtime compatibility identity owns target-libdir
content. Every executable is admitted by the exact `RustcActionIdentity`: that
semantic toolchain projection, working and scratch directories, ordered
arguments, every generated-Rust
filename and content hash, runtime artifact path and implementation hash,
link-plan and compatibility identities, output profile, target, portable CPU
and feature policy, and publication paths. Rustc receives an absolute linker
and runs after `env_clear()`. Debug and release therefore reuse the same
generated Rust while producing distinct terminal actions and executables.
Corrupt or stale terminal state is a terminal miss and must not poison a valid
generated-Rust entry.

The terminal action is still not fully hermetic on hosted targets: the admitted
rustc launcher can load host driver/codegen libraries outside the descriptor,
and a selected linker can consume transitive helper binaries and system
libraries whose bytes are not yet enumerated by `TerminalToolchain`; the Apple
SDK record currently owns SDK selection/settings rather than every linkable
stub. The target-libdir guard is a cheap revision recheck of that exact
compatibility inventory. Tool verification and process spawn are also still
path-separated, so a hostile path replacement can race the admitted revision.
Treat the host
compiler-driver closure, platform link closures, and handle-to-exec boundary as
P0 cache and performance debt. No performance scenario may be promoted until
both closures are content-addressed and execution consumes the admitted handles
or immutable CAS paths. Exact executable hits reconstruct recorded identities
without touching the historical toolchain; a terminal miss performs live
content admission.

An exact warm executable hit must be admitted from the current source snapshot,
generated and terminal manifests, recorded generated-product hashes, and the
current executable hash and executable mode before any generated-Rust read,
runtime provider
materialization, runtime-artifact read, rustup lookup, rustc probe or stat,
target-rustlib inventory, or link-plan reselection. It executes directly. A
generated-Rust hit that still needs a refreshed link descriptor or executable
may resolve the terminal provider only after that cheaper admission fails.

The CLI product model has three precise surfaces:
`gors build` always requests the portable production profile and atomically
publishes one runnable executable, `gors emit-rust -o <directory>` is the only
source-export command, and `gors run` accepts program arguments only after a
literal `--`. Build and `run --release` share the same internal production
`ExecutableProduct`; the public output path is presentation state and cannot
force a relink. Production rustc uses `opt-level=2`, LTO off, debug info zero,
`target-cpu=generic`, and an empty requested target-feature set. Generated-Rust
commands must not resolve rustup, materialize the runtime, or publish a terminal
link descriptor.

Internal Unix executables are admitted through no-follow descriptors, hashed
against stable `fstat` snapshots, normalized to mode `0o755`, atomically renamed
with inode continuity, and synced with their containing directory before the
terminal manifest commits. The terminal manifest records the admitted mode;
removing execute permission is a terminal cache miss even when bytes are
unchanged.

Unix public executable publication opens the destination parent once and uses
descriptor-relative no-follow lock, temporary, admission, and rename
operations. An exact size, mode, and SHA-256 match is a zero-write warm hit.
Publication syncs the new file and parent directory and never pre-deletes the
destination; non-Unix hosts fail closed until an equally atomic implementation
exists.

The browser compiler remains target-neutral. It transports only the runtime
dependency schema, contract identity, and canonical operation IDs; the V86
runner owns selection of its separately precompiled target artifact. The guest
uses the Rust package version captured by its digest-bound Alpine resolution,
not the native workspace host toolchain. If its rustlib tree is trimmed, retain
the original producer identity and recompute only consumer compatibility before
the final external-link smoke. Never make the Wasm compiler select or embed a
runnable-program runtime provider. V86 publication is manifest-last: admission
must verify the provider hash, rootfs index hash, exact referenced blob-set
identity and count, and every content-addressed blob before reusing an image.

The reduced V86 provider-helper workspace owns its own
`www/v86/Cargo.v86.toml` and `www/v86/Cargo.v86.lock`. Its temporary Docker
context renames those files to Cargo's standard names and must consume that
exact lock; the full compiler workspace lock is not a valid locked resolution
for the reduced helper workspace. Content-addressed V86 boot assets are copied
as finalized Webpack assets so production minimizers cannot rewrite them before
the manifest-last emitted-byte verification.

The image build runs `gors-warmup --smoke` after the final runtime publication
to prove an external link and execution. Guest startup calls `gors-warmup`
without that flag and must publish `GORS_BOOT_READY` as soon as Linux and the
serial shell are operational; do not put rustc work or recursive runtime
inventory in the browser boot-ready path. User compilation owns its explicit
runtime-provider verification and rustc work after the VM becomes ready.

V86 guest execution is strict single-flight: an overlapping compile or run is
rejected with a typed busy error rather than replacing the active job. Every
admitted flight owns a fresh 128-bit nonce, an abort-aware deadline, one exact
line-framed marker waiter, and nonce-scoped status/output paths. Missing or
malformed markers and exit-status files are protocol failures, never successful
exit zero. Preserve guest stdout and stderr exactly; presentation trimming does
not belong in the runner. Once guest filesystem or serial work has started, any
cancellation, timeout, or protocol failure permanently poisons that emulator
generation. It must reject all later work, notify the runner, and be stopped and
destroyed through the pinned asynchronous V86 API before a fresh generation is
created. Cancellation and disposal must reject the active flight exactly once,
and serial bytes from a stale emulator or nonce must never settle a later
flight. Rootfs download-error monitoring remains installed for the full
emulator lifetime so lazy 9p failures invalidate a ready generation promptly.

`www/v86/boot-contract.json` is the checked-in source of truth for V86 machine
settings and the guest command/marker protocol. Webpack emits one strictly
validated boot manifest whose full SHA-256 identity binds that contract, exact
V86/Wasm and BIOS content, and the verified rootfs index/blob-set evidence;
an empty blob set is invalid, and there are no truncated asset identities or
unhashed filename fallbacks. Browser manifest reads reject oversized declared
lengths before consuming the body and abort a streaming download as soon as its
actual body exceeds 128 KiB. The fixed commit manifest is fetched with
`cache: no-store`, and Webpack rehashes the exact final emitted buffers for
every V86/BIOS asset, the rootfs index, and every hash-named blob before it
emits that manifest. The browser still trusts the same-origin deployment and
CDN to serve bytes matching those immutable full-hash names; it does not
independently rehash the V86, BIOS, or lazy rootfs responses. Do not describe
that deployment trust boundary as end-to-end browser content admission.
Browser saved state uses only IndexedDB schema and record schema 2, with the
exact boot identity, bounded byte length, full state checksum, and payload.
Outdated, corrupt, oversized, or identity-mismatched records are deleted and
treated as cold misses, as are all IndexedDB failures. A valid warm restore
omits the rootfs index and lets V86 restore its serialized 9p state. Cold boot
supplies exactly one content-addressed rootfs index. Acquisition, warm restore,
cleanup, and cold boot each own an abort-aware deadline. A failed restore tears
down the emulator, deletes the state, and receives exactly one fresh-deadline
cold retry; download and cold-boot failures never enter a retry loop.

Until the Wasm compiler exposes cooperative cancellation, cancelling or
superseding an active browser compilation terminates that worker generation.
Only the current worker identity and request ID may report progress, resolve a
caller, or publish retained-session/cache state; a terminated or stale worker
must be observationally inert. Loader failures are retryable and a watchdog
termination must leave the next compilation able to start from a fresh worker.

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
- Generated-Rust cache identity and terminal `RustcActionIdentity` are separate
  domains. Build profile and all terminal toolchain, target, provider, CPU, and
  feature facts may invalidate an executable but must not invalidate identical
  generated Rust.
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
once and projects that temporary AST exactly once into owned, trivia-insensitive
structural function and constant syntax plus canonical header/body token
streams. Semantic queries never retain or revisit the AST or raw source.
Revision-local physical ranges live only in `FunctionLayout`, `ConstantLayout`,
`VariableLayout`, and `TypeDeclarationLayout`; successful semantic lowering
publishes a physical-free source plan which the presentation query joins to the
current layout. Package-function
`SyntaxAnchor`s use the package-level name, while method anchors use the named
receiver and method name. Neither form contains an offset, traversal ordinal,
or token index. Repeated `init` declarations are not independently nameable Go
definitions: file projection retains each body as an ordered fragment under
one stable package-owned `init` identity, and package-variable initialization
composes those fragments in package filename and source declaration order.
Identical initializer bodies therefore require no unstable declaration
ordinal. Generic receiver and type syntax retains every explicit type argument
rather than collapsing multi-argument instantiations back into parser-only
syntax. Demand queries independently type function headers,
package constants, package variables, and function bodies before reaching
function-relative typed HIR, per-definition
verified MIR, mandatory normalized/reverified MIR, configured verified Rust IR,
and deterministic package Rust-IR assembly. Function verification reads only
its own and direct callees' signatures; unrelated declaration or signature
edits must leave a leaf function's HIR, MIR, normalized MIR, and Rust IR green.
Lexical reference collection respects parameter, named-result, declaration,
short-declaration, and nested control-flow scopes, so shadowed names do not
create false package dependencies. Package-constant dependencies resolve by
stable name across the complete package, permit forward and cross-file
references, and reject cycles with a deterministic path. Package variables
have distinct stable declarations and typed initializer queries; immutable
reads materialize exact constant or zero initial values, while mutation and
address-taking remain rejected until global storage lowering exists. Exported
constant and variable type/value semantics participate in the package
public-API fingerprint.
Function-local constant declarations are scoped semantic bindings. Their exact
values, explicit types, repeated specification expressions, and `iota` values
are resolved before HIR expression lowering; they never become storage places
or MIR locals, and assignment to one is a source diagnostic.
Function-local variable declarations may consume one multi-valued call,
comma-ok map lookup, comma-ok interface assertion, or comma-ok channel receive.
The RHS is lowered once before any name in that `ValueSpec` enters scope, and
typed component coercions remain explicit in HIR `LetTuple` lowering.
One sole multi-valued function call used as another call's argument remains an
explicit HIR binding. MIR evaluates and freezes any method receiver first,
executes the source call exactly once, applies each recorded assignment
coercion in result order, and packs only the variadic remainder. A nonempty
`...int` remainder receives a fresh slice whose length and capacity equal its
bound result count; an empty remainder is nil.
Multi-valued package-variable initialization remains rejected until the
package initializer model represents tuple-producing execution.
Every assignment to existing storage, including compound assignment,
increment/decrement, and range `=`, carries a typed, `SourceRef`-provenanced
HIR `AssignTarget`. MIR uses one prepare/read/write lifecycle: all dynamic
target operands are prepared exactly once in source order, compound targets
are read before their RHS, and writes occur left to right. Range targets are
prepared anew on every admitted iteration, and range `=` retains and applies
one explicit `ValueCoercion` per generated iteration value before its write.
Nested local struct paths rebuild the value from leaf to root with explicit
`StructSet` operations; the emitter must never reconstruct or repair an
assignment path.
Address-taking of a non-nested integer local is explicit HIR intent. MIR plans
one shared pointer-backed storage cell for each such local, initializes
parameters and declarations at their Go sequence points, and routes subsequent
direct and indirect reads and writes through that cell. Nested control-flow and
function-literal address-taking remain diagnosed until their lifetime and
per-iteration storage semantics are represented.
Production program
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
remains outside the semantic queries. Parsing and structural projection are
still file-granular, so a whitespace or comment edit executes `FileProjection`;
it may replace layouts and definition source tables while executing zero typed
signature, typed HIR, MIR, or Rust-IR queries. This is not incremental parsing.
Compact per-definition `SourceRef` values and separate definition source tables
allow unchanged semantic stage products to backdate. Query and scheduler
counters are not a memory budget, complete cancellation protocol, global
scheduler, or persistent CAS; do not claim those target properties from the
current kernel.

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

Semantic provenance is explicit throughout the pipeline. HIR, Go MIR, and Rust
IR retain only compact, owner-scoped `SourceRef` values. A separately tracked,
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
`FunctionProvenance`, a `compiler::db::provenance` side channel, and arithmetic
function-relative rebasing are forbidden. Keep these domains separate and keep
parsing behind the query-owned file projection.

The file projection owns decoded direct-import occurrences, structured invalid
imports, and owned source comments from its one ephemeral parse. Those products
contain `FileId` and content-relative provenance, never checkout paths. Import
paths are sorted and deduplicated only at the package-analysis boundary;
occurrence products preserve source order and duplicates. Every valid occurrence
also retains its exact `default`, named, blank, or dot binding and separate
compiler-owned physical ranges for the binding token and import literal; never
reconstruct aliases from an import-path basename.
Owned expression syntax preserves selectors as recursive `base.member` nodes
with independent source identities for the selector, base, and member. The
member is not an unqualified lexical reference. Name resolution must consume
that structure rather than flattening a selector into a string path or
reconstructing it from source text.

`compiler::package_dag` is a pure boundary over already-resolved package IDs.
It performs no discovery, resolution, or input mutation. A command-line entry
has a real `PackageKey::CommandLine` node and no synthetic import path; every
dependency target must have an exact canonical `PackageKey::ImportPath`.
Canonical topology layers place dependencies before importers and sort ready
packages by stable `PackageId`. Illegal strongly connected components select
one deterministic closed path apiece with exact import-occurrence evidence.

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
`PackageKey` values encoded directly by the collision-checked semantic
interner; do not flatten them into caller-constructed strings.
Cross-package semantic references use `QualifiedDefId { package, definition }`.
Keep `DefId` for package-owned declarations and provenance owners, but never
infer a referenced definition's package from its opaque digest or carry package
ownership in an adjacent string side channel.
`WorkspaceKey::Module` and `PackageKey::ImportPath` each own a validated
`CanonicalImportPath`, never an unchecked string. `ProgramInput` owns one
authoritative entry manifest plus an object-safe immutable
`PackageManifestCatalog`. Session admission installs the entry, queries its
owned direct-import facts, and materializes dependencies in deterministic
sorted waves until the reachable closure is complete. Unrelated packages must
never become Salsa `SourceInput` or `PackageInput` values, retain database
bytes, or advance the query revision.

Session input installation is one delta transaction. The compiler journals only
changed, inserted, and removed source and file-scoped resolved-import inputs,
includes both kinds of stale-input removal in the same rollback boundary, and
rolls mutations back in reverse application order. Every active source owns a
resolved-import product, including an explicit empty product. Default bindings
store the target package's actual parsed package-clause name; named, blank, and
dot bindings remain distinct. Exact no-op installs must not call a Salsa setter
or synthesize rollback snapshots.
Admission must journal only changed reachable files; an
all-`program.packages()` installation loop or O(all-active-files) rollback
snapshot is forbidden. Missing packages, catalog failures, and package cycles
abort the same transaction and preserve both the preceding database revision
and its published package DAG. A successful admission publishes the pure
`compiler::package_dag` result with dependency-first ready layers for later
parallel semantic work.

The raw filesystem workspace loader requires an explicit caller-owned
`WorkspaceKey`; it must never synthesize an ad-hoc identity from a checkout
path. The production `load_program_files_auto` boundary may replace the CLI's
fallback ad-hoc key only with the validated module identity read from the
nearest containing `go.mod`. Directory entry packages receive their canonical
module import path; explicit file lists remain `PackageKey::CommandLine` while
sharing the local-module dependency catalog.
Every `PackageInputManifest` also owns its canonical Go language version.
Local-module manifests use the `go` directive, or Go's fixed `go1.16` default
when it is absent; synthetic and embedded-SDK manifests default to the pinned
compiler version. File projection records exact language-gated token evidence
and any `//go:build go1.N` file version during its one parser pass. A separate
tracked compatibility query combines those facts with manifest metadata, so a
go.mod-only version change cannot reparse source or invalidate unchanged HIR,
MIR, or Rust IR.

The raw workspace loader deliberately performs no recursive module or import
discovery. Module resolution is query-owned manifest expansion from decoded
direct-import facts and resolver source metadata; parser recursion and a second
pre-query parse are forbidden.

Canonical decoded package identities live in the frontend-neutral
`gors::import_path::CanonicalImportPath`; parser import-literal decoding must
consume that type's shared decoded-path validator instead of maintaining a
parser-local validator. `workspace::local_module::LocalModuleCatalog` is the
filesystem-only first boundary for local module discovery. Opening it
canonicalizes one explicit module root and reads only its strict `module` and
optional `go` directives. It materializes and memoizes one explicitly requested
local package at a time, verifies lexical and canonical containment, reads only
immediate eligible non-test Go files in canonical order, and never parses
source or follows imports. `LocalModuleManifestCatalog` is its compiler adapter: it lazily
converts and memoizes immutable `PackageInputManifest` values, reports external
imports as unowned, and preserves concrete local loading failures through the
catalog error chain. `ProgramInput` owns that adapter and the session admits its
reachable closure from query-owned direct-import occurrences without another
parse. The production auto-loader composes the embedded Go SDK source catalog
ahead of the local-module catalog, so standard-library identity wins while
unknown canonical paths fall through to local module ownership; ad-hoc
filesystem workspaces still own an embedded-SDK catalog when no local module
exists. Each SDK request materializes and memoizes only that immutable
package's raw build-selected Go files and never parses or follows imports.
Module discovery is `go.mod` based; GORSPATH is unsupported.
Until the compiler publishes a closed reachable-input snapshot for the CLI
manifest,
module-catalog builds must conservatively bypass cross-invocation
generated-artifact cache admission;
an entry-only snapshot is not sufficient evidence for a cache hit.

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

## Executable coverage frontier

The production pipeline currently executes this focused, fully verified subset:

- source packages and resolved Go-source imports within the executable type
  subset;
- `bool`, every scalar signed and unsigned integer type in the 64-bit Go data
  model, `float64`, `complex128`, byte-string values, named numeric types,
  aliases, `[]int`, `[]byte`, `[]string` and named slices whose element's
  underlying type is `string`, `map[string]int`, `map[int]string`, `*int`,
  `chan int`, scalar fixed arrays, and integer-field structs;
- exact typed and untyped constants, including `iota` and complex constants;
- free functions, value methods and method values, direct non-escaping
  closures, parameters, multiple and named results, locals, immutable package
  variable reads, defer, panic, and recover;
- explicit-order assignments, calls, slice and map built-ins, expression
  switches, labels, goto, range over slices and maps, and structured loops;
- print and println intrinsics through the versioned runtime ABI.

String-element slices use one typed `GoSlice<GoString>` runtime
representation. Nil, make, len, cap, index, range, set, append, copy, clear,
nil testing, and interface box/unbox are explicit HIR/MIR operations and stable
runtime ABI operations; the MIR verifier preserves the source slice and element
types even when distinct named slice types share an identical named string
element type. Runtime copying is overlap-safe and allocation-free so its
representation-effect summary remains exact.

Concrete map range lowering snapshots the initial candidate keys exactly once,
then checks live membership immediately before each body entry and reads the
live value only after that check. Deleting an unreached candidate suppresses
its iteration, updating an unreached value is observable when reached, and new
keys are not added to the active range. Both `map[string]int` and
`map[int]string` use typed ABI operations; the legacy indexed-key operation
remains only for compatibility with older generated artifacts.

Supported control flow and typed panic/recover behavior are executable today.
Every deferred action is an explicit ordered Go-MIR and Rust-IR region. It
clears its registration flag before invocation, owns an independent unwind
boundary and newest-panic replacement transition, and continues at the next
earlier action; terminal emission mechanically renders that verified plan.
Dynamic division or remainder by zero and negative dynamic signed shift counts
reach the versioned integer runtime boundary and preserve Go's ordered
evaluation and panic semantics. Unsigned shift counts are statically
nonnegative and therefore carry no negative-shift panic effect.

Interface payloads for `*int` and pointers to defined types whose underlying
type is Go `int` use the canonical `GoPointerI64` representation. Boxing and
unboxing are explicit versioned ABI operations: typed nil pointers remain
non-nil interfaces, extraction clones the pointer header and preserves pointee
alias identity, interface equality compares pointee identity, and exact dynamic
type identities keep defined pointer types distinct while aliases retain their
target identity.

The remaining frontier receives precise source diagnostics until its semantics
are represented in HIR and MIR:

- broader Go stdlib coverage and package initialization;
- mutable package variables, broader aggregate representations, struct
  pointers, pointer-receiver methods, broader interfaces, and generics;
- escaping function values and type switches;
- string, integer, channel, and iterator-function ranges; select, goroutines,
  and channels;
- unsafe and host-resource integration.

Every predeclared signed and unsigned scalar integer type has an exact
ABI-owned `IntegerKind` over one canonical `i64` carrier. Mandatory Rust
representation lowering selects width-specific wrapping arithmetic, negation,
bit operations, comparisons, min/max, and conversions; the verifier rejects
noncanonical carriers and mismatched kinds before emission. Signed and unsigned
printing select distinct runtime operations, so high-bit `uint64` values retain
their Go rendering. Dynamic division and remainder select exact-width signed or
unsigned runtime members for all eight scalar integer kinds, including Go's
`MIN / -1` and `MIN % -1` rules. Shifts preserve the exact left-hand kind while
retaining an independently typed integer count; signed and unsigned count
members keep their distinct panic contracts. Broader integer slice, map,
pointer, and channel families remain outside this scalar checkpoint.

Typed floating-point and complex constants are quantized from the exact
rational constant algebra at every declaration, conversion, and typed
operation using the destination IEEE format's round-to-nearest, ties-to-even
rule. HIR and MIR retain that canonical rounded value, the MIR verifier rejects
unquantized typed constants, and representation lowering consumes the exact
verified bits rather than making the first rounding decision or routing a
`float32` constant through host `float64`.
Untyped floating-point and complex components are canonical reduced arbitrary-
precision rationals with a positive denominator, including values such as
`22/7` that have no finite decimal spelling. Semantic operations, comparisons,
min/max, HIR/MIR products, and their fingerprints retain the structural
numerator and denominator; IEEE rounding occurs only at a concrete float or
complex typing boundary.

Executable channels use one direction-neutral shared handle representation per
element representation while retaining exact send/receive direction in HIR and
Go MIR. The canonical runtime ABI currently provides complete operation
families for channels carrying `int`, `string`, or another `chan int` value;
direction-only assignment and explicit conversion are representation-preserving
and must never become runtime calls. Other channel element representations stay
diagnosed until they receive an equally complete typed ABI family.

The existing Go-spec, stdlib, repository, and arbitrary-program fixtures are a
prioritized coverage map and differential oracle. Only complete, unfiltered
runs may establish a conformance baseline.

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
        artifact/           embedded native runtime provider and CAS publication
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
architecture contract. `COMPILER_AUDIT.md` records the broader architecture
and roadmap.

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

The broad generated-program suites expose the remaining language and library
coverage. A red unsupported fixture is actionable input for a generic compiler
or runtime improvement.

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

Alternate compiler modules or imports should be absent:

    rg -n 'compiler::(ir|typeinfer|passes)|mod (ir|typeinfer|passes)' gors gors-cli www

Generated-Rust resolver/cache concepts should be absent:

    rg -ni 'resolver.?cache|resolved.?module|partial.?declaration|type.?environment.?cache' gors gors-cli www

Production sources must not reference the differential fixture corpus:

    rg -n 'tests/fixtures|go_spec/|go_stdlib/|go_programs/|go_repositories/' gors/src gors-cli/src gors-runtime/src gors-runtime-abi/src www/wasm www/src --glob '!tests.rs'

Mixed cache/action identities and host-native terminal codegen should be
absent from production compiler, CLI, and performance paths:

    rg -n 'GORS_CLI_ABI_FINGERPRINT|CacheRequest|RustcArgs|target-cpu=native' gors/src gors-cli/src perf/perf_harness

Semantic syn manipulation should be limited to the terminal emitter and public
output facade:

    rg -n 'syn::|quote!|parse_quote!' gors/src/compiler

Semantic work must not move beyond verified Rust IR:

    rg -ni 'post.?syn|rust.?ast.?pass|ast.?to.?syn|fallback.?lower' gors/src

Mixed or arithmetically rebased semantic provenance is forbidden:

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
- Never condition compilation on fixture names, test paths, input text
  patterns, or input digests; fixtures gain support only through general
  pipeline improvements.
- Never publish conformance percentages from filtered runs.
- Remove obsolete modules, tests, configuration, and documentation in the same
  change that replaces them.

## 2026 architecture direction

Development order is:

1. Preserve the single compiler pipeline and independent parser contract, and
   install machine-readable stage and performance measurement.
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

Compatibility is measured exclusively against Go behavior.
