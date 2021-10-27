.PHONY: all
all:

.PHONY: test
test: $(patsubst %.raw.go,%.fmt.go,$(wildcard ./tests/files/*.raw.go)) $(patsubst %.go,%.go.tokens,$(wildcard ./tests/files/*.go)) $(patsubst %.go,%.go.ast,$(wildcard ./tests/files/*.go))

.PHONY: clean
clean:
	find . -name '*.ast' -exec rm -rf {} \;

%.fmt.go: %.go
	gofmt -s $< > $@

%.go.tokens: %.go
	GO111MODULE=off go run ./go-parse tokens $< > $@

%.go.ast: %.go
	GO111MODULE=off go run ./go-parse ast $< > $@
