# Native performance certification

This directory implements the executable evidence boundary specified by
`COMPILER_PERFORMANCE.md`. It does not contain a baseline and does not establish
that gors is faster than Go. `acceptance-v1.json` intentionally starts with zero
promoted scenarios.

## Current artifact boundary

The gors side measures exactly one production command:

    gors build --jobs <budget> -o <executable> \
      --timings-json <timings.json> <sources>

That command owns semantic compilation, Rust emission, runtime-provider
selection, rustc, linking, caching, and atomic executable publication. The
harness does not invoke `emit-rust`, read generated Rust or link descriptors,
or run rustc itself. `emit-rust` remains a separate inspection command and is
never part of certification.

The Go side measures the repository-pinned SDK's `go build` through the same
artifact-publication boundary. Program execution is never timed. Both artifacts
are executed afterward, and exit status plus raw stdout and stderr must match the
checked-in oracle exactly.

The driver is recorded as `gors-build-production-v1`. Raw command evidence must
contain exactly one direct `gors build` child and one direct `go build` child
per measured side. Result validation rejects output-directory,
`.gors-link.json`, generated-`main.rs`, `--extern`, and external-rustc drivers.

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
build loads one immutable input revision. A full miss reports
`cli.source_load`, `cli.cache_lookup`, `cli.compile`, `cli.print`,
`cli.file_writes`, and `cli.rustc` in order. A generated-Rust hit with a
terminal miss reports the two lookup phases followed by `cli.rustc`; an exact
executable hit reports only the lookup phases and an all-zero compiler
scheduler. Exactly one compiler and one rustc cache event are required, and
contradictory states fail closed.

The result keeps every randomized pair, child CPU and peak-RSS observations,
artifact sizes, direct-child counts, gors internal phase timings, exact behavior
digests, raw and normalized p50/p95, median absolute deviation, paired ratios,
and a deterministic 10,000-resample
bootstrap confidence interval. Exact byte-I/O counters and semantic-stage
fingerprints and complete process-tree counts are marked unavailable rather than invented.

Result schema v4 records the exact gors and Go executable hashes, their version
evidence, the typed runtime-contract identity reported by gors, the production
driver, corpus, hardware class, and job budget. Internal rustc and runtime
provider details stay owned by the timed `gors build` process instead of being
duplicated as harness configuration. Pre-v4 evidence is unsupported; the gate
does not reinterpret evidence from another schema.

The acceptance gate succeeds without timing while no scenarios are promoted.
After promotion it requires fresh certification evidence for the current clean
commit and exact corpus, hardware, job-budget, compiler binaries, runtime
contract, cache, and driver configuration:

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
