# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)

[gors](https://github.com/aymericbeaumet/gors) is an experimental go toolchain
written in rust (parser, compiler, codegen).

## Install

### Using git

_This method requires the [Rust
toolchain](https://www.rust-lang.org/tools/install) to be installed on your
machine._

```
git clone -–depth=1 https://github.com/aymericbeaumet/gors.git /tmp/gors
cargo install --path=/tmp/gors/gors-cli
```

## Development

```
brew install rustup go@1.17 binaryen watchexec
rustup toolchain install stable && rustup toolchain install nightly && rustup default stable
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
cargo install --force cargo-fuzz
```

```
cargo clippy
cargo build
cargo test -- --nocapture --test-threads=1
cargo +nightly fuzz run scanner
cargo doc -p gors --open
```

```
RUST_LOG=debug cargo run -- tokens gors-cli/tests/programs/fizzbuzz.go
RUST_LOG=debug cargo run -- ast gors-cli/tests/programs/fizzbuzz.go
RUST_LOG=debug cargo run -- build --emit=rust gors-cli/tests/programs/fizzbuzz.go
RUST_LOG=debug cargo run -- run gors-cli/tests/programs/fizzbuzz.go
```
