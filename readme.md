```
brew install go@1.17 rustup-init watchexec
rustup update && rustup component add rustfmt rls rust-analysis rust-src
```

```
make build
make dev
./go/go -- tokens .repositories/go/test/typeparam/issue47892.go
RUST_LOG=trace cargo run -- tokens .repositories/go/test/typeparam/issue47892.go
```
