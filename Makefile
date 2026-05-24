.PHONY: all build format lint test test-unit test-integration-lexer test-integration-parser test-integration-run test-integration-generate fuzz
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
	# rust
	cargo build --workspace

lint:
	# rust
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	# rust
	GORS_FUZZ_CASES=$(FUZZ_SMOKE_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_SMOKE_EDGE_CASES) GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --workspace --features gors/test_integration_lexer,gors/test_integration_parser,gors/test_integration_run,gors/test_integration_generate -- --nocapture

test-unit:
	# rust
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --workspace --lib --bins --examples -- --nocapture

test-integration-lexer:
	# rust
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --release --package=gors --features test_integration_lexer --test test_integration_lexer -- --nocapture

test-integration-parser:
	# rust
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --release --package=gors --features test_integration_parser --test test_integration_parser -- --nocapture

test-integration-run:
	# rust
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --release --package=gors --features test_integration_run --test test_integration_run -- --nocapture

test-integration-generate:
	# rust
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --release --package=gors --features test_integration_generate --test test_integration_generate -- --nocapture

format: web-install
	# rust
	cargo fmt --all
	# web
	npm --prefix www run format

fuzz:
	# rust
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) cargo test --package=fuzz --test proptest -- --nocapture

#######
# web #
#######

web-install:
	# web
	npm --prefix www ci --no-audit --fund=false --loglevel=error

web-build: web-install
	# web
	npm --prefix www run build

web-lint: web-install
	# web
	npm --prefix www run format:check
	npm --prefix www run lint

web-dev: web-install
	# web
	npm --prefix www run dev

web-format: web-install
	# web
	npm --prefix www run format
