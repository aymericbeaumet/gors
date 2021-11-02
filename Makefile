.PHONY: all
all: build lint test

.PHONY: build
build:
	cargo build --release

.PHONY: lint
lint:
	cargo clippy

.PHONY: test
test: ./go/go
	@cargo build --release
	@./scripts/test.sh "release"

.PHONY: dev
dev: ./go/go
	@watchexec --restart --clear 'cargo build && cargo clippy && ./scripts/test.sh "debug"'

./go/go:
	cd ./go && go build -o go .

.PHONY: clean
clean:
	rm -rf .tests target ./go/go
