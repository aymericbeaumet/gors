use super::{compile_and_run, raw_program};
use crate::compiler::compile_program;

#[test]
fn generic_inference_lets_typed_arguments_determine_type_parameters() {
    let run = compile_and_run(
        r#"
            package main

            func pickSame[T any](a, b T) T { return a }

            func main() {
                var v float64 = 7
                if pickSame(1, v) != 1.0 {
                    panic("a typed argument must determine T")
                }
                if pickSame(v, 2)+0.5 != 7.5 {
                    panic("an untyped constant must adapt to the typed argument")
                }
                if pickSame(1, 2.5)+0.5 != 1.5 {
                    panic("untyped constants must merge default types")
                }
                if pickSame(2, 3) != 2 {
                    panic("matching untyped constants changed")
                }
                println("generic-inference: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"generic-inference: ok\n");
}

#[test]
fn generic_inference_rejects_unmergeable_untyped_constant_kinds() {
    let error = compile_program(raw_program(
        "generics.go",
        "generics.go",
        "package main\nfunc pickSame[T any](a, b T) T { return a }\nfunc main() { _ = pickSame(\"x\", 1) }\n",
    ))
    .err()
    .expect("mismatched untyped constant kinds must be rejected");
    assert!(
        error.to_string().contains("mismatched default types"),
        "{error}"
    );
}

#[test]
fn generic_inference_rejects_non_representable_untyped_constants() {
    let error = compile_program(raw_program(
        "generics.go",
        "generics.go",
        "package main\nfunc pickSame[T any](a, b T) T { return a }\nfunc main() { var v float64 = 7; _ = pickSame(v, \"x\") }\n",
    ))
    .err()
    .expect("an unassignable constant argument must be rejected");
    assert!(error.to_string().contains("cannot use"), "{error}");
}
