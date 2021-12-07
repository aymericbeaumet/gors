CARGO_TEST := cargo test -p tests -- --nocapture --test-threads=1

.PHONY: all
all: build lint test

.PHONY: build
build:
	cargo build --release

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: go-cli build
	git submodule update --init
	$(CARGO_TEST)

.PHONY: dev
dev: go-cli
	watchexec --restart --clear 'cargo build && RELEASE_BUILD=false LOCAL_FILES_ONLY=true PRINT_FILES=true $(CARGO_TEST)'

# TODO: move this to as a Cargo custom build in ./tests/build.rs
.PHONY: go-cli
go-cli: tests/go-cli/go-cli
tests/go-cli/go-cli:
	cd ./tests/go-cli && go build .
