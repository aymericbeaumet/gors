# Gors

## Development

```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
ulimit -n 8192
make lint test
```

## TODO

- split the lexer/parser/cli into their own crates
- make all the crates `#![no_std]`
