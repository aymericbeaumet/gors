## Known-failing fixture (skipped via `_` prefix)

`ints.Set(5)` calls `Set(*Holder[T])` on a non-addressable value receiver,
which Go silently address-of's. The current lowering emits
`compile_error!("pointer receiver is not addressable")` at
`gors/src/compiler/mod.rs` (`pointer_receiver_arg_expr_from_owned`) because
there is no `GorsPtr<T>` constructor that can share storage with a plain
`&mut T` lvalue.

To re-enable: either declare locals that later become pointer-receiver
receivers as `GorsPtr<T>` at declaration time, or add an lvalue-sharing
`GorsPtr` constructor.
