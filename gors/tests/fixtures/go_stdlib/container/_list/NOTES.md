## Known-failing fixture (skipped via `_` prefix)

`rustc` fails to compile the generated `main.rs` (exit status 1). The
fixture exercises most of the `container/list` API — `New`, `Front`,
`Back`, `PushBack`/`PushFront`, `InsertAfter`/`InsertBefore`,
`Move*`, `Remove`, `Push*List`, `Init` — so the lowering issue is
somewhere inside the `container/list` package translation, not
specific to a single call site.

To re-enable: run the stdlib integration test without `FAIL_FAST` to
capture the actual `rustc` diagnostic, then fix the underlying
lowering in `container/list`.
