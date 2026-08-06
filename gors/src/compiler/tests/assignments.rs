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
