use super::{compile_and_run, raw_program};
use crate::compiler::compile_program;

#[test]
fn package_variables_materialize_typed_initial_values() {
    let run = compile_and_run(
        r#"
            package main

            const base = 40
            var answer = base + 2
            var empty string

            func main() {
                println(answer)
                println(empty == "")
            }
        "#,
    );

    assert_eq!(run.stderr, b"42\ntrue\n");
}

#[test]
fn package_variable_mutation_waits_for_global_storage_lowering() {
    let error = compile_program(raw_program(
        "variables.go",
        "variables.go",
        "package main\nvar count = 1\nfunc main() { count = 2 }\n",
    ))
    .err()
    .expect("mutable package storage must not be simulated as a constant");

    assert!(
        error
            .to_string()
            .contains("package variable mutation requires global storage lowering"),
        "{error}"
    );
}
