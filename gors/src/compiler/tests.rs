use super::*;

mod arrays;
mod assignments;
mod calls;
mod channels;
mod constants;
mod control_flow;
mod generics;
mod goroutines;
mod imports;
mod integer_runtime;
mod interfaces;
mod local_types;
mod maps;
mod pipeline;
mod pointer_interfaces;
mod pointers;
mod recovery;
mod runes;
mod slices;
mod structs;
mod variables;

struct GeneratedRun {
    rust: String,
    stderr: Vec<u8>,
}

fn raw_program(logical_path: &str, diagnostic_path: &str, source: &str) -> input::ProgramInput {
    raw_program_files([(logical_path, diagnostic_path, source)])
}

fn raw_program_files<const N: usize>(files: [(&str, &str, &str); N]) -> input::ProgramInput {
    let package = input::PackageKey::command_line();
    let files = files.map(|(logical_path, diagnostic_path, source)| {
        input::SourceFileInput::from_source(logical_path, diagnostic_path, source).unwrap()
    });
    let manifest = input::PackageInputManifest::new(package, files).unwrap();
    input::ProgramInput::standalone(
        input::WorkspaceKey::ad_hoc("compiler-tests").unwrap(),
        manifest,
    )
    .unwrap()
}

fn compile_and_run(source: &str) -> GeneratedRun {
    compile_program_and_run(raw_program("generated.go", "generated.go", source))
}

fn compile_program_and_run(program: input::ProgramInput) -> GeneratedRun {
    let compiled = compile_program(program).expect("compile Go through verified MIR");
    let generated = crate::printer::generate_single(compiled)
        .expect("package generated Rust with an external runtime dependency");
    let rust = generated
        .files
        .get("main.rs")
        .expect("single-file output owns main.rs")
        .clone();
    let directory = tempfile::tempdir().expect("create generated-program directory");
    let source_path = directory.path().join("generated.rs");
    let binary_path = directory.path().join("generated-program");
    let runtime_path = crate::artifact::embedded_runtime_artifact()
        .materialize(&directory.path().join("runtime-cache"))
        .expect("materialize precompiled runtime artifact");
    std::fs::write(&source_path, &rust).expect("write generated Rust");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compilation = std::process::Command::new(rustc)
        .arg("--edition=2024")
        .arg("--extern")
        .arg(format!(
            "{}={}",
            crate::artifact::RUNTIME_CRATE_NAME,
            runtime_path.display()
        ))
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("run rustc for generated program");
    assert!(
        compilation.status.success(),
        "generated Rust did not compile:\n{}\n{rust}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    let output = std::process::Command::new(binary_path)
        .output()
        .expect("run generated program");
    assert!(
        output.status.success(),
        "generated program failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    GeneratedRun {
        rust,
        stderr: output.stderr,
    }
}

#[test]
fn repeated_blank_package_declarations_do_not_create_bindings_or_roots() {
    let run = compile_and_run(
        r#"
            package main

            const _ = 1
            const _ = 2
            var _ int
            var _ = 3
            type _ int
            type _ = string
            func _() { panic("blank function ran") }
            func _() { panic("second blank function ran") }

            func main() { println("blank declarations: ok") }
        "#,
    );

    assert_eq!(run.stderr, b"blank declarations: ok\n");
}

#[test]
fn stage_products_are_real_and_mandatory_lowering_is_deterministic() {
    let source = r#"
        package main
        func answer() int {
            if true {
                return 40 + 2
            }
            return 0
        }
        func main() { println(answer()) }
    "#;
    let hir = lower_to_hir("stages.go", source).expect("typed HIR");
    assert_eq!(hir.functions.len(), 2);

    let mir = lower_to_mir(&hir).expect("verified explicit-order MIR");
    assert!(
        mir.as_file()
            .functions
            .iter()
            .any(|function| function.blocks.len() > 1)
    );

    let rust_ir = lower_to_rust_ir(mir.clone()).expect("verified explicit Rust representation");
    let lowered_once = format!("{rust_ir:#?}");
    let rust_ir_again = lower_to_rust_ir(mir).expect("deterministic mandatory lowering");
    assert_eq!(format!("{rust_ir_again:#?}"), lowered_once);

    let rust = emit_rust_ir(&rust_ir).expect("terminal syn emission");
    assert!(rust.items.len() >= 2);
}

#[test]
fn compiled_program_retains_the_verified_runtime_dependency() {
    let compiled = compile_program(raw_program(
        "runtime.go",
        "runtime.go",
        "package main\nfunc main() { println(\"value\") }\n",
    ))
    .unwrap();
    let expected_contract = gors_runtime_abi::RuntimeAbiManifest::current().identity();

    assert_eq!(compiled.runtime.contract(), expected_contract);
    for operation in [
        gors_runtime_abi::RuntimeOp::GoStringFromStatic,
        gors_runtime_abi::RuntimeOp::PrintGoString,
        gors_runtime_abi::RuntimeOp::PrintNewline,
    ] {
        assert!(compiled.runtime.requirement().contains(operation));
    }

    let runtime = compiled.runtime.clone();
    let generated = crate::printer::generate_single(compiled).unwrap();
    assert_eq!(generated.runtime, runtime);
    assert_eq!(
        generated
            .files
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["main.rs"]
    );
    let main = generated
        .files
        .get("main.rs")
        .expect("single-file output owns main.rs");
    assert!(main.contains("::__gors_runtime::"), "{}", main);
    assert!(!main.contains("mod __gors_runtime"));
}

#[test]
fn generated_rust_executes_go_int_edge_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                maximum := 9223372036854775807
                minimum := -9223372036854775807 - 1
                one := 1
                minusOne := -1
                shift := 64
                println(maximum + one)
                println(minimum - one)
                println(maximum * 2)
                println(-minimum)
                println(minimum / minusOne)
                println(minimum % minusOne)
                println(one << shift)
                println(-2 >> shift)
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        b"-9223372036854775808\n9223372036854775807\n-2\n-9223372036854775808\n-9223372036854775808\n0\n0\n-1\n"
    );
    for primitive in [
        ".wrapping_add(",
        ".wrapping_sub(",
        ".wrapping_mul(",
        ".wrapping_neg()",
    ] {
        assert!(
            run.rust.contains(primitive),
            "missing direct Rust primitive {primitive}"
        );
    }
    for helper in ["int_div", "int_rem", "int_shl", "int_shr"] {
        assert!(run.rust.contains(helper), "missing runtime helper {helper}");
    }
    for removed in ["int_add", "int_sub", "int_mul", "int_neg"] {
        assert!(
            !run.rust.contains(removed),
            "obsolete runtime helper survived: {removed}"
        );
    }
}

#[test]
fn runtime_int32_addition_and_negation_wrap_before_i64_storage() {
    let run = compile_and_run(
        r#"
            package main

            func addOne(value int32) int32 { return value + 1 }
            func negate(value rune) rune { return -value }

            func main() {
                var maximum int32 = 2147483647
                var minimum int32 = -2147483647 - 1
                wrapped := addOne(maximum)
                negated := negate(minimum)
                if wrapped != minimum || negated != minimum {
                    panic("int32/rune wrapping changed")
                }
                println(wrapped, negated)
            }
        "#,
    );

    assert_eq!(run.stderr, b"-2147483648 -2147483648\n");
    assert!(run.rust.contains("as i32"), "{}", run.rust);
    assert!(
        run.rust.contains(".wrapping_add("),
        "missing int32 wrapping addition: {}",
        run.rust
    );
    assert!(
        run.rust.contains(".wrapping_neg()"),
        "missing int32 wrapping negation: {}",
        run.rust
    );
}

#[test]
fn exact_i64_backed_scalar_types_preserve_values_and_interface_identities() {
    let run = compile_and_run(
        r#"
            package main

            func pick[T any](left, right T) T { return left }

            func main() {
                var zeroNarrow int8
                var zeroUnsigned uint
                var narrow int8 = pick(4, int8(5))
                var unsigned uint = pick(6, uint(7))

                var narrowInterface any = narrow
                narrowValue, narrowOK := narrowInterface.(int8)
                _, narrowIsInt32 := narrowInterface.(int32)

                var unsignedInterface any = unsigned
                unsignedValue, unsignedOK := unsignedInterface.(uint)
                _, unsignedIsInt := unsignedInterface.(int)

                if zeroNarrow != 0 || !narrowOK || narrowIsInt32 || narrowValue != 4 || narrow <= zeroNarrow {
                    panic("int8 dynamic type identity changed")
                }
                if zeroUnsigned != 0 || !unsignedOK || unsignedIsInt || unsignedValue != 6 || unsigned <= zeroUnsigned {
                    panic("uint dynamic type identity changed")
                }
                println("exact-i64-backed-scalars: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"exact-i64-backed-scalars: ok\n");
    for identity in [
        "builtin:int8",
        "builtin:int32",
        "builtin:uint",
        "builtin:int",
    ] {
        assert!(
            run.rust.contains(identity),
            "missing interface identity {identity}"
        );
    }
}

#[test]
fn runtime_int_negation_retains_i64_wrapping_semantics() {
    let run = compile_and_run(
        r#"
            package main

            func negate(value int) int { return -value }

            func main() {
                minimum := -9223372036854775807 - 1
                println(negate(minimum))
            }
        "#,
    );

    assert_eq!(run.stderr, b"-9223372036854775808\n");
    assert!(run.rust.contains(".wrapping_neg()"), "{}", run.rust);
}

#[test]
fn untyped_package_constants_remain_exact_until_use() {
    let run = compile_and_run(
        r#"
            package main

            const (
                highBit = 1 << 255
                folded = ((1 << 200) + (1 << 199)) >> 190
                lowBits = (highBit - 1) & 0xffff
            )

            func main() {
                println(folded)
                println(lowBits)
            }
        "#,
    );

    assert_eq!(run.stderr, b"1536\n65535\n");
}

#[test]
fn package_constants_support_iota_repetition_and_complex_components() {
    let run = compile_and_run(
        r#"
            package main

            const (
                zero = iota
                one
                repeated = 10
                repeatedAgain
            )
            const value = 1 + 2i

            func main() {
                if zero != 0 || one != 1 || repeatedAgain != 10 {
                    panic("constant repetition changed")
                }
                if real(value) != 1 || imag(value) != 2 {
                    panic("complex components changed")
                }
                println("constants: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"constants: ok\n");
}

#[test]
fn numeric_builtins_lower_constants_and_dynamic_values() {
    let run = compile_and_run(
        r#"
            package main

            func bounded(a int, b int, c int) int {
                return max(a, min(b, c), min(c))
            }

            func components(r float64, i float64) float64 {
                value := complex(r, i)
                return real(value) + imag(value)
            }

            func main() {
                if min(9, 4, 7) != 4 || max(9, 4, 7) != 9 || min(2, 1.5) != 1.5 {
                    panic("constant min/max changed")
                }
                if real(complex128(1.5)) != 1.5 || imag(complex128(1.5)) != 0.0 {
                    panic("converted complex components changed")
                }
                if bounded(3, 8, 5) != 5 || components(1.5, 2.5) != 4.0 {
                    panic("dynamic numeric built-in changed")
                }
                zero := 0.0
                negativeZero := -zero
                if 1.0 / min(zero, negativeZero) > 0.0 {
                    panic("min lost negative zero")
                }
                if 1.0 / max(negativeZero, zero) < 0.0 {
                    panic("max lost positive zero")
                }
                nan := zero / zero
                minimumNaN := min(1.0, nan)
                maximumNaN := max(nan, 1.0)
                if minimumNaN == minimumNaN || maximumNaN == maximumNaN {
                    panic("min/max did not propagate NaN")
                }
                println("numeric-builtins: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"numeric-builtins: ok\n");
    for primitive in ["is_nan", "is_sign_negative", "is_sign_positive"] {
        assert!(run.rust.contains(primitive), "{}", run.rust);
    }
}

#[test]
fn defined_numeric_types_keep_identity_through_operations_and_conversions() {
    let source = r#"
            package main

            type Score int
            type Ratio float64

            func main() {
                score := Score(4)
                score += Score(3)
                ratio := Ratio(2.5)
                ratio += Ratio(1.5)
                if int(score) != 7 || float64(ratio) != 4.0 {
                    panic("defined numeric type changed")
                }
                println("named-types: ok")
            }
        "#;
    let run = compile_and_run(source);

    assert_eq!(run.stderr, b"named-types: ok\n");
}

#[test]
fn labeled_loop_branches_target_the_named_enclosing_loop() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                total := 0
            Outer:
                for i := 0; i < 3; i++ {
                    for j := 0; j < 3; j++ {
                        if j == 1 {
                            continue Outer
                        }
                        total++
                    }
                }

            Stop:
                for i := 0; i < 3; i++ {
                    for {
                        total += 10
                        break Stop
                    }
                }
                println(total)
            }
        "#,
    );

    assert_eq!(run.stderr, b"13\n");
}

#[test]
fn expression_switch_evaluates_its_tag_once() {
    let run = compile_and_run(
        r#"
            package main

            func tag() int {
                println("tag")
                return 2
            }

            func main() {
                switch value := tag(); value {
                case 1, 2:
                    println("matched")
                default:
                    panic("switch default selected")
                }
            }
        "#,
    );

    assert_eq!(run.stderr, b"tag\nmatched\n");
}

#[test]
fn goto_uses_predeclared_forward_and_backward_targets() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                total := 0
                goto Start
                total = 100
            Start:
                total++
                if total < 3 {
                    goto Start
                }
                println(total)
            }
        "#,
    );

    assert_eq!(run.stderr, b"3\n");
}

#[test]
fn generated_rust_preserves_arbitrary_string_bytes() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                value := "\xff\x00A"
                println(value)
                println("\xff" < "\x00")
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        [0xff, 0, b'A', b'\n', b'f', b'a', b'l', b's', b'e', b'\n']
    );
    assert!(run.rust.contains("go_string_from_static(b"), "{}", run.rust);
}

#[test]
fn generated_string_concatenation_preserves_aliases_and_empty_values() {
    let run = compile_and_run(
        r#"
            package main
            func twice(value string) string { return value + value }
            func grow(value string) string {
                value = value + "b"
                value = value + "c"
                return value
            }
            func main() {
                value := "\xff"
                copy := value
                println("" + "")
                println(twice(value))
                println(copy)
                println(grow("a"))
            }
        "#,
    );

    assert_eq!(
        run.stderr,
        [
            b'\n', 0xff, 0xff, b'\n', 0xff, b'\n', b'a', b'b', b'c', b'\n'
        ]
    );
}

#[test]
fn generated_rust_executes_verified_last_use_moves() {
    let run = compile_and_run(
        r#"
            package main
            func forward(value string) string { return value }
            func choose(flag bool, value string) string {
                saved := value
                if flag { println(value) }
                return saved
            }
            func main() {
                println(forward("move"))
                println(choose(true, "branch"))
                println(choose(false, "other"))
            }
        "#,
    );

    assert_eq!(run.stderr, b"move\nbranch\nbranch\nother\n");
    assert!(
        run.rust
            .contains(".take().expect(\"compiler move of uninitialized Go local\")"),
        "{}",
        run.rust
    );
    assert!(run.rust.contains(".clone()"), "{}", run.rust);
}

#[test]
fn generated_integer_slices_preserve_backing_array_aliases() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                values := []int{1, 2, 3}
                alias := values[1:]
                alias[0] = 9
                values[1] = 7
                alias[1] += 3
                println(values[0], values[1], values[2], alias[0], alias[1])
            }
        "#,
    );

    assert_eq!(run.stderr, b"1 7 6 7 6\n");
    assert!(
        run.rust.contains("go_slice_i64_from_static"),
        "{}",
        run.rust
    );
    assert!(run.rust.contains("go_slice_i64_range"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_i64_set"), "{}", run.rust);
}

#[test]
fn generated_boolean_slices_preserve_index_and_assignment_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                flags := []bool{false, true}
                flags[0] = true
                println(flags[0])
                println(flags[1])
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\ntrue\n");
    assert!(
        run.rust.contains("go_slice_bool_from_static"),
        "{}",
        run.rust
    );
    assert!(run.rust.contains("go_slice_bool_set"), "{}", run.rust);
}

#[test]
fn generated_integer_slice_append_respects_capacity() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                base := make([]int, 2, 4)
                base[0] = 1
                base[1] = 2
                shared := append(base[:1], 9)
                detached := append(base[:1:1], 7)
                detached[0] = 8
                println(len(shared), cap(shared), base[0], base[1])
                println(len(detached), cap(detached), detached[0], detached[1])
            }
        "#,
    );

    assert_eq!(run.stderr, b"2 4 1 9\n2 2 8 7\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_i64_append"), "{}", run.rust);
}

#[test]
fn generated_multiple_results_preserve_call_and_return_arity() {
    let run = compile_and_run(
        r#"
            package main
            func pair() (int, int) { return 3, 4 }
            func forward() (int, int) { return pair() }
            func named() (left int, right int) {
                left = 5
                right = 6
                return
            }
            func main() {
                first, second := forward()
                first, second = named()
                println(first, second)
                first, first = pair()
                println(first)
            }
        "#,
    );

    assert_eq!(run.stderr, b"5 6\n4\n");
    assert!(run.rust.contains("let (__gors_result_"), "{}", run.rust);
}

#[test]
fn generated_local_functions_capture_mutable_state_and_return_directly() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                total := 0
                add := func(value int) (result int) {
                    total = total + value
                    result = total
                    return
                }
                println(add(2), add(3))
                add(4)
                println(total)
            }
        "#,
    );

    assert_eq!(run.stderr, b"2 5\n9\n");
}

#[test]
fn generated_parallel_assignments_freeze_dynamic_targets_before_writes() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                values := []int{0, 1}
                values[0], values[values[0]] = 1, 2
                println(values[0], values[1])
            }
        "#,
    );

    assert_eq!(run.stderr, b"2 1\n");
}

#[test]
fn generated_integer_slice_copy_preserves_overlap_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                values := []int{1, 2, 3, 4}
                clone := make([]int, len(values))
                count := copy(clone, values)
                copy(values[1:], values[:3])
                println(count, clone[3], values[0], values[1], values[2], values[3])
            }
        "#,
    );

    assert_eq!(run.stderr, b"4 4 1 1 2 3\n");
}

#[test]
fn generated_slice_range_evaluates_once_and_continues_through_post() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                calls := 0
                values := func() []int {
                    calls++
                    return []int{2, 3}
                }
                total := 0
                for index, value := range values() {
                    if index == 0 {
                        continue
                    }
                    total += value
                }
                println(calls, total)
            }
        "#,
    );

    assert_eq!(run.stderr, b"1 3\n");
}

#[test]
fn generated_expression_switch_fallthrough_skips_the_next_case_test() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                value := 1
                switch value {
                case 1:
                    value++
                    fallthrough
                case 99:
                    value += 10
                default:
                    value = 0
                }
                println(value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"12\n");
}

#[test]
fn generated_deferred_closures_capture_arguments_and_update_named_results() {
    let run = compile_and_run(
        r#"
            package main
            func deferred() (result int) {
                value := 1
                defer func(saved int) { result = result*10 + saved }(value)
                value = 2
                defer func(saved int) { result = result*10 + saved }(value)
                return 3
            }
            func main() { println(deferred()) }
        "#,
    );

    assert_eq!(run.stderr, b"321\n");
}

#[test]
fn generated_deferred_recover_consumes_the_active_panic() {
    let run = compile_and_run(
        r#"
            package main
            func safe() {
                defer func() {
                    if recover() == nil {
                        panic("missing panic")
                    }
                }()
                panic("boom")
                panic("continued after panic")
            }
            func main() {
                safe()
                println("recovered")
            }
        "#,
    );

    assert_eq!(run.stderr, b"recovered\n");
    assert!(run.rust.contains("catch_unwind"), "{}", run.rust);
    assert!(run.rust.contains("resume_unwind"), "{}", run.rust);
}

#[test]
fn generated_maps_preserve_nil_and_shared_reference_semantics() {
    let run = compile_and_run(
        r#"
            package main
            func nilWritePanics() (panicked bool) {
                defer func() { panicked = recover() != nil }()
                var values map[string]int
                values["missing"] = 1
                return false
            }
            func main() {
                original := map[string]int{"value": 1, "delete": 2}
                alias := original
                alias["value"] = 42
                delete(original, "delete")
                if original["value"] != 42 || len(alias) != 1 {
                    panic("map identity changed")
                }
                clear(alias)
                var nilMap map[string]int
                delete(nilMap, "missing")
                clear(nilMap)
                if nilMap != nil || nilMap["missing"] != 0 || len(nilMap) != 0 {
                    panic("nil map behavior changed")
                }
                if !nilWritePanics() { panic("nil map write did not panic") }
                println("maps: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"maps: ok\n");
    assert!(run.rust.contains("GoMapStringI64"), "{}", run.rust);
    assert!(run.rust.contains("go_map_string_i64_set"), "{}", run.rust);
}

#[test]
fn generated_maps_support_comma_ok_and_key_value_ranges() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                values := map[string]int{"zero": 0, "answer": 42}
                zero, zeroOK := values["zero"]
                missing, missingOK := values["missing"]
                count, total := 0, 0
                for key, value := range values {
                    if key == "zero" || key == "answer" { count++ }
                    total += value
                }
                if zero != 0 || !zeroOK || missing != 0 || missingOK {
                    panic("comma-ok lookup changed")
                }
                if count != 2 || total != 42 { panic("map range changed") }
                println("map-lookup-range: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"map-lookup-range: ok\n");
    assert!(
        run.rust.contains("go_map_string_i64_contains"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_map_string_i64_range_keys"),
        "{}",
        run.rust
    );
}

#[test]
fn generated_integer_arrays_preserve_value_semantics_and_checked_indexing() {
    let run = compile_and_run(
        r#"
            package main
            func outOfBoundsPanics() (panicked bool) {
                defer func() { panicked = recover() != nil }()
                values := [2]int{1, 2}
                _ = values[2]
                return false
            }
            func main() {
                original := [3]int{1, 2: 3}
                duplicate := original
                duplicate[1] = 9
                original[0] += 4
                total := 0
                for index, value := range original {
                    total += index + value
                }
                if len(original) != 3 || original[0] != 5 || original[1] != 0 {
                    panic("array values changed")
                }
                if duplicate[0] != 1 || duplicate[1] != 9 || total != 11 {
                    panic("array copy or range changed")
                }
                if original == duplicate || original != [3]int{5, 0, 3} {
                    panic("array equality changed")
                }
                if !outOfBoundsPanics() { panic("array bounds did not panic") }
                println("arrays: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"arrays: ok\n");
    assert!(run.rust.contains("[i64; 3]"), "{}", run.rust);
    assert!(run.rust.contains(".get("), "{}", run.rust);
}
