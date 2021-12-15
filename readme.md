# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)

[gors](https://github.com/aymericbeaumet/gors) is an experimental go toolchain
written in rust (parser, compiler).

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
brew install go@1.17 watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
cargo build
cargo clippy
cargo test -- --nocapture --test-threads=1

watchexec --restart --clear 'cargo test -- --nocapture --test-threads=1'
RUST_LOG=debug cargo run -- ast gors-cli/tests/files/comment.go
```

## TODO

- inline rust code from go code
