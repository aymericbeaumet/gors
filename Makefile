.PHONY: all
all:

.PHONY: test
test: $(patsubst %.go,%.gofmt,$(wildcard ./tests/files/*.go)) $(patsubst %.go,%.goast,$(wildcard ./tests/files/*.go))

%.goast: %.go
	astextract $< > $@

%.gofmt: %.go
	gofmt -s $< > $@
