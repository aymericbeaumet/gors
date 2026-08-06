use super::compile_and_run;

#[test]
fn generated_dynamic_slice_literals_evaluate_elements_left_to_right_once() {
    let run = compile_and_run(
        r#"
            package main
            func main() {
                order := []int{}
                next := func(value int) int {
                    order = append(order, value)
                    return value * 2
                }
                values := []int{next(1), next(2), next(3)}
                if values[0] != 2 || values[1] != 4 || values[2] != 6 {
                    panic("dynamic slice literal values changed")
                }
                if order[0] != 1 || order[1] != 2 || order[2] != 3 {
                    panic("dynamic slice literal order changed")
                }
                println("dynamic-slice: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"dynamic-slice: ok\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
    assert!(run.rust.contains("go_slice_i64_set"), "{}", run.rust);
}
