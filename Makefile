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
	cargo build --release
	./scripts/test.sh release

.PHONY: tdd
tdd: ./go/go
	cargo build && ./scripts/test.sh debug tdd

.PHONY: last
last: ./go/go
	cargo build && ./scripts/test.sh debug last

.PHONY: last-debug
last-debug:
	RUST_LOG=debug cargo run -- tokens $(shell cat .tests/_last)

.PHONY: trace-last
last-trace:
	RUST_LOG=trace cargo run -- tokens $(shell cat .tests/_last)

./go/go:
	cd ./go && go build -o go .

.PHONY: clean
clean:
	rm -rf .tests target ./go/go
