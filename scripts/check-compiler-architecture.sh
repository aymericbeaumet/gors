#!/usr/bin/env bash

set -euo pipefail

failed=0

fail_on_matches() {
  local description="$1"
  local pattern="$2"
  shift 2

  local matches
  if matches="$(rg -n --glob '*.rs' --glob '*.toml' --glob '*.ts' --glob '*.mjs' \
    "${pattern}" "$@")"; then
    printf '%s\n%s\n' "${description}" "${matches}" >&2
    failed=1
  fi
}

for obsolete in \
  gors-builtin \
  gors/src/compiler/backend \
  gors/src/compiler/rust_lowering \
  gors/src/mapping \
  www/wasm/pkg-threads
do
  if [[ -e "${obsolete}" ]]; then
    printf 'obsolete compiler path still exists: %s\n' "${obsolete}" >&2
    failed=1
  fi
done

fail_on_matches \
  'legacy compiler modules or imports are forbidden:' \
  'compiler::(ir|typeinfer|passes)|mod (ir|typeinfer|passes)' \
  gors/src gors-cli/src www/wasm fuzz/src

fail_on_matches \
  'generated-Rust resolver/cache concepts are forbidden:' \
  'resolver.?cache|resolved.?module|partial.?declaration|type.?environment.?cache' \
  gors/src gors-cli/src www/wasm fuzz/src

fail_on_matches \
  'post-syntax or fallback semantic lowering is forbidden:' \
  'post.?syn|rust.?ast.?pass|ast.?to.?syn|fallback.?lower' \
  gors/src

fail_on_matches \
  'compiler semantic thread-local state is forbidden:' \
  'thread_local!' \
  gors/src/compiler gors/src/resolve

rayon_files="$(rg -l 'rayon::|ThreadPoolBuilder|into_par_iter' \
  gors/src/compiler --glob '*.rs' || true)"
while IFS= read -r source; do
  [[ -z "${source}" ]] && continue
  case "${source}" in
    gors/src/compiler/scheduler.rs) ;;
    *)
      printf 'compiler parallel work must use the one host scheduler: %s\n' "${source}" >&2
      failed=1
      ;;
  esac
done <<< "${rayon_files}"

fail_on_matches \
  'tracked compiler queries must not create or submit nested worker jobs:' \
  'rayon::|ThreadPoolBuilder|into_par_iter|std::thread::spawn' \
  gors/src/compiler/db

fail_on_matches \
  'high-level parser products must not leak source or publish static ASTs:' \
  "Box::leak|ast::File<'static>|merge_files" \
  gors/src/parser

fail_on_matches \
  'deleted compiler/runtime/browser paths must not be referenced:' \
  'gors[_-]builtin|wasm-threads|pkg-threads|compile_program_multi' \
  Cargo.toml gors gors-cli www fuzz

syn_files="$(rg -l 'syn::|quote!|parse_quote!' gors/src/compiler --glob '*.rs' || true)"
while IFS= read -r source; do
  [[ -z "${source}" ]] && continue
  case "${source}" in
    gors/src/compiler/emit.rs|gors/src/compiler/mod.rs) ;;
    *)
      printf 'semantic compiler module depends on terminal Rust syntax: %s\n' "${source}" >&2
      failed=1
      ;;
  esac
done <<< "${syn_files}"

if [[ -f gors/src/compiler/emit.rs ]]; then
  if matches="$(rg -n '(^|[^[:alnum:]_])(hir|mir|lowering)::|super::(hir|mir|lowering)|crate::compiler::(hir|mir|lowering)' \
    gors/src/compiler/emit.rs)"; then
    printf '%s\n%s\n' \
      'terminal emission must consume Rust IR only:' \
      "${matches}" >&2
    failed=1
  fi
  if matches="$(rg -n 'function\.name|__gors_fn_|[=!]=[[:space:]]*"main"' \
    gors/src/compiler/emit.rs)"; then
    printf '%s\n%s\n' \
      'terminal emission must render verified symbols and linkage without Go-name inference:' \
      "${matches}" >&2
    failed=1
  fi
fi

fail_on_matches \
  'post-semantic compiler stages must use stage-neutral diagnostics:' \
  '(^|[^[:alnum:]_])semantic::|super::semantic|crate::compiler::semantic' \
  gors/src/compiler/mir \
  gors/src/compiler/lowering \
  gors/src/compiler/rust_ir \
  gors/src/compiler/emit.rs

if [[ -f gors/src/compiler/mod.rs ]]; then
  production_facade="$({
    sed -n '/^fn compile_program_impl(/,/^}/p' gors/src/compiler/mod.rs
  } || true)"
  if matches="$(rg -n 'semantic::|mir::|lowering::|lower_to_(hir|mir|rust_ir)|compile_file' \
    <<< "${production_facade}")"; then
    printf '%s\n%s\n' \
      'production compile_program_impl must delegate only through CompilerSession:' \
      "${matches}" >&2
    failed=1
  fi
fi

fail_on_matches \
  'CLI and Wasm production callers must not bypass CompilerSession/facade compilation:' \
  'compile_file_to_rust_syntax|lower_to_(hir|mir|rust_ir)|compiler::db::CompilerDatabase|CompilerDatabase::' \
  gors-cli/src \
  www/wasm

fail_on_matches \
  'stateless Wasm build_rust exports are forbidden; retain GorsCompiler::build_rust only:' \
  '^pub[[:space:]]+fn[[:space:]]+build_rust|^export[[:space:]].*build_rust|^[[:space:]]*build_rust\??:[[:space:]]*\(' \
  www/wasm \
  www/gors-wasm-loader.ts

fail_on_matches \
  'generic or newline-fused runtime print ABI entry points are forbidden:' \
  'PrintlnEmpty|PrintValue|PrintlnValue|PrintlnGoString|print_value|println_value|println_empty|println_go_string' \
  gors/src/compiler \
  gors-runtime/src

exit "${failed}"
