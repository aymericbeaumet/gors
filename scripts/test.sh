#!/bin/sh

ROOT_DIR="$(cd -- "$(dirname "$0")/.." >/dev/null 2>&1 ; pwd -P)"
RUST_BIN='./target/debug/go2rust'
GO_BIN='./go/go'

# make sure our go reference repositories are up-to-date
git submodule update --init

# prepare the working directory
rm -rf "$ROOT_DIR/.tests"
mkdir "$ROOT_DIR/.tests"

# find all go files
find . -name '*.go' | cut -c3- > "$ROOT_DIR/.tests/.index"

# generate tokens with the Go implementation + from the Rust implementation
while read gofile; do
  echo ">> $ROOT_DIR/$gofile"

  ln -sf "$ROOT_DIR/$gofile" "$ROOT_DIR/.tests/${gofile//\//--}"

  go_tokens="$ROOT_DIR/.tests/${gofile//\//--}.tokens-go"
  "$GO_BIN" tokens "$gofile" > "$go_tokens"

  rust_tokens="$ROOT_DIR/.tests/${gofile//\//--}.tokens-rust"
  "$RUST_BIN" tokens "$gofile" > "$rust_tokens"

  git --no-pager diff --no-index "$go_tokens" "$rust_tokens" || exit 1
done < .tests/.index

# TODO: AST
