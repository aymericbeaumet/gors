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
if [ "$2" = 'last' ]; then
  echo "$last" >.tests/_index
else
  for dir in tests .repositories; do
    find "$dir" -type f -name '*.go' | sort > .tests/_index
  done
fi

# keep track of how far we are
i=0
total="$(wc -l < .tests/_index)"

# compare tokens generated with the Go/Rust implementation
while read go_source; do
  i=$((i+1))

  echo "$go_source" > .tests/_last
  printf "%0.2d%% %s\n" "$((i*100/$total))" "$go_source"

  go_tokens=".tests/${go_source//\//--}.tokens-go"
  "$GO_BIN" tokens "$go_source" > "$go_tokens" || {
    echo "$go_source" >> .tests/_failures
    continue
  }

  rust_tokens=".tests/${go_source//\//--}.tokens-rust"
  "$RUST_BIN" tokens "$go_source" > "$rust_tokens" || exit 1

  git diff --no-index "$go_tokens" "$rust_tokens" || exit 2
done < .tests/_index

echo 'Success!'

if [ -r .tests/_failures ]; then
  echo
  echo 'WARNING: these files were skipped as the Go parser failed to process them:'
  cat .tests/_failures
fi
