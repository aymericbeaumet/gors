CARGO_TEST_BIN_ARGS := --nocapture --test-threads=1

.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: go-cli
	cargo test --release -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-lexer
test-lexer: go-cli
	cargo test --release lexer -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-parser
test-parser: go-cli
	cargo test --release parser -- $(CARGO_TEST_BIN_ARGS)

.PHONY: dev-lexer
dev-lexer: go-cli
	watchexec --restart --clear 'FAST_BUILD=true LOCAL_FILES_ONLY=true PRINT_FILES=true cargo test lexer -- $(CARGO_TEST_BIN_ARGS)'

.PHONY: dev-parser
dev-parser: go-cli
	watchexec --restart --clear 'FAST_BUILD=true LOCAL_FILES_ONLY=true PRINT_FILES=true cargo test parser -- $(CARGO_TEST_BIN_ARGS)'

.PHONY: go-cli
go-cli: tests/go-cli/go-cli

tests/go-cli/go-cli:
	cd ./tests/go-cli && go build .
