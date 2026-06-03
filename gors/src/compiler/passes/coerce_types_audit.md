# coerce_types hardcode audit

Date: 2026-06-03

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

## coerce_types pass modules

| Area | Current trigger | Category | Generic rule to implement |
| --- | --- | --- | --- |
| Reflection fallback pruning in `structural_helpers.rs` | `printArg`, `printValue`, `fmtPointer`, `reflect` AST path/type-path checks | stdlib workaround | Represent reflect/type-switch support as compiler/runtime semantic facts; prune only unreachable IR/control-flow branches, not branches selected by generated token text. |
| `builtin::append` first/second args in `value_materialization.rs` | builtin append path | runtime primitive | Destination is an lvalue/owned slice update and appended element must be value-copied. This is a Go builtin contract. |
| Format flush insertion in `structural_helpers.rs` | receiver impls with generated `__gors_flush_fmt` hook calling `self.printArg` / `self.printValue` | stdlib workaround | Flush side effects should be represented as method/lowering semantics for receiver-buffer aliasing, or removed by correctly modeling the buffer alias. |

## Other production hardcodes

| File | Current trigger | Category | Generic rule to implement |
| --- | --- | --- | --- |
| `gors/src/compiler/mod.rs` | `reflect` module replacement in post-prune helpers | runtime primitive | Reflect support is currently a runtime primitive boundary; keep isolated until generic reflect IR/runtime support exists. |
| `gors/src/compiler/mod.rs` | `sync.Pool` module replacement in post-prune helpers | runtime primitive | Pooling is modeled as a minimal runtime primitive for host allocation reuse; keep it isolated and avoid package consumer rewrites. |
| `gors/src/compiler/mod.rs` | `os.Stdout`/`os.File` host-resource replacement | runtime primitive | Host resources may be injected, but must preserve unrelated compiled stdlib items. |
| `gors/src/compiler/mod.rs` | `reflect.TypeOf(x).Kind() == reflect.K` detection | runtime primitive | This is a reflect runtime boundary; future work should expose it as IR reflect-kind operation instead of AST pattern matching. |
| `gors/src/resolve/mod.rs` | injected `pp` `State` impl and `__gors_flush_fmt` | stdlib workaround | Interface implementation and receiver-buffer aliasing should be produced by generic method/interface lowering, not resolver post-processing. |
| `gors-builtin/src/lib.rs` | predeclared print/println, interface, reflect-kind helpers | runtime primitive | Builtin language/runtime support is valid, but must not implement stdlib package behavior. |

## Replacement order

1. Move `value_materialization.rs` rules into expected-type expression lowering
   and lvalue lowering where the Go semantic facts are already available.
2. Replace `structural_helpers.rs` reflection pruning and fmt flush insertion
   with semantic reflect support and receiver-buffer aliasing.
3. Resolver/compiler post-prune fmt helper removal after receiver-buffer aliasing
   is represented semantically.

## Completed removals

| Area | Replacement |
| --- | --- |
| Rust-name-driven `len`/`cap` post-pass casts in `coerce_types.rs` | Builtin lowering now emits Go `int` casts only for actual predeclared `len`/`cap` calls, so user functions or shadowed identifiers named `len`/`cap` are not rewritten. |
| `Box::new` field clone in `value_materialization.rs` | The broad postpass rewrite was removed. Interface field copies are handled by typed interface boxing through generated `__gors_clone_box`, and arbitrary `Box::new(field)` calls are left unchanged. |
| integer-typed float local initializer repair in `coerce_types.rs` | Local `var` lowering already compiles explicit Go types through expected-type expression lowering; a compiler regression now pins `var max int = 1e6` to `(1e6 as isize)`. |
| monolithic `coerce_types.rs` visitor responsibilities | Pointer cells, structural helpers, evaluation order, static false pruning, call arguments, tuple newtypes, binary comparisons, value materialization, and shared syntax predicates are split into focused modules under `coerce_types/`. |
| `sort.Slice*` custom lowering in `compiler/stdlib_workarounds.rs` | The workaround module was removed. `sort.Slice*` now compile through ordinary stdlib code using generic `any` call-value copying, function-value callbacks, reflectlite slice swapper support, and writeback for addressable slices passed through `any`. |
| `strconv` string value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now clones path, field, and index arguments according to the callee's generated `String`/cloneable value parameter types. |
| `slices::Sort` mutable argument borrowing in `coerce_types.rs` | Cross-module mutable-reference call analysis now borrows arguments according to generated callee `&mut` parameter types. |
| non-append `unicode/utf8` value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now clones `String` and `Vec<u8>` path, field, and index arguments according to generated callee parameter types. |
| `write`/`writeString` method value argument cloning in `coerce_types.rs` | Receiver-qualified method call analysis now clones path, field, and index arguments according to the resolved receiver type and generated method `String`/cloneable value parameter types. |
| `argNumber` second method value argument cloning in `coerce_types.rs` | Receiver-qualified method call analysis now applies the same signature-driven cloneable value argument rule to non-first method arguments. |
| `parsenum`/`getField` function value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now handles these generated helper calls according to their generated `String`/cloneable value parameter types. |
| `Write` method slice-to-`Vec<u8>` argument coercion in `coerce_types.rs` | Receiver-qualified method call analysis now materializes range-index slice arguments with `.to_vec()` when the resolved method signature expects a `Vec<T>` value parameter. |
| stale `fmtsort::Sort` argument cloning in `coerce_types.rs` | The package-specific branch was removed; current generated calls are handled by generic call-signature borrowing and cloneable-value analysis. |
| stale `reflect::TypeOf` argument borrowing in `coerce_types.rs` | The package-specific branch and its private borrow helpers were removed; current supported generated paths do not require name-selected `TypeOf` borrowing. |
| stale `reflect::ValueOf` argument coercion in `coerce_types.rs` | The package-specific branch and helper were removed; current supported generated paths prune the reflection fallback before this name-selected coercion is needed. |
| `intFromArg` local argument move in `coerce_types.rs` | Cross-module value-argument analysis now treats by-value `Vec<Box<dyn Any>>` parameters as non-cloneable lvalue takes, driven by the callee signature rather than helper name. |
| `unicode/utf8.AppendRune` receiver argument move in `coerce_types.rs` | Cross-module value-argument analysis now treats by-value `Vec<T>` parameters passed `*self` as lvalue takes, driven by callee signature and receiver lvalue role rather than function name. |
| local initializer cloning by `value`/`f`/`fmtFlags` names in `coerce_types.rs` | Compiler lowering already clones binding initializers through IR addressability and Go type copy semantics; the postpass no longer clones locals solely by identifier or field name. |
| dead `printArg` unsupported-format pruning in `coerce_types.rs` | The name-selected branch had no active predicate; generic static-false pruning and reflection fallback pruning now run without a `printArg`-specific no-op path. |
| stale `printValue` argument coercion in `coerce_types.rs` | Current generated supported paths prune reflection fallback calls before method-argument coercion; the postpass no longer rewrites arbitrary `printValue` calls based on field names like `Key` or `Value`. |
| stale `printArg(err)` boxing in `coerce_types.rs` | Current generated supported paths do not need local-name-selected `err` boxing; `Box<dyn Any>` argument materialization should come from expected-type lowering instead of local variable names. |
| stale `printArg(index)` replacement in `coerce_types.rs` | Current generated supported paths lower `[]any` indexes through the generic `clone_any` binding path; the postpass no longer rewrites arbitrary indexed `printArg` arguments by method name. |
| `printArg(self.arg)` empty-any replacement in `coerce_types.rs` | Empty-interface field reads and `any`-to-`any` assignments now copy through the generic `clone_any` runtime helper; the postpass no longer rewrites arbitrary `printArg(self.arg)` calls by method and field name. |
| stale `err = w` rewrite in `coerce_types.rs` | Current generated supported paths do not need local-name-selected extraction from `w.err`; assigning interface values from concrete pointer cells is handled by compiler/interface lowering instead of local variable names. |
| `self.arg = arg` empty-any replacement in `coerce_types.rs` | Empty-interface assignment now copies through generic `clone_any`; the postpass no longer replaces arbitrary `self.arg = arg` assignments by field and local name. |
| `strconv.AppendFloat` call-site lowering and `builtin::append_float` | `strconv` now compiles through the ordinary stdlib path after generic reachable-declaration recovery, scoped local type facts, shared-lvalue parenthesization, typed-constant arithmetic, and folded constant index lowering. The dead runtime helper was removed. |
| `self.value = value` cloning in `coerce_types.rs` | Expected-type expression lowering already clones addressable same-type non-Copy RHS values from Go type facts; the postpass no longer clones arbitrary assignments by field and local name. |
| rendered-token matching in `coerce_types.rs` body/pruning triggers | Remaining body replacement and reflection-pruning gates now use `syn` AST visitors for identifiers, methods, fields, expression paths, and type paths. String literals and formatting cannot accidentally trigger generated-body rewrites or reflection pruning. |
| `padString` body replacement in `coerce_types.rs` | The generated Go body now compiles through generic method-call and string argument lowering, preserving width-padding behavior instead of replacing the named method with a direct write. |
| `fmtString` body replacement in `coerce_types.rs` | The generated Go body now compiles through generic branch, receiver-method, and string argument lowering, preserving `%q`, `%x`, and `% X` string formatting behavior instead of replacing the named method with `fmtS`. |
| `newPrinter` body replacement in `coerce_types.rs` | The generated Go body now compiles through pointer-cell ownership, generic receiver-method argument hoisting, manual cloning for structs with `any` fields, and a minimal `sync.Pool` runtime primitive instead of replacing the named function body. |
| `free` body removal in `coerce_types.rs` | The generated Go method body is no longer deleted by method name; currently supported pool behavior is handled at the `sync.Pool` runtime boundary. |
| fmt flush insertion by receiver type name in `coerce_types.rs` | Flush insertion is now gated by generated receiver metadata collected across impl blocks, and a narrow post-structural-helper pass runs after resolver helper injection. Arbitrary receivers named differently or receivers without the hook no longer get name-selected flush calls. |
| `pp` receiver-name gate for `self.value` reflection fallback pruning | The fallback prune now uses generated self-value reflection fallback shapes instead of the literal type name `pp` or the flush hook, so helper ordering does not leave unsupported reflect branches in generated output. |
