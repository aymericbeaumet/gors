#!/bin/sh

ROOT_DIR="$(cd -- "$(dirname "$0")/.." >/dev/null 2>&1 ; pwd -P)"
RUST_BIN="$ROOT_DIR/target/debug/go2rust"
GO_BIN="$ROOT_DIR/go/go"

cd "$ROOT_DIR"
rm -rf .tests
mkdir -p .tests/samples

# add all the samples files
for i in samples/*; do
  ln -s "$ROOT_DIR/$i" "$ROOT_DIR/.tests/$i"
done

# format/simplify all the samples files
for i in .tests/samples/*; do
  gofmt -s "$ROOT_DIR/$i" > "$ROOT_DIR/${i%.go}-gofmt.go"
  goimports "$ROOT_DIR/$i" > "$ROOT_DIR/${i%.go}-goimports.go"
done

# generate tokens with the Go implementation + from the Rust implementation
cd "$ROOT_DIR/.tests/samples"
for i in ./*.go; do
  go_tokens="${i%.go}.tokens-go"
  "$GO_BIN" tokens "$i" > "$go_tokens"

  rust_tokens="${i%.go}.tokens-rust"
  "$RUST_BIN" tokens "$i" > "$rust_tokens"

  echo
  git --no-pager diff --no-index "$go_tokens" "$rust_tokens" && echo "CORRECT"
done

# TODO: AST
