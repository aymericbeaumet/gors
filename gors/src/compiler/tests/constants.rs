use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_file};

#[test]
fn local_constants_are_exact_scoped_and_support_repeated_specs_and_iota() {
    let run = compile_and_run(
        r#"
            package main

            const packageValue = 90

            func value() int {
                const (
                    first = iota + 1
                    second
                    typed int32 = 1<<31 - 1
                )
                const packageValue = second + 5
                {
                    const packageValue = 7
                    if packageValue != 7 {
                        panic("nested constant scope changed")
                    }
                }
                if first != 1 || second != 2 || typed != 2147483647 {
                    panic("local constant evaluation changed")
                }
                return packageValue
            }

            func main() {
                println(packageValue, value())
            }
        "#,
    );

    assert_eq!(run.stderr, b"90 7\n");
}

#[test]
fn local_constants_are_not_assignment_places() {
    let errors = compile_file(
        "main.go",
        "package main\nfunc main() { const answer = 42; answer = 0 }\n",
    )
    .err()
    .expect("assignment to a local constant must be rejected");

    assert!(
        errors.iter().any(|error| {
            error.code == "GORS2002" && error.message.contains("cannot assign to constant answer")
        }),
        "{errors:?}"
    );
}

#[test]
fn local_constant_comment_edits_do_not_reexecute_semantic_stages() {
    let before = r#"
        package main
        func answer() int {
            const value = 40 + 2
            return value
        }
        func main() { println(answer()) }
    "#;
    let after = r#"
        package main
        func answer() int {
            // The physical layout changed; the local constant did not.
            const value = 40 + 2
            return value
        }
        func main() { println(answer()) }
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

#[test]
fn exact_float_constant_arithmetic_folds_at_integer_sites() {
    let run = compile_and_run(
        r#"
            package main

            const fromFloat int = 14 / 2.0
            const scaled = 1.5 * 4
            const quarter = 3 / 2.0 / 6

            func main() {
                var n int = 14 / 2.0
                var f float64 = 15 / 2.0
                if quarter != 0.25 || f != 7.5 {
                    panic("exact float constant arithmetic changed")
                }
                println(fromFloat, n, int(scaled))
            }
        "#,
    );

    assert_eq!(run.stderr, b"7 7 6\n");
}

#[test]
fn non_integral_float_constants_are_rejected_at_integer_sites() {
    for source in [
        "package main\nconst bad int = 15 / 2.0\nfunc main() { println(bad) }\n",
        "package main\nfunc main() { var n int = 15 / 2.0; println(n) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("a non-integral float constant at an integer site must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("not representable")),
            "{errors:?}"
        );
    }
}

#[test]
fn float_constant_division_by_zero_is_rejected() {
    for source in [
        "package main\nconst bad = 1.0 / 0.0\nfunc main() { println(bad) }\n",
        "package main\nfunc main() { x := 1.5 / 0.0; println(x) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("constant float division by zero must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("division by zero")),
            "{errors:?}"
        );
    }
}

#[test]
fn constant_shifts_convert_untyped_left_operands_to_integer_constants() {
    let run = compile_and_run(
        r#"
            package main

            const sf = 1.0 << 3
            const cw = (2 + 0i) << 2

            func main() {
                x := 1.0 << 3
                var f = 1.0 << 3
                y := 1 << 2.0
                var z float64 = 1 << 3
                if sf != 8 || cw != 8 || y != 4 || z != 8.0 {
                    panic("constant shift conversion changed")
                }
                println(sf, x, f)
            }
        "#,
    );

    assert_eq!(run.stderr, b"8 8 8\n");
}

#[test]
fn non_integer_constant_shift_operands_are_rejected() {
    for source in [
        "package main\nconst bad = 1.5 << 1\nfunc main() { println(bad) }\n",
        "package main\nfunc main() { x := 1.5 << 1; println(x) }\n",
        "package main\nfunc main() { x := 1 << 1.5; println(x) }\n",
        "package main\nfunc main() { x := true << 1; println(x) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("non-integer constant shift operands must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("must be an integer constant")),
            "{errors:?}"
        );
    }
}

#[test]
fn constant_string_operands_support_byte_indexing() {
    let run = compile_and_run(
        r#"
            package main

            const text = "abc"

            func main() {
                b := "abc"[1]
                c := text[2]
                println(int(b), int(c))
            }
        "#,
    );

    assert_eq!(run.stderr, b"98 99\n");
    assert!(run.rust.contains("go_string_index"), "{}", run.rust);
}

#[test]
fn indexing_a_non_string_untyped_constant_is_rejected() {
    let errors = compile_file(
        "main.go",
        "package main\nfunc main() { b := true[0]; println(b) }\n",
    )
    .err()
    .expect("indexing a non-string constant must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("indexing is not yet implemented")),
        "{errors:?}"
    );
}

#[test]
fn predeclared_constant_functions_preserve_exact_values_and_kinds() {
    let run = compile_and_run(
        r#"
            package main

            const textLength = len("héllo")
            const minimum = min(3, 2.0)
            const maximum = max('a', 100)
            const lexicalMinimum = min("go", "Go")
            const lexicalMaximum = max("bar", "foo")
            const pair = complex(1, 2)
            const realPart = real(pair)
            const imaginaryPart = imag(pair)

            func main() {
                var sized [textLength]int
                if len(sized) != 6 || minimum != 2 || maximum != 100 || lexicalMinimum != "Go" || lexicalMaximum != "foo" || realPart != 1 || imaginaryPart != 2 {
                    panic("constant built-in evaluation changed")
                }
                println(textLength, int(minimum), int(maximum), lexicalMinimum, lexicalMaximum, int(realPart), int(imaginaryPart))
            }
        "#,
    );

    assert_eq!(run.stderr, b"6 2 100 Go foo 1 2\n");
}

#[test]
fn invalid_constant_function_calls_are_rejected() {
    for (source, expected) in [
        (
            "package main\nconst bad = len(123)\nfunc main() { println(bad) }\n",
            "constant len currently requires a constant string",
        ),
        (
            "package main\nconst bad = min()\nfunc main() { println(bad) }\n",
            "call to min requires at least one argument",
        ),
        (
            "package main\nconst bad = complex(1)\nfunc main() { println(bad) }\n",
            "call to complex requires exactly two arguments",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("invalid constant function call must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.code == "GORS2002" && error.message.contains(expected)),
            "expected {expected:?}, got {errors:?}"
        );
    }
}
