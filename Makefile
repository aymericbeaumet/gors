all: rust-all web-all

########
# rust #
########

rust-all: rust-lint rust-build rust-test

RUST_TEST_PARTIAL_PROFILE ?= ci
RUST_TEST_FULL_INTEGRATION_PROFILE ?= release
RUST_TEST_INTEGRATION_PROFILE ?= $(RUST_TEST_PARTIAL_PROFILE)
GORS_TEST_FAIL_FAST ?= 1

rust-format:
	cargo fmt --all

rust-lint:
	bash scripts/check-source-layout.sh
	bash scripts/check-compiler-architecture.sh
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

rust-build:
	cargo build --workspace

rust-test: rust-test-unit rust-test-backend-smoke

rust-test-unit:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_PARTIAL_PROFILE) --workspace --lib --bins --examples -- --nocapture

rust-test-backend-smoke:
	@test "$$(cargo run --quiet --package gors-cli -- run fuzz/corpus/compiler/runtime_smoke.go 2>&1)" = "8"

# Broad compatibility suites stay opt-in because they exercise the complete
# fixture corpus and pinned Go toolchain.
rust-test-integration:
	$(MAKE) rust-test-integration-go-repositories RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-spec RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-stdlib RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	$(MAKE) rust-test-integration-go-programs RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)

rust-test-integration-go-repositories:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_repositories --test test_integration_go_repositories -- --nocapture

rust-test-integration-go-spec:
	GORS_TEST_FAIL_FAST=$(GORS_TEST_FAIL_FAST) GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_spec --test test_integration_go_spec -- --nocapture

rust-test-integration-go-stdlib:
	GORS_TEST_FAIL_FAST=$(GORS_TEST_FAIL_FAST) GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_stdlib --test test_integration_go_stdlib -- --nocapture

rust-test-integration-go-programs:
	GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_programs --test test_integration_go_programs -- --nocapture

rust-test-integration-go-spec-fixture:
	@test -n "$(FIXTURE)" || (echo "FIXTURE is required" >&2; exit 2)
	GORS_TEST_FILTER="$(FIXTURE)" GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_spec --test test_integration_go_spec run_go_spec_generated_rust -- --exact --nocapture

rust-test-integration-go-stdlib-fixture:
	@test -n "$(FIXTURE)" || (echo "FIXTURE is required" >&2; exit 2)
	GORS_TEST_FILTER="$(FIXTURE)" GORS_TEST_FAIL_FAST=1 GORS_TEST_VERBOSE=1 cargo test --profile $(RUST_TEST_INTEGRATION_PROFILE) --package=gors --features test_integration_go_stdlib --test test_integration_go_stdlib run_go_stdlib_generated_rust -- --exact --nocapture

conformance-report:
	GORS_UPDATE_CONFORMANCE_REPORTS=1 $(MAKE) rust-test-integration-go-spec GORS_TEST_FAIL_FAST=0 RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)
	GORS_UPDATE_CONFORMANCE_REPORTS=1 $(MAKE) rust-test-integration-go-stdlib GORS_TEST_FAIL_FAST=0 RUST_TEST_INTEGRATION_PROFILE=$(RUST_TEST_FULL_INTEGRATION_PROFILE)

conformance-check: conformance-report
	git diff --exit-code -- gors/tests/reports

clean-integration-cache:
	rm -rf target/gors-integration-run

########
# perf #
########

PERF_PYTHON ?= python3
PERF_SAMPLES ?= 50
PERF_SESSIONS ?= 3
PERF_SEED ?= 20260722
PERF_JOBS ?= $(shell $(PERF_PYTHON) -c 'import os; print(os.cpu_count() or 1)')
PERF_HARDWARE_CLASS ?= unclassified-local
PERF_BASELINE_OUTPUT ?= target/perf/baseline.json
PERF_CERTIFICATION_OUTPUT ?= target/perf/certification.json
PERF_RESULT ?=
PERF_ARGS ?=

# Real measurements are opt-in. PERF_ARGS="--smoke" permits small harness
# exercises, but smoke evidence is permanently ineligible for promotion.
perf-baseline:
	PYTHONDONTWRITEBYTECODE=1 $(PERF_PYTHON) perf/run.py baseline --output $(PERF_BASELINE_OUTPUT) --samples $(PERF_SAMPLES) --sessions $(PERF_SESSIONS) --seed $(PERF_SEED) --jobs $(PERF_JOBS) --hardware-class $(PERF_HARDWARE_CLASS) $(PERF_ARGS)

perf-certify:
	PYTHONDONTWRITEBYTECODE=1 $(PERF_PYTHON) perf/run.py certify --output $(PERF_CERTIFICATION_OUTPUT) --samples $(PERF_SAMPLES) --sessions $(PERF_SESSIONS) --seed $(PERF_SEED) --jobs $(PERF_JOBS) --hardware-class $(PERF_HARDWARE_CLASS) $(PERF_ARGS)

perf-gate:
	PYTHONDONTWRITEBYTECODE=1 $(PERF_PYTHON) perf/run.py gate $(if $(strip $(PERF_RESULT)),--result $(PERF_RESULT),)

# Safe for ordinary correctness CI: no wall-clock measurements are run here.
perf-test:
	PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=perf $(PERF_PYTHON) -m unittest discover -s perf/tests -v
	PYTHONDONTWRITEBYTECODE=1 $(PERF_PYTHON) perf/run.py validate
	$(MAKE) perf-gate

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
FUZZ_PROPTEST_SEED ?= 1
FUZZ_TOOLCHAIN ?= nightly-2026-07-01

fuzz-all: fuzz-scanner fuzz-parser fuzz-roundtrip fuzz-compiler

fuzz-test:
	GORS_FUZZ_CASES=$(FUZZ_CASES) GORS_FUZZ_EDGE_CASES=$(FUZZ_EDGE_CASES) PROPTEST_RNG_SEED=$(FUZZ_PROPTEST_SEED) cargo test --profile ci --package fuzz --tests

fuzz-scanner:
	CARGO_PROFILE_RELEASE_LTO=off cargo +$(FUZZ_TOOLCHAIN) fuzz run --features fuzzing --codegen-units 16 scanner

fuzz-parser:
	CARGO_PROFILE_RELEASE_LTO=off cargo +$(FUZZ_TOOLCHAIN) fuzz run --features fuzzing --codegen-units 16 parser

fuzz-roundtrip:
	CARGO_PROFILE_RELEASE_LTO=off cargo +$(FUZZ_TOOLCHAIN) fuzz run --features fuzzing --codegen-units 16 roundtrip

fuzz-compiler:
	CARGO_PROFILE_RELEASE_LTO=off cargo +$(FUZZ_TOOLCHAIN) fuzz run --features fuzzing --codegen-units 16 compiler

# .phony
.PHONY: all dev
.PHONY: rust-all rust-build rust-format rust-lint rust-test rust-test-unit rust-test-backend-smoke rust-test-integration rust-test-integration-go-repositories rust-test-integration-go-spec rust-test-integration-go-stdlib rust-test-integration-go-programs
.PHONY: perf-baseline perf-certify perf-gate perf-test
.PHONY: web-all web-build web-format web-install web-lint web-test web-test-unit web-test-integration
.PHONY: fuzz-all fuzz-test fuzz-scanner fuzz-parser fuzz-roundtrip fuzz-compiler
