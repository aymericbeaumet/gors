## Known-failing fixture (skipped via `_` prefix)

`ptr := &acc; ptr.Add(1, 2)` lowers to a method call on
`GorsPtrGuard<'_, Accumulator>`, which doesn't expose Go-defined methods
like `Add`. rustc fails with E0599 (no method named `Add`). The lowering
needs to dispatch the call through the underlying `Accumulator` (via the
guard's `Deref`/`DerefMut`) or rewrite as a free-function call on the
lock guard's contents.
