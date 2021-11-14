CARGO_TEST_BIN_ARGS := --nocapture --test-threads=1

.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test:
	cargo test --release -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-lexer
test-lexer:
	cargo test --release lexer -- $(CARGO_TEST_BIN_ARGS)

.PHONY: test-parser
test-parser:
	cargo test --release parser -- $(CARGO_TEST_BIN_ARGS)

.PHONY: dev-lexer
dev-lexer:
	watchexec --restart --clear 'DEV=true cargo test lexer -- $(CARGO_TEST_BIN_ARGS)'

.PHONY: dev-parser
dev-parser:
	watchexec --restart --clear 'DEV=true cargo test parser -- $(CARGO_TEST_BIN_ARGS)'
