# gors Makefile
# Abstracts Rust/Cargo/Clippy/wasm-pack commands for development and CI

.PHONY: all help
.PHONY: setup install-tools
.PHONY: fmt fmt-check lint clippy doc
.PHONY: build build-release build-wasm build-wasm-dev
.PHONY: test test-unit test-integrations test-lexer test-parser _test-lexer _test-parser
.PHONY: fuzz fuzz-test fuzz-scanner fuzz-parser fuzz-roundtrip fuzz-build fuzz-export
.PHONY: www www-install www-lint www-build www-dev
.PHONY: clean clean-all
.PHONY: package

# Default target
all: lint test build

#------------------------------------------------------------------------------
# Help
#------------------------------------------------------------------------------

help:
	@echo "gors Makefile"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Setup:"
	@echo "  setup            Initialize git submodules (required for tests)"
	@echo "  install-tools    Install required development tools"
	@echo ""
	@echo "Development:"
	@echo "  all              Run lint, test, and build (default)"
	@echo ""
	@echo "Linting & Formatting:"
	@echo "  fmt              Format code with rustfmt"
	@echo "  fmt-check        Check code formatting (CI mode)"
	@echo "  lint             Run all linters (fmt-check + clippy)"
	@echo "  clippy           Run clippy linter"
	@echo "  doc              Generate documentation"
	@echo ""
	@echo "Building:"
	@echo "  build            Build all packages (debug)"
	@echo "  build-release    Build all packages (release)"
	@echo "  build-wasm       Build gors-wasm package (release)"
	@echo "  build-wasm-dev   Build gors-wasm package (dev)"
	@echo ""
	@echo "Testing:"
	@echo "  test-unit        Run unit tests (fast, no external dependencies)"
	@echo "  test-integrations Run lexer+parser integration tests in parallel (requires: make setup)"
	@echo "  test-lexer       Run lexer integration tests (with output)"
	@echo "  test-parser      Run parser integration tests (with output)"
	@echo "  test             Alias for test-unit (backward compatibility)"
	@echo ""
	@echo "Fuzzing:"
	@echo "  fuzz-test        Run property-based fuzz tests (stable Rust, CI-friendly)"
	@echo "  fuzz-scanner     Fuzz the Go scanner with AFL (requires: cargo install afl)"
	@echo "  fuzz-parser      Fuzz the Go parser with AFL"
	@echo "  fuzz-roundtrip   Fuzz parse->print->reparse with AFL"
	@echo "  fuzz-build       Build all AFL fuzz targets"
	@echo "  fuzz-export      Export crash inputs as test files"
	@echo ""
	@echo "Website:"
	@echo "  www              Build website (install deps + build wasm + build www)"
	@echo "  www-install      Install www dependencies"
	@echo "  www-lint         Lint www code"
	@echo "  www-build        Build www code"
	@echo "  www-dev          Start www development server"
	@echo ""
	@echo "Packaging:"
	@echo "  package          Build release binary and create tarball"
	@echo ""
	@echo "Cleanup:"
	@echo "  clean            Clean build artifacts"
	@echo "  clean-all        Clean everything including dependencies"

#------------------------------------------------------------------------------
# Setup
#------------------------------------------------------------------------------

setup:
	@echo "Initializing git submodules..."
	git submodule update --init --recursive --depth 1
	@echo "Submodules initialized successfully"

install-tools:
	@echo "Installing development tools..."
	rustup component add rustfmt clippy
	@command -v wasm-pack >/dev/null 2>&1 || \
		(echo "Installing wasm-pack..." && \
		curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh)
	@echo "Tools installed successfully"

#------------------------------------------------------------------------------
# Formatting & Linting
#------------------------------------------------------------------------------

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-features -- -D warnings

lint: fmt-check clippy

doc:
	cargo doc --package=gors --no-deps

#------------------------------------------------------------------------------
# Building
#------------------------------------------------------------------------------

build:
	cargo build --workspace --exclude=gors-fuzz

build-release:
	cargo build --workspace --exclude=gors-fuzz --release

build-wasm:
	cd www/wasm && wasm-pack build --release

build-wasm-dev:
	cd www/wasm && wasm-pack build --dev

#------------------------------------------------------------------------------
# Testing
#------------------------------------------------------------------------------

# Unit tests: fast tests for core library (no external dependencies)
test-unit:
	cargo test --package=gors

# Integration tests: lexer and parser tests against real Go repositories
# Runs both in parallel using make's job parallelization
# The test runner internally uses all available CPU cores
test-integrations: build-release
	@$(MAKE) -j2 _test-lexer _test-parser

# Internal targets for parallel execution (do not call directly)
_test-lexer:
	cargo test --release --package=gors lexer

_test-parser:
	cargo test --release --package=gors parser

# Individual test targets (for debugging specific failures)
test-lexer:
	cargo test --release --package=gors lexer -- --nocapture

test-parser:
	cargo test --release --package=gors parser -- --nocapture

# Legacy alias for backward compatibility
test: test-unit

#------------------------------------------------------------------------------
# Fuzzing
#------------------------------------------------------------------------------

# Run property-based tests (stable Rust, suitable for CI)
fuzz-test:
	cargo test --package gors-fuzz

# Alias for fuzz-test
fuzz: fuzz-test

# Build fuzz targets with AFL instrumentation
fuzz-build:
	cd fuzz && cargo afl build --release --features afl-fuzz

# Fuzz the scanner/lexer
fuzz-scanner:
	./fuzz/scripts/fuzz.sh scanner

# Fuzz the parser
fuzz-parser:
	./fuzz/scripts/fuzz.sh parser

# Fuzz the roundtrip (parse->print->reparse)
fuzz-roundtrip:
	./fuzz/scripts/fuzz.sh roundtrip

# Export crash inputs as test files to gors/tests/files/
fuzz-export:
	./fuzz/scripts/export-crashes.sh

#------------------------------------------------------------------------------
# Website
#------------------------------------------------------------------------------

www: build-wasm www-install www-build

www-install:
	cd www && npm install

www-lint:
	cd www && npm run lint

www-build:
	cd www && npm run build

www-dev: build-wasm-dev www-install
	cd www && npm run dev

#------------------------------------------------------------------------------
# Packaging
#------------------------------------------------------------------------------

# Detect OS and architecture for packaging
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)

ifeq ($(UNAME_S),Linux)
    ifeq ($(UNAME_M),x86_64)
        TARGET := x86_64-unknown-linux-gnu
        PACKAGE_NAME := gors-linux-x86_64
    else ifeq ($(UNAME_M),aarch64)
        TARGET := aarch64-unknown-linux-gnu
        PACKAGE_NAME := gors-linux-aarch64
    endif
else ifeq ($(UNAME_S),Darwin)
    ifeq ($(UNAME_M),x86_64)
        TARGET := x86_64-apple-darwin
        PACKAGE_NAME := gors-darwin-x86_64
    else ifeq ($(UNAME_M),arm64)
        TARGET := aarch64-apple-darwin
        PACKAGE_NAME := gors-darwin-aarch64
    endif
endif

package: build-release
ifdef TARGET
	@echo "Packaging for $(TARGET)..."
	cd target/release && tar -czvf ../../$(PACKAGE_NAME).tar.gz gors
	@echo "Created $(PACKAGE_NAME).tar.gz"
else
	@echo "Error: Unsupported platform $(UNAME_S)/$(UNAME_M)"
	@exit 1
endif

#------------------------------------------------------------------------------
# Cleanup
#------------------------------------------------------------------------------

clean:
	cargo clean
	rm -rf www/dist
	rm -rf www/wasm/pkg
	rm -f gors-*.tar.gz

clean-all: clean
	rm -rf www/node_modules
	rm -rf target
	rm -rf fuzz/out fuzz/sync
