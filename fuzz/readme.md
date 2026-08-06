# Fuzzing gors

The fuzz crate has two complementary feedback loops:

- Stable, deterministic property and corpus-replay tests for pull requests.
- Coverage-guided `cargo-fuzz` runs for local, scheduled, and manually
  dispatched deeper testing.

The compiler target exercises the complete typed compiler pipeline through
verified Rust IR and terminal Rust source printing. It does not contain Rust
replacements for Go standard-library packages.

## Fast stable checks

From the repository root:

```bash
make fuzz-test

# Or choose explicit deterministic case budgets.
PROPTEST_RNG_SEED=1 GORS_FUZZ_CASES=128 GORS_FUZZ_EDGE_CASES=32 \
  cargo test --profile ci --package fuzz --tests
```

The environment variables only control the proptest seed and case counts.
Checked-in corpus inputs are always replayed.

## Coverage-guided fuzzing

Install cargo-fuzz once:

```bash
rustup toolchain install nightly-2026-07-01
cargo install cargo-fuzz --version 0.13.2 --locked
```

The repository Makefile exposes unbounded interactive targets:

```bash
make fuzz-scanner
make fuzz-parser
make fuzz-roundtrip
make fuzz-compiler
```

For an explicitly bounded local run, use the helper:

```bash
./fuzz/scripts/fuzz.sh scanner -t 300
./fuzz/scripts/fuzz.sh parser -j 4 -t 1800
./fuzz/scripts/fuzz.sh compiler -t 3600
```

The helper and scheduled workflow keep optimized fuzzing code but disable fat
LTO and use 16 codegen units. This makes instrumented builds much faster, so
more of a bounded job is spent exploring inputs. Both default to the pinned
`nightly-2026-07-01` toolchain; set `GORS_FUZZ_TOOLCHAIN` to test another
installed nightly explicitly. The libFuzzer binaries are also gated behind the
`fuzzing` Cargo feature so ordinary workspace builds do not link them.

| Target | Property |
| --- | --- |
| `scanner` | Arbitrary bytes do not panic the Go scanner. |
| `parser` | Arbitrary bytes do not panic the Go parser. |
| `roundtrip` | Independent parses produce identical AST snapshots. |
| `compiler` | Accepted Go programs do not panic semantic lowering, verification, or source printing. |

The `roundtrip` target parses each input independently and compares its AST
snapshots. `ast::fprint` supplies the deterministic diagnostic dump; that dump
is not reparsed as Go source.

## Corpus and regressions

Each target owns a reviewed seed corpus under `fuzz/corpus/<target>/`.
libFuzzer writes new coverage inputs into ignored
`fuzz/work-corpus/<target>/`, so an ordinary fuzz run does not dirty the
reviewed seeds. Scheduled runs cache that evolving corpus between jobs.
Crashes, timeouts, and OOM inputs go under ignored `fuzz/artifacts/`.

After minimizing and understanding an artifact, promote it into the checked-in
corpus:

```bash
./fuzz/scripts/export-crashes.sh compiler
cargo test --package fuzz --test corpus
```

Never commit a finding blindly. Give a promoted input a descriptive name when
possible and add a focused compiler regression test when the root cause warrants
one.
