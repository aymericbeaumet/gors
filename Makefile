# gors Makefile
# Abstracts Rust/Cargo/Clippy/wasm-pack commands for development and CI

.PHONY: all help
.PHONY: setup install-tools
.PHONY: fmt fmt-check lint clippy doc
.PHONY: build build-release build-wasm build-wasm-dev
.PHONY: test test-unit test-integration test-lexer test-parser
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
	@echo "  test             Run unit tests"
	@echo "  test-integration Run integration tests on all submodules (requires: make setup)"
	@echo "  test-lexer       Run lexer integration tests"
	@echo "  test-parser      Run parser integration tests"
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
	cargo build --workspace --all-features

build-release:
	cargo build --workspace --all-features --release

build-wasm:
	cd www/wasm && wasm-pack build --release

build-wasm-dev:
	cd www/wasm && wasm-pack build --dev

#------------------------------------------------------------------------------
# Testing
#------------------------------------------------------------------------------

test:
	cargo test --workspace --all-features --exclude=gors-cli

test-lexer:
	cargo test --release --workspace --all-features --package=gors-cli lexer -- --nocapture --test-threads=1

test-parser:
	cargo test --release --workspace --all-features --package=gors-cli parser -- --nocapture --test-threads=1

#------------------------------------------------------------------------------
# Fuzzing
#------------------------------------------------------------------------------

# Run property-based tests (stable Rust, suitable for CI)
fuzz-test:
	cargo test --package gors-fuzz

# Build fuzz targets with AFL instrumentation
fuzz-build:
	cd gors-fuzz && cargo afl build --release --features afl-fuzz

# Fuzz the scanner/lexer
fuzz-scanner:
	./gors-fuzz/scripts/fuzz.sh scanner

# Fuzz the parser
fuzz-parser:
	./gors-fuzz/scripts/fuzz.sh parser

# Fuzz the roundtrip (parse->print->reparse)
fuzz-roundtrip:
	./gors-fuzz/scripts/fuzz.sh roundtrip

# Export crash inputs as test files to gors-cli/tests/files/
fuzz-export:
	./gors-fuzz/scripts/export-crashes.sh

#------------------------------------------------------------------------------
# Website
#------------------------------------------------------------------------------

www: build-wasm www-install www-build

www-install:
	cd www && npm ci

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
	rm -rf gors-fuzz/out gors-fuzz/sync
