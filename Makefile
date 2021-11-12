.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test:
	cargo test --release -- --nocapture
