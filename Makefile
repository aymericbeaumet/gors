CARGO_TEST_BIN_ARGS := --nocapture --test-threads=1

.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: build
build:
	cargo build --release

.PHONY: test
test: go-cli
	cargo test --release -- $(CARGO_TEST_BIN_ARGS)

.PHONY: dev
dev: go-cli
	watchexec --restart --clear 'RELEASE_BUILD=false LOCAL_FILES_ONLY=true PRINT_FILES=true cargo test -- $(CARGO_TEST_BIN_ARGS)'

.PHONY: go-cli
go-cli: tests/go-cli/go-cli

tests/go-cli/go-cli:
	cd ./tests/go-cli && go build .

.PHONY: clean
clean:
	rm -f ./tests/go-cli/go-cli
