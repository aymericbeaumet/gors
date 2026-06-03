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
| Reflection fallback pruning in `structural_helpers/reflection_fallback.rs` | `reflect` AST path/type-path checks and generated `reflect::Value` self-field metadata | stdlib workaround | Represent reflect/type-switch support as compiler/runtime semantic facts; prune only unreachable IR/control-flow branches, not branches selected by generated token text. |
| Format flush insertion in `structural_helpers/fmt_flush.rs` | generated `__gors_flush_fmt` hook source-field metadata plus receiver self-call graph | stdlib workaround | Flush side effects should be represented as method/lowering semantics for receiver-buffer aliasing, or removed by correctly modeling the buffer alias. |

## Other production hardcodes

| File | Current trigger | Category | Generic rule to implement |
| --- | --- | --- | --- |
| `gors/src/compiler/runtime_primitives/reflect.rs` | `reflect` module replacement in post-prune helpers | runtime primitive | Reflect support is currently a runtime primitive boundary; keep isolated until generic reflect IR/runtime support exists. |
| `gors/src/compiler/runtime_primitives/sync.rs` | `sync.Pool` module replacement in post-prune helpers | runtime primitive | Pooling is modeled as a minimal runtime primitive for host allocation reuse; keep it isolated and avoid package consumer rewrites. |
| `gors/src/compiler/runtime_primitives/os.rs` | `os.Stdout`/`os.File` host-resource replacement | runtime primitive | Host resources may be injected, but must preserve unrelated compiled stdlib items. |
| `gors/src/compiler/reflect_kind.rs` | import-path-aware `reflect.TypeOf(x).Kind() == reflect.K` detection | runtime primitive | This is a reflect runtime boundary; future work should expose it as IR reflect-kind operation instead of AST pattern matching. |
| `gors/src/compiler/reflect_slice_any.rs` | writeback planning for addressable slices passed to `any` parameters, used by reflectlite `Swapper` paths | runtime primitive | Keep this isolated until reflect-backed slice mutation is modeled as generic aliasing/interface lowering. |
| `gors/src/resolve/runtime_primitives.rs` | synthetic `runtime` and `internal/reflectlite` module generation before ordinary resolver parsing | runtime primitive | Keep these as isolated resolver-level runtime primitives until generic runtime/reflectlite lowering can compile the ordinary packages. |
| `gors/src/resolve/structural_helpers/mut_ref_forwarders.rs` and `fmt_flush.rs` | generated mutable-reference `State` forwarding impls and injected `pp.__gors_flush_fmt` | stdlib workaround | Interface implementation and receiver-buffer aliasing should be produced by generic method/interface lowering, not resolver post-processing. |
| `gors-builtin/src/lib.rs` | predeclared print/println, interface, reflect-kind helpers | runtime primitive | Builtin language/runtime support is valid, but must not implement stdlib package behavior. |

## Replacement order

1. Replace `structural_helpers.rs` reflection pruning and fmt flush insertion
   with semantic reflect support and receiver-buffer aliasing.
2. Resolver/compiler post-prune fmt helper removal after receiver-buffer aliasing
   is represented semantically.

## Completed removals

| Area | Replacement |
| --- | --- |
| Rust-name-driven `len`/`cap` post-pass casts in `coerce_types.rs` | Builtin lowering now emits Go `int` casts only for actual predeclared `len`/`cap` calls, so user functions or shadowed identifiers named `len`/`cap` are not rewritten. |
| `Box::new` field clone in `value_materialization.rs` | The broad postpass rewrite was removed. Interface field copies are handled by typed interface boxing through generated `__gors_clone_box`, and arbitrary `Box::new(field)` calls are left unchanged. |
| `builtin::append` first/second args in `value_materialization.rs` | The postpass module was removed. Append source values now clone addressable non-Copy lvalues by default, assignment lowering turns same-lvalue append updates into `std::mem::take`, and append elements are cloned from Go element type/addressability facts. |
| integer-typed float local initializer repair in `coerce_types.rs` | Local `var` lowering already compiles explicit Go types through expected-type expression lowering; a compiler regression now pins `var max int = 1e6` to `(1e6 as isize)`. |
| monolithic `coerce_types.rs` visitor responsibilities | Pointer cells, structural helpers, evaluation order, static false pruning, call arguments, tuple newtypes, binary comparisons, and shared syntax predicates are split into focused modules under `coerce_types/`; the former value-materialization postpass has been folded back into typed lowering. |
| `sort.Slice*` custom lowering in `compiler/stdlib_workarounds.rs` | The workaround module was removed. `sort.Slice*` now compile through ordinary stdlib code using generic `any` call-value copying, function-value callbacks, reflectlite slice swapper support, and writeback for addressable slices passed through `any`. |
| `strconv` string value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now clones path, field, and index arguments according to the callee's generated `String`/cloneable value parameter types. |
| `slices::Sort` mutable argument borrowing in `coerce_types.rs` | Cross-module mutable-reference call analysis now borrows arguments according to generated callee `&mut` parameter types. |
| non-append `unicode/utf8` value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now clones `String` and `Vec<u8>` path, field, and index arguments according to generated callee parameter types. |
| `write`/`writeString` method value argument cloning in `coerce_types.rs` | Receiver-qualified method call analysis now clones path, field, and index arguments according to the resolved receiver type and generated method `String`/cloneable value parameter types. |
| `argNumber` second method value argument cloning in `coerce_types.rs` | Receiver-qualified method call analysis now applies the same signature-driven cloneable value argument rule to non-first method arguments. |
| `parsenum`/`getField` function value argument cloning in `coerce_types.rs` | Cross-module cloneable-value call analysis now handles these generated helper calls according to their generated `String`/cloneable value parameter types. |
| `Write` method slice-to-`Vec<u8>` argument coercion in `coerce_types.rs` | Receiver-qualified method call analysis now materializes range-index slice arguments with `.to_vec()` when the resolved method signature expects a `Vec<T>` value parameter. |
| mutable byte-slice calls over sliced pointer fields in `coerce_types.rs` | Signature-driven mutable slice argument rewriting now looks through parenthesized cloned field blocks inside slice indexes and borrows the original lvalue, so `[]byte` writers mutate backing storage instead of cloned temporaries. |
| stale `fmtsort::Sort` argument cloning in `coerce_types.rs` | The package-specific branch was removed; current generated calls are handled by generic call-signature borrowing and cloneable-value analysis. |
| stale `reflect::TypeOf` argument borrowing in `coerce_types.rs` | The package-specific branch and its private borrow helpers were removed; current supported generated paths do not require name-selected `TypeOf` borrowing. |
| stale `reflect::ValueOf` argument coercion in `coerce_types.rs` | The package-specific branch and helper were removed; current supported generated paths prune the reflection fallback before this name-selected coercion is needed. |
| `intFromArg` local argument move in `coerce_types.rs` | Cross-module value-argument analysis now treats by-value `Vec<Box<dyn Any>>` parameters as non-cloneable lvalue takes, driven by the callee signature rather than helper name. |
| `unicode/utf8.AppendRune` receiver argument move in `coerce_types.rs` | Cross-module value-argument analysis now treats by-value `Vec<T>` parameters passed dereferenced lvalues as lvalue takes, driven by callee signature and lvalue role rather than function name or receiver spelling. |
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
| broad `reflect` segment matching in `structural_helpers.rs` | Reflection fallback pruning now recognizes actual `reflect::...` / `crate::reflect::...` module paths instead of pruning any local identifier or type path segment named `reflect`. |
| `self.value` reflection fallback pruning in `structural_helpers.rs` | Receiver fallback pruning now derives the prunable self fields from struct fields typed as `reflect::Value`, instead of assuming any field literally named `value` is a generated reflection fallback. |
| flush insertion receiver gates in `structural_helpers.rs` | Flush insertion now records the actual generated flush-trigger methods per receiver with a `__gors_flush_fmt` hook, so statement rewriting uses collected receiver metadata instead of an independent receiver-name plus method-name check. |
| `self.printValue` / `self.fmtPointer` reflection pruning in `structural_helpers.rs` | Reflection fallback pruning no longer drops receiver method calls solely by method name. It prunes actual reflect paths and reflect-typed self fields, then uses dependency-aware block pruning so locals removed from fallback paths also remove their consumers. |
| duplicated flush insertion loops in `coerce_types.rs` and `structural_helpers.rs` | Both passes now call `Metadata::push_stmt_with_flush`, keeping the flush decision and emitted hook call in one structural-helper boundary. |
| stale `print_arg` names in reflection fallback pruning internals | Reflection pruning helpers are named for their actual fallback-pruning responsibility instead of the older `printArg` call-site workaround. |
| mixed fmt/reflection structural-helper metadata | `Metadata` now delegates to `FmtFlushMetadata` and `ReflectionFallbackMetadata`, so flush insertion and reflection fallback pruning keep separate collection and query responsibilities. |
| fmt flush metadata mixed into `structural_helpers.rs` | Flush-source detection, receiver self-call expansion, and flush insertion now live in `structural_helpers/fmt_flush.rs`; the parent structural helper module keeps only post-helper orchestration plus reflection fallback pruning. |
| reflection fallback pruning mixed into `structural_helpers.rs` | Reflection fallback metadata, reflect-path detection, and dependency pruning now live in `structural_helpers/reflection_fallback.rs`; the parent structural helper module is a post-helper orchestration boundary. |
| duplicated structural self-field visitors | `structural_helpers/self_fields.rs` now owns self-field detection and direct self-field collection shared by fmt flush metadata and reflection fallback pruning. |
| string-encoded `& mut pp` resolver helper matching | Resolver structural-helper injection is split by responsibility, and impl self-type checks now use explicit `syn::Type` matching instead of a rendered self-type string. |
| inline runtime primitive post-prune replacement in `compiler/mod.rs` | Reflect, `os.Stdout`, and `sync.Pool` replacement policy now lives in `compiler/runtime_primitives.rs`, leaving the main compiler pipeline responsible for orchestration and module pruning. |
| mixed runtime primitive replacements in `runtime_primitives.rs` | The runtime primitive dispatcher now delegates reflect, os, and sync replacement bodies to focused modules under `compiler/runtime_primitives/`. |
| inline preserved `reflect` stub insertion in `compiler/mod.rs` | Missing preserved reflect module materialization now lives in the reflect runtime primitive module; the main pipeline only computes preserved modules and prunes. |
| inline `reflect.TypeOf(...).Kind()` detector in `compiler/mod.rs` | Reflect-kind comparison detection, argument extraction, and kind mapping now live in `compiler/reflect_kind.rs`; binary expression lowering only emits the resulting builtin check. |
| inline reflect slice-to-`any` writeback lowering in `compiler/mod.rs` | Reflect-backed slice writeback planning and call lowering now live in `compiler/reflect_slice_any.rs`; ordinary call lowering delegates only when the focused module detects an addressable slice passed to an `any` parameter. |
| rendered-token `.lock().unwrap()` receiver detection in `compiler/mod.rs` | Scoped method receiver temporaries are now selected by a `syn` visitor that detects the `lock().unwrap()` method-call chain structurally instead of searching formatted Rust source text. |
| rendered-token `Box::new(()) as Box<dyn Any>` matching in `compiler/mod.rs` | Empty-interface zero-value detection now matches the cast and `Box::new(())` call through the `syn` AST, so formatting and whitespace no longer control package-static initialization behavior. |
| rendered-token fallback in `syn_expr_is_self` | Receiver `self` detection for numeric conversions now accepts only explicit `syn` self path/reference/paren/group forms; unrelated expressions no longer fall back to formatted-token equality. |
| rendered-token `Any` trait-bound matching in `compiler/mod.rs` | `Box<dyn Any>` recognition now uses shared `syn` trait-object bound matching instead of comparing formatted trait-bound paths. |
| rendered-token same-lvalue RHS matching in `compiler/mod.rs` | `take_rhs_lvalue_reads` now matches assignment targets and RHS reads through conservative `syn` expression structure instead of rendered expression strings. |
| rendered-token type-parameter bound dedupe in `compiler/mod.rs` | Generic constraint bound dedupe/append/ensure helpers now compare `syn::TypeParamBound` structure instead of formatted bound strings. |
| rendered-token interface implementor type dedupe in `compiler/mod.rs` | Interface implementor type lists now dedupe through shared structural `syn::Type` matching instead of formatted type strings. |
| rendered-token `#[allow(dead_code)]` detection in `compiler/mod.rs` | Generated dead-code preservation now parses attribute metadata instead of searching the formatted attribute token stream. |
| rendered-token post-merge interface helper dedupe in `compiler/mod.rs` | Noop supertrait helper insertion now compares generated impl trait/self targets structurally instead of filtering by formatted item tokens. |
| inline resolver structural-helper injection in `resolve/mod.rs` | Resolver-owned generated helper insertion now lives in `resolve/structural_helpers.rs`, so package resolution no longer owns the helper predicates and injected item bodies inline. |
| mixed resolver structural-helper policies | Noop interface sentinels, mutable-reference trait forwarding, and fmt flush hook injection now live in focused modules under `resolve/structural_helpers/`; the parent module keeps dispatch, facts, and shared AST predicates. |
| inline resolver runtime primitive module generation in `resolve/mod.rs` | Synthetic `runtime` and `internal/reflectlite` module bodies now live in `resolve/runtime_primitives.rs`, leaving `resolve/mod.rs` responsible for resolver orchestration and caching. |
| rendered-token resolver use-item dedupe in `resolve/mod.rs` | Generated `use` items now dedupe through structural `syn::UseTree` matching, so formatting changes do not control resolver output. |
| literal `reflect` qualifier in reflect-kind detection | `compiler/reflect_kind.rs` now receives an import-path predicate from current-file import facts, so aliased imports such as `import r "reflect"` lower through the same runtime primitive. |
| literal `printArg` / `printValue` fmt flush triggers | Flush insertion now derives trigger methods from the generated hook's source field and the receiver's self-call graph, so arbitrary method names only flush when they can write through the buffered source field. |
| hardcoded `State for &mut pp` resolver helper body | Resolver structural helpers now derive mutable-reference `State` forwarding impls from existing named `impl State for T` blocks and the actual trait method signatures, so the forwarder is no longer tied to `pp` or a fixed method list. |
| `padString` body replacement in `coerce_types.rs` | The generated Go body now compiles through generic method-call and string argument lowering, preserving width-padding behavior instead of replacing the named method with a direct write. |
| `fmtString` body replacement in `coerce_types.rs` | The generated Go body now compiles through generic branch, receiver-method, and string argument lowering, preserving `%q`, `%x`, and `% X` string formatting behavior instead of replacing the named method with `fmtS`. |
| `newPrinter` body replacement in `coerce_types.rs` | The generated Go body now compiles through pointer-cell ownership, generic receiver-method argument hoisting, manual cloning for structs with `any` fields, and a minimal `sync.Pool` runtime primitive instead of replacing the named function body. |
| `free` body removal in `coerce_types.rs` | The generated Go method body is no longer deleted by method name; currently supported pool behavior is handled at the `sync.Pool` runtime boundary. |
| fmt flush insertion by receiver type name in `coerce_types.rs` | Flush insertion is now gated by generated receiver metadata collected across impl blocks, and a narrow post-structural-helper pass runs after resolver helper injection. Arbitrary receivers named differently or receivers without the hook no longer get name-selected flush calls. |
| `pp` receiver-name gate for `self.value` reflection fallback pruning | The fallback prune now uses generated self-value reflection fallback shapes instead of the literal type name `pp` or the flush hook, so helper ordering does not leave unsupported reflect branches in generated output. |
