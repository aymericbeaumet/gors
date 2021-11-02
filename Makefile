.PHONY: all
all: build lint test

.PHONY: build
build:
	cargo build --release

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test:
	@cargo build
	@cd ./go && go build .
	@./scripts/test.sh
