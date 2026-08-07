use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_file};

#[test]
fn generated_dynamic_slice_literals_evaluate_elements_left_to_right_once() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                order := []int{}
                next := func(value int) int {
                    order = append(order, value)
                    return value * 2
                }
                values := []int{next(1), next(2), next(3)}
                if values[0] != 2 || values[1] != 4 || values[2] != 6 {
                    panic("dynamic slice literal values changed")
                }
                if order[0] != 1 || order[1] != 2 || order[2] != 3 {
                    panic("dynamic slice literal order changed")
                }
                println("dynamic-slice: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"dynamic-slice: ok\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_i64_set"), "{}", run.rust);
}

#[test]
fn generated_variadic_calls_pack_final_arguments_after_fixed_arguments() {
    let run = compile_and_run(
        r#"
            package main

            func total(base int, values ...int) int {
                for _, value := range values {
                    base += value
                }
                return base
            }

            func main() {
                order := []int{}
                next := func(value int) int {
                    order = append(order, value)
                    return value
                }
                packed := total(next(10), next(1), next(2))
                spread := total(20, []int{3, 4}...)
                if packed != 13 || spread != 27 {
                    panic("variadic values changed")
                }
                if order[0] != 10 || order[1] != 1 || order[2] != 2 {
                    panic("variadic argument order changed")
                }
                println("variadic-pack: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"variadic-pack: ok\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
}

#[test]
fn variadic_calls_without_final_arguments_pass_the_nil_slice() {
    let run = compile_and_run(
        r#"
            package main

            func inspect(base int, values ...int) int {
                if values == nil {
                    return base
                }
                return base + len(values)
            }

            func main() {
                if inspect(1) != 1 {
                    panic("an empty variadic tail must pass nil")
                }
                if inspect(1, 2, 3) != 3 {
                    panic("packed variadic arguments changed")
                }
                var empty []int
                if inspect(4, empty...) != 4 {
                    panic("spreading a nil slice must stay nil")
                }
                println("variadic-nil: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"variadic-nil: ok\n");
    assert!(run.rust.contains("go_slice_i64_nil"), "{}", run.rust);
}

#[test]
fn constant_make_lengths_cannot_exceed_constant_capacities() {
    for slice in ["[]int", "[]byte"] {
        let source =
            format!("package main\nfunc main() {{\n\ts := make({slice}, 5, 3)\n\t_ = s\n}}\n");
        let errors = compile_file("main.go", &source)
            .err()
            .expect("a constant make length larger than its capacity must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("length and capacity swapped")),
            "{errors:?}"
        );
    }
}

#[test]
fn constant_negative_make_sizes_are_rejected() {
    for source in [
        "package main\nfunc main() {\n\ts := make([]int, -1)\n\t_ = s\n}\n",
        "package main\nfunc main() {\n\ts := make([]int, 2, -3)\n\t_ = s\n}\n",
        "package main\nfunc main() {\n\ts := make([]byte, -1)\n\t_ = s\n}\n",
        "package main\nfunc main() {\n\ts := make([]byte, 2, -3)\n\t_ = s\n}\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("a constant negative make size must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("must not be negative")),
            "{errors:?}"
        );
    }
}

#[test]
fn swapped_constant_slice_bounds_are_rejected() {
    for (source, expected) in [
        (
            "package main\nfunc main() {\n\ts := []int{1, 2, 3}\n\t_ = s[2:1]\n}\n",
            "invalid slice indices: 1 < 2",
        ),
        (
            "package main\nfunc main() {\n\ts := []int{1, 2, 3}\n\t_ = s[1:2:1]\n}\n",
            "invalid slice indices: 1 < 2",
        ),
        (
            "package main\nfunc main() {\n\ts := []int{1, 2, 3}\n\t_ = s[3:2:4]\n}\n",
            "invalid slice indices: 2 < 3",
        ),
        (
            "package main\nfunc main() {\n\ts := \"abc\"\n\t_ = s[2:1]\n}\n",
            "invalid slice indices: 1 < 2",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("swapped constant slice bounds must be rejected");
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn ordered_constant_and_runtime_slice_bounds_stay_accepted() {
    // Runtime bounds keep their runtime panic; only constant violations are
    // compile-time rejections.
    compile_file(
        "main.go",
        "package main\nfunc bound() int { return 2 }\nfunc main() {\n\ts := []int{1, 2, 3}\n\t_ = s[1:2]\n\t_ = s[2:2]\n\t_ = s[:2]\n\t_ = s[1:]\n\t_ = s[1:2:3]\n\t_ = s[bound():1]\n\tt := make([]int, 3, 5)\n\t_ = t\n}\n",
    )
    .expect("ordered constant bounds and runtime bounds must stay accepted");
}

#[test]
fn variadic_calls_still_require_their_fixed_arguments() {
    let errors = compile_file(
        "main.go",
        "package main\nfunc total(base int, values ...int) int { return base + len(values) }\nfunc main() { println(total()) }\n",
    )
    .err()
    .expect("missing fixed arguments must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("requires at least")),
        "{errors:?}"
    );
}

#[test]
fn byte_slice_make_set_and_copy_use_typed_runtime_operations() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                bytes := make([]byte, 4, 6)
                bytes[0] = 'a'
                bytes[1] = 'b'
                bytes[2] = 'c'
                bytes[3] = 'd'
                if copy(bytes[1:], bytes[:3]) != 3 || string(bytes) != "aabc" {
                    panic("overlapping byte copy changed")
                }
                if copy(bytes, "go") != 2 || string(bytes) != "gobc" {
                    panic("string to byte copy changed")
                }
                var nilBytes []byte
                if copy(bytes, nilBytes) != 0 || copy(nilBytes, bytes) != 0 {
                    panic("nil byte copy changed")
                }
                println("byte-slice-make-copy: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"byte-slice-make-copy: ok\n");
    for symbol in [
        "go_slice_u8_make",
        "go_slice_u8_set",
        "go_slice_u8_copy",
        "go_slice_u8_copy_string",
    ] {
        assert!(run.rust.contains(symbol), "missing {symbol}: {}", run.rust);
    }
}

#[test]
fn explicit_nil_slice_conversions_use_the_typed_nil_operations() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                ints := []int(nil)
                bytes := []byte(nil)
                if ints != nil || bytes != nil || copy(ints, []int{1}) != 0 {
                    panic("explicit nil slice conversion changed")
                }
                println("nil-slice-conversion: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"nil-slice-conversion: ok\n");
    assert!(run.rust.contains("go_slice_i64_nil"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_u8_nil"), "{}", run.rust);
}

#[test]
fn explicit_nil_conversion_rejects_non_nilable_types() {
    let errors = compile_file("main.go", "package main\nfunc main() { _ = int(nil) }\n")
        .err()
        .expect("nil must not convert to a non-nilable type");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("cannot convert nil")),
        "{errors:?}"
    );
}

#[test]
fn byte_slice_make_rejects_unsupported_elements_and_unimplemented_compound_writes() {
    for (source, expected) in [
        (
            "package main\nfunc main() { _ = make([]string, 1) }\n",
            "make currently supports []int and []byte values",
        ),
        (
            "package main\nfunc main() { b := make([]byte, 1); b[0] += 1 }\n",
            "compound []byte assignment requires uint8 wrapping semantics",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("unsupported byte-slice forms must be rejected");
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "{errors:?}"
        );
    }
}

#[test]
fn byte_slice_comment_edits_do_not_reexecute_semantic_stages() {
    let before = r#"
        package main
        func copied() int {
            bytes := make([]byte, 2)
            return copy(bytes, "go")
        }
        func main() { println(copied()) }
    "#;
    let after = r#"
        package main
        func copied() int {
            // Byte make and copy are unchanged semantic inputs.
            bytes := make([]byte, 2)
            return copy(bytes, "go")
        }
        func main() { println(copied()) }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    session.database().reset_telemetry();

    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
}
