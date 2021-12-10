# gors [![GitHub Actions](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml/badge.svg)](https://github.com/aymericbeaumet/gors/actions/workflows/ci.yml)


## Install

```
git clone -–depth=1 https://github.com/aymericbeaumet/gors.git /tmp/gors
cargo install --path=/tmp/gors/gors-cli
```

## Development

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
RUST_LOG=trace cargo run -- <command> <file>
cargo build
cargo clippy
cargo test -- --nocapture --test-threads=1
watchexec --restart --clear 'cargo test -- --nocapture --test-threads=1'
```

## TODO

- inline rust code from go code
