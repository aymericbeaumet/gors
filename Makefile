.PHONY: all
all:

.PHONY: test
test: gofmt
	$(MAKE) go-parse-tokens go-parse-ast

.PHONY: clean
clean:
	find . \( -name '*.fmt.go' -o -name '*.go.ast' -o -name '*.go.tokens' \) -exec rm -rf {} \;

.PHONY: gofmt
gofmt: $(patsubst %.raw.go,%.fmt.go,$(wildcard ./tests/files/*.raw.go))

.PHONY: go-parse-tokens
go-parse-tokens: $(patsubst %.go,%.go.tokens,$(wildcard ./tests/files/*.go))

.PHONY: go-parse-ast
go-parse-ast: $(patsubst %.go,%.go.ast,$(wildcard ./tests/files/*.go))

%.fmt.go: %.raw.go
	gofmt -s $< > $@

%.go.tokens: %.go
	GO111MODULE=off go run ./go-parse tokens $< > $@

%.go.ast: %.go
	GO111MODULE=off go run ./go-parse ast $< > $@
