use super::compile_and_run;

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
