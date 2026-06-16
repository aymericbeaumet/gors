## Known-failing fixture (skipped via `_` prefix)

Compiler panics at `gors/src/compiler/mod.rs:13115` with
`"buffer.Buffer" is not a valid Ident` — the same dotted-qualified-type
class of bug as `image/_gif`, just hit through `log/slog/internal/buffer`
instead of `image/color`.

To re-enable: fix the underlying ident-construction call site (see
`image/_gif/NOTES.md`) and drop the `_` prefix.
