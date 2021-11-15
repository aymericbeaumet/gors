CARGO_TEST_BIN_ARGS := --nocapture --test-threads=1

.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: tests/go-cli/go-cli
	cargo test --release -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-lexer
test-lexer: tests/go-cli/go-cli
	cargo test --release lexer -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-parser
test-parser: tests/go-cli/go-cli
	cargo test --release parser -- $(CARGO_TEST_BIN_ARGS)

.PHONY: dev-lexer
dev-lexer: tests/go-cli/go-cli
	watchexec --restart --clear 'FAST_BUILD=true LOCAL_FILES_ONLY=true VERBOSE=true cargo test lexer -- $(CARGO_TEST_BIN_ARGS)'

.PHONY: dev-parser
dev-parser: tests/go-cli/go-cli
	watchexec --restart --clear 'FAST_BUILD=true LOCAL_FILES_ONLY=true VERBOSE=true cargo test parser -- $(CARGO_TEST_BIN_ARGS)'

tests/go-cli/go-cli:
	cd ./tests/go-cli && go build .
