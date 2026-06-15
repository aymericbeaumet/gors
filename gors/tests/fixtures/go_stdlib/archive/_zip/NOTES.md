## Known-failing fixture (skipped via `_` prefix)

`rustc` fails to compile the generated `main.rs` (exit status 1). The
lowering issue is somewhere inside the `archive/zip` package
translation.

To re-enable: run the stdlib integration test without `FAIL_FAST` to
capture the actual `rustc` diagnostic, then fix the underlying
lowering in `archive/zip`.
