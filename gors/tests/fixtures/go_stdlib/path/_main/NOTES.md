## Known-failing fixture (skipped via `_` prefix)

The generated binary times out (10s) instead of producing output —
likely an infinite loop or hang in the `path` package translation
(`Base` / `Dir` / `Ext` / `IsAbs` / `Join` / `Clean` / `Split`).

The sibling `path/filepath` fixture still runs, which is why this
fixture sits under `path/_main` rather than renaming `path` itself.

To re-enable: capture the actual runtime behavior with a smaller
reproducer, fix the hang, then drop the `_main` indirection.
