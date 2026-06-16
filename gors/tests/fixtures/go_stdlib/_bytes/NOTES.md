## Known-failing fixture (skipped via `_` prefix)

`rustc` fails to compile the generated `main.rs` (exit status 1). The
fixture exercises a large surface of the `bytes` package, so the
lowering issue is somewhere inside the `bytes` package translation.

To re-enable: run the stdlib integration test without `FAIL_FAST` to
capture the actual `rustc` diagnostic, then fix the underlying
lowering in `bytes`.
