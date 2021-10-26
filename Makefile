.PHONY: all
all:

.PHONY: test
test: $(patsubst %.go,%.ast,$(wildcard ./tests/files/*.go))

.PHONY: clean
clean:
	find . -name '*.ast' -exec rm -rf {} \;

%.ast: %.go
	GO111MODULE=off go run ./go-ast $< > $@
