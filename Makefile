.PHONY: all
all: build test

.PHONY: build
build:
	cargo build --release
	cd ./go && go build .

.PHONY: test
test: build
	./scripts/test.sh
