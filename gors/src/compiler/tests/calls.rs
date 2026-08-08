use super::{CompilerSession, compile_and_run, compile_file, raw_program};
use crate::compiler::db::QueryKind;

#[test]
fn sole_multi_result_calls_forward_once_into_fixed_and_variadic_parameters() {
    let run = compile_and_run(
        r#"
            package main

            func pair(calls *int) (int, int) {
                *calls = *calls + 1
                return 2, 3
            }
            func triple() (int, int, int) { return 1, 2, 3 }
            func add(left, right int) int { return left + right }
            func summarize(head int, rest ...int) (int, int, int) {
                total := 0
                for _, value := range rest { total += value }
                return head, total, cap(rest)
            }
            func fixedTail(left, right int, rest ...int) (int, bool) {
                return left + right, rest == nil
            }
            func next() (int, int) { return 8, 9 }
            func swap(left, right int) (int, int) { return right, left }
            func all(values ...int) (int, int) {
                return len(values), cap(values)
            }

            func main() {
                calls := 0
                if add(pair(&calls)) != 5 || calls != 1 {
                    panic("fixed forwarding changed evaluation")
                }
                head, total, capacity := summarize(triple())
                if head != 1 || total != 5 || capacity != 2 {
                    panic("variadic remainder was not packed exactly")
                }
                sum, nilTail := fixedTail(pair(&calls))
                if sum != 5 || !nilTail || calls != 2 {
                    panic("empty variadic remainder was not nil")
                }
                length, allCapacity := all(triple())
                if length != 3 || allCapacity != 3 {
                    panic("all forwarded results were not packed")
                }
                if add(swap(next())) != 17 {
                    panic("nested forwarding changed")
                }
                println("forwarded-calls: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"forwarded-calls: ok\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_i64_nil"), "{}", run.rust);
}

#[test]
fn direct_method_receiver_is_evaluated_before_the_forwarded_call() {
    let run = compile_and_run(
        r#"
            package main

            type marker int

            func receiver(order *int) marker {
                *order = *order * 10 + 1
                return marker(0)
            }
            func arguments(order *int) (int, int) {
                *order = *order * 10 + 2
                return 4, 5
            }
            func (marker) consume(left, right int) int { return left + right }

            func main() {
                order := 0
                if receiver(&order).consume(arguments(&order)) != 9 {
                    panic("forwarded method call result changed")
                }
                if order != 12 {
                    panic("method receiver was not evaluated first")
                }
                println("method-forwarding: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"method-forwarding: ok\n");
}

#[test]
fn generic_calls_infer_type_arguments_from_forwarded_results() {
    let run = compile_and_run(
        r#"
            package main

            func pair() (int, int) { return 6, 7 }
            func triple() (int, int, int) { return 1, 2, 3 }
            func add[T ~int](left, right T) T { return left + right }
            func count[T ~int](values ...T) (int, int) {
                return len(values), cap(values)
            }

            func main() {
                if add(pair()) != 13 {
                    panic("generic fixed forwarding changed")
                }
                length, capacity := count(triple())
                if length != 3 || capacity != 3 {
                    panic("generic variadic forwarding changed")
                }
                println("generic-forwarding: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"generic-forwarding: ok\n");
}

#[test]
fn invalid_multi_result_call_argument_forms_are_diagnosed() {
    for (source, expected) in [
        (
            "package main\nfunc pair() (int, int) { return 1, 2 }\nfunc take(int, int) {}\nfunc main() { take(0, pair()) }\n",
            "cannot use",
        ),
        (
            "package main\nfunc pair() (int, int) { return 1, 2 }\nfunc take(int) {}\nfunc main() { take(pair()) }\n",
            "forwarded results",
        ),
        (
            "package main\nfunc pair() (int, int) { return 1, 2 }\nfunc take(string, string) {}\nfunc main() { take(pair()) }\n",
            "forwarded result",
        ),
        (
            "package main\nfunc pair() (int, int) { return 1, 2 }\nfunc take(values ...int) {}\nfunc main() { take(pair()...) }\n",
            "cannot use ...",
        ),
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("invalid forwarded call arguments must fail");
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "expected {expected:?} in {errors:?}"
        );
    }
}

#[test]
fn forwarded_call_comment_edits_do_not_reexecute_semantic_stages() {
    let before = r#"
        package main
        func pair() (int, int) { return 1, 2 }
        func add(left, right int) int { return left + right }
        func main() { println(add(pair())) }
    "#;
    let after = r#"
        package main
        func pair() (int, int) { return 1, 2 }
        func add(left, right int) int { return left + right }
        func main() {
            // Forwarding has unchanged semantic inputs.
            println(add(pair()))
        }
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
