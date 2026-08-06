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
fn package_struct_variables_materialize_typed_value_copies() {
    let run = compile_and_run(
        r#"
            package main

            type Pair struct {
                Left int
                Right int
            }

            var pair = Pair{Right: 4, Left: 3}

            func main() {
                local := pair
                local.Left = 8
                if pair.Left != 3 || pair.Right != 4 || local.Left != 8 {
                    panic("package struct value copy changed")
                }
                println("package-struct: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"package-struct: ok\n");
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

#[test]
fn address_taken_integer_locals_share_pointer_backing() {
    let run = compile_and_run(
        r#"
            package main

            func update(parameter int) int {
                pointer := &parameter
                parameter = 5
                *pointer = *pointer + 1
                return parameter
            }

            func main() {
                value := 2
                pointer := &value
                value, delta := 4, 3
                alias := pointer
                *alias = *alias + delta
                println(value)
                println(update(1))
            }
        "#,
    );

    assert_eq!(run.stderr, b"7\n6\n");
    assert!(run.rust.contains("go_pointer_i64_new"), "{}", run.rust);
    assert!(run.rust.contains("go_pointer_i64_get"), "{}", run.rust);
    assert!(run.rust.contains("go_pointer_i64_set"), "{}", run.rust);
}

#[test]
fn nested_address_taking_is_rejected_until_lifetimes_are_explicit() {
    let error = compile_program(raw_program(
        "nested-address.go",
        "nested-address.go",
        "package main\nfunc main() { value := 1; if true { _ = &value } }\n",
    ))
    .err()
    .expect("nested address-taking must not bypass storage lifetime planning");

    assert!(
        error
            .to_string()
            .contains("address-taking in nested control flow"),
        "{error}"
    );
}
