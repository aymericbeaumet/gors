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

#[test]
fn generated_variadic_calls_pack_final_arguments_after_fixed_arguments() {
    let run = compile_and_run(
        r#"
            package main

            func total(base int, values ...int) int {
                for _, value := range values {
                    base += value
                }
                return base
            }

            func main() {
                order := []int{}
                next := func(value int) int {
                    order = append(order, value)
                    return value
                }
                packed := total(next(10), next(1), next(2))
                spread := total(20, []int{3, 4}...)
                if packed != 13 || spread != 27 {
                    panic("variadic values changed")
                }
                if order[0] != 10 || order[1] != 1 || order[2] != 2 {
                    panic("variadic argument order changed")
                }
                println("variadic-pack: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"variadic-pack: ok\n");
    assert!(run.rust.contains("go_slice_i64_make"), "{}", run.rust);
}
