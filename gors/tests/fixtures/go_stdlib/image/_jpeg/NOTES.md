## Known-failing fixture (skipped via `_` prefix)

Same compiler panic as `image/_gif`: `gors/src/compiler/mod.rs:13115`
fires `"color.Palette" is not a valid Ident` because a qualified Go
type leaks into a position that expects a bare Rust ident.

To re-enable: see `image/_gif/NOTES.md`.
