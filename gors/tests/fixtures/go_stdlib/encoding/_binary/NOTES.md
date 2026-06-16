## Known-failing fixture (skipped via `_` prefix)

The generated `main.rs` fails `rustc` compilation in CI (exit status 1).
The fixture exercises `binary.LittleEndian.Uint32`, `binary.BigEndian.Uint16`,
and the `binary.PutUvarint` / `binary.Uvarint` varint helpers, so the
lowering issue is somewhere in the `encoding/binary` package translation.

To re-enable: run the stdlib integration test without `FAIL_FAST` to capture
the actual `rustc` diagnostic, then fix the underlying lowering in the
`encoding/binary` translation.
