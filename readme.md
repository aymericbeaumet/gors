# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)

[gors](https://github.com/aymericbeaumet/gors) is an experimental Go toolchain
written in Rust, featuring a parser, compiler, and code printer that transpiles
Go to Rust.

## Features

- **Scanner/Lexer**: Tokenizes Go source code
- **Parser**: Generates an AST compatible with Go's `go/ast` package
- **Compiler**: Transpiles Go AST to Rust `syn` AST
- **Code Generator**: Outputs formatted Rust code

## Supported Go Constructs

- Package declarations and imports
- Functions and methods
- Variables and constants
- Control flow: `if`, `for`, `switch`, `select`
- Branch statements: `break`, `continue`, `goto`, `fallthrough`
- Labeled statements
- Basic types and composite literals
- Pointers and references
- Channels (parsing only)

## Install

### Using Homebrew (Recommended)

```bash
brew tap aymericbeaumet/tap
brew install gors
```

### Using Cargo

_This method requires the [Rust
toolchain](https://www.rust-lang.org/tools/install) to be installed on your
machine._

```bash
cargo install --git https://github.com/aymericbeaumet/gors.git gors-cli
```

### From Source

```bash
git clone --depth=1 https://github.com/aymericbeaumet/gors.git /tmp/gors
cargo install --path=/tmp/gors/gors-cli
```

## Usage

```bash
# Tokenize a Go file
gors tokens path/to/file.go

# Parse and print AST
gors ast path/to/file.go

# Compile to Rust (outputs main.rs)
gors build --emit=rust path/to/file.go

# Compile and run
gors run path/to/file.go
```

### Example

```go
// hello.go
package main

import "fmt"

func main() {
    fmt.Println("Hello, World!")
}
```

```bash
$ gors run hello.go
Hello, World!
```

## Development

### Prerequisites

```bash
brew install rustup binaryen watchexec
rustup toolchain install stable && rustup toolchain install nightly && rustup default stable
cargo install --force cargo-fuzz
```

The Go SDK is pinned by `.go-version`; the Rust build downloads and extracts
that SDK into `$CARGO_HOME/gors-cache/` for the embedded stdlib and integration
test oracle.

### Building and Testing

```bash
# Lint
cargo clippy --workspace -- -D warnings

# Build
cargo build --workspace

# Run unit tests
make test-unit

# Run integration suites
make test-integration-lexer
make test-integration-parser
make test-integration-run

# Fuzz testing
cargo +nightly fuzz run scanner
cargo +nightly fuzz run parser

# Generate documentation
cargo doc -p gors --open
```

### Debug Mode

```bash
RUST_LOG=debug cargo run -- tokens tests/fixtures/go_programs/fizzbuzz/main.go
RUST_LOG=debug cargo run -- ast tests/fixtures/go_programs/fizzbuzz/main.go
RUST_LOG=debug cargo run -- build tests/fixtures/go_programs/fizzbuzz
RUST_LOG=debug cargo run -- run tests/fixtures/go_programs/fizzbuzz
```

## License

MIT
