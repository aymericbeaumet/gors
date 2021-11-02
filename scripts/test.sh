#!/bin/sh

cd "$(cd -- "$(dirname "$0")/.." >/dev/null 2>&1 ; pwd -P)"

RUST_BIN='./target/debug/go2rust'
GO_BIN='./go/go'

# make sure our go reference repositories are up-to-date
git submodule update --init

# prepare the working directory
rm -rf .tests
mkdir .tests

# find all go files
find . -name '*.go' > .tests/index

# generate tokens with the Go implementation + from the Rust implementation
while read gofile; do
  echo "$gofile"

  go_tokens="${gofile%.go}"
  go_tokens=".tests/${go_tokens//\//--}.tokens-go"
  "$GO_BIN" tokens "$gofile" > "$go_tokens"

  rust_tokens="${gofile%.go}"
  rust_tokens=".tests/${rust_tokens//\//--}.tokens-rust"
  "$RUST_BIN" tokens "$gofile" > "$rust_tokens"

  git --no-pager diff --no-index "$go_tokens" "$rust_tokens" || exit 1
done < .tests/index

# TODO: AST
