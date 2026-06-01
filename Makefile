all: rust-all web-all

########
# rust #
########

rust-all: rust-lint rust-build rust-test

RUST_TEST_PARTIAL_PROFILE ?= ci
RUST_TEST_FULL_INTEGRATION_PROFILE ?= release
RUST_TEST_INTEGRATION_PROFILE ?= $(RUST_TEST_PARTIAL_PROFILE)

rust-format:
	cargo fmt --all

rust-lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

rust-build:
	cargo build --workspace

rust-test: rust-test-unit rust-test-integration

rust-test-unit:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_PARTIAL_PROFILE) --workspace --lib --bins --examples -- --nocapture

rust-test-integration:
	$(MAKE) rust-test-integration-go-repositories RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-spec RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-stdlib RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-programs RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)

rust-test-integration-go-repositories:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_repositories --test test_integration_go_repositories -- --nocapture

rust-test-integration-go-spec:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_spec --test test_integration_go_spec -- --nocapture

rust-test-integration-go-stdlib:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_stdlib --test test_integration_go_stdlib -- --nocapture

rust-test-integration-go-programs:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_programs --test test_integration_go_programs -- --nocapture

#######
# web #
#######

PLAYWRIGHT_INSTALL_ARGS ?= chromium

web-all: web-lint web-build web-test

web-install:
	npm --prefix www ci --no-audit --fund=false --loglevel=error

web-format: web-install
	npm --prefix www run format

web-lint: web-install
	npm --prefix www run format:check
	npm --prefix www run lint

web-build: web-install
	npm --prefix www run build

web-test: web-test-unit web-test-integration

web-test-unit: web-install
	npm --prefix www run test:unit

web-test-integration: web-install
	npm --prefix www exec playwright install $(PLAYWRIGHT_INSTALL_ARGS)
	npm --prefix www run test:integration

#######
# dev #
#######

dev: web-install
	npm --prefix www run dev

########
# fuzz #
########

FUZZ_CASES ?= 128
FUZZ_EDGE_CASES ?= 32
FUZZ_SMOKE_CASES ?= 1
FUZZ_SMOKE_EDGE_CASES ?= 1

fuzz-all: fuzz-scanner fuzz-parser fuzz-roundtrip

fuzz-scanner:
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) cargo +nightly fuzz run scanner

fuzz-parser:
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) cargo +nightly fuzz run parser

fuzz-roundtrip:
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) cargo +nightly fuzz run roundtrip

# .phony
.PHONY: all dev
.PHONY: rust-all rust-build rust-format rust-lint rust-test rust-test-unit rust-test-integration rust-test-integration-go-repositories rust-test-integration-go-spec rust-test-integration-go-stdlib rust-test-integration-go-programs
.PHONY: web-all web-build web-format web-install web-lint web-test web-test-unit web-test-integration
.PHONY: fuzz-all fuzz-scanner fuzz-parser fuzz-roundtrip
