use std::sync::Arc;

use super::{compile_and_run, raw_program};
use crate::compiler::db::{CompilerDatabase, QueryKind};
use crate::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};
use crate::compiler::{CompilerSession, compile_program, fingerprint};

#[test]
fn method_constraints_infer_results_iteratively_through_exact_method_sets() {
    let run = compile_and_run(
        r#"
            package main

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](value P) T { return value.Produce() }

            type Word struct{}
            func (Word) Produce() string { return "word" }

            type Number struct{}
            func (Number) Produce() int { return 7 }

            type Embedded struct { Word }

            type StringProducer interface { Produce() string }
            func named[P StringProducer](value P) string { return value.Produce() }

            func main() {
                if produce(Word{}) != "word" || produce(Number{}) != 7 {
                    panic("method constraint inference changed")
                }
                if produce(Embedded{}) != "word" || named(Embedded{}) != "word" {
                    panic("promoted method constraint inference changed")
                }
                println("method-constraint-inference: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"method-constraint-inference: ok\n");
}

#[test]
fn generic_receiver_methods_satisfy_direct_and_promoted_constraints() {
    let run = compile_and_run(
        r#"
            package main

            type Box[T any] struct { value T }
            func (box Box[T]) Produce() T { return box.value }
            type Wrapped[T any] struct { Box[T] }

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](value P) T { return value.Produce() }

            func main() {
                var wrapped Wrapped[int]
                println(produce(Box[int]{value: 7}), produce(wrapped))
            }
        "#,
    );

    assert_eq!(run.stderr, b"7 0\n");
}

#[test]
fn generic_pointer_receiver_methods_satisfy_pointer_method_sets() {
    let run = compile_and_run(
        r#"
            package main

            type Box[T any] struct { value T }
            func (box *Box[T]) Produce() T { return box.value }

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](value P) T { return value.Produce() }

            func main() {
                box := Box[int]{value: 11}
                println(produce(&box))
            }
        "#,
    );

    assert_eq!(run.stderr, b"11\n");
}

#[test]
fn method_constraints_find_types_through_local_bindings() {
    let run = compile_and_run(
        r#"
            package main

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](value P) T { return value.Produce() }
            type Used struct{}
            func (Used) Produce() int { return 7 }
            type Holder struct { value Used }

            func fromParameter(value Used) int { return produce(value) }

            func main() {
                var declared Used
                inferred := Used{}
                holder := Holder{}
                println(produce(declared), produce(inferred), fromParameter(Used{}), produce(holder.value))
            }
        "#,
    );

    assert_eq!(run.stderr, b"7 7 7 7\n");
}

#[test]
fn unresolved_constraint_receiver_fallback_is_per_type_parameter() {
    let run = compile_and_run(
        r#"
            package main

            type Marker struct { value int }
            type Producer[T any] interface { Produce() T }
            func use[P Producer[T], T any](producer P, marker T) T {
                return producer.Produce()
            }
            type Used struct{}
            func (Used) Produce() Marker { return Marker{value: 7} }
            type Holder struct { value Used }

            func main() {
                holder := Holder{value: Used{}}
                println(use(holder.value, Marker{}).value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"7\n");
}

#[test]
fn unresolved_nested_generic_call_results_fall_back_by_constraint_method() {
    let run = compile_and_run(
        r#"
            package main

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](producer P) T { return producer.Produce() }
            func identity[T any](value T) T { return value }
            type Used struct{}
            func (Used) Produce() int { return 7 }

            func main() { println(produce(identity(Used{}))) }
        "#,
    );

    assert_eq!(run.stderr, b"7\n");
}

#[test]
fn anonymous_promoted_generic_type_arguments_use_constraint_fallback() {
    let run = compile_and_run(
        r#"
            package main

            type Box[P interface { Produce() int }] struct { value P }
            type Used struct{}
            func (Used) Produce() int { return 7 }

            func main() {
                var value Box[struct { Used }]
                _ = value
                println("anonymous-promoted: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"anonymous-promoted: ok\n");
}

#[test]
fn promoted_generic_receiver_dependencies_follow_underlying_chains() {
    let run = compile_and_run(
        r#"
            package main

            type Producer[T any] interface { Produce() T }
            func produce[P Producer[T], T any](producer P) T { return producer.Produce() }
            type Word struct{}
            func (Word) Produce() int { return 7 }
            type Base[T any] struct { Word }
            type Wrapper[T any] Base[T]

            func main() { println(produce(Wrapper[int]{})) }
        "#,
    );

    assert_eq!(run.stderr, b"7\n");
}

#[test]
fn inline_generic_type_constraints_load_the_instantiated_method_set() {
    let run = compile_and_run(
        r#"
            package main

            type Box[P interface { Produce() int }] struct { value P }
            type Word struct{}
            func (Word) Produce() int { return 7 }

            func main() {
                value := Box[Word]{value: Word{}}
                println(value.value.Produce())
            }
        "#,
    );

    assert_eq!(run.stderr, b"7\n");
}

#[test]
fn explicit_function_type_arguments_load_and_validate_method_sets() {
    let run = compile_and_run(
        r#"
            package main

            type StringProducer interface { Produce() string }
            func accept[P StringProducer]() string { var value P; return value.Produce() }
            type Used struct{}
            func (Used) Produce() string { return "used" }

            func main() { println(accept[Used]()) }
        "#,
    );

    assert_eq!(run.stderr, b"used\n");

    assert_method_constraint_error(
        r#"package main
type StringProducer interface { Produce() string }
func accept[P StringProducer]() {}
type Wrong struct{}
func (Wrong) Produce() int { return 1 }
func main() {
    accept[Wrong]()
}
"#,
        7,
        "constraint requires",
    );
}

#[test]
fn method_constraints_infer_defined_types_before_defaulting_untyped_constants() {
    let run = compile_and_run(
        r#"
            package main

            type MyInt int
            type Concrete struct{}
            func (Concrete) Produce() MyInt { return 0 }
            type Producer[T any] interface { Produce() T }
            func use[P Producer[T], T any](producer P, value T) T { return value }

            func main() { println(use(Concrete{}, 7)) }
        "#,
    );

    assert_eq!(run.stderr, b"7\n");

    assert_method_constraint_error(
        r#"package main
type MyInt int
type Concrete struct{}
func (Concrete) Produce() MyInt { return 0 }
type Producer[T any] interface { Produce() T }
func use[P Producer[T], T any](producer P, value T) T { return value }
func main() {
    var value int
    _ = use(Concrete{}, value)
}
"#,
        9,
        "does not satisfy the constraint for P",
    );
}

#[test]
fn generic_receiver_constraint_failures_are_precise() {
    assert_method_constraint_error(
        r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Box[T any] struct { value T }
func (*Box[T]) Produce() T { var zero T; return zero }
func main() {
    _ = produce(Box[int]{})
}
"#,
        7,
        "has no field or method Produce",
    );

    assert_method_constraint_error(
        r#"package main
type StringProducer interface { Produce() string }
func accept[P StringProducer](value P) {}
type Box[T any] struct { value T }
func (Box[T]) Produce() T { var zero T; return zero }
func main() {
    accept(Box[int]{})
}
"#,
        7,
        "constraint requires",
    );

    assert_method_constraint_diagnostic(
        r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Phantom[T any] struct{}
func (Phantom[T]) Produce() T { var zero T; return zero }
func main() {
    _ = produce(Phantom[int]{})
}
"#,
        "GORS2001",
        7,
        "cannot recover generic receiver type arguments T",
    );
}

#[test]
fn method_constraint_inference_rejects_missing_methods_at_the_call() {
    assert_method_constraint_error(
        r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Missing struct{}
func main() {
    _ = produce(Missing{})
}
"#,
        6,
        "has no field or method Produce",
    );
}

#[test]
fn named_method_constraints_reject_wrong_exact_signatures() {
    assert_method_constraint_error(
        r#"package main
type StringProducer interface { Produce() string }
func accept[P StringProducer](value P) {}
type Wrong struct{}
func (Wrong) Produce() int { return 1 }
func main() {
    accept(Wrong{})
}
"#,
        7,
        "constraint requires",
    );
}

#[test]
fn method_constraint_inference_excludes_pointer_only_methods_from_value_method_sets() {
    assert_method_constraint_error(
        r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Word struct{}
func (*Word) Produce() string { return "word" }
func main() {
    _ = produce(Word{})
}
"#,
        7,
        "has no field or method Produce",
    );
}

#[test]
fn method_constraint_inference_rejects_conflicting_method_equations() {
    assert_method_constraint_error(
        r#"package main
type Both[T any] interface { First() T; Second() T }
func infer[P Both[T], T any](value P) T { return value.First() }
type Conflict struct{}
func (Conflict) First() int { return 1 }
func (Conflict) Second() string { return "two" }
func main() {
    _ = infer(Conflict{})
}
"#,
        8,
        "conflicting inferred types for T",
    );
}

#[test]
fn method_constraint_inference_rejects_ambiguous_promoted_methods() {
    assert_method_constraint_error(
        r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Left struct{}
func (Left) Produce() int { return 1 }
type Right struct{}
func (Right) Produce() int { return 2 }
type Ambiguous struct { Left; Right }
func main() {
    _ = produce(Ambiguous{})
}
"#,
        10,
        "selector Produce is ambiguous",
    );
}

fn assert_method_constraint_error(source: &str, line: usize, message: &str) {
    assert_method_constraint_diagnostic(source, "GORS2002", line, message);
}

fn assert_method_constraint_diagnostic(source: &str, code: &str, line: usize, message: &str) {
    let error = compile_program(raw_program("generics.go", "/checkout/generics.go", source))
        .err()
        .expect("invalid method constraint call must be rejected");
    let diagnostic = error
        .diagnostics()
        .first()
        .expect("method constraint failure must have a diagnostic");
    assert_eq!(diagnostic.code, code, "{diagnostic:?}");
    assert!(diagnostic.message.contains(message), "{diagnostic:?}");
    assert_eq!(diagnostic.file, "/checkout/generics.go");
    assert_eq!(diagnostic.line, line, "diagnostic must point at the call");
}

#[test]
fn method_constraint_dependencies_select_only_the_actual_method_set() {
    const BASE: &str = r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Used struct{}
func (Used) Produce() int { return 1 }
type Unrelated struct{}
func (Unrelated) Produce() int { return 2 }
func main() { _ = produce(Used{}); _ = Unrelated{} }
"#;
    const UNRELATED_EDIT: &str = r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Used struct{}
func (Used) Produce() int { return 1 }
type Unrelated struct{}
func (Unrelated) Produce() string { return "two" }
func main() { _ = produce(Used{}); _ = Unrelated{} }
"#;
    const SELECTED_EDIT: &str = r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Used struct{}
func (Used) Produce() string { return "one" }
type Unrelated struct{}
func (Unrelated) Produce() string { return "two" }
func main() { _ = produce(Used{}); _ = Unrelated{} }
"#;

    assert_dependency_selection(BASE, UNRELATED_EDIT, SELECTED_EDIT);
}

#[test]
fn named_method_constraint_dependencies_select_only_the_actual_method_set() {
    const BASE: &str = r#"package main
type IntProducer interface { Produce() int }
func accept[P IntProducer](value P) int { return value.Produce() }
type Used struct{}
func (Used) Produce() int { return 1 }
type Unrelated struct{}
func (Unrelated) Produce() int { return 2 }
func main() { _ = accept(Used{}); _ = Unrelated{} }
"#;
    const UNRELATED_EDIT: &str = r#"package main
type IntProducer interface { Produce() int }
func accept[P IntProducer](value P) int { return value.Produce() }
type Used struct{}
func (Used) Produce() int { return 1 }
type Unrelated struct{}
func (Unrelated) Produce() string { return "two" }
func main() { _ = accept(Used{}); _ = Unrelated{} }
"#;
    const SELECTED_EDIT: &str = r#"package main
type StringProducer interface { Produce() string }
func accept[P StringProducer](value P) string { return value.Produce() }
type Used struct{}
func (Used) Produce() string { return "one" }
type Unrelated struct{}
func (Unrelated) Produce() string { return "two" }
func main() { _ = accept(Used{}); _ = Unrelated{} }
"#;

    assert_dependency_selection(BASE, UNRELATED_EDIT, SELECTED_EDIT);
}

#[test]
fn constraint_dependency_method_receiver_pairs_do_not_cross() {
    const BASE: &str = r#"package main
type Producer[T any] interface { Produce() T }
type Consumer[T any] interface { Consume() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
func consume[C Consumer[T], T any](value C) T { return value.Consume() }
type Used struct{}
func (Used) Produce() int { return 1 }
func (Used) Consume() int { return 2 }
type Other struct{}
func (Other) Consume() int { return 3 }
func main() { _ = produce(Used{}); _ = consume(Other{}) }
"#;
    const CROSS_PAIR_EDIT: &str = r#"package main
type Producer[T any] interface { Produce() T }
type Consumer[T any] interface { Consume() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
func consume[C Consumer[T], T any](value C) T { return value.Consume() }
type Used struct{}
func (Used) Produce() int { return 1 }
func (Used) Consume() string { return "two" }
type Other struct{}
func (Other) Consume() int { return 3 }
func main() { _ = produce(Used{}); _ = consume(Other{}) }
"#;

    assert_unrelated_signature_edit_stays_green(BASE, CROSS_PAIR_EDIT);
}

#[test]
fn constraint_receiver_dependencies_ignore_non_embedded_field_types() {
    const BASE: &str = r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Payload struct{}
func (Payload) Produce() int { return 1 }
type Holder struct { payload Payload }
func (Holder) Produce() int { return 7 }
func main() { _ = produce(Holder{}) }
"#;
    const FIELD_METHOD_EDIT: &str = r#"package main
type Producer[T any] interface { Produce() T }
func produce[P Producer[T], T any](value P) T { return value.Produce() }
type Payload struct{}
func (Payload) Produce() string { return "one" }
type Holder struct { payload Payload }
func (Holder) Produce() int { return 7 }
func main() { _ = produce(Holder{}) }
"#;

    assert_unrelated_signature_edit_stays_green(BASE, FIELD_METHOD_EDIT);
}

fn assert_unrelated_signature_edit_stays_green(base: &str, edited: &str) {
    let workspace = WorkspaceKey::ad_hoc("compiler-tests").unwrap();
    let package = PackageKey::command_line();
    let mut database = CompilerDatabase::default();
    let file = database
        .set_source(
            &workspace,
            &package,
            "main.go",
            Arc::new(SourceSnapshot::from_source("main.go", base).unwrap()),
        )
        .unwrap();
    let file = file.file();
    let main = database
        .analyze_file(file)
        .unwrap()
        .functions()
        .iter()
        .find(|function| function.name() == "main")
        .expect("main definition")
        .id();
    let before = database.typed_hir(file, main).unwrap();

    database.reset_telemetry();
    let update = database
        .set_source(
            &workspace,
            &package,
            "main.go",
            Arc::new(SourceSnapshot::from_source("main.go", edited).unwrap()),
        )
        .unwrap();
    assert_eq!(update.file(), file);
    let after = database.typed_hir(file, main).unwrap();

    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(
        database.telemetry().executions(QueryKind::TypedHir),
        0,
        "an unselected method signature must not reexecute typed HIR"
    );
}

fn assert_dependency_selection(base: &str, unrelated_edit: &str, selected_edit: &str) {
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", base))
        .unwrap();
    let file = *session
        .database()
        .active_files()
        .first()
        .expect("compiled method constraint source file");
    let main = session
        .database()
        .analyze_file(file)
        .unwrap()
        .functions()
        .iter()
        .find(|function| function.name() == "main")
        .expect("main definition")
        .id();
    let before = session.database().typed_hir(file, main).unwrap();
    let before_fingerprint = fingerprint::hir_function(before.function());

    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", unrelated_edit))
        .unwrap();
    let after_unrelated = session.database().typed_hir(file, main).unwrap();
    assert!(Arc::ptr_eq(&before, &after_unrelated));
    assert_eq!(
        before_fingerprint,
        fingerprint::hir_function(after_unrelated.function())
    );

    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", selected_edit))
        .unwrap();
    let after_selected = session.database().typed_hir(file, main).unwrap();
    assert!(!Arc::ptr_eq(&after_unrelated, &after_selected));
    assert_ne!(
        before_fingerprint,
        fingerprint::hir_function(after_selected.function())
    );
    assert!(
        session
            .database()
            .telemetry()
            .executions(QueryKind::TypedHir)
            > 0,
        "the selected method signature edit must recheck typed HIR"
    );
}
