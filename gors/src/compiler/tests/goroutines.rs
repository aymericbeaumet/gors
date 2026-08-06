use super::{compile_and_run, raw_program};
use crate::compiler::compile_program;

#[test]
fn proven_empty_goroutines_preserve_argument_evaluation_order() {
    let run = compile_and_run(
        r#"
            package main

            func mark(value int) int {
                println(value)
                return value
            }

            func main() {
                go func(value int) {}(mark(7))
                println(9)
            }
        "#,
    );

    assert_eq!(run.stderr, b"7\n9\n");
}

#[test]
fn nonempty_goroutines_require_the_typed_scheduler_closure_abi() {
    let error = compile_program(raw_program(
        "goroutine.go",
        "goroutine.go",
        "package main\nfunc main() { go func() { println(1) }() }\n",
    ))
    .err()
    .expect("nonempty goroutines must retain their scheduler boundary");

    assert!(
        error
            .to_string()
            .contains("goroutine bodies require the typed scheduler closure ABI"),
        "{error}"
    );
}
