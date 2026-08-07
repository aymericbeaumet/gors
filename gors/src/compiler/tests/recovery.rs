use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_file};

#[test]
fn recover_is_an_ordinary_interface_value_and_consumes_the_active_panic() {
    let run = compile_and_run(
        r#"
            package main

            func recoveredString() (result string) {
                defer func() {
                    indirect := func() {
                        if recover() != nil {
                            panic("indirect recover consumed the panic")
                        }
                    }
                    indirect()
                    recovered := recover()
                    text, ok := recovered.(string)
                    if !ok {
                        panic("wrong recovered string type")
                    }
                    if recover() != nil {
                        panic("panic not consumed")
                    }
                    result = text
                }()
                panic("boom")
            }

            func recoveredInt() (result int) {
                defer func() {
                    recovered := recover()
                    value, ok := recovered.(int)
                    if ok {
                        result = value
                    }
                }()
                panic(42)
            }

            func nilPanic() (nonNil bool, isError bool) {
                defer func() {
                    recovered := recover()
                    nonNil = recovered != nil
                    _, isError = recovered.(error)
                }()
                panic(nil)
            }

            func typedNilPanic() (nonNil bool, isError bool) {
                var failure error
                defer func() {
                    recovered := recover()
                    nonNil = recovered != nil
                    _, isError = recovered.(error)
                }()
                panic(failure)
            }

            func main() {
                nonNil, isError := nilPanic()
                typedNonNil, typedIsError := typedNilPanic()
                println(recoveredString(), recoveredInt(), recover() == nil, nonNil, isError, typedNonNil, typedIsError)
            }
        "#,
    );

    assert_eq!(run.stderr, b"boom 42 true true true true true\n");
    assert!(run.rust.contains("panic_go_interface"), "{}", run.rust);
    assert!(
        run.rust.contains("go_panic_payload_to_interface"),
        "{}",
        run.rust
    );
    assert!(
        run.rust.contains("go_interface_is_runtime_error"),
        "{}",
        run.rust
    );
}

#[test]
fn recover_rejects_arguments_at_the_builtin_boundary() {
    let errors = compile_file(
        "recover.go",
        "package main\nfunc main() { _ = recover(1) }\n",
    )
    .err()
    .expect("recover with an argument must be rejected");

    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("recover requires no arguments")),
        "{errors:?}"
    );
}

#[test]
fn recover_comment_edits_reuse_semantic_stages() {
    let before = r#"
        package main
        func guarded() (result string) {
            defer func() {
                recovered := recover()
                result, _ = recovered.(string)
            }()
            panic("boom")
        }
        func main() { println(guarded()) }
    "#;
    let after = r#"
        package main
        func guarded() (result string) {
            defer func() {
                // Recovery remains the same ordinary interface expression.
                recovered := recover()
                result, _ = recovered.(string)
            }()
            panic("boom")
        }
        func main() { println(guarded()) }
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
