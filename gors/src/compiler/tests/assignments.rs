use super::compile_and_run;

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
