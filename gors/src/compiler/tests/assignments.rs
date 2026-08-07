use super::{compile_and_run, compile_file};

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
