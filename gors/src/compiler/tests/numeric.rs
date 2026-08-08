use super::*;

#[test]
fn float32_and_numeric_conversions_execute_through_exact_width_operations() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                big := float32(1 << 24)
                if big + 1 != big { panic("float32 addition did not round") }
                tenth := float32(0.1)
                if float64(tenth) <= 0.1 { panic("float32 widening changed") }
                negative := -2.9
                if int32(negative) != -2 { panic("float truncation changed") }
                var signed int8 = -1
                if uint64(signed) != ^uint64(0) { panic("integer extension changed") }
                value := complex64(complex(1.5, -2.5))
                if real(value) != 1.5 || imag(value) != -2.5 {
                    panic("complex64 conversion changed")
                }
                println(big + 1 == big, float64(tenth) > 0.1)
            }
        "#,
    );
    assert_eq!(run.stderr, b"true true\n");
}

#[test]
fn unsupported_complex64_arithmetic_is_diagnosed_before_mir() {
    let errors = crate::compiler::lower_to_hir(
        "complex64.go",
        "package main\nfunc add(left complex64, right complex64) complex64 { return left + right }\n",
    )
    .unwrap_err();
    assert!(
        errors.iter().any(|diagnostic| {
            diagnostic.code == "GORS2002"
                && diagnostic
                    .message
                    .contains("operator Add is invalid for Complex(Complex64)")
        }),
        "{errors:?}"
    );
}
