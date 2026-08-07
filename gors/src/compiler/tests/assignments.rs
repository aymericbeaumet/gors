use super::{CompilerSession, compile_and_run, compile_file, raw_program_files};
use crate::compiler::db::QueryKind;

#[test]
fn map_element_compound_assignment_and_incdec_evaluate_operands_once() {
    let run = compile_and_run(
        r#"
            package main
            func nilCompoundWritePanics() (panicked bool) {
                defer func() { panicked = recover() != nil }()
                var missing map[string]int
                missing["value"] += 1
                return false
            }
            func main() {
                counts := map[string]int{"a": 1}
                calls := 0
                key := func() string {
                    calls++
                    return "a"
                }
                counts[key()] += 2
                counts["a"]++
                counts["missing"]++
                counts["negative"] -= 3
                if calls != 1 { panic("key evaluated more than once") }
                if counts["a"] != 4 || counts["missing"] != 1 || counts["negative"] != -3 {
                    panic("compound map assignment changed")
                }
                if !nilCompoundWritePanics() { panic("nil map compound write did not panic") }
                println("map-compound: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"map-compound: ok\n");
    assert!(run.rust.contains("go_map_string_i64_get"), "{}", run.rust);
    assert!(run.rust.contains("go_map_string_i64_set"), "{}", run.rust);
}

#[test]
fn map_element_compound_assignment_rejects_mismatched_values() {
    let errors = compile_file(
        "main.go",
        r#"package main
func main() {
    counts := map[string]int{"a": 1}
    counts["a"] += "text"
    println(counts["a"])
}
"#,
    )
    .err()
    .expect("a non-integer compound map operand must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("cannot use")),
        "{errors:?}"
    );
}

#[test]
fn tuple_assignment_prepares_dynamic_targets_before_the_call() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                values := map[string]int{"old": 1}
                slot := 0
                flags := []bool{false}
                slot, flags[slot] = values["old"]
                println(slot, flags[0])
            }
        "#,
    );

    assert_eq!(run.stderr, b"1 true\n");
    assert!(
        run.rust.contains("go_map_string_i64_contains"),
        "{}",
        run.rust
    );
    assert!(run.rust.contains("go_slice_bool_set"), "{}", run.rust);
}

#[test]
fn interface_assertions_support_checked_and_comma_ok_forms() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                var value any = 42
                number, numberOK := value.(int)
                text, textOK := value.(string)
                println(value.(int), number, numberOK, text == "", textOK)
            }
        "#,
    );

    assert_eq!(run.stderr, b"42 42 true true false\n");
    assert!(run.rust.contains("go_interface_is_type"), "{}", run.rust);
    assert!(run.rust.contains("go_interface_unbox_i64"), "{}", run.rust);
}

#[test]
fn pointer_assignment_targets_are_frozen_before_writes() {
    let run = compile_and_run(
        r#"
            package main
            type holder struct { value int }
            func main() {
                first, second := 4, 5
                pointer, replacement := &first, &second
                values := func() (*int, int) { return replacement, 8 }
                pointer, *pointer = values()

                oldHolder, newHolder := holder{value: 4}, holder{value: 5}
                holderPointer, holderReplacement := &oldHolder, &newHolder
                holderPointer, holderPointer.value = holderReplacement, 9

                println(first, second, *pointer)
                println(oldHolder.value, newHolder.value, holderPointer.value)
            }
        "#,
    );

    assert_eq!(run.stderr, b"8 5 5\n9 5 5\n");
    assert!(run.rust.contains("go_pointer_i64_set"), "{}", run.rust);
    assert!(
        run.rust.contains("go_pointer_struct_i64_set"),
        "{}",
        run.rust
    );
}

#[test]
fn assignment_targets_share_one_ordered_prepare_read_write_lifecycle() {
    let run = compile_and_run(
        r#"
            package main

            type leaf struct { value int; keep int }
            type middle struct { leaf; sibling int }
            type outer struct { middle; tail int }

            func main() {
                order := 0
                values := []int{3}
                index := func() int {
                    order = order*10 + 1
                    return 0
                }
                right := func() int {
                    order = order*10 + 2
                    values[0] = 40
                    return 2
                }
                values[index()] += right()
                if order != 12 || values[0] != 5 {
                    panic("slice compound assignment order changed")
                }

                order = 0
                values[index()]++
                if order != 1 || values[0] != 6 {
                    panic("slice increment evaluated its target more than once")
                }

                var array [1]int
                array[0] = 5
                order = 0
                arrayRight := func() int {
                    order = order*10 + 2
                    array[0] = 40
                    return 3
                }
                array[index()] += arrayRight()
                if order != 12 || array[0] != 8 {
                    panic("array compound assignment order changed")
                }

                record := outer{
                    middle: middle{leaf: leaf{value: 1, keep: 2}, sibling: 3},
                    tail: 4,
                }
                recordRight := func() int {
                    record.sibling = 9
                    return 8
                }
                record.value = recordRight()
                if record.value != 8 || record.keep != 2 || record.sibling != 9 || record.tail != 4 {
                    panic("nested struct assignment did not preserve sibling writes")
                }

                ranged := []int{10, 20}
                destination := []int{0}
                rangeIndex := 1
                rangeTarget := func() int {
                    ranged[0] = 99
                    return 0
                }
                for rangeIndex, destination[rangeTarget()] = range ranged {
                    break
                }
                if rangeIndex != 0 || destination[0] != 99 {
                    panic("range target preparation order changed")
                }
                println("assignment-targets: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"assignment-targets: ok\n");
    assert!(run.rust.contains("go_slice_i64_set"), "{}", run.rust);
}

#[test]
fn unsupported_nested_assignment_paths_have_a_source_diagnostic() {
    let errors = compile_file(
        "main.go",
        r#"package main
type item struct { value int }
type holder struct { item }
func main() {
    value := holder{item: item{value: 1}}
    pointer := &value
    pointer.item.value = 2
}

"#,
    )
    .err()
    .expect("an unrepresented nested assignment path must be rejected");

    assert!(
        errors.iter().any(|error| {
            error.code == "GORS2001"
                && error
                    .message
                    .contains("assignment through this nested selector path")
        }),
        "{errors:?}"
    );
}

#[test]
fn range_assignments_apply_explicit_value_coercions() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                numbers := []int{7}
                var index any
                var element any
                for index, element = range numbers {
                    break
                }
                if index.(int) != 0 || element.(int) != 7 {
                    panic("slice range assignment coercion changed")
                }

                words := map[int]string{3: "three"}
                var key any
                var value any
                for key, value = range words {
                    break
                }
                if key.(int) != 3 || value.(string) != "three" {
                    panic("map range assignment coercion changed")
                }
                println("range-coercions: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"range-coercions: ok\n");
    assert!(run.rust.contains("go_interface_box_i64"), "{}", run.rust);
    assert!(
        run.rust.contains("go_interface_box_go_string"),
        "{}",
        run.rust
    );
}

#[test]
fn unrelated_body_edits_do_not_invalidate_assignment_target_roots() {
    let input = |helper_value| {
        raw_program_files([
            (
                "main.go",
                "main.go",
                "package main\ntype inner struct { value int }\ntype outer struct { inner }\nfunc stable() int { values := []int{1}; values[0]++; record := outer{}; record.value = values[0]; return record.value }\nfunc main() { println(stable(), helper()) }\n",
            ),
            ("helper.go", "helper.go", helper_value),
        ])
    };
    let mut session = CompilerSession::default();
    session
        .compile_program(input("package main\nfunc helper() int { return 1 }\n"))
        .unwrap();
    session.database().reset_telemetry();

    session
        .compile_program(input("package main\nfunc helper() int { return 2 }\n"))
        .unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
}
