# Gors

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

## Development

```
make dev
RUST_LOG=trace cargo run -- tokens <file>
RUST_LOG=debug cargo run -- ast <file>
RUST_LOG=info cargo run -- build <file>
```

## Testing

```
ulimit -n 8192
make lint test
```
