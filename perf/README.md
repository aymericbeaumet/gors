# Native performance certification

This directory implements the executable evidence boundary specified by
`COMPILER_PERFORMANCE.md`. It does not contain a baseline and does not establish
that gors is faster than Go. `acceptance-v1.json` intentionally starts with zero
promoted scenarios.

## Current artifact boundary

The current CLI has no artifact-only command: `gors build` emits Rust source
plus a schema-1 `.gors-link.json` transport containing a schema-2 link
descriptor for the exact precompiled runtime provider, while `gors run`
compiles and executes it. The harness therefore owns
a temporary bootstrap artifact driver whose measured interval is:

    gors build --release --jobs <budget>
      -> strict descriptor, target, payload, artifact, and link-plan validation
      -> repository-pinned rustc with the CLI release flags
         and exactly one --extern __gors_runtime=<validated rlib>
      -> link
      -> atomic executable publication

The Go side measures the repository-pinned SDK's `go build` through the same
artifact-publication boundary. Program execution is never timed. Both artifacts
are executed afterward, and exit status plus raw stdout and stderr must match the
checked-in oracle exactly.

Descriptor reading, runtime-payload hashing, argument expansion, and linking are
all inside the measured interval. Missing, malformed, stale, duplicate-key, or
unknown-schema descriptors fail closed; there is no runtime-source fallback.
The expanded rustc argv and every validated contract, operation, target,
host-neutral rustc release, recursive target-libdir ABI, producer provenance,
consumer compatibility, implementation, artifact, and link-plan identities
remain in raw sample evidence. Producer provenance is audit-only and never a
consumer link-request input.

This driver is recorded as `bootstrap-generated-rust-v3` and is explicitly
non-production. Its results cannot promote a budget. A real default
user-facing artifact command must replace it before performance can become an
acceptance claim.

## Corpus and scenarios

`corpus/v1` is content-digested and network-free. The bootstrap workload uses
only the authoritative single-file language frontier. Every workload measures:

- cold build with isolated empty compiler and Go caches;
- no-op build from a new process after an untimed successful build;
- leaf implementation edit after an untimed base build, with a required changed
  output sentinel.

Dependency-body and dependency-API edits are machine-readably marked unsupported
until the authoritative compiler supports imports. They must be added rather
than silently omitted once that frontier moves.

## Commands

Deterministic schema, statistics, manifest, and gate tests are safe for ordinary
CI and perform no benchmark measurement:

    make perf-test

A local trend baseline defaults to 50 paired samples per scenario across three
sessions. Baseline mode is never promotable, even on a dedicated host:

    make perf-baseline

An explicit small end-to-end harness exercise must carry the smoke label. Its
result permanently records that it cannot promote:

    make perf-baseline PERF_SAMPLES=1 PERF_SESSIONS=1 PERF_ARGS=--smoke

Certification is opt-in and refuses an unnamed or non-dedicated worker. The
default is 50 pairs per scenario split across three independent sessions:

    make perf-certify \
      PERF_HARDWARE_CLASS=linux-perf-v1 \
      PERF_JOBS=16 \
      PERF_ARGS=--dedicated

`PERF_JOBS` is passed to both the gors semantic compiler host and Go's
`-p`/`GOMAXPROCS` boundary. The harness rejects missing, malformed, or
out-of-budget gors scheduler evidence. It accepts only timing schema v5: every
build loads one immutable input revision, so a compiler-cache miss must report
`cli.source_load`, `cli.cache_lookup`, `cli.compile`, `cli.print`, and
`cli.file_writes` in order; a proven cache hit must report `cli.source_load`
then `cli.cache_lookup` and an all-zero scheduler. Missing or contradictory
cache events fail closed. The separately launched descriptor-validation and
rustc/link step is not yet admitted through that host, so end-to-end job-budget
symmetry remains a promotion blocker rather than a property of the current
harness.

The result keeps every randomized pair, child CPU and peak-RSS observations,
artifact sizes, direct-child counts, gors internal phase timings, exact behavior
digests, raw and normalized p50/p95, median absolute deviation, paired ratios,
and a deterministic 10,000-resample
bootstrap confidence interval. Exact byte-I/O counters and semantic-stage
fingerprints and complete process-tree counts are marked unavailable rather than invented.

Result schema v3 requires the exact lowercase SHA-256 identities of the typed
target-neutral runtime contract, schema-2 host-neutral rustc compatibility
identity, canonical rustc release and recursive target-libdir record digests,
runtime payload, schema-2 artifact, and validated link plan. It also retains
the exact expanded rustc argv and includes the descriptor-validation protocol
in the versioned configuration fingerprint. Pre-v3 evidence is unsupported;
the gate does not reinterpret or migrate legacy evidence.
Any runtime ABI operation catalog, capability set, artifact recipe, or producer
identity change requires a matching driver/schema revision; stale harness
assumptions fail closed instead of silently producing comparable evidence.

The acceptance gate succeeds without timing while no scenarios are promoted.
After promotion it requires fresh certification evidence for the current clean
commit and exact corpus, hardware, job-budget, runtime/toolchain, linker, cache,
and driver configuration:

    make perf-gate PERF_RESULT=target/perf/certification.json

It also revalidates the immutable baseline evidence digest, the 3% maximum
promotion headroom, the pinned-Go ceiling, behavior, p50/p95 budgets, and the
paired confidence bound. Normalization factors, raw and normalized summaries,
confidence bounds, host health, and achievement are recomputed from the raw
session timings before any stored aggregate is trusted. Missing, smoke, stale,
dirty, undersampled, or configuration-mismatched evidence fails closed.

## Host-health normalization

Each session runs the versioned `python-sha256-fsync-v1` CPU/filesystem
calibration before and after its pairs. The geometric center normalizes samples
within the run; raw durations remain present. More than 10% before/after drift
marks the session unhealthy and blocks achievement. This is a first executable
calibration boundary, not a substitute for defining and provisioning the named
dedicated hardware classes required for promotion.
