CARGO_TEST_ARGS := -p gors-tests -- --nocapture --test-threads=1

.PHONY: all
all: lint test

.PHONY: lint
lint:
	cargo clippy

.PHONY: build
build:
	cargo build --release

.PHONY: test
test: go-cli
	cargo test $(CARGO_TEST_ARGS)

.PHONY: dev
dev: go-cli
	watchexec --restart --clear 'RELEASE_BUILD=false LOCAL_FILES_ONLY=true PRINT_FILES=true cargo test $(CARGO_TEST_ARGS)'

.PHONY: go-cli
go-cli: gors-tests/go-cli/go-cli

gors-tests/go-cli/go-cli:
	cd ./gors-tests/go-cli && go build .

.PHONY: clean
clean:
	rm -f ./gors-tests/go-cli/go-cli
