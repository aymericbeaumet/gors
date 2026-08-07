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
fn exact_float_constant_intermediates_need_not_fit_float64() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                var result float64 = 1e300 * 1e300 / 1e300
                println(result == 1e300)
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\n");
}

#[test]
fn nonterminating_rational_constants_remain_exact_until_concrete_typing() {
    let run = compile_and_run(
        r#"
            package main

            const fraction = 22.0 / 7
            const complexFraction = (1.0 + 2.0i) / 3

            func main() {
                var rounded float64 = fraction
                if fraction != 22.0/7 || fraction*7 != 22 ||
                    min(1e100+1, 1e100+2) != 1e100+1 ||
                    real(complexFraction)*3 != 1 || imag(complexFraction)*3 != 2 ||
                    5/2 != 2 {
                    panic("exact rational constant algebra changed")
                }
                println(rounded == 22.0/7)
            }
        "#,
    );

    assert_eq!(run.stderr, b"true\n");
}

#[test]
fn equivalent_rational_constant_edits_backdate_dependent_function_stages() {
    let before = r#"
        package main
        const ratio = 22.0 / 7
        func value() float64 { return ratio }
        func main() { println(value()) }
    "#;
    let after = r#"
        package main
        const ratio = 44.0 / 14
        func value() float64 { return ratio }
        func main() { println(value()) }
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
    assert_eq!(telemetry.executions(QueryKind::TypedConstant), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
}

#[test]
fn typed_float_constants_quantize_at_declarations_conversions_and_operations() {
    let run = compile_and_run(
        r#"
            package main

            const lowerTie float32 = 16777217
            const upperTie float32 = 16777219
            const wideTie float64 = 1<<53 + 1
            const converted = float32(16777217)
            const base float32 = 16777216
            const roundedOperation = base + 1
            const doubleRound float32 = 1.0000000596046447753906250000000000000001
            const underflowTie float32 = 0x1p-150
            const underflowAbove float32 = 0x1.000002p-150
            const negativeUnderflow float32 = -0x1p-150

            func keep32(value float32) float32 { return value }
            func keep64(value float64) float64 { return value }

            func main() {
                if keep32(lowerTie) != 16777216 ||
                    keep32(upperTie) != 16777220 ||
                    keep64(wideTie) != 9007199254740992 ||
                    keep32(converted) != 16777216 ||
                    keep32(roundedOperation) != 16777216 ||
                    keep32(doubleRound) <= 1 ||
                    keep32(underflowTie) != 0 ||
                    keep32(underflowAbove) == 0 {
                    panic("typed floating-point constant quantization changed")
                }
                zero := keep64(float64(negativeUnderflow))
                one := 1.0
                if one/zero < 0 {
                    panic("constant underflow produced negative zero")
                }
                println("typed float constants: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"typed float constants: ok\n");
}

#[test]
fn decimal_only_imaginary_literals_do_not_use_legacy_octal_values() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                backwardCompatible := 0123i
                separated := 0_123i
                explicitOctal := 0o123i
                println(
                    backwardCompatible == 123i,
                    separated == 123i,
                    explicitOctal == 83i,
                )
            }
        "#,
    );

    assert_eq!(run.stderr, b"true true true\n");
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
fn exact_constants_that_round_to_infinity_are_rejected_at_the_typing_boundary() {
    for source in [
        "package main\nconst bad float32 = 0x1.ffffffp127\nfunc main() { println(bad) }\n",
        "package main\nconst bad float64 = 0x1.fffffffffffff8p1023\nfunc main() { println(bad) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("a constant that rounds to infinity must be rejected");
        assert!(
            errors.iter().any(|error| {
                error.code == "GORS2002" && error.message.contains("not representable")
            }),
            "{errors:?}"
        );
    }
}

#[test]
fn integral_widths_wrap_convert_compare_and_preserve_constant_bounds() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                i8 := int8(127); i8++
                u8 := uint8(0); u8--
                i16 := int16(-32768); i16--
                u16 := uint16(65535); u16++
                i32 := int32(2147483647); i32++
                u32 := uint32(4294967295); u32++
                i64 := int64(-9223372036854775808); i64--
                u64 := uint64(18446744073709551615); u64++
                raw := uint8(255)
                converted := uint16(int8(raw))
                println(i8, u8, i16, u16, i32, u32, i64, u64, converted)
                println(^uint8(0), uint64(18446744073709551615), ^uint64(0), uint64(18446744073709551615) > uint64(1), min(uint64(9), uint64(3)))
            }
        "#,
    );
    assert_eq!(
        run.stderr,
        b"-128 255 32767 0 -2147483648 0 9223372036854775807 0 65535\n255 18446744073709551615 18446744073709551615 true 3\n"
    );

    for source in [
        "package main\nfunc main() { _ = int8(128) }\n",
        "package main\nfunc main() { _ = uint64(18446744073709551616) }\n",
        "package main\nfunc main() { _ = uint(-1) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("out-of-range integral constant must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("not representable")),
            "expected representability diagnostic for {source:?}, found {errors:?}"
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
