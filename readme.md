# Gors

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

## Development

- Work on the lexer:

```
make test-lexer
make dev-lexer
RUST_LOG=trace cargo run -- tokens <file>
```

- Work on the parser:

```
make test-parser
make dev-parser
RUST_LOG=debug cargo run -- ast <file>
```

## Testing

```
ulimit -n 8192
make lint test
```
