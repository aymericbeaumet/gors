#!/bin/sh

cd -- "$(dirname "$0")/.." >/dev/null 2>&1
git submodule update --init
RUST_BIN="./target/${1:=release}/gors"
GO_BIN='./go/go'

# read the last file tested (if any)
last="$(cat .tests/_last 2>/dev/null)"

# prepare the working directory
rm -rf ".tests/"
mkdir ".tests/"

# define the go files we want to test against
case "$2" in
  'tdd')
    for dir in tests; do
      find "$dir" -type f -name '*.go' >> .tests/_index
    done
    ;;
  'last')
    echo "$last" >> .tests/_index
    ;;
  *)
    for dir in tests .repositories; do
      find "$dir" -type f -name '*.go' >> .tests/_index
    done
    ;;
esac

# keep track of progress
i=0
total="$(wc -l < .tests/_index)"

# compare outputs from the Go/Rust implementation
while read go_source; do
  i=$((i+1))

  echo "$go_source" > .tests/_last
  printf "%0.2d%% %s\n" "$((i*100/$total))" "$go_source"

  # compare tokens outputs

  go_tokens=".tests/${go_source//\//--}.tokens-go"
  "$GO_BIN" tokens "$go_source" > "$go_tokens" || { echo "$go_source" >> .tests/_skipped; continue; }

  rust_tokens=".tests/${go_source//\//--}.tokens-rust"
  "$RUST_BIN" tokens "$go_source" > "$rust_tokens" || exit 1

  git diff --no-index "$go_tokens" "$rust_tokens" || exit 2

  # compare ast outputs

  go_ast=".tests/${go_source//\//--}.ast-go"
  "$GO_BIN" ast "$go_source" > "$go_ast" || exit 3

  rust_ast=".tests/${go_source//\//--}.ast-rust"
  "$RUST_BIN" ast "$go_source" > "$rust_ast" || exit 4

  git diff --no-index "$go_ast" "$rust_ast" || exit 5

done < .tests/_index

echo 'Success!'

if [ -r .tests/_skipped ]; then
  echo
  echo 'WARNING: these files were skipped as Go failed to process them:'
  cat .tests/_skipped
fi
