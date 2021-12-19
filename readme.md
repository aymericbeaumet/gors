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
brew install rustup go@1.17 watchexec
rustup toolchain install stable && rustup default stable
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

```
cargo build
cargo clippy
cargo test
cargo test -- --nocapture --test-threads=1
RUST_LOG=debug cargo run -- ast gors-cli/tests/files/comment.go
```

## TODO

- fuzzing (https://lcamtuf.coredump.cx/afl)
