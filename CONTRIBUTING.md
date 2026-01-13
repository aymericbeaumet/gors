# Contributing to gors

Thank you for your interest in contributing to gors! This document provides
guidelines and information for contributors.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/gors.git`
3. Set up the development environment (see README.md)
4. Create a feature branch: `git checkout -b feature/your-feature`

## Development Setup

### Prerequisites

- Rust stable toolchain (latest)
- Rust nightly toolchain (for fuzzing)
- Go 1.21+ (for integration tests)
- wasm-pack (for WebAssembly builds)

### Building

```bash
cargo build --workspace
```

### Running Tests

```bash
# Unit tests
cargo test --workspace --exclude=gors-cli

# Integration tests (requires Go)
cargo test --workspace --package=gors-cli -- --nocapture --test-threads=1

# Specific test types
cargo test --package=gors-cli lexer   # Lexer tests
cargo test --package=gors-cli parser  # Parser tests
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` and fix all warnings
- Follow Rust naming conventions
- Add documentation comments for public APIs

## Project Structure

```
gors/
├── gors/                 # Core library
│   ├── ast/              # Go AST definitions
│   ├── codegen/          # Rust code generation
│   ├── compiler/         # Go AST to Rust syn conversion
│   │   └── passes/       # Compiler transformation passes
│   ├── parser/           # Go parser
│   ├── scanner/          # Go lexer/tokenizer
│   └── token/            # Token definitions
├── gors-cli/             # Command-line interface
│   └── tests/            # Integration tests
│       ├── files/        # Go test files
│       └── programs/     # Complete Go programs
├── gors-wasm/            # WebAssembly bindings
├── fuzz/                 # Fuzz testing targets
└── www/                  # Web playground
```

## Adding New Go Language Features

### Parser

1. Add AST types to `gors/ast/mod.rs`
2. Add `Printable` implementations in `gors/ast/printable.rs`
3. Implement parsing in `gors/parser/mod.rs`
4. Add test files in `gors-cli/tests/files/`

### Compiler

1. Add `From` implementations in `gors/compiler/mod.rs`
2. Add compiler passes if needed in `gors/compiler/passes/`
3. Add unit tests in the `tests` module

## Submitting Changes

1. Ensure all tests pass
2. Update documentation if needed
3. Create a pull request with a clear description
4. Reference any related issues

## Reporting Issues

When reporting bugs, please include:

- Go source code that triggers the issue
- Expected behavior
- Actual behavior
- Rust/Go versions

## Questions?

Feel free to open an issue for any questions or discussions.
