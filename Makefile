.PHONY: all
all:

.PHONY: test
test: gofmt
	$(MAKE) go-tokens go-ast

.PHONY: clean
clean:
	find . \( -name '*.fmt.go' -o -name '*.go.ast' -o -name '*.go.tokens' \) -exec rm -rf {} \;

.PHONY: gofmt
gofmt: $(patsubst %.raw.go,%.fmt.go,$(wildcard ./tests/files/*.raw.go))

.PHONY: go-tokens
go-tokens: $(patsubst %.go,%.go.tokens,$(wildcard ./tests/files/*.go))

.PHONY: go-ast
go-ast: $(patsubst %.go,%.go.ast,$(wildcard ./tests/files/*.go))

%.fmt.go: %.raw.go
	gofmt -s $< > $@

%.go.tokens: %.go
	GO111MODULE=off go run ./go tokens $< > $@

%.go.ast: %.go
	GO111MODULE=off go run ./go ast $< > $@
