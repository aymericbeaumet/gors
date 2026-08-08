use super::{compile_and_run, raw_program};
use crate::compiler::db::QueryKind;
use crate::compiler::{CompilerSession, compile_file, compile_program};

#[test]
fn append_evaluates_every_operand_before_one_growth_decision() {
    let run = compile_and_run(
        r#"
            package main

            func destination(order, values []int) []int {
                order[0] = order[0]*10 + 1
                return values
            }

            func element(order []int, value int) int {
                order[0] = order[0]*10 + value
                return value
            }

            func main() {
                base := make([]int, 1, 2)
                base[0] = 1
                oldBacking := base[:2]
                oldBacking[1] = 99
                order := []int{0}

                result := append(destination(order, base), element(order, 2), element(order, 3))
                if order[0] != 123 || result[0] != 1 || result[1] != 2 || result[2] != 3 {
                    panic("append evaluation order changed")
                }
                if oldBacking[1] != 99 {
                    panic("append mutated insufficient backing before growth")
                }
                println("append-order: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"append-order: ok\n");
    assert!(
        run.rust.contains("go_slice_i64_append_slice"),
        "{}",
        run.rust
    );
}

#[test]
fn append_snapshots_overlap_and_boxes_mixed_interface_elements() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                backing := make([]int, 4, 8)
                backing[0], backing[1], backing[2], backing[3] = 1, 2, 3, 4
                result := append(backing[1:3], backing[:4]...)
                if len(result) != 6 || result[0] != 2 || result[1] != 3 ||
                    result[2] != 1 || result[3] != 2 || result[4] != 3 || result[5] != 4 {
                    panic("overlapping append changed")
                }

                var mixed []any
                mixed = append(mixed, 42, 3.1415, "foo")
                if len(mixed) != 3 || mixed[0] != 42 || mixed[1] != 3.1415 || mixed[2] != "foo" {
                    panic("interface append changed")
                }
                println("append-overlap-interface: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"append-overlap-interface: ok\n");
    assert!(
        run.rust.contains("go_slice_interface_append"),
        "{}",
        run.rust
    );
}

#[test]
fn append_preserves_named_types_zero_identity_and_resolved_nil() {
    let run = compile_and_run(
        r#"
            package main

            type Flags []bool
            type Ints []int
            type Other []int
            type Byte byte
            type ByteValues []Byte
            type Bytes []byte
            type Text string

            func appendPredeclaredNil(values []int) []int {
                return append(values, nil...)
            }

            func main() {
                var flags Flags
                if append(flags) != nil {
                    panic("zero append changed nil named slice")
                }

                ints := append(Ints{1}, Other{2, 3}...)
                if len(ints) != 3 || ints[2] != 3 {
                    panic("named integer spread changed")
                }

                var byteDestination Bytes
                bytes := append(byteDestination, Text("go")...)
                if len(bytes) != 2 || bytes[0] != 'g' || bytes[1] != 'o' {
                    panic("named string spread changed")
                }
                var byteValueDestination ByteValues
                byteValues := append(byteValueDestination, Byte(7), Byte(8))
                if len(byteValues) != 2 || byteValues[1] != 8 {
                    panic("named byte append changed")
                }

                nil := []int{4, 5}
                shadowed := append([]int{1}, nil...)
                if len(shadowed) != 3 || shadowed[1] != 4 || shadowed[2] != 5 {
                    panic("shadowed nil was treated as predeclared")
                }
                unchanged := appendPredeclaredNil(shadowed)
                if len(unchanged) != 3 || unchanged[2] != 5 {
                    panic("predeclared nil spread changed")
                }
                println("append-named-nil: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"append-named-nil: ok\n");
}

#[test]
fn append_without_elements_preserves_non_nil_header_without_runtime_call() {
    let run = compile_and_run(
        r#"
            package main

            type Ints []int

            func main() {
                base := Ints{1}
                same := append(base)
                base[0] = 7
                if len(same) != 1 || same[0] != 7 {
                    panic("zero append changed the non-nil slice header")
                }
                println("append-zero-header: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"append-zero-header: ok\n");
    assert!(!run.rust.contains("go_slice_i64_append"), "{}", run.rust);
}

#[test]
fn append_rejects_mismatched_defined_spread_elements_and_string_targets() {
    for source in [
        "package main\ntype E int\nfunc main() { var d []E; var s []int; _ = append(d, s...) }\n",
        "package main\ntype E byte\ntype U string\nfunc main() { var d []E; var s U; _ = append(d, s...) }\n",
    ] {
        let errors = compile_file("main.go", source)
            .err()
            .expect("an invalid append spread must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("append spread source")),
            "{errors:?}"
        );
    }
}

#[test]
fn append_resolves_an_import_binding_named_nil_before_the_predeclared_identifier() {
    let error = compile_program(raw_program(
        "main.go",
        "main.go",
        r#"
            package main
            import nil "unsafe"
            func main() {
                var value int
                _ = nil.Sizeof(value)
                var values []int
                _ = append(values, nil...)
            }
        "#,
    ))
    .err()
    .expect("a package import named nil must shadow the predeclared identifier");

    assert!(
        error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("undefined identifier nil")),
        "{error}"
    );
}

#[test]
fn append_comment_edits_preserve_all_semantic_products() {
    let before = r#"
        package main
        func values() []int { return append([]int{1}, 2, 3) }
        func main() { println(len(values())) }
    "#;
    let after = r#"
        package main
        func values() []int {
            // The append operation is semantically unchanged.
            return append([]int{1}, 2, 3)
        }
        func main() { println(len(values())) }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    let scheduler = session.scheduler_telemetry();
    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    let telemetry = session.database().telemetry();
    for kind in [
        QueryKind::TypedHir,
        QueryKind::VerifiedGoMir,
        QueryKind::NormalizedGoMir,
        QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 0, "{kind:?}");
    }
    assert_eq!(session.scheduler_telemetry(), scheduler);
}

#[test]
fn append_semantic_edits_invalidate_only_the_owning_root() {
    let before = r#"
        package main
        func values() []int { return append([]int{1}, 2, 3) }
        func count() int { return len(values()) }
        func untouched() int { return 7 }
        func main() { println(count() + untouched()) }
    "#;
    let after = r#"
        package main
        func values() []int { return append([]int{1}, 2, 4) }
        func count() int { return len(values()) }
        func untouched() int { return 7 }
        func main() { println(count() + untouched()) }
    "#;
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", before))
        .unwrap();
    let scheduled = session.scheduler_telemetry().scheduled_roots;
    session.database().reset_telemetry();
    session
        .compile_program(raw_program("main.go", "main.go", after))
        .unwrap();

    assert_eq!(session.scheduler_telemetry().scheduled_roots, scheduled + 1);
    let telemetry = session.database().telemetry();
    for kind in [
        QueryKind::TypedHir,
        QueryKind::VerifiedGoMir,
        QueryKind::NormalizedGoMir,
        QueryKind::VerifiedRustIr,
    ] {
        assert_eq!(telemetry.executions(kind), 1, "{kind:?}");
    }
}
