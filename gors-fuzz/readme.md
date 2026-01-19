# Fuzzing gors

This directory contains fuzzing infrastructure for gors. It supports two approaches:

1. **Property-based testing** with proptest (runs on stable Rust, suitable for CI)
2. **Coverage-guided fuzzing** with AFL (requires `cargo-afl`, for deep testing)

## Quick Start

### Property-based Tests (Stable Rust)

These tests can run without any special setup:

```bash
# From the project root
cargo test --package gors-fuzz

# Or run with more test cases
PROPTEST_CASES=10000 cargo test --package gors-fuzz
```

### Coverage-guided Fuzzing with AFL

AFL fuzzing requires the `cargo-afl` tool:

```bash
# Install cargo-afl (one-time setup)
cargo install afl

# Fuzz the scanner (uses all CPUs)
make fuzz-scanner

# Fuzz the parser
make fuzz-parser

# Fuzz the roundtrip (parse -> print -> reparse)
make fuzz-roundtrip

# Or use the script directly for more options
./gors-fuzz/scripts/fuzz.sh scanner -j 4    # Use 4 CPUs
./gors-fuzz/scripts/fuzz.sh parser -c       # Continue previous session
```

## Fuzzing Targets

| Target | Description |
|--------|-------------|
| `fuzz_scanner` | Fuzz the Go lexer/scanner |
| `fuzz_parser` | Fuzz the Go parser |
| `fuzz_roundtrip` | Fuzz parse -> print -> reparse cycle |

## Directory Structure

```
gors-fuzz/
├── Cargo.toml          # Crate configuration
├── corpus/             # Seed inputs for fuzzing
│   ├── scanner/        # Scanner seeds
│   ├── parser/         # Parser seeds
│   └── roundtrip/      # Roundtrip seeds
├── fuzz_targets/       # AFL fuzz target binaries
│   ├── scanner.rs
│   ├── parser.rs
│   └── roundtrip.rs
├── scripts/
│   ├── fuzz.sh         # Multi-CPU fuzzing script
│   └── export-crashes.sh  # Export crashes as test files
├── out/                # AFL output (gitignored)
├── sync/               # AFL sync directory (gitignored)
└── tests/
    └── proptest.rs     # Property-based tests
```

## Exporting Crashes as Test Files

When fuzzing finds crashes, you can export them as test files:

```bash
# Export all crashes to gors-cli/tests/files/fuzz_*.go
make fuzz-export

# Or export a specific target
./gors-fuzz/scripts/export-crashes.sh parser
```

The exported files will be automatically picked up by the integration tests.

## Adding New Corpus Files

To improve fuzzing effectiveness, add representative Go source files to the corpus:

```bash
# Add a new scanner seed
echo 'package main' > gors-fuzz/corpus/scanner/my_seed

# Add a new parser seed
cp my_complex_file.go gors-fuzz/corpus/parser/
```

## Continuous Fuzzing

For extended fuzzing sessions:

```bash
# Start fuzzing (will use all CPUs)
./gors-fuzz/scripts/fuzz.sh parser

# In another terminal, monitor progress
watch -n 1 'ls -la gors-fuzz/sync/parser/*/crashes/ 2>/dev/null | head -20'

# Stop with Ctrl+C, then export findings
make fuzz-export
```

## Tips

- Start with a good corpus - the existing test files in `gors-cli/tests/files/` are good seeds
- Run fuzzing for at least a few hours for meaningful coverage
- After finding crashes, export them and add regression tests
- The roundtrip target is particularly good at finding consistency bugs
