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

#[test]
fn labeled_fallthrough_preserves_labels_and_source_order() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                trace := ""
                switch 5 {
                case 1:
                    trace += "one,"
                default:
                    trace += "default,"
                    goto Next
                Next:
                    fallthrough
                case 2:
                    trace += "two,"
                }
                println(trace)
            }
        "#,
    );

    assert_eq!(run.stderr, b"default,two,\n");
}

#[test]
fn labeled_fallthrough_must_be_final_and_have_a_following_clause() {
    let non_final = compile_program(raw_program(
        "fallthrough.go",
        "/checkout/fallthrough.go",
        r#"
            package main
            func main() {
                switch 1 {
                case 1:
                Label:
                    fallthrough
                    println("not final")
                case 2:
                }
            }
        "#,
    ))
    .err()
    .expect("a non-final labeled fallthrough must be rejected");
    assert!(
        non_final
            .to_string()
            .contains("fallthrough must be the final non-empty statement"),
        "{non_final}"
    );

    let final_case = compile_program(raw_program(
        "fallthrough.go",
        "/checkout/fallthrough.go",
        r#"
            package main
            func main() {
                switch 1 {
                case 1:
                Label:
                    fallthrough
                }
            }
        "#,
    ))
    .err()
    .expect("a final-case labeled fallthrough must be rejected");
    assert!(
        final_case
            .to_string()
            .contains("the final switch case cannot fall through"),
        "{final_case}"
    );
}
