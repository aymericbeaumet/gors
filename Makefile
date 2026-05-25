.PHONY: all build format lint test test-unit test-integration-lexer test-integration-parser test-integration-run fuzz
.PHONY: web-install web-build web-format web-lint web-dev

FUZZ_CASES ?= 128
FUZZ_EDGE_CASES ?= 32
FUZZ_SMOKE_CASES ?= 1
FUZZ_SMOKE_EDGE_CASES ?= 1

########
# rust #
########

all: build lint test

build:
	cargo build --workspace

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test: test-unit test-integration-lexer test-integration-parser test-integration-run

test-unit:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --workspace --lib --bins --examples -- --nocapture

test-integration-lexer:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile ci --package=gors --features test_integration_lexer --test test_integration_lexer -- --nocapture

test-integration-parser:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile ci --package=gors --features test_integration_parser --test test_integration_parser -- --nocapture

test-integration-run:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile ci --package=gors --features test_integration_run --test test_integration_run -- --nocapture

format:
	cargo fmt --all

fuzz:
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) cargo test --package=fuzz --test proptest -- --nocapture

#######
# web #
#######

web-all: web-install web-build web-lint

web-install:
	npm --prefix www ci --no-audit --fund=false --loglevel=error

web-build: web-install
	npm --prefix www run build

web-lint: web-install
	npm --prefix www run format:check
	npm --prefix www run lint

web-dev: web-install
	npm --prefix www run dev

web-format: web-install
	npm --prefix www run format
