use super::{compile_and_run, compile_program, raw_program};

#[test]
fn labels_and_defer_share_one_verified_control_flow_graph() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                value := 0
                captured := value
                defer func(deferred int) { println(deferred) }(captured)
                values := []int{1}
            Loop:
                value += values[0]
                if value < 2 {
                    goto Loop
                }
                println(value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"2\n0\n");
}

#[test]
fn goto_may_leave_a_nested_block_without_introducing_target_variables() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                value := 0
            Again:
                value++
                if value < 2 {
                    nested := 99
                    _ = nested
                    goto Again
                }
                println(value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"2\n");
}

#[test]
fn goto_may_not_enter_a_nested_lexical_block() {
    let error = compile_program(raw_program(
        "goto.go",
        "/checkout/goto.go",
        r#"
            package main

            func main() {
                goto Nested
                {
                Nested:
                    println("unreachable")
                }
            }
        "#,
    ))
    .err()
    .expect("goto into a nested block must be rejected");

    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(diagnostic.message, "goto Nested jumps into block");
    assert_eq!(diagnostic.file, "/checkout/goto.go");
    assert!(diagnostic.line > 0);
}

#[test]
fn goto_may_not_skip_a_variable_declaration_in_its_block() {
    let error = compile_program(raw_program(
        "goto.go",
        "/checkout/goto.go",
        r#"
            package main

            func main() {
                goto Done
                value := 1
            Done:
                println(value)
            }
        "#,
    ))
    .err()
    .expect("goto across a variable declaration must be rejected");

    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(
        diagnostic.message,
        "goto Done jumps over declaration of value"
    );
    assert_eq!(diagnostic.file, "/checkout/goto.go");
    assert!(diagnostic.line > 0);
}
