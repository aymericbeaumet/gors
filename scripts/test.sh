#!/bin/sh

ROOT_DIR="$(cd -- "$(dirname "$0")/.." >/dev/null 2>&1 ; pwd -P)"
RUST_BIN="./target/${1:=release}/go2rust"
GO_BIN='./go/go'

# fix root directory
cd "$ROOT_DIR"

# make sure our go reference repositories are up-to-date
git submodule update --init

# read the last file tested (if any)
last="$(cat .tests/_last)"

# prepare the working directory
rm -rf "$ROOT_DIR/.tests"
mkdir "$ROOT_DIR/.tests"

# find all go files in the reference repositories
if [ "$2" = 'last' ]; then
  echo "$last" > .tests/_index
else
  find . -name '*.go' | cut -c3- > .tests/_index
fi

# generate tokens with the Go implementation + from the Rust implementation
while read go_source; do
  go_tokens=".tests/${go_source//\//--}.tokens-go"
  "$GO_BIN" tokens "$go_source" > "$go_tokens" 2>/dev/null || continue

  echo "$go_source" > .tests/_last
  echo ">> $go_source"

  rust_tokens=".tests/${go_source//\//--}.tokens-rust"
  "$RUST_BIN" tokens "$go_source" > "$rust_tokens" || exit 1

  git diff --no-index "$go_tokens" "$rust_tokens" || exit 2
done < .tests/_index

# TODO: AST
