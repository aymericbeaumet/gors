# Gors

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

## Development

```
make dev-lexer
make dev-parser
RUST_LOG=trace cargo run -- ast tests/files/6_arithmetic_operators.go
```

## Testing

```
ulimit -n 8192
make lint test
```
