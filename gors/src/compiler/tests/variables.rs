use super::{compile_and_run, compile_program_and_run, raw_program, raw_program_files};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_program};

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
fn local_var_declarations_expand_one_multi_valued_rhs_exactly_once() {
    let run = compile_and_run(
        r#"
            package main

            func pair(calls *int) (int, string) {
                *calls = *calls + 1
                return 7, "seven"
            }

            func integers(calls *int) (int, int) {
                *calls = *calls + 1
                return 8, 9
            }

            func echo(value int) (int, int) { return value, value + 1 }

            func main() {
                calls := 0
                var number, text = pair(&calls)
                var first, second int = integers(&calls)
                var boxedFirst, boxedSecond any = integers(&calls)
                var _, _ = integers(&calls)

                entries := map[string]int{"x": 1}
                var _, found = entries["x"]
                var _, missing = entries["missing"]

                var dynamic any = "text"
                var asserted, assertionOK = dynamic.(int)

                channel := make(chan int, 1)
                channel <- 12
                var received, open = <-channel

                outer := 20
                {
                    var outer, next = echo(outer)
                    if outer != 20 || next != 21 {
                        panic("the new bindings were visible while evaluating their RHS")
                    }
                }

                if calls != 4 || number != 7 || text != "seven" {
                    panic("multi-valued calls were not evaluated exactly once")
                }
                if first != 8 || second != 9 || boxedFirst.(int) != 8 || boxedSecond.(int) != 9 {
                    panic("explicit tuple destination coercions changed")
                }
                if !found || missing || asserted != 0 || assertionOK {
                    panic("comma-ok declaration results changed")
                }
                if received != 12 || !open {
                    panic("channel comma-ok declaration changed")
                }
                println("multi-var: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"multi-var: ok\n");
}

#[test]
fn multi_valued_var_declarations_reject_arity_and_type_mismatches() {
    let arity = compile_program(raw_program(
        "variables.go",
        "/checkout/variables.go",
        r#"
            package main
            func pair() (int, int) { return 1, 2 }
            func main() { var first, second, third = pair() }
        "#,
    ))
    .err()
    .expect("tuple arity mismatches must be rejected");
    assert!(
        arity
            .to_string()
            .contains("variable declaration has 3 names and 2 result values"),
        "{arity}"
    );

    let ty = compile_program(raw_program(
        "variables.go",
        "/checkout/variables.go",
        r#"
            package main
            func pair() (int, string) { return 1, "text" }
            func main() { var first, second int = pair() }
        "#,
    ))
    .err()
    .expect("tuple component type mismatches must be rejected");
    assert!(ty.to_string().contains("not assignable"), "{ty}");

    let package = compile_program(raw_program(
        "variables.go",
        "/checkout/variables.go",
        r#"
            package main
            func pair() (int, int) { return 1, 2 }
            var first, second = pair()
            func main() {}
        "#,
    ))
    .err()
    .expect("package tuple initialization must remain explicit unsupported syntax");
    assert!(
        package
            .to_string()
            .contains("multi-valued package variable initializers are not yet represented"),
        "{package}"
    );
}

#[test]
fn repeated_init_declarations_compose_as_ordered_package_fragments() {
    let run = compile_and_run(
        r#"
            package main

            var initialized = 0

            func init() { initialized++ }
            func init() { initialized++ }

            func main() { println(initialized) }
        "#,
    );

    assert_eq!(run.stderr, b"2\n");
}

#[test]
fn package_initializer_fragments_follow_filename_then_source_order() {
    let run = compile_program_and_run(raw_program_files([
        (
            "zz_main.go",
            "/checkout/zz_main.go",
            "package main\nfunc main() { println(initialized) }\n",
        ),
        (
            "bb_second.go",
            "/checkout/bb_second.go",
            "package main\nfunc init() { initialized = 12 }\n",
        ),
        (
            "aa_first.go",
            "/checkout/aa_first.go",
            "package main\nvar initialized = 0\nfunc init() { initialized = 1 }\n",
        ),
    ]));

    assert_eq!(run.stderr, b"12\n");
}

#[test]
fn initializer_fragment_comment_edits_reuse_variable_and_function_semantics() {
    let before = r#"
        package main
        var initialized = 0
        func init() { initialized++ }
        func init() { initialized++ }
        func main() { println(initialized) }
    "#;
    let after = r#"
        package main
        var initialized = 0
        // Physical layout changed; initializer semantics did not.
        func init() { initialized++ }
        func init() { initialized++ }
        func main() { println(initialized) }
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
    assert_eq!(telemetry.executions(QueryKind::TypedVariable), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
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
