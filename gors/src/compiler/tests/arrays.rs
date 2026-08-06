use super::compile_and_run;

#[test]
fn generated_scalar_arrays_preserve_literals_updates_and_evaluation_order() {
    let run = compile_and_run(
        r#"
            package main

            func main() {
                counter := 0
                next := func() int {
                    counter++
                    return counter
                }

                dynamic := [2]int{1: next(), 0: next()}
                vowels := [128]bool{'a': true, 'e': true}
                filter := [6]float64{-1, 4: -0.1, -0.1}
                days := [...]string{"Sat", "Sun"}
                saved := days

                vowels['a'] = false
                filter[1] = 0.5
                days[0] = "Fri"

                if counter != 2 || dynamic[0] != 2 || dynamic[1] != 1 {
                    panic("array element evaluation order changed")
                }
                if vowels['a'] || !vowels['e'] || vowels['i'] {
                    panic("bool array literal or update changed")
                }
                if filter[0] != -1 || filter[1] != 0.5 || filter[4] != -0.1 || filter[5] != -0.1 {
                    panic("float array literal or update changed")
                }
                if len(days) != 2 || days[0] != "Fri" || saved[0] != "Sat" || saved[1] != "Sun" {
                    panic("string array literal, copy, or update changed")
                }
                println("scalar-arrays: ok")
            }
        "#,
    );

    assert_eq!(run.stderr, b"scalar-arrays: ok\n");
    assert!(run.rust.contains("[bool; 128]"), "{}", run.rust);
    assert!(run.rust.contains("[f64; 6]"), "{}", run.rust);
    assert!(run.rust.contains("GoString; 2]"), "{}", run.rust);
}
