.PHONY: all
all: build lint test

.PHONY: build
build:
	cargo build --release

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: ./go/go
	@cargo build --release
	@./scripts/test.sh release

.PHONY: dev
dev: ./go/go
	@watchexec --restart --clear 'cargo build && ./scripts/test.sh debug'

.PHONY: dev-last
dev-last: ./go/go
	@watchexec --restart --clear 'cargo build && ./scripts/test.sh debug last'

.PHONY: debug-last
debug-last:
	@watchexec --restart --clear 'RUST_LOG=debug cargo run -- tokens $(shell cat .tests/_last)'

.PHONY: trace-last
trace-last:
	@watchexec --restart --clear 'RUST_LOG=trace cargo run -- tokens $(shell cat .tests/_last)'

./go/go:
	cd ./go && go build -o go .

.PHONY: clean
clean:
	rm -rf .tests target ./go/go
