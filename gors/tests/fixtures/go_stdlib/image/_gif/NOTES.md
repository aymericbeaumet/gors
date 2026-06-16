## Known-failing fixture (skipped via `_` prefix)

The compiler panics at `gors/src/compiler/mod.rs:13115` with
`"color.Palette" is not a valid Ident`. The lowering passes a qualified
Go type (`image/color.Palette`) into a path that expects a bare Rust
identifier. The same panic surfaces for `image/_jpeg` and `image/_png`.

To re-enable: teach the identifier-building call site to handle dotted
Go type names, then drop the `_` prefix from this and the sibling
`image/_jpeg`, `image/_png` directories.
