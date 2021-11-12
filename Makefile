.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test:
	cargo test --release -- --nocapture

.PHONY: test-lexer
test-lexer:
	cargo test --release lexer -- --nocapture

.PHONY: test-parser
test-parser:
	cargo test --release parser -- --nocapture

.PHONY: dev-lexer
dev-lexer:
	watchexec --restart --clear 'DEV=true cargo test lexer -- --nocapture'

.PHONY: dev-parser
dev-parser:
	watchexec --restart --clear 'DEV=true cargo test parser -- --nocapture'
