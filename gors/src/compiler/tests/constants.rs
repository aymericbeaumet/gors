use super::compile_and_run;
use crate::compiler::compile_file;

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
