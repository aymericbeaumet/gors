# Gors

## Install

```
git clone https://github.com/aymericbeaumet/gors.git /tmp/gors
cargo install --path=/tmp/gors/gors-cli
```

## Development

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
make dev
RUST_LOG=info cargo run -- build <file>
RUST_LOG=info cargo run -- run <file>
RUST_LOG=debug cargo run -- ast <file>
RUST_LOG=trace cargo run -- tokens <file>
```

## Testing

```
make lint test
```

## TODO

- inline rust code from go code
