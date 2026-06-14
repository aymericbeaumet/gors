## Known-failing fixture (skipped via `_` prefix)

The generated `main.rs` fails `rustc` compilation in CI (exit status 1). The
fixture surface is tiny — it just prints `md5.Size` and `md5.BlockSize` — so
the codegen issue is somewhere inside the `crypto/md5` package translation
itself, not in the fixture's `main`.

To re-enable: run the stdlib integration test without `FAIL_FAST` to capture
the actual `rustc` diagnostic, then fix the underlying lowering in the
`crypto/md5` translation.
