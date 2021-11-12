# Gors

## Development

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
ulimit -n 4096
make lint test
```

## TODO

- split the lexer/parser/cli into their own crates
- make all the crates `#![no_std]`
- add support for `go run ./go/go ./...` and `cargo run -- ./...` syntax
