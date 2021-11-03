```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
make build
make dev
RUST_LOG=debug cargo run -- tokens .repositories/go/misc/cgo/gmp/gmp.go
```
