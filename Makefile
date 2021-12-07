CARGO_TEST_ARGS := -p gors-tests -- --nocapture --test-threads=1

.PHONY: all
all: build lint test

.PHONY: build
build:
	cargo build --release

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: go-cli
	cargo test $(CARGO_TEST_ARGS)

.PHONY: dev
dev: go-cli
	watchexec --restart --clear 'cargo build && RELEASE_BUILD=false LOCAL_FILES_ONLY=true PRINT_FILES=true cargo test $(CARGO_TEST_ARGS)'

# TODO: move this to as a Cargo custom build in ./gors-tests/build.rs
.PHONY: go-cli
go-cli: gors-tests/go-cli/go-cli
gors-tests/go-cli/go-cli:
	cd ./gors-tests/go-cli && go build .
