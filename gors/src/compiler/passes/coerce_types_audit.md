# coerce_types hardcode audit

Date: 2026-06-02

Scope: production matches from:

```bash
rtk rg -n "fmt|strconv|reflect|strings|sort|unicode|encoding|archive|cmp|TODO|hack|special|hardcode" gors/src/compiler gors/src/resolve gors-builtin/src
```

The intent is to replace package/function-name coercions with compiler facts:
expected Go type, call signature, receiver semantics, lvalue/rvalue role, and
runtime primitive ownership contracts. Tests and ordinary Rust names such as
`std::fmt`, `sort_unstable`, and diagnostics are not tracked here.

## Categories

- `runtime primitive`: generated support for Go builtins, runtime contracts, or
  host resources.
- `generated-language rule`: a generic lowering/ownership rule already expressed
  in terms of Rust or Go shapes rather than a stdlib symbol.
- `stdlib workaround`: package/function-name logic that should be replaced by a
  semantic compiler rule.
- `formatting artifact`: generated-format cleanup or report/debug wording.

## coerce_types.rs

| Area | Current trigger | Category | Generic rule to implement |
| --- | --- | --- | --- |
| Function body replacement | `newPrinter` plus `ppFree` token search | stdlib workaround | Lower package/function bodies from Go semantics without replacing named bodies. The missing rule is a typed initialization/aliasing model for package-level pooled state and self-referential buffers, driven by field types and assignment targets. |
| Method body removal | `free` plus `ppFree` token search | stdlib workaround | Model unsupported host/runtime pool operations through a runtime primitive or generic sync-pool lowering, not by deleting a named method body. |
| Method body replacement | `fmtString` plus `fmtQ`/`fmtSx` token search | stdlib workaround | Use call signatures and supported formatting primitives to lower string formatting branches; unsupported branches should be represented as explicit missing runtime primitives. |
| Method body replacement | `padString` plus `RuneCountInString` token search | stdlib workaround | Drive string/slice borrowing from expected parameter types and receiver mutation, so UTF-8 helper calls do not force named-method rewrites. |
| Reflection fallback pruning | `printArg`, `printValue`, `fmtPointer`, `reflect` token searches | stdlib workaround | Represent reflect/type-switch support as compiler/runtime semantic facts; prune only unreachable IR/control-flow branches, not branches selected by generated token text. |
| Method arg coercion | method names `Write`, `argNumber`, `write`, `writeString`, `printArg`, `printValue` | stdlib workaround | Coerce method arguments using resolved receiver type plus method signature, including ownership requirements for interface values and mutable buffers. |
| `Box::new` field clone | field argument to `Box::new` | generated-language rule | Preserve Go value-copy semantics when boxing addressable field values. Keep this generic but move to expected-type expression lowering if possible. |
| `builtin::append` first/second args | builtin append path | runtime primitive | Destination is an lvalue/owned slice update and appended element must be value-copied. This is a Go builtin contract. |
| UTF-8 append rune | `unicode__utf8::AppendRune` / `utf8::AppendRune` | stdlib workaround | First-argument move/take should come from signature and lvalue role for any function that returns an updated destination slice. |
| Sort helper clone | `fmtsort::Sort` | stdlib workaround | Argument value-copy should follow the callee parameter type and whether the argument is reused, not the helper name. |
| Reflect value coercion | `reflect::ValueOf` | stdlib workaround | Boxing/moving interface arguments should come from expected parameter type `any` and argument lvalue/rvalue role. |
| Local helpers | `parsenum`, `intFromArg`, `getField` | stdlib workaround | These are generated helper calls from stdlib lowering; their ownership contracts should be expressed in generated signatures or helper metadata. |
| Borrow first arg | UTF-8 and reflect `TypeOf` path lists | stdlib workaround | Borrowing should be signature-driven for any function expecting string/byte-slice references, not selected by package/function name. |
| Local init cloning | identifiers `value`, `f`, field `fmtFlags` | stdlib workaround | Cloning should follow Go value-copy semantics, binding type, and later use/move analysis, not chosen by local variable or field name. |
| Format flush insertion | method calls `self.printArg` / `self.printValue` | stdlib workaround | Flush side effects should be represented as method/lowering semantics for receiver-buffer aliasing, or removed by correctly modeling the buffer alias. |

## Other production hardcodes

| File | Current trigger | Category | Generic rule to implement |
| --- | --- | --- | --- |
| `gors/src/compiler/mod.rs` | `reflect` module replacement in post-prune helpers | runtime primitive | Reflect support is currently a runtime primitive boundary; keep isolated until generic reflect IR/runtime support exists. |
| `gors/src/compiler/mod.rs` | `os.Stdout`/`os.File` host-resource replacement | runtime primitive | Host resources may be injected, but must preserve unrelated compiled stdlib items. |
| `gors/src/compiler/mod.rs` | `sort.Slice*` custom lowering | stdlib workaround | Lower function-typed callback arguments and slice mutation generically, then compile the stdlib implementation normally. |
| `gors/src/compiler/mod.rs` | `strconv.AppendFloat` custom lowering to `builtin::append_float` | stdlib workaround | Implement the missing formatting/runtime primitive behind generic function lowering or type-directed expected arguments. |
| `gors/src/compiler/mod.rs` | `reflect.TypeOf(x).Kind() == reflect.K` detection | runtime primitive | This is a reflect runtime boundary; future work should expose it as IR reflect-kind operation instead of AST pattern matching. |
| `gors/src/resolve/mod.rs` | injected `pp` `State` impl and `__gors_flush_fmt` | stdlib workaround | Interface implementation and receiver-buffer aliasing should be produced by generic method/interface lowering, not resolver post-processing. |
| `gors-builtin/src/lib.rs` | predeclared print/println, interface, reflect-kind helpers | runtime primitive | Builtin language/runtime support is valid, but must not implement stdlib package behavior. |

## Replacement order

1. Signature-driven call argument ownership in `coerce_types.rs`.
2. Receiver-aware method argument coercion in `coerce_types.rs`.
3. Local binding cloning from Go value-copy semantics.
4. Generated helper ownership metadata for currently named local helpers.
5. Resolver/compiler post-prune fmt helper removal after receiver-buffer aliasing
   is represented semantically.

## Completed removals

| Area | Replacement |
| --- | --- |
| `strconv` string value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now clones path, field, and index arguments according to the callee's generated `String`/cloneable value parameter types. |
| `slices::Sort` mutable argument borrowing in `coerce_types.rs` | Cross-module mutable-reference call analysis now borrows arguments according to generated callee `&mut` parameter types. |
