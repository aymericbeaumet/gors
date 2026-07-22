#!/usr/bin/env bash

set -euo pipefail

readonly max_lines="${GORS_MAX_SOURCE_LINES:-1000}"
readonly inline_test_split_lines="${GORS_INLINE_TEST_SPLIT_LINES:-600}"
readonly cli_main_max_lines="${GORS_CLI_MAIN_MAX_LINES:-150}"

sources=()
while IFS= read -r source; do
  sources+=("${source}")
done < <(
  rg --files \
    -g '*.css' \
    -g '*.go' \
    -g '*.js' \
    -g '*.mjs' \
    -g '*.py' \
    -g '*.rs' \
    -g '*.sh' \
    -g '*.svelte' \
    -g '*.ts' \
    gors gors-cli gors-runtime fuzz perf scripts www \
    | rg -v '(^|/)(fixtures|node_modules|pkg|target|v86)(/|$)' \
    | LC_ALL=C sort -u
)

failed=0
for source in "${sources[@]}"; do
  lines="$(wc -l < "${source}" | tr -d ' ')"
  if ((lines > max_lines)); then
    printf '%s has %s lines; split it below the %s-line limit\n' \
      "${source}" "${lines}" "${max_lines}" >&2
    failed=1
  fi

  if [[ "${source}" == *.rs ]] \
    && ((lines > inline_test_split_lines)) \
    && rg -q '^mod tests \{' "${source}"; then
    printf '%s has inline tests at %s lines; move them to a sibling test module\n' \
      "${source}" "${lines}" >&2
    failed=1
  fi
done

cli_main_lines="$(wc -l < gors-cli/src/main.rs | tr -d ' ')"
if ((cli_main_lines > cli_main_max_lines)); then
  printf '%s has %s lines; keep command dispatch below the %s-line limit\n' \
    'gors-cli/src/main.rs' "${cli_main_lines}" "${cli_main_max_lines}" >&2
  failed=1
fi

if [[ -d gors/src/compiler/backend ]]; then
  printf '%s\n' 'gors/src/compiler/backend is redundant; stages belong directly under compiler' >&2
  failed=1
fi

if [[ -d gors/src/compiler/rust_lowering ]]; then
  printf '%s\n' 'gors/src/compiler/rust_lowering is obsolete; use gors/src/compiler/lowering' >&2
  failed=1
fi

if [[ -d gors/src/mapping ]]; then
  printf '%s\n' 'gors/src/mapping is obsolete; use gors/src/sourcemap' >&2
  failed=1
fi

exit "${failed}"
