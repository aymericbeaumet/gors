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
  gors/src/compiler/db/provenance.rs \
  gors/src/compiler/rust_lowering \
  gors/src/compiler/source \
  gors/src/mapping \
  gors/src/parser/program.rs \
  www/wasm/pkg-threads
do
  if [[ -e "${obsolete}" ]]; then
    printf 'obsolete compiler path still exists: %s\n' "${obsolete}" >&2
    failed=1
  fi
done

fail_on_matches \
  'frontend source layers must not depend on the semantic compiler:' \
  'crate::compiler' \
  gors/src/source \
  gors/src/scanner \
  gors/src/parser \
  gors/src/token

fail_on_matches \
  'legacy mixed or arithmetic-rebased compiler provenance is forbidden:' \
  'SourceSpan|FunctionProvenance|make_function_relative|rebase_function_diagnostic' \
  gors/src/compiler \
  gors/tests \
  gors-cli/src \
  www/wasm \
  fuzz/src \
  --glob '!tests.rs'

fail_on_matches \
  'HIR, Go MIR, and Rust IR fields must carry SourceRef through `source`, not `span`:' \
  '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?span[[:space:]]*:' \
  gors/src/compiler/hir.rs \
  gors/src/compiler/mir \
  gors/src/compiler/rust_ir

fail_on_matches \
  'legacy compiler modules or imports are forbidden:' \
  'compiler::(ir|typeinfer|passes)|mod (ir|typeinfer|passes)' \
  gors/src gors-cli/src www/wasm fuzz/src

fail_on_matches \
  'generated-Rust resolver/cache concepts are forbidden:' \
  'resolver.?cache|resolved.?module|partial.?declaration|type.?environment.?cache' \
  gors/src gors-cli/src www/wasm fuzz/src

fail_on_matches \
  'legacy mixed generated-Rust/terminal cache identities are forbidden:' \
  'GORS_(CLI_ABI|COMPILER)_FINGERPRINT|(^|[^[:alnum:]_])(COMPILER_FINGERPRINT|CacheRequest|RustcArgs)([^[:alnum:]_]|$)' \
  gors/src \
  gors/build.rs \
  gors/build \
  gors-cli \
  perf/perf_harness \
  --glob '*.py'

fail_on_matches \
  'host-native terminal codegen is forbidden; CPU and features must be explicit and portable:' \
  'target-cpu=native|(^|[^[:alnum:]_])(target_cpu|TARGET_CPU)[^;]*"native"' \
  gors/src/compiler \
  gors/src/printer \
  gors/build \
  gors-cli/src \
  perf/perf_harness \
  --glob '*.py' \
  --glob '!**/tests.rs'

fail_on_matches \
  'post-syntax or fallback semantic lowering is forbidden:' \
  'post.?syn|rust.?ast.?pass|ast.?to.?syn|fallback.?lower' \
  gors/src

fail_on_matches \
  'authoritative compiler stages must not expose a second public orchestration API:' \
  '^[[:space:]]*pub fn (compile_file|lower_to_hir|lower_to_mir|lower_to_rust_ir|emit_rust_ir)' \
  gors/src/compiler

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
  'semantic queries must consume path-independent SourceContent only:' \
  'SourceSnapshot|diagnostic_snapshot|source_snapshot' \
  gors/src/compiler/db/queries.rs

fail_on_matches \
  'raw Salsa storage snapshots must remain compiler-scheduler-internal:' \
  '^[[:space:]]*pub fn snapshot' \
  gors/src/compiler/db/mod.rs

fail_on_matches \
  'whole-session source snapshot rollback is forbidden; journal input deltas:' \
  'previous_sources|restore_source_snapshot|rollback_install' \
  gors/src/compiler/session.rs \
  gors/src/compiler/db

fail_on_matches \
  'bootstrap sessions must materialize only the entry manifest:' \
  '\.packages\(\)' \
  gors/src/compiler/session.rs

if ! rg -q \
  '^[[:space:]]*pub\(in crate::compiler\) fn snapshot\(&self\) -> CompilerDatabaseSnapshot' \
  gors/src/compiler/db/mod.rs; then
  printf '%s\n' \
    'raw Salsa snapshot constructor visibility drifted outside the compiler scheduler boundary' \
    >&2
  failed=1
fi

snapshot_callers="$(rg -l 'database\.snapshot\(\)' \
  gors/src/compiler --glob '*.rs' || true)"
while IFS= read -r source; do
  [[ -z "${source}" ]] && continue
  case "${source}" in
    gors/src/compiler/session/prewarm.rs) ;;
    *)
      printf 'raw Salsa snapshots may only be created by the prewarm scheduler: %s\n' \
        "${source}" >&2
      failed=1
      ;;
  esac
done <<< "${snapshot_callers}"

fail_on_matches \
  'high-level parser products must not leak source or publish static ASTs:' \
  "Box::leak|ast::File<'static>|merge_files" \
  gors/src/parser

fail_on_matches \
  'parser-owned program/package graphs and pre-compiler parsing are forbidden:' \
  'ParsedProgram|ParsedPackage|PathParseError|parse_program(_files|_from_source)?' \
  gors/src \
  gors-cli/src \
  www/wasm \
  gors/tests

fail_on_matches \
  'workspace loading must consume caller-owned identity, never synthesize it:' \
  'WorkspaceKey::|COMMAND_LINE_WORKSPACE|"command-line"' \
  gors/src/workspace/loader.rs

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
  www/wasm \
  fuzz/src

if [[ -f gors-cli/src/program.rs ]]; then
  warm_executable_prefix="$(
    awk '
      /^[[:space:]]*pub fn ensure_executable\(/ { inside = 1 }
      inside && /^[[:space:]]*self\.ensure_generated\(/ { exit }
      inside { print }
    ' gors-cli/src/program.rs
  )"
  if [[ -z "${warm_executable_prefix}" ]] || \
    ! rg -q 'admit_executable' <<< "${warm_executable_prefix}" || \
    ! rg -q 'return Ok\(executable\)' <<< "${warm_executable_prefix}"; then
    printf '%s\n' \
      'program cache must expose an explicit warm executable admission branch' >&2
    failed=1
  fi
  if matches="$(rg -n \
    'resolve_runtime\(|generated_files_are_current|embedded_runtime_artifact|\.materialize\(|NATIVE_RUNTIME_RUST_TOOLCHAIN|rustup[[:space:]]+(run|which)' \
    <<< "${warm_executable_prefix}" || true)" && \
    [[ -n "${matches}" ]]; then
    printf '%s\n%s\n' \
      'warm executable admission must precede generated-file, runtime, and toolchain resolution:' \
      "${matches}" >&2
    failed=1
  fi
fi

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

fail_on_matches \
  'legacy Rust-IR operation shadows and bundled print plans are forbidden:' \
  'RuntimePrint|PrintStep|print_plan|enum[[:space:]]+(BinaryOp|UnaryOp)' \
  gors/src/compiler/rust_ir \
  gors/src/compiler/emit.rs \
  gors/src/compiler/lowering

fail_on_matches \
  'runtime symbols outside the ABI catalog are forbidden in Rust-IR consumers:' \
  'go_string_from_(bytes|static)|concat_go_strings|int_(div|rem|shl|shr)|print_(bool|i64|space|newline|go_string)' \
  gors/src/compiler/emit.rs \
  gors/src/compiler/rust_ir/effects.rs \
  gors/src/compiler/fingerprint/rust_ir.rs

fail_on_matches \
  'obsolete runtime arithmetic and empty-print helpers are forbidden:' \
  'fn[[:space:]]+(int_add|int_sub|int_mul|int_neg|print_empty)' \
  gors-runtime/src \
  gors/src/compiler/emit.rs

fail_on_matches \
  'runtime source bundling and relative runtime modules are forbidden:' \
  '(^|[^[:alnum:]_])(RUNTIME_SOURCE|RUNTIME_MODULE_NAME)([^[:alnum:]_]|$)|include_str!\([^)]*gors-runtime|crate::__gors_runtime|^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+__gors_runtime|#\[path[[:space:]]*=[^]]*__gors_runtime' \
  gors/src \
  gors-cli/src \
  www/wasm \
  fuzz/src

fail_on_matches \
  'the browser compiler must transport dependencies without selecting target artifacts:' \
  'RuntimeArtifactManifest|RuntimeArtifactFormat|RuntimeLinkPlan|RuntimeLinkRequest|RustRlib(Producer|Compatibility)|embedded_runtime_artifact|\.materialize\(' \
  www/wasm

fail_on_matches \
  'native runtime provider production must not inherit Cargo RUSTC:' \
  'required_environment_path\("RUSTC"\)|std::env::var(_os)?\("RUSTC"\)' \
  gors/build/runtime_artifact.rs

for native_runtime_toolchain_owner in \
  gors/build/runtime_artifact.rs \
  gors-cli/src/runtime_link.rs
do
  if ! rg -q 'NATIVE_RUNTIME_RUST_TOOLCHAIN' "${native_runtime_toolchain_owner}"; then
    printf 'native runtime boundary does not use the ABI-owned rustup toolchain: %s\n' \
      "${native_runtime_toolchain_owner}" >&2
    failed=1
  fi
done

if matches="$(rg -n 'NATIVE_RUNTIME_RUST_TOOLCHAIN|Command::new\("rustup"\)' \
  gors-cli/src/rustc.rs || true)" && [[ -n "${matches}" ]]; then
  printf '%s\n%s\n' \
    'terminal rustc actions must execute the exact probed compiler path directly:' \
    "${matches}" >&2
  failed=1
fi

for terminal_linker in \
  gors-cli/src/rustc.rs \
  gors/src/compiler/tests.rs \
  gors/src/printer/mod.rs \
  gors/tests/common/runner.rs \
  perf/perf_harness/runtime_link.py \
  www/v86/rootfs/gors-compile
do
  if [[ ! -f "${terminal_linker}" ]] || ! rg -q -- '--extern' "${terminal_linker}"; then
    printf 'terminal Rust linker does not require the external runtime: %s\n' \
      "${terminal_linker}" >&2
    failed=1
  fi
done

fail_on_matches \
  'runtime ABI identity must come from the typed contract, never source scraping or build-script environment strings:' \
  'GORS_RUNTIME_ABI_VERSION|GORS_RUNTIME_ABI_ID|RUNTIME_ABI_ID|read_runtime_abi_id|runtimeAbiVersion|runtime_abi_version' \
  gors/build.rs \
  gors/src \
  gors-cli/src \
  gors-runtime/src \
  gors-runtime-abi/src \
  perf \
  www/wasm \
  fuzz/src

exit "${failed}"
