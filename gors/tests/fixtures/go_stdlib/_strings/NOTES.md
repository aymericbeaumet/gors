## Known-failing fixture (skipped via `_` prefix)

`strings.Builder` calls pointer-receiver methods (e.g. `builder.Grow(8)`,
`builder.WriteByte(':')`) on a non-addressable value receiver, which the
current lowering can't make addressable without dedicated `GorsPtr<T>`
storage. The generated `main.rs` emits
`compile_error!("pointer receiver is not addressable")` and fails rustc.

To re-enable: same underlying fix as `go_programs/_generic_type_method`.
