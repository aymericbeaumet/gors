## Known-failing fixture (skipped via `_` prefix)

`rustc` fails to compile the generated `main.rs` (exit status 1). The
fixture exercises `base64.StdPadding`, `base64.NoPadding`,
`StdEncoding.EncodedLen`/`DecodedLen`, and the raw-encoding equivalents,
so the lowering issue is somewhere inside the `encoding/base64`
package translation.

To re-enable: run the stdlib integration test without `FAIL_FAST` to
capture the actual `rustc` diagnostic, then fix the underlying
lowering in `encoding/base64`.
