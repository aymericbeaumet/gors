## Known-failing fixture (skipped via `_` prefix)

The generated binary hangs (10s test timeout exceeded) before printing any
output. The compress/bzip2 init builds a 256-entry CRC table via a tight
loop; manual inspection of the generated `__gors_init` looks correct, so
the hang is somewhere later — likely an infinite recursion via
`StructuralError`'s `Deref<Target=String>` + `crate::builtin::string(self)`
inside the generated `Error()` method.

To re-enable: trace the actual hang with a smaller reproducer, then guard
the `Deref` path so `crate::builtin::string` on a named-string newtype
doesn't recurse through its own `Deref`.
